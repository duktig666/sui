# DEX Indexer 技术方案 V5

> 版本: V5
> 日期: 2026-02-03
> 更新: 2026-02-05 (链上订单簿快照推送方案)
> 状态: 设计中
> 基于: V4 方案 + 模块分离架构

## 参考文档

Phase 2 详细分析文档位于 `sui/mynotes/dex/analyst/phase2_realtime/`：

| 文档 | 内容 |
|------|------|
| 01-phase2-analysis-summary.md | Phase 2 缺失项总结、与 dYdX 差异分析 |
| 02-sui-rpc-subscription-guide.md | sui_subscribeEvent API、代码示例、重连策略 |
| 03-dydx-streaming-reference.md | MemClob 结构、快照机制、Redis 缓存角色 |
| 04-design-decisions.md | 技术选型决策记录 |
| 05-event-definitions.md | 事件结构详解、发射点汇总 |
| 06-implementation-checklist.md | 文件修改清单、测试验证清单 |

---

## 1. 版本演进与设计决策

### 1.1 V4 → V5 核心变化

| 维度 | V4 方案 | V5 方案 | 变化原因 |
|------|---------|---------|----------|
| 模块结构 | 单一 dex-indexer crate，两个二进制 | 四个独立 crate | 职责分离，独立部署 |
| API 服务 | dex-indexer 内置 | 独立 dex-api crate | 避免资源竞争 |
| 实时通道 | 未规划 | dex-realtime + dex-ws | 双通道架构 |
| 类型共享 | 内部定义 | sui-types + 各 crate 内部 | 减少依赖层级 |

### 1.2 V5 架构核心理念

**"职责分离，独立扩展"**：每个服务单一职责，可独立部署和扩展

1. **dex-indexer**：Checkpoint 事件处理，写入 PostgreSQL
2. **dex-api**：REST API 查询服务，读取 PostgreSQL
3. **dex-realtime**：RPC 实时监听，发布 Redis Stream
4. **dex-ws**：WebSocket 推送服务，消费 Redis Stream

### 1.3 设计决策总结

| 决策 | 选择 | 理由 |
|------|------|------|
| 模块粒度 | 四个独立 crate | 职责清晰，故障隔离 |
| 共享类型 | 不创建 dex-types | 事件类型已在 sui-types，API 类型各自定义 |
| REST+WS | 拆分为两个 crate | 避免长连接与短连接资源竞争 |
| handlers+realtime | 拆分为两个 crate | 保持一致性，独立扩展 |
| schema 位置 | 保留在 dex-indexer | dex-api 依赖 dex-indexer 获取 schema |

---

## 2. 四模块架构

### 2.1 Phase 1 结构（2 个 crate）

```
dex-sui/crates/
├── dex-indexer/         # Checkpoint 索引服务
│   └── src/
│       ├── lib.rs
│       ├── main.rs      # dex-indexer 二进制
│       ├── schema/      # 数据库定义 + migrations
│       │   ├── mod.rs
│       │   └── stored_types.rs
│       └── handlers/    # Checkpoint 事件处理
│           ├── mod.rs
│           ├── fills.rs
│           ├── balances.rs
│           └── positions.rs
│
└── dex-api/             # REST API 服务
    └── src/
        ├── lib.rs
        ├── main.rs      # dex-api 二进制
        ├── types.rs     # API 请求/响应类型
        ├── server.rs    # Axum 服务器
        ├── handlers.rs  # 查询处理
        └── cache/       # [Phase 3] Redis 缓存
            └── mod.rs
```

### 2.2 Phase 2 结构（4 个 crate）

```
dex-sui/crates/
├── dex-indexer/         # (不变)
├── dex-api/             # REST API (不变)
│
├── dex-realtime/        # [新增] 实时事件采集
│   └── src/
│       ├── lib.rs
│       ├── main.rs      # dex-realtime 二进制
│       ├── listener.rs  # Sui RPC 事件订阅
│       └── publisher.rs # Redis Stream 发布
│
└── dex-ws/              # [新增] WebSocket 推送服务
    └── src/
        ├── lib.rs
        ├── main.rs      # dex-ws 二进制
        ├── types.rs     # WS 订阅/消息类型
        ├── server.rs    # WebSocket 服务器
        ├── channels.rs  # 订阅频道管理
        └── subscriber.rs # Redis 订阅消费
```

### 2.3 类型定义策略

| 类型 | 位置 | 说明 |
|------|------|------|
| 事件类型 | `sui-types/src/dex_events.rs` | 已存在，统一使用 *EventV1 命名 |
| API 类型 | `dex-api/src/types.rs` | InfoRequest, FillResponse 等 |
| WS 类型 | `dex-ws/src/types.rs` | Subscription, WsMessage 等 |

### 2.4 事件命名规范

**统一命名**：所有事件使用 `*EventV1` 格式

| 现有事件 | 新名称 |
|----------|--------|
| FillEvent | FillEventV1 |
| PositionUpdateEvent | PositionUpdateEventV1 |
| BalanceUpdateEvent | BalanceUpdateEventV1 |
| TransferEvent | TransferEventV1 |
| LiquidationEvent | LiquidationEventV1 |
| FundingSettlementEvent | FundingSettlementEventV1 |
| PerpetualCreatedEvent | PerpetualCreatedEventV1 |

**新增事件**：

| 事件 | 用途 | 触发时机 |
|------|------|----------|
| OrderPlacedEventV1 | 订单进入订单簿 | 订单未完全成交，剩余进入簿 |
| OrderRemovedEventV1 | 订单移除 | 取消、过期、完全成交、清算 |
| **OrderbookSnapshotEvent** | 链上订单簿全量快照 | 每 250ms（交易触发） |

**设计理由**：
- 事件在同一位置发射（sui-execution/src/dex.rs）
- dex-indexer 和 dex-realtime 处理相同事件结构
- 简化维护，避免重复定义

> 详细事件结构见 `sui/mynotes/dex/analyst/phase2_realtime/05-event-definitions.md`

### 2.4 依赖关系

```
    sui-types
        │
        ├─────────────────────┐
        │                     │
    dex-indexer           dex-realtime
        │                     │
        │                Redis Stream
        │                     │
        ├─────────────────────┤
        │                     │
    dex-api               dex-ws
  (PostgreSQL)           (Redis)
```

---

## 3. 各模块详细设计

### 3.1 dex-indexer

**职责**：从 Checkpoint 中提取 DEX 事件，写入 PostgreSQL

**核心组件**：

| 组件 | 职责 |
|------|------|
| `handlers/fills.rs` | 处理 FillEvent，写入 dex_fills 表 |
| `handlers/balances.rs` | 处理 BalanceUpdateEvent，写入 dex_balances 表 |
| `handlers/positions.rs` | 处理 PositionUpdateEvent，写入 dex_positions 表 |
| `schema/mod.rs` | Diesel 表定义，StoredFill, StoredBalance 等 |

**依赖**：
- `sui-indexer-alt-framework`：Checkpoint 处理框架
- `sui-types`：DEX 事件类型定义
- `diesel`：PostgreSQL ORM

### 3.2 dex-api

**职责**：提供 REST API 查询服务

**核心组件**：

| 组件 | 职责 |
|------|------|
| `types.rs` | InfoRequest 枚举，各种 Response 类型 |
| `server.rs` | Axum 服务器，路由配置 |
| `handlers.rs` | 查询处理，调用数据库 |
| `cache/` | [Phase 3] Redis 缓存层 |

**API 端点**（参考 Hyperliquid）：

| 端点 | 方法 | 请求类型 | 说明 |
|------|------|----------|------|
| `/health` | GET | - | 健康检查 |
| `/info` | POST | `type: "userFills"` | 用户成交历史 |
| `/info` | POST | `type: "userBalances"` | 用户余额变化 |
| `/info` | POST | `type: "recentFills"` | 市场最近成交 |
| `/info` | POST | `type: "clearinghouseState"` | 用户持仓状态 |

**依赖**：
- `dex-indexer`：仅用于 schema 定义
- `axum`：Web 框架
- `sui-pg-db`：数据库连接池

### 3.3 dex-realtime（Phase 2）

**职责**：订阅 Sui RPC 事件，发布到 Redis Stream，处理订单簿快照和聚合数据

**核心组件**：

| 组件 | 职责 |
|------|------|
| `config.rs` | 配置加载（RPC URL、Redis、重连策略） |
| `listener.rs` | sui_subscribeEvent 订阅，指数退避重连 |
| `publisher.rs` | Redis Stream 批量发布 + 订单簿快照写入 Redis Hash |
| `candles.rs` | K 线聚合（1m/5m/15m/1h/4h/1d） |
| `market_stats.rs` | 市场统计（中间价、24h成交量） |

> **架构简化说明**：订单簿数据采用「链上内存订单簿快照推送」方案（方案 A - 复用 Sui Event），dex-realtime 直接消费链上 `OrderbookSnapshotEvent`，无需本地维护订单簿状态（删除 orderbook.rs），也无需启动恢复逻辑（删除 recovery.rs）。

**订阅事件（实时通道）**：

| 事件 | Redis Key | 类型 | 用途 |
|------|-----------|------|------|
| FillEventV1 | `dex:stream:fills` | Stream | 成交记录、K线聚合 |
| OrderPlacedEventV1 | `dex:stream:orders` | Stream | 订单状态通知 |
| OrderRemovedEventV1 | `dex:stream:orders` | Stream | 订单状态通知 |
| PositionUpdateEventV1 | `dex:stream:positions` | Stream | 持仓变化 |
| LiquidationEventV1 | `dex:stream:liquidations` | Stream | 清算通知 |
| **OrderbookSnapshotEvent** | `dex:orderbook:{id}` | **Hash** | 订单簿全量快照（每 250ms） |

**RPC 订阅配置**：
```rust
// 使用 sui_subscribeEvent + MoveEventType 过滤
let filters = vec![
    EventFilter::MoveEventType("PKG::dex_events::FillEventV1".parse()?),
    EventFilter::MoveEventType("PKG::dex_events::OrderPlacedEventV1".parse()?),
    EventFilter::MoveEventType("PKG::dex_events::OrderRemovedEventV1".parse()?),
    // ...
];
let filter = EventFilter::Any(filters);
```

**重连策略**：
- 初始延迟：1s
- 最大延迟：30s
- 退避乘数：2

**依赖**：
- `sui-sdk`：Sui RPC 客户端
- `redis`：Redis 客户端（Stream + Hash）
- `diesel`：PostgreSQL（启动恢复）

### 3.4 dex-ws（Phase 2）

**职责**：WebSocket 推送服务

**核心组件**：

| 组件 | 职责 |
|------|------|
| `config.rs` | 配置（端口、Redis、心跳） |
| `types.rs` | 订阅请求、推送消息类型 |
| `server.rs` | WebSocket 服务器 + 心跳检测 |
| `channels.rs` | 订阅频道管理 |
| `subscriber.rs` | Redis Stream 消费者组 |

**订阅频道**（对标 Hyperliquid）：

| 频道 | 数据源 | 说明 |
|------|--------|------|
| `trades:{perpetual_id}` | dex:stream:fills | 实时成交 |
| `l2Book:{perpetual_id}` | dex:orderbook:{id} | 订单簿快照（链上 OrderbookSnapshotEvent） |
| `candle:{perpetual_id}:{interval}` | dex:candles:{id}:{interval} | K线更新 |
| `orderUpdates:{subaccount}` | dex:stream:orders | 用户订单状态 |
| `userFills:{subaccount}` | dex:stream:fills | 用户成交 |
| `userPositions:{subaccount}` | dex:stream:positions | 用户持仓 |

**推送机制**：
- 新连接：发送全量快照（从 Redis 读取）
- 正常推送：增量更新（从 Redis Stream 消费）
- 心跳间隔：1s
- 心跳超时：5s

**依赖**：
- `tokio-tungstenite`：WebSocket 库
- `redis`：Redis 客户端（Stream 消费者组）

---

## 4. 数据流设计

### 4.1 Phase 1: OnChain 数据流

```
Sui Checkpoint ──► dex-indexer ──► PostgreSQL ──► dex-api
                                                     │
                                                REST 客户端
```

**延迟**：~3-5 秒（Checkpoint 确认时间）

**特点**：
- 数据一致性高（最终确认）
- 无需回滚处理
- 适合历史查询

### 4.2 Phase 2: 双通道数据流

```
Sui Node
    │
    ├── RPC 订阅 ──► dex-realtime ──► Redis Stream ──► dex-ws
    │   (400-650ms)                                       │
    │                                              WebSocket 客户端
    │
    └── Checkpoint ──► dex-indexer ──► PostgreSQL ──► dex-api
        (3-5s)                                           │
                                                    REST 客户端
```

**双通道对比**：

| 通道 | 延迟 | 数据来源 | 用途 |
|------|------|----------|------|
| OnChain | 3-5s | Checkpoint | 历史查询、持久化 |
| OffChain | 400-650ms (生产) | RPC 事件 | 实时推送、订单簿 |

### 4.3 Phase 3: 缓存优化

```
dex-realtime ──► Redis Stream
                      │
              ┌───────┴───────┐
              │               │
          dex-ws         dex-api/cache
                              │
                          dex-api
```

**缓存策略**：

| 数据类型 | 优先数据源 | 回退数据源 | 合并 | 说明 |
|---------|-----------|-----------|:----:|------|
| 订单簿 | Redis | - | ❌ | 仅 Redis，实时状态 |
| 市场统计 | Redis | - | ❌ | 仅 Redis，聚合值 |
| 最近成交 | Redis + PostgreSQL | - | ✅ | 合并：实时 + 历史 |
| K 线数据 | Redis + PostgreSQL | - | ✅ | 合并：当前周期 + 历史 |
| 用户持仓 | Redis | PostgreSQL | ⚠️ | Redis 优先，回退 |
| 成交历史 | PostgreSQL | - | ❌ | 仅 PostgreSQL |
| 余额历史 | PostgreSQL | - | ❌ | 仅 PostgreSQL |

**数据来源差异**：
- Redis 数据来自 **dex-realtime** 的 RPC 实时订阅（延迟 ~200ms）
- PostgreSQL 数据来自 **dex-indexer** 的 Checkpoint（延迟 ~2-3s）

**合并策略**：
- **最近成交**：Redis 获取最新 N 条，不足时从 PostgreSQL 补充更早记录，按时间戳排序去重
- **K 线**：历史（已完结周期）从 PostgreSQL，当前（未完结周期）从 Redis，按时间边界合并
- **用户持仓**：优先 Redis 快照，使用 Checkpoint 序列号判断新鲜度，过期/缺失时回退 PostgreSQL

**一致性保障**：
- 时间戳边界（updated_at）
- Checkpoint 序列号（cp_sequence_number）
- TTL 过期机制
- tx_digest/order_id 去重

> 详细合并逻辑见实施计划 Phase 3.2

---

## 5. 关键设计决策

### 5.1 为什么 REST 和 WebSocket 分离？

**问题分析**：

| 维度 | REST API | WebSocket |
|------|----------|-----------|
| 连接类型 | 短连接（请求/响应） | 长连接（持久） |
| 内存占用 | 低（无状态） | 高（每连接状态） |
| 并发模型 | 请求级 | 连接级 |

**分离优势**：
1. **资源隔离**：WebSocket 高并发不影响 REST 响应
2. **独立扩展**：可按需扩展 WebSocket 实例
3. **故障隔离**：一个服务异常不影响另一个

### 5.2 为什么 handlers 和 realtime 分离？

**问题分析**：

| 维度 | handlers (Checkpoint) | realtime (RPC) |
|------|----------------------|----------------|
| 数据源 | Checkpoint 批量 | RPC 事件流 |
| 故障恢复 | 可从任意点恢复 | 断连则丢失 |
| 存储后端 | PostgreSQL | Redis |

**分离优势**：
1. **故障隔离**：RPC 断连不影响 Checkpoint 索引
2. **独立扩展**：可部署多实例监听
3. **一致性**：与 REST/WS 分离保持一致

### 5.3 为什么不创建 dex-types？

**分析**：
- 事件类型已在 `sui-types/src/dex_events.rs`
- API 类型仅 dex-api 使用
- WS 类型仅 dex-ws 使用

**结论**：无需额外共享层，各 crate 内部定义类型更简洁

---

## 6. 技术栈

### 6.1 各模块依赖

| 模块 | 核心依赖 |
|------|----------|
| dex-indexer | sui-indexer-alt-framework, diesel, sui-types |
| dex-api | axum, sui-pg-db, dex-indexer (schema) |
| dex-realtime | sui-sdk, redis |
| dex-ws | tokio-tungstenite, redis |

### 6.2 存储系统

| 存储 | 用途 | 数据 |
|------|------|------|
| PostgreSQL | 持久化 | 成交历史、余额变化、持仓状态 |
| Redis Stream | 实时流 | 事件推送、订单簿更新 |
| Redis Hash | 缓存 | 活跃订单、当前仓位 |

---

## 7. 与 V4 的兼容性

### 7.1 保留的设计

| 设计 | 说明 |
|------|------|
| 虚拟 Package 地址 | DEX_EVENTS_PACKAGE 常量不变 |
| 事件类型定义 | FillEvent, PositionUpdateEvent 等不变 |
| API 风格 | POST /info 模式不变 |
| 数据库 schema | 表结构不变 |

### 7.2 迁移路径

1. **Step 1**：创建 dex-api crate，迁移 API 代码
2. **Step 2**：从 dex-indexer 删除 API 相关代码
3. **Step 3**：更新 workspace 配置
4. **Step 4**：验证编译和测试

---

## 8. 设计参考

| 设计维度 | 参考来源 | 说明 |
|---------|---------|------|
| **REST API** | Hyperliquid | POST /info 模式 |
| **WebSocket** | Hyperliquid | 订阅频道设计 |
| **事件设计** | dYdX v4 | OnChain/OffChain 双通道 |
| **模块分离** | 微服务架构 | 单一职责原则 |

---

## 9. Phase 2 技术选型

### 9.1 选型决策

| 决策项 | 选择 | 备选方案 | 理由 |
|--------|------|----------|------|
| 订阅 API | sui_subscribeEvent | sui_subscribeTransaction | 细粒度事件过滤，数据量小 |
| 消息队列 | Redis Stream | Kafka, RabbitMQ | 轻量，支持持久化，与缓存复用 |
| 节点连接（MVP） | Full Node 标准订阅 | - | 快速验证，延迟 2-4s |
| 节点连接（生产） | **Validator + 索引节点** | P2P 索引节点 | 延迟 400-650ms，安全性高 |
| 订单簿维护 | **链上快照推送（方案 A）** | 链下内存构建 | 链下逻辑极简，状态绝对一致，无需启动恢复 |

> **生产环境节点架构**：Validator 不直接暴露 RPC，同机器部署索引节点对内网提供服务。
> 详见 `sui/mynotes/dex/analyst/phase2_realtime/08-realtime-latency-analysis.md`

### 9.2 与 dYdX 的差异

| 方面 | dYdX v4 | 本项目 | 差异原因 |
|------|---------|--------|----------|
| 事件来源 | MemClob 内存状态 | 链上事件 | Sui 无应用层钩子 |
| 实时通道 | gRPC Stream | sui_subscribeEvent | 使用标准 RPC |
| 索引节点 | 专用 Full Node | 标准 Sui Full Node | 简化运维 |
| 订单簿恢复 | 内存重建 | **无需恢复（链上快照推送）** | 链上是权威数据源，等待下一个快照即可 |

> 详细差异分析见 `sui/mynotes/dex/analyst/phase2_realtime/03-dydx-streaming-reference.md`

---

## 10. Hyperliquid API 对标

### 10.1 HTTP API 对标

| Hyperliquid API | Phase 1 | Phase 2 | 数据源 |
|-----------------|:-------:|:-------:|--------|
| meta | ✓ | ✓ | dex_perpetuals |
| metaAndAssetCtxs | - | ✓ | PostgreSQL + Redis |
| l2Book | - | ✓ | Redis orderbook |
| candleSnapshot | - | ✓ | Redis candles |
| clearinghouseState | ✓ | ✓ | dex_positions + dex_balances |
| openOrders | - | ✓ | dex_orders |
| userFills | ✓ | ✓ | dex_fills |

### 10.2 WebSocket API 对标

| Hyperliquid WS | Phase 2 | 数据源 |
|----------------|:-------:|--------|
| allMids | ✓ | Redis market stats |
| l2Book | ✓ | Redis orderbook + Stream |
| trades | ✓ | Redis Stream fills |
| candle | ✓ | Redis candles |
| orderUpdates | ✓ | Redis Stream orders |
| userFills | ✓ | Redis Stream fills |
| userFundings | ✓ | Redis Stream funding |

---

## 11. Redis 存储设计

### 11.1 Stream（事件流，TTL 1h）

| Key | 消息类型 | 消费者 |
|-----|----------|--------|
| `dex:stream:fills` | FillEventV1 | dex-ws |
| `dex:stream:orders` | OrderPlaced/RemovedEventV1 | dex-ws |
| `dex:stream:positions` | PositionUpdateEventV1 | dex-ws |
| `dex:stream:liquidations` | LiquidationEventV1 | dex-ws |

### 11.2 Hash（状态数据）

| Key | Fields | 更新频率 | 来源 |
|-----|--------|----------|------|
| `dex:orderbook:{id}` | bids, asks, best_bid, best_ask, stats, sequence_number, updated_at | ~250ms | 链上 OrderbookSnapshotEvent |
| `dex:market:{id}` | mid_px, best_bid, best_ask, volume_24h | ~1s | dex-realtime 聚合 |
| `dex:candle:{id}:{interval}` | current OHLCV | 每次成交 | dex-realtime 聚合 |

### 11.3 Sorted Set（排序数据）

| Key | Score | 保留数量 |
|-----|-------|----------|
| `dex:trades:{id}` | timestamp | 1000 |
| `dex:candles:{id}:{interval}` | timestamp | 500 |
