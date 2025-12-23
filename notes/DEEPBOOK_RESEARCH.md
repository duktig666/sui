# DeepBook 深度研究 | DeepBook Deep Dive

> **研究对象**: DeepBook - Sui 区块链的原生去中心化订单簿交易所
> **Research Target**: DeepBook - Sui's Native Decentralized Order Book Exchange

---

## 📑 目录 | Table of Contents

1. [概述与状态](#1-概述与状态--overview--status)
2. [架构设计](#2-架构设计--architecture-design)
3. [核心数据结构](#3-核心数据结构--core-data-structures)
4. [订单匹配机制](#4-订单匹配机制--order-matching-mechanism)
5. [托管模型](#5-托管模型--custodian-model)
6. [Critbit 树详解](#6-critbit-树详解--critbit-tree-deep-dive)
7. [费用机制](#7-费用机制--fee-mechanism)
8. [代码导航](#8-代码导航--code-navigation)
9. [实践探索](#9-实践探索--hands-on-exploration)
10. [DeepBook V3](#10-deepbook-v3)

---

## 1. 概述与状态 | Overview & Status

### 1.1 什么是 DeepBook？

DeepBook 是一个**完全链上的中央限价订单簿 (CLOB)** 交易所，专为 Sui 区块链设计。它提供：

- ✅ **完全去中心化**: 无需许可，任何人都可以创建交易对
- ⚡ **低延迟**: 利用 Sui 的并行执行能力
- 📊 **价格-时间优先**: 传统交易所的匹配算法
- 🔐 **隔离托管**: 用户资金由智能合约管理
- 💰 **灵活费率**: Maker/Taker 费用可配置

### 1.2 版本状态 | Version Status

**⚠️ 重要提示**:

| 版本 | 状态 | 说明 |
|------|------|------|
| **DeepBook V1** | ❌ 已弃用 | 原始实现 |
| **DeepBook V2** | ⚠️ 部分弃用 | 框架中的版本，大部分功能已禁用 |
| **DeepBook V3** | ✅ 推荐使用 | 独立仓库，生产环境使用 |

**V2 当前状态**:
- 大部分函数会 `abort` 并返回错误码 `1337`
- 现有池仍可用于取消订单和提取资产
- 不建议创建新的 V2 池

### 1.3 代码位置 | Code Location

```
sui/
├── crates/sui-framework/packages/deepbook/
│   ├── sources/
│   │   ├── clob_v2.move           (1119行) - 主 CLOB 实现
│   │   ├── custodian_v2.move      (174行)  - 资产托管
│   │   ├── critbit.move           (453行)  - 订单簿数据结构
│   │   ├── math.move              (112行)  - 定点数运算
│   │   └── order_query.move       (205行)  - 订单分页查询
│   └── Move.toml                  - 包配置
├── crates/sui-deepbook-indexer/   - V3 索引器 (Rust)
└── docs/content/standards/deepbookv3/ - V3 设计文档
```

**包地址**: `0xdee9`

---

## 2. 架构设计 | Architecture Design

### 2.1 整体架构 | Overall Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Pool<Base, Quote>                        │
│                   (共享对象 Shared Object)                   │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────────┐         ┌──────────────────┐        │
│  │   Order Book     │         │   Custodians     │        │
│  ├──────────────────┤         ├──────────────────┤        │
│  │ Bids (Critbit)   │         │ Base Custodian   │        │
│  │  ├─ TickLevel@10 │         │  └─ Accounts     │        │
│  │  ├─ TickLevel@9  │         │                  │        │
│  │  └─ TickLevel@8  │         │ Quote Custodian  │        │
│  │                  │         │  └─ Accounts     │        │
│  │ Asks (Critbit)   │         └──────────────────┘        │
│  │  ├─ TickLevel@11 │                                     │
│  │  ├─ TickLevel@12 │         ┌──────────────────┐        │
│  │  └─ TickLevel@13 │         │  User Mappings   │        │
│  └──────────────────┘         ├──────────────────┤        │
│                                │ usr_open_orders  │        │
│  ┌──────────────────┐         │  address ->      │        │
│  │   Fee Config     │         │    order_id ->   │        │
│  ├──────────────────┤         │      price       │        │
│  │ taker_fee_rate   │         └──────────────────┘        │
│  │ maker_rebate_rate│                                     │
│  │ tick_size        │         ┌──────────────────┐        │
│  │ lot_size         │         │  Order IDs       │        │
│  └──────────────────┘         ├──────────────────┤        │
│                                │ next_bid_order_id│        │
│                                │ next_ask_order_id│        │
└────────────────────────────────┴──────────────────────────┘
```

### 2.2 关键组件 | Key Components

#### **A. Pool (交易对)**

每个交易对是一个 `Pool<BaseAsset, QuoteAsset>` 共享对象:

```move
struct Pool<phantom BaseAsset, phantom QuoteAsset> has key, store {
    id: UID,
    // 订单簿
    bids: CritbitTree<TickLevel>,
    asks: CritbitTree<TickLevel>,
    // 托管
    base_custodian: Custodian<BaseAsset>,
    quote_custodian: Custodian<QuoteAsset>,
    // 用户状态
    usr_open_orders: Table<address, LinkedTable<u64, u64>>,
    // 费用配置
    taker_fee_rate: u64,
    maker_rebate_rate: u64,
    tick_size: u64,
    lot_size: u64,
    // ...
}
```

**文件位置**: `crates/sui-framework/packages/deepbook/sources/clob_v2.move:205-235`

#### **B. TickLevel (价格级别)**

每个价格级别包含该价格的所有订单:

```move
struct TickLevel has store {
    price: u64,
    open_orders: LinkedTable<u64, Order>  // 按时间排序
}
```

**特性**:
- 使用 `LinkedTable` 保证时间优先
- 同价格订单按 FIFO 匹配
- 空 TickLevel 自动删除以节省存储

#### **C. Order (订单)**

```move
struct Order has store, drop {
    order_id: u64,              // 唯一标识符
    client_order_id: u64,       // 用户自定义 ID
    price: u64,                 // 限价
    original_quantity: u64,     // 初始数量
    quantity: u64,              // 剩余数量
    is_bid: bool,               // 买单/卖单
    owner: address,             // AccountCap 所有者
    expire_timestamp: u64,      // 过期时间 (毫秒)
    self_matching_prevention: u8 // 自成交防止策略
}
```

**Order ID 设计** (重要!):
- **Bid orders**: `0` ~ `(1 << 63) - 1`
- **Ask orders**: `(1 << 63)` ~ `u64::MAX`
- 第 63 位区分买卖单 (0=买, 1=卖)
- 递增分配保证时间优先

**文件位置**: `clob_v2.move:175-196`

---

## 3. 核心数据结构 | Core Data Structures

### 3.1 Critbit Tree (订单簿核心)

**为什么使用 Critbit Tree?**

传统链上订单簿面临的挑战:
- ❌ 简单数组: O(n) 查找，O(n) 插入
- ❌ 哈希表: 无序，无法快速找最优价格
- ✅ **Critbit Tree**: O(log n) 所有操作 + 有序

**Critbit Tree 特性**:

```
Binary Patricia Trie (前缀树的二进制版本)

示例: 存储价格 [5, 9, 10, 11]

        Internal (mask=8, bit 3)
       /                      \
   Leaf(5)              Internal (mask=2, bit 1)
                       /                      \
                  Leaf(9)                Internal (mask=1, bit 0)
                                        /                  \
                                   Leaf(10)             Leaf(11)
```

**优势**:
- 🔍 O(log n) 查找、插入、删除
- 📊 有序遍历 (最优买/卖价)
- 🎯 高效范围查询 (市场深度)
- 💾 节省存储 (内部节点可重用)

**数据结构**:

```move
// critbit.move:39-48
struct CritbitTree<V: store> has store {
    root: u64,
    internal_nodes: Table<u64, InternalNode>,
    leaves: Table<u64, Leaf<V>>,
    min_leaf: u64,  // 最小价格 (最优买价)
    max_leaf: u64,  // 最大价格 (最优卖价)
    next_internal_node_index: u64,
    next_leaf_index: u64
}

// critbit.move:24-28
struct Leaf<V: store> has store {
    key: u64,       // 价格
    value: V,       // TickLevel
    parent: u64
}

// critbit.move:30-36
struct InternalNode has store {
    mask: u64,      // 关键位掩码
    left_child: u64,
    right_child: u64,
    parent: u64
}
```

**关键算法**:

1. **插入** (`insert_leaf` - Line 145-223):
   ```move
   public fun insert_leaf<V: store>(
       tree: &mut CritbitTree<V>,
       key: u64,
       value: V
   ): u64
   ```
   - 找到应插入位置
   - 创建新叶节点
   - 必要时创建内部节点
   - 更新父子关系

2. **查找最优价格**:
   - 买单: `min_leaf(tree)` - O(1)!
   - 卖单: `max_leaf(tree)` - O(1)!

3. **遍历**:
   - `next_leaf()` - 下一个价格级别
   - `previous_leaf()` - 上一个价格级别

**文件位置**: `critbit.move:1-453`

### 3.2 Custodian (托管系统)

**设计哲学**: 用户资金不存储在订单中，而是集中托管。

```move
// custodian_v2.move:31-35
struct Custodian<T> has key, store {
    id: UID,
    account_balances: Table<address, Account<T>>
}

// custodian_v2.move:13-16
struct Account<T> has store {
    available_balance: Balance<T>,  // 可用余额
    locked_balance: Balance<T>      // 锁定余额 (挂单中)
}
```

**AccountCap (账户权限)**:

```move
// custodian_v2.move:23-28
struct AccountCap has key, store {
    id: UID,
    owner: address  // 从对象 ID 派生
}
```

**AccountCap 模型**:
- **管理账户**: `id == owner` (可创建子账户)
- **子账户**: `id != owner` (只能访问自己的资金)

**资金流转**:
```
1. 存款:    Coin<T> -> available_balance
2. 下单:    available_balance -> locked_balance
3. 成交:    locked_balance -> 对手方 available_balance
4. 取款:    available_balance -> Coin<T>
```

**文件位置**: `custodian_v2.move:1-174`

---

## 4. 订单匹配机制 | Order Matching Mechanism

### 4.1 匹配流程 | Matching Flow

```
┌─────────────────────────────────────────────────────────┐
│ 1. 订单到达 (Place Limit Order)                         │
└─────────────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────────────┐
│ 2. 验证                                                 │
│    ✓ tick_size 对齐 (price % tick_size == 0)           │
│    ✓ lot_size 对齐 (quantity % lot_size == 0)          │
│    ✓ expire_timestamp 有效                              │
│    ✓ AccountCap 验证                                    │
└─────────────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────────────┐
│ 3. 尝试匹配对手盘                                       │
│                                                         │
│  Buy Order (Bid):                                       │
│    ├─ 查找最低卖价: asks.min_leaf()                     │
│    ├─ 如果 best_ask_price <= bid_price: 匹配           │
│    └─ 遍历 TickLevel.open_orders (FIFO)                │
│                                                         │
│  Sell Order (Ask):                                      │
│    ├─ 查找最高买价: bids.max_leaf()                     │
│    ├─ 如果 best_bid_price >= ask_price: 匹配           │
│    └─ 遍历 TickLevel.open_orders (FIFO)                │
└─────────────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────────────┐
│ 4. 执行成交                                             │
│    ├─ 计算成交数量 = min(order.quantity, match.quantity)│
│    ├─ 计算费用:                                         │
│    │   - Taker Fee = filled_qty * price * taker_rate   │
│    │   - Maker Rebate = filled_qty * price * maker_rate│
│    ├─ 转移资产:                                         │
│    │   Maker locked -> Taker available (扣除 Taker Fee)│
│    │   Taker locked -> Maker available (加 Maker Rebate)│
│    ├─ 更新订单数量                                      │
│    └─ 发出 OrderFilled 事件                             │
└─────────────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────────────┐
│ 5. 处理剩余 (Post Order)                                │
│    如果订单未完全成交:                                   │
│    ├─ 找到或创建 TickLevel (Critbit insert)             │
│    ├─ 将订单插入 TickLevel.open_orders                  │
│    ├─ 更新 usr_open_orders 映射                         │
│    ├─ 锁定用户资金在 Custodian                          │
│    └─ 发出 OrderPlaced 事件                             │
└─────────────────────────────────────────────────────────┘
```

### 4.2 价格-时间优先 | Price-Time Priority

**价格优先**:
- Critbit Tree 天然支持
- 买单: 从高到低遍历 (max → min)
- 卖单: 从低到高遍历 (min → max)

**时间优先**:
- Order ID 递增分配
- `LinkedTable<u64, Order>` 按 ID 排序
- 同价格订单按 FIFO 匹配

**示例**:

```
Bids (买单):
  Price 10: [Order#1, Order#5, Order#8]  <- 按时间顺序
  Price 9:  [Order#2, Order#6]
  Price 8:  [Order#3]

Asks (卖单):
  Price 11: [Order#4, Order#7]
  Price 12: [Order#9]
  Price 13: [Order#10]

新卖单 @ 9:
  1. 匹配 Price 10 的 Order#1 (最高价)
  2. 如果还有剩余，匹配 Order#5
  3. 如果还有剩余，匹配 Order#8
  4. 如果还有剩余，匹配 Price 9 的 Order#2
```

### 4.3 自成交防止 | Self-Matching Prevention

```move
const CANCEL_TAKER: u8 = 0;  // 取消 Taker 订单
const CANCEL_MAKER: u8 = 1;  // 取消 Maker 订单
```

当 Taker 和 Maker 是同一个账户时:
- `CANCEL_TAKER`: 取消新订单（Taker），保留挂单（Maker）
- `CANCEL_MAKER`: 取消挂单（Maker），继续执行新订单

---

## 5. 托管模型 | Custodian Model

### 5.1 为什么需要托管？ | Why Custodian?

**传统 DEX 问题**:
- 用户每次交易都需要授权转账
- 每笔订单锁定独立的 Coin 对象
- Gas 消耗高，用户体验差

**DeepBook 解决方案**:
- ✅ 用户一次性存入资金到 Custodian
- ✅ 后续交易只更新账户余额（链上状态）
- ✅ 降低 Gas，提高效率

### 5.2 账户生命周期 | Account Lifecycle

```
┌───────────────────────────────────────────────────────┐
│ 1. 创建账户                                           │
│    let account_cap = create_account(ctx);             │
│    // 用户持有 AccountCap 对象                        │
└───────────────────────────────────────────────────────┘
                    ↓
┌───────────────────────────────────────────────────────┐
│ 2. 存入资金 (V2 已弃用)                               │
│    deposit_base(pool, coin, account_cap);             │
│    // coin 转入 custodian                             │
│    // available_balance += coin.value()               │
└───────────────────────────────────────────────────────┘
                    ↓
┌───────────────────────────────────────────────────────┐
│ 3. 下单                                               │
│    place_limit_order(                                 │
│        pool, price, quantity, ..., account_cap        │
│    );                                                 │
│    // available_balance → locked_balance              │
└───────────────────────────────────────────────────────┘
                    ↓
┌───────────────────────────────────────────────────────┐
│ 4. 订单成交                                           │
│    // 自动执行:                                       │
│    // Maker: locked → Taker's available (+ rebate)    │
│    // Taker: locked → Maker's available (- fee)       │
└───────────────────────────────────────────────────────┘
                    ↓
┌───────────────────────────────────────────────────────┐
│ 5. 取款                                               │
│    let coin = withdraw_base(pool, quantity, account); │
│    // available_balance → Coin<T>                     │
│    transfer::public_transfer(coin, recipient);        │
└───────────────────────────────────────────────────────┘
```

### 5.3 子账户系统 | Child Account System

**用例**: 交易机器人、托管服务

```move
// 管理员创建子账户
let child_cap = create_child_account_cap(admin_cap, ctx);

// 子账户只能访问自己的资金
// child_cap.owner != child_cap.id
```

**权限控制**:
```move
// custodian_v2.move:50-58
fun account_owner(account_cap: &AccountCap): address {
    account_cap.owner
}

// 所有操作验证:
assert!(account_owner(account_cap) == expected, EInvalidUser);
```

---

## 6. Critbit 树详解 | Critbit Tree Deep Dive

### 6.1 索引编码 | Index Encoding

**关键设计**: 使用单个 `u64` 同时表示叶节点和内部节点

```rust
const PARTITION_INDEX: u64 = 0x8000000000000000;  // 1 << 63

// 叶节点: index >= PARTITION_INDEX
// 内部节点: index < PARTITION_INDEX

// 叶节点实际索引
actual_leaf_index = MAX_U64 - index
```

**为什么这样设计？**
- 单一类型 `u64` 简化指针管理
- 位运算快速判断节点类型
- 节省存储空间

### 6.2 插入算法详解 | Insertion Algorithm

**步骤** (`insert_leaf` - Line 145-223):

```
1. 树为空?
   └─ 创建根叶节点，返回

2. 查找插入位置:
   while (当前是内部节点) {
       if (key & node.mask == 0) 走左子树
       else 走右子树
   }
   └─ 到达叶节点

3. 计算关键位 (critical bit):
   xor = new_key XOR existing_leaf.key
   critical_bit = 最高位 1 的位置

4. 创建新内部节点:
   mask = critical_bit
   left/right = 根据 critical_bit 决定

5. 插入新叶节点:
   找到应插入的父节点
   更新父子关系

6. 更新 min/max:
   if (new_key < min_leaf.key) min_leaf = new_key
   if (new_key > max_leaf.key) max_leaf = new_key
```

**示例**: 插入 [5, 9, 10]

```
插入 5:
  Leaf(5)

插入 9:
  5  = 0101
  9  = 1001
  XOR= 1100
  Critical bit = bit 3 (mask = 8)

        Internal(mask=8)
       /              \
   Leaf(5)          Leaf(9)

插入 10:
  9  = 1001
  10 = 1010
  XOR= 0011
  Critical bit = bit 1 (mask = 2)

        Internal(mask=8)
       /              \
   Leaf(5)       Internal(mask=2)
                 /              \
             Leaf(9)         Leaf(10)
```

### 6.3 遍历算法 | Traversal Algorithms

**最小/最大叶节点** (O(1)):
```move
// critbit.move:72-84
public fun min_leaf<V: store>(tree: &CritbitTree<V>): (u64, u64) {
    (tree.min_leaf, MAX_U64 - tree.min_leaf)
}

public fun max_leaf<V: store>(tree: &CritbitTree<V>): (u64, u64) {
    (tree.max_leaf, MAX_U64 - tree.max_leaf)
}
```

**下一个叶节点** (`next_leaf` - Line 109-124):
```
1. 如果是最大叶节点 → 返回空
2. 向上遍历找第一个右转的节点
3. 从该节点的右子树找最小值
4. 向下遍历，始终走左子树
```

### 6.4 性能分析 | Performance Analysis

| 操作 | 时间复杂度 | 说明 |
|------|-----------|------|
| 插入 | O(log n) | 最多遍历树高度 |
| 删除 | O(log n) | 需要更新父子关系 |
| 查找 | O(log n) | 二分查找 |
| 最小/最大 | **O(1)** | 维护缓存 |
| 下一个/上一个 | O(log n) | 最坏情况遍历高度 |
| 范围查询 | O(log n + k) | k 是结果数量 |

**空间复杂度**:
- n 个叶节点 → 最多 n-1 个内部节点
- 总空间: O(n)

---

## 7. 费用机制 | Fee Mechanism

### 7.1 费率配置 | Fee Configuration

```move
// Pool 中的费率字段
taker_fee_rate: u64,      // Taker 支付的费用
maker_rebate_rate: u64,   // Maker 获得的回扣

// 比例尺 (scaling factor)
const FLOAT_SCALING: u64 = 1_000_000_000;  // 10^9
```

**示例**:
```
0.25% 费用 = 2_500_000   (0.0025 * 10^9)
0.15% 回扣 = 1_500_000   (0.0015 * 10^9)
```

**约束**:
```move
assert!(taker_fee_rate >= maker_rebate_rate, ETakerFeeTooLow);
```
- Taker 费用 ≥ Maker 回扣
- 差额归协议金库

### 7.2 费用计算 | Fee Calculation

**成交时计算** (伪代码):

```move
// math.move 中的定点数运算
let base_quantity = fill_quantity;
let quote_quantity = fill_quantity * price / FLOAT_SCALING;

// Taker 费用
let taker_fee = quote_quantity * taker_fee_rate / FLOAT_SCALING;

// Maker 回扣
let maker_rebate = quote_quantity * maker_rebate_rate / FLOAT_SCALING;

// 协议收益
let protocol_fee = taker_fee - maker_rebate;
```

**资金流转**:
```
Taker: 支付 quote_quantity + taker_fee
Maker: 收到 quote_quantity + maker_rebate
Pool:  累积 taker_fee - maker_rebate
```

### 7.3 费用提取 | Fee Withdrawal

```move
// clob_v2.move:281-290
public fun withdraw_fees<BaseAsset, QuoteAsset>(
    pool: &mut Pool<BaseAsset, QuoteAsset>,
    _: &PoolOwnerCap
): Coin<QuoteAsset>
```

**权限**:
- 需要 `PoolOwnerCap` (只有池创建者持有)
- 提取累积的 `quote_asset_trading_fees`

**文件位置**: `clob_v2.move:281-290`

---

## 8. 代码导航 | Code Navigation

### 8.1 关键函数索引 | Key Functions Index

#### **订单管理** (V2 已弃用大部分)

| 函数 | 位置 | 说明 | 状态 |
|------|------|------|------|
| `place_limit_order` | clob_v2.move:534-572 | 下限价单 | ⚠️ DEPRECATED |
| `place_market_order` | clob_v2.move:574-594 | 下市价单 | ⚠️ DEPRECATED |
| `cancel_order` | clob_v2.move:596-629 | 取消单个订单 | ✅ 可用 |
| `batch_cancel_order` | clob_v2.move:709-776 | 批量取消 | ✅ 可用 |
| `cancel_all_orders` | clob_v2.move:650-699 | 取消所有订单 | ✅ 可用 |

#### **查询函数**

| 函数 | 位置 | 说明 |
|------|------|------|
| `get_market_price` | clob_v2.move:894-910 | 获取最优买卖价 |
| `get_level2_book_status_bid_side` | clob_v2.move:916-952 | 买盘深度 |
| `get_level2_book_status_ask_side` | clob_v2.move:958-994 | 卖盘深度 |
| `list_open_orders` | clob_v2.move:847-878 | 列出用户订单 |
| `account_balance` | clob_v2.move:881-889 | 查询账户余额 |

#### **托管函数**

| 函数 | 位置 | 说明 |
|------|------|------|
| `create_account` | custodian_v2.move:39-47 | 创建账户 |
| `account_balance` | custodian_v2.move:60-72 | 余额查询 |
| `lock_balance` | custodian_v2.move:127-137 | 锁定资金 |
| `unlock_balance` | custodian_v2.move:139-144 | 解锁资金 |

#### **Critbit 操作**

| 函数 | 位置 | 说明 |
|------|------|------|
| `insert_leaf` | critbit.move:145-223 | 插入价格级别 |
| `remove_leaf_by_index` | critbit.move:249-299 | 删除价格级别 |
| `min_leaf` | critbit.move:72-77 | 最低价格 |
| `max_leaf` | critbit.move:79-84 | 最高价格 |
| `next_leaf` | critbit.move:109-124 | 下一个价格 |
| `previous_leaf` | critbit.move:89-104 | 上一个价格 |

### 8.2 事件定义 | Event Definitions

```move
// clob_v2.move:40-150

struct PoolCreated has copy, drop { ... }        // Line 40-50
struct OrderPlaced has copy, drop { ... }        // Line 54-68
struct OrderCanceled has copy, drop { ... }      // Line 71-84
struct AllOrdersCanceled has copy, drop { ... }  // Line 87-106
struct OrderFilled has copy, drop { ... }        // Line 109-129
struct DepositAsset has copy, drop { ... }       // Line 133-140
struct WithdrawAsset has copy, drop { ... }      // Line 143-150
```

**事件用途**:
- 链下索引器监听
- 前端实时更新
- 审计和分析

---

## 9. 实践探索 | Hands-On Exploration

### 9.1 阅读代码路径 | Code Reading Path

**推荐学习顺序** (3-5小时):

#### **第一步: 理解数据结构** (1小时)

```bash
# 1. Pool 结构
less crates/sui-framework/packages/deepbook/sources/clob_v2.move
# 跳转到 Line 205-235，理解 Pool 定义

# 2. Order 和 TickLevel
# 同文件 Line 175-202

# 3. Custodian
less crates/sui-framework/packages/deepbook/sources/custodian_v2.move
# 完整阅读，只有 174 行

# 4. Critbit Tree
less crates/sui-framework/packages/deepbook/sources/critbit.move
# 重点: Line 39-48 (数据结构)
#       Line 145-223 (插入算法)
```

#### **第二步: 理解核心流程** (1.5小时)

```bash
# 1. 订单放置 (虽然已弃用，但逻辑仍可学习)
# clob_v2.move:534-572
# 阅读 place_limit_order 函数

# 2. 订单取消
# clob_v2.move:596-629
# 阅读 cancel_order 函数

# 3. 费用计算
# math.move:21-27, 53-59
# 理解定点数运算
```

#### **第三步: 查询和工具函数** (1小时)

```bash
# 1. 市场深度查询
# clob_v2.move:894-994

# 2. 分页查询
# order_query.move:27-103

# 3. 测试工具
# critbit.move:384-451 (test_only functions)
```

### 9.2 运行测试 | Running Tests

#### **编译 DeepBook**

```bash
# 进入 DeepBook 目录
cd crates/sui-framework/packages/deepbook

# 构建
sui move build

# 检查是否有错误
# 输出: Build Successful
```

#### **运行 Move 单元测试**

```bash
# 在 deepbook 目录
sui move test

# 运行特定测试
sui move test --filter critbit

# 详细输出
sui move test -v
```

#### **运行框架测试**

```bash
# 回到 sui 根目录
cd ../../../../

# 运行包含 DeepBook 的框架测试
UPDATE=1 cargo test -p sui-framework --test build-system-packages

# 运行特定测试
cargo test -p sui-framework deepbook
```

### 9.3 实验: 模拟订单簿 | Experiment: Simulate Order Book

创建一个简单的 Move 脚本来理解订单簿行为:

```bash
cd notes/experiments
mkdir deepbook-simulation
cd deepbook-simulation

# 创建 Move 包
sui move new order_book_sim
cd order_book_sim
```

**编辑 `sources/simulation.move`**:

```move
module order_book_sim::simulation {
    use deepbook::clob_v2::{Self, Pool};
    use deepbook::custodian_v2::{Self, AccountCap};
    use sui::coin::{Self, Coin};
    use sui::sui::SUI;

    // 模拟基础资产
    struct USD {}

    #[test_only]
    public fun test_order_flow() {
        use sui::test_scenario;

        let admin = @0xABCD;
        let trader1 = @0x1111;
        let trader2 = @0x2222;

        let scenario_val = test_scenario::begin(admin);
        let scenario = &mut scenario_val;

        // 1. 创建池 (需要 V3, V2 已弃用)
        // ...

        // 2. 创建账户
        // ...

        // 3. 下单测试
        // ...

        test_scenario::end(scenario_val);
    }
}
```

**注意**: 由于 V2 已弃用，实际实验应使用 DeepBook V3。

### 9.4 查询链上数据 | Query On-Chain Data

**使用 Sui CLI**:

```bash
# 查询 DeepBook 包
sui client object 0xdee9

# 查询特定 Pool (需要知道 Pool ID)
sui client object <POOL_ID>

# 调用只读函数
sui client call \
  --package 0xdee9 \
  --module clob_v2 \
  --function get_market_price \
  --args <POOL_ID> \
  --gas-budget 10000000
```

**使用 RPC**:

```bash
# 查询对象
curl -X POST https://fullnode.mainnet.sui.io:443 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sui_getObject",
    "params": ["<POOL_ID>"]
  }'
```

---

## 10. DeepBook V3

### 10.1 V3 改进 | V3 Improvements

**独立仓库**: https://github.com/MystenLabs/deepbookv3

**主要变化**:

1. **BigVector 替代 Critbit Tree**
   - B+ Tree 实现
   - 支持任意大小订单簿
   - 更好的动态字段利用

2. **BalanceManager 替代 AccountCap**
   - 单个共享对象管理所有池的余额
   - 跨池资金管理
   - 更好的用户体验

3. **DEEP Token 集成**
   - 质押和治理
   - 基于交易量的 Maker 回扣
   - 零费用白名单池

4. **新功能**:
   - 闪电贷 (Flash Loans)
   - EWMA 价格预言机
   - 提案式治理

### 10.2 V3 文档位置 | V3 Documentation

```
docs/content/standards/deepbookv3/
├── design.mdx              - 整体设计
├── pools.mdx               - 池管理
├── orders.mdx              - 订单系统
├── routing.mdx             - 路由和聚合
├── query-the-pool.mdx      - 查询接口
├── balance-manager.mdx     - 余额管理
├── governance.mdx          - 治理机制
└── trade-and-swap.mdx      - 交易接口
```

### 10.3 V2 到 V3 迁移 | V2 to V3 Migration

**用户需要**:
1. 取消所有 V2 订单
2. 提取 V2 池中的资金
3. 在 V3 创建 BalanceManager
4. 存入资金到 V3
5. 开始在 V3 交易

**V2 函数仍可用** (用于迁移):
- `cancel_order`
- `batch_cancel_order`
- `cancel_all_orders`
- `withdraw_base`
- `withdraw_quote`

---

## 11. 关键概念速查 | Key Concepts Cheat Sheet

### 订单簿术语 | Order Book Terms

| 术语 | 解释 |
|------|------|
| **CLOB** | Central Limit Order Book - 中央限价订单簿 |
| **Bid** | 买单 - 出价购买 |
| **Ask** | 卖单 - 要价出售 |
| **TickLevel** | 价格级别 - 相同价格的订单集合 |
| **Tick Size** | 最小价格变动单位 |
| **Lot Size** | 最小交易数量单位 |
| **Maker** | 挂单方 - 提供流动性 |
| **Taker** | 吃单方 - 消耗流动性 |
| **Spread** | 买卖价差 - best_ask - best_bid |

### 数据结构对照 | Data Structure Mapping

| 概念 | DeepBook 实现 |
|------|---------------|
| 订单簿 | `CritbitTree<TickLevel>` |
| 价格级别 | `TickLevel { price, open_orders }` |
| 同价格订单队列 | `LinkedTable<u64, Order>` |
| 用户资金 | `Custodian<T> -> Account<T>` |
| 账户权限 | `AccountCap` |

### 性能特性 | Performance Characteristics

| 操作 | 复杂度 | 说明 |
|------|--------|------|
| 下单 | O(log n + m) | n=价格级别数, m=匹配订单数 |
| 取消订单 | O(log n) | 需要在 Critbit 中查找价格 |
| 查询最优价格 | O(1) | Critbit 缓存 min/max |
| 查询市场深度 | O(log n + k) | k=查询的价格级别数 |
| 批量取消 | O(k log n) | k=订单数 |

---

## 12. 进阶研究主题 | Advanced Research Topics

### 12.1 Gas 优化技术

**批量取消的优化** (clob_v2.move:705-708):
```move
// 将相同价格的订单分组
// 每个价格级别只遍历一次 Critbit
// 显著降低 Gas 消耗
```

**为什么重要？**
- 单次取消: O(log n) 查找 + O(1) 删除
- N 次单独取消: N * O(log n)
- 批量取消（分组）: M * O(log n)，其中 M < N

### 12.2 MEV 防护

**自成交防止**:
```move
self_matching_prevention: u8
```
- 防止用户与自己的订单匹配
- 避免人为制造交易量

**订单过期机制**:
```move
expire_timestamp: u64
```
- 时间限制订单有效期
- 防止陈旧订单执行

### 12.3 对比其他 DEX

| 特性 | DeepBook | AMM (Uniswap) | Hybrid |
|------|----------|---------------|--------|
| 价格发现 | 订单驱动 | 算法定价 | 混合 |
| 滑点 | 可预测 | 依赖流动性 | 中等 |
| Gas 成本 | 中等 | 低 | 高 |
| 专业交易者 | 友好 | 一般 | 友好 |
| 链上复杂度 | 高 | 低 | 很高 |

### 12.4 可能的改进方向

1. **批量匹配**
   - 一次交易匹配多个订单
   - 降低 Gas 和延迟

2. **跨池路由**
   - 自动寻找最优执行路径
   - 类似 1inch

3. **止损/止盈订单**
   - 条件订单支持
   - 需要预言机集成

4. **保证金交易**
   - DeepBook Margin (已有文档)
   - 杠杆和清算机制

---

## 13. 学习检查清单 | Learning Checklist

### 基础理解 ✅

- [ ] 理解 CLOB 工作原理
- [ ] 掌握 Pool、Order、TickLevel 结构
- [ ] 了解 Critbit Tree 优势
- [ ] 理解 Custodian 托管模型
- [ ] 知道 Order ID 编码规则

### 中级理解 ✅

- [ ] 能解释订单匹配流程
- [ ] 理解价格-时间优先算法
- [ ] 掌握 Critbit 插入/删除算法
- [ ] 理解费用计算和分配
- [ ] 知道 AccountCap 权限模型

### 高级理解 ✅

- [ ] 能分析 Gas 优化策略
- [ ] 理解 MEV 防护机制
- [ ] 对比 DeepBook 与其他 DEX
- [ ] 了解 V3 改进
- [ ] 能设计自定义交易策略

### 实践能力 ✅

- [ ] 能阅读 DeepBook 源码
- [ ] 能运行和调试测试
- [ ] 能查询链上订单簿
- [ ] 能集成 DeepBook 到 dApp
- [ ] 能贡献改进建议

---

## 14. 参考资源 | References

### 官方文档
- **DeepBook V2 包**: `crates/sui-framework/packages/deepbook/`
- **API 文档**: `crates/sui-framework/docs/deepbook/`
- **V3 设计**: `docs/content/standards/deepbookv3/`

### 相关论文
- **Critbit Tree**: https://cr.yp.to/critbit.html
- **Order Book Algorithms**: Academic papers on CLOB implementations

### 社区资源
- **Sui Discord**: DeepBook 专门频道
- **GitHub Issues**: https://github.com/MystenLabs/sui/issues
- **DeepBook V3 Repo**: https://github.com/MystenLabs/deepbookv3

---

**研究愉快! Happy Researching!** 📚🔍

> **下一步**: 深入研究 DeepBook V3，探索最新的设计和实现。
> **Next Step**: Dive into DeepBook V3 to explore the latest design and implementation.
