# dex-sui 节点下单交易分析

## 1. 结论摘要

**dex-sui 完全支持通过启动节点发送 DEX 下单交易。**

| 项目 | 状态 | 说明 |
|-----|------|------|
| 下单交易 | ✅ 支持 | `DexCommand::PlaceOrder` |
| 撤单交易 | ✅ 支持 | `DexCommand::CancelOrder` |
| 批量撤单 | ✅ 支持 | `DexCommand::CancelAllOrders` |
| 交易类型 | `TransactionKind::ProgrammableDex` | 独立于 Move VM 的原生执行 |
| CLI 命令 | ⚠️ 部分实现 | 仅 subaccount 命令，无 order 命令 |

### 核心实现文件

```
dex-sui/
├── crates/sui-types/src/dex.rs           # DEX 类型定义
├── crates/sui-types/src/dex_builder.rs   # 交易构建器
├── sui-execution/src/dex.rs              # DEX 执行器 + 撮合引擎
├── crates/sui/src/dex_commands.rs        # CLI 命令
└── crates/sui-e2e-tests/tests/dex_order_tests.rs  # E2E 测试
```

---

## 2. 节点启动方式

### 2.1 编译 dex-sui 二进制

```bash
cd /path/to/dex-sui

# 编译 sui 二进制
cargo build --bin sui
```

### 2.2 初始化本地网络

```bash
# 设置配置目录
export SUI_CHAIN_DIR="$PWD/local-network-config"

# 初始化 genesis 配置
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000
```

### 2.3 启动验证者节点

**方式 A: 使用预配置目录**

```bash
export SUI_CHAIN_DIR="$PWD/local-network-config"
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9000
```

**方式 B: 临时启动（每次重置）**

```bash
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start \
    --with-faucet \
    --force-regenesis \
    --fullnode-rpc-port 9000
```

### 2.4 清理旧配置（如需要）

```bash
rm -rf ~/.sui/sui_config/network.yaml
rm -rf ~/.sui/sui_config/authorities_db/
rm -rf ~/.sui/sui_config/consensus_db/
```

---

## 3. DEX 交易类型详解

### 3.1 ProgrammableDexTransaction 结构

```rust
/// 类似 PTB (Programmable Transaction Block) 的 DEX 交易格式
pub struct ProgrammableDexTransaction {
    /// 输入参数（对象和纯值）
    pub inputs: Vec<CallArg>,
    /// 要执行的命令序列
    pub commands: Vec<DexCommand>,
}
```

### 3.2 支持的 DexCommand 枚举

```rust
pub enum DexCommand {
    // ===== 订单操作 =====
    PlaceOrder {
        subaccount: Argument,  // 下单子账户
        perpetual: Argument,   // 永续合约市场
        params: Argument,      // 订单参数 (PlaceOrderParams)
    },
    CancelOrder {
        order: Argument,       // 要撤销的订单
        subaccount: Argument,  // 订单所属子账户
    },
    CancelAllOrders {
        subaccount: Argument,
        perpetual_id: Option<Argument>,  // 可选：仅撤销指定市场
    },

    // ===== 子账户操作 =====
    CreateSubaccount { subaccount_number: Argument },
    DepositSubaccount { subaccount: Argument, amount: Argument },
    WithdrawSubaccount { subaccount: Argument, amount: Argument },
    DeleteSubaccount { subaccount: Argument },

    // ===== 市场操作 =====
    CreatePerpetual {
        perpetual_id: Argument,
        liquidity_tier_id: Argument,
        atomic_resolution: Argument,
    },
}
```

### 3.3 订单参数结构

```rust
pub struct PlaceOrderParams {
    pub client_id: u32,          // 客户端订单 ID
    pub perpetual_id: u32,       // 市场 ID (0=BTC-USD, 1=ETH-USD, ...)
    pub side: Side,              // Buy / Sell
    pub quantums: u64,           // 数量（基础单位）
    pub subticks: u64,           // 价格（0 = 市价单）
    pub worst_price: Option<u64>, // 滑点保护（市价单必填）
    pub time_in_force: TimeInForce, // Unspecified / IOC / PostOnly
    pub good_til_block_time: u64,   // 过期时间
    pub reduce_only: bool,          // 仅减仓
}
```

---

## 4. 下单交易完整示例

### 4.1 前置条件

下单前需要完成以下步骤：

1. **创建 Subaccount** - 交易子账户
2. **存入资金** - Deposit 到子账户
3. **创建 Perpetual 市场** - 如果市场不存在

### 4.2 完整 Rust SDK 示例

```rust
use sui_types::base_types::{ObjectID, SequenceNumber, SuiAddress};
use sui_types::dex::{PlaceOrderParams, ProgrammableDexTransaction, Side, TimeInForce};
use sui_types::dex_builder::ProgrammableDexTransactionBuilder;
use sui_types::transaction::{Transaction, TransactionData, TransactionKind};
use shared_crypto::intent::Intent;

/// 下单交易完整示例
async fn place_order_example(
    client: &SuiClient,
    keystore: &FileBasedKeystore,
    sender: SuiAddress,
    subaccount_id: ObjectID,
    subaccount_version: SequenceNumber,
    perpetual_obj_id: ObjectID,
    perpetual_version: SequenceNumber,
) -> Result<ObjectID, anyhow::Error> {
    // ========================================
    // Step 1: 构建下单交易
    // ========================================
    let mut builder = ProgrammableDexTransactionBuilder::new();

    // 添加共享对象输入
    let subaccount = builder.shared_obj_mutable(subaccount_id, subaccount_version);
    let perpetual = builder.shared_obj_mutable(perpetual_obj_id, perpetual_version);

    // 构建限价单参数
    let params = PlaceOrderParams::limit_order(
        1,                          // client_id: 客户端订单 ID
        0,                          // perpetual_id: 市场 ID (0 = BTC-USD)
        Side::Buy,                  // side: 买入
        100,                        // quantums: 数量
        50000,                      // subticks: 价格
        TimeInForce::Unspecified,   // time_in_force
        u64::MAX,                   // good_til_block_time: 永不过期
        false,                      // reduce_only: 非仅减仓
    );

    // 添加下单命令
    builder.place_order(subaccount, perpetual, params)?;
    let dex_tx = builder.finish();

    // ========================================
    // Step 2: 创建交易数据
    // ========================================
    let tx_data = TransactionData::new_dex(
        TransactionKind::ProgrammableDex(dex_tx),
        sender,
    );

    // ========================================
    // Step 3: 签名交易
    // ========================================
    let signature = keystore
        .sign_secure(&sender, &tx_data, Intent::sui_transaction())
        .await?;

    let transaction = Transaction::from_data(tx_data, vec![signature]);

    // ========================================
    // Step 4: 提交交易到节点
    // ========================================
    let response = client
        .quorum_driver_api()
        .execute_transaction_block(
            transaction,
            SuiTransactionBlockResponseOptions::full_content(),
            None,
        )
        .await?;

    // ========================================
    // Step 5: 解析结果
    // ========================================
    let effects = response.effects.ok_or_else(|| anyhow!("No effects"))?;

    if effects.status().is_ok() {
        // 获取创建的订单对象 ID
        let order_id = effects.created()[0].object_id();
        println!("Order placed successfully!");
        println!("  Order ID: {}", order_id);
        println!("  Transaction: {}", response.digest);
        Ok(order_id)
    } else {
        Err(anyhow!("Transaction failed: {:?}", effects.status()))
    }
}
```

### 4.3 市价单示例

```rust
// 市价单参数
let params = PlaceOrderParams::market_order(
    2,                  // client_id
    0,                  // perpetual_id: BTC-USD
    Side::Sell,         // side: 卖出
    50,                 // quantums: 数量
    45000,              // worst_price: 最差价格（滑点保护）
    u64::MAX,           // good_til_block_time
    false,              // reduce_only
);

// 市价单自动设置 time_in_force = IOC
// subticks = 0 表示市价单
```

### 4.4 撤单示例

```rust
/// 撤销指定订单
async fn cancel_order_example(
    builder: &mut ProgrammableDexTransactionBuilder,
    order_id: ObjectID,
    order_version: SequenceNumber,
    subaccount_id: ObjectID,
    subaccount_version: SequenceNumber,
) {
    let order = builder.shared_obj_mutable(order_id, order_version);
    let subaccount = builder.shared_obj_mutable(subaccount_id, subaccount_version);

    builder.cancel_order(order, subaccount);
}

/// 批量撤销所有订单
async fn cancel_all_orders_example(
    builder: &mut ProgrammableDexTransactionBuilder,
    subaccount_id: ObjectID,
    subaccount_version: SequenceNumber,
    perpetual_id: Option<u32>,  // None = 撤销所有市场订单
) -> anyhow::Result<()> {
    let subaccount = builder.shared_obj_mutable(subaccount_id, subaccount_version);

    builder.cancel_all_orders(subaccount, perpetual_id)?;
    Ok(())
}
```

---

## 5. 交易执行流程

```
┌─────────────────────────────────────────────────────────────────────┐
│                        客户端 (Client)                               │
├─────────────────────────────────────────────────────────────────────┤
│  1. ProgrammableDexTransactionBuilder::new()                        │
│  2. builder.place_order(subaccount, perpetual, params)              │
│  3. builder.finish() → ProgrammableDexTransaction                   │
│  4. TransactionData::new_dex(TransactionKind::ProgrammableDex(...)) │
│  5. keystore.sign_secure() → Signature                              │
│  6. Transaction::from_data(tx_data, signatures)                     │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ RPC: execute_transaction_block
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      验证者节点 (Validator)                          │
├─────────────────────────────────────────────────────────────────────┤
│  1. QuorumDriver 接收交易                                            │
│  2. 验证签名和 Gas                                                   │
│  3. 识别 TransactionKind::ProgrammableDex                           │
│  4. 路由到 DexExecutor (绕过 Move VM)                                │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      DexExecutor 执行层                              │
│                   (sui-execution/src/dex.rs)                        │
├─────────────────────────────────────────────────────────────────────┤
│  execute_programmable_dex_transaction()                             │
│    │                                                                │
│    ├─► execute_command(PlaceOrder)                                  │
│    │     │                                                          │
│    │     ├─► 验证订单参数                                            │
│    │     ├─► 检查子账户余额/保证金                                    │
│    │     ├─► 创建 Order 对象                                        │
│    │     └─► 调用撮合引擎                                            │
│    │                                                                │
│    └─► MemOrderbook::match_order()                                  │
│          │                                                          │
│          ├─► 遍历对手盘 (bids/asks)                                  │
│          ├─► FIFO 价格优先撮合                                       │
│          ├─► 生成 Fill 列表                                          │
│          └─► 更新 Order 和 Subaccount 状态                           │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      TransactionEffects                             │
├─────────────────────────────────────────────────────────────────────┤
│  - created: [Order 对象]                                            │
│  - mutated: [Subaccount, Perpetual, 对手方订单...]                   │
│  - status: Success / Failure                                        │
│  - gas_used: ...                                                    │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 6. CLI 扩展建议

当前 `sui dex` 命令仅支持 `subaccount` 子命令。建议扩展添加 `order` 子命令：

### 6.1 建议的命令结构

```bash
# 下单
sui dex order place \
  --subaccount <SUBACCOUNT_ID> \
  --perpetual <PERPETUAL_ID> \
  --side buy \
  --quantity 100 \
  --price 50000 \
  --time-in-force unspecified

# 市价单
sui dex order place \
  --subaccount <SUBACCOUNT_ID> \
  --perpetual <PERPETUAL_ID> \
  --side sell \
  --quantity 50 \
  --market \
  --worst-price 45000

# 撤单
sui dex order cancel --order <ORDER_ID> --subaccount <SUBACCOUNT_ID>

# 批量撤单
sui dex order cancel-all --subaccount <SUBACCOUNT_ID> [--perpetual 0]
```

### 6.2 实现参考

参考 `dex_commands.rs` 中 `SubaccountCommand` 的实现模式：

```rust
#[derive(Subcommand)]
pub enum OrderCommand {
    /// Place a new order
    #[clap(name = "place")]
    Place {
        /// Subaccount object ID
        #[clap(long)]
        subaccount: ObjectID,
        /// Perpetual market ID
        #[clap(long, default_value = "0")]
        perpetual: u32,
        /// Order side: buy or sell
        #[clap(long)]
        side: String,
        /// Order quantity in quantums
        #[clap(long)]
        quantity: u64,
        /// Order price in subticks (omit for market order)
        #[clap(long)]
        price: Option<u64>,
        /// Worst acceptable price for market orders
        #[clap(long)]
        worst_price: Option<u64>,
        /// Gas budget
        #[clap(long, default_value = "10000000")]
        gas_budget: u64,
    },
    /// Cancel an order
    #[clap(name = "cancel")]
    Cancel {
        /// Order object ID
        order: ObjectID,
        /// Subaccount object ID
        #[clap(long)]
        subaccount: ObjectID,
        /// Gas budget
        #[clap(long, default_value = "10000000")]
        gas_budget: u64,
    },
}
```

---

## 7. 测试验证

### 7.1 运行 E2E 测试

```bash
cd dex-sui

# 运行所有 DEX 订单测试
cargo simtest -p sui-e2e-tests -- dex_order

# 运行特定测试
cargo simtest -p sui-e2e-tests -- test_dex_place_limit_order
cargo simtest -p sui-e2e-tests -- test_dex_order_matching
```

### 7.2 测试覆盖的场景

| 测试名称 | 场景 |
|---------|------|
| `test_dex_order_create_subaccount` | 创建子账户 |
| `test_dex_order_multiple_subaccounts` | 多子账户 |
| `test_dex_create_perpetual` | 创建永续合约市场 |
| `test_dex_place_limit_order` | 限价单下单 |
| `test_dex_place_market_order` | 市价单下单 |
| `test_dex_cancel_order` | 撤销订单 |
| `test_dex_cancel_all_orders` | 批量撤单 |
| `test_dex_order_matching` | 订单撮合 |

### 7.3 预期结果

成功执行后，应看到：

```
test test_dex_place_limit_order ... ok
test test_dex_order_matching ... ok
...

test result: ok. X passed; 0 failed
```

---

## 8. 关键代码引用

### 8.1 交易类型定义
- `dex-sui/crates/sui-types/src/transaction.rs:TransactionKind::ProgrammableDex`

### 8.2 DEX 类型定义
- `dex-sui/crates/sui-types/src/dex.rs:1-2001`

### 8.3 交易构建器
- `dex-sui/crates/sui-types/src/dex_builder.rs:1-335`

### 8.4 执行器实现
- `dex-sui/sui-execution/src/dex.rs:718-1900` (execute_* 函数)

### 8.5 撮合引擎
- `dex-sui/sui-execution/src/dex.rs:50-500` (MemOrderbook)

### 8.6 E2E 测试示例
- `dex-sui/crates/sui-e2e-tests/tests/dex_order_tests.rs:1-887`

---

## 9. 总结

dex-sui 已实现完整的 DEX 下单功能：

1. **交易层面**: `ProgrammableDexTransaction` 支持所有订单操作
2. **执行层面**: `DexExecutor` 提供原生 Rust 执行，绕过 Move VM
3. **撮合层面**: 内存撮合引擎支持限价单、市价单、FIFO 撮合
4. **RPC 层面**: 标准 `execute_transaction_block` 接口支持 DEX 交易

**当前可用**:
- ✅ 编程方式下单 (Rust SDK)
- ✅ E2E 测试验证

**待扩展**:
- ⚠️ CLI 命令 (`sui dex order`)
- ⚠️ TypeScript SDK 支持
