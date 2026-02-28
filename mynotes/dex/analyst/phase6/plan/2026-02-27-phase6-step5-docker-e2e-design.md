# Phase 6 Step 5: Docker 集成 & E2E 验证设计文档

> 创建日期: 2026-02-27
> 状态: 设计确认

## 目标

将 dex-stream-indexer 集成到 Docker 全栈开发环境，启用 sui-node 的 gRPC streaming，并创建自动化 node-test 验证脚本验证完整 pipeline。

## 三部分工作

### 1. `sui start` CLI 增强

**现状：** `NodeConfig.dex_streaming` 默认为 `None`，`sui genesis` 不生成该配置。

**方案：** 新增 `--dex-streaming-address` CLI 参数：

```
sui start --network.config /data/sui \
  --with-faucet=0.0.0.0:9123 \
  --fullnode-rpc-port 9001 \
  --dex-streaming-address 0.0.0.0:50052
```

**实现：**
- `SuiCommand::Start` 新增 `dex_streaming_address: Option<String>` 字段
- `start()` 函数接收该参数
- 在 `swarm_builder.build()` 之前，注入 `DexStreamingConfig` 到所有 validator configs

**受影响文件：**
- `crates/sui/src/sui_commands.rs` — CLI 定义 + 注入逻辑

### 2. Docker 集成

**新增文件：**

| 文件 | 说明 |
|------|------|
| `docker/dex-dev/Dockerfile.dex-stream-indexer` | Rust 服务 Dockerfile |
| `docker/dex-test/Dockerfile.dex-stream-indexer` | 同上（test 环境） |

**修改文件：**

| 文件 | 变更 |
|------|------|
| `docker/dex-dev/docker-compose.yml` | 新增 dex-stream-indexer 服务 |
| `docker/dex-test/docker-compose.yml` | 同上 |
| `docker/dex-dev/Makefile` | 新增 rebuild-streamer/restart-streamer/logs-streamer，更新 build target |
| `docker/dex-test/Makefile` | 同上 |
| `docker/dex-dev/entrypoint-sui-node.sh` | 添加 `--dex-streaming-address` 参数 |
| `docker/dex-test/entrypoint-sui-node.sh` | 同上 |

**dex-stream-indexer 服务定义：**

```yaml
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

**sui-node 变更：**
- entrypoint 添加 `--dex-streaming-address 0.0.0.0:50052`
- 不需要暴露 50052 端口到宿主机（仅 Docker 网络内部访问）

### 3. Node-test 验证脚本

**新增文件：** `crates/dex-node-test/examples/streaming_verify.rs`

**验证流程：**

```
1. 通过 tx-gateway 执行 setup（创建 perpetual）
2. 通过 tx-gateway 下买单 + 卖单（触发成交 + orderbook 变化）
3. 等待 2-5s
4. 检查 Redis dex:l2book:0 是否有数据（HGETALL）
5. 检查 Redis dex:bbo:0 是否有数据（HGETALL）
6. 调用 REST API query l2Book（验证 dual-source fallback 工作）
7. 输出验证结果
```

**CLI 参数：**
```bash
cargo run -p dex-node-test --example streaming_verify -- \
  --fullnode-url http://127.0.0.1:9001 \
  --api-url http://127.0.0.1:9100 \
  --redis-url redis://:dex_redis_dev@localhost:6379 \
  --gateway-url http://127.0.0.1:3200
```

**依赖变更：** `crates/dex-node-test/Cargo.toml` 添加 `redis = { version = "0.24", features = ["aio", "tokio-comp"] }`

## 不变的部分

- dex-stream-indexer 代码本身（Phase 6 Step 3 已完成）
- dex-api 代码本身（Phase 6 Step 4 已完成）
- WS 消息格式和订阅协议

## 验证标准

| 指标 | 标准 |
|------|------|
| sui-node gRPC | `--dex-streaming-address` 参数正确启动 gRPC server |
| dex-stream-indexer Docker | 容器正常启动，连接 gRPC 和 Redis |
| Redis L2 数据 | 下单后 `dex:l2book:0` 有 bid/ask 数据 |
| Redis BBO 数据 | `dex:bbo:0` 有 best_bid/best_ask |
| REST l2Book | API 返回 dex-stream-indexer 写入的数据（非 checkpoint fallback） |
| make targets | rebuild-streamer/restart-streamer/logs-streamer 正常工作 |
