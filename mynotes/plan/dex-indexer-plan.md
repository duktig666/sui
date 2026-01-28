# DEX Indexer 实施计划

> 原生 Rust DEX 引擎的链下索引服务

## 1. 项目概述

### 1.1 目标

构建一个与 Hyperliquid API 对标的 DEX Indexer，提供：
- 市场数据查询 (订单簿、K线、成交)
- 用户数据查询 (账户、订单、仓位)
- 实时数据推送 (WebSocket)

### 1.2 架构背景

**DEX 引擎特点**:
- 使用原生 Rust 开发（非 Move 合约）
- 事件使用原生 Rust 发出
- 链上仅处理资产托管和结算

**核心问题**: sui-indexer-alt 只能索引链上 Move 事件，无法直接索引原生 Rust DEX 引擎的事件。

### 1.3 架构方案对比

#### 方案概览

| 方案 | 传输方式 | 持久化 | 解耦程度 | 复杂度 | 延迟 |
|------|---------|--------|---------|--------|------|
| A: gRPC Streaming | 双向流 | ❌ 内存 | 中 | 低 | <1ms |
| B: 消息队列 (Kafka) | Pub/Sub | ✅ 磁盘 | 高 | 高 | 5-50ms |
| C: Redis Streams | Pub/Sub | ✅ 可选 | 高 | 中 | 1-5ms |
| D: 共享数据库 | PostgreSQL | ✅ 磁盘 | 低 | 低 | 10-50ms |
| E: 内嵌模式 | 函数调用 | ❌ | 无 | 最低 | <0.1ms |
| F: HTTP Webhook | HTTP POST | ❌ | 中 | 低 | 1-10ms |

---

#### 方案 A: gRPC Streaming ⭐ 推荐

```
DEX Engine ─── gRPC Stream ───→ Indexer ───→ PostgreSQL
```

| 维度 | 评价 |
|------|------|
| **优点** | 类型安全(Proto)、双向通信、背压控制、低延迟 |
| **缺点** | 事件不持久化、需要重连重放机制 |
| **适用** | 实时性要求高、单 Indexer 实例 |
| **参考** | dYdX v4 (gRPC between modules) |

**架构图**:
```
┌─────────────────────────────────────────────────────────────────┐
│                        DEX 引擎 (Rust)                           │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐ │
│  │ OrderBook │  │ Matching │  │ Position │  │ gRPC Server      │ │
│  └──────────┘  └──────────┘  └──────────┘  └────────┬─────────┘ │
└─────────────────────────────────────────────────────┼───────────┘
                                                      │ Stream
                                                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                      DEX Indexer (Rust)                          │
│  ┌──────────────┐  ┌──────────┐  ┌──────────┐  ┌─────────────┐  │
│  │ gRPC Client  │→│ Handlers │→│ PostgreSQL│→│ REST/WS API │  │
│  └──────────────┘  └──────────┘  └──────────┘  └─────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

#### 方案 B: 消息队列 (Kafka)

```
DEX Engine ───→ Kafka ───→ Indexer(s) ───→ PostgreSQL
```

| 维度 | 评价 |
|------|------|
| **优点** | 持久化、多消费者、水平扩展、事件回放 |
| **缺点** | 运维复杂、延迟较高、额外基础设施 |
| **适用** | 多 Indexer 实例、事件审计需求 |
| **参考** | 传统金融交易所 |

**架构图**:
```
┌────────────┐     ┌─────────┐     ┌────────────┐
│ DEX Engine │────→│  Kafka  │────→│ Indexer 1  │───→ PostgreSQL
└────────────┘     │ Cluster │────→│ Indexer 2  │───→ PostgreSQL (副本)
                   └─────────┘     └────────────┘
```

---

#### 方案 C: Redis Streams

```
DEX Engine ───→ Redis Streams ───→ Indexer ───→ PostgreSQL
```

| 维度 | 评价 |
|------|------|
| **优点** | 轻量级、低延迟、可持久化、消费者组 |
| **缺点** | 单点 (需 Cluster)、内存限制 |
| **适用** | 中等规模、已有 Redis 基础设施 |
| **参考** | 中小型交易平台 |

**架构图**:
```
┌────────────┐     ┌─────────────┐     ┌──────────┐
│ DEX Engine │────→│ Redis       │────→│ Indexer  │───→ PostgreSQL
└────────────┘     │ Streams     │     └──────────┘
                   └──────┬──────┘
                          │ Pub/Sub
                          ▼
                   ┌──────────────┐
                   │ WebSocket    │
                   └──────────────┘
```

---

#### 方案 D: 共享数据库

```
DEX Engine ───→ PostgreSQL ←─── Indexer (只读)
```

| 维度 | 评价 |
|------|------|
| **优点** | 最简单、无中间件、事务一致性 |
| **缺点** | 强耦合、写入性能瓶颈、难以扩展 |
| **适用** | MVP 验证、单机部署 |
| **参考** | 传统单体应用 |

**架构图**:
```
┌────────────┐          ┌────────────┐
│ DEX Engine │─ Write ─→│ PostgreSQL │←─ Read ─│ Indexer │───→ API
└────────────┘          │  (events)  │         └─────────┘
                        │  (orders)  │
                        │  (fills)   │
                        └────────────┘
```

---

#### 方案 E: 内嵌模式

```
┌─────────────────────────────────────┐
│          DEX Engine Process         │
│  ┌──────────┐  ┌──────────────────┐ │
│  │ Matching │──│ Indexer (库)     │─┼──→ PostgreSQL
│  └──────────┘  └──────────────────┘ │
└─────────────────────────────────────┘
```

| 维度 | 评价 |
|------|------|
| **优点** | 零延迟、无网络开销、部署简单 |
| **缺点** | 无法独立扩展/升级、故障影响引擎 |
| **适用** | 嵌入式场景、极致性能要求 |
| **参考** | 高频交易系统 |

---

#### 方案 F: HTTP Webhook

```
DEX Engine ─── HTTP POST ───→ Indexer ───→ PostgreSQL
```

| 维度 | 评价 |
|------|------|
| **优点** | 简单易实现、跨语言、防火墙友好 |
| **缺点** | 无背压控制、重试逻辑复杂、性能一般 |
| **适用** | 事件量小、跨网络部署 |
| **参考** | 第三方集成 |

---

### 1.4 方案选型建议

| 场景 | 推荐方案 | 理由 |
|------|---------|------|
| **MVP / Phase 1** | D (共享数据库) 或 A (gRPC) | 快速验证 |
| **生产环境** | A (gRPC) 或 C (Redis Streams) | 平衡性能和复杂度 |
| **高可用/多实例** | B (Kafka) 或 C (Redis Streams) | 消费者组支持 |
| **极致低延迟** | E (内嵌模式) | 零网络开销 |

**本项目推荐**: **方案 A (gRPC Streaming)** + **Phase 3 引入 Redis**

理由：
1. gRPC 提供类型安全和双向通信
2. Proto 定义可作为引擎和索引器的契约
3. Phase 3 引入 Redis 后可支持 WebSocket 广播
4. 复杂度适中，无需 Kafka 运维成本

---

### 1.5 选定架构: gRPC + Redis (Phase 3)

```
Phase 1-2:
DEX Engine ─── gRPC Stream ───→ Indexer ───→ PostgreSQL ───→ API

Phase 3:
DEX Engine ─── gRPC Stream ───→ Indexer ─┬→ PostgreSQL (历史)
                                         └→ Redis (热数据) → WebSocket
```

### 1.7 技术栈

| 组件 | 技术选型 | 说明 |
|------|---------|------|
| 事件传输 | gRPC Streaming (tonic) | DEX → Indexer 事件推送 |
| 存储 | PostgreSQL | 持久化存储 |
| 缓存 | Redis [Phase 3] | 实时数据缓存 + Pub/Sub |
| API | Axum | REST (Hyperliquid 风格) + WebSocket |
| ORM | Diesel | PostgreSQL ORM |

### 1.8 参考实现

- **dYdX v4**: 原生 Go 引擎 + gRPC + Kafka
- **Hyperliquid**: 高性能 DEX API 设计
- **Binance**: WebSocket 推送模式

---

## 2. 技术方案核心内容

### 2.1 事件定义 (Rust)

DEX Indexer 需要解析的事件类型：

```rust
// === 订单事件 ===
#[derive(Debug, Clone, Deserialize)]
pub struct OrderPlacedEvent {
    pub order_id: ObjectID,
    pub market_id: ObjectID,
    pub owner: SuiAddress,
    pub side: u8,           // 0=Buy, 1=Sell
    pub price: u64,
    pub quantity: u64,
    pub order_type: u8,     // 0=Limit, 1=Market, 2=StopLimit
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderMatchedEvent {
    pub fill_id: u64,
    pub market_id: ObjectID,
    pub maker_order_id: ObjectID,
    pub taker_order_id: ObjectID,
    pub maker_address: SuiAddress,
    pub taker_address: SuiAddress,
    pub price: u64,
    pub quantity: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderCanceledEvent {
    pub order_id: ObjectID,
    pub reason: u8,         // 0=User, 1=Expired, 2=Insufficient
    pub timestamp: u64,
}

// === 仓位事件 ===
#[derive(Debug, Clone, Deserialize)]
pub struct PositionOpenedEvent {
    pub position_id: ObjectID,
    pub owner: SuiAddress,
    pub market_id: ObjectID,
    pub side: u8,           // 0=Long, 1=Short
    pub size: u64,
    pub entry_price: u64,
    pub margin: u64,
    pub leverage: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PositionClosedEvent {
    pub position_id: ObjectID,
    pub realized_pnl: i64,  // 可正可负
    pub timestamp: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PositionLiquidatedEvent {
    pub position_id: ObjectID,
    pub liquidation_price: u64,
    pub timestamp: u64,
}

// === 资金费率事件 ===
#[derive(Debug, Clone, Deserialize)]
pub struct FundingPaidEvent {
    pub market_id: ObjectID,
    pub rate: i64,          // 可正可负
    pub mark_price: u64,
    pub index_price: u64,
    pub timestamp: u64,
}

// === 市场事件 ===
#[derive(Debug, Clone, Deserialize)]
pub struct MarketCreatedEvent {
    pub market_id: ObjectID,
    pub base_asset: String,
    pub quote_asset: String,
    pub tick_size: u64,
    pub lot_size: u64,
    pub max_leverage: u64,
}
```

### 2.2 API 端点设计 (对标 Hyperliquid)

> **设计原则**: 完全对标 Hyperliquid API 风格，使用 `POST /info` + JSON body

#### 统一端点: `POST /info`

**市场数据查询**:

| type 参数 | 请求示例 | 说明 |
|----------|---------|------|
| `meta` | `{"type": "meta"}` | 市场元数据列表 |
| `metaAndAssetCtxs` | `{"type": "metaAndAssetCtxs"}` | 市场+实时数据 |
| `l2Book` | `{"type": "l2Book", "coin": "BTC"}` | 订单簿深度 |
| `candleSnapshot` | `{"type": "candleSnapshot", "req": {"coin": "BTC", "interval": "1h", ...}}` | K线数据 |
| `recentTrades` | `{"type": "recentTrades", "coin": "BTC"}` | 最近成交 |
| `allMids` | `{"type": "allMids"}` | 所有中间价 |
| `fundingHistory` | `{"type": "fundingHistory", "coin": "BTC", "startTime": ...}` | 资金费率历史 |

**用户数据查询**:

| type 参数 | 请求示例 | 说明 |
|----------|---------|------|
| `clearinghouseState` | `{"type": "clearinghouseState", "user": "0x..."}` | 账户状态 |
| `openOrders` | `{"type": "openOrders", "user": "0x..."}` | 当前挂单 |
| `historicalOrders` | `{"type": "historicalOrders", "user": "0x..."}` | 历史订单 |
| `userFills` | `{"type": "userFills", "user": "0x..."}` | 成交记录 |
| `userFunding` | `{"type": "userFunding", "user": "0x..."}` | 用户资金费 |

#### 请求/响应示例

```json
// 请求: 获取订单簿
POST /info
{
  "type": "l2Book",
  "coin": "BTC"
}

// 响应
{
  "coin": "BTC",
  "levels": [
    [{"px": "42000.0", "sz": "1.5", "n": 3}, ...],  // bids
    [{"px": "42010.0", "sz": "2.0", "n": 5}, ...]   // asks
  ],
  "time": 1706428800000
}
```

```json
// 请求: 获取账户状态
POST /info
{
  "type": "clearinghouseState",
  "user": "0x..."
}

// 响应
{
  "marginSummary": {
    "accountValue": "10000.0",
    "totalMarginUsed": "2000.0",
    "totalNtlPos": "5000.0"
  },
  "assetPositions": [...],
  "withdrawable": "8000.0"
}
```

#### WebSocket 订阅 [P2]:

| 频道 | 对标 Hyperliquid | 说明 |
|------|-----------------|------|
| `l2Book:{market_id}` | `l2Book` | 订单簿更新 |
| `trades:{market_id}` | `trades` | 实时成交 |
| `candle:{market_id}:{interval}` | `candle` | K线更新 |
| `orderUpdates:{address}` | `orderUpdates` | 订单状态 |

### 2.3 数据模型

**核心实体**:

| 实体 | 表名 | 分区策略 |
|------|------|---------|
| Market | markets | 无分区 |
| Order | orders | 无分区 |
| Fill | fills | 按天分区 |
| Candle | candles | 按月分区 |
| Position | perpetual_positions | 无分区 |
| FundingRate | funding_rates | 按月分区 |

---

## 3. 实施阶段

### Phase 1: 事件总线 + 存储 (功能验证)

**目标**: 验证事件传输和数据模型

**范围**:
- ✅ gRPC 事件接收 (从 DEX 引擎)
- ✅ PostgreSQL 存储
- ✅ REST API (Hyperliquid 风格 POST /info)
- ❌ WebSocket 推送
- ❌ Redis 缓存

**数据流**:
```
DEX Engine ─── gRPC Stream ───→ Indexer ───→ PostgreSQL ───→ REST API
```

**事件传输协议** (gRPC):
```protobuf
service DexEvents {
  rpc StreamEvents(Empty) returns (stream DexEvent);
}

message DexEvent {
  oneof event {
    OrderPlacedEvent order_placed = 1;
    OrderMatchedEvent order_matched = 2;
    OrderCanceledEvent order_canceled = 3;
    PositionOpenedEvent position_opened = 4;
    // ...
  }
}
```

### Phase 2: WebSocket 推送

**目标**: 支持实时数据订阅推送

**新增范围**:
- ✅ WebSocket 服务器
- ✅ 订阅管理 (l2Book, trades, candle, orderUpdates)
- ✅ 事件驱动推送 (gRPC 事件触发 WebSocket)

**数据流**:
```
DEX Engine ─── gRPC ───→ Indexer ─┬→ PostgreSQL → REST API
                                  └→ WebSocket 推送
```

### Phase 3: Redis 缓存层 (体验优化)

**目标**: 降低查询延迟，优化订单簿

**新增范围**:
- ✅ Redis 缓存 (订单簿、活跃订单)
- ✅ Redis Pub/Sub (WebSocket 广播)
- ✅ 热数据分离 (Redis) vs 冷数据 (PostgreSQL)

**数据流**:
```
DEX Engine ─── gRPC ───→ Indexer ─┬→ Redis (热数据) → WebSocket
                                  └→ PostgreSQL (冷数据) → REST API
```

---

## 4. 目录结构

### 4.1 新建 crate: dex-indexer

```
dex/indexer/                                  # 独立项目 (不在 sui crates 内)
├── Cargo.toml
├── build.rs                                  # protobuf 编译
├── proto/
│   └── dex_events.proto                      # gRPC 事件定义
├── src/
│   ├── main.rs                               # 入口
│   ├── lib.rs                                # 库导出
│   ├── config.rs                             # 配置
│   ├── error.rs                              # 错误定义
│   │
│   ├── proto.rs                              # 生成的 protobuf 代码
│   ├── events.rs                             # DEX 事件类型 (Rust)
│   ├── types.rs                              # 核心类型定义
│   │
│   ├── receiver/                             # gRPC 事件接收
│   │   ├── mod.rs
│   │   └── grpc_client.rs                    # gRPC streaming client
│   │
│   ├── handlers/                             # 事件处理器
│   │   ├── mod.rs
│   │   ├── orders.rs                         # 订单事件处理
│   │   ├── fills.rs                          # 成交事件处理
│   │   ├── positions.rs                      # 仓位事件处理
│   │   ├── candles.rs                        # K线聚合
│   │   └── funding.rs                        # 资金费率
│   │
│   ├── models.rs                             # Diesel ORM 模型
│   ├── schema.rs                             # Diesel schema
│   │
│   ├── api/                                  # REST API (Hyperliquid 风格)
│   │   ├── mod.rs                            # POST /info 路由分发
│   │   ├── info.rs                           # 统一入口，按 type 分发
│   │   ├── markets.rs                        # meta, metaAndAssetCtxs
│   │   ├── orderbook.rs                      # l2Book
│   │   ├── trades.rs                         # recentTrades
│   │   ├── candles.rs                        # candleSnapshot
│   │   ├── account.rs                        # clearinghouseState
│   │   ├── orders.rs                         # openOrders, historicalOrders
│   │   ├── fills.rs                          # userFills
│   │   └── funding.rs                        # fundingHistory, userFunding
│   │
│   ├── ws/                                   # WebSocket [Phase 2]
│   │   ├── mod.rs
│   │   ├── server.rs                         # WebSocket 服务器
│   │   ├── subscriptions.rs                  # 订阅管理
│   │   └── broadcaster.rs                    # 消息广播
│   │
│   ├── cache/                                # Redis 缓存 [Phase 3]
│   │   ├── mod.rs
│   │   └── redis.rs                          # Redis 操作
│   │
│   ├── server.rs                             # HTTP 服务器 (Axum)
│   └── metrics.rs                            # Prometheus 指标
│
└── migrations/                               # Diesel 迁移
    ├── 2026-01-28-000001_create_markets/
    │   ├── up.sql
    │   └── down.sql
    ├── 2026-01-28-000002_create_orders/
    ├── 2026-01-28-000003_create_fills/
    ├── 2026-01-28-000004_create_positions/
    ├── 2026-01-28-000005_create_candles/
    └── 2026-01-28-000006_create_funding_rates/
```

### 4.2 DEX 引擎事件发布模块

```
dex/engine/                                   # DEX 引擎项目
├── src/
│   ├── events/
│   │   ├── mod.rs
│   │   ├── publisher.rs                      # 事件发布器
│   │   └── grpc_server.rs                    # gRPC 服务端
│   └── ...
└── proto/
    └── dex_events.proto                      # 共享 proto 定义
```

### 4.3 共享 Proto 定义

```protobuf
// proto/dex_events.proto
syntax = "proto3";
package dex;

service DexEventStream {
  rpc Subscribe(SubscribeRequest) returns (stream DexEvent);
}

message SubscribeRequest {
  repeated string event_types = 1;  // 过滤事件类型
}

message DexEvent {
  uint64 sequence = 1;              // 事件序号 (用于重放)
  uint64 timestamp = 2;
  oneof event {
    OrderPlacedEvent order_placed = 10;
    OrderMatchedEvent order_matched = 11;
    OrderCanceledEvent order_canceled = 12;
    PositionOpenedEvent position_opened = 20;
    PositionClosedEvent position_closed = 21;
    PositionLiquidatedEvent position_liquidated = 22;
    FundingPaidEvent funding_paid = 30;
    MarketCreatedEvent market_created = 40;
  }
}

message OrderPlacedEvent {
  bytes order_id = 1;
  bytes market_id = 2;
  bytes owner = 3;
  uint32 side = 4;        // 0=Buy, 1=Sell
  uint64 price = 5;
  uint64 quantity = 6;
  uint32 order_type = 7;  // 0=Limit, 1=Market
}

// ... 其他事件定义
```

### 4.4 依赖关系

```toml
# dex-indexer/Cargo.toml
[package]
name = "dex-indexer"
version = "0.1.0"
edition = "2021"

[dependencies]
# gRPC
tonic = "0.11"
prost = "0.12"

# 数据库
diesel = { version = "2.1", features = ["postgres", "chrono", "numeric", "r2d2"] }
diesel-async = { version = "0.4", features = ["postgres", "deadpool"] }

# Web 框架
axum = "0.7"
tower-http = { version = "0.5", features = ["cors", "trace"] }
tokio = { version = "1", features = ["full"] }

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 工具
anyhow = "1"
thiserror = "1"
tracing = "0.1"
chrono = { version = "0.4", features = ["serde"] }

# Phase 2: WebSocket
tokio-tungstenite = { version = "0.21", optional = true }

# Phase 3: Redis
redis = { version = "0.24", optional = true }

[build-dependencies]
tonic-build = "0.11"

[features]
default = []
websocket = ["tokio-tungstenite"]
cache = ["redis"]
```

---

## 5. 任务分工

> 两名工程师并行开发，通过 Proto 定义解耦

### 工程师 A: DEX 引擎 (Engine)

**职责**: 原生 Rust DEX 引擎开发、事件发布

| 阶段 | 任务 | 交付物 |
|------|------|--------|
| P1 | 实现订单簿核心逻辑 | `order_book.rs` |
| P1 | 实现撮合引擎 | `matching_engine.rs` |
| P1 | 定义共享 Proto | `dex_events.proto` |
| P1 | 实现 gRPC 事件发布 | `grpc_server.rs` |
| P1 | 编写单元测试 | Rust 测试 |
| P2 | 优化事件字段 (根据 Indexer 反馈) | Proto 更新 |

**产出目录**:
```
dex/engine/
├── proto/
│   └── dex_events.proto     # 共享事件定义
├── src/
│   ├── order_book.rs        # 订单簿逻辑
│   ├── matching_engine.rs   # 撮合引擎
│   ├── position.rs          # 仓位管理
│   └── events/
│       ├── publisher.rs     # 事件发布
│       └── grpc_server.rs   # gRPC 服务
└── tests/
```

### 工程师 B: Indexer (索引服务)

**职责**: 事件接收、存储设计、API 实现

| 阶段 | 任务 | 交付物 |
|------|------|--------|
| P1 | 搭建 dex-indexer 项目 | 项目框架 |
| P1 | 实现 gRPC 事件接收 | receiver/grpc_client.rs |
| P1 | 实现事件处理 Handler | handlers/*.rs |
| P1 | 设计数据库 Schema | migrations/ |
| P1 | 实现 REST API | api/*.rs |
| P2 | 实现 WebSocket 服务器 | ws/*.rs |
| P2 | 实现订阅管理 | subscriptions.rs |
| P3 | 集成 Redis 缓存 | cache/redis.rs |

**产出目录**:
```
dex/indexer/
├── proto/
│   └── dex_events.proto     # 从 engine 复制/软链接
├── src/
│   ├── receiver/            # gRPC 事件接收
│   ├── handlers/            # 事件处理
│   ├── api/                 # REST API
│   ├── ws/                  # WebSocket [P2]
│   └── cache/               # Redis [P3]
└── migrations/
```

### 协作接口

**Proto 契约** (引擎发布 → 索引接收):

| 事件 | 工程师 A (gRPC 发布) | 工程师 B (gRPC 接收) |
|------|---------------------|---------------------|
| OrderPlacedEvent | grpc_server.rs | grpc_client.rs → OrderHandler |
| OrderMatchedEvent | grpc_server.rs | grpc_client.rs → FillHandler |
| PositionOpenedEvent | grpc_server.rs | grpc_client.rs → PositionHandler |

**协作流程**:
```
1. A + B 共同定义 Proto 结构 → Review 确认字段完整性
2. A 实现 gRPC 服务端 → B 实现 gRPC 客户端
3. A 启动引擎发送测试事件 → B 验证接收和存储
4. B 反馈字段需求 → A 更新 Proto 和发布逻辑
```

**解耦策略**:
- Proto 定义先行，双方可独立开发
- B 可用 Mock gRPC Server 测试
- A 可用 Mock gRPC Client 验证发布

---

## 6. 里程碑

### M1: Proto 定义 + 项目框架 [P1]

| 任务 | 负责人 | 状态 |
|------|--------|------|
| 定义共享 Proto (dex_events.proto) | A + B | [ ] |
| 创建 dex-engine 项目框架 | A | [ ] |
| 创建 dex-indexer 项目框架 | B | [ ] |
| 配置 Cargo.toml + tonic-build | A + B | [ ] |

### M2: 事件总线 [P1]

| 任务 | 负责人 | 状态 |
|------|--------|------|
| 实现 gRPC 服务端 (事件发布) | A | [ ] |
| 实现 gRPC 客户端 (事件接收) | B | [ ] |
| 事件序列化/反序列化测试 | A + B | [ ] |
| 事件重放机制 (sequence) | A | [ ] |

### M3: 引擎核心逻辑 [P1]

| 任务 | 负责人 | 状态 |
|------|--------|------|
| 实现订单簿核心逻辑 | A | [ ] |
| 实现撮合引擎 | A | [ ] |
| 集成事件发布 | A | [ ] |
| 编写单元测试 | A | [ ] |

### M4: Indexer 存储层 [P1]

| 任务 | 负责人 | 状态 |
|------|--------|------|
| 实现 OrderHandler | B | [ ] |
| 实现 FillHandler | B | [ ] |
| 实现 PositionHandler | B | [ ] |
| 创建 Diesel migrations | B | [ ] |
| 实现 schema.rs + models.rs | B | [ ] |
| 批量写入优化 | B | [ ] |

### M5: REST API [P1]

| 任务 | 负责人 | 状态 |
|------|--------|------|
| 实现 POST /info 入口 | B | [ ] |
| 实现市场数据 API | B | [ ] |
| 实现用户数据 API | B | [ ] |
| 端到端测试 (引擎 → Indexer → API) | A + B | [ ] |

### M6: WebSocket 推送 [P2]

| 任务 | 负责人 | 状态 |
|------|--------|------|
| 实现 WebSocket 服务器 | B | [ ] |
| 实现订阅管理 | B | [ ] |
| 事件驱动推送 | B | [ ] |
| 前端集成测试 | A + B | [ ] |

### M7: Redis 缓存 [P3]

| 任务 | 负责人 | 状态 |
|------|--------|------|
| 集成 Redis 缓存 | B | [ ] |
| 热数据写入 Redis | B | [ ] |
| 优化实时订单簿查询 | B | [ ] |
| 性能压测 | A + B | [ ] |

---

## 7. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| Proto 变更 | 两端不兼容 | 版本化 Proto，向后兼容设计 |
| gRPC 连接中断 | 事件丢失 | 事件序号 + 重放机制 |
| 高并发写入 | DB 性能瓶颈 | 批量写入，分区表，连接池 |
| Redis 故障 | 热数据丢失 | Redis Cluster + 从 PG 重建 |
| 协作阻塞 | 开发进度延迟 | Proto 先行，Mock Server/Client |
| gRPC 方案不适用 | 架构调整 | 备选：Redis Streams 或 共享数据库 |

### 7.1 备选方案切换条件

如果 gRPC 方案遇到以下问题，考虑切换：

| 问题 | 切换方案 | 理由 |
|------|---------|------|
| 需要多个 Indexer 实例 | Redis Streams | 消费者组支持 |
| 需要事件持久化/审计 | Kafka | 磁盘持久化 |
| MVP 快速验证 | 共享数据库 | 最简单 |
| 极致性能要求 | 内嵌模式 | 零延迟 |

---

## 8. 与 sui-indexer-alt 的关系

| 场景 | 方案 |
|------|------|
| DEX 交易事件 | 原生 Rust gRPC → dex-indexer (本方案) |
| 资产托管/结算 | Move 合约 → sui-indexer-alt (链上数据) |
| 用户余额查询 | 可选：对接 sui-indexer-alt 或直接 RPC |

**说明**:
- 本 Indexer 专注于 DEX 引擎产生的交易事件
- 链上资产变更（充值/提现/结算）如需索引，可独立使用 sui-indexer-alt
- 两者可并行运行，API 层可整合

---

## 9. 下一步

1. **评审本计划**
2. **A + B 共同**: 定义 Proto 结构 (dex_events.proto)
3. **工程师 A 启动**: 实现 gRPC 服务端 + 订单簿核心逻辑
4. **工程师 B 启动**: 实现 gRPC 客户端 + Handler + 存储
5. **创建技术方案文档** `sui/mynotes/dex/tech/dex-indexer-tech.md`
   - 完整的 Proto 定义
   - API 请求/响应详细格式
   - 数据库 DDL
   - Handler 完整代码示例
