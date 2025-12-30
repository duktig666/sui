# DEX Layer 1 区块链设计方案 (Sui Fork)

## 目标
- **撮合执行延迟**: 50ms
- **吞吐量**: 10万 TPS
- **架构**: 中心化 Sequencer
- **兼容性**: 完全兼容 Sui 生态

---

## 架构概览

```
┌─────────────────────────────────────────────────────────────────┐
│                      DEX L1 Architecture                        │
├─────────────────────────────────────────────────────────────────┤
│  Client Layer                                                   │
│  └── Sui SDK/Wallets ── JSON-RPC ── WebSocket                  │
├─────────────────────────────────────────────────────────────────┤
│  Sequencer Layer (NEW)                                         │
│  └── Order Gateway → Tx Sequencer → Sequence Publisher         │
│      (< 5ms)          (FIFO)         (DA Layer)                │
├─────────────────────────────────────────────────────────────────┤
│  Native DEX Engine (NEW)                                       │
│  └── Order Manager → Matching Engine → Risk Engine             │
│      (Rust Native)    (< 10us/match)   (Margin/Liquidation)    │
├─────────────────────────────────────────────────────────────────┤
│  Modified Sui Execution Layer                                  │
│  └── DEX Precompile → Move VM → Balance Manager               │
│      (Bypass VM)      (Non-DEX)  (Fast Path)                   │
├─────────────────────────────────────────────────────────────────┤
│  Optimized Storage Layer                                       │
│  └── Orderbook State → Balance Cache → RocksDB                │
│      (In-Memory)       (DashMap)       (Persistence)           │
└─────────────────────────────────────────────────────────────────┘
```

---

## 核心设计决策

### 1. 中心化 Sequencer 替代共识

**原因**: Mysticeti 共识延迟 ~600ms，无法达到 50ms 目标

**设计**:
- 单一 Sequencer 进行交易排序（FIFO）
- 热备份 Sequencer 实现故障切换（< 100ms）
- 序列发布到 DA 层确保可审计性

**关键代码位置**:
- 修改: `/crates/sui-core/src/authority.rs` - 添加 DEX 交易路由
- 修改: `/crates/sui-core/src/consensus_adapter.rs` - 集成 Sequencer

### 2. 原生 Rust 撮合引擎

**原因**: Move VM 执行开销太大，无法达到 10万 TPS

**设计**:
- BTreeMap 实现价格优先、时间优先的订单簿
- 内存状态 + WAL 持久化
- DEX Precompile 桥接 Move 接口

**数据结构**:
```rust
pub struct Orderbook {
    bids: BTreeMap<Reverse<Price>, VecDeque<Order>>,  // 买单（降序）
    asks: BTreeMap<Price, VecDeque<Order>>,           // 卖单（升序）
    order_index: HashMap<OrderId, OrderRef>,          // O(1) 查找
}
```

### 3. 双路径执行

| 交易类型 | 执行路径 | 延迟 |
|---------|---------|-----|
| DEX 订单 | Sequencer → Native Engine | < 50ms |
| 存取款 | Sequencer → Move VM | < 100ms |
| 其他交易 | Mysticeti → Move VM | ~600ms |

---

## 新增 Crates

| Crate | 功能 | 优先级 |
|-------|-----|-------|
| `crates/dex-sequencer` | 中心化交易排序 | P0 |
| `crates/dex-engine` | 原生撮合引擎 | P0 |
| `crates/dex-storage` | 内存订单簿存储 | P0 |
| `crates/dex-perpetuals` | 永续合约逻辑 | P1 |
| `crates/dex-framework` | Move 接口包 | P1 |

---

## 需修改的核心文件

### P0 - 核心修改

1. **`/crates/sui-core/src/authority.rs`**
   - 添加 `is_dex_transaction()` 检测
   - 添加 `submit_to_dex_sequencer()` 路由
   - 修改 `handle_transaction()` 分流逻辑

2. **`/sui-execution/latest/sui-adapter/src/execution_engine.rs`**
   - 添加 DEX Precompile 钩子
   - 在 `execute_transaction_to_effects()` 中检测 DEX 调用

3. **`/crates/sui-core/src/consensus_adapter.rs`**
   - 添加 `DexSequencerClient` 集成
   - 实现 Sequencer 故障切换逻辑

### P1 - 配置和存储

4. **`/consensus/config/src/parameters.rs`**
   - 添加 `DexModeConfig` 结构
   - 添加 DEX 模式参数

5. **`/crates/sui-core/src/authority/authority_store_tables.rs`**
   - 添加 `DexPerpetualTables`
   - 订单、持仓、交易历史表

6. **`/crates/sui-node/src/lib.rs`**
   - Sequencer 初始化逻辑
   - DEX 引擎初始化

---

## 永续合约设计

### 资金费率
- 每小时计算一次
- 公式: `Funding Rate = Premium + Clamp(Interest - Premium, -0.05%, 0.05%)`
- 自动应用到所有持仓

### 强制平仓
- 维持保证金率: 0.5%
- 清算惩罚: 1%
- 保险基金兜底

### 保证金管理
- 初始保证金率: 1% (100x 杠杆)
- 支持全仓/逐仓模式

---

## 性能目标

| 指标 | 目标 | 当前 Sui | 提升 |
|-----|-----|---------|-----|
| 撮合延迟 (P99) | **< 50ms** | ~700ms | 14x |
| 订单吞吐量 | **100,000 TPS** | ~2,000 TPS | 50x |
| 订单簿更新 | **< 10ms** | N/A | - |
| 软确认 | **< 100ms** | ~2s | 20x |

---

## 实现阶段

### Phase 1: 核心基础设施
- [ ] 实现 `dex-sequencer` crate
- [ ] 实现 `dex-engine` 核心撮合逻辑
- [ ] 实现 `dex-storage` 内存层
- [ ] 集成到 Sui authority

### Phase 2: Move 集成
- [ ] 创建 DEX Move framework
- [ ] 实现 precompile 桥接
- [ ] 钱包兼容性测试

### Phase 3: 永续合约
- [ ] 资金费率机制
- [ ] 清算引擎
- [ ] 保证金管理

### Phase 4: 生产加固
- [ ] Sequencer 故障切换
- [ ] 性能优化
- [ ] 安全审计准备

---

## 关键技术点

### Sequencer 设计
```rust
pub struct DexSequencer {
    sequence_counter: AtomicU64,           // 全局序列号
    pending_queue: Receiver<SequencedTx>,  // 待处理队列
    standby: Option<StandbySequencer>,     // 热备份
}
```

### DEX Precompile
```rust
pub struct DexPrecompile {
    engine: Arc<RwLock<MatchingEngine>>,
    move_vm: Arc<MoveVM>,
}

// DEX 交易直接调用原生引擎，绕过 Move VM
// 存取款仍通过 Move VM 处理对象
```

### Move 接口
```move
module dex::orderbook {
    public entry fun place_order(...) {
        // 触发 DEX precompile
        native_place_order(...)
    }

    native fun native_place_order(...);
}
```

---

---

## Sequencer 高可用方案（复用验证者网络）

### 设计原则
复用 Sui 现有的验证者网络基础设施，避免引入额外的网络层。

### 架构：轮转 Leader Sequencer

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Sequencer HA Architecture                                │
│                  (Reusing Validator Network)                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                   │
│    │ Validator A │    │ Validator B │    │ Validator C │    ...            │
│    │ (Leader)    │    │ (Standby)   │    │ (Standby)   │                   │
│    │             │    │             │    │             │                   │
│    │ ┌─────────┐ │    │ ┌─────────┐ │    │ ┌─────────┐ │                   │
│    │ │Sequencer│ │    │ │Sequencer│ │    │ │Sequencer│ │                   │
│    │ │ Active  │ │    │ │ Passive │ │    │ │ Passive │ │                   │
│    │ └────┬────┘ │    │ └────┬────┘ │    │ └────┬────┘ │                   │
│    │      │      │    │      │      │    │      │      │                   │
│    │ ┌────┴────┐ │    │ ┌────┴────┐ │    │ ┌────┴────┐ │                   │
│    │ │DEX      │ │    │ │DEX      │ │    │ │DEX      │ │                   │
│    │ │Engine   │ │    │ │Engine   │ │    │ │Engine   │ │                   │
│    │ └─────────┘ │    │ └─────────┘ │    │ └─────────┘ │                   │
│    └──────┬──────┘    └──────┬──────┘    └──────┬──────┘                   │
│           │                  │                  │                           │
│           └──────────────────┼──────────────────┘                           │
│                              │                                              │
│                    ┌─────────┴─────────┐                                    │
│                    │  Existing Sui P2P │                                    │
│                    │  Network (anemo)  │                                    │
│                    └───────────────────┘                                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Leader 选举机制

**复用 Mysticeti DAG 的 Leader 调度**:

```rust
// 复用现有代码: consensus/core/src/leader_schedule.rs

pub struct SequencerLeaderSchedule {
    /// 复用 Mysticeti 的 leader 调度逻辑
    inner: LeaderSchedule,

    /// Sequencer epoch (比共识 epoch 更短，如 1 分钟)
    sequencer_epoch_duration: Duration,
}

impl SequencerLeaderSchedule {
    /// 确定当前 Sequencer Leader
    pub fn current_sequencer_leader(&self, timestamp: u64) -> AuthorityIndex {
        // 基于时间戳的确定性轮转
        let epoch = timestamp / self.sequencer_epoch_duration.as_millis() as u64;
        let committee = self.inner.committee();

        // 按 stake 加权的轮转
        committee.leader_by_epoch(epoch)
    }

    /// 故障切换：跳到下一个 leader
    pub fn next_leader(&self, failed_leader: AuthorityIndex) -> AuthorityIndex {
        self.inner.elect_leader_excluding(failed_leader)
    }
}
```

### 故障检测与切换

```rust
pub struct SequencerFailover {
    /// 心跳超时阈值
    heartbeat_timeout: Duration,  // 50ms

    /// 故障检测窗口
    detection_window: Duration,   // 100ms

    /// 当前 leader 状态
    leader_state: Arc<RwLock<LeaderState>>,
}

impl SequencerFailover {
    /// 故障检测循环 (每个验证者运行)
    pub async fn monitor_leader(&self) {
        loop {
            let leader = self.schedule.current_sequencer_leader(now());

            // 检查心跳
            if !self.received_heartbeat_from(leader, self.heartbeat_timeout).await {
                // 广播故障检测
                self.broadcast_leader_failure(leader).await;

                // 等待 2f+1 确认
                if self.collect_failure_votes(leader).await >= self.quorum() {
                    // 切换到下一个 leader
                    self.switch_to_next_leader(leader).await;
                }
            }

            sleep(self.heartbeat_timeout / 2).await;
        }
    }

    /// 故障切换流程
    async fn switch_to_next_leader(&self, failed: AuthorityIndex) {
        let new_leader = self.schedule.next_leader(failed);

        // 1. 新 leader 从 DA 层获取最后确认的序列号
        let last_seq = self.fetch_last_confirmed_sequence().await;

        // 2. 新 leader 激活 Sequencer
        if self.is_me(new_leader) {
            self.activate_sequencer(last_seq).await;
        }

        // 3. 广播 leader 变更
        self.broadcast_leader_change(new_leader).await;
    }
}
```

### 序列广播与确认

```rust
/// 利用现有 P2P 网络广播序列
pub struct SequenceBroadcaster {
    /// 复用 Sui 的 anemo 网络
    network: Arc<AuthorityNetwork>,

    /// 序列确认收集器
    confirmations: Arc<DashMap<u64, HashSet<AuthorityIndex>>>,
}

impl SequenceBroadcaster {
    /// Leader 广播序列批次
    pub async fn broadcast_sequence_batch(&self, batch: SequenceBatch) {
        // 1. 签名批次
        let signed = self.sign_batch(&batch);

        // 2. 通过现有 P2P 网络广播
        self.network.broadcast(NetworkMessage::SequenceBatch(signed)).await;

        // 3. 写入 DA 层 (异步，不阻塞)
        self.da_layer.write(batch.clone()).await;
    }

    /// Standby 节点接收并确认
    pub async fn handle_sequence_batch(&self, batch: SignedSequenceBatch) {
        // 1. 验证 leader 签名
        self.verify_leader_signature(&batch)?;

        // 2. 本地执行 (确定性重放)
        self.dex_engine.replay_batch(&batch.inner)?;

        // 3. 发送确认给 leader
        self.send_confirmation(batch.sequence_range()).await;
    }
}
```

### 关键修改点

| 文件 | 修改 |
|-----|-----|
| `/consensus/core/src/leader_schedule.rs` | 添加 `SequencerLeaderSchedule` |
| `/crates/sui-network/src/lib.rs` | 添加 Sequencer 消息类型 |
| `/crates/sui-core/src/authority.rs` | 添加故障检测逻辑 |

---

## 多节点执行时序图

### 1. DEX 订单执行时序

```
┌──────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────┐
│  Client  │  │  Validator A │  │  Validator B │  │  Validator C │  │ Fullnode │
│          │  │  (Leader)    │  │  (Standby)   │  │  (Standby)   │  │          │
└────┬─────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └────┬─────┘
     │               │                 │                 │               │
     │ 1. Place Order│                 │                 │               │
     │──────────────>│                 │                 │               │
     │               │                 │                 │               │
     │               │ 2. Assign SeqNo │                 │               │
     │               │ (seq=12345)     │                 │               │
     │               │ ────────────────│                 │               │
     │               │                 │                 │               │
     │               │ 3. Execute in   │                 │               │
     │               │ Native Engine   │                 │               │
     │               │ (< 10us)        │                 │               │
     │               │ ────────────────│                 │               │
     │               │                 │                 │               │
     │   4. Soft ACK │                 │                 │               │
     │<──────────────│                 │                 │               │
     │   (< 50ms)    │                 │                 │               │
     │               │                 │                 │               │
     │               │ 5. Broadcast SequenceBatch (via P2P)              │
     │               │────────────────>│                 │               │
     │               │─────────────────────────────────->│               │
     │               │──────────────────────────────────────────────────>│
     │               │                 │                 │               │
     │               │                 │ 6. Verify &     │ 6. Verify &   │
     │               │                 │ Replay Batch    │ Replay Batch  │
     │               │                 │ ───────────     │ ───────────   │
     │               │                 │                 │               │
     │               │  7. Confirmation│                 │               │
     │               │<────────────────│                 │               │
     │               │<─────────────────────────────────│               │
     │               │                 │                 │               │
     │               │ 8. Got 2f+1     │                 │               │
     │               │ Confirmations   │                 │               │
     │               │ (Hard Finality) │                 │               │
     │               │ ────────────────│                 │               │
     │               │                 │                 │               │
     │               │ 9. Write to DA Layer (async)      │               │
     │               │─────────────────────────────────────────────────> │
     │               │                 │                 │               │
     │               │                 │                 │  10. Fullnode │
     │               │                 │                 │  Sync & Apply │
     │               │                 │                 │<──────────────│
     │               │                 │                 │               │
     ▼               ▼                 ▼                 ▼               ▼

Timeline:
  0ms ─── 5ms ─── 10ms ─── 50ms ─── 100ms ─── 200ms ─── 500ms
  │       │       │        │        │         │         │
  │       │       │        │        │         │         └─ DA Write Complete
  │       │       │        │        │         └─ Fullnode Synced
  │       │       │        │        └─ Hard Finality (2f+1)
  │       │       │        └─ Soft ACK to Client
  │       │       └─ Matching Complete
  │       └─ Sequence Assigned
  └─ Order Received
```

### 2. Move VM 交易执行时序（非 DEX）

```
┌──────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────┐
│  Client  │  │  Validator A │  │  Validator B │  │  Validator C │  │ Fullnode │
│          │  │              │  │              │  │              │  │          │
└────┬─────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └────┬─────┘
     │               │                 │                 │               │
     │ 1. Submit Tx  │                 │                 │               │
     │ (Shared Obj)  │                 │                 │               │
     │──────────────>│                 │                 │               │
     │               │                 │                 │               │
     │               │ 2. Forward to Mysticeti Consensus │               │
     │               │────────────────>│                 │               │
     │               │─────────────────────────────────->│               │
     │               │                 │                 │               │
     │               │     ┌───────────────────────────────────┐         │
     │               │     │  3. Mysticeti DAG Consensus       │         │
     │               │     │  - Propose Block (Round N)        │         │
     │               │     │  - Vote (Round N+1)               │         │
     │               │     │  - Decide (Round N+2)             │         │
     │               │     │  (~400-600ms)                     │         │
     │               │     └───────────────────────────────────┘         │
     │               │                 │                 │               │
     │               │ 4. Consensus Output: Tx Ordered   │               │
     │               │<────────────────────────────────->│               │
     │               │                 │                 │               │
     │               │ 5. Execute in   │ 5. Execute in   │ 5. Execute in │
     │               │ Move VM         │ Move VM         │ Move VM       │
     │               │ (Parallel)      │ (Parallel)      │ (Parallel)    │
     │               │ ───────────     │ ───────────     │ ───────────   │
     │               │                 │                 │               │
     │               │ 6. Sign Effects │ 6. Sign Effects │ 6. Sign Effects
     │               │ ───────────     │ ───────────     │ ───────────   │
     │               │                 │                 │               │
     │               │ 7. Collect 2f+1 Effect Signatures │               │
     │               │<────────────────────────────────->│               │
     │               │                 │                 │               │
     │ 8. Tx Finalized                 │                 │               │
     │<──────────────│                 │                 │               │
     │ (~700ms)      │                 │                 │               │
     │               │                 │                 │               │
     │               │ 9. Checkpoint   │ 9. Checkpoint   │ 9. Checkpoint │
     │               │ Sync            │ Sync            │ Sync          │
     │               │ ───────────────>│<───────────────>│<─────────────>│
     │               │                 │                 │               │
     │               │                 │                 │  10. Fullnode │
     │               │                 │                 │  Checkpoint   │
     │               │                 │                 │  Sync         │
     │               │                 │                 │<──────────────│
     │               │                 │                 │               │
     ▼               ▼                 ▼                 ▼               ▼

Timeline:
  0ms ─── 50ms ─── 200ms ─── 400ms ─── 600ms ─── 800ms ─── 2000ms
  │       │        │         │         │         │         │
  │       │        │         │         │         │         └─ Checkpoint Complete
  │       │        │         │         │         └─ Effects Certified
  │       │        │         │         └─ Consensus Decided
  │       │        │         └─ Voting Complete
  │       │        └─ Block Proposed
  │       └─ Tx in Mempool
  └─ Tx Received
```

### 3. DEX 存取款时序（混合路径）

```
┌──────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│  Client  │  │  Validator A │  │  Validator B │  │  Validator C │
│          │  │  (Seq Leader)│  │              │  │              │
└────┬─────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘
     │               │                 │                 │
     │ 1. Deposit Tx │                 │                 │
     │ (需要操作Coin)│                 │                 │
     │──────────────>│                 │                 │
     │               │                 │                 │
     │               │ 2. Route to     │                 │
     │               │ Sequencer       │                 │
     │               │ (DEX Tx检测)    │                 │
     │               │ ────────────────│                 │
     │               │                 │                 │
     │               │ 3. Assign SeqNo │                 │
     │               │ & Broadcast     │                 │
     │               │────────────────>│                 │
     │               │─────────────────────────────────->│
     │               │                 │                 │
     │               │ 4. Execute Deposit in Move VM     │
     │               │ (Transfer Coin to DEX Custody)    │
     │               │ ───────────     │ ───────────     │
     │               │                 │                 │
     │               │ 5. Credit Balance in Native Engine│
     │               │ ───────────     │ ───────────     │
     │               │                 │                 │
     │  6. Deposit   │                 │                 │
     │  Confirmed    │                 │                 │
     │<──────────────│                 │                 │
     │  (< 100ms)    │                 │                 │
     │               │                 │                 │
     ▼               ▼                 ▼                 ▼

Note: 存取款需要 Move VM 处理 Coin 对象，但仍走 Sequencer 路径
      以保证与 DEX 订单的顺序一致性
```

### 4. Sequencer 故障切换时序

```
┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌───────────┐
│  Validator A │  │  Validator B │  │  Validator C │  │ DA Layer  │
│  (Leader)    │  │  (Standby)   │  │  (Standby)   │  │           │
└──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └─────┬─────┘
       │                 │                 │                 │
       │ ─────────────── │                 │                 │
       │ │ 1. Leader    ││                 │                 │
       │ │ Crashes!     ││                 │                 │
       │ ─────────────── │                 │                 │
       │        X        │                 │                 │
       │                 │                 │                 │
       │                 │ 2. Heartbeat    │                 │
       │                 │ Timeout (50ms)  │                 │
       │                 │ ────────────────│                 │
       │                 │                 │ 2. Heartbeat    │
       │                 │                 │ Timeout (50ms)  │
       │                 │                 │ ────────────────│
       │                 │                 │                 │
       │                 │ 3. Broadcast Failure Detection    │
       │                 │<───────────────>│                 │
       │                 │                 │                 │
       │                 │ 4. Collect 2f+1 │                 │
       │                 │ Failure Votes   │                 │
       │                 │ ────────────────│                 │
       │                 │                 │                 │
       │                 │ 5. Fetch Last Confirmed Seq       │
       │                 │─────────────────────────────────->│
       │                 │<──────────────────────────────────│
       │                 │ (seq = 12345)   │                 │
       │                 │                 │                 │
       │                 │ 6. B Becomes    │                 │
       │                 │ New Leader      │                 │
       │                 │ (Resume from    │                 │
       │                 │  seq 12346)     │                 │
       │                 │ ────────────────│                 │
       │                 │                 │                 │
       │                 │ 7. Broadcast Leader Change        │
       │                 │────────────────>│                 │
       │                 │                 │                 │
       │                 │ 8. Continue     │ 8. Redirect     │
       │                 │ Processing      │ to New Leader   │
       │                 │ Orders          │ ────────────────│
       │                 │                 │                 │
       ▼                 ▼                 ▼                 ▼

Failover Timeline:
  0ms ─── 50ms ─── 80ms ─── 100ms
  │       │        │        │
  │       │        │        └─ New Leader Active
  │       │        └─ 2f+1 Votes Collected
  │       └─ Heartbeat Timeout Detected
  └─ Leader Crash

  Total Failover Time: < 100ms
```

---

## 风险和缓解

| 风险 | 缓解措施 |
|-----|---------|
| Sequencer 单点故障 | 复用验证者网络的轮转 Leader + 50ms 心跳检测 + 100ms 故障切换 |
| Leader 切换数据丢失 | DA 层持久化 + 从最后确认序列恢复 |
| 数据一致性 | WAL + 定期快照 + 确定性重放 |
| Move 兼容性 | Precompile 桥接，保留完整 Move 接口 |
| 安全性 | 原生引擎需严格测试和审计 |
| 网络分区 | 2f+1 确认机制 + DA 层作为最终仲裁 |
