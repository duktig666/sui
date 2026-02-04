# Phase 2 设计决策记录

## 概述

本文档记录 Phase 2（dex-realtime + dex-ws）设计过程中的所有技术决策，包括选型理由、与 dYdX 设计的差异分析，以及简化方案说明。

---

## 1. 技术选型决策

### 1.1 订阅 API 选择

| 决策项 | **选择：sui_subscribeEvent** |
|--------|------------------------------|
| 备选方案 | sui_subscribeTransaction |
| 决策时间 | 2026-02-04 |

**选型理由**：

| 对比项 | sui_subscribeEvent | sui_subscribeTransaction |
|--------|-------------------|-------------------------|
| 数据粒度 | 单个事件 | 完整交易 |
| 过滤能力 | MoveEventType 精确过滤 | 交易级别过滤 |
| 网络开销 | 小 | 大 |
| 解析复杂度 | 低 | 高 |

**结论**：sui_subscribeEvent 提供更细粒度的事件过滤，数据量小，处理简单，适合实时事件监听场景。

---

### 1.2 消息队列选择

| 决策项 | **选择：Redis Stream** |
|--------|------------------------|
| 备选方案 | Kafka、RabbitMQ、Redis Pub/Sub |
| 决策时间 | 2026-02-04 |

**选型理由**：

| 对比项 | Redis Stream | Kafka | RabbitMQ | Redis Pub/Sub |
|--------|-------------|-------|----------|---------------|
| 运维复杂度 | 低 | 高 | 中 | 低 |
| 消息持久化 | ✓ | ✓ | ✓ | ✗ |
| 消费者组 | ✓ | ✓ | ✓ | ✗ |
| 与缓存复用 | ✓ | ✗ | ✗ | ✓ |
| 吞吐量 | 中高 | 极高 | 中 | 高 |

**结论**：Redis Stream 轻量、支持持久化和消费者组、可与缓存层复用同一 Redis 实例，运维成本低，适合中等规模场景。

---

### 1.3 节点连接策略

| 决策项 | **选择：直接连接 Sui Full Node RPC** |
|--------|-------------------------------------|
| 备选方案 | 专用索引节点、多节点集群 |
| 决策时间 | 2026-02-04 |

**选型理由**：

| 方案 | 优点 | 缺点 |
|------|------|------|
| 直接连接标准 RPC | 简单、无额外运维 | 单点风险 |
| 专用索引节点 | 隔离生产流量 | 额外运维成本 |
| 多节点集群 | 高可用 | 复杂度高 |

**分阶段策略**：
1. **开发/测试**：公共 RPC（wss://sui-testnet.mystenlabs.com）
2. **生产初期**：自有 Full Node
3. **生产成熟**：多节点冗余 + 故障转移

**结论**：初期使用单节点连接降低复杂度，架构设计预留多节点扩展能力。

---

### 1.4 订单簿维护方案

| 决策项 | **选择：dex-realtime 内存维护 + Redis 缓存** |
|--------|---------------------------------------------|
| 备选方案 | 纯 Redis 维护、PostgreSQL 实时查询 |
| 决策时间 | 2026-02-04 |

**选型理由**：

| 方案 | 性能 | 一致性 | 复杂度 |
|------|------|--------|--------|
| 内存 + Redis 缓存 | 高 | 需双写 | 中 |
| 纯 Redis | 中 | 单一数据源 | 低 |
| PostgreSQL 实时查询 | 低 | 高 | 低 |

**设计要点**：
1. dex-realtime 启动时从 PostgreSQL 恢复订单簿状态
2. 监听 OrderPlaced/Removed 事件实时更新内存
3. 定期（~100ms）将订单簿快照写入 Redis
4. dex-ws 和 dex-api 从 Redis 读取

**结论**：内存维护提供最佳性能，Redis 缓存支持多实例读取，PostgreSQL 提供启动恢复能力。

---

### 1.5 OrderUpdateEvent 处理

| 决策项 | **选择：延后实现** |
|--------|-------------------|
| 备选方案 | 立即实现 |
| 决策时间 | 2026-02-04 |

**理由**：当前 DEX 不支持订单修改功能，OrderUpdateEvent 无实际触发场景。待订单修改功能开发时再补充该事件。

---

## 2. 事件命名决策

### 2.1 统一命名规范

| 决策项 | **选择：所有事件使用 `*EventV1` 格式** |
|--------|---------------------------------------|
| 备选方案 | 区分 On-chain/Off-chain 命名 |
| 决策时间 | 2026-02-04 |

**命名规则**：

| 规则 | 示例 |
|------|------|
| `{业务名}EventV1` | FillEventV1, OrderPlacedEventV1 |

**统一命名理由**：
1. 事件在同一位置发射（sui-execution/src/dex.rs）
2. dex-indexer 和 dex-realtime 处理相同事件结构
3. 区别仅在数据来源（Checkpoint vs RPC 订阅）
4. 简化维护，避免重复定义

**现有事件重命名**：

| 现有名称 | 新名称 |
|----------|--------|
| FillEvent | FillEventV1 |
| PositionUpdateEvent | PositionUpdateEventV1 |
| BalanceUpdateEvent | BalanceUpdateEventV1 |
| TransferEvent | TransferEventV1 |
| LiquidationEvent | LiquidationEventV1 |
| FundingSettlementEvent | FundingSettlementEventV1 |
| PerpetualCreatedEvent | PerpetualCreatedEventV1 |

---

## 3. 与 dYdX 设计差异

### 3.1 架构差异对照

| 方面 | dYdX v4 | 本项目 | 差异原因 |
|------|---------|--------|----------|
| 事件来源 | MemClob 内存状态 | 链上事件 | Sui 无应用层钩子 |
| 实时通道 | gRPC Stream | sui_subscribeEvent | 使用标准 RPC |
| 索引节点 | 专用 Full Node Stream | 标准 Sui Full Node | 简化运维 |
| 订单簿恢复 | 内存重建 | PostgreSQL 加载 | 无内存状态持久化 |

### 3.2 无需专用索引节点

**dYdX 需要专用索引节点的原因**：
- FullNodeStreamingManager 在应用层实现
- 需要订阅节点内部的 MemClob 变更
- 与生产交易流量隔离

**本项目不需要的原因**：
- 使用 Sui 标准 RPC 接口（sui_subscribeEvent）
- 任何 Full Node 都提供相同的事件订阅能力
- 初期使用公共 RPC 即可满足需求

### 3.3 统一事件结构

**dYdX 的双事件设计**：
```
On-chain Events (Indexer)
├─ OrderFillEventV1
├─ OrderPlaceEventV1
└─ ...

Off-chain Updates (Streaming)
├─ StreamOrderbookUpdate
├─ StreamFill
└─ ...
```

**本项目的统一设计**：
```
统一事件定义 (*EventV1)
├─ FillEventV1           → dex-indexer + dex-realtime
├─ OrderPlacedEventV1    → dex-indexer + dex-realtime
├─ OrderRemovedEventV1   → dex-indexer + dex-realtime
└─ ...
```

**统一设计的优点**：
1. 减少代码重复
2. 简化测试
3. 降低维护成本
4. 避免事件定义分歧

---

## 4. 双通道事件分配决策

### 4.1 事件分配矩阵

| 事件 | dex-realtime | dex-indexer | 说明 |
|------|:------------:|:-----------:|------|
| FillEventV1 | ✓ | ✓ | 成交需实时推送 + 持久化 |
| OrderPlacedEventV1 | ✓ | ✓ | 订单簿更新需实时推送 |
| OrderRemovedEventV1 | ✓ | ✓ | 订单状态需实时推送 |
| PositionUpdateEventV1 | ✓ | ✓ | 持仓变化需实时推送 |
| LiquidationEventV1 | ✓ | ✓ | 清算需实时通知 |
| BalanceUpdateEventV1 | - | ✓ | 仅需持久化，无实时需求 |
| TransferEventV1 | - | ✓ | 仅需持久化 |
| FundingSettlementEventV1 | - | ✓ | 仅需持久化 |

### 4.2 分配理由

**实时通道（dex-realtime）关注**：
- 影响交易决策的事件（成交、订单簿变化）
- 影响风控的事件（持仓、清算）
- 用户需立即知晓的事件

**持久化通道（dex-indexer）关注**：
- 所有需要历史查询的事件
- 财务相关事件（余额、转账、资金费）
- 审计和对账需要的事件

---

## 5. Redis 存储结构决策

### 5.1 存储类型选择

| 数据类型 | Redis 类型 | 理由 |
|----------|-----------|------|
| 事件流 | Stream | 支持消费者组和持久化 |
| 订单簿快照 | Hash | 结构化存储，部分更新 |
| 市场统计 | Hash | 多字段聚合数据 |
| K 线历史 | Sorted Set | 按时间排序，范围查询 |
| 最近成交 | Sorted Set | 按时间排序，保留固定数量 |

### 5.2 键命名规范

```
前缀规则: dex:{类型}:{标识}[:子类型]

示例:
dex:stream:fills                    # 成交事件流
dex:orderbook:{perpetual_id}        # 订单簿快照
dex:market:{perpetual_id}           # 市场统计
dex:candles:{perpetual_id}:{interval} # K线数据
dex:trades:{perpetual_id}           # 最近成交
```

### 5.3 TTL 策略

| 数据类型 | TTL | 理由 |
|----------|-----|------|
| Stream 消息 | 1h | 断线重连窗口 |
| 订单簿快照 | 无 | 持续更新 |
| 市场统计 | 无 | 持续更新 |
| K 线数据 | 7d | 热数据缓存 |
| 最近成交 | 1d | 保留最近数据 |

---

## 6. 快照与增量机制决策

### 6.1 订单簿推送策略

| 场景 | 推送方式 | 数据来源 |
|------|----------|----------|
| 新连接 | 全量快照 | Redis orderbook |
| 正常推送 | 增量更新 | Redis Stream |

### 6.2 快照频率

| 数据 | 快照频率 | 理由 |
|------|----------|------|
| 订单簿 | ~100ms | 平衡实时性和性能 |
| 市场统计 | ~1s | 聚合计算需要时间 |
| K 线 | 实时 | 每次成交立即更新 |

### 6.3 增量推送格式

```json
{
  "type": "orderbook_update",
  "perpetual_id": 1,
  "updates": [
    {"op": "add", "side": "bid", "price": "97000", "qty": "1.5"},
    {"op": "remove", "side": "ask", "price": "97100", "order_id": "xxx"}
  ],
  "timestamp": 1707000000000
}
```

---

## 7. 性能与可靠性决策

### 7.1 批处理配置

| 参数 | 值 | 理由 |
|------|-----|------|
| 批处理间隔 | 10ms | 参考 dYdX，平衡延迟和吞吐 |
| 批大小上限 | 100 | 防止单批过大 |
| 缓冲区大小 | 1000 | 应对突发流量 |

### 7.2 重连策略

| 参数 | 值 | 理由 |
|------|-----|------|
| 初始延迟 | 1s | 快速首次重试 |
| 最大延迟 | 30s | 避免过长等待 |
| 退避乘数 | 2 | 标准指数退避 |

### 7.3 心跳配置

| 参数 | 值 | 理由 |
|------|-----|------|
| 心跳间隔 | 1s | 及时检测断连 |
| 超时时间 | 5s | 允许网络抖动 |

---

## 8. 待决策事项

### 8.1 K 线聚合位置

**选项**：
1. 在 dex-realtime 中聚合
2. 独立 dex-candle 服务

**倾向**：选项 1（在 dex-realtime 中聚合），理由：
- 减少服务数量
- 共享事件流
- 简化部署

**状态**：待用户确认

### 8.2 市场统计计算

**待确定项**：
- 中间价计算方式（最优买卖价平均 vs 加权）
- 标记价格来源（订单簿计算 vs 预言机）
- 资金费率计算周期

**状态**：待匹配引擎设计确定

### 8.3 多 perpetual 支持

**当前假设**：单一 perpetual（BTC-USDC）

**待确定**：
- 多 perpetual 时的事件路由策略
- Redis 键分片策略
- 订阅过滤优化

**状态**：待产品确认
