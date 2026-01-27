# 基于 Sui 的 DEX Indexer 架构设计计划

## 任务目标

设计一个基于 Sui 的高性能 DEX Indexer 架构方案，解决以下核心问题：
1. Sui Checkpoint 约 3 秒的延迟对 DEX 实时数据的影响
2. 订单簿、K线、订单、仓位等数据的低延迟索引
3. 数据库选型（支持亿级数据高频写入和查询）
4. 完整的数据流设计

## 输出文件
`sui/mynotes/dex/analyst/dex-indexer-analyst.md`

---

## 背景分析

### 1. Sui 现有索引机制的局限性

**sui-indexer-alt 特点**：
- 数据来源：Checkpoint（每 ~3 秒生成）
- 延迟：1-5 秒（相对链上）
- 适用场景：历史数据查询、区块浏览器、通用 DApp

**对 DEX 的影响**：
| 数据类型 | 延迟要求 | Checkpoint 能否满足 |
|---------|---------|------------------|
| 订单簿 | <100ms | ❌ 不满足 |
| 最新成交 | <500ms | ❌ 不满足 |
| K线数据 | <1s | ⚠️ 勉强 |
| 仓位数据 | <1s | ⚠️ 勉强 |
| 历史订单 | 秒级 | ✅ 满足 |
| 历史成交 | 秒级 | ✅ 满足 |

### 2. DYDX Indexer 的核心设计借鉴

**双通道数据流**：
- On-chain Events (to-ender): 区块级批量处理，历史数据
- Off-chain Updates (to-vulcan): 实时订单更新，低延迟

**多级缓存架构**：
- Redis: 订单簿、订单状态（实时）
- PostgreSQL: 历史数据持久化

---

## 方案设计

### 架构总览

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Sui DEX Indexer 架构                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                          链上层 (On-chain)                            │   │
│  │  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐         │   │
│  │  │ DEX 合约     │     │ 订单匹配     │     │ 事件发射     │         │   │
│  │  │ (Move)      │────▶│ (链上执行)   │────▶│ (Events)     │         │   │
│  │  └──────────────┘     └──────────────┘     └──────────────┘         │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                    │                               │                         │
│                    │ 交易提交                       │ Checkpoint               │
│                    │ (毫秒级)                       │ (~3秒)                   │
│                    ▼                               ▼                         │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                        数据摄取层 (Dual-Channel)                       │   │
│  │                                                                        │   │
│  │  ┌─────────────────────────┐     ┌─────────────────────────┐         │   │
│  │  │   FastPath Listener     │     │ sui-indexer-alt-core    │         │   │
│  │  │   (低延迟通道)           │     │ (Checkpoint 通道)        │         │   │
│  │  │   - 订阅节点 RPC         │     │ - 读取 Checkpoint        │         │   │
│  │  │   - 实时交易/效果        │     │ - 批量解析事件           │         │   │
│  │  │   - <500ms 延迟         │     │ - 3-5s 延迟              │         │   │
│  │  └────────────┬────────────┘     └────────────┬────────────┘         │   │
│  │               │                               │                        │   │
│  └───────────────┼───────────────────────────────┼────────────────────────┘   │
│                  │                               │                         │
│                  ▼                               ▼                         │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                         消息队列层                                     │   │
│  │                                                                        │   │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐       │   │
│  │  │ realtime-orders │  │ realtime-trades │  │ checkpoint-data │       │   │
│  │  │ (Redis Stream)  │  │ (Redis Stream)  │  │ (Kafka/Pulsar) │       │   │
│  │  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘       │   │
│  │           │                    │                    │                 │   │
│  └───────────┼────────────────────┼────────────────────┼─────────────────┘   │
│              │                    │                    │                     │
│              ▼                    ▼                    ▼                     │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                         处理服务层                                     │   │
│  │                                                                        │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                │   │
│  │  │ Orderbook    │  │ Trade        │  │ History      │                │   │
│  │  │ Processor    │  │ Processor    │  │ Processor    │                │   │
│  │  │ (实时订单簿) │  │ (K线/成交)   │  │ (历史数据)   │                │   │
│  │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘                │   │
│  │         │                 │                 │                         │   │
│  └─────────┼─────────────────┼─────────────────┼─────────────────────────┘   │
│            │                 │                 │                             │
│            ▼                 ▼                 ▼                             │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                          存储层                                        │   │
│  │                                                                        │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                │   │
│  │  │    Redis     │  │  TimescaleDB │  │ ClickHouse   │                │   │
│  │  │  (实时缓存)  │  │  (时序数据)   │  │ (分析查询)   │                │   │
│  │  │  - 订单簿    │  │  - K线       │  │ - 历史订单   │                │   │
│  │  │  - 订单状态  │  │  - 成交记录  │  │ - 历史仓位   │                │   │
│  │  │  - 仓位快照  │  │  - 资金费率  │  │ - 聚合分析   │                │   │
│  │  └──────────────┘  └──────────────┘  └──────────────┘                │   │
│  │                                                                        │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                    │                                         │
│                                    ▼                                         │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                          API 服务层                                    │   │
│  │                                                                        │   │
│  │  ┌──────────────┐                    ┌──────────────┐                │   │
│  │  │  REST API    │                    │  WebSocket   │                │   │
│  │  │  (查询)      │                    │  (订阅推送)  │                │   │
│  │  └──────────────┘                    └──────────────┘                │   │
│  │                                                                        │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                    │                                         │
│                                    ▼                                         │
│                              ┌──────────┐                                    │
│                              │  客户端   │                                    │
│                              └──────────┘                                    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 核心组件设计

#### 1. 双通道数据摄取

**FastPath Listener (低延迟通道)**：
```
目的: 获取实时订单和成交数据，延迟 <500ms

数据来源:
- 订阅 Sui 节点的 TransactionEffects 流
- 使用 sui_subscribeTransaction 或轮询最新交易
- 解析 DEX 相关事件 (OrderPlaced, OrderMatched, OrderCancelled)

处理流程:
1. 监听节点交易效果
2. 过滤 DEX 合约相关交易
3. 解析事件数据
4. 推送到 Redis Stream
```

**Checkpoint Processor (批量通道)**：
```
目的: 保证数据完整性，处理历史数据

数据来源:
- sui-indexer-alt-framework
- Checkpoint 批量读取

处理流程:
1. 按 Checkpoint 批量读取
2. 解析所有 DEX 事件
3. 与 FastPath 数据对账
4. 写入持久化存储
```

#### 2. 数据库选型分析

**实时数据 - Redis Cluster**：
| 特性 | 说明 |
|-----|------|
| 数据类型 | 订单簿、活跃订单、仓位快照 |
| 写入频率 | 10,000+ ops/s |
| 读取延迟 | <1ms |
| 数据结构 | HSET (订单簿)、STRING (订单)、ZSET (排序) |
| 持久化 | AOF + RDB |
| 扩展性 | Cluster 分片 |

**时序数据 - TimescaleDB**：
| 特性 | 说明 |
|-----|------|
| 数据类型 | K线、成交记录、资金费率 |
| 写入频率 | 100,000+ rows/s (批量) |
| 查询场景 | 时间范围查询、聚合计算 |
| 特性 | 自动分区、压缩、连续聚合 |
| 容量 | 亿级数据 |

**历史分析 - ClickHouse**：
| 特性 | 说明 |
|-----|------|
| 数据类型 | 历史订单、历史仓位、交易分析 |
| 写入频率 | 1,000,000+ rows/s (批量) |
| 查询场景 | OLAP 分析、多维聚合 |
| 特性 | 列式存储、向量化执行 |
| 容量 | 百亿级数据 |

#### 3. DEX 特定数据结构

**订单簿缓存 (Redis)**：
```
# 价格层级
Key: dex:orderbook:{market_id}:{side}
Type: HSET
Value: { [price]: total_quantity }

# 订单明细
Key: dex:orders:{order_id}
Type: STRING
Value: OrderData (protobuf/JSON)

# 用户订单索引
Key: dex:user_orders:{address}
Type: SET
Value: [order_id, ...]

# 仓位快照
Key: dex:positions:{address}:{market_id}
Type: STRING
Value: PositionData
```

**K线数据 (TimescaleDB)**：
```sql
CREATE TABLE candles (
    market_id TEXT NOT NULL,
    resolution TEXT NOT NULL,  -- '1m', '5m', '1h', '1d'
    open_time TIMESTAMPTZ NOT NULL,
    open NUMERIC NOT NULL,
    high NUMERIC NOT NULL,
    low NUMERIC NOT NULL,
    close NUMERIC NOT NULL,
    volume NUMERIC NOT NULL,
    trade_count INTEGER NOT NULL,
    PRIMARY KEY (market_id, resolution, open_time)
);

-- 创建超表
SELECT create_hypertable('candles', 'open_time');

-- 连续聚合 (自动计算)
CREATE MATERIALIZED VIEW candles_5m
WITH (timescaledb.continuous) AS
SELECT
    market_id,
    time_bucket('5 minutes', open_time) AS bucket,
    first(open, open_time) AS open,
    max(high) AS high,
    min(low) AS low,
    last(close, open_time) AS close,
    sum(volume) AS volume,
    sum(trade_count) AS trade_count
FROM candles
WHERE resolution = '1m'
GROUP BY market_id, bucket;
```

**历史数据 (ClickHouse)**：
```sql
CREATE TABLE orders (
    order_id String,
    market_id String,
    owner String,
    side Enum8('buy' = 1, 'sell' = 2),
    order_type Enum8('limit' = 1, 'market' = 2),
    price Decimal(38, 18),
    quantity Decimal(38, 18),
    filled_quantity Decimal(38, 18),
    status Enum8('open' = 1, 'filled' = 2, 'cancelled' = 3, 'expired' = 4),
    created_at DateTime64(3),
    updated_at DateTime64(3),
    tx_digest String,
    checkpoint_sequence UInt64
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (market_id, owner, created_at)
TTL created_at + INTERVAL 1 YEAR;
```

#### 4. 数据流设计

**链上 → FastPath (实时路径)**：
```
DEX 合约执行
    │
    │ emit Event
    ▼
Sui 节点
    │
    │ TransactionEffects
    ▼
FastPath Listener
    │
    │ 过滤 DEX 事件
    │ 解析事件数据
    ▼
Redis Stream (realtime-*)
    │
    ├──► Orderbook Processor ──► Redis (订单簿)
    │
    ├──► Trade Processor ──► TimescaleDB (K线)
    │
    └──► Position Processor ──► Redis (仓位)
            │
            ▼
        WebSocket Server ──► 客户端推送
```

**链上 → Checkpoint (批量路径)**：
```
Checkpoint Store
    │
    │ 批量读取
    ▼
sui-indexer-alt-core
    │
    │ 解析 Checkpoint
    ▼
Kafka/Pulsar (checkpoint-data)
    │
    ├──► History Processor ──► ClickHouse (历史订单)
    │
    ├──► Reconciliation ──► 与 Redis 对账
    │
    └──► TimescaleDB Writer ──► TimescaleDB (补充K线)
```

#### 5. 客户端连接方式

| 需求 | 连接目标 | 数据来源 | 延迟 |
|-----|---------|---------|-----|
| 实时订单簿 | WebSocket | Redis | <100ms |
| 实时K线 | WebSocket | Redis + TimescaleDB | <500ms |
| 账户仓位 | WebSocket | Redis | <100ms |
| 历史K线 | REST API | TimescaleDB | <100ms |
| 历史订单 | REST API | ClickHouse | <500ms |
| 历史成交 | REST API | ClickHouse | <500ms |
| 下单/取消 | Sui 节点 | - | - |

#### 6. 高性能优化策略

**写入优化**：
- Redis Pipeline 批量写入
- TimescaleDB COPY 批量插入
- ClickHouse 异步批量插入 (Buffer 表)

**查询优化**：
- Redis: 内存数据，微秒级响应
- TimescaleDB: 分区 + 连续聚合，毫秒级响应
- ClickHouse: 列式存储 + 向量化，秒级分析

**缓存策略**：
```
请求 → Redis (实时) → TimescaleDB (时序) → ClickHouse (历史)
        <1ms           <10ms              <100ms
```

---

## 关键技术选型总结

| 组件 | 选型 | 原因 |
|-----|------|------|
| 实时缓存 | Redis Cluster | 低延迟、支持复杂数据结构 |
| 时序存储 | TimescaleDB | 时序优化、连续聚合、PostgreSQL 兼容 |
| 分析存储 | ClickHouse | 超高写入吞吐、OLAP 分析 |
| 消息队列 | Redis Stream + Kafka | 实时 + 批量双通道 |
| API 服务 | Rust (Axum/Actix) | 高性能、类型安全 |

---

## 执行计划

将上述设计内容整理写入 `sui/mynotes/dex/analyst/dex-indexer-analyst.md`
