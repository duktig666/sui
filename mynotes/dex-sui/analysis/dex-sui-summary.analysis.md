# dex-sui 项目实现分析

## 概述

dex-sui 是基于 Sui 区块链源码 fork 的低延迟高吞吐 DEX 项目。核心思路是**绕过 Move VM**，在 Sui 执行层原生实现永续合约 DEX 逻辑。

## 架构设计

```
┌─────────────────────────────────────────────────────────────────┐
│                      Sui Validator Node                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  TransactionKind                                                 │
│  ├── ProgrammableTransaction  →  Move VM                        │
│  ├── Dex                      →  DexExecutor (原生 Rust)        │
│  └── ProgrammableDex          →  DexExecutor (PTB 风格)         │
│                                                                  │
│  ┌──────────────┐     ┌──────────────────────────────────┐      │
│  │  Move VM     │     │  DexExecutor (sui-execution)     │      │
│  │  (不变)      │     │  ├── MemOrderbook (内存订单簿)   │      │
│  └──────────────┘     │  ├── Order/Subaccount 对象      │      │
│                       │  └── 撮合引擎                    │      │
│                       └──────────────────────────────────┘      │
└─────────────────────────────────────────────────────────────────┘
```

## 核心改动

### 1. 新增 TransactionKind

**文件**: `crates/sui-types/src/transaction.rs:480-483`

```rust
pub enum TransactionKind {
    // ... 原有类型
    /// 原生 Dex 交易（绕过 Move VM）
    Dex(crate::dex::DexTransaction),
    /// PTB 风格的 Dex 交易
    ProgrammableDex(crate::dex::ProgrammableDexTransaction),
}
```

### 2. DEX 对象类型

**文件**: `crates/sui-types/src/dex.rs`

```rust
/// DEX 对象类型
pub enum DexObject {
    Order(Order),           // 订单（共享对象）
    Subaccount(Subaccount), // 子账户（共享对象）
}

/// 订单状态
pub enum OrderStatus {
    Open, PartiallyFilled, Filled, Canceled, Expired
}

/// 订单方向
pub enum Side { Buy, Sell }

/// 有效期类型
pub enum TimeInForce { Unspecified, IOC, PostOnly }
```

### 3. 内存订单簿

**文件**: `sui-execution/src/dex.rs`

```rust
/// 单个市场的内存订单簿
pub struct MemOrderbook {
    pub perpetual_id: u32,
    /// 买单: BTreeMap 按价格降序
    pub bids: BTreeMap<u64, PriceLevel>,
    /// 卖单: BTreeMap 按价格升序
    pub asks: BTreeMap<u64, PriceLevel>,
    /// O(1) 订单查找索引
    pub order_index: HashMap<ObjectID, OrderBookEntry>,
    /// 子账户订单索引
    pub subaccount_orders: HashMap<SubaccountId, HashSet<ObjectID>>,
}
```

### 4. 执行引擎集成

**文件**: `sui-execution/latest/sui-adapter/src/execution_engine.rs:856-874`

```rust
TransactionKind::Dex(_) => {
    // Dex 交易由 DexExecutor 处理，不经过 Move VM
    Err((ExecutionError::new_with_source(
        ExecutionErrorKind::InvariantViolation,
        "Dex transactions should be handled by DexExecutor",
    ), vec![]))
}
```

### 5. Gas 处理

**文件**: `sui-execution/latest/sui-adapter/src/execution_engine.rs:143-144`

```rust
// DEX 交易免 Gas（类似系统交易）
let payment_method = if gas_data.is_unmetered()
    || transaction_kind.is_system_tx()
    || transaction_kind.is_dex_tx() {
    PaymentMethod::Unmetered
}
```

## 目录结构变更

```
dex-sui/
├── crates/
│   └── sui-types/src/
│       ├── dex.rs              # DEX 对象类型定义 (~1600 行)
│       ├── dex_builder.rs      # DEX 交易构建器 (~500 行)
│       └── transaction.rs      # 新增 Dex/ProgrammableDex 类型
├── sui-execution/src/
│   └── dex.rs                  # DEX 执行层 (~4000 行)
│       ├── MemOrderbook        # 内存订单簿
│       ├── DexExecutor         # DEX 执行器
│       └── 撮合逻辑
├── dex-sui/                    # DEX 扩展目录（占位）
│   ├── crates/sui-types/
│   ├── sui-execution/
│   └── docs/
└── v4-chain/                   # dYdX v4 参考结构
    ├── proto/dydxprotocol/
    │   ├── clob/
    │   ├── perpetuals/
    │   ├── prices/
    │   └── subaccounts/
    └── protocol/x/
        └── clob/memclob/
```

## 与 dYdX v4 的对比

| 特性 | dex-sui | dYdX v4 |
|------|---------|---------|
| 底层链 | Sui (fork) | Cosmos SDK (fork) |
| 执行方式 | 原生 Rust (绕过 Move VM) | 原生 Go (绕过 WASM) |
| 订单簿 | MemOrderbook (BTreeMap) | MemClob |
| 共识 | Mysticeti | CometBFT |
| 延迟 | ~400ms (目标) | ~1-2s |

## 关键设计决策

1. **绕过 Move VM**: DEX 交易不经过 Move VM，直接在执行层处理
2. **共享对象**: Order 和 Subaccount 作为 Sui 共享对象存储
3. **免 Gas**: DEX 交易当前免 Gas（`is_dex_tx()` 检查）
4. **PTB 兼容**: 支持 ProgrammableDex（PTB 风格的输入/命令结构）

## Commit 历史

| Commit | 说明 | 改动规模 |
|--------|------|----------|
| `394926b4d7` | counter v1 - 初始 DEX 框架 | +20000 行 |
| `80b142ba49` | v2 - 完善类型和执行 | +3000 行 |
| `3cccf9f20a` | match v1 with gasless | +10000/-13000 行 |

## 当前状态

- ✅ 基础类型定义 (Order, Subaccount, DexObject)
- ✅ 交易类型集成 (TransactionKind::Dex)
- ✅ 内存订单簿实现 (MemOrderbook)
- ✅ 执行层集成 (DexExecutor)
- ⏳ 测试用例 (dex_order_tests, dex_subaccount_tests)
- ⏳ v4-chain 参考结构（占位目录）
