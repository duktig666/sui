# DEX Indexer 技术方案 V2

> 分析日期: 2026-01-29
> 核心问题: 借鉴 sui-indexer-alt 架构实现 Checkpoint-Only 的 OnChainUpdates

## 1. 背景与决策

### 1.1 方案选择

根据 `dex-indexer-full-by-dydx-analysis.md` 分析，DEX 采用 **方案 A（纯 Checkpoint）** 处理 OnChainUpdates：

| 方案 | 延迟 | 复杂度 | 数据一致性 |
|------|------|--------|-----------|
| **方案 A: 纯 Checkpoint** ✅ | ~700ms+ | 低 | 最高 |
| 方案 B: 双层设计 | Optimistic ~400ms | 中 | 需状态转换 |

**选择理由**：
- 实现简单，维护成本低
- 数据一致性最高（单一来源）
- 不需要处理回滚情况
- API 设计简单（单一事件流）

### 1.2 sui-indexer-alt 可借鉴性分析

**❌ 不能直接使用**：sui-indexer-alt 从 Checkpoint 中提取 **Move 事件**，而原生 Rust DEX 引擎的事件是应用层事件。

**✅ 可以借鉴的架构模式**：
- Pipeline 架构（Processor → Collector → Committer → Watermark → Pruner）
- gRPC Streaming 连接模式
- 批量写入与背压机制
- Watermark 进度追踪
- 幂等写入保证

---

## 2. 系统架构

### 2.1 整体架构图

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        原生 Rust DEX 引擎                                │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐      ┌──────────────┐                                 │
│  │  撮合引擎    │ ──►  │ OnChainEvents│ ───── gRPC Stream ─────►       │
│  │ (Matching)   │      │  (Checkpoint) │                     │          │
│  └──────────────┘      └──────────────┘                      │          │
│                                                               │          │
│  ┌──────────────┐      ┌──────────────┐                      │          │
│  │ OffChainEvents│ ──►  │ WebSocket    │ ───── 实时推送 ─────┼──►      │
│  │ (订单状态)   │      │  Server      │                      │          │
│  └──────────────┘      └──────────────┘                      │          │
└──────────────────────────────────────────────────────────────┼──────────┘
                                                               │
                                                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     DEX Indexer（借鉴 sui-indexer-alt）                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌────────────────────┐   借鉴 streaming_client.rs                      │
│  │ DexEventStreamClient│  - gRPC 连接管理                                │
│  │ (事件流订阅)        │  - 自动重连                                     │
│  └─────────┬──────────┘                                                  │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────┐   借鉴 pipeline/concurrent/mod.rs               │
│  │     Processor      │  - 事件解析                                      │
│  │ (FANOUT=N 并发)    │  - 类型转换                                      │
│  └─────────┬──────────┘                                                  │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────┐   借鉴 collector.rs                             │
│  │     Collector      │  - MIN_EAGER_ROWS=50 批量阈值                    │
│  │ (批量收集)         │  - MAX_PENDING_ROWS=5000 背压控制                │
│  └─────────┬──────────┘                                                  │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────┐   借鉴 committer.rs                             │
│  │     Committer      │  - 指数退避重试                                  │
│  │ (并发写入)         │  - on_conflict_do_nothing 幂等                   │
│  └─────────┬──────────┘                                                  │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────┐   借鉴 commit_watermark.rs                      │
│  │  CommitWatermark   │  - checkpoint_hi_inclusive 进度追踪              │
│  │ (进度追踪)         │  - reader_lo 读取水位                            │
│  └─────────┬──────────┘                                                  │
│            │                                                             │
│            ▼                                                             │
│  ┌────────────────────┐   借鉴 pruner.rs                                │
│  │      Pruner        │  - retention 数据保留策略                        │
│  │ (数据清理)         │  - max_chunk_size 分批删除                       │
│  └────────────────────┘                                                  │
│                                                                          │
└───────────────────────────────────────┬─────────────────────────────────┘
                                        │
                                        ▼
                              ┌──────────────────┐
                              │   PostgreSQL     │
                              └────────┬─────────┘
                                       │
                                       ▼
                              ┌──────────────────┐
                              │   REST API       │
                              │   (Axum)         │
                              └──────────────────┘
```

### 2.2 数据流分类

| 通道 | 延迟 | 数据内容 | 存储 |
|------|------|---------|------|
| **OffChainUpdates** | <10ms | 订单状态 (Place/Update/Remove) | Redis + WebSocket |
| **OnChainUpdates** | ~700ms+ | Fills、Positions、Balances、Transfers | PostgreSQL |

---

## 3. 借鉴 sui-indexer-alt 的关键组件

### 3.1 Pipeline 架构模式

**源码参考**: `sui-indexer-alt-framework/src/pipeline/concurrent/mod.rs:206-288`

```rust
// 关键接口设计（可直接借鉴）
pub trait Handler: Processor {
    type Store: Store;
    type Batch: Default + Send + Sync + 'static;

    /// 批量提交阈值：积攒够多少行才提交
    const MIN_EAGER_ROWS: usize = 50;

    /// 背压阈值：达到此阈值时暂停上游处理
    const MAX_PENDING_ROWS: usize = 5000;

    /// 单批次最大 Watermark 更新数
    const MAX_WATERMARK_UPDATES: usize = 10_000;

    /// 添加数据到批次
    fn batch(&self, batch: &mut Self::Batch, values: &mut IntoIter<Self::Value>) -> BatchStatus;

    /// 提交批次到数据库
    async fn commit<'a>(&self, batch: &Self::Batch, conn: &mut Connection<'a>) -> Result<usize>;

    /// 清理旧数据
    async fn prune<'a>(&self, from: u64, to_exclusive: u64, conn: &mut Connection<'a>) -> Result<usize>;
}
```

### 3.2 gRPC Streaming 客户端

**源码参考**: `sui-indexer-alt-framework/src/ingestion/streaming_client.rs:26-86`

```rust
/// DEX 事件流客户端 trait（借鉴 CheckpointStreamingClient）
#[async_trait]
pub trait DexEventStreamingClient {
    async fn connect(&mut self) -> Result<DexEventStream>;
}

/// gRPC 客户端实现
pub struct GrpcDexEventClient {
    uri: Uri,
    connection_timeout: Duration,
}

#[async_trait]
impl DexEventStreamingClient for GrpcDexEventClient {
    async fn connect(&mut self) -> Result<DexEventStream> {
        let endpoint = Endpoint::from(self.uri.clone())
            .connect_timeout(self.connection_timeout);

        let mut client = DexEventServiceClient::connect(endpoint)
            .await
            .map_err(|err| Error::RpcClientError(Status::from_error(err.into())))?
            .max_decoding_message_size(MAX_GRPC_MESSAGE_SIZE_BYTES);

        let stream = client
            .subscribe_events(SubscribeEventsRequest::default())
            .await
            .map_err(Error::RpcClientError)?
            .into_inner();

        // 转换为内部类型的 Stream
        let converted_stream = stream.map(|result| match result {
            Ok(response) => response
                .event
                .context("Event data missing in response")
                .and_then(|event| DexEvent::try_from(&event))
                .map_err(Error::StreamingError),
            Err(e) => Err(Error::RpcClientError(e)),
        });

        Ok(Box::pin(converted_stream))
    }
}
```

### 3.3 批量收集与背压机制

**源码参考**: `sui-indexer-alt-framework/src/pipeline/concurrent/collector.rs`

```rust
/// Collector 配置（借鉴 sui-indexer-alt）
pub struct CollectorConfig {
    /// 收集间隔（毫秒）
    pub collect_interval_ms: u64,  // 默认 500ms

    /// 批量提交阈值
    pub min_eager_rows: usize,     // 默认 50

    /// 背压阈值
    pub max_pending_rows: usize,   // 默认 5000
}

/// 批量触发条件
async fn collect_batch(&mut self) {
    // 条件 1: 数据量足够，立即触发
    if self.pending_rows >= self.config.min_eager_rows {
        self.flush_batch().await;
    }

    // 条件 2: 定时触发
    if self.poll.tick().await {
        self.flush_batch().await;
    }

    // 条件 3: 背压控制
    if self.pending_rows >= self.config.max_pending_rows {
        // 暂停接收新数据，形成背压
        self.pause_receiving();
    }
}
```

### 3.4 指数退避重试机制

**源码参考**: `sui-indexer-alt-framework/src/pipeline/concurrent/committer.rs:58-63`

```rust
/// 重试配置
const INITIAL_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// 带重试的提交
async fn commit_with_retry<H: Handler>(
    handler: &H,
    batch: &H::Batch,
    conn: &mut Connection<'_>,
) -> Result<usize> {
    backoff::future::retry(
        backoff::ExponentialBackoff {
            initial_interval: INITIAL_RETRY_INTERVAL,
            max_interval: MAX_RETRY_INTERVAL,
            max_elapsed_time: None,  // 永不放弃
            ..Default::default()
        },
        || async {
            handler.commit(batch, conn)
                .await
                .map_err(backoff::Error::transient)
        }
    ).await
}
```

### 3.5 Watermark 进度追踪

**源码参考**: `sui-indexer-alt-schema/src/schema.rs:231-241`

```sql
-- Watermark 表结构设计（借鉴 sui-indexer-alt）
CREATE TABLE dex_watermarks (
    pipeline TEXT PRIMARY KEY,                    -- Pipeline 名称
    epoch_hi_inclusive BIGINT NOT NULL,           -- 已处理的最高 epoch
    checkpoint_hi_inclusive BIGINT NOT NULL,      -- 已提交的最高 checkpoint
    tx_hi BIGINT NOT NULL,                        -- 已处理的最高交易序号
    timestamp_ms_hi_inclusive BIGINT NOT NULL,    -- 最高时间戳
    reader_lo BIGINT NOT NULL,                    -- 可安全读取的最低点
    pruner_timestamp TIMESTAMP NOT NULL,          -- 上次 prune 时间
    pruner_hi BIGINT NOT NULL                     -- 已 prune 到的位置
);
```

### 3.6 幂等写入保证

**源码参考**: `sui-indexer-alt/src/handlers/kv_transactions.rs:76-80`

```rust
/// 使用 on_conflict_do_nothing 保证幂等性
async fn commit<'a>(&self, batch: &Self::Batch, conn: &mut Connection<'a>) -> Result<usize> {
    Ok(diesel::insert_into(dex_fills::table)
        .values(batch)
        .on_conflict_do_nothing()   // 冲突时忽略，保证幂等
        .execute(conn)
        .await?)
}
```

---

## 4. DEX 事件定义

### 4.1 OnChainUpdates 事件（Checkpoint 时发送）

```rust
/// 成交事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillEvent {
    pub fill_id: u64,
    pub market_id: u64,
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub maker_address: String,
    pub taker_address: String,
    pub side: Side,               // Buy / Sell
    pub price: Decimal,
    pub quantity: Decimal,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

/// 仓位变更事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionUpdateEvent {
    pub position_id: u64,
    pub owner: String,
    pub market_id: u64,
    pub size: Decimal,            // 正数=多头，负数=空头
    pub entry_price: Decimal,
    pub leverage: Decimal,
    pub margin: Decimal,
    pub unrealized_pnl: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

/// 余额变更事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceUpdateEvent {
    pub owner: String,
    pub asset: String,
    pub balance: Decimal,
    pub available: Decimal,
    pub locked: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

/// 资金费率事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingRateEvent {
    pub market_id: u64,
    pub rate: Decimal,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}

/// 清算事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidationEvent {
    pub position_id: u64,
    pub owner: String,
    pub market_id: u64,
    pub size: Decimal,
    pub price: Decimal,
    pub liquidator: String,
    pub timestamp_ms: u64,
    pub checkpoint_sequence: u64,
}
```

### 4.2 OffChainUpdates 事件（实时发送，不经过索引器）

```rust
/// 订单放置事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderPlaceEvent {
    pub order_id: u64,
    pub market_id: u64,
    pub owner: String,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Decimal,
    pub quantity: Decimal,
    pub timestamp_ms: u64,
}

/// 订单更新事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderUpdateEvent {
    pub order_id: u64,
    pub filled_quantity: Decimal,
    pub remaining_quantity: Decimal,
    pub status: OrderStatus,
    pub timestamp_ms: u64,
}

/// 订单移除事件
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderRemoveEvent {
    pub order_id: u64,
    pub reason: OrderRemoveReason,  // Filled / Canceled / Expired / PostOnlyRejected
    pub timestamp_ms: u64,
}
```

---

## 5. 存储设计

### 5.1 表结构

```sql
-- 市场配置表
CREATE TABLE markets (
    id BIGSERIAL PRIMARY KEY,
    symbol VARCHAR(32) NOT NULL UNIQUE,
    base_asset VARCHAR(32) NOT NULL,
    quote_asset VARCHAR(32) NOT NULL,
    price_precision INT NOT NULL,
    quantity_precision INT NOT NULL,
    min_order_size DECIMAL(36, 18) NOT NULL,
    max_leverage DECIMAL(10, 2),
    maker_fee DECIMAL(10, 6) NOT NULL,
    taker_fee DECIMAL(10, 6) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'active',
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- 成交记录表（按天分区）
CREATE TABLE fills (
    id BIGSERIAL,
    fill_id BIGINT NOT NULL,
    market_id BIGINT NOT NULL,
    maker_order_id BIGINT NOT NULL,
    taker_order_id BIGINT NOT NULL,
    maker_address VARCHAR(66) NOT NULL,
    taker_address VARCHAR(66) NOT NULL,
    side VARCHAR(4) NOT NULL,
    price DECIMAL(36, 18) NOT NULL,
    quantity DECIMAL(36, 18) NOT NULL,
    maker_fee DECIMAL(36, 18) NOT NULL,
    taker_fee DECIMAL(36, 18) NOT NULL,
    timestamp_ms BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, timestamp_ms)
) PARTITION BY RANGE (timestamp_ms);

-- 仓位表
CREATE TABLE perpetual_positions (
    id BIGSERIAL PRIMARY KEY,
    position_id BIGINT NOT NULL UNIQUE,
    owner VARCHAR(66) NOT NULL,
    market_id BIGINT NOT NULL,
    size DECIMAL(36, 18) NOT NULL,
    entry_price DECIMAL(36, 18) NOT NULL,
    leverage DECIMAL(10, 2) NOT NULL,
    margin DECIMAL(36, 18) NOT NULL,
    unrealized_pnl DECIMAL(36, 18) NOT NULL,
    liquidation_price DECIMAL(36, 18),
    status VARCHAR(16) NOT NULL DEFAULT 'open',
    opened_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    checkpoint_sequence BIGINT NOT NULL,
    UNIQUE (owner, market_id)
);

-- K线表（按月分区）
CREATE TABLE candles (
    id BIGSERIAL,
    market_id BIGINT NOT NULL,
    interval VARCHAR(8) NOT NULL,  -- 1m, 5m, 15m, 1h, 4h, 1d
    open_time BIGINT NOT NULL,
    open DECIMAL(36, 18) NOT NULL,
    high DECIMAL(36, 18) NOT NULL,
    low DECIMAL(36, 18) NOT NULL,
    close DECIMAL(36, 18) NOT NULL,
    volume DECIMAL(36, 18) NOT NULL,
    quote_volume DECIMAL(36, 18) NOT NULL,
    trade_count INT NOT NULL,
    close_time BIGINT NOT NULL,
    PRIMARY KEY (id, open_time)
) PARTITION BY RANGE (open_time);

-- 资金费率历史表（按月分区）
CREATE TABLE funding_rates (
    id BIGSERIAL,
    market_id BIGINT NOT NULL,
    rate DECIMAL(18, 10) NOT NULL,
    mark_price DECIMAL(36, 18) NOT NULL,
    index_price DECIMAL(36, 18) NOT NULL,
    timestamp_ms BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    PRIMARY KEY (id, timestamp_ms)
) PARTITION BY RANGE (timestamp_ms);

-- 进度追踪表
CREATE TABLE dex_watermarks (
    pipeline TEXT PRIMARY KEY,
    epoch_hi_inclusive BIGINT NOT NULL DEFAULT 0,
    checkpoint_hi_inclusive BIGINT NOT NULL DEFAULT 0,
    tx_hi BIGINT NOT NULL DEFAULT 0,
    timestamp_ms_hi_inclusive BIGINT NOT NULL DEFAULT 0,
    reader_lo BIGINT NOT NULL DEFAULT 0,
    pruner_timestamp TIMESTAMP NOT NULL DEFAULT NOW(),
    pruner_hi BIGINT NOT NULL DEFAULT 0
);
```

### 5.2 索引设计

```sql
-- fills 表索引
CREATE INDEX idx_fills_market_timestamp ON fills (market_id, timestamp_ms DESC);
CREATE INDEX idx_fills_maker_address ON fills (maker_address, timestamp_ms DESC);
CREATE INDEX idx_fills_taker_address ON fills (taker_address, timestamp_ms DESC);
CREATE INDEX idx_fills_checkpoint ON fills (checkpoint_sequence);

-- perpetual_positions 表索引
CREATE INDEX idx_positions_owner ON perpetual_positions (owner);
CREATE INDEX idx_positions_market ON perpetual_positions (market_id);
CREATE INDEX idx_positions_status ON perpetual_positions (status) WHERE status = 'open';

-- candles 表索引
CREATE INDEX idx_candles_market_interval ON candles (market_id, interval, open_time DESC);

-- funding_rates 表索引
CREATE INDEX idx_funding_market_time ON funding_rates (market_id, timestamp_ms DESC);
```

---

## 6. Handler 实现示例

### 6.1 FillsHandler

```rust
use async_trait::async_trait;
use diesel::prelude::*;

pub struct FillsHandler {
    config: HandlerConfig,
}

impl FillsHandler {
    pub fn new(config: HandlerConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Processor for FillsHandler {
    const NAME: &'static str = "dex_fills";
    const FANOUT: usize = 2;  // 并发处理数
    type Value = StoredFill;

    async fn process(&self, event_batch: &DexEventBatch) -> Result<Vec<Self::Value>> {
        event_batch.events.iter()
            .filter_map(|event| {
                if let DexEvent::Fill(fill) = event {
                    Some(StoredFill::from(fill))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[async_trait]
impl Handler for FillsHandler {
    type Store = PgStore;
    type Batch = Vec<StoredFill>;

    const MIN_EAGER_ROWS: usize = 100;     // 高吞吐场景
    const MAX_PENDING_ROWS: usize = 10000;

    fn batch(
        &self,
        batch: &mut Self::Batch,
        values: &mut std::vec::IntoIter<Self::Value>,
    ) -> BatchStatus {
        batch.extend(values);
        if batch.len() >= 1000 {
            BatchStatus::Ready
        } else {
            BatchStatus::Pending
        }
    }

    async fn commit<'a>(
        &self,
        batch: &Self::Batch,
        conn: &mut Connection<'a>,
    ) -> Result<usize> {
        // 幂等写入
        Ok(diesel::insert_into(fills::table)
            .values(batch)
            .on_conflict_do_nothing()
            .execute(conn)
            .await?)
    }

    async fn prune<'a>(
        &self,
        from: u64,
        to_exclusive: u64,
        conn: &mut Connection<'a>,
    ) -> Result<usize> {
        // 按 checkpoint_sequence 范围删除
        let filter = fills::table.filter(
            fills::checkpoint_sequence.between(from as i64, to_exclusive as i64 - 1)
        );
        Ok(diesel::delete(filter).execute(conn).await?)
    }
}
```

### 6.2 CandlesHandler（K线聚合）

```rust
pub struct CandlesHandler {
    intervals: Vec<CandleInterval>,  // 1m, 5m, 15m, 1h, 4h, 1d
}

#[async_trait]
impl Processor for CandlesHandler {
    const NAME: &'static str = "dex_candles";
    type Value = CandleUpdate;

    async fn process(&self, event_batch: &DexEventBatch) -> Result<Vec<Self::Value>> {
        let fills: Vec<_> = event_batch.events.iter()
            .filter_map(|e| if let DexEvent::Fill(f) = e { Some(f) } else { None })
            .collect();

        if fills.is_empty() {
            return Ok(vec![]);
        }

        let mut updates = Vec::new();

        // 按市场分组
        let fills_by_market = fills.into_iter()
            .fold(HashMap::new(), |mut acc, fill| {
                acc.entry(fill.market_id).or_insert_with(Vec::new).push(fill);
                acc
            });

        // 为每个时间间隔生成 K 线更新
        for (market_id, market_fills) in fills_by_market {
            for interval in &self.intervals {
                let candle_updates = self.aggregate_candles(market_id, &market_fills, interval);
                updates.extend(candle_updates);
            }
        }

        Ok(updates)
    }
}

impl CandlesHandler {
    fn aggregate_candles(
        &self,
        market_id: u64,
        fills: &[&FillEvent],
        interval: &CandleInterval,
    ) -> Vec<CandleUpdate> {
        let mut candles: HashMap<u64, CandleUpdate> = HashMap::new();

        for fill in fills {
            let bucket_start = self.get_bucket_start(fill.timestamp_ms, interval);

            let candle = candles.entry(bucket_start).or_insert_with(|| {
                CandleUpdate {
                    market_id,
                    interval: interval.clone(),
                    open_time: bucket_start,
                    open: fill.price,
                    high: fill.price,
                    low: fill.price,
                    close: fill.price,
                    volume: Decimal::ZERO,
                    quote_volume: Decimal::ZERO,
                    trade_count: 0,
                }
            });

            candle.high = candle.high.max(fill.price);
            candle.low = candle.low.min(fill.price);
            candle.close = fill.price;
            candle.volume += fill.quantity;
            candle.quote_volume += fill.quantity * fill.price;
            candle.trade_count += 1;
        }

        candles.into_values().collect()
    }

    fn get_bucket_start(&self, timestamp_ms: u64, interval: &CandleInterval) -> u64 {
        let interval_ms = interval.to_millis();
        (timestamp_ms / interval_ms) * interval_ms
    }
}
```

---

## 7. API 设计（对标 Hyperliquid）

Hyperliquid 采用 **POST 请求 + type 字段** 的设计模式，而非 RESTful 风格。DEX Indexer 遵循相同设计。

### 7.1 API 端点概览

| 端点 | 用途 | 签名 |
|------|------|------|
| `POST /info` | 查询数据（元数据、订单簿、持仓等） | 无需 |
| `POST /exchange` | 交易操作（下单、取消、修改等） | 需要 EIP-712 |

### 7.2 Info API（查询端点）

**端点**: `POST /info`
**Content-Type**: `application/json`

#### 市场数据查询

| type | 参数 | 说明 |
|------|------|------|
| `meta` | 无 | 永续合约元数据（交易对、精度、杠杆上限） |
| `metaAndAssetCtxs` | 无 | 永续元数据 + 实时数据（资金费率、价格、OI） |
| `spotMeta` | 无 | 现货代币/交易对元数据 |
| `spotMetaAndAssetCtxs` | 无 | 现货元数据 + 实时数据 |
| `l2Book` | `coin: string` | 订单簿深度 |
| `candleSnapshot` | `coin: string, interval: string, startTime: number, endTime: number` | K线数据 |
| `recentTrades` | `coin: string` | 最近成交 |
| `allMids` | 无 | 所有交易对中间价 |
| `fundingHistory` | `coin: string, startTime: number, endTime?: number` | 资金费率历史 |

#### 用户数据查询

| type | 参数 | 说明 |
|------|------|------|
| `clearinghouseState` | `user: address` | 永续账户状态（余额、持仓、保证金） |
| `spotClearinghouseState` | `user: address` | 现货余额 |
| `openOrders` | `user: address` | 当前挂单（简化版） |
| `frontendOpenOrders` | `user: address` | 当前挂单（完整版，含时间戳） |
| `userFills` | `user: address, aggregateByTime?: bool` | 成交记录 |
| `userFillsByTime` | `user: address, startTime: number, endTime?: number` | 按时间范围成交记录 |
| `userFunding` | `user: address, startTime: number, endTime?: number` | 用户资金费记录 |
| `historicalOrders` | `user: address` | 历史订单 |
| `orderStatus` | `user: address, oid: number` | 单个订单状态 |
| `maxBuilderFee` | `user: address, builder: address` | Builder 授权状态 |

#### 请求示例

```json
// 获取永续合约元数据
{ "type": "meta" }

// 获取订单簿
{ "type": "l2Book", "coin": "BTC" }

// 获取K线数据
{
  "type": "candleSnapshot",
  "coin": "ETH",
  "interval": "1h",
  "startTime": 1704067200000,
  "endTime": 1704153600000
}

// 获取用户账户状态
{ "type": "clearinghouseState", "user": "0x..." }

// 获取用户成交记录
{ "type": "userFills", "user": "0x..." }

// 获取用户资金费记录
{
  "type": "userFunding",
  "user": "0x...",
  "startTime": 1704067200000
}
```

### 7.3 Exchange API（交易端点）

**端点**: `POST /exchange`
**Content-Type**: `application/json`
**签名**: 需要 EIP-712 签名

#### 操作类型

| action.type | 用途 | 签名方法 |
|-------------|------|----------|
| `order` | 下单 | signL1Action |
| `cancel` | 撤单 | signL1Action |
| `cancelByCloid` | 按 cloid 撤单 | signL1Action |
| `modify` | 修改订单 | signL1Action |
| `batchModify` | 批量修改 | signL1Action |
| `updateLeverage` | 更新杠杆 | signL1Action |
| `updateIsolatedMargin` | 更新逐仓保证金 | signL1Action |
| `approveBuilderFee` | 授权 Builder 费率 | signUserSignedAction |
| `usdSend` | USDC 转账 | signUserSignedAction |
| `withdraw3` | 提现到 L1 | signUserSignedAction |
| `vaultDeposit` | 存入 Vault | signL1Action |
| `vaultWithdraw` | 取出 Vault | signL1Action |

#### 请求结构

```json
{
  "action": {
    "type": "order",
    "orders": [
      {
        "a": 0,                    // asset index
        "b": true,                 // isBuy
        "p": "50000",              // limitPx
        "s": "0.1",                // sz
        "r": false,                // reduceOnly
        "t": { "limit": { "tif": "Gtc" } }  // orderType
      }
    ],
    "grouping": "na",
    "builder": {                   // BuildCode（可选）
      "b": "0x...",                // builder address
      "f": 10                      // fee (bps)
    }
  },
  "nonce": 1704067200000,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  },
  "vaultAddress": null             // 如果是 Vault 操作
}
```

#### 下单示例

```json
// 市价买入 0.1 BTC
{
  "action": {
    "type": "order",
    "orders": [{
      "a": 0,
      "b": true,
      "p": "99999",
      "s": "0.1",
      "r": false,
      "t": { "limit": { "tif": "Ioc" } }
    }],
    "grouping": "na"
  },
  "nonce": 1704067200000,
  "signature": { "r": "0x...", "s": "0x...", "v": 27 }
}

// 限价卖出 1 ETH @ 2500
{
  "action": {
    "type": "order",
    "orders": [{
      "a": 1,
      "b": false,
      "p": "2500",
      "s": "1",
      "r": false,
      "t": { "limit": { "tif": "Gtc" } }
    }],
    "grouping": "na"
  },
  "nonce": 1704067200001,
  "signature": { "r": "0x...", "s": "0x...", "v": 27 }
}

// 撤单
{
  "action": {
    "type": "cancel",
    "cancels": [{ "a": 0, "o": 12345678 }]
  },
  "nonce": 1704067200002,
  "signature": { "r": "0x...", "s": "0x...", "v": 27 }
}
```

### 7.4 响应格式

#### Info API 响应示例

```json
// meta 响应
{
  "universe": [
    {
      "name": "BTC",
      "szDecimals": 5,
      "maxLeverage": 50,
      "onlyIsolated": false
    },
    {
      "name": "ETH",
      "szDecimals": 4,
      "maxLeverage": 50,
      "onlyIsolated": false
    }
  ]
}

// l2Book 响应
{
  "coin": "BTC",
  "levels": [
    [
      { "px": "50100", "sz": "1.5", "n": 3 },    // bid levels
      { "px": "50050", "sz": "2.3", "n": 5 }
    ],
    [
      { "px": "50150", "sz": "0.8", "n": 2 },    // ask levels
      { "px": "50200", "sz": "1.2", "n": 4 }
    ]
  ],
  "time": 1704067200000
}

// clearinghouseState 响应
{
  "marginSummary": {
    "accountValue": "10000.5",
    "totalNtlPos": "5000",
    "totalRawUsd": "5000.5",
    "totalMarginUsed": "100"
  },
  "crossMarginSummary": { ... },
  "crossMaintenanceMarginUsed": "50",
  "withdrawable": "4900.5",
  "assetPositions": [
    {
      "position": {
        "coin": "BTC",
        "szi": "0.1",
        "entryPx": "50000",
        "positionValue": "5000",
        "unrealizedPnl": "50",
        "leverage": { "type": "cross", "value": 10 },
        "liquidationPx": "45000"
      }
    }
  ]
}

// userFills 响应
[
  {
    "coin": "BTC",
    "px": "50100",
    "sz": "0.1",
    "side": "B",
    "time": 1704067200000,
    "startPosition": "0",
    "dir": "Open Long",
    "closedPnl": "0",
    "hash": "0x...",
    "oid": 12345678,
    "crossed": true,
    "fee": "0.5",
    "tid": 987654321,
    "feeToken": "USDC"
  }
]
```

#### Exchange API 响应示例

```json
// 下单成功
{
  "status": "ok",
  "response": {
    "type": "order",
    "data": {
      "statuses": [
        {
          "resting": {
            "oid": 12345678
          }
        }
      ]
    }
  }
}

// 下单失败
{
  "status": "err",
  "response": "User does not have enough margin"
}

// 撤单成功
{
  "status": "ok",
  "response": {
    "type": "cancel",
    "data": {
      "statuses": ["success"]
    }
  }
}
```

### 7.5 Rust 类型定义

```rust
use serde::{Deserialize, Serialize};

/// Info API 请求
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InfoRequest {
    // 市场数据
    Meta,
    MetaAndAssetCtxs,
    SpotMeta,
    SpotMetaAndAssetCtxs,
    AllMids,

    // 订单簿
    L2Book { coin: String },

    // K线
    CandleSnapshot {
        coin: String,
        interval: String,
        start_time: u64,
        end_time: u64,
    },

    // 最近成交
    RecentTrades { coin: String },

    // 资金费率
    FundingHistory {
        coin: String,
        start_time: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
    },

    // 用户数据
    ClearinghouseState { user: String },
    SpotClearinghouseState { user: String },
    OpenOrders { user: String },
    FrontendOpenOrders { user: String },
    UserFills {
        user: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        aggregate_by_time: Option<bool>,
    },
    UserFillsByTime {
        user: String,
        start_time: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
    },
    UserFunding {
        user: String,
        start_time: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
    },
    HistoricalOrders { user: String },
    OrderStatus { user: String, oid: u64 },
    MaxBuilderFee { user: String, builder: String },
}

/// Exchange API 请求
#[derive(Debug, Serialize, Deserialize)]
pub struct ExchangeRequest {
    pub action: ExchangeAction,
    pub nonce: u64,
    pub signature: Signature,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExchangeAction {
    Order {
        orders: Vec<OrderRequest>,
        grouping: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        builder: Option<BuilderInfo>,
    },
    Cancel {
        cancels: Vec<CancelRequest>,
    },
    CancelByCloid {
        cancels: Vec<CancelByCloidRequest>,
    },
    Modify {
        oid: u64,
        order: OrderRequest,
    },
    BatchModify {
        modifies: Vec<ModifyRequest>,
    },
    UpdateLeverage {
        asset: u32,
        is_cross: bool,
        leverage: u32,
    },
    ApproveBuilderFee {
        builder: String,
        max_fee_rate: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OrderRequest {
    pub a: u32,                    // asset index
    pub b: bool,                   // isBuy
    pub p: String,                 // limitPx
    pub s: String,                 // sz
    pub r: bool,                   // reduceOnly
    pub t: OrderType,              // orderType
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,         // cloid (client order id)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuilderInfo {
    pub b: String,                 // builder address
    pub f: u32,                    // fee in bps
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Signature {
    pub r: String,
    pub s: String,
    pub v: u8,
}
```

---

## 8. 配置系统

### 8.1 配置文件结构

```toml
# dex-indexer.toml

[server]
host = "0.0.0.0"
port = 8080
metrics_port = 9090

[grpc]
dex_engine_url = "http://localhost:50051"
connection_timeout_ms = 5000
max_message_size_bytes = 52428800  # 50MB

[database]
url = "postgres://user:pass@localhost:5432/dex_indexer"
max_connections = 20
min_connections = 5

[ingestion]
checkpoint_buffer_size = 5000
ingest_concurrency = 200
retry_interval_ms = 200

[committer]
write_concurrency = 5
collect_interval_ms = 500
watermark_interval_ms = 500

[pruner]
enabled = true
interval_ms = 300000      # 5 分钟
delay_ms = 120000         # 2 分钟
retention = 4000000       # 保留 checkpoint 数
max_chunk_size = 2000

# 每个 Pipeline 可覆盖全局配置
[pipeline.dex_fills]
min_eager_rows = 100
max_pending_rows = 10000

[pipeline.dex_candles]
min_eager_rows = 50
max_pending_rows = 5000
```

---

## 9. 部署架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Load Balancer (Nginx)                           │
└───────────────────────────────────┬─────────────────────────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
           ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
           │ API Server 1│  │ API Server 2│  │ API Server N│
           └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
                  │                │                │
                  └────────────────┼────────────────┘
                                   │
                    ┌──────────────┴──────────────┐
                    ▼                             ▼
           ┌─────────────────┐           ┌─────────────────┐
           │  PostgreSQL     │           │   Redis         │
           │  (Primary)      │           │   (Cluster)     │
           └────────┬────────┘           └─────────────────┘
                    │
                    ▼
           ┌─────────────────┐
           │  PostgreSQL     │
           │  (Replica)      │
           └─────────────────┘
```

---

## 10. 监控指标

```rust
/// Prometheus 指标定义
pub struct IndexerMetrics {
    // Ingestion 指标
    pub checkpoints_received: IntCounter,
    pub checkpoints_processed: IntCounter,
    pub checkpoint_lag: IntGauge,

    // Pipeline 指标
    pub pipeline_pending_rows: IntGaugeVec,      // 按 pipeline 分组
    pub pipeline_committed_rows: IntCounterVec,
    pub pipeline_commit_latency: HistogramVec,

    // 数据库指标
    pub db_connections_active: IntGauge,
    pub db_query_latency: HistogramVec,

    // API 指标
    pub api_requests_total: IntCounterVec,
    pub api_request_latency: HistogramVec,
}
```

---

## 11. 消息队列决策分析（Kafka vs 直连）

### 11.1 dYdX 使用 Kafka 的场景

dYdX 架构中 Kafka 承担核心消息中转角色：

| Kafka Topic | 生产者 | 消费者 | 用途 |
|-------------|--------|--------|------|
| `to-ender` | 全节点 (OnChainEvents) | Ender 服务 | 链上事件持久化 → PostgreSQL |
| `to-vulcan` | 全节点 (OffChainUpdates) | Vulcan 服务 | 订单状态实时更新 → Redis |
| `to-websockets-*` | Ender/Vulcan | Socks 服务 | WebSocket 推送 |

**dYdX 引入 Kafka 的核心原因**：

1. **多服务解耦**：全节点与 5+ 个 Indexer 服务（Ender/Vulcan/Socks/Comlink/Auxo）异步通信
2. **一消息多订阅**：同一事件需要被多个服务消费（Fan-out 模式）
3. **削峰填谷**：应对突发交易量，Kafka 作为缓冲层
4. **持久化重放**：服务重启后可从 Kafka offset 重新消费

### 11.2 sui-indexer-alt 的设计（无 Kafka）

```
Checkpoint 源 (gRPC/HTTP)
        │
        ▼
┌───────────────────┐
│  Ingestion Layer  │ ← 内存 buffer (checkpoint_buffer_size)
└─────────┬─────────┘
          │
    ┌─────┴─────┐
    ▼           ▼
┌─────────┐ ┌─────────┐
│Pipeline1│ │Pipeline2│ ← 独立 channel，背压控制
└────┬────┘ └────┬────┘    (MAX_PENDING_ROWS=5000)
     │           │
     ▼           ▼
┌──────────────────────┐
│     PostgreSQL       │
└──────────────────────┘
```

**sui-indexer-alt 不用 Kafka 的原因**：

| 设计选择 | 说明 |
|---------|------|
| 单一消费者 | 每个 Pipeline 独立消费，不需要消息广播 |
| 背压机制 | 通过 `MAX_PENDING_ROWS` 控制流量，无需外部缓冲 |
| Watermark 进度 | 崩溃恢复从数据库读取进度，无需 Kafka offset |
| 简化运维 | 减少外部依赖，降低运维复杂度 |

### 11.3 方案对比

| 维度 | dYdX (Kafka) | sui-indexer-alt (无 Kafka) | DEX Indexer |
|------|-------------|---------------------------|-------------|
| **服务数量** | 5+ 独立服务 | 单体 + Pipeline | Phase 1: 单体 |
| **消息消费** | 一消息多订阅 | 独立 Pipeline | 独立处理 |
| **实时推送** | Kafka → Socks | 不涉及 | 直连 WebSocket |
| **故障恢复** | Kafka 重放 | Watermark 断点续传 | Watermark |
| **运维复杂度** | 高（Kafka 集群） | 低 | 优先低复杂度 |
| **延迟** | +5~50ms（Kafka 开销） | 最低 | 最低 |

### 11.4 DEX Indexer 推荐方案

#### Phase 1：不引入 Kafka（推荐）

```
┌─────────────────────────────────────────────────────────────────┐
│                     DEX Engine                                   │
│  ┌───────────────┐           ┌───────────────┐                  │
│  │ OnChainUpdates│           │OffChainUpdates│                  │
│  │ (Checkpoint)  │           │ (实时订单)    │                  │
│  └───────┬───────┘           └───────┬───────┘                  │
└──────────┼───────────────────────────┼──────────────────────────┘
           │ gRPC Stream               │ 直连
           ▼                           ▼
┌──────────────────────┐     ┌──────────────────────┐
│    DEX Indexer       │     │   WebSocket Server   │
│  ┌────────────────┐  │     │                      │
│  │ Pipeline (内存)│  │     │  • 订单簿推送        │
│  │ Watermark 进度 │  │     │  • 订单状态推送      │
│  └────────┬───────┘  │     │                      │
│           ▼          │     └──────────────────────┘
│  ┌────────────────┐  │
│  │  PostgreSQL    │  │
│  └────────────────┘  │
└──────────────────────┘
```

**理由**：
- Phase 1 服务数量少，不需要消息广播
- OffChainUpdates 直接推送 WebSocket，无需中转
- 借鉴 sui-indexer-alt 的 Watermark 机制实现故障恢复
- 降低运维复杂度，专注功能验证

#### Phase 2+：可选引入 Kafka

**触发条件**：

| 触发条件 | 说明 |
|---------|------|
| 多 Indexer 实例 | 需要事件广播到多个消费者 |
| 流量峰值 > 10x | 需要削峰填谷 |
| 服务解耦需求 | 新增独立的数据分析/风控服务等 |
| 跨数据中心 | 多区域部署需要可靠消息传递 |

**引入 Kafka 后的架构**：

```
┌─────────────────────────────────────────────────────────────────┐
│                     DEX Engine                                   │
│  ┌───────────────┐           ┌───────────────┐                  │
│  │ OnChainUpdates│           │OffChainUpdates│                  │
│  └───────┬───────┘           └───────┬───────┘                  │
└──────────┼───────────────────────────┼──────────────────────────┘
           │                           │
           ▼                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Kafka                                     │
│  ┌──────────────┐  ┌───────────────┐  ┌─────────────────────┐   │
│  │ dex-onchain  │  │ dex-offchain  │  │ dex-websockets-*    │   │
│  └──────┬───────┘  └───────┬───────┘  └──────────┬──────────┘   │
└─────────┼──────────────────┼─────────────────────┼──────────────┘
          │                  │                     │
          ▼                  ▼                     ▼
┌──────────────────┐ ┌─────────────────┐ ┌──────────────────┐
│   Indexer        │ │  Order Cache    │ │  WebSocket Svc   │
│   (PostgreSQL)   │ │  (Redis)        │ │  (推送)          │
└──────────────────┘ └─────────────────┘ └──────────────────┘
```

### 11.5 决策总结

| Phase | Kafka | 理由 |
|-------|-------|------|
| **Phase 1** | ❌ 不引入 | 功能验证阶段，优先简化架构 |
| **Phase 2** | ⚠️ 评估 | 根据实际流量和服务扩展需求决定 |
| **Phase 3** | ✅ 可选 | 多消费者、高可用场景下引入 |

**关键结论**：
1. Kafka 是 **可选优化**，不是 **必须组件**
2. sui-indexer-alt 证明了无 Kafka 架构在高吞吐场景下可行
3. 保持架构简单，按需引入复杂度
4. 使用 Watermark + 幂等写入实现故障恢复，替代 Kafka offset 重放

---

## 12. 实施建议

### 12.1 推荐方案：独立实现，借鉴架构模式

由于 DEX 使用原生 Rust 引擎，事件来源与 sui-indexer-alt 不同，建议：

1. **独立实现** DEX Indexer，不直接依赖 sui-indexer-alt-framework
2. **借鉴架构模式**：Pipeline、Collector、Committer、Watermark、Pruner
3. **复用设计模式**：gRPC Streaming、批量处理、背压控制、指数退避重试

### 12.2 关键借鉴清单

| 组件 | 借鉴来源 | 关键代码位置 |
|------|----------|-------------|
| gRPC 客户端 | `streaming_client.rs` | :26-86 |
| Handler 接口 | `pipeline/concurrent/mod.rs` | :58-105 |
| 批量收集 | `collector.rs` | :58-72, :211-213 |
| 并发写入 | `committer.rs` | :58-63, :89-91 |
| Watermark | `commit_watermark.rs` | 全文 |
| 数据清理 | `pruner.rs` | 全文 |

---

## 参考文档

- `sui/mynotes/dex/analyst/dex-indexer-full-by-dydx-analysis.md` - 双通道机制分析
- `sui/mynotes/dex/analyst/sui-indexer-alt-analyst.md` - sui-indexer-alt 架构分析
- `sui/mynotes/dex/analyst/dydx-indexer-analyst.md` - dYdX 索引器参考
- `sui/crates/sui-indexer-alt-framework/` - 源码参考
