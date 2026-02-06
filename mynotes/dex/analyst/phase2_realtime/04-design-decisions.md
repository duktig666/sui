# Phase 2 设计决策记录

## 概述

本文档记录 Phase 2 设计过程中的所有技术决策，包括选型理由、与 dYdX 设计的差异分析，以及简化方案说明。

> **重要更新（2026-02-05）**：基于时序验证结论，Phase 2 架构已从双通道（dex-realtime + dex-ws）简化为单通道（dex-indexer + dex-ws）。详见第 10 节「单通道架构决策」。

---

## 10. 单通道架构决策（2026-02-05 新增）

### 10.1 Checkpoint 主通道决策

| 决策项 | **选择：Checkpoint 主通道（移除 dex-realtime）** |
|--------|------------------------------------------------|
| 备选方案 | 双通道（Checkpoint + RPC） |
| 决策时间 | 2026-02-05 |

**背景**：

原 V5 方案计划使用双通道架构：
- Checkpoint 通道：dex-indexer → PostgreSQL（延迟 3-5s）
- RPC 通道：dex-realtime → Redis（延迟 400-650ms）

**时序验证结论**：

通过源码插桩测试 8030 笔交易，发现：
1. `sui start` 命令启动的是 Fullnode + Validator 双节点
2. Fullnode 需要先同步 Validator 的共识结果才能执行交易
3. RPC 订阅的事件已经是共识确认后的数据（无法绕过 Checkpoint）
4. 100% 的事件在 Checkpoint 生成后才到达（平均延迟 395.6ms）

| 部署方式 | Checkpoint 延迟 | RPC 订阅延迟 |
|----------|-----------------|--------------|
| **Validator 同机器** | **200-500ms** | 400-600ms |
| 标准 Full Node | 3-5s | 2-4s |

**决策结论**：

| 维度 | V5 方案（双通道） | V6 方案（单通道） |
|------|------------------|------------------|
| 数据通道 | Checkpoint + RPC | **仅 Checkpoint** |
| 模块数量 | 4 个 | **3 个** |
| dex-realtime | 需要 | **不需要** |
| Redis 写入 | dex-realtime | **dex-indexer** |
| 实际延迟 | 400-650ms | **200-500ms** |

**架构简化收益**：

1. **减少服务组件**：移除 dex-realtime 模块
2. **消除一致性问题**：单一数据源，无需双通道合并
3. **简化运维**：更少组件，更少故障点
4. **实际延迟更低**：Validator 同机器部署时，Checkpoint 通道更快

**结论**：采用 Checkpoint 主通道设计，移除 dex-realtime 模块。

> 详见 `13-event-timing-verification.md`

---

### 10.2 dex-indexer Redis 发布功能

| 决策项 | **选择：在 dex-indexer 中实现 Redis 写入** |
|--------|-------------------------------------------|
| 备选方案 | 保留 dex-realtime 负责 Redis 写入 |
| 决策时间 | 2026-02-05 |

**理由**：

既然 Checkpoint 通道是主通道，那么 Redis 写入逻辑应该在 dex-indexer 中实现：

1. **职责一致**：dex-indexer 处理事件后同时写入 PostgreSQL 和 Redis
2. **无需额外服务**：不需要 dex-realtime 作为中间层
3. **数据一致性**：单一写入点，确保数据一致

**实现方式**：

```
dex-indexer
├── handlers/
│   ├── fills.rs      → 写入 PostgreSQL + Redis
│   ├── positions.rs  → 写入 PostgreSQL + Redis
│   └── orders.rs     → 写入 PostgreSQL + Redis
└── redis_publisher.rs  # [新增] Redis 写入模块
```

**结论**：在 dex-indexer 中新增 `redis_publisher.rs` 模块，处理 Redis 写入。

---

### 10.3 dex-ws 数据源变更

| 决策项 | **dex-ws 从 dex-indexer 写入的 Redis 消费** |
|--------|---------------------------------------------|
| 原方案 | dex-ws 从 dex-realtime 写入的 Redis 消费 |
| 决策时间 | 2026-02-05 |

**变更说明**：

| 维度 | 原方案 | 新方案 |
|------|--------|--------|
| Redis 写入者 | dex-realtime | **dex-indexer** |
| dex-ws 消费 | dex-realtime 的 Stream | **dex-indexer 的 Stream** |
| 数据内容 | 完全相同 | 完全相同 |

**对 dex-ws 的影响**：

- **无代码修改**：dex-ws 仍然消费相同的 Redis Stream
- **延迟变化**：从 400-650ms 降低到 200-500ms（Validator 同机器部署）

---

### 10.4 移除的组件和任务

以下组件和任务在单通道架构中已移除：

| 组件/任务 | 说明 |
|-----------|------|
| **dex-realtime crate** | 不再需要独立的 RPC 订阅服务 |
| listener.rs | RPC 事件监听（移除） |
| recovery.rs | 启动恢复逻辑（移除） |
| 双通道合并逻辑 | 无需合并，单一数据源 |

---

### 10.5 保留的设计决策

以下设计决策在单通道架构中保持不变：

| 决策 | 说明 |
|------|------|
| Redis Stream 作为消息队列 | dex-ws 消费 Redis Stream |
| Redis Hash 存储状态数据 | 订单簿、市场统计、K 线 |
| 幂等发布机制 | 使用 tx_digest + event_seq 去重 |
| 事件命名规范 | *EventV1 格式 |
| WebSocket 频道设计 | trades, l2Book, candle 等 |

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

| 决策项 | **选择：Validator + 同机器索引节点** |
|--------|-------------------------------------|
| 备选方案 | 直接暴露 Validator RPC、Full Node 订阅、P2P 索引节点 |
| 决策时间 | 2026-02-05（更新） |

**背景**：原方案使用 Full Node 标准订阅，实测延迟 2-4s，与 Checkpoint 通道相近，失去 realtime 价值。

**选型理由**：

| 方案 | 延迟 | 安全性 | 复杂度 | 推荐 |
|------|------|--------|--------|------|
| Full Node 标准订阅 | 2-4s | 高 | 低 | MVP |
| 直接暴露 Validator RPC | 400-600ms | 🔴 低 | 低 | ❌ 不推荐 |
| **Validator + 索引节点** | **400-650ms** | **🟢 高** | **中** | **✅ 推荐** |
| P2P 索引节点 | 400-600ms | 高 | 高 | 无 Validator 时备选 |

**分阶段策略**：
1. **MVP/开发**：Full Node 订阅（延迟 2-4s，快速验证）
2. **生产环境**：Validator + 同机器索引节点（延迟 400-650ms）

**结论**：生产环境采用 Validator + 同机器索引节点架构，兼顾低延迟和安全性。

> 详细分析见 `08-realtime-latency-analysis.md` 和 `09-validator-security-analysis.md`

---

### 1.4 同机器同步方案

| 决策项 | **选择：内网 RPC 订阅** |
|--------|-------------------------|
| 备选方案 | 共享存储、Unix Socket |
| 决策时间 | 2026-02-05 |

**方案对比**：

| 方案 | 额外延迟 | 复杂度 | 优点 | 缺点 |
|------|----------|--------|------|------|
| **共享存储** | ~几毫秒 | 中 | 延迟最低 | 需配置锁，可能有竞争 |
| **内网 RPC 订阅** | ~10-50ms | 低 | 实现简单，标准 API | 略有延迟增加 |
| **Unix Socket** | ~几毫秒 | 高 | 延迟最低，无网络栈 | 需修改 Sui 源码 |

**选型理由**：

1. **复杂度权衡**：内网 RPC 订阅使用标准 Sui SDK，无需修改 Sui 源码或自定义存储层
2. **延迟可接受**：10-50ms 额外延迟在总延迟 400-650ms 中占比较小（<10%）
3. **运维简单**：索引节点配置 `--json-rpc-address 127.0.0.1:9000`，dex-realtime 连接本地地址
4. **安全隔离**：Validator 不对外暴露任何端口，RPC 仅在同机器内网可用

**部署架构**：

```
┌──────────────────────────────────────────────────────────┐
│  同一物理机/虚拟机                                        │
├──────────────────────────────────────────────────────────┤
│                                                          │
│   Validator Process          Indexer Process             │
│   ├─ 共识参与               ├─ --json-rpc-address        │
│   ├─ 交易执行               │   127.0.0.1:9000           │
│   └─ 不暴露 RPC             └─ sui_subscribeEvent        │
│                                     │                    │
│                                     ↓                    │
│                            dex-realtime Process          │
│                            └─ ws://127.0.0.1:9000        │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**结论**：采用内网 RPC 订阅方案，平衡实现复杂度和延迟需求。

> 详细分析见 `08-realtime-latency-analysis.md` 和 `09-validator-security-analysis.md`

---

### 1.5 订单簿推送方案（已更新 2026-02-05）

| 决策项 | **选择：链上内存订单簿快照推送（方案 A - 复用 Sui Event）** |
|--------|-----------------------------------------------------------|
| 备选方案 | 链下构建订单簿、自定义 Streamer |
| 决策时间 | 2026-02-05（更新） |

**背景**：原方案（链下构建）存在以下问题：
- Maker 订单部分成交时缺少更新事件
- 链下状态可能与链上不一致
- 启动恢复需要从 PostgreSQL 加载历史订单

**选型理由**：

| 方案 | 延迟 | 一致性 | 链下复杂度 | 启动恢复 |
|------|------|--------|------------|----------|
| ❌ 链下构建 | 低 | 可能不一致 | 高 | 复杂 |
| ✅ **链上快照推送** | ~125ms（平均） | **绝对一致** | **极简** | **简单** |
| ❌ 自定义 Streamer | 与 Sui Event 相同 | 高 | 低 | 简单 |

**为什么选择方案 A（Sui Event）而非自定义 Streamer**：

1. **延迟相同**：自定义 Streamer 无法降低延迟（瓶颈在共识和 Checkpoint）
2. **开发成本**：Sui Event 零成本，自定义 Streamer 需 1-2 周
3. **兼容性**：Sui Event 兼容所有节点，自定义 Streamer 需自建
4. **一致性**：两者都有链上保证

**设计要点**：
1. 链上 PerpetualState 维护内存订单簿（InlineOrderbook）
2. 每 250ms 发射 OrderbookSnapshotEvent（交易触发时检查）
3. dex-realtime 直接使用快照，无需构建维护
4. 启动恢复：等待下一个快照即可

**实现参数**：
- 推送频率：250ms（与 Hyperliquid 对标）
- 快照深度：100 档
- 触发方式：交易触发（非定时器）

**结论**：采用链上快照推送方案，链下逻辑极简，状态绝对一致。

> 详细分析见 `12-orderbook-push-strategy-analysis.md`

---

### 1.6 OrderUpdateEvent 处理

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

## 8. 多实例一致性决策

### 8.1 多 realtime 实例去重方案

| 决策项 | **选择：幂等发布 (SET NX + XADD)** |
|--------|-----------------------------------|
| 备选方案 | Leader Election、分区负责 |
| 决策时间 | 2026-02-05 |

**问题背景**：

生产环境部署多个 dex-realtime 实例时，同一事件可能被多个实例处理并发布到 Redis Stream，导致消息重复。

**方案对比**：

| 方案 | 复杂度 | 可靠性 | 延迟影响 | 推荐 |
|------|--------|--------|----------|------|
| **幂等发布** | 低 | 高 | 无 | ✅ 推荐 |
| Leader Election | 中 | 高 | 切换时有延迟 | 备选 |
| 分区负责 | 中 | 中 | 无 | 不推荐 |

**幂等发布实现**：

使用 Sui 事件的唯一标识（tx_digest + event_seq）作为去重键：

```rust
// 生成去重键
let sui_event_id = format!("{}-{}", tx_digest, event_seq);
let dedup_key = format!("dex:event:seen:{}", sui_event_id);

// SETNX 检查是否已处理
let is_new: bool = redis::cmd("SET")
    .arg(&dedup_key)
    .arg("1")
    .arg("NX")           // 仅在键不存在时设置
    .arg("EX")
    .arg(3600)           // 1 小时过期
    .query_async(&mut conn)
    .await?;

// 仅新事件发布到 Stream
if is_new {
    redis::cmd("XADD")
        .arg(&stream_key)
        .arg("MAXLEN")
        .arg("~")
        .arg(10000)
        .arg("*")
        .arg("event_id").arg(&sui_event_id)
        .arg("data").arg(&json_data)
        .query_async(&mut conn)
        .await?;
}
```

**优点**：
1. 无需协调：各实例独立运行，无选举或心跳
2. 无单点故障：任何实例故障不影响其他实例
3. 延迟稳定：无 Leader 切换抖动
4. 实现简单：利用 Redis 原子操作

**TTL 设计**：
- 去重键 TTL = 1 小时
- 足够覆盖网络延迟和重试窗口
- 不会无限增长

**结论**：采用幂等发布方案，利用 Redis SET NX 原子操作实现去重。

---

### 8.2 事件丢失补齐方案

| 决策项 | **选择：dex-indexer 通过 Checkpoint 补齐** |
|--------|-------------------------------------------|
| 备选方案 | dex-realtime 自行补齐、双通道同时写入 |
| 决策时间 | 2026-02-05 |

**问题背景**：

dex-realtime 可能因以下原因丢失事件：
- WebSocket 连接断开
- 订阅缓冲区溢出
- 服务重启

**方案对比**：

| 方案 | 复杂度 | 数据一致性 | 说明 |
|------|--------|------------|------|
| **indexer 补齐** | 低 | 最终一致 | 利用已有 Checkpoint 通道 |
| realtime 自补 | 高 | 强一致 | 需维护游标和补齐逻辑 |
| 双通道写入 | 中 | 最终一致 | 增加复杂度和资源 |

**补齐流程**：

```
1. dex-realtime 断线
       ↓
2. 重连后从最新事件开始（不回溯）
       ↓
3. 中间丢失的事件由 Checkpoint 通道补齐
       ↓
4. dex-indexer 处理 Checkpoint 时写入 Redis Stream
       ↓
5. Redis Stream 最终包含完整事件序列
```

**实现要点**：

1. **dex-indexer 责任扩展**：
   - 原有：写入 PostgreSQL
   - 新增：同时写入 Redis Stream（使用幂等发布）

2. **事件顺序**：
   - realtime 通道：先到达（低延迟）
   - checkpoint 通道：后到达（补齐）
   - 幂等发布保证不重复

3. **消费端处理**：
   - dex-ws 消费 Redis Stream 时无需关心来源
   - 同一事件只出现一次（幂等发布保证）

**优点**：
1. 复用已有架构：不需要新建补齐服务
2. 数据一致性：Checkpoint 包含所有已确认事件
3. 简化 realtime：无需维护历史游标
4. 最终一致：短暂丢失不影响最终状态

**延迟影响**：
- 正常情况：realtime 通道 400-650ms
- 断线期间：checkpoint 通道 3-5s
- 可接受：断线是异常情况，恢复后自动补齐

**结论**：利用 dex-indexer 的 Checkpoint 处理能力补齐丢失事件，简化架构复杂度。

> 详细分析见 `07-multi-node-consistency.md`

---

### 8.3 DEX Engine 乐观事件

| 决策项 | **选择：暂不实现** |
|--------|-------------------|
| 备选方案 | 实现乐观事件推送 |
| 决策时间 | 2026-02-05 |

**问题背景**：

dYdX 采用「乐观事件」模式：订单提交后立即推送预期状态，无需等待链上确认。这可实现 <100ms 延迟。

**分析**：

| 方面 | dYdX | 本项目 |
|------|------|--------|
| 乐观延迟 | 10-50ms | 需定制 DEX Engine |
| 确认延迟 | 1-2s | 400-650ms |
| 差距 | 较大，值得优化 | 较小，优化收益有限 |

**暂不实现的理由**：

1. **延迟差距小**：
   - 当前确认延迟 400-650ms
   - 乐观模式收益 ~300-500ms
   - 对交易体验提升有限

2. **实现复杂度高**：
   - 需要修改 DEX Engine
   - 需要处理乐观状态与链上状态不一致
   - 增加前端复杂度（显示乐观 vs 确认状态）

3. **一致性风险**：
   - 乐观事件可能被回滚
   - 需要设计状态回滚通知机制

**未来条件**：

当以下条件满足时可考虑实现：
1. 用户反馈延迟影响交易决策
2. DEX Engine 架构支持事件钩子
3. 有足够资源投入前端状态管理

**结论**：当前 450ms 延迟可接受，DEX Engine 乐观事件暂不实现。

---

## 9. 待决策事项

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
