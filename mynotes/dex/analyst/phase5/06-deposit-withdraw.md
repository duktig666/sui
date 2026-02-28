# 06 跨链充提适配

> 日期：2026-02-25
> 依赖：工程师 A（真实跨链充值和提现引擎实现）
> 优先级：P2

---

## 一、当前状态

### 1.1 现有充值机制

当前使用 `DexCommand::MintCoin` + `DexCommand::Deposit` 模拟充值：
- `MintCoin` 凭空创建 USDC Coin（仅限 dev/test）
- `Deposit` 将 Coin 转入 DEX 子账户余额
- 通过 `mint_and_deposit()` 链式调用

### 1.2 BalanceUpdateEvent

```rust
pub struct BalanceUpdateEvent {
    pub subaccount: Vec<u8>,
    pub delta: i128,          // 正=入金, 负=出金
    pub new_balance: i128,
    pub update_type: u8,      // 0=Deposit, 1=Withdraw, 2=Transfer
    pub timestamp_ms: u64,
}
```

- `update_type = 0` (Deposit): 当前仅限 MintCoin 模拟充值
- `update_type = 1` (Withdraw): 引擎层有定义但未实际使用
- `update_type = 2` (Transfer): 子账户间转账（TransferEvent 同步触发）

### 1.3 已有 handler

`balances.rs` handler 处理所有 `BalanceUpdateEvent`：
- INSERT `dex_balances` 表
- Redis Stream publish `dex:stream:balances`
- WS 广播到 `clearinghouseState:{address}` 频道

### 1.4 Gateway 端点

```
POST /tx/deposit      — 充值（目前用 mint_and_deposit）
POST /tx/mint-usdc    — 单独 USDC 铸造
```

---

## 二、需要与工程师 A 确认的事项

### 2.1 核心问题

| # | 问题 | 影响范围 | 可能方案 |
|---|------|---------|---------|
| 1 | 跨链充提是否复用 `BalanceUpdateEvent`？ | balances handler | 如果是，只需区分 update_type |
| 2 | `update_type` 是否新增值？ | balances handler + API | 如 3=CrossChainDeposit, 4=CrossChainWithdraw |
| 3 | 是否需要新事件追踪提款状态？ | 可能需要新 handler | Pending → Confirmed → Failed |
| 4 | 是否需要 `dex_withdrawals` 表？ | 新 migration + handler | 追踪跨链提款的中间状态 |
| 5 | 提款的链上确认如何通知？ | 是否需要 watcher 服务 | 监听目标链的确认 |
| 6 | 是否有跨链桥的 txHash？ | 数据模型 | 用于前端展示和验证 |

### 2.2 事件设计场景

**场景 A：复用 BalanceUpdateEvent（最简方案）**

```rust
// 引擎只需扩展 update_type
pub struct BalanceUpdateEvent {
    pub update_type: u8,
    // 0=Deposit (原有), 1=Withdraw (原有)
    // 2=Transfer (原有)
    // 3=CrossChainDeposit (新增)
    // 4=CrossChainWithdraw (新增)
    // 5=Liquidation (新增, 可选)
    // 6=Funding (新增, 可选)
}
```

- **优点**：indexer 层改动最小，balances handler 自动处理
- **缺点**：无法追踪提款的中间状态

**场景 B：新增 WithdrawalEvent**

```rust
// 新事件用于追踪提款生命周期
pub struct WithdrawalRequestEvent {
    pub withdrawal_id: Vec<u8>,    // 唯一 ID
    pub subaccount: Vec<u8>,
    pub amount: u128,
    pub destination_chain: u8,     // 0=Ethereum, 1=BSC, 2=Arbitrum...
    pub destination_address: Vec<u8>,
    pub status: u8,                // 0=Pending, 1=Processing
    pub timestamp_ms: u64,
}

pub struct WithdrawalConfirmedEvent {
    pub withdrawal_id: Vec<u8>,
    pub bridge_tx_hash: Vec<u8>,   // 跨链桥交易哈希
    pub status: u8,                // 2=Confirmed, 3=Failed
    pub timestamp_ms: u64,
}
```

- **优点**：完整追踪提款生命周期
- **缺点**：需要新 handler、新表、新 API

**场景 C：混合方案**

- 充值：复用 BalanceUpdateEvent（update_type=3）
- 提款：新增 WithdrawalEvent 追踪状态，同时触发 BalanceUpdateEvent（update_type=4）

---

## 三、Hyperliquid 参考

### 3.1 充值

Hyperliquid 的充值通过 Arbitrum L1 → Hyperliquid L1 桥接：
- 用户在 Arbitrum 上发送 USDC 到桥接合约
- 桥接验证后在 Hyperliquid 上记账
- 通过 `userNonFundingLedgerUpdates` 可查询到 type="deposit"

### 3.2 提款

```json
// Exchange API: 发起提款
{
  "type": "withdraw3",
  "hyperliquidChain": "Mainnet",
  "signatureChainId": "0xa4b1",
  "destination": "0x...",
  "amount": "100.0",
  "time": 1700000000000,
  "nonce": 12345
}
```

提款状态通过 `userNonFundingLedgerUpdates` 查询：
```json
{
  "time": 1700000000000,
  "hash": "0x...",
  "delta": {"type": "withdraw", "usdc": "-100.0"},
  "nid": 12345
}
```

### 3.3 关键设计点

- Hyperliquid 不提供"提款状态查询"的独立端点
- 提款在 L1 链上确认后直接反映在余额中
- 前端通过轮询 `userNonFundingLedgerUpdates` 判断提款是否完成

---

## 四、Indexer 适配方案（基于场景 A）

### 4.1 balances.rs 修改

如果工程师 A 复用 BalanceUpdateEvent 并新增 update_type 值：

```rust
// balances.rs - 无需修改 process/commit 逻辑
// BalanceUpdateEvent 的新 update_type 值会自动存入 dex_balances.update_type

// 但 userNonFundingLedgerUpdates 需要扩展映射：
fn balance_update_type_to_string(update_type: i16) -> &'static str {
    match update_type {
        0 => "deposit",
        1 => "withdraw",
        2 => "transfer",
        3 => "crossChainDeposit",
        4 => "crossChainWithdraw",
        _ => "unknown",
    }
}
```

### 4.2 API 扩展

**userNonFundingLedgerUpdates** 返回中增加跨链充提类型：
```json
{
  "time": 1700000000000,
  "hash": "0x...",
  "delta": "100.0",
  "updateType": "crossChainDeposit",
  "sourceChain": "Ethereum"     // 可选字段
}
```

---

## 五、Indexer 适配方案（基于场景 B）

### 5.1 新表 dex_withdrawals

```sql
CREATE TABLE dex_withdrawals (
    withdrawal_id           BYTEA NOT NULL PRIMARY KEY,
    account_address         TEXT NOT NULL,
    subaccount_number       INT NOT NULL,
    amount                  BYTEA NOT NULL,          -- u128 LE
    destination_chain       SMALLINT NOT NULL,
    destination_address     BYTEA NOT NULL,
    status                  SMALLINT NOT NULL,        -- 0=Pending, 1=Processing, 2=Confirmed, 3=Failed
    bridge_tx_hash          BYTEA,                    -- 跨链桥交易哈希
    request_cp              BIGINT NOT NULL,
    request_tx_digest       BYTEA NOT NULL,
    confirm_cp              BIGINT,
    confirm_tx_digest       BYTEA,
    request_timestamp_ms    BIGINT NOT NULL,
    confirm_timestamp_ms    BIGINT
);

CREATE INDEX idx_withdrawals_user ON dex_withdrawals (account_address, request_timestamp_ms DESC);
CREATE INDEX idx_withdrawals_status ON dex_withdrawals (status, request_timestamp_ms DESC);
```

### 5.2 新 Handler: withdrawals.rs

```rust
pub struct Withdrawals;

impl Processor for Withdrawals {
    const NAME: &'static str = "dex_withdrawals";
    type Value = WithdrawalUpdate;

    fn process(checkpoint: &CheckpointData) -> Result<Vec<Self::Value>> {
        // 处理 WithdrawalRequestEvent → INSERT
        // 处理 WithdrawalConfirmedEvent → UPDATE status
    }
}
```

### 5.3 新 API 端点

```rust
// 查询提款记录
pub struct UserWithdrawalsRequest {
    pub user: String,
    pub status: Option<u8>,       // 过滤状态
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: i64,
}
```

### 5.4 Exchange API

```rust
// 发起提款
ExchangeAction::Withdraw(WithdrawAction),

pub struct WithdrawAction {
    pub destination: String,       // 目标地址
    pub amount: String,            // USDC 金额
    pub destination_chain: u8,     // 目标链
    pub subaccount_number: Option<u32>,
}
```

---

## 六、WS 推送

### 6.1 充值完成推送

通过已有的 `balances` stream → `clearinghouseState:{address}` 频道自动推送。

### 6.2 提款状态推送

如果实现场景 B：
```rust
// 新增 stream: dex:stream:withdrawals
// consumer.rs 消费后推送到 user:{address} 频道
let server_msg = ServerMessage::ChannelData {
    channel: format!("user:{}", address),
    data: json!({
        "updateType": "withdrawal",
        "data": {
            "withdrawalId": "0x...",
            "status": "confirmed",
            "amount": "100.0",
            "bridgeTxHash": "0x..."
        }
    }),
};
```

---

## 七、Gateway 适配

### 7.1 当前 Gateway

```
POST /tx/deposit  — 使用 mint_and_deposit（模拟）
POST /tx/mint-usdc — 单独铸造
```

### 7.2 真实跨链后

- `/tx/deposit` 可能需要改为调用跨链桥合约（而非 MintCoin）
- 或者充值改为被动模式：用户直接与桥接合约交互，引擎监听事件
- `/tx/mint-usdc` 保留用于 testnet

### 7.3 新增提款端点

```
POST /tx/withdraw
{
  "sender_index": 0,
  "amount": "100.0",
  "destination_chain": 0,
  "destination_address": "0x..."
}
```

---

## 八、开放问题

| # | 问题 | 等待确认 |
|---|------|---------|
| 1 | 跨链桥选型（Wormhole / LayerZero / 自建）？ | 工程师 A |
| 2 | 充值确认延迟（几个区块确认？） | 工程师 A |
| 3 | 提款冷却期（安全考虑，如 24h 延迟）？ | 产品决策 |
| 4 | 是否支持多链充提（ETH / ARB / BSC）？ | 产品决策 |
| 5 | 提款失败重试机制？ | 工程师 A |
| 6 | 充值最小/最大金额限制？ | 产品决策 |

---

## 九、文件清单（取决于场景选择）

### 场景 A（最小改动）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-api/src/handlers.rs` | 修改 | update_type 映射扩展 |
| `dex-types/src/api/responses.rs` | 修改 | LedgerUpdate 增加 sourceChain |

### 场景 B（完整方案）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-indexer/migrations/.../up.sql` | 新建 | dex_withdrawals 表 |
| `dex-indexer/src/handlers/withdrawals.rs` | 新建 | Withdrawal handler |
| `dex-indexer/src/handlers/mod.rs` | 修改 | 注册 handler |
| `dex-indexer/src/schema/mod.rs` | 修改 | table! + StoredWithdrawal |
| `dex-types/src/api/requests.rs` | 修改 | UserWithdrawalsRequest |
| `dex-types/src/api/responses.rs` | 修改 | WithdrawalResponse |
| `dex-types/src/api/exchange.rs` | 修改 | WithdrawAction |
| `dex-api/src/handlers.rs` | 修改 | query_user_withdrawals |
| `dex-api/src/server.rs` | 修改 | 路由注册 |
| `dex-api/src/ws/consumer.rs` | 修改 | withdrawals stream 消费 |
| `sui-types/src/dex_events.rs` | **引擎新增** | WithdrawalRequestEvent/ConfirmedEvent |

---

## 十、建议

1. **先与工程师 A 对齐事件设计**，再决定场景 A 或 B
2. **充值侧优先**：充值通常复用 BalanceUpdateEvent 即可
3. **提款侧谨慎**：需要考虑安全性（冷却期、限额、多签）
4. **testnet 保留 MintCoin**：开发和测试仍需要模拟充值
