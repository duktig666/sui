# Phase 6: API/WebSocket 集成设计

> 更新日期: 2026-02-27
> 状态: ⚠️ 需更新 — WS 改为全量推送，移除 l2BookDelta 频道

> **2026-02-27 架构决策变更通知**
>
> 根据 [08-architecture-qa.md](./08-architecture-qa.md) 确认的决策：
> - **Q2=A**: WS l2Book 频道改为**全量推送**（对标 Hyperliquid l2Book），不实现增量 l2BookDelta 频道
> - **Q3=C+**: Checkpoint 通道不再发送订单簿事件
>
> **本文档需更新的部分**：
> - §3 WS 增量推送 → 简化为全量推送，移除 l2BookDelta 频道
> - §3.2 新增 ChannelType → 不需要 `L2BookDelta` variant
> - §3.3 StreamConsumer → 消费 `dex:stream:l2:update` 后读取 HGETALL 推送全量快照
> - §4 消息格式 → 移除 delta 消息格式，仅保留全量快照格式
> - §6 Sequence 同步协议 → 简化（全量推送无需 sequence 同步）
> - §8.3 Hyperliquid 兼容 → l2Book 频道直接对标 Hyperliquid
>
> REST API 部分（l2Book handler、BBO endpoint）保持不变。

## 概述

本文档描述 Phase 6 中 dex-api 的增强方案：将 dex-streamer 产生的低延迟 L2 orderbook 数据通过 REST API 和 WebSocket 推送给客户端。核心变化包括：

1. REST `l2Book` endpoint 改为读取 dex-streamer 的 Redis 数据（`dex:l2book:{id}`），延迟从 1-3s 降至 <50ms
2. WS `l2Book` 频道改为推送**完整 L2 快照**（Q2=A，对标 Hyperliquid l2Book）
3. 新增 `bbo` REST endpoint 和 WebSocket 频道
4. ~~l2BookDelta 增量频道~~ — 本阶段不实现（Q2=A 决策简化）

## 1. 现有架构分析

### 1.1 REST API 现状

当前 `query_l2_book()` 函数位于 `dex-sui/crates/dex-api/src/handlers.rs`（第 696-742 行），从 Redis HSET `dex:orderbook:{perpetual_id}` 读取全量快照：

```rust
// 当前实现：读取 dex-indexer 写入的 checkpoint 快照
let key = format!("dex:orderbook:{}", req.perpetual_id);
let fields: Vec<Option<String>> = redis::cmd("HMGET")
    .arg(&key)
    .arg("bids")
    .arg("asks")
    .arg("timestamp_ms")
    .query_async(&mut conn).await?;
```

数据源是 dex-indexer 按 checkpoint 频率写入的全量 orderbook 快照，延迟较高（checkpoint 间隔）。

### 1.2 WebSocket 现状

WebSocket 架构由以下文件组成（均在 `dex-sui/crates/dex-api/src/ws/` 下）：

| 文件 | 职责 |
|------|------|
| `mod.rs` | 模块导出 |
| `config.rs` | `WsConfig`：Redis URL、block_ms、batch_count |
| `consumer.rs` | `StreamConsumer`：从 Redis Streams (XREAD) 消费事件并广播 |
| `handler.rs` | WebSocket 连接处理、心跳、订阅消息分发 |
| `subscription.rs` | `SubscriptionManager`：管理客户端订阅、消息路由 |
| `types.rs` | `ChannelType`、`ServerMessage`、`SubscriptionRequest` 等类型定义 |
| `snapshot.rs` | Candle 快照获取（订阅时发送） |

**当前订阅频道**（定义在 `types.rs` 的 `ChannelType` 枚举）：

| ChannelType | channel string | 数据来源 |
|-------------|---------------|----------|
| `Trades` | `trades:{id}` | `dex:stream:fills` |
| `OrderBook` | `orderbook:{id}` | `dex:stream:orderbook` |
| `Candle` | `candle:{id}:{interval}` | `dex:stream:candles` |
| `Bbo` | `bbo:{id}` | 从 orderbook 数据中提取 |
| `AllMids` | `allMids` | `dex:stream:market_stats` |
| `User` | `user:{address}` | positions/balances/transfers |
| `OrderUpdates` | `orderUpdates:{address}` | `dex:stream:orders` |
| `ClearinghouseState` | `clearinghouseState:{address}` | positions/balances |
| `OpenOrders` | `openOrders:{address}` | `dex:stream:orders` |
| `Notification` | `notification:{address}` | 预留 |

**StreamConsumer**（`consumer.rs`）当前消费 8 个 Redis Stream：

```rust
pub mod stream_keys {
    pub const FILLS: &str = "dex:stream:fills";
    pub const POSITIONS: &str = "dex:stream:positions";
    pub const BALANCES: &str = "dex:stream:balances";
    pub const TRANSFERS: &str = "dex:stream:transfers";
    pub const ORDERS: &str = "dex:stream:orders";
    pub const ORDERBOOK: &str = "dex:stream:orderbook";
    pub const CANDLES: &str = "dex:stream:candles";
    pub const MARKET_STATS: &str = "dex:stream:market_stats";
}
```

当前 orderbook 推送流程：`dex:stream:orderbook` -> `broadcast_orderbook()` -> `orderbook:{id}` 频道（全量快照）+ `bbo:{id}` 频道（BBO 提取）。

## 2. REST API 增强

### 2.1 l2Book endpoint 改造

改造 `handlers.rs` 中的 `query_l2_book()`，优先从增量 L2 book 读取数据，fallback 到 checkpoint 快照：

```rust
// handlers.rs - Phase 6 enhanced l2Book handler
pub async fn query_l2_book(
    redis: &Option<MultiplexedConnection>,
    req: L2BookRequest,
) -> Result<L2BookResponse> {
    let conn = redis
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Redis not configured"))?;
    let mut conn = conn.clone();

    // Phase 6: 优先从增量 L2 book 读取（dex-streamer 写入）
    let incr_key = format!("dex:l2book:{}", req.perpetual_id);
    let fields: HashMap<String, String> = redis::cmd("HGETALL")
        .arg(&incr_key)
        .query_async(&mut conn)
        .await?;

    if !fields.is_empty() {
        // 解析 b:{price} / a:{price} 格式的字段
        let mut bids = Vec::new();
        let mut asks = Vec::new();
        for (key, value) in &fields {
            // 跳过 meta 字段
            if key == "sequence" || key == "timestamp_ms" {
                continue;
            }
            let qty: i64 = value.parse().unwrap_or(0);
            if qty == 0 {
                continue; // 数量为零表示价格档位已清除
            }
            if let Some(price_str) = key.strip_prefix("b:") {
                if let Ok(price) = price_str.parse::<i64>() {
                    bids.push(L2Level {
                        price: price.to_string(),
                        size: qty.to_string(),
                        count: 1,
                    });
                }
            } else if let Some(price_str) = key.strip_prefix("a:") {
                if let Ok(price) = price_str.parse::<i64>() {
                    asks.push(L2Level {
                        price: price.to_string(),
                        size: qty.to_string(),
                        count: 1,
                    });
                }
            }
        }

        // 排序：bids 降序，asks 升序
        bids.sort_by(|a, b| {
            b.price.parse::<i64>().unwrap_or(0)
                .cmp(&a.price.parse::<i64>().unwrap_or(0))
        });
        asks.sort_by(|a, b| {
            a.price.parse::<i64>().unwrap_or(0)
                .cmp(&b.price.parse::<i64>().unwrap_or(0))
        });

        let depth = req.depth.min(200);
        let timestamp_ms: u64 = fields
            .get("timestamp_ms")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        return Ok(L2BookResponse {
            perpetual_id: req.perpetual_id,
            bids: bids.into_iter().take(depth).collect(),
            asks: asks.into_iter().take(depth).collect(),
            timestamp_ms,
        });
    }

    // Fallback: 读取 checkpoint 快照（现有逻辑）
    let key = format!("dex:orderbook:{}", req.perpetual_id);
    let snapshot_fields: Vec<Option<String>> = redis::cmd("HMGET")
        .arg(&key)
        .arg("bids")
        .arg("asks")
        .arg("timestamp_ms")
        .query_async(&mut conn)
        .await?;

    let bids: Vec<L2Level> = snapshot_fields[0]
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let asks: Vec<L2Level> = snapshot_fields[1]
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let timestamp_ms: u64 = snapshot_fields[2]
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let depth = req.depth.min(200);
    Ok(L2BookResponse {
        perpetual_id: req.perpetual_id,
        bids: bids.into_iter().take(depth).collect(),
        asks: asks.into_iter().take(depth).collect(),
        timestamp_ms,
    })
}
```

**关键设计点**：

- `dex:l2book:{perpetual_id}` 的字段格式为 `b:{price}` / `a:{price}`，value 为数量
- 数量为 0 的字段表示该价格档位已清除，查询时过滤掉
- 保留 `dex:orderbook:{perpetual_id}` 作为 fallback，保证 dex-streamer 未启动时仍可用

### 2.2 新增 BBO endpoint

在 `handlers.rs` 新增 BBO 查询，从 `dex:bbo:{perpetual_id}` 读取：

```rust
/// Query best bid/offer from Redis
pub async fn query_bbo(
    redis: &Option<MultiplexedConnection>,
    req: BboRequest,
) -> Result<BboResponse> {
    let conn = redis
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Redis not configured"))?;
    let mut conn = conn.clone();

    let key = format!("dex:bbo:{}", req.perpetual_id);
    let fields: Vec<Option<String>> = redis::cmd("HMGET")
        .arg(&key)
        .arg("best_bid")
        .arg("best_bid_qty")
        .arg("best_ask")
        .arg("best_ask_qty")
        .arg("mid_price")
        .arg("timestamp_ms")
        .query_async(&mut conn)
        .await?;

    Ok(BboResponse {
        perpetual_id: req.perpetual_id,
        best_bid: fields[0].clone().unwrap_or_default(),
        best_bid_qty: fields[1].clone().unwrap_or_default(),
        best_ask: fields[2].clone().unwrap_or_default(),
        best_ask_qty: fields[3].clone().unwrap_or_default(),
        mid_price: fields[4].clone().unwrap_or_default(),
        timestamp_ms: fields[5]
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
    })
}
```

REST API 请求格式（通过 `/info` endpoint）：

```json
POST /info
{ "type": "bbo", "perpetualId": 0 }
```

响应格式：

```json
{
    "perpetualId": 0,
    "bestBid": "50000",
    "bestBidQty": "15000",
    "bestAsk": "50100",
    "bestAskQty": "8000",
    "midPrice": "50050",
    "timestampMs": 1709000000050
}
```

需在 `types.rs` 的 `InfoRequest` 枚举中新增 `Bbo` variant，并在 `server.rs` 的 `info_handler` 中添加分发逻辑。

### 2.3 allMids endpoint 改造

当前 `query_all_mids()` 从 `dex:orderbook:*` 扫描 `mid_price` 字段。Phase 6 改为优先从 `dex:bbo:{id}` 读取 `mid_price`，数据更新更及时：

```rust
// 优先查 dex:bbo:* 的 mid_price，fallback 到 dex:orderbook:*
```

## 3. WebSocket 增量推送

### 3.1 新增 Redis Stream

dex-streamer 写入新的 Redis Stream：

| Stream Key | 内容 | 生产者 |
|-----------|------|--------|
| `dex:stream:l2:delta` | 增量 L2 book delta | dex-streamer |

每条 delta 消息格式：

```json
{
    "perpetualId": 0,
    "sequence": 43,
    "timestampMs": 1709000000050,
    "updates": [
        { "side": "bid", "price": "50000", "size": "15000" },
        { "side": "ask", "price": "51000", "size": "0" }
    ]
}
```

`size` 为 "0" 表示删除该价格档位。

### 3.2 新增 ChannelType

在 `dex-sui/crates/dex-api/src/ws/types.rs` 的 `ChannelType` 枚举中新增：

```rust
pub enum ChannelType {
    // ... 现有 variants ...

    /// Incremental L2 book delta: l2BookDelta:{perpetual_id}
    L2BookDelta { perpetual_id: i32 },
}
```

对应解析和序列化：

```rust
impl ChannelType {
    pub fn parse(channel: &str) -> Option<Self> {
        let parts: Vec<&str> = channel.split(':').collect();
        match parts.as_slice() {
            // ... 现有匹配 ...
            ["l2BookDelta", perpetual_id] => perpetual_id
                .parse()
                .ok()
                .map(|id| ChannelType::L2BookDelta { perpetual_id: id }),
            _ => None,
        }
    }

    pub fn to_channel_string(&self) -> String {
        match self {
            // ... 现有匹配 ...
            ChannelType::L2BookDelta { perpetual_id } => {
                format!("l2BookDelta:{}", perpetual_id)
            }
        }
    }
}
```

### 3.3 StreamConsumer 增强

在 `dex-sui/crates/dex-api/src/ws/consumer.rs` 中添加新的 stream key 和消费逻辑：

```rust
pub mod stream_keys {
    // ... 现有 keys ...
    pub const L2_DELTA: &str = "dex:stream:l2:delta";
}
```

`StreamIds` 新增字段：

```rust
struct StreamIds {
    // ... 现有字段 ...
    l2_delta: String,
}
```

`read_streams()` 在 XREAD 参数中加入 `dex:stream:l2:delta`。

`broadcast_message()` 新增路由分支：

```rust
s if s == stream_keys::L2_DELTA => {
    self.broadcast_l2_delta(&data);
}
```

新增广播函数：

```rust
/// 广播增量 L2 book delta 到 l2BookDelta:{id} 频道
fn broadcast_l2_delta(&self, data: &serde_json::Value) {
    let perpetual_id = match data.get("perpetualId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => {
            warn!("L2 delta message missing perpetualId");
            return;
        }
    };

    // Delta 消息直接推送到 l2BookDelta:{id} 频道
    let delta_msg = serde_json::json!({
        "type": "delta",
        "coin": data.get("coin").unwrap_or(&serde_json::Value::Null),
        "sequence": data.get("sequence").unwrap_or(&serde_json::Value::Null),
        "time": data.get("timestampMs").unwrap_or(&serde_json::Value::Null),
        "updates": data.get("updates").unwrap_or(&serde_json::Value::Null),
    });

    let channel = format!("l2BookDelta:{}", perpetual_id);
    self.manager
        .broadcast(ServerMessage::channel_data(&channel, delta_msg));

    // 同时从 delta 更新中提取 BBO 变化（如果有）
    self.maybe_broadcast_bbo_from_delta(perpetual_id, data);
}

/// 如果 delta 影响了 best bid/ask，推送 BBO 更新
fn maybe_broadcast_bbo_from_delta(
    &self,
    perpetual_id: i64,
    data: &serde_json::Value,
) {
    // BBO 由 dex-streamer 在 Redis 中维护，此处通过 Redis pub/sub 或
    // 直接从 delta stream 中标记的 bbo_changed 字段判断是否需要推送
    if let Some(bbo) = data.get("bbo") {
        let bbo_channel = format!("bbo:{}", perpetual_id);
        self.manager
            .broadcast(ServerMessage::channel_data(&bbo_channel, bbo.clone()));
    }
}
```

### 3.4 l2BookDelta 订阅流程

客户端订阅 `l2BookDelta:{perpetual_id}` 时的完整流程：

```
客户端                  dex-api (handler.rs)              Redis
  │                         │                               │
  │─ subscribeChannel ────→│                               │
  │  "l2BookDelta:0"       │                               │
  │                        │── HGETALL dex:l2book:0 ─────→│
  │                        │← {b:50000=1.5, a:50100=0.8} ──│
  │                        │── GET dex:l2book:0:sequence ─→│
  │                        │← 42 ──────────────────────────│
  │                        │                               │
  │← snapshot message ─────│                               │
  │  {type:"snapshot",     │                               │
  │   sequence:42,         │                               │
  │   levels:[...]}        │                               │
  │                        │                               │
  │← channelResponse ──────│                               │
  │  {success:true}        │                               │
  │                        │                               │
  │  ··· 后续增量推送 ···    │                               │
  │                        │                               │
  │                        │← XREAD dex:stream:l2:delta ───│
  │← delta message ────────│                               │
  │  {type:"delta",        │                               │
  │   sequence:43,         │                               │
  │   updates:[...]}       │                               │
```

在 `handler.rs` 的 `handle_client_message()` 中，`SubscribeChannel` 分支需要为 `L2BookDelta` 类型发送快照：

```rust
SubscriptionRequest::SubscribeChannel(params) => {
    let channel_str = params.channel.clone();
    match ChannelType::parse(&params.channel) {
        Some(channel) => {
            // 1. 先注册订阅（确保不丢增量消息）
            manager.subscribe_channel(client_id, channel.clone()).await;

            let mut messages = Vec::new();

            // 2. 对 L2BookDelta 类型发送初始快照
            if let ChannelType::L2BookDelta { perpetual_id } = &channel {
                if let Some(redis_conn) = redis {
                    match fetch_l2_book_snapshot(redis_conn, *perpetual_id).await {
                        Ok(snapshot) => {
                            messages.push(ServerMessage::channel_data(
                                &channel_str,
                                snapshot,
                            ));
                        }
                        Err(e) => warn!("Failed to fetch L2 book snapshot: {}", e),
                    }
                }
            }

            // 3. 对 Candle 类型发送快照（现有逻辑）
            if let ChannelType::Candle { perpetual_id, ref interval } = channel {
                // ... 现有 candle snapshot 逻辑 ...
            }

            // 4. 最后发订阅确认
            messages.push(ServerMessage::channel_success("subscribeChannel", &channel_str));
            messages
        }
        // ...
    }
}
```

### 3.5 L2 Book 快照获取

在 `dex-sui/crates/dex-api/src/ws/snapshot.rs` 中新增 L2 book 快照函数：

```rust
/// 获取 L2 book 快照用于 WebSocket 订阅初始化
///
/// 从 dex:l2book:{perpetual_id} 读取当前全量 orderbook，
/// 从 dex:l2book:{perpetual_id}:meta 读取当前 sequence number。
/// 客户端收到快照后，后续只需应用 sequence > snapshot_sequence 的 delta。
pub async fn fetch_l2_book_snapshot(
    redis: &MultiplexedConnection,
    perpetual_id: i32,
) -> anyhow::Result<serde_json::Value> {
    let mut conn = redis.clone();

    // 读取 sequence（原子性保证）
    let meta_key = format!("dex:l2book:{}:meta", perpetual_id);
    let sequence: u64 = redis::cmd("HGET")
        .arg(&meta_key)
        .arg("sequence")
        .query_async::<_, Option<u64>>(&mut conn)
        .await?
        .unwrap_or(0);

    let timestamp_ms: u64 = redis::cmd("HGET")
        .arg(&meta_key)
        .arg("timestamp_ms")
        .query_async::<_, Option<u64>>(&mut conn)
        .await?
        .unwrap_or(0);

    // 读取全量 L2 book
    let book_key = format!("dex:l2book:{}", perpetual_id);
    let fields: HashMap<String, String> = redis::cmd("HGETALL")
        .arg(&book_key)
        .query_async(&mut conn)
        .await?;

    // 解析为 levels 数组：[price, size, side]
    let mut levels: Vec<serde_json::Value> = Vec::new();
    for (key, value) in &fields {
        let qty: i64 = match value.parse() {
            Ok(q) if q > 0 => q,
            _ => continue,
        };
        if let Some(price) = key.strip_prefix("b:") {
            levels.push(serde_json::json!([price, qty.to_string(), "bid"]));
        } else if let Some(price) = key.strip_prefix("a:") {
            levels.push(serde_json::json!([price, qty.to_string(), "ask"]));
        }
    }

    Ok(serde_json::json!({
        "type": "snapshot",
        "sequence": sequence,
        "time": timestamp_ms,
        "levels": levels,
    }))
}
```

## 4. WebSocket 消息格式

### 4.1 l2BookDelta 频道

**Snapshot 消息**（订阅时发送一次）：

```json
{
    "type": "data",
    "channel": "l2BookDelta:0",
    "data": {
        "type": "snapshot",
        "coin": "BTC-USDC",
        "sequence": 42,
        "time": 1709000000000,
        "levels": [
            ["50000", "15000", "bid"],
            ["49900", "20000", "bid"],
            ["50100", "8000", "ask"],
            ["50200", "12000", "ask"]
        ]
    }
}
```

**Delta 消息**（每次变化推送）：

```json
{
    "type": "data",
    "channel": "l2BookDelta:0",
    "data": {
        "type": "delta",
        "coin": "BTC-USDC",
        "sequence": 43,
        "time": 1709000000050,
        "updates": [
            ["50000", "20000", "bid"],
            ["51000", "0", "ask"]
        ]
    }
}
```

- `updates` 数组中每个元素为 `[price, size, side]`
- `size` 为 "0" 表示删除该价格档位
- `sequence` 单调递增，客户端用于检测丢包

### 4.2 BBO 频道

```json
{
    "type": "data",
    "channel": "bbo:0",
    "data": {
        "coin": "BTC-USDC",
        "bestBid": "50000",
        "bestBidQty": "20000",
        "bestAsk": "50100",
        "bestAskQty": "8000",
        "midPrice": "50050",
        "time": 1709000000050
    }
}
```

### 4.3 orderbook 频道（保持不变）

现有 `orderbook:{id}` 频道继续发送全量快照，格式不变：

```json
{
    "type": "data",
    "channel": "orderbook:0",
    "data": {
        "perpetualId": 0,
        "bids": [["50000", "15000", 1], ["49900", "20000", 1]],
        "asks": [["50100", "8000", 1], ["50200", "12000", 1]],
        "bestBid": "50000",
        "bestBidQty": "15000",
        "bestAsk": "50100",
        "bestAskQty": "8000",
        "midPrice": "50050",
        "timestampMs": 1709000000000
    }
}
```

## 5. 订阅协议增强

### 5.1 新增订阅类型

在现有订阅协议基础上，新增两种订阅：

```json
// 订阅增量 L2 book
{
    "method": "subscribeChannel",
    "subscription": { "channel": "l2BookDelta:0" }
}

// 订阅 BBO（已有 ChannelType::Bbo 支持）
{
    "method": "subscribeChannel",
    "subscription": { "channel": "bbo:0" }
}
```

取消订阅：

```json
{
    "method": "unsubscribeChannel",
    "subscription": { "channel": "l2BookDelta:0" }
}
```

### 5.2 订阅确认消息

```json
{
    "type": "channelResponse",
    "data": {
        "method": "subscribeChannel",
        "channel": "l2BookDelta:0",
        "success": true
    }
}
```

## 6. 客户端 Sequence 同步协议

### 6.1 连接与同步

客户端维护本地 orderbook 状态的流程：

1. **订阅** `l2BookDelta:{id}`
2. **收到 snapshot**：初始化本地 orderbook，记录 `sequence`
3. **收到 delta**：
   - 如果 `delta.sequence == local_sequence + 1`：正常应用更新
   - 如果 `delta.sequence > local_sequence + 1`：检测到丢包，需要重新同步
   - 如果 `delta.sequence <= local_sequence`：忽略重复消息

### 6.2 重新同步

客户端检测到 sequence gap 时的恢复策略：

```
1. 取消订阅 l2BookDelta:{id}
2. 通过 REST GET /info { "type": "l2Book", "perpetualId": id } 获取最新全量
3. 重新订阅 l2BookDelta:{id}（会收到新 snapshot）
4. 从新 snapshot 的 sequence 开始跟踪
```

### 6.3 服务端 Sequence 管理

dex-streamer 负责维护 sequence：

- `dex:l2book:{id}:meta` HSET 存储 `sequence` 和 `timestamp_ms`
- 每次写入 delta 到 `dex:stream:l2:delta` 时 `sequence` 自增
- 每次更新 `dex:l2book:{id}` HSET 时同步更新 meta 中的 `sequence`
- Sequence 保证单调递增，但不保证连续（批量更新可能跳跃）

## 7. Redis Key 总览

Phase 6 涉及的所有 Redis key：

| Key Pattern | 类型 | 写入者 | 读取者 | 说明 |
|-------------|------|--------|--------|------|
| `dex:l2book:{id}` | HSET | dex-streamer | dex-api (REST + WS snapshot) | 增量 L2 book，字段 `b:{price}` / `a:{price}` |
| `dex:l2book:{id}:meta` | HSET | dex-streamer | dex-api (WS snapshot) | sequence + timestamp_ms |
| `dex:bbo:{id}` | HSET | dex-streamer | dex-api (REST bbo) | best_bid, best_bid_qty, best_ask, best_ask_qty, mid_price |
| `dex:stream:l2:delta` | Stream | dex-streamer | dex-api (StreamConsumer) | 增量 delta 事件流 |
| `dex:orderbook:{id}` | HSET | dex-indexer | dex-api (fallback) | Checkpoint 全量快照（保留） |
| `dex:stream:orderbook` | Stream | dex-indexer | dex-api (StreamConsumer) | 全量 orderbook 事件流（保留） |

## 8. 向后兼容

### 8.1 REST API 兼容

- `POST /info { "type": "l2Book" }` 透明切换到增量数据源，响应格式不变
- 增量数据不可用时自动 fallback 到 checkpoint 快照
- 新增 `POST /info { "type": "bbo" }`，不影响现有 endpoint

### 8.2 WebSocket 兼容

| 频道 | 行为 | 兼容性 |
|------|------|--------|
| `orderbook:{id}` | 继续发送全量快照（来自 `dex:stream:orderbook`） | 完全兼容 |
| `bbo:{id}` | 继续从 orderbook 数据提取 + 从 delta 提取 | 完全兼容 |
| `l2BookDelta:{id}` | **新增**：snapshot + 增量 delta | 新功能 |

客户端可以同时订阅 `orderbook:{id}`（全量）和 `l2BookDelta:{id}`（增量），在过渡期进行数据对比验证。

### 8.3 Hyperliquid API 兼容

Hyperliquid 的 WebSocket `l2Book` 订阅发送全量快照。本项目的兼容策略：

- **`orderbook:{id}` 频道**：等价于 Hyperliquid 的 `l2Book`，发送全量快照
- **`l2BookDelta:{id}` 频道**：本项目增强，提供增量更新（更低延迟）
- 客户端根据需求选择：简单场景用 `orderbook`，高性能场景用 `l2BookDelta`

## 9. 性能考量

### 9.1 带宽优化

| 推送方式 | 每次消息大小 | 适用场景 |
|----------|-------------|----------|
| 全量快照 (`orderbook`) | ~10KB (200 档位) | 低频查询、简单客户端 |
| 增量 delta (`l2BookDelta`) | ~100B-1KB (仅变化档位) | 高频交易、专业客户端 |
| BBO (`bbo`) | ~100B | 仅需最优价、极低带宽 |

增量推送可将带宽消耗降低 90% 以上。

### 9.2 延迟优化

- **全量快照路径**：checkpoint interval (数秒) -> dex-indexer -> Redis -> dex-api -> client
- **增量 delta 路径**：engine event -> dex-streamer (亚毫秒) -> Redis Stream -> dex-api -> client
- 增量路径绕过 checkpoint 和 PG，端到端延迟可控制在 **<10ms**

### 9.3 StreamConsumer 压力

新增一个 Redis Stream (`dex:stream:l2:delta`) 到 XREAD 列表中。由于 XREAD 支持多 stream 并行阻塞读取，新增一个 stream 对 `read_streams()` 的性能影响可忽略。需要注意的是 delta 消息频率可能远高于其他 stream（每次撮合都产生 delta），可能需要调整 `batch_count` 参数。

## 10. 实施步骤

### Step 1: 类型扩展

- 在 `ws/types.rs` 的 `ChannelType` 中新增 `L2BookDelta` variant
- 在 `types.rs` 中新增 `BboRequest` / `BboResponse`
- 在 `types.rs` 的 `InfoRequest` 中新增 `Bbo` variant

### Step 2: REST handler

- 改造 `handlers.rs` 的 `query_l2_book()` 函数
- 新增 `query_bbo()` 函数
- 在 `server.rs` 的 `info_handler` 中添加 `Bbo` 分发

### Step 3: WS snapshot

- 在 `ws/snapshot.rs` 中新增 `fetch_l2_book_snapshot()` 函数
- 在 `ws/handler.rs` 的 `handle_client_message()` 中为 `L2BookDelta` 订阅发送快照

### Step 4: StreamConsumer

- 在 `ws/consumer.rs` 中新增 `dex:stream:l2:delta` 的消费和路由逻辑
- 新增 `broadcast_l2_delta()` 方法

### Step 5: 集成测试

- 验证 REST `l2Book` endpoint 从增量数据源正确读取
- 验证 WS `l2BookDelta` 订阅的 snapshot + delta 推送
- 验证 sequence 同步和 fallback 机制
- 验证与现有 `orderbook` 频道的共存
