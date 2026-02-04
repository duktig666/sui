# Phase 2 实施清单

## 概述

本文档列出 Phase 2（dex-realtime + dex-ws）实施过程中需要修改的文件清单、具体实现任务和测试验证清单。

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

### 1.4 实时层（dex-realtime，新建 crate）

| 文件 | 类型 | 说明 |
|------|------|------|
| dex-realtime/Cargo.toml | 新增 | Crate 配置 |
| dex-realtime/src/lib.rs | 新增 | 库入口 |
| dex-realtime/src/main.rs | 新增 | 二进制入口 |
| dex-realtime/src/config.rs | 新增 | 配置定义 |
| dex-realtime/src/listener.rs | 新增 | Sui RPC 事件订阅 |
| dex-realtime/src/publisher.rs | 新增 | Redis Stream 发布 |
| dex-realtime/src/orderbook.rs | 新增 | 订单簿内存维护 |
| dex-realtime/src/candles.rs | 新增 | K 线聚合 |
| dex-realtime/src/market_stats.rs | 新增 | 市场统计 |
| dex-realtime/src/recovery.rs | 新增 | 启动恢复逻辑 |

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
| dex-sui/Cargo.toml | 修改 | 添加 dex-realtime, dex-ws 到 workspace members |

---

## 2. dex-realtime 实现清单

### 2.1 核心模块

#### 2.1.1 config.rs - 配置模块

```
□ 定义 RealtimeConfig 结构
  - sui_ws_url: String
  - package_id: String
  - redis_url: String
  - database_url: String

□ 定义 ReconnectConfig 结构
  - initial_delay_ms: u64
  - max_delay_ms: u64
  - backoff_multiplier: u32

□ 实现配置加载
  - 从环境变量读取
  - 从配置文件读取
  - 默认值处理
```

#### 2.1.2 listener.rs - 事件监听

```
□ 实现 SuiEventListener
  - connect(): 建立 WebSocket 连接
  - subscribe(): 订阅事件过滤器
  - reconnect(): 重连逻辑（指数退避）

□ 实现事件解析
  - parse_fill_event()
  - parse_order_placed_event()
  - parse_order_removed_event()
  - parse_position_update_event()
  - parse_liquidation_event()

□ 实现事件分发
  - 发送到内部 channel
  - 错误处理和日志
```

#### 2.1.3 publisher.rs - Redis 发布

```
□ 实现 RedisPublisher
  - connect(): 建立 Redis 连接
  - publish_to_stream(): 写入 Redis Stream
  - set_orderbook_snapshot(): 更新订单簿快照
  - set_market_stats(): 更新市场统计

□ 实现批处理
  - 累积事件到批次
  - 定时刷新（10ms）
  - 批大小上限（100）
```

#### 2.1.4 orderbook.rs - 订单簿维护

```
□ 定义 Orderbook 结构
  - bids: BTreeMap<Price, Vec<Order>>
  - asks: BTreeMap<Price, Vec<Order>>
  - orders: HashMap<OrderId, Order>

□ 实现订单簿操作
  - add_order(): 添加订单
  - remove_order(): 移除订单
  - update_order_quantity(): 更新订单数量
  - get_snapshot(): 获取快照

□ 实现 L2 聚合
  - aggregate_bids(): 按价格聚合买盘
  - aggregate_asks(): 按价格聚合卖盘
  - get_best_bid_ask(): 获取最优价

□ 实现 Redis 同步
  - 定期快照到 Redis（~100ms）
  - 快照格式定义
```

#### 2.1.5 candles.rs - K 线聚合

```
□ 定义 CandleAggregator
  - 支持多周期：1m, 5m, 15m, 1h, 4h, 1d
  - 当前 K 线状态
  - 历史 K 线缓存

□ 实现聚合逻辑
  - process_fill(): 处理成交更新 K 线
  - get_current_candle(): 获取当前 K 线
  - get_history(): 获取历史 K 线

□ 实现存储
  - 实时 K 线存 Redis
  - 历史 K 线存 PostgreSQL（可选）
```

#### 2.1.6 market_stats.rs - 市场统计

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
  - 中间价：(best_bid + best_ask) / 2
  - 24h 成交量：滚动窗口累加
  - 未平仓量：持仓事件聚合

□ 实现 Redis 同步
  - 定期更新到 Redis（~1s）
```

#### 2.1.7 recovery.rs - 启动恢复

```
□ 实现订单簿恢复
  - 从 PostgreSQL 加载 open 状态订单
  - 构建内存订单簿
  - 记录恢复点时间戳

□ 实现 K 线恢复
  - 从 Redis 加载当前 K 线状态
  - 从 PostgreSQL 加载历史 K 线（如需要）

□ 实现统计恢复
  - 从 Redis 加载最近统计
  - 计算滚动窗口统计
```

### 2.2 主流程

```
□ main.rs 实现
  1. 加载配置
  2. 建立数据库连接
  3. 执行启动恢复
  4. 建立 Redis 连接
  5. 启动事件监听
  6. 启动事件处理循环
  7. 启动定时任务（快照、统计）
  8. 优雅关闭处理
```

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
□ 实现 StreamConsumer
  - 创建消费者组
  - 消费 Redis Stream
  - 消息确认

□ 实现消息处理
  - 解析消息类型
  - 转换为 WS 消息格式
  - 发送到对应频道
```

#### 3.1.5 types.rs - 消息类型

```
□ 定义 WS 消息格式
  - SubscribeRequest
  - UnsubscribeRequest
  - TradeUpdate
  - OrderbookSnapshot
  - OrderbookDelta
  - CandleUpdate
  - UserUpdate
  - Heartbeat
  - Error
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
□ dex-realtime 单元测试
  - [ ] config 加载测试
  - [ ] 事件解析测试
  - [ ] 订单簿操作测试
  - [ ] K 线聚合测试
  - [ ] 市场统计计算测试

□ dex-ws 单元测试
  - [ ] 消息序列化测试
  - [ ] 频道管理测试
  - [ ] 订阅逻辑测试
```

### 6.2 集成测试

```
□ dex-realtime 集成测试
  - [ ] Sui RPC 订阅测试（testnet）
  - [ ] Redis Stream 发布测试
  - [ ] 订单簿快照测试
  - [ ] 启动恢复测试

□ dex-ws 集成测试
  - [ ] WebSocket 连接测试
  - [ ] 订阅/取消订阅测试
  - [ ] 消息推送测试
  - [ ] 心跳超时测试
```

### 6.3 端到端测试

```
□ 完整链路测试
  - [ ] 下单 → FillEvent → dex-realtime → Redis → dex-ws → 客户端
  - [ ] 下单 → OrderPlacedEvent → 订单簿更新 → 订单簿快照推送
  - [ ] 取消订单 → OrderRemovedEvent → 订单簿更新
  - [ ] 连续成交 → K 线更新 → K 线推送

□ 故障恢复测试
  - [ ] dex-realtime 重启后订单簿恢复
  - [ ] Redis 连接断开后重连
  - [ ] Sui RPC 断开后重连
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

### Phase 2.1: dex-realtime 基础功能

```
1. 创建 dex-realtime crate 结构
2. 实现 config 模块
3. 实现 listener 模块（Sui RPC 订阅）
4. 实现 publisher 模块（Redis Stream 发布）
5. 实现 main.rs 基础流程
6. 验证：事件可订阅并发布到 Redis
```

### Phase 2.2: 订单簿维护

```
1. 实现 dex-indexer Orders Handler
2. 执行数据库迁移
3. 实现 recovery 模块
4. 实现 orderbook 模块
5. 实现订单簿快照到 Redis
6. 验证：订单簿快照可查询
```

### Phase 2.3: K 线与市场统计

```
1. 实现 candles 模块
2. 实现 market_stats 模块
3. 验证：K 线和市场统计可查询
```

### Phase 2.4: dex-ws WebSocket 服务

```
1. 创建 dex-ws crate 结构
2. 实现 server 模块
3. 实现 channels 模块
4. 实现 subscriber 模块
5. 实现 types 模块
6. 验证：客户端可订阅实时数据
```

### Phase 2.5: dex-api 扩展

```
1. 添加 l2Book 查询接口
2. 添加 candleSnapshot 查询接口
3. 添加市场统计查询接口
4. 验证：HTTP API 与 Hyperliquid 对标
```

---

## 8. 依赖清单

### 8.1 dex-realtime 依赖

```toml
[dependencies]
# Sui SDK
sui-sdk = { path = "../../sui/crates/sui-sdk" }
sui-types = { path = "../../sui/crates/sui-types" }

# Async runtime
tokio = { version = "1", features = ["full"] }
futures = "0.3"

# Redis
redis = { version = "0.24", features = ["tokio-comp", "streams"] }

# Database
diesel = { version = "2", features = ["postgres", "r2d2"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bcs = "0.1"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Config
config = "0.13"
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
