# Phase 6 Step 4: dex-api 集成实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 dex-stream-indexer 的低延迟 L2 数据集成到 dex-api 的 WebSocket 推送和 REST 查询，实现 <50ms 延迟并自动降级到 checkpoint 数据。

**Architecture:** StreamConsumer 新增监听 `dex:stream:l2:update`，收到通知后从 `dex:l2book:{id}` HGETALL 读取完整 L2，构造全量快照广播到 `orderbook:{id}` 和 `bbo:{id}` 频道。当 l2:update 活跃时抑制 checkpoint 的 `dex:stream:orderbook` 广播。REST query_l2_book 优先读 `dex:l2book:{id}`，fallback 到 `dex:orderbook:{id}`。

**Tech Stack:** Rust, redis 0.24, tokio, serde_json

**设计文档:** `docs/plans/2026-02-27-phase6-step4-dex-api-integration-design.md`

---

## Task 1: StreamConsumer 添加 l2:update 流监听

**Files:**
- Modify: `crates/dex-api/src/ws/consumer.rs`

**关键架构说明：**

当前 `broadcast_message()` 是同步方法（无 Redis 连接）。新增的 `handle_l2_update()` 需要异步读 Redis HGETALL。因此 l2:update 在 `read_streams()` 中直接处理（有 `conn` 参数），不经过 `broadcast_message()`。

checkpoint 抑制需要跟踪每个市场的 l2:update 最后时间。使用 `HashMap<u32, Instant>` 作为 `run()` 中的局部变量，通过参数传递给 `read_streams()` 和 `broadcast_message()`。

**Step 1: 在 stream_keys 模块添加新常量**

文件: `crates/dex-api/src/ws/consumer.rs:24-33`

在 `MARKET_STATS` 之后添加：
```rust
pub const L2_UPDATE: &str = "dex:stream:l2:update";
```

**Step 2: StreamIds 新增 l2_update 字段**

文件: `crates/dex-api/src/ws/consumer.rs:372-382` (StreamIds struct)

```rust
struct StreamIds {
    fills: String,
    positions: String,
    balances: String,
    transfers: String,
    orders: String,
    orderbook: String,
    candles: String,
    market_stats: String,
    l2_update: String,    // 新增
}
```

**Step 3: 修改 run() — 初始化 l2_update 和 l2_active_markets**

文件: `crates/dex-api/src/ws/consumer.rs:49-82`

在 `run()` 方法中：

1. 新增 import: `use std::collections::HashMap;` 和 `use std::time::Instant;`（文件顶部）
2. StreamIds 初始化中新增 `l2_update: "$".to_string(),`
3. 在 `loop` 前添加: `let mut l2_active_markets: HashMap<u32, Instant> = HashMap::new();`
4. 修改 `read_streams` 调用，传入 `&mut l2_active_markets`

完整 `run()` 方法：
```rust
pub async fn run(&self) -> anyhow::Result<()> {
    info!("Connecting to Redis: {}", self.config.redis_url);

    let client = redis::Client::open(self.config.redis_url.as_str())?;
    let mut conn = client.get_multiplexed_async_connection().await?;

    info!("Redis consumer connected, starting stream consumption");

    let mut last_ids = StreamIds {
        fills: "$".to_string(),
        positions: "$".to_string(),
        balances: "$".to_string(),
        transfers: "$".to_string(),
        orders: "$".to_string(),
        orderbook: "$".to_string(),
        candles: "$".to_string(),
        market_stats: "$".to_string(),
        l2_update: "$".to_string(),
    };

    // Track markets with active l2:update stream (for checkpoint suppression)
    let mut l2_active_markets: HashMap<u32, Instant> = HashMap::new();

    loop {
        match self
            .read_streams(&mut conn, &mut last_ids, &mut l2_active_markets)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    debug!("Processed {} messages from Redis Streams", count);
                }
            }
            Err(e) => {
                error!("Error reading from Redis Streams: {}", e);
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
```

**Step 4: 修改 read_streams() — 添加 l2:update 到 XREAD 并特殊处理**

文件: `crates/dex-api/src/ws/consumer.rs:84-160`

1. 函数签名添加 `l2_active: &mut HashMap<u32, Instant>` 参数
2. XREAD 的 keys 数组和 IDs 数组各追加一项
3. last_id 更新的 match 添加 L2_UPDATE 分支
4. l2:update 消息走 `handle_l2_update()`，其余走 `broadcast_message()`（传入 `l2_active`）

完整 `read_streams()` 方法：
```rust
async fn read_streams(
    &self,
    conn: &mut MultiplexedConnection,
    last_ids: &mut StreamIds,
    l2_active: &mut HashMap<u32, Instant>,
) -> anyhow::Result<usize> {
    let opts = StreamReadOptions::default()
        .block(self.config.block_ms)
        .count(self.config.batch_count);

    let result: StreamReadReply = conn
        .xread_options(
            &[
                stream_keys::FILLS,
                stream_keys::POSITIONS,
                stream_keys::BALANCES,
                stream_keys::TRANSFERS,
                stream_keys::ORDERS,
                stream_keys::ORDERBOOK,
                stream_keys::CANDLES,
                stream_keys::MARKET_STATS,
                stream_keys::L2_UPDATE,
            ],
            &[
                &last_ids.fills,
                &last_ids.positions,
                &last_ids.balances,
                &last_ids.transfers,
                &last_ids.orders,
                &last_ids.orderbook,
                &last_ids.candles,
                &last_ids.market_stats,
                &last_ids.l2_update,
            ],
            &opts,
        )
        .await?;

    let mut total_count = 0;

    for stream_key in result.keys {
        let stream_name = &stream_key.key;

        for entry in stream_key.ids {
            total_count += 1;

            // Update last ID
            match stream_name.as_str() {
                s if s == stream_keys::FILLS => last_ids.fills = entry.id.clone(),
                s if s == stream_keys::POSITIONS => last_ids.positions = entry.id.clone(),
                s if s == stream_keys::BALANCES => last_ids.balances = entry.id.clone(),
                s if s == stream_keys::TRANSFERS => last_ids.transfers = entry.id.clone(),
                s if s == stream_keys::ORDERS => last_ids.orders = entry.id.clone(),
                s if s == stream_keys::ORDERBOOK => last_ids.orderbook = entry.id.clone(),
                s if s == stream_keys::CANDLES => last_ids.candles = entry.id.clone(),
                s if s == stream_keys::MARKET_STATS => {
                    last_ids.market_stats = entry.id.clone()
                }
                s if s == stream_keys::L2_UPDATE => {
                    last_ids.l2_update = entry.id.clone()
                }
                _ => {}
            }

            // Extract data field
            let data: Option<String> = entry.map.get("data").and_then(|v| {
                if let redis::Value::Data(bytes) = v {
                    String::from_utf8(bytes.clone()).ok()
                } else {
                    None
                }
            });

            if let Some(json_data) = data {
                // l2:update needs async Redis access — handle separately
                if stream_name.as_str() == stream_keys::L2_UPDATE {
                    self.handle_l2_update(&json_data, conn, l2_active).await;
                } else {
                    self.broadcast_message(stream_name, &json_data, l2_active);
                }
            } else {
                warn!("No data field in stream entry: {}", entry.id);
            }
        }
    }

    Ok(total_count)
}
```

**Step 5: 修改 broadcast_message() — 添加 l2_active 参数 + checkpoint 抑制**

文件: `crates/dex-api/src/ws/consumer.rs:162-206`

1. 函数签名添加 `l2_active: &HashMap<u32, Instant>` 参数
2. ORDERBOOK 分支添加 `l2_active` 参数到 `broadcast_orderbook`

```rust
fn broadcast_message(
    &self,
    stream_name: &str,
    json_data: &str,
    l2_active: &HashMap<u32, Instant>,
) {
    let data: serde_json::Value = match serde_json::from_str(json_data) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse JSON from stream {}: {}", stream_name, e);
            return;
        }
    };

    match stream_name {
        s if s == stream_keys::FILLS => {
            self.broadcast_fill(&data);
            self.manager.broadcast(ServerMessage::Fills { data });
        }
        s if s == stream_keys::POSITIONS => {
            self.broadcast_clearinghouse_state(&data, "position");
            self.manager.broadcast(ServerMessage::Positions { data });
        }
        s if s == stream_keys::BALANCES => {
            self.broadcast_clearinghouse_state(&data, "balance");
            self.manager.broadcast(ServerMessage::Balances { data });
        }
        s if s == stream_keys::TRANSFERS => {
            self.manager.broadcast(ServerMessage::Transfers { data });
        }
        s if s == stream_keys::ORDERS => {
            self.broadcast_open_orders(&data);
            self.manager.broadcast(ServerMessage::Orders { data });
        }
        s if s == stream_keys::ORDERBOOK => {
            self.broadcast_orderbook(&data, l2_active);
        }
        s if s == stream_keys::CANDLES => {
            self.broadcast_candle(&data);
        }
        s if s == stream_keys::MARKET_STATS => {
            self.broadcast_market_stats(&data);
        }
        _ => {
            warn!("Unknown stream: {}", stream_name);
        }
    }
}
```

**Step 6: 编译验证**

Run: `cargo check -p dex-api`
Expected: 编译错误（handle_l2_update 和 broadcast_orderbook 签名不匹配）— 这些在 Task 2 和 Task 3 中实现

**Step 7: Commit（与 Task 2、3 合并提交）**

---

## Task 2: 实现 handle_l2_update — 从 Redis 读取 L2 并广播

**Files:**
- Modify: `crates/dex-api/src/ws/consumer.rs`

**Step 1: 实现 handle_l2_update 方法**

在 `broadcast_market_stats()` 方法之后添加：

```rust
/// Handle l2:update notification from dex-stream-indexer.
///
/// Reads full L2 snapshot from dex:l2book:{id} via HGETALL,
/// then broadcasts to orderbook:{id} and bbo:{id} channels.
async fn handle_l2_update(
    &self,
    json_data: &str,
    conn: &mut MultiplexedConnection,
    l2_active: &mut HashMap<u32, Instant>,
) {
    let notification: serde_json::Value = match serde_json::from_str(json_data) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse l2:update notification: {}", e);
            return;
        }
    };

    let perpetual_id = match notification.get("perpetual_id").and_then(|v| v.as_u64()) {
        Some(id) => id as u32,
        None => {
            warn!("l2:update notification missing perpetual_id");
            return;
        }
    };

    // Mark this market as having active l2:update
    l2_active.insert(perpetual_id, Instant::now());

    // Read full L2 from Redis
    let l2_key = format!("dex:l2book:{}", perpetual_id);
    let fields: HashMap<String, String> =
        match redis::cmd("HGETALL").arg(&l2_key).query_async(conn).await {
            Ok(f) => f,
            Err(e) => {
                error!(perpetual_id, "Failed to HGETALL dex:l2book: {}", e);
                return;
            }
        };

    if fields.is_empty() {
        debug!(perpetual_id, "dex:l2book is empty, skipping broadcast");
        return;
    }

    // Parse b:{price} and a:{price} fields into bid/ask levels
    let mut bids: Vec<(u64, u64)> = Vec::new();
    let mut asks: Vec<(u64, u64)> = Vec::new();

    for (key, value) in &fields {
        let qty: u64 = match value.parse() {
            Ok(q) => q,
            Err(_) => continue,
        };
        if let Some(price_str) = key.strip_prefix("b:") {
            if let Ok(price) = price_str.parse::<u64>() {
                bids.push((price, qty));
            }
        } else if let Some(price_str) = key.strip_prefix("a:") {
            if let Ok(price) = price_str.parse::<u64>() {
                asks.push((price, qty));
            }
        }
    }

    // Sort: bids descending by price, asks ascending by price
    bids.sort_by(|a, b| b.0.cmp(&a.0));
    asks.sort_by(|a, b| a.0.cmp(&b.0));

    // Get timestamp from notification
    let timestamp_ms = notification
        .get("timestamp_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // Construct L2 levels as JSON arrays (matching Hyperliquid format)
    let bid_levels: Vec<serde_json::Value> = bids
        .iter()
        .map(|(p, q)| {
            serde_json::json!({
                "px": p.to_string(),
                "sz": q.to_string(),
                "n": 1
            })
        })
        .collect();
    let ask_levels: Vec<serde_json::Value> = asks
        .iter()
        .map(|(p, q)| {
            serde_json::json!({
                "px": p.to_string(),
                "sz": q.to_string(),
                "n": 1
            })
        })
        .collect();

    // Broadcast full L2 snapshot to orderbook:{id}
    let orderbook_data = serde_json::json!({
        "perpetualId": perpetual_id,
        "levels": [bid_levels, ask_levels],
        "time": timestamp_ms,
    });
    let channel = format!("orderbook:{}", perpetual_id);
    self.manager
        .broadcast(ServerMessage::channel_data(&channel, orderbook_data));

    // Read BBO from Redis and broadcast to bbo:{id}
    let bbo_key = format!("dex:bbo:{}", perpetual_id);
    let bbo_fields: Vec<Option<String>> = match redis::cmd("HMGET")
        .arg(&bbo_key)
        .arg("best_bid")
        .arg("best_bid_qty")
        .arg("best_ask")
        .arg("best_ask_qty")
        .query_async(conn)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            debug!(perpetual_id, "Failed to read BBO: {}", e);
            return;
        }
    };

    let best_bid = bbo_fields[0].as_deref().unwrap_or("0");
    let best_bid_qty = bbo_fields[1].as_deref().unwrap_or("0");
    let best_ask = bbo_fields[2].as_deref().unwrap_or("0");
    let best_ask_qty = bbo_fields[3].as_deref().unwrap_or("0");

    // Compute mid price
    let bid_val: f64 = best_bid.parse().unwrap_or(0.0);
    let ask_val: f64 = best_ask.parse().unwrap_or(0.0);
    let mid = if bid_val > 0.0 && ask_val > 0.0 {
        (bid_val + ask_val) / 2.0
    } else {
        0.0
    };

    let bbo_data = serde_json::json!({
        "perpetualId": perpetual_id,
        "bestBid": best_bid,
        "bestBidSize": best_bid_qty,
        "bestAsk": best_ask,
        "bestAskSize": best_ask_qty,
        "midPrice": mid.to_string(),
        "timestampMs": timestamp_ms,
    });
    let bbo_channel = format!("bbo:{}", perpetual_id);
    self.manager
        .broadcast(ServerMessage::channel_data(&bbo_channel, bbo_data));

    debug!(
        perpetual_id,
        bids = bids.len(),
        asks = asks.len(),
        "Broadcast L2 from dex-stream-indexer"
    );
}
```

**Step 2: 添加 imports**

在文件顶部 imports 中添加:
```rust
use std::collections::HashMap;
use std::time::Instant;
```

**Step 3: 编译验证**

Run: `cargo check -p dex-api`
Expected: 编译错误（broadcast_orderbook 签名需要更新）— Task 3 修复

---

## Task 3: Checkpoint 抑制 — 修改 broadcast_orderbook

**Files:**
- Modify: `crates/dex-api/src/ws/consumer.rs`

**Step 1: 修改 broadcast_orderbook 签名和逻辑**

替换现有 `broadcast_orderbook` 方法：

```rust
/// Broadcast orderbook snapshot to `orderbook:{id}` and BBO to `bbo:{id}` channels.
///
/// When dex-stream-indexer l2:update is active for this market (within 30s),
/// checkpoint broadcasts are suppressed to prevent time-reversal.
fn broadcast_orderbook(
    &self,
    data: &serde_json::Value,
    l2_active: &HashMap<u32, Instant>,
) {
    let perpetual_id = match data.get("perpetualId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => {
            warn!("Orderbook message missing perpetualId");
            return;
        }
    };

    // Suppress checkpoint broadcast if l2:update is active for this market
    if let Some(last_update) = l2_active.get(&(perpetual_id as u32)) {
        if last_update.elapsed() < Duration::from_secs(30) {
            debug!(
                perpetual_id,
                "Suppressing checkpoint orderbook (l2:update active)"
            );
            return;
        }
    }

    // Full L2 snapshot → orderbook:{id}
    let channel = format!("orderbook:{}", perpetual_id);
    self.manager
        .broadcast(ServerMessage::channel_data(&channel, data.clone()));

    // BBO extract → bbo:{id}
    let bbo = serde_json::json!({
        "perpetualId": perpetual_id,
        "bestBid": data.get("bestBid").unwrap_or(&serde_json::Value::Null),
        "bestBidSize": data.get("bestBidSize").unwrap_or(&serde_json::Value::Null),
        "bestAsk": data.get("bestAsk").unwrap_or(&serde_json::Value::Null),
        "bestAskSize": data.get("bestAskSize").unwrap_or(&serde_json::Value::Null),
        "midPrice": data.get("midPrice").unwrap_or(&serde_json::Value::Null),
        "timestampMs": data.get("timestampMs").unwrap_or(&serde_json::Value::Null),
    });
    let bbo_channel = format!("bbo:{}", perpetual_id);
    self.manager
        .broadcast(ServerMessage::channel_data(&bbo_channel, bbo));
}
```

**Step 2: 编译验证**

Run: `cargo check -p dex-api`
Expected: 编译通过

**Step 3: Commit Tasks 1-3**

```bash
git add crates/dex-api/src/ws/consumer.rs
git commit -m "feat(dex): integrate dex-stream-indexer l2:update into StreamConsumer with checkpoint suppression"
```

---

## Task 4: REST query_l2_book 双源 Fallback

**Files:**
- Modify: `crates/dex-api/src/handlers.rs`

**Step 1: 修改 query_l2_book 函数**

替换现有 `query_l2_book` 函数（`crates/dex-api/src/handlers.rs:696-742`）：

```rust
/// Query L2 order book from Redis
///
/// Dual data source with fallback:
/// 1. Try dex:l2book:{id} (dex-stream-indexer, low-latency)
/// 2. Fallback to dex:orderbook:{id} (checkpoint, higher latency)
pub async fn query_l2_book(
    redis: &Option<MultiplexedConnection>,
    req: L2BookRequest,
) -> Result<L2BookResponse> {
    let conn = redis
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Redis not configured"))?;
    let mut conn = conn.clone();

    let depth = req.depth.min(200);

    // 1. Try dex-stream-indexer L2 data (low latency)
    let l2_meta_key = format!("dex:l2book:{}:meta", req.perpetual_id);
    let meta_fields: Vec<Option<String>> = redis::cmd("HMGET")
        .arg(&l2_meta_key)
        .arg("timestamp")
        .query_async(&mut conn)
        .await
        .unwrap_or_else(|_| vec![None]);

    let streamer_fresh = if let Some(Some(ts_str)) = meta_fields.first() {
        if let Ok(ts) = ts_str.parse::<u64>() {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            // Fresh if timestamp is within 10 seconds
            now_ms.saturating_sub(ts) < 10_000
        } else {
            false
        }
    } else {
        false
    };

    if streamer_fresh {
        let l2_key = format!("dex:l2book:{}", req.perpetual_id);
        let fields: std::collections::HashMap<String, String> = redis::cmd("HGETALL")
            .arg(&l2_key)
            .query_async(&mut conn)
            .await
            .unwrap_or_default();

        if !fields.is_empty() {
            let mut bids = Vec::new();
            let mut asks = Vec::new();

            for (key, value) in &fields {
                let qty_str = value;
                if let Some(price_str) = key.strip_prefix("b:") {
                    bids.push(L2Level {
                        price: price_str.to_string(),
                        size: qty_str.to_string(),
                    });
                } else if let Some(price_str) = key.strip_prefix("a:") {
                    asks.push(L2Level {
                        price: price_str.to_string(),
                        size: qty_str.to_string(),
                    });
                }
            }

            // Sort: bids descending, asks ascending (by numeric price)
            bids.sort_by(|a, b| {
                b.price
                    .parse::<u64>()
                    .unwrap_or(0)
                    .cmp(&a.price.parse::<u64>().unwrap_or(0))
            });
            asks.sort_by(|a, b| {
                a.price
                    .parse::<u64>()
                    .unwrap_or(0)
                    .cmp(&b.price.parse::<u64>().unwrap_or(0))
            });

            let timestamp_ms: u64 = meta_fields
                .first()
                .and_then(|f| f.as_ref())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            return Ok(L2BookResponse {
                perpetual_id: req.perpetual_id,
                bids: bids.into_iter().take(depth).collect(),
                asks: asks.into_iter().take(depth).collect(),
                timestamp_ms,
            });
        }
    }

    // 2. Fallback: read from checkpoint Redis (dex:orderbook:{id})
    let key = format!("dex:orderbook:{}", req.perpetual_id);

    let fields: Vec<Option<String>> = redis::cmd("HMGET")
        .arg(&key)
        .arg("bids")
        .arg("asks")
        .arg("timestamp_ms")
        .query_async(&mut conn)
        .await?;

    let bids: Vec<L2Level> = fields[0]
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let asks: Vec<L2Level> = fields[1]
        .as_ref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let timestamp_ms: u64 = fields[2]
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Ok(L2BookResponse {
        perpetual_id: req.perpetual_id,
        bids: bids.into_iter().take(depth).collect(),
        asks: asks.into_iter().take(depth).collect(),
        timestamp_ms,
    })
}
```

**Step 2: 编译验证**

Run: `cargo check -p dex-api`
Expected: 编译通过

**Step 3: Commit**

```bash
git add crates/dex-api/src/handlers.rs
git commit -m "feat(dex): add dual-source L2 fallback in REST query_l2_book"
```

---

## Task 5: 全量编译 + Clippy

**Step 1: 编译验证**

Run: `cargo check -p dex-api`
Expected: 编译通过

**Step 2: Clippy**

Run: `cargo clippy -p dex-api`
Expected: 无新增 warning

**Step 3: 修复 clippy 问题（如有）**

**Step 4: Commit（如有修复）**

```bash
git add crates/dex-api/
git commit -m "chore: fix clippy warnings in dex-api"
```

---

## 验证标准

| 指标 | 标准 |
|------|------|
| 编译通过 | `cargo check -p dex-api` 无错误 |
| Clippy 通过 | 无新增 warning |
| StreamConsumer | 监听 9 个 Redis Stream（新增 l2:update） |
| L2 广播 | l2:update → HGETALL → orderbook:{id} + bbo:{id} |
| Checkpoint 抑制 | l2:update 活跃时跳过同市场的 checkpoint 广播 |
| REST fallback | query_l2_book 优先 dex:l2book，fallback dex:orderbook |
| 向后兼容 | 现有 WS 消息格式不变 |
