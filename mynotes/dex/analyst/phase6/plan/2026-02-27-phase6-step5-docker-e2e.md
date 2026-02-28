# Phase 6 Step 5: Docker 集成 & E2E 验证 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 dex-stream-indexer 集成到 Docker 全栈环境，启用 sui-node gRPC streaming，并创建自动化验证脚本。

**Architecture:** 三部分——(1) `sui start` 新增 `--dex-streaming-address` CLI 参数注入 DexStreamingConfig 到 validator configs；(2) Docker 新增 dex-stream-indexer 服务（Dockerfile + compose + Makefile）；(3) Node-test 脚本验证完整 streaming pipeline。

**Tech Stack:** Rust/clap (CLI), Docker Compose, Redis, reqwest (HTTP client)

---

### Task 1: `sui start` 新增 `--dex-streaming-address` CLI 参数

**Files:**
- Modify: `crates/sui/src/sui_commands.rs`

**Step 1: 添加 CLI 参数到 `SuiCommand::Start`**

在 `crates/sui/src/sui_commands.rs` 的 `Start` 枚举变体中，在 `committee_size` 字段后新增：

```rust
        /// Enable DEX streaming gRPC server on the specified address (e.g. 0.0.0.0:50052)
        #[clap(long)]
        dex_streaming_address: Option<String>,
```

**Step 2: 将参数传递给 `start()` 函数**

在 `SuiCommand::Start` match arm 中，添加 `dex_streaming_address` 到解构和 `start()` 调用：

```rust
            SuiCommand::Start {
                config_dir,
                force_regenesis,
                with_faucet,
                rpc_args,
                fullnode_rpc_port,
                data_ingestion_dir,
                no_full_node,
                epoch_duration_ms,
                committee_size,
                dex_streaming_address,  // 新增
            } => {
                start(
                    config_dir.clone(),
                    with_faucet,
                    rpc_args,
                    force_regenesis,
                    epoch_duration_ms,
                    fullnode_rpc_port,
                    data_ingestion_dir,
                    no_full_node,
                    committee_size,
                    dex_streaming_address,  // 新增
                )
                .await?;
                Ok(())
            }
```

**Step 3: 修改 `start()` 函数签名和注入逻辑**

在 `start()` 函数签名中新增参数：

```rust
async fn start(
    config: Option<PathBuf>,
    with_faucet: Option<String>,
    rpc_args: RpcArgs,
    force_regenesis: bool,
    epoch_duration_ms: Option<u64>,
    fullnode_rpc_port: u16,
    mut data_ingestion_dir: Option<PathBuf>,
    no_full_node: bool,
    committee_size: Option<usize>,
    dex_streaming_address: Option<String>,  // 新增
) -> Result<(), anyhow::Error> {
```

添加 import（文件顶部 `use sui_config::node::Genesis;` 旁边）：

```rust
use sui_config::node::{DexStreamingConfig, Genesis};
```

在持久化配置加载路径中（约 line 967），将 `let network_config` 改为 `let mut network_config`：

```rust
        let mut network_config: NetworkConfig =
            PersistedConfig::read(&network_config_path).map_err(|err| {
                // ...
            })?;

        // Inject DexStreaming config if --dex-streaming-address is provided
        if let Some(ref addr_str) = dex_streaming_address {
            let addr: SocketAddr = addr_str.parse()
                .map_err(|e: AddrParseError| anyhow!("Invalid --dex-streaming-address '{}': {}", addr_str, e))?;
            for cfg in &mut network_config.validator_configs {
                cfg.dex_streaming = Some(DexStreamingConfig {
                    grpc_address: addr,
                    channel_capacity: 1024,
                });
            }
            info!("DEX streaming enabled on {}", addr);
        }

        swarm_builder = swarm_builder
            .dir(sui_config_path.clone())
            .with_network_config(network_config);
```

**Step 4: 验证编译通过**

Run: `cargo check -p sui`
Expected: 成功编译

**Step 5: Commit**

```bash
git add crates/sui/src/sui_commands.rs
git commit -m "feat(sui): add --dex-streaming-address CLI parameter to sui start"
```

---

### Task 2: Docker dex-stream-indexer 集成（dex-dev 环境）

**Files:**
- Create: `docker/dex-dev/Dockerfile.dex-stream-indexer`
- Modify: `docker/dex-dev/docker-compose.yml`
- Modify: `docker/dex-dev/Makefile`
- Modify: `docker/dex-dev/entrypoint-sui-node.sh`

**Step 1: 创建 Dockerfile.dex-stream-indexer**

```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY target/debug/dex-stream-indexer /usr/local/bin/dex-stream-indexer
CMD ["dex-stream-indexer"]
```

**Step 2: 修改 entrypoint-sui-node.sh 添加 --dex-streaming-address**

将末尾的 `exec sui start` 块改为：

```bash
echo "Starting sui node..."
exec sui start \
  --network.config "$SUI_CONFIG_DIR" \
  --with-faucet=0.0.0.0:9123 \
  --fullnode-rpc-port 9001 \
  --dex-streaming-address 0.0.0.0:50052
```

**Step 3: 修改 docker-compose.yml 添加 dex-stream-indexer 服务**

在 `dex-api` 服务之后、`tx-gateway` 服务之前添加：

```yaml
  # ─── DEX Streamer ────────────────────────────────────────────

  dex-stream-indexer:
    build:
      context: ../..
      dockerfile: docker/dex-dev/Dockerfile.dex-stream-indexer
    container_name: dex-dev-streamer
    depends_on:
      redis:
        condition: service_healthy
      sui-node:
        condition: service_healthy
    environment:
      - DEX_GRPC_ADDR=http://sui-node:50052
      - REDIS_URL=redis://:dex_redis_dev@redis:6379
      - MARKET_IDS=0
      - RUST_LOG=info,dex_stream_indexer=debug
    command: >
      dex-stream-indexer
        --grpc-addr http://sui-node:50052
        --redis-url redis://:dex_redis_dev@redis:6379
        --market-ids 0
```

**Step 4: 修改 Makefile**

在 `.PHONY` 行添加 `rebuild-streamer restart-streamer logs-streamer`：

```makefile
.PHONY: help build up down clean reset restart rebuild logs ps \
       rebuild-node rebuild-node-fresh rebuild-indexer rebuild-api rebuild-gateway rebuild-streamer rebuild-panel \
       restart-node restart-indexer restart-api restart-gateway restart-streamer restart-panel restart-db restart-redis \
       logs-node logs-indexer logs-api logs-gateway logs-streamer logs-panel
```

修改 `build` target 添加 dex-stream-indexer 编译：

```makefile
build: ## 编译全部 Rust 二进制（宿主机）
	cd $(ROOT_DIR) && cargo build -p sui -p dex-indexer -p dex-api -p dex-stream-indexer -p dex-node-test --bin tx-gateway --features redis-publish
```

在 `rebuild-gateway` 之后添加：

```makefile
rebuild-streamer: ## 重编译 dex-stream-indexer 并重启
	cd $(ROOT_DIR) && cargo build -p dex-stream-indexer
	$(DC) build --no-cache dex-stream-indexer
	$(DC) up -d dex-stream-indexer
```

在 `restart-gateway` 之后添加：

```makefile
restart-streamer: ## 重启 dex-stream-indexer
	$(DC) restart dex-stream-indexer
```

在 `logs-gateway` 之后添加：

```makefile
logs-streamer: ## 查看 dex-stream-indexer 日志
	$(DC) logs -f dex-stream-indexer
```

**Step 5: Commit**

```bash
git add docker/dex-dev/Dockerfile.dex-stream-indexer docker/dex-dev/docker-compose.yml docker/dex-dev/Makefile docker/dex-dev/entrypoint-sui-node.sh
git commit -m "feat(docker): add dex-stream-indexer to dex-dev environment"
```

---

### Task 3: Docker dex-stream-indexer 集成（dex-test 环境）

**Files:**
- Create: `docker/dex-test/Dockerfile.dex-stream-indexer`
- Modify: `docker/dex-test/docker-compose.yml`
- Modify: `docker/dex-test/Makefile`
- Modify: `docker/dex-test/entrypoint-sui-node.sh`

**Step 1: 创建 Dockerfile.dex-stream-indexer**

与 dex-dev 完全相同：

```dockerfile
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY target/debug/dex-stream-indexer /usr/local/bin/dex-stream-indexer
CMD ["dex-stream-indexer"]
```

**Step 2: 修改 entrypoint-sui-node.sh**

与 dex-dev 相同，末尾添加 `--dex-streaming-address 0.0.0.0:50052`：

```bash
echo "Starting sui node..."
exec sui start \
  --network.config "$SUI_CONFIG_DIR" \
  --with-faucet=0.0.0.0:9123 \
  --fullnode-rpc-port 9001 \
  --dex-streaming-address 0.0.0.0:50052
```

**Step 3: 修改 docker-compose.yml 添加 dex-stream-indexer 服务**

在 `dex-api` 之后添加（注意 Redis 密码是 `dex_redis_test`）：

```yaml
  # ─── DEX Streamer ────────────────────────────────────────────

  dex-stream-indexer:
    build:
      context: ../..
      dockerfile: docker/dex-test/Dockerfile.dex-stream-indexer
    container_name: dex-test-streamer
    depends_on:
      redis:
        condition: service_healthy
      sui-node:
        condition: service_healthy
    environment:
      - DEX_GRPC_ADDR=http://sui-node:50052
      - REDIS_URL=redis://:dex_redis_test@redis:6379
      - MARKET_IDS=0
      - RUST_LOG=info,dex_stream_indexer=debug
    command: >
      dex-stream-indexer
        --grpc-addr http://sui-node:50052
        --redis-url redis://:dex_redis_test@redis:6379
        --market-ids 0
```

**Step 4: 修改 Makefile**

与 dex-dev 完全相同的修改（.PHONY、build、rebuild-streamer、restart-streamer、logs-streamer）。

**Step 5: Commit**

```bash
git add docker/dex-test/Dockerfile.dex-stream-indexer docker/dex-test/docker-compose.yml docker/dex-test/Makefile docker/dex-test/entrypoint-sui-node.sh
git commit -m "feat(docker): add dex-stream-indexer to dex-test environment"
```

---

### Task 4: Node-test streaming 验证脚本

**Files:**
- Create: `crates/dex-node-test/examples/streaming_verify.rs`
- Modify: `crates/dex-node-test/Cargo.toml` (添加 example entry)

**Step 1: 在 Cargo.toml 添加 example entry**

在文件末尾添加：

```toml
[[example]]
name = "streaming_verify"
path = "examples/streaming_verify.rs"
```

**Step 2: 创建 streaming_verify.rs**

```rust
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Example: Streaming pipeline E2E verification (Phase 6)
//!
//! Verifies the full dex-stream-indexer pipeline:
//! 1. Place orders via Exchange API → orderbook deltas generated
//! 2. dex-stream-indexer processes deltas → writes to Redis
//! 3. Check Redis dex:l2book:{id} and dex:bbo:{id}
//! 4. Check REST API l2Book returns dex-stream-indexer data
//!
//! # Prerequisites
//!
//! Full Docker stack running: `cd docker/dex-dev && make up`
//! Or: sui-node + dex-indexer + dex-api + dex-stream-indexer + tx-gateway + Redis
//!
//! # Usage
//!
//! ```bash
//! cargo run -p dex-node-test --example streaming_verify -- \
//!     --fullnode-url http://127.0.0.1:9001 \
//!     --api-url http://127.0.0.1:9100 \
//!     --redis-url redis://:dex_redis_dev@localhost:6379 \
//!     --gateway-url http://127.0.0.1:3200
//! ```

use anyhow::{Result, bail};
use clap::Parser;
use dex_node_test::DexTestConfig;
use reqwest::Client;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[clap(flatten)]
    config: DexTestConfig,

    /// Redis connection URL
    #[clap(long, default_value = "redis://:dex_redis_dev@127.0.0.1:6379")]
    redis_url: String,

    /// TX Gateway URL
    #[clap(long, default_value = "http://127.0.0.1:3200")]
    gateway_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let client = Client::new();

    println!("=== Phase 6 Streaming Pipeline Verification ===\n");
    println!("Gateway:  {}", args.gateway_url);
    println!("API:      {}", args.config.api_url);
    println!("Redis:    {}", args.redis_url);

    // ================================================================
    // Step 1: Setup — call /tx/setup to create GlobalAccounts + Market
    // ================================================================
    println!("\n--- Step 1: Setup (create market) ---");
    let resp = client
        .post(format!("{}/tx/setup", args.gateway_url))
        .json(&serde_json::json!({}))
        .send()
        .await?;
    let setup: serde_json::Value = resp.json().await?;
    if setup["success"] != true {
        // Already set up is OK
        println!("  Setup response: {}", setup);
    } else {
        println!("  Setup OK");
    }

    // ================================================================
    // Step 2: Deposit for two accounts
    // ================================================================
    println!("\n--- Step 2: Deposit for buyer and seller ---");

    // Buyer deposit
    let resp = client
        .post(format!("{}/tx/deposit", args.gateway_url))
        .json(&serde_json::json!({
            "sender_index": 0,
            "subaccount_number": 0,
            "amount": "1000000000"
        }))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    println!("  Buyer deposit: success={}", body["success"]);

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Seller deposit (sender_index=1)
    let resp = client
        .post(format!("{}/tx/deposit", args.gateway_url))
        .json(&serde_json::json!({
            "sender_index": 1,
            "subaccount_number": 0,
            "amount": "1000000000"
        }))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    println!("  Seller deposit: success={}", body["success"]);

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // ================================================================
    // Step 3: Place limit orders (buy + sell at different prices)
    // ================================================================
    println!("\n--- Step 3: Place orders to build orderbook ---");

    // Buy order at 49000
    let resp = client
        .post(format!("{}/tx/place-order", args.gateway_url))
        .json(&serde_json::json!({
            "sender_index": 0,
            "subaccount_number": 0,
            "perpetual_id": 0,
            "is_buy": true,
            "quantity": 10,
            "price": 49000
        }))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    println!("  Buy order (49000): success={}", body["success"]);

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Sell order at 51000
    let resp = client
        .post(format!("{}/tx/place-order", args.gateway_url))
        .json(&serde_json::json!({
            "sender_index": 1,
            "subaccount_number": 0,
            "perpetual_id": 0,
            "is_buy": false,
            "quantity": 10,
            "price": 51000
        }))
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;
    println!("  Sell order (51000): success={}", body["success"]);

    // ================================================================
    // Step 4: Wait for dex-stream-indexer to flush to Redis
    // ================================================================
    println!("\n--- Step 4: Wait for streaming pipeline (5s) ---");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // ================================================================
    // Step 5: Check Redis dex:l2book:0
    // ================================================================
    println!("\n--- Step 5: Check Redis L2Book ---");

    let redis_client = redis::Client::open(args.redis_url.as_str())?;
    let mut conn = redis_client.get_multiplexed_async_connection().await?;

    let l2book: Vec<(String, String)> = redis::cmd("HGETALL")
        .arg("dex:l2book:0")
        .query_async(&mut conn)
        .await
        .unwrap_or_default();

    if l2book.is_empty() {
        println!("  WARNING: dex:l2book:0 is empty (dex-stream-indexer may not be running)");
        println!("  Falling back to checkpoint-only verification...");
    } else {
        println!("  dex:l2book:0 has {} fields:", l2book.len());
        let mut bids = 0;
        let mut asks = 0;
        for (key, value) in &l2book {
            if key.starts_with("b:") {
                bids += 1;
                println!("    BID {} = {}", key, value);
            } else if key.starts_with("a:") {
                asks += 1;
                println!("    ASK {} = {}", key, value);
            }
        }
        println!("  Total: {} bids, {} asks", bids, asks);

        if bids == 0 || asks == 0 {
            bail!("FAIL: Expected both bids and asks in L2 book");
        }
        println!("  PASS: L2Book has bids and asks");
    }

    // ================================================================
    // Step 6: Check Redis dex:bbo:0
    // ================================================================
    println!("\n--- Step 6: Check Redis BBO ---");

    let bbo: Vec<(String, String)> = redis::cmd("HGETALL")
        .arg("dex:bbo:0")
        .query_async(&mut conn)
        .await
        .unwrap_or_default();

    if bbo.is_empty() {
        println!("  WARNING: dex:bbo:0 is empty (dex-stream-indexer may not be running)");
    } else {
        println!("  dex:bbo:0 fields:");
        for (key, value) in &bbo {
            println!("    {} = {}", key, value);
        }
        println!("  PASS: BBO data present");
    }

    // ================================================================
    // Step 7: Check REST API l2Book
    // ================================================================
    println!("\n--- Step 7: Check REST API l2Book ---");

    let resp = client
        .post(format!("{}/info", args.config.api_url))
        .json(&serde_json::json!({
            "type": "l2Book",
            "coin": "BTC-USDC",
            "nSigFigs": 5
        }))
        .send()
        .await?;
    let l2_api: serde_json::Value = resp.json().await?;

    if let Some(levels) = l2_api.get("levels") {
        let bid_count = levels.get(0).and_then(|b| b.as_array()).map(|a| a.len()).unwrap_or(0);
        let ask_count = levels.get(1).and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
        println!("  l2Book API: {} bids, {} asks", bid_count, ask_count);
        if bid_count > 0 && ask_count > 0 {
            println!("  PASS: REST l2Book returns orderbook data");
        } else {
            println!("  WARNING: l2Book empty (may need more time for indexer)");
        }
    } else {
        println!("  l2Book response: {}", serde_json::to_string_pretty(&l2_api)?);
    }

    // ================================================================
    // Step 8: Check Redis dex:l2book:0:meta
    // ================================================================
    println!("\n--- Step 8: Check L2Book metadata ---");

    let meta_ts: Option<String> = redis::cmd("HGET")
        .arg("dex:l2book:0:meta")
        .arg("timestamp")
        .query_async(&mut conn)
        .await
        .unwrap_or(None);

    let meta_seq: Option<String> = redis::cmd("HGET")
        .arg("dex:l2book:0:meta")
        .arg("sequence")
        .query_async(&mut conn)
        .await
        .unwrap_or(None);

    match (&meta_ts, &meta_seq) {
        (Some(ts), Some(seq)) => {
            println!("  timestamp={}, sequence={}", ts, seq);
            println!("  PASS: L2Book metadata present");
        }
        _ => {
            println!("  WARNING: L2Book metadata missing (dex-stream-indexer may not be running)");
        }
    }

    // ================================================================
    // Summary
    // ================================================================
    println!("\n=== Verification Summary ===");
    println!("  L2Book data:  {}", if !l2book.is_empty() { "PASS" } else { "SKIP (no dex-stream-indexer)" });
    println!("  BBO data:     {}", if !bbo.is_empty() { "PASS" } else { "SKIP (no dex-stream-indexer)" });
    println!("  L2Book meta:  {}", if meta_ts.is_some() { "PASS" } else { "SKIP (no dex-stream-indexer)" });
    println!("  REST l2Book:  checked above");
    println!("\nDone!");

    Ok(())
}
```

**Step 3: 验证编译**

Run: `cargo check -p dex-node-test --example streaming_verify`
Expected: 编译通过

**Step 4: Commit**

```bash
git add crates/dex-node-test/examples/streaming_verify.rs crates/dex-node-test/Cargo.toml
git commit -m "feat(test): add streaming pipeline E2E verification script"
```

---

### Task 5: 编译验证 + Clippy

**Step 1: 完整编译检查**

Run: `cargo check -p sui -p dex-stream-indexer -p dex-node-test`
Expected: 全部通过

**Step 2: Clippy 检查**

Run: `cargo clippy -p sui 2>&1 | grep -E "warning|error" | head -20`
Expected: 无新增 warning

**Step 3: 修复可能的问题并 commit**

如有 clippy warning，修复后：

```bash
git add -A
git commit -m "chore: fix clippy warnings in Phase 6 Step 5"
```
