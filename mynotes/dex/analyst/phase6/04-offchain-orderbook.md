# 链下订单簿构建 -- dex-streamer 组件设计

> 创建日期: 2026-02-25
> 更新日期: 2026-02-27
> 状态: ⚠️ 需更新 — gRPC 消费 + GetSnapshot 恢复
> 前置文档: [00-overview](./00-overview.md)

> **2026-02-27 架构决策变更通知**
>
> 根据 [08-architecture-qa.md](./08-architecture-qa.md) 确认的决策：
> - **Q7=B**: dex-streamer 作为**独立 Docker 服务**，通过 gRPC 消费（不是 broadcast channel）
> - **Q3=C+**: 恢复机制改为 gRPC `GetSnapshot()` 从 InlineOrderbook 读取（不再依赖 Checkpoint 快照）
>
> **本文档需更新的部分**：
> - §1 概述 → 数据源从 broadcast channel 改为 gRPC Subscribe()
> - §2 Crate 结构 → 新增 `grpc_client.rs`，移除 `transport.rs`
> - §7 初始化与恢复 → 启动时调用 gRPC GetSnapshot() 而非等待 broadcast 快照
> - §7.3 Gap 恢复 → 从 ~~Redis dex:orderbook~~ 改为 gRPC GetSnapshot()
> - §8 对账 → 对账源从 ~~Checkpoint Redis~~ 改为 gRPC GetSnapshot()
> - §9 职责划分 → 移除 Checkpoint 订单簿相关职责
>
> 核心设计（L2Book、BboTracker、RedisWriter、Redis key 设计）保持不变。
> 当前文档内容保留作为参考，实际实现以 07-implementation-plan.md Step 3 为准。

## 1. 概述

dex-streamer 是 Phase 6 引入的新 crate，负责从 DexStreamingManager 的 broadcast channel 消费增量 `OrderbookDeltaEvent` 事件，在内存中维护 L2 订单簿，并将增量更新写入 Redis。它运行在验证器节点同一台机器上（独立进程或线程），实现 <50ms 的订单簿更新延迟。

**当前系统**（Checkpoint 通道）：

```
DEX 引擎执行 → OrderbookSnapshotEvent → Checkpoint → dex-indexer
    → Redis HSET dex:orderbook:{perpetual_id} (bids/asks JSON 全量)
    → Redis XADD dex:stream:orderbook (WS 推送)
    → 延迟: 1.5-3.5s
```

**目标系统**（Stream 通道，本文档设计）：

```
DEX 引擎执行 → OrderbookDeltaEvent → DexStreamingManager broadcast
    → dex-streamer OrderbookBuilder
    → Redis HSET dex:l2book:{perpetual_id} (增量 field 更新)
    → Redis XADD dex:stream:l2:delta (增量 delta 推送)
    → 延迟: <50ms
```

---

## 2. Crate 结构

```
dex-sui/crates/dex-streamer/
├── Cargo.toml
├── src/
│   ├── main.rs              # 入口：连接 transport，启动 builders
│   ├── lib.rs               # 库接口（供集成测试和嵌入式部署使用）
│   ├── config.rs            # 配置（Redis URL, channel capacity, flush interval 等）
│   ├── orderbook_builder.rs # L2 订单簿构建器（核心模块）
│   ├── bbo_tracker.rs       # BBO (Best Bid/Offer) 追踪器
│   ├── redis_writer.rs      # Redis 写入层（增量 HSET + XADD, pipeline 批处理）
│   ├── reconciler.rs        # Checkpoint 对账模块（与 dex-indexer 快照比对）
│   └── transport.rs         # StreamTransport trait 实现（复用 sui-core streaming 机制）
```

### 2.1 Cargo.toml 依赖

```toml
[package]
name = "dex-streamer"
version = "0.1.0"
edition = "2024"

[dependencies]
# 核心运行时
tokio = { workspace = true, features = ["full"] }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }

# Redis
redis = { workspace = true, features = ["tokio-comp", "aio"] }

# 序列化
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
bcs.workspace = true

# Sui 类型（事件定义）
sui-types = { path = "../sui-types" }

# 配置
clap = { workspace = true, features = ["env"] }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }
```

---

## 3. OrderbookBuilder 核心设计

### 3.1 数据结构

```rust
use std::collections::{BTreeMap, HashMap};

/// L2 订单簿构建器
///
/// 消费 OrderbookDeltaEvent 流，维护每个市场的内存 L2 订单簿，
/// 并将增量变更写入 Redis。
pub struct OrderbookBuilder {
    /// 每个市场的内存 L2 订单簿
    books: HashMap<u32, L2Book>,
    /// Redis 写入器（pipeline 批处理）
    redis: RedisWriter,
    /// BBO 追踪器
    bbo: BboTracker,
    /// 每个市场最后处理的 sequence number（用于 gap 检测）
    sequences: HashMap<u32, u64>,
    /// 待刷新的 delta 缓冲区（micro-batch）
    pending_deltas: Vec<PendingDelta>,
    /// flush 间隔（默认 5ms）
    flush_interval_ms: u64,
}

/// 单个市场的 L2 订单簿
pub struct L2Book {
    pub perpetual_id: u32,
    /// Bid 价格档位: price -> quantity
    /// BTreeMap 保证按价格有序，方便提取 BBO
    pub bids: BTreeMap<u64, u64>,
    /// Ask 价格档位: price -> quantity
    pub asks: BTreeMap<u64, u64>,
    /// 当前 sequence number
    pub sequence: u64,
    /// 最后更新时间戳 (ms)
    pub updated_at: u64,
}

/// 待刷新到 Redis 的增量变更
struct PendingDelta {
    perpetual_id: u32,
    side: Side,
    price: u64,
    /// new_quantity = 0 表示删除该档位
    new_quantity: u64,
    sequence: u64,
    timestamp_ms: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bid,
    Ask,
}
```

### 3.2 L2Book 操作

```rust
impl L2Book {
    /// 创建空订单簿
    pub fn new(perpetual_id: u32) -> Self {
        Self {
            perpetual_id,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            sequence: 0,
            updated_at: 0,
        }
    }

    /// 从 OrderbookSnapshotEvent 初始化（启动恢复用）
    pub fn from_snapshot(event: &OrderbookSnapshotEvent) -> Self {
        let mut book = Self::new(event.perpetual_id);
        for level in &event.bids {
            if level.quantity > 0 {
                book.bids.insert(level.price, level.quantity);
            }
        }
        for level in &event.asks {
            if level.quantity > 0 {
                book.asks.insert(level.price, level.quantity);
            }
        }
        book.updated_at = event.timestamp_ms;
        book
    }

    /// 应用单个 delta 到内存订单簿
    ///
    /// 返回 (bbo_changed, old_quantity) 用于 BBO 追踪和 Redis 写入决策
    pub fn apply_delta(
        &mut self,
        side: Side,
        price: u64,
        new_quantity: u64,
        sequence: u64,
        timestamp_ms: u64,
    ) -> (bool, Option<u64>) {
        let levels = match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };

        let old_quantity = if new_quantity == 0 {
            levels.remove(&price)
        } else {
            levels.insert(price, new_quantity)
        };

        // 检查 BBO 是否变化
        let bbo_changed = match side {
            Side::Bid => {
                // 最高买价 = BTreeMap 最后一个 key
                let best = self.bids.keys().next_back().copied();
                best == Some(price) || old_quantity.is_none()
            }
            Side::Ask => {
                // 最低卖价 = BTreeMap 第一个 key
                let best = self.asks.keys().next().copied();
                best == Some(price) || old_quantity.is_none()
            }
        };

        self.sequence = sequence;
        self.updated_at = timestamp_ms;

        (bbo_changed, old_quantity)
    }

    /// 获取 BBO (Best Bid/Offer)
    pub fn bbo(&self) -> BboSnapshot {
        let (best_bid, best_bid_qty) = self
            .bids
            .iter()
            .next_back()
            .map(|(&p, &q)| (p, q))
            .unwrap_or((0, 0));

        let (best_ask, best_ask_qty) = self
            .asks
            .iter()
            .next()
            .map(|(&p, &q)| (p, q))
            .unwrap_or((0, 0));

        BboSnapshot {
            perpetual_id: self.perpetual_id,
            best_bid,
            best_bid_qty,
            best_ask,
            best_ask_qty,
            sequence: self.sequence,
            timestamp_ms: self.updated_at,
        }
    }

    /// 导出为全量快照（用于恢复和对账）
    pub fn to_snapshot(&self) -> Vec<(Side, u64, u64)> {
        let mut levels = Vec::new();
        for (&price, &qty) in self.bids.iter().rev() {
            levels.push((Side::Bid, price, qty));
        }
        for (&price, &qty) in &self.asks {
            levels.push((Side::Ask, price, qty));
        }
        levels
    }
}

/// BBO 快照
pub struct BboSnapshot {
    pub perpetual_id: u32,
    pub best_bid: u64,
    pub best_bid_qty: u64,
    pub best_ask: u64,
    pub best_ask_qty: u64,
    pub sequence: u64,
    pub timestamp_ms: u64,
}
```

### 3.3 事件处理流程

```rust
impl OrderbookBuilder {
    /// 处理一批 DexStreamBatch 中的 OrderbookDeltaEvent
    ///
    /// 处理流程:
    /// 1. 检查 sequence 连续性（检测 gap）
    /// 2. 应用 delta 到内存 L2Book
    /// 3. 如果 BBO 变化，更新 BboTracker
    /// 4. 将变更加入 pending_deltas 缓冲区
    /// 5. 如果距上次 flush 超过 flush_interval，执行 Redis 写入
    pub async fn process_batch(&mut self, batch: &DexStreamBatch) -> Result<()> {
        // 从 batch.events 中提取 OrderbookDeltaEvent
        for stream_event in &batch.events {
            let event = match stream_event {
                DexStreamEvent::OrderbookDelta(delta) => delta,
                _ => continue, // 跳过非 delta 事件
            };

            // 1. Gap 检测
            let expected_seq = self
                .sequences
                .get(&event.perpetual_id)
                .map(|s| s + 1)
                .unwrap_or(0);

            if event.sequence != expected_seq && expected_seq != 0 {
                tracing::warn!(
                    perpetual_id = event.perpetual_id,
                    expected = expected_seq,
                    got = event.sequence,
                    "Sequence gap detected, triggering recovery"
                );
                self.recover_from_snapshot(event.perpetual_id).await?;
                continue;
            }

            // 2. 获取或创建 L2Book
            let book = self
                .books
                .entry(event.perpetual_id)
                .or_insert_with(|| L2Book::new(event.perpetual_id));

            // 3. 应用每个变更的价格档位（updates: Vec<OrderbookDelta>）
            for delta in &event.updates {
                let side = if delta.side == 0 { Side::Bid } else { Side::Ask };
                let (bbo_changed, _old_qty) = book.apply_delta(
                    side,
                    delta.price,
                    delta.quantity,
                    event.sequence,
                    event.timestamp_ms,
                );

                // 4. BBO 追踪
                if bbo_changed {
                    self.bbo.update(book.bbo());
                }

                // 5. 加入 pending buffer
                self.pending_deltas.push(PendingDelta {
                    perpetual_id: event.perpetual_id,
                    side,
                    price: delta.price,
                    new_quantity: delta.quantity,
                    sequence: event.sequence,
                    timestamp_ms: event.timestamp_ms,
                });
            }

            // 更新 sequence
            self.sequences.insert(event.perpetual_id, event.sequence);
        }

        // 6. 检查是否需要 flush
        self.maybe_flush().await?;

        Ok(())
    }

    /// 按 flush_interval 检查并批量写入 Redis
    async fn maybe_flush(&mut self) -> Result<()> {
        if self.pending_deltas.is_empty() {
            return Ok(());
        }

        // micro-batch: 累积 deltas，每 5-10ms flush 一次
        // 实际 flush 由外层 tick 触发，这里只是检查
        self.redis.write_deltas(&self.pending_deltas, &self.bbo).await?;
        self.pending_deltas.clear();

        Ok(())
    }
}
```

---

## 4. BBO 追踪器

```rust
/// BBO (Best Bid/Offer) 追踪器
///
/// 独立追踪每个市场的最优买卖价，仅在 BBO 变化时触发 Redis 更新。
/// 这确保 BBO 频道的更新频率远低于全量 delta，降低 Redis 写入压力。
pub struct BboTracker {
    /// 每个市场的当前 BBO
    snapshots: HashMap<u32, BboSnapshot>,
}

impl BboTracker {
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
        }
    }

    /// 更新 BBO 快照
    ///
    /// 仅当 best_bid 或 best_ask 实际变化时返回 true，
    /// 表示需要写入 Redis `dex:bbo:{perpetual_id}` 和推送 WS。
    pub fn update(&mut self, new_bbo: BboSnapshot) -> bool {
        let changed = match self.snapshots.get(&new_bbo.perpetual_id) {
            Some(old) => {
                old.best_bid != new_bbo.best_bid
                    || old.best_ask != new_bbo.best_ask
                    || old.best_bid_qty != new_bbo.best_bid_qty
                    || old.best_ask_qty != new_bbo.best_ask_qty
            }
            None => true,
        };

        if changed {
            self.snapshots.insert(new_bbo.perpetual_id, new_bbo);
        }

        changed
    }

    /// 获取当前 BBO（用于 Redis 写入）
    pub fn get(&self, perpetual_id: u32) -> Option<&BboSnapshot> {
        self.snapshots.get(&perpetual_id)
    }
}
```

---

## 5. Redis 增量存储方案

### 5.1 Key 设计

与现有 dex-indexer 的 Redis key 并行存在，互不干扰：

| Key | 类型 | 用途 | 来源 |
|-----|------|------|------|
| `dex:orderbook:{perpetual_id}` | HSET | 全量快照 (bids/asks JSON) | dex-indexer (现有) |
| `dex:stream:orderbook` | Stream | 全量快照推送 | dex-indexer (现有) |
| **`dex:l2book:{perpetual_id}`** | **HSET** | **增量 L2 book: field=`{side}:{price}`, value=`{quantity}`** | **dex-streamer (新)** |
| **`dex:l2book:{perpetual_id}:meta`** | **HSET** | **元数据: sequence, timestamp, bid_count, ask_count** | **dex-streamer (新)** |
| **`dex:bbo:{perpetual_id}`** | **HSET** | **BBO: best_bid, best_bid_qty, best_ask, best_ask_qty** | **dex-streamer (新)** |
| **`dex:stream:l2:delta`** | **Stream** | **增量 delta 推送给 WS consumer** | **dex-streamer (新)** |

### 5.2 HSET field 格式

`dex:l2book:{perpetual_id}` 的 field 命名规则：

- **Bid**: `b:{price}` (例如 `b:50000`)
- **Ask**: `a:{price}` (例如 `a:50100`)
- **Value**: quantity 的字符串表示

当 quantity 变为 0 时，使用 `HDEL` 删除该 field 而非设置为 "0"，避免无效 field 堆积。

**优势对比**：

| 维度 | 现有方案 (dex:orderbook) | 新方案 (dex:l2book) |
|------|--------------------------|---------------------|
| 存储格式 | JSON 数组 (整体序列化/反序列化) | 独立 field (单档位原子更新) |
| 更新粒度 | 全量替换 bids/asks field | 单个 field HSET/HDEL |
| 读取方式 | `HGET ... bids` + JSON parse | `HSCAN` 或 `HMGET b:* a:*` |
| 写入开销 | O(n) 全量序列化 | O(k) 仅变化档位 |
| Redis 内存 | ~10KB/市场 (JSON 开销) | ~5KB/市场 (field 开销较低) |

### 5.3 Redis 写入层 (RedisWriter)

```rust
use redis::aio::MultiplexedConnection;
use redis::Pipeline;

/// Stream key for L2 delta events
const L2_DELTA_STREAM: &str = "dex:stream:l2:delta";

/// Maximum stream length (approximate trimming)
const L2_STREAM_MAX_LEN: usize = 10_000;

/// Redis 增量写入器
///
/// 使用 pipeline 将多个 delta 合并为单次 Redis round-trip。
/// 典型场景：一次撮合产生 3-5 个档位变更，pipeline 合并后仅 1 次网络请求。
pub struct RedisWriter {
    conn: MultiplexedConnection,
}

impl RedisWriter {
    /// 批量写入 delta 到 Redis
    ///
    /// 所有操作通过 pipeline 合并，单次 round-trip 完成：
    /// 1. HSET/HDEL dex:l2book:{id} — 更新变化的价格档位
    /// 2. HSET dex:l2book:{id}:meta — 更新 sequence 和 timestamp
    /// 3. HSET dex:bbo:{id} — 更新 BBO（仅在 BBO 变化时）
    /// 4. XADD dex:stream:l2:delta — 推送增量到 WS consumer
    pub async fn write_deltas(
        &self,
        deltas: &[PendingDelta],
        bbo_tracker: &BboTracker,
    ) -> Result<()> {
        if deltas.is_empty() {
            return Ok(());
        }

        let mut pipe = redis::pipe();
        let mut updated_markets: HashMap<u32, (u64, u64)> = HashMap::new(); // perpetual_id -> (seq, ts)
        let mut bbo_updates: HashMap<u32, bool> = HashMap::new();

        for delta in deltas {
            let book_key = format!("dex:l2book:{}", delta.perpetual_id);
            let side_prefix = match delta.side {
                Side::Bid => "b",
                Side::Ask => "a",
            };
            let field = format!("{}:{}", side_prefix, delta.price);

            if delta.new_quantity == 0 {
                // 档位清空 → HDEL
                pipe.cmd("HDEL").arg(&book_key).arg(&field);
            } else {
                // 档位更新 → HSET
                pipe.cmd("HSET")
                    .arg(&book_key)
                    .arg(&field)
                    .arg(delta.new_quantity.to_string());
            }

            updated_markets.insert(
                delta.perpetual_id,
                (delta.sequence, delta.timestamp_ms),
            );
        }

        // 更新每个变更市场的 metadata
        for (&perpetual_id, &(sequence, timestamp_ms)) in &updated_markets {
            let meta_key = format!("dex:l2book:{}:meta", perpetual_id);
            pipe.cmd("HSET")
                .arg(&meta_key)
                .arg("sequence")
                .arg(sequence)
                .arg("timestamp")
                .arg(timestamp_ms);

            // BBO 更新（如果 tracker 中有变化）
            if let Some(bbo) = bbo_tracker.get(perpetual_id) {
                let bbo_key = format!("dex:bbo:{}", perpetual_id);
                pipe.cmd("HSET")
                    .arg(&bbo_key)
                    .arg("best_bid")
                    .arg(bbo.best_bid)
                    .arg("best_bid_qty")
                    .arg(bbo.best_bid_qty)
                    .arg("best_ask")
                    .arg(bbo.best_ask)
                    .arg("best_ask_qty")
                    .arg(bbo.best_ask_qty)
                    .arg("sequence")
                    .arg(bbo.sequence)
                    .arg("timestamp")
                    .arg(bbo.timestamp_ms);
            }
        }

        // XADD 增量 delta 到 stream（供 dex-api WS consumer 消费）
        let stream_data = self.build_stream_payload(deltas);
        pipe.cmd("XADD")
            .arg(L2_DELTA_STREAM)
            .arg("MAXLEN")
            .arg("~")
            .arg(L2_STREAM_MAX_LEN)
            .arg("*")
            .arg("data")
            .arg(&stream_data);

        // 执行 pipeline
        let mut conn = self.conn.clone();
        let _: () = pipe
            .query_async(&mut conn)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Redis pipeline write failed: {}", e);
            });

        Ok(())
    }

    /// 构建 Stream 推送的 JSON payload
    fn build_stream_payload(&self, deltas: &[PendingDelta]) -> String {
        // 按 perpetual_id 分组
        let mut grouped: HashMap<u32, Vec<&PendingDelta>> = HashMap::new();
        for delta in deltas {
            grouped
                .entry(delta.perpetual_id)
                .or_default()
                .push(delta);
        }

        let payload: Vec<serde_json::Value> = grouped
            .iter()
            .map(|(&perpetual_id, deltas)| {
                let levels: Vec<serde_json::Value> = deltas
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "side": if d.side == Side::Bid { "bid" } else { "ask" },
                            "price": d.price.to_string(),
                            "size": d.new_quantity.to_string(),
                        })
                    })
                    .collect();

                serde_json::json!({
                    "perpetualId": perpetual_id,
                    "levels": levels,
                    "sequence": deltas.last().map(|d| d.sequence).unwrap_or(0),
                    "timestampMs": deltas.last().map(|d| d.timestamp_ms).unwrap_or(0),
                })
            })
            .collect();

        serde_json::to_string(&payload).unwrap_or_else(|_| "[]".to_string())
    }
}
```

### 5.4 Redis 操作示例

单次撮合后的典型 Redis pipeline（假设 BTC-USDC 市场 perpetual_id=0，一笔 taker 吃掉了 50100 档位的 ask，并在 50000 增加了 bid 深度）：

```redis
# === Pipeline 开始 ===

# 1. 更新变化的价格档位
HSET dex:l2book:0 b:50000 1500          # 50000 档 bid 数量更新为 1500
HDEL dex:l2book:0 a:50100                # 50100 档 ask 被完全吃掉，删除

# 2. 更新 metadata
HSET dex:l2book:0:meta sequence 42 timestamp 1709000000000

# 3. 更新 BBO（因 best_ask 变化）
HSET dex:bbo:0 best_bid 50000 best_bid_qty 1500 best_ask 50200 best_ask_qty 800 sequence 42 timestamp 1709000000000

# 4. 推送增量 delta 到 Stream
XADD dex:stream:l2:delta MAXLEN ~ 10000 * data '[{"perpetualId":0,"levels":[{"side":"bid","price":"50000","size":"1500"},{"side":"ask","price":"50100","size":"0"}],"sequence":42,"timestampMs":1709000000000}]'

# === Pipeline 结束（单次 round-trip） ===
```

### 5.5 从 HSET 恢复全量快照

当新的 WS 客户端连接或 gap 恢复时，需要从 Redis 重建全量订单簿：

```redis
# 1. 获取所有档位
HSCAN dex:l2book:0 0 COUNT 1000
# 返回: ["b:50000", "1500", "b:49900", "200", "a:50200", "800", "a:50300", "1200", ...]

# 2. 获取当前 sequence
HGET dex:l2book:0:meta sequence
# 返回: "42"

# 3. 客户端从 sequence 42 开始接收后续增量 delta
```

恢复代码：

```rust
impl RedisWriter {
    /// 从 Redis HSET 重建全量 L2Book
    pub async fn load_snapshot(&self, perpetual_id: u32) -> Result<L2Book> {
        let mut conn = self.conn.clone();
        let book_key = format!("dex:l2book:{}", perpetual_id);
        let meta_key = format!("dex:l2book:{}:meta", perpetual_id);

        // HGETALL 获取所有档位
        let fields: HashMap<String, String> = redis::cmd("HGETALL")
            .arg(&book_key)
            .query_async(&mut conn)
            .await?;

        let mut book = L2Book::new(perpetual_id);
        for (field, value) in &fields {
            let qty: u64 = value.parse().unwrap_or(0);
            if qty == 0 {
                continue;
            }

            if let Some(price_str) = field.strip_prefix("b:") {
                if let Ok(price) = price_str.parse::<u64>() {
                    book.bids.insert(price, qty);
                }
            } else if let Some(price_str) = field.strip_prefix("a:") {
                if let Ok(price) = price_str.parse::<u64>() {
                    book.asks.insert(price, qty);
                }
            }
        }

        // 获取 sequence
        let seq: Option<u64> = redis::cmd("HGET")
            .arg(&meta_key)
            .arg("sequence")
            .query_async(&mut conn)
            .await
            .ok();
        book.sequence = seq.unwrap_or(0);

        Ok(book)
    }
}
```

---

## 6. Snapshot + Delta 模式 (客户端协议)

### 6.1 新连接流程

当 WS 客户端订阅 `l2BookDelta:{perpetual_id}` 频道时：

```
客户端                    dex-api                     Redis
  │  subscribe l2Book:0     │                           │
  │ ──────────────────────> │                           │
  │                         │  HSCAN dex:l2book:0       │
  │                         │ ─────────────────────────>│
  │                         │  <─── 全量 field 数据     │
  │                         │  HGET dex:l2book:0:meta   │
  │                         │ ─────────────────────────>│
  │                         │  <─── sequence=42         │
  │  snapshot (seq=42)      │                           │
  │ <────────────────────── │                           │
  │                         │                           │
  │  delta (seq=43)         │   XREAD l2:delta          │
  │ <────────────────────── │ <─────────────────────────│
  │  delta (seq=44)         │                           │
  │ <────────────────────── │                           │
```

### 6.2 消息格式

**快照消息**（首次推送）：

```json
{
  "channel": "l2Book",
  "data": {
    "perpetualId": 0,
    "type": "snapshot",
    "bids": [
      {"price": "50000", "size": "1500"},
      {"price": "49900", "size": "200"}
    ],
    "asks": [
      {"price": "50200", "size": "800"},
      {"price": "50300", "size": "1200"}
    ],
    "sequence": 42,
    "timestampMs": 1709000000000
  }
}
```

**增量消息**（后续推送）：

```json
{
  "channel": "l2Book",
  "data": {
    "perpetualId": 0,
    "type": "delta",
    "levels": [
      {"side": "bid", "price": "50000", "size": "1800"},
      {"side": "ask", "price": "50200", "size": "0"}
    ],
    "sequence": 43,
    "timestampMs": 1709000000100
  }
}
```

**BBO 消息**（独立频道，更高频）：

```json
{
  "channel": "bbo",
  "data": {
    "perpetualId": 0,
    "bestBid": "50000",
    "bestBidQty": "1800",
    "bestAsk": "50300",
    "bestAskQty": "1200",
    "sequence": 43,
    "timestampMs": 1709000000100
  }
}
```

---

## 7. 初始化与恢复

### 7.1 启动流程

```
dex-streamer 启动
    │
    ├── 1. 解析配置（Redis URL, channel capacity, flush interval）
    │
    ├── 2. 连接 Redis
    │      └── 检查连通性，打印版本信息
    │
    ├── 3. 连接 DexStreamingManager broadcast channel
    │      └── 通过 StreamTransport trait 抽象（进程内 channel 或 gRPC）
    │
    ├── 4. 等待初始 snapshot
    │      │
    │      ├── 方案 A: 从 stream 接收第一个 OrderbookSnapshotEvent
    │      │   └── DexStreamingManager 启动时会发送一次全量快照
    │      │
    │      └── 方案 B: 从 Redis 加载 dex:orderbook:{id}（dex-indexer 写入的全量快照）
    │          └── 作为 fallback，当 stream 还未收到快照时使用
    │
    ├── 5. 初始化每个市场的 L2Book
    │      └── 从快照构建内存订单簿，设置 sequence
    │
    ├── 6. 将初始状态写入 Redis dex:l2book:{id}
    │      └── 逐个 field HSET（保证初始状态一致）
    │
    ├── 7. 启动 delta 处理循环
    │      └── tokio::select! { stream_event, flush_tick, reconcile_tick }
    │
    └── 8. 启动对账定时器（每 30s）
```

### 7.2 主循环

```rust
/// dex-streamer 主运行循环
pub async fn run(
    mut transport: impl StreamTransport,
    redis_writer: RedisWriter,
    config: StreamerConfig,
) -> Result<()> {
    let mut builder = OrderbookBuilder::new(redis_writer, config.clone());

    // 初始化: 加载快照
    let initial_batch = transport.recv().await?;
    builder.initialize_from_batch(&initial_batch).await?;

    // 定时器
    let mut flush_interval = tokio::time::interval(
        Duration::from_millis(config.flush_interval_ms)
    );
    let mut reconcile_interval = tokio::time::interval(
        Duration::from_secs(config.reconcile_interval_secs)
    );

    loop {
        tokio::select! {
            // 接收 delta 事件
            batch = transport.recv() => {
                match batch {
                    Ok(batch) => {
                        builder.process_batch(&batch).await?;
                    }
                    Err(e) => {
                        tracing::error!("Transport recv error: {}", e);
                        // 短暂等待后重试
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
            // 定时 flush
            _ = flush_interval.tick() => {
                builder.flush().await?;
            }
            // 定时对账
            _ = reconcile_interval.tick() => {
                builder.reconcile().await?;
            }
        }
    }
}
```

### 7.3 Gap 恢复策略

当检测到 sequence gap 时（如 dex-streamer 短暂断连或 broadcast channel 溢出）：

```
检测到 gap (expected=43, got=46)
    │
    ├── 1. 记录 warn 日志，含 gap 范围
    │
    ├── 2. 从 Redis 加载 dex:orderbook:{id}（dex-indexer 的全量快照）
    │      └── 这是 Checkpoint 通道写入的权威数据
    │
    ├── 3. 用快照替换内存 L2Book
    │
    ├── 4. 重写 Redis dex:l2book:{id}（先 DEL 再全量 HSET）
    │
    ├── 5. 重置 sequence = 快照的 sequence
    │
    └── 6. 继续处理后续 delta
```

---

## 8. Checkpoint 对账 (Reconciliation)

### 8.1 对账原理

dex-indexer 通过 Checkpoint 管线处理 `OrderbookSnapshotEvent`，写入 `dex:orderbook:{perpetual_id}`。这是经过共识确认的权威数据。dex-streamer 的增量数据是 best-effort 的乐观更新。

对账器定期比较两者，确保增量数据不会长期偏离权威数据。

### 8.2 Reconciler 实现

```rust
/// Checkpoint 对账模块
///
/// 定期比对 dex-streamer 增量构建的 L2Book 与 dex-indexer Checkpoint 快照，
/// 发现偏差时用 Checkpoint 数据修正增量数据。
pub struct Reconciler {
    /// 对账检查间隔（默认 30s）
    check_interval: Duration,
    /// 容忍的最大偏差档位数（超过此数触发修正）
    max_drift_levels: usize,
    /// 累计修正次数（metrics 指标）
    drift_corrections: u64,
}

impl Reconciler {
    pub fn new(check_interval: Duration, max_drift_levels: usize) -> Self {
        Self {
            check_interval,
            max_drift_levels,
            drift_corrections: 0,
        }
    }

    /// 执行对账
    ///
    /// 对比步骤:
    /// 1. 从 Redis 读取 dex:orderbook:{perpetual_id} (dex-indexer 的 Checkpoint 快照)
    /// 2. 与内存 L2Book 逐档位比较
    /// 3. 如果偏差超过阈值，用 Checkpoint 快照替换
    pub async fn reconcile(
        &mut self,
        book: &mut L2Book,
        redis_conn: &mut MultiplexedConnection,
    ) -> Result<ReconcileResult> {
        let ob_key = format!("dex:orderbook:{}", book.perpetual_id);

        // 1. 读取 dex-indexer 写入的全量快照
        let (bids_json, asks_json): (Option<String>, Option<String>) =
            redis::cmd("HMGET")
                .arg(&ob_key)
                .arg("bids")
                .arg("asks")
                .query_async(redis_conn)
                .await?;

        let checkpoint_bids = parse_price_levels(&bids_json.unwrap_or_default());
        let checkpoint_asks = parse_price_levels(&asks_json.unwrap_or_default());

        // 2. 逐档位比较
        let mut drift_count = 0;
        let mut drifts: Vec<String> = Vec::new();

        // 比较 bids
        for (&price, &qty) in &checkpoint_bids {
            match book.bids.get(&price) {
                Some(&book_qty) if book_qty != qty => {
                    drift_count += 1;
                    drifts.push(format!(
                        "bid@{}: streamer={} checkpoint={}",
                        price, book_qty, qty
                    ));
                }
                None => {
                    drift_count += 1;
                    drifts.push(format!("bid@{}: streamer=MISSING checkpoint={}", price, qty));
                }
                _ => {} // 一致
            }
        }
        // 检查 streamer 中有但 checkpoint 中没有的档位
        for (&price, &qty) in &book.bids {
            if !checkpoint_bids.contains_key(&price) {
                drift_count += 1;
                drifts.push(format!("bid@{}: streamer={} checkpoint=MISSING", price, qty));
            }
        }

        // 比较 asks（同理）
        for (&price, &qty) in &checkpoint_asks {
            match book.asks.get(&price) {
                Some(&book_qty) if book_qty != qty => {
                    drift_count += 1;
                    drifts.push(format!(
                        "ask@{}: streamer={} checkpoint={}",
                        price, book_qty, qty
                    ));
                }
                None => {
                    drift_count += 1;
                    drifts.push(format!("ask@{}: streamer=MISSING checkpoint={}", price, qty));
                }
                _ => {}
            }
        }
        for (&price, &qty) in &book.asks {
            if !checkpoint_asks.contains_key(&price) {
                drift_count += 1;
                drifts.push(format!("ask@{}: streamer={} checkpoint=MISSING", price, qty));
            }
        }

        // 3. 判断是否需要修正
        if drift_count > self.max_drift_levels {
            tracing::warn!(
                perpetual_id = book.perpetual_id,
                drift_count,
                drifts = ?drifts,
                "Drift exceeded threshold, replacing with checkpoint snapshot"
            );

            // 用 Checkpoint 数据替换内存
            book.bids = checkpoint_bids;
            book.asks = checkpoint_asks;

            self.drift_corrections += 1;

            Ok(ReconcileResult::Corrected { drift_count })
        } else if drift_count > 0 {
            tracing::info!(
                perpetual_id = book.perpetual_id,
                drift_count,
                "Minor drift detected, within tolerance"
            );
            Ok(ReconcileResult::MinorDrift { drift_count })
        } else {
            tracing::debug!(
                perpetual_id = book.perpetual_id,
                "Reconciliation passed, no drift"
            );
            Ok(ReconcileResult::Ok)
        }
    }
}

pub enum ReconcileResult {
    /// 完全一致
    Ok,
    /// 小幅偏差，在容忍范围内
    MinorDrift { drift_count: usize },
    /// 偏差超标，已用 Checkpoint 数据修正
    Corrected { drift_count: usize },
}

/// 解析 dex-indexer 存储的 JSON 价格档位
/// 格式: [["50000","1500"],["49900","200"]]
fn parse_price_levels(json: &str) -> BTreeMap<u64, u64> {
    let mut levels = BTreeMap::new();
    if let Ok(arr) = serde_json::from_str::<Vec<[String; 2]>>(json) {
        for pair in arr {
            if let (Ok(price), Ok(qty)) = (pair[0].parse::<u64>(), pair[1].parse::<u64>()) {
                if qty > 0 {
                    levels.insert(price, qty);
                }
            }
        }
    }
    levels
}
```

### 8.3 对账时序

```
时间线:
0s ─── dex-streamer 启动，加载初始快照
       │
5ms ── delta(seq=1) → Redis
10ms ─ delta(seq=2) → Redis
...
30s ── 首次对账: 读取 dex:orderbook:0, 比对 → OK (0 drift)
       │
60s ── 第二次对账: 读取 dex:orderbook:0, 比对 → MinorDrift (2 levels)
       │  (2 < max_drift_levels=5, 不修正)
       │
90s ── 第三次对账: OK
       │
...
```

---

## 9. 与 dex-indexer 的职责划分

| 职责 | dex-indexer | dex-streamer |
|------|------------|-------------|
| **数据源** | Checkpoint (可靠, 1-3s 延迟) | DexStreamingManager broadcast (<50ms 延迟) |
| **存储目标** | PostgreSQL + Redis (全量) | Redis 仅 (增量) |
| **订单簿** | `dex:orderbook:{id}` (全量快照 JSON) | `dex:l2book:{id}` (增量 HSET field) |
| **Stream** | `dex:stream:orderbook` (全量推送) | `dex:stream:l2:delta` (增量推送) |
| **K线/统计** | CandleAggregator, MarketStatsAggregator | 不负责 |
| **订单历史** | dex_orders, dex_fills 表 (PostgreSQL) | 不负责 |
| **BBO** | 从全量快照中提取 (附带在 HSET 中) | 独立 `dex:bbo:{id}` (实时更新) |
| **可靠性** | 保证不丢数据 (Checkpoint 保证) | Best-effort + 对账修正 |
| **延迟** | 1.5-3.5s | <50ms |
| **持久化** | PostgreSQL 永久存储 | Redis 仅 (进程重启从 Checkpoint 恢复) |

### 9.1 数据一致性模型

```
dex-streamer (乐观, 快)        dex-indexer (权威, 慢)
    │                              │
    │  t=10ms: delta → Redis       │
    │                              │
    │  t=20ms: delta → Redis       │
    │                              │
    │                              │  t=2000ms: snapshot → Redis
    │                              │
    │  t=30000ms: 对账             │
    │  ← 读取 dex:orderbook ──────│
    │  比对 → OK / Corrected       │
```

**关键原则**：

1. **dex-streamer 的数据永远是 "乐观" 的**：可能比 Checkpoint 超前，也可能因 gap 而短暂不一致
2. **dex-indexer 的数据是 "权威" 的**：经过共识确认，保证最终一致
3. **对账是单向的**：只从 dex-indexer → dex-streamer 方向修正，不反过来
4. **两个通道的 Redis key 完全独立**：`dex:orderbook:*` vs `dex:l2book:*`，不会互相覆盖

---

## 10. 配置

```rust
use std::time::Duration;

/// dex-streamer 配置
#[derive(Clone, Debug)]
pub struct StreamerConfig {
    /// Redis 连接 URL
    pub redis_url: String,
    /// broadcast channel 容量
    pub channel_capacity: usize,
    /// Redis flush 间隔 (ms)
    pub flush_interval_ms: u64,
    /// 对账检查间隔 (s)
    pub reconcile_interval_secs: u64,
    /// 对账最大容忍偏差档位数
    pub max_drift_levels: usize,
    /// L2 delta stream 最大长度 (MAXLEN ~)
    pub l2_stream_max_len: usize,
}

impl Default for StreamerConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://localhost:6379".to_string(),
            channel_capacity: 10_000,
            flush_interval_ms: 5,
            reconcile_interval_secs: 30,
            max_drift_levels: 5,
            l2_stream_max_len: 10_000,
        }
    }
}
```

---

## 11. 性能预估

### 11.1 延迟分解

| 阶段 | 延迟 |
|------|------|
| DEX 引擎执行完成 → DexStreamingManager broadcast | <1ms |
| broadcast → dex-streamer 接收 | <1ms |
| 内存 L2Book 更新 | <0.01ms |
| pending buffer → Redis pipeline 写入 | ~2-5ms |
| **总计 (引擎 → Redis)** | **<10ms** |
| Redis → dex-api StreamConsumer → WS 推送 | ~5-10ms |
| **总计 (引擎 → 前端)** | **<20-50ms** |

### 11.2 吞吐量预估

| 指标 | 数值 |
|------|------|
| 单次 delta 处理 | <1us (内存操作) |
| Redis pipeline (5 个 delta) | ~2-5ms (网络 RTT) |
| 理论峰值 (每 5ms flush) | ~200 batch/s = ~1000 delta/s |
| 10 个市场，每市场 100 delta/s | 1000 delta/s (在峰值范围内) |

### 11.3 Redis 内存占用

| 数据 | 计算 | 估算 |
|------|------|------|
| L2 book (100 档/市场) | 200 field x ~30 bytes x 10 市场 | ~60KB |
| Meta (每市场) | 2 field x 10 市场 | ~0.5KB |
| BBO (每市场) | 6 field x 10 市场 | ~2KB |
| L2 delta stream | 10000 entries x ~200 bytes | ~2MB |
| **总计** | | **~2.1MB** |

---

## 12. 参考资料

| 资料 | 位置 |
|------|------|
| Phase 6 概览 | [00-overview.md](./00-overview.md) |
| 事件类型定义 | `dex-sui/crates/sui-types/src/dex_events.rs` |
| 当前 orderbook handler | `dex-sui/crates/dex-indexer/src/handlers/orderbook_snapshots.rs` |
| Redis publisher | `dex-sui/crates/dex-indexer/src/redis/publisher.rs` |
| Redis stream consumer | `dex-sui/crates/dex-api/src/ws/consumer.rs` |
| dYdX OffChain 分析 | `phase2_realtime/14-dydx-offchain-updates-analysis.md` |
| dYdX Streaming 参考 | `phase2_realtime/03-dydx-streaming-reference.md` |
| Redis 消息规范 | `phase2_realtime/10-redis-message-spec.md` |
