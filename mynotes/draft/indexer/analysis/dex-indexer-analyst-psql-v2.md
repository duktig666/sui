# 基于 Sui 的 DEX Indexer 架构设计 (PostgreSQL 版本)

> 借鉴 DYDX Indexer 设计，结合 Sui 特性，采用 **Redis + PostgreSQL** 两层存储架构。

## 版本说明

本文档是 `dex-indexer-analyst.md` 的 PostgreSQL 简化版本:
- **原版**: Redis + TimescaleDB + ClickHouse 三层架构
- **本版**: Redis + PostgreSQL 两层架构

## 设计要点总结

### 核心问题解决方案

**Sui Checkpoint 延迟问题** → **双通道数据摄取架构**
- **Realtime Listener**: 订阅 Sui 节点 RPC，延迟 <500ms
- **Checkpoint Processor**: 复用 sui-indexer-alt，保证数据完整性

### 数据库两层架构

| 层级 | 数据库 | 用途 | 延迟 |
|------|--------|------|------|
| 实时层 | Redis Cluster | 订单簿、仓位、活跃订单 | <1ms |
| 持久层 | PostgreSQL | K线、成交、订单、仓位、历史分析 | <50ms |

### 客户端连接方式

- **查看行情/仓位**: 连接 Indexer 的 WebSocket/REST
- **下单/取消**: 直接连接 Sui 节点 RPC

---

## 1. 背景与问题分析

(与原文档相同，此处省略)

---

## 2. 整体架构设计

### 2.1 架构总览

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Sui DEX Indexer 架构 (PostgreSQL 版)                       │
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
│  │  │   Realtime Listener     │     │   Checkpoint Processor  │         │   │
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
│  │                          存储层 (两层架构)                             │   │
│  │                                                                        │   │
│  │  ┌──────────────────────────┐  ┌──────────────────────────┐          │   │
│  │  │         Redis            │  │       PostgreSQL          │          │   │
│  │  │       (实时缓存)          │  │      (持久化存储)          │          │   │
│  │  │  - 订单簿                │  │  - 订单 (orders)          │          │   │
│  │  │  - 订单状态              │  │  - 成交 (fills)           │          │   │
│  │  │  - 仓位快照              │  │  - K线 (candles)          │          │   │
│  │  │                          │  │  - 仓位 (positions)       │          │   │
│  │  │                          │  │  - 资金费率               │          │   │
│  │  │                          │  │  - 分析物化视图           │          │   │
│  │  └──────────────────────────┘  └──────────────────────────┘          │   │
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

### 2.2 与原方案对比

| 组件 | 原方案 (三层) | PostgreSQL 方案 (两层) | 说明 |
|-----|-------------|---------------------|------|
| 实时缓存 | Redis | Redis | 不变 |
| 时序存储 | TimescaleDB | PostgreSQL (分区表) | 使用原生分区替代 Hypertable |
| 分析存储 | ClickHouse | PostgreSQL (物化视图) | 使用物化视图替代列式存储 |
| 运维复杂度 | 高 (3套系统) | 低 (2套系统) | 减少运维负担 |
| 性能上限 | 更高 | 够用 | 适合中小规模 DEX |

---

## 3. 双通道数据摄取设计

(与原文档相同，此处省略)

---

## 4. 数据库选型与设计

### 4.1 两层存储架构

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
│  │                     PostgreSQL                               │    │
│  │                   (统一持久化层)                              │    │
│  │                                                               │    │
│  │  ┌─────────────────────────────────────────────────────┐    │    │
│  │  │  OLTP 数据 (普通表)                                  │    │    │
│  │  │  - orders (订单)                                     │    │    │
│  │  │  - perpetual_positions (仓位)                        │    │    │
│  │  │  - markets (市场配置)                                │    │    │
│  │  └─────────────────────────────────────────────────────┘    │    │
│  │                                                               │    │
│  │  ┌─────────────────────────────────────────────────────┐    │    │
│  │  │  时序数据 (分区表)                                   │    │    │
│  │  │  - fills (成交记录) - 按天分区                       │    │    │
│  │  │  - candles (K线) - 按月分区                          │    │    │
│  │  │  - funding_rates (资金费率) - 按月分区               │    │    │
│  │  └─────────────────────────────────────────────────────┘    │    │
│  │                                                               │    │
│  │  ┌─────────────────────────────────────────────────────┐    │    │
│  │  │  分析数据 (物化视图)                                 │    │    │
│  │  │  - user_trading_stats (用户交易统计)                 │    │    │
│  │  │  - market_daily_stats (市场日统计)                   │    │    │
│  │  │  - candles_5m, candles_15m... (聚合K线)              │    │    │
│  │  └─────────────────────────────────────────────────────┘    │    │
│  │                                                               │    │
│  │  写入频率: 50,000+ rows/s (批量)                             │    │
│  │  查询延迟: <50ms (热数据), <200ms (历史)                     │    │
│  │  数据保留: 可配置分区保留策略                                 │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.2 Redis 数据结构设计

(与原文档相同)

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

### 4.3 PostgreSQL 数据模型

#### 4.3.1 OLTP 表 (普通表)

**订单表**:
```sql
CREATE TABLE orders (
    id TEXT PRIMARY KEY,
    market_id TEXT NOT NULL,
    owner TEXT NOT NULL,
    side TEXT NOT NULL,
    order_type TEXT NOT NULL,
    time_in_force TEXT NOT NULL,
    price NUMERIC(38, 18) NOT NULL,
    quantity NUMERIC(38, 18) NOT NULL,
    filled_quantity NUMERIC(38, 18) NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    reduce_only BOOLEAN NOT NULL DEFAULT FALSE,
    trigger_price NUMERIC(38, 18),
    tx_digest TEXT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

-- 索引
CREATE INDEX idx_orders_owner_status ON orders (owner, status, created_at DESC);
CREATE INDEX idx_orders_market_status ON orders (market_id, status);
CREATE INDEX idx_orders_checkpoint ON orders (checkpoint_sequence);
```

**仓位表**:
```sql
CREATE TABLE perpetual_positions (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    market_id TEXT NOT NULL,
    side TEXT NOT NULL,
    status TEXT NOT NULL,
    size NUMERIC(38, 18) NOT NULL,
    entry_price NUMERIC(38, 18) NOT NULL,
    margin NUMERIC(38, 18) NOT NULL,
    leverage NUMERIC(10, 2) NOT NULL,
    unrealized_pnl NUMERIC(38, 18),
    realized_pnl NUMERIC(38, 18) NOT NULL DEFAULT 0,
    liquidation_price NUMERIC(38, 18),
    settled_funding NUMERIC(38, 18) NOT NULL DEFAULT 0,
    checkpoint_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    closed_at TIMESTAMPTZ
);

-- 索引
CREATE INDEX idx_positions_owner ON perpetual_positions (owner, market_id, status);
CREATE INDEX idx_positions_market ON perpetual_positions (market_id, status);
```

#### 4.3.2 时序表 (分区表)

**成交记录表 (按天分区)**:
```sql
CREATE TABLE fills (
    id BIGSERIAL,
    market_id TEXT NOT NULL,
    tx_digest TEXT NOT NULL,
    maker_order_id TEXT NOT NULL,
    taker_order_id TEXT NOT NULL,
    maker_address TEXT NOT NULL,
    taker_address TEXT NOT NULL,
    side TEXT NOT NULL,
    price NUMERIC(38, 18) NOT NULL,
    quantity NUMERIC(38, 18) NOT NULL,
    quote_quantity NUMERIC(38, 18) NOT NULL,
    maker_fee NUMERIC(38, 18) NOT NULL,
    taker_fee NUMERIC(38, 18) NOT NULL,
    liquidity TEXT NOT NULL,
    fill_type TEXT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

-- 创建分区 (需要定期创建新分区)
CREATE TABLE fills_2024_01 PARTITION OF fills
    FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');
CREATE TABLE fills_2024_02 PARTITION OF fills
    FOR VALUES FROM ('2024-02-01') TO ('2024-03-01');
-- ... 持续创建

-- 索引 (在各分区上自动创建)
CREATE INDEX idx_fills_market ON fills (market_id, created_at DESC);
CREATE INDEX idx_fills_maker ON fills (maker_address, created_at DESC);
CREATE INDEX idx_fills_taker ON fills (taker_address, created_at DESC);
```

**K线表 (按月分区)**:
```sql
CREATE TABLE candles (
    market_id TEXT NOT NULL,
    resolution TEXT NOT NULL,
    bucket TIMESTAMPTZ NOT NULL,
    open NUMERIC(38, 18) NOT NULL,
    high NUMERIC(38, 18) NOT NULL,
    low NUMERIC(38, 18) NOT NULL,
    close NUMERIC(38, 18) NOT NULL,
    volume NUMERIC(38, 18) NOT NULL,
    quote_volume NUMERIC(38, 18) NOT NULL,
    trade_count INTEGER NOT NULL,
    PRIMARY KEY (market_id, resolution, bucket)
) PARTITION BY RANGE (bucket);

-- 创建分区
CREATE TABLE candles_2024_01 PARTITION OF candles
    FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');
-- ... 持续创建

-- BRIN 索引 (适合时序数据)
CREATE INDEX idx_candles_bucket_brin ON candles USING BRIN (bucket);
```

**资金费率表 (按月分区)**:
```sql
CREATE TABLE funding_rates (
    market_id TEXT NOT NULL,
    rate NUMERIC(38, 18) NOT NULL,
    mark_price NUMERIC(38, 18) NOT NULL,
    index_price NUMERIC(38, 18) NOT NULL,
    funding_index NUMERIC(38, 18) NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (market_id, timestamp)
) PARTITION BY RANGE (timestamp);

-- 创建分区
CREATE TABLE funding_rates_2024_01 PARTITION OF funding_rates
    FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');
```

#### 4.3.3 分析视图 (物化视图)

**K线聚合视图 (5分钟)**:
```sql
CREATE MATERIALIZED VIEW candles_5m AS
SELECT
    market_id,
    '5m' AS resolution,
    date_trunc('5 minutes', bucket) AS bucket,
    (array_agg(open ORDER BY bucket ASC))[1] AS open,
    max(high) AS high,
    min(low) AS low,
    (array_agg(close ORDER BY bucket DESC))[1] AS close,
    sum(volume) AS volume,
    sum(quote_volume) AS quote_volume,
    sum(trade_count) AS trade_count
FROM candles
WHERE resolution = '1m'
GROUP BY market_id, date_trunc('5 minutes', bucket);

-- 唯一索引 (支持并发刷新)
CREATE UNIQUE INDEX idx_candles_5m_pk ON candles_5m (market_id, bucket);
```

**K线聚合视图 (15分钟, 1小时, 4小时, 1天)**:
```sql
-- 15分钟
CREATE MATERIALIZED VIEW candles_15m AS
SELECT
    market_id, '15m' AS resolution,
    date_trunc('15 minutes', bucket) AS bucket,
    (array_agg(open ORDER BY bucket ASC))[1] AS open,
    max(high) AS high, min(low) AS low,
    (array_agg(close ORDER BY bucket DESC))[1] AS close,
    sum(volume) AS volume, sum(quote_volume) AS quote_volume,
    sum(trade_count) AS trade_count
FROM candles WHERE resolution = '1m'
GROUP BY market_id, date_trunc('15 minutes', bucket);

CREATE UNIQUE INDEX idx_candles_15m_pk ON candles_15m (market_id, bucket);

-- 1小时
CREATE MATERIALIZED VIEW candles_1h AS
SELECT
    market_id, '1h' AS resolution,
    date_trunc('1 hour', bucket) AS bucket,
    (array_agg(open ORDER BY bucket ASC))[1] AS open,
    max(high) AS high, min(low) AS low,
    (array_agg(close ORDER BY bucket DESC))[1] AS close,
    sum(volume) AS volume, sum(quote_volume) AS quote_volume,
    sum(trade_count) AS trade_count
FROM candles WHERE resolution = '1m'
GROUP BY market_id, date_trunc('1 hour', bucket);

CREATE UNIQUE INDEX idx_candles_1h_pk ON candles_1h (market_id, bucket);

-- 4小时
CREATE MATERIALIZED VIEW candles_4h AS
SELECT
    market_id, '4h' AS resolution,
    date_trunc('4 hours', bucket) AS bucket,
    (array_agg(open ORDER BY bucket ASC))[1] AS open,
    max(high) AS high, min(low) AS low,
    (array_agg(close ORDER BY bucket DESC))[1] AS close,
    sum(volume) AS volume, sum(quote_volume) AS quote_volume,
    sum(trade_count) AS trade_count
FROM candles WHERE resolution = '1m'
GROUP BY market_id, date_trunc('4 hours', bucket);

CREATE UNIQUE INDEX idx_candles_4h_pk ON candles_4h (market_id, bucket);

-- 1天
CREATE MATERIALIZED VIEW candles_1d AS
SELECT
    market_id, '1d' AS resolution,
    date_trunc('1 day', bucket) AS bucket,
    (array_agg(open ORDER BY bucket ASC))[1] AS open,
    max(high) AS high, min(low) AS low,
    (array_agg(close ORDER BY bucket DESC))[1] AS close,
    sum(volume) AS volume, sum(quote_volume) AS quote_volume,
    sum(trade_count) AS trade_count
FROM candles WHERE resolution = '1m'
GROUP BY market_id, date_trunc('1 day', bucket);

CREATE UNIQUE INDEX idx_candles_1d_pk ON candles_1d (market_id, bucket);
```

**用户交易统计视图**:
```sql
CREATE MATERIALIZED VIEW user_trading_stats AS
SELECT
    taker_address AS address,
    market_id,
    date_trunc('day', created_at) AS trade_date,
    count(*) AS trade_count,
    sum(quote_quantity) AS total_volume,
    sum(taker_fee) AS total_fees
FROM fills
GROUP BY taker_address, market_id, date_trunc('day', created_at);

CREATE UNIQUE INDEX idx_user_stats_pk ON user_trading_stats (address, market_id, trade_date);
CREATE INDEX idx_user_stats_address ON user_trading_stats (address);
```

**市场日统计视图**:
```sql
CREATE MATERIALIZED VIEW market_daily_stats AS
SELECT
    market_id,
    date_trunc('day', created_at) AS trade_date,
    count(*) AS trade_count,
    sum(quantity) AS total_quantity,
    sum(quote_quantity) AS total_volume,
    count(DISTINCT maker_address) + count(DISTINCT taker_address) AS unique_traders
FROM fills
GROUP BY market_id, date_trunc('day', created_at);

CREATE UNIQUE INDEX idx_market_stats_pk ON market_daily_stats (market_id, trade_date);
```

---

## 5. PostgreSQL 特有优化

### 5.1 分区管理

**自动创建分区 (pg_partman 扩展)**:
```sql
-- 安装扩展
CREATE EXTENSION pg_partman;

-- 配置自动分区
SELECT partman.create_parent(
    p_parent_table => 'public.fills',
    p_control => 'created_at',
    p_type => 'native',
    p_interval => '1 day',
    p_premake => 7  -- 预创建 7 天分区
);

-- 设置保留策略 (保留 90 天)
UPDATE partman.part_config
SET retention = '90 days', retention_keep_table = false
WHERE parent_table = 'public.fills';

-- 定时维护 (pg_cron)
SELECT cron.schedule('partition_maintenance', '0 3 * * *',
    $$CALL partman.run_maintenance_proc()$$);
```

### 5.2 物化视图刷新

**定时刷新任务 (pg_cron)**:
```sql
-- 安装扩展
CREATE EXTENSION pg_cron;

-- K线聚合刷新 (每分钟)
SELECT cron.schedule('refresh_candles_5m', '* * * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY candles_5m');
SELECT cron.schedule('refresh_candles_15m', '* * * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY candles_15m');

-- 小时级刷新
SELECT cron.schedule('refresh_candles_1h', '0 * * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY candles_1h');
SELECT cron.schedule('refresh_candles_4h', '0 */4 * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY candles_4h');

-- 日级刷新
SELECT cron.schedule('refresh_candles_1d', '5 0 * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY candles_1d');
SELECT cron.schedule('refresh_user_stats', '10 0 * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY user_trading_stats');
SELECT cron.schedule('refresh_market_stats', '15 0 * * *',
    'REFRESH MATERIALIZED VIEW CONCURRENTLY market_daily_stats');
```

### 5.3 批量写入优化

**使用 COPY 批量插入**:
```rust
// Rust 代码示例
pub async fn batch_insert_fills(
    client: &Client,
    fills: &[Fill],
) -> Result<()> {
    let mut writer = client
        .copy_in("COPY fills (market_id, tx_digest, ...) FROM STDIN WITH (FORMAT binary)")
        .await?;

    for fill in fills {
        writer.write(&fill.to_binary()).await?;
    }

    writer.finish().await?;
    Ok(())
}
```

**批量 UPSERT K线**:
```sql
-- 使用 INSERT ... ON CONFLICT 批量更新K线
INSERT INTO candles (market_id, resolution, bucket, open, high, low, close, volume, quote_volume, trade_count)
VALUES
    ($1, $2, $3, $4, $4, $4, $4, $5, $6, 1),
    ($7, $8, $9, $10, $10, $10, $10, $11, $12, 1)
    -- ... 更多行
ON CONFLICT (market_id, resolution, bucket) DO UPDATE SET
    high = GREATEST(candles.high, EXCLUDED.high),
    low = LEAST(candles.low, EXCLUDED.low),
    close = EXCLUDED.close,
    volume = candles.volume + EXCLUDED.volume,
    quote_volume = candles.quote_volume + EXCLUDED.quote_volume,
    trade_count = candles.trade_count + 1;
```

### 5.4 查询优化

**分区裁剪**:
```sql
-- 查询自动裁剪到相关分区
EXPLAIN ANALYZE
SELECT * FROM fills
WHERE created_at >= '2024-01-15' AND created_at < '2024-01-16'
AND market_id = 'BTC-USDC';

-- 结果: 只扫描 fills_2024_01 分区
```

**BRIN 索引 (时序数据)**:
```sql
-- BRIN 索引适合时序数据，占用空间小
CREATE INDEX idx_fills_created_brin ON fills USING BRIN (created_at);

-- 查询时自动使用 BRIN 索引
SELECT * FROM fills WHERE created_at > NOW() - INTERVAL '1 hour';
```

---

## 6. 处理服务设计

(与原文档类似，调整存储目标为 PostgreSQL)

### 6.1 Orderbook Processor (订单簿处理器)

**职责**: 消费订单事件，维护 Redis 订单簿

(与原文档相同)

### 6.2 Trade Processor (成交处理器)

**职责**: 处理成交事件，更新 K线和成交记录

```rust
pub struct TradeProcessor {
    postgres: PgPool,
    redis: RedisConnection,
}

impl TradeProcessor {
    pub async fn process_trade(&self, trade: &TradeEvent) -> Result<()> {
        // 1. 写入 PostgreSQL 成交记录
        sqlx::query!(
            r#"
            INSERT INTO fills (market_id, tx_digest, price, quantity, ...)
            VALUES ($1, $2, $3, $4, ...)
            "#,
            trade.market_id,
            trade.tx_digest,
            trade.price,
            trade.quantity,
            // ...
        )
        .execute(&self.postgres)
        .await?;

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
        sqlx::query!(
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
            market_id, resolution, bucket, trade.price, trade.quantity, trade.quote_quantity
        )
        .execute(&self.postgres)
        .await?;

        Ok(())
    }
}
```

### 6.3 History Processor (历史数据处理器)

**职责**: 处理 Checkpoint 数据，写入 PostgreSQL

```rust
pub struct HistoryProcessor {
    postgres: PgPool,
}

impl HistoryProcessor {
    pub async fn process_checkpoint_batch(&self, events: &[DexEvent]) -> Result<()> {
        // 批量写入订单
        let orders: Vec<_> = events.iter().filter_map(|e| e.as_order_event()).collect();
        if !orders.is_empty() {
            self.batch_insert_orders(&orders).await?;
        }

        // 批量写入仓位更新
        let positions: Vec<_> = events.iter().filter_map(|e| e.as_position_event()).collect();
        if !positions.is_empty() {
            self.batch_upsert_positions(&positions).await?;
        }

        Ok(())
    }
}
```

---

## 7. API 服务设计

(与原文档相同)

### 7.1 REST API 端点

| 端点 | 方法 | 数据来源 | 说明 |
|-----|------|---------|------|
| `/v1/orderbook/{market_id}` | GET | Redis | 获取订单簿 |
| `/v1/trades/{market_id}` | GET | PostgreSQL | 获取最近成交 |
| `/v1/candles/{market_id}` | GET | PostgreSQL | 获取K线数据 |
| `/v1/orders` | GET | Redis + PostgreSQL | 获取用户订单 |
| `/v1/positions/{address}` | GET | Redis | 获取用户仓位 |
| `/v1/funding-rates/{market_id}` | GET | PostgreSQL | 获取资金费率历史 |
| `/v1/markets` | GET | 内存缓存 | 获取市场列表 |
| `/v1/ticker/{market_id}` | GET | Redis | 获取24小时行情 |

### 7.2 WebSocket 订阅频道

(与原文档相同)

---

## 8. 高性能优化策略

### 8.1 写入优化

| 组件 | 优化策略 | 预期吞吐量 |
|-----|---------|----------|
| Redis | Pipeline 批量写入 | 100,000+ ops/s |
| PostgreSQL | COPY 批量插入 | 50,000+ rows/s |
| PostgreSQL | 批量 UPSERT | 20,000+ rows/s |

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
    ├── 近期K线 (7天内) → PostgreSQL 分区表 (<20ms)
    │
    └── 历史K线 (7天外) → PostgreSQL 分区表 (<100ms)

分析数据请求
    │
    └── 统计/聚合 → PostgreSQL 物化视图 (<200ms)
```

### 8.3 缓存策略

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
│  │  L3: PostgreSQL │  K线、成交、历史数据              │
│  │   (分区表)      │  热数据: <50ms, 冷数据: <200ms    │
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
│  │  Realtime Listener × 2         Checkpoint Processor × 2      │    │
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
│  │  Redis Cluster (6节点)         PostgreSQL (主从 + 读副本)    │    │
│  │  Kafka Cluster (3节点)                                        │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 9.2 资源估算

| 组件 | CPU | 内存 | 存储 | 网络 |
|-----|-----|------|------|------|
| Realtime Listener | 2核 | 4GB | 10GB | 1Gbps |
| Checkpoint Processor | 4核 | 8GB | 100GB | 1Gbps |
| Orderbook Processor | 4核 | 8GB | 10GB | 1Gbps |
| Trade Processor | 4核 | 8GB | 10GB | 1Gbps |
| REST API | 4核 | 8GB | 10GB | 10Gbps |
| WebSocket Server | 8核 | 16GB | 10GB | 10Gbps |
| Redis Cluster (6节点) | 4核×6 | 32GB×6 | 100GB×6 | 10Gbps |
| PostgreSQL 主节点 | 16核 | 64GB | 2TB SSD | 10Gbps |
| PostgreSQL 读副本 | 8核 | 32GB | 2TB SSD | 10Gbps |

---

## 10. 关键技术选型总结

| 组件 | 选型 | 选择原因 |
|-----|------|---------|
| 实时缓存 | Redis Cluster | 低延迟、复杂数据结构、Lua 脚本 |
| 持久化存储 | PostgreSQL | 成熟稳定、分区表、物化视图、生态丰富 |
| 消息队列 | Redis Stream + Kafka | 实时 + 批量双通道 |
| API 框架 | Axum (Rust) | 高性能、类型安全、异步 |
| WebSocket | tokio-tungstenite | Rust 原生、高性能 |

### 与原方案对比

| 维度 | 原方案 | PostgreSQL 方案 |
|-----|-------|----------------|
| 数据库数量 | 3 (Redis + TimescaleDB + ClickHouse) | 2 (Redis + PostgreSQL) |
| 运维复杂度 | 高 | 中 |
| 时序写入性能 | 100,000+ rows/s | 50,000+ rows/s |
| OLAP 性能 | 极高 (ClickHouse) | 中等 (物化视图) |
| 成本 | 高 | 中 |
| 适用规模 | 大型 DEX | 中小型 DEX |

---

## 11. 参考资料

- 原版设计文档: `sui/mynotes/dex/analyst/dex-indexer-analyst.md`
- PostgreSQL 分区文档: https://www.postgresql.org/docs/current/ddl-partitioning.html
- pg_partman 文档: https://github.com/pgpartman/pg_partman
- pg_cron 文档: https://github.com/citusdata/pg_cron
