# Phase 2 实时通道分析总结

## 概述

本文档总结 Phase 2（实时数据推送）技术方案的分析结论，包括与 dYdX 的差异分析、Sui 架构限制以及推荐的实现方案。

> **重要更新（2026-02-05）**：基于时序验证结论，Phase 2 采用 **单通道架构**（Checkpoint 主通道），移除原计划的 dex-realtime 模块。详见 §6 架构变更说明。

---

## 1. Phase 2 缺失项总结

### 1.1 当前状态

Phase 1 已完成 dex-indexer 和 dex-api 的基础功能：
- dex-indexer：通过 Checkpoint API 持久化 FillEventV1、PositionUpdateEventV1 等事件
- dex-api：提供 Hyperliquid 兼容的 HTTP 查询接口

### 1.2 Phase 2 缺失的核心组件

| 组件 | 状态 | 说明 |
|------|------|------|
| ~~dex-realtime~~ | ~~缺失~~ | ~~实时事件监听和发布服务~~ **已移除，功能合并到 dex-indexer** |
| dex-indexer Redis 发布 | 缺失 | Checkpoint 事件处理后写入 Redis |
| dex-ws | 缺失 | WebSocket 推送服务 |
| 订单簿维护 | 缺失 | ~~内存订单簿构建~~ → 直接使用 OrderbookSnapshotEvent |
| K 线聚合 | 缺失 | 多周期 K 线实时计算（集成到 dex-indexer） |
| 市场统计 | 缺失 | 24h 成交量、资金费率等统计数据（集成到 dex-indexer） |

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

**本项目设计**（更新后 - 单通道架构）：
```
┌─────────────────────────────────────────────────────┐
│  DEX 事件分发（单通道：Checkpoint 主通道）            │
├─────────────────────────────────────────────────────┤
│  dex-indexer (Checkpoint API)                       │
│  - 轮询 Checkpoint 获取事件                          │
│  - 部署在 Validator 同机器：延迟 ~200-500ms          │
│  - 同时写入 PostgreSQL + Redis                      │
│  - 用于持久化 + 实时推送                             │
├─────────────────────────────────────────────────────┤
│  dex-ws (消费 Redis Stream)                         │
│  - 从 Redis 读取事件                                │
│  - 推送到 WebSocket 客户端                          │
│  - 端到端延迟 ~200-300ms                            │
└─────────────────────────────────────────────────────┘
```

**原设计**（已废弃 - 双通道架构）：
```
┌─────────────────────────────────────────────────────┐
│  DEX 事件分发（统一事件，双通道消费）                   │
├─────────────────────────────────────────────────────┤
│  dex-indexer (Checkpoint API)                       │
│  - 轮询 Checkpoint 获取事件                          │
│  - 最终一致性，延迟 ~2-3s                            │
│  - 用于持久化和历史查询                               │
├─────────────────────────────────────────────────────┤
│  ~~dex-realtime (sui_subscribeEvent)~~              │
│  ~~- WebSocket 订阅链上事件~~                        │
│  ~~- 低延迟 <500ms~~                                │
│  ~~- 用于实时推送~~                                  │
└─────────────────────────────────────────────────────┘
```

### 2.3 关键差异说明

1. **无 Off-chain Updates**：Sui 不支持类似 dYdX 的应用层事件分发，所有事件必须通过链上发射
2. **统一事件定义**：同一事件结构服务持久化和实时推送
3. **单通道架构**：Checkpoint 通道同时服务持久化和实时推送（部署在 Validator 同机器时延迟可控）
4. **无需专用索引节点**：使用标准 Sui Checkpoint 接口

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

**最终方案：Checkpoint 主通道（单通道架构）**

> **更新（2026-02-05）**：基于时序验证，发现 Checkpoint 通道在 Validator 同机器部署时延迟更低。

```
sui-execution (匹配引擎)
        │
        │ emit(*EventV1)
        ▼
   Sui Transaction
        │
        ▼
   Checkpoint Store
   (Validator 本地)
        │
        ▼
   dex-indexer
   (同机器部署，延迟 ~200-500ms)
        │
        ├──► PostgreSQL (持久化)
        │
        └──► Redis (实时推送)
                │
                ▼
            dex-ws
            (WebSocket 推送)
```

**原方案**（已废弃）：
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
  dex-indexer                           ~~dex-realtime~~
  (~2-3s 延迟)                          ~~(<500ms 延迟)~~
```

---

## 4. 推荐方案总结

### 4.1 技术选型（更新后）

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 数据通道 | Checkpoint 主通道 | Validator 同机器部署时延迟更低（~200-500ms） |
| 消息队列 | Redis Stream | 轻量，与缓存层复用 |
| 订单簿数据 | OrderbookSnapshotEvent | 链上快照，无需内存维护 |
| K 线聚合 | dex-indexer 实时计算 | 基于 FillEventV1 |
| Redis 写入 | dex-indexer 双写 | 同时写入 PostgreSQL + Redis |

### 4.2 架构图（更新后 - 单通道架构）

```
                    Sui Validator
                         │
                         ▼
                  Checkpoint Store
                    (本地存储)
                         │
                         ▼
                    dex-indexer
              (Validator 同机器部署)
              (延迟 ~200-500ms)
                         │
         ┌───────────────┴───────────────┐
         │                               │
         ▼                               ▼
    PostgreSQL                      Redis
    (持久化存储)                  (Stream + Hash)
         │                               │
         │                               │
         ▼                               ▼
    dex-api (HTTP)               dex-ws (WebSocket)
    (历史查询)                    (实时推送)
```

### 4.3 实施优先级（更新后）

| 优先级 | 功能 | 依赖 |
|--------|------|------|
| P0 | dex-indexer Redis 发布功能 | - |
| P0 | OrderbookSnapshotEvent 处理 | 链上快照发射 |
| P1 | K 线聚合（集成到 dex-indexer） | FillEventV1 |
| P1 | dex-ws 基础服务 | Redis Stream |
| P2 | 市场统计（集成到 dex-indexer） | 多事件聚合 |
| P2 | dex-api 扩展（订单簿、K 线查询） | Redis 缓存 |

---

## 5. 后续文档

| 文档 | 内容 |
|------|------|
| 02-sui-rpc-subscription-guide.md | Sui RPC 订阅技术指南（参考，非主要通道） |
| 03-dydx-streaming-reference.md | dYdX 实时通道参考分析 |
| 04-design-decisions.md | 设计决策记录（含单通道架构决策） |
| 05-event-definitions.md | 事件定义规范 |
| 06-implementation-checklist.md | 实施清单（已更新为单通道架构） |
| 10-redis-message-spec.md | Redis 消息格式规范 |
| 11-websocket-protocol-spec.md | WebSocket 协议规范 |
| 13-event-timing-verification.md | 事件时序验证报告 |

---

## 6. 架构变更说明（2026-02-05）

### 6.1 变更原因

基于事件时序验证（详见 `13-event-timing-verification.md`）：

| 通道 | 预期延迟 | 实测延迟 | 说明 |
|------|----------|----------|------|
| Checkpoint | ~2-3s（标准 Fullnode） | **~200-500ms**（Validator 同机器） | 部署位置关键 |
| RPC 订阅 | <500ms | ~400-600ms | 需要 Fullnode 同步 |

**结论**：Checkpoint 通道在正确部署时延迟更低、架构更简单。

### 6.2 架构变更

| 方面 | 原方案（双通道） | 新方案（单通道） |
|------|-----------------|-----------------|
| 模块数量 | 4 个（indexer, api, realtime, ws） | 3 个（indexer, api, ws） |
| 数据通道 | Checkpoint + RPC 订阅 | 仅 Checkpoint |
| Redis 写入 | dex-realtime | dex-indexer |
| 订单簿维护 | dex-realtime 内存构建 | 直接使用 OrderbookSnapshotEvent |
| 复杂度 | 需要双通道合并、一致性处理 | 简单直接 |

### 6.3 收益

1. **架构简化**：减少一个服务，降低运维复杂度
2. **一致性保证**：单数据源，无合并冲突
3. **延迟可控**：Validator 同机器部署可达 200-300ms 端到端
4. **恢复简单**：Checkpoint 本身支持断点续传
