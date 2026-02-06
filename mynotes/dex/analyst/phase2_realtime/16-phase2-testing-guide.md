# Phase 2 实时通道测试指南

本文档提供 Phase 2（实时通道）的完整测试方法，涵盖从环境搭建到性能验证的全流程。

## 1. 测试环境搭建

### 1.1 基础设施

| 组件 | 容器名/进程 | 端口 | 说明 |
|------|------------|------|------|
| PostgreSQL | dex-indexer-db | 5432 | 索引数据存储 |
| Redis | dex-indexer-redis | 6379 | Stream + 聚合数据 |
| Sui 节点 | - | 9001 (RPC) / 9123 (Faucet) | 本地链 |
| dex-indexer | - | - | 事件索引 + Redis 发布 + 聚合 |
| dex-api | - | 3000 (HTTP + WS) | REST API + WebSocket（同端口） |

> **架构变更**：WebSocket 已合并回 dex-api，同一端口 9100 同时提供 REST (`POST /info`) 和 WebSocket (`GET /ws`)。启动时带 `--redis-url` 即自动启用 WebSocket。

```bash
# 启动 PostgreSQL + Redis
cd dex-sui/docker/dex-indexer
sudo docker compose up -d

# 编译所有组件
cargo build -p sui -p dex-indexer -p dex-api -p dex-node-test
```

### 1.2 服务启动顺序

**终端 1：Sui 节点**

```bash
export SUI_CHAIN_DIR="$PWD/local-network-config"
sudo RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9001
```

**终端 2：dex-indexer（带 Redis）**

```bash
RUST_LOG=dex_indexer=info ./target/debug/dex-indexer \
    --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
    --rpc-api-url http://127.0.0.1:9001 \
    --redis-url redis://localhost:6379 \
    --first-checkpoint 0
```

**终端 3：dex-api（REST + WebSocket）**

```bash
./target/debug/dex-api \
    --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
    --api-listen-address 0.0.0.0:9100 \
    --redis-url redis://localhost:6379
```

> 带 `--redis-url` 启动时，dex-api 自动在同一端口提供 WebSocket (`/ws`) 端点。

### 1.3 环境验证

```bash
# 检查 PostgreSQL
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT 1;"

# 检查 Redis
sudo docker exec dex-indexer-redis redis-cli PING
# 预期：PONG

# 检查 dex-api 健康
curl -s http://127.0.0.1:9100/health
# 预期：OK

# 检查 WebSocket（同端口 9100）
websocat ws://localhost:9100/ws
# 预期：连接成功，可发送消息
```

---

## 2. 核心链路测试（P0）

### 2.1 Redis Stream 发布验证

**目标**：验证 dex-indexer 将链上事件正确发布到 5 条 Redis Stream。

**步骤**：

1. 确保 dex-indexer 启动时带 `--redis-url` 参数
2. 执行一笔交易触发事件：
   ```bash
   cargo run -p dex-node-test --example fill_with_api_verify -- \
       --fullnode-url http://127.0.0.1:9001 \
       --api-url http://127.0.0.1:9100
   ```
3. 验证 Redis Stream 数据：
   ```bash
   # 查看各 Stream 是否有数据
   sudo docker exec dex-indexer-redis redis-cli XLEN dex:stream:fills
   sudo docker exec dex-indexer-redis redis-cli XLEN dex:stream:positions
   sudo docker exec dex-indexer-redis redis-cli XLEN dex:stream:balances
   sudo docker exec dex-indexer-redis redis-cli XLEN dex:stream:transfers
   sudo docker exec dex-indexer-redis redis-cli XLEN dex:stream:orders

   # 查看消息内容
   sudo docker exec dex-indexer-redis redis-cli XRANGE dex:stream:fills - + COUNT 3
   ```

**预期输出**：

- `dex:stream:fills` 应至少有 1 条消息
- `dex:stream:balances` 应至少有 2 条消息（买卖双方各一次 deposit）
- `dex:stream:orders` 应有对应的下单消息（如果交易包含 OrderPlacedEventV1）
- 消息 `data` 字段为 JSON 格式，包含正确的事件数据

**消息格式参考**：

```
fills:     { "perpetual_id": 0, "taker_account_address": "0x...", "price": 50000, "quantity": 100, ... }
positions: { "account_address": "0x...", "perpetual_id": 0, "size": 100, ... }
balances:  { "account_address": "0x...", "delta": "5000000000", "new_balance": "5000000000", ... }
transfers: { "from_account_address": "0x...", "to_account_address": "0x...", "amount": "100", ... }
orders:    { "perpetual_id": 0, "order_id": "ab12...", "account_address": "0x...", "side": 0, "price": 50000, ... }
```

### 2.2 WebSocket 实时推送验证

**目标**：验证 dex-api 将 Redis Stream 消息通过 WebSocket 推送给订阅客户端。

**步骤**：

1. 连接 WebSocket（端口 9100，与 REST 同端口）：
   ```bash
   websocat ws://localhost:9100/ws
   ```

2. 订阅事件类型：
   ```json
   {"method":"subscribe","subscription":{"type":"fills"}}
   ```
   预期收到确认：
   ```json
   {"type":"subscriptionResponse","data":{"method":"subscribe","type":"fills","success":true}}
   ```

3. 在另一终端执行交易：
   ```bash
   cargo run -p dex-node-test --example fill_with_api_verify -- \
       --fullnode-url http://127.0.0.1:9001 --api-url http://127.0.0.1:9100
   ```

4. 观察 WebSocket 是否收到推送消息

**预期**：交易完成后 WebSocket 客户端收到 fills 推送，延迟 < 1s。

5. 测试取消订阅：
   ```json
   {"method":"unsubscribe","subscription":{"type":"fills"}}
   ```
   再次执行交易，确认不再收到推送。

6. 测试频道订阅：
   ```json
   {"method":"subscribeChannel","subscription":{"channel":"trades:0"}}
   ```
   预期收到：
   ```json
   {"type":"channelResponse","data":{"method":"subscribeChannel","channel":"trades:0","success":true}}
   ```

7. 测试订单更新频道订阅：
   ```json
   {"method":"subscribeChannel","subscription":{"channel":"orderUpdates:0x<USER_ADDRESS>"}}
   ```
   预期：下单时收到订单事件推送。

### 2.3 Orders 事件全链路

**目标**：验证 OrderPlacedEventV1 从链上到 DB → Redis Stream → API 的完整链路。

**步骤**：

1. 执行订单生命周期测试：
   ```bash
   cargo run -p dex-node-test --example order_lifecycle_verify -- \
       --fullnode-url http://127.0.0.1:9001 --api-url http://127.0.0.1:9100
   ```

2. 验证 Redis Stream：
   ```bash
   sudo docker exec dex-indexer-redis redis-cli XRANGE dex:stream:orders - + COUNT 5
   ```

3. 验证数据库：
   ```bash
   sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c \
       "SELECT order_id, perpetual_id, side, price, quantity, status FROM dex_orders ORDER BY timestamp_ms DESC LIMIT 5;"
   ```

4. 验证 API（openOrders 端点）：
   ```bash
   curl -s http://127.0.0.1:9100/info -X POST \
       -H "Content-Type: application/json" \
       -d '{"type": "openOrders", "user": "<USER_ADDRESS>"}' | jq
   ```

**预期**：
- 下单后 `dex_orders` 表有 `status=0`（Open）记录
- Redis `dex:stream:orders` 有对应消息
- API `openOrders` 返回正确的订单列表

> **注意**：当前仅实现 OrderPlacedEventV1 处理（append-only），OrderRemovedEventV1 的 status 更新逻辑待后续实现。

### 2.4 订单簿快照链路（待 Orderbook Handler 实现后）

**目标**：验证 OrderbookSnapshotEvent 到 Redis Hash 到 API l2Book 端点的链路。

**步骤**：

1. 执行多笔不同价格的下单操作
2. 验证 Redis Hash：
   ```bash
   # 查看完整订单簿快照
   sudo docker exec dex-indexer-redis redis-cli HGETALL dex:orderbook:0

   # 分别查看 bids 和 asks
   sudo docker exec dex-indexer-redis redis-cli HGET dex:orderbook:0 bids
   sudo docker exec dex-indexer-redis redis-cli HGET dex:orderbook:0 asks
   ```

3. 通过 REST API 查询 L2 订单簿：
   ```bash
   curl -s http://127.0.0.1:9100/info -X POST \
       -H "Content-Type: application/json" \
       -d '{"type": "l2Book", "perpetualId": 0, "depth": 10}' | jq
   ```

4. 通过 WebSocket 订阅 l2Book 频道（端口 9100）：
   ```json
   {"method":"subscribeChannel","subscription":{"channel":"orderbook:0"}}
   ```

5. 执行交易，验证收到订单簿更新推送

**预期**：
- Redis Hash `dex:orderbook:{perpetual_id}` 包含正确的 bids/asks
- API `l2Book` 返回正确的价格层级
- WebSocket 推送订单簿快照数据

### 2.5 K 线聚合链路

**目标**：验证多笔成交 → K 线聚合 → Redis Hash/Sorted Set → API candleSnapshot。

**步骤**：

1. 执行多笔不同价格的成交交易
2. 验证当前 K 线（Redis Hash）：
   ```bash
   # 查看 1 分钟 K 线（当前周期）
   sudo docker exec dex-indexer-redis redis-cli HGETALL dex:candle:0:1m

   # 查看所有 K 线 key
   sudo docker exec dex-indexer-redis redis-cli KEYS "dex:candle:*"
   ```

3. 验证历史 K 线（Redis Sorted Set）：
   ```bash
   # 查看历史 K 线条数
   sudo docker exec dex-indexer-redis redis-cli ZCARD dex:candles:0:1m

   # 查看最近的历史 K 线
   sudo docker exec dex-indexer-redis redis-cli ZREVRANGE dex:candles:0:1m 0 4
   ```

4. 通过 REST API 查询 K 线快照：
   ```bash
   curl -s http://127.0.0.1:9100/info -X POST \
       -H "Content-Type: application/json" \
       -d '{"type": "candleSnapshot", "perpetualId": 0, "interval": "1m", "limit": 10}' | jq
   ```

5. 通过 WebSocket 订阅 candle 频道（端口 9100）：
   ```json
   {"method":"subscribeChannel","subscription":{"channel":"candle:0:1m"}}
   ```

**预期**：
- 当前 K 线 Hash 包含正确的 open/high/low/close/volume/num_trades
- 多周期 K 线同时更新（1m, 5m, 15m, 1h, 4h, 1d）
- API `candleSnapshot` 返回正确的历史 K 线 + 当前未结 K 线
- 新成交后 K 线数据实时更新

### 2.6 市场统计链路

**目标**：验证成交 → 市场统计聚合 → Redis Hash → API marketStats。

**步骤**：

1. 执行若干成交交易
2. 验证 Redis Hash：
   ```bash
   # 查看市场统计
   sudo docker exec dex-indexer-redis redis-cli HGETALL dex:market:0

   # 查看最近交易条数
   sudo docker exec dex-indexer-redis redis-cli ZCARD dex:trades:0
   ```

3. 通过 REST API 查询市场统计：
   ```bash
   # 查询单个市场
   curl -s http://127.0.0.1:9100/info -X POST \
       -H "Content-Type: application/json" \
       -d '{"type": "marketStats", "perpetualId": 0}' | jq

   # 查询所有市场
   curl -s http://127.0.0.1:9100/info -X POST \
       -H "Content-Type: application/json" \
       -d '{"type": "marketStats"}' | jq
   ```

**预期**：
- Redis Hash `dex:market:{perpetual_id}` 包含 last_price、volume_24h、high_24h、low_24h、num_trades_24h
- API `marketStats` 返回正确的市场统计数据
- 24h 滚动窗口数据正确（过期交易自动清理）

---

## 3. 数据一致性测试

### 3.1 DB vs Redis 数据对比

**方法**：执行交易后，分别从 DB 和 Redis 查询数据，对比字段值。

```bash
# 从 DB 查询最新成交
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c \
    "SELECT perpetual_id, price, quantity, timestamp_ms FROM dex_fills ORDER BY timestamp_ms DESC LIMIT 1;"

# 从 Redis 查询最新成交
sudo docker exec dex-indexer-redis redis-cli XREVRANGE dex:stream:fills + - COUNT 1
```

**对比项**：perpetual_id、price、quantity、timestamp_ms 应完全一致。

### 3.2 链上事件 vs API 数据对比

使用 `fill_with_api_verify` 等示例自动完成此验证：

```bash
cargo run -p dex-node-test --example fill_with_api_verify -- \
    --fullnode-url http://127.0.0.1:9001 --api-url http://127.0.0.1:9100
```

输出会对比链上 FillEvent 和 API 返回的 price/quantity。

### 3.3 Redis Stream vs WebSocket 推送对比

**方法**：

1. 打开 websocat 连接并订阅 fills（`ws://localhost:9100/ws`）
2. 执行交易
3. 对比 WebSocket 收到的推送数据和 Redis Stream 中的消息

两者的 `data` 字段应包含相同的 JSON 内容。

### 3.4 聚合数据一致性

**方法**：验证 K 线和市场统计数据与原始成交数据的一致性。

```bash
# 从 DB 查询最近 1 分钟的成交汇总
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c \
    "SELECT min(price) as low, max(price) as high, sum(quantity) as volume, count(*) as trades
     FROM dex_fills
     WHERE perpetual_id = 0 AND timestamp_ms >= (extract(epoch from now()) * 1000 - 60000)::bigint;"

# 从 Redis 读取当前 1m K 线
sudo docker exec dex-indexer-redis redis-cli HGETALL dex:candle:0:1m
```

**对比项**：high、low、volume、num_trades 应一致（注意 K 线 open/close 取首/末成交价）。

---

## 4. 故障恢复测试（P1）

### 4.1 dex-indexer 重启恢复

**步骤**：

1. 启动全部服务，执行若干交易
2. 记录当前 watermarks 值：
   ```bash
   sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "SELECT * FROM watermarks;"
   ```
3. 停止 dex-indexer（Ctrl+C）
4. 在 dex-indexer 停止期间执行更多交易
5. 重新启动 dex-indexer（不带 `--first-checkpoint`，让它从 watermarks 恢复）
6. 等待 indexer 处理完成
7. 验证所有交易都被正确索引

**预期**：dex-indexer 从上次 checkpoint 恢复，不丢失事件。

### 4.2 Redis 断连重连

**步骤**：

1. 启动全部服务
2. 停止 Redis：`sudo docker compose stop redis`
3. 观察 dex-indexer、dex-api 的日志（应有 Redis 连接错误警告）
4. 重启 Redis：`sudo docker compose start redis`
5. 执行交易，验证 Redis Stream 数据恢复正常

**预期**：Redis 重启后 dex-indexer 自动重连，新事件正常发布到 Stream。dex-api 的 WebSocket 消费者自动恢复。

### 4.3 WebSocket 客户端重连

**步骤**：

1. websocat 连接 dex-api（端口 9100）并订阅 fills
2. 断开 websocat（Ctrl+C）
3. 等待 10 秒
4. 重新连接并重新订阅
5. 执行交易，验证收到推送

**预期**：重新订阅后立即收到新事件推送。

> **注意**：当前设计不支持断线期间消息重放。客户端重连后只接收新消息。如需历史数据，应通过 REST API 查询。

---

## 5. 性能测试（P2）

### 5.1 端到端延迟测量方法

**目标**：链上交易确认 → WebSocket 推送延迟 < 500ms。

**方法**：

1. 在 `ws_realtime_verify` 示例中已内置延迟测量
2. 记录交易提交时间（client 端）
3. 记录 WebSocket 收到推送时间
4. 计算差值

```bash
cargo run -p dex-node-test --example ws_realtime_verify -- \
    --fullnode-url http://127.0.0.1:9001 --ws-url ws://127.0.0.1:9100/ws
```

**延迟分解**：
- 交易执行 → checkpoint 确认：~200ms（Sui checkpoint 最小间隔）
- checkpoint → dex-indexer 处理：~50ms
- dex-indexer → Redis XADD：~1ms
- Redis → dex-api 消费：~50ms（XREAD block 间隔）
- dex-api → WebSocket 推送：~1ms

### 5.2 吞吐量测试方法

**目标**：
- dex-indexer 处理 1000 事件/秒
- WebSocket 支持 1000 并发连接

**事件处理吞吐量**：

观察 dex-indexer 日志中 checkpoint 处理速度：
```bash
RUST_LOG=dex_indexer=debug,sui_indexer_alt_framework::pipeline::logging=info \
    ./target/debug/dex-indexer \
    --database-url postgres://dex:dex123@localhost:5432/dex_indexer \
    --rpc-api-url http://127.0.0.1:9001 \
    --redis-url redis://localhost:6379 \
    --first-checkpoint 0
```

**WebSocket 并发连接测试**（连接 dex-api 端口 9100）：

```bash
# 简单并发测试（10 个连接）
for i in $(seq 1 10); do
    websocat ws://localhost:9100/ws &
done
```

---

## 6. 自动化测试

### 6.1 dex-node-test 新增示例

| 示例 | 优先级 | 说明 |
|------|--------|------|
| `order_lifecycle_verify.rs` | P0 | 下单 → 成交/撤单 → 验证 Events + API 订单状态 |
| `ws_realtime_verify.rs` | P0 | WebSocket 订阅 → 交易 → 验证推送到达 + 延迟 |
| `redis_stream_verify.rs` | P0 | 交易 → 读 Redis Stream → 验证消息格式和数据 |
| `aggregation_verify.rs` | P1 | 多笔成交 → 验证 K 线 + 市场统计数据正确性 |

运行命令：

```bash
# 所有 Phase 2 示例（需要 Redis、dex-api）
cargo run -p dex-node-test --example order_lifecycle_verify -- \
    --fullnode-url http://127.0.0.1:9001 --api-url http://127.0.0.1:9100
cargo run -p dex-node-test --example ws_realtime_verify -- \
    --fullnode-url http://127.0.0.1:9001 --ws-url ws://127.0.0.1:9100/ws
cargo run -p dex-node-test --example redis_stream_verify -- \
    --fullnode-url http://127.0.0.1:9001 --api-url http://127.0.0.1:9100
cargo run -p dex-node-test --example aggregation_verify -- \
    --fullnode-url http://127.0.0.1:9001 --api-url http://127.0.0.1:9100
```

### 6.2 E2E 测试新增用例

Phase 2 相关的 E2E 测试建议（在 `dex-indexer-e2e-test/tests/` 中添加）：

| 测试文件 | 说明 |
|----------|------|
| `order_tests.rs` | OrderPlacedEventV1 处理、dex_orders 表验证 |
| `redis_publish_tests.rs` | 5 条 Redis Stream 发布功能验证 |
| `aggregation_tests.rs` | K 线聚合和市场统计数据验证 |

### 6.3 单元测试概览

以下测试已实现并通过：

| Crate | 测试模块 | 数量 | 说明 |
|-------|----------|------|------|
| dex-api | `ws::types` | 10 | 频道解析、消息格式（含 orders） |
| dex-api | `ws::subscription` | 5 | 订阅管理、频道匹配（含 orderUpdates） |
| dex-api | `ws::handler` | 3 | 事件信息提取 |
| dex-indexer | `handlers::*` | 6 | 各 handler 名称验证（含 orders） |
| dex-indexer | `redis::types` | 4 | 消息序列化（含 orders） |
| dex-indexer | `redis::publisher` | 2 | Redis 发布（需 Redis） |
| dex-indexer | `aggregators::candles` | 3 | K 线间隔、序列化、对齐 |
| dex-indexer | `aggregators::market_stats` | 2 | 交易序列化、窗口常量 |
| dex-indexer | `lib` | 5 | subaccount 编解码 |
| dex-api | `cache::keys` | 6 | 缓存键生成 |
| dex-api | `cache::client` | 3 | Redis 客户端（需 Redis） |
| dex-api | `server` | 1 | 默认配置 |
| sui-types | `dex_events` | 16 | 事件序列化、struct_tag 唯一性 |
| dex-types | `common` | 5 | subaccount 工具函数 |

运行所有单元测试：

```bash
cargo test -p dex-api --lib
cargo test -p dex-indexer --lib
cargo test -p sui-types --lib -- dex_events
cargo test -p dex-types --lib
```

---

## 7. REST API 端点测试

### 7.1 已有端点

```bash
# 用户成交记录
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "userFills", "user": "0x<ADDRESS>"}' | jq

# 用户余额更新
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "userBalances", "user": "0x<ADDRESS>"}' | jq

# 用户转账记录
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "userTransfers", "user": "0x<ADDRESS>"}' | jq

# 市场最近成交
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "recentFills", "perpetualId": 0}' | jq

# 用户持仓和保证金
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "clearinghouseState", "user": "0x<ADDRESS>"}' | jq

# 市场元数据
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "meta"}' | jq
```

### 7.2 Phase 2 新增端点

```bash
# 用户挂单（从 PostgreSQL 查询）
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "openOrders", "user": "0x<ADDRESS>"}' | jq

# 带过滤条件的挂单查询
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "openOrders", "user": "0x<ADDRESS>", "perpetualId": 0, "subaccountNumber": 0}' | jq

# L2 订单簿（从 Redis 查询）
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "l2Book", "perpetualId": 0, "depth": 10}' | jq

# K 线快照（从 Redis 查询）
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "candleSnapshot", "perpetualId": 0, "interval": "1m", "limit": 100}' | jq

# 带时间范围的 K 线查询
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "candleSnapshot", "perpetualId": 0, "interval": "1h", "startTime": 1738700000000, "endTime": 1738800000000}' | jq

# 市场统计 - 单个市场（从 Redis 查询）
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "marketStats", "perpetualId": 0}' | jq

# 市场统计 - 所有市场
curl -s http://127.0.0.1:9100/info -X POST \
    -H "Content-Type: application/json" \
    -d '{"type": "marketStats"}' | jq
```

### 7.3 API 响应格式验证

**openOrders 响应**：
```json
[
  {
    "orderId": "0xab12...",
    "perpetualId": 0,
    "accountAddress": "0x...",
    "subaccountNumber": 0,
    "side": 0,
    "price": 50000,
    "quantity": 100,
    "remainingQuantity": 100,
    "orderType": 0,
    "timeInForce": 0,
    "reduceOnly": false,
    "status": 0,
    "timestampMs": 1738746000000
  }
]
```

**candleSnapshot 响应**：
```json
[
  {
    "open": 50000,
    "high": 51000,
    "low": 49500,
    "close": 50500,
    "volume": 1000,
    "numTrades": 42,
    "timestampMs": 1738746000000
  }
]
```

**marketStats 响应**：
```json
[
  {
    "perpetualId": 0,
    "lastPrice": "50500",
    "volume24h": "125000",
    "high24h": "51000",
    "low24h": "49000",
    "numTrades24h": 342
  }
]
```

---

## 8. 常见问题排查

### Redis 连接失败

```
ERROR dex_indexer: Failed to connect to Redis: Connection refused
```

**解决**：检查 Redis 是否运行 `sudo docker exec dex-indexer-redis redis-cli PING`

### WebSocket 连接失败

```
error: Connection failed: Connection refused
```

**解决**：
- 确认 dex-api 已启动（端口 9100）
- 确认启动时带了 `--redis-url` 参数（WebSocket 需要 Redis）

### Redis Stream 无数据

1. 确认 dex-indexer 启动时带 `--redis-url` 参数
2. 检查 dex-indexer 日志是否有 Redis 发布错误
3. 确认有链上事件产生（检查 watermarks 表）

### WebSocket 未收到推送

1. 确认已发送订阅消息并收到确认
2. 确认连接的是 `ws://localhost:9100/ws`
3. 检查 Redis Stream 是否有数据
4. 检查 dex-api 日志是否有 WebSocket 消费者错误

### Redis 缓存端点返回 "Redis not configured"

确认 dex-api 启动时带 `--redis-url` 参数。l2Book、candleSnapshot、marketStats 端点需要 Redis 连接。

### 数据不一致

1. 检查 dex-indexer 是否有事件处理错误
2. 对比 DB 和 Redis 中的 timestamp_ms
3. 确认 checkpoint 已被完整处理

---

## 9. 测试检查清单

### P0 核心验证

- [ ] T1: Redis 基础功能 — dex-indexer 连接 Redis + 5 个 Stream 有数据
- [ ] T2: WebSocket 订阅推送 — dex-api /ws 连接/订阅/推送/取消全链路（端口 9100）
- [ ] T3: Orders 事件链路 — 下单 → DB + Redis Stream + API openOrders
- [ ] T4: 订单簿快照链路 — 快照 → Redis Hash → API l2Book
- [ ] T5: K 线聚合 — 多笔成交 → 多周期 K 线 → Redis → API candleSnapshot
- [ ] T6: 市场统计 — 成交 → 24h 统计 → Redis → API marketStats
- [ ] T7: 订单更新频道 — orderUpdates:0x... WebSocket 推送

### P1 稳定性验证

- [ ] T8: 幂等发布 — 重复事件不产生重复消息
- [ ] T9: 故障恢复 — indexer 重启/Redis 断连/WS 重连
- [ ] T10: 故障恢复 — dex-api 重启后 WebSocket 和 REST 均恢复正常

### P2 性能验证

- [ ] T11: 端到端延迟 — 链上确认到 WS 推送 < 500ms
- [ ] T12: 吞吐量 — 1000 事件/秒处理能力

---

## 10. Redis 数据结构一览

| Key 模式 | 类型 | 说明 | 写入方 | 读取方 |
|----------|------|------|--------|--------|
| `dex:stream:fills` | Stream | 成交事件流 | dex-indexer | dex-api |
| `dex:stream:positions` | Stream | 持仓更新流 | dex-indexer | dex-api |
| `dex:stream:balances` | Stream | 余额更新流 | dex-indexer | dex-api |
| `dex:stream:transfers` | Stream | 转账事件流 | dex-indexer | dex-api |
| `dex:stream:orders` | Stream | 订单事件流 | dex-indexer | dex-api |
| `dex:candle:{id}:{interval}` | Hash | 当前 K 线 | dex-indexer | dex-api |
| `dex:candles:{id}:{interval}` | Sorted Set | 历史 K 线 | dex-indexer | dex-api |
| `dex:market:{id}` | Hash | 市场统计 | dex-indexer | dex-api |
| `dex:trades:{id}` | Sorted Set | 最近交易（24h 窗口） | dex-indexer | dex-indexer |
| `dex:orderbook:{id}` | Hash | 订单簿快照 | 待实现 | dex-api |
