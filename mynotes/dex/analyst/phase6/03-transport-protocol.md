# 传输协议设计

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: ⚠️ 需重写 — gRPC 取代 BroadcastTransport
> 上级文档: [00-overview.md](./00-overview.md)

> **2026-02-27 架构决策变更通知**
>
> 根据 [08-architecture-qa.md](./08-architecture-qa.md) 确认的决策：
> - **Q7=B**: gRPC streaming 一步到位（不再分 Phase 1 broadcast + Phase 2 gRPC）
> - **Q3=C+**: gRPC `GetSnapshot()` 用于恢复（从 InlineOrderbook 读取）
>
> **本文档需重写的部分**：
> - §2 StreamTransport trait → 移除抽象层，直接实现 gRPC server
> - §3 BroadcastTransport → 改为 DexStreamingManager 内嵌 gRPC server
> - §4 gRPC Transport → 提升为 Phase 1（不再是升级路径）
> - 新增 `GetSnapshot()` RPC 接口（从 InlineOrderbook 读取完整 L2）
> - §6 背压处理 → gRPC HTTP/2 流控 + 重连恢复机制
>
> 当前文档内容保留作为参考，实际实现以 07-implementation-plan.md Step 2 为准。

## 1. 概述

DexStreamingManager 在 sui-core 中拦截 DEX 执行后的事件，生成 `DexStreamBatch`。这些事件需要以最低延迟传输到 dex-streamer（消费端）。

**分阶段策略**：

| 阶段 | 传输方式 | 延迟 | 适用场景 |
|------|----------|------|----------|
| Phase 1 | 进程内 `tokio::broadcast` channel | <10us | 单节点部署，streamer 与 validator 同进程 |
| Phase 2 | gRPC streaming | ~100us | 远程消费者，streamer 独立部署 |

为支持平滑升级，定义 `StreamTransport` trait 抽象层，Phase 1 和 Phase 2 分别实现。

---

## 2. StreamTransport trait 抽象

核心设计原则：**生产者永不阻塞**。执行路径不能因为流式消费者而减速。

```rust
use async_trait::async_trait;

/// 传输层抽象，支持多种实现（broadcast channel / gRPC）
#[async_trait]
pub trait StreamTransport: Send + Sync + 'static {
    /// 创建一个新的订阅者，返回接收端
    async fn subscribe(&self) -> Result<Box<dyn StreamReceiver>>;

    /// 发布一批事件（由 DexStreamingManager 调用）
    /// 注意：此方法不得阻塞，即使没有订阅者也应立即返回
    async fn publish(&self, batch: DexStreamBatch) -> Result<()>;

    /// 获取传输层统计信息
    fn stats(&self) -> TransportStats;
}

/// 接收端抽象
#[async_trait]
pub trait StreamReceiver: Send + 'static {
    /// 阻塞等待下一个批次
    async fn recv(&mut self) -> Result<DexStreamBatch>;

    /// 非阻塞尝试接收
    fn try_recv(&mut self) -> Result<Option<DexStreamBatch>>;
}

/// 传输层统计信息
pub struct TransportStats {
    /// 已发送消息总数
    pub messages_sent: u64,
    /// 因消费者落后而丢弃的消息数
    pub messages_dropped: u64,
    /// 当前活跃订阅者数量
    pub active_subscribers: u32,
    /// 平均发布延迟（微秒）
    pub avg_latency_us: u64,
}
```

trait 设计要点：

1. **`publish()` 是 async 但不应阻塞**：BroadcastTransport 中 `send()` 是同步操作，async 签名是为了兼容 gRPC 实现
2. **`subscribe()` 返回 `Box<dyn StreamReceiver>`**：擦除具体类型，消费端不感知传输实现
3. **`stats()` 是同步方法**：统计数据通过原子操作维护，无需异步

---

## 3. BroadcastTransport（Phase 1 - 进程内通道）

### 3.1 实现

```rust
use tokio::sync::broadcast;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};

pub struct BroadcastTransport {
    sender: broadcast::Sender<DexStreamBatch>,
    stats: Arc<AtomicTransportStats>,
}

/// 原子统计计数器
struct AtomicTransportStats {
    messages_sent: AtomicU64,
    messages_dropped: AtomicU64,
    active_subscribers: AtomicU32,
}

impl Default for AtomicTransportStats {
    fn default() -> Self {
        Self {
            messages_sent: AtomicU64::new(0),
            messages_dropped: AtomicU64::new(0),
            active_subscribers: AtomicU32::new(0),
        }
    }
}

impl BroadcastTransport {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            stats: Arc::new(AtomicTransportStats::default()),
        }
    }
}

#[async_trait]
impl StreamTransport for BroadcastTransport {
    async fn subscribe(&self) -> Result<Box<dyn StreamReceiver>> {
        let receiver = self.sender.subscribe();
        self.stats.active_subscribers.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(BroadcastReceiver {
            inner: receiver,
            stats: Arc::clone(&self.stats),
        }))
    }

    async fn publish(&self, batch: DexStreamBatch) -> Result<()> {
        match self.sender.send(batch) {
            Ok(_) => {
                self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                // 没有活跃的接收者，静默丢弃
                self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    fn stats(&self) -> TransportStats {
        TransportStats {
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            messages_dropped: self.stats.messages_dropped.load(Ordering::Relaxed),
            active_subscribers: self.stats.active_subscribers.load(Ordering::Relaxed),
            avg_latency_us: 0, // broadcast channel 延迟可忽略
        }
    }
}
```

### 3.2 接收端实现

```rust
struct BroadcastReceiver {
    inner: broadcast::Receiver<DexStreamBatch>,
    stats: Arc<AtomicTransportStats>,
}

#[async_trait]
impl StreamReceiver for BroadcastReceiver {
    async fn recv(&mut self) -> Result<DexStreamBatch> {
        loop {
            match self.inner.recv().await {
                Ok(batch) => return Ok(batch),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // 消费者落后，跳过丢失的消息，继续接收
                    // 消费端需要通过 sequence number 检测 gap 并触发恢复
                    tracing::warn!(
                        skipped = n,
                        "stream receiver lagged, {} messages dropped",
                        n
                    );
                    self.stats.messages_dropped.fetch_add(n, Ordering::Relaxed);
                    continue; // 继续接收下一条
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(anyhow::anyhow!("stream transport closed"));
                }
            }
        }
    }

    fn try_recv(&mut self) -> Result<Option<DexStreamBatch>> {
        match self.inner.try_recv() {
            Ok(batch) => Ok(Some(batch)),
            Err(broadcast::error::TryRecvError::Empty) => Ok(None),
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "stream receiver lagged on try_recv");
                self.stats.messages_dropped.fetch_add(n, Ordering::Relaxed);
                Ok(None) // 返回 None，下次 recv 时获取最新数据
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                Err(anyhow::anyhow!("stream transport closed"))
            }
        }
    }
}

impl Drop for BroadcastReceiver {
    fn drop(&mut self) {
        self.stats.active_subscribers.fetch_sub(1, Ordering::Relaxed);
    }
}
```

### 3.3 tokio::broadcast 语义详解

`tokio::broadcast::channel` 的关键特性，直接影响设计选择：

| 特性 | 行为 | 对本项目的影响 |
|------|------|---------------|
| 容量限制 | 固定大小环形缓冲区 | 设置 `capacity = 10000`，足够缓冲 ~10s 高频交易 |
| 消息克隆 | 每个接收者获得消息的 clone | `DexStreamBatch` 需要实现 `Clone`；批次体积应控制在小范围 |
| Lagged 语义 | 慢消费者追不上时返回 `RecvError::Lagged(n)` | 消费者跳过丢失消息，通过 seq number 检测 gap |
| 无接收者 | `send()` 返回 `Err`，但消息仍写入缓冲区 | 新订阅者可能收到订阅前的消息（如果还在缓冲区中） |
| 内存回收 | 所有接收者都读取后才释放消息 | 一个慢接收者会阻止缓冲区回收 |
| 多生产者 | `Sender` 可以 clone | 本项目只有一个生产者（DexStreamingManager） |

**容量选择依据**：

```
capacity = 10000
假设每秒 1000 笔交易 × 每笔 1 个 batch = 1000 batches/s
缓冲区可容纳 ~10s 的数据
单个 DexStreamBatch ≈ 200-500 bytes
总内存 ≈ 10000 × 500 bytes = 5MB（可接受）
```

### 3.4 性能特征

| 指标 | 值 | 说明 |
|------|-----|------|
| 发布延迟 | <1us | `broadcast::Sender::send()` 仅需获取锁 + memcpy |
| 接收延迟 | <10us | 含 Tokio waker 通知开销 |
| 序列化 | 无 | 同进程内传递 Rust 对象，零序列化 |
| 吞吐 | >100K msg/s | 远超 DEX 需求（~1K msg/s） |
| 内存 | ~5MB | 10000 × ~500 bytes/batch |

---

## 4. gRPC Transport（Phase 2 - 远程升级路径）

### 4.1 何时升级

当以下任一条件成立时考虑升级到 gRPC：

1. **dex-streamer 需要独立部署**：与 validator 不在同一进程/机器
2. **多个消费者在不同机器**：如多个 dex-api 实例各自需要流式数据
3. **跨数据中心部署**：validator 和索引服务在不同区域

### 4.2 Protobuf 定义

```protobuf
syntax = "proto3";

package dex.streaming.v1;

service DexStreaming {
    // 订阅 DEX 流式事件，返回无限流
    rpc Subscribe(SubscribeRequest) returns (stream DexStreamBatchProto);
}

message SubscribeRequest {
    // 可选：只订阅特定市场的事件
    repeated uint64 market_ids = 1;
    // 可选：从指定 sequence number 开始（用于断线重连）
    optional uint64 from_sequence = 2;
}

message DexStreamBatchProto {
    // 批次序列号（全局递增）
    uint64 sequence = 1;
    // 交易摘要
    bytes tx_digest = 2;
    // 执行时间戳
    uint64 timestamp_us = 3;
    // 事件列表
    repeated DexStreamEventProto events = 4;
}

message DexStreamEventProto {
    oneof event {
        OrderbookDeltaEventProto orderbook_delta = 1;
        FillEventProto fill = 2;
        OrderUpdateEventProto order_update = 3;
        BboUpdateEventProto bbo_update = 4;
    }
}

message OrderbookDeltaEventProto {
    uint64 perpetual_id = 1;
    uint64 sequence = 2;
    repeated OrderbookDeltaProto updates = 3;
    uint64 timestamp_ms = 4;
}

message OrderbookDeltaProto {
    uint32 side = 1;      // 0 = Bid, 1 = Ask
    uint64 price = 2;
    uint64 quantity = 3;  // 0 表示删除该价格档位
}
```

### 4.3 实现概要

```rust
use tonic::{transport::Server, Request, Response, Status, Streaming};

pub struct GrpcTransport {
    /// 内部仍然使用 broadcast channel 作为事件源
    inner: BroadcastTransport,
    /// gRPC server 地址
    addr: SocketAddr,
}

impl GrpcTransport {
    pub async fn start(addr: SocketAddr, capacity: usize) -> Result<Self> {
        let inner = BroadcastTransport::new(capacity);

        // 启动 gRPC server
        let service = DexStreamingService {
            transport: inner.clone(), // 需要 clone sender
        };

        tokio::spawn(async move {
            Server::builder()
                .add_service(DexStreamingServer::new(service))
                .serve(addr)
                .await
                .expect("gRPC server failed");
        });

        Ok(Self { inner, addr })
    }
}

/// gRPC 服务实现
struct DexStreamingService {
    transport: BroadcastTransport,
}

#[tonic::async_trait]
impl DexStreaming for DexStreamingService {
    type SubscribeStream = ReceiverStream<Result<DexStreamBatchProto, Status>>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let req = request.into_inner();
        let mut receiver = self.transport.subscribe().await
            .map_err(|e| Status::internal(e.to_string()))?;

        let (tx, rx) = tokio::sync::mpsc::channel(256);

        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(batch) => {
                        // 过滤市场（如果指定了）
                        let proto = batch.to_proto(); // DexStreamBatch → Protobuf
                        if tx.send(Ok(proto)).await.is_err() {
                            break; // 客户端断开
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
```

### 4.4 gRPC Transport 性能特征

| 指标 | 值 | 说明 |
|------|-----|------|
| 序列化延迟 | ~50-100us | Protobuf encode/decode 每批次 |
| 网络延迟 | ~100-500us | 同机器 loopback；跨机器取决于网络 |
| 总延迟 | ~200-600us | 序列化 + 网络 + 反序列化 |
| 吞吐 | ~50K msg/s | 受序列化和网络限制 |
| 认证 | mTLS 或 API key | 通过 tonic interceptor 实现 |

### 4.5 向后兼容

可以同时运行两种传输方式：

```rust
/// 复合传输层：同时通过 broadcast 和 gRPC 分发事件
pub struct CompositeTransport {
    broadcast: BroadcastTransport,
    grpc: Option<GrpcTransport>,
}

#[async_trait]
impl StreamTransport for CompositeTransport {
    async fn publish(&self, batch: DexStreamBatch) -> Result<()> {
        // 两个通道同时发布
        self.broadcast.publish(batch.clone()).await?;
        if let Some(grpc) = &self.grpc {
            grpc.publish(batch).await?;
        }
        Ok(())
    }

    // ...
}
```

---

## 5. 批处理策略

### 5.1 生产端（DexStreamingManager）

DexStreamingManager 将单笔交易执行产生的所有 DEX 事件打包为一个 `DexStreamBatch`：

```
一笔交易执行
  ├── PlaceOrder(BTC-USDC, Buy, 50000, 1.0)
  │     ├── Fill(maker_order_1, 0.3 BTC)
  │     ├── Fill(maker_order_2, 0.5 BTC)
  │     ├── OrderbookDelta(bid remove 50000×0.3, bid remove 50001×0.5)
  │     ├── OrderbookDelta(ask add 50000×0.2)  // 剩余挂单
  │     └── BboUpdate(best_bid=50000, best_ask=50002)
  └── 打包为 DexStreamBatch { tx_digest, events: [...] }
```

**设计要点**：

- 每个 `DexStreamBatch` 对应一笔交易的所有 DEX 事件
- 不做时间窗口合并（time-based batching）——事件在引擎执行完成时已经确定
- 批次内事件保持执行顺序
- 批次本身附带全局递增的 sequence number

### 5.2 消费端（dex-streamer）

消费端可以根据自身需求做微批处理，优化 Redis 写入效率：

```rust
/// dex-streamer 消费端的微批处理逻辑
async fn consume_loop(mut receiver: Box<dyn StreamReceiver>) {
    let mut pending_deltas: HashMap<MarketId, Vec<OrderbookDelta>> = HashMap::new();
    let mut last_flush = Instant::now();
    let flush_interval = Duration::from_millis(5); // 5ms 微批窗口

    loop {
        // 非阻塞尝试接收，积累多个批次
        match tokio::time::timeout(flush_interval, receiver.recv()).await {
            Ok(Ok(batch)) => {
                // 积累 delta
                for event in batch.events {
                    if let DexStreamEvent::OrderbookDelta(delta) = event {
                        pending_deltas
                            .entry(delta.perpetual_id)
                            .or_default()
                            .push(delta);
                    }
                }
            }
            Ok(Err(_)) => break, // 通道关闭
            Err(_) => {} // 超时，进入 flush
        }

        // 每 5ms 或积累足够数据时 flush 到 Redis
        if last_flush.elapsed() >= flush_interval && !pending_deltas.is_empty() {
            flush_to_redis(&mut pending_deltas).await;
            last_flush = Instant::now();
        }
    }
}

/// 使用 Redis pipeline 批量写入
async fn flush_to_redis(deltas: &mut HashMap<u32, Vec<OrderbookDelta>>) {
    let mut pipe = redis::pipe();

    for (perpetual_id, market_deltas) in deltas.iter() {
        // 合并同一价格档位的多次变更（取最后一次）
        let merged = merge_deltas(market_deltas);

        for delta in &merged {
            // HSET dex:l2book:{perpetual_id} b:{price}/a:{price} {quantity}
            let side_prefix = if delta.side == 0 { "b" } else { "a" };
            let field = format!("{}:{}", side_prefix, delta.price);
            if delta.quantity == 0 {
                pipe.cmd("HDEL")
                    .arg(format!("dex:l2book:{}", perpetual_id))
                    .arg(&field);
            } else {
                pipe.cmd("HSET")
                    .arg(format!("dex:l2book:{}", perpetual_id))
                    .arg(&field)
                    .arg(delta.quantity.to_string());
            }
        }

        // XADD dex:stream:l2:delta 发布增量事件
        pipe.xadd(
            "dex:stream:l2:delta",
            "*",
            &[("data", serde_json::to_string(&merged).unwrap())],
        );
    }

    pipe.query_async::<_, ()>(&mut conn).await.ok();
    deltas.clear();
}
```

**微批参数选择**：

| 参数 | 值 | 理由 |
|------|-----|------|
| flush_interval | 5ms | 平衡延迟（<50ms 目标）和 Redis 效率 |
| 最大积累批次 | 100 | 防止内存无限增长 |
| Redis pipeline 大小 | 不限 | 由 flush_interval 内积累的数据量决定 |

---

## 6. 背压处理

### 6.1 各场景对比

| 场景 | BroadcastTransport | gRPC Transport |
|------|-------------------|----------------|
| 消费者处理慢 | `RecvError::Lagged(n)`，跳过 n 条消息继续 | gRPC HTTP/2 流控，服务端缓冲 |
| 消费者断开连接 | 事件静默丢弃（`send()` 返回 Err） | gRPC 检测到流关闭，清理资源 |
| 缓冲区满 | 最旧的消息被覆盖（环形缓冲区语义） | gRPC 背压或丢弃策略 |
| 恢复机制 | 消费者通过 sequence gap 检测丢失，请求快照恢复 | 同左 + 支持 `from_sequence` 重连 |

### 6.2 核心原则

**生产者永不阻塞**：

```rust
// DexStreamingManager 中的发布逻辑
impl DexStreamingManager {
    pub fn on_transaction_executed(&self, effects: &TransactionEffects) {
        if let Some(batch) = self.extract_dex_events(effects) {
            // send() 是非阻塞的：
            // - 有接收者：消息写入缓冲区，O(1)
            // - 无接收者：返回 Err，静默丢弃
            // - 缓冲区满：最旧消息被覆盖，慢消费者下次 recv 得到 Lagged
            let _ = self.transport.publish(batch);
            //  ^--- 故意忽略错误，生产者不关心消费者状态
        }
    }
}
```

这确保了 DEX 执行路径（关键路径）不会因为流式推送而产生任何延迟。即使所有消费者都宕机，引擎执行性能也不受影响。

### 6.3 Lagged 消费者恢复流程

```
消费者发现 RecvError::Lagged(n)
    │
    ├── 记录 warning 日志 + 更新 messages_dropped 计数
    │
    ├── 检查 sequence gap 大小
    │     ├── gap < 100: 继续接收，依赖后续增量补齐
    │     └── gap >= 100: 触发全量恢复
    │
    └── 全量恢复流程:
          ├── 从 Redis HGETALL dex:l2book:{perpetual_id} 获取当前快照
          ├── 或从 Checkpoint 管线获取最新确认状态
          └── 重建本地订单簿，继续增量消费
```

---

## 7. 可靠性保证

### 7.1 保证级别

| 保证维度 | 级别 | 实现机制 |
|----------|------|----------|
| 消息顺序 | 单市场有序 | `OrderbookDeltaEvent` 中的 `sequence` 字段（每市场单调递增） |
| 投递保证（Stream） | 至多一次 (at-most-once) | broadcast channel 语义：Lagged 时消息丢失 |
| 投递保证（Checkpoint） | 至少一次 (at-least-once) | Checkpoint pipeline 可靠投递，幂等处理 |
| Gap 检测 | 消费端检测 | 消费者比较连续 sequence number，发现缺口触发恢复 |
| 最终恢复 | 全量快照 | 从 Redis 当前状态或 Checkpoint 最新确认状态恢复 |

### 7.2 双通道互补

```
Stream 通道 (低延迟, at-most-once)
    │  延迟 <50ms
    │  可能丢消息
    │
    └──→ dex-streamer → Redis (增量状态)
              │
              │  定期对账 ←──── Checkpoint 通道 (高延迟, at-least-once)
              │                      │  延迟 1-3s
              │                      │  保证不丢
              ▼                      ▼
         Redis 增量状态  ←── 校验/修正 ── Checkpoint 确认状态
```

Stream 提供低延迟的「乐观」状态更新；Checkpoint 提供高延迟但可靠的「最终」状态确认。两者协同确保：

1. **正常情况**：用户在 <50ms 内看到订单簿变化
2. **丢消息时**：消费者通过 sequence gap 自动恢复
3. **极端情况**：Checkpoint 通道兜底校正，确保最终一致性

### 7.3 Sequence Number 设计

```rust
/// 每个市场维护独立的 sequence counter
pub struct MarketSequencer {
    /// market_id → 下一个 sequence number
    sequences: HashMap<u64, AtomicU64>,
}

impl MarketSequencer {
    /// 为指定市场生成下一个 sequence number
    pub fn next(&self, market_id: u64) -> u64 {
        self.sequences
            .entry(market_id)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed)
    }
}
```

消费端校验逻辑：

```rust
/// 消费端 sequence 连续性校验
fn check_sequence(
    expected: &mut HashMap<u64, u64>,  // market_id → expected_seq
    event: &OrderbookDeltaEvent,
) -> SequenceCheck {
    let market_id = event.perpetual_id;
    let seq = event.sequence;

    match expected.get(&market_id) {
        None => {
            // 首次收到该市场的事件，初始化
            expected.insert(market_id, seq + 1);
            SequenceCheck::Ok
        }
        Some(&exp) if seq == exp => {
            // 连续，正常
            expected.insert(market_id, seq + 1);
            SequenceCheck::Ok
        }
        Some(&exp) if seq > exp => {
            // 有 gap，触发恢复
            let gap = seq - exp;
            expected.insert(market_id, seq + 1);
            SequenceCheck::Gap { market_id, missing: gap }
        }
        Some(&exp) => {
            // seq < exp，重复消息（可能是恢复后的重放），忽略
            SequenceCheck::Duplicate
        }
    }
}

enum SequenceCheck {
    Ok,
    Gap { market_id: u64, missing: u64 },
    Duplicate,
}
```

---

## 8. 监控指标

### 8.1 Prometheus 指标定义

| 指标名 | 类型 | 标签 | 说明 |
|--------|------|------|------|
| `dex_stream_events_published_total` | Counter | `transport_type` | 已发布的事件批次总数 |
| `dex_stream_events_dropped_total` | Counter | `transport_type`, `reason` | 丢弃的事件数（`reason`: no_subscriber / lagged） |
| `dex_stream_publish_latency_us` | Histogram | `transport_type` | 发布操作延迟直方图（微秒） |
| `dex_stream_active_subscribers` | Gauge | `transport_type` | 当前活跃订阅者数量 |
| `dex_stream_consumer_lag` | Gauge | `consumer_id`, `market_id` | 消费者滞后量（events behind） |
| `dex_stream_sequence_gaps_total` | Counter | `market_id` | Sequence gap 检测次数 |
| `dex_stream_recovery_total` | Counter | `market_id`, `recovery_type` | 快照恢复触发次数（`recovery_type`: redis / checkpoint） |

### 8.2 集成到现有监控

```rust
use prometheus::{IntCounter, IntGauge, Histogram, HistogramOpts, register_int_counter,
    register_int_gauge, register_histogram};

lazy_static! {
    static ref EVENTS_PUBLISHED: IntCounter = register_int_counter!(
        "dex_stream_events_published_total",
        "Total DEX stream events published"
    ).unwrap();

    static ref EVENTS_DROPPED: IntCounter = register_int_counter!(
        "dex_stream_events_dropped_total",
        "Total DEX stream events dropped"
    ).unwrap();

    static ref PUBLISH_LATENCY: Histogram = register_histogram!(
        HistogramOpts::new(
            "dex_stream_publish_latency_us",
            "DEX stream publish latency in microseconds"
        )
        // 桶范围：1us, 5us, 10us, 50us, 100us, 500us, 1ms, 5ms
        .buckets(vec![1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0])
    ).unwrap();

    static ref ACTIVE_SUBSCRIBERS: IntGauge = register_int_gauge!(
        "dex_stream_active_subscribers",
        "Number of active stream subscribers"
    ).unwrap();
}
```

### 8.3 告警规则

| 条件 | 级别 | 说明 |
|------|------|------|
| `dex_stream_events_dropped_total` 增长率 > 100/min | Warning | 消费者频繁落后 |
| `dex_stream_active_subscribers` == 0 持续 > 30s | Warning | 无活跃消费者，数据未被消费 |
| `dex_stream_consumer_lag` > 1000 | Critical | 消费者严重滞后，可能需要快照恢复 |
| `dex_stream_sequence_gaps_total` 增长率 > 10/min | Warning | 频繁 gap，传输可靠性下降 |

---

## 9. 配置

```rust
/// 传输层配置
#[derive(Clone, Debug, Deserialize)]
pub struct StreamTransportConfig {
    /// 传输类型: "broadcast" | "grpc" | "composite"
    pub transport_type: String,

    /// broadcast channel 容量（默认 10000）
    #[serde(default = "default_channel_capacity")]
    pub channel_capacity: usize,

    /// gRPC 监听地址（仅 grpc/composite 模式）
    pub grpc_addr: Option<SocketAddr>,

    /// 消费端微批窗口（毫秒，默认 5）
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,
}

fn default_channel_capacity() -> usize { 10000 }
fn default_flush_interval_ms() -> u64 { 5 }
```

---

## 10. 与其他文档关系

| 文档 | 关系 |
|------|------|
| [01-streaming-source](./01-streaming-source.md) | 上游：DexStreamingManager 生产 DexStreamBatch，通过本文档描述的传输层投递 |
| [02-event-design](./02-event-design.md) | 数据：DexStreamBatch 内包含的事件类型（OrderbookDelta、Fill 等） |
| [04-offchain-orderbook](./04-offchain-orderbook.md) | 下游：dex-streamer 作为消费端，通过 StreamReceiver 接收事件 |
| [06-consistency-model](./06-consistency-model.md) | 互补：本文档的 at-most-once 保证 + Checkpoint at-least-once 保证 = 最终一致性 |
