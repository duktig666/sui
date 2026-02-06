# Phase 2 实施清单

## 概述

本文档列出 Phase 2（dex-indexer 增强 + dex-ws）实施过程中需要修改的文件清单、具体实现任务和测试验证清单。

> **重要更新（2026-02-05）**：基于时序验证结论，Phase 2 采用 **单通道架构**（Checkpoint 主通道），移除原计划的 dex-realtime 模块。dex-indexer 增强 Redis 写入功能，直接向 Redis 发布事件。详见 `04-design-decisions.md` §10。

---

## 1. 需要修改的文件清单

### 1.1 事件定义层

| 文件 | 修改类型 | 说明 |
|------|----------|------|
| sui-types/src/dex_events.rs | 新增 + 修改 | 添加 OrderPlacedEventV1, OrderRemovedEventV1；现有事件添加 V1 后缀 |
| sui-types/src/lib.rs | 修改 | 导出新增事件类型 |

### 1.2 执行层

| 文件 | 修改类型 | 说明 |
|------|----------|------|
| sui-execution/src/dex.rs | 修改 | 在订单生命周期各阶段发射新增事件 |
| sui-execution/src/lib.rs | 可能修改 | 导出相关类型 |

### 1.3 索引层（dex-indexer）

| 文件 | 修改类型 | 说明 |
|------|----------|------|
| dex-indexer/src/handlers/mod.rs | 修改 | 导出 Orders Handler |
| dex-indexer/src/handlers/orders.rs | 新增 | 处理 OrderPlaced/Removed 事件 |
| dex-indexer/src/handlers/fills.rs | 修改 | 事件类型名更新为 FillEventV1 |
| dex-indexer/src/handlers/positions.rs | 修改 | 事件类型名更新 |
| dex-indexer/src/handlers/transfers.rs | 修改 | 事件类型名更新 |
| dex-indexer/src/schema/mod.rs | 修改 | 添加 dex_orders 表定义 |
| dex-indexer/migrations/YYYYMMDDHHMMSS_create_orders_table/up.sql | 新增 | 订单表迁移 |
| dex-indexer/migrations/YYYYMMDDHHMMSS_create_orders_table/down.sql | 新增 | 回滚迁移 |
| dex-indexer/src/main.rs | 修改 | 注册 Orders Handler |

### 1.4 dex-indexer Redis 发布层（增强现有 crate）

> **重要更新（2026-02-05）**：采用单通道架构，**移除 dex-realtime 模块**。Redis 发布功能集成到 dex-indexer 中。

| 文件 | 类型 | 说明 |
|------|------|------|
| dex-indexer/src/redis_publisher.rs | 新增 | Redis 发布核心模块 |
| dex-indexer/src/redis_config.rs | 新增 | Redis 连接配置 |
| dex-indexer/src/candles.rs | 新增 | K 线聚合计算 |
| dex-indexer/src/market_stats.rs | 新增 | 市场统计计算 |
| dex-indexer/src/handlers/fills.rs | 修改 | 增加 Redis Stream + Hash 写入 |
| dex-indexer/src/handlers/orders.rs | 修改 | 增加 Redis Stream 写入 |
| dex-indexer/src/handlers/positions.rs | 修改 | 增加 Redis Stream + Hash 写入 |
| dex-indexer/src/handlers/orderbook.rs | 新增 | OrderbookSnapshotEvent 处理，写入 Redis Hash |

**已移除的模块**（单通道架构不再需要）：
- ~~dex-realtime crate~~ → 功能合并到 dex-indexer
- ~~listener.rs（RPC 订阅）~~ → 使用 Checkpoint 通道
- ~~recovery.rs（启动恢复）~~ → Checkpoint 本身支持恢复

### 1.5 WebSocket 层（dex-ws，新建 crate）

| 文件 | 类型 | 说明 |
|------|------|------|
| dex-ws/Cargo.toml | 新增 | Crate 配置 |
| dex-ws/src/lib.rs | 新增 | 库入口 |
| dex-ws/src/main.rs | 新增 | 二进制入口 |
| dex-ws/src/config.rs | 新增 | 配置定义 |
| dex-ws/src/server.rs | 新增 | WebSocket 服务器 |
| dex-ws/src/channels.rs | 新增 | 频道管理 |
| dex-ws/src/subscriber.rs | 新增 | Redis Stream 消费 |
| dex-ws/src/types.rs | 新增 | WS 消息类型 |

### 1.6 工作区配置

| 文件 | 修改类型 | 说明 |
|------|----------|------|
| dex-sui/Cargo.toml | 修改 | 添加 dex-ws 到 workspace members（~~dex-realtime 已移除~~） |

---

## 2. dex-indexer Redis 发布功能实现清单

> **架构变更（2026-02-05）**：原 dex-realtime 模块的功能已合并到 dex-indexer。以下是增强后的实现清单。

### 2.1 核心模块

#### 2.1.1 redis_config.rs - Redis 配置模块

```
□ 定义 RedisConfig 结构
  - redis_url: String
  - stream_max_len: usize (默认 10000)
  - hash_ttl_seconds: u64 (可选)

□ 实现配置加载
  - 从环境变量读取 REDIS_URL
  - 从配置文件读取
  - 默认值处理
```

#### 2.1.2 redis_publisher.rs - Redis 发布核心

```
□ 实现 RedisPublisher
  - new(): 建立 Redis 连接
  - publish_to_stream(): 写入 Redis Stream
  - update_hash(): 更新 Redis Hash
  - update_sorted_set(): 更新 Redis Sorted Set

□ 实现幂等发布（详见 10-redis-message-spec.md）
  - 生成去重键：dex:event:seen:{tx_digest}-{event_seq}
  - 使用 SET NX EX 3600 原子检查
  - 仅新事件执行 XADD

□ 实现消息格式（详见 10-redis-message-spec.md）
  - XADD 字段：event_id, event_type, perpetual_id, timestamp, data
  - MAXLEN ~ 10000 控制 Stream 长度
  - JSON 序列化 data 字段

□ 实现具体发布方法
  - publish_fill(): FillEventV1 → dex:stream:fills + dex:trades:* + dex:market:*
  - publish_order(): Order 事件 → dex:stream:orders
  - publish_position(): PositionUpdate → dex:stream:positions + dex:position:*
  - publish_liquidation(): Liquidation → dex:stream:liquidations
  - update_orderbook(): OrderbookSnapshot → dex:orderbook:*
  - update_candle(): K 线更新 → dex:candle:*
```

#### 2.1.3 handlers/ 增强 - 事件处理器 Redis 写入

```
□ fills.rs 增强
  - 处理 FillEventV1 后调用 redis_publisher.publish_fill()
  - 同时更新 K 线：redis_publisher.update_candle()
  - 同时更新市场统计：redis_publisher.update_market_stats()

□ orders.rs 增强
  - 处理 OrderPlacedEventV1 后调用 redis_publisher.publish_order("placed", ...)
  - 处理 OrderRemovedEventV1 后调用 redis_publisher.publish_order("removed", ...)

□ positions.rs 增强
  - 处理 PositionUpdateEventV1 后调用 redis_publisher.publish_position()

□ orderbook.rs 新增
  - 处理 OrderbookSnapshotEvent
  - 调用 redis_publisher.update_orderbook()
  - 不写入 PostgreSQL（仅实时状态）
```

#### 2.1.4 candles.rs - K 线聚合

```
□ 定义 CandleAggregator
  - 支持多周期：1m, 5m, 15m, 1h, 4h, 1d
  - 当前 K 线状态（内存）
  - 历史 K 线缓存

□ 实现聚合逻辑
  - process_fill(): 处理成交更新 K 线
  - get_current_candle(): 获取当前 K 线
  - flush_to_redis(): 写入 Redis Hash

□ 实现存储
  - 实时 K 线存 Redis Hash：dex:candle:{perpetual_id}:{interval}
  - 历史 K 线存 Redis Sorted Set：dex:candles:{perpetual_id}:{interval}
  - 完整历史存 PostgreSQL（可选）
```

#### 2.1.5 market_stats.rs - 市场统计

```
□ 定义 MarketStats 结构
  - mid_price: 中间价
  - best_bid: 最优买价
  - best_ask: 最优卖价
  - volume_24h: 24h 成交量
  - volume_24h_usd: 24h 成交额
  - open_interest: 未平仓量
  - funding_rate: 资金费率

□ 实现统计计算
  - 中间价：从 OrderbookSnapshot 获取
  - 24h 成交量：从成交事件累加
  - 未平仓量：从持仓事件聚合

□ 实现 Redis 同步
  - 每次成交后更新到 Redis Hash：dex:market:{perpetual_id}
```

### 2.2 主流程变更

```
□ dex-indexer main.rs 增强
  1. 加载配置（新增 Redis 配置）
  2. 建立数据库连接
  3. 建立 Redis 连接 [新增]
  4. 初始化 RedisPublisher [新增]
  5. 初始化 CandleAggregator [新增]
  6. 初始化 MarketStats [新增]
  7. 启动 Checkpoint 处理循环
  8. 事件处理时同时写入 PostgreSQL + Redis [增强]
  9. 优雅关闭处理
```

### 2.3 已移除的模块（单通道架构不再需要）

| 原模块 | 说明 | 替代方案 |
|--------|------|----------|
| ~~dex-realtime crate~~ | RPC 订阅服务 | 功能合并到 dex-indexer |
| ~~listener.rs~~ | Sui RPC 事件订阅 | 使用 Checkpoint 通道 |
| ~~recovery.rs~~ | 启动恢复逻辑 | Checkpoint 本身支持恢复 |
| ~~orderbook.rs（内存维护）~~ | 订单簿内存构建 | 直接使用 OrderbookSnapshotEvent |

---

## 3. dex-ws 实现清单

### 3.1 核心模块

#### 3.1.1 config.rs - 配置模块

```
□ 定义 WsConfig 结构
  - host: String
  - port: u16
  - redis_url: String

□ 定义连接配置
  - max_connections: usize
  - heartbeat_interval_ms: u64
  - heartbeat_timeout_ms: u64
```

#### 3.1.2 server.rs - WebSocket 服务器

```
□ 实现 WsServer
  - start(): 启动服务器
  - handle_connection(): 处理新连接
  - handle_message(): 处理客户端消息

□ 实现连接管理
  - 连接池管理
  - 心跳检测
  - 连接超时处理
```

#### 3.1.3 channels.rs - 频道管理

```
□ 定义频道类型
  - trades:{perpetual_id}
  - orderbook:{perpetual_id}
  - candle:{perpetual_id}:{interval}
  - user:{subaccount_id}

□ 实现订阅管理
  - subscribe(): 订阅频道
  - unsubscribe(): 取消订阅
  - get_subscribers(): 获取频道订阅者

□ 实现消息分发
  - 按频道分发消息
  - 支持通配符订阅（可选）
```

#### 3.1.4 subscriber.rs - Redis 消费

```
□ 实现 StreamConsumer（详见 10-redis-message-spec.md）
  - 创建消费者组：XGROUP CREATE ... dex-ws-group $ MKSTREAM
  - 使用 XREADGROUP 消费 Redis Stream
  - 处理后使用 XACK 确认消息

□ 实现消息处理
  - 解析 event_type 字段路由
  - 解析 data 字段 JSON
  - 转换为 WS 消息格式
  - 发送到对应频道
```

#### 3.1.5 types.rs - 消息类型

```
□ 定义 WS 消息格式（详见 11-websocket-protocol-spec.md）
  - SubscribeRequest: {"method": "subscribe", "subscription": {...}}
  - UnsubscribeRequest: {"method": "unsubscribe", "subscription": {...}}
  - SubscriptionResponse: {"channel": "subscriptionResponse", "data": {...}}
  - TradeUpdate: {"channel": "trades", "data": {...}}
  - OrderbookSnapshot: {"channel": "l2Book", "data": {"type": "snapshot", ...}}
  - OrderbookDelta: {"channel": "l2Book", "data": {"type": "delta", ...}}
  - CandleUpdate: {"channel": "candle", "data": {...}}
  - UserFillsUpdate: {"channel": "userFills", "data": {...}}
  - UserOrdersUpdate: {"channel": "userOrders", "data": {...}}
  - UserPositionsUpdate: {"channel": "userPositions", "data": {...}}
  - Heartbeat: {"method": "ping"} / {"channel": "pong"}
  - Error: {"channel": "error", "data": {"code": "...", "message": "..."}}
```

### 3.2 主流程

```
□ main.rs 实现
  1. 加载配置
  2. 建立 Redis 连接
  3. 启动 Redis Stream 消费者
  4. 启动 WebSocket 服务器
  5. 启动心跳任务
  6. 优雅关闭处理
```

---

## 4. 数据库迁移清单

### 4.1 新增订单表

```sql
-- migrations/YYYYMMDDHHMMSS_create_orders_table/up.sql

CREATE TABLE dex_orders (
    id BIGSERIAL PRIMARY KEY,
    perpetual_id INTEGER NOT NULL,
    order_id BYTEA NOT NULL UNIQUE,
    subaccount BYTEA NOT NULL,
    side SMALLINT NOT NULL,
    price BIGINT NOT NULL,
    original_quantity BIGINT NOT NULL,
    remaining_quantity BIGINT NOT NULL,
    filled_quantity BIGINT NOT NULL DEFAULT 0,
    order_type SMALLINT NOT NULL,
    reduce_only BOOLEAN NOT NULL DEFAULT FALSE,
    client_order_id BIGINT,
    status SMALLINT NOT NULL DEFAULT 0,  -- 0=open, 1=filled, 2=cancelled, 3=expired
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL,
    removed_at TIMESTAMP WITH TIME ZONE,
    removal_reason SMALLINT
);

-- 索引
CREATE INDEX idx_orders_perpetual_status ON dex_orders(perpetual_id, status);
CREATE INDEX idx_orders_subaccount ON dex_orders(subaccount);
CREATE INDEX idx_orders_created_at ON dex_orders(created_at);
```

```sql
-- migrations/YYYYMMDDHHMMSS_create_orders_table/down.sql

DROP TABLE IF EXISTS dex_orders;
```

### 4.2 新增 K 线表（可选）

```sql
-- migrations/YYYYMMDDHHMMSS_create_candles_table/up.sql

CREATE TABLE dex_candles (
    id BIGSERIAL PRIMARY KEY,
    perpetual_id INTEGER NOT NULL,
    interval VARCHAR(10) NOT NULL,  -- '1m', '5m', '15m', '1h', '4h', '1d'
    open_time TIMESTAMP WITH TIME ZONE NOT NULL,
    open BIGINT NOT NULL,
    high BIGINT NOT NULL,
    low BIGINT NOT NULL,
    close BIGINT NOT NULL,
    volume BIGINT NOT NULL,
    quote_volume BIGINT NOT NULL,
    trade_count INTEGER NOT NULL,
    UNIQUE (perpetual_id, interval, open_time)
);

-- 索引
CREATE INDEX idx_candles_perpetual_interval_time ON dex_candles(perpetual_id, interval, open_time DESC);
```

---

## 5. Redis 键初始化清单

### 5.1 Stream 创建

```bash
# 成交事件流
XGROUP CREATE dex:stream:fills dex-ws-group $ MKSTREAM

# 订单事件流
XGROUP CREATE dex:stream:orders dex-ws-group $ MKSTREAM

# 持仓事件流
XGROUP CREATE dex:stream:positions dex-ws-group $ MKSTREAM

# 清算事件流
XGROUP CREATE dex:stream:liquidations dex-ws-group $ MKSTREAM
```

### 5.2 初始数据结构

```bash
# 订单簿快照（Hash）
HSET dex:orderbook:1 bids "{}" asks "{}" updated_at 0

# 市场统计（Hash）
HSET dex:market:1 mid_px "0" best_bid "0" best_ask "0" volume_24h "0" updated_at 0
```

---

## 6. 测试验证清单

### 6.1 单元测试

```
□ dex-indexer Redis 发布单元测试
  - [ ] redis_config 加载测试
  - [ ] redis_publisher 连接测试
  - [ ] 幂等发布逻辑测试
  - [ ] K 线聚合测试
  - [ ] 市场统计计算测试

□ dex-ws 单元测试
  - [ ] 消息序列化测试
  - [ ] 频道管理测试
  - [ ] 订阅逻辑测试
```

### 6.2 集成测试

```
□ dex-indexer Redis 发布集成测试
  - [ ] Checkpoint 事件处理 + Redis 写入测试
  - [ ] Redis Stream 发布测试
  - [ ] Redis Hash 更新测试
  - [ ] 订单簿快照处理测试

□ dex-ws 集成测试
  - [ ] WebSocket 连接测试
  - [ ] 订阅/取消订阅测试
  - [ ] 消息推送测试
  - [ ] 心跳超时测试
```

### 6.3 端到端测试

```
□ 完整链路测试
  - [ ] 下单 → FillEvent → Checkpoint → dex-indexer → Redis → dex-ws → 客户端
  - [ ] 下单 → OrderPlacedEvent → dex-indexer → 订单簿快照推送
  - [ ] 取消订单 → OrderRemovedEvent → dex-indexer → 订单更新推送
  - [ ] 连续成交 → dex-indexer K 线聚合 → K 线推送

□ 故障恢复测试
  - [ ] dex-indexer 重启后从 Checkpoint 恢复
  - [ ] Redis 连接断开后重连
  - [ ] dex-ws 客户端断开后重连订阅
```

### 6.4 性能测试

```
□ 延迟测试
  - [ ] 事件从链上到 dex-ws 推送的端到端延迟 < 500ms
  - [ ] 订单簿快照更新延迟 < 100ms

□ 吞吐测试
  - [ ] dex-realtime 处理 1000 事件/秒
  - [ ] dex-ws 支持 1000 并发连接
  - [ ] Redis Stream 写入吞吐量
```

---

## 7. 实施顺序建议

> **更新（2026-02-05）**：采用单通道架构后，实施顺序调整如下。

### Phase 2.1: dex-indexer Redis 发布功能

```
1. 添加 redis 依赖到 dex-indexer
2. 实现 redis_config 模块
3. 实现 redis_publisher 模块（核心）
4. 修改 fills handler 增加 Redis 写入
5. 修改 positions handler 增加 Redis 写入
6. 验证：事件可写入 Redis Stream
```

### Phase 2.2: 订单事件和订单簿处理

```
1. 实现 dex-indexer Orders Handler
2. 执行数据库迁移（dex_orders 表）
3. 实现 orderbook handler（处理 OrderbookSnapshotEvent）
4. 实现订单簿快照写入 Redis Hash
5. 验证：订单簿快照可从 Redis 查询
```

### Phase 2.3: K 线与市场统计

```
1. 实现 candles 模块（集成到 dex-indexer）
2. 实现 market_stats 模块（集成到 dex-indexer）
3. 在 fills handler 中调用 K 线聚合
4. 验证：K 线和市场统计可从 Redis 查询
```

### Phase 2.4: dex-ws WebSocket 服务

```
1. 创建 dex-ws crate 结构
2. 实现 server 模块
3. 实现 channels 模块
4. 实现 subscriber 模块（消费 Redis Stream）
5. 实现 types 模块
6. 验证：客户端可订阅实时数据
```

### Phase 2.5: dex-api 扩展（可选 Redis 缓存）

```
1. 添加 l2Book 查询接口（从 Redis Hash 读取）
2. 添加 candleSnapshot 查询接口（Redis + PostgreSQL 合并）
3. 添加市场统计查询接口（从 Redis Hash 读取）
4. 验证：HTTP API 与 Hyperliquid 对标
```

---

## 8. 依赖清单

### 8.1 dex-indexer 新增依赖（Redis 发布功能）

```toml
[dependencies]
# 现有依赖保持不变...

# Redis（新增）
redis = { version = "0.24", features = ["tokio-comp", "streams"] }
```

### 8.2 dex-ws 依赖

```toml
[dependencies]
# WebSocket
tokio-tungstenite = "0.21"

# Async runtime
tokio = { version = "1", features = ["full"] }

# Redis
redis = { version = "0.24", features = ["tokio-comp", "streams"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Config
config = "0.13"
```

---

## 9. 规范文档索引

实施过程中需参考以下规范文档：

| 文档 | 内容 | 使用场景 |
|------|------|----------|
| `10-redis-message-spec.md` | Redis Stream 消息格式规范 | publisher.rs, subscriber.rs 实现 |
| `11-websocket-protocol-spec.md` | WebSocket 协议规范 | types.rs, channels.rs, server.rs 实现 |
| `04-design-decisions.md` | 设计决策记录 | 架构决策参考 |
| `07-multi-node-consistency.md` | 多节点一致性分析 | 幂等发布、事件补齐实现 |

### 关键实现参考

| 实现任务 | 参考章节 |
|----------|----------|
| 幂等发布 (SET NX + XADD) | `10-redis-message-spec.md` §4 去重机制 |
| Redis Stream 消息结构 | `10-redis-message-spec.md` §2-3 |
| WebSocket 订阅/响应格式 | `11-websocket-protocol-spec.md` §3 |
| 频道消息格式 | `11-websocket-protocol-spec.md` §4 |
| 心跳机制 | `11-websocket-protocol-spec.md` §5 |
| 错误处理 | `11-websocket-protocol-spec.md` §6 |

---

## 10. 链上实现清单（2026-02-05 新增）

本节记录已完成的链上订单簿快照推送实现。

### 10.1 已完成的修改

| 文件 | 操作 | 内容 |
|------|------|------|
| `sui-types/src/dex_events.rs` | ✅ 新增 | OrderbookSnapshotEvent, PriceLevelSnapshot, OrderbookStats |
| `sui-types/src/dex/perpetual.rs` | ✅ 修改 | 添加 ORDERBOOK_SNAPSHOT_INTERVAL_MS, ORDERBOOK_SNAPSHOT_MAX_DEPTH 常量；添加 last_snapshot_timestamp_ms, snapshot_sequence_number 字段；添加 should_emit_snapshot(), record_snapshot_emitted() 方法 |
| `sui-execution/src/dex/helpers.rs` | ✅ 新增 | generate_orderbook_snapshot() 函数 |
| `sui-execution/src/dex/commands/order.rs` | ✅ 修改 | 在 execute_place_order, execute_cancel_order, execute_cancel_all_orders 中添加快照发射逻辑 |

### 10.2 配置参数

```rust
// sui-types/src/dex/perpetual.rs

/// Orderbook snapshot push interval in milliseconds
pub const ORDERBOOK_SNAPSHOT_INTERVAL_MS: u64 = 250;

/// Maximum depth (number of price levels) in orderbook snapshot
pub const ORDERBOOK_SNAPSHOT_MAX_DEPTH: usize = 100;
```

### 10.3 快照发射逻辑

```rust
// 在每个订单操作函数末尾
if state.should_emit_snapshot(current_timestamp_ms) {
    let snapshot = generate_orderbook_snapshot(
        &state,
        checkpoint_sequence,
        current_timestamp_ms,
        None, // Use default depth (100 levels)
    );
    events.push(snapshot.to_sui_event(ctx.transaction_signer));
    state.record_snapshot_emitted(current_timestamp_ms);
}
```

### 10.4 待完成项（TODO）

| TODO | 说明 | 影响 |
|------|------|------|
| 时间戳来源 | 目前使用 0，需要从执行上下文获取 | 快照时间不准确 |
| Checkpoint 序列号 | 目前使用 0，需要从执行上下文获取 | 无法追踪快照与 Checkpoint 的对应关系 |

### 10.5 架构简化收益

| 方面 | 原方案（链下构建） | 新方案（链上快照） |
|------|-------------------|-------------------|
| dex-realtime 订单簿逻辑 | 复杂（add/remove/update） | **极简**（直接使用快照） |
| 启动恢复 | 从 PostgreSQL 加载 | **等待下一个快照** |
| 一致性 | 可能漂移 | **绝对一致** |
| 代码量（dex-realtime） | ~500 行 | **~50 行** |
