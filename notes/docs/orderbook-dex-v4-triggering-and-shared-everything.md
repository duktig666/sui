# OrderBook DEX V4 架构补充：触发机制与 Shared Everything 分析

**版本**: v4.1
**日期**: 2025-12-24
**状态**: 架构补充文档

---

## 目录

1. [撮合与结算触发机制](#1-撮合与结算触发机制)
2. [Shared Everything 架构分析](#2-shared-everything-架构分析)
3. [V4 改进方案对比](#3-v4-改进方案对比)
4. [推荐方案](#4-推荐方案)

---

## 1. 撮合与结算触发机制

### 1.1 问题陈述

在 V4 三阶段架构中：
- **阶段一（预锁定）**：用户发送交易触发 ✓ 明确
- **阶段二（撮合）**：如何触发？ ← 需要明确
- **阶段三（结算）**：如何触发？ ← 需要明确

### 1.2 Sui 共识输出处理机制

Sui 的共识输出通过 `ConsensusHandler` 处理：

```rust
// 文件: crates/sui-core/src/consensus_handler.rs

impl<C> ConsensusHandler<C> {
    /// 处理共识提交 - 这是共识输出的入口点
    pub(crate) async fn handle_consensus_commit(
        &mut self,
        consensus_commit: impl ConsensusCommitAPI,
    ) {
        // 1. 等待背压释放
        self.backpressure_subscriber.await_no_backpressure().await;

        // 2. 处理共识输出中的交易
        // consensus_commit 包含了经过共识排序的交易列表

        // 3. 按序执行这些交易
        // ...
    }
}
```

**关键洞察**：Sui 的 `ConsensusHandler` 在每次共识提交后被自动调用，这是触发撮合的天然时机。

### 1.3 触发方案设计

#### 方案 A：共识输出直接触发（推荐）

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    共识输出触发撮合流程                                   │
└─────────────────────────────────────────────────────────────────────────┘

时间线:
T0                    T1                    T2                    T3
│                     │                     │                     │
▼                     ▼                     ▼                     ▼
[用户下单]             [共识提交]             [撮合执行]             [结算完成]
    │                     │                     │                     │
    │  PlaceOrder TX      │                     │                     │
    │  (Owned Object)     │                     │                     │
    ├─────────────────────>                     │                     │
    │                     │                     │                     │
    │     Order Created   │                     │                     │
    │     Event           │                     │                     │
    │        │            │                     │                     │
    │        ▼            │                     │                     │
    │  ┌──────────────┐   │                     │                     │
    │  │Order Collector│   │                     │                     │
    │  │(Off-chain)    │   │                     │                     │
    │  └──────┬───────┘   │                     │                     │
    │         │           │                     │                     │
    │         │ 订单列表   │                     │                     │
    │         ▼           │                     │                     │
    │  ┌──────────────────┴─────────────────┐   │                     │
    │  │      MatchBatch TX                 │   │                     │
    │  │      (Shared Object)               │   │                     │
    │  │                                    │   │                     │
    │  │  触发方式: 验证者/排序器 发起      │   │                     │
    │  └────────────────────────────────────┘   │                     │
    │                     │                     │                     │
    │                     │  Mysticeti 共识     │                     │
    │                     │  全局排序           │                     │
    │                     ▼                     │                     │
    │              ┌──────────────────────────────────────────────┐   │
    │              │          ConsensusHandler                    │   │
    │              │                                              │   │
    │              │  handle_consensus_commit() 被调用             │   │
    │              │                                              │   │
    │              │  1. 解析 MatchBatch TX                       │   │
    │              │  2. 提取排序后的订单列表                      │   │
    │              │  3. 调用撮合引擎                              │   │
    │              │  4. 生成撮合结果                              │   │
    │              └───────────────────────┬──────────────────────┘   │
    │                                      │                          │
    │                                      │ MatchResult              │
    │                                      ▼                          │
    │              ┌──────────────────────────────────────────────┐   │
    │              │          Settlement TX                       │   │
    │              │          (自动生成并执行)                      │   │
    │              │                                              │   │
    │              │  • 资金划转                                   │   │
    │              │  • 状态更新                                   │   │
    │              │  • 事件发送                                   │   │
    │              └──────────────────────────────────────────────┘   │
    │                                                                 │
    │                                                                 ▼
    │                                                      用户收到成交通知
```

#### 核心代码设计

```rust
/// DEX 扩展的共识处理器
pub struct DexConsensusHandler {
    /// 基础共识处理器
    base_handler: ConsensusHandler<CheckpointService>,

    /// 撮合引擎
    matching_engine: Arc<Mutex<MatchingEngine>>,

    /// 订单收集器
    order_collector: Arc<OrderCollector>,
}

impl DexConsensusHandler {
    /// 处理共识提交 - 扩展点
    pub async fn handle_consensus_commit(
        &mut self,
        consensus_commit: impl ConsensusCommitAPI,
    ) {
        // 1. 从共识输出中提取 DEX 相关交易
        let dex_txs = self.extract_dex_transactions(&consensus_commit);

        // 2. 分类处理
        for tx in dex_txs {
            match tx.kind {
                // 下单交易：收集到待撮合队列
                DexTxKind::PlaceOrder { order } => {
                    self.order_collector.add_order(order);
                }

                // 撤单交易：标记订单为已取消
                DexTxKind::CancelOrder { order_id } => {
                    self.order_collector.mark_cancelled(order_id);
                }

                // 撮合批次：执行撮合（由验证者发起）
                DexTxKind::MatchBatch { batch_id } => {
                    self.execute_matching(batch_id).await;
                }
            }
        }

        // 3. 检查是否需要发起新的撮合批次
        if self.should_trigger_matching() {
            self.submit_match_batch_tx().await;
        }
    }

    /// 执行撮合（阶段二 + 阶段三 原子执行）
    async fn execute_matching(&mut self, batch_id: u64) {
        // 1. 获取待撮合订单（按共识顺序）
        let orders = self.order_collector.get_pending_orders(batch_id);

        // 2. 执行撮合算法
        let match_results = {
            let mut engine = self.matching_engine.lock().await;
            engine.match_batch(orders)
        };

        // 3. 执行结算（原子操作）
        for result in &match_results {
            self.execute_settlement(result).await;
        }

        // 4. 发送事件通知
        self.emit_trade_events(&match_results);
    }

    /// 判断是否应该触发撮合
    fn should_trigger_matching(&self) -> bool {
        let pending_count = self.order_collector.pending_count();
        let last_match_time = self.order_collector.last_match_time();
        let now = SystemTime::now();

        // 条件 1: 累积足够订单
        if pending_count >= BATCH_SIZE_THRESHOLD {
            return true;
        }

        // 条件 2: 超过时间窗口
        if now.duration_since(last_match_time).unwrap() >= MATCH_INTERVAL {
            return pending_count > 0;
        }

        false
    }

    /// 提交撮合批次交易
    async fn submit_match_batch_tx(&self) {
        let batch_id = self.order_collector.next_batch_id();
        let order_ids = self.order_collector.get_pending_order_ids();

        let tx = DexTransaction::MatchBatch {
            batch_id,
            order_ids,
        };

        // 提交到共识
        self.consensus_adapter.submit(tx).await;
    }
}

// 配置参数
const BATCH_SIZE_THRESHOLD: usize = 100;  // 每批次最大订单数
const MATCH_INTERVAL: Duration = Duration::from_millis(100);  // 最大等待时间
```

#### 方案 B：Leader 轮次自动触发

```rust
/// 基于共识轮次的自动触发
impl DexConsensusHandler {
    pub async fn handle_consensus_commit(
        &mut self,
        consensus_commit: impl ConsensusCommitAPI,
    ) {
        let round = consensus_commit.leader_round();

        // 每个共识轮次结束时自动触发撮合
        // 无需额外的 MatchBatch 交易

        // 1. 收集本轮次内的所有订单
        let orders = self.collect_orders_from_commit(&consensus_commit);

        // 2. 立即执行撮合
        if !orders.is_empty() {
            let results = self.matching_engine.lock().await.match_batch(orders);

            // 3. 执行结算
            for result in results {
                self.execute_settlement(&result).await;
            }
        }
    }
}
```

**方案对比**：

| 方案 | 优点 | 缺点 | 适用场景 |
|-----|------|------|---------|
| **A: 显式触发** | 可控、灵活、可审计 | 需要额外交易 | 生产环境 ✓ |
| **B: 自动触发** | 简单、低开销 | 不够灵活 | 原型验证 |

### 1.4 撮合与结算的原子性

**关键设计**：撮合（阶段二）和结算（阶段三）在同一个共识处理周期内原子执行。

```rust
/// 撮合结算一体化执行
impl DexConsensusHandler {
    /// 阶段二 + 阶段三 原子执行
    async fn execute_matching_and_settlement(&mut self, orders: Vec<Order>) {
        // 开始数据库事务（隐式，通过 Sui 的对象模型保证）

        // 阶段二：撮合
        let results = self.matching_engine.lock().await.match_batch(orders);

        // 阶段三：结算（与撮合在同一执行上下文）
        for result in &results {
            // 更新买方余额
            let buyer = self.get_user_state(&result.buyer);
            buyer.frozen -= result.frozen_consumed;
            buyer.balances[result.base_token] += result.base_received;

            // 更新卖方余额
            let seller = self.get_user_state(&result.seller);
            seller.frozen -= result.frozen_consumed;
            seller.balances[result.quote_token] += result.quote_received;

            // 更新订单状态
            let order = self.get_order(&result.order_id);
            order.filled_quantity += result.filled_quantity;
            order.status = result.new_status;
        }

        // 事务自动提交（通过 Sui 的状态模型）
    }
}
```

---

## 2. Shared Everything 架构分析

### 2.1 什么是 Shared Everything

**Shared Everything** 架构指：将所有 DEX 状态（包括用户余额）都设计为 Shared Object，所有操作都通过共识排序后执行。

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Shared Everything 架构                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  所有状态都是 Shared Object:                                             │
│                                                                          │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐          │
│  │   OrderBook     │  │  UserBalances   │  │   TradeHistory  │          │
│  │   (Shared)      │  │   (Shared)      │  │   (Shared)      │          │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘          │
│           │                    │                    │                   │
│           └────────────────────┼────────────────────┘                   │
│                                │                                        │
│                                ▼                                        │
│                    ┌─────────────────────┐                              │
│                    │   Mysticeti 共识     │                              │
│                    │   全局排序           │                              │
│                    └──────────┬──────────┘                              │
│                               │                                         │
│                               ▼                                         │
│                    ┌─────────────────────┐                              │
│                    │   统一执行           │                              │
│                    │   (顺序处理)         │                              │
│                    └─────────────────────┘                              │
│                                                                          │
│  所有操作都经过共识:                                                      │
│  • PlaceOrder → 共识 → 验证余额 + 冻结 + 撮合 + 结算                     │
│  • CancelOrder → 共识 → 验证权限 + 解冻                                  │
│  • Deposit/Withdraw → 共识 → 更新余额                                    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Shared Everything 的核心特点

#### 2.2.1 数据模型

```rust
/// Shared Everything 数据模型
module dex_shared::state {
    /// 全局 DEX 状态 - 单一 Shared Object
    struct DexState has key {
        id: UID,

        /// 所有交易对的订单簿
        orderbooks: Table<TradingPair, OrderBook>,

        /// 所有用户的余额
        /// 注意：这里不是每用户一个对象，而是统一管理
        balances: Table<address, UserBalance>,

        /// 成交历史
        trades: vector<Trade>,

        /// 全局订单计数器
        next_order_id: u64,

        /// 全局交易计数器
        next_trade_id: u64,
    }

    /// 订单簿
    struct OrderBook has store {
        bids: LinkedTable<OrderKey, Order>,
        asks: LinkedTable<OrderKey, Order>,
        best_bid: Option<u64>,
        best_ask: Option<u64>,
    }

    /// 用户余额
    struct UserBalance has store {
        available: Table<TypeName, u64>,
        frozen: Table<TypeName, u64>,
    }
}
```

#### 2.2.2 执行流程

```rust
/// Shared Everything 下单流程
public entry fun place_order(
    state: &mut DexState,  // 全局状态，Shared Object
    pair: TradingPair,
    side: u8,
    price: u64,
    quantity: u64,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let trader = tx_context::sender(ctx);

    // 1. 验证并冻结余额（直接操作 state.balances）
    let balance = table::borrow_mut(&mut state.balances, trader);
    let freeze_amount = calculate_freeze_amount(side, price, quantity);

    assert!(get_available(balance, get_freeze_token(side, &pair)) >= freeze_amount,
            E_INSUFFICIENT_BALANCE);

    freeze(balance, get_freeze_token(side, &pair), freeze_amount);

    // 2. 创建订单
    let order_id = state.next_order_id;
    state.next_order_id = state.next_order_id + 1;

    let order = Order {
        id: order_id,
        trader,
        pair,
        side,
        price,
        quantity,
        remaining: quantity,
        timestamp: clock::timestamp_ms(clock),
    };

    // 3. 立即尝试撮合（在同一交易中）
    let orderbook = table::borrow_mut(&mut state.orderbooks, pair);
    let fills = match_order(orderbook, &order);

    // 4. 执行结算（在同一交易中）
    for fill in fills {
        settle_fill(state, &fill);
    }

    // 5. 如果有剩余，加入订单簿
    if order.remaining > 0 && is_limit_order(side) {
        add_to_orderbook(orderbook, order);
    }

    // 6. 发送事件
    emit_order_placed_event(order_id, trader, pair, side, price, quantity);
    for fill in fills {
        emit_trade_event(&fill);
    }
}
```

### 2.3 Shared Everything vs V4 三阶段 对比

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    架构对比                                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  V4 三阶段架构:                                                          │
│  ┌───────────────────┐  ┌───────────────────┐  ┌───────────────────┐   │
│  │ 阶段一: 预锁定     │  │ 阶段二: 撮合      │  │ 阶段三: 结算      │   │
│  │ Owned Object      │  │ 共识后执行        │  │ Shared Object     │   │
│  │ 完全并发          │  │ 全局排序          │  │ 原子执行          │   │
│  │ ~200-400ms        │  │ ~400-500ms        │  │ 与撮合一起        │   │
│  └───────────────────┘  └───────────────────┘  └───────────────────┘   │
│                                                                          │
│  Shared Everything:                                                      │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │              一体化执行（全部在共识后）                             │  │
│  │                                                                    │  │
│  │  验证余额 → 冻结 → 撮合 → 结算 → 事件                              │  │
│  │                                                                    │  │
│  │  Shared Object                                                     │  │
│  │  ~400-500ms                                                        │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.4 详细对比分析

| 维度 | V4 三阶段 | Shared Everything |
|-----|----------|-------------------|
| **下单延迟** | ~200-400ms (预锁定) | ~400-500ms (全流程) |
| **成交延迟** | ~600-900ms | ~400-500ms |
| **下单吞吐** | 100K+ TPS (并发) | ~10K TPS (共识限制) |
| **撮合吞吐** | ~10K TPS | ~10K TPS |
| **状态模型** | 复杂（Owned + Shared） | **简单（全 Shared）** ✓ |
| **实现复杂度** | 较高（三阶段协调） | **较低** ✓ |
| **回滚风险** | 无 | 无 |
| **一致性保证** | 需要协调 | **天然一致** ✓ |
| **余额验证时机** | 预锁定时 | 共识后 |
| **重复下单检测** | 需要额外机制 | **共识天然去重** ✓ |

### 2.5 Shared Everything 的优势

```
1. 简化的状态模型
   ┌─────────────────────────────────────────────────────────────────┐
   │  V4 需要管理:                                                   │
   │  • UserBalance (Owned) × N 用户                                │
   │  • Order (Owned) × M 订单                                      │
   │  • OrderBook (Shared)                                          │
   │  • SettlementQueue (Shared)                                    │
   │                                                                 │
   │  Shared Everything:                                             │
   │  • DexState (Shared) × 1                                       │
   │    └── 包含所有状态                                            │
   └─────────────────────────────────────────────────────────────────┘

2. 原子性保证
   ┌─────────────────────────────────────────────────────────────────┐
   │  V4: 预锁定成功 ≠ 撮合成功                                      │
   │       可能出现: 订单创建了但永远不会被撮合                       │
   │                                                                 │
   │  Shared Everything:                                             │
   │       验证 + 冻结 + 撮合 + 结算 = 原子操作                       │
   │       要么全成功，要么全失败                                     │
   └─────────────────────────────────────────────────────────────────┘

3. 即时撮合
   ┌─────────────────────────────────────────────────────────────────┐
   │  V4: 下单后需要等待下一个撮合批次                               │
   │                                                                 │
   │  Shared Everything: 下单交易内直接撮合                          │
   │       如果有匹配的对手方，立即成交                               │
   └─────────────────────────────────────────────────────────────────┘

4. 无协调开销
   ┌─────────────────────────────────────────────────────────────────┐
   │  V4: 需要订单收集器、批次管理、触发机制                         │
   │                                                                 │
   │  Shared Everything: 共识自然提供顺序，无需额外协调              │
   └─────────────────────────────────────────────────────────────────┘
```

### 2.6 Shared Everything 的劣势

```
1. 下单吞吐量受限
   ┌─────────────────────────────────────────────────────────────────┐
   │  所有下单都经过共识                                             │
   │  吞吐量 ≈ 共识吞吐量 ≈ 10K TPS                                  │
   │                                                                 │
   │  V4 下单: 100K+ TPS (Owned Object 并发)                        │
   └─────────────────────────────────────────────────────────────────┘

2. 下单延迟增加
   ┌─────────────────────────────────────────────────────────────────┐
   │  Shared Everything: 下单必须等待共识 ~400-500ms                 │
   │  V4: 预锁定只需 ~200-400ms                                     │
   └─────────────────────────────────────────────────────────────────┘

3. 单点状态
   ┌─────────────────────────────────────────────────────────────────┐
   │  所有交易对共享一个 DexState                                    │
   │  • 难以水平扩展                                                 │
   │  • 热点问题                                                     │
   │                                                                 │
   │  缓解: 按交易对分片                                             │
   └─────────────────────────────────────────────────────────────────┘
```

---

## 3. V4 改进方案对比

### 3.1 三种架构方案

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    三种架构方案对比                                       │
└─────────────────────────────────────────────────────────────────────────┘

方案 A: V4 三阶段（原始）
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│ 预锁定       │ → │ 撮合        │ → │ 结算        │
│ (Owned)     │    │ (共识后)    │    │ (Shared)    │
│ 100K TPS    │    │ 10K TPS     │    │ 10K TPS     │
│ 200-400ms   │    │ 400-500ms   │    │ 原子        │
└─────────────┘    └─────────────┘    └─────────────┘

方案 B: Shared Everything
┌─────────────────────────────────────────────────────────────────────────┐
│           一体化执行（验证 → 冻结 → 撮合 → 结算）                          │
│           (全部 Shared)                                                  │
│           10K TPS | 400-500ms | 简单                                     │
└─────────────────────────────────────────────────────────────────────────┘

方案 C: 混合架构（V4 改进版）
┌─────────────┐    ┌─────────────────────────────────────────────────────┐
│ 预锁定       │ → │ 一体化撮合结算                                        │
│ (Owned)     │    │ (Shared Everything for matching & settlement)       │
│ 100K TPS    │    │ 10K TPS | 原子 | 简单                                │
│ 200-400ms   │    │                                                      │
└─────────────┘    └─────────────────────────────────────────────────────┘
```

### 3.2 方案 C 详解：混合架构（推荐）

**核心思想**：保留 V4 的预锁定阶段（高并发），但将撮合和结算合并为 Shared Everything 模式。

```rust
/// 混合架构数据模型
module dex_hybrid::types {
    /// 用户余额 - 保持 Owned Object（阶段一使用）
    struct UserBalance has key, store {
        id: UID,
        owner: address,
        available: Table<TypeName, Coin>,
        frozen: Table<TypeName, u64>,
    }

    /// DEX 撮合状态 - Shared Object（阶段二三使用）
    struct DexMatchingState has key {
        id: UID,

        /// 订单簿（按交易对分片）
        orderbooks: Table<TradingPair, OrderBook>,

        /// 待撮合订单队列（来自阶段一）
        pending_orders: Table<TradingPair, vector<PendingOrder>>,

        /// 成交记录
        trades: vector<Trade>,

        /// 统计信息
        stats: DexStats,
    }

    /// 待撮合订单（阶段一创建，阶段二消费）
    struct PendingOrder has store, drop {
        order_id: u64,
        trader: address,
        pair: TradingPair,
        side: OrderSide,
        price: u64,
        quantity: u64,
        frozen_amount: u64,
        created_at: u64,
    }
}
```

#### 混合架构执行流程

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    混合架构执行流程                                       │
└─────────────────────────────────────────────────────────────────────────┘

阶段一（保持不变）: 预锁定
┌─────────────────────────────────────────────────────────────────────────┐
│  用户发起 PlaceOrder 交易                                                │
│  ├── 操作 UserBalance (Owned Object)                                    │
│  │   ├── 验证余额: available >= freeze_amount                          │
│  │   ├── 冻结资金: available -= X, frozen += X                         │
│  │   └── 版本递增: version += 1                                        │
│  │                                                                      │
│  └── 发送订单创建事件                                                    │
│      OrderCreatedEvent { order_id, trader, pair, side, price, qty }    │
│                                                                          │
│  特性:                                                                   │
│  • Owned Object，完全并发                                                │
│  • 无需共识排序                                                          │
│  • 延迟: ~200-400ms                                                      │
│  • 吞吐: 100K+ TPS                                                       │
└──────────────────────────────────────────┬──────────────────────────────┘
                                           │
                                           │ 订单事件
                                           ▼
阶段二三（合并优化）: 撮合 + 结算 = Shared Everything
┌─────────────────────────────────────────────────────────────────────────┐
│  系统发起 MatchAndSettle 交易                                            │
│  ├── 操作 DexMatchingState (Shared Object)                              │
│  │                                                                       │
│  │  1. 收集待撮合订单                                                    │
│  │     从 pending_orders 队列获取（按共识顺序）                          │
│  │                                                                       │
│  │  2. 执行撮合算法                                                      │
│  │     ├── 遍历订单                                                      │
│  │     ├── 匹配订单簿                                                    │
│  │     ├── 生成成交记录                                                  │
│  │     └── 更新订单簿                                                    │
│  │                                                                       │
│  │  3. 执行结算（原子）                                                   │
│  │     ├── 更新 UserBalance（通过动态字段或引用）                        │
│  │     ├── 划转资金                                                      │
│  │     └── 记录成交                                                      │
│  │                                                                       │
│  └── 发送成交事件                                                        │
│      TradeEvent { trade_id, maker, taker, price, qty }                  │
│                                                                          │
│  特性:                                                                   │
│  • Shared Object，需要共识                                               │
│  • 撮合 + 结算原子执行                                                   │
│  • 延迟: ~400-500ms                                                      │
│  • 吞吐: ~10K TPS                                                        │
└─────────────────────────────────────────────────────────────────────────┘
```

#### 混合架构代码示例

```rust
module dex_hybrid::matching {
    /// 撮合并结算（阶段二三合并）
    ///
    /// 触发方式: 由验证者周期性调用，或达到阈值时调用
    public entry fun match_and_settle(
        state: &mut DexMatchingState,
        user_balances: &mut vector<UserBalance>,  // 动态传入相关用户余额
        pair: TradingPair,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        // 1. 获取待撮合订单
        let pending = table::borrow_mut(&mut state.pending_orders, pair);
        let orders = vector::drain(pending);

        if (vector::is_empty(&orders)) {
            return
        };

        // 2. 获取订单簿
        let orderbook = table::borrow_mut(&mut state.orderbooks, pair);

        // 3. 逐个撮合
        let len = vector::length(&orders);
        let mut i = 0;
        while (i < len) {
            let order = vector::borrow(&orders, i);

            // 尝试撮合
            let fills = match_single_order(orderbook, order);

            // 执行结算
            settle_fills(user_balances, &fills);

            // 如果有剩余且是限价单，加入订单簿
            let remaining = order.quantity - sum_fill_quantity(&fills);
            if (remaining > 0 && order.order_type == LIMIT_ORDER) {
                add_to_orderbook(orderbook, create_resting_order(order, remaining));
            }

            // 记录成交
            record_trades(&mut state.trades, &fills);

            i = i + 1;
        };

        // 4. 发送事件
        emit_batch_matched_event(pair, len, clock::timestamp_ms(clock));
    }

    /// 单订单撮合
    fun match_single_order(orderbook: &mut OrderBook, order: &PendingOrder): vector<Fill> {
        let mut fills = vector::empty<Fill>();

        let opposite_book = if (order.side == BUY) {
            &mut orderbook.asks
        } else {
            &mut orderbook.bids
        };

        let mut remaining = order.quantity;

        // 价格-时间优先撮合
        while (remaining > 0 && !linked_table::is_empty(opposite_book)) {
            let best_key = linked_table::front(opposite_book);
            let best_order = linked_table::borrow(opposite_book, best_key);

            // 检查价格是否匹配
            if (!price_matches(order.side, order.price, best_order.price)) {
                break
            };

            // 计算成交数量
            let fill_qty = math::min(remaining, best_order.remaining);

            // 记录成交
            vector::push_back(&mut fills, Fill {
                maker_order_id: best_order.id,
                taker_order_id: order.order_id,
                maker: best_order.trader,
                taker: order.trader,
                price: best_order.price,  // 使用挂单价（maker 价格）
                quantity: fill_qty,
            });

            remaining = remaining - fill_qty;

            // 更新或移除挂单
            if (fill_qty == best_order.remaining) {
                linked_table::pop_front(opposite_book);
            } else {
                let order_mut = linked_table::borrow_mut(opposite_book, best_key);
                order_mut.remaining = order_mut.remaining - fill_qty;
            };
        };

        fills
    }

    /// 批量结算
    fun settle_fills(balances: &mut vector<UserBalance>, fills: &vector<Fill>) {
        let len = vector::length(fills);
        let mut i = 0;
        while (i < len) {
            let fill = vector::borrow(fills, i);

            // 找到买卖双方的余额对象
            let buyer_balance = find_balance(balances, fill.taker);
            let seller_balance = find_balance(balances, fill.maker);

            let trade_value = fill.price * fill.quantity;

            // 买方: 解冻 quote，获得 base
            buyer_balance.frozen[QUOTE] = buyer_balance.frozen[QUOTE] - trade_value;
            add_coin(&mut buyer_balance.available[BASE], fill.quantity);

            // 卖方: 解冻 base，获得 quote
            seller_balance.frozen[BASE] = seller_balance.frozen[BASE] - fill.quantity;
            add_coin(&mut seller_balance.available[QUOTE], trade_value);

            i = i + 1;
        };
    }
}
```

### 3.3 方案对比总结

| 维度 | A: V4 三阶段 | B: Shared Everything | C: 混合架构 |
|-----|-------------|---------------------|------------|
| **下单延迟** | ~200-400ms ✓ | ~400-500ms | ~200-400ms ✓ |
| **成交延迟** | ~600-900ms | ~400-500ms ✓ | ~600-900ms |
| **下单吞吐** | 100K+ TPS ✓ | ~10K TPS | 100K+ TPS ✓ |
| **撮合吞吐** | ~10K TPS | ~10K TPS | ~10K TPS |
| **状态模型** | 复杂 | 简单 ✓ | 中等 |
| **实现复杂度** | 高 | 低 ✓ | 中 |
| **原子性** | 需协调 | 天然 ✓ | 部分 |
| **扩展性** | 高 ✓ | 低 | 中 |

---

## 4. 推荐方案

### 4.1 场景选择指南

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    场景选择决策树                                         │
└─────────────────────────────────────────────────────────────────────────┘

                              开始
                                │
                                ▼
                    ┌───────────────────────┐
                    │  下单吞吐量需求 > 10K? │
                    └───────────┬───────────┘
                                │
                    ┌───────────┴───────────┐
                    │                       │
                    ▼                       ▼
                   是                      否
                    │                       │
                    ▼                       ▼
        ┌───────────────────┐    ┌───────────────────┐
        │ 实现复杂度敏感?    │    │ Shared Everything │
        │                   │    │ 方案 B            │
        └─────────┬─────────┘    │ 最简单            │
                  │              └───────────────────┘
        ┌─────────┴─────────┐
        │                   │
        ▼                   ▼
       是                  否
        │                   │
        ▼                   ▼
┌───────────────────┐  ┌───────────────────┐
│ 混合架构 (方案 C)  │  │ V4 三阶段 (方案 A) │
│ 平衡性能与复杂度  │  │ 最高性能          │
└───────────────────┘  └───────────────────┘
```

### 4.2 推荐：混合架构（方案 C）

**理由**：

1. **保留高吞吐下单**：阶段一使用 Owned Object，100K+ TPS
2. **简化撮合结算**：阶段二三合并为 Shared Everything，降低复杂度
3. **原子性保证**：撮合与结算在同一交易内完成
4. **易于实现**：比完整 V4 简单，比纯 Shared Everything 性能更好

### 4.3 实施建议

```
Phase 1: MVP (4-6 周)
├── 实现 Shared Everything 版本
│   ├── 单一 DexState Shared Object
│   ├── 同步下单 + 撮合 + 结算
│   └── 验证正确性
│
└── 目标: 验证业务逻辑，无性能目标

Phase 2: 性能优化 (4-6 周)
├── 升级为混合架构
│   ├── 分离预锁定阶段（Owned Object）
│   ├── 保持撮合结算一体化（Shared）
│   └── 实现触发机制
│
└── 目标: 下单 > 50K TPS，成交 < 1s

Phase 3: 扩展 (2-4 周)
├── 交易对分片
├── 高可用设计
└── 监控告警

总计: 10-16 周
```

---

## 附录

### A. 触发机制代码示例

```rust
/// 撮合触发器
pub struct MatchingTrigger {
    /// 待撮合订单计数
    pending_count: AtomicUsize,

    /// 上次撮合时间
    last_match_time: AtomicU64,

    /// 配置
    config: TriggerConfig,
}

pub struct TriggerConfig {
    /// 批次大小阈值
    pub batch_size: usize,

    /// 最大等待时间
    pub max_wait_ms: u64,

    /// 最小间隔
    pub min_interval_ms: u64,
}

impl MatchingTrigger {
    /// 检查是否应该触发撮合
    pub fn should_trigger(&self, now_ms: u64) -> bool {
        let pending = self.pending_count.load(Ordering::Relaxed);
        let last = self.last_match_time.load(Ordering::Relaxed);
        let elapsed = now_ms - last;

        // 条件 1: 达到批次大小
        if pending >= self.config.batch_size {
            return elapsed >= self.config.min_interval_ms;
        }

        // 条件 2: 超过最大等待时间
        if elapsed >= self.config.max_wait_ms && pending > 0 {
            return true;
        }

        false
    }

    /// 记录订单到达
    pub fn on_order_arrived(&self) {
        self.pending_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录撮合完成
    pub fn on_match_completed(&self, matched_count: usize, now_ms: u64) {
        self.pending_count.fetch_sub(matched_count, Ordering::Relaxed);
        self.last_match_time.store(now_ms, Ordering::Relaxed);
    }
}
```

### B. 性能基准测试框架

```rust
#[cfg(test)]
mod benchmarks {
    use super::*;

    /// 测试下单吞吐量
    #[tokio::test]
    async fn bench_place_order_throughput() {
        let mut dex = setup_dex().await;
        let num_orders = 100_000;

        let start = Instant::now();

        // 并发下单
        let handles: Vec<_> = (0..num_orders)
            .map(|i| {
                let dex = dex.clone();
                tokio::spawn(async move {
                    dex.place_order(create_random_order(i)).await
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        let elapsed = start.elapsed();
        let tps = num_orders as f64 / elapsed.as_secs_f64();

        println!("Place Order TPS: {:.0}", tps);
        assert!(tps > 50_000.0);
    }

    /// 测试撮合延迟
    #[tokio::test]
    async fn bench_matching_latency() {
        let mut dex = setup_dex().await;

        // 准备订单簿
        setup_orderbook(&mut dex, 1000).await;

        let latencies: Vec<Duration> = (0..1000)
            .map(|_| {
                let start = Instant::now();
                dex.match_single_order(create_market_order()).await;
                start.elapsed()
            })
            .collect();

        let avg_latency = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let p99_latency = percentile(&latencies, 0.99);

        println!("Matching Latency - Avg: {:?}, P99: {:?}", avg_latency, p99_latency);
    }
}
```

---

**文档状态**: ✅ 完成
**版本**: v4.1
**最后更新**: 2025-12-24
