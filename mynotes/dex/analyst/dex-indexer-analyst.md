# 基于 Sui 的 DEX Indexer 架构设计

> 借鉴 DYDX Indexer 设计，结合 Sui 特性，设计高性能 DEX 数据索引方案。

## 设计要点总结

### 核心问题解决方案

**Sui Checkpoint 延迟问题** → **双通道数据摄取架构**
- **FastPath Listener**: 订阅 Sui 节点 RPC，延迟 <500ms
- **Checkpoint Processor**: 复用 sui-indexer-alt，保证数据完整性

### 数据库三层架构

| 层级 | 数据库 | 用途 | 延迟 |
|------|--------|------|------|
| 实时层 | Redis Cluster | 订单簿、仓位、活跃订单 | <1ms |
| 时序层 | TimescaleDB | K线、成交、资金费率 | <10ms |
| 分析层 | ClickHouse | 历史订单、交易分析 | <500ms |

### 客户端连接方式

- **查看行情/仓位**: 连接 Indexer 的 WebSocket/REST
- **下单/取消**: 直接连接 Sui 节点 RPC

---

## 1. 背景与问题分析

### 1.1 Sui 现有索引机制的局限性

**sui-indexer-alt 特点** (参考 `sui_indexer_data_flow.md`):
- 数据来源：Checkpoint（每 ~3 秒生成）
- 延迟：1-5 秒（相对链上）
- 适用场景：历史数据查询、区块浏览器、通用 DApp

**对 DEX 的影响**:

| 数据类型 | 延迟要求 | Checkpoint 能否满足 | 说明 |
|---------|---------|------------------|------|
| 订单簿 | <100ms | ❌ 不满足 | 用户需要实时看到挂单变化 |
| 最新成交 | <500ms | ❌ 不满足 | 交易者需要快速确认成交 |
| K线数据 | <1s | ⚠️ 勉强 | 1分钟K线可接受，秒级不满足 |
| 仓位数据 | <1s | ⚠️ 勉强 | 清算场景需要更低延迟 |
| 历史订单 | 秒级 | ✅ 满足 | 历史查询对延迟不敏感 |
| 历史成交 | 秒级 | ✅ 满足 | 历史查询对延迟不敏感 |

### 1.2 核心问题

1. **实时性不足**: Checkpoint 3秒延迟无法满足交易界面的实时需求
2. **数据完整性**: 仅依赖 Checkpoint 可能丢失中间状态
3. **高频写入**: DEX 订单量大，需支持 10,000+ TPS 写入
4. **复杂查询**: K线聚合、历史分析等需要专门的存储方案

### 1.3 DYDX 解决方案的启示

DYDX 采用**双通道数据流**:
- **On-chain Events (to-ender)**: 区块级批量处理，保证数据完整性
- **Off-chain Updates (to-vulcan)**: 实时订单更新，低延迟响应

这种设计同样适用于 Sui DEX，但需要适配 Sui 的技术特性。

---

## 2. 整体架构设计

### 2.1 架构总览

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Sui DEX Indexer 架构                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                          链上层 (On-chain)                            │   │
│  │  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐         │   │
│  │  │ DEX 引擎     │     │ 订单匹配     │     │ 事件发射     │         │   │
│  │  │ (Native)    │────▶│ (引擎执行)   │────▶│ (Events)     │         │   │
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
│  │  │   FastPath Listener     │     │   Checkpoint Processor  │         │   │
│  │  │   (低延迟通道)           │     │   (批量通道)             │         │   │
│  │  │   - 订阅节点 RPC         │     │   - sui-indexer-alt     │         │   │
│  │  │   - 实时交易效果         │     │   - 批量解析事件         │         │   │
│  │  │   - <500ms 延迟         │     │   - 3-5s 延迟            │         │   │
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

### 2.2 与 DYDX 架构对比

| 组件 | DYDX | Sui DEX | 差异说明 |
|-----|------|---------|---------|
| 实时通道 | to-vulcan (Kafka) | FastPath (RPC + Redis Stream) | Sui 使用 RPC 订阅替代链下消息 |
| 批量通道 | to-ender (Kafka) | Checkpoint Processor | 从 Checkpoint 批量读取 |
| 实时缓存 | Redis | Redis | 相同 |
| 持久化 | PostgreSQL | TimescaleDB + ClickHouse | 针对时序和分析优化 |
| API | Comlink + Socks | REST + WebSocket | 相同模式 |

---

## 3. 双通道数据摄取设计

### 3.1 FastPath Listener (低延迟通道)

**目的**: 获取实时订单和成交数据，延迟 <500ms

**数据来源**:
```
Sui 节点 RPC
    │
    ├── sui_subscribeTransaction (WebSocket)
    │   - 订阅特定 Package 的交易
    │   - 返回 TransactionEffects
    │
    └── sui_getTransactions (轮询备选)
        - 按时间范围查询最新交易
        - 作为 WebSocket 断连的降级方案
```

**处理流程**:
```
Sui 节点
    │
    │ TransactionEffects
    ▼
FastPath Listener
    │
    │ 1. 过滤 DEX Package ID
    │ 2. 解析 Events
    │    - OrderPlaced
    │    - OrderMatched
    │    - OrderCancelled
    │    - PositionUpdated
    │ 3. 转换为内部消息格式
    ▼
Redis Stream
    │
    ├── realtime-orders   (订单变更)
    ├── realtime-trades   (成交记录)
    └── realtime-positions (仓位更新)
```

**Rust 伪代码**:
```rust
// FastPath Listener 核心逻辑
pub struct FastPathListener {
    sui_client: SuiClient,
    dex_engine_id: ObjectID,
    redis: RedisConnection,
}

impl FastPathListener {
    pub async fn start(&self) -> Result<()> {
        // 订阅 DEX 引擎的交易
        let filter = TransactionFilter::DexEngine {
            engine_id: self.dex_engine_id,
        };

        let mut subscription = self.sui_client
            .subscribe_transaction(filter)
            .await?;

        while let Some(effects) = subscription.next().await {
            self.process_transaction_effects(effects).await?;
        }
        Ok(())
    }

    async fn process_transaction_effects(&self, effects: TransactionEffects) -> Result<()> {
        // 解析事件
        for event in effects.events.iter() {
            match event.type_.name.as_str() {
                "OrderPlaced" => {
                    let order: OrderPlacedEvent = bcs::from_bytes(&event.contents)?;
                    self.redis.xadd("realtime-orders", &order.to_message()).await?;
                }
                "OrderMatched" => {
                    let trade: OrderMatchedEvent = bcs::from_bytes(&event.contents)?;
                    self.redis.xadd("realtime-trades", &trade.to_message()).await?;
                }
                "PositionUpdated" => {
                    let position: PositionUpdatedEvent = bcs::from_bytes(&event.contents)?;
                    self.redis.xadd("realtime-positions", &position.to_message()).await?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}
```

### 3.2 Checkpoint Processor (批量通道)

**目的**: 保证数据完整性，处理历史数据，与 FastPath 对账

**数据来源**:
```
Checkpoint Store (RocksDB)
    │
    │ 通过 sui-indexer-alt-framework 读取
    ▼
sui-indexer-alt Pipeline
    │
    │ 1. 读取 Checkpoint
    │ 2. 提取 DEX 相关交易和事件
    │ 3. 批量处理
    ▼
消息队列 (Kafka/Pulsar)
    │
    └── checkpoint-data (批量事件)
```

**与 sui-indexer-alt 集成**:
```rust
// 自定义 Pipeline Handler
use sui_indexer_alt_framework::{Handler, pipeline};

pub struct DexEventHandler {
    kafka_producer: KafkaProducer,
}

#[async_trait]
impl Handler for DexEventHandler {
    const NAME: &'static str = "dex_events";

    type Value = DexEvent;

    async fn process(&self, checkpoint: &CheckpointData) -> Result<Vec<Self::Value>> {
        let mut events = vec![];

        for tx in checkpoint.transactions.iter() {
            // 过滤 DEX Package
            if !tx.is_dex_transaction(&self.dex_package_id) {
                continue;
            }

            // 提取事件
            for event in tx.events.iter() {
                if let Some(dex_event) = DexEvent::try_from(event) {
                    events.push(dex_event);
                }
            }
        }

        // 发送到 Kafka
        self.kafka_producer.send_batch("checkpoint-data", &events).await?;

        Ok(events)
    }
}
```

### 3.3 双通道数据对账

**问题**: FastPath 可能因网络抖动丢失事件，需要与 Checkpoint 数据对账

**对账策略**:
```
FastPath 数据 (Redis)
    │
    │ checkpoint_sequence 标记
    ▼
Reconciliation Service
    │
    │ 1. 对比 Checkpoint 数据
    │ 2. 发现遗漏事件
    │ 3. 补充到 Redis
    ▼
一致性保证
```

**实现方式**:
```rust
// 对账服务
pub struct ReconciliationService {
    redis: RedisConnection,
    timescaledb: TimescaleClient,
}

impl ReconciliationService {
    pub async fn reconcile(&self, checkpoint_seq: u64) -> Result<()> {
        // 1. 获取 Checkpoint 中的所有事件
        let checkpoint_events = self.get_checkpoint_events(checkpoint_seq).await?;

        // 2. 获取 Redis 中对应时间范围的事件
        let redis_events = self.get_redis_events_by_checkpoint(checkpoint_seq).await?;

        // 3. 找出遗漏的事件
        let missing = checkpoint_events
            .iter()
            .filter(|e| !redis_events.contains(e))
            .collect::<Vec<_>>();

        // 4. 补充遗漏事件
        for event in missing {
            self.redis.xadd("reconcile-events", event).await?;
        }

        Ok(())
    }
}
```

---

## 4. 数据库选型与设计

### 4.1 三层存储架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                          存储层架构                                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                     Redis Cluster                            │    │
│  │                     (实时数据层)                              │    │
│  │                                                               │    │
│  │  数据类型: 订单簿、活跃订单、仓位快照                          │    │
│  │  写入频率: 10,000+ ops/s                                      │    │
│  │  读取延迟: <1ms                                               │    │
│  │  数据保留: 热数据 (当前状态)                                   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                        │
│                              │ 异步同步                               │
│                              ▼                                        │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                     TimescaleDB                              │    │
│  │                     (时序数据层)                              │    │
│  │                                                               │    │
│  │  数据类型: K线、成交记录、资金费率                             │    │
│  │  写入频率: 100,000+ rows/s (批量)                             │    │
│  │  查询场景: 时间范围查询、连续聚合                              │    │
│  │  数据保留: 90天热数据                                         │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                        │
│                              │ 定期归档                               │
│                              ▼                                        │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                     ClickHouse                               │    │
│  │                     (分析数据层)                              │    │
│  │                                                               │    │
│  │  数据类型: 历史订单、历史仓位、交易分析                        │    │
│  │  写入频率: 1,000,000+ rows/s (批量)                           │    │
│  │  查询场景: OLAP分析、多维聚合、大范围查询                      │    │
│  │  数据保留: 永久 (可配置TTL)                                    │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.2 Redis 数据结构设计

**订单簿缓存**:
```
# 买单价格层级 (Sorted Set - 按价格降序)
Key: dex:orderbook:{market_id}:bids
Score: price
Value: total_quantity

# 卖单价格层级 (Sorted Set - 按价格升序)
Key: dex:orderbook:{market_id}:asks
Score: price
Value: total_quantity

# 订单明细 (Hash)
Key: dex:orders:{order_id}
Fields:
  - market_id
  - owner
  - side
  - price
  - quantity
  - filled_quantity
  - status
  - created_at
  - tx_digest

# 用户活跃订单索引 (Set)
Key: dex:user_orders:{address}
Value: [order_id, ...]

# 仓位快照 (Hash)
Key: dex:positions:{address}:{market_id}
Fields:
  - size
  - entry_price
  - unrealized_pnl
  - margin
  - updated_at

# 订单簿中间价缓存 (String)
Key: dex:mid_price:{market_id}
Value: mid_price
```

**Lua 脚本原子操作**:
```lua
-- 原子更新订单簿价格层级
-- KEYS[1] = orderbook key
-- ARGV[1] = price
-- ARGV[2] = quantity_delta
local current = redis.call('ZSCORE', KEYS[1], ARGV[1])
if current == false then
    current = 0
end
local new_quantity = current + tonumber(ARGV[2])
if new_quantity <= 0 then
    redis.call('ZREM', KEYS[1], ARGV[1])
    return 0
else
    redis.call('ZADD', KEYS[1], ARGV[1], new_quantity)
    return new_quantity
end
```

### 4.3 TimescaleDB 数据模型

**K线表 (Hypertable)**:
```sql
CREATE TABLE candles (
    market_id TEXT NOT NULL,
    resolution TEXT NOT NULL,  -- '1m', '5m', '15m', '1h', '4h', '1d'
    bucket TIMESTAMPTZ NOT NULL,
    open NUMERIC(38, 18) NOT NULL,
    high NUMERIC(38, 18) NOT NULL,
    low NUMERIC(38, 18) NOT NULL,
    close NUMERIC(38, 18) NOT NULL,
    volume NUMERIC(38, 18) NOT NULL,
    quote_volume NUMERIC(38, 18) NOT NULL,
    trade_count INTEGER NOT NULL,
    PRIMARY KEY (market_id, resolution, bucket)
);

-- 转换为超表
SELECT create_hypertable('candles', 'bucket',
    chunk_time_interval => INTERVAL '1 day');

-- 压缩策略
ALTER TABLE candles SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'market_id, resolution'
);
SELECT add_compression_policy('candles', INTERVAL '7 days');
```

**连续聚合 (自动计算高周期K线)**:
```sql
-- 从 1分钟 自动聚合到 5分钟
CREATE MATERIALIZED VIEW candles_5m
WITH (timescaledb.continuous) AS
SELECT
    market_id,
    '5m' AS resolution,
    time_bucket('5 minutes', bucket) AS bucket,
    first(open, bucket) AS open,
    max(high) AS high,
    min(low) AS low,
    last(close, bucket) AS close,
    sum(volume) AS volume,
    sum(quote_volume) AS quote_volume,
    sum(trade_count) AS trade_count
FROM candles
WHERE resolution = '1m'
GROUP BY market_id, time_bucket('5 minutes', bucket);

-- 自动刷新策略
SELECT add_continuous_aggregate_policy('candles_5m',
    start_offset => INTERVAL '1 hour',
    end_offset => INTERVAL '1 minute',
    schedule_interval => INTERVAL '1 minute');
```

**成交记录表**:
```sql
CREATE TABLE trades (
    id BIGSERIAL,
    market_id TEXT NOT NULL,
    tx_digest TEXT NOT NULL,
    maker_order_id TEXT NOT NULL,
    taker_order_id TEXT NOT NULL,
    maker_address TEXT NOT NULL,
    taker_address TEXT NOT NULL,
    side TEXT NOT NULL,  -- 'buy' or 'sell' (taker side)
    price NUMERIC(38, 18) NOT NULL,
    quantity NUMERIC(38, 18) NOT NULL,
    quote_quantity NUMERIC(38, 18) NOT NULL,
    maker_fee NUMERIC(38, 18) NOT NULL,
    taker_fee NUMERIC(38, 18) NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (id, created_at)
);

SELECT create_hypertable('trades', 'created_at',
    chunk_time_interval => INTERVAL '1 day');

-- 索引
CREATE INDEX idx_trades_market ON trades (market_id, created_at DESC);
CREATE INDEX idx_trades_maker ON trades (maker_address, created_at DESC);
CREATE INDEX idx_trades_taker ON trades (taker_address, created_at DESC);
```

**资金费率表**:
```sql
CREATE TABLE funding_rates (
    market_id TEXT NOT NULL,
    rate NUMERIC(38, 18) NOT NULL,
    mark_price NUMERIC(38, 18) NOT NULL,
    index_price NUMERIC(38, 18) NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (market_id, timestamp)
);

SELECT create_hypertable('funding_rates', 'timestamp',
    chunk_time_interval => INTERVAL '1 day');
```

### 4.4 ClickHouse 数据模型

**历史订单表**:
```sql
CREATE TABLE orders (
    order_id String,
    market_id LowCardinality(String),
    owner String,
    side Enum8('buy' = 1, 'sell' = 2),
    order_type Enum8('limit' = 1, 'market' = 2, 'stop_limit' = 3, 'stop_market' = 4),
    time_in_force Enum8('gtc' = 1, 'ioc' = 2, 'fok' = 3, 'post_only' = 4),
    price Decimal(38, 18),
    quantity Decimal(38, 18),
    filled_quantity Decimal(38, 18),
    status Enum8('open' = 1, 'partial' = 2, 'filled' = 3, 'cancelled' = 4, 'expired' = 5),
    tx_digest String,
    checkpoint_sequence UInt64,
    created_at DateTime64(3),
    updated_at DateTime64(3)
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (market_id, owner, created_at)
TTL created_at + INTERVAL 2 YEAR;
```

**历史仓位快照表**:
```sql
CREATE TABLE position_snapshots (
    address String,
    market_id LowCardinality(String),
    size Decimal(38, 18),
    entry_price Decimal(38, 18),
    mark_price Decimal(38, 18),
    unrealized_pnl Decimal(38, 18),
    realized_pnl Decimal(38, 18),
    margin Decimal(38, 18),
    leverage Decimal(10, 2),
    liquidation_price Decimal(38, 18),
    checkpoint_sequence UInt64,
    snapshot_time DateTime64(3)
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(snapshot_time)
ORDER BY (address, market_id, snapshot_time);
```

**交易分析表 (物化视图)**:
```sql
-- 用户交易统计
CREATE MATERIALIZED VIEW user_trading_stats
ENGINE = SummingMergeTree()
ORDER BY (address, market_id, trade_date)
AS SELECT
    taker_address AS address,
    market_id,
    toDate(created_at) AS trade_date,
    count() AS trade_count,
    sum(quote_quantity) AS total_volume,
    sum(taker_fee) AS total_fees
FROM trades
GROUP BY taker_address, market_id, toDate(created_at);

-- 市场统计
CREATE MATERIALIZED VIEW market_daily_stats
ENGINE = SummingMergeTree()
ORDER BY (market_id, trade_date)
AS SELECT
    market_id,
    toDate(created_at) AS trade_date,
    count() AS trade_count,
    sum(quantity) AS total_quantity,
    sum(quote_quantity) AS total_volume,
    uniq(maker_address, taker_address) AS unique_traders
FROM trades
GROUP BY market_id, toDate(created_at);
```

---

## 5. 处理服务设计

### 5.1 Orderbook Processor (订单簿处理器)

**职责**: 消费订单事件，维护 Redis 订单簿

```rust
pub struct OrderbookProcessor {
    redis: RedisConnection,
}

impl OrderbookProcessor {
    pub async fn process_order_placed(&self, event: &OrderPlacedEvent) -> Result<()> {
        // 1. 保存订单详情
        self.redis.hset(&format!("dex:orders:{}", event.order_id), &[
            ("market_id", &event.market_id),
            ("owner", &event.owner),
            ("side", &event.side.to_string()),
            ("price", &event.price.to_string()),
            ("quantity", &event.quantity.to_string()),
            ("filled_quantity", "0"),
            ("status", "open"),
        ]).await?;

        // 2. 添加到用户订单索引
        self.redis.sadd(
            &format!("dex:user_orders:{}", event.owner),
            &event.order_id
        ).await?;

        // 3. 更新订单簿价格层级
        let side_key = match event.side {
            Side::Buy => "bids",
            Side::Sell => "asks",
        };
        self.update_orderbook_level(
            &event.market_id,
            side_key,
            &event.price,
            &event.quantity,
        ).await?;

        Ok(())
    }

    pub async fn process_order_matched(&self, event: &OrderMatchedEvent) -> Result<()> {
        // 1. 更新 Maker 订单
        self.update_order_fill(&event.maker_order_id, &event.quantity).await?;

        // 2. 更新 Taker 订单
        self.update_order_fill(&event.taker_order_id, &event.quantity).await?;

        // 3. 减少订单簿深度
        let maker_side = match event.taker_side {
            Side::Buy => "asks",  // Taker 买入，消耗卖单
            Side::Sell => "bids",
        };
        self.update_orderbook_level(
            &event.market_id,
            maker_side,
            &event.price,
            &(-event.quantity),  // 减少数量
        ).await?;

        Ok(())
    }
}
```

### 5.2 Trade Processor (成交处理器)

**职责**: 处理成交事件，更新 K线和成交记录

```rust
pub struct TradeProcessor {
    timescaledb: TimescaleClient,
    redis: RedisConnection,
}

impl TradeProcessor {
    pub async fn process_trade(&self, trade: &TradeEvent) -> Result<()> {
        // 1. 写入 TimescaleDB 成交记录
        self.timescaledb.execute(
            "INSERT INTO trades (...) VALUES (...)",
            &[
                &trade.market_id,
                &trade.tx_digest,
                &trade.price,
                &trade.quantity,
                // ...
            ],
        ).await?;

        // 2. 更新 1分钟 K线
        self.update_candle(&trade.market_id, "1m", trade).await?;

        // 3. 更新 Redis 最新价格
        self.redis.set(
            &format!("dex:last_price:{}", trade.market_id),
            &trade.price.to_string(),
        ).await?;

        // 4. 发送 WebSocket 推送消息
        self.redis.xadd("ws-trades", &WsTrade::from(trade)).await?;

        Ok(())
    }

    async fn update_candle(
        &self,
        market_id: &str,
        resolution: &str,
        trade: &TradeEvent,
    ) -> Result<()> {
        let bucket = time_bucket(resolution, trade.timestamp);

        // UPSERT K线
        self.timescaledb.execute(
            r#"
            INSERT INTO candles (market_id, resolution, bucket, open, high, low, close, volume, quote_volume, trade_count)
            VALUES ($1, $2, $3, $4, $4, $4, $4, $5, $6, 1)
            ON CONFLICT (market_id, resolution, bucket) DO UPDATE SET
                high = GREATEST(candles.high, EXCLUDED.high),
                low = LEAST(candles.low, EXCLUDED.low),
                close = EXCLUDED.close,
                volume = candles.volume + EXCLUDED.volume,
                quote_volume = candles.quote_volume + EXCLUDED.quote_volume,
                trade_count = candles.trade_count + 1
            "#,
            &[market_id, resolution, &bucket, &trade.price, &trade.quantity, &trade.quote_quantity],
        ).await?;

        Ok(())
    }
}
```

### 5.3 History Processor (历史数据处理器)

**职责**: 处理 Checkpoint 数据，写入 ClickHouse

```rust
pub struct HistoryProcessor {
    clickhouse: ClickHouseClient,
}

impl HistoryProcessor {
    pub async fn process_checkpoint_batch(&self, events: &[DexEvent]) -> Result<()> {
        // 批量写入订单
        let orders: Vec<_> = events
            .iter()
            .filter_map(|e| e.as_order_event())
            .collect();

        if !orders.is_empty() {
            self.clickhouse.insert_batch("orders", &orders).await?;
        }

        // 批量写入仓位快照
        let positions: Vec<_> = events
            .iter()
            .filter_map(|e| e.as_position_event())
            .collect();

        if !positions.is_empty() {
            self.clickhouse.insert_batch("position_snapshots", &positions).await?;
        }

        Ok(())
    }
}
```

---

## 6. API 服务设计

### 6.1 REST API 端点

| 端点 | 方法 | 数据来源 | 说明 |
|-----|------|---------|------|
| `/v1/orderbook/{market_id}` | GET | Redis | 获取订单簿 |
| `/v1/trades/{market_id}` | GET | TimescaleDB | 获取最近成交 |
| `/v1/candles/{market_id}` | GET | TimescaleDB | 获取K线数据 |
| `/v1/orders` | GET | Redis + ClickHouse | 获取用户订单 |
| `/v1/positions/{address}` | GET | Redis | 获取用户仓位 |
| `/v1/funding-rates/{market_id}` | GET | TimescaleDB | 获取资金费率历史 |
| `/v1/markets` | GET | 内存缓存 | 获取市场列表 |
| `/v1/ticker/{market_id}` | GET | Redis | 获取24小时行情 |

**订单簿 API 实现**:
```rust
async fn get_orderbook(
    State(state): State<AppState>,
    Path(market_id): Path<String>,
    Query(params): Query<OrderbookParams>,
) -> Result<Json<OrderbookResponse>> {
    let depth = params.depth.unwrap_or(100);

    // 从 Redis 获取买卖盘
    let bids = state.redis
        .zrevrange_with_scores(&format!("dex:orderbook:{}:bids", market_id), 0, depth - 1)
        .await?;
    let asks = state.redis
        .zrange_with_scores(&format!("dex:orderbook:{}:asks", market_id), 0, depth - 1)
        .await?;

    Ok(Json(OrderbookResponse {
        market_id,
        bids: bids.into_iter().map(|(price, qty)| [price, qty]).collect(),
        asks: asks.into_iter().map(|(price, qty)| [price, qty]).collect(),
        timestamp: Utc::now(),
    }))
}
```

### 6.2 WebSocket 订阅频道

| 频道 | 订阅格式 | 数据内容 | 更新频率 |
|-----|---------|---------|---------|
| `orderbook` | `orderbook:{market_id}` | 订单簿增量更新 | 实时 |
| `trades` | `trades:{market_id}` | 成交推送 | 实时 |
| `candles` | `candles:{market_id}:{resolution}` | K线更新 | 每秒 |
| `ticker` | `ticker:{market_id}` | 24h行情 | 每秒 |
| `account` | `account:{address}` | 账户更新 (订单/仓位) | 实时 |
| `positions` | `positions:{address}` | 仓位更新 | 实时 |

**WebSocket 消息格式**:
```json
// 订阅请求
{
    "op": "subscribe",
    "channel": "orderbook",
    "market_id": "BTC-USDC"
}

// 订单簿增量更新
{
    "channel": "orderbook",
    "market_id": "BTC-USDC",
    "type": "update",
    "data": {
        "bids": [["50000.00", "1.5"]],
        "asks": [["50001.00", "0"]],  // 0 表示删除该价位
        "timestamp": 1706284800000
    }
}

// 成交推送
{
    "channel": "trades",
    "market_id": "BTC-USDC",
    "type": "trade",
    "data": {
        "id": "trade_123",
        "price": "50000.50",
        "quantity": "0.1",
        "side": "buy",
        "timestamp": 1706284800000
    }
}
```

---

## 7. 客户端连接方式总结

### 7.1 连接决策表

| 需求 | 连接目标 | 数据来源 | 延迟 |
|-----|---------|---------|-----|
| 实时订单簿 | WebSocket (Indexer) | Redis | <100ms |
| 订单簿快照 | REST API (Indexer) | Redis | <50ms |
| 实时成交 | WebSocket (Indexer) | Redis Stream | <100ms |
| 实时K线 | WebSocket (Indexer) | Redis + TimescaleDB | <500ms |
| 账户仓位 | WebSocket (Indexer) | Redis | <100ms |
| 历史K线 | REST API (Indexer) | TimescaleDB | <100ms |
| 历史订单 | REST API (Indexer) | ClickHouse | <500ms |
| 历史成交 | REST API (Indexer) | ClickHouse | <500ms |
| **下单/取消** | **Sui 节点 RPC** | - | - |
| **查询链上状态** | **Sui 节点 RPC** | - | - |

### 7.2 典型交易流程

```
用户
  │
  ├─────────────────────────────────────────┐
  │ 1. 查看行情 (WebSocket)                  │
  │    订阅 orderbook, trades, candles       │
  │                                          ▼
  │                                    ┌──────────┐
  │                                    │ Indexer  │
  │                                    │ WS Server│
  │                                    └──────────┘
  │
  ├─────────────────────────────────────────┐
  │ 2. 下单 (RPC)                            │
  │    构造 MoveCall 交易                     │
  │    签名并提交                             ▼
  │                                    ┌──────────┐
  │                                    │ Sui Node │
  │                                    │   RPC    │
  │                                    └──────────┘
  │
  └─────────────────────────────────────────┐
    3. 接收订单状态更新 (WebSocket)            │
       - OrderPlaced 事件                     │
       - OrderMatched 事件                    ▼
                                       ┌──────────┐
                                       │ Indexer  │
                                       │ WS Server│
                                       └──────────┘
```

---

## 8. 高性能优化策略

### 8.1 写入优化

| 组件 | 优化策略 | 预期吞吐量 |
|-----|---------|----------|
| Redis | Pipeline 批量写入 | 100,000+ ops/s |
| TimescaleDB | COPY 批量插入 | 100,000+ rows/s |
| ClickHouse | Buffer 表异步写入 | 1,000,000+ rows/s |

**Redis Pipeline 示例**:
```rust
async fn batch_update_orderbook(
    redis: &RedisConnection,
    updates: &[OrderbookUpdate],
) -> Result<()> {
    let mut pipe = redis.pipe();

    for update in updates {
        pipe.zadd(
            &format!("dex:orderbook:{}:{}", update.market_id, update.side),
            &update.price,
            update.quantity,
        );
    }

    pipe.execute().await?;
    Ok(())
}
```

### 8.2 查询优化

```
请求路由策略:

实时数据请求
    │
    ├── 订单簿/仓位/活跃订单 → Redis (<1ms)
    │
    └── 最新成交 → Redis Stream + 内存缓存 (<5ms)

时序数据请求
    │
    ├── 近期K线 (7天内) → TimescaleDB 热分区 (<10ms)
    │
    └── 历史K线 (7天外) → TimescaleDB 压缩分区 (<50ms)

分析数据请求
    │
    └── 历史订单/交易统计 → ClickHouse (<500ms)
```

### 8.3 缓存策略

**多级缓存架构**:
```
┌────────────────────────────────────────────────────────┐
│                    缓存层次                              │
├────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────────┐                                   │
│  │   L1: 进程内存   │  市场配置、资产信息               │
│  │   (HashMap)      │  TTL: 60s, 读取: <1μs            │
│  └────────┬────────┘                                   │
│           │                                             │
│           ▼                                             │
│  ┌─────────────────┐                                   │
│  │   L2: Redis     │  订单簿、订单、仓位                │
│  │   (Cluster)     │  TTL: 实时, 读取: <1ms            │
│  └────────┬────────┘                                   │
│           │                                             │
│           ▼                                             │
│  ┌─────────────────┐                                   │
│  │  L3: TimescaleDB │  K线、成交                       │
│  │   (Hypertable)  │  TTL: 90d热, 读取: <10ms          │
│  └────────┬────────┘                                   │
│           │                                             │
│           ▼                                             │
│  ┌─────────────────┐                                   │
│  │  L4: ClickHouse │  历史数据、分析                    │
│  │   (Cold)        │  TTL: 永久, 读取: <500ms          │
│  └─────────────────┘                                   │
│                                                         │
└────────────────────────────────────────────────────────┘
```

---

## 9. 部署架构建议

### 9.1 服务部署

```
┌─────────────────────────────────────────────────────────────────────┐
│                        生产部署架构                                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                      数据摄取层                               │    │
│  │                                                               │    │
│  │  FastPath Listener × 2         Checkpoint Processor × 2      │    │
│  │  (Active-Standby)              (Active-Standby)              │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                      处理服务层                               │    │
│  │                                                               │    │
│  │  Orderbook Processor × 3       Trade Processor × 2           │    │
│  │  History Processor × 2         Reconciliation × 1            │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                      API 服务层                               │    │
│  │                                                               │    │
│  │  REST API × 4 (负载均衡)       WebSocket × 4 (负载均衡)      │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                      存储层                                   │    │
│  │                                                               │    │
│  │  Redis Cluster (6节点)         TimescaleDB (主从)            │    │
│  │  Kafka Cluster (3节点)         ClickHouse (2分片2副本)       │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 9.2 资源估算

| 组件 | CPU | 内存 | 存储 | 网络 |
|-----|-----|------|------|------|
| FastPath Listener | 2核 | 4GB | 10GB | 1Gbps |
| Checkpoint Processor | 4核 | 8GB | 100GB | 1Gbps |
| Orderbook Processor | 4核 | 8GB | 10GB | 1Gbps |
| Trade Processor | 4核 | 8GB | 10GB | 1Gbps |
| REST API | 4核 | 8GB | 10GB | 10Gbps |
| WebSocket Server | 8核 | 16GB | 10GB | 10Gbps |
| Redis Cluster (6节点) | 4核×6 | 32GB×6 | 100GB×6 | 10Gbps |
| TimescaleDB | 16核 | 64GB | 2TB SSD | 10Gbps |
| ClickHouse (4节点) | 8核×4 | 32GB×4 | 4TB×4 | 10Gbps |

---

## 10. 关键技术选型总结

| 组件 | 选型 | 选择原因 |
|-----|------|---------|
| 实时缓存 | Redis Cluster | 低延迟、复杂数据结构、Lua 脚本 |
| 时序存储 | TimescaleDB | 时序优化、连续聚合、PostgreSQL 兼容 |
| 分析存储 | ClickHouse | 超高写入吞吐、列式存储、OLAP 分析 |
| 消息队列 | Redis Stream + Kafka | 实时 + 批量双通道 |
| API 框架 | Axum (Rust) | 高性能、类型安全、异步 |
| WebSocket | tokio-tungstenite | Rust 原生、高性能 |

---

## 11. 代码级别详细分析: DEX Engine Events vs Off-chain Updates

> 参考 DYDX Indexer 的双通道数据流设计，定义 Sui DEX 的链上事件与链下更新机制。

### 11.1 DEX Engine Events (链上事件)

DEX Engine Events 由自定义 DEX 引擎在订单匹配、仓位更新等操作时发射，仍通过 Sui Events 机制被打包进 Checkpoint，由 Checkpoint Processor 批量处理后写入持久化存储。与传统 Move 智能合约不同，DEX 引擎直接内置于链上执行层，提供更高的性能和更低的延迟。

#### 事件类型定义

| 事件类型 | 触发场景 | DEX 引擎模块 | 存储目标 |
|---------|---------|-------------|---------|
| **OrderPlaced** | 有状态订单放置 (长期/条件订单) | `engine::orderbook` | TimescaleDB (orders) |
| **OrderMatched** | 订单成交 | `engine::matching` | TimescaleDB (fills, orders) |
| **OrderCancelled** | 订单取消 | `engine::orderbook` | TimescaleDB (orders) |
| **PositionUpdated** | 仓位变化 | `engine::position` | TimescaleDB (perpetual_positions) |
| **FundingRateUpdated** | 资金费率计算 | `engine::funding` | TimescaleDB (funding_rates) |
| **LiquidationExecuted** | 清算执行 | `engine::liquidation` | TimescaleDB (fills) + ClickHouse |
| **MarketCreated** | 市场创建 | `engine::market` | TimescaleDB (markets) |
| **TransferExecuted** | 转账事件 | `engine::transfer` | TimescaleDB (transfers) |

#### DEX 引擎事件结构示例

```rust
// dex_engine/events.rs
// 事件通过 Sui Events 机制发射，被 Checkpoint 收集

use serde::{Serialize, Deserialize};
use sui_types::{ObjectID, Address};

/// 订单成交事件
#[derive(Clone, Serialize, Deserialize)]
pub struct OrderMatchedEvent {
    pub market_id: ObjectID,
    pub maker_order_id: ObjectID,
    pub taker_order_id: ObjectID,
    pub maker_address: Address,
    pub taker_address: Address,
    pub side: Side,           // Buy / Sell (Taker side)
    pub price: u64,           // 价格 (subticks)
    pub quantity: u64,        // 数量 (quantums)
    pub maker_fee: u64,
    pub taker_fee: u64,
    pub timestamp: u64,
}

/// 仓位更新事件
#[derive(Clone, Serialize, Deserialize)]
pub struct PositionUpdatedEvent {
    pub owner: Address,
    pub market_id: ObjectID,
    pub side: Side,           // Long / Short
    pub size: u64,            // 仓位大小
    pub entry_price: u64,     // 入场价格
    pub margin: u64,          // 保证金
    pub unrealized_pnl: i64,
    pub checkpoint_sequence: u64,
}

/// 资金费率事件
#[derive(Clone, Serialize, Deserialize)]
pub struct FundingRateEvent {
    pub market_id: ObjectID,
    pub rate: i64,            // 资金费率 (有符号)
    pub mark_price: u64,
    pub index_price: u64,
    pub timestamp: u64,
}
```

#### 事件数据流

```
DEX Engine (Native)
    │
    │ engine.emit_event(OrderMatchedEvent {...})
    ▼
Transaction Effects (Sui Events)
    │
    │ 包含在 Checkpoint 中
    ▼
Checkpoint Store (RocksDB)
    │
    │ sui-indexer-alt 读取
    ▼
Checkpoint Processor
    │
    │ 解析事件 → 写入数据库
    ▼
TimescaleDB / ClickHouse
```

### 11.2 Off-chain Updates (链下更新)

Off-chain Updates 通过 FastPath Listener 订阅 Sui 节点 RPC 实时获取，用于**立即更新 Redis 缓存**，提供低延迟的订单簿和订单状态。

#### 更新类型定义

| 更新类型 | 触发场景 | 数据来源 | 存储目标 | 状态性质 |
|---------|---------|---------|---------|---------|
| **OrderPlaceUpdate** | 短期订单放置 | RPC 订阅 | Redis | 乐观状态 (BEST_EFFORT_OPENED) |
| **OrderUpdateUpdate** | 订单部分成交 | RPC 订阅 | Redis | 乐观状态 |
| **OrderRemoveUpdate** | 订单移除/取消/完全成交 | RPC 订阅 | Redis | 乐观状态 (BEST_EFFORT_CANCELED) |
| **PositionSnapshotUpdate** | 仓位快照更新 | RPC 订阅 | Redis | 乐观状态 |

#### 更新数据结构

```rust
// off_chain_updates.rs
#[derive(Clone, Serialize, Deserialize)]
pub enum OffChainUpdate {
    OrderPlace(OrderPlaceUpdate),
    OrderUpdate(OrderUpdateUpdate),
    OrderRemove(OrderRemoveUpdate),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OrderPlaceUpdate {
    pub order_id: String,
    pub market_id: String,
    pub owner: String,
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub placement_status: PlacementStatus,  // BEST_EFFORT_OPENED
    pub tx_digest: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OrderRemoveUpdate {
    pub order_id: String,
    pub removal_reason: RemovalReason,      // USER_CANCELED / FULLY_FILLED / EXPIRED / POST_ONLY_REJECTED
    pub removal_status: RemovalStatus,       // BEST_EFFORT_CANCELED
}

#[derive(Clone, Copy)]
pub enum PlacementStatus {
    BestEffortOpened,   // 乐观状态，可能被链上覆盖
    Opened,             // 链上确认
}

#[derive(Clone, Copy)]
pub enum RemovalStatus {
    BestEffortCanceled, // 乐观状态
    Canceled,           // 链上确认
    Filled,             // 完全成交
}
```

#### 链下更新数据流

```
Sui 节点 RPC
    │
    │ sui_subscribeTransaction (WebSocket)
    ▼
FastPath Listener
    │
    │ 1. 过滤 DEX Package ID
    │ 2. 解析 TransactionEffects
    │ 3. 生成 OffChainUpdate
    ▼
Redis Stream (realtime-orders)
    │
    ▼
Orderbook Processor
    │
    │ 更新 Redis 缓存
    ▼
Redis
    │
    ├── OrdersCache (订单数据)
    ├── OrderbookLevelsCache (价格层级)
    └── SubaccountOrderIdsCache (用户订单映射)
```

### 11.3 双通道对比总结

| 维度 | DEX Engine Events (链上事件) | Off-chain Updates (链下更新) |
|-----|----------------------------|---------------------------|
| **数据类型** | 8+ 种事件类型 | 4 种更新类型 |
| **触发时机** | 交易执行时发射事件 | RPC 订阅实时获取 |
| **发送方式** | Checkpoint 批量处理 | 实时单条处理 |
| **消息队列** | Kafka (checkpoint-data) | Redis Stream (realtime-*) |
| **处理服务** | Checkpoint Processor + Trade/History Processor | FastPath Listener + Orderbook Processor |
| **存储目标** | TimescaleDB + ClickHouse | Redis |
| **延迟** | 3-5 秒 (Checkpoint 间隔) | <500ms |
| **数据特点** | 最终确定、持久化 | 乐观更新、可能回滚 |

### 11.4 设计哲学

1. **最终一致性 vs 实时性**:
   - DEX Engine Events 保证最终一致性，但延迟较高 (Checkpoint 时间)
   - Off-chain Updates 提供实时响应，但可能因 Checkpoint 确认而需要修正

2. **数据分层**:
   - 持久化数据 (仓位、成交记录、历史订单) → DEX Engine Events → TimescaleDB/ClickHouse
   - 实时状态 (订单簿、活跃订单) → Off-chain Updates → Redis

3. **乐观更新策略**:
   - 短期订单使用 `BEST_EFFORT_OPENED` 状态
   - 取消使用 `BEST_EFFORT_CANCELED` 状态
   - 表示该状态是乐观的，可能被后续 Checkpoint 确认覆盖

---

## 12. 存储层详细分析

> 解答关键问题: FastPath 和 Checkpoint 数据各自存储到哪里？

### 12.1 核心结论

| 数据通道 | 处理服务 | 存储目标 | 是否写入 TimescaleDB |
|---------|---------|---------|---------------------|
| **FastPath (实时)** | Orderbook Processor | **Redis 仅缓存** | ❌ **否** |
| **Checkpoint (批量)** | Trade/History Processor | **TimescaleDB + ClickHouse** | ✅ **是** |

**关键发现**:
- **FastPath (Off-chain Updates) 只写入 Redis，不写入 TimescaleDB**
- **Checkpoint (DEX Engine Events) 写入 TimescaleDB/ClickHouse，同时更新部分 Redis 缓存用于数据一致性**
- TimescaleDB 中的数据是 Checkpoint 确认后的最终状态，Redis 中的数据是乐观的实时状态

### 12.2 数据流存储路径详解

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                            存储层数据流详解                                           │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   FastPath (实时通道)                                                                │
│   ════════════════════                                                              │
│                                                                                      │
│   Sui 节点 RPC                                                                       │
│       │                                                                              │
│       │ 1. 订阅交易效果 (TransactionEffects)                                         │
│       ▼                                                                              │
│   FastPath Listener                                                                  │
│       │                                                                              │
│       │ 2. 生成 OffChainUpdate 消息                                                  │
│       ▼                                                                              │
│   Redis Stream (realtime-orders)                                                     │
│       │                                                                              │
│       │ 3. Orderbook Processor 消费                                                  │
│       ▼                                                                              │
│   ┌─────────────────────────────────────────────────────────────────┐               │
│   │                   Orderbook Processor                            │               │
│   │                                                                  │               │
│   │   OrderPlaceHandler ──┬──► Redis (OrdersCache)                  │               │
│   │                       │                                          │               │
│   │                       ├──► Redis (OrderbookLevelsCache)         │               │
│   │                       │                                          │               │
│   │                       └──► Redis (UserOrdersCache)              │               │
│   │                                                                  │               │
│   │   OrderUpdateHandler ──► Redis (OrdersCache - 更新成交量)       │               │
│   │                                                                  │               │
│   │   OrderRemoveHandler ──► Redis (移除订单/更新订单簿)             │               │
│   │                                                                  │               │
│   │   ⚠️ 注意: Orderbook Processor 不写入任何 TimescaleDB 表        │               │
│   │                                                                  │               │
│   └─────────────────────────────────────────────────────────────────┘               │
│                                                                                      │
│   ──────────────────────────────────────────────────────────────────────────────    │
│                                                                                      │
│   Checkpoint (批量通道)                                                              │
│   ═════════════════════                                                             │
│                                                                                      │
│   Checkpoint Store (RocksDB)                                                         │
│       │                                                                              │
│       │ 1. sui-indexer-alt Pipeline 读取                                             │
│       ▼                                                                              │
│   Checkpoint Processor                                                               │
│       │                                                                              │
│       │ 2. 解析 DEX 事件，发送到 Kafka                                               │
│       ▼                                                                              │
│   Kafka (checkpoint-data)                                                            │
│       │                                                                              │
│       │ 3. Trade/History Processor 消费                                              │
│       ▼                                                                              │
│   ┌─────────────────────────────────────────────────────────────────┐               │
│   │                   Trade Processor                                │               │
│   │                                                                  │               │
│   │   OrderMatchedHandler ──┬──► TimescaleDB (fills)                │               │
│   │                         │                                        │               │
│   │                         ├──► TimescaleDB (orders - 更新状态)     │               │
│   │                         │                                        │               │
│   │                         └──► TimescaleDB (candles - 更新K线)    │               │
│   │                                                                  │               │
│   │   PositionUpdateHandler ──► TimescaleDB (perpetual_positions)   │               │
│   │                                                                  │               │
│   │   FundingRateHandler ──► TimescaleDB (funding_rates)            │               │
│   │                                                                  │               │
│   │   同时更新 Redis 缓存用于数据一致性:                             │               │
│   │   └──► Redis (StateFilledQuantumsCache)                         │               │
│   └─────────────────────────────────────────────────────────────────┘               │
│                                                                                      │
│   ┌─────────────────────────────────────────────────────────────────┐               │
│   │                   History Processor                              │               │
│   │                                                                  │               │
│   │   OrderEventHandler ──► ClickHouse (orders - 历史订单)          │               │
│   │                                                                  │               │
│   │   PositionSnapshotHandler ──► ClickHouse (position_snapshots)   │               │
│   │                                                                  │               │
│   │   TradeAnalyticsHandler ──► ClickHouse (物化视图)               │               │
│   └─────────────────────────────────────────────────────────────────┘               │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### 12.3 TimescaleDB 表结构详解

#### 核心表分类

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                          TimescaleDB 表结构分类                                       │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  交易核心表 (Trading Core)                                                    │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  orders                │ 订单记录                │ OrderPlaced, OrderMatched │  │
│   │  fills                 │ 成交记录 (Hypertable)   │ OrderMatched              │  │
│   │  candles               │ K线数据 (Hypertable)    │ OrderMatched (聚合计算)   │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  账户仓位表 (Account & Position)                                              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  perpetual_positions   │ 永续合约仓位            │ PositionUpdated           │  │
│   │  asset_positions       │ 资产余额                │ PositionUpdated           │  │
│   │  subaccounts           │ 子账户信息              │ 首次交易时创建            │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  资金费率表 (Funding)                                                         │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  funding_rates         │ 资金费率历史 (Hypertable)│ FundingRateUpdated       │  │
│   │  funding_payments      │ 资金费支付记录          │ PositionUpdated           │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  市场配置表 (Market Configuration)                                            │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  markets               │ 市场配置                │ MarketCreated             │  │
│   │  checkpoints           │ Checkpoint 同步状态     │ 每个 Checkpoint           │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

#### 详细表结构

##### 1. orders 表 (订单记录)

```sql
CREATE TABLE orders (
    id TEXT PRIMARY KEY,                  -- 订单 ID (ObjectID)
    market_id TEXT NOT NULL,              -- 市场 ID
    owner TEXT NOT NULL,                  -- 所有者地址
    side TEXT NOT NULL,                   -- 'buy' / 'sell'
    order_type TEXT NOT NULL,             -- 'limit' / 'market' / 'stop_limit' / 'stop_market'
    time_in_force TEXT NOT NULL,          -- 'gtc' / 'ioc' / 'fok' / 'post_only'
    price NUMERIC(38, 18) NOT NULL,       -- 订单价格
    quantity NUMERIC(38, 18) NOT NULL,    -- 订单数量
    filled_quantity NUMERIC(38, 18) NOT NULL DEFAULT 0, -- 已成交数量
    status TEXT NOT NULL,                 -- 'open' / 'partial' / 'filled' / 'cancelled' / 'expired'
    reduce_only BOOLEAN NOT NULL DEFAULT FALSE,
    trigger_price NUMERIC(38, 18),        -- 条件订单触发价
    tx_digest TEXT NOT NULL,              -- 创建交易哈希
    checkpoint_sequence BIGINT NOT NULL,  -- Checkpoint 序号
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,

    -- 索引
    CONSTRAINT idx_orders_owner_status UNIQUE (owner, status, created_at DESC),
    INDEX idx_orders_market_status (market_id, status)
);
```

##### 2. fills 表 (成交记录 - Hypertable)

```sql
CREATE TABLE fills (
    id BIGSERIAL,
    market_id TEXT NOT NULL,
    tx_digest TEXT NOT NULL,              -- 交易哈希
    maker_order_id TEXT NOT NULL,
    taker_order_id TEXT NOT NULL,
    maker_address TEXT NOT NULL,
    taker_address TEXT NOT NULL,
    side TEXT NOT NULL,                   -- Taker side: 'buy' / 'sell'
    price NUMERIC(38, 18) NOT NULL,
    quantity NUMERIC(38, 18) NOT NULL,
    quote_quantity NUMERIC(38, 18) NOT NULL,
    maker_fee NUMERIC(38, 18) NOT NULL,
    taker_fee NUMERIC(38, 18) NOT NULL,
    liquidity TEXT NOT NULL,              -- 'maker' / 'taker' (当前记录是哪一方)
    fill_type TEXT NOT NULL,              -- 'limit' / 'liquidation' / 'deleveraging'
    checkpoint_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (id, created_at)
);

-- 转换为 Hypertable
SELECT create_hypertable('fills', 'created_at',
    chunk_time_interval => INTERVAL '1 day');

-- 压缩策略
ALTER TABLE fills SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'market_id'
);
SELECT add_compression_policy('fills', INTERVAL '7 days');

-- 索引
CREATE INDEX idx_fills_market ON fills (market_id, created_at DESC);
CREATE INDEX idx_fills_maker ON fills (maker_address, created_at DESC);
CREATE INDEX idx_fills_taker ON fills (taker_address, created_at DESC);
```

##### 3. perpetual_positions 表 (永续仓位)

```sql
CREATE TABLE perpetual_positions (
    id TEXT PRIMARY KEY,                  -- Position ID
    owner TEXT NOT NULL,
    market_id TEXT NOT NULL,
    side TEXT NOT NULL,                   -- 'long' / 'short'
    status TEXT NOT NULL,                 -- 'open' / 'closed'
    size NUMERIC(38, 18) NOT NULL,        -- 仓位大小
    entry_price NUMERIC(38, 18) NOT NULL, -- 入场均价
    margin NUMERIC(38, 18) NOT NULL,      -- 保证金
    leverage NUMERIC(10, 2) NOT NULL,     -- 杠杆倍数
    unrealized_pnl NUMERIC(38, 18),       -- 未实现盈亏
    realized_pnl NUMERIC(38, 18) NOT NULL DEFAULT 0, -- 已实现盈亏
    liquidation_price NUMERIC(38, 18),    -- 清算价格
    settled_funding NUMERIC(38, 18) NOT NULL DEFAULT 0, -- 已结算资金费
    checkpoint_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    closed_at TIMESTAMPTZ,

    -- 索引
    INDEX idx_positions_owner (owner, market_id, status),
    INDEX idx_positions_market (market_id, status)
);
```

##### 4. funding_rates 表 (资金费率 - Hypertable)

```sql
CREATE TABLE funding_rates (
    market_id TEXT NOT NULL,
    rate NUMERIC(38, 18) NOT NULL,        -- 资金费率
    mark_price NUMERIC(38, 18) NOT NULL,  -- 标记价格
    index_price NUMERIC(38, 18) NOT NULL, -- 指数价格
    funding_index NUMERIC(38, 18) NOT NULL, -- 累计资金指数
    checkpoint_sequence BIGINT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (market_id, timestamp)
);

SELECT create_hypertable('funding_rates', 'timestamp',
    chunk_time_interval => INTERVAL '1 day');
```

##### 5. checkpoints 表 (同步状态)

```sql
CREATE TABLE checkpoints (
    sequence_number BIGINT PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL,
    digest TEXT NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    events_count INTEGER NOT NULL DEFAULT 0
);

-- 用于快速查找最新处理的 Checkpoint
CREATE INDEX idx_checkpoints_processed ON checkpoints (processed_at DESC);
```

### 12.4 Redis 缓存结构详解

| 缓存名称 | Redis Key 格式 | 数据类型 | 用途 | 写入来源 |
|---------|---------------|---------|------|---------|
| **OrdersCache** | `dex:orders:{order_id}` | HASH | 订单完整数据 | FastPath |
| **OrderbookLevelsCache** | `dex:orderbook:{market_id}:{side}` | ZSET | 价格层级聚合 | FastPath |
| **UserOrdersCache** | `dex:user_orders:{address}` | SET | 用户活跃订单 ID 列表 | FastPath |
| **PositionsCache** | `dex:positions:{address}:{market_id}` | HASH | 仓位快照 | FastPath |
| **MidPriceCache** | `dex:mid_price:{market_id}` | STRING | 订单簿中间价 | FastPath |
| **LastPriceCache** | `dex:last_price:{market_id}` | STRING | 最新成交价 | FastPath |
| **StateFilledQuantumsCache** | `dex:filled:{order_id}` | STRING | 链上确认成交量 | Checkpoint |

#### Redis 数据结构详解

```
# 订单簿价格层级 (ZSET)
Key: dex:orderbook:{market_id}:bids
Score: price (降序排列)
Value: total_quantity (该价位总数量)

Key: dex:orderbook:{market_id}:asks
Score: price (升序排列)
Value: total_quantity

# 订单详情 (HASH)
Key: dex:orders:{order_id}
Fields:
  - market_id: string
  - owner: string
  - side: string
  - price: string
  - quantity: string
  - filled_quantity: string
  - status: string  # 'BEST_EFFORT_OPENED' / 'OPEN' / 'FILLED' / 'BEST_EFFORT_CANCELED'
  - created_at: timestamp
  - tx_digest: string

# 仓位快照 (HASH)
Key: dex:positions:{address}:{market_id}
Fields:
  - side: string
  - size: string
  - entry_price: string
  - margin: string
  - unrealized_pnl: string
  - liquidation_price: string
  - updated_at: timestamp
```

### 12.5 数据一致性保证

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                           数据一致性保证机制                                          │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   时间线:                                                                            │
│   ───────                                                                            │
│                                                                                      │
│   T0: 用户提交订单 (通过 Sui RPC)                                                    │
│        │                                                                             │
│   T1: 交易执行 (链上)                                                                │
│        │                                                                             │
│        ├──► FastPath 订阅到交易效果                                                  │
│        │    └──► Orderbook Processor ──► Redis (OrdersCache, OrderbookLevelsCache)  │
│        │         订单状态: BEST_EFFORT_OPENED                                        │
│        │                                                                             │
│   T2: 交易进入 Checkpoint (~3秒后)                                                   │
│        │                                                                             │
│        ├──► Checkpoint Processor 读取                                                │
│        │    └──► Trade Processor ──► TimescaleDB (orders, fills)                    │
│        │                            Redis (StateFilledQuantumsCache)                 │
│        │         订单状态: OPEN / FILLED                                             │
│        │                                                                             │
│   T3: Trade Processor 通知更新 Redis 状态                                            │
│        │                                                                             │
│        └──► Redis (OrdersCache 状态更新为 OPEN / FILLED / CANCELED)                  │
│                                                                                      │
│   ──────────────────────────────────────────────────────────────────────────────    │
│                                                                                      │
│   冲突解决策略:                                                                       │
│   ═════════════                                                                      │
│                                                                                      │
│   1. TimescaleDB 数据权威性 > Redis                                                  │
│      - 发生冲突时以 Checkpoint 确认的 TimescaleDB 数据为准                            │
│                                                                                      │
│   2. StateFilledQuantumsCache 作为桥梁                                               │
│      - Trade Processor 写入 Redis 中已确认的成交量                                    │
│      - Orderbook Processor 读取此缓存判断订单实际状态                                 │
│                                                                                      │
│   3. 乐观状态回滚                                                                    │
│      - 如果 Checkpoint 中没有该订单事件 (可能因为失败)                                │
│      - Trade Processor 会发送 OrderRemove 更新                                       │
│      - Orderbook Processor 收到后从 Redis 中移除该订单                               │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### 12.6 设计原理总结

| 设计决策 | 原因 |
|---------|------|
| **FastPath 不写 TimescaleDB** | 短期订单生命周期短，写入 TimescaleDB 会造成大量无效 I/O；Redis 内存操作延迟低，适合高频更新 |
| **Checkpoint 写 TimescaleDB** | Checkpoint 确认的数据是最终状态，需要持久化存储用于历史查询、审计和恢复 |
| **Redis 作为实时层** | 订单簿需要毫秒级响应，TimescaleDB 无法满足；Redis 支持复杂数据结构 (ZSET 用于价格层级) |
| **双缓存同步** | Trade Processor 更新 StateFilledQuantumsCache 通知 Orderbook Processor 实际链上状态，实现最终一致性 |

---

## 13. 部署架构分析

### 13.1 核心结论

**Sui DEX Indexer 采用统一部署模式，不是每个验证器节点配套一套。**

与 DYDX 类似，整个 Indexer 基础设施是独立于验证器网络的统一服务。

### 13.2 架构图

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                         Sui DEX Indexer 部署架构                                      │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                          Sui 验证器网络层                                    │   │
│   │  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐               │   │
│   │  │ Validator │  │ Validator │  │ Validator │  │ Validator │               │   │
│   │  │    #1     │  │    #2     │  │    #3     │  │    #N     │               │   │
│   │  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘               │   │
│   │        │              │              │              │                       │   │
│   │        └──────────────┴──────────────┴──────────────┘                       │   │
│   │                           │                                                  │   │
│   │                           │ Narwhal/Bullshark 共识                           │   │
│   │                           ▼                                                  │   │
│   └─────────────────────────────────────────────────────────────────────────────┘   │
│                               │                                                      │
│                               │ Checkpoint 同步                                      │
│                               ▼                                                      │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                    Indexer 专用全节点 (Full Node)                            │   │
│   │                                                                              │   │
│   │   配置:                                                                       │   │
│   │   --enable-indexer=true                                                      │   │
│   │   --rpc-server-address=0.0.0.0:9000                                         │   │
│   │                                                                              │   │
│   │   ┌─────────────────────────────┐    ┌─────────────────────────────┐        │   │
│   │   │   FastPath Listener         │    │   sui-indexer-alt Pipeline  │        │   │
│   │   │   (RPC 订阅实时交易)         │    │   (Checkpoint 批量处理)      │        │   │
│   │   └────────────┬────────────────┘    └────────────┬────────────────┘        │   │
│   │                │                                  │                          │   │
│   │                │ realtime-*                       │ checkpoint-data          │   │
│   │                ▼                                  ▼                          │   │
│   └────────────────┼──────────────────────────────────┼──────────────────────────┘   │
│                    │                                  │                              │
│                    └────────────────┬─────────────────┘                              │
│                                     │                                                │
│                                     ▼                                                │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                         消息队列层                                            │   │
│   │                                                                              │   │
│   │   ┌──────────────────┐              ┌──────────────────┐                    │   │
│   │   │   Redis Stream   │              │      Kafka       │                    │   │
│   │   │  (实时数据)       │              │  (批量数据)       │                    │   │
│   │   │                  │              │                  │                    │   │
│   │   │ • realtime-orders│              │ • checkpoint-data│                    │   │
│   │   │ • realtime-trades│              │ • to-websockets-*│                    │   │
│   │   │ • realtime-pos   │              │                  │                    │   │
│   │   └────────┬─────────┘              └────────┬─────────┘                    │   │
│   │            │                                 │                               │   │
│   └────────────┼─────────────────────────────────┼───────────────────────────────┘   │
│                │                                 │                                    │
│                ▼                                 ▼                                    │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                   Indexer 服务集群 (统一部署)                                 │   │
│   │                                                                              │   │
│   │   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │   │
│   │   │  Orderbook   │  │    Trade     │  │   History    │  │ Reconciliation│   │   │
│   │   │  Processor   │  │  Processor   │  │  Processor   │  │   Service    │   │   │
│   │   │ (实时订单簿) │  │  (K线/成交)  │  │  (历史数据)  │  │  (数据对账)  │   │   │
│   │   └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘   │   │
│   │          │                 │                 │                 │            │   │
│   │          └─────────────────┴─────────────────┴─────────────────┘            │   │
│   │                                     │                                        │   │
│   │                            ┌────────┴────────┐                               │   │
│   │                            ▼                 ▼                               │   │
│   │                      ┌───────────┐     ┌───────────┐                        │   │
│   │                      │   Redis   │     │TimescaleDB│                        │   │
│   │                      │ (实时缓存) │     │(时序存储) │                        │   │
│   │                      └───────────┘     └───────────┘                        │   │
│   │                            │                 │                               │   │
│   │                            │                 ▼                               │   │
│   │                            │           ┌───────────┐                        │   │
│   │                            │           │ClickHouse │                        │   │
│   │                            │           │(分析存储) │                        │   │
│   │                            │           └───────────┘                        │   │
│   │                            │                                                 │   │
│   └────────────────────────────┼─────────────────────────────────────────────────┘   │
│                                │                                                      │
│                                ▼                                                      │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                         API 服务层                                            │   │
│   │                                                                              │   │
│   │   ┌──────────────────┐                    ┌──────────────────┐              │   │
│   │   │    REST API      │                    │   WebSocket      │              │   │
│   │   │   (Axum × N)     │                    │  Server (× N)    │              │   │
│   │   │                  │                    │                  │              │   │
│   │   │ • 订单簿查询     │                    │ • 订单簿订阅     │              │   │
│   │   │ • 历史订单       │                    │ • 成交推送       │              │   │
│   │   │ • K线数据        │                    │ • 仓位更新       │              │   │
│   │   │ • 仓位查询       │                    │ • K线更新        │              │   │
│   │   └──────────────────┘                    └──────────────────┘              │   │
│   │                                                                              │   │
│   └─────────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                                 │
│                                    ▼                                                 │
│                              ┌──────────┐                                            │
│                              │  客户端   │                                            │
│                              │ (交易前端)│                                            │
│                              └──────────┘                                            │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### 13.3 节点类型对比

```
┌─────────────────────────────────────────────────────────────────┐
│                      Sui 节点类型                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    Validator (验证器)                    │    │
│  │                                                          │    │
│  │  功能: 参与共识、出块、签名 Checkpoint                    │    │
│  │  Indexer: ❌ 不运行 Indexer 服务                         │    │
│  │  RPC: 通常不对外开放                                      │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │               Regular Full Node (普通全节点)             │    │
│  │                                                          │    │
│  │  功能: 同步 Checkpoint、提供 RPC 服务                     │    │
│  │  Indexer: ❌ 不运行 Indexer 服务                         │    │
│  │  RPC: ✅ 对外提供查询服务                                │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │            Indexer Full Node (Indexer 专用全节点)        │    │
│  │                                                          │    │
│  │  功能: 同步 Checkpoint + 运行 sui-indexer-alt Pipeline   │    │
│  │        + FastPath Listener (RPC 订阅)                    │    │
│  │  Indexer: ✅ 发送数据到消息队列                          │    │
│  │  RPC: ✅ 支持交易订阅                                    │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### 关键配置参数对比

| 参数 | Validator | 普通全节点 | Indexer 全节点 |
|-----|-----------|-----------|---------------|
| 参与共识 | ✅ | ❌ | ❌ |
| 同步 Checkpoint | ✅ | ✅ | ✅ |
| RPC 服务 | 可选 | ✅ | ✅ |
| 交易订阅 (WebSocket) | ❌ | ✅ | ✅ |
| sui-indexer-alt | ❌ | 可选 | ✅ |
| FastPath Listener | ❌ | ❌ | ✅ |
| 发送到 Kafka | ❌ | ❌ | ✅ |

### 13.4 扩展性设计

```
                    ┌─────────────────┐
                    │ Indexer Full    │
                    │  Node #1        │──┐
                    │  (Primary)      │  │
                    └─────────────────┘  │
                                         │
                    ┌─────────────────┐  │    ┌───────────────────┐
                    │ Indexer Full    │──┼───▶│   Redis Stream    │
                    │  Node #2        │  │    │   + Kafka         │
                    │  (Backup)       │  │    └─────────┬─────────┘
                    └─────────────────┘  │              │
                                         │              ▼
                    ┌─────────────────┐  │    ┌───────────────────┐
                    │ Indexer Full    │──┘    │  Indexer Services │
                    │  Node #N        │       │  (Horizontal Scale)│
                    │  (Geo-replica)  │       └───────────────────┘
                    └─────────────────┘
```

**扩展策略**:

1. **多 Indexer 全节点**: 提高数据源可用性，支持故障切换
2. **Kafka 分区**: 按 market_id 分区，支持消费者组水平扩展
3. **Processor 水平扩展**: Orderbook/Trade/History Processor 可多副本部署
4. **API 服务水平扩展**: REST API 和 WebSocket Server 可通过负载均衡扩展

---

## 14. Checkpoint 回滚与数据恢复策略

### 14.1 Sui Checkpoint 特性

| 特性 | 描述 |
|-----|------|
| **生成间隔** | ~3 秒 |
| **包含内容** | 多个交易的执行结果、事件 |
| **最终性** | Checkpoint 一旦被 2f+1 验证器签名，具有即时最终性 |
| **回滚可能性** | 无 (与 CometBFT 类似) |

### 14.2 与 DYDX (CometBFT) 区块对比

| 特性 | Sui Checkpoint | CometBFT 区块 |
|-----|---------------|--------------|
| 生成间隔 | ~3 秒 | ~1-2 秒 |
| 最终性 | 即时最终 | 即时最终 |
| 回滚可能性 | 无 | 无 |
| 数据来源 | Checkpoint Store (RocksDB) | 区块链 |
| 事件包含方式 | Transaction Effects 中的 Events | 区块中的 Events |

### 14.3 Checkpoint 处理策略

```
┌─────────────────────────────────────────────────────────────────┐
│                    Checkpoint 处理流程                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   收到 Checkpoint 数据                                           │
│                    │                                             │
│                    ▼                                             │
│   ┌────────────────────────────────────┐                        │
│   │   shouldSkipCheckpoint(sequence)   │                        │
│   │   检查: sequence <= 当前已处理?     │                        │
│   └────────────────┬───────────────────┘                        │
│         是 │              │ 否                                   │
│            ▼              ▼                                      │
│   ┌─────────────┐  ┌─────────────────────────┐                  │
│   │  跳过处理    │  │ BEGIN TRANSACTION       │                  │
│   │  (幂等性)   │  │ 开始数据库事务           │                  │
│   └─────────────┘  └───────────┬─────────────┘                  │
│                                │                                 │
│                                ▼                                 │
│                    ┌───────────────────────┐                    │
│                    │   处理 Checkpoint     │                    │
│                    │   - 解析事件          │                    │
│                    │   - 写入 TimescaleDB  │                    │
│                    │   - 更新 Redis        │                    │
│                    └───────────┬───────────┘                    │
│                      成功 │         │ 失败                       │
│                          ▼         ▼                             │
│           ┌─────────────────┐  ┌─────────────────┐              │
│           │ COMMIT          │  │ ROLLBACK        │              │
│           │ 更新 checkpoints│  │ 抛出错误        │              │
│           │ 表记录          │  │ 等待重试        │              │
│           └─────────────────┘  └─────────────────┘              │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### 处理逻辑说明

| 情况 | 处理方式 | 原因 |
|-----|---------|------|
| **正常 Checkpoint** (seq = current+1) | 正常处理，提交事务 | 顺序处理 |
| **重复 Checkpoint** (seq <= current) | 跳过 | 幂等性保证 |
| **处理失败** | 事务回滚，抛出错误 | 可重试，消息队列不 ack |
| **跳跃 Checkpoint** (seq > current+1) | 刷新状态，尝试处理 | 可能是服务重启 |

### 14.4 数据恢复机制

由于 Sui Checkpoint 具有即时最终性，不会发生 Reorg，因此**不需要自动回滚机制**。

但需要提供手动恢复工具用于以下场景：

1. **Indexer 升级**: 数据格式变化需要重新同步
2. **数据不一致**: 发现 Bug 导致的数据错误
3. **测试环境重置**: 开发测试时清理数据

#### 恢复工具设计

```rust
// recovery_tool.rs
pub struct RecoveryTool {
    redis: RedisConnection,
    timescaledb: TimescaleClient,
    clickhouse: ClickHouseClient,
}

impl RecoveryTool {
    /// 清空 Redis 缓存
    pub async fn clear_redis(&self) -> Result<()> {
        self.redis.flushdb().await?;
        Ok(())
    }

    /// 清空 TimescaleDB 数据 (保留表结构)
    pub async fn clear_timescaledb(&self) -> Result<()> {
        self.timescaledb.execute("TRUNCATE orders, fills, perpetual_positions, candles, funding_rates CASCADE").await?;
        self.timescaledb.execute("TRUNCATE checkpoints").await?;
        Ok(())
    }

    /// 重置到指定 Checkpoint
    pub async fn reset_to_checkpoint(&self, sequence: u64) -> Result<()> {
        // 1. 删除该 Checkpoint 之后的所有数据
        self.timescaledb.execute(
            "DELETE FROM fills WHERE checkpoint_sequence > $1",
            &[&sequence]
        ).await?;
        self.timescaledb.execute(
            "DELETE FROM orders WHERE checkpoint_sequence > $1",
            &[&sequence]
        ).await?;
        // ... 其他表

        // 2. 更新 checkpoints 表
        self.timescaledb.execute(
            "DELETE FROM checkpoints WHERE sequence_number > $1",
            &[&sequence]
        ).await?;

        // 3. 清空 Redis 缓存 (将由重新同步填充)
        self.clear_redis().await?;

        Ok(())
    }

    /// 重新同步指定范围的 Checkpoint
    pub async fn resync_checkpoints(&self, from: u64, to: u64) -> Result<()> {
        // 从 Checkpoint Store 重新读取并处理
        // ...
        Ok(())
    }
}
```

#### 使用示例

```bash
# 清空所有数据并重新同步
recovery-tool --clear-all --resync

# 重置到指定 Checkpoint
recovery-tool --reset-to-checkpoint 1000000

# 重新同步指定范围
recovery-tool --resync --from 1000000 --to 1001000
```

### 14.5 监控与告警

为确保数据一致性，需要监控以下指标：

| 指标 | 告警阈值 | 说明 |
|-----|---------|------|
| Checkpoint 处理延迟 | > 30 秒 | Indexer 落后于链上 |
| Redis 与 TimescaleDB 订单数量差异 | > 100 | 可能存在数据不一致 |
| Checkpoint 处理失败率 | > 1% | 需要排查错误原因 |
| FastPath 与 Checkpoint 对账差异 | > 50 | 可能存在数据丢失 |

---

## 15. 与 DYDX 方案对比

| 维度 | DYDX | Sui DEX | 说明 |
|-----|------|---------|------|
| 链上数据源 | CometBFT 区块 | Sui Checkpoint | 不同共识机制 |
| 实时通道 | 链下消息 (to-vulcan) | 节点 RPC 订阅 | Sui 无独立链下通道，通过 RPC 订阅实现 |
| 批量通道 | Kafka (to-ender) | sui-indexer-alt + Kafka | 复用 Sui 官方索引框架 |
| 订单簿存储 | Redis | Redis | 相同 |
| K线存储 | PostgreSQL | TimescaleDB | 针对时序数据优化 |
| 历史存储 | PostgreSQL | ClickHouse | 针对大规模分析优化 |
| API 技术栈 | TypeScript (Node.js) | Rust (Axum) | 更高性能 |

---

## 16. 后续工作

1. **DEX 引擎事件设计**: 实现上述定义的 DEX Engine 事件结构
2. **FastPath 实现**: 开发基于 Sui RPC 订阅的实时数据监听器
3. **sui-indexer-alt 扩展**: 实现自定义 Pipeline Handler
4. **数据库 Schema 实现**: 根据第12节设计创建表结构
5. **API 规范定义**: 制定完整的 OpenAPI 和 WebSocket 协议规范
6. **数据一致性测试**: 验证双通道数据对账机制
7. **恢复工具实现**: 开发第14节设计的数据恢复工具
8. **性能测试**: 验证各组件在目标负载下的表现

---

## 参考资料

- Sui Indexer 数据流分析: `sui/mynotes/sui/analysis/sui_indexer_data_flow.md`
- sui-indexer-alt 源码: `sui/crates/sui-indexer-alt/`
- DYDX Indexer 分析: `sui/mynotes/dex/analyst/dydx-indexer-analyst.md`
- TimescaleDB 文档: https://docs.timescale.com/
- ClickHouse 文档: https://clickhouse.com/docs/

---

## 附录: 与 DYDX Indexer 章节对照表

| DYDX Indexer 章节 | Sui DEX Indexer 对应章节 | 说明 |
|------------------|------------------------|------|
| 1. 整体架构概览 | 2. 整体架构设计 | 架构总览 |
| 2. 核心服务组件 | 5. 处理服务设计 | 服务组件详解 |
| 3. 数据流详解 | 3. 双通道数据摄取设计 | 数据流设计 |
| 4. 数据存储设计 | 4. 数据库选型与设计 | 存储层设计 |
| 5. 高性能设计 | 8. 高性能优化策略 | 性能优化 |
| 6. 客户端连接方式 | 6. API 服务设计 + 7. 客户端连接方式 | API 设计 |
| 7. 完整数据流图 | 2. 整体架构设计 | 数据流图 |
| 11. 代码级别详细分析 | 11. 代码级别详细分析 | **新增** - DEX Engine Events vs Off-chain Updates |
| 12. 存储层详细分析 | 12. 存储层详细分析 | **新增** - 存储职责分离 |
| 13. 索引服务部署架构 | 13. 部署架构分析 | **新增** - 部署架构 |
| - | 14. Checkpoint 回滚策略 | **新增** - Sui 特有的 Checkpoint 处理 |
