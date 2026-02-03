# DEX Indexer 技术方案

> 原生 Rust DEX 引擎的链下索引服务技术规格

## 1. 概述

### 1.1 文档目的

本文档定义 DEX Indexer 的完整技术规格，包括：
- gRPC 事件传输协议
- 数据库表结构设计
- REST API 接口规范
- WebSocket 订阅协议
- 核心代码实现示例

### 1.2 系统架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                         DEX Engine (Rust)                            │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌─────────────────┐  │
│  │ OrderBook │  │ Matching  │  │ Position  │  │ Event Publisher │  │
│  └───────────┘  └───────────┘  └───────────┘  └────────┬────────┘  │
└────────────────────────────────────────────────────────┼────────────┘
                                                         │ gRPC Stream
                                                         ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         DEX Indexer (Rust)                           │
│  ┌─────────────┐  ┌──────────┐  ┌────────────┐  ┌───────────────┐  │
│  │gRPC Receiver│→ │ Handlers │→ │ PostgreSQL │→ │ REST/WS API   │  │
│  └─────────────┘  └──────────┘  └────────────┘  └───────────────┘  │
│                                       ↓ [P3]                        │
│                                 ┌──────────┐                        │
│                                 │  Redis   │→ WebSocket Broadcast   │
│                                 └──────────┘                        │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.3 技术栈

| 层级 | 技术 | 版本 |
|------|------|------|
| gRPC | tonic + prost | 0.11 / 0.12 |
| 数据库 | PostgreSQL | 15+ |
| ORM | Diesel | 2.1 |
| HTTP | Axum | 0.7 |
| WebSocket | tokio-tungstenite | 0.21 |
| 缓存 | Redis | 7.0+ |

---

## 2. Proto 定义

### 2.1 服务定义

```protobuf
// proto/dex_events.proto
syntax = "proto3";

package dex.events.v1;

option go_package = "dex/events/v1;eventsv1";

// ============================================================
// 服务定义
// ============================================================

service DexEventService {
  // 订阅事件流
  rpc Subscribe(SubscribeRequest) returns (stream DexEvent);

  // 重放历史事件 (用于 Indexer 重启恢复)
  rpc Replay(ReplayRequest) returns (stream DexEvent);

  // 获取最新事件序号
  rpc GetLatestSequence(Empty) returns (SequenceResponse);
}

// ============================================================
// 请求/响应消息
// ============================================================

message Empty {}

message SubscribeRequest {
  // 从指定序号开始订阅 (0 表示从最新开始)
  uint64 from_sequence = 1;

  // 事件类型过滤 (空表示订阅所有)
  repeated string event_types = 2;

  // 市场过滤 (空表示所有市场)
  repeated bytes market_ids = 3;
}

message ReplayRequest {
  uint64 from_sequence = 1;
  uint64 to_sequence = 2;
}

message SequenceResponse {
  uint64 sequence = 1;
}

// ============================================================
// 事件包装
// ============================================================

message DexEvent {
  // 全局递增序号 (用于断点续传)
  uint64 sequence = 1;

  // 事件时间戳 (Unix 毫秒)
  uint64 timestamp = 2;

  // 事件类型标识
  string event_type = 3;

  // 事件体
  oneof event {
    // 订单事件
    OrderPlacedEvent order_placed = 10;
    OrderMatchedEvent order_matched = 11;
    OrderCanceledEvent order_canceled = 12;
    OrderExpiredEvent order_expired = 13;

    // 成交事件
    TradeEvent trade = 20;

    // 仓位事件
    PositionOpenedEvent position_opened = 30;
    PositionUpdatedEvent position_updated = 31;
    PositionClosedEvent position_closed = 32;
    PositionLiquidatedEvent position_liquidated = 33;

    // 资金费率事件
    FundingPaidEvent funding_paid = 40;

    // 市场事件
    MarketCreatedEvent market_created = 50;
    MarketUpdatedEvent market_updated = 51;
  }
}

// ============================================================
// 订单事件
// ============================================================

message OrderPlacedEvent {
  bytes order_id = 1;          // 32 bytes
  bytes market_id = 2;         // 32 bytes
  bytes owner = 3;             // 32 bytes (Sui address)

  Side side = 4;
  OrderType order_type = 5;
  TimeInForce time_in_force = 6;

  string price = 7;            // 价格 (字符串避免精度问题)
  string quantity = 8;         // 数量
  string filled_quantity = 9;  // 已成交数量

  bool reduce_only = 10;       // 只减仓
  bool post_only = 11;         // 只做 Maker

  uint64 client_order_id = 12; // 客户端订单 ID
}

message OrderMatchedEvent {
  bytes order_id = 1;
  string filled_quantity = 2;
  string remaining_quantity = 3;
  OrderStatus status = 4;
}

message OrderCanceledEvent {
  bytes order_id = 1;
  CancelReason reason = 2;
}

message OrderExpiredEvent {
  bytes order_id = 1;
}

// ============================================================
// 成交事件
// ============================================================

message TradeEvent {
  uint64 trade_id = 1;
  bytes market_id = 2;

  bytes maker_order_id = 3;
  bytes taker_order_id = 4;
  bytes maker_address = 5;
  bytes taker_address = 6;

  Side taker_side = 7;
  string price = 8;
  string quantity = 9;

  string maker_fee = 10;
  string taker_fee = 11;
}

// ============================================================
// 仓位事件
// ============================================================

message PositionOpenedEvent {
  bytes position_id = 1;
  bytes owner = 2;
  bytes market_id = 3;

  Side side = 4;
  string size = 5;
  string entry_price = 6;
  string margin = 7;
  uint32 leverage = 8;
}

message PositionUpdatedEvent {
  bytes position_id = 1;
  string size = 2;
  string entry_price = 3;
  string margin = 4;
  string unrealized_pnl = 5;
}

message PositionClosedEvent {
  bytes position_id = 1;
  string exit_price = 2;
  string realized_pnl = 3;
  CloseReason reason = 4;
}

message PositionLiquidatedEvent {
  bytes position_id = 1;
  string liquidation_price = 2;
  string bankruptcy_price = 3;
  bytes liquidator = 4;
}

// ============================================================
// 资金费率事件
// ============================================================

message FundingPaidEvent {
  bytes market_id = 1;
  string funding_rate = 2;     // 可正可负
  string mark_price = 3;
  string index_price = 4;
  uint64 next_funding_time = 5;
}

// ============================================================
// 市场事件
// ============================================================

message MarketCreatedEvent {
  bytes market_id = 1;
  string symbol = 2;           // e.g., "BTC-PERP"
  string base_asset = 3;       // e.g., "BTC"
  string quote_asset = 4;      // e.g., "USDC"

  string tick_size = 5;        // 价格精度
  string lot_size = 6;         // 数量精度
  uint32 max_leverage = 7;

  string initial_margin_rate = 8;
  string maintenance_margin_rate = 9;
}

message MarketUpdatedEvent {
  bytes market_id = 1;
  string tick_size = 2;
  string lot_size = 3;
  uint32 max_leverage = 4;
  bool is_active = 5;
}

// ============================================================
// 枚举定义
// ============================================================

enum Side {
  SIDE_UNSPECIFIED = 0;
  SIDE_BUY = 1;
  SIDE_SELL = 2;
}

enum OrderType {
  ORDER_TYPE_UNSPECIFIED = 0;
  ORDER_TYPE_LIMIT = 1;
  ORDER_TYPE_MARKET = 2;
  ORDER_TYPE_STOP_LIMIT = 3;
  ORDER_TYPE_STOP_MARKET = 4;
  ORDER_TYPE_TAKE_PROFIT_LIMIT = 5;
  ORDER_TYPE_TAKE_PROFIT_MARKET = 6;
}

enum TimeInForce {
  TIME_IN_FORCE_UNSPECIFIED = 0;
  TIME_IN_FORCE_GTC = 1;       // Good Till Cancel
  TIME_IN_FORCE_IOC = 2;       // Immediate Or Cancel
  TIME_IN_FORCE_FOK = 3;       // Fill Or Kill
  TIME_IN_FORCE_GTX = 4;       // Good Till Crossing (Post Only)
}

enum OrderStatus {
  ORDER_STATUS_UNSPECIFIED = 0;
  ORDER_STATUS_OPEN = 1;
  ORDER_STATUS_PARTIALLY_FILLED = 2;
  ORDER_STATUS_FILLED = 3;
  ORDER_STATUS_CANCELED = 4;
  ORDER_STATUS_EXPIRED = 5;
}

enum CancelReason {
  CANCEL_REASON_UNSPECIFIED = 0;
  CANCEL_REASON_USER = 1;
  CANCEL_REASON_INSUFFICIENT_MARGIN = 2;
  CANCEL_REASON_SELF_TRADE = 3;
  CANCEL_REASON_POST_ONLY_FAILED = 4;
  CANCEL_REASON_IOC_UNFILLED = 5;
  CANCEL_REASON_FOK_UNFILLED = 6;
  CANCEL_REASON_REDUCE_ONLY_FAILED = 7;
}

enum CloseReason {
  CLOSE_REASON_UNSPECIFIED = 0;
  CLOSE_REASON_USER = 1;
  CLOSE_REASON_LIQUIDATION = 2;
  CLOSE_REASON_ADL = 3;        // Auto-Deleveraging
  CLOSE_REASON_SETTLEMENT = 4;
}
```

### 2.2 Rust 代码生成

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)  // Indexer 只需要 client
        .build_client(true)
        .out_dir("src/proto")
        .compile(&["proto/dex_events.proto"], &["proto/"])?;
    Ok(())
}
```

---

## 3. 数据库设计

### 3.1 表结构 DDL

```sql
-- migrations/001_create_markets.sql

-- 市场表
CREATE TABLE markets (
    id              BYTEA PRIMARY KEY,          -- 32 bytes
    symbol          VARCHAR(32) NOT NULL UNIQUE,
    base_asset      VARCHAR(16) NOT NULL,
    quote_asset     VARCHAR(16) NOT NULL,

    tick_size       NUMERIC(38, 18) NOT NULL,
    lot_size        NUMERIC(38, 18) NOT NULL,
    max_leverage    INTEGER NOT NULL,

    initial_margin_rate      NUMERIC(10, 8) NOT NULL,
    maintenance_margin_rate  NUMERIC(10, 8) NOT NULL,

    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_markets_symbol ON markets(symbol);
CREATE INDEX idx_markets_active ON markets(is_active) WHERE is_active = TRUE;
```

```sql
-- migrations/002_create_orders.sql

-- 订单表
CREATE TABLE orders (
    id              BYTEA PRIMARY KEY,          -- 32 bytes
    market_id       BYTEA NOT NULL REFERENCES markets(id),
    owner           BYTEA NOT NULL,             -- 32 bytes

    side            SMALLINT NOT NULL,          -- 1=Buy, 2=Sell
    order_type      SMALLINT NOT NULL,          -- 1=Limit, 2=Market, ...
    time_in_force   SMALLINT NOT NULL,          -- 1=GTC, 2=IOC, 3=FOK
    status          SMALLINT NOT NULL,          -- 1=Open, 2=PartialFilled, ...

    price           NUMERIC(38, 18) NOT NULL,
    quantity        NUMERIC(38, 18) NOT NULL,
    filled_quantity NUMERIC(38, 18) NOT NULL DEFAULT 0,

    reduce_only     BOOLEAN NOT NULL DEFAULT FALSE,
    post_only       BOOLEAN NOT NULL DEFAULT FALSE,
    client_order_id BIGINT,

    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL,

    -- 事件追踪
    event_sequence  BIGINT NOT NULL
);

-- 索引
CREATE INDEX idx_orders_owner ON orders(owner);
CREATE INDEX idx_orders_market ON orders(market_id);
CREATE INDEX idx_orders_owner_status ON orders(owner, status);
CREATE INDEX idx_orders_market_side_price ON orders(market_id, side, price)
    WHERE status IN (1, 2);  -- Open 或 PartialFilled
CREATE INDEX idx_orders_created_at ON orders(created_at DESC);
CREATE INDEX idx_orders_event_seq ON orders(event_sequence);
```

```sql
-- migrations/003_create_fills.sql

-- 成交表 (按天分区)
CREATE TABLE fills (
    id              BIGSERIAL,
    trade_id        BIGINT NOT NULL,
    market_id       BYTEA NOT NULL,

    maker_order_id  BYTEA NOT NULL,
    taker_order_id  BYTEA NOT NULL,
    maker_address   BYTEA NOT NULL,
    taker_address   BYTEA NOT NULL,

    taker_side      SMALLINT NOT NULL,
    price           NUMERIC(38, 18) NOT NULL,
    quantity        NUMERIC(38, 18) NOT NULL,

    maker_fee       NUMERIC(38, 18) NOT NULL,
    taker_fee       NUMERIC(38, 18) NOT NULL,

    created_at      TIMESTAMPTZ NOT NULL,
    event_sequence  BIGINT NOT NULL,

    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

-- 创建分区 (示例: 2026年1月)
CREATE TABLE fills_2026_01 PARTITION OF fills
    FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');

-- 索引
CREATE INDEX idx_fills_market ON fills(market_id, created_at DESC);
CREATE INDEX idx_fills_maker ON fills(maker_address, created_at DESC);
CREATE INDEX idx_fills_taker ON fills(taker_address, created_at DESC);
CREATE INDEX idx_fills_trade_id ON fills(trade_id);
```

```sql
-- migrations/004_create_positions.sql

-- 仓位表
CREATE TABLE positions (
    id              BYTEA PRIMARY KEY,
    owner           BYTEA NOT NULL,
    market_id       BYTEA NOT NULL REFERENCES markets(id),

    side            SMALLINT NOT NULL,          -- 1=Long, 2=Short
    size            NUMERIC(38, 18) NOT NULL,
    entry_price     NUMERIC(38, 18) NOT NULL,
    margin          NUMERIC(38, 18) NOT NULL,
    leverage        INTEGER NOT NULL,

    unrealized_pnl  NUMERIC(38, 18) NOT NULL DEFAULT 0,
    realized_pnl    NUMERIC(38, 18) NOT NULL DEFAULT 0,

    status          SMALLINT NOT NULL DEFAULT 1, -- 1=Open, 2=Closed

    opened_at       TIMESTAMPTZ NOT NULL,
    closed_at       TIMESTAMPTZ,
    updated_at      TIMESTAMPTZ NOT NULL,

    event_sequence  BIGINT NOT NULL
);

-- 索引
CREATE INDEX idx_positions_owner ON positions(owner);
CREATE INDEX idx_positions_owner_open ON positions(owner) WHERE status = 1;
CREATE INDEX idx_positions_market ON positions(market_id);
CREATE UNIQUE INDEX idx_positions_owner_market_open
    ON positions(owner, market_id) WHERE status = 1;
```

```sql
-- migrations/005_create_candles.sql

-- K线表 (按月分区)
CREATE TABLE candles (
    id              BIGSERIAL,
    market_id       BYTEA NOT NULL,
    interval        VARCHAR(4) NOT NULL,        -- '1m', '5m', '1h', '1d'

    open_time       TIMESTAMPTZ NOT NULL,
    close_time      TIMESTAMPTZ NOT NULL,

    open            NUMERIC(38, 18) NOT NULL,
    high            NUMERIC(38, 18) NOT NULL,
    low             NUMERIC(38, 18) NOT NULL,
    close           NUMERIC(38, 18) NOT NULL,
    volume          NUMERIC(38, 18) NOT NULL,
    turnover        NUMERIC(38, 18) NOT NULL,   -- 成交额
    trade_count     INTEGER NOT NULL,

    PRIMARY KEY (id, open_time)
) PARTITION BY RANGE (open_time);

-- 创建分区
CREATE TABLE candles_2026_01 PARTITION OF candles
    FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');

-- 索引
CREATE UNIQUE INDEX idx_candles_market_interval_time
    ON candles(market_id, interval, open_time DESC);
```

```sql
-- migrations/006_create_funding_rates.sql

-- 资金费率表
CREATE TABLE funding_rates (
    id              BIGSERIAL,
    market_id       BYTEA NOT NULL,

    funding_rate    NUMERIC(20, 10) NOT NULL,   -- 可正可负
    mark_price      NUMERIC(38, 18) NOT NULL,
    index_price     NUMERIC(38, 18) NOT NULL,

    funding_time    TIMESTAMPTZ NOT NULL,
    next_funding_time TIMESTAMPTZ NOT NULL,

    event_sequence  BIGINT NOT NULL,

    PRIMARY KEY (id, funding_time)
) PARTITION BY RANGE (funding_time);

-- 索引
CREATE INDEX idx_funding_rates_market ON funding_rates(market_id, funding_time DESC);
```

```sql
-- migrations/007_create_indexer_state.sql

-- Indexer 状态表 (用于断点续传)
CREATE TABLE indexer_state (
    id              INTEGER PRIMARY KEY DEFAULT 1,
    last_sequence   BIGINT NOT NULL DEFAULT 0,
    last_event_time TIMESTAMPTZ,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT single_row CHECK (id = 1)
);

INSERT INTO indexer_state (id, last_sequence) VALUES (1, 0);
```

### 3.2 Diesel Schema

```rust
// src/schema.rs
diesel::table! {
    markets (id) {
        id -> Bytea,
        symbol -> Varchar,
        base_asset -> Varchar,
        quote_asset -> Varchar,
        tick_size -> Numeric,
        lot_size -> Numeric,
        max_leverage -> Int4,
        initial_margin_rate -> Numeric,
        maintenance_margin_rate -> Numeric,
        is_active -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    orders (id) {
        id -> Bytea,
        market_id -> Bytea,
        owner -> Bytea,
        side -> Int2,
        order_type -> Int2,
        time_in_force -> Int2,
        status -> Int2,
        price -> Numeric,
        quantity -> Numeric,
        filled_quantity -> Numeric,
        reduce_only -> Bool,
        post_only -> Bool,
        client_order_id -> Nullable<Int8>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        event_sequence -> Int8,
    }
}

diesel::table! {
    fills (id, created_at) {
        id -> Int8,
        trade_id -> Int8,
        market_id -> Bytea,
        maker_order_id -> Bytea,
        taker_order_id -> Bytea,
        maker_address -> Bytea,
        taker_address -> Bytea,
        taker_side -> Int2,
        price -> Numeric,
        quantity -> Numeric,
        maker_fee -> Numeric,
        taker_fee -> Numeric,
        created_at -> Timestamptz,
        event_sequence -> Int8,
    }
}

diesel::table! {
    positions (id) {
        id -> Bytea,
        owner -> Bytea,
        market_id -> Bytea,
        side -> Int2,
        size -> Numeric,
        entry_price -> Numeric,
        margin -> Numeric,
        leverage -> Int4,
        unrealized_pnl -> Numeric,
        realized_pnl -> Numeric,
        status -> Int2,
        opened_at -> Timestamptz,
        closed_at -> Nullable<Timestamptz>,
        updated_at -> Timestamptz,
        event_sequence -> Int8,
    }
}

diesel::table! {
    candles (id, open_time) {
        id -> Int8,
        market_id -> Bytea,
        interval -> Varchar,
        open_time -> Timestamptz,
        close_time -> Timestamptz,
        open -> Numeric,
        high -> Numeric,
        low -> Numeric,
        close -> Numeric,
        volume -> Numeric,
        turnover -> Numeric,
        trade_count -> Int4,
    }
}

diesel::table! {
    funding_rates (id, funding_time) {
        id -> Int8,
        market_id -> Bytea,
        funding_rate -> Numeric,
        mark_price -> Numeric,
        index_price -> Numeric,
        funding_time -> Timestamptz,
        next_funding_time -> Timestamptz,
        event_sequence -> Int8,
    }
}

diesel::table! {
    indexer_state (id) {
        id -> Int4,
        last_sequence -> Int8,
        last_event_time -> Nullable<Timestamptz>,
        updated_at -> Timestamptz,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    markets,
    orders,
    fills,
    positions,
    candles,
    funding_rates,
    indexer_state,
);
```

---

## 4. Handler 实现

### 4.1 事件接收器

```rust
// src/receiver/grpc_client.rs
use crate::proto::dex_events::v1::{
    dex_event_service_client::DexEventServiceClient,
    DexEvent, SubscribeRequest,
};
use tokio::sync::mpsc;
use tonic::transport::Channel;

pub struct EventReceiver {
    client: DexEventServiceClient<Channel>,
    last_sequence: u64,
}

impl EventReceiver {
    pub async fn new(endpoint: &str) -> Result<Self, tonic::transport::Error> {
        let client = DexEventServiceClient::connect(endpoint.to_string()).await?;
        Ok(Self {
            client,
            last_sequence: 0,
        })
    }

    pub async fn subscribe(
        &mut self,
        from_sequence: u64,
        tx: mpsc::Sender<DexEvent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = SubscribeRequest {
            from_sequence,
            event_types: vec![],
            market_ids: vec![],
        };

        let mut stream = self.client.subscribe(request).await?.into_inner();

        while let Some(event) = stream.message().await? {
            self.last_sequence = event.sequence;
            if tx.send(event).await.is_err() {
                break;
            }
        }

        Ok(())
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}
```

### 4.2 订单 Handler

```rust
// src/handlers/orders.rs
use crate::models::{NewOrder, Order, OrderStatus};
use crate::proto::dex_events::v1::{
    dex_event::Event, OrderCanceledEvent, OrderMatchedEvent, OrderPlacedEvent,
};
use crate::schema::orders;
use bigdecimal::BigDecimal;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use std::str::FromStr;

pub struct OrderHandler;

impl OrderHandler {
    pub async fn handle_order_placed(
        conn: &mut AsyncPgConnection,
        event: &OrderPlacedEvent,
        sequence: u64,
        timestamp: u64,
    ) -> Result<(), diesel::result::Error> {
        let new_order = NewOrder {
            id: &event.order_id,
            market_id: &event.market_id,
            owner: &event.owner,
            side: event.side as i16,
            order_type: event.order_type as i16,
            time_in_force: event.time_in_force as i16,
            status: OrderStatus::Open as i16,
            price: BigDecimal::from_str(&event.price).unwrap_or_default(),
            quantity: BigDecimal::from_str(&event.quantity).unwrap_or_default(),
            filled_quantity: BigDecimal::from_str(&event.filled_quantity).unwrap_or_default(),
            reduce_only: event.reduce_only,
            post_only: event.post_only,
            client_order_id: if event.client_order_id > 0 {
                Some(event.client_order_id as i64)
            } else {
                None
            },
            created_at: chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap(),
            updated_at: chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap(),
            event_sequence: sequence as i64,
        };

        diesel::insert_into(orders::table)
            .values(&new_order)
            .on_conflict(orders::id)
            .do_update()
            .set(&new_order)
            .execute(conn)
            .await?;

        Ok(())
    }

    pub async fn handle_order_matched(
        conn: &mut AsyncPgConnection,
        event: &OrderMatchedEvent,
        sequence: u64,
        timestamp: u64,
    ) -> Result<(), diesel::result::Error> {
        let filled_qty = BigDecimal::from_str(&event.filled_quantity).unwrap_or_default();
        let remaining_qty = BigDecimal::from_str(&event.remaining_quantity).unwrap_or_default();

        diesel::update(orders::table.find(&event.order_id))
            .set((
                orders::filled_quantity.eq(&filled_qty),
                orders::status.eq(event.status as i16),
                orders::updated_at.eq(chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap()),
                orders::event_sequence.eq(sequence as i64),
            ))
            .execute(conn)
            .await?;

        Ok(())
    }

    pub async fn handle_order_canceled(
        conn: &mut AsyncPgConnection,
        event: &OrderCanceledEvent,
        sequence: u64,
        timestamp: u64,
    ) -> Result<(), diesel::result::Error> {
        diesel::update(orders::table.find(&event.order_id))
            .set((
                orders::status.eq(OrderStatus::Canceled as i16),
                orders::updated_at.eq(chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap()),
                orders::event_sequence.eq(sequence as i64),
            ))
            .execute(conn)
            .await?;

        Ok(())
    }
}
```

### 4.3 成交 Handler

```rust
// src/handlers/fills.rs
use crate::models::NewFill;
use crate::proto::dex_events::v1::TradeEvent;
use crate::schema::fills;
use bigdecimal::BigDecimal;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use std::str::FromStr;

pub struct FillHandler;

impl FillHandler {
    pub async fn handle_trade(
        conn: &mut AsyncPgConnection,
        event: &TradeEvent,
        sequence: u64,
        timestamp: u64,
    ) -> Result<(), diesel::result::Error> {
        let new_fill = NewFill {
            trade_id: event.trade_id as i64,
            market_id: &event.market_id,
            maker_order_id: &event.maker_order_id,
            taker_order_id: &event.taker_order_id,
            maker_address: &event.maker_address,
            taker_address: &event.taker_address,
            taker_side: event.taker_side as i16,
            price: BigDecimal::from_str(&event.price).unwrap_or_default(),
            quantity: BigDecimal::from_str(&event.quantity).unwrap_or_default(),
            maker_fee: BigDecimal::from_str(&event.maker_fee).unwrap_or_default(),
            taker_fee: BigDecimal::from_str(&event.taker_fee).unwrap_or_default(),
            created_at: chrono::Utc.timestamp_millis_opt(timestamp as i64).unwrap(),
            event_sequence: sequence as i64,
        };

        diesel::insert_into(fills::table)
            .values(&new_fill)
            .execute(conn)
            .await?;

        Ok(())
    }
}
```

### 4.4 事件分发器

```rust
// src/handlers/dispatcher.rs
use crate::handlers::{FillHandler, OrderHandler, PositionHandler, CandleHandler};
use crate::proto::dex_events::v1::{dex_event::Event, DexEvent};
use diesel_async::AsyncPgConnection;
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct EventDispatcher {
    rx: mpsc::Receiver<DexEvent>,
}

impl EventDispatcher {
    pub fn new(rx: mpsc::Receiver<DexEvent>) -> Self {
        Self { rx }
    }

    pub async fn run(&mut self, conn: &mut AsyncPgConnection) {
        while let Some(event) = self.rx.recv().await {
            if let Err(e) = self.dispatch(conn, &event).await {
                error!("Failed to process event {}: {:?}", event.sequence, e);
                // 记录失败的事件,后续可重试
            }
        }
    }

    async fn dispatch(
        &self,
        conn: &mut AsyncPgConnection,
        event: &DexEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sequence = event.sequence;
        let timestamp = event.timestamp;

        match &event.event {
            Some(Event::OrderPlaced(e)) => {
                OrderHandler::handle_order_placed(conn, e, sequence, timestamp).await?;
            }
            Some(Event::OrderMatched(e)) => {
                OrderHandler::handle_order_matched(conn, e, sequence, timestamp).await?;
            }
            Some(Event::OrderCanceled(e)) => {
                OrderHandler::handle_order_canceled(conn, e, sequence, timestamp).await?;
            }
            Some(Event::Trade(e)) => {
                FillHandler::handle_trade(conn, e, sequence, timestamp).await?;
                CandleHandler::update_candle(conn, e, timestamp).await?;
            }
            Some(Event::PositionOpened(e)) => {
                PositionHandler::handle_position_opened(conn, e, sequence, timestamp).await?;
            }
            Some(Event::PositionUpdated(e)) => {
                PositionHandler::handle_position_updated(conn, e, sequence, timestamp).await?;
            }
            Some(Event::PositionClosed(e)) => {
                PositionHandler::handle_position_closed(conn, e, sequence, timestamp).await?;
            }
            // ... 其他事件
            _ => {}
        }

        // 更新 indexer 状态
        self.update_state(conn, sequence, timestamp).await?;

        Ok(())
    }

    async fn update_state(
        &self,
        conn: &mut AsyncPgConnection,
        sequence: u64,
        timestamp: u64,
    ) -> Result<(), diesel::result::Error> {
        use crate::schema::indexer_state;
        use diesel::prelude::*;

        diesel::update(indexer_state::table)
            .set((
                indexer_state::last_sequence.eq(sequence as i64),
                indexer_state::last_event_time.eq(
                    chrono::Utc.timestamp_millis_opt(timestamp as i64)
                ),
                indexer_state::updated_at.eq(chrono::Utc::now()),
            ))
            .execute(conn)
            .await?;

        Ok(())
    }
}
```

---

## 5. REST API 设计

### 5.1 统一入口

```rust
// src/api/info.rs
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum InfoRequest {
    // 市场数据
    #[serde(rename = "meta")]
    Meta,

    #[serde(rename = "metaAndAssetCtxs")]
    MetaAndAssetCtxs,

    #[serde(rename = "l2Book")]
    L2Book { coin: String },

    #[serde(rename = "candleSnapshot")]
    CandleSnapshot { req: CandleRequest },

    #[serde(rename = "recentTrades")]
    RecentTrades { coin: String },

    #[serde(rename = "allMids")]
    AllMids,

    #[serde(rename = "fundingHistory")]
    FundingHistory {
        coin: String,
        #[serde(rename = "startTime")]
        start_time: u64,
        #[serde(rename = "endTime")]
        end_time: Option<u64>,
    },

    // 用户数据
    #[serde(rename = "clearinghouseState")]
    ClearinghouseState { user: String },

    #[serde(rename = "openOrders")]
    OpenOrders { user: String },

    #[serde(rename = "historicalOrders")]
    HistoricalOrders { user: String },

    #[serde(rename = "userFills")]
    UserFills { user: String },

    #[serde(rename = "userFunding")]
    UserFunding { user: String },
}

#[derive(Debug, Deserialize)]
pub struct CandleRequest {
    pub coin: String,
    pub interval: String,
    #[serde(rename = "startTime")]
    pub start_time: u64,
    #[serde(rename = "endTime")]
    pub end_time: Option<u64>,
}

pub async fn info_handler(
    State(state): State<AppState>,
    Json(request): Json<InfoRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match request {
        InfoRequest::Meta => {
            let markets = state.market_service.get_meta().await?;
            Ok(Json(serde_json::to_value(markets)?))
        }
        InfoRequest::L2Book { coin } => {
            let orderbook = state.orderbook_service.get_l2_book(&coin).await?;
            Ok(Json(serde_json::to_value(orderbook)?))
        }
        InfoRequest::CandleSnapshot { req } => {
            let candles = state.candle_service.get_snapshot(
                &req.coin,
                &req.interval,
                req.start_time,
                req.end_time,
            ).await?;
            Ok(Json(serde_json::to_value(candles)?))
        }
        InfoRequest::OpenOrders { user } => {
            let orders = state.order_service.get_open_orders(&user).await?;
            Ok(Json(serde_json::to_value(orders)?))
        }
        InfoRequest::ClearinghouseState { user } => {
            let account = state.account_service.get_state(&user).await?;
            Ok(Json(serde_json::to_value(account)?))
        }
        // ... 其他处理
        _ => Err(ApiError::NotImplemented),
    }
}
```

### 5.2 响应格式 (对标 Hyperliquid)

```rust
// src/api/types.rs

/// 市场元数据
#[derive(Debug, Serialize)]
pub struct MarketMeta {
    pub universe: Vec<AssetMeta>,
}

#[derive(Debug, Serialize)]
pub struct AssetMeta {
    pub name: String,           // "BTC"
    #[serde(rename = "szDecimals")]
    pub sz_decimals: u8,        // 数量精度
    #[serde(rename = "maxLeverage")]
    pub max_leverage: u32,
}

/// 订单簿
#[derive(Debug, Serialize)]
pub struct L2Book {
    pub coin: String,
    pub levels: [Vec<L2Level>; 2],  // [bids, asks]
    pub time: u64,
}

#[derive(Debug, Serialize)]
pub struct L2Level {
    pub px: String,     // 价格
    pub sz: String,     // 数量
    pub n: u32,         // 订单数
}

/// K线
#[derive(Debug, Serialize)]
pub struct Candle {
    pub t: u64,         // 开始时间 (ms)
    pub o: String,      // 开盘价
    pub h: String,      // 最高价
    pub l: String,      // 最低价
    pub c: String,      // 收盘价
    pub v: String,      // 成交量
    pub n: u32,         // 成交笔数
}

/// 账户状态
#[derive(Debug, Serialize)]
pub struct ClearinghouseState {
    #[serde(rename = "marginSummary")]
    pub margin_summary: MarginSummary,
    #[serde(rename = "assetPositions")]
    pub asset_positions: Vec<AssetPosition>,
    pub withdrawable: String,
}

#[derive(Debug, Serialize)]
pub struct MarginSummary {
    #[serde(rename = "accountValue")]
    pub account_value: String,
    #[serde(rename = "totalMarginUsed")]
    pub total_margin_used: String,
    #[serde(rename = "totalNtlPos")]
    pub total_ntl_pos: String,
}

/// 订单
#[derive(Debug, Serialize)]
pub struct Order {
    pub oid: String,            // Order ID
    pub coin: String,
    pub side: String,           // "B" or "A"
    #[serde(rename = "limitPx")]
    pub limit_px: String,
    pub sz: String,
    #[serde(rename = "origSz")]
    pub orig_sz: String,
    pub timestamp: u64,
}

/// 成交记录
#[derive(Debug, Serialize)]
pub struct Fill {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,           // "B" or "A"
    pub time: u64,
    pub fee: String,
    #[serde(rename = "feeToken")]
    pub fee_token: String,
    pub oid: String,
    pub tid: u64,               // Trade ID
}
```

### 5.3 路由配置

```rust
// src/server.rs
use axum::{routing::post, Router};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/info", post(info_handler))
        .with_state(state)
        .layer(
            tower_http::cors::CorsLayer::permissive()
        )
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
        )
}
```

---

## 6. WebSocket 设计

### 6.1 订阅协议

```rust
// src/ws/protocol.rs
use serde::{Deserialize, Serialize};

/// 客户端请求
#[derive(Debug, Deserialize)]
pub struct WsRequest {
    pub method: String,
    pub subscription: Subscription,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Subscription {
    #[serde(rename = "l2Book")]
    L2Book { coin: String },

    #[serde(rename = "trades")]
    Trades { coin: String },

    #[serde(rename = "candle")]
    Candle { coin: String, interval: String },

    #[serde(rename = "orderUpdates")]
    OrderUpdates { user: String },

    #[serde(rename = "userFills")]
    UserFills { user: String },
}

/// 服务端推送
#[derive(Debug, Serialize)]
pub struct WsMessage {
    pub channel: String,
    pub data: serde_json::Value,
}

/// 订单簿更新
#[derive(Debug, Serialize)]
pub struct L2BookUpdate {
    pub coin: String,
    pub time: u64,
    pub levels: [Vec<L2Level>; 2],
}

/// 成交推送
#[derive(Debug, Serialize)]
pub struct TradeUpdate {
    pub coin: String,
    pub time: u64,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub tid: u64,
}

/// 订单状态更新
#[derive(Debug, Serialize)]
pub struct OrderUpdate {
    pub order: Order,
    pub status: String,
    #[serde(rename = "statusTimestamp")]
    pub status_timestamp: u64,
}
```

### 6.2 WebSocket 服务器

```rust
// src/ws/server.rs
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::sync::{broadcast, RwLock};

pub struct WsServer {
    // 频道 -> 广播器
    channels: RwLock<HashMap<String, broadcast::Sender<String>>>,
}

impl WsServer {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    pub async fn handle_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<WsServer>,
    ) -> Response {
        ws.on_upgrade(|socket| Self::handle_socket(socket, state))
    }

    async fn handle_socket(socket: WebSocket, state: WsServer) {
        let (mut sender, mut receiver) = socket.split();
        let mut subscriptions: Vec<broadcast::Receiver<String>> = vec![];

        // 处理客户端消息
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(request) = serde_json::from_str::<WsRequest>(&text) {
                    match request.method.as_str() {
                        "subscribe" => {
                            let channel = Self::get_channel_name(&request.subscription);
                            let rx = state.subscribe(&channel).await;
                            subscriptions.push(rx);
                        }
                        "unsubscribe" => {
                            // 处理取消订阅
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub async fn subscribe(&self, channel: &str) -> broadcast::Receiver<String> {
        let mut channels = self.channels.write().await;

        if let Some(tx) = channels.get(channel) {
            tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel(1024);
            channels.insert(channel.to_string(), tx);
            rx
        }
    }

    pub async fn broadcast(&self, channel: &str, message: &str) {
        let channels = self.channels.read().await;
        if let Some(tx) = channels.get(channel) {
            let _ = tx.send(message.to_string());
        }
    }

    fn get_channel_name(sub: &Subscription) -> String {
        match sub {
            Subscription::L2Book { coin } => format!("l2Book:{}", coin),
            Subscription::Trades { coin } => format!("trades:{}", coin),
            Subscription::Candle { coin, interval } => format!("candle:{}:{}", coin, interval),
            Subscription::OrderUpdates { user } => format!("orderUpdates:{}", user),
            Subscription::UserFills { user } => format!("userFills:{}", user),
        }
    }
}
```

---

## 7. 部署架构

### 7.1 单机部署

```yaml
# docker-compose.yml
version: '3.8'

services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: dex_indexer
      POSTGRES_USER: indexer
      POSTGRES_PASSWORD: ${DB_PASSWORD}
    volumes:
      - postgres_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"

  redis:
    image: redis:7
    ports:
      - "6379:6379"

  indexer:
    build: ../../../dex/tech
    environment:
      DATABASE_URL: postgres://indexer:${DB_PASSWORD}@postgres/dex_indexer
      REDIS_URL: redis://redis:6379
      DEX_ENGINE_URL: ${DEX_ENGINE_GRPC_URL}
    ports:
      - "8080:8080"   # REST API
      - "8081:8081"   # WebSocket
    depends_on:
      - postgres
      - redis

volumes:
  postgres_data:
```

### 7.2 配置文件

```toml
# config.toml
[server]
http_port = 8080
ws_port = 8081

[grpc]
engine_url = "http://dex-engine:50051"
reconnect_interval_ms = 1000
max_reconnect_attempts = 10

[database]
url = "postgres://indexer:password@localhost/dex_indexer"
max_connections = 20
min_connections = 5

[redis]
url = "redis://localhost:6379"
pool_size = 10

[logging]
level = "info"
format = "json"
```

---

## 8. 监控指标

### 8.1 Prometheus 指标

```rust
// src/metrics.rs
use prometheus::{Counter, Gauge, Histogram, Registry};

pub struct Metrics {
    pub events_received: Counter,
    pub events_processed: Counter,
    pub events_failed: Counter,
    pub event_lag: Gauge,
    pub processing_duration: Histogram,
    pub db_query_duration: Histogram,
    pub ws_connections: Gauge,
    pub ws_subscriptions: Gauge,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            events_received: Counter::new(
                "indexer_events_received_total",
                "Total events received from DEX engine"
            ).unwrap(),
            events_processed: Counter::new(
                "indexer_events_processed_total",
                "Total events successfully processed"
            ).unwrap(),
            events_failed: Counter::new(
                "indexer_events_failed_total",
                "Total events failed to process"
            ).unwrap(),
            event_lag: Gauge::new(
                "indexer_event_lag_seconds",
                "Lag between event timestamp and processing time"
            ).unwrap(),
            processing_duration: Histogram::new(
                "indexer_event_processing_duration_seconds",
                "Event processing duration"
            ).unwrap(),
            db_query_duration: Histogram::new(
                "indexer_db_query_duration_seconds",
                "Database query duration"
            ).unwrap(),
            ws_connections: Gauge::new(
                "indexer_ws_connections",
                "Current WebSocket connections"
            ).unwrap(),
            ws_subscriptions: Gauge::new(
                "indexer_ws_subscriptions",
                "Current WebSocket subscriptions"
            ).unwrap(),
        }
    }
}
```

---

## 9. 附录

### 9.1 错误码

| 错误码 | 说明 |
|--------|------|
| 1001 | 无效请求格式 |
| 1002 | 未知请求类型 |
| 1003 | 缺少必要参数 |
| 2001 | 市场不存在 |
| 2002 | 用户不存在 |
| 3001 | 数据库错误 |
| 3002 | Redis 错误 |
| 4001 | gRPC 连接失败 |

### 9.2 参考链接

- [Hyperliquid API Docs](https://hyperliquid.gitbook.io/hyperliquid-docs)
- [dYdX v4 Architecture](https://dydx.exchange/blog/v4-deep-dive-indexer)
- [tonic gRPC](https://github.com/hyperium/tonic)
- [Diesel ORM](https://diesel.rs/)
