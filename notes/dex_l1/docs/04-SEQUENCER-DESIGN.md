# DEX L1 Sequencer 设计 / Sequencer Design

> **版本**: v1.0
> **状态**: Draft
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

*文档版本: v1.0 | 最后更新: 2025-01-01*
