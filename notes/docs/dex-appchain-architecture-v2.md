# DEX AppChain 架构设计文档 V2 - 低延迟优化版

**版本**: v2.0
**日期**: 2025-12-16
**作者**: Architecture Team
**状态**: Design Review
**目标延迟**: < 100ms (相比 V1 的 ~400ms)

---

## 🎯 V2 核心目标

**突破性改进**: 将端到端延迟从 400ms 降低到 **< 100ms**

**约束条件**:
- ✅ 必须使用现有 Mysticeti 共识 (~400ms 固定延迟)
- ✅ 保证最终一致性
- ✅ 架构层面优化为主

**关键创新**:
- 🚀 乐观执行 (Optimistic Execution)
- 🔄 双层状态机 (Two-tier State Machine)
- 📊 冲突检测与回滚
- ⚡ 快速路径 + 确认路径

---

## 📋 目录

1. [V2 vs V1 对比](#1-v2-vs-v1-对比)
2. [核心创新点](#2-核心创新点)
3. [V2 整体架构](#3-v2-整体架构)
4. [双层状态机设计](#4-双层状态机设计)
5. [乐观执行机制](#5-乐观执行机制)
6. [冲突检测与回滚](#6-冲突检测与回滚)
7. [数据流与时序](#7-数据流与时序)
8. [性能分析](#8-性能分析)
9. [风险与权衡](#9-风险与权衡)
10. [实现路线图](#10-实现路线图)

---

## 1. V2 vs V1 对比

### 1.1 延迟对比

| 指标 | V1 | V2 | 改进 |
|-----|----|----|------|
| **端到端延迟 P50** | ~400ms | **~50ms** | **8x** |
| **端到端延迟 P99** | ~600ms | **~100ms** | **6x** |
| **最终确认时间** | ~400ms | ~400ms | 无变化 |
| **吞吐量** | 1K TPS | **5K TPS** | **5x** |

### 1.2 架构对比

| 维度 | V1 | V2 |
|-----|----|----|
| **执行模式** | 同步等待共识 | 乐观执行 + 异步确认 |
| **状态机** | 单层状态 | 双层状态 (Pending + Committed) |
| **冲突处理** | 无需处理 | 主动检测 + 回滚 |
| **用户体验** | 等待 400ms | 等待 50ms (99%情况) |
| **复杂度** | 简单 | 中等 |

### 1.3 适用场景

**V1 适用**:
- 绝对一致性要求
- 低频交易
- 原型验证

**V2 适用**:
- 高频交易
- 用户体验优先
- 生产环境

---

## 2. 核心创新点

### 2.1 乐观执行 (Optimistic Execution)

**核心思想**: "先执行，后确认"

```
传统流程 (V1):
客户端 → 提交 → [等待共识 400ms] → 执行 → 返回

乐观流程 (V2):
客户端 → 提交 → [乐观执行 10ms] → 返回 "Pending"
           ↓
        [后台共识 400ms] → 最终确认
```

**收益**:
- 用户感知延迟: 400ms → 50ms
- 吞吐量提升: 1K TPS → 5K TPS

### 2.2 双层状态机

```
┌─────────────────────────────────────────┐
│         Pending State (乐观层)           │
│  - 快速执行                             │
│  - 乐观结果                             │
│  - 可能回滚                             │
└─────────────────────────────────────────┘
              ↓ (异步确认)
┌─────────────────────────────────────────┐
│       Committed State (共识层)           │
│  - 共识确认                             │
│  - 最终结果                             │
│  - 不可回滚                             │
└─────────────────────────────────────────┘
```

### 2.3 冲突检测

**无冲突交易** (95%+):
- 快速路径: 直接成功
- 延迟: ~10ms

**有冲突交易** (< 5%):
- 降级到慢速路径
- 延迟: ~400ms (回退到 V1)

### 2.4 MVCC (多版本并发控制)

```rust
pub struct VersionedState {
    // 每个版本的状态
    versions: BTreeMap<Version, State>,

    // 当前最新版本
    latest_version: Version,

    // Pending 交易映射
    pending_txs: HashMap<TxId, PendingExecution>,
}
```

---

## 3. V2 整体架构

### 3.1 五层架构

```
┌─────────────────────────────────────────────────────────────┐
│                        API Layer                             │
│         (快速响应 Pending，异步通知 Confirmed)                │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                   Optimistic Execution Layer                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  • 冲突检测                                          │   │
│  │  • 乐观执行                                          │   │
│  │  • Pending 状态管理                                  │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                   Consensus Layer (异步)                     │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  • 后台提交                                          │   │
│  │  • 批量确认                                          │   │
│  │  • 冲突解决                                          │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                   Confirmation Layer                         │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  • 验证乐观执行结果                                  │   │
│  │  • 触发回滚（如需要）                                │   │
│  │  • 更新 Committed State                              │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                   Storage Layer                              │
│  • Pending State (内存)                                      │
│  • Committed State (内存 + 可选持久化)                       │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 核心组件

```rust
/// V2 执行器（支持乐观执行）
pub struct OptimisticDexExecutor {
    /// Pending 状态（乐观执行结果）
    pending_state: Arc<RwLock<PendingState>>,

    /// Committed 状态（共识确认结果）
    committed_state: Arc<RwLock<CommittedState>>,

    /// 冲突检测器
    conflict_detector: ConflictDetector,

    /// 回滚管理器
    rollback_manager: RollbackManager,

    /// 共识提交队列（异步）
    consensus_queue: AsyncQueue<Transaction>,
}

/// Pending 状态
pub struct PendingState {
    /// 基础状态（从 Committed 派生）
    base_version: Version,

    /// 乐观执行的订单
    pending_orders: HashMap<OrderId, Order>,

    /// 临时余额变化
    balance_deltas: HashMap<(Address, AssetId), i64>,

    /// 依赖图（用于冲突检测）
    dependency_graph: DependencyGraph,
}

/// Committed 状态（与 V1 相同）
pub struct CommittedState {
    balances: HashMap<Address, HashMap<AssetId, u64>>,
    matching_engine: MatchingEngine,
    recent_trades: VecDeque<Fill>,
}
```

---

## 4. 双层状态机设计

### 4.1 状态转换

```
初始状态: Committed State (v0)
    │
    ├─ 乐观执行 Tx1 → Pending State (v0 + delta1)
    ├─ 乐观执行 Tx2 → Pending State (v0 + delta1 + delta2)
    ├─ 乐观执行 Tx3 → Pending State (v0 + delta1 + delta2 + delta3)
    │
    ↓ (共识确认)
    │
    ├─ Tx1 确认 ✓ → Committed State (v1)
    ├─ Tx2 确认 ✓ → Committed State (v2)
    ├─ Tx3 冲突 ✗ → 回滚，重新执行 → Committed State (v3')
    │
    ↓
最终状态: Committed State (v3')
```

### 4.2 状态查询

**查询优先级**:
1. 先查 Pending State
2. 如果未找到，查 Committed State

```rust
impl OptimisticDexExecutor {
    /// 查询订单状态（优先 Pending）
    pub async fn get_order(&self, order_id: OrderId) -> Option<Order> {
        // 1. 先查 Pending
        if let Some(order) = self.pending_state.read().await
            .pending_orders.get(&order_id) {
            return Some(order.clone());
        }

        // 2. 再查 Committed
        self.committed_state.read().await
            .matching_engine.get_order(order_id)
    }

    /// 查询余额（合并 Pending 和 Committed）
    pub async fn get_balance(&self, user: &Address, asset: &AssetId)
        -> u64 {
        let pending = self.pending_state.read().await;
        let committed = self.committed_state.read().await;

        let base_balance = committed.balances
            .get(user).and_then(|m| m.get(asset))
            .copied().unwrap_or(0);

        let delta = pending.balance_deltas
            .get(&(*user, *asset))
            .copied().unwrap_or(0);

        base_balance.saturating_add_signed(delta)
    }
}
```

### 4.3 Pending → Committed 转换

```rust
/// 共识确认后的处理
async fn on_consensus_confirmed(&mut self, tx: Transaction, result: ConsensusResult) {
    match result {
        ConsensusResult::Accepted => {
            // 将 Pending 提升到 Committed
            self.promote_to_committed(tx).await?;
        }

        ConsensusResult::Rejected(reason) => {
            // 回滚 Pending 状态
            self.rollback_pending(tx).await?;

            // 通知客户端
            self.notify_rollback(tx.id, reason).await?;
        }
    }
}

/// 提升到 Committed
async fn promote_to_committed(&mut self, tx: Transaction) -> Result<()> {
    let mut pending = self.pending_state.write().await;
    let mut committed = self.committed_state.write().await;

    // 应用 delta 到 Committed
    for ((user, asset), delta) in &pending.balance_deltas {
        let balance = committed.balances
            .entry(*user).or_default()
            .entry(*asset).or_default();
        *balance = balance.saturating_add_signed(*delta);
    }

    // 移动订单到 Committed
    if let Some(order) = pending.pending_orders.remove(&tx.order_id) {
        committed.matching_engine.add_order(order);
    }

    // 清理 Pending
    pending.clear_tx(tx.id);

    Ok(())
}
```

---

## 5. 乐观执行机制

### 5.1 乐观执行流程

```rust
/// 乐观执行入口
pub async fn optimistic_execute(&mut self, tx: Transaction)
    -> Result<OptimisticResult> {

    // 1. 冲突检测
    let conflict = self.conflict_detector.check(&tx).await?;

    match conflict {
        Conflict::None => {
            // 快速路径：无冲突
            self.fast_path_execute(tx).await
        }

        Conflict::Potential => {
            // 中速路径：可能冲突，需要乐观执行
            self.optimistic_path_execute(tx).await
        }

        Conflict::Certain => {
            // 慢速路径：必然冲突，降级到同步
            self.slow_path_execute(tx).await
        }
    }
}

/// 快速路径（无冲突）
async fn fast_path_execute(&mut self, tx: Transaction)
    -> Result<OptimisticResult> {

    let start = Instant::now();

    // 1. 在 Pending State 上执行
    let result = self.execute_on_pending(&tx).await?;

    // 2. 后台异步提交到共识
    self.submit_to_consensus_async(tx.clone());

    // 3. 立即返回（不等待共识）
    Ok(OptimisticResult {
        status: ExecutionStatus::Pending,
        order_id: result.order_id,
        fills: result.fills,
        latency: start.elapsed(),  // ~10ms
        confidence: 0.99,  // 99% 不会回滚
    })
}

/// 慢速路径（冲突）
async fn slow_path_execute(&mut self, tx: Transaction)
    -> Result<OptimisticResult> {

    // 降级到 V1 同步模式
    self.sync_execute(tx).await
}
```

### 5.2 冲突检测

```rust
pub struct ConflictDetector {
    /// 活跃交易集合
    active_txs: HashSet<TxId>,

    /// 依赖图
    dependency_graph: DependencyGraph,
}

impl ConflictDetector {
    /// 检查交易是否冲突
    pub fn check(&self, tx: &Transaction) -> Conflict {
        match tx {
            Transaction::PlaceOrder { trader, pair, .. } => {
                // 检查是否有同用户的 pending 订单
                if self.has_pending_order(trader, pair) {
                    return Conflict::Potential;
                }

                // 检查余额是否足够
                if !self.has_sufficient_balance(trader) {
                    return Conflict::Certain;
                }

                Conflict::None
            }

            Transaction::CancelOrder { trader, order_id } => {
                // 检查订单是否在 Pending
                if self.is_order_pending(order_id) {
                    return Conflict::Certain;
                }

                Conflict::None
            }

            _ => Conflict::None,
        }
    }
}

pub enum Conflict {
    None,       // 无冲突，快速路径
    Potential,  // 可能冲突，乐观执行
    Certain,    // 必然冲突，同步路径
}
```

### 5.3 依赖追踪

```rust
/// 依赖图（用于回滚）
pub struct DependencyGraph {
    /// Tx → 依赖的 Txs
    dependencies: HashMap<TxId, HashSet<TxId>>,

    /// Tx → 被依赖的 Txs
    dependents: HashMap<TxId, HashSet<TxId>>,
}

impl DependencyGraph {
    /// 添加依赖
    pub fn add_dependency(&mut self, tx: TxId, depends_on: TxId) {
        self.dependencies.entry(tx)
            .or_default()
            .insert(depends_on);

        self.dependents.entry(depends_on)
            .or_default()
            .insert(tx);
    }

    /// 获取需要回滚的交易（级联）
    pub fn get_rollback_cascade(&self, failed_tx: TxId) -> Vec<TxId> {
        let mut to_rollback = vec![failed_tx];
        let mut visited = HashSet::new();

        let mut queue = vec![failed_tx];
        while let Some(tx) = queue.pop() {
            if visited.contains(&tx) {
                continue;
            }
            visited.insert(tx);

            // 找到所有依赖此交易的交易
            if let Some(deps) = self.dependents.get(&tx) {
                for &dep in deps {
                    to_rollback.push(dep);
                    queue.push(dep);
                }
            }
        }

        to_rollback
    }
}
```

---

## 6. 冲突检测与回滚

### 6.1 回滚触发条件

| 条件 | 说明 | 概率 |
|-----|------|------|
| 余额不足 | 共识时余额变化 | < 1% |
| 订单冲突 | 同时操作同一订单 | < 2% |
| 撮合失败 | 市价单无流动性 | < 1% |
| 系统错误 | 网络、节点故障 | < 1% |
| **总计** | | **< 5%** |

### 6.2 回滚机制

```rust
pub struct RollbackManager {
    /// 回滚日志
    rollback_log: Vec<RollbackEntry>,

    /// 补偿操作
    compensations: HashMap<TxId, Vec<CompensationOp>>,
}

#[derive(Debug)]
pub struct RollbackEntry {
    tx_id: TxId,
    reason: RollbackReason,
    timestamp: u64,
    affected_orders: Vec<OrderId>,
    balance_restore: HashMap<Address, HashMap<AssetId, u64>>,
}

impl RollbackManager {
    /// 执行回滚
    pub async fn rollback(&mut self, tx_id: TxId, reason: RollbackReason)
        -> Result<()> {

        // 1. 获取依赖级联
        let cascade = self.dependency_graph.get_rollback_cascade(tx_id);

        // 2. 按依赖顺序回滚
        for tx in cascade.iter().rev() {
            self.rollback_single(*tx).await?;
        }

        // 3. 记录回滚日志
        self.log_rollback(tx_id, reason, cascade);

        // 4. 通知受影响的客户端
        self.notify_affected_users(cascade).await?;

        Ok(())
    }

    /// 回滚单个交易
    async fn rollback_single(&mut self, tx_id: TxId) -> Result<()> {
        let mut pending = self.pending_state.write().await;

        // 1. 恢复余额
        if let Some(deltas) = pending.balance_deltas_by_tx.remove(&tx_id) {
            for ((user, asset), delta) in deltas {
                pending.apply_delta(user, asset, -delta);
            }
        }

        // 2. 移除订单
        if let Some(orders) = pending.orders_by_tx.remove(&tx_id) {
            for order_id in orders {
                pending.pending_orders.remove(&order_id);
            }
        }

        // 3. 清理依赖
        pending.dependency_graph.remove_tx(tx_id);

        Ok(())
    }
}

#[derive(Debug)]
pub enum RollbackReason {
    InsufficientBalance,
    OrderConflict,
    ConsensusRejection,
    Timeout,
}
```

### 6.3 用户通知机制

```rust
/// 状态变更通知
pub enum StatusUpdate {
    /// Pending：乐观执行成功
    Pending {
        tx_id: TxId,
        estimated_confirm_time: Duration,
        confidence: f64,  // 0.95 = 95% 不会回滚
    },

    /// Confirmed：共识确认
    Confirmed {
        tx_id: TxId,
        final_result: ExecutionResult,
    },

    /// Rolled Back：回滚
    RolledBack {
        tx_id: TxId,
        reason: RollbackReason,
        retry_suggested: bool,
    },
}

/// 订阅状态更新
pub async fn subscribe_status(&self, tx_id: TxId)
    -> Receiver<StatusUpdate> {
    let (tx, rx) = mpsc::channel(10);

    self.status_subscribers.lock().await
        .insert(tx_id, tx);

    rx
}
```

---

## 7. 数据流与时序

### 7.1 快速路径时序图（95%情况）

```
客户端              API              乐观执行器          共识层           确认层
  │                  │                  │                 │               │
  │  PlaceOrder      │                  │                 │               │
  ├─────────────────>│                  │                 │               │
  │                  │  Check Conflict  │                 │               │
  │                  ├─────────────────>│                 │               │
  │                  │  No Conflict ✓   │                 │               │
  │                  │<─────────────────┤                 │               │
  │                  │                  │                 │               │
  │                  │  Optimistic Exec │                 │               │
  │                  ├─────────────────>│                 │               │
  │                  │  (10ms)          │                 │               │
  │                  │  Pending Result  │                 │               │
  │                  │<─────────────────┤                 │               │
  │                  │                  │                 │               │
  │  OrderId(Pending)│                  │                 │               │
  │<─────────────────┤                  │                 │               │
  │  [用户感知延迟: ~50ms]              │                 │               │
  │                  │                  │                 │               │
  │                  │                  │  Async Submit   │               │
  │                  │                  ├────────────────>│               │
  │                  │                  │  (后台 400ms)   │               │
  │                  │                  │                 │  Consensus OK │
  │                  │                  │                 ├──────────────>│
  │                  │                  │                 │  Confirm      │
  │                  │                  │<────────────────┴───────────────┤
  │                  │                  │  Promote to Committed            │
  │                  │                  │                                  │
  │  Notification(Confirmed)            │                                  │
  │<────────────────────────────────────┤                                  │
  │  [后台通知: ~450ms]                 │                                  │
```

**关键时间点**:
- T0: 提交请求
- T+10ms: 冲突检测完成
- T+20ms: 乐观执行完成
- **T+50ms: 返回 Pending 结果 ← 用户感知延迟**
- T+450ms: 共识确认
- T+460ms: 后台通知 Confirmed

### 7.2 慢速路径时序图（5%情况）

```
客户端              API              乐观执行器          共识层
  │                  │                  │                 │
  │  CancelOrder     │                  │                 │
  ├─────────────────>│                  │                 │
  │                  │  Check Conflict  │                 │
  │                  ├─────────────────>│                 │
  │                  │  Conflict! ✗     │                 │
  │                  │<─────────────────┤                 │
  │                  │                  │                 │
  │                  │  Sync Mode       │                 │
  │                  ├─────────────────>│                 │
  │                  │                  │  Wait Consensus │
  │                  │                  ├────────────────>│
  │                  │  (400ms)         │  (400ms)        │
  │                  │                  │<────────────────┤
  │                  │  Final Result    │                 │
  │                  │<─────────────────┤                 │
  │  OrderId(Confirmed)                 │                 │
  │<─────────────────┤                  │                 │
  │  [用户感知延迟: ~450ms] (降级到V1)  │                 │
```

### 7.3 回滚场景时序图（< 1%情况）

```
客户端              API              乐观执行器          共识层           确认层
  │                  │                  │                 │               │
  │  (已返回Pending) │                  │                 │               │
  │                  │                  │  Async Submit   │               │
  │                  │                  ├────────────────>│               │
  │                  │                  │                 │  Rejected ✗   │
  │                  │                  │                 ├──────────────>│
  │                  │                  │                 │  Trigger      │
  │                  │                  │<────────────────┴───────────────┤
  │                  │                  │  Rollback                        │
  │                  │                  │  - Restore Balance               │
  │                  │                  │  - Remove Order                  │
  │                  │                  │  - Log Event                     │
  │  Notification(RolledBack)           │                                  │
  │<────────────────────────────────────┤                                  │
  │  Reason: InsufficientBalance        │                                  │
```

---

## 8. 性能分析

### 8.1 延迟分析

#### V1 延迟组成:
```
总延迟 = API处理 + 共识延迟 + 执行时间
       = 1ms + 400ms + 10μs
       ≈ 401ms
```

#### V2 延迟组成（快速路径）:
```
用户感知延迟 = API处理 + 冲突检测 + 乐观执行
             = 1ms + 5ms + 10ms
             = 16ms

后台确认延迟 = 共识延迟 + 确认处理
             = 400ms + 10ms
             = 410ms (用户无感知)
```

**延迟改进**: 401ms → 16ms = **25x 提升**

### 8.2 吞吐量分析

#### V1 吞吐量:
```
单节点吞吐 = 1 / 共识延迟
           = 1 / 0.4s
           = 2.5 TPS

批量优化后 = 1000笔/批 / 0.4s
           = 2500 TPS
```

#### V2 吞吐量:
```
乐观执行吞吐 = 1 / 乐观执行时间
             = 1 / 0.01s
             = 100 TPS (单线程)

并行处理后 = 100 TPS × 50并发
           = 5000 TPS
```

**吞吐量提升**: 2500 TPS → 5000 TPS = **2x 提升**

### 8.3 回滚开销

**回滚概率**: < 5%

**回滚成本**:
- 计算开销: ~1ms (恢复状态)
- 存储开销: ~100KB (回滚日志)
- 通知开销: ~10ms (用户通知)

**平均成本**:
```
平均额外延迟 = 回滚概率 × 回滚开销
             = 5% × (1ms + 10ms)
             = 0.55ms

可忽略不计 ✓
```

### 8.4 内存开销

**V1 内存**:
- Committed State: ~100MB (10万订单)

**V2 额外内存**:
- Pending State: ~20MB (2万pending订单)
- 依赖图: ~10MB
- 回滚日志: ~5MB

**总内存**: 100MB + 35MB = **135MB** ✓

---

## 9. 风险与权衡

### 9.1 权衡分析

| 维度 | V1 | V2 | 权衡 |
|-----|----|----|------|
| **延迟** | 400ms | 50ms | V2 胜出 |
| **吞吐** | 2.5K TPS | 5K TPS | V2 胜出 |
| **一致性** | 强一致 | 最终一致 | V1 更严格 |
| **复杂度** | 低 | 中 | V1 更简单 |
| **回滚风险** | 无 | < 5% | V1 更可靠 |
| **内存开销** | 100MB | 135MB | V1 更少 |

### 9.2 风险评估

| 风险 | 严重性 | 概率 | 缓解措施 |
|-----|--------|------|---------|
| 回滚率过高 | 高 | 低 | 改进冲突检测算法 |
| 级联回滚 | 高 | 极低 | 依赖图隔离 |
| 状态不一致 | 高 | 低 | 严格测试 |
| 内存泄漏 | 中 | 低 | 定期GC pending state |
| 通知丢失 | 中 | 低 | 持久化通知队列 |

### 9.3 适用场景

**V2 最适合**:
- ✅ 高频交易场景
- ✅ 用户体验敏感
- ✅ 冲突率低（< 5%）
- ✅ 可容忍最终一致性

**V2 不适合**:
- ❌ 金融结算（需强一致）
- ❌ 监管审计（需完整历史）
- ❌ 冲突率高（> 10%）

### 9.4 降级策略

**自动降级条件**:
1. 回滚率 > 10%
2. Pending 队列 > 10000
3. 共识延迟 > 1s
4. 内存使用 > 80%

**降级操作**:
```rust
pub enum ExecutionMode {
    Optimistic,  // V2 模式
    Sync,        // V1 模式（降级）
}

impl OptimisticDexExecutor {
    pub async fn execute(&mut self, tx: Transaction) -> Result<()> {
        match self.current_mode() {
            ExecutionMode::Optimistic => {
                self.optimistic_execute(tx).await
            }
            ExecutionMode::Sync => {
                self.sync_execute(tx).await
            }
        }
    }

    fn current_mode(&self) -> ExecutionMode {
        let metrics = self.metrics.read();

        // 检查降级条件
        if metrics.rollback_rate > 0.1 {
            return ExecutionMode::Sync;
        }

        if metrics.pending_queue_size > 10000 {
            return ExecutionMode::Sync;
        }

        ExecutionMode::Optimistic
    }
}
```

---

## 10. 实现路线图

### 10.1 阶段划分

**阶段 1: 基础框架**（2-3天）
- [ ] 双层状态机
- [ ] Pending State 管理
- [ ] 基础冲突检测
- [ ] 单元测试

**阶段 2: 乐观执行**（2-3天）
- [ ] 快速路径实现
- [ ] 慢速路径降级
- [ ] 异步共识提交
- [ ] 集成测试

**阶段 3: 回滚机制**（2天）
- [ ] 依赖图追踪
- [ ] 回滚管理器
- [ ] 补偿事务
- [ ] 回滚测试

**阶段 4: 性能优化**（2天）
- [ ] 并发优化
- [ ] 内存优化
- [ ] 批量处理
- [ ] 性能测试

**阶段 5: 监控与降级**（1天）
- [ ] 指标采集
- [ ] 自动降级
- [ ] 告警系统
- [ ] 压力测试

**总计**: 9-11天

### 10.2 关键里程碑

| 里程碑 | 验收标准 | 预期时间 |
|-------|---------|---------|
| M1: 框架完成 | 双层状态机可运行 | Day 3 |
| M2: 乐观执行 | 快速路径 < 50ms | Day 6 |
| M3: 回滚机制 | 回滚率 < 5% | Day 8 |
| M4: 性能达标 | 5K TPS, P50 < 50ms | Day 10 |
| M5: 生产就绪 | 监控完善，可降级 | Day 11 |

### 10.3 测试策略

**单元测试**:
- 冲突检测逻辑
- 回滚机制
- 状态转换

**集成测试**:
- 端到端流程
- 回滚场景
- 降级逻辑

**性能测试**:
- 延迟分布（P50, P95, P99）
- 吞吐量测试
- 回滚率统计
- 并发压力测试

**混沌测试**:
- 随机节点故障
- 网络分区
- 极端负载

---

## 附录

### A. V2 vs V1 完整对比表

| 特性 | V1 | V2 |
|-----|----|----|
| **端到端延迟 P50** | ~400ms | ~50ms |
| **端到端延迟 P99** | ~600ms | ~100ms |
| **吞吐量** | 2.5K TPS | 5K TPS |
| **一致性模型** | 强一致 | 最终一致 |
| **回滚概率** | 0% | < 5% |
| **内存开销** | 100MB | 135MB |
| **实现复杂度** | 低 | 中 |
| **适用场景** | 原型验证 | 生产环境 |

### B. 性能对比图

```
延迟对比 (ms):
V1: ████████████████████████████████████████  400ms
V2: █████                                      50ms
    └────────────────────────────────────────┘
    0        100       200       300       400

吞吐量对比 (TPS):
V1: ████████████  2,500 TPS
V2: ████████████████████████  5,000 TPS
    └────────────────────────────────────────┘
    0      1K      2K      3K      4K      5K
```

### C. 决策树

```
新交易到达
    │
    ├─ 冲突检测
    │   ├─ 无冲突 (95%) → 快速路径 → 50ms
    │   ├─ 可能冲突 (4%) → 乐观执行 → 100ms
    │   └─ 必然冲突 (1%) → 同步执行 → 400ms
    │
    ├─ 共识确认 (后台)
    │   ├─ 成功 (95%) → Promote to Committed
    │   └─ 失败 (5%) → 触发回滚
    │
    └─ 用户通知
        ├─ Pending (立即)
        ├─ Confirmed (400ms后)
        └─ RolledBack (如需要)
```

---

**文档版本**: v2.0
**对应实现**: V1 为基础，V2 为优化版
**推荐策略**: 先实现 V1 验证，再迭代到 V2
**最后更新**: 2025-12-16
