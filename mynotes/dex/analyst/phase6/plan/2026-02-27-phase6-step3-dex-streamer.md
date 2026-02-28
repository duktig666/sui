# Phase 6 Step 3: dex-stream-indexer 实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 实现 dex-stream-indexer 独立服务，从 sui-node gRPC 接收 DEX 事件，维护内存 L2 订单簿，写入 Redis 供 dex-api 推送 WebSocket。

**Architecture:** dex-stream-indexer 作为独立二进制连接 sui-node 的 gRPC Subscribe 流，接收 OrderbookDelta 事件更新内存 BTreeMap L2Book，每 5ms pipeline flush 到 Redis HSET + XADD 通知 dex-api。

**Tech Stack:** Rust, tonic gRPC client, redis 0.24, tokio, clap

**设计文档:** `docs/plans/2026-02-27-phase6-step3-dex-stream-indexer-design.md`

---

## Task 1: 创建 dex-stream-indexer crate 基础结构

**Files:**
- Create: `crates/dex-stream-indexer/Cargo.toml`
- Create: `crates/dex-stream-indexer/src/main.rs`
- Create: `crates/dex-stream-indexer/src/lib.rs`
- Create: `crates/dex-stream-indexer/src/config.rs`
- Modify: `Cargo.toml` (workspace members + dependencies)

**Step 1: 创建 Cargo.toml**

文件: `crates/dex-stream-indexer/Cargo.toml`

```toml
[package]
name = "dex-stream-indexer"
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"
publish = false

[[bin]]
name = "dex-stream-indexer"
path = "src/main.rs"

[dependencies]
dex-node-stream-framework.workspace = true
tokio = { workspace = true, features = ["full"] }
tonic.workspace = true
tonic-prost.workspace = true
prost.workspace = true
tokio-stream.workspace = true
tracing.workspace = true
telemetry-subscribers.workspace = true
clap = { workspace = true, features = ["env"] }
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
redis = { version = "0.24", features = ["aio", "tokio-comp"] }
```

**Step 2: 创建 config.rs**

文件: `crates/dex-stream-indexer/src/config.rs`

```rust
use std::time::Duration;

/// Configuration for the dex-stream-indexer service
#[derive(Debug, Clone)]
pub struct StreamerConfig {
    /// gRPC server address (sui-node DexStreaming)
    pub grpc_addr: String,
    /// Redis connection URL
    pub redis_url: String,
    /// Redis flush interval
    pub flush_interval: Duration,
    /// Redis stream max length for l2:update notifications
    pub l2_stream_max_len: usize,
    /// Market IDs to subscribe (empty = all)
    pub market_ids: Vec<u32>,
}

impl StreamerConfig {
    pub fn new(
        grpc_addr: String,
        redis_url: String,
        flush_interval_ms: u64,
        l2_stream_max_len: usize,
        market_ids: Vec<u32>,
    ) -> Self {
        Self {
            grpc_addr,
            redis_url,
            flush_interval: Duration::from_millis(flush_interval_ms),
            l2_stream_max_len,
            market_ids,
        }
    }
}
```

**Step 3: 创建 lib.rs**

文件: `crates/dex-stream-indexer/src/lib.rs`

```rust
pub mod config;
```

**Step 4: 创建 main.rs（空壳）**

文件: `crates/dex-stream-indexer/src/main.rs`

```rust
use anyhow::Result;
use clap::Parser;
use tracing::info;

use dex_stream_indexer::config::StreamerConfig;

#[derive(Parser, Debug)]
#[command(name = "dex-stream-indexer", about = "DEX L2 Orderbook Streaming Service")]
struct Args {
    /// gRPC address of sui-node DexStreaming service
    #[arg(long, default_value = "http://127.0.0.1:50052", env = "DEX_GRPC_ADDR")]
    grpc_addr: String,

    /// Redis connection URL
    #[arg(long, env = "REDIS_URL")]
    redis_url: String,

    /// Redis flush interval in milliseconds
    #[arg(long, default_value = "5", env = "FLUSH_INTERVAL_MS")]
    flush_interval_ms: u64,

    /// Redis stream max length for l2:update
    #[arg(long, default_value = "10000", env = "L2_STREAM_MAX_LEN")]
    l2_stream_max_len: usize,

    /// Market IDs to subscribe (comma-separated, empty = all)
    #[arg(long, value_delimiter = ',', env = "MARKET_IDS")]
    market_ids: Vec<u32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _guard = telemetry_subscribers::TelemetryConfig::new()
        .with_env()
        .init();

    let args = Args::parse();

    info!(
        grpc_addr = %args.grpc_addr,
        redis_url = %args.redis_url,
        flush_interval_ms = args.flush_interval_ms,
        market_ids = ?args.market_ids,
        "Starting dex-stream-indexer"
    );

    let _config = StreamerConfig::new(
        args.grpc_addr,
        args.redis_url,
        args.flush_interval_ms,
        args.l2_stream_max_len,
        args.market_ids,
    );

    // TODO: implement run loop in subsequent tasks
    info!("dex-stream-indexer started (no-op placeholder)");
    Ok(())
}
```

**Step 5: 添加到 workspace**

文件: `Cargo.toml` (workspace root)

在 `"crates/dex-node-stream-framework",` 之后添加:
```toml
  "crates/dex-stream-indexer",
```

在 `[workspace.dependencies]` 中添加:
```toml
dex-stream-indexer = { path = "crates/dex-stream-indexer" }
```

**Step 6: 编译验证**

Run: `cargo check -p dex-stream-indexer`
Expected: 编译通过

**Step 7: Commit**

```bash
git add crates/dex-stream-indexer/ Cargo.toml Cargo.lock
git commit -m "feat(dex): create dex-stream-indexer crate with CLI skeleton"
```

---

## Task 2: L2Book 内存订单簿实现

**Files:**
- Create: `crates/dex-stream-indexer/src/orderbook_builder.rs`
- Modify: `crates/dex-stream-indexer/src/lib.rs`

**Step 1: 创建 orderbook_builder.rs**

文件: `crates/dex-stream-indexer/src/orderbook_builder.rs`

```rust
use std::collections::{BTreeMap, HashMap};

use dex_node_stream_framework::proto::dex_streaming_v1::{
    DexStreamBatchProto, OrderbookDeltaEventProto, dex_stream_event_proto,
};
use tracing::{debug, warn};

/// In-memory L2 orderbook for a single market
#[derive(Debug)]
pub struct L2Book {
    pub perpetual_id: u32,
    /// Bid side: price → quantity (descending iteration for best bid)
    pub bids: BTreeMap<u64, u64>,
    /// Ask side: price → quantity (ascending iteration for best ask)
    pub asks: BTreeMap<u64, u64>,
    /// Last applied sequence number
    pub sequence: u64,
    /// Last update timestamp
    pub updated_at: u64,
    /// Whether book has pending changes not yet flushed to Redis
    pub dirty: bool,
    /// Whether this book needs a full snapshot recovery
    pub stale: bool,
}

impl L2Book {
    pub fn new(perpetual_id: u32) -> Self {
        Self {
            perpetual_id,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            sequence: 0,
            updated_at: 0,
            dirty: false,
            stale: true, // starts stale until first snapshot
        }
    }

    /// Initialize from a full snapshot
    pub fn init_from_snapshot(
        &mut self,
        bids: &[(u64, u64)],
        asks: &[(u64, u64)],
        sequence: u64,
        timestamp_ms: u64,
    ) {
        self.bids.clear();
        self.asks.clear();
        for &(price, qty) in bids {
            if qty > 0 {
                self.bids.insert(price, qty);
            }
        }
        for &(price, qty) in asks {
            if qty > 0 {
                self.asks.insert(price, qty);
            }
        }
        self.sequence = sequence;
        self.updated_at = timestamp_ms;
        self.dirty = true;
        self.stale = false;
    }

    /// Apply a single delta event. Returns true if applied, false if gap detected.
    pub fn apply_delta(&mut self, delta: &OrderbookDeltaEventProto) -> bool {
        // Gap detection: expect sequence = current + 1
        if !self.stale && delta.sequence != self.sequence + 1 {
            // Possible node restart: sequence dropped significantly
            if delta.sequence < self.sequence {
                warn!(
                    perpetual_id = self.perpetual_id,
                    expected = self.sequence + 1,
                    got = delta.sequence,
                    "Sequence regression detected, likely node restart"
                );
            } else {
                warn!(
                    perpetual_id = self.perpetual_id,
                    expected = self.sequence + 1,
                    got = delta.sequence,
                    "Sequence gap detected"
                );
            }
            self.stale = true;
            return false;
        }

        // Apply each price level update
        for update in &delta.updates {
            let book = if update.side == 0 { &mut self.bids } else { &mut self.asks };
            if update.quantity == 0 {
                book.remove(&update.price);
            } else {
                book.insert(update.price, update.quantity);
            }
        }

        self.sequence = delta.sequence;
        self.updated_at = delta.timestamp_ms;
        self.dirty = true;
        false // no gap (stale=false means applied ok)
    }

    /// Get best bid (highest price)
    pub fn best_bid(&self) -> Option<(u64, u64)> {
        self.bids.iter().next_back().map(|(&p, &q)| (p, q))
    }

    /// Get best ask (lowest price)
    pub fn best_ask(&self) -> Option<(u64, u64)> {
        self.asks.iter().next().map(|(&p, &q)| (p, q))
    }
}

/// Manages L2Books for all markets
pub struct OrderbookBuilder {
    books: HashMap<u32, L2Book>,
}

impl OrderbookBuilder {
    pub fn new(market_ids: &[u32]) -> Self {
        let mut books = HashMap::new();
        for &id in market_ids {
            books.insert(id, L2Book::new(id));
        }
        Self { books }
    }

    /// Get or create a book for a market
    pub fn get_or_create(&mut self, perpetual_id: u32) -> &mut L2Book {
        self.books
            .entry(perpetual_id)
            .or_insert_with(|| L2Book::new(perpetual_id))
    }

    /// Process a batch of events from gRPC stream
    pub fn process_batch(&mut self, batch: &DexStreamBatchProto) -> Vec<u32> {
        let mut stale_markets = Vec::new();

        for event_proto in &batch.events {
            if let Some(ref event) = event_proto.event {
                match event {
                    dex_stream_event_proto::Event::OrderbookDelta(delta) => {
                        let book = self.get_or_create(delta.perpetual_id);
                        if book.stale {
                            // Already stale, skip until snapshot recovery
                            if !stale_markets.contains(&delta.perpetual_id) {
                                stale_markets.push(delta.perpetual_id);
                            }
                            continue;
                        }
                        book.apply_delta(delta);
                        if book.stale {
                            stale_markets.push(delta.perpetual_id);
                        }
                    }
                    // Other events (fills, orders) are passed through but don't update L2Book
                    _ => {}
                }
            }
        }

        stale_markets
    }

    /// Get all dirty books that need Redis flush
    pub fn dirty_books(&self) -> Vec<&L2Book> {
        self.books.values().filter(|b| b.dirty && !b.stale).collect()
    }

    /// Mark a book as flushed
    pub fn mark_flushed(&mut self, perpetual_id: u32) {
        if let Some(book) = self.books.get_mut(&perpetual_id) {
            book.dirty = false;
        }
    }

    /// Get all stale markets that need snapshot recovery
    pub fn stale_markets(&self) -> Vec<u32> {
        self.books
            .values()
            .filter(|b| b.stale)
            .map(|b| b.perpetual_id)
            .collect()
    }

    /// Get a mutable reference to a book
    pub fn get_book_mut(&mut self, perpetual_id: u32) -> Option<&mut L2Book> {
        self.books.get_mut(&perpetual_id)
    }
}
```

**Step 2: 更新 lib.rs**

文件: `crates/dex-stream-indexer/src/lib.rs`

```rust
pub mod config;
pub mod orderbook_builder;
```

**Step 3: 编译验证**

Run: `cargo check -p dex-stream-indexer`
Expected: 编译通过

**Step 4: Commit**

```bash
git add crates/dex-stream-indexer/src/orderbook_builder.rs crates/dex-stream-indexer/src/lib.rs
git commit -m "feat(dex): implement L2Book in-memory orderbook with delta application"
```

---

## Task 3: Redis Writer 实现

**Files:**
- Create: `crates/dex-stream-indexer/src/redis_writer.rs`
- Modify: `crates/dex-stream-indexer/src/lib.rs`

**Step 1: 创建 redis_writer.rs**

文件: `crates/dex-stream-indexer/src/redis_writer.rs`

使用 `redis::cmd()` 模式（避免 trait 方法类型推断问题，参考 MEMORY.md）。

```rust
use anyhow::Result;
use redis::aio::MultiplexedConnection;
use tracing::{debug, error};

use crate::orderbook_builder::L2Book;

/// Redis key patterns
pub mod keys {
    pub fn l2book(perpetual_id: u32) -> String {
        format!("dex:l2book:{}", perpetual_id)
    }

    pub fn l2book_meta(perpetual_id: u32) -> String {
        format!("dex:l2book:{}:meta", perpetual_id)
    }

    pub fn bbo(perpetual_id: u32) -> String {
        format!("dex:bbo:{}", perpetual_id)
    }

    pub const L2_UPDATE_STREAM: &str = "dex:stream:l2:update";
}

/// Handles batched Redis writes using pipeline
pub struct RedisWriter {
    conn: MultiplexedConnection,
    l2_stream_max_len: usize,
}

impl RedisWriter {
    pub async fn new(redis_url: &str, l2_stream_max_len: usize) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let conn = client.get_multiplexed_async_connection().await?;
        Ok(Self {
            conn,
            l2_stream_max_len,
        })
    }

    /// Flush a single L2Book to Redis using pipeline
    pub async fn flush_book(
        &mut self,
        book: &L2Book,
        prev_bbo: Option<(Option<(u64, u64)>, Option<(u64, u64)>)>,
    ) -> Result<()> {
        let l2_key = keys::l2book(book.perpetual_id);
        let meta_key = keys::l2book_meta(book.perpetual_id);

        // First: delete the entire L2 hash and rewrite (simpler than diffing)
        // For small orderbooks (<200 levels) this is efficient enough
        let mut pipe = redis::pipe();

        // Delete old L2 data
        pipe.cmd("DEL").arg(&l2_key);

        // Write all bid levels
        for (&price, &qty) in &book.bids {
            pipe.cmd("HSET")
                .arg(&l2_key)
                .arg(format!("b:{}", price))
                .arg(qty);
        }

        // Write all ask levels
        for (&price, &qty) in &book.asks {
            pipe.cmd("HSET")
                .arg(&l2_key)
                .arg(format!("a:{}", price))
                .arg(qty);
        }

        // Update metadata
        pipe.cmd("HSET")
            .arg(&meta_key)
            .arg("sequence")
            .arg(book.sequence)
            .arg("timestamp")
            .arg(book.updated_at);

        // Update BBO
        let bbo_key = keys::bbo(book.perpetual_id);
        let best_bid = book.best_bid();
        let best_ask = book.best_ask();

        // Only write BBO if changed
        let bbo_changed = match prev_bbo {
            Some((prev_bid, prev_ask)) => best_bid != prev_bid || best_ask != prev_ask,
            None => true,
        };

        if bbo_changed {
            pipe.cmd("HSET")
                .arg(&bbo_key)
                .arg("best_bid")
                .arg(best_bid.map(|(p, _)| p).unwrap_or(0))
                .arg("best_bid_qty")
                .arg(best_bid.map(|(_, q)| q).unwrap_or(0))
                .arg("best_ask")
                .arg(best_ask.map(|(p, _)| p).unwrap_or(0))
                .arg("best_ask_qty")
                .arg(best_ask.map(|(_, q)| q).unwrap_or(0));
        }

        // Publish notification to l2:update stream
        let notification = serde_json::json!({
            "perpetual_id": book.perpetual_id,
            "sequence": book.sequence,
            "timestamp_ms": book.updated_at,
            "type": "l2_update"
        });
        pipe.cmd("XADD")
            .arg(keys::L2_UPDATE_STREAM)
            .arg("MAXLEN")
            .arg("~")
            .arg(self.l2_stream_max_len)
            .arg("*")
            .arg("data")
            .arg(notification.to_string());

        // Execute pipeline
        let _: Vec<redis::Value> = pipe
            .query_async(&mut self.conn)
            .await
            .map_err(|e| {
                error!(perpetual_id = book.perpetual_id, "Redis pipeline failed: {}", e);
                e
            })?;

        debug!(
            perpetual_id = book.perpetual_id,
            sequence = book.sequence,
            bids = book.bids.len(),
            asks = book.asks.len(),
            "Flushed L2Book to Redis"
        );

        Ok(())
    }
}
```

**Step 2: 更新 lib.rs**

添加 `pub mod redis_writer;`

**Step 3: 编译验证**

Run: `cargo check -p dex-stream-indexer`
Expected: 编译通过

**Step 4: Commit**

```bash
git add crates/dex-stream-indexer/src/redis_writer.rs crates/dex-stream-indexer/src/lib.rs
git commit -m "feat(dex): implement Redis pipeline writer for L2Book flush"
```

---

## Task 4: gRPC Client + 主运行循环

**Files:**
- Create: `crates/dex-stream-indexer/src/grpc_client.rs`
- Modify: `crates/dex-stream-indexer/src/main.rs`
- Modify: `crates/dex-stream-indexer/src/lib.rs`

**Step 1: 创建 grpc_client.rs**

文件: `crates/dex-stream-indexer/src/grpc_client.rs`

```rust
use anyhow::{Context, Result};
use dex_node_stream_framework::proto::dex_streaming_v1::{
    DexStreamBatchProto, L2BookSnapshot, SnapshotRequest, SubscribeRequest,
    dex_streaming_client::DexStreamingClient,
};
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tracing::{error, info, warn};

/// Connect to the gRPC server with retry
pub async fn connect(grpc_addr: &str) -> Result<DexStreamingClient<Channel>> {
    info!(addr = %grpc_addr, "Connecting to DexStreaming gRPC server");
    let client = DexStreamingClient::connect(grpc_addr.to_string())
        .await
        .context("Failed to connect to DexStreaming gRPC")?;
    info!("Connected to DexStreaming gRPC server");
    Ok(client)
}

/// Connect with exponential backoff retry
pub async fn connect_with_retry(grpc_addr: &str, max_retries: u32) -> Result<DexStreamingClient<Channel>> {
    let mut delay_ms = 100u64;
    for attempt in 0..max_retries {
        match DexStreamingClient::connect(grpc_addr.to_string()).await {
            Ok(client) => {
                info!(attempt, "Connected to DexStreaming gRPC server");
                return Ok(client);
            }
            Err(e) => {
                warn!(attempt, delay_ms, "gRPC connection failed: {}, retrying...", e);
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms = (delay_ms * 2).min(10_000);
            }
        }
    }
    anyhow::bail!("Failed to connect after {} retries", max_retries)
}

/// Request a full L2 snapshot for a market
pub async fn get_snapshot(
    client: &mut DexStreamingClient<Channel>,
    market_id: u32,
) -> Result<L2BookSnapshot> {
    let response = client
        .get_snapshot(SnapshotRequest { market_id })
        .await
        .context(format!("GetSnapshot failed for market {}", market_id))?;
    Ok(response.into_inner())
}
```

**Step 2: 修改 main.rs — 实现完整运行循环**

文件: `crates/dex-stream-indexer/src/main.rs`

```rust
use std::collections::HashMap;

use anyhow::Result;
use clap::Parser;
use dex_node_stream_framework::proto::dex_streaming_v1::SubscribeRequest;
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

use dex_stream_indexer::config::StreamerConfig;
use dex_stream_indexer::grpc_client;
use dex_stream_indexer::orderbook_builder::OrderbookBuilder;
use dex_stream_indexer::redis_writer::RedisWriter;

#[derive(Parser, Debug)]
#[command(name = "dex-stream-indexer", about = "DEX L2 Orderbook Streaming Service")]
struct Args {
    /// gRPC address of sui-node DexStreaming service
    #[arg(long, default_value = "http://127.0.0.1:50052", env = "DEX_GRPC_ADDR")]
    grpc_addr: String,

    /// Redis connection URL
    #[arg(long, env = "REDIS_URL")]
    redis_url: String,

    /// Redis flush interval in milliseconds
    #[arg(long, default_value = "5", env = "FLUSH_INTERVAL_MS")]
    flush_interval_ms: u64,

    /// Redis stream max length for l2:update
    #[arg(long, default_value = "10000", env = "L2_STREAM_MAX_LEN")]
    l2_stream_max_len: usize,

    /// Market IDs to subscribe (comma-separated, empty = all)
    #[arg(long, value_delimiter = ',', env = "MARKET_IDS")]
    market_ids: Vec<u32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _guard = telemetry_subscribers::TelemetryConfig::new()
        .with_env()
        .init();

    let args = Args::parse();

    info!(
        grpc_addr = %args.grpc_addr,
        redis_url = %args.redis_url,
        flush_interval_ms = args.flush_interval_ms,
        market_ids = ?args.market_ids,
        "Starting dex-stream-indexer"
    );

    let config = StreamerConfig::new(
        args.grpc_addr,
        args.redis_url,
        args.flush_interval_ms,
        args.l2_stream_max_len,
        args.market_ids,
    );

    // Main loop with automatic reconnection
    loop {
        match run_streaming_loop(&config).await {
            Ok(()) => {
                info!("Streaming loop completed normally");
                break;
            }
            Err(e) => {
                error!("Streaming loop failed: {:#}, reconnecting in 5s...", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    Ok(())
}

async fn run_streaming_loop(config: &StreamerConfig) -> Result<()> {
    // Connect to gRPC
    let mut client = grpc_client::connect_with_retry(&config.grpc_addr, 10).await?;

    // Connect to Redis
    let mut redis_writer = RedisWriter::new(&config.redis_url, config.l2_stream_max_len).await?;
    info!("Connected to Redis");

    // Create orderbook builder
    let mut builder = OrderbookBuilder::new(&config.market_ids);

    // Initialize all markets with snapshots
    for &market_id in &config.market_ids {
        match grpc_client::get_snapshot(&mut client, market_id).await {
            Ok(snapshot) => {
                let bids: Vec<(u64, u64)> = snapshot.bids.iter().map(|l| (l.price, l.quantity)).collect();
                let asks: Vec<(u64, u64)> = snapshot.asks.iter().map(|l| (l.price, l.quantity)).collect();
                let book = builder.get_or_create(market_id);
                book.init_from_snapshot(&bids, &asks, snapshot.sequence, snapshot.timestamp_ms);
                info!(market_id, sequence = snapshot.sequence, "Initialized L2Book from snapshot");
            }
            Err(e) => {
                warn!(market_id, "Failed to get initial snapshot: {:#}, will wait for deltas", e);
            }
        }
    }

    // Subscribe to gRPC stream
    let request = SubscribeRequest {
        market_ids: config.market_ids.clone(),
    };
    let response = client.subscribe(request).await?;
    let mut stream = response.into_inner();
    info!("Subscribed to DexStreaming gRPC");

    // BBO tracking for change detection
    let mut prev_bbos: HashMap<u32, (Option<(u64, u64)>, Option<(u64, u64)>)> = HashMap::new();

    // Flush interval
    let mut flush_interval = tokio::time::interval(config.flush_interval);

    loop {
        tokio::select! {
            message = stream.next() => {
                match message {
                    Some(Ok(batch)) => {
                        let stale_markets = builder.process_batch(&batch);

                        // Recover stale markets
                        for market_id in stale_markets {
                            match grpc_client::get_snapshot(&mut client, market_id).await {
                                Ok(snapshot) => {
                                    let bids: Vec<(u64, u64)> = snapshot.bids.iter().map(|l| (l.price, l.quantity)).collect();
                                    let asks: Vec<(u64, u64)> = snapshot.asks.iter().map(|l| (l.price, l.quantity)).collect();
                                    let book = builder.get_or_create(market_id);
                                    book.init_from_snapshot(&bids, &asks, snapshot.sequence, snapshot.timestamp_ms);
                                    info!(market_id, sequence = snapshot.sequence, "Recovered L2Book from snapshot");
                                }
                                Err(e) => {
                                    error!(market_id, "Snapshot recovery failed: {:#}", e);
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        error!("gRPC stream error: {}", e);
                        return Err(e.into());
                    }
                    None => {
                        warn!("gRPC stream ended");
                        return Err(anyhow::anyhow!("gRPC stream closed"));
                    }
                }
            }
            _ = flush_interval.tick() => {
                let dirty: Vec<u32> = builder.dirty_books().iter().map(|b| b.perpetual_id).collect();
                for perpetual_id in dirty {
                    if let Some(book) = builder.get_book_mut(perpetual_id) {
                        let prev_bbo = prev_bbos.get(&perpetual_id).copied();
                        if let Err(e) = redis_writer.flush_book(book, prev_bbo).await {
                            error!(perpetual_id, "Redis flush failed: {:#}", e);
                            continue;
                        }
                        // Track BBO for next comparison
                        prev_bbos.insert(perpetual_id, (book.best_bid(), book.best_ask()));
                        book.dirty = false;
                    }
                }
            }
        }
    }
}
```

**Step 3: 更新 lib.rs**

```rust
pub mod config;
pub mod grpc_client;
pub mod orderbook_builder;
pub mod redis_writer;
```

**Step 4: 编译验证**

Run: `cargo check -p dex-stream-indexer`
Expected: 编译通过

**Step 5: Commit**

```bash
git add crates/dex-stream-indexer/
git commit -m "feat(dex): implement dex-stream-indexer main loop with gRPC + Redis flush"
```

---

## Task 5: 全量编译 + Clippy

**Step 1: 全量编译**

Run: `cargo check -p dex-stream-indexer`
Expected: 编译通过

**Step 2: Clippy**

Run: `cargo clippy -p dex-stream-indexer`
Expected: 无 warning

**Step 3: 修复 clippy 问题（如有）**

**Step 4: Commit（如有修复）**

```bash
git add crates/dex-stream-indexer/
git commit -m "chore: fix clippy warnings in dex-stream-indexer"
```

---

## 验证标准

| 指标 | 标准 |
|------|------|
| 编译通过 | `cargo check -p dex-stream-indexer` 无错误 |
| Clippy 通过 | 无 warning |
| CLI 可运行 | `cargo run -p dex-stream-indexer -- --help` 显示用法 |
| 二进制可构建 | `cargo build -p dex-stream-indexer` 生成 `target/debug/dex-stream-indexer` |
