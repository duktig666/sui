# 01 现阶段可做的准备工作（不依赖引擎）

> 日期：2026-02-25
> 依赖：无引擎依赖，可立即开始
> 优先级：P0

---

## 一、数据库 Migrations

### M1: dex_liquidations 新表

LiquidationEvent 已在 `sui-types/src/dex_events.rs` 定义，但无 handler 和表。

```sql
-- up.sql: 2026-02-25-000001_dex_liquidations

CREATE TABLE dex_liquidations (
    cp_sequence_number      BIGINT NOT NULL,
    tx_sequence_number      BIGINT NOT NULL,
    event_index             INT NOT NULL,
    tx_digest               BYTEA NOT NULL,
    perpetual_id            INT NOT NULL,
    liquidated_account_address    TEXT NOT NULL,
    liquidated_subaccount_number  INT NOT NULL,
    liquidator_account_address    TEXT NOT NULL,
    liquidator_subaccount_number  INT NOT NULL,
    size_liquidated         BIGINT NOT NULL,
    liquidation_price       BIGINT NOT NULL,
    insurance_payout        BIGINT NOT NULL,
    timestamp_ms            BIGINT NOT NULL,
    PRIMARY KEY (cp_sequence_number, tx_sequence_number, event_index)
);

-- 索引设计（参照 dex_fills 模式）
CREATE INDEX idx_liquidations_liquidated ON dex_liquidations (liquidated_account_address, timestamp_ms DESC);
CREATE INDEX idx_liquidations_liquidator ON dex_liquidations (liquidator_account_address, timestamp_ms DESC);
CREATE INDEX idx_liquidations_perpetual ON dex_liquidations (perpetual_id, timestamp_ms DESC);
CREATE INDEX idx_liquidations_time ON dex_liquidations (timestamp_ms DESC);
```

**设计说明**：
- PK 与其他事件表一致：`(cp_sequence_number, tx_sequence_number, event_index)`
- 子账户拆分为 `account_address TEXT` + `subaccount_number INT`（与 subaccount_split 迁移保持一致）
- `size_liquidated` / `liquidation_price` / `insurance_payout` 均为 `u64`，用 `BIGINT` 存储
- 索引支持"查询被清算账户"、"查询清算人"、"按市场查"、"按时间查"四种查询模式

### M2: dex_positions 增加杠杆列

```sql
-- up.sql: 2026-02-25-000002_positions_leverage

ALTER TABLE dex_positions
    ADD COLUMN leverage_value INT NOT NULL DEFAULT 0,
    ADD COLUMN margin_mode SMALLINT NOT NULL DEFAULT 0,
    ADD COLUMN isolated_margin BYTEA,
    ADD COLUMN cum_funding_since_open BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN cum_funding_all_time BIGINT NOT NULL DEFAULT 0;
```

**字段说明**：

| 字段 | 类型 | 说明 |
|------|------|------|
| `leverage_value` | INT | 杠杆倍数（整数，如 20 表示 20x） |
| `margin_mode` | SMALLINT | 0=cross（全仓）, 1=isolated（逐仓） |
| `isolated_margin` | BYTEA | 逐仓保证金（i128, 16 字节 LE），全仓时 NULL |
| `cum_funding_since_open` | BIGINT | 自开仓以来累计资金费 |
| `cum_funding_all_time` | BIGINT | 账户总累计资金费 |

**Hyperliquid 对应字段**：
- `leverage.type` → `margin_mode`（"cross" / "isolated"）
- `leverage.value` → `leverage_value`
- `leverage.rawUsd` → `isolated_margin`（逐仓时为保证金金额）
- `cumFunding.sinceOpen` → `cum_funding_since_open`
- `cumFunding.allTime` → `cum_funding_all_time`

### M3: dex_mark_prices 新表

为后续 MarkPriceUpdateEvent handler 准备。

```sql
-- up.sql: 2026-02-25-000003_dex_mark_prices

CREATE TABLE dex_mark_prices (
    perpetual_id    INT NOT NULL,
    mark_price      BIGINT NOT NULL,
    oracle_price    BIGINT NOT NULL,
    funding_rate    BIGINT NOT NULL,
    open_interest   BIGINT NOT NULL,
    premium         BIGINT NOT NULL DEFAULT 0,
    timestamp_ms    BIGINT NOT NULL,
    PRIMARY KEY (perpetual_id, timestamp_ms)
);

CREATE INDEX idx_mark_prices_time ON dex_mark_prices (timestamp_ms DESC);
CREATE INDEX idx_mark_prices_perpetual_time ON dex_mark_prices (perpetual_id, timestamp_ms DESC);
```

**设计说明**：
- PK 为 `(perpetual_id, timestamp_ms)`，支持按市场按时间查询
- 不用 `cp_sequence_number` 因为 mark price 可能每个 checkpoint 更新多次
- `funding_rate` 是 i64 (scaled by 1e18)，存为 BIGINT
- `premium` 为预测溢价，用于展示预期资金费率

### M4: dex_perpetuals 增加配置列

```sql
-- up.sql: 2026-02-25-000004_perpetuals_config

ALTER TABLE dex_perpetuals
    ADD COLUMN max_leverage INT NOT NULL DEFAULT 20,
    ADD COLUMN sz_decimals INT NOT NULL DEFAULT 4,
    ADD COLUMN initial_margin_ppm INT NOT NULL DEFAULT 50000,
    ADD COLUMN maintenance_margin_ppm INT NOT NULL DEFAULT 30000;
```

**字段说明**：

| 字段 | 类型 | 说明 |
|------|------|------|
| `max_leverage` | INT | 最大杠杆（默认 20x） |
| `sz_decimals` | INT | 数量精度小数位（Hyperliquid 对应 `szDecimals`） |
| `initial_margin_ppm` | INT | 初始保证金比例（百万分之，50000 = 5%） |
| `maintenance_margin_ppm` | INT | 维持保证金比例（百万分之，30000 = 3%） |

**来源**：当前 `get_market_config()` 在 handlers.rs 中硬编码了 BTC(sz=4,lev=20) 和 ETH(sz=3,lev=20)。
迁移后从 DB 读取，消除硬编码。

**数据初始化**：migration 后需要 UPDATE 现有记录：
```sql
UPDATE dex_perpetuals SET max_leverage = 20, sz_decimals = 4 WHERE ticker LIKE 'BTC%';
UPDATE dex_perpetuals SET max_leverage = 20, sz_decimals = 3 WHERE ticker LIKE 'ETH%';
```

### M5: dex_orders 增加 TP/SL 列

```sql
-- up.sql: 2026-02-25-000005_orders_tpsl

ALTER TABLE dex_orders
    ADD COLUMN trigger_price BIGINT,
    ADD COLUMN trigger_condition TEXT,
    ADD COLUMN parent_order_id BYTEA,
    ADD COLUMN grouping TEXT NOT NULL DEFAULT 'na';
```

**字段说明**：

| 字段 | 类型 | 说明 |
|------|------|------|
| `trigger_price` | BIGINT, nullable | 触发价格（TP/SL 条件单） |
| `trigger_condition` | TEXT, nullable | 触发条件："tp"（take profit）, "sl"（stop loss） |
| `parent_order_id` | BYTEA, nullable | 父订单 ID（TP/SL 关联的主订单） |
| `grouping` | TEXT | 分组类型："na"（普通单）, "normalTpsl", "positionTpsl" |

**Hyperliquid 对应字段**：
- `triggerPx` → `trigger_price`
- `tpsl` → `trigger_condition`
- `children` / `parentId` → `parent_order_id`
- `orderType` → `grouping`（Hyperliquid 在 `frontendOpenOrders` 端点返回）

---

## 二、Liquidation Handler 实现

详见 [05-liquidation.md](05-liquidation.md)。

核心要点：
- 新文件：`dex-indexer/src/handlers/liquidations.rs`
- 参照 `funding_payments.rs` 模式（事件解析 → PG INSERT → Redis publish）
- 注册到 indexer pipeline（lib.rs 或 main.rs 中 add_handler）

---

## 三、FundingPayments Redis 发布

详见 [04-funding-rate.md](04-funding-rate.md)。

核心修改：
- `dex-indexer/src/handlers/funding_payments.rs` 的 `commit()` 方法增加 Redis Stream 发布
- 新增 Redis Stream key: `dex:stream:funding_settlements`
- 参照 `balances.rs` 的 `tokio::spawn` 异步发布模式

---

## 四、消除 clearinghouseState 硬编码

详见 [08-api-enrichment.md](08-api-enrichment.md)。

核心修改（`dex-api/src/handlers.rs`）：

**当前硬编码**：
```rust
fn get_market_config(perpetual_id: i32) -> (i32, i32) {
    match perpetual_id {
        0 => (4, 20),  // BTC: sz_decimals=4, max_leverage=20
        1 => (3, 20),  // ETH
        _ => (4, 10),  // default
    }
}
```

**改为 DB 查询**：
```rust
// 从 dex_perpetuals 表读取 sz_decimals, max_leverage
fn get_market_config(db: &Db, perpetual_id: i32) -> Result<(i32, i32)> {
    let perp = dex_perpetuals::table
        .filter(dex_perpetuals::perpetual_id.eq(perpetual_id))
        .select((dex_perpetuals::sz_decimals, dex_perpetuals::max_leverage))
        .first(db)?;
    Ok(perp)
}
```

---

## 五、Schema 和类型定义更新

### 5.1 dex-indexer/src/schema/mod.rs

新增表定义：
- `dex_liquidations` — 对应 M1 migration
- `dex_mark_prices` — 对应 M3 migration

修改现有表：
- `dex_positions` — 增加 `leverage_value`, `margin_mode`, `isolated_margin`, `cum_funding_since_open`, `cum_funding_all_time`
- `dex_perpetuals` — 增加 `max_leverage`, `sz_decimals`, `initial_margin_ppm`, `maintenance_margin_ppm`
- `dex_orders` — 增加 `trigger_price`, `trigger_condition`, `parent_order_id`, `grouping`

新增 Stored 类型：
- `StoredLiquidation` — 对应 dex_liquidations 行
- `StoredMarkPrice` — 对应 dex_mark_prices 行

修改现有 Stored 类型：
- `StoredPosition` — 增加新字段（AsChangeset）
- `StoredPerpetual` — 增加新字段
- `StoredOrder` — 增加新字段

### 5.2 dex-types/src/api/responses.rs

新增类型：

```rust
/// metaAndAssetCtxs 端点响应（Part 2 用）
pub struct MetaAndAssetCtxsResponse {
    pub meta: MetaResponse,
    pub asset_ctxs: Vec<AssetCtx>,
}

pub struct AssetCtx {
    pub perpetual_id: i32,
    pub mark_px: String,          // 标记价格
    pub oracle_px: String,        // Oracle 价格
    pub funding: String,          // 当前资金费率
    pub open_interest: String,    // 持仓量
    pub premium: String,          // 溢价
    pub day_ntl_vlm: String,      // 24h 名义交易量
    pub prev_day_px: String,      // 前日收盘价
}

/// 清算记录响应
pub struct LiquidationResponse {
    pub tx_digest: String,
    pub checkpoint: i64,
    pub perpetual_id: i32,
    pub liquidated_account: String,
    pub liquidated_subaccount_number: u8,
    pub liquidator_account: String,
    pub liquidator_subaccount_number: u8,
    pub size_liquidated: i64,
    pub liquidation_price: i64,
    pub insurance_payout: i64,
    pub timestamp_ms: i64,
}

/// 累计资金费（嵌入 AssetPosition）
pub struct CumFunding {
    pub since_open: String,       // i128 as string
    pub all_time: String,
}
```

### 5.3 dex-types/src/api/requests.rs

新增请求类型：

```rust
/// InfoRequest 新增变体
MetaAndAssetCtxs(MetaAndAssetCtxsRequest),
FrontendOpenOrders(FrontendOpenOrdersRequest),

pub struct MetaAndAssetCtxsRequest {}  // 无字段

pub struct FrontendOpenOrdersRequest {
    pub user: String,
    pub subaccount_number: Option<u8>,
}
```

### 5.4 dex-types/src/api/exchange.rs

新增 exchange action（Part 2 用）：

```rust
/// ExchangeAction 新增变体
UpdateLeverage(UpdateLeverageAction),
UpdateIsolatedMargin(UpdateIsolatedMarginAction),

pub struct UpdateLeverageAction {
    pub asset: u32,               // perpetual_id
    pub is_cross: bool,
    pub leverage: u32,
    pub subaccount_number: Option<u32>,
}

pub struct UpdateIsolatedMarginAction {
    pub asset: u32,               // perpetual_id
    pub is_buy: bool,
    pub ntli: i64,                // margin change amount
    pub subaccount_number: Option<u32>,
}
```

---

## 六、WS 频道类型注册

### 6.1 dex-api/src/ws/types.rs

新增 ChannelType 变体：

```rust
// 市场数据频道：实时 mark price、funding rate、OI
ActiveAssetCtx { perpetual_id: i32 },

// 用户级事件频道
UserFills { address: String },
UserFundings { address: String },
```

**解析格式**：
- `"activeAssetCtx:0"` → `ActiveAssetCtx { perpetual_id: 0 }`
- `"userFills:0xabc..."` → `UserFills { address: "0xabc..." }`
- `"userFundings:0xabc..."` → `UserFundings { address: "0xabc..." }`

### 6.2 dex-api/src/ws/consumer.rs

新增 Stream key 常量：

```rust
pub const LIQUIDATIONS:        &str = "dex:stream:liquidations";
pub const FUNDING_SETTLEMENTS: &str = "dex:stream:funding_settlements";
pub const MARK_PRICES:         &str = "dex:stream:mark_prices";
```

广播路由新增：

| Stream | Channel 广播 | Event-type 广播 |
|--------|-------------|----------------|
| `LIQUIDATIONS` | `user:{liquidated_address}` | 无 |
| `FUNDING_SETTLEMENTS` | `userFundings:{address}` | 无 |
| `MARK_PRICES` | `activeAssetCtx:{perpetualId}` | 无 |

---

## 七、Redis Key 设计

新增 Redis key：

| Key | 类型 | 说明 | 写入方 |
|-----|------|------|-------|
| `dex:mark_price:{perpetual_id}` | Hash | mark_price, oracle_price, funding_rate, open_interest, premium | mark_prices handler |
| `dex:stream:liquidations` | Stream | 清算事件 | liquidations handler |
| `dex:stream:funding_settlements` | Stream | 资金费结算事件 | funding_payments handler |
| `dex:stream:mark_prices` | Stream | 标记价格更新 | mark_prices handler |

**Hash 字段设计**（`dex:mark_price:{perpetual_id}`）：

```
mark_price      — 标记价格 (i64 as string)
oracle_price    — Oracle 价格 (i64 as string)
funding_rate    — 当前资金费率 (i64 as string, scaled 1e18)
open_interest   — 持仓量 (i64 as string)
premium         — 溢价 (i64 as string)
timestamp_ms    — 更新时间戳
```

**读取方**：
- `query_clearinghouse_state()` → 用 mark_price 替代 mid_price 计算 unrealizedPnl
- `query_meta_and_asset_ctxs()` → 组合 mark_price + market_stats
- WS `activeAssetCtx` 频道 → 消费 mark_prices stream

---

## 八、额外 Candle 周期

当前支持 6 种：`1m`, `5m`, `15m`, `1h`, `4h`, `1d`

Hyperliquid 支持 14 种，新增 8 种：

| 新增周期 | 分钟数 | 说明 |
|---------|--------|------|
| `3m` | 3 | |
| `30m` | 30 | |
| `2h` | 120 | |
| `8h` | 480 | |
| `12h` | 720 | |
| `3d` | 4320 | |
| `1w` | 10080 | |
| `1M` | 动态 | 月级别，特殊处理 |

**修改位置**：
- `dex-indexer/src/handlers/fills.rs` 中的 `CandleAggregator` 间隔列表
- `dex-api/src/handlers.rs` 中 `query_candle_snapshot` 的间隔验证
- WS candle 频道的间隔解析

**注意**：`1M`（月）需要特殊的时间戳计算逻辑（每月天数不同），可考虑暂不实现或用 30 天近似。

---

## 九、文件清单

### Part 1 需要修改的文件

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-indexer/migrations/2026-02-25-000001_*/up.sql` | 新建 | M1: dex_liquidations |
| `dex-indexer/migrations/2026-02-25-000002_*/up.sql` | 新建 | M2: positions_leverage |
| `dex-indexer/migrations/2026-02-25-000003_*/up.sql` | 新建 | M3: dex_mark_prices |
| `dex-indexer/migrations/2026-02-25-000004_*/up.sql` | 新建 | M4: perpetuals_config |
| `dex-indexer/migrations/2026-02-25-000005_*/up.sql` | 新建 | M5: orders_tpsl |
| `dex-indexer/src/schema/mod.rs` | 修改 | 新增 table! + 修改 Stored 类型 |
| `dex-indexer/src/handlers/liquidations.rs` | 新建 | Liquidation handler |
| `dex-indexer/src/handlers/mod.rs` | 修改 | 注册新 handler |
| `dex-indexer/src/handlers/funding_payments.rs` | 修改 | 增加 Redis publish |
| `dex-api/src/handlers.rs` | 修改 | get_market_config 从 DB 读取 |
| `dex-types/src/api/responses.rs` | 修改 | 新增响应类型 |
| `dex-types/src/api/requests.rs` | 修改 | 新增请求类型 |
| `dex-types/src/api/exchange.rs` | 修改 | 新增 exchange action 类型 |
| `dex-api/src/ws/types.rs` | 修改 | 新增频道类型 |
| `dex-api/src/ws/consumer.rs` | 修改 | 新增 stream key + 广播路由 |
