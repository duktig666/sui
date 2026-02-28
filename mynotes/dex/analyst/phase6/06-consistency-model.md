# 06 - 一致性模型 — 乐观状态与最终确认

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: ⚠️ 需更新 — 对账机制从 Checkpoint 改为 gRPC GetSnapshot
> 前置依赖: [01-streaming-source.md](./01-streaming-source.md), [04-offchain-orderbook.md](./04-offchain-orderbook.md)

> **2026-02-27 架构决策变更通知**
>
> 根据 [08-architecture-qa.md](./08-architecture-qa.md) 确认的决策：
> - **Q3=C+**: Checkpoint 不再发送订单簿事件，不再有 Checkpoint 订单簿快照可用于对账
> - 对账机制改为：dex-streamer 周期性调用 gRPC `GetSnapshot()` 从 InlineOrderbook 获取快照，与内存 L2Book 比对
> - 恢复机制：gRPC GetSnapshot 替代 Checkpoint 快照作为权威数据源
>
> **本文档需更新的部分**：
> - §1 双通道一致性模型 → 订单簿数据不再通过 Checkpoint 通道，权威数据源改为 InlineOrderbook
> - 对账流程 → 从 ~~Redis dex:orderbook~~ 改为 gRPC GetSnapshot
> - 恢复流程 → 从 ~~Checkpoint 快照~~ 改为 gRPC GetSnapshot

## 1. 双通道一致性模型

Phase 6 引入 gRPC 快速通道：低延迟 gRPC Stream 通道（<50ms）用于订单簿实时更新。Checkpoint 通道（1-3s）继续用于 fills/orders/positions 持久化，但**不再处理订单簿事件**。订单簿的权威数据源为 InlineOrderbook（引擎内存订单簿），通过 gRPC GetSnapshot() 访问。

```
                    DEX 引擎执行完成
                         │
            ┌────────────┴────────────┐
            │                         │
            ▼                         ▼
   Stream 通道 (<50ms)       Checkpoint 通道 (1-3s)
   DexStreamingManager       Checkpoint pipeline
            │                         │
            ▼                         ▼
   dex-streamer               dex-indexer
            │                         │
            ▼                         ▼
   dex:l2book:{id}            dex:orderbook:{id}
   (乐观状态)                  (最终状态)
            │                         │
            └────────────┬────────────┘
                         │
                    Reconciler
                    (定期对账)
```

### 1.1 状态分类

| 状态类型 | 数据源 | 延迟 | 可靠性 | Redis 位置 | 更新方式 |
|---------|--------|------|--------|-----------|---------|
| 乐观状态 (Optimistic) | Stream channel | <50ms | Best-effort | `dex:l2book:{id}` | 增量 delta |
| 最终状态 (Final) | Checkpoint channel | 1-3s | 保证不丢 | `dex:orderbook:{id}` | 全量快照 |

**API 读取优先级**：

- REST `l2Book` 端点：优先读取 `dex:l2book:{id}`（低延迟），若不存在则 fallback 到 `dex:orderbook:{id}`（可靠）
- WS `l2BookDelta` 订阅：从 `dex:l2book:{id}` 推送增量更新
- WS `l2Book` 订阅：从 `dex:orderbook:{id}` 推送全量快照（每次 checkpoint 更新）

### 1.2 与 dYdX 的关键区别

dYdX v4 使用 OffChainUpdates（Vulcan）+ OnChainUpdates（Ender）双通道，两者的本质差异如下：

| 维度 | dYdX v4 | 本项目 |
|------|---------|--------|
| 低延迟通道触发时机 | CheckTx（交易验证，尚未执行） | post_process_one_tx（执行完成后） |
| 数据性质 | **乐观预测** — 区块执行可能产生不同结果 | **确定性事件** — 执行已完成，结果不会改变 |
| 回滚场景 | 需要处理 — CheckTx 与 FinalizeBlock 不一致 | 不存在 — 事件是 post-execution 产物 |
| 低延迟通道名称 | OffChainUpdates | Stream events |
| 可靠通道名称 | OnChainUpdates | Checkpoint events |

**核心推论**：

由于本项目的 Stream 事件在 `AuthorityState` 执行后触发（见 `01-streaming-source.md` 第 4 节），这些事件是**确定性的** — 执行已完成，效果（effects）已生成，结果不会改变。因此：

1. **不存在 dYdX 式的"乐观回滚"场景**：Stream 事件与 Checkpoint 事件描述的是同一个确定性执行结果
2. **不一致只可能来自**：事件丢失（channel overflow / consumer lag）、处理错误（BCS 反序列化失败）、时序差异（Stream 先到，Checkpoint 后到）
3. **一致性问题比 dYdX 简单得多**：不需要 dYdX 的 OrderRemove + OrderPlace 回滚重放机制

---

## 2. 不一致来源分析

既然不存在语义不一致（两个通道描述的是同一个执行结果），那么不一致只能来自传输层和处理层的问题。

### 2.1 不一致来源矩阵

| # | 来源 | 发生可能性 | 影响范围 | 影响程度 | 检测方式 | 恢复方式 |
|---|------|-----------|---------|---------|---------|---------|
| 1 | **Channel overflow** — broadcast channel 满，旧消息被覆盖 | 中等（高峰期） | 部分 delta 丢失，L2 book 不准 | 单个市场 | Sequence gap detection | Snapshot recovery |
| 2 | **dex-streamer 重启** — 进程崩溃或部署更新 | 确定发生 | L2 book 内存状态全部丢失 | 所有市场 | Startup detection | Load from checkpoint snapshot |
| 3 | **BCS 反序列化错误** — 事件结构体版本不匹配 | 极低 | 单个事件丢失 | 单个事件 | Error logging + metric | 下一个 delta 自然覆盖 |
| 4 | **Redis 写入失败** — Redis 连接断开或内存满 | 低 | L2 book 未更新 | 所有市场 | Redis error handling | 重连后 snapshot recovery |
| 5 | **时序差异** — Stream 先到，Checkpoint 后到 | 常见（正常行为） | Stream 和 Checkpoint 短暂不一致 | 正常 | 无需检测 | 无需处理 |
| 6 | **增量累积误差** — 浮点或舍入导致的微小偏差 | 极低 | L2 book 数量微偏 | 单个档位 | Reconciliation diff | 定期对账修正 |

### 2.2 关键观察

**来源 1（Channel overflow）是最常见的不一致来源**。`tokio::broadcast` 当所有 receiver 都落后于 sender 超过 channel 容量时，最旧的消息被自动覆盖。receiver 收到 `RecvError::Lagged(n)` 表示跳过了 n 条消息。

当前 channel 容量设计（见 `01-streaming-source.md` 第 3.3 节）：

```
容量: 10000 批次
高峰 100 tx/s → 缓冲 ~100 秒
```

在正常运行中，dex-streamer 的消费速度（BCS 反序列化 + Redis 写入 ~1ms/batch）远快于生产速度，overflow 主要发生在：
- dex-streamer 启动阶段（连接 Redis、初始化数据结构）
- Redis 短暂不可用导致写入阻塞
- GC pause 或系统负载异常

---

## 3. 序列号机制

序列号是 gap detection 的基础。每个永续合约市场维护独立的单调递增序列计数器。

### 3.1 OrderbookDeltaEvent 序列号

```rust
/// 订单簿增量事件（Phase 6 新增）
/// 由 DEX 引擎在订单簿变更时生成
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookDeltaEvent {
    /// 永续合约 ID
    pub perpetual_id: u32,
    /// 本市场的序列号（单调递增，从 1 开始）
    pub sequence: u64,
    /// 变更的价格档位列表
    pub updates: Vec<OrderbookDelta>,
    /// 事件时间戳（毫秒）
    pub timestamp_ms: u64,
}

/// 单个价格档位的变更描述（与 02-event-design.md 定义一致）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookDelta {
    /// 方向: 0 = Bid (买), 1 = Ask (卖)
    pub side: u8,
    /// 价格（u64 定点数）
    pub price: u64,
    /// 新数量（u64 定点数，0 表示删除该档位）
    pub quantity: u64,
}
```

### 3.2 序列号生成

序列号在 DEX 引擎内部生成，每个 `PerpetualState` 维护独立计数器（与 `02-event-design.md` 第 4 节一致）：

```rust
// sui-execution/src/dex/state/perpetual.rs（概念设计）
pub struct PerpetualState {
    pub perpetual_id: u32,
    // ... 其他字段 ...

    /// 订单簿 delta 序列号计数器
    /// 每次订单簿变更时递增
    delta_sequence: u64,
}

impl PerpetualState {
    /// 生成下一个 delta 序列号
    fn next_delta_sequence(&mut self) -> u64 {
        self.delta_sequence += 1;
        self.delta_sequence
    }

    /// 生成 OrderbookDeltaEvent
    pub fn emit_delta(&mut self, updates: Vec<OrderbookDelta>, timestamp_ms: u64) -> OrderbookDeltaEvent {
        OrderbookDeltaEvent {
            perpetual_id: self.perpetual_id,
            sequence: self.next_delta_sequence(),
            updates,
            timestamp_ms,
        }
    }
}
```

### 3.3 Gap Detection

dex-streamer 的 `OrderbookBuilder` 为每个市场维护期望的下一个序列号：

```rust
/// dex-streamer 中的订单簿构建器
pub struct OrderbookBuilder {
    /// 每个市场的当前序列号
    sequences: HashMap<u32, u64>,
    /// 每个市场的 L2 Book 状态
    books: HashMap<u32, L2Book>,
}

/// Delta 处理结果
pub enum DeltaResult {
    /// 正常应用
    Applied,
    /// 检测到序列号间隙（事件丢失）
    GapDetected { expected: u64, actual: u64 },
    /// 重复或旧事件，已跳过
    Skipped,
}

impl OrderbookBuilder {
    /// 应用增量 delta，带序列号检测
    pub fn apply_delta(&mut self, delta: &OrderbookDeltaEvent) -> DeltaResult {
        let expected = self.sequences
            .get(&delta.perpetual_id)
            .copied()
            .unwrap_or(0) + 1;

        if delta.sequence == expected {
            // 正常情况：按序到达，应用 delta
            self.apply_delta_inner(delta);
            self.sequences.insert(delta.perpetual_id, delta.sequence);

            metrics::counter!("dex_streamer_deltas_applied_total",
                "perpetual_id" => delta.perpetual_id.to_string())
                .increment(1);

            DeltaResult::Applied
        } else if delta.sequence > expected {
            // 间隙：中间有事件丢失
            let gap_size = delta.sequence - expected;
            warn!(
                perpetual_id = delta.perpetual_id,
                expected = expected,
                actual = delta.sequence,
                gap_size = gap_size,
                "Sequence gap detected, requesting snapshot recovery"
            );

            metrics::counter!("dex_streamer_sequence_gaps_total",
                "perpetual_id" => delta.perpetual_id.to_string())
                .increment(1);

            // 触发快照恢复
            self.request_snapshot_recovery(delta.perpetual_id);

            DeltaResult::GapDetected { expected, actual: delta.sequence }
        } else {
            // 旧事件或重复：直接跳过
            debug!(
                perpetual_id = delta.perpetual_id,
                expected = expected,
                actual = delta.sequence,
                "Duplicate or old delta, skipping"
            );
            DeltaResult::Skipped
        }
    }

    /// 内部方法：将 delta 应用到 L2 Book
    fn apply_delta_inner(&mut self, delta: &OrderbookDeltaEvent) {
        let book = self.books
            .entry(delta.perpetual_id)
            .or_insert_with(L2Book::new);

        for update in &delta.updates {
            let side = if update.side == 0 { Side::Bid } else { Side::Ask };
            if update.quantity == 0 {
                // 数量为 0 表示删除该档位
                book.remove_level(side, update.price);
            } else {
                // 更新或插入档位
                book.update_level(side, update.price, update.quantity);
            }
        }
    }

    /// 请求快照恢复
    fn request_snapshot_recovery(&mut self, perpetual_id: u32) {
        // 标记该市场需要恢复
        // 恢复逻辑见第 4 节
        if let Some(book) = self.books.get_mut(&perpetual_id) {
            book.mark_stale();
        }
    }
}
```

### 3.4 序列号设计要点

| 要点 | 说明 |
|------|------|
| 每市场独立 | `perpetual_id` 为 key，不同市场互不影响 |
| 从 1 开始 | `delta_sequence` 初始为 0，首个 delta 序列号为 1 |
| 单调递增 | 同一市场内严格 +1，不存在跳号（除非事件丢失） |
| 引擎内部生成 | 序列号在 DEX 引擎内生成，不受共识或网络影响 |
| 不持久化 | 序列号在引擎内存中维护，节点重启后从 0 开始（见第 4.4 节） |

---

## 4. 恢复策略

### 4.1 Snapshot Recovery（快照恢复）

当检测到序列号间隙或 dex-streamer 启动时，需要从权威数据源恢复 L2 Book 状态。

**恢复流程**：

```
检测到间隙 / dex-streamer 启动
    │
    ├─ [1] 标记市场为 stale（停止增量更新 Redis）
    │
    ├─ [2] 从 Redis 读取 checkpoint 快照
    │      HMGET dex:orderbook:{perpetual_id} bids asks timestamp_ms
    │      │
    │      ├─ 快照存在 → 用快照初始化 L2 Book
    │      │
    │      └─ 快照不存在 → 等待下一个 OrderbookSnapshotEvent（从 Stream 通道）
    │
    ├─ [3] 重置序列号（接受下一个 delta 的序列号作为新基准）
    │
    └─ [4] 标记市场为 active（恢复增量更新 Redis）
```

```rust
impl OrderbookBuilder {
    /// 从 checkpoint 快照恢复指定市场
    pub async fn recover_from_snapshot(
        &mut self,
        perpetual_id: u32,
        redis: &mut redis::aio::MultiplexedConnection,
    ) -> Result<(), RecoveryError> {
        let start = Instant::now();

        // 从 dex-indexer 写入的 checkpoint 快照读取（HSET 格式，含 bids/asks JSON 字段）
        let key = format!("dex:orderbook:{}", perpetual_id);
        let (bids_json, asks_json): (Option<String>, Option<String>) =
            redis::cmd("HMGET")
                .arg(&key)
                .arg("bids")
                .arg("asks")
                .query_async(redis)
                .await?;

        match (bids_json, asks_json) {
            (Some(bids), Some(asks)) => {
                let book = L2Book::from_checkpoint_json(&bids, &asks);

                // 用快照初始化本地 L2 Book
                self.books.insert(perpetual_id, book.clone());

                // 重置序列号：接受下一个到达的 delta 序列号
                // 设为 0 表示"未知"，下一个 delta 的序列号将成为新基准
                self.sequences.remove(&perpetual_id);

                // 同步写入增量通道的 Redis key（HSET 格式，b:{price}/a:{price} 字段）
                let l2_key = format!("dex:l2book:{}", perpetual_id);
                // 先清空旧数据，再按 field 写入
                redis::cmd("DEL").arg(&l2_key).query_async::<_, ()>(redis).await?;
                let mut pipe = redis::pipe();
                for (&price, &qty) in &book.bids {
                    pipe.cmd("HSET").arg(&l2_key).arg(format!("b:{}", price)).arg(qty);
                }
                for (&price, &qty) in &book.asks {
                    pipe.cmd("HSET").arg(&l2_key).arg(format!("a:{}", price)).arg(qty);
                }
                pipe.query_async::<_, ()>(redis).await?;

                let duration = start.elapsed();
                info!(
                    perpetual_id,
                    duration_ms = duration.as_millis() as u64,
                    "Snapshot recovery completed"
                );

                metrics::histogram!("dex_streamer_recovery_duration_seconds")
                    .record(duration.as_secs_f64());

                Ok(())
            }
            _ => {
                warn!(
                    perpetual_id,
                    "No checkpoint snapshot available, waiting for stream snapshot"
                );
                // 标记为等待快照状态
                // 当收到 OrderbookSnapshotEvent 时自动初始化
                self.books.insert(perpetual_id, L2Book::new_awaiting_snapshot());
                Err(RecoveryError::NoSnapshotAvailable)
            }
        }
    }
}
```

### 4.2 Periodic Reconciliation（定期对账）

即使没有检测到序列号间隙，也应定期对账以发现潜在的累积偏差。

**对账流程**（每 30 秒，每个市场独立）：

```
Reconciler 定时触发（30s interval）
    │
    ├─ [1] 读取 checkpoint 快照: HMGET dex:orderbook:{perpetual_id} bids asks
    │
    ├─ [2] 获取 stream L2 book: 从 OrderbookBuilder 内存读取
    │
    ├─ [3] 比较 top N 档位 (N=20，双边)
    │      │
    │      ├─ 一致 → emit metric: dex_streamer_reconciliation_ok_total
    │      │         无需操作
    │      │
    │      └─ 不一致 → 进入修正流程
    │
    └─ [4] 修正流程:
           ├─ Log drift 详情（哪些档位不同，偏差多少）
           ├─ 用 checkpoint 快照替换 stream L2 book（内存）
           ├─ DEL dex:l2book:{perpetual_id} + HSET 重建（b:{price}/a:{price} 字段）
           ├─ 重置本地 OrderbookBuilder 状态
           └─ emit metric: dex_streamer_reconciliation_corrections_total
```

```rust
/// 对账器
pub struct Reconciler {
    /// 对账间隔
    interval: Duration,
    /// 比较的档位深度
    compare_depth: usize,
}

impl Reconciler {
    pub fn new() -> Self {
        Self {
            interval: Duration::from_secs(30),
            compare_depth: 20,  // top 20 档位（双边共 40 档）
        }
    }

    /// 对账单个市场
    pub async fn reconcile_market(
        &self,
        perpetual_id: u32,
        builder: &mut OrderbookBuilder,
        redis: &mut redis::aio::MultiplexedConnection,
    ) -> ReconcileResult {
        // 1. 读取 checkpoint 快照（ground truth，HSET 格式含 bids/asks JSON 字段）
        let checkpoint_key = format!("dex:orderbook:{}", perpetual_id);
        let (bids_json, asks_json): (Option<String>, Option<String>) =
            match redis::cmd("HMGET")
                .arg(&checkpoint_key)
                .arg("bids")
                .arg("asks")
                .query_async(redis)
                .await
            {
                Ok(data) => data,
                Err(e) => {
                    warn!(perpetual_id, error = %e, "Failed to read checkpoint snapshot");
                    return ReconcileResult::Error;
                }
            };

        let (Some(bids_json), Some(asks_json)) = (bids_json, asks_json) else {
            // Checkpoint 快照不存在（市场可能刚创建，indexer 尚未处理）
            return ReconcileResult::NoCheckpoint;
        };

        let checkpoint_book = match L2BookSnapshot::from_json(&bids_json, &asks_json) {
            Ok(b) => b,
            Err(e) => {
                error!(perpetual_id, error = %e, "Failed to parse checkpoint snapshot");
                return ReconcileResult::Error;
            }
        };

        // 2. 获取 stream L2 book 状态
        let Some(stream_book) = builder.books.get(&perpetual_id) else {
            // Stream book 不存在（可能正在等待初始化）
            return ReconcileResult::NoStreamBook;
        };

        // 3. 比较 top N 档位
        let drifts = self.compare_books(
            &checkpoint_book,
            stream_book,
            self.compare_depth,
        );

        if drifts.is_empty() {
            // 一致
            metrics::counter!("dex_streamer_reconciliation_ok_total",
                "perpetual_id" => perpetual_id.to_string())
                .increment(1);
            ReconcileResult::Consistent
        } else {
            // 不一致：记录并修正
            warn!(
                perpetual_id,
                drift_count = drifts.len(),
                "Reconciliation drift detected, correcting"
            );

            for drift in &drifts {
                info!(
                    perpetual_id,
                    side = ?drift.side,
                    price = drift.price,
                    checkpoint_qty = drift.checkpoint_quantity,
                    stream_qty = drift.stream_quantity,
                    "Drift detail"
                );
            }

            // 用 checkpoint 快照替换 stream book
            let corrected_book = L2Book::from_snapshot(&checkpoint_book);
            builder.books.insert(perpetual_id, corrected_book.clone());

            // 更新 Redis 中的 stream L2 book（DEL + HSET 重建 b:/a: 字段）
            let l2_key = format!("dex:l2book:{}", perpetual_id);
            let mut pipe = redis::pipe();
            pipe.cmd("DEL").arg(&l2_key);
            for (&price, &qty) in &corrected_book.bids {
                pipe.cmd("HSET").arg(&l2_key).arg(format!("b:{}", price)).arg(qty);
            }
            for (&price, &qty) in &corrected_book.asks {
                pipe.cmd("HSET").arg(&l2_key).arg(format!("a:{}", price)).arg(qty);
            }
            let _ = pipe.query_async::<_, ()>(redis).await;

            metrics::counter!("dex_streamer_reconciliation_corrections_total",
                "perpetual_id" => perpetual_id.to_string())
                .increment(1);

            ReconcileResult::Corrected { drift_count: drifts.len() }
        }
    }

    /// 比较两个订单簿的 top N 档位
    fn compare_books(
        &self,
        checkpoint: &L2BookSnapshot,
        stream: &L2Book,
        depth: usize,
    ) -> Vec<DriftEntry> {
        let mut drifts = Vec::new();

        // 比较 bids (买方) top N
        let checkpoint_bids = &checkpoint.bids[..depth.min(checkpoint.bids.len())];
        let stream_bids = stream.top_bids(depth);
        Self::compare_side(Side::Bid, checkpoint_bids, &stream_bids, &mut drifts);

        // 比较 asks (卖方) top N
        let checkpoint_asks = &checkpoint.asks[..depth.min(checkpoint.asks.len())];
        let stream_asks = stream.top_asks(depth);
        Self::compare_side(Side::Ask, checkpoint_asks, &stream_asks, &mut drifts);

        drifts
    }

    fn compare_side(
        side: Side,
        checkpoint_levels: &[(u64, u64)],
        stream_levels: &[(u64, u64)],
        drifts: &mut Vec<DriftEntry>,
    ) {
        // 构建 price → quantity 映射
        let checkpoint_map: HashMap<u64, u64> = checkpoint_levels.iter().copied().collect();
        let stream_map: HashMap<u64, u64> = stream_levels.iter().copied().collect();

        // 检查 checkpoint 中存在但 stream 中缺失或不同的档位
        for (&price, &checkpoint_qty) in &checkpoint_map {
            let stream_qty = stream_map.get(&price).copied().unwrap_or(0);
            if checkpoint_qty != stream_qty {
                drifts.push(DriftEntry {
                    side,
                    price,
                    checkpoint_quantity: checkpoint_qty,
                    stream_quantity: stream_qty,
                });
            }
        }

        // 检查 stream 中存在但 checkpoint 中不存在的档位
        for (&price, &stream_qty) in &stream_map {
            if !checkpoint_map.contains_key(&price) {
                drifts.push(DriftEntry {
                    side,
                    price,
                    checkpoint_quantity: 0,
                    stream_quantity: stream_qty,
                });
            }
        }
    }
}

#[derive(Debug)]
pub struct DriftEntry {
    pub side: Side,
    pub price: u64,
    pub checkpoint_quantity: u64,
    pub stream_quantity: u64,
}

pub enum ReconcileResult {
    Consistent,
    Corrected { drift_count: usize },
    NoCheckpoint,
    NoStreamBook,
    Error,
}
```

### 4.3 Recovery Time Targets（恢复时间目标）

| 场景 | 恢复时间 | 恢复方法 | 自动/手动 |
|------|---------|---------|----------|
| 单个 delta 丢失 | <100ms | Gap detection → snapshot recovery | 自动 |
| 多个连续 delta 丢失 | <100ms | 同上（与单个 delta 丢失处理相同） | 自动 |
| dex-streamer 重启 | <5s | 启动时从 Redis checkpoint snapshot 加载 | 自动 |
| Redis 重启 | <10s | 等待下一个 checkpoint 写入 + snapshot recovery | 自动 |
| 长时间 stream 中断 | 持续 | 客户端自动 fallback 到 checkpoint-based l2Book | 自动 |
| dex-indexer 落后 | N/A | Stream 不受影响；reconciliation 使用旧 checkpoint | 自动 |

### 4.4 节点重启时的序列号处理

DEX 引擎的 `delta_sequence` 在内存中维护，节点重启后从 0 重新开始。这会导致 dex-streamer 的序列号检测失效（新的 sequence=1 小于之前记录的 sequence=N）。

**处理方案**：

```rust
impl OrderbookBuilder {
    /// 处理节点重启导致的序列号重置
    fn handle_potential_node_restart(&mut self, delta: &OrderbookDeltaEvent) -> bool {
        let current = self.sequences.get(&delta.perpetual_id).copied().unwrap_or(0);

        // 启发式判断：如果新序列号远小于当前值，且为小数（1-10），
        // 很可能是节点重启
        if current > 100 && delta.sequence <= 10 {
            info!(
                perpetual_id = delta.perpetual_id,
                old_sequence = current,
                new_sequence = delta.sequence,
                "Detected likely node restart, resetting sequence tracking"
            );

            // 重置序列号，触发 snapshot recovery
            self.sequences.remove(&delta.perpetual_id);
            self.request_snapshot_recovery(delta.perpetual_id);
            return true;
        }

        false
    }
}
```

---

## 5. 客户端一致性

### 5.1 WS 客户端 L2 Book 同步协议

客户端通过 WebSocket 订阅 `l2BookDelta` 后的同步流程：

```
客户端                            dex-api (WS)
  │                                  │
  │ ── subscribe l2BookDelta ──────► │
  │                                  │
  │ ◄── snapshot (seq=S) ────────── │  ← 首次连接返回全量快照 + 当前序列号
  │                                  │
  │ ◄── delta (seq=S+1) ─────────── │  ← 后续增量推送
  │ ◄── delta (seq=S+2) ─────────── │
  │ ◄── delta (seq=S+3) ─────────── │
  │      ...                         │
  │                                  │
  │     [gap: S+5 missing]          │  ← 网络抖动导致消息丢失
  │                                  │
  │ ◄── delta (seq=S+6) ─────────── │
  │                                  │
  │     客户端检测到 gap             │
  │                                  │
  │ ── unsubscribe ────────────────► │  ← 方案 A: 重新订阅
  │ ── subscribe l2BookDelta ──────► │
  │                                  │
  │ ◄── snapshot (seq=S+8) ──────── │  ← 获取新的全量快照
  │ ◄── delta (seq=S+9) ─────────── │
  │      ...                         │
```

**客户端 gap 检测规则**：

```typescript
// 客户端伪代码
class L2BookClient {
    private expectedSeq: number = 0;
    private stale: boolean = false;

    onSnapshot(snapshot: L2BookSnapshot) {
        this.book = snapshot.data;
        this.expectedSeq = snapshot.sequence;
        this.stale = false;
    }

    onDelta(delta: L2BookDelta) {
        if (delta.sequence === this.expectedSeq + 1) {
            // 正常：按序到达
            this.applyDelta(delta);
            this.expectedSeq = delta.sequence;
        } else if (delta.sequence > this.expectedSeq + 1) {
            // Gap 检测：中间有消息丢失
            console.warn(`Sequence gap: expected ${this.expectedSeq + 1}, got ${delta.sequence}`);
            this.requestResync();
        }
        // delta.sequence <= expectedSeq: 旧消息，忽略
    }

    requestResync() {
        // 方案 A: 重新订阅（简单可靠）
        this.ws.send({ method: "unsubscribe", subscription: { type: "l2BookDelta" } });
        this.ws.send({ method: "subscribe", subscription: { type: "l2BookDelta", coin: "BTC-USDC" } });

        // 方案 B: 标记 stale（不中断流，等待下一个服务端推送的快照）
        // this.stale = true;
    }
}
```

### 5.2 Dual Subscription 模式（高可靠客户端）

对于交易机器人等需要高可靠性的客户端，建议同时订阅两个频道：

| 订阅 | 数据源 | 用途 | 更新频率 |
|------|--------|------|---------|
| `l2BookDelta` | Stream channel | 主数据源，低延迟 | 每次变更 |
| `l2Book` | Checkpoint channel | 校验源，定期确认 | 每个 checkpoint (~1-3s) |

```typescript
// 高可靠客户端伪代码
class ReliableL2BookClient {
    private streamBook: L2Book;       // 来自 l2BookDelta（低延迟）
    private checkpointBook: L2Book;   // 来自 l2Book（可靠）

    // 交易决策使用 streamBook（低延迟）
    getBestBid(): Price { return this.streamBook.bestBid; }
    getBestAsk(): Price { return this.streamBook.bestAsk; }

    // 定期用 checkpointBook 校验 streamBook
    onCheckpointUpdate(snapshot: L2BookSnapshot) {
        this.checkpointBook = snapshot;

        // 比较 top 5 档位
        if (!this.isConsistent(this.streamBook, this.checkpointBook, 5)) {
            console.warn("Stream/checkpoint drift detected, resetting stream book");
            this.streamBook = this.checkpointBook.clone();
            // 可选：重新订阅 l2BookDelta 以获取正确的序列号基准
        }
    }
}
```

### 5.3 客户端状态机

```
                    ┌──────────────────┐
                    │                  │
    subscribe ──────►  WAITING_SNAPSHOT │
                    │                  │
                    └────────┬─────────┘
                             │ receive snapshot
                             ▼
                    ┌──────────────────┐
                    │                  │◄──── delta (seq=expected)
    normal ─────────►    SYNCED        │      apply & increment
                    │                  │
                    └────────┬─────────┘
                             │ gap detected
                             ▼
                    ┌──────────────────┐
                    │                  │
    gap ────────────►     STALE        │───── resync request
                    │                  │
                    └────────┬─────────┘
                             │ receive new snapshot
                             ▼
                    ┌──────────────────┐
                    │                  │
                    │    SYNCED        │ (回到正常状态)
                    │                  │
                    └──────────────────┘
```

---

## 6. Checkpoint 作为 Ground Truth

### 6.1 设计原则

**Checkpoint 数据永远是权威数据源（Ground Truth）。**

这一原则贯穿整个一致性模型：

| 组件 | 数据来源 | 角色 | 不一致时处理 |
|------|---------|------|------------|
| `dex:orderbook:{id}` | dex-indexer (Checkpoint) | **Ground Truth** | 不修改 |
| `dex:l2book:{id}` | dex-streamer (Stream) | Optimistic Cache | 被 checkpoint 覆盖 |
| 客户端本地 L2 Book | WS delta | Ephemeral State | 重新订阅获取快照 |

### 6.2 为什么 Checkpoint 是 Ground Truth

1. **Checkpoint 由共识保证**：每个 Checkpoint 包含确定性的状态根（state root），所有诚实节点对同一 Checkpoint 的内容完全一致
2. **Checkpoint pipeline 保证不丢失**：dex-indexer 使用 `sui-indexer-alt-framework` 的 committed cursor 机制，确保每个 Checkpoint 恰好处理一次
3. **PostgreSQL 持久化**：即使 Redis 丢失，也能从 PG 重建
4. **Stream 通道是派生数据**：`dex:l2book:{id}` 是从 Stream 事件增量构建的，本质上是 `dex:orderbook:{id}` 的低延迟近似

### 6.3 API 层的 Fallback 机制

```rust
// dex-api 中的 l2Book 读取逻辑
pub async fn get_l2_book(
    perpetual_id: u32,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<L2BookResponse, ApiError> {
    // 优先读取 stream L2 book（低延迟，HSET 格式 b:{price}/a:{price}）
    let l2_key = format!("dex:l2book:{}", perpetual_id);
    let fields: HashMap<String, String> = redis::cmd("HGETALL")
        .arg(&l2_key)
        .query_async(redis)
        .await?;

    if !fields.is_empty() {
        // Stream book 存在，从 HSET 字段解析
        let book = L2BookSnapshot::from_hset_fields(&fields)?;
        return Ok(L2BookResponse {
            data: book,
            source: DataSource::Stream,
        });
    }

    // Fallback: 读取 checkpoint 快照（HSET 格式含 bids/asks JSON 字段）
    let checkpoint_key = format!("dex:orderbook:{}", perpetual_id);
    let (bids, asks): (Option<String>, Option<String>) = redis::cmd("HMGET")
        .arg(&checkpoint_key)
        .arg("bids")
        .arg("asks")
        .query_async(redis)
        .await?;

    match (bids, asks) {
        (Some(bids_json), Some(asks_json)) => {
            let book = L2BookSnapshot::from_json(&bids_json, &asks_json)?;
            Ok(L2BookResponse {
                data: book,
                source: DataSource::Checkpoint,
            })
        }
        _ => Err(ApiError::MarketNotFound(perpetual_id)),
    }
}

/// 数据来源标识（可选：包含在 API 响应中供客户端判断）
pub enum DataSource {
    Stream,     // 来自 dex:l2book（低延迟，可能有微小偏差）
    Checkpoint, // 来自 dex:orderbook（可靠，但延迟较高）
}
```

### 6.4 一致性保证总结

| 保证 | 描述 | 机制 |
|------|------|------|
| **最终一致性** | Stream L2 book 最终会与 Checkpoint 一致 | 定期对账（30s） |
| **有界延迟** | 用户看到的数据最多落后一个对账周期 | Reconciler interval |
| **无数据丢失** | 即使 Stream 通道完全失败，Checkpoint 通道仍可用 | Fallback 机制 |
| **自动恢复** | 任何不一致都会在下一个对账周期自动修正 | Reconciler |
| **客户端感知** | 客户端通过序列号可以检测自身的数据是否完整 | Sequence gap detection |

---

## 7. 故障场景分析

### 7.1 场景一：DexStreamingManager broadcast channel 满

**触发条件**：dex-streamer 消费速度持续低于 DexStreamingManager 生产速度，导致 broadcast channel 中最旧的消息被覆盖。

| 维度 | 详情 |
|------|------|
| **检测方式** | dex-streamer 收到 `RecvError::Lagged(n)`，报告跳过 n 条消息 |
| **影响范围** | 被跳过的消息中涉及的市场的 L2 book 可能不准确 |
| **用户感知** | L2 book 短暂停滞或出现档位偏差（<30s，直到 reconciliation 修正） |
| **恢复步骤** | 1. dex-streamer 收到 Lagged 错误 → 2. 对所有市场触发 snapshot recovery → 3. 从 `dex:orderbook:{id}` 重建 L2 book → 4. 继续消费后续 delta |
| **恢复时间** | <1s（Redis 读取快照 + 内存重建） |
| **预防措施** | 增大 channel 容量（当前 10000）；优化 dex-streamer 消费速度；监控 lag 指标 |

```rust
// dex-streamer 消费循环中的 Lagged 处理
match receiver.recv().await {
    Err(broadcast::error::RecvError::Lagged(skipped)) => {
        warn!(skipped, "Stream consumer lagged, initiating full recovery");

        metrics::counter!("dex_streamer_channel_lags_total").increment(1);
        metrics::gauge!("dex_streamer_last_lag_size").set(skipped as f64);

        // 对所有活跃市场触发 snapshot recovery
        let market_ids: Vec<u32> = builder.books.keys().copied().collect();
        for perpetual_id in market_ids {
            builder.recover_from_snapshot(perpetual_id, &mut redis).await?;
        }
    }
    // ...
}
```

### 7.2 场景二：dex-streamer 进程崩溃

**触发条件**：dex-streamer 进程因 panic、OOM、手动重启、部署更新等原因终止。

| 维度 | 详情 |
|------|------|
| **检测方式** | 进程退出 → systemd/docker 自动重启 → 启动时检测到无内存状态 |
| **影响范围** | 所有市场的 `dex:l2book:{id}` 停止更新 |
| **用户感知** | L2 book 数据停滞（显示旧数据），直到 dex-streamer 重启完成 |
| **恢复步骤** | 1. 进程重启 → 2. 连接 Redis → 3. 为每个市场从 `dex:orderbook:{id}` 加载快照 → 4. 订阅 DexStreamingManager 的 broadcast channel → 5. 开始消费新 delta |
| **恢复时间** | <5s（进程启动 + Redis 连接 + 快照加载） |
| **预防措施** | Docker restart policy: `unless-stopped`；健康检查；graceful shutdown 先取消订阅 |

```rust
// dex-streamer 启动时的恢复流程
impl DexStreamer {
    pub async fn startup(&mut self) -> Result<()> {
        info!("dex-streamer starting, loading snapshots from checkpoint");

        let start = Instant::now();

        // 获取所有活跃市场 ID
        let market_ids = self.get_active_market_ids().await?;

        // 并行加载所有市场的 checkpoint 快照
        for perpetual_id in &market_ids {
            match self.builder.recover_from_snapshot(*perpetual_id, &mut self.redis).await {
                Ok(()) => info!(perpetual_id, "Loaded checkpoint snapshot"),
                Err(RecoveryError::NoSnapshotAvailable) => {
                    warn!(perpetual_id, "No checkpoint snapshot, will initialize from first stream event");
                }
                Err(e) => error!(perpetual_id, error = %e, "Failed to load snapshot"),
            }
        }

        let duration = start.elapsed();
        info!(
            market_count = market_ids.len(),
            duration_ms = duration.as_millis() as u64,
            "Startup recovery completed"
        );

        // 订阅 broadcast channel
        self.receiver = self.authority_state
            .dex_streaming
            .as_ref()
            .expect("dex_streaming must be enabled")
            .subscribe();

        Ok(())
    }
}
```

### 7.3 场景三：Redis 连接丢失

**触发条件**：Redis 服务器重启、网络中断、连接池耗尽。

| 维度 | 详情 |
|------|------|
| **检测方式** | Redis 命令返回连接错误；健康检查失败 |
| **影响范围** | `dex:l2book:{id}` 无法更新；API 层 fallback 到 `dex:orderbook:{id}`（若 dex-indexer 也连接同一 Redis 则两者都受影响） |
| **用户感知** | 如果 dex-indexer 的 Redis 也断开：REST API 返回错误或缓存数据；WS 推送停止 |
| **恢复步骤** | 1. Redis 恢复 → 2. dex-streamer 重连（自动重试） → 3. 对所有市场触发 snapshot recovery → 4. 恢复正常 delta 写入 |
| **恢复时间** | Redis 恢复后 <10s |
| **预防措施** | Redis 高可用部署（Sentinel/Cluster）；连接池自动重连；dex-streamer 写入失败时缓冲 delta |

**特别注意**：dex-streamer 的 broadcast channel 消费不依赖 Redis。即使 Redis 断开，dex-streamer 仍然继续消费 broadcast channel 中的 delta，在内存中维护 L2 book 状态。Redis 恢复后，立即将内存状态写入 Redis。

```rust
// Redis 写入失败时的容错逻辑
async fn write_l2book_to_redis(
    &self,
    perpetual_id: u32,
    book: &L2Book,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<(), StreamerError> {
    let key = format!("dex:l2book:{}", perpetual_id);

    // 使用 pipeline 增量写入 HSET b:{price}/a:{price} 字段
    let mut pipe = redis::pipe();
    for (side, price, qty) in &book.to_snapshot() {
        let field = match side {
            Side::Bid => format!("b:{}", price),
            Side::Ask => format!("a:{}", price),
        };
        pipe.cmd("HSET").arg(&key).arg(&field).arg(qty);
    }
    match pipe.query_async::<_, ()>(redis).await {
        Ok(()) => Ok(()),
        Err(e) => {
            warn!(
                perpetual_id,
                error = %e,
                "Redis write failed, L2 book maintained in memory only"
            );

            metrics::counter!("dex_streamer_redis_write_errors_total",
                "perpetual_id" => perpetual_id.to_string())
                .increment(1);

            // 不中断消费循环，内存中的 L2 book 仍然是准确的
            // Redis 恢复后，下一次写入会自动更新
            Err(StreamerError::RedisUnavailable(e))
        }
    }
}
```

### 7.4 场景四：dex-indexer 处理延迟（Checkpoint 落后）

**触发条件**：dex-indexer 处理 Checkpoint 的速度跟不上 Checkpoint 产出速度（例如 PostgreSQL 写入慢、大量历史 Checkpoint 需要回放）。

| 维度 | 详情 |
|------|------|
| **检测方式** | dex-indexer 的 checkpoint cursor 落后于最新 checkpoint；`dex_indexer_checkpoint_lag` 指标增大 |
| **影响范围** | `dex:orderbook:{id}` 中的快照数据过时；Reconciler 对账使用旧基准 |
| **用户感知** | Stream L2 book 正常（不受 indexer 影响）；但 REST l2Book 的 Checkpoint fallback 数据较旧 |
| **恢复步骤** | 1. dex-indexer 追上最新 checkpoint → 2. `dex:orderbook:{id}` 自动更新 → 3. Reconciler 下一周期使用最新快照对账 |
| **恢复时间** | 取决于 indexer 追赶速度（通常分钟级） |
| **预防措施** | 监控 indexer lag；优化 PG 写入性能；增加 indexer 并发度 |

**影响分析**：

这是一个**低影响**场景。因为：
1. Stream 通道独立于 Checkpoint 通道，`dex:l2book:{id}` 的更新不受影响
2. Reconciler 使用旧 checkpoint 对账，可能检测到"不一致"——但这是因为 checkpoint 落后，不是 stream 错误
3. Reconciler 不应在 indexer 落后时将 stream book 回退到旧快照

**改进**：Reconciler 应检查 checkpoint 快照的时间戳，如果明显落后于 stream 的最新更新时间，则跳过本次对账：

```rust
// Reconciler 中的时间戳检查
if checkpoint_book.timestamp_ms + self.max_checkpoint_age_ms < stream_book.last_update_ms {
    debug!(
        perpetual_id,
        checkpoint_age_ms = stream_book.last_update_ms - checkpoint_book.timestamp_ms,
        "Checkpoint too old, skipping reconciliation"
    );
    return ReconcileResult::CheckpointStale;
}
```

### 7.5 场景五：网络分区（gRPC 模式下，Validator 与 dex-streamer 断开）

**触发条件**：（未来 gRPC 升级后适用）Validator 节点与 dex-streamer 之间的网络中断。

| 维度 | 详情 |
|------|------|
| **检测方式** | gRPC stream 断开；heartbeat 超时 |
| **影响范围** | Stream 通道完全中断，`dex:l2book:{id}` 停止更新 |
| **用户感知** | 与 dex-streamer 崩溃相同：L2 book 停滞 |
| **恢复步骤** | 1. gRPC 自动重连（exponential backoff） → 2. 重连后触发所有市场 snapshot recovery → 3. 恢复 delta 消费 |
| **恢复时间** | 网络恢复后 <10s（重连 + snapshot 加载） |
| **预防措施** | gRPC keepalive；多节点冗余连接；fallback 到 Checkpoint 通道 |

**注意**：当前阶段使用进程内 `tokio::broadcast`，不存在网络分区问题。此场景仅适用于未来升级到 gRPC 传输后。

### 7.6 故障场景总结矩阵

| 场景 | 可能性 | 影响程度 | 检测速度 | 恢复时间 | 需要人工介入 |
|------|--------|---------|---------|---------|------------|
| Channel overflow | 中等 | 低 | 即时 | <1s | 否 |
| dex-streamer 崩溃 | 低 | 中 | <1s | <5s | 否 |
| Redis 连接丢失 | 低 | 中-高 | <1s | <10s | 否 |
| dex-indexer 落后 | 中等 | 低 | 秒级 | 分钟级 | 否 |
| 网络分区 (gRPC) | 低 | 中 | <5s | <10s | 否 |

---

## 8. 监控指标

### 8.1 核心指标

| 指标名 | 类型 | 标签 | 含义 |
|--------|------|------|------|
| `dex_streamer_deltas_applied_total` | Counter | `perpetual_id` | 成功应用的 delta 总数 |
| `dex_streamer_sequence_gaps_total` | Counter | `perpetual_id` | 检测到的序列号间隙次数 |
| `dex_streamer_channel_lags_total` | Counter | - | broadcast channel lagged 事件次数 |
| `dex_streamer_last_lag_size` | Gauge | - | 最近一次 lag 跳过的消息数 |
| `dex_streamer_reconciliation_ok_total` | Counter | `perpetual_id` | 对账结果一致的次数 |
| `dex_streamer_reconciliation_corrections_total` | Counter | `perpetual_id` | 对账修正的次数 |
| `dex_streamer_recovery_duration_seconds` | Histogram | - | Snapshot recovery 耗时分布 |
| `dex_streamer_redis_write_errors_total` | Counter | `perpetual_id` | Redis 写入失败次数 |

### 8.2 新鲜度指标

| 指标名 | 类型 | 标签 | 含义 |
|--------|------|------|------|
| `dex_streamer_l2book_freshness_ms` | Gauge | `perpetual_id` | L2 book 距上次更新的毫秒数（越小越好） |
| `dex_streamer_checkpoint_lag_ms` | Gauge | `perpetual_id` | Stream 领先 Checkpoint 的毫秒数（正常值 1000-3000） |
| `dex_streamer_sequence_current` | Gauge | `perpetual_id` | 当前已处理的序列号（用于监控消费进度） |

### 8.3 告警规则

```yaml
# Prometheus alerting rules（参考）
groups:
  - name: dex-streamer-consistency
    rules:
      # 连续 5 分钟 L2 book 无更新
      - alert: L2BookStale
        expr: dex_streamer_l2book_freshness_ms > 300000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "L2 book stale for market {{ $labels.perpetual_id }}"

      # 对账修正频率过高（每分钟超过 5 次）
      - alert: ReconciliationDriftHigh
        expr: rate(dex_streamer_reconciliation_corrections_total[5m]) > 0.1
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "High reconciliation drift rate for market {{ $labels.perpetual_id }}"

      # Channel lag 频繁发生
      - alert: ChannelLagFrequent
        expr: rate(dex_streamer_channel_lags_total[5m]) > 0.05
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Frequent broadcast channel lags, stream may be unreliable"

      # Recovery 耗时过长
      - alert: RecoveryDurationHigh
        expr: histogram_quantile(0.99, dex_streamer_recovery_duration_seconds_bucket) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Snapshot recovery taking >5s at p99"
```

### 8.4 运维仪表盘建议

| 面板 | 展示内容 | 用途 |
|------|---------|------|
| Stream vs Checkpoint 延迟 | `dex_streamer_checkpoint_lag_ms` 时序图 | 观察双通道延迟差 |
| 对账健康度 | ok_total vs corrections_total 比率 | 评估 stream 准确性 |
| 序列号进度 | 每市场 `dex_streamer_sequence_current` | 确认 stream 正常消费 |
| Gap 和 Lag 事件 | `sequence_gaps_total` + `channel_lags_total` | 发现异常模式 |
| Recovery 耗时 | `recovery_duration_seconds` p50/p95/p99 | 评估恢复性能 |

---

## 9. 设计决策总结

| # | 决策 | 选择 | 理由 | 替代方案 |
|---|------|------|------|---------|
| 1 | 不一致修正方向 | Checkpoint 覆盖 Stream | Checkpoint 有共识保证，是 ground truth | Stream 覆盖 Checkpoint（不安全） |
| 2 | 对账频率 | 30 秒 | 平衡修正延迟与 Redis 读取开销 | 10 秒（更及时但开销更大）；60 秒（开销更小但修正慢） |
| 3 | 对账比较深度 | Top 20 档位 | 覆盖大部分交易活跃区域 | 全量比较（开销大）；Top 5（覆盖不足） |
| 4 | 序列号作用域 | 每市场独立 | 不同市场的更新频率不同，避免无关市场的 gap 互相影响 | 全局序列号（简单但不精确） |
| 5 | Gap 恢复策略 | 立即 snapshot recovery | 最快恢复，不尝试补缺 | 等待缺失事件（可能永远不到）；从 PG 查补缺事件（太慢） |
| 6 | 节点重启检测 | 启发式（序列号大幅回退） | 简单有效，无需额外协议 | 引擎广播 restart 事件（增加复杂度） |
| 7 | Checkpoint 过期检查 | 比较时间戳 | 避免用旧 checkpoint 覆盖新 stream 数据 | 无检查（可能错误回退） |

---

## 10. 与其他文档的关系

| 文档 | 与本文档的关系 |
|------|--------------|
| [01-streaming-source.md](./01-streaming-source.md) | DexStreamingManager 是 Stream 通道的数据源，本文档定义了其 channel overflow 的处理策略 |
| [02-event-design.md](./02-event-design.md) | OrderbookDeltaEvent 的序列号字段是本文档 gap detection 的基础 |
| [03-transport-protocol.md](./03-transport-protocol.md) | broadcast channel 的背压行为直接影响本文档的 channel overflow 场景 |
| [04-offchain-orderbook.md](./04-offchain-orderbook.md) | dex-streamer 的 OrderbookBuilder 是本文档 Reconciler 的对象 |
| [05-api-ws-integration.md](./05-api-ws-integration.md) | 客户端 gap detection 和 dual subscription 是本文档客户端一致性的实现 |
| [07-implementation-plan.md](./07-implementation-plan.md) | 本文档中的 Reconciler 和序列号机制在实施计划中对应具体的开发步骤 |
