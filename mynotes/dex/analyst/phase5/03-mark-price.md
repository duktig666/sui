# 03 标记价格 / Oracle

> 日期：2026-02-25
> 依赖：工程师 B（Oracle 标记价格引擎实现）
> 优先级：P1（交易页面核心数据）

---

## 一、当前状态

### 1.1 未实现盈亏计算使用 mark price

当前 `clearinghouseState` 使用 orderbook 中间价（mid price）计算 unrealizedPnl：

```rust
// handlers.rs - query_clearinghouse_state()
// Redis: 批量获取 mid price
// dex:orderbook:{id} → mid_price 字段
let unrealized_pnl = (mid_price - entry_price) * position_size;
```

**问题**：mid price 容易被操纵，且 orderbook 为空时无法计算。应使用 mark price（综合 Oracle + orderbook 信息）。

### 1.2 无 mark price 事件和存储

- `dex_events.rs` 中无 `MarkPriceUpdateEvent`
- 无 `dex_mark_prices` 表（M3 migration 将创建）
- 无 Redis Hash `dex:mark_price:{id}`
- 无 mark prices handler

### 1.3 无 metaAndAssetCtxs 端点

Hyperliquid 的 `metaAndAssetCtxs` 是交易页面最核心的端点，包含 mark price、funding rate、OI 等实时数据。当前完全缺失。

### 1.4 无 activeAssetCtx WS 频道

实时标记价格/资金费率推送频道缺失。

---

## 二、Hyperliquid 规范

### 2.1 metaAndAssetCtxs 端点

请求：
```json
{"type": "metaAndAssetCtxs"}
```

响应（数组嵌套）：
```json
[
  {
    "universe": [
      {"name": "BTC", "szDecimals": 5, "maxLeverage": 50, ...},
      {"name": "ETH", "szDecimals": 4, "maxLeverage": 50, ...}
    ]
  },
  [
    {
      "funding": "0.00003125",
      "openInterest": "1234.5",
      "prevDayPx": "89500.0",
      "dayNtlVlm": "5000000.0",
      "premium": "0.0001",
      "oraclePx": "90000.5",
      "markPx": "90001.2",
      "midPx": "90002.0",
      "impactPxs": ["89999.0", "90003.0"]
    },
    ...
  ]
]
```

**关键字段**：
- `markPx` — 标记价格（用于 PnL 计算和清算判断）
- `oraclePx` — Oracle 价格（外部价格源）
- `funding` — 当前预测资金费率
- `openInterest` — 持仓量（全市场）
- `premium` — mark price 与 oracle price 的偏差
- `prevDayPx` — UTC 00:00 的收盘价
- `dayNtlVlm` — 24h 名义交易量
- `midPx` — orderbook 中间价
- `impactPxs` — 冲击价格（买入/卖出一定量的实际成交均价）

### 2.2 activeAssetCtx WS 频道

订阅：
```json
{"method": "subscribe", "subscription": {"type": "activeAssetCtx", "coin": "BTC"}}
```

推送格式：
```json
{
  "channel": "activeAssetCtx",
  "data": {
    "coin": "BTC",
    "ctx": {
      "funding": "0.00003125",
      "openInterest": "1234.5",
      "prevDayPx": "89500.0",
      "dayNtlVlm": "5000000.0",
      "premium": "0.0001",
      "oraclePx": "90000.5",
      "markPx": "90001.2",
      "midPx": "90002.0",
      "impactPxs": ["89999.0", "90003.0"]
    }
  }
}
```

### 2.3 clearinghouseState 对 mark price 的使用

Hyperliquid 的 `clearinghouseState` 使用 mark price 计算：
- `unrealizedPnl` = `(markPx - entryPx) * szi`（而非 mid price）
- `liquidationPx` = 基于 mark price + maintenance margin 计算
- `positionValue` = `markPx * szi`

---

## 三、需要引擎提供的事件

### 3.1 MarkPriceUpdateEvent（新事件）

```rust
// dex_events.rs - 需要工程师 B 新增
pub struct MarkPriceUpdateEvent {
    pub perpetual_id: u32,
    pub mark_price: u64,       // subticks
    pub oracle_price: u64,     // subticks
    pub funding_rate: i64,     // scaled by 1e18
    pub open_interest: u64,    // quantums（全市场总持仓）
    pub premium: i64,          // mark - oracle 偏差，scaled
    pub timestamp_ms: u64,
}
```

**发射时机**：每个 checkpoint（或至少每 N 秒）

**讨论点**：
- 频率：Hyperliquid 每 3 秒更新一次。如果每个 checkpoint 都发射，频率取决于 checkpoint 间隔
- OI 计算：引擎需要维护全市场持仓量汇总
- funding_rate 是"预测"费率（下一次结算的预期值），与 FundingSettlementEvent 的实际费率不同

---

## 四、Indexer 实现

### 4.1 新 Handler: mark_prices.rs

```rust
// dex-indexer/src/handlers/mark_prices.rs

pub struct MarkPrices;

impl Processor for MarkPrices {
    const NAME: &'static str = "dex_mark_prices";
    type Value = StoredMarkPrice;

    fn process(checkpoint: &CheckpointData) -> Result<Vec<Self::Value>> {
        // 从 checkpoint 提取 MarkPriceUpdateEvent
        // 转换为 StoredMarkPrice
    }
}

impl Handler for MarkPrices {
    async fn commit(values: &[Self::Value], conn: &mut PgConnection) -> Result<usize> {
        // 1. INSERT INTO dex_mark_prices (ON CONFLICT DO NOTHING)
        // 2. Redis HSET dex:mark_price:{perpetual_id}
        //    - mark_price, oracle_price, funding_rate, open_interest, premium, timestamp_ms
        // 3. Redis XADD dex:stream:mark_prices
    }
}
```

**StoredMarkPrice**：
```rust
pub struct StoredMarkPrice {
    pub perpetual_id: i32,
    pub mark_price: i64,
    pub oracle_price: i64,
    pub funding_rate: i64,
    pub open_interest: i64,
    pub premium: i64,
    pub timestamp_ms: i64,
}
```

### 4.2 数据流

```
引擎 → MarkPriceUpdateEvent
    ↓
mark_prices handler
    ├─→ dex_mark_prices (PG)    — 历史记录
    ├─→ dex:mark_price:{id} (Redis Hash)  — 最新状态
    └─→ dex:stream:mark_prices (Redis Stream)  — 实时推送
```

### 4.3 数据裁剪

mark price 数据量大（每个 checkpoint × 每个市场），需要裁剪策略：
- 保留最近 7 天的详细数据
- 可选：按小时/天聚合的历史数据

---

## 五、API 实现

### 5.1 metaAndAssetCtxs 端点

```rust
// requests.rs
pub struct MetaAndAssetCtxsRequest {}

// handlers.rs
pub async fn query_meta_and_asset_ctxs(
    db: &Db,
    redis: &MultiplexedConnection,
    _req: MetaAndAssetCtxsRequest,
) -> Result<MetaAndAssetCtxsResponse> {
    // 1. 查询 meta（复用 query_meta 逻辑）
    let meta = query_meta(db, MetaRequest {}).await?;

    // 2. 对每个 perpetual，组合多个 Redis 数据源
    let mut asset_ctxs = vec![];
    for perp in &meta.universe {
        let id = perp.perpetual_id;

        // mark price 数据
        let mark_data: HashMap<String, String> = redis.hgetall(
            format!("dex:mark_price:{}", id)
        ).await.unwrap_or_default();

        // market stats 数据
        let stats_data: HashMap<String, String> = redis.hgetall(
            format!("dex:market:{}", id)
        ).await.unwrap_or_default();

        // orderbook mid price
        let mid_price: Option<String> = redis.hget(
            format!("dex:orderbook:{}", id), "mid_price"
        ).await.ok();

        asset_ctxs.push(AssetCtx {
            perpetual_id: id,
            mark_px: mark_data.get("mark_price").cloned().unwrap_or_default(),
            oracle_px: mark_data.get("oracle_price").cloned().unwrap_or_default(),
            funding: mark_data.get("funding_rate").cloned().unwrap_or_default(),
            open_interest: mark_data.get("open_interest").cloned().unwrap_or_default(),
            premium: mark_data.get("premium").cloned().unwrap_or_default(),
            day_ntl_vlm: stats_data.get("volume_24h").cloned().unwrap_or_default(),
            prev_day_px: "0".to_string(),  // TODO: 需要从 candle 历史获取
            mid_px: mid_price.unwrap_or_default(),
        });
    }

    Ok(MetaAndAssetCtxsResponse { meta, asset_ctxs })
}
```

### 5.2 clearinghouseState 改用 mark price

```rust
// handlers.rs - query_clearinghouse_state()
// 修改：从 dex:mark_price:{id} 读取 mark_price，替代 orderbook mid_price
// 回退：如果 mark_price 不存在，继续使用 mid_price

let mark_price = redis.hget::<_, _, Option<String>>(
    format!("dex:mark_price:{}", perpetual_id), "mark_price"
).await.ok().flatten();

let price_for_pnl = mark_price
    .or(mid_price)
    .and_then(|p| p.parse::<i64>().ok())
    .unwrap_or(0);
```

### 5.3 server.rs 路由

```rust
InfoRequest::MetaAndAssetCtxs(req) => {
    handlers::query_meta_and_asset_ctxs(&state.db, redis, req).await
}
```

---

## 六、WS 实现

### 6.1 activeAssetCtx 频道

**consumer.rs 新增广播逻辑**：

```rust
// 消费 dex:stream:mark_prices
// 广播到 activeAssetCtx:{perpetual_id} channel
fn handle_mark_price_message(data: &str, ws_state: &WsState) {
    if let Ok(msg) = serde_json::from_str::<MarkPriceStreamMessage>(data) {
        let channel = format!("activeAssetCtx:{}", msg.perpetual_id);
        let server_msg = ServerMessage::ChannelData {
            channel,
            data: json!({
                "perpetualId": msg.perpetual_id,
                "markPx": msg.mark_price.to_string(),
                "oraclePx": msg.oracle_price.to_string(),
                "funding": msg.funding_rate.to_string(),
                "openInterest": msg.open_interest.to_string(),
                "premium": msg.premium.to_string(),
                "timestampMs": msg.timestamp_ms,
            }),
        };
        ws_state.broadcast_to_channel(&channel, &server_msg);
    }
}
```

### 6.2 订阅时快照

类似 candle 频道，订阅 `activeAssetCtx:{id}` 时推送当前 mark price 快照：

```rust
// 从 Redis Hash dex:mark_price:{id} 读取当前值
async fn fetch_mark_price_snapshot(redis, perpetual_id: i32) -> Option<ServerMessage> {
    let data: HashMap<String, String> = redis.hgetall(
        format!("dex:mark_price:{}", perpetual_id)
    ).await.ok()?;
    // 构造快照消息
}
```

---

## 七、对其他模块的影响

### 7.1 clearinghouseState 连锁影响

mark price 实现后，以下计算全部改进：
- `unrealizedPnl` — 使用 mark price（当前用 mid price）
- `positionValue` — 使用 mark price
- `accountValue` — 基于更准确的 PnL
- `marginUsed` — 可选使用 mark price
- `liquidationPx` — 基于 mark price + maintenance margin 计算（详见 05-liquidation.md）

### 7.2 资金费率连锁影响

mark price 事件中的 `funding_rate` 是"预测"费率，用于：
- `metaAndAssetCtxs` 端点展示
- `activeAssetCtx` WS 推送
- 前端资金费率倒计时展示

详见 [04-funding-rate.md](04-funding-rate.md)。

---

## 八、prevDayPx 计算方案

Hyperliquid 返回 `prevDayPx`（UTC 00:00 的收盘价），用于展示 24h 价格变化。

**方案 A：从 candle 数据获取**
```rust
// 查询昨天 UTC 00:00 的 1d candle 的 close 价格
let yesterday_close = dex_candles::table
    .filter(dex_candles::perpetual_id.eq(id))
    .filter(dex_candles::interval.eq("1d"))
    .filter(dex_candles::timestamp_ms.lt(today_utc_start_ms))
    .order(dex_candles::timestamp_ms.desc())
    .select(dex_candles::close)
    .first::<i64>(db)
    .ok();
```

**方案 B：Redis 缓存**
- 每天 UTC 00:00 定时更新 `dex:prev_day_px:{id}` key
- 通过 mark price handler 在跨天时检测并更新

推荐方案 A（简单，可缓存在 Redis 中）。

---

## 九、文件清单

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `sui-types/src/dex_events.rs` | **引擎新增** | MarkPriceUpdateEvent |
| `dex-indexer/src/handlers/mark_prices.rs` | 新建 | Mark price handler |
| `dex-indexer/src/handlers/mod.rs` | 修改 | 注册新 handler |
| `dex-indexer/src/schema/mod.rs` | 修改 | StoredMarkPrice + table! |
| `dex-types/src/api/responses.rs` | 修改 | MetaAndAssetCtxsResponse, AssetCtx |
| `dex-types/src/api/requests.rs` | 修改 | MetaAndAssetCtxsRequest |
| `dex-api/src/handlers.rs` | 修改 | query_meta_and_asset_ctxs + 改 PnL 计算 |
| `dex-api/src/server.rs` | 修改 | MetaAndAssetCtxs 路由 |
| `dex-api/src/ws/types.rs` | 修改 | ActiveAssetCtx channel type |
| `dex-api/src/ws/consumer.rs` | 修改 | mark_prices stream 消费 + 广播 |
| `dex-api/src/ws/handler.rs` | 修改 | 订阅快照 |

---

## 十、依赖关系

```
M3 (dex_mark_prices 表)
    ↓
MarkPriceUpdateEvent (引擎)  ← 等待工程师 B
    ↓
mark_prices.rs handler (indexer)
    ├─→ dex:mark_price:{id} (Redis Hash)
    ├─→ dex:stream:mark_prices (Redis Stream)
    └─→ dex_mark_prices (PG)
        ↓
    ┌───────────────────────────────┐
    │ metaAndAssetCtxs API          │
    │ clearinghouseState PnL 改进   │
    │ activeAssetCtx WS 频道        │
    └───────────────────────────────┘
```
