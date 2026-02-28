# Phase 6 Step 2: DexStreamingManager + gRPC 实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 DEX 执行层产生的 OrderbookDeltaEvent 和其他事件通过 gRPC streaming 推送给外部消费者，延迟 <5ms。

**Architecture:** 扩展 DexExecutionResult 增加 delta_events 字段，经 InnerTemporaryStore 传到 authority.rs 的 post_process_one_tx()，由新建的 dex-node-stream-framework crate 提供 DexStreamingManager（broadcast channel + gRPC server）。

**Tech Stack:** Rust, tonic 0.14.2, prost 0.14.1, tokio broadcast channel

**设计文档:** `docs/plans/2026-02-27-phase6-step2-dex-streaming-design.md`

---

## Task 1: DexExecutionResult 扩展 delta_events 字段

**Files:**
- Modify: `sui-execution/src/dex/result.rs:17-26`
- Modify: `sui-execution/src/dex/mod.rs:86-132` (execute_programmable_dex_transaction)
- Modify: `sui-execution/src/dex/mod.rs:138-145` (execute_command signature)
- Modify: `sui-execution/src/dex/commands/order.rs` (5 处 `_delta_event` → `delta_events.push()`)

**Step 1: 修改 DexExecutionResult，添加 delta_events 字段**

文件: `sui-execution/src/dex/result.rs`

在 import 区添加:
```rust
use sui_types::dex_events::OrderbookDeltaEvent;
```

在 struct 中 `events` 之后添加:
```rust
    /// Orderbook delta events for gRPC streaming (not in TransactionEvents)
    pub delta_events: Vec<OrderbookDeltaEvent>,
```

**Step 2: 修改 execute_command 签名，增加 delta_events 参数**

文件: `sui-execution/src/dex/mod.rs:138-145`

```rust
    fn execute_command(
        command: &DexCommand,
        inputs: &[sui_types::transaction::CallArg],
        ctx: &mut DexExecutionContext,
        written: &mut WrittenObjects,
        changed_objects: &mut BTreeMap<ObjectID, EffectsObjectChange>,
        events: &mut Vec<Event>,
        delta_events: &mut Vec<OrderbookDeltaEvent>,  // 新增
    ) -> Result<sui_types::dex::CommandResult, ExecutionError> {
```

需在文件顶部 import 区添加:
```rust
use sui_types::dex_events::OrderbookDeltaEvent;
```

**Step 3: 修改 execute_programmable_dex_transaction，创建并传递 delta_events**

文件: `sui-execution/src/dex/mod.rs:86-132`

在 `let mut events = Vec::new();`（line 105）之后添加:
```rust
        let mut delta_events = Vec::new();
```

在 `Self::execute_command()` 调用（lines 109-116）末尾添加 `&mut delta_events` 参数:
```rust
            let result = Self::execute_command(
                command,
                &dex_tx.inputs,
                &mut ctx,
                &mut written,
                &mut changed_objects,
                &mut events,
                &mut delta_events,  // 新增
            )
```

在 `Ok(DexExecutionResult { ... })`（lines 126-131）中添加:
```rust
        Ok(DexExecutionResult {
            written,
            changed_objects,
            status: ExecutionStatus::Success,
            events,
            delta_events,  // 新增
        })
```

**Step 4: 将 delta_events 参数传递给 5 个 order 命令**

文件: `sui-execution/src/dex/mod.rs`

在以下 5 个 `commands::execute_*` 调用中添加 `delta_events` 参数（仅限订单相关命令）:

1. `commands::execute_place_order(...)` (line ~311-320) → 添加 `delta_events,`
2. `commands::execute_cancel_order(...)` (line ~338-346) → 添加 `delta_events,`
3. `commands::execute_cancel_all_orders(...)` (line ~364-372) → 添加 `delta_events,`
4. `commands::execute_place_order_with_eip712(...)` (line ~394-403) → 添加 `delta_events,`
5. `commands::execute_cancel_order_with_eip712(...)` (line ~425-434) → 添加 `delta_events,`

非订单命令（CreateGlobalAccounts, CreatePerpetualState, MintCoin, Deposit, Withdraw, Transfer）不需要修改。

**Step 5: 修改 5 个 execute_* 函数签名并收集 delta events**

文件: `sui-execution/src/dex/commands/order.rs`

在 import 区添加:
```rust
use sui_types::dex_events::OrderbookDeltaEvent;
```

对每个函数:
1. 函数签名添加 `delta_events: &mut Vec<OrderbookDeltaEvent>,` 参数
2. 将 `let _delta_event = state.orderbook.build_delta(...)` 改为 `let delta_event = state.orderbook.build_delta(...)`
3. 在 build_delta 之后添加 `delta_events.push(delta_event);`

涉及的 5 个函数及 build_delta 位置:
- `execute_place_order` → line 517
- `execute_cancel_order` → line 1161
- `execute_cancel_all_orders` → line 1393
- `execute_place_order_with_eip712` → line 1549
- `execute_cancel_order_with_eip712` → line 1756

**Step 6: 运行编译验证**

Run: `cargo check -p sui-execution`
Expected: 编译成功

**Step 7: 运行测试**

Run: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-execution --lib`
Expected: 全部通过

**Step 8: Commit**

```bash
git add sui-execution/src/dex/result.rs sui-execution/src/dex/mod.rs sui-execution/src/dex/commands/order.rs
git commit -m "feat(dex): thread delta_events through DexExecutionResult and execute_command"
```

---

## Task 2: InnerTemporaryStore 扩展 + executor_trait 桥接

**Files:**
- Modify: `crates/sui-types/src/inner_temporary_store.rs:26-39`
- Modify: `sui-execution/src/dex/helpers.rs:170-204` (build_temporary_store)
- Modify: `sui-execution/src/dex/executor_trait.rs:77-104` (success path)

**Step 1: InnerTemporaryStore 添加 dex_delta_events 字段**

文件: `crates/sui-types/src/inner_temporary_store.rs`

在 import 区添加:
```rust
use crate::dex_events::OrderbookDeltaEvent;
```

在 struct InnerTemporaryStore 的 `lamport_version` 字段后添加:
```rust
    /// Orderbook delta events for gRPC streaming (DEX only, not in TransactionEvents)
    pub dex_delta_events: Vec<OrderbookDeltaEvent>,
```

**Step 2: 修复所有 InnerTemporaryStore 构造处**

添加新字段后，所有构造 InnerTemporaryStore 的地方都需要加上 `dex_delta_events: vec![]`。

搜索项目中所有 `InnerTemporaryStore {` 构造位置:

已知需修改:
- `sui-execution/src/dex/helpers.rs:192-203` (build_temporary_store)
- 其他位置通过编译错误发现并添加 `dex_delta_events: vec![]`

**Step 3: 修改 build_temporary_store 接受 delta_events 参数**

文件: `sui-execution/src/dex/helpers.rs:170-204`

添加 import:
```rust
use sui_types::dex_events::OrderbookDeltaEvent;
```

修改签名:
```rust
pub fn build_temporary_store(
    input_objects: CheckedInputObjects,
    written: WrittenObjects,
    lamport_version: SequenceNumber,
    events: TransactionEvents,
    dex_delta_events: Vec<OrderbookDeltaEvent>,  // 新增
) -> InnerTemporaryStore {
```

在构造体中添加:
```rust
    InnerTemporaryStore {
        input_objects: input_map,
        stream_ended_consensus_objects: BTreeMap::new(),
        mutable_inputs,
        written,
        loaded_runtime_objects: BTreeMap::new(),
        events,
        accumulator_events: vec![],
        binary_config: BinaryConfig::standard(),
        runtime_packages_loaded_from_db: BTreeMap::new(),
        lamport_version,
        dex_delta_events,  // 新增
    }
```

**Step 4: 修改 executor_trait.rs 的 success path，传递 delta_events**

文件: `sui-execution/src/dex/executor_trait.rs`

Success path (line 99-104):
```rust
                    let temp_store = build_temporary_store(
                        input_objects,
                        dex_result.written.clone(),
                        effects_lamport_version,
                        events,
                        dex_result.delta_events,  // 新增
                    );
```

Error path (line 129-134): 传空 vec:
```rust
                    let temp_store = build_temporary_store(
                        input_objects,
                        WrittenObjects::new(),
                        SequenceNumber::lamport_increment(gas.payment.iter().map(|(_, v, _)| *v)),
                        TransactionEvents::default(),
                        vec![],  // 新增：执行失败时没有 delta events
                    );
```

**Step 5: 编译验证**

Run: `cargo check -p sui-execution -p sui-types`
Expected: 可能有其他 crate 中 InnerTemporaryStore 构造编译错误

**Step 6: 修复其他 InnerTemporaryStore 构造处**

Run: `cargo check 2>&1 | head -50`

对编译报错中每个 `InnerTemporaryStore` 构造位置，添加 `dex_delta_events: vec![]` 字段。
常见位置:
- `sui-execution/latest/sui-adapter/src/execution_engine.rs`
- `sui-execution/latest/sui-move-natives/src/test_scenario.rs`
- `crates/sui-types/src/inner_temporary_store.rs` (可能有 test 构造)
- 其他版本的 execution engine (v0, v1, v2)

**Step 7: 全量编译验证**

Run: `cargo check`
Expected: 编译成功

**Step 8: 运行测试**

Run: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-types -p sui-execution --lib`
Expected: 全部通过

**Step 9: Commit**

```bash
git add crates/sui-types/src/inner_temporary_store.rs sui-execution/src/dex/helpers.rs sui-execution/src/dex/executor_trait.rs
# 加上所有其他修改的文件
git commit -m "feat(dex): thread delta_events through InnerTemporaryStore"
```

---

## Task 3: 新建 dex-node-stream-framework crate（Proto + 基础结构）

**Files:**
- Create: `crates/dex-node-stream-framework/Cargo.toml`
- Create: `crates/dex-node-stream-framework/build.rs`
- Create: `crates/dex-node-stream-framework/proto/dex_streaming.proto`
- Create: `crates/dex-node-stream-framework/src/lib.rs`
- Create: `crates/dex-node-stream-framework/src/proto.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: 创建目录结构**

```bash
mkdir -p crates/dex-node-stream-framework/proto crates/dex-node-stream-framework/src
```

**Step 2: 创建 Cargo.toml**

文件: `crates/dex-node-stream-framework/Cargo.toml`

```toml
[package]
name = "dex-node-stream-framework"
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"
publish = false

[dependencies]
sui-types.workspace = true
tokio = { workspace = true, features = ["sync", "rt"] }
tonic.workspace = true
tonic-prost.workspace = true
prost.workspace = true
tokio-stream.workspace = true
tracing.workspace = true

[build-dependencies]
tonic-prost-build.workspace = true
protox.workspace = true
```

**Step 3: 创建 proto 文件**

文件: `crates/dex-node-stream-framework/proto/dex_streaming.proto`

完整 proto 内容见设计文档 `docs/plans/2026-02-27-phase6-step2-dex-streaming-design.md` 第 3 节。

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

**Step 4: 创建 build.rs**

文件: `crates/dex-node-stream-framework/build.rs`

```rust
use std::path::PathBuf;

fn main() {
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = crate_dir.join("proto");
    let out_dir = crate_dir.join("src/generated");

    println!("cargo:rerun-if-changed={}", proto_dir.display());

    std::fs::create_dir_all(&out_dir).expect("create generated dir");

    let proto_files = vec![proto_dir.join("dex_streaming.proto")];

    let file_descriptors =
        protox::compile(&proto_files, [&proto_dir]).expect("failed to compile proto");

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .out_dir(&out_dir)
        .compile_fds(file_descriptors)
        .expect("failed to generate gRPC code");
}
```

**Step 5: 创建 src/proto.rs**

文件: `crates/dex-node-stream-framework/src/proto.rs`

```rust
// Generated code from proto/dex_streaming.proto
#[allow(clippy::all)]
pub mod dex_streaming_v1 {
    include!("generated/dex.streaming.v1.rs");
}
```

**Step 6: 创建 src/lib.rs**

文件: `crates/dex-node-stream-framework/src/lib.rs`

```rust
pub mod proto;
```

**Step 7: 添加到 workspace members**

文件: `Cargo.toml` (workspace root)

在 `"crates/dex-types",`（line 115）之后添加:
```toml
  "crates/dex-node-stream-framework",
```

**Step 8: 编译验证 proto 生成**

Run: `cargo check -p dex-node-stream-framework`
Expected: proto 生成成功，crate 编译通过

**Step 9: Commit**

```bash
git add crates/dex-node-stream-framework/ Cargo.toml
git commit -m "feat(dex): create dex-node-stream-framework crate with proto definitions"
```

---

## Task 4: DexStreamingManager 实现

**Files:**
- Create: `crates/dex-node-stream-framework/src/manager.rs`
- Create: `crates/dex-node-stream-framework/src/grpc_server.rs`
- Create: `crates/dex-node-stream-framework/src/convert.rs`
- Modify: `crates/dex-node-stream-framework/src/lib.rs`

**Step 1: 创建 convert.rs — 类型转换**

文件: `crates/dex-node-stream-framework/src/convert.rs`

实现 sui-types DEX 事件 → proto 类型转换:

```rust
use sui_types::dex_events::{
    FillEvent, OrderPlacedEventV1, OrderRemovedEventV1, OrderUpdateEvent, OrderbookDeltaEvent,
};

use crate::proto::dex_streaming_v1::*;

impl From<&OrderbookDeltaEvent> for OrderbookDeltaEventProto {
    fn from(e: &OrderbookDeltaEvent) -> Self {
        Self {
            perpetual_id: e.perpetual_id,
            sequence: e.sequence,
            updates: e.updates.iter().map(|u| OrderbookDeltaProto {
                side: u.side as u32,
                price: u.price,
                quantity: u.quantity,
            }).collect(),
            timestamp_ms: e.timestamp_ms,
        }
    }
}

impl From<&FillEvent> for FillEventProto {
    fn from(e: &FillEvent) -> Self {
        Self {
            perpetual_id: e.perpetual_id,
            taker_order_id: e.taker_order_id.to_bytes().to_vec(),
            maker_order_id: e.maker_order_id.to_bytes().to_vec(),
            taker_subaccount: e.taker_subaccount_id.to_bytes().to_vec(),
            maker_subaccount: e.maker_subaccount_id.to_bytes().to_vec(),
            side: e.side as u32,
            price: e.price,
            quantity: e.quantity,
            taker_fee: e.taker_fee,
            maker_fee: e.maker_fee,
            timestamp_ms: e.timestamp_ms,
        }
    }
}

// 类似实现 OrderUpdateEvent, OrderPlacedEventV1, OrderRemovedEventV1 的转换
```

注意：具体字段名需参考 `crates/sui-types/src/dex_events.rs` 中各事件 struct 的实际字段。

**Step 2: 创建 manager.rs — DexStreamingManager**

文件: `crates/dex-node-stream-framework/src/manager.rs`

```rust
use std::sync::Arc;
use tokio::sync::broadcast;
use sui_types::base_types::TransactionDigest;
use sui_types::dex_events::{
    FillEvent, OrderPlacedEventV1, OrderRemovedEventV1, OrderUpdateEvent, OrderbookDeltaEvent,
};
use sui_types::event::Event;
use tracing::{debug, warn};

use crate::proto::dex_streaming_v1::*;
use crate::convert;

#[derive(Clone, Debug)]
pub struct DexStreamBatch {
    pub tx_digest: TransactionDigest,
    pub timestamp_ms: u64,
    pub events: Vec<DexStreamEventProto>,
}

pub struct DexStreamingManager {
    broadcast_tx: broadcast::Sender<DexStreamBatch>,
}

impl DexStreamingManager {
    pub fn new(channel_capacity: usize) -> Self {
        let (broadcast_tx, _) = broadcast::channel(channel_capacity);
        Self { broadcast_tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DexStreamBatch> {
        self.broadcast_tx.subscribe()
    }

    /// 处理一笔 DEX 交易的事件，过滤并广播
    pub fn process_dex_execution(
        &self,
        tx_digest: TransactionDigest,
        timestamp_ms: u64,
        transaction_events: &[Event],
        delta_events: &[OrderbookDeltaEvent],
    ) {
        // 无订阅者时直接返回，零开销
        if self.broadcast_tx.receiver_count() == 0 {
            return;
        }

        let mut proto_events = Vec::new();

        // 从 TransactionEvents 中过滤 DEX 事件
        for event in transaction_events {
            if let Some(proto_event) = self.convert_event(event) {
                proto_events.push(proto_event);
            }
        }

        // 添加 delta events
        for delta in delta_events {
            proto_events.push(DexStreamEventProto {
                event: Some(dex_stream_event_proto::Event::OrderbookDelta(
                    OrderbookDeltaEventProto::from(delta),
                )),
            });
        }

        if proto_events.is_empty() {
            return;
        }

        let batch = DexStreamBatch {
            tx_digest,
            timestamp_ms,
            events: proto_events,
        };

        // broadcast send 是非阻塞的
        if let Err(e) = self.broadcast_tx.send(batch) {
            debug!("No active subscribers for dex streaming: {}", e);
        }
    }

    fn convert_event(&self, event: &Event) -> Option<DexStreamEventProto> {
        // 根据 event.type_ (StructTag) 匹配 DEX 事件类型
        // 使用 bcs::from_bytes 反序列化
        let type_tag = &event.type_;

        if *type_tag == FillEvent::struct_tag() {
            let fill: FillEvent = bcs::from_bytes(&event.contents).ok()?;
            Some(DexStreamEventProto {
                event: Some(dex_stream_event_proto::Event::Fill(
                    FillEventProto::from(&fill),
                )),
            })
        } else if *type_tag == OrderUpdateEvent::struct_tag() {
            let update: OrderUpdateEvent = bcs::from_bytes(&event.contents).ok()?;
            Some(DexStreamEventProto {
                event: Some(dex_stream_event_proto::Event::OrderUpdate(
                    OrderUpdateEventProto::from(&update),
                )),
            })
        } else if *type_tag == OrderPlacedEventV1::struct_tag() {
            let placed: OrderPlacedEventV1 = bcs::from_bytes(&event.contents).ok()?;
            Some(DexStreamEventProto {
                event: Some(dex_stream_event_proto::Event::OrderPlaced(
                    OrderPlacedEventProto::from(&placed),
                )),
            })
        } else if *type_tag == OrderRemovedEventV1::struct_tag() {
            let removed: OrderRemovedEventV1 = bcs::from_bytes(&event.contents).ok()?;
            Some(DexStreamEventProto {
                event: Some(dex_stream_event_proto::Event::OrderRemoved(
                    OrderRemovedEventProto::from(&removed),
                )),
            })
        } else {
            None
        }
    }
}
```

**Step 3: 创建 grpc_server.rs — gRPC 服务实现**

文件: `crates/dex-node-stream-framework/src/grpc_server.rs`

```rust
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::broadcast;
use tokio_stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::manager::{DexStreamBatch, DexStreamingManager};
use crate::proto::dex_streaming_v1::*;
use crate::proto::dex_streaming_v1::dex_streaming_server::{DexStreaming, DexStreamingServer};

pub struct DexStreamingService {
    manager: Arc<DexStreamingManager>,
}

impl DexStreamingService {
    pub fn new(manager: Arc<DexStreamingManager>) -> Self {
        Self { manager }
    }
}

#[tonic::async_trait]
impl DexStreaming for DexStreamingService {
    type SubscribeStream = Pin<Box<dyn Stream<Item = Result<DexStreamBatchProto, Status>> + Send>>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let market_ids: Vec<u32> = request.into_inner().market_ids;
        let rx = self.manager.subscribe();

        let stream = async_stream::stream! {
            let mut rx = rx;
            loop {
                match rx.recv().await {
                    Ok(batch) => {
                        // 如果指定了 market_ids，则过滤
                        let filtered_events = if market_ids.is_empty() {
                            batch.events
                        } else {
                            batch.events.into_iter().filter(|e| {
                                match &e.event {
                                    Some(dex_stream_event_proto::Event::OrderbookDelta(d)) => {
                                        market_ids.contains(&d.perpetual_id)
                                    }
                                    Some(dex_stream_event_proto::Event::Fill(f)) => {
                                        market_ids.contains(&f.perpetual_id)
                                    }
                                    Some(dex_stream_event_proto::Event::OrderUpdate(u)) => {
                                        market_ids.contains(&u.perpetual_id)
                                    }
                                    Some(dex_stream_event_proto::Event::OrderPlaced(p)) => {
                                        market_ids.contains(&p.perpetual_id)
                                    }
                                    Some(dex_stream_event_proto::Event::OrderRemoved(r)) => {
                                        market_ids.contains(&r.perpetual_id)
                                    }
                                    None => false,
                                }
                            }).collect()
                        };

                        if !filtered_events.is_empty() {
                            yield Ok(DexStreamBatchProto {
                                tx_digest: batch.tx_digest.into_inner().to_vec(),
                                timestamp_ms: batch.timestamp_ms,
                                events: filtered_events,
                            });
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("dex streaming subscriber lagged by {} messages", n);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_snapshot(
        &self,
        _request: Request<SnapshotRequest>,
    ) -> Result<Response<L2BookSnapshot>, Status> {
        // GetSnapshot 将在 Step 3 实现（需要 AuthorityState 读取链上对象）
        Err(Status::unimplemented("GetSnapshot not yet implemented"))
    }
}

/// 启动 gRPC server
pub async fn start_grpc_server(
    addr: SocketAddr,
    manager: Arc<DexStreamingManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = DexStreamingService::new(manager);

    info!("DexStreaming gRPC server listening on {}", addr);

    tonic::transport::Server::builder()
        .add_service(DexStreamingServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
```

**Step 4: 更新 lib.rs**

文件: `crates/dex-node-stream-framework/src/lib.rs`

```rust
pub mod proto;
pub mod convert;
pub mod manager;
pub mod grpc_server;

pub use manager::{DexStreamBatch, DexStreamingManager};
```

**Step 5: 添加 Cargo.toml 依赖**

确认 `crates/dex-node-stream-framework/Cargo.toml` 包含:
```toml
[dependencies]
sui-types.workspace = true
tokio = { workspace = true, features = ["sync", "rt"] }
tonic.workspace = true
tonic-prost.workspace = true
prost.workspace = true
tokio-stream.workspace = true
tracing.workspace = true
async-stream.workspace = true
bcs.workspace = true
```

**Step 6: 编译验证**

Run: `cargo check -p dex-node-stream-framework`
Expected: 编译通过

**Step 7: Commit**

```bash
git add crates/dex-node-stream-framework/
git commit -m "feat(dex): implement DexStreamingManager with gRPC Subscribe"
```

---

## Task 5: NodeConfig 扩展 DexStreamingConfig

**Files:**
- Modify: `crates/sui-config/src/node.rs:50-225`
- Modify: `crates/sui-config/Cargo.toml` (如果需要)

**Step 1: 添加 DexStreamingConfig 结构体**

文件: `crates/sui-config/src/node.rs`

在文件末尾（在最后一个 struct 定义之后）添加:

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct DexStreamingConfig {
    pub grpc_address: SocketAddr,
    #[serde(default = "default_dex_streaming_channel_capacity")]
    pub channel_capacity: usize,
}

fn default_dex_streaming_channel_capacity() -> usize {
    1024
}
```

**Step 2: 在 NodeConfig 中添加字段**

文件: `crates/sui-config/src/node.rs`

在 `transaction_driver_config` 字段（line ~224）之前添加:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dex_streaming: Option<DexStreamingConfig>,
```

**Step 3: 编译验证**

Run: `cargo check -p sui-config`
Expected: 编译通过

**Step 4: Commit**

```bash
git add crates/sui-config/src/node.rs
git commit -m "feat(dex): add DexStreamingConfig to NodeConfig"
```

---

## Task 6: Authority 集成 — dex_streaming 字段 + post_process_one_tx 钩子

**Files:**
- Modify: `crates/sui-core/Cargo.toml`
- Modify: `crates/sui-core/src/authority.rs` (AuthorityState struct, new(), post_process_one_tx())

**Step 1: 添加 dex-node-stream-framework 依赖到 sui-core**

文件: `crates/sui-core/Cargo.toml`

在 `[dependencies]` 中添加:
```toml
dex-node-stream-framework.workspace = true
```

同时在根 `Cargo.toml` 的 `[workspace.dependencies]` 添加:
```toml
dex-node-stream-framework = { path = "crates/dex-node-stream-framework" }
```

**Step 2: AuthorityState 添加 dex_streaming 字段**

文件: `crates/sui-core/src/authority.rs`

在 import 区添加:
```rust
use dex_node_stream_framework::DexStreamingManager;
```

在 AuthorityState struct（line 903-963）中，在 `notify_epoch` 字段（line 962）之前添加:
```rust
    /// DEX streaming manager for gRPC event streaming
    pub dex_streaming: Option<Arc<DexStreamingManager>>,
```

**Step 3: 在 AuthorityState::new() 中初始化**

文件: `crates/sui-core/src/authority.rs`

在 `let state = Arc::new(AuthorityState { ... })` 构造体（lines 3646-3672）中添加:

在 `fork_recovery_state,`（line 3670）之后添加:
```rust
            dex_streaming: None,  // 将在 SuiNode 启动时设置
```

或者，如果在 new() 中直接初始化:
```rust
            dex_streaming: config.dex_streaming.as_ref().map(|cfg| {
                Arc::new(DexStreamingManager::new(cfg.channel_capacity))
            }),
```

**Step 4: 在 AuthorityState::new() 中启动 gRPC server**

在 `let state = Arc::new(...)` 之后，`spawn_monitored_task!(fix_indexes(...))` 之前（line ~3674），添加:

```rust
        // 启动 DEX streaming gRPC server
        if let Some(ref dex_cfg) = state.config.dex_streaming {
            if let Some(ref manager) = state.dex_streaming {
                let manager_clone = manager.clone();
                let addr = dex_cfg.grpc_address;
                tokio::spawn(async move {
                    if let Err(e) = dex_node_stream_framework::grpc_server::start_grpc_server(addr, manager_clone).await {
                        tracing::error!("DexStreaming gRPC server failed: {}", e);
                    }
                });
            }
        }
```

**Step 5: 在 post_process_one_tx() 中调用 DexStreamingManager**

文件: `crates/sui-core/src/authority.rs:3286-3367`

在 `subscription_handler.process_tx(...)` 调用之后（line ~3360），`self.metrics.post_processing_total_events_emitted` 之前（line ~3362），添加:

```rust
            // DEX streaming: 推送 delta events 和 DEX 事件
            if let Some(ref dex_streaming) = self.dex_streaming {
                dex_streaming.process_dex_execution(
                    *tx_digest,
                    timestamp_ms,
                    &inner_temporary_store.events.data,
                    &inner_temporary_store.dex_delta_events,
                );
            }
```

**Step 6: 编译验证**

Run: `cargo check -p sui-core`
Expected: 编译通过

**Step 7: Commit**

```bash
git add crates/sui-core/Cargo.toml crates/sui-core/src/authority.rs Cargo.toml
git commit -m "feat(dex): integrate DexStreamingManager into AuthorityState"
```

---

## Task 7: 全量编译 + Clippy + 测试

**Step 1: 全量编译**

Run: `cargo build`
Expected: 编译成功

**Step 2: Clippy 检查**

Run: `cargo clippy -p dex-node-stream-framework -p sui-execution -p sui-types -p sui-core -p sui-config`
Expected: 无 warning

**Step 3: 运行单元测试**

Run: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-types -p sui-execution -p dex-node-stream-framework --lib`
Expected: 全部通过

**Step 4: Commit（如有 clippy 修复）**

```bash
git add -A
git commit -m "chore: fix clippy warnings for Phase 6 Step 2"
```

---

## 验证标准

| 指标 | 标准 |
|------|------|
| 编译通过 | `cargo build` 无错误 |
| Clippy 通过 | 相关 crate 无 warning |
| 单元测试通过 | 全部现有测试仍然通过 |
| Delta 线程化 | delta_events 从 execute_* → DexExecutionResult → InnerTemporaryStore → post_process_one_tx |
| gRPC server 启动 | 配置 dex_streaming 后 server 正常监听 |
| 无订阅者零开销 | receiver_count() == 0 直接返回 |
| 向后兼容 | dex_streaming: None 时完全无影响 |
