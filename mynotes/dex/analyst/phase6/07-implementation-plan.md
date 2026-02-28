# Phase 6-07: 分步实施计划

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: 实施规划（架构决策已全部确认，见 [08-architecture-qa.md](./08-architecture-qa.md)）
> 关联文档: [00-overview](./00-overview.md) | [01-streaming-source](./01-streaming-source.md) | [02-event-design](./02-event-design.md) | [03-transport-protocol](./03-transport-protocol.md) | [08-architecture-qa](./08-architecture-qa.md)

## 1. 概述

本文档定义 Phase 6 低延迟流式订单簿架构的分步实施计划。整体目标是将订单簿更新延迟从当前的 1.5-3.5s（Checkpoint 通道）降低到 <50ms（流式通道），同时保持与现有 Checkpoint 管道的向后兼容。

**实施原则**：

| 原则 | 说明 |
|------|------|
| 增量交付 | 每个 Step 独立可验证，不依赖后续 Step 即可测试 |
| 向后兼容 | 现有 Checkpoint 管道和 API 不受影响，流式通道为 opt-in |
| 可回滚 | 每个 Step 均有独立回滚方案，任何 Step 失败不影响线上服务 |
| 测试先行 | 每个 Step 完成前必须通过对应的验证标准 |

**总体架构路径**（2026-02-27 更新，反映 Q3=C+, Q5=A2, Q7=B 决策）：

```
Step 1: 事件层        → OrderbookDeltaEvent 类型定义 + 引擎 delta 计算（不 emit 到 TransactionEvents）
Step 2: 流式源+gRPC   → DexStreamingManager + gRPC server（Subscribe + GetSnapshot）
Step 3: 消费者层      → dex-streamer 独立服务（gRPC 消费 + 内存 L2Book + Redis 写入）
Step 4: API 集成      → dex-api 增强（REST l2Book + WS 全量推送 + BBO）
Step 5: Docker & E2E  → 全栈验证（新增 dex-streamer Docker 服务）
```

**关键变更（vs 原方案）**：
- ~~Step 3 BroadcastTransport~~ → 合并到 Step 2（gRPC 一步到位，Q7=B）
- ~~Checkpoint 订单簿事件~~ → 移除（Q3=C+, OrderbookSnapshotEvent 停止发射）
- ~~OrderbookDeltaEvent in TransactionEvents~~ → 不纳入（Q5=A2, 仅走 gRPC）
- ~~WS 增量 delta 推送~~ → 改为全量 L2 快照推送（Q2=A, 对标 Hyperliquid）
- Step 总数从 6 步减为 5 步（传输层与流式源合并）

---

## 2. Step 1: 事件层 — OrderbookDeltaEvent 类型 + Delta 计算（1-2 天）

### 2.1 目标

定义 `OrderbookDeltaEvent` 类型和引擎内 delta 计算逻辑。**注意**：根据 Q3=C+ 和 Q5=A2 决策，delta 事件**不通过 `emit_event()` 写入 TransactionEvents**，而是由 DexStreamingManager 直接通过 gRPC 推送。同时，`OrderbookSnapshotEvent` **停止发射**（不再需要 Checkpoint 订单簿数据）。

### 2.2 修改文件

#### 文件 1: `dex-sui/crates/sui-types/src/dex_events.rs`

**变更内容**：

1. 新增 `OrderbookDeltaEvent` 结构体（详见 [02-event-design.md](./02-event-design.md) 第 2.1 节）
2. 新增 `OrderbookDelta` 结构体（单个价格档位变更描述）
3. 实现 `struct_tag()` 和 `to_sui_event()` 方法，遵循现有事件的模式
4. 在 `Perpetual` 状态中新增 `delta_sequence: u64` 字段，每个市场独立递增

**关键设计**：

```rust
pub struct OrderbookDeltaEvent {
    pub perpetual_id: u32,
    pub sequence: u64,           // 每市场独立递增，用于缺口检测
    pub updates: Vec<OrderbookDelta>,
    pub timestamp_ms: u64,
}

pub struct OrderbookDelta {
    pub side: u8,                // 0 = Bid, 1 = Ask
    pub price: u64,              // subticks
    pub quantity: u64,           // 0 表示该档位已移除
}
```

#### 文件 2: `dex-sui/sui-execution/src/dex/commands/order.rs`

**变更内容**：

| 函数 | 修改方式 |
|------|----------|
| `execute_place_order()` | 匹配前快照相关价格档位，匹配后对比差异，emit delta event |
| `execute_cancel_order()` | 记录撤单前该价格档位的数量，撤单后 emit delta |
| `execute_cancel_all_orders()` | 记录所有被撤订单涉及的价格档位变更，一次性 emit delta |
| `execute_place_order_with_eip712()` | 同 `execute_place_order()` |
| `execute_cancel_order_with_eip712()` | 同 `execute_cancel_order()` |

**Delta 捕获逻辑**（以 `execute_place_order` 为例）：

```
1. 记录匹配前的相关价格档位（best bid/ask 附近）
2. 执行撮合逻辑（match_order）
3. 对比匹配前后的价格档位差异
4. 构造 OrderbookDeltaEvent（仅包含变更的档位）
5. perpetual.delta_sequence += 1
6. emit delta event
```

**现有事件处理**（Q3=C+ 决策变更）：
- `OrderbookSnapshotEvent` **停止发射**（移除 `emit_event()` 调用）
- `OrderbookDeltaEvent` **不写入 TransactionEvents**（不调用 `emit_event()`）
- Delta 数据通过 DexStreamingManager → gRPC → dex-streamer 流转
- 恢复机制：dex-streamer 调用 gRPC `GetSnapshot()` 从 InlineOrderbook 读取完整 L2

**引擎执行流程变更**：
```
execute_place_order():
  1. 记录匹配前 InlineOrderbook 相关价格档位
  2. 执行撮合逻辑（match_order）
  3. 对比匹配前后差异 → 构造 OrderbookDeltaEvent
  4. perpetual.delta_sequence += 1
  5. 将 delta 写入执行结果（NOT TransactionEvents）
  6. ※ 移除 OrderbookSnapshotEvent 的 emit_event() 调用
```

### 2.3 验证标准

| 验证项 | 方法 | 通过标准 |
|--------|------|---------|
| 类型定义 | 单元测试 roundtrip | `BCS serialize → deserialize` 无损 |
| Delta 计算 | sim test | delta 计算正确（可通过全量快照交叉校验） |
| 事件大小 | 单元测试 | 单个 delta event < 100 bytes（典型场景 ~60 bytes） |
| sequence 递增 | sim test | 同市场连续操作的 sequence 严格递增 |
| Snapshot 已移除 | sim test | `OrderbookSnapshotEvent` 不再出现在 TransactionEvents 中 |
| Checkpoint 管道不受影响 | sim test | fills/orders/positions 等非订单簿事件正常处理 |

### 2.4 关键风险

| 风险 | 缓解措施 |
|------|---------|
| 移除 OrderbookSnapshotEvent 影响 dex-indexer | dex-indexer 的 orderbook_snapshots handler 不再收到事件，需确认不影响其他功能 |
| delta_sequence 在 Perpetual 状态增加存储 | 仅 8 bytes（u64），影响可忽略 |
| 撮合逻辑路径复杂，delta 捕获遗漏 | 测试中对比全量快照验证（InlineOrderbook.to_snapshot() vs 累积 delta） |

---

## 3. Step 2: 流式源 + gRPC — DexStreamingManager（3-4 天）

### 3.1 目标

在 sui-core 层拦截 DEX 执行结果（包括 Delta 和其他事件），通过内嵌的 gRPC server 提供流式推送和快照查询。这是 Q7=B 决策的核心实现。

### 3.2 修改文件

#### 文件 1: `sui/crates/sui-core/src/dex_streaming.rs` — 新建

**核心实现**：

```rust
pub struct DexStreamingManager {
    /// 内部 broadcast channel 用于多订阅者扇出
    broadcast_tx: broadcast::Sender<DexStreamBatch>,
    /// gRPC server handle
    grpc_handle: Option<JoinHandle<()>>,
    /// 引用 InlineOrderbook（用于 GetSnapshot）
    orderbook_reader: Arc<dyn OrderbookSnapshotReader>,
    metrics: DexStreamingMetrics,
}

/// gRPC 服务定义
service DexStreaming {
    rpc Subscribe(SubscribeRequest) returns (stream DexStreamBatch);
    rpc GetSnapshot(SnapshotRequest) returns (L2BookSnapshot);
}

impl DexStreamingManager {
    /// 从执行结果中提取 DEX 事件，通过 broadcast channel 推送
    /// Delta 事件不来自 TransactionEvents，而是从执行层直接传递
    pub fn process_dex_execution(
        &self,
        tx_digest: &TransactionDigest,
        timestamp_ms: u64,
        events: &TransactionEvents,       // Fill + OrderUpdate
        delta: Option<OrderbookDeltaEvent>, // 直接从执行层传递，不在 TransactionEvents 中
    );

    /// GetSnapshot: 从 InlineOrderbook 读取完整 L2 快照
    pub async fn get_snapshot(&self, market_id: u32) -> Result<L2BookSnapshot>;
}
```

**关键行为**：
- 按 `package_id == DEX_EVENTS_PACKAGE` 过滤 TransactionEvents 中的 Fill/OrderUpdate
- Delta 事件直接从执行层参数传递（不在 TransactionEvents 中）
- `process_dex_execution()` 不得阻塞执行路径（使用 broadcast `send()` 非阻塞）
- gRPC server 在单独的 tokio task 中运行
- `GetSnapshot()` 从 InlineOrderbook 直接读取，返回完整 L2 快照

#### 文件 2: `sui/crates/sui-core/src/authority.rs`

**变更内容**：

1. 在 `AuthorityState` 中新增字段：
   ```rust
   dex_streaming: Option<Arc<DexStreamingManager>>,
   ```

2. 在 `post_process_one_tx()` 中，紧跟 `subscription_handler.process_tx()` 之后调用：
   ```rust
   if let Some(ref dex_streaming) = self.dex_streaming {
       dex_streaming.process_dex_execution(tx_digest, timestamp_ms, events, delta);
   }
   ```
   其中 `delta` 由执行层计算后通过执行结果传递。

3. 在 `AuthorityState::new()` 中根据配置决定是否初始化 DexStreamingManager + gRPC server

#### 文件 3: Proto 定义 — `dex-sui/proto/dex_streaming.proto` — 新建

```protobuf
service DexStreaming {
    rpc Subscribe(SubscribeRequest) returns (stream DexStreamBatch);
    rpc GetSnapshot(SnapshotRequest) returns (L2BookSnapshot);
}

message SubscribeRequest {
    repeated uint64 market_ids = 1;  // 可选：过滤市场
}

message SnapshotRequest {
    uint64 market_id = 1;
}

message L2BookSnapshot {
    uint64 market_id = 1;
    repeated PriceLevel bids = 2;
    repeated PriceLevel asks = 3;
    uint64 sequence = 4;
    uint64 timestamp_ms = 5;
}
```

#### 文件 4: 配置变更

- 节点配置新增 `enable_dex_streaming: bool`，默认 `false`（opt-in）
- gRPC 监听地址配置：`dex_stream_grpc_addr: SocketAddr`，默认 `0.0.0.0:50051`
- broadcast channel 容量配置：`dex_stream_buffer_size: usize`，默认 `10000`

### 3.3 验证标准

| 验证项 | 方法 | 通过标准 |
|--------|------|---------|
| 事件过滤正确 | 单元测试 | 仅 DEX 事件被处理，非 DEX 事件忽略 |
| gRPC Subscribe | 集成测试 | gRPC 客户端在 tx 执行后收到 DexStreamBatch |
| gRPC GetSnapshot | 集成测试 | 返回 InlineOrderbook 的完整 L2 快照 |
| 延迟 | 基准测试 | 执行完成到 gRPC stream push < 5ms |
| 不阻塞执行 | 压力测试 | 无消费者时、消费者落后时，执行路径无阻塞 |
| 功能开关 | 单元测试 | `enable_dex_streaming: false` 时无额外开销 |

### 3.4 关键风险

| 风险 | 缓解措施 |
|------|---------|
| 修改 authority.rs 引入 bug | 仅增加一个 if-let 调用，不修改现有逻辑 |
| gRPC server 占用额外端口 | 可配置端口，默认关闭（opt-in） |
| InlineOrderbook 读取竞争 | GetSnapshot 使用只读快照，不影响执行路径 |
| tonic 依赖增加编译时间 | 仅在 enable_dex_streaming 时引入 |

---

## 4. ~~Step 3: 传输层~~ — 已合并到 Step 2

> **2026-02-27 决策变更**：根据 Q7=B（gRPC 一步到位），原 Step 3（StreamTransport trait + BroadcastTransport）已合并到 Step 2。gRPC server 内嵌在 DexStreamingManager 中，dex-streamer 作为独立服务通过 gRPC 连接。不再需要独立的传输层抽象 Step。

---

## 5. Step 3（原 Step 4）: 消费者层 — dex-streamer 独立服务（3-4 天）

### 5.1 目标

新建 `dex-streamer` crate 作为**独立 Docker 服务**（Q7=B），通过 gRPC `Subscribe()` 消费 DexStreamingManager 的事件流，在内存中维护增量 L2 订单簿，并写入 Redis 供 dex-api 读取。断线/gap 恢复通过 gRPC `GetSnapshot()` 从 InlineOrderbook 获取完整 L2 快照（Q3=C+）。

### 5.2 新建文件结构

```
dex-sui/crates/dex-streamer/
├── Cargo.toml
├── src/
│   ├── main.rs               # 启动入口（独立二进制）
│   ├── lib.rs                 # 库入口
│   ├── config.rs              # 配置（gRPC addr, Redis URL, flush interval 等）
│   ├── grpc_client.rs         # gRPC 客户端（Subscribe + GetSnapshot）
│   ├── orderbook_builder.rs   # 核心：内存 L2 订单簿
│   ├── bbo_tracker.rs         # BBO（最优买卖价）变更检测
│   ├── redis_writer.rs        # Redis 写入（pipelined）
│   └── reconciler.rs          # 周期性对账（调用 GetSnapshot vs 内存 L2Book）
```

### 5.3 关键组件实现

#### gRPC Client (`grpc_client.rs`) — 新增

**职责**：连接 DexStreamingManager gRPC server，提供 Subscribe 和 GetSnapshot。

```rust
pub struct DexStreamingClient {
    client: DexStreamingClient<tonic::transport::Channel>,
}

impl DexStreamingClient {
    /// 连接 gRPC server
    pub async fn connect(addr: &str) -> Result<Self>;

    /// 订阅事件流（返回 tonic::Streaming）
    pub async fn subscribe(&self, market_ids: Vec<u64>) -> Result<Streaming<DexStreamBatch>>;

    /// 获取快照（从 InlineOrderbook 读取）
    pub async fn get_snapshot(&self, market_id: u64) -> Result<L2BookSnapshot>;
}
```

#### OrderbookBuilder (`orderbook_builder.rs`)

**职责**：接收 `OrderbookDeltaEvent`，维护内存中的 L2 订单簿。

```rust
pub struct OrderbookBuilder {
    /// 每个市场的 L2 订单簿: perpetual_id → L2Book
    books: HashMap<u32, L2Book>,
    /// 每个市场的最新 sequence
    sequences: HashMap<u32, u64>,
    /// gRPC 客户端（用于 GetSnapshot 恢复）
    grpc_client: DexStreamingClient,
}

pub struct L2Book {
    pub bids: BTreeMap<u64, u64>,   // price → quantity (降序遍历)
    pub asks: BTreeMap<u64, u64>,   // price → quantity (升序遍历)
}

impl OrderbookBuilder {
    /// 应用 delta 事件，返回是否需要恢复
    pub fn apply_delta(&mut self, event: &OrderbookDeltaEvent) -> ApplyResult;

    /// 从 gRPC GetSnapshot 恢复（Q3=C+ 核心恢复机制）
    pub async fn recover_from_grpc(&mut self, market_id: u32) -> Result<()>;

    /// 获取当前 L2 快照（用于 Redis 全量写入和 WS 推送）
    pub fn get_snapshot(&self, perpetual_id: u32, depth: usize) -> Option<L2Snapshot>;
}

pub enum ApplyResult {
    Ok,
    SequenceGap { expected: u64, got: u64 },  // 需要 gRPC GetSnapshot 恢复
}
```

**Gap 检测与恢复逻辑**（Q3=C+ 变更）：
1. 收到 delta event 时检查 `event.sequence == expected_sequence + 1`
2. 若不连续，标记该市场为 `NeedRecovery`
3. 调用 gRPC `GetSnapshot(market_id)` 从 InlineOrderbook 获取完整 L2 快照
4. ~~从 Checkpoint 通道获取~~ — 不再需要（Q3=C+ 决策）
5. 恢复后重置 sequence，继续增量应用

#### RedisWriter (`redis_writer.rs`)

**职责**：将 OrderbookBuilder 的状态写入 Redis。

**Redis 数据结构**：

| Key | Type | 说明 |
|-----|------|------|
| `dex:l2book:{perpetual_id}` | Hash | L2 订单簿：field=`{side}:{price}`, value=`{quantity}` |
| `dex:bbo:{perpetual_id}` | Hash | 最优买卖价（best_bid, best_ask, mid_price） |
| `dex:stream:l2:update` | Stream | L2 更新通知流（供 dex-api WS 消费） |
| `dex:l2book:{perpetual_id}:meta` | Hash | 元数据: sequence, timestamp |

**写入策略**：
- 使用 Redis pipeline 批量写入，减少 RTT
- `dex:l2book:{id}` 每次 delta 后增量 HSET/HDEL（仅变化档位）
- `dex:stream:l2:update` 使用 XADD 追加更新通知（触发 WS 推送）
- `dex:bbo:{id}` 仅在 BBO 变更时更新

#### BboTracker (`bbo_tracker.rs`)

**职责**：检测 BBO（Best Bid/Offer）变更，避免无变化时重复写入 Redis。

```rust
pub struct BboTracker {
    bbos: HashMap<u32, Bbo>,
}

pub struct Bbo {
    pub best_bid: Option<(u64, u64)>,  // (price, quantity)
    pub best_ask: Option<(u64, u64)>,
}

impl BboTracker {
    /// 检查 L2Book 的 BBO 是否与上次不同
    pub fn check_and_update(&mut self, perpetual_id: u32, book: &L2Book) -> Option<Bbo>;
}
```

#### Reconciler (`reconciler.rs`) — 变更

**职责**（Q3=C+ 变更）：周期性调用 gRPC `GetSnapshot()` 从 InlineOrderbook 获取快照，与内存 L2Book 对比。

**对账逻辑**（已更新）：
1. 每 30s（可配置）调用 gRPC `GetSnapshot(market_id)` 获取 InlineOrderbook 快照
2. ~~从 Checkpoint Redis 读取全量快照~~ — 不再需要
3. 与 OrderbookBuilder 内存中的 L2Book 逐档位对比
4. 若不一致，记录 metric + warn 日志
5. 超过阈值（如连续 3 次不一致）触发强制恢复（用 GetSnapshot 数据替换）

### 5.4 验证标准

| 验证项 | 方法 | 通过标准 |
|--------|------|---------|
| delta 应用 | 单元测试 | apply_delta 后 L2Book 状态正确 |
| gap 检测 | 单元测试 | sequence 不连续时返回 SequenceGap |
| gRPC GetSnapshot 恢复 | 集成测试 | 从 InlineOrderbook 获取快照后继续增量应用正确 |
| Redis 写入 | 集成测试 | `dex:l2book:{id}` 和 `dex:bbo:{id}` 数据正确 |
| 全链路 | 集成测试 | DexStreamingManager → gRPC → dex-streamer → Redis 端到端 |
| BBO 变更检测 | 单元测试 | 仅 BBO 变化时写入 Redis |
| 对账 | 集成测试 | gRPC GetSnapshot 与内存 L2Book 比对后发现不一致时自动恢复 |

### 5.5 关键风险

| 风险 | 缓解措施 |
|------|---------|
| dex-streamer 崩溃导致数据缺失 | 重启后调用 gRPC GetSnapshot 恢复，delta 重新应用 |
| gRPC 连接中断 | 自动重连 + 重连后 GetSnapshot 恢复 |
| 内存占用过大（大量市场） | 仅维护活跃市场的 L2Book，不活跃市场定期淘汰 |
| Redis 写入延迟影响端到端延迟 | pipeline 批量写入 + 监控写入耗时 |

---

## 6. Step 4（原 Step 5）: API 集成 — dex-api 增强（2-3 天）

### 6.1 目标

dex-api 支持从 dex-streamer 的 Redis 数据（`dex:l2book:{id}`）读取低延迟订单簿，WS l2Book 频道改为推送**完整 L2 快照**（Q2=A，对标 Hyperliquid）。

### 6.2 修改文件

#### 文件 1: `dex-sui/crates/dex-api/src/handlers.rs`

**变更内容**：

1. `l2Book` handler 优先从 `dex:l2book:{id}`（dex-streamer Redis）读取，fallback 到 Checkpoint Redis
2. 新增 `bbo` handler，从 `dex:bbo:{id}` 读取最优买卖价

**Fallback 逻辑**（不变）：

```
l2Book 请求
    ↓
读取 dex:l2book:{id}（dex-streamer Redis）
    ↓ 若为空或过期
读取 dex:orderbook:{id}（Checkpoint Redis，现有路径）
    ↓
返回结果
```

#### 文件 2: `dex-sui/crates/dex-api/src/ws/consumer.rs`

**变更内容**（Q2=A 全量推送）：

1. 在 `StreamConsumer` 中新增对 `dex:stream:l2:update` 的消费
2. 收到更新通知后，从 `dex:l2book:{id}` 读取**完整 L2 快照**推送给客户端
3. **不需要 l2BookDelta 增量频道**（Q2=A 简化了 WS 推送逻辑）

**推送流程**（对标 Hyperliquid l2Book）：

```
dex-streamer Redis XADD dex:stream:l2:update
    ↓ StreamConsumer XREAD
收到通知: {perpetualId: 0, sequence: 42}
    ↓
从 Redis HGETALL dex:l2book:0 读取完整 L2
    ↓
构造全量快照消息（与 Hyperliquid 格式对标）：
{
  "channel": "l2Book",
  "data": {
    "coin": "BTC-USDC",
    "levels": [
      [{"px": "50000", "sz": "1500", "n": 1}, ...],  // bids
      [{"px": "50100", "sz": "800", "n": 1}, ...]     // asks
    ],
    "time": 1709000000000
  }
}
    ↓
推送给 l2Book:{perpetualId} 订阅者
```

#### 文件 3: `dex-sui/crates/dex-api/src/ws/types.rs`

**变更内容**（Q2=A 简化）：

- ~~新增 `l2BookDelta` 订阅类型~~ — 不需要（Q2=A 全量推送）
- 现有 `l2Book` / `orderbook` 频道切换数据源到 `dex:l2book:{id}`（低延迟）
- 新增 `bbo` REST endpoint（`dex:bbo:{id}`）

**WS l2Book 频道行为变更**：
- 数据源从 `dex:stream:orderbook`（Checkpoint，1-3s）切换到 `dex:stream:l2:update`（dex-streamer，<50ms）
- 推送格式保持全量快照不变（向后兼容）
- 推送频率可限制为至少 0.5s 间隔（对标 Hyperliquid）

### 6.3 WS/API 详细调整清单

以下列出 Step 4 中 dex-api 需要进行的所有 WS 和 REST API 调整：

#### 6.3.1 Redis 数据源迁移

| 数据类型 | 现有 Key (dex-indexer) | 新 Key (dex-streamer) | 迁移策略 |
|----------|----------------------|----------------------|----------|
| L2 订单簿 | `dex:orderbook:{id}` (Hash: bids/asks/timestamp_ms) | `dex:l2book:{id}` (Hash: `b:{price}`/`a:{price}` 字段) | 优先读新 key，fallback 读旧 key |
| BBO | 无（从 orderbook 计算） | `dex:bbo:{id}` (Hash: best_bid/best_ask/mid_price) | 新增 REST endpoint |
| L2 更新通知 | `dex:stream:orderbook` (Stream) | `dex:stream:l2:update` (Stream) | StreamConsumer 新增监听 |
| L2 元数据 | 无 | `dex:l2book:{id}:meta` (Hash: sequence/timestamp) | 新增，用于过期检测 |

**新旧 Redis Key 共存策略**：
- Phase 6 部署后，`dex:orderbook:{id}`（Checkpoint 通道）和 `dex:l2book:{id}`（流式通道）同时存在
- dex-api 优先读取 `dex:l2book:{id}`，若为空或 meta.timestamp 过期（>10s）则 fallback 到 `dex:orderbook:{id}`
- 过渡期结束后（确认流式通道稳定），可关闭 dex-indexer 的 orderbook_snapshots handler

#### 6.3.2 StreamConsumer 变更

**现有架构**（`dex-api/src/ws/consumer.rs`）：
- StreamConsumer 消费 8 条 Redis Streams（含 `dex:stream:orderbook`）
- `broadcast_orderbook()` 从 Stream 消息体直接解析完整快照并推送

**Phase 6 变更**：
- 新增消费 `dex:stream:l2:update`（dex-streamer 写入的通知流）
- 收到通知后，执行 `HGETALL dex:l2book:{perpetual_id}` 读取完整 L2 数据
- 将 Hash 字段（`b:{price}` → bid, `a:{price}` → ask）转换为现有 WS 消息格式
- 推送给 `l2Book:{perpetual_id}` 订阅者（全量快照）
- **可选**：增加最小推送间隔（如 50-100ms），避免高频交易场景下压垮客户端

**消费优先级**：
- 若同时收到 `dex:stream:l2:update` 和 `dex:stream:orderbook` 的消息，优先处理前者
- 低延迟通道（`dex:stream:l2:update`）消息到达后，忽略同一 perpetual_id 的 Checkpoint 通道消息（避免"时间倒退"）

#### 6.3.3 WS 消息格式兼容性

**现有 WS l2Book 消息格式**（保持不变）：
```json
{
  "channel": "l2Book",
  "data": {
    "coin": "BTC-USDC",
    "levels": [
      [{"px": "50000", "sz": "1500", "n": 1}, ...],
      [{"px": "50100", "sz": "800", "n": 1}, ...]
    ],
    "time": 1709000000000
  }
}
```

**兼容性保证**：
- 消息结构完全不变（全量快照格式，Q2=A）
- `levels[0]` = bids（降序），`levels[1]` = asks（升序），与现有行为一致
- `time` 字段使用 dex-streamer Redis meta 中的 timestamp_ms
- 客户端（含 dex-test-panel）无需任何修改

#### 6.3.4 REST API 变更

| Endpoint | 变更 | 说明 |
|----------|------|------|
| `POST /info` (type: l2Book) | 数据源切换 | 优先 `dex:l2book:{id}`，fallback `dex:orderbook:{id}` |
| `POST /info` (type: bbo) | **新增** | 从 `dex:bbo:{id}` 读取最优买卖价 |
| `POST /info` (type: allMids) | 数据源扩展 | 可从 `dex:bbo:{id}` 获取 mid_price（更低延迟） |
| 其他 endpoints | 不变 | orderStatus、openOrders、userState 等不受影响 |

### 6.4 验证标准

| 验证项 | 方法 | 通过标准 |
|--------|------|---------|
| REST l2Book | API 测试 | 从 dex-streamer Redis 返回正确数据 |
| REST l2Book fallback | API 测试 | dex-streamer Redis 无数据时回退到 Checkpoint Redis |
| WS l2Book 全量推送 | WS 测试 | 每次订单簿变化推送完整 L2 快照 |
| WS bbo 订阅 | WS 测试 | 订阅后收到 BBO 变更 |
| Redis key 迁移 | 集成测试 | 新旧 key 共存时 fallback 逻辑正确，无"时间倒退" |
| 向后兼容 | 回归测试 | 现有 WS 客户端无需修改，仅延迟降低 |
| 推送频率 | 压力测试 | 高频交易下推送频率不超过 20Hz |

### 6.5 关键风险

| 风险 | 缓解措施 |
|------|---------|
| 全量推送带宽过大 | 100 档 L2 book ~3KB，10 市场 × 10Hz = ~300KB/s，可接受 |
| 高频推送压垮前端 | 服务端配置最小推送间隔（如 50ms，对标 Hyperliquid 0.5s） |
| dex-streamer Redis 不可用 | fallback 到 Checkpoint Redis（现有路径） |
| 新旧 Redis key 不一致 | 过渡期两条通道并存，fallback 保证服务可用；meta.timestamp 判断过期 |
| StreamConsumer "时间倒退" | 低延迟通道消息到达后，忽略同一 perpetual_id 的 Checkpoint 消息 |

---

## 7. Step 5（原 Step 6）: Docker 集成 & E2E 测试（2-3 天）

### 7.1 目标

全栈 Docker 环境中验证流式订单簿架构的端到端正确性和延迟。新增 dex-streamer 作为独立 Docker 服务。

### 7.2 修改文件

#### 文件 1: `docker/dex-dev/docker-compose.yml`

**新增服务**（dex-streamer 独立容器，Q7=B）：

```yaml
dex-streamer:
  build:
    context: ../..
    dockerfile: docker/dex-dev/Dockerfile.dex-streamer
  depends_on:
    redis:
      condition: service_healthy
    sui-node:
      condition: service_healthy
  environment:
    - GRPC_ADDR=sui-node:50051         # 连接 sui-node 内嵌 gRPC server
    - REDIS_URL=redis://redis:6379
    - FLUSH_INTERVAL_MS=5
    - RECONCILE_INTERVAL_SECS=30
  restart: unless-stopped
```

**sui-node 服务变更**：

```yaml
sui-node:
  # ... 现有配置 ...
  environment:
    - ENABLE_DEX_STREAMING=true        # 启用 DexStreamingManager
    - DEX_STREAM_GRPC_ADDR=0.0.0.0:50051
  ports:
    - "50051:50051"                    # gRPC 端口
```

#### 文件 2: `docker/dex-dev/Dockerfile.dex-streamer` — 新建

与现有 Dockerfile 模式一致（复制宿主编译的二进制）。

#### 文件 3: `docker/dex-dev/Makefile`

**新增命令**：

```makefile
rebuild-streamer:   ## 重建 dex-streamer 镜像
    docker compose build dex-streamer --no-cache
    docker compose up -d dex-streamer

logs-streamer:      ## 查看 dex-streamer 日志
    docker compose logs -f dex-streamer
```

### 7.3 测试组件适配

Phase 6 引入的流式订单簿架构影响三个测试组件，需在 Step 5 中一并适配。

#### 7.3.1 dex-test-panel（React 前端测试面板）

**影响评估**：**低** — API 向后兼容，无强制变更。

**现状**：dex-test-panel 是 React 19 + TypeScript + Vite 构建的前端测试面板，包含 16+ 个功能面板（OrderBookPanel、TradePanel、FaucetPanel 等）。通过 REST `/info` 调用 dex-api（l2Book、bbo 等），通过 WS 订阅 `orderbook:{id}`、`bbo:{id}` 等频道。

**适配策略**：

| 项目 | 说明 | 优先级 |
|------|------|--------|
| 现有面板兼容 | WS l2Book 数据源切换对客户端透明（全量快照格式不变），无需修改 | — |
| OrderBookPanel | 已有面板继续工作，延迟从 1-3s 降低到 <50ms | 无需修改 |
| 可选：BBO 面板 | 新增 BBO 独立展示面板（读取 `dex:bbo:{id}` 或 WS `bbo` 频道） | P2 |
| 可选：延迟监控面板 | 展示 dex-streamer metrics（gRPC 延迟、Redis 写入延迟等） | P3 |

**结论**：dex-test-panel 无需阻塞 Phase 6 进度，可选面板在后续迭代中添加。

#### 7.3.2 dex-indexer-e2e-test（Simulacrum E2E 测试框架）

**影响评估**：**中** — DexFullCluster 需扩展以纳入 dex-streamer。

**现状**：基于 Simulacrum 的 E2E 测试框架，`DexFullCluster` 组合了 Simulacrum + PostgreSQL + dex-indexer + dex-api，测试模块包括 api_balance_tests、api_order_tests、perpetual_tests 等。

**适配内容**：

| 项目 | 修改 | 说明 |
|------|------|------|
| DexFullCluster 扩展 | 在 cluster 中启动 dex-streamer（作为嵌入式组件或独立进程） | dex-streamer 需连接 gRPC + Redis |
| gRPC mock/embed | 在 Simulacrum 环境中提供 DexStreamingManager 的 gRPC endpoint | Simulacrum 不运行真正的 sui-node，需模拟或嵌入 gRPC server |
| 新增测试模块 | `l2book_streaming_tests.rs` — 验证 gRPC → dex-streamer → Redis → dex-api 全链路 | 核心验证场景 |
| 现有测试兼容 | api_order_tests 中 l2Book 相关断言需适配新 Redis key（`dex:l2book:{id}`） | fallback 逻辑保证现有测试不中断 |

**关键挑战**：Simulacrum 是内存中的模拟器，不运行真正的 sui-node 进程。DexStreamingManager 的 gRPC server 需要以下方案之一：
1. **方案 A**：在 DexFullCluster 中嵌入 DexStreamingManager（启动 gRPC server），Simulacrum 执行 DEX 交易后手动调用 `process_dex_execution()`
2. **方案 B**：Mock gRPC server，直接向 dex-streamer 注入测试数据
3. **推荐**：方案 A（更接近真实环境，测试覆盖更完整）

#### 7.3.3 dex-node-test（Live Node 测试库）

**影响评估**：**中** — Docker 环境变更 + 新增 Redis stream 验证。

**现状**：包含 DexClient、DexApiClient、RedisTestClient、WsTestClient 和 tx-gateway HTTP 服务器，提供 15+ 个 example 脚本用于 live node 测试。

**适配内容**：

| 项目 | 修改 | 说明 |
|------|------|------|
| Docker 环境更新 | dex-dev/dex-test 的 docker-compose.yml 新增 dex-streamer 服务 | 见 7.2 |
| sui-node 配置更新 | fullnode.yaml 新增 `enable_dex_streaming: true` + gRPC 端口 | sui-node 启动时开启 DexStreamingManager |
| RedisTestClient 扩展 | 新增读取 `dex:l2book:{id}`（Hash）和 `dex:bbo:{id}` 的方法 | 验证 dex-streamer Redis 输出 |
| WsTestClient 验证 | 验证 WS l2Book 频道延迟从 1-3s 降低到 <50ms | 延迟指标验证 |
| 新增 example 脚本 | `streaming_orderbook.rs` — 演示 gRPC Subscribe + L2Book 构建 | gRPC 客户端使用示例 |
| dex-test 端口映射 | dex-streamer gRPC 端口映射（如 50052:50051） | 与 dex-dev 的 50051 不冲突 |

### 7.4 E2E 测试场景（Q2=A 全量推送 + Q3=C+ gRPC 恢复）

| # | 场景 | 操作 | 验证标准 |
|---|------|------|---------|
| 1 | 下单更新 | 下限价单 | WS `l2Book` 收到完整 L2 快照更新，延迟 <50ms |
| 2 | 撤单更新 | 撤销已有挂单 | WS `l2Book` 快照中对应档位已消失 |
| 3 | 成交更新 | 下市价单触发成交 | WS `l2Book` 快照中 maker 侧档位数量减少 |
| 4 | gRPC Subscribe | 连续下单 10 笔 | dex-streamer 通过 gRPC 收到所有 delta，sequence 连续 |
| 5 | gRPC GetSnapshot 恢复 | 杀掉 dex-streamer，重启 | 重启后通过 gRPC GetSnapshot 从 InlineOrderbook 恢复 |
| 6 | 数据一致性 | 对比 gRPC 快照与 Redis | REST `l2Book` 数据与 gRPC GetSnapshot 一致 |
| 7 | BBO 推送 | 新订单改变最优价 | WS `bbo` 通道收到 BBO 变更通知 |
| 8 | 向后兼容 | 使用旧版 WS 订阅 | 现有 WS 客户端格式兼容，仅延迟降低 |
| 9 | Redis fallback | 停止 dex-streamer | REST l2Book 自动 fallback 到 Checkpoint Redis（`dex:orderbook:{id}`） |
| 10 | dex-test-panel 兼容 | 在前端面板操作下单/撤单 | OrderBookPanel 展示实时更新，无异常 |

### 7.5 延迟测量

**测量点**：

```
T0: 客户端发送下单请求（记录时间戳）
T1: 引擎执行完成（DexStreamingManager 收到事件）
T2: dex-streamer 收到 broadcast event
T3: Redis 写入完成
T4: dex-api WS 推送到客户端

目标: T4 - T1 < 50ms
```

**测量方法**：
- 在各环节打印 `timestamp_ms`
- 使用 dex-streamer 的 metrics 端点获取各阶段延迟分布
- Docker 环境下实测端到端延迟

---

## 8. 总体时间线（2026-02-27 更新）

| Step | 内容 | 预计时间 | 前置依赖 | 说明 |
|------|------|---------|---------|------|
| 1 | OrderbookDeltaEvent 类型 + Delta 计算 | 1-2 天 | 无 | 类型定义 + 引擎 delta 计算，不 emit 到 TransactionEvents |
| 2 | DexStreamingManager + gRPC server | 3-4 天 | Step 1 | 含原 Step 3 传输层，gRPC Subscribe + GetSnapshot |
| 3 | dex-streamer 独立服务 | 3-4 天 | Step 2 | gRPC 消费 + 内存 L2Book + Redis 写入 |
| 4 | dex-api 集成 | 2-3 天 | Step 3 | REST l2Book + WS 全量推送 + BBO |
| 5 | Docker & E2E 测试 | 2-3 天 | Step 3, 4 | 新增 dex-streamer Docker 服务 |
| **总计** | | **11-16 天** | | |

**关键路径**：

```
Step 1 (1-2天) → Step 2 (3-4天) → Step 3 (3-4天) → Step 4 (2-3天) → Step 5 (2-3天)

关键路径: 11-16 天
所有 Step 串行（每个 Step 依赖前一个的产出）
```

**vs 原方案对比**：
- 原方案 6 步 → 现 5 步（传输层合并到流式源）
- 总工期不变（11-16 天），但 Step 2 增加了 gRPC 实现（+1 天）
- 原 Step 3（BroadcastTransport）节省的 1 天抵消 gRPC 增量

---

## 9. 关键里程碑（2026-02-27 更新）

| Milestone | 对应 Step | 验证标准 | 预计完成 |
|-----------|----------|---------|---------|
| **M1: Delta 类型可用** | Step 1 | OrderbookDeltaEvent 类型定义 + 引擎 delta 计算正确，sim tests 通过 | 第 2 天 |
| **M2: gRPC 通道连通** | Step 2 | gRPC Subscribe 在执行后 <5ms 收到事件；GetSnapshot 返回 InlineOrderbook 数据 | 第 6 天 |
| **M3: Redis 增量可用** | Step 3 | dex-streamer 通过 gRPC 消费，`dex:l2book:{id}` 被填充，`dex:bbo:{id}` 正确更新 | 第 10 天 |
| **M4: API 可用** | Step 4 | REST l2Book 从 dex-streamer Redis 读取，WS l2Book 推送全量快照（低延迟） | 第 13 天 |
| **M5: E2E 通过** | Step 5 | Docker 全栈测试通过（含 dex-streamer 服务），端到端延迟 <50ms 已验证 | 第 16 天 |

---

## 10. 回滚方案

每个 Step 均可独立回滚，不影响现有服务：

| Step | 回滚方式 | 影响范围 |
|------|---------|---------|
| Step 1 | 恢复 OrderbookSnapshotEvent emit | 无影响（回到原始行为） |
| Step 2 | 设置 `enable_dex_streaming: false` | 无影响（gRPC server 不启动） |
| Step 3 | 停止 dex-streamer Docker 服务 | 回退到 Checkpoint-only 模式 |
| Step 4 | dex-api fallback 到 Checkpoint Redis | 现有 API 行为不变（延迟回退到 1-3s） |
| Step 5 | 从 docker-compose.yml 移除 dex-streamer 服务 | 回退到当前架构 |

**整体回滚策略**：关闭 `enable_dex_streaming` 配置 + 停止 dex-streamer 容器 + 恢复 OrderbookSnapshotEvent emit，系统回退到 Phase 5 的 Checkpoint-only 模式，对用户完全透明。

---

## 11. 后续优化方向（Phase 6.1+）

以下优化不在本次实施范围内，作为后续迭代方向记录：

| 优化项 | 说明 | 优先级 |
|--------|------|--------|
| WS 增量频道 | 新增 `l2BookDelta` WS 频道（首次快照 + 后续增量），为高性能客户端提供低带宽选择 | P1 |
| 推送频率控制 | 高频市场数据的 rate limiting / throttling（对标 Hyperliquid 0.5s 最小间隔） | P1 |
| L3 订单簿流 | 推送单笔订单级别的变更（而非聚合后的价格档位） | P2 |
| 市场数据聚合 | 在 dex-streamer 中计算 trades、VWAP 等衍生数据 | P2 |
| 多市场批量优化 | 多个市场的 delta 合并为单次 Redis pipeline 写入 | P2 |
| gRPC 多消费者 | 支持多个 dex-streamer 实例连接同一 gRPC server | P3 |
| 客户端 SDK | 封装 l2Book 订阅逻辑 | P3 |
