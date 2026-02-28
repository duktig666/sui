# Phase 6: 订单簿索引优化 — 低延迟流式架构

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: 架构设计完成（全部决策已确认，见 [08-architecture-qa.md](./08-architecture-qa.md)）

## 1. 问题分析

### 1.1 当前架构延迟瓶颈

当前订单簿数据从引擎执行到前端展示的完整链路：

```
DEX 引擎执行 (order.rs)
    │ build_snapshot() → OrderbookSnapshotEvent (全量快照, ~10KB)
    ↓
TransactionEvents → TransactionEffects
    ↓
Checkpoint 构建 ─────────────────────── ~1-3s
    ↓
dex-indexer (Checkpoint pipeline)
    │ BCS 反序列化 → Redis HSET + XADD
    ↓
Redis Stream (dex:stream:orderbook)
    ↓
dex-api StreamConsumer → WebSocket 推送
    ↓
前端显示 ───────────────────────────── 总延迟 200-500ms+
```

**核心问题**：

| 问题 | 描述 | 影响 |
|------|------|------|
| **Checkpoint 延迟** | 事件必须等待 Checkpoint 构建和同步后才能被索引 | +1-3s 延迟 |
| **全量快照模式** | 每次订单变更推送完整订单簿 (`bids` + `asks` 全部价格档位) | ~10KB/event，带宽浪费 |
| **无增量推送** | 客户端收到的是全量替换，无法做增量更新 | 前端闪烁，渲染压力 |
| **单通道架构** | 只有 Checkpoint 一个数据通道 | 无法满足 <50ms 延迟需求 |

### 1.2 延迟分解

基于 Phase 2 延迟分析 (`08-realtime-latency-analysis.md`)：

| 阶段 | 延迟 | 累计 |
|------|------|------|
| 用户下单 → 共识排序 | ~400ms | 400ms |
| 引擎撮合计算 | <10ms | 410ms |
| **引擎执行完成 → effects 生成** | **~1ms** | **411ms** |
| effects → Checkpoint 构建 | ~1-3s | 1.4-3.4s |
| Checkpoint → Indexer 处理 | ~100ms | 1.5-3.5s |
| Redis → WS 推送 | ~10ms | 1.5-3.5s |

**关键发现**：引擎执行完成到 effects 生成仅需 ~1ms，但 effects 到 Checkpoint 需要 1-3s。如果能在 effects 生成后立即拦截事件并推送，可以将后半段延迟从 1-3s 降至 <50ms。

### 1.3 数据量分析

当前全量快照模式（100 档深度）：

```
单次快照: 200 档 × 16 bytes (price u64 + quantity u64) = 3.2KB + 开销 ≈ 5-10KB
10 个市场 × 4 次/秒 = 200-400 KB/s
```

增量 delta 模式：

```
单次 delta: 1-5 个价格档位变更 × ~60 bytes = 60-300 bytes
10 个市场 × 100 次/秒 = 60-300 KB/s（可能更低）
```

增量模式在高频场景下带宽节省可达 10-50 倍。

---

## 2. 设计目标

### 2.1 延迟目标

| 指标 | 当前值 | 目标值 | 参考（dYdX） |
|------|--------|--------|-------------|
| 订单簿更新延迟 | 1.5-3.5s | **<50ms** | 10-50ms (OffChain) |
| BBO 更新延迟 | 1.5-3.5s | **<20ms** | ~10ms |
| 成交通知延迟 | 1.5-3.5s | **<50ms** | 10-50ms |

### 2.2 功能目标

1. **全量订单簿低延迟推送**: WS l2Book 频道推送完整 L2 快照（对标 Hyperliquid），延迟从 1.5-3.5s 降至 <50ms
2. **BBO (Best Bid/Offer) 频道**: 专用最优买卖价频道，极低延迟
3. **gRPC 流式通道**: DexStreamingManager 内嵌 gRPC server，dex-streamer 作为独立服务消费
4. **通用流式框架**: 快速通道承载 OrderbookDelta + Fill + OrderUpdate 三类事件
5. **故障自愈**: dex-streamer 断连后通过 gRPC `GetSnapshot()` 从 InlineOrderbook 恢复

### 2.3 非目标

- 本阶段不修改共识机制或执行流程
- 不改变现有 Checkpoint 索引管线（保留 fills/orders/positions 的持久化）
- Checkpoint 通道不再发送订单簿事件（OrderbookSnapshotEvent 停止发射）
- 本阶段不实现 WS 增量推送频道（l2BookDelta），全部使用全量推送

---

## 3. 整体架构

### 3.1 双通道架构（2026-02-27 更新）

参考 dYdX v4 的 MemClob + gRPC Stream 模式，结合本项目 InlineOrderbook：

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Sui Validator Node (sui-node)                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │ DEX Execution Engine (sui-execution/src/dex/)                     │  │
│  │  execute_place_order()  → fills + orderbook changes               │  │
│  │  execute_cancel_order() → orderbook changes                       │  │
│  │  InlineOrderbook ← 内存订单簿（类似 dYdX MemClob）                │  │
│  └──────────┬────────────────────────────────────────────────────────┘  │
│             │ TransactionOutputs (DEX events: Fill + OrderUpdate)       │
│             │ ※ OrderbookDeltaEvent 不在 TransactionEvents 中           │
│             │                                                           │
│    ┌────────┴─────────────────────────────────────────────────────┐     │
│    │                                                              │     │
│    ▼ (gRPC 快速通道, <5ms)                        ▼ (Checkpoint 通道)   │
│  ┌─────────────────────────┐         ┌──────────────────────┐          │
│  │ DexStreamingManager     │         │ Checkpoint pipeline  │          │
│  │ (AuthorityState 回调)   │         │ (现有 dex-indexer)   │          │
│  │ - 内嵌 gRPC server      │         │ - fills → PG+Redis   │          │
│  │ - Subscribe() stream    │         │ - orders → PG+Redis  │          │
│  │ - GetSnapshot() 快照    │         │ - positions → PG     │          │
│  │   ↑ 读取 InlineOrderbook │         │ - candle/stats 聚合 │          │
│  └──────────┬──────────────┘         └──────────┬───────────┘          │
│             │ gRPC                               │                      │
└─────────────┼────────────────────────────────────┼──────────────────────┘
              │                                    │
              ▼                                    ▼
┌──────────────────────────┐       ┌──────────────────────────────┐
│  dex-streamer             │       │  dex-indexer (现有)           │
│  (独立 Docker 服务)       │       │  - fills → PG + Redis        │
│  - gRPC Subscribe() 消费  │       │  - orders → PG + Redis       │
│  - GetSnapshot() 恢复     │       │  - positions → PG + Redis    │
│  - 内存 L2Book 构建       │       │  - candle/stats 聚合         │
│  - Redis 增量写入         │       │  ※ 不再处理订单簿事件         │
└──────────┬───────────────┘       └──────────────────────────────┘
           │
           ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  Redis                                                                    │
│  ┌─────────────────────────────┐   ┌──────────────────────────────────┐  │
│  │ L2 Book 通道 (新)           │   │ 持久化通道 (现有)                │  │
│  │ HSET dex:l2book:{id}       │   │ fills/orders/positions → PG      │  │
│  │ XADD dex:stream:l2:update  │   │ XADD dex:stream:fills           │  │
│  │ HSET dex:bbo:{id}          │   │ XADD dex:stream:orders          │  │
│  └─────────────────────────────┘   └──────────────────────────────────┘  │
└──────────────────┬───────────────────────────────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  dex-api (增强)                                                          │
│  - REST: l2Book 读 dex:l2book:{id} (HSET)                               │
│  - WS: l2Book 推送完整 L2 快照（对标 Hyperliquid，全量推送）              │
│  - WS: BBO 频道 (dex:bbo:{id})                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

### 3.2 与 dYdX 对比

| 维度 | dYdX v4 | 本项目 Phase 6 |
|------|---------|----------------|
| 低延迟通道 | FullNodeStreamingManager (gRPC) | DexStreamingManager (gRPC) |
| 内存订单簿 | MemClob | InlineOrderbook |
| 触发时机 | CheckTx (交易验证阶段) | AuthorityState 执行后回调 |
| 传输协议 | gRPC streaming | gRPC streaming |
| 中间层 | 内嵌在节点 + 外部索引器 | dex-streamer (独立 Docker 服务) |
| 恢复机制 | InitializeNewStreams() 从 MemClob | GetSnapshot() 从 InlineOrderbook |
| Checkpoint 订单簿 | 不通过链上通道发送 | 不通过 Checkpoint 发送 (Q3=C+) |
| 订单簿存储 | Redis HSET (L3 订单级) | Redis HSET (L2 价格档位级) |
| WS 推送 | snapshot + 增量 | 全量快照推送 (Q2=A, 对标 Hyperliquid) |

### 3.3 关键架构决策（2026-02-27 确认）

| # | 决策 | 选择 | 理由 | QA 编号 |
|---|------|------|------|---------|
| 1 | Hook 点位置 | AuthorityState 层 `post_process_one_tx()` 回调 | 微秒级延迟（验证者），类似 dYdX FullNodeStreamingManager | Q1=B |
| 2 | 传输协议 | **gRPC streaming**（一步到位） | GetSnapshot 恢复需要 gRPC；dex-streamer 独立服务需要跨进程通信 | Q7=B |
| 3 | 增量粒度 | L2 价格档位 delta | ~60 bytes/event，L3 由现有 OrderUpdateEvent 覆盖 | Q8=L2 |
| 4 | Checkpoint 订单簿 | **不发送任何订单簿事件** | 恢复通过 gRPC GetSnapshot() 从 InlineOrderbook 读取 | Q3=C+ |
| 5 | 快速通道事件范围 | **OrderbookDelta + Fill + OrderUpdate** | 覆盖交易核心；Delta 不纳入 TransactionEvents | Q4=B, Q5=A2 |
| 6 | WS 推送格式 | **全量 L2 快照推送** | 对标 Hyperliquid l2Book；客户端简单（直接替换） | Q2=A |
| 7 | 新 crate 与部署 | 新建 `dex-streamer` 独立 Docker 服务 | 故障隔离；独立扩展；与 sui-node 解耦 | Q7=B |

---

## 4. 文档索引

| # | 文档 | 内容 | 状态 |
|---|------|------|------|
| 01 | [streaming-source](./01-streaming-source.md) | DexStreamingManager 设计：Sui 执行层 hook 点、事件拦截、gRPC server | ⚠️ 待更新（gRPC） |
| 02 | [event-design](./02-event-design.md) | 增量事件类型设计：OrderbookDeltaEvent、事件版本策略 | ⚠️ 待更新（不纳入 TransactionEvents） |
| 03 | [transport-protocol](./03-transport-protocol.md) | 传输协议：gRPC streaming、批处理、背压 | ⚠️ 待更新（gRPC 为 Phase 1） |
| 04 | [offchain-orderbook](./04-offchain-orderbook.md) | 链下订单簿构建：dex-streamer 独立服务、Redis 增量存储 | ⚠️ 待更新（gRPC 恢复） |
| 05 | [api-ws-integration](./05-api-ws-integration.md) | API/WebSocket 集成：全量 L2 推送、BBO 频道 | ⚠️ 待更新（全量推送） |
| 06 | [consistency-model](./06-consistency-model.md) | 一致性模型：gRPC GetSnapshot 恢复、故障自愈 | ⚠️ 待更新（gRPC 恢复） |
| 07 | [implementation-plan](./07-implementation-plan.md) | 分步实施计划、里程碑、验证方案 | ⚠️ 待更新 |
| 08 | [architecture-qa](./08-architecture-qa.md) | 架构 Q&A：8 个关键问题分析与决策（全部已确认） | ✅ 完成 |

---

## 5. 参考资料

| 资料 | 位置 |
|------|------|
| Phase 2 延迟分析 | `phase2_realtime/08-realtime-latency-analysis.md` |
| 订单簿推送策略分析 | `phase2_realtime/12-orderbook-push-strategy-analysis.md` |
| dYdX OffChain 分析 | `phase2_realtime/14-dydx-offchain-updates-analysis.md` |
| dYdX Streaming 参考 | `phase2_realtime/03-dydx-streaming-reference.md` |
| Redis 消息规范 | `phase2_realtime/10-redis-message-spec.md` |
| WebSocket 协议规范 | `phase2_realtime/11-websocket-protocol-spec.md` |
| 当前索引器架构 | `../../docs/indexer/arch/dex-indexer-structure-latest.md` |
| DEX 事件类型 | `dex-sui/crates/sui-types/src/dex_events.rs` |
| 订单执行代码 | `dex-sui/sui-execution/src/dex/commands/order.rs` |
