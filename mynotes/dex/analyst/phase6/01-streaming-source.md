# 01 - DexStreamingManager: 流式数据源设计

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: ⚠️ 需更新 — 传输方式从 broadcast channel 改为 gRPC
> 前置依赖: [00-overview.md](./00-overview.md)

> **2026-02-27 架构决策变更通知**
>
> 根据 [08-architecture-qa.md](./08-architecture-qa.md) 确认的决策：
> - **Q7=B**: DexStreamingManager 内嵌 gRPC server（替代 broadcast channel 作为外部传输）
> - **Q3=C+**: 新增 `GetSnapshot()` RPC，从 InlineOrderbook 读取完整 L2
> - **Q5=A2**: OrderbookDeltaEvent 不纳入 TransactionEvents，直接通过 gRPC 推送
>
> **本文档需更新的部分**：
> - §1 概述 → 传输方式改为 gRPC streaming
> - §3 DexStreamingManager → 内嵌 gRPC server (Subscribe + GetSnapshot)
> - §4 事件过滤 → Delta 事件不从 TransactionEvents 读取，从执行层直接传递
> - 新增 GetSnapshot 接口（读取 InlineOrderbook）
>
> 当前文档内容保留作为参考，实际实现以 07-implementation-plan.md Step 2 为准。

## 1. 概述

DexStreamingManager 是 Phase 6 低延迟架构的核心组件，负责在 Sui 执行层拦截 DEX 事件并通过**内嵌 gRPC server** 分发给下游消费者（dex-streamer 独立服务）。其设计目标是将事件从引擎执行完成到推送的延迟从当前的 1-3s（Checkpoint 通道）降低到 <50ms。

**职责边界**：

| 职责 | DexStreamingManager | Checkpoint 通道 |
|------|---------------------|------------------|
| 延迟 | <10ms（进程内） | 1-3s |
| 数据完整性 | 尽力交付（可丢弃） | 保证交付 |
| 持久化 | 无 | PostgreSQL |
| 角色 | 实时推送主路径 | 校验兜底 + 持久化 |

---

## 2. Hook 点分析

### 2.1 AuthorityState 现有回调机制

Sui 的 `AuthorityState` 在交易执行后有一个 `post_process_one_tx()` 方法，已经实现了事件分发模式。

**文件**: `sui/crates/sui-core/src/authority.rs`

```rust
// authority.rs:3273-3344
#[instrument(level = "trace", skip_all, err(level = "debug"))]
fn post_process_one_tx(
    &self,
    certificate: &VerifiedExecutableTransaction,
    effects: &TransactionEffects,
    inner_temporary_store: &InnerTemporaryStore,
    epoch_store: &Arc<AuthorityPerEpochStore>,
) -> SuiResult {
    // ...
    let tx_digest = certificate.digest();
    let timestamp_ms = Self::unixtime_now_ms();
    let events = &inner_temporary_store.events;
    // ...

    // 行 3324-3337: 现有的 SubscriptionHandler 回调
    self.subscription_handler
        .process_tx(certificate.data().transaction_data(), &effects, &events)
        .tap_ok(|_| {
            self.metrics
                .post_processing_total_tx_had_event_processed
                .inc()
        })
        .tap_err(|e| {
            warn!(?tx_digest,
                "Post processing - Couldn't process events for tx: {}", e
            )
        })?;
    // ...
}
```

**关键观察**：

1. `post_process_one_tx()` 在每笔交易执行后同步调用（行 3273）
2. `inner_temporary_store.events` 包含完整的 `TransactionEvents`，其中包括所有 DEX 事件
3. `SubscriptionHandler` 已经作为 `subscription_handler: Arc<SubscriptionHandler>` 注册在 AuthorityState 上（行 926）
4. `SubscriptionHandler.process_tx()` 使用 `try_send()` 非阻塞发送（见 `subscription_handler.rs:99-111`），不会阻塞执行路径

### 2.2 SubscriptionHandler 的局限性

现有的 `SubscriptionHandler`（`sui/crates/sui-core/src/subscription_handler.rs`）不适合直接用于 DEX 流式推送：

| 维度 | SubscriptionHandler | DexStreamingManager 需求 |
|------|---------------------|--------------------------|
| 事件格式 | `SuiEvent`（JSON-RPC 格式，含 layout 解析） | 原始 BCS 字节，零解析开销 |
| 过滤方式 | 按订阅者的 `EventFilter` 逐个匹配 | 按 `package_id == DEX_EVENTS_PACKAGE` 批量过滤 |
| 分发模型 | mpsc 多通道（每个订阅者一个） | broadcast 单通道（所有消费者共享） |
| 前置转换 | 需要 `make_transaction_block_events()` 做 layout 解析 | 直接使用 `inner_temporary_store.events`，跳过解析 |
| buffer 大小 | `EVENT_DISPATCH_BUFFER_SIZE = 1000` | 需要更大 buffer（10000+） |

**核心区别**：`SubscriptionHandler` 处理的是 `SuiTransactionBlockEvents`（经过 layout 解析的 JSON-RPC 格式），而 DexStreamingManager 可以直接消费原始的 `TransactionEvents`（BCS 格式），**跳过 `make_transaction_block_events()` 的 layout 解析开销**。这是性能的关键优化点。

### 2.3 选定的 Hook 策略

在 `post_process_one_tx()` 中，紧接现有 `subscription_handler.process_tx()` 之后（或之前），添加 `dex_streaming.process_dex_events()` 调用。

**选择理由**：

1. **侵入性最小**：复用现有的后处理框架，只新增一行调用
2. **时序正确**：在 effects 写入缓存之后调用，事件数据已确定
3. **原始数据可用**：`inner_temporary_store.events` 包含原始 BCS 事件，无需二次序列化
4. **类比 dYdX**：等价于 dYdX 在 `DeliverTx` 阶段调用 `FullNodeStreamingManager.SendOrderbookUpdates()`

**与 SubscriptionHandler 调用的先后顺序**：DexStreamingManager 应放在 `subscription_handler.process_tx()` **之前**，因为：
- DexStreamingManager 直接使用 `inner_temporary_store.events`（原始 BCS），不依赖 `make_transaction_block_events()` 的转换结果
- 优先处理低延迟路径

---

## 3. DexStreamingManager 核心设计

### 3.1 模块位置

```
sui/crates/sui-core/src/
├── authority.rs                 # AuthorityState（添加 dex_streaming 字段）
├── subscription_handler.rs      # 现有事件订阅（保持不变）
├── dex_streaming.rs             # 新文件：DexStreamingManager
└── ...
```

### 3.2 数据结构

```rust
// sui-core/src/dex_streaming.rs

use sui_types::base_types::TransactionDigest;
use sui_types::dex_events::*;
use sui_types::event::Event;
use sui_types::messages::TransactionEvents;
use tokio::sync::broadcast;

/// 单笔 DEX 交易产生的事件批次
#[derive(Clone, Debug)]
pub struct DexStreamBatch {
    /// 交易摘要
    pub tx_digest: TransactionDigest,
    /// 交易执行时间戳（毫秒）
    pub timestamp_ms: u64,
    /// 解析后的 DEX 事件列表
    pub events: Vec<DexStreamEvent>,
}

/// 类型化的 DEX 事件
#[derive(Clone, Debug)]
pub enum DexStreamEvent {
    /// 成交事件（taker-maker 撮合）
    Fill(FillEvent),
    /// 订单簿快照（全量）
    OrderbookSnapshot(OrderbookSnapshotEvent),
    /// 订单状态变更
    OrderUpdate(OrderUpdateEvent),
    /// 仓位变更
    PositionUpdate(PositionUpdateEvent),
    /// 余额变更
    BalanceUpdate(BalanceUpdateEvent),
    /// 订单上簿
    OrderPlaced(OrderPlacedEventV1),
    /// 订单移除
    OrderRemoved(OrderRemovedEventV1),
    // 未来扩展：
    // OrderbookDelta(OrderbookDeltaEvent),  // Phase 6 Step 2 增量事件
    // Liquidation(LiquidationEvent),
    // FundingSettlement(FundingSettlementEvent),
}
```

### 3.3 DexStreamingManager 实现

```rust
use std::sync::Arc;
use sui_types::base_types::{ObjectID, TransactionDigest};
use sui_types::dex_events::{DEX_EVENTS_PACKAGE, DEX_EVENTS_MODULE};
use sui_types::event::Event;
use sui_types::messages::TransactionEvents;
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

/// broadcast channel 容量
/// 10000 批次 ≈ 假设每批 1KB → ~10MB 内存占用
/// 在高峰 100 tx/s 下可缓冲约 100 秒
const DEX_STREAM_CHANNEL_CAPACITY: usize = 10_000;

pub struct DexStreamingManager {
    /// broadcast 发送端
    tx: broadcast::Sender<DexStreamBatch>,
    /// DEX 虚拟包地址（用于过滤）
    dex_package_id: ObjectID,
}

impl DexStreamingManager {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(DEX_STREAM_CHANNEL_CAPACITY);
        Self {
            tx,
            dex_package_id: ObjectID::from(DEX_EVENTS_PACKAGE),
        }
    }

    /// 获取事件订阅接收端
    /// 消费者调用此方法获取 Receiver，通过 tokio::broadcast 接收事件
    pub fn subscribe(&self) -> broadcast::Receiver<DexStreamBatch> {
        self.tx.subscribe()
    }

    /// 当前活跃订阅者数量
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// 处理单笔交易的事件
    ///
    /// 从 TransactionEvents 中过滤 DEX 事件，BCS 反序列化为类型化事件，
    /// 打包为 DexStreamBatch 通过 broadcast channel 发送。
    ///
    /// 此方法设计为非阻塞：
    /// - 使用 try_send() 避免阻塞执行路径
    /// - 如果 channel 满（所有 receiver 落后），直接丢弃
    /// - 消费者可通过 Checkpoint 通道恢复丢失的事件
    pub fn process_dex_events(
        &self,
        tx_digest: &TransactionDigest,
        timestamp_ms: u64,
        events: &TransactionEvents,
    ) {
        // 快速路径：无订阅者时直接返回
        if self.tx.receiver_count() == 0 {
            return;
        }

        // 过滤并解析 DEX 事件
        let dex_events = self.filter_and_parse_events(&events.data);

        // 无 DEX 事件时直接返回
        if dex_events.is_empty() {
            return;
        }

        let batch = DexStreamBatch {
            tx_digest: *tx_digest,
            timestamp_ms,
            events: dex_events,
        };

        let event_count = batch.events.len();

        // 非阻塞发送
        match self.tx.send(batch) {
            Ok(receivers) => {
                trace!(
                    ?tx_digest,
                    event_count,
                    receivers,
                    "DEX stream: batch sent"
                );
            }
            Err(_) => {
                // broadcast::SendError 只有在 receiver_count == 0 时才会发生
                // 但我们已经在上面检查过，此处作为防御性处理
                debug!(
                    ?tx_digest,
                    event_count,
                    "DEX stream: no active receivers, batch dropped"
                );
            }
        }
    }

    /// 从原始事件列表中过滤 DEX 事件并反序列化
    fn filter_and_parse_events(&self, events: &[Event]) -> Vec<DexStreamEvent> {
        let mut result = Vec::new();

        for event in events {
            // 第一层过滤：package_id 必须是 DEX 虚拟包地址
            if event.package_id != self.dex_package_id {
                continue;
            }

            // 第二层过滤：module 必须是 dex_events
            if event.transaction_module.as_str() != DEX_EVENTS_MODULE {
                continue;
            }

            // 按事件类型名称分发反序列化
            let parsed = match event.type_.name.as_str() {
                "FillEvent" => {
                    bcs::from_bytes::<FillEvent>(&event.contents)
                        .map(DexStreamEvent::Fill)
                        .ok()
                }
                "OrderbookSnapshotEvent" => {
                    bcs::from_bytes::<OrderbookSnapshotEvent>(&event.contents)
                        .map(DexStreamEvent::OrderbookSnapshot)
                        .ok()
                }
                "OrderUpdateEvent" => {
                    bcs::from_bytes::<OrderUpdateEvent>(&event.contents)
                        .map(DexStreamEvent::OrderUpdate)
                        .ok()
                }
                "PositionUpdateEvent" => {
                    bcs::from_bytes::<PositionUpdateEvent>(&event.contents)
                        .map(DexStreamEvent::PositionUpdate)
                        .ok()
                }
                "BalanceUpdateEvent" => {
                    bcs::from_bytes::<BalanceUpdateEvent>(&event.contents)
                        .map(DexStreamEvent::BalanceUpdate)
                        .ok()
                }
                "OrderPlacedEventV1" => {
                    bcs::from_bytes::<OrderPlacedEventV1>(&event.contents)
                        .map(DexStreamEvent::OrderPlaced)
                        .ok()
                }
                "OrderRemovedEventV1" => {
                    bcs::from_bytes::<OrderRemovedEventV1>(&event.contents)
                        .map(DexStreamEvent::OrderRemoved)
                        .ok()
                }
                other => {
                    // 未知事件类型（如 PerpetualCreatedEvent、GlobalAccountsCreatedEvent 等管理事件）
                    // 不需要低延迟推送，跳过
                    trace!(event_type = other, "DEX stream: skipping non-trading event");
                    None
                }
            };

            if let Some(event) = parsed {
                result.push(event);
            }
        }

        result
    }
}
```

### 3.4 事件过滤逻辑详解

DEX 事件系统使用虚拟包地址 `DEX_EVENTS_PACKAGE`（`0x0000...44455800`），定义在 `dex-sui/crates/sui-types/src/dex_events.rs:41-44`：

```rust
// dex_events.rs:41-44
pub const DEX_EVENTS_PACKAGE: AccountAddress = AccountAddress::new([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x44, 0x45, 0x58, 0x00,
]);

pub const DEX_EVENTS_MODULE: &str = "dex_events";
```

过滤流程：

```
Event
  │
  ├─ event.package_id == DEX_EVENTS_PACKAGE ?
  │  └─ No → 跳过（Move 合约事件、系统事件等）
  │
  ├─ event.transaction_module == "dex_events" ?
  │  └─ No → 跳过（理论上不会出现，但防御性检查）
  │
  └─ match event.type_.name.as_str()
     ├─ "FillEvent"              → BCS deserialize → DexStreamEvent::Fill
     ├─ "OrderbookSnapshotEvent" → BCS deserialize → DexStreamEvent::OrderbookSnapshot
     ├─ "OrderUpdateEvent"       → BCS deserialize → DexStreamEvent::OrderUpdate
     ├─ "PositionUpdateEvent"    → BCS deserialize → DexStreamEvent::PositionUpdate
     ├─ "BalanceUpdateEvent"     → BCS deserialize → DexStreamEvent::BalanceUpdate
     ├─ "OrderPlacedEventV1"     → BCS deserialize → DexStreamEvent::OrderPlaced
     ├─ "OrderRemovedEventV1"    → BCS deserialize → DexStreamEvent::OrderRemoved
     └─ 其他（管理类事件）        → 跳过
```

**选择性过滤的理由**：并非所有 DEX 事件都需要低延迟推送。`GlobalAccountsCreatedEvent`、`PerpetualCreatedEvent` 等管理类事件是一次性的，走 Checkpoint 通道即可。低延迟通道只处理交易相关事件。

---

## 4. 与 AuthorityState 的集成

### 4.1 AuthorityState 字段变更

**文件**: `sui/crates/sui-core/src/authority.rs`

在 AuthorityState 结构体（行 903 起）中添加字段：

```rust
pub struct AuthorityState {
    // ... 现有字段 ...

    pub indexes: Option<Arc<IndexStore>>,
    pub rpc_index: Option<Arc<RpcIndexStore>>,

    pub subscription_handler: Arc<SubscriptionHandler>,

    /// DEX 低延迟事件流管理器
    /// 在 effects 生成后拦截 DEX 事件，通过 broadcast channel 推送给下游消费者
    /// 通过 NodeConfig.enable_dex_streaming 控制是否启用
    pub dex_streaming: Option<Arc<DexStreamingManager>>,

    pub checkpoint_store: Arc<CheckpointStore>,
    // ...
}
```

### 4.2 初始化

在 `AuthorityState` 构造函数（行 3623 起）中初始化：

```rust
// authority.rs 构造函数中
let dex_streaming = if config.enable_dex_streaming {
    Some(Arc::new(DexStreamingManager::new()))
} else {
    None
};

let state = Arc::new(AuthorityState {
    name,
    secret,
    // ... 现有字段 ...
    subscription_handler: Arc::new(SubscriptionHandler::new(prometheus_registry)),
    dex_streaming,           // 新增
    checkpoint_store,
    // ...
});
```

### 4.3 调用点

在 `post_process_one_tx()`（行 3273 起）中添加调用：

```rust
fn post_process_one_tx(
    &self,
    certificate: &VerifiedExecutableTransaction,
    effects: &TransactionEffects,
    inner_temporary_store: &InnerTemporaryStore,
    epoch_store: &Arc<AuthorityPerEpochStore>,
) -> SuiResult {
    // ... 现有 indexing 逻辑 ...

    let tx_digest = certificate.digest();
    let timestamp_ms = Self::unixtime_now_ms();
    let events = &inner_temporary_store.events;

    // ===== 新增：DEX 低延迟事件流 =====
    // 在 SubscriptionHandler 之前调用，因为不依赖 layout 解析
    // 直接使用原始 TransactionEvents（BCS 格式），避免
    // make_transaction_block_events() 的解析开销
    if let Some(dex_streaming) = &self.dex_streaming {
        dex_streaming.process_dex_events(tx_digest, timestamp_ms, events);
    }
    // ===== 新增结束 =====

    // Index tx
    if let Some(indexes) = &self.indexes {
        // ... 现有 indexing + subscription_handler 逻辑 ...
    }

    Ok(())
}
```

**关键设计决策**：

1. DexStreamingManager 的调用放在 `indexes.is_none()` 检查之前（或独立检查），因为即使节点不开启 indexing，也可能需要 DEX 流式推送
2. 使用 `inner_temporary_store.events`（原始 `TransactionEvents`）而非 `make_transaction_block_events()` 的结果，避免 layout 解析开销
3. `process_dex_events()` 内部已经是非阻塞的，不会影响执行路径延迟

### 4.4 调用时序图

```
execute_certificate()
    │
    ↓
post_process_one_tx()
    │
    ├─ [1] dex_streaming.process_dex_events()      ← 新增，直接用原始 events
    │      │
    │      ├─ 过滤 DEX_EVENTS_PACKAGE 事件
    │      ├─ BCS 反序列化为类型化事件
    │      └─ broadcast::send(DexStreamBatch)       ← <1ms，非阻塞
    │
    ├─ [2] index_tx()                               ← 现有
    │
    ├─ [3] make_transaction_block_events()           ← 现有，layout 解析
    │
    └─ [4] subscription_handler.process_tx()         ← 现有
```

---

## 5. 配置管理

### 5.1 NodeConfig 扩展

**文件**: `sui/crates/sui-config/src/node.rs`

```rust
#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct NodeConfig {
    // ... 现有字段 ...

    /// 是否启用 DEX 低延迟事件流
    /// 启用后，DEX 交易事件将在 effects 生成后立即通过 broadcast channel 推送
    /// 默认关闭，仅在需要低延迟订单簿推送的节点上启用
    #[serde(default)]
    pub enable_dex_streaming: bool,
}
```

**默认关闭的理由**：
- 标准 Sui 验证器不需要此功能
- 仅 DEX 专用节点需要启用
- 避免对非 DEX 场景的性能影响（虽然很小）

### 5.2 Docker 配置示例

```yaml
# docker/dex-dev/node-config.yaml
enable-dex-streaming: true
```

---

## 6. 性能分析

### 6.1 关键路径延迟

| 操作 | 预期延迟 | 说明 |
|------|----------|------|
| `package_id` 过滤 | ~10ns | ObjectID 比较，32 字节 |
| `type_.name` 匹配 | ~50ns | 字符串匹配 |
| BCS 反序列化（单个事件） | ~100-500ns | 取决于事件大小 |
| `broadcast::send()` | ~100ns | 非阻塞，atomic 操作 |
| **总计（典型 5 个事件）** | **<5us** | 不影响执行路径 |

对比 `make_transaction_block_events()`（现有 SubscriptionHandler 路径）：
- 需要 layout 解析（`type_layout_resolver`）
- 涉及 Move VM 类型系统查询
- 估计延迟 ~100us-1ms

DexStreamingManager 通过跳过 layout 解析，实现 **~100x 的延迟优化**。

### 6.2 内存开销

```
broadcast channel 容量: 10000 批次
典型 DexStreamBatch 大小: ~200-1000 bytes
    - FillEvent: ~120 bytes
    - OrderUpdateEvent: ~100 bytes
    - OrderbookSnapshotEvent: ~5-10KB（全量快照，较大）
    - 其他事件: ~50-200 bytes

最大内存占用: 10000 × 1KB（平均） = ~10MB
峰值（含快照）: 10000 × 5KB = ~50MB
```

### 6.3 背压策略

`tokio::broadcast` 的行为特性：

- **发送端永不阻塞**：`send()` 总是立即返回
- **接收端落后时**：最旧的消息被覆盖，接收端收到 `RecvError::Lagged(n)` 错误
- **所有接收端断开时**：`send()` 返回 `SendError`，但消息已被丢弃

这意味着：
1. 执行路径永远不会因 DexStreamingManager 而阻塞
2. 慢消费者自动跳过旧消息，只收到最新数据
3. 丢失的消息可通过 Checkpoint 通道恢复（见 `06-consistency-model.md`）

### 6.4 与 SubscriptionHandler 的对比

| 维度 | SubscriptionHandler | DexStreamingManager |
|------|---------------------|---------------------|
| channel 类型 | `mpsc`（每个订阅者独立） | `broadcast`（所有消费者共享） |
| 容量 | 1000（`EVENT_DISPATCH_BUFFER_SIZE`） | 10000 |
| 事件格式 | `SuiEvent`（JSON-RPC 格式） | `DexStreamEvent`（BCS 反序列化后） |
| 过滤时机 | 发送时按 `EventFilter` 过滤 | 发送前按 `package_id` 批量过滤 |
| 阻塞行为 | `try_send()`，channel 满时丢弃 | `send()`，覆盖旧消息 |
| 典型消费者 | JSON-RPC WebSocket 订阅 | dex-streamer（订单簿构建 + Redis 写入） |

---

## 7. 与 dYdX FullNodeStreamingManager 对比

### 7.1 架构对比

dYdX v4 的 `FullNodeStreamingManager`（`dydx-v4-chain/protocol/streaming/full_node_streaming_manager.go`）：

```
dYdX:
  MemClob.PlaceOrder() / CancelOrder()
      │
      ↓ 在 CheckTx / DeliverTx 阶段
  FullNodeStreamingManager.SendOrderbookUpdates()
      │
      ↓ gRPC stream
  Indexer / WebSocket 服务

本项目:
  execute_place_order() / execute_cancel_order()
      │
      ↓ 在 post_process_one_tx() 阶段
  DexStreamingManager.process_dex_events()
      │
      ↓ tokio::broadcast
  dex-streamer / dex-api
```

### 7.2 关键差异

| 维度 | dYdX FullNodeStreamingManager | DexStreamingManager |
|------|-------------------------------|---------------------|
| **触发时机** | CheckTx（乐观，可能被回滚） | post_process_one_tx（确定性，不会回滚） |
| **数据确定性** | 乐观更新 + FinalizeBlock 确认 | 所有事件均为最终状态 |
| **传输协议** | gRPC stream（跨进程） | tokio::broadcast（进程内） |
| **订阅管理** | 客户端直接连 gRPC | 进程内 Receiver，需要 dex-streamer 做二次分发 |
| **回滚处理** | 需要（CheckTx → FinalizeBlock 可能不一致） | 不需要（post-execution 数据已确定） |
| **事件粒度** | L3 订单级（单个订单变更） | L2 快照级（当前）+ L2 delta（规划） |

### 7.3 我们的优势

1. **无回滚风险**：dYdX 在 `CheckTx` 阶段推送乐观更新，如果 `FinalizeBlock` 与 `CheckTx` 结果不同，需要发送回滚消息。我们在 `post_process_one_tx()` 阶段推送，数据已是最终状态，永远不需要回滚。

2. **进程内零拷贝**：dYdX 通过 gRPC 序列化传输（protobuf 编解码开销），我们使用 `tokio::broadcast` 在进程内共享 `Arc` 引用，延迟更低。

3. **简化的一致性模型**：无需区分 "乐观状态" 和 "最终状态"，减少了 `dex-streamer` 的实现复杂度。

### 7.4 我们的劣势

1. **延迟略高**：dYdX 在共识前（CheckTx）就推送，我们在执行后推送。理论上 dYdX 可以更快 ~数百毫秒，但 dYdX 的乐观更新可能不准确。

2. **耦合度**：DexStreamingManager 嵌入在 `sui-core` 中，而 dYdX 的 `FullNodeStreamingManager` 是独立模块。但我们通过 `Option<Arc<DexStreamingManager>>` 和配置开关做了解耦。

---

## 8. 消费者接口

### 8.1 订阅方式

下游组件（如 dex-streamer）通过 `AuthorityState` 获取订阅：

```rust
// dex-streamer 启动时
let receiver = authority_state
    .dex_streaming
    .as_ref()
    .expect("dex_streaming must be enabled")
    .subscribe();

// 消费循环
tokio::spawn(async move {
    loop {
        match receiver.recv().await {
            Ok(batch) => {
                for event in &batch.events {
                    match event {
                        DexStreamEvent::Fill(fill) => {
                            // 处理成交事件
                        }
                        DexStreamEvent::OrderbookSnapshot(snapshot) => {
                            // 更新本地订单簿 + 写入 Redis
                        }
                        DexStreamEvent::OrderUpdate(update) => {
                            // 更新订单状态
                        }
                        // ...
                        _ => {}
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                warn!(
                    skipped,
                    "DEX stream consumer lagged, requesting checkpoint recovery"
                );
                // 触发 Checkpoint 恢复流程
            }
            Err(broadcast::error::RecvError::Closed) => {
                // channel 关闭，节点正在关闭
                break;
            }
        }
    }
});
```

### 8.2 Lagged 恢复策略

当消费者收到 `RecvError::Lagged(n)` 时，意味着丢失了 `n` 条消息。恢复策略：

1. **记录当前 Checkpoint 序号**
2. **继续消费新消息**（不中断流）
3. **异步从 Checkpoint 通道补数据**（PG 查询丢失区间）
4. **合并流式数据和补偿数据**

详细设计见 `06-consistency-model.md`。

---

## 9. 扩展性考虑

### 9.1 新事件类型支持

添加新事件类型只需三步：

1. 在 `sui-types/src/dex_events.rs` 中定义新事件结构体（现有模式）
2. 在 `DexStreamEvent` 枚举中添加新变体
3. 在 `filter_and_parse_events()` 的 match 中添加新分支

示例 — 添加 `OrderbookDeltaEvent`（Phase 6 Step 2）：

```rust
// 步骤 1: dex_events.rs（已有模式，详见 02-event-design.md）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookDeltaEvent {
    pub perpetual_id: u32,
    pub sequence: u64,
    pub updates: Vec<OrderbookDelta>,
    pub timestamp_ms: u64,
}

// 步骤 2: dex_streaming.rs
pub enum DexStreamEvent {
    // ...
    OrderbookDelta(OrderbookDeltaEvent),
}

// 步骤 3: filter_and_parse_events()
"OrderbookDeltaEvent" => {
    bcs::from_bytes::<OrderbookDeltaEvent>(&event.contents)
        .map(DexStreamEvent::OrderbookDelta)
        .ok()
}
```

### 9.2 传输协议升级路径

当前使用 `tokio::broadcast`（进程内），未来可升级为 gRPC stream（跨进程）：

```rust
/// 传输层抽象 trait（未来扩展）
pub trait DexStreamTransport: Send + Sync {
    fn send(&self, batch: DexStreamBatch) -> Result<(), DexStreamError>;
    fn subscribe(&self) -> Box<dyn Stream<Item = DexStreamBatch> + Send>;
}

/// 当前实现：进程内 broadcast
pub struct InProcessTransport {
    tx: broadcast::Sender<DexStreamBatch>,
}

/// 未来实现：gRPC stream
pub struct GrpcTransport {
    // tonic gRPC server
}
```

当前阶段不引入此抽象层，等有实际跨进程需求时再添加。

---

## 10. 测试策略

### 10.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_dex_events_only() {
        let manager = DexStreamingManager::new();

        // 构造一个 DEX FillEvent
        let fill = FillEvent { /* ... */ };
        let dex_event = fill.to_sui_event(SuiAddress::ZERO);

        // 构造一个非 DEX 事件（Move 合约事件）
        let move_event = Event {
            package_id: ObjectID::random(),
            // ...
        };

        let events = vec![move_event, dex_event];
        let parsed = manager.filter_and_parse_events(&events);

        assert_eq!(parsed.len(), 1);
        assert!(matches!(parsed[0], DexStreamEvent::Fill(_)));
    }

    #[tokio::test]
    async fn test_broadcast_to_multiple_receivers() {
        let manager = DexStreamingManager::new();
        let mut rx1 = manager.subscribe();
        let mut rx2 = manager.subscribe();

        let events = TransactionEvents { data: vec![/* DEX events */] };
        manager.process_dex_events(
            &TransactionDigest::random(),
            1704067200000,
            &events,
        );

        let batch1 = rx1.recv().await.unwrap();
        let batch2 = rx2.recv().await.unwrap();
        assert_eq!(batch1.events.len(), batch2.events.len());
    }

    #[test]
    fn test_no_subscribers_noop() {
        let manager = DexStreamingManager::new();
        // 无订阅者时，process_dex_events 应该是 noop
        // 不应 panic 或报错
        let events = TransactionEvents { data: vec![] };
        manager.process_dex_events(
            &TransactionDigest::random(),
            1704067200000,
            &events,
        );
    }
}
```

### 10.2 集成测试

在 `dex-indexer-e2e-test` 中添加端到端测试：

1. 启动带 `enable_dex_streaming: true` 的测试节点
2. 订阅 DexStreamingManager 的 broadcast channel
3. 提交 DEX 交易（下单 + 撮合）
4. 验证在 Checkpoint 产出之前就能收到 `DexStreamBatch`
5. 对比 broadcast 事件与 Checkpoint 事件的一致性

---

## 11. 实现步骤

| # | 步骤 | 文件变更 | 预估工作量 |
|---|------|----------|-----------|
| 1 | 创建 `dex_streaming.rs` | `sui-core/src/dex_streaming.rs`（新文件） | 2h |
| 2 | NodeConfig 添加 `enable_dex_streaming` | `sui-config/src/node.rs` | 0.5h |
| 3 | AuthorityState 集成 | `sui-core/src/authority.rs`（3 处修改） | 1h |
| 4 | 单元测试 | `sui-core/src/dex_streaming.rs` 内 `#[cfg(test)]` | 2h |
| 5 | Docker 配置 | `docker/dex-dev/node-config.yaml` | 0.5h |
| **合计** | | | **6h** |

---

## 12. 文件变更清单

| 文件 | 操作 | 变更内容 |
|------|------|----------|
| `sui/crates/sui-core/src/dex_streaming.rs` | **新建** | DexStreamingManager、DexStreamBatch、DexStreamEvent |
| `sui/crates/sui-core/src/lib.rs` | 修改 | 添加 `pub mod dex_streaming;` |
| `sui/crates/sui-core/src/authority.rs` | 修改 | (1) 添加 `dex_streaming` 字段 (2) 构造函数初始化 (3) `post_process_one_tx()` 调用 |
| `sui/crates/sui-config/src/node.rs` | 修改 | 添加 `enable_dex_streaming: bool` 字段 |
| `sui/crates/sui-core/Cargo.toml` | 修改 | 无新依赖（`tokio::sync::broadcast` 和 `bcs` 已在依赖中） |
