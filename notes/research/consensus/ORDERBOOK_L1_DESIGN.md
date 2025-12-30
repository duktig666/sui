# Orderbook Layer1 设计文档 | Orderbook L1 Design Document

> **目标**: 基于 Sui 共识架构设计高吞吐量、低延迟的 Orderbook Layer1
> **Goal**: Design a high-throughput, low-latency Orderbook Layer1 based on Sui consensus architecture

---

## 目录 | Table of Contents

1. [设计目标与挑战](#1-设计目标与挑战)
2. [核心洞察: 为什么传统 DEX 慢](#2-核心洞察-为什么传统-dex-慢)
3. [架构设计: Intent-Centric Orderbook](#3-架构设计-intent-centric-orderbook)
4. [对象模型设计](#4-对象模型设计)
5. [交易流程详解](#5-交易流程详解)
6. [共识层优化](#6-共识层优化)
7. [撮合引擎设计](#7-撮合引擎设计)
8. [性能分析与优化](#8-性能分析与优化)
9. [容错与安全机制](#9-容错与安全机制)
10. [实现路线图](#10-实现路线图)

---

## 1. 设计目标与挑战

### 1.1 业务需求

用户视角的单一操作：
```
用户: place_order(price, quantity, side) → 订单成交/挂单
```

系统内部三个步骤：
```
Step 1: Place Order    → 锁定余额，产生订单意图
Step 2: Order Matching → 意图排序，订单簿撮合，产生结算事件
Step 3: Settlement     → 结算划转锁定余额
```

### 1.2 性能目标

| 指标 | 目标值 | 对比 Sui DeepBook |
|------|--------|-------------------|
| **下单确认时延** | < 300ms | ~2000ms |
| **最终确认时延** | < 1000ms | ~2500ms |
| **吞吐量** | > 10,000 orders/s | ~2,000 orders/s |
| **撮合延迟** | < 500ms | ~800ms |

### 1.3 核心挑战

```
传统 Orderbook 的困境:

┌─────────────────────────────────────────────────────────────┐
│                    Shared Orderbook                         │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ Bids           │ Asks                               │   │
│  │ $99.50 x 100   │ $100.00 x 50                       │   │
│  │ $99.00 x 200   │ $100.50 x 75                       │   │
│  │ ...            │ ...                                │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
         ↑              ↑              ↑              ↑
      Order A        Order B        Order C        Order D
         │              │              │              │
         └──────────────┴──────────────┴──────────────┘
                            │
                     ⚠️ 全部需要共识排序!
                     ⚠️ 串行执行!
                     ⚠️ 高竞争!

问题:
1. 订单簿是 Shared Object → 所有订单必须经过共识
2. 撮合必须串行 → 无法并行处理
3. 全局锁竞争 → 吞吐量受限
```

---

## 2. 核心洞察: 为什么传统 DEX 慢

### 2.1 Sui DeepBook 的时延分解

```
DeepBook 下单流程 (Shared Object):

T0:      用户提交下单交易
         ↓ (10ms)
T10:     验证器收到交易
         ↓
         检测: 访问 Shared Object (Pool) ⚠️
         ↓ (5ms)
T15:     提交到共识队列
         ↓ (50ms)
T65:     打包进 Consensus Block
         ↓ (600ms) ← 共识 2-3 轮
T665:    共识排序完成
         ↓ (50ms)
T715:    撮合执行
         ↓ (20ms)
T735:    生成 Effects
         ↓ (~1000ms)
T1735:   等待 Checkpoint
         ↓ (200ms)
T1935:   最终确认 ✅

总时延: ~2 秒
瓶颈: 共识排序 (600ms) + Checkpoint 等待 (1000ms)
```

### 2.2 为什么不能用 Fast Path?

```
Fast Path 要求: 所有输入对象都是 Owned Objects

订单簿操作:
  ✅ 用户余额 → Owned Object (可以 Fast Path)
  ❌ 订单簿   → Shared Object (必须共识!)

因为:
  - 订单簿被所有用户共享
  - 撮合结果依赖于全局订单顺序
  - 无法避免 Shared Object
```

### 2.3 核心洞察: 分离意图与执行

**关键发现**: 下单操作可以分解为两个独立步骤:

```
步骤分解:

1. 意图声明 (Intent Declaration):
   - 用户意图: "我想以 $100 买入 10 BTC"
   - 操作: 锁定用户余额
   - 对象类型: 仅涉及用户 Owned Objects!
   - → 可以走 Fast Path! ✅

2. 撮合执行 (Match Execution):
   - 系统操作: 批量收集意图，排序撮合
   - 对象类型: 涉及 Shared Object (订单簿)
   - → 需要共识 ⚠️
   - 但是: 可以批量处理，分摊成本!
```

---

## 3. 架构设计: Intent-Centric Orderbook

### 3.1 核心架构

```
                     ┌────────────────────────────────────────────┐
                     │           Orderbook Layer1                │
                     ├────────────────────────────────────────────┤
                     │                                            │
    用户层            │   ┌──────────────────────────────────┐    │
    (User Layer)      │   │     User Wallet (Owned)          │    │
                     │   │  ┌────────┐  ┌─────────────────┐  │    │
                     │   │  │Balance │  │ IntentBox       │  │    │
                     │   │  │ USDC   │  │ (Locked Assets) │  │    │
                     │   │  │ BTC    │  │                 │  │    │
                     │   │  └────────┘  └─────────────────┘  │    │
                     │   └──────────────────────────────────┘    │
                     │              │                            │
                     │              ↓ Fast Path (~200ms)         │
                     │   ┌──────────────────────────────────┐    │
    意图层            │   │      Intent Pool (Shared)        │    │
    (Intent Layer)    │   │  ┌────────────────────────────┐  │    │
                     │   │  │ Pending Intents Queue      │  │    │
                     │   │  │ [Intent1, Intent2, ...]    │  │    │
                     │   │  └────────────────────────────┘  │    │
                     │   └──────────────────────────────────┘    │
                     │              │                            │
                     │              ↓ Consensus (~400ms)         │
                     │   ┌──────────────────────────────────┐    │
    撮合层            │   │       Matching Engine            │    │
    (Matching Layer)  │   │  ┌────────────────────────────┐  │    │
                     │   │  │ Sharded Orderbooks         │  │    │
                     │   │  │ [Shard0] [Shard1] [Shard2] │  │    │
                     │   │  └────────────────────────────┘  │    │
                     │   │              │                    │    │
                     │   │              ↓                    │    │
                     │   │  ┌────────────────────────────┐  │    │
                     │   │  │ Settlement Events         │  │    │
                     │   │  └────────────────────────────┘  │    │
                     │   └──────────────────────────────────┘    │
                     │              │                            │
                     │              ↓ Parallel Settlement        │
                     │   ┌──────────────────────────────────┐    │
    结算层            │   │     Settlement Engine            │    │
    (Settlement Layer)│   │  - Update User Balances         │    │
                     │   │  - Release Locked Assets         │    │
                     │   │  - Emit Events                   │    │
                     │   └──────────────────────────────────┘    │
                     │                                            │
                     └────────────────────────────────────────────┘
```

### 3.2 设计原则

**原则 1: 最大化 Fast Path 使用**
```
将用户操作拆分:
  - 意图声明 → Fast Path (Owned Objects)
  - 实际撮合 → 系统自动触发 (Shared Objects)

效果:
  - 用户感知的确认时延: ~200ms (Intent 确认)
  - 实际成交时延: ~500-800ms (后台撮合)
```

**原则 2: 批量处理分摊共识成本**
```
传统方式:
  每个订单单独共识 → N 个订单 = N 次共识开销

优化方式:
  收集 100ms 内的意图 → 批量提交共识 → 一次共识处理 N 个订单
  共识成本分摊: 600ms / N 订单 = 6ms/订单 (N=100)
```

**原则 3: 订单簿分片减少竞争**
```
按价格区间分片:
  Shard 0: $0 - $100
  Shard 1: $100 - $200
  Shard 2: $200 - $300
  ...

不同分片可以并行处理 → 提高吞吐量
```

### 3.3 与 Sui 架构的对应

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Sui 架构映射                                      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Sui Concept          →  Orderbook L1 Application                  │
│  ─────────────────────────────────────────────────────────────────  │
│  Owned Object         →  User Balance, IntentBox, OrderIntent      │
│  Shared Object        →  Intent Pool, Orderbook Shards             │
│  Fast Path            →  Intent Declaration (锁定 + 创建意图)       │
│  Consensus Path       →  Batch Matching (批量撮合)                  │
│  Mysticeti DAG        →  Intent Ordering (意图排序)                 │
│  Checkpoint           →  Final Settlement Confirmation             │
│  Transaction Effects  →  Match Results, Settlement Events          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 4. 对象模型设计

### 4.1 核心对象定义

```rust
// ============================================
// Layer 1: User Layer Objects (Owned)
// ============================================

/// 用户钱包余额 (Owned Object)
/// 完全由用户控制，可以走 Fast Path
struct Balance<T> {
    id: UID,
    owner: address,
    available: u64,    // 可用余额
    locked: u64,       // 锁定余额 (用于订单)
}

/// 意图盒子 (Owned Object)
/// 存储用户锁定的资产，与订单意图关联
/// 关键: 这是 Owned Object，可以 Fast Path 操作!
struct IntentBox {
    id: UID,
    owner: address,
    locked_assets: Bag,           // 锁定的资产
    active_intents: vector<ID>,   // 活跃意图 ID 列表
    nonce: u64,                   // 防重放
}

/// 订单意图 (Owned Object → 转移给系统后变成 Shared)
/// 用户创建时是 Owned，提交后转移给 IntentPool
struct OrderIntent {
    id: UID,
    creator: address,
    intent_box_id: ID,           // 关联的 IntentBox
    market: MarketId,            // 交易对
    side: Side,                  // Buy/Sell
    price: u64,                  // 价格 (定点数)
    quantity: u64,               // 数量
    order_type: OrderType,       // Limit/Market/IOC/FOK
    timestamp: u64,              // 创建时间
    expiry: Option<u64>,         // 过期时间
    locked_asset_ref: AssetRef,  // 锁定资产引用
}

enum Side { Buy, Sell }
enum OrderType { Limit, Market, IOC, FOK, PostOnly }

// ============================================
// Layer 2: Intent Layer Objects (Shared)
// ============================================

/// 意图池 (Shared Object)
/// 收集所有待处理的订单意图
/// 需要共识排序
struct IntentPool {
    id: UID,
    pending_intents: Table<ID, OrderIntent>,  // 待处理意图
    batch_counter: u64,                        // 批次计数
    last_batch_timestamp: u64,                 // 上次批处理时间
}

/// 意图批次 (由系统创建)
/// 包含一批要撮合的意图
struct IntentBatch {
    id: UID,
    batch_number: u64,
    intents: vector<OrderIntent>,
    consensus_timestamp: u64,     // 共识确定的时间戳
    ordered_by_consensus: bool,   // 是否已排序
}

// ============================================
// Layer 3: Matching Layer Objects (Shared)
// ============================================

/// 订单簿分片 (Shared Object)
/// 按价格区间分片，减少竞争
struct OrderbookShard {
    id: UID,
    market: MarketId,
    shard_index: u8,
    price_range: (u64, u64),      // 价格区间 [min, max)
    bids: CritBitTree<Order>,     // 买单 (按价格降序)
    asks: CritBitTree<Order>,     // 卖单 (按价格升序)
    order_count: u64,
}

/// 订单簿路由 (Shared Object)
/// 管理所有分片
struct OrderbookRouter {
    id: UID,
    market: MarketId,
    shards: vector<ID>,           // 分片 ID 列表
    shard_boundaries: vector<u64>, // 分片价格边界
    total_bids: u64,
    total_asks: u64,
}

/// 活跃订单 (存储在 Shard 中)
struct Order {
    id: ID,
    intent_id: ID,               // 原始意图 ID
    owner: address,
    price: u64,
    remaining_quantity: u64,
    original_quantity: u64,
    timestamp: u64,
    intent_box_id: ID,           // 用于结算
}

// ============================================
// Layer 4: Settlement Layer Objects
// ============================================

/// 结算事件 (Event, 非 Object)
struct SettlementEvent has copy, drop {
    match_id: ID,
    market: MarketId,
    maker_order_id: ID,
    taker_order_id: ID,
    maker: address,
    taker: address,
    price: u64,
    quantity: u64,
    maker_receives: AssetTransfer,
    taker_receives: AssetTransfer,
    timestamp: u64,
}

/// 资产转移描述
struct AssetTransfer has copy, drop, store {
    asset_type: TypeName,
    amount: u64,
    from_intent_box: ID,
    to_address: address,
}
```

### 4.2 对象所有权与生命周期

```
订单生命周期:

┌─────────────────────────────────────────────────────────────────────┐
│                                                                     │
│  阶段 1: 意图创建 (Fast Path)                                        │
│  ─────────────────────────────────                                  │
│                                                                     │
│  User Wallet          IntentBox           OrderIntent               │
│  ┌──────────┐        ┌──────────┐        ┌──────────┐              │
│  │ Balance  │ ──────→│ Locked   │        │ Created  │              │
│  │ -100 USDC│        │ +100 USDC│        │ Buy BTC  │              │
│  │ (Owned)  │        │ (Owned)  │        │ (Owned)  │              │
│  └──────────┘        └──────────┘        └──────────┘              │
│                                                 │                   │
│                                                 │ 转移给系统         │
│                                                 ↓                   │
│  阶段 2: 意图提交 (Consensus)                                        │
│  ─────────────────────────────                                      │
│                                                                     │
│                           IntentPool (Shared)                       │
│                          ┌─────────────────────┐                   │
│                          │ Pending Intents     │                   │
│                          │ [Intent1, Intent2]  │                   │
│                          └─────────────────────┘                   │
│                                     │                               │
│                                     │ 共识排序                       │
│                                     ↓                               │
│  阶段 3: 批量撮合                                                    │
│  ───────────────                                                    │
│                           IntentBatch                               │
│                          ┌─────────────────────┐                   │
│                          │ Ordered Intents     │                   │
│                          │ [I1, I2, I3, ...]   │                   │
│                          └─────────────────────┘                   │
│                                     │                               │
│                                     │ 撮合引擎处理                   │
│                                     ↓                               │
│                    ┌────────────────┴────────────────┐             │
│                    ↓                                 ↓             │
│              完全成交                            部分成交/挂单       │
│         ┌─────────────────┐                 ┌─────────────────┐   │
│         │ 删除 Intent     │                 │ 转为 Order      │   │
│         │ 结算资产        │                 │ 存入 Shard      │   │
│         └─────────────────┘                 └─────────────────┘   │
│                    │                                 │             │
│                    │                                 │             │
│                    ↓                                 ↓             │
│  阶段 4: 结算                                                       │
│  ──────────                                                        │
│                                                                     │
│  IntentBox A        IntentBox B        OrderbookShard              │
│  ┌──────────┐      ┌──────────┐       ┌──────────────┐            │
│  │ -100 USDC│      │ +100 USDC│       │ Order stored │            │
│  │ +0.01 BTC│      │ -0.01 BTC│       │ waiting match│            │
│  └──────────┘      └──────────┘       └──────────────┘            │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.3 对象访问模式分析

```
对象访问分类:

┌─────────────────┬─────────────┬───────────────┬────────────────────┐
│ 对象            │ 所有权      │ 访问路径      │ 并发特性           │
├─────────────────┼─────────────┼───────────────┼────────────────────┤
│ Balance         │ Owned       │ Fast Path     │ 用户独占           │
│ IntentBox       │ Owned       │ Fast Path     │ 用户独占           │
│ OrderIntent     │ Owned→Shared│ Fast→Consensus│ 创建后转移         │
│ IntentPool      │ Shared      │ Consensus     │ 写入并发高         │
│ IntentBatch     │ System      │ Internal      │ 系统内部           │
│ OrderbookShard  │ Shared      │ Consensus     │ 分片减少竞争       │
│ OrderbookRouter │ Shared      │ Consensus     │ 只读热点           │
└─────────────────┴─────────────┴───────────────┴────────────────────┘

优化策略:
1. Balance/IntentBox: 用户独占，无竞争，Fast Path 最优
2. IntentPool: 高并发写入 → 使用队列结构减少锁竞争
3. OrderbookShard: 分片隔离 → 不同价格区间可并行
4. OrderbookRouter: 读多写少 → 可缓存
```

---

## 5. 交易流程详解

### 5.1 用户下单完整流程

```
用户调用: place_order(market, side, price, quantity)

════════════════════════════════════════════════════════════════════════
 Phase 1: Intent Declaration (Fast Path) - 用户感知的主要时延
════════════════════════════════════════════════════════════════════════

T0:      用户签名交易
         ↓
T5:      交易到达验证器
         │
         │ 验证器检查:
         │ - 输入对象: Balance (Owned), IntentBox (Owned) ✅
         │ - 判定: Fast Path!
         ↓
T10:     执行 Intent 创建:
         │
         │ move_call: orderbook::place_order {
         │   1. 检查余额充足
         │   2. 从 Balance 锁定资产到 IntentBox
         │   3. 创建 OrderIntent (Owned)
         │   4. 递增 nonce (防重放)
         │   5. 返回 intent_id
         │ }
         ↓
T15:     生成 TransactionEffects
         ↓
T20:     签名 Effects
         ↓
T70:     广播签名
         ↓
T150:    收集 2f+1 签名 → Certificate
         │
         │ ⭐ 此时用户收到确认:
         │    "订单已提交，等待撮合"
         │    intent_id: 0xabc...
         ↓
T200:    用户确认完成 ✅ (感知时延: ~200ms)

════════════════════════════════════════════════════════════════════════
 Phase 2: Intent Submission (Consensus Path) - 后台自动执行
════════════════════════════════════════════════════════════════════════

T200:    OrderIntent 提交到 IntentPool
         │
         │ 这是第二个交易:
         │ move_call: intent_pool::submit_intent {
         │   input: OrderIntent (转移所有权)
         │   shared: IntentPool
         │ }
         │
         │ 因为涉及 Shared Object → Consensus Path
         ↓
T210:    提交到共识队列
         ↓
T260:    打包进 Consensus Block
         │
         │ Block 包含多个 submit_intent 交易
         │ 批量处理!
         ↓
T660:    共识排序完成 (2-3 轮, ~400ms)
         ↓
T680:    执行 submit_intent:
         │ - OrderIntent 加入 IntentPool.pending_intents
         │ - 检查是否触发批处理
         ↓
T700:    Intent 已提交到池中 ✅

════════════════════════════════════════════════════════════════════════
 Phase 3: Batch Matching (System Triggered) - 系统自动触发
════════════════════════════════════════════════════════════════════════

         触发条件 (满足任一):
         - 累积意图数 > 100
         - 距上次批处理 > 100ms
         - 有 Market Order 需要立即处理

T700:    系统触发批处理交易:
         │
         │ move_call: matching_engine::process_batch {
         │   shared: IntentPool
         │   shared: OrderbookShard[] (相关分片)
         │ }
         ↓
T710:    提交共识 (已经在共识中，无需等待)
         ↓
T750:    执行撮合:
         │
         │ ┌─────────────────────────────────────┐
         │ │ Matching Engine Algorithm          │
         │ │                                     │
         │ │ 1. 从 IntentPool 获取批次          │
         │ │ 2. 按时间戳排序 (公平排序)         │
         │ │ 3. 对每个 Intent:                  │
         │ │    a. 确定目标 Shard              │
         │ │    b. 尝试撮合                     │
         │ │    c. 记录 SettlementEvent        │
         │ │    d. 未成交部分转为 Order         │
         │ │ 4. 返回 SettlementEvents          │
         │ └─────────────────────────────────────┘
         ↓
T850:    撮合完成，生成 SettlementEvents ✅

════════════════════════════════════════════════════════════════════════
 Phase 4: Settlement (Parallel Execution) - 并行结算
════════════════════════════════════════════════════════════════════════

T850:    触发结算交易 (可并行):
         │
         │ 对每个 SettlementEvent:
         │ move_call: settlement::execute {
         │   owned: IntentBox (maker)
         │   owned: IntentBox (taker)
         │ }
         │
         │ ⭐ 注意: 不同用户的结算可以并行!
         │    因为 IntentBox 是 Owned Object
         ↓
T870:    执行结算 (并行):
         │
         │ User A IntentBox: -100 USDC, +0.01 BTC
         │ User B IntentBox: +100 USDC, -0.01 BTC
         ↓
T890:    解锁资产到 Balance:
         │
         │ User A Balance: +0.01 BTC (可用)
         │ User B Balance: +100 USDC (可用)
         ↓
T900:    结算完成 ✅

════════════════════════════════════════════════════════════════════════
 Phase 5: Checkpoint Finality
════════════════════════════════════════════════════════════════════════

T900:    等待 Checkpoint (~100-1000ms)
         ↓
T1100:   打包进 Checkpoint
         ↓
T1200:   最终确认 ✅

════════════════════════════════════════════════════════════════════════

总结:
┌─────────────────────────────────────────────────────────────────────┐
│                                                                     │
│  用户感知时延:                                                       │
│    Intent 确认: ~200ms (Fast Path)                                 │
│    成交通知:    ~900ms (从下单到成交)                               │
│    最终确认:    ~1200ms (Checkpoint)                               │
│                                                                     │
│  对比传统 DEX:                                                      │
│    传统下单确认: ~2000ms (全程共识)                                 │
│    本方案下单确认: ~200ms (Fast Path)                              │
│    改进: 10x 更快的用户响应!                                        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 5.2 交易类型汇总

```rust
// ============================================
// 交易类型定义
// ============================================

/// Tx 1: 创建意图 (Fast Path)
/// 输入: Balance (Owned), IntentBox (Owned)
/// 输出: OrderIntent (Owned), 更新后的 Balance, IntentBox
public entry fun place_order(
    balance: &mut Balance<USDC>,
    intent_box: &mut IntentBox,
    market: MarketId,
    side: Side,
    price: u64,
    quantity: u64,
    order_type: OrderType,
    ctx: &mut TxContext,
): OrderIntent {
    // 1. 计算需要锁定的金额
    let lock_amount = calculate_lock_amount(side, price, quantity);

    // 2. 从 Balance 转移到 IntentBox
    let locked = balance::withdraw(balance, lock_amount);
    intent_box::deposit(intent_box, locked);

    // 3. 创建 OrderIntent
    let intent = OrderIntent {
        id: object::new(ctx),
        creator: tx_context::sender(ctx),
        intent_box_id: object::id(intent_box),
        market,
        side,
        price,
        quantity,
        order_type,
        timestamp: tx_context::epoch_timestamp_ms(ctx),
        expiry: option::none(),
        locked_asset_ref: /* ... */,
    };

    // 4. 记录到 IntentBox
    vector::push_back(&mut intent_box.active_intents, object::id(&intent));
    intent_box.nonce = intent_box.nonce + 1;

    intent
}

/// Tx 2: 提交意图到池 (Consensus Path)
/// 输入: OrderIntent (Owned, 转移), IntentPool (Shared)
public entry fun submit_intent(
    intent: OrderIntent,
    pool: &mut IntentPool,
    ctx: &TxContext,
) {
    // 验证意图
    validate_intent(&intent);

    // 加入待处理队列
    table::add(&mut pool.pending_intents, object::id(&intent), intent);

    // 检查是否应该触发批处理
    if should_trigger_batch(pool, ctx) {
        // 发出事件通知系统
        event::emit(BatchTriggerEvent { batch_number: pool.batch_counter });
    }
}

/// Tx 3: 批量撮合 (Consensus Path, 系统调用)
/// 输入: IntentPool (Shared), OrderbookShard[] (Shared)
public entry fun process_batch(
    pool: &mut IntentPool,
    shards: &mut vector<OrderbookShard>,
    clock: &Clock,
    ctx: &TxContext,
) {
    // 只允许系统调用
    assert!(is_system_caller(ctx), E_UNAUTHORIZED);

    // 1. 收集当前批次的意图
    let batch = collect_batch(pool);

    // 2. 公平排序
    let ordered = fair_sort(batch);

    // 3. 执行撮合
    let settlements = vector::empty<SettlementEvent>();

    for intent in ordered {
        let shard = find_shard(shards, &intent);
        let (matched, remaining) = match_intent(shard, intent);

        // 记录成交
        vector::append(&mut settlements, matched);

        // 未成交部分转为挂单
        if (remaining.quantity > 0) {
            insert_order(shard, remaining);
        }
    }

    // 4. 发出结算事件
    for event in settlements {
        event::emit(event);
    }

    pool.batch_counter = pool.batch_counter + 1;
    pool.last_batch_timestamp = clock::timestamp_ms(clock);
}

/// Tx 4: 执行结算 (Fast Path, 可并行)
/// 输入: IntentBox (Owned) x2
public entry fun settle(
    maker_box: &mut IntentBox,
    taker_box: &mut IntentBox,
    event: SettlementEvent,
    ctx: &TxContext,
) {
    // 验证事件
    validate_settlement_event(&event);

    // 执行资产转移
    let maker_receives = intent_box::withdraw(
        taker_box,
        event.maker_receives.asset_type,
        event.maker_receives.amount
    );
    intent_box::deposit(maker_box, maker_receives);

    let taker_receives = intent_box::withdraw(
        maker_box,
        event.taker_receives.asset_type,
        event.taker_receives.amount
    );
    intent_box::deposit(taker_box, taker_receives);

    // 更新活跃意图列表
    cleanup_intent(maker_box, event.maker_order_id);
    cleanup_intent(taker_box, event.taker_order_id);
}
```

### 5.3 取消订单流程

```
取消订单流程:

════════════════════════════════════════════════════════════════════════
 Case 1: 意图未提交 (Fast Path)
════════════════════════════════════════════════════════════════════════

用户持有 OrderIntent (Owned Object):
  → 直接销毁 + 解锁资产
  → Fast Path, ~200ms

move_call: orderbook::cancel_intent {
    intent: OrderIntent (Owned, 销毁)
    intent_box: &mut IntentBox (Owned)
}

════════════════════════════════════════════════════════════════════════
 Case 2: 意图已提交到 Pool (Consensus Path)
════════════════════════════════════════════════════════════════════════

Intent 已转移到 IntentPool:
  → 需要从 Pool 中移除
  → Consensus Path, ~800ms

move_call: orderbook::cancel_from_pool {
    pool: &mut IntentPool (Shared)
    intent_id: ID
    intent_box: &mut IntentBox (Owned)
}

验证: 只有原创建者可以取消

════════════════════════════════════════════════════════════════════════
 Case 3: 已转为挂单 (Consensus Path)
════════════════════════════════════════════════════════════════════════

Order 存储在 OrderbookShard:
  → 需要从 Shard 中移除
  → Consensus Path, ~800ms

move_call: orderbook::cancel_order {
    shard: &mut OrderbookShard (Shared)
    order_id: ID
    intent_box: &mut IntentBox (Owned)
}

验证: 只有原创建者可以取消
```

---

## 6. 共识层优化

### 6.1 Mysticeti 参数调优

```rust
// consensus/config/src/parameters.rs

/// Orderbook L1 优化参数
impl OrderbookParameters {
    /// 更激进的轮次时间
    /// 原因: Orderbook 对延迟敏感
    pub fn leader_timeout() -> Duration {
        Duration::from_millis(150)  // Sui 默认: 200ms
    }

    /// 更短的最小轮次延迟
    /// 原因: 高频交易场景
    pub fn min_round_delay() -> Duration {
        Duration::from_millis(30)   // Sui 默认: 50ms
    }

    /// 优化的批处理大小
    pub fn max_block_transactions() -> usize {
        500  // 单 block 最大交易数
    }

    /// 意图批处理间隔
    pub fn intent_batch_interval() -> Duration {
        Duration::from_millis(100)  // 100ms 收集一批
    }
}
```

### 6.2 批处理策略

```
Intent 批处理策略:

┌─────────────────────────────────────────────────────────────────────┐
│                     Intent Batch Collection                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  时间窗口: 100ms                                                    │
│                                                                     │
│  T0─────────T50────────T100ms                                      │
│   │         │           │                                           │
│   │ Intent1 │ Intent3   │                                           │
│   │ Intent2 │ Intent4   │                                           │
│   │         │ Intent5   │                                           │
│   └─────────┴───────────┘                                           │
│             │                                                       │
│             ↓                                                       │
│   ┌─────────────────────────┐                                      │
│   │ IntentBatch #N          │                                      │
│   │ [I1, I2, I3, I4, I5]    │                                      │
│   │ consensus_order: [...]  │                                      │
│   └─────────────────────────┘                                      │
│             │                                                       │
│             ↓ 一次共识处理 5 个意图                                   │
│                                                                     │
│  共识成本分摊:                                                       │
│    原本: 5 × 400ms = 2000ms (串行)                                  │
│    现在: 400ms + 50ms(撮合) = 450ms (批量)                          │
│    提升: 4.4x                                                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

触发条件 (OR):
┌─────────────────────────────────────────────────────────────────────┐
│ 1. 时间触发: 距上次批处理 >= 100ms                                  │
│ 2. 数量触发: 累积意图数 >= 100                                      │
│ 3. 优先触发: 存在 Market Order (需立即处理)                         │
│ 4. 跨价触发: 存在可立即成交的意图对                                  │
└─────────────────────────────────────────────────────────────────────┘
```

### 6.3 共识优先级

```rust
/// 交易优先级队列
/// 高优先级交易优先打包
enum TransactionPriority {
    /// 最高: Market Orders (需要立即执行)
    Critical = 0,

    /// 高: 可立即成交的 Limit Orders
    High = 1,

    /// 中: 普通 Limit Orders
    Normal = 2,

    /// 低: 取消订单
    Low = 3,
}

impl ConsensusBlock {
    /// 按优先级排序交易
    fn sort_transactions(&mut self) {
        self.transactions.sort_by_key(|tx| {
            match tx {
                Tx::MarketOrder(_) => TransactionPriority::Critical,
                Tx::LimitOrder(o) if o.can_match_immediately()
                    => TransactionPriority::High,
                Tx::LimitOrder(_) => TransactionPriority::Normal,
                Tx::CancelOrder(_) => TransactionPriority::Low,
            }
        });
    }
}
```

### 6.4 Checkpoint 频率优化

```rust
// Orderbook L1 的 Checkpoint 策略

/// 更频繁的 Checkpoint
/// 原因: 交易确认需要 Checkpoint
const MIN_CHECKPOINT_INTERVAL_MS: u64 = 100;  // Sui: 200ms

/// 动态 Checkpoint 策略
fn should_create_checkpoint(
    last_checkpoint: u64,
    pending_settlements: u64,
    current_time: u64,
) -> bool {
    let elapsed = current_time - last_checkpoint;

    // 条件 1: 最小间隔
    if elapsed < MIN_CHECKPOINT_INTERVAL_MS {
        return false;
    }

    // 条件 2: 有足够结算等待确认
    if pending_settlements > 50 {
        return true;
    }

    // 条件 3: 最大间隔
    if elapsed > 500 {
        return true;
    }

    false
}
```

---

## 7. 撮合引擎设计

### 7.1 撮合算法

```rust
/// 撮合引擎核心算法
pub struct MatchingEngine;

impl MatchingEngine {
    /// 处理意图批次
    pub fn process_batch(
        &mut self,
        batch: IntentBatch,
        shards: &mut Vec<OrderbookShard>,
    ) -> Vec<SettlementEvent> {
        let mut settlements = Vec::new();

        // 1. 公平排序: 按共识时间戳 + 意图创建时间
        let sorted_intents = self.fair_sort(batch.intents);

        // 2. 逐个处理意图
        for intent in sorted_intents {
            let shard = self.find_shard(shards, &intent);

            match intent.order_type {
                OrderType::Market => {
                    // Market Order: 尽可能成交
                    let matched = self.match_market_order(shard, intent);
                    settlements.extend(matched);
                }
                OrderType::Limit => {
                    // Limit Order: 撮合或挂单
                    let (matched, remaining) = self.match_limit_order(shard, intent);
                    settlements.extend(matched);
                    if let Some(order) = remaining {
                        shard.insert_order(order);
                    }
                }
                OrderType::IOC => {
                    // Immediate-Or-Cancel: 只尝试撮合
                    let matched = self.match_ioc_order(shard, intent);
                    settlements.extend(matched);
                    // 未成交部分自动取消
                }
                OrderType::FOK => {
                    // Fill-Or-Kill: 全部成交或取消
                    if self.can_fill_completely(shard, &intent) {
                        let matched = self.match_fok_order(shard, intent);
                        settlements.extend(matched);
                    }
                    // 无法全部成交则取消
                }
                OrderType::PostOnly => {
                    // Post-Only: 只挂单，不吃单
                    if !self.would_cross_book(shard, &intent) {
                        shard.insert_order(intent.into_order());
                    }
                    // 会吃单则取消
                }
            }
        }

        settlements
    }

    /// 撮合 Limit Order
    fn match_limit_order(
        &mut self,
        shard: &mut OrderbookShard,
        intent: OrderIntent,
    ) -> (Vec<SettlementEvent>, Option<Order>) {
        let mut settlements = Vec::new();
        let mut remaining_qty = intent.quantity;

        // 获取对手盘
        let opposite_orders = match intent.side {
            Side::Buy => shard.asks.iter_ascending(),
            Side::Sell => shard.bids.iter_descending(),
        };

        for order in opposite_orders {
            // 检查价格是否匹配
            if !self.prices_cross(intent.side, intent.price, order.price) {
                break;
            }

            // 计算成交量
            let match_qty = std::cmp::min(remaining_qty, order.remaining_quantity);

            // 创建结算事件
            let event = SettlementEvent {
                match_id: generate_id(),
                market: intent.market,
                maker_order_id: order.id,
                taker_order_id: object::id(&intent),
                maker: order.owner,
                taker: intent.creator,
                price: order.price,  // 使用 maker 价格
                quantity: match_qty,
                maker_receives: calculate_maker_receives(&intent, match_qty),
                taker_receives: calculate_taker_receives(&order, match_qty),
                timestamp: current_timestamp(),
            };
            settlements.push(event);

            // 更新剩余数量
            remaining_qty -= match_qty;
            order.remaining_quantity -= match_qty;

            // 移除完全成交的订单
            if order.remaining_quantity == 0 {
                shard.remove_order(order.id);
            }

            if remaining_qty == 0 {
                break;
            }
        }

        // 返回未成交部分
        let remaining = if remaining_qty > 0 {
            Some(Order {
                id: generate_id(),
                intent_id: object::id(&intent),
                owner: intent.creator,
                price: intent.price,
                remaining_quantity: remaining_qty,
                original_quantity: intent.quantity,
                timestamp: intent.timestamp,
                intent_box_id: intent.intent_box_id,
            })
        } else {
            None
        };

        (settlements, remaining)
    }
}
```

### 7.2 订单簿数据结构

```rust
/// 使用 Crit-Bit Tree 实现高效订单簿
/// 特点: O(log N) 插入/删除, O(1) 最优价格
pub struct CritBitTree<V> {
    root: Option<Node<V>>,
    len: usize,
}

enum Node<V> {
    Internal {
        prefix: u64,
        bit_index: u8,
        left: Box<Node<V>>,
        right: Box<Node<V>>,
    },
    Leaf {
        key: u64,          // price
        values: Vec<V>,    // orders at this price (FIFO)
    },
}

impl OrderbookShard {
    /// 插入订单
    /// 时间复杂度: O(log P), P = 价格档位数
    pub fn insert_order(&mut self, order: Order) {
        let tree = match order.side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        match tree.get_mut(&order.price) {
            Some(orders) => {
                // 价格档位已存在，追加到队列
                orders.push(order);
            }
            None => {
                // 新价格档位
                tree.insert(order.price, vec![order]);
            }
        }

        self.order_count += 1;
    }

    /// 获取最优买价
    pub fn best_bid(&self) -> Option<u64> {
        self.bids.max_key()
    }

    /// 获取最优卖价
    pub fn best_ask(&self) -> Option<u64> {
        self.asks.min_key()
    }
}
```

### 7.3 分片策略

```
订单簿分片策略:

┌─────────────────────────────────────────────────────────────────────┐
│                       BTC/USDC Market                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  动态分片边界 (基于历史价格分布):                                     │
│                                                                     │
│  Shard 0: $0 - $30,000         (极低价, 低频)                       │
│  Shard 1: $30,000 - $40,000    (低价区)                             │
│  Shard 2: $40,000 - $45,000    (活跃区下)                           │
│  Shard 3: $45,000 - $50,000    (活跃区中) ← 当前价格附近            │
│  Shard 4: $50,000 - $55,000    (活跃区上)                           │
│  Shard 5: $55,000 - $70,000    (高价区)                             │
│  Shard 6: $70,000+             (极高价, 低频)                       │
│                                                                     │
│  分片优势:                                                          │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ Order at $46,000 → Shard 3                                    │ │
│  │ Order at $51,000 → Shard 4                                    │ │
│  │                                                               │ │
│  │ 两个订单访问不同 Shard → 可并行处理!                            │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  跨分片撮合:                                                        │
│  当 Taker 订单可能跨越多个分片时:                                    │
│  1. Market Order 买入大量 → 可能消耗多个分片的卖单                   │
│  2. 撮合引擎按顺序访问相邻分片                                       │
│  3. 原子性保证: 要么全部成功，要么回滚                               │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

分片数量选择:
┌─────────────────────────────────────────────────────────────────────┐
│ 分片数  │ 优点                    │ 缺点                          │
├─────────┼─────────────────────────┼───────────────────────────────┤
│ 少 (4)  │ 简单，跨分片少          │ 热点分片竞争高                │
│ 中 (8)  │ 平衡                    │ 推荐                          │
│ 多 (16) │ 并行度高                │ 跨分片操作复杂                │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 8. 性能分析与优化

### 8.1 时延分解

```
完整时延分解 (下单到成交):

┌─────────────────────────────────────────────────────────────────────┐
│                                                                     │
│  阶段                    │ 时间      │ 累计      │ 关键路径?        │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  Phase 1: Intent (Fast)  │           │           │                 │
│  ├─ 网络传输             │ 20ms      │ 20ms      │ ✅              │
│  ├─ 锁定余额执行         │ 10ms      │ 30ms      │ ✅              │
│  ├─ 签名 Effects         │ 5ms       │ 35ms      │ ✅              │
│  ├─ 广播签名             │ 50ms      │ 85ms      │ ✅              │
│  └─ 收集 2f+1 签名       │ 100ms     │ 185ms     │ ✅              │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  ⭐ 用户确认点           │           │ ~200ms    │                 │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  Phase 2: Submit (Cons)  │           │           │                 │
│  ├─ 提交共识             │ 10ms      │ 195ms     │ ✅              │
│  ├─ 打包进 Block         │ 50ms      │ 245ms     │ ✅              │
│  └─ 共识排序             │ 300ms     │ 545ms     │ ✅              │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  Phase 3: Batch Match    │           │           │                 │
│  ├─ 批量收集             │ ~50ms     │ 595ms     │ 可重叠          │
│  ├─ 公平排序             │ 5ms       │ 600ms     │ ✅              │
│  └─ 撮合执行             │ 20ms      │ 620ms     │ ✅              │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  Phase 4: Settlement     │           │           │                 │
│  ├─ 结算执行 (并行)      │ 30ms      │ 650ms     │ 可并行          │
│  └─ 签名收集             │ 80ms      │ 730ms     │ ✅              │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  ⭐ 成交确认点           │           │ ~750ms    │                 │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  Phase 5: Checkpoint     │           │           │                 │
│  ├─ 等待 Checkpoint      │ ~300ms    │ 1050ms    │ 可重叠          │
│  └─ Checkpoint 确认      │ 100ms     │ 1150ms    │ ✅              │
│  ────────────────────────┼───────────┼───────────┼─────────────────│
│  ⭐ 最终确认点           │           │ ~1150ms   │                 │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

对比传统 DEX (如 Sui DeepBook):
┌─────────────────────────────────────────────────────────────────────┐
│                          │ 本方案      │ DeepBook    │ 改进        │
├──────────────────────────┼─────────────┼─────────────┼─────────────┤
│ 用户确认                 │ 200ms       │ 2000ms      │ 10x ⬆️      │
│ 成交确认                 │ 750ms       │ 2000ms      │ 2.7x ⬆️     │
│ 最终确认                 │ 1150ms      │ 2500ms      │ 2.2x ⬆️     │
└─────────────────────────────────────────────────────────────────────┘
```

### 8.2 吞吐量分析

```
吞吐量分析:

════════════════════════════════════════════════════════════════════════
 Phase 1: Intent (Fast Path) - 无瓶颈
════════════════════════════════════════════════════════════════════════

理论吞吐量:
  - 每个验证器独立处理
  - 并行执行 (不同用户无冲突)
  - 限制因素: 验证器 CPU/网络

  单验证器: ~10,000 intents/s
  100 验证器并行: 实际 ~50,000 intents/s (考虑网络)

════════════════════════════════════════════════════════════════════════
 Phase 2: Submit (Consensus Path) - 第一个瓶颈
════════════════════════════════════════════════════════════════════════

共识吞吐量:
  - 轮次时间: 200ms
  - 每轮 Block: 100 验证器 × 1 block = 100 blocks
  - 每 Block 交易数: 500

  TPS = 100 blocks × 500 txs / 0.2s = 250,000 TPS (理论)

  实际限制:
  - 网络带宽
  - 签名验证
  - 状态访问

  实际 TPS: ~10,000-20,000 TPS

════════════════════════════════════════════════════════════════════════
 Phase 3: Batch Match - 第二个瓶颈
════════════════════════════════════════════════════════════════════════

撮合吞吐量:
  - 批处理间隔: 100ms
  - 每批意图数: ~1,000 (假设)
  - 撮合时间: ~50ms

  TPS = 1,000 / 0.15s = 6,666 orders/s

  优化方向:
  - 分片并行: 8 分片 × 6,666 = 53,328 orders/s
  - 更大批次: 10,000 / 0.15s = 66,666 orders/s

════════════════════════════════════════════════════════════════════════
 Phase 4: Settlement - 可并行
════════════════════════════════════════════════════════════════════════

结算吞吐量:
  - 每个结算独立 (不同用户)
  - 可完全并行
  - 无共享状态竞争

  TPS: 与 Phase 1 类似，不是瓶颈

════════════════════════════════════════════════════════════════════════
 总吞吐量估算
════════════════════════════════════════════════════════════════════════

瓶颈分析:
  Phase 1 (Intent):    50,000+  ← 非瓶颈
  Phase 2 (Consensus): 10,000-20,000  ← 共识限制
  Phase 3 (Matching):  10,000-50,000  ← 取决于分片
  Phase 4 (Settle):    50,000+  ← 非瓶颈

实际吞吐量: min(10,000, 10,000, 50,000) = ~10,000 orders/s

对比:
  DeepBook: ~2,000 orders/s
  本方案:   ~10,000 orders/s
  改进:     5x ⬆️
```

### 8.3 优化策略

```rust
/// 优化策略汇总
mod optimizations {

    /// 1. 预执行优化
    /// 在共识前预测并行执行
    fn speculative_execution(intents: &[OrderIntent]) -> Vec<SpecResult> {
        // 假设意图按当前顺序处理
        // 预先计算可能的结果
        // 共识后快速验证或重算
    }

    /// 2. 热点分片动态调整
    fn rebalance_shards(router: &mut OrderbookRouter, stats: &ShardStats) {
        // 监控每个分片的访问频率
        // 将热点分片进一步拆分
        // 将冷门分片合并
    }

    /// 3. 意图聚合
    /// 相同价格的意图合并处理
    fn aggregate_intents(batch: Vec<OrderIntent>) -> Vec<AggregatedIntent> {
        // 按 (市场, 方向, 价格) 分组
        // 合并数量
        // 减少订单簿操作次数
    }

    /// 4. 零拷贝序列化
    fn zero_copy_serialize<T: BorshSerialize>(obj: &T) -> &[u8] {
        // 使用内存映射
        // 避免数据复制
    }

    /// 5. 批量签名验证
    fn batch_verify_signatures(sigs: &[Signature]) -> bool {
        // 使用 BLS 聚合签名
        // 一次验证多个签名
    }

    /// 6. 流水线处理
    /// 共识和撮合重叠执行
    fn pipeline_process() {
        // Round N: 共识中
        // Round N-1: 撮合中
        // Round N-2: 结算中
    }
}
```

### 8.4 性能监控指标

```rust
/// 关键性能指标
struct PerformanceMetrics {
    // 时延指标
    intent_latency_p50: Duration,      // Intent 确认 P50
    intent_latency_p99: Duration,      // Intent 确认 P99
    match_latency_p50: Duration,       // 成交 P50
    match_latency_p99: Duration,       // 成交 P99
    settlement_latency: Duration,      // 结算时延

    // 吞吐量指标
    intents_per_second: u64,           // Intent TPS
    matches_per_second: u64,           // 成交 TPS
    settlements_per_second: u64,       // 结算 TPS

    // 批处理指标
    avg_batch_size: u64,               // 平均批次大小
    batch_interval: Duration,          // 批处理间隔
    batch_utilization: f64,            // 批次利用率

    // 共识指标
    consensus_round_time: Duration,    // 共识轮次时间
    consensus_throughput: u64,         // 共识 TPS

    // 分片指标
    shard_load_distribution: Vec<f64>, // 分片负载分布
    cross_shard_rate: f64,             // 跨分片操作比例
}

/// Prometheus 查询
const PROMETHEUS_QUERIES: &str = r#"
# Intent P99 时延
histogram_quantile(0.99, rate(intent_latency_bucket[1m]))

# 成交 TPS
rate(matches_total[1m])

# 分片负载不均衡度
stddev(shard_operations_total) / avg(shard_operations_total)

# 批处理效率
avg(batch_size) / max_batch_size
"#;
```

---

## 9. 容错与安全机制

### 9.1 故障场景处理

```
故障场景与处理:

════════════════════════════════════════════════════════════════════════
 场景 1: Intent 提交后验证器崩溃
════════════════════════════════════════════════════════════════════════

时间线:
T0:    用户创建 Intent (Fast Path 成功)
T50:   提交 Intent 到 Pool
T100:  验证器崩溃 ⚠️

处理:
1. Intent 已经有 Certificate → 不可逆
2. 其他验证器继续处理
3. 崩溃验证器恢复后从 Checkpoint 同步

保证:
✅ Intent 不会丢失
✅ 资产锁定状态不变
✅ 系统自动恢复

════════════════════════════════════════════════════════════════════════
 场景 2: 共识期间网络分区
════════════════════════════════════════════════════════════════════════

时间线:
T0:    IntentBatch 提交共识
T100:  网络分区 ⚠️
       Partition A: 60% 验证器
       Partition B: 40% 验证器

处理:
1. Partition A 满足 2f+1，继续共识
2. Partition B 无法达成共识，等待
3. 网络恢复后，Partition B 同步

保证:
✅ 不会出现双花
✅ 不会出现不一致
✅ 最终一致性

════════════════════════════════════════════════════════════════════════
 场景 3: 撮合引擎错误
════════════════════════════════════════════════════════════════════════

可能错误:
- 计算溢出
- 状态不一致
- Bug 导致错误撮合

处理:
1. 所有撮合操作在 Move VM 中执行
2. Move 类型系统保证安全
3. 任何错误导致交易回滚

保证:
✅ 资产不会丢失
✅ 订单簿状态一致
✅ 自动回滚保护

════════════════════════════════════════════════════════════════════════
 场景 4: Intent 过期未处理
════════════════════════════════════════════════════════════════════════

时间线:
T0:      用户创建 Intent (expiry = T0 + 1h)
T1h:     Intent 过期，未被撮合

处理:
1. 系统定期扫描过期 Intent
2. 自动取消并解锁资产
3. 发送事件通知用户

代码:
```rust
public entry fun cleanup_expired_intents(
    pool: &mut IntentPool,
    clock: &Clock,
) {
    let now = clock::timestamp_ms(clock);
    let expired = pool.get_expired(now);

    for intent in expired {
        // 从池中移除
        pool.remove(intent.id);

        // 解锁资产
        unlock_assets(intent);

        // 发送事件
        event::emit(IntentExpiredEvent { ... });
    }
}
```

保证:
✅ 资产不会永久锁定
✅ 用户收到通知
✅ 自动清理
```

### 9.2 安全保障

```rust
/// 安全检查清单
mod security {

    /// 1. 防重放攻击
    /// 每个 IntentBox 维护 nonce
    fn check_nonce(intent_box: &IntentBox, expected: u64) {
        assert!(intent_box.nonce == expected, E_INVALID_NONCE);
    }

    /// 2. 防止恶意意图
    fn validate_intent(intent: &OrderIntent) {
        // 价格合理性
        assert!(intent.price > 0, E_INVALID_PRICE);
        assert!(intent.price < MAX_PRICE, E_PRICE_TOO_HIGH);

        // 数量合理性
        assert!(intent.quantity > MIN_QUANTITY, E_QTY_TOO_SMALL);
        assert!(intent.quantity < MAX_QUANTITY, E_QTY_TOO_LARGE);

        // 过期时间合理性
        if let Some(expiry) = intent.expiry {
            assert!(expiry > current_time(), E_ALREADY_EXPIRED);
            assert!(expiry < current_time() + MAX_EXPIRY, E_EXPIRY_TOO_LONG);
        }
    }

    /// 3. 权限检查
    fn check_permissions(ctx: &TxContext, intent: &OrderIntent) {
        // 只有创建者可以取消
        assert!(
            tx_context::sender(ctx) == intent.creator,
            E_NOT_CREATOR
        );
    }

    /// 4. 余额一致性
    fun verify_balance_invariant(intent_box: &IntentBox) {
        // 锁定资产 = 活跃意图的总锁定量
        let expected_locked = calculate_total_locked(&intent_box.active_intents);
        assert!(intent_box.locked_assets.total() == expected_locked, E_BALANCE_MISMATCH);
    }

    /// 5. 订单簿一致性
    fun verify_orderbook_invariant(shard: &OrderbookShard) {
        // 所有订单的锁定资产都有效
        for order in shard.all_orders() {
            let intent_box = get_intent_box(order.intent_box_id);
            assert!(intent_box.has_locked(order.id), E_MISSING_LOCK);
        }
    }
}
```

### 9.3 审计日志

```rust
/// 关键事件记录
mod audit {

    /// 意图创建事件
    struct IntentCreatedEvent has copy, drop {
        intent_id: ID,
        creator: address,
        market: MarketId,
        side: Side,
        price: u64,
        quantity: u64,
        timestamp: u64,
    }

    /// 意图取消事件
    struct IntentCancelledEvent has copy, drop {
        intent_id: ID,
        reason: CancelReason,
        refunded_amount: u64,
        timestamp: u64,
    }

    /// 成交事件 (详细版)
    struct TradeEvent has copy, drop {
        trade_id: ID,
        market: MarketId,
        maker_order_id: ID,
        taker_intent_id: ID,
        maker: address,
        taker: address,
        side: Side,          // Taker 方向
        price: u64,
        quantity: u64,
        maker_fee: u64,
        taker_fee: u64,
        timestamp: u64,
    }

    /// 结算事件
    struct SettlementCompletedEvent has copy, drop {
        trade_id: ID,
        maker_received: AssetTransfer,
        taker_received: AssetTransfer,
        timestamp: u64,
    }

    /// 异常事件
    struct AnomalyEvent has copy, drop {
        event_type: AnomalyType,
        description: String,
        affected_entities: vector<ID>,
        timestamp: u64,
    }
}
```

---

## 10. 实现路线图

### 10.1 阶段规划

```
实现路线图:

════════════════════════════════════════════════════════════════════════
 Phase 1: 核心框架 (4-6 周)
════════════════════════════════════════════════════════════════════════

Week 1-2: 对象模型
├─ [ ] Balance 模块
├─ [ ] IntentBox 模块
├─ [ ] OrderIntent 模块
└─ [ ] 基础测试

Week 3-4: Intent 流程
├─ [ ] place_order (Fast Path)
├─ [ ] cancel_intent
├─ [ ] IntentPool 基础实现
└─ [ ] 集成测试

Week 5-6: 共识集成
├─ [ ] 共识参数调优
├─ [ ] 批处理机制
├─ [ ] 共识排序集成
└─ [ ] 端到端测试

════════════════════════════════════════════════════════════════════════
 Phase 2: 撮合引擎 (4-6 周)
════════════════════════════════════════════════════════════════════════

Week 7-8: 订单簿
├─ [ ] CritBitTree 实现
├─ [ ] OrderbookShard 模块
├─ [ ] 分片路由
└─ [ ] 单元测试

Week 9-10: 撮合逻辑
├─ [ ] Limit Order 撮合
├─ [ ] Market Order 撮合
├─ [ ] IOC/FOK/PostOnly
└─ [ ] 撮合测试

Week 11-12: 结算
├─ [ ] Settlement 模块
├─ [ ] 并行结算优化
├─ [ ] 事件系统
└─ [ ] 集成测试

════════════════════════════════════════════════════════════════════════
 Phase 3: 优化与测试 (4 周)
════════════════════════════════════════════════════════════════════════

Week 13-14: 性能优化
├─ [ ] 批处理优化
├─ [ ] 分片重平衡
├─ [ ] 流水线处理
└─ [ ] 性能基准测试

Week 15-16: 测试与文档
├─ [ ] 压力测试
├─ [ ] 故障注入测试
├─ [ ] 安全审计
└─ [ ] 技术文档

════════════════════════════════════════════════════════════════════════
 Phase 4: 生产准备 (2-4 周)
════════════════════════════════════════════════════════════════════════

Week 17-18: 部署准备
├─ [ ] 配置管理
├─ [ ] 监控仪表盘
├─ [ ] 告警规则
└─ [ ] 运维文档

Week 19-20: 上线
├─ [ ] Testnet 部署
├─ [ ] 灰度发布
├─ [ ] 性能监控
└─ [ ] 问题修复
```

### 10.2 关键里程碑

```
关键里程碑:

┌─────────────────────────────────────────────────────────────────────┐
│ M1: Intent MVP (Week 6)                                             │
│ ────────────────────────                                            │
│ ✓ Fast Path 下单 (~200ms 确认)                                      │
│ ✓ Intent 取消                                                       │
│ ✓ 基础共识集成                                                      │
│ 验收标准: Intent 创建时延 < 300ms                                   │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│ M2: Matching Engine (Week 12)                                       │
│ ────────────────────────────                                        │
│ ✓ 完整撮合逻辑                                                      │
│ ✓ 订单簿分片                                                        │
│ ✓ 结算系统                                                          │
│ 验收标准: 端到端成交时延 < 1000ms                                   │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│ M3: Performance Target (Week 16)                                    │
│ ─────────────────────────────                                       │
│ ✓ 吞吐量 > 10,000 orders/s                                          │
│ ✓ P99 时延 < 1500ms                                                 │
│ ✓ 故障恢复 < 30s                                                    │
│ 验收标准: 压力测试通过                                              │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│ M4: Production Ready (Week 20)                                      │
│ ──────────────────────────                                          │
│ ✓ 安全审计通过                                                      │
│ ✓ Testnet 稳定运行 1 周                                             │
│ ✓ 完整监控和告警                                                    │
│ 验收标准: 可上线生产环境                                            │
└─────────────────────────────────────────────────────────────────────┘
```

### 10.3 技术选型

```
技术选型:

┌─────────────────────────────────────────────────────────────────────┐
│ 组件              │ 选择              │ 理由                        │
├───────────────────┼───────────────────┼─────────────────────────────┤
│ 共识协议          │ Mysticeti         │ 复用 Sui 实现，成熟可靠     │
│ 虚拟机            │ Move VM           │ 安全，类型系统强            │
│ 存储              │ RocksDB           │ 复用 Sui 存储层             │
│ 网络层            │ Sui 网络栈        │ 已优化的 P2P 网络           │
│ 序列化            │ BCS               │ 紧凑，性能好                │
│ 订单簿数据结构    │ CritBitTree       │ O(log N) 操作，常数因子小   │
│ 分片策略          │ 价格区间分片      │ 热点隔离，支持并行          │
│ 批处理            │ 时间+数量触发     │ 平衡时延和吞吐量            │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 11. 总结

### 11.1 设计亮点

```
核心创新:

1. Intent-Centric 架构
   ┌────────────────────────────────────────┐
   │ 传统: User → Orderbook (Shared)        │
   │ 本方案: User → Intent (Owned) → Batch  │
   │                                        │
   │ 收益: 用户操作走 Fast Path             │
   │      10x 更快的确认时间                │
   └────────────────────────────────────────┘

2. 批量处理分摊共识成本
   ┌────────────────────────────────────────┐
   │ 传统: 每订单单独共识                    │
   │ 本方案: 批量收集 → 一次共识             │
   │                                        │
   │ 收益: 共识成本 ÷ 批次大小               │
   │      5x 更高的吞吐量                   │
   └────────────────────────────────────────┘

3. 订单簿分片
   ┌────────────────────────────────────────┐
   │ 传统: 单一订单簿全局锁                  │
   │ 本方案: 价格区间分片                    │
   │                                        │
   │ 收益: 减少竞争，支持并行                │
   │      线性扩展撮合能力                  │
   └────────────────────────────────────────┘

4. 结算并行化
   ┌────────────────────────────────────────┐
   │ 传统: 串行结算                          │
   │ 本方案: IntentBox (Owned) 并行结算      │
   │                                        │
   │ 收益: 结算不是瓶颈                      │
   │      无额外时延                        │
   └────────────────────────────────────────┘
```

### 11.2 性能对比

```
最终性能对比:

┌─────────────────────────────────────────────────────────────────────┐
│                          │ 本方案      │ DeepBook   │ 中心化交易所 │
├──────────────────────────┼─────────────┼────────────┼──────────────┤
│ 用户确认时延             │ 200ms       │ 2000ms     │ <10ms        │
│ 成交时延                 │ 750ms       │ 2000ms     │ <50ms        │
│ 最终确认时延             │ 1150ms      │ 2500ms     │ N/A          │
│ 吞吐量                   │ 10,000/s    │ 2,000/s    │ 100,000+/s   │
│ 去中心化程度             │ 高          │ 高         │ 低           │
│ 抗审查性                 │ 高          │ 高         │ 低           │
└─────────────────────────────────────────────────────────────────────┘

定位:
  ✅ 比传统 DEX 快 10x (用户体验)
  ✅ 保持完全去中心化
  ✅ 在延迟和去中心化之间取得最佳平衡
```

### 11.3 适用场景

```
适用场景:

✅ 适合:
   - 高频交易 DEX
   - 永续合约交易所
   - 期权市场
   - 现货交易所
   - 需要链上订单簿的 DeFi

⚠️ 需要评估:
   - 超低延迟场景 (<100ms)
   - 极端高频做市

❌ 不适合:
   - 微秒级交易 (考虑 Sequencer)
   - 完全中心化可接受的场景
```

---

## 附录 A: 代码示例

### A.1 Intent 创建完整代码

```rust
module orderbook::intent {
    use sui::object::{Self, UID, ID};
    use sui::tx_context::{Self, TxContext};
    use sui::transfer;
    use sui::coin::{Self, Coin};
    use sui::event;

    // ========== 错误码 ==========
    const E_INSUFFICIENT_BALANCE: u64 = 1;
    const E_INVALID_PRICE: u64 = 2;
    const E_INVALID_QUANTITY: u64 = 3;

    // ========== 常量 ==========
    const MIN_QUANTITY: u64 = 1000;      // 最小数量
    const MAX_PRICE: u64 = 1_000_000_000; // 最大价格

    // ========== 事件 ==========
    struct IntentCreatedEvent has copy, drop {
        intent_id: ID,
        creator: address,
        market: ID,
        side: u8,
        price: u64,
        quantity: u64,
        timestamp: u64,
    }

    // ========== 主函数 ==========

    /// 创建订单意图 (Fast Path)
    /// 锁定资产并创建 Intent
    public entry fun place_limit_order<BaseAsset, QuoteAsset>(
        market_id: ID,
        side: u8,  // 0 = Buy, 1 = Sell
        price: u64,
        quantity: u64,
        intent_box: &mut IntentBox,
        payment: Coin<QuoteAsset>,  // 买单用 Quote，卖单用 Base
        ctx: &mut TxContext,
    ) {
        // 1. 验证参数
        assert!(price > 0 && price < MAX_PRICE, E_INVALID_PRICE);
        assert!(quantity >= MIN_QUANTITY, E_INVALID_QUANTITY);

        // 2. 计算锁定金额
        let lock_amount = if (side == 0) {
            // Buy: 锁定 quote asset
            (price as u128) * (quantity as u128) / 1_000_000_000
        } else {
            // Sell: 锁定 base asset
            quantity
        };

        // 3. 验证支付充足
        assert!(coin::value(&payment) >= (lock_amount as u64), E_INSUFFICIENT_BALANCE);

        // 4. 锁定资产到 IntentBox
        let locked = coin::split(&mut payment, lock_amount as u64, ctx);
        intent_box::deposit(intent_box, locked);

        // 5. 退还多余
        if (coin::value(&payment) > 0) {
            transfer::public_transfer(payment, tx_context::sender(ctx));
        } else {
            coin::destroy_zero(payment);
        };

        // 6. 创建 Intent
        let intent = OrderIntent {
            id: object::new(ctx),
            creator: tx_context::sender(ctx),
            intent_box_id: object::id(intent_box),
            market: market_id,
            side,
            price,
            quantity,
            order_type: 0, // Limit
            timestamp: tx_context::epoch_timestamp_ms(ctx),
            expiry: option::none(),
            nonce: intent_box.nonce,
        };

        let intent_id = object::id(&intent);

        // 7. 记录到 IntentBox
        vector::push_back(&mut intent_box.active_intents, intent_id);
        intent_box.nonce = intent_box.nonce + 1;

        // 8. 发出事件
        event::emit(IntentCreatedEvent {
            intent_id,
            creator: tx_context::sender(ctx),
            market: market_id,
            side,
            price,
            quantity,
            timestamp: tx_context::epoch_timestamp_ms(ctx),
        });

        // 9. 返回 Intent (Owned Object)
        // 后续用户需要调用 submit_intent 提交到 Pool
        transfer::transfer(intent, tx_context::sender(ctx));
    }
}
```

### A.2 配置参考

```toml
# orderbook_config.toml

[consensus]
# 共识参数
leader_timeout_ms = 150
min_round_delay_ms = 30
max_block_transactions = 500

[checkpoint]
# Checkpoint 参数
min_interval_ms = 100
max_interval_ms = 500

[matching]
# 撮合参数
batch_interval_ms = 100
max_batch_size = 1000
shard_count = 8

[settlement]
# 结算参数
parallel_settlements = 100
timeout_ms = 5000

[performance]
# 性能目标
target_intent_latency_ms = 200
target_match_latency_ms = 750
target_throughput = 10000
```

---

**文档完成!**

> 这份设计文档详细描述了如何基于 Sui 架构构建高性能 Orderbook Layer1。
> 核心创新在于 Intent-Centric 设计，将用户操作分离为 Fast Path 意图创建和 Consensus Path 批量撮合，
> 实现 10x 更快的用户响应和 5x 更高的吞吐量。

---

*作者: Claude | 日期: 2024*
