# Phase 2 实时通道分析总结

## 概述

本文档总结 Phase 2（dex-realtime + dex-ws）技术方案的分析结论，包括与 dYdX 的差异分析、Sui 架构限制以及推荐的实现方案。

---

## 1. Phase 2 缺失项总结

### 1.1 当前状态

Phase 1 已完成 dex-indexer 和 dex-api 的基础功能：
- dex-indexer：通过 Checkpoint API 持久化 FillEventV1、PositionUpdateEventV1 等事件
- dex-api：提供 Hyperliquid 兼容的 HTTP 查询接口

### 1.2 Phase 2 缺失的核心组件

| 组件 | 状态 | 说明 |
|------|------|------|
| dex-realtime | 缺失 | 实时事件监听和发布服务 |
| dex-ws | 缺失 | WebSocket 推送服务 |
| 订单簿维护 | 缺失 | 内存订单簿构建和快照缓存 |
| K 线聚合 | 缺失 | 多周期 K 线实时计算 |
| 市场统计 | 缺失 | 24h 成交量、资金费率等统计数据 |

### 1.3 缺失的事件类型

| 事件 | 用途 | 当前状态 |
|------|------|----------|
| OrderPlacedEventV1 | 订单进入订单簿 | 未定义 |
| OrderRemovedEventV1 | 订单移除（取消/成交/清算） | 未定义 |

---

## 2. 与 dYdX 的差异分析

### 2.1 架构差异

| 方面 | dYdX v4 | 本项目 |
|------|---------|--------|
| 共识层 | Cosmos SDK + CometBFT | Sui + Narwhal/Bullshark |
| 匹配引擎 | Application Chain 内置 | Rust 自定义引擎（sui-execution） |
| 事件分发 | Off-chain Updates + gRPC Stream | 链上事件 + RPC 订阅 |
| 索引节点 | 专用 Full Node Stream | 标准 Sui Full Node |

### 2.2 事件设计差异

**dYdX 双通道设计**：
```
┌─────────────────────────────────────────────────────┐
│  dYdX v4 事件分发                                    │
├─────────────────────────────────────────────────────┤
│  On-chain Updates (Indexer)                         │
│  - 通过区块数据获取                                   │
│  - 最终一致性，延迟 ~2-3s                            │
│  - 用于持久化和历史查询                               │
├─────────────────────────────────────────────────────┤
│  Off-chain Updates (Streaming)                      │
│  - 通过 FullNodeStreamingManager                    │
│  - 低延迟 <100ms                                    │
│  - 用于实时推送                                      │
└─────────────────────────────────────────────────────┘
```

**本项目设计**：
```
┌─────────────────────────────────────────────────────┐
│  DEX 事件分发（统一事件，双通道消费）                   │
├─────────────────────────────────────────────────────┤
│  dex-indexer (Checkpoint API)                       │
│  - 轮询 Checkpoint 获取事件                          │
│  - 最终一致性，延迟 ~2-3s                            │
│  - 用于持久化和历史查询                               │
├─────────────────────────────────────────────────────┤
│  dex-realtime (sui_subscribeEvent)                  │
│  - WebSocket 订阅链上事件                            │
│  - 低延迟 <500ms                                    │
│  - 用于实时推送                                      │
└─────────────────────────────────────────────────────┘
```

### 2.3 关键差异说明

1. **无 Off-chain Updates**：Sui 不支持类似 dYdX 的应用层事件分发，所有事件必须通过链上发射
2. **统一事件定义**：同一事件结构服务两个通道，简化维护
3. **无需专用索引节点**：使用标准 Sui Full Node 的 RPC 接口

---

## 3. Sui 架构对 Off-chain Events 的限制

### 3.1 Sui 事件机制

Sui 的事件系统特点：
- 事件在交易执行时通过 `emit()` 发射
- 事件包含在 Checkpoint 中，具有最终一致性
- RPC 提供 `sui_subscribeEvent` 进行实时订阅

### 3.2 无法实现纯 Off-chain 事件的原因

| 限制 | 说明 |
|------|------|
| 无应用层钩子 | Sui 不提供类似 dYdX 的 `StreamOrderbookUpdates` 接口 |
| 事件必须上链 | 所有事件数据都需要包含在交易中 |
| 共识延迟 | 事件可见性受共识确认时间限制 |

### 3.3 替代方案

**推荐方案：链上事件 + RPC 实时监听**

```
sui-execution (匹配引擎)
        │
        │ emit(*EventV1)
        ▼
   Sui Transaction
        │
        ├──────────────────────────────────────┐
        │                                      │
        ▼                                      ▼
  Checkpoint                          sui_subscribeEvent
  (最终一致)                            (WebSocket 订阅)
        │                                      │
        ▼                                      ▼
  dex-indexer                           dex-realtime
  (~2-3s 延迟)                          (<500ms 延迟)
```

---

## 4. 推荐方案总结

### 4.1 技术选型

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 订阅 API | sui_subscribeEvent | 细粒度事件过滤，数据量小 |
| 消息队列 | Redis Stream | 轻量，与缓存层复用 |
| 节点连接 | 直接连接 Sui Full Node RPC | 无需专门索引节点 |
| 订单簿维护 | dex-realtime 内存 + Redis 缓存 | 启动从 PostgreSQL 恢复 |
| K 线聚合 | dex-realtime 实时计算 | 基于 FillEventV1 |

### 4.2 架构图

```
                    Sui Full Node
                         │
         ┌───────────────┴───────────────┐
         │                               │
    Checkpoint API              sui_subscribeEvent
    (定期轮询)                    (WebSocket 订阅)
         │                               │
         ▼                               ▼
    dex-indexer                    dex-realtime
    (最终一致 ~2-3s)               (低延迟 <500ms)
         │                               │
         │                               ├─→ 订单簿内存维护
         │                               ├─→ K 线聚合
         │                               └─→ 市场统计
         │                               │
         ▼                               ▼
    PostgreSQL                     Redis Stream
         │                          + Redis Cache
         │                               │
         └───────────┬───────────────────┘
                     ▼
               dex-api (HTTP)
               dex-ws (WebSocket)
```

### 4.3 实施优先级

| 优先级 | 功能 | 依赖 |
|--------|------|------|
| P0 | Sui RPC 订阅 + Redis Stream 发布 | - |
| P0 | 订单簿内存维护 | OrderPlaced/RemovedEventV1 |
| P1 | K 线聚合 | FillEventV1 |
| P1 | dex-ws 基础服务 | Redis Stream |
| P2 | 市场统计 | 多事件聚合 |
| P2 | dex-api 扩展（订单簿、K 线查询） | Redis 缓存 |

---

## 5. 后续文档

| 文档 | 内容 |
|------|------|
| 02-sui-rpc-subscription-guide.md | Sui RPC 订阅技术指南 |
| 03-dydx-streaming-reference.md | dYdX 实时通道参考分析 |
| 04-design-decisions.md | 设计决策记录 |
| 05-event-definitions.md | 事件定义规范 |
| 06-implementation-checklist.md | 实施清单 |
