# 07 杠杆与保证金模式

> 日期：2026-02-25
> 依赖：工程师 B（逐仓保证金、杠杆调整引擎实现）
> 优先级：P1

---

## 一、当前状态

### 1.1 hardcoded 杠杆

`clearinghouseState` 中杠杆信息全部硬编码：

```rust
// handlers.rs - query_clearinghouse_state()
LeverageInfo {
    type_: "cross".to_string(),   // 硬编码全仓
    value: 1,                      // 硬编码 1x
}
```

### 1.2 dex_positions 表

当前缺少杠杆相关列：
```
account_address, subaccount_number, perpetual_id,
size, avg_entry_price,
last_update_reason, last_cp_sequence_number, last_tx_sequence_number, last_timestamp_ms
```

M2 migration 将新增：`leverage_value`, `margin_mode`, `isolated_margin`, `cum_funding_since_open`, `cum_funding_all_time`

### 1.3 无杠杆调整事件

`dex_events.rs` 中无 `LeverageUpdateEvent`，引擎需新增。

### 1.4 Exchange API 无杠杆操作

`ExchangeAction` 枚举中无 `UpdateLeverage` 和 `UpdateIsolatedMargin`。

---

## 二、Hyperliquid 规范

### 2.1 杠杆信息展示

`clearinghouseState` 响应中：

```json
{
  "assetPositions": [{
    "position": {
      "coin": "BTC",
      "leverage": {
        "type": "cross",          // "cross" | "isolated"
        "value": 10,               // 有效杠杆倍数
        "rawUsd": "1000.0"        // 逐仓时: 逐仓保证金; 全仓时: 同 value 数字
      }
    }
  }]
}
```

### 2.2 updateLeverage Exchange Action

```json
{
  "action": {
    "type": "updateLeverage",
    "asset": 0,                   // perpetual_id
    "isCross": true,              // true=全仓, false=逐仓
    "leverage": 20                // 杠杆倍数
  },
  "nonce": 12345,
  "signature": {...}
}
```

### 2.3 updateIsolatedMargin Exchange Action

```json
{
  "action": {
    "type": "updateIsolatedMargin",
    "asset": 0,
    "isBuy": true,                // 方向（多头/空头）
    "ntli": 100                   // 保证金变化量 (正=增加, 负=减少)
  },
  "nonce": 12345,
  "signature": {...}
}
```

### 2.4 有效杠杆 vs 设定杠杆

| 概念 | 说明 |
|------|------|
| 设定杠杆 | 用户设置的杠杆倍数（如 20x），决定初始保证金要求 |
| 有效杠杆 | position_value / margin_used，随市场价变化 |

Hyperliquid 的 `leverage.value` 返回的是**设定杠杆**，前端可以据此计算有效杠杆。

---

## 三、需要引擎提供的事件

### 3.1 LeverageUpdateEvent（新事件）

```rust
// dex_events.rs - 需要工程师 B 新增
pub struct LeverageUpdateEvent {
    pub perpetual_id: u32,
    pub subaccount: Vec<u8>,         // 36 bytes
    pub leverage_value: u32,         // 杠杆倍数
    pub margin_mode: u8,             // 0=cross, 1=isolated
    pub isolated_margin: i128,       // 逐仓保证金（全仓时为 0）
    pub timestamp_ms: u64,
}
```

**发射时机**：
- 用户调用 `updateLeverage` 时
- 用户调用 `updateIsolatedMargin` 时

### 3.2 DexCommand 扩展

```rust
// 引擎需要新增的 DexCommand
DexCommand::UpdateLeverage {
    perpetual_id: u32,
    subaccount_number: u32,
    leverage_value: u32,
    is_cross: bool,
}

DexCommand::UpdateIsolatedMargin {
    perpetual_id: u32,
    subaccount_number: u32,
    is_buy: bool,
    amount: i64,
}
```

---

## 四、Indexer 实现

### 4.1 无需新 Handler

`LeverageUpdateEvent` 直接更新 `dex_positions` 表，不需要独立的历史表。

可以通过以下两种方式处理：

**方案 A：新 Handler（leverage_updates.rs）**
```rust
pub struct LeverageUpdates;

impl Handler for LeverageUpdates {
    async fn commit(values: &[StoredLeverageUpdate], conn: &mut PgConnection) -> Result<usize> {
        for update in values {
            diesel::update(dex_positions::table)
                .filter(dex_positions::account_address.eq(&update.account_address))
                .filter(dex_positions::subaccount_number.eq(update.subaccount_number))
                .filter(dex_positions::perpetual_id.eq(update.perpetual_id))
                .set((
                    dex_positions::leverage_value.eq(update.leverage_value),
                    dex_positions::margin_mode.eq(update.margin_mode),
                    dex_positions::isolated_margin.eq(&update.isolated_margin),
                ))
                .execute(conn)?;
        }
        // Redis publish
    }
}
```

**方案 B：集成到 positions.rs**

在现有 positions handler 中增加 LeverageUpdateEvent 处理。

**推荐方案 A**：独立 handler 更清晰，避免 positions.rs 过于复杂。

### 4.2 StoredLeverageUpdate

```rust
pub struct StoredLeverageUpdate {
    pub account_address: String,
    pub subaccount_number: i32,
    pub perpetual_id: i32,
    pub leverage_value: i32,
    pub margin_mode: i16,
    pub isolated_margin: Option<Vec<u8>>,  // i128 LE bytes
    pub timestamp_ms: i64,
}
```

### 4.3 Redis 发布

```rust
// 发布到 positions stream（复用现有频道）
// 或新增 dex:stream:leverage_updates
// consumer.rs 广播到 clearinghouseState:{address} 频道
```

推荐复用 `dex:stream:positions`，因为杠杆变更本质上是 position 属性的更新。

---

## 五、API 适配

### 5.1 clearinghouseState 改进

```rust
// handlers.rs - query_clearinghouse_state()

// 当前硬编码：
LeverageInfo {
    type_: "cross".to_string(),
    value: 1,
}

// 改为从 DB 读取：
LeverageInfo {
    type_: match position.margin_mode {
        0 => "cross".to_string(),
        1 => "isolated".to_string(),
        _ => "cross".to_string(),
    },
    value: position.leverage_value,
    // 新增 raw_usd 字段（Hyperliquid 对应）
    raw_usd: if position.margin_mode == 1 {
        Some(format_i128_bytea(&position.isolated_margin))
    } else {
        None
    },
}
```

### 5.2 LeverageInfo 类型扩展

```rust
// responses.rs
pub struct LeverageInfo {
    pub type_: String,           // "cross" | "isolated"（Hyperliquid: "type"）
    pub value: i32,              // 杠杆倍数
    pub raw_usd: Option<String>, // 逐仓保证金金额（Hyperliquid: "rawUsd"）
}
```

### 5.3 margin_used 计算改进

```rust
// 当前：margin_used = position_value / max_leverage（全局 max_leverage）
// 改为：margin_used = position_value / leverage_value（用户设定的杠杆）

let margin_used = if position.leverage_value > 0 {
    position_value / position.leverage_value as i128
} else {
    position_value / max_leverage as i128  // fallback
};
```

### 5.4 withdrawable 计算改进

```rust
// 全仓模式：
// withdrawable = account_value - total_margin_used
// 其中 total_margin_used = sum(|position_value_i| / leverage_i)

// 逐仓模式：
// withdrawable = free_collateral（不计入逐仓保证金的部分）
// 逐仓保证金锁定在具体仓位中，不可提取
```

---

## 六、Exchange 适配

### 6.1 ExchangeAction 新增

```rust
// exchange.rs
pub enum ExchangeAction {
    // 现有...
    Order(OrderAction),
    Cancel(CancelAction),
    CancelByCloid(CancelByCloidAction),
    ClosePosition(ClosePositionAction),

    // 新增
    UpdateLeverage(UpdateLeverageAction),
    UpdateIsolatedMargin(UpdateIsolatedMarginAction),
}

pub struct UpdateLeverageAction {
    pub asset: u32,                // perpetual_id
    pub is_cross: bool,
    pub leverage: u32,
    pub subaccount_number: Option<u32>,
}

pub struct UpdateIsolatedMarginAction {
    pub asset: u32,
    pub is_buy: bool,
    pub ntli: i64,                 // 保证金变化量
    pub subaccount_number: Option<u32>,
}
```

### 6.2 Exchange Handler

```rust
// exchange/handlers.rs

async fn handle_update_leverage(
    exchange: &ExchangeState,
    sig_data: &SignatureData,
    nonce: u64,
    deadline: u64,
    action: UpdateLeverageAction,
) -> Result<ExchangeResponse> {
    let subaccount_number = action.subaccount_number.unwrap_or(0);

    // EIP-712 签名验证
    let signature = parse_eip712_signature(sig_data)?;
    // 提取 signer address

    // 构造 DexCommand::UpdateLeverage
    let builder = ProgrammableDexTransactionBuilder::new(signer_address);
    builder.update_leverage(
        action.asset,
        subaccount_number,
        action.leverage,
        action.is_cross,
    );

    // 提交交易
    let digest = exchange.submit_dex_transaction(builder.build()).await?;

    Ok(ExchangeResponse::ok(json!({
        "type": "updateLeverage",
        "data": {"digest": digest}
    })))
}

async fn handle_update_isolated_margin(
    exchange: &ExchangeState,
    sig_data: &SignatureData,
    nonce: u64,
    deadline: u64,
    action: UpdateIsolatedMarginAction,
) -> Result<ExchangeResponse> {
    // 类似逻辑
    let builder = ProgrammableDexTransactionBuilder::new(signer_address);
    builder.update_isolated_margin(
        action.asset,
        subaccount_number,
        action.is_buy,
        action.ntli,
    );

    let digest = exchange.submit_dex_transaction(builder.build()).await?;

    Ok(ExchangeResponse::ok(json!({
        "type": "updateIsolatedMargin",
        "data": {"digest": digest}
    })))
}
```

### 6.3 exchange_handler 分发

```rust
// exchange/mod.rs 的 exchange_handler 中
ExchangeAction::UpdateLeverage(action) => {
    handle_update_leverage(&state.exchange, &req.signature, req.nonce, req.deadline, action).await
}
ExchangeAction::UpdateIsolatedMargin(action) => {
    handle_update_isolated_margin(&state.exchange, &req.signature, req.nonce, req.deadline, action).await
}
```

---

## 七、WS 适配

### 7.1 杠杆变更推送

通过 `clearinghouseState:{address}` 频道推送：

```json
{
  "channel": "clearinghouseState:0xabc...",
  "data": {
    "updateType": "leverage",
    "data": {
      "perpetualId": 0,
      "leverageValue": 20,
      "marginMode": "cross"
    }
  }
}
```

### 7.2 逐仓保证金变更推送

同样通过 `clearinghouseState:{address}` 频道：

```json
{
  "channel": "clearinghouseState:0xabc...",
  "data": {
    "updateType": "isolatedMargin",
    "data": {
      "perpetualId": 0,
      "isolatedMargin": "1000.0",
      "direction": "buy"
    }
  }
}
```

---

## 八、EIP-712 签名结构

### 8.1 UpdateLeverage 签名

```rust
// EIP-712 type hash
pub struct Eip712UpdateLeverageParams {
    pub subaccount_number: u32,
    pub perpetual_id: u32,
    pub leverage: u32,
    pub is_cross: bool,
    pub nonce: u64,
    pub deadline: u64,
}
```

**TypeHash**：
```
UpdateLeverage(uint32 subaccountNumber,uint32 perpetualId,uint32 leverage,bool isCross,uint64 nonce,uint64 deadline)
```

### 8.2 UpdateIsolatedMargin 签名

```rust
pub struct Eip712UpdateIsolatedMarginParams {
    pub subaccount_number: u32,
    pub perpetual_id: u32,
    pub is_buy: bool,
    pub amount: i64,
    pub nonce: u64,
    pub deadline: u64,
}
```

---

## 九、文件清单

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `sui-types/src/dex_events.rs` | **引擎新增** | LeverageUpdateEvent |
| `dex-indexer/src/handlers/leverage_updates.rs` | 新建 | Leverage handler |
| `dex-indexer/src/handlers/mod.rs` | 修改 | 注册 handler |
| `dex-indexer/src/schema/mod.rs` | 修改 | StoredPosition 新字段 |
| `dex-types/src/api/responses.rs` | 修改 | LeverageInfo 扩展 |
| `dex-types/src/api/exchange.rs` | 修改 | UpdateLeverage/UpdateIsolatedMargin action |
| `dex-api/src/handlers.rs` | 修改 | clearinghouseState 杠杆读取 + margin 计算 |
| `dex-api/src/exchange/handlers.rs` | 修改 | handle_update_leverage + handle_update_isolated_margin |
| `dex-api/src/exchange/state.rs` | 修改 | submit 方法 |
| `dex-api/src/ws/consumer.rs` | 修改 | 杠杆变更广播 |

---

## 十、依赖关系

```
M2 (dex_positions 增加 leverage 列)
    ↓
LeverageUpdateEvent (引擎)  ← 等待工程师 B
    ↓
leverage_updates.rs handler
    ├─→ UPDATE dex_positions (leverage_value, margin_mode, isolated_margin)
    └─→ Redis Stream → WS clearinghouseState:{address}
        ↓
clearinghouseState API 改进
    ├─→ leverage.type/value/rawUsd 从 DB 读取
    ├─→ margin_used 使用用户设定杠杆
    └─→ withdrawable 考虑逐仓锁定

Exchange API
    ├─→ updateLeverage action
    └─→ updateIsolatedMargin action
```
