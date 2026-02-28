# 08 API 丰富化

> 日期：2026-02-25
> 依赖：部分无依赖，部分依赖 mark price
> 优先级：P0（硬编码消除）/ P2（格式对标）

---

## 一、当前 API 完整度

### 1.1 Info 端点清单（19 个已实现）

| 端点 | 状态 | 差距 |
|------|------|------|
| `userFills` | ✅ 完整 | 无 is_liquidation 标记 |
| `userBalances` | ✅ 完整 | — |
| `userTransfers` | ✅ 完整 | — |
| `recentFills` | ✅ 完整 | HL 叫 `recentTrades` |
| `clearinghouseState` | ⚠️ 核心硬编码 | 见下文 |
| `meta` | ⚠️ 缺字段 | 见下文 |
| `openOrders` | ✅ 完整 | — |
| `l2Book` | ✅ 基本完整 | 缺每档订单数 `n` |
| `candleSnapshot` | ✅ 基本完整 | 仅 6 种周期 |
| `marketStats` | ✅ 完整 | — |
| `allMids` | ✅ 完整 | — |
| `orderStatus` | ✅ 完整 | — |
| `historicalOrders` | ✅ 完整 | — |
| `subAccounts` | ✅ 完整 | — |
| `userNonFundingLedgerUpdates` | ⚠️ 缺类型 | 无 liquidation 类型 |
| `userFillsByTime` | ✅ 完整 | — |
| `userFunding` | ✅ 完整 | — |
| `fundingHistory` | ✅ 完整 | — |
| `userRateLimit` | ⚠️ 占位 | 固定返回值 |

### 1.2 缺失端点（3 个）

| 端点 | 重要性 | 依赖 |
|------|--------|------|
| `metaAndAssetCtxs` | **高** | mark price（详见 03-mark-price.md） |
| `frontendOpenOrders` | 中 | TP/SL 订单（详见 02-order-types.md） |
| `userLiquidations`（自定义） | 低 | Liquidation handler（详见 05-liquidation.md） |

---

## 二、P0 消除硬编码

### 2.1 clearinghouseState 问题清单

| 问题 | 当前值 | 正确实现 |
|------|--------|---------|
| `leverage.type` | "cross" | 从 `dex_positions.margin_mode` 读取 |
| `leverage.value` | 1 | 从 `dex_positions.leverage_value` 读取 |
| `unrealizedPnl` | 用 mid price | 应用 mark price（回退 mid price） |
| `liquidationPx` | None | 需计算（详见 05-liquidation.md） |
| `marginUsed` | position_value / max_leverage | position_value / leverage_value |
| `cumFunding` | 无 | 从 `dex_positions.cum_funding_*` 读取 |

### 2.2 meta 问题清单

**当前实现**：
```rust
fn get_market_config(perpetual_id: i32) -> (i32, i32) {
    match perpetual_id {
        0 => (4, 20),   // BTC
        1 => (3, 20),   // ETH
        _ => (4, 10),
    }
}
```

**正确实现**：从 `dex_perpetuals` 表读取 `sz_decimals`, `max_leverage`（M4 migration 后可用）。

```rust
// 改为
async fn get_market_config(db: &Db, perpetual_id: i32) -> Result<MarketConfig> {
    use crate::schema::dex_perpetuals;
    let perp = dex_perpetuals::table
        .filter(dex_perpetuals::perpetual_id.eq(perpetual_id))
        .select((
            dex_perpetuals::sz_decimals,
            dex_perpetuals::max_leverage,
            dex_perpetuals::initial_margin_ppm,
            dex_perpetuals::maintenance_margin_ppm,
        ))
        .first::<(i32, i32, i32, i32)>(db.get().await?.deref_mut())?;
    Ok(MarketConfig {
        sz_decimals: perp.0,
        max_leverage: perp.1,
        initial_margin_ppm: perp.2,
        maintenance_margin_ppm: perp.3,
    })
}
```

### 2.3 MarketConfig 结构

```rust
pub struct MarketConfig {
    pub sz_decimals: i32,
    pub max_leverage: i32,
    pub initial_margin_ppm: i32,       // 百万分之
    pub maintenance_margin_ppm: i32,
}
```

### 2.4 meta 响应改进

```rust
// query_meta() 改用 DB 数据
pub async fn query_meta(db: &Db, _req: MetaRequest) -> Result<MetaResponse> {
    let perpetuals = dex_perpetuals::table
        .order(dex_perpetuals::perpetual_id.asc())
        .load::<StoredPerpetual>(db.get().await?.deref_mut())?;

    let universe = perpetuals.iter().map(|p| PerpetualInfo {
        perpetual_id: p.perpetual_id,
        object_id: format!("0x{}", hex::encode(&p.object_id)),
        liquidity_tier_id: p.liquidity_tier_id,
        atomic_resolution: p.atomic_resolution,
        coin: p.ticker.clone(),
        sz_decimals: p.sz_decimals,          // 从 DB 读取
        max_leverage: p.max_leverage,        // 从 DB 读取
        created_at_ms: p.timestamp_ms,
    }).collect();

    Ok(MetaResponse { universe })
}
```

---

## 三、额外 Candle 周期

### 3.1 当前支持

6 种：`1m`, `5m`, `15m`, `1h`, `4h`, `1d`

### 3.2 Hyperliquid 支持

14 种：`1m`, `3m`, `5m`, `15m`, `30m`, `1h`, `2h`, `4h`, `8h`, `12h`, `1d`, `3d`, `1w`, `1M`

### 3.3 新增周期

| 周期 | 毫秒数 | 说明 |
|------|--------|------|
| `3m` | 180_000 | |
| `30m` | 1_800_000 | |
| `2h` | 7_200_000 | |
| `8h` | 28_800_000 | |
| `12h` | 43_200_000 | |
| `3d` | 259_200_000 | |
| `1w` | 604_800_000 | |
| `1M` | 动态 | 月级别 |

### 3.4 修改位置

**CandleAggregator（fills.rs）**：

```rust
// 当前间隔定义
const INTERVALS: &[(&str, i64)] = &[
    ("1m", 60_000),
    ("5m", 300_000),
    ("15m", 900_000),
    ("1h", 3_600_000),
    ("4h", 14_400_000),
    ("1d", 86_400_000),
];

// 扩展为
const INTERVALS: &[(&str, i64)] = &[
    ("1m", 60_000),
    ("3m", 180_000),
    ("5m", 300_000),
    ("15m", 900_000),
    ("30m", 1_800_000),
    ("1h", 3_600_000),
    ("2h", 7_200_000),
    ("4h", 14_400_000),
    ("8h", 28_800_000),
    ("12h", 43_200_000),
    ("1d", 86_400_000),
    ("3d", 259_200_000),
    ("1w", 604_800_000),
    // "1M" 需要特殊处理
];
```

**candleSnapshot API**：

```rust
// 间隔验证
const VALID_INTERVALS: &[&str] = &[
    "1m", "3m", "5m", "15m", "30m",
    "1h", "2h", "4h", "8h", "12h",
    "1d", "3d", "1w", "1M",
];
```

**WS candle 频道**：自动支持（按字符串匹配 interval）。

### 3.5 月级别 candle 处理

`1M` 周期需要特殊的时间戳计算：

```rust
fn get_month_start(timestamp_ms: i64) -> i64 {
    let dt = Utc.timestamp_millis_opt(timestamp_ms).unwrap();
    let month_start = Utc.with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0).unwrap();
    month_start.timestamp_millis()
}

fn get_next_month_start(timestamp_ms: i64) -> i64 {
    let dt = Utc.timestamp_millis_opt(timestamp_ms).unwrap();
    let (year, month) = if dt.month() == 12 {
        (dt.year() + 1, 1)
    } else {
        (dt.year(), dt.month() + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).unwrap().timestamp_millis()
}
```

**建议**：1M 可以暂不实现（非核心），优先实现固定时长的周期。

### 3.6 Redis 资源影响

新增 8 种周期 × N 个市场：
- Redis Sorted Set: 每个市场 8 个新 key（如 `dex:candles:0:3m`）
- Redis Hash: 每个市场 8 个新 key（如 `dex:candle:0:3m`）
- 内存影响较小（candle 数据点紧凑）

---

## 四、l2Book 增强

### 4.1 当前返回格式

```json
{
  "perpetualId": 0,
  "bids": [{"price": "90000", "size": "100"}],
  "asks": [{"price": "90100", "size": "50"}],
  "timestampMs": 1700000000000
}
```

### 4.2 Hyperliquid 格式

```json
{
  "coin": "BTC",
  "levels": [
    [{"px": "90000.0", "sz": "0.5", "n": 3}],   // bids
    [{"px": "90100.0", "sz": "0.3", "n": 2}]     // asks
  ],
  "time": 1700000000000
}
```

### 4.3 差距

| 差异 | DEX | HL | 重要性 |
|------|-----|-----|--------|
| 每档订单数 `n` | 无 | 有 | 低 |
| 价格聚合 `nSigFigs` | 不支持 | 支持 | 低 |
| 结构格式 | `{bids, asks}` | `levels` 数组 | 低（前端适配） |

### 4.4 实现每档订单数

需要 OrderbookSnapshotEvent 扩展：

```rust
// 当前 PriceLevel
pub struct PriceLevel {
    pub price: u64,
    pub quantity: u64,
}

// 扩展为
pub struct PriceLevel {
    pub price: u64,
    pub quantity: u64,
    pub num_orders: u32,    // 该档位的订单数
}
```

这需要引擎在构建 orderbook snapshot 时统计每个价格档位的订单数量。

**优先级低**，可以延后实现。

---

## 五、userNonFundingLedgerUpdates 增强

### 5.1 当前支持的类型

```rust
fn balance_update_type_to_string(update_type: i16) -> &'static str {
    match update_type {
        0 => "deposit",
        1 => "withdraw",
        _ => "unknown",
    }
}
// transfer 从 dex_transfers 表查询，type = "transfer_in" / "transfer_out"
```

### 5.2 需要新增的类型

| 类型 | 来源 | 说明 |
|------|------|------|
| `"liquidation"` | dex_liquidations | 清算导致的余额变化 |
| `"crossChainDeposit"` | dex_balances (type=3) | 跨链充值（依赖工程师 A） |
| `"crossChainWithdraw"` | dex_balances (type=4) | 跨链提款（依赖工程师 A） |

详见 [05-liquidation.md](05-liquidation.md) §4 和 [06-deposit-withdraw.md](06-deposit-withdraw.md) §4。

---

## 六、WS 增强

### 6.1 userFills 频道

Hyperliquid 没有独立的 `userFills` 频道（fills 通过 `user:{address}` 推送）。

如果我们要实现独立的 `userFills` 频道：

```rust
// types.rs
UserFills { address: String },

// consumer.rs - 消费 fills stream
fn handle_fills_for_user_channel(data: &str, ws_state: &WsState) {
    // 解析 taker/maker address
    // 广播到 userFills:{taker_address} 和 userFills:{maker_address}
}
```

**优先级低**：已有 `user:{address}` 频道接收 fills 推送。

### 6.2 userFundings 频道

详见 [04-funding-rate.md](04-funding-rate.md) §3.3。

### 6.3 WS 数据格式对标

当前 DEX 与 Hyperliquid 的 WS 消息格式有系统性差异：

| 差异 | DEX | HL | 建议 |
|------|-----|-----|------|
| trades 推送 | 单对象 | 数组 | **保持现状**（前端适配） |
| l2Book 推送 | `{bids, asks}` | `{levels}` | **保持现状** |
| candle 推送 | 全称字段 + 整数 | 单字母 + 字符串 | **保持现状** |
| allMids 推送 | 增量 | 全量 | 可选改为全量 |
| bbo 推送 | 平铺 | 嵌套 | **保持现状** |
| pong 格式 | `{"type":"pong"}` | `{"channel":"pong"}` | **保持现状** |

**结论**：WS 格式差异是架构选择，前端通过适配层处理。不建议为了对标而改动后端。

---

## 七、userRateLimit 实现

### 7.1 当前实现（占位）

```rust
pub async fn query_user_rate_limit(_req: UserRateLimitRequest) -> Result<UserRateLimitResponse> {
    Ok(UserRateLimitResponse {
        cum_vlm: "0".to_string(),
        n_requests_used: 0,
        n_requests_cap: 1000,
    })
}
```

### 7.2 实际实现方案

**方案 A：Redis 计数器**

```rust
// 在 API 中间件中计数
// Redis key: dex:rate_limit:{address}:{window}
// 使用 Redis INCR + EXPIRE

pub async fn query_user_rate_limit(
    redis: &MultiplexedConnection,
    req: UserRateLimitRequest,
) -> Result<UserRateLimitResponse> {
    let key = format!("dex:rate_limit:{}:1m", req.user);
    let used: i64 = redis.get(&key).await.unwrap_or(0);

    Ok(UserRateLimitResponse {
        cum_vlm: "0".to_string(),  // 需要从 fills 累计
        n_requests_used: used,
        n_requests_cap: 1000,       // 可配置
    })
}
```

**方案 B：暂时保持占位**

rate limit 实现需要 API 中间件支持，可以延后。

**推荐方案 B**：当前阶段不是核心功能。

---

## 八、PerpetualInfo 增强

### 8.1 当前字段

```rust
pub struct PerpetualInfo {
    pub perpetual_id: i32,
    pub object_id: String,
    pub liquidity_tier_id: i32,
    pub atomic_resolution: i32,
    pub coin: String,             // ticker
    pub sz_decimals: i32,
    pub max_leverage: i32,
    pub created_at_ms: i64,
}
```

### 8.2 Hyperliquid 完整字段

```json
{
  "name": "BTC",
  "szDecimals": 5,
  "maxLeverage": 50,
  "onlyIsolated": false,
  "fundingInterval": "1h",
  "markMethod": "oracle"
}
```

### 8.3 建议新增字段

```rust
pub struct PerpetualInfo {
    // 现有字段...

    // 新增（M4 migration 后）
    pub initial_margin_ppm: i32,      // 初始保证金比例
    pub maintenance_margin_ppm: i32,  // 维持保证金比例

    // 可选新增
    pub only_isolated: bool,          // 是否仅支持逐仓（默认 false）
    pub funding_interval: String,     // "1h" | "8h"（默认 "8h"）
}
```

---

## 九、文件清单

### P0 工作（消除硬编码，依赖 M4 migration）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-api/src/handlers.rs` | 修改 | get_market_config 从 DB 读取 |
| `dex-types/src/api/responses.rs` | 修改 | PerpetualInfo 新字段 |

### P2 工作（额外 candle 周期）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-indexer/src/handlers/fills.rs` | 修改 | INTERVALS 数组扩展 |
| `dex-api/src/handlers.rs` | 修改 | VALID_INTERVALS 扩展 |

### P2 工作（其他增强）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-api/src/handlers.rs` | 修改 | userNonFundingLedgerUpdates 增加 liquidation 类型 |
| `dex-api/src/ws/types.rs` | 修改 | UserFills/UserFundings 频道（可选） |
| `dex-api/src/ws/consumer.rs` | 修改 | 新频道广播（可选） |
