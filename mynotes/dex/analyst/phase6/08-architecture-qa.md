# Phase 6 架构 Q&A: 关键问题分析与决策

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: ✅ 全部决策已确认
> 关联文档: [00-overview](./00-overview.md) | [01-streaming-source](./01-streaming-source.md) | [02-event-design](./02-event-design.md)

---

## Q1: 快速通道事件触发时机 — AuthorityState 执行后回调是否有问题？

### 1.1 当前方案

当前规划使用 `post_process_one_tx()`（`authority.rs:3286`）作为 hook 点，在每笔交易执行完成后立即拦截事件。

### 1.2 Sui 执行管线中的所有可选 Hook 点

| # | Hook 点 | 位置 | 时机 | 延迟 | 可用数据 |
|---|---------|------|------|------|----------|
| **A** | `post_process_one_tx()` | `authority.rs:3286` | VM 执行后，commit 前 | **微秒级** | `InnerTemporaryStore.events`（原始 BCS） |
| **B** | `subscription_handler.process_tx()` | `subscription_handler.rs:87` | 在 A 内部调用 | 同 A | `SuiTransactionBlockEvents`（经过 layout 解析） |
| **C** | ExecutionScheduler dispatch | `consensus_handler.rs:999` | 共识排序后，执行前 | 微秒级 | 无事件（还没执行） |
| **D** | Checkpoint 完成回调 | `checkpoint_executor/mod.rs:1034` | Checkpoint 构建后 | **1-3 秒** | 完整 Checkpoint |
| **E** | `rpc_index.index_checkpoint()` | `checkpoint_executor/mod.rs:695` | Checkpoint 索引时 | 同 D | 完整 Checkpoint |

### 1.3 关键发现：验证者 vs 全节点行为差异

**这是最重要的发现**：`post_process_one_tx()` 在验证者和全节点上的行为完全不同。

#### 验证者节点（Validator）

```
共识排序 → ExecutionScheduler → 逐笔执行 → ★ post_process_one_tx() → commit
                                               ↑ 每笔交易立即触发
```

- 每笔交易执行完成后**立即**调用 `post_process_one_tx()`
- `InnerTemporaryStore.events` 包含完整事件数据
- 延迟：**微秒级**（VM 执行完 → hook 触发）

#### 全节点（Fullnode）

```
同步 Checkpoint → 批量执行所有交易 → 等待全部完成 → ★ 逐笔 post_process_one_tx() → Checkpoint 完成
                                                       ↑ 等 Checkpoint 内所有交易都执行完后才逐笔回调
```

- 全节点从其他节点**同步 Checkpoint**，然后批量执行
- `post_process_one_tx()` 仍会逐笔触发，但时机是在整个 Checkpoint 的交易都执行完之后
- 延迟：**取决于 Checkpoint 同步时间**（通常 1-3 秒）

**结论**：要获得真正的低延迟（<50ms），**必须在验证者节点上运行**。全节点上 `post_process_one_tx()` 的延迟与 Checkpoint 通道相差不大。

### 1.4 与 dYdX 的对比

| 维度 | dYdX | 本项目（当前方案） |
|------|------|-------------------|
| 乐观通道 | CheckTx 阶段（交易到达即触发） | `post_process_one_tx()`（执行完成后触发） |
| 最终通道 | DeliverTx + FinalizeBlock | Checkpoint 通道 |
| 乐观数据是否可能回滚 | **是**（CheckTx 可能被 FinalizeBlock 推翻） | **否**（执行后数据已最终确定） |
| 延迟差 | 乐观比最终快 ~1-2 秒 | 同样，快速通道比 Checkpoint 快 ~1-3 秒 |
| 需要回滚处理 | **需要** | **不需要** |

### 1.5 结论

**`post_process_one_tx()` 是正确的 hook 点**，但有一个重要约束：

> **低延迟效果仅在验证者节点上生效**。全节点虽然也能触发，但延迟优势不明显。

这与 dYdX 的情况一致 — dYdX 的 FullNodeStreamingManager 也是在验证节点上才有最低延迟。

### 1.6 决策项

> **[决策 Q1]** 快速通道部署策略：
> - A) 仅支持验证者节点部署（简单，当前开发环境已是验证者）
> - **✅ B) 验证者 + 全节点都支持（全节点退化为 Checkpoint 级延迟，但功能一致）** ← 已确认
>
> **决策理由**：代码层面无区别（`post_process_one_tx()` 在两者上都触发），NodeConfig 统一配置。当前开发/测试环境是单验证者节点，天然支持低延迟。

---

## Q2: API 和 orderbook 推送 — 全量 vs 增量

### 2.1 分层设计

需要区分**三个层面**的数据格式：

| 层面 | 说明 | 推荐格式 |
|------|------|----------|
| **事件层**（引擎 → TransactionEvents） | 链上事件发射 | **增量 delta**（节省带宽） |
| **索引层**（dex-streamer 内存 → Redis） | 中间存储 | **增量 delta → Redis HSET 全量覆写** |
| **推送层**（dex-api → 客户端 WS） | 对外暴露 | **见下方分析** |

### 2.2 推送层选项分析

| 选项 | REST API | WS 订阅 | 客户端复杂度 | 带宽 |
|------|----------|---------|-------------|------|
| **A) 全量推送** | 返回完整 L2 | 每次推送完整 L2 | 最简单（直接替换） | 高（100档 ~3KB/次） |
| **B) 混合推送** | 返回完整 L2 | 首次快照 + 后续增量 | 中等（需维护本地 book） | 低 |
| **C) 纯增量** | 返回完整 L2 | 仅增量（无快照） | 最复杂 | 最低 |

### 2.3 dYdX 的做法

dYdX 采用**方案 B（混合推送）**：
- `InitializeNewStreams()` 首次连接时发送组合快照（订单簿 + 子账户 + 价格）
- 之后发送增量更新（`StreamOrderbookUpdate` 带 `snapshot: false`）
- 可配置 `snapshotBlockInterval` 周期性发送快照

### 2.4 Hyperliquid 的做法

Hyperliquid WS `l2Book` 频道：
- 订阅后推送完整 L2 快照
- 之后推送增量更新（`{coin, levels: [{px, sz, n}], time}`）
- 客户端需要维护本地订单簿

### 2.5 建议

**推荐方案 A（全量推送），事件和索引层用增量。理由：**

1. **客户端复杂度低**：前端只需 `state = newData`，无需维护本地 orderbook、处理 sequence gap、实现恢复逻辑
2. **调试和验证简单**：每次收到的都是完整状态，不存在增量累积错误
3. **带宽在可接受范围内**：10 个市场 × 每秒 10 次更新 × 3KB = ~300KB/s，对现代网络不是问题
4. **事件层仍是增量**：引擎发射 delta event → dex-streamer 维护内存 L2Book → 推送时取全量快照

```
引擎 → OrderbookDeltaEvent (增量, ~55bytes)
         ↓
dex-streamer 内存 L2Book (apply delta)
         ↓
Redis HSET dex:l2book:{id} (全量覆写)
         ↓
dex-api WS l2Book (推送全量 L2 快照)
```

**如果未来带宽成为瓶颈**，可以新增 `l2BookDelta` WS 频道（增量），与 `l2Book`（全量）并存，让客户端选择。

### 2.6 决策项

> **[决策 Q2]** WS 订单簿推送格式：
> - **✅ A) 全量推送** ← 已确认
> - B) 首次快照 + 后续增量（参考 dYdX/Hyperliquid）
> - C) 两种频道并存：`l2Book` 全量 + `l2BookDelta` 增量
>
> **决策理由**：与 Hyperliquid `l2Book` 频道一致 — Hyperliquid 每次推送完整订单簿快照（所有 bids + asks），推送频率限制为至少间隔 0.5s。全量推送避免了增量同步的复杂性（序号管理、断线重连恢复、客户端状态维护）。未来按需新增增量频道。

---

## Q3: dYdX 订单簿双通道验证与全量快照

### 3.1 dYdX 的双通道验证机制

**关键发现**：dYdX **没有显式的双通道交叉验证**。

dYdX 的两个通道是**独立运作**的：

| 维度 | CheckTx 通道（Vulcan） | DeliverTx 通道（Ender） |
|------|----------------------|----------------------|
| 触发 | 交易到达时（乐观） | 区块确认时（最终） |
| 内容 | 订单簿增量 + 成交 + Taker 状态 | 订单簿增量 + 成交 + 子账户 + 价格 |
| 处理 | Redis 热数据 | PostgreSQL 持久化 |
| **快照** | **仅首次连接时发送** | **不发送全量快照** |

dYdX 的 FinalizeBlock 阶段会"同步本地操作队列"（`SyncLocalOpsQueueUpdates`），这是一种**状态对齐**机制——将 DeliverTx 的最终状态与 CheckTx 的乐观状态对齐，但不是独立的"双通道验证"。

### 3.2 dYdX 全量快照策略

dYdX 的全量快照行为：

1. **新订阅连接时**：`InitializeNewStreams()` 发送一次组合快照（含完整订单簿）
2. **可选周期快照**：通过 `snapshotBlockInterval` 配置，设置 > 0 时每 N 个块发送一次快照
3. **FinalizeBlock 后不发送全量快照**：只发送增量更新
4. **Checkpoint 通道不单独发送订单簿快照**：OnChainUpdates 通过 Ender 处理，写入 PostgreSQL

### 3.3 我们是否需要双通道验证？

**分析**：

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **无验证** | 快速通道独立运行，不与 Checkpoint 对比 | 简单 | 数据偏离无法检测 |
| **定时对账** | dex-streamer 定期与 Checkpoint 全量快照对比 | 能检测偏离 | 需要 Checkpoint 提供快照 |
| **序列号校验** | 通过 sequence gap 检测丢失，用快照恢复 | 轻量级 | 只能检测丢失，不能检测错误 |

### 3.4 Checkpoint 通道的事件策略

> **核心问题**：Checkpoint 通道还需要发送 `OrderbookSnapshotEvent` 吗？

**现状**：当前每次订单操作都发射 `OrderbookSnapshotEvent`（全量，~3KB），通过 Checkpoint 通道进入 dex-indexer → Redis。

**选项**：

| 选项 | Checkpoint 事件 | 快速通道事件 | 带宽 | 恢复能力 |
|------|----------------|-------------|------|---------|
| **A) 保持现状** | 每次全量快照 | 增量 delta | 高（~3KB/操作） | 强 |
| **B) Checkpoint 降频** | 每 N 操作一次全量快照 | 增量 delta | 中 | 中 |
| **C) Checkpoint 不发送订单簿** | 无 | 增量 delta + 定时全量 | 低 | 依赖快速通道 |
| **D) Checkpoint 改为增量** | 增量 delta | 增量 delta | 低 | 强（序列号连续可重建） |

### 3.5 建议

**推荐方案 B（Checkpoint 降频快照）**：

1. **快速通道**：每次操作发射 `OrderbookDeltaEvent`（增量，~55bytes）
2. **Checkpoint 通道**：每 100 次操作或每 5 秒发射一次 `OrderbookSnapshotEvent`（全量）
3. **恢复机制**：dex-streamer 检测到 sequence gap 时，等待下一次 Checkpoint 全量快照恢复
4. **定时对账**：dex-streamer 每 30 秒对比内存 L2Book 与 Checkpoint 快照

这样 Checkpoint 通道的带宽从 ~3KB/操作 降为 ~3KB/5秒 = ~600B/s，减少了 **99%**。

### 3.6 决策项

> **[决策 Q3]** Checkpoint 通道订单簿事件策略：
> - A) 保持每次操作发送全量快照（现状，最安全）
> - B) 降频发送：每 100 次操作或每 5 秒一次全量快照
> - C) 完全不发送订单簿事件
> - D) Checkpoint 也改为增量 delta
> - **✅ C+) Checkpoint 不发送任何订单簿事件 + gRPC GetSnapshot() 从 InlineOrderbook 恢复** ← 已确认
>
> **决策理由**：参考 dYdX 模式 — dYdX 不通过链上通道发送订单簿快照，订单簿完全在内存中维护（MemClob）。本项目的 DEX 引擎已有 `InlineOrderbook`（内存订单簿），与 dYdX MemClob 等价。恢复机制通过 gRPC `GetSnapshot()` 直接从 InlineOrderbook 读取完整 L2 快照，无需 Checkpoint 通道参与。
>
> **C+ 方案核心**：
> - Checkpoint 通道完全不发送 `OrderbookSnapshotEvent`（当前每次操作发射的全量快照可移除）
> - `OrderbookDeltaEvent` 也不通过 Checkpoint（不纳入 TransactionEvents），仅走 gRPC
> - dex-streamer 断线/gap 恢复：调用 gRPC `GetSnapshot(market_id)` → 从 InlineOrderbook 读取完整 L2
> - 与 dYdX 的 MemClob + gRPC Stream 模式一致

---

## Q4: 除了订单簿，还有哪些功能走快速通道？

### 4.1 dYdX 快速通道的事件类型

dYdX 的 `StreamUpdate` 包含 5 种事件类型：

| 事件类型 | CheckTx（乐观） | DeliverTx（最终） | 说明 |
|---------|----------------|-------------------|------|
| OrderbookUpdate | ✅ | ✅ | 订单放置/移除/更新 |
| OrderFill | ✅ | ✅ | 成交事件 |
| TakerOrder | ✅ | ❌ | Taker 订单状态（仅乐观） |
| SubaccountUpdate | ❌ | ✅ | 子账户余额变化 |
| PriceUpdate | ❌ | ✅ | Oracle 价格更新 |

### 4.2 本项目候选事件分析

| 事件 | 低延迟需求 | 数据频率 | 建议 |
|------|-----------|---------|------|
| **OrderbookDeltaEvent** | 极高 — 订单簿是交易核心 | 高（每笔订单操作） | **Phase 6 必须** |
| **FillEvent** | 高 — 成交通知、最近成交展示 | 中（有成交时） | **Phase 6 建议包含** |
| **OrderUpdateEvent** | 高 — 用户订单状态实时更新 | 中 | **Phase 6 建议包含** |
| **OrderPlacedEventV1** | 中 — 新订单上簿 | 中 | 可选（OrderUpdate 已覆盖） |
| **OrderRemovedEventV1** | 中 — 订单移除 | 低 | 可选（OrderUpdate 已覆盖） |
| PositionUpdateEvent | 中 — 仓位变化 | 低 | Phase 6+ |
| BalanceUpdateEvent | 低 — 余额变化 | 低 | Phase 6+ |
| FundingSettlementEvent | 低 — 资金费结算 | 极低 | 不需要快速通道 |
| LiquidationEvent | 低 — 清算事件 | 极低 | 不需要快速通道 |

### 4.3 建议

**Phase 6 快速通道初始事件清单**：

| 优先级 | 事件 | 理由 |
|--------|------|------|
| **P0** | `OrderbookDeltaEvent` | 订单簿是核心目标 |
| **P0** | `FillEvent` | 成交通知延迟直接影响交易体验 |
| **P1** | `OrderUpdateEvent` | 用户订单状态更新 |
| **P1** | `OrderPlacedEventV1` | 新订单上簿通知 |
| **P1** | `OrderRemovedEventV1` | 订单移除通知 |

框架设计为通用化（`DexStreamEvent` 枚举已包含上述类型），后续扩展只需在 `filter_and_parse_events()` 中新增分支。

### 4.4 决策项

> **[决策 Q4]** Phase 6 快速通道初始事件范围：
> - A) 仅 OrderbookDeltaEvent（最小范围，聚焦订单簿优化）
> - **✅ B) OrderbookDeltaEvent + FillEvent + OrderUpdateEvent（覆盖交易核心）** ← 已确认
> - C) 所有交易相关事件（最大范围，含 Position/Balance 等）

---

## Q5: 快速通道事件类型 — 新类型 vs 复用 Sui 事件

### 5.1 问题拆解

这个问题实际上包含两个子问题：

1. 快速通道的事件是否需要**新的传输类型**（类似 dYdX 的 `OffChainUpdates`）？
2. 快速通道事件与 Checkpoint 通道事件是否需要**不同的数据结构**？

### 5.2 dYdX 的事件类型架构

dYdX 有**三套独立的事件类型定义**：

```
┌────────────────────────────────┐
│ 1. OffChainUpdates (off_chain_updates.proto)    │
│    - OrderPlaceV1                                │
│    - OrderRemoveV1                               │
│    - OrderUpdateV1                               │
│    用途：Kafka → Vulcan 消费                     │
└────────────────────────────────┘
         ↕ 不同类型
┌────────────────────────────────┐
│ 2. StreamUpdate (query.proto)                    │
│    - StreamOrderbookUpdate                       │
│    - StreamOrderbookFill                         │
│    - StreamTakerOrder                            │
│    - StreamSubaccountUpdate                      │
│    - StreamPriceUpdate                           │
│    用途：gRPC Stream → 客户端/索引器             │
└────────────────────────────────┘
         ↕ 不同类型
┌────────────────────────────────┐
│ 3. OnChainUpdates (transaction events)           │
│    - Cosmos 交易事件                             │
│    用途：Kafka → Ender → PostgreSQL              │
└────────────────────────────────┘
```

dYdX 之所以需要三套类型，是因为：
- OffChainUpdates 走 Kafka，需要 protobuf 序列化
- StreamUpdate 走 gRPC，需要独立 proto 定义
- OnChainUpdates 走 Cosmos 事件系统，格式固定

### 5.3 我们的情况

本项目与 dYdX 的关键差异：

| 维度 | dYdX | 本项目 |
|------|------|--------|
| 链上事件格式 | Cosmos 事件（attribute-based） | Sui 事件（BCS 序列化） |
| 事件标识 | 无统一标识 | `DEX_EVENTS_PACKAGE` 虚拟包地址 |
| 快速通道传输 | Kafka / gRPC（跨进程） | `tokio::broadcast`（进程内） |
| 序列化格式 | protobuf | BCS（已有） |

**关键优势**：我们的事件已经有统一的 `DEX_EVENTS_PACKAGE` 标识，通过 `event.package_id` 过滤只需一次 32 字节比较（~10ns），**无需定义新的事件类型来区分通道**。

### 5.4 方案对比

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **A) 复用 Sui 事件** | 快速通道直接读取 `TransactionEvents` 中的 DEX 事件 | 零额外定义；事件一处定义、两通道共用；filter 仅需 package_id 比较 | 两通道共享同一事件流 |
| **B) 定义新的 OffChain 类型** | 新增 `DexOffChainEvent` 枚举，独立于 `TransactionEvents` | 通道完全解耦；可针对通道优化字段 | 维护两套类型定义；增加转换层 |
| **C) 混合：复用 + 新增** | 复用现有事件类型，但 `OrderbookDeltaEvent` 是新增的，仅走快速通道 | 兼顾两者 | `OrderbookDeltaEvent` 不走 Checkpoint 需要特殊处理 |

### 5.5 建议

**推荐方案 A（复用 Sui 事件）**，理由：

1. **过滤已有现成机制**：`DEX_EVENTS_PACKAGE` 过滤只需 ~10ns，无需额外类型区分
2. **事件定义单一来源**：`dex_events.rs` 一处定义，避免同步维护两套类型
3. **BCS 序列化已经高效**：进程内 broadcast 传递 `Arc<DexStreamBatch>` 甚至无需序列化
4. **新事件（OrderbookDeltaEvent）也走统一机制**：同时出现在 TransactionEvents 和快速通道
5. **dex-streamer/dex-indexer 可共享反序列化代码**

`OrderbookDeltaEvent` 作为新事件类型，纳入 `TransactionEvents`（会进入 Checkpoint），同时通过快速通道推送。Checkpoint 通道的 dex-indexer 可以选择处理或忽略它。

### 5.6 决策项

> **[决策 Q5]** 快速通道事件类型策略：
> - **✅ A adjusted) 复用 Sui 事件类型定义，但 OrderbookDeltaEvent 不纳入 TransactionEvents** ← 已确认
> - B) 定义独立的 OffChain 事件类型
>
> **附加决策**：OrderbookDeltaEvent 是否纳入 TransactionEvents（进入 Checkpoint）？
> - A1) 纳入（事件定义统一，Checkpoint 可选处理）
> - **✅ A2) 不纳入（仅走 gRPC 快速通道）** ← 已确认（配合 Q3=C+ 决策）
>
> **决策理由**：配合 Q3=C+ 决策，Checkpoint 通道不需要任何订单簿事件。OrderbookDeltaEvent 的类型定义仍复用 `dex_events.rs`（统一事件格式），但不通过 `emit_event()` 写入 TransactionEvents。引擎内部计算 delta 后，直接通过 DexStreamingManager 的 gRPC 通道推送，不经过 Checkpoint。同时，`OrderbookSnapshotEvent` 也可以停止发射（移除带宽浪费），因为恢复机制已改为 gRPC GetSnapshot()。

---

## Q6: DexStreamingManager 在全节点还是验证者节点？

### 6.1 执行管线分析结果

基于深入的代码分析（见 Q1 §1.3），关键结论：

| 节点类型 | `post_process_one_tx()` 触发时机 | 事件延迟 |
|---------|------|------|
| **验证者** | 每笔交易执行后**立即** | **微秒级** |
| **全节点** | Checkpoint 批量执行后**逐笔** | **Checkpoint 级**（1-3 秒） |

### 6.2 原因分析

**验证者节点**的执行路径：
```
handle_consensus_commit() → ExecutionScheduler → 逐笔执行 → post_process_one_tx()
```
每笔交易独立执行、独立触发，事件立即可用。

**全节点**的执行路径：
```
同步 Checkpoint → execute_checkpoint() → 批量执行所有交易 → 等待全部完成 → process_checkpoint_data()
```
全节点从网络同步 Checkpoint，然后批量执行，`post_process_one_tx()` 虽然也会逐笔触发，但整个执行发生在 Checkpoint 同步之后，**延迟等同于 Checkpoint 通道**。

### 6.3 dYdX 的做法

dYdX 的 `FullNodeStreamingManager`：
- **验证者和全节点都可以运行**（通过 `getFullNodeStreamingManagerFromOptions()` 配置）
- 提供 `NoOp` 实现用于禁用
- CheckTx 阶段的乐观事件仅在**接收交易的节点**上触发（通常是验证者）

### 6.4 建议

通过 `NodeConfig.enable_dex_streaming` 控制，**两种节点都支持**：

| 场景 | 配置 | 效果 |
|------|------|------|
| DEX 验证者节点 | `enable_dex_streaming: true` | 低延迟（<50ms） |
| DEX 全节点 | `enable_dex_streaming: true` | Checkpoint 级延迟（功能正确，延迟无优势） |
| 普通 Sui 节点 | `enable_dex_streaming: false`（默认） | 零开销 |

**当前开发/测试环境**：Docker 中运行的是单验证者节点，天然支持低延迟。

### 6.5 决策项

> **[决策 Q6]** 此决策较为明确，无需用户决策。
>
> - NodeConfig 配置控制，默认关闭
> - 验证者和全节点都支持启用
> - 文档中注明：低延迟效果仅在验证者节点上生效

---

## Q7: dex-streamer — 新服务 vs 进程内 vs gRPC

### 7.1 三种架构选项

#### 选项 A: 进程内 `tokio::broadcast`（当前方案）

```
┌─────────────────────────────────────────┐
│  sui-node 进程                            │
│  ┌──────────────────┐  ┌──────────────┐ │
│  │ DexStreamingMgr  │→→│ dex-streamer │ │
│  │ (broadcast send) │  │ (subscribe)  │ │
│  └──────────────────┘  └──────┬───────┘ │
│                                │         │
└────────────────────────────────┼─────────┘
                                 │ Redis
                                 ↓
                          ┌──────────────┐
                          │   dex-api    │
                          └──────────────┘
```

- dex-streamer 是 sui-node 进程内的一个模块（或 tokio task）
- 通过 `tokio::broadcast` 接收事件
- 延迟：<1μs（进程内内存共享）
- 部署：与 sui-node 同进程

#### 选项 B: gRPC Streaming（类似 dYdX）

```
┌──────────────────────┐        ┌──────────────────┐
│  sui-node 进程         │ gRPC   │  dex-streamer     │
│  ┌──────────────────┐ │ stream │  (独立进程/服务)   │
│  │ DexStreamingMgr  │─┼───────→│  - orderbook build │
│  │ (gRPC server)    │ │        │  - Redis write     │
│  └──────────────────┘ │        └────────┬───────────┘
└──────────────────────┘                  │ Redis
                                          ↓
                                   ┌──────────────┐
                                   │   dex-api    │
                                   └──────────────┘
```

- dex-streamer 是独立进程/服务
- 通过 gRPC stream 接收事件（类似 dYdX `StreamOrderbookUpdates`）
- 延迟：~1-5ms（gRPC protobuf 序列化 + 网络）
- 部署：可独立扩展

#### 选项 C: 混合（先进程内，trait 抽象支持升级）

```rust
// StreamTransport trait（当前方案中已定义）
#[async_trait]
pub trait StreamTransport: Send + Sync + 'static {
    async fn subscribe(&self) -> Result<Box<dyn StreamReceiver>>;
    async fn publish(&self, batch: DexStreamBatch) -> Result<()>;
}

// Phase 6: BroadcastTransport (进程内)
// Phase 6.1+: GrpcTransport (跨进程)
```

### 7.2 详细对比

| 维度 | A) 进程内 broadcast | B) gRPC Streaming | C) 混合 |
|------|-------------------|-------------------|---------|
| **延迟** | <1μs | ~1-5ms | <1μs → ~1-5ms |
| **部署复杂度** | 低（嵌入 sui-node） | 中（独立服务） | 低 → 中 |
| **扩展性** | 低（单进程） | 高（独立扩展） | 低 → 高 |
| **故障隔离** | 差（崩溃影响 node） | 好（独立重启） | 差 → 好 |
| **序列化开销** | 零（Arc 共享） | 有（protobuf/BCS） | 零 → 有 |
| **开发工作量** | 低（~2 天） | 中（~4-5 天） | 低（先 A，再升级） |
| **Docker 服务数** | 不增加 | +1（dex-streamer） | 不增加 → +1 |
| **运维** | 简单 | 需要管理新服务 | 简单 → 需要 |

### 7.3 dYdX 为什么用 gRPC

dYdX 使用 gRPC 是因为：
1. **Go 语言生态**：gRPC 是 Go 的标准 RPC 框架
2. **Cosmos SDK 架构**：Cosmos 节点通过 gRPC 暴露服务是标准模式
3. **多消费者需求**：支持外部索引器、交易前端等多个消费者
4. **独立部署**：索引器可以独立扩展
5. **10ms 批处理**：批处理摊平了 gRPC 的序列化开销

### 7.4 建议

**推荐选项 A（进程内 broadcast），不建议直接一步到位 gRPC**。理由：

1. **当前部署场景简单**：单验证者节点 + 单 dex-streamer 消费者，不需要跨进程通信
2. **延迟优势明显**：进程内 <1μs vs gRPC ~1-5ms
3. **开发速度快**：少 2-3 天开发时间，可以更快验证架构可行性
4. **升级成本低**：`StreamTransport` trait 抽象已在方案中（03-transport-protocol.md），后续加 gRPC 实现只需 1-2 天
5. **Docker 不增加服务**：dex-streamer 作为 sui-node 进程内的 tokio task 运行

**如果未来需要 gRPC**（多节点部署、独立扩展），只需实现 `GrpcTransport`，代码改动量小。

### 7.5 决策项

> **[决策 Q7]** dex-streamer 部署模式：
> - A) 进程内 tokio::broadcast
> - **✅ B) 直接 gRPC streaming（dex-streamer 作为独立服务）** ← 已确认
> - C) 先 A 后 B
>
> **决策理由**：配合 Q3=C+ 决策，gRPC 是必要的传输方式：
> 1. gRPC `GetSnapshot()` 接口用于 dex-streamer 断线恢复（从 InlineOrderbook 读取完整 L2）
> 2. gRPC `Subscribe()` 接口用于流式接收 DexStreamBatch
> 3. dex-streamer 作为独立 Docker 服务，实现故障隔离（崩溃不影响 sui-node）
> 4. 与 dYdX FullNodeStreamingManager 架构一致

---

## Q8: L2 Book vs L3 Book

### 8.1 定义

| 维度 | L2 Book（价格档位聚合） | L3 Book（单笔订单级） |
|------|----------------------|---------------------|
| **数据粒度** | 每个价格点的**总数量** | 每笔**独立订单** |
| **典型数据** | `{price: 50000, qty: 15.5}` | `{order_id: "abc", price: 50000, qty: 3.2, time: ...}` |
| **展示** | 交易所常见的"深度图" | 高级订单流面板 |
| **数据量（100档）** | 200 × 16B = ~3.2KB | 2000 订单 × ~100B = ~200KB |
| **更新频率** | 每次价格档位变化 | 每笔订单变化 |
| **隐私** | 不泄露单笔订单信息 | 暴露所有订单细节 |

### 8.2 常见交易所的选择

| 交易所 | 公开 API | 内部/高级 |
|--------|---------|---------|
| **Binance** | L2（depth stream） | L2 |
| **Hyperliquid** | L2 (`l2Book`) | L2 |
| **dYdX v4** | **L3**（StreamOrderbookUpdate 含单笔订单） | L3 |
| **Coinbase** | L2 + L3（分别提供） | L3 |

### 8.3 dYdX 使用 L3 的原因

dYdX 发送的 `OffchainUpdates` 是 L3 级别（`OrderPlaceV1`/`OrderRemoveV1`/`OrderUpdateV1`），每条消息对应一笔具体订单。这是因为：

1. dYdX 的订单簿存储在内存中（`MemClob`），天然以单笔订单为单位管理
2. dYdX 是全透明的链上 DEX，所有订单信息公开
3. L3 数据允许做市商精确跟踪队列位置

### 8.4 我们的推荐

**Phase 6 使用 L2**（当前方案），理由：

1. **数据量小**：L2 delta ~55 bytes vs L3 单笔订单 ~100 bytes × N 笔
2. **客户端兼容**：前端深度图显示的就是 L2 数据（价格-数量聚合）
3. **对标 Hyperliquid**：Hyperliquid 的 `l2Book` WS 频道推送的就是 L2
4. **隐私保护**：不暴露单笔订单详情
5. **现有 `OrderUpdateEvent` 已提供 L3 信息**：用户级订单状态通过 `orderUpdates` WS 频道推送

**未来如果需要 L3**：
- 可新增 `l3Book` WS 频道
- 复用现有的 `OrderPlacedEventV1` / `OrderRemovedEventV1` / `OrderUpdateEvent`
- 这些事件已经在快速通道的候选列表中

### 8.5 决策项

> **[决策 Q8]** 此决策较为明确，无需用户决策。
>
> - Phase 6 使用 L2 Book（价格档位聚合），与 Hyperliquid 对标
> - L3 数据已通过 `orderUpdates` WS 频道提供
> - 未来按需新增 `l3Book` 频道

---

## 决策汇总

> 更新日期: 2026-02-27 | 全部决策已确认

| # | 决策项 | 确认选项 | 状态 |
|---|--------|---------|------|
| Q1 | 快速通道部署策略 | B) 验证者 + 全节点都支持，NodeConfig 配置 | ✅ 已确认 |
| Q2 | WS 订单簿推送格式 | A) 全量推送（对标 Hyperliquid l2Book） | ✅ 已确认 |
| Q3 | Checkpoint 通道订单簿事件 | C+) 不发送任何订单簿事件 + gRPC GetSnapshot 从 InlineOrderbook 恢复 | ✅ 已确认 |
| Q4 | Phase 6 快速通道事件范围 | B) OrderbookDelta + Fill + OrderUpdate | ✅ 已确认 |
| Q5 | 快速通道事件类型策略 | A adjusted) 复用 Sui 事件类型，但 Delta 不纳入 TransactionEvents，仅走 gRPC | ✅ 已确认 |
| Q6 | 全节点 vs 验证者 | 两者都支持，低延迟仅验证者有效 | ✅ 无需决策 |
| Q7 | dex-streamer 部署模式 | B) gRPC streaming，dex-streamer 作为独立 Docker 服务 | ✅ 已确认 |
| Q8 | L2 vs L3 Book | L2 Book（对标 Hyperliquid） | ✅ 无需决策 |

### 决策间的关联关系

```
Q3=C+ (Checkpoint 不发送订单簿)
  ├─→ Q5=A2 (OrderbookDeltaEvent 不纳入 TransactionEvents)
  ├─→ Q7=B  (gRPC 必要 — GetSnapshot 用于恢复)
  └─→ OrderbookSnapshotEvent 可停止发射（节省带宽）

Q7=B (gRPC streaming)
  ├─→ dex-streamer 独立 Docker 服务
  ├─→ DexStreamingManager 内嵌 gRPC server（Subscribe + GetSnapshot）
  └─→ 延迟从 <1μs 变为 ~1-5ms（仍满足 <50ms 目标）

Q2=A (WS 全量推送)
  ├─→ 简化客户端（无需维护本地 orderbook）
  ├─→ 与 Hyperliquid l2Book 行为一致
  └─→ l2BookDelta 增量频道暂不实现（未来按需新增）
```
