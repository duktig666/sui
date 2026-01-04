# DEX L1 Sequencer 设计 / Sequencer Design

> **版本**: v1.2
> **状态**: Draft
> **最后更新**: 2025-12-31
> **目标读者**: 技术评审 / 架构师

---

## 1. 概述 / Overview

### 1.1 设计目标 / Design Goals

1. **低延迟排序**: < 5ms 序列号分配
2. **高可用**: < 100ms 故障切换
3. **确定性**: 所有验证者产生相同排序
4. **复用 Sui 基础设施**: 网络层、选举逻辑

### 1.2 设计原则 / Design Principles

- **复用 mysten-network**: P2P 网络层 (anemo)
- **复用 consensus-core**: Leader 选举逻辑
- **复用 shared-crypto**: 签名验证
- **DEX 专用**: 序列号分配、批次聚合

---

## 2. 复用 Sui 基础设施 / Reusing Sui Infrastructure

### 2.1 网络层复用 / Network Layer Reuse

```
┌─────────────────────────────────────────────────────────────┐
│                    Network Layer                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                 mysten-network                           ││
│  │  ┌───────────┐  ┌───────────┐  ┌───────────┐           ││
│  │  │  anemo    │  │   tonic   │  │  codecs   │           ││
│  │  │  (P2P)    │  │  (gRPC)   │  │  (BCS)    │           ││
│  │  └───────────┘  └───────────┘  └───────────┘           ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │               DEX Sequencer Protocol                     ││
│  │  • Order broadcast                                       ││
│  │  • Sequence confirmation                                 ││
│  │  • Heartbeat / health check                              ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**复用组件**：
- `anemo::Network`: P2P 网络连接管理
- `anemo::Router`: 消息路由
- `tonic`: gRPC 框架
- `BCS`: 序列化

### 2.2 Leader 选举复用 / Leader Election Reuse

```
consensus/core/src/leader_schedule.rs
┌─────────────────────────────────────────────────────────────┐
│                    Leader Schedule                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  LeaderSchedule (Sui)                                    ││
│  │  • stake-weighted leader 选举                            ││
│  │  • round-robin 轮换                                      ││
│  │  • LeaderSwapTable 故障检测                              ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  DexLeaderSchedule (复用 + 扩展)                         ││
│  │  • 复用 stake-weighted 算法                              ││
│  │  • 更快的故障检测 (50ms vs 500ms)                        ││
│  │  • DEX 专用 Leader 状态                                  ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**复用代码**：
```rust
// 直接复用 Sui LeaderSchedule
use consensus_core::leader_schedule::{
    LeaderSchedule,
    LeaderSwapTable,
};

pub struct DexLeaderSchedule {
    inner: LeaderSchedule,
    // DEX 专用配置
    heartbeat_interval: Duration,
    failure_threshold: Duration,
}
```

### 2.3 签名验证复用 / Signature Verification Reuse

```rust
// 复用 shared-crypto
use shared_crypto::intent::{Intent, IntentScope};
use fastcrypto::ed25519::Ed25519Signature;

// DEX 专用 Intent Scope
pub const DEX_ORDER_INTENT: IntentScope = IntentScope::DexOrder;
pub const DEX_CANCEL_INTENT: IntentScope = IntentScope::DexCancel;
```

---

## 3. 状态机设计 / State Machine Design

### 3.1 Sequencer 状态 / Sequencer States

```
┌─────────────────────────────────────────────────────────────┐
│                   Sequencer State Machine                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│                    ┌─────────┐                              │
│          ┌────────►│ Stopped │◄────────┐                    │
│          │         └────┬────┘         │                    │
│          │              │ start()      │ stop()             │
│          │              ▼              │                    │
│          │         ┌─────────┐         │                    │
│          │    ┌───►│ Syncing │────┐    │                    │
│          │    │    └────┬────┘    │    │                    │
│          │    │         │ synced  │    │                    │
│          │    │         ▼         │    │                    │
│    fail  │    │    ┌─────────┐    │    │                    │
│          │    │    │ Standby │    │ fail                    │
│          │    │    └────┬────┘    │    │                    │
│          │    │         │ elected │    │                    │
│          │    │         ▼         │    │                    │
│          │    │    ┌─────────┐    │    │                    │
│          └────┼────│ Leader  │────┼────┘                    │
│               │    └─────────┘    │                         │
│               │         │         │                         │
│               │    lost │         │                         │
│               └─────────┘         │                         │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 状态转换 / State Transitions

| 当前状态 | 事件 | 目标状态 | 动作 |
|---------|------|---------|------|
| Stopped | start() | Syncing | 开始同步状态 |
| Syncing | 同步完成 | Standby | 进入待命 |
| Syncing | 同步失败 | Stopped | 清理资源 |
| Standby | 当选 Leader | Leader | 开始处理订单 |
| Leader | 失去 Leadership | Syncing | 同步最新状态 |
| Leader | 故障检测 | Standby | 等待切换完成 |
| * | stop() | Stopped | 优雅关闭 |

---

## 4. 序列号分配机制 / Sequence Number Assignment

### 4.1 序列号结构 / Sequence Number Structure

```
┌─────────────────────────────────────────────────────────────┐
│                    Sequence Number                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  64-bit Sequence Number                               │   │
│  │  ┌──────────────┬─────────────────────────────────┐  │   │
│  │  │   Epoch (16) │    Counter (48)                 │  │   │
│  │  └──────────────┴─────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  Epoch:   Leader 任期编号                                    │
│  Counter: 单调递增计数器                                     │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 分配流程 / Assignment Flow

```
Client              Sequencer           Validators
  │                     │                    │
  │ ─── Order ────────► │                    │
  │                     │                    │
  │                     │ ── Assign SeqNo    │
  │                     │    (atomic)        │
  │                     │                    │
  │ ◄── SeqNo + Sig ─── │                    │
  │                     │                    │
  │                     │ ═══ Broadcast ═══► │
  │                     │                    │
  │                     │ ◄═══ ACKs ════════ │
  │                     │    (2f+1)          │
  │                     │                    │
  │ ◄── Confirmed ───── │                    │
```

### 4.3 序列号分配优化 / Optimization

```rust
/// 序列号分配器 / Sequence Number Allocator
pub struct SeqAllocator {
    /// 当前序列号 (原子操作)
    current: AtomicU64,
    /// 预分配批次大小
    batch_size: u64,
    /// 下一个预分配边界
    next_boundary: AtomicU64,
}

impl SeqAllocator {
    /// 快速分配 (无锁)
    pub fn allocate(&self) -> SeqNumber {
        // 原子递增，无锁
        let seq = self.current.fetch_add(1, Ordering::SeqCst);
        SeqNumber(seq)
    }

    /// 批量预分配
    pub fn allocate_batch(&self, count: u64) -> Range<SeqNumber> {
        let start = self.current.fetch_add(count, Ordering::SeqCst);
        SeqNumber(start)..SeqNumber(start + count)
    }
}
```

---

## 5. 高可用设计 / High Availability Design

### 5.1 故障检测 / Failure Detection

```
┌─────────────────────────────────────────────────────────────┐
│                    Failure Detection                         │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Timeline:                                                   │
│                                                              │
│  T0        T0+25ms      T0+50ms      T0+75ms      T0+100ms  │
│  │           │            │            │            │        │
│  │ Heartbeat │ Heartbeat  │ MISS       │ Detected   │ Switch │
│  │    OK     │    OK      │            │ (2f+1)     │ Done   │
│  ▼           ▼            ▼            ▼            ▼        │
│  ●───────────●────────────●────────────●────────────●        │
│                           │                                  │
│                      Timeout                                 │
│                      (50ms)                                  │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 切换流程 / Failover Flow

```
Old Leader          Validators           New Leader
     │                   │                    │
     │ ─── Heartbeat ──► │                    │
     │                   │                    │
     X (Failure)         │                    │
                         │                    │
                    ┌────┴────┐               │
                    │ Timeout │               │
                    │ (50ms)  │               │
                    └────┬────┘               │
                         │                    │
                         │ ◄─ Vote Request ── │
                         │                    │
                         │ ─── Vote (2f+1) ─► │
                         │                    │
                         │ ◄── Leader Claim ─ │
                         │                    │
                         │ ─── ACK ─────────► │
                         │                    │
                         │                    │ ← Start Processing
```

### 5.3 状态同步 / State Synchronization

```rust
/// 状态同步协议 / State Sync Protocol
pub struct StateSyncProtocol {
    /// 最新确认的序列号
    confirmed_seq: SeqNumber,
    /// 待确认的交易
    pending_txs: BTreeMap<SeqNumber, Transaction>,
}

impl StateSyncProtocol {
    /// 新 Leader 同步状态
    pub async fn sync_from_peers(&mut self) -> Result<()> {
        // 1. 查询所有验证者的最新 confirmed_seq
        let peer_seqs = self.query_peer_sequences().await?;

        // 2. 确定需要同步的范围
        let max_seq = peer_seqs.values().max();

        // 3. 获取缺失的交易
        for seq in self.confirmed_seq..=*max_seq {
            if !self.pending_txs.contains_key(&seq) {
                let tx = self.fetch_transaction(seq).await?;
                self.pending_txs.insert(seq, tx);
            }
        }

        Ok(())
    }
}
```

---

## 6. 序列广播与确认 / Sequence Broadcast & Confirmation

### 6.1 广播协议 / Broadcast Protocol

```
┌─────────────────────────────────────────────────────────────┐
│                    Broadcast Protocol                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Batch Message                                          ││
│  │  ┌─────────────┬─────────────┬─────────────┬─────────┐ ││
│  │  │ Batch ID    │ Seq Range   │ Transactions │ Sig     │ ││
│  │  │ (8 bytes)   │ (16 bytes)  │ (variable)   │(64 bytes││
│  │  └─────────────┴─────────────┴─────────────┴─────────┘ ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  Broadcast Flow:                                             │
│                                                              │
│  Leader ─────► Validator A ─────► ACK                       │
│         ─────► Validator B ─────► ACK                       │
│         ─────► Validator C ─────► ACK                       │
│                                                              │
│  Collect 2f+1 ACKs → Batch Confirmed                        │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 确认机制 / Confirmation Mechanism

```rust
/// 软确认 / Soft Confirmation
pub struct SoftConfirmation {
    pub seq: SeqNumber,
    pub tx_digest: TxDigest,
    pub leader_sig: Signature,
}

/// 硬确认 / Hard Confirmation
pub struct HardConfirmation {
    pub seq: SeqNumber,
    pub tx_digest: TxDigest,
    pub certificate: Certificate, // 2f+1 签名
}

/// 确认收集器 / Confirmation Collector
pub struct ConfirmationCollector {
    threshold: usize, // 2f+1
    votes: DashMap<SeqNumber, Vec<ValidatorVote>>,
}

impl ConfirmationCollector {
    pub fn add_vote(&self, seq: SeqNumber, vote: ValidatorVote) -> Option<HardConfirmation> {
        let mut votes = self.votes.entry(seq).or_default();
        votes.push(vote);

        if votes.len() >= self.threshold {
            Some(HardConfirmation::from_votes(&votes))
        } else {
            None
        }
    }
}
```

---

## 7. 批次聚合策略 / Batch Aggregation Strategy

### 7.1 批次配置 / Batch Configuration

```rust
pub struct BatchConfig {
    /// 最大批次大小
    pub max_batch_size: usize,      // 默认: 1000
    /// 最大等待时间
    pub max_batch_timeout: Duration, // 默认: 5ms
    /// 最小批次大小
    pub min_batch_size: usize,       // 默认: 10
}
```

### 7.2 批次形成逻辑 / Batch Formation

```
┌─────────────────────────────────────────────────────────────┐
│                    Batch Formation                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Condition: Batch forms when ANY condition is met           │
│                                                              │
│  1. Size threshold:  orders.len() >= max_batch_size         │
│  2. Time threshold:  elapsed >= max_batch_timeout           │
│  3. Priority signal: high_priority_order received           │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                        Timeline                          ││
│  │  T0      T1      T2      T3      T4      T5             ││
│  │  │       │       │       │       │       │               ││
│  │  │ +10   │ +50   │ +100  │ +200  │ +500  │ +1000        ││
│  │  │orders │orders │orders │orders │orders │orders         ││
│  │  ▼       ▼       ▼       ▼       ▼       ▼               ││
│  │  ├───────┴───────┴───────┴───────┼───────┤               ││
│  │  │         Batch 1 (860)         │  B2   │               ││
│  │  │     (timeout @ 5ms)           │(size) │               ││
│  │  └───────────────────────────────┴───────┘               ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 8. 配置参数权衡 / Configuration Trade-offs

### 8.1 参数矩阵 / Parameter Matrix

| 参数 | 默认值 | 范围 | 影响 |
|-----|-------|------|------|
| `heartbeat_interval` | 25ms | 10-100ms | 故障检测速度 vs 网络开销 |
| `failure_threshold` | 50ms | 25-200ms | 误判率 vs 切换速度 |
| `max_batch_size` | 1000 | 100-10000 | 吞吐量 vs 延迟 |
| `max_batch_timeout` | 5ms | 1-50ms | 延迟 vs 吞吐量 |
| `vote_threshold` | 2f+1 | - | 安全性 (固定) |

### 8.2 权衡分析 / Trade-off Analysis

```
┌─────────────────────────────────────────────────────────────┐
│                    Trade-off Analysis                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Heartbeat Interval:                                         │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  10ms ────────────────────────────────────────── 100ms │ │
│  │    │                                               │    │ │
│  │    │ Fast detection          Slow detection       │    │ │
│  │    │ High network overhead   Low network overhead │    │ │
│  │    │ Risk of false positive  Slow failover        │    │ │
│  │    ▼                                               ▼    │ │
│  │   [===]                                                 │ │
│  │     ↑                                                   │ │
│  │   25ms (recommended)                                    │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  Batch Size vs Latency:                                      │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Size ↑  │  Throughput ↑  │  Latency ↑               │ │
│  │  Size ↓  │  Throughput ↓  │  Latency ↓               │ │
│  │                                                        │ │
│  │  Optimal: Dynamic batching based on load              │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 9. 关键数据结构 / Key Data Structures

### 9.1 Sequencer 核心结构

```rust
pub struct Sequencer {
    /// 节点身份
    identity: ValidatorId,
    /// 状态
    state: AtomicState,
    /// 序列号分配器
    seq_allocator: SeqAllocator,
    /// 批次聚合器
    batch_aggregator: BatchAggregator,
    /// Leader 调度器
    leader_schedule: DexLeaderSchedule,
    /// 网络层
    network: Arc<anemo::Network>,
    /// 确认收集器
    confirmation_collector: ConfirmationCollector,
    /// 配置
    config: SequencerConfig,
}
```

### 9.2 消息类型

```rust
/// Sequencer 消息类型
pub enum SequencerMessage {
    /// 订单提交
    SubmitOrder(Order),
    /// 批次广播
    BroadcastBatch(Batch),
    /// 投票
    Vote(Vote),
    /// 心跳
    Heartbeat(HeartbeatMessage),
    /// 状态查询
    QueryState(QueryStateRequest),
}
```

---

## 10. 性能指标 / Performance Metrics

### 10.1 关键指标 / Key Metrics

| 指标 | 目标 | 测量点 |
|-----|------|-------|
| 序列号分配延迟 | < 1μs | SeqAllocator.allocate() |
| 批次形成延迟 | < 5ms | BatchAggregator.form() |
| 广播延迟 | < 10ms | Network.broadcast() |
| 2f+1 确认延迟 | < 50ms | ConfirmationCollector |
| 故障检测延迟 | < 50ms | LeaderSchedule.detect() |
| 切换完成延迟 | < 100ms | Failover.complete() |

### 10.2 监控指标 / Monitoring Metrics

```rust
// Prometheus 指标
lazy_static! {
    pub static ref SEQ_ASSIGN_LATENCY: Histogram = register_histogram!(
        "dex_seq_assign_latency_seconds",
        "Sequence number assignment latency",
        vec![0.000001, 0.00001, 0.0001, 0.001]
    ).unwrap();

    pub static ref BATCH_SIZE: Histogram = register_histogram!(
        "dex_batch_size",
        "Batch size distribution",
        vec![10.0, 50.0, 100.0, 500.0, 1000.0]
    ).unwrap();

    pub static ref CONFIRMATION_LATENCY: Histogram = register_histogram!(
        "dex_confirmation_latency_seconds",
        "Time to collect 2f+1 confirmations",
        vec![0.01, 0.02, 0.05, 0.1, 0.2]
    ).unwrap();
}
```

---

## 11. 安全不变量与缓解措施 / Security Invariants & Mitigations

> **关联文档**：本节补充 `02-ARCHITECTURE-OVERVIEW.md` 5.2 威胁建模章节，聚焦 Sequencer 特定安全问题。

### 11.1 安全不变量 / Security Invariants

| 不变量 ID | 描述 | 违反后果 | 验证机制 |
|----------|------|---------|---------|
| **SI-SEQ-001** | 序列号严格单调递增，无间隙 | 订单丢失/重排 | Validator 检测间隙 |
| **SI-SEQ-002** | 同一 Epoch 内仅一个 Leader 有效 | 双花/冲突 | 2f+1 签名验证 |
| **SI-SEQ-003** | 批次内订单顺序不可篡改 | MEV 抢跑 | 批次签名 + Merkle Root |
| **SI-SEQ-004** | Leader 必须在 `max_batch_timeout` 内广播 | 审查攻击 | Validator 超时检测 |
| **SI-SEQ-005** | 相同输入必须产生相同序列号 | 非确定性 | 确定性随机数种子 |

### 11.2 Sequencer 作恶缓解 / Sequencer Misbehavior Mitigation

#### 11.2.1 作恶行为分类 / Misbehavior Categories

```
┌─────────────────────────────────────────────────────────────────────┐
│                 Sequencer Misbehavior Categories                     │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐  │
│  │  Censorship      │  │  Front-running   │  │  DoS Attack      │  │
│  │  (审查攻击)       │  │  (抢跑攻击)       │  │  (拒绝服务)       │  │
│  │                  │  │                  │  │                  │  │
│  │  • 拒绝特定用户   │  │  • 重排订单      │  │  • 不广播批次    │  │
│  │  • 延迟特定订单   │  │  • 插入自有订单  │  │  • 不响应心跳    │  │
│  │  • 选择性丢弃     │  │  • 三明治攻击    │  │  • 产生无效批次  │  │
│  └──────────────────┘  └──────────────────┘  └──────────────────┘  │
│                                                                      │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐  │
│  │  Equivocation    │  │  Sequence Gap    │  │  Stale Data      │  │
│  │  (双重签名)       │  │  (序列号间隙)     │  │  (陈旧数据)       │  │
│  │                  │  │                  │  │                  │  │
│  │  • 同 seq 不同内容│  │  • 跳过序列号    │  │  • 旧状态响应    │  │
│  │  • 分叉批次      │  │  • 创造间隙      │  │  • 错误余额      │  │
│  └──────────────────┘  └──────────────────┘  └──────────────────┘  │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

#### 11.2.2 缓解机制 / Mitigation Mechanisms

**1. 审查攻击缓解 (Censorship Resistance)**

```rust
/// 强制包含机制 / Forced Inclusion Mechanism
pub struct ForcedInclusion {
    /// 订单在 pending 队列的最大停留时间
    max_pending_time: Duration, // 默认: 500ms
    /// 超时订单强制进入下一批次
    force_include_after_timeout: bool,
}

impl Sequencer {
    /// Validators 检测审查行为
    fn detect_censorship(&self, order: &Order) -> Option<CensorshipEvidence> {
        let pending_since = self.pending_orders.get_timestamp(&order.id)?;
        let elapsed = Instant::now() - pending_since;

        if elapsed > self.config.max_pending_time {
            // 订单被延迟超过阈值，产生证据
            Some(CensorshipEvidence {
                order_id: order.id,
                pending_since,
                elapsed,
                batches_skipped: self.count_skipped_batches(&order.id),
            })
        } else {
            None
        }
    }
}
```

**2. 抢跑攻击缓解 (Front-running Prevention)**

| 缓解策略 | 描述 | 延迟影响 | 实现复杂度 |
|---------|------|---------|-----------|
| **Commit-Reveal** | 订单先提交 Hash，再揭示内容 | +1 RTT (~50ms) | 中等 |
| **时间锁订单** | 订单携带最早执行时间 | 无额外延迟 | 低 |
| **批次密封** | 批次形成后不可修改 | 无额外延迟 | 低 |
| **随机延迟** | Leader 不知道精确执行时间 | +随机 0-10ms | 低 |

```rust
/// 批次密封机制 / Batch Sealing
pub struct SealedBatch {
    /// 批次 ID
    id: BatchId,
    /// 订单 Merkle Root (密封后不可修改)
    order_root: [u8; 32],
    /// 密封时间戳
    sealed_at: u64,
    /// Leader 签名
    leader_sig: Signature,
}

impl SealedBatch {
    /// 验证批次完整性
    pub fn verify(&self, orders: &[Order]) -> Result<(), BatchError> {
        let computed_root = merkle_root(orders);
        if computed_root != self.order_root {
            return Err(BatchError::TamperedOrders);
        }
        Ok(())
    }
}
```

**3. 双重签名检测 (Equivocation Detection)**

```rust
/// 双重签名检测器 / Equivocation Detector
pub struct EquivocationDetector {
    /// 已见批次: (Epoch, SeqRange) -> BatchDigest
    seen_batches: DashMap<(u64, SeqRange), BatchDigest>,
}

impl EquivocationDetector {
    /// 检测双重签名
    pub fn check_and_record(&self, batch: &SignedBatch) -> Result<(), EquivocationProof> {
        let key = (batch.epoch, batch.seq_range.clone());

        match self.seen_batches.entry(key) {
            Entry::Occupied(entry) => {
                if *entry.get() != batch.digest() {
                    // 检测到双重签名！
                    return Err(EquivocationProof {
                        epoch: batch.epoch,
                        seq_range: batch.seq_range.clone(),
                        digest_a: *entry.get(),
                        digest_b: batch.digest(),
                        sig_a: entry.get().signature.clone(),
                        sig_b: batch.signature.clone(),
                    });
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(batch.digest());
            }
        }
        Ok(())
    }
}
```

**4. 序列号间隙检测 (Gap Detection)**

```rust
/// 间隙检测器 / Gap Detector
pub struct GapDetector {
    /// 期望的下一个序列号
    expected_next: AtomicU64,
    /// 已发现的间隙
    gaps: DashMap<SeqRange, GapInfo>,
}

impl GapDetector {
    /// 检测间隙
    pub fn on_batch_received(&self, batch: &Batch) -> Option<GapAlert> {
        let expected = self.expected_next.load(Ordering::SeqCst);
        let batch_start = batch.seq_range.start;

        if batch_start > expected {
            // 发现间隙
            let gap = SeqRange { start: expected, end: batch_start };
            self.gaps.insert(gap.clone(), GapInfo {
                detected_at: Instant::now(),
                expected,
                received: batch_start,
            });

            Some(GapAlert { gap, batch_id: batch.id })
        } else {
            self.expected_next.store(batch.seq_range.end, Ordering::SeqCst);
            None
        }
    }
}
```

### 11.3 故障 vs 作恶区分 / Distinguishing Faults from Attacks

| 行为 | 可能原因 | 证据要求 | 响应措施 |
|------|---------|---------|---------|
| 心跳超时 | 网络分区 / 宕机 / 故意不响应 | 2f+1 观察到超时 | 触发 Leader 切换 |
| 批次延迟 | 高负载 / 网络拥塞 / 审查 | 持续 N 个批次超时 | 降低信誉分 + 切换 |
| 序列号间隙 | Bug / 存储故障 / 故意跳过 | Gap 证据 + 多验证者确认 | 立即切换 + Slash |
| 双重签名 | **仅可能是故意攻击** | Equivocation Proof | 立即 Slash |

### 11.4 惩罚机制 / Slashing Conditions

```rust
/// Slash 条件 / Slashing Conditions
pub enum SlashableOffense {
    /// 双重签名 (最严重)
    Equivocation {
        proof: EquivocationProof,
        penalty_pct: u8, // 100% 全额 Slash
    },
    /// 持续审查
    PersistentCensorship {
        evidence: Vec<CensorshipEvidence>,
        penalty_pct: u8, // 50% Slash
    },
    /// 故意间隙
    IntentionalGap {
        proof: GapProof,
        penalty_pct: u8, // 30% Slash
    },
    /// 无效批次
    InvalidBatch {
        proof: InvalidBatchProof,
        penalty_pct: u8, // 20% Slash
    },
}
```

### 11.5 安全监控指标 / Security Monitoring Metrics

```rust
// Sequencer 安全相关 Prometheus 指标
lazy_static! {
    /// 审查检测计数
    pub static ref CENSORSHIP_DETECTIONS: IntCounter = register_int_counter!(
        "dex_sequencer_censorship_detections_total",
        "Number of potential censorship events detected"
    ).unwrap();

    /// 双重签名检测计数
    pub static ref EQUIVOCATION_DETECTIONS: IntCounter = register_int_counter!(
        "dex_sequencer_equivocation_detections_total",
        "Number of equivocation events detected"
    ).unwrap();

    /// 序列号间隙计数
    pub static ref GAP_DETECTIONS: IntCounter = register_int_counter!(
        "dex_sequencer_gap_detections_total",
        "Number of sequence gaps detected"
    ).unwrap();

    /// Leader 异常切换计数
    pub static ref ABNORMAL_LEADER_SWITCHES: IntCounter = register_int_counter!(
        "dex_sequencer_abnormal_leader_switches_total",
        "Number of leader switches due to misbehavior"
    ).unwrap();
}
```

---

## 变更历史 / Change History

| 版本 | 日期 | 变更内容 | 状态 |
|-----|------|---------|------|
| v1.0 | 2025-12-31 | 初始版本 | ✅ 有效 |
| v1.1 | 2025-12-31 | 添加 11. 安全不变量与缓解措施 | ✅ 有效 |
| v1.2 | 2025-12-31 | 补充文档元数据 | ✅ 有效 |

### 待对齐事项 / Alignment Notes

| 章节 | 状态 | 说明 |
|-----|------|------|
| 11. 安全不变量 | ✅ 有效 | 与 02-ARCHITECTURE 5.2 互补 |
| 6.2 确认机制 | ✅ 有效 | 引用 ADR-006 确认语义 |
| 8.1 参数矩阵 | ⚠️ 待验证 | 生产环境需性能测试调优 |

---

*文档版本: v1.2 | 最后更新: 2025-12-31*
