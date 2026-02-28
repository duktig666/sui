# Phase 6 Step 2: DexStreamingManager + gRPC 设计文档

> 创建日期: 2026-02-27
> 状态: 设计确认

## 目标

在 sui-node 执行 DEX 交易后，通过 gRPC streaming 将 OrderbookDeltaEvent 和其他 DEX 事件以微秒级延迟推送给外部消费者（dex-stream-indexer）。同时提供 GetSnapshot RPC 用于恢复完整 L2 订单簿。

## 架构概览

```
execute_place_order()  ──→ delta_events.push(delta)
execute_cancel_order() ──→ delta_events.push(delta)
         │
         ↓
DexExecutionResult { events, delta_events }
         │
         ↓
executor_trait.rs → InnerTemporaryStore.dex_delta_events
         │
         ↓
authority.rs post_process_one_tx()
         │
         ├─→ subscription_handler.process_tx()      [现有，不变]
         │
         └─→ dex_streaming.process_dex_execution()  [新增]
               │
               ├─ 过滤 TransactionEvents 中的 DEX 事件
               ├─ 合并 delta_events
               └─ broadcast_tx.send(DexStreamBatch)
                      │
                      ↓
               gRPC Subscribe stream → dex-stream-indexer (Step 3)
```

## 关键设计决策

| # | 决策 | 选择 | 理由 |
|---|------|------|------|
| 1 | Delta 线程化方式 | 扩展 DexExecutionResult | 显式、清晰，与现有 events 流一致 |
| 2 | Proto 和 gRPC 位置 | 新建 dex-node-stream-framework crate | 职责清晰，不侵入 sui-core |
| 3 | GetSnapshot 数据源 | AuthorityState.get_object() | 无需维护额外状态，直接读链上对象 |

## 组件设计

### 1. Delta 线程化

**变更链路：**

**1a. DexExecutionResult 扩展**
文件: `sui-execution/src/dex/mod.rs`
```rust
pub struct DexExecutionResult {
    pub written: WrittenObjects,
    pub changed_objects: BTreeMap<ObjectID, EffectsObjectChange>,
    pub status: ExecutionStatus,
    pub events: Vec<Event>,
    pub delta_events: Vec<OrderbookDeltaEvent>,  // 新增
}
```

**1b. execute_* 函数参数扩展**
文件: `sui-execution/src/dex/commands/order.rs`
- 5 个函数新增 `delta_events: &mut Vec<OrderbookDeltaEvent>` 参数
- `_delta_event` 改为 `delta_events.push(delta_event)`
- `execute_command()` 新增同参数并传递

**1c. InnerTemporaryStore 扩展**
文件: `sui-types/src/inner_temporary_store.rs`
```rust
pub struct InnerTemporaryStore {
    // ... 现有字段 ...
    pub dex_delta_events: Vec<OrderbookDeltaEvent>,  // 新增
}
```

**1d. executor_trait.rs 桥接**
文件: `sui-execution/src/dex/executor_trait.rs`
- 从 `DexExecutionResult.delta_events` 提取
- 写入 `InnerTemporaryStore.dex_delta_events`

**1e. authority.rs 消费**
文件: `sui-core/src/authority.rs`
- `post_process_one_tx()` 从 `inner_temporary_store.dex_delta_events` 读取
- 传给 `dex_streaming.process_dex_execution()`

### 2. dex-node-stream-framework crate

**目录结构：**
```
crates/dex-node-stream-framework/
├── Cargo.toml
├── build.rs                    # tonic-prost 编译 proto
├── proto/
│   └── dex_streaming.proto     # gRPC 服务定义
└── src/
    ├── lib.rs                  # 导出
    ├── manager.rs              # DexStreamingManager
    ├── grpc_server.rs          # Subscribe + GetSnapshot 实现
    └── proto/
        └── generated/          # build.rs 生成
```

**DexStreamingManager 核心结构：**
```rust
pub struct DexStreamingManager {
    broadcast_tx: broadcast::Sender<DexStreamBatch>,
    grpc_handle: Option<JoinHandle<()>>,
}

pub struct DexStreamBatch {
    pub tx_digest: TransactionDigest,
    pub timestamp_ms: u64,
    pub events: Vec<DexStreamEvent>,
}

pub enum DexStreamEvent {
    Fill(FillEvent),
    OrderUpdate(OrderUpdateEvent),
    OrderPlaced(OrderPlacedEventV1),
    OrderRemoved(OrderRemovedEventV1),
    OrderbookDelta(OrderbookDeltaEvent),
}
```

**核心方法：**
- `new(config) → Self` — 创建 manager 和 broadcast channel
- `start_grpc_server(authority: Arc<AuthorityState>) → JoinHandle` — 启动 gRPC server
- `process_dex_execution(tx_digest, timestamp_ms, events, delta_events)` — 过滤+推送

### 3. gRPC 服务定义

```protobuf
syntax = "proto3";
package dex.streaming.v1;

service DexStreaming {
    rpc Subscribe(SubscribeRequest) returns (stream DexStreamBatchProto);
    rpc GetSnapshot(SnapshotRequest) returns (L2BookSnapshot);
}

message SubscribeRequest {
    repeated uint32 market_ids = 1;
}

message SnapshotRequest {
    uint32 market_id = 1;
}

message L2BookSnapshot {
    uint32 market_id = 1;
    repeated PriceLevelProto bids = 2;
    repeated PriceLevelProto asks = 3;
    uint64 sequence = 4;
    uint64 timestamp_ms = 5;
}

message PriceLevelProto {
    uint64 price = 1;
    uint64 quantity = 2;
}

message DexStreamBatchProto {
    bytes tx_digest = 1;
    uint64 timestamp_ms = 2;
    repeated DexStreamEventProto events = 3;
}

message DexStreamEventProto {
    oneof event {
        OrderbookDeltaEventProto orderbook_delta = 1;
        FillEventProto fill = 2;
        OrderUpdateEventProto order_update = 3;
        OrderPlacedEventProto order_placed = 4;
        OrderRemovedEventProto order_removed = 5;
    }
}

message OrderbookDeltaEventProto {
    uint32 perpetual_id = 1;
    uint64 sequence = 2;
    repeated OrderbookDeltaProto updates = 3;
    uint64 timestamp_ms = 4;
}

message OrderbookDeltaProto {
    uint32 side = 1;
    uint64 price = 2;
    uint64 quantity = 3;
}

message FillEventProto {
    uint32 perpetual_id = 1;
    bytes taker_order_id = 2;
    bytes maker_order_id = 3;
    bytes taker_subaccount = 4;
    bytes maker_subaccount = 5;
    uint32 side = 6;
    uint64 price = 7;
    uint64 quantity = 8;
    int64 taker_fee = 9;
    int64 maker_fee = 10;
    uint64 timestamp_ms = 11;
}

message OrderUpdateEventProto {
    uint32 perpetual_id = 1;
    bytes order_id = 2;
    bytes subaccount = 3;
    uint32 side = 4;
    uint64 price = 5;
    uint64 original_quantity = 6;
    uint64 filled_quantity = 7;
    uint32 status = 8;
    uint32 time_in_force = 9;
    bool reduce_only = 10;
    uint64 timestamp_ms = 11;
}

message OrderPlacedEventProto {
    uint32 perpetual_id = 1;
    bytes order_id = 2;
    bytes subaccount = 3;
    uint32 side = 4;
    uint64 price = 5;
    uint64 quantity = 6;
    uint32 order_type = 7;
    uint32 time_in_force = 8;
    bool reduce_only = 9;
    uint64 client_id = 10;
    uint64 timestamp_ms = 11;
}

message OrderRemovedEventProto {
    uint32 perpetual_id = 1;
    bytes order_id = 2;
    bytes subaccount = 3;
    uint32 reason = 4;
    uint64 remaining_quantity = 5;
    uint64 timestamp_ms = 6;
}
```

### 4. NodeConfig 扩展

文件: `sui-config/src/node.rs`
```rust
pub struct NodeConfig {
    // ... 现有 ...
    #[serde(default)]
    pub dex_streaming: Option<DexStreamingConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DexStreamingConfig {
    pub grpc_address: SocketAddr,
    #[serde(default = "default_channel_capacity")]
    pub channel_capacity: usize,  // 默认 1024
}
```

### 5. 启动流程

在 AuthorityState 初始化时（authority.rs `new()` 或 `new_for_testing()`）:
```
if let Some(config) = node_config.dex_streaming {
    let manager = DexStreamingManager::new(config.channel_capacity);
    let handle = manager.start_grpc_server(config.grpc_address, authority_arc.clone());
    authority.dex_streaming = Some(Arc::new(manager));
}
```

### 6. GetSnapshot 实现

通过 AuthorityState 读取 PerpetualState 对象：
```rust
async fn get_snapshot(&self, request: SnapshotRequest) -> L2BookSnapshot {
    let perpetual_id = request.market_id;
    let object_id = ... // 从已知的 PerpetualState object ID 映射
    let object = authority.get_object(&object_id).await?;
    let state = PerpetualState::from_bcs_bytes(&object.data)?;
    let snapshot = state.orderbook.build_snapshot(perpetual_id, now_ms());
    // 转换为 proto 格式
}
```

注意：需要一个 market_id → ObjectID 的映射。可以在 DexStreamingManager 启动时从链上读取 PerpetualCreatedEvent 建立，或通过配置传入。

## 受影响文件总览

| 操作 | 文件 | 说明 |
|------|------|------|
| 新建 | `crates/dex-node-stream-framework/` | 新 crate（proto + manager + grpc server） |
| 修改 | `sui-execution/src/dex/mod.rs` | DexExecutionResult + delta_events 参数 |
| 修改 | `sui-execution/src/dex/commands/order.rs` | 5 个函数 `_delta` → `delta_events.push()` |
| 修改 | `sui-execution/src/dex/executor_trait.rs` | delta_events → InnerTemporaryStore |
| 修改 | `sui-types/src/inner_temporary_store.rs` | 新增 dex_delta_events 字段 |
| 修改 | `sui-core/src/authority.rs` | dex_streaming 字段 + post_process_one_tx 调用 |
| 修改 | `sui-config/src/node.rs` | DexStreamingConfig |
| 修改 | 根 `Cargo.toml` | workspace members 新增 dex-node-stream-framework |

## 验证标准

| 指标 | 标准 |
|------|------|
| gRPC Subscribe 延迟 | 执行后 <5ms 收到事件 |
| GetSnapshot 正确性 | 返回完整 L2 数据 |
| 不阻塞执行路径 | broadcast send 是非阻塞的 |
| 无订阅者时零开销 | receiver_count() == 0 直接返回 |
| 向后兼容 | dex_streaming: None 时完全无影响 |
