# DYDX Indexer 机制分析计划

## 任务目标
详细分析DYDX的Indexer机制，包括数据索引方式、高性能保持机制、客户端连接方式和数据流。

## 分析成果

基于对 `dydx-v4-chain/indexer` 和 `dydx-v4-chain/protocol/indexer` 代码的深入分析，以下是完整的分析报告内容。

---

## 输出文件
`sui/mynotes/dex/analyst/dydx-indexer-analyst.md`

## 分析报告内容

### 1. 整体架构概览

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           DYDX v4 Indexer 架构                            │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐                                                        │
│  │  Full Node   │  链上事件生成                                           │
│  │  (Protocol)  │──────────────────┐                                     │
│  └──────────────┘                  │                                     │
│        │                           │                                     │
│        │ Off-chain Updates         │ On-chain Events                     │
│        │ (订单放置/取消/更新)        │ (成交/仓位/资金费率等)              │
│        ▼                           ▼                                     │
│  ┌─────────────────────────────────────────────────────────┐            │
│  │                     Kafka Message Queue                  │            │
│  │  Topics:                                                 │            │
│  │  - to-ender (链上事件)                                   │            │
│  │  - to-vulcan (链下订单更新)                               │            │
│  │  - to-websockets-* (WebSocket推送)                       │            │
│  └─────────────────────────────────────────────────────────┘            │
│        │                           │                                     │
│        ▼                           ▼                                     │
│  ┌──────────────┐           ┌──────────────┐                            │
│  │    Ender     │           │    Vulcan    │                            │
│  │ (链上事件处理) │           │(订单更新处理) │                            │
│  └──────────────┘           └──────────────┘                            │
│        │                           │                                     │
│        ├───────────────────────────┼──────────────────┐                 │
│        ▼                           ▼                  ▼                 │
│  ┌──────────────┐           ┌──────────────┐   ┌──────────────┐        │
│  │  PostgreSQL  │           │    Redis     │   │    Kafka     │        │
│  │ (持久化存储)  │           │ (订单簿缓存)  │   │ (WebSocket)  │        │
│  └──────────────┘           └──────────────┘   └──────────────┘        │
│        │                           │                  │                 │
│        └───────────────────────────┼──────────────────┘                 │
│                                    ▼                                     │
│  ┌─────────────────────────────────────────────────────────┐            │
│  │                       Comlink                            │            │
│  │                   (REST API 服务)                        │            │
│  └─────────────────────────────────────────────────────────┘            │
│                                    │                                     │
│  ┌─────────────────────────────────────────────────────────┐            │
│  │                        Socks                             │            │
│  │                  (WebSocket 服务)                        │            │
│  └─────────────────────────────────────────────────────────┘            │
│                                    │                                     │
│                                    ▼                                     │
│  ┌─────────────────────────────────────────────────────────┐            │
│  │                       客户端                              │            │
│  │          (前端应用 / 交易机器人 / 第三方服务)              │            │
│  └─────────────────────────────────────────────────────────┘            │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2. 核心服务组件

| 服务 | 功能 | 数据来源 | 数据输出 |
|-----|------|---------|---------|
| **Ender** | 处理链上事件，写入数据库 | Kafka (to-ender) | PostgreSQL, Kafka (WebSocket topics) |
| **Vulcan** | 处理链下订单更新，维护订单簿 | Kafka (to-vulcan) | Redis, Kafka (WebSocket topics) |
| **Comlink** | REST API 服务 | PostgreSQL, Redis | HTTP 响应 |
| **Socks** | WebSocket 服务 | Kafka (WebSocket topics), Comlink | WebSocket 推送 |
| **Auxo** | 辅助服务 (PnL计算等) | PostgreSQL | PostgreSQL |
| **Bazooka** | 批量操作服务 | Redis | Redis |

### 3. 数据流详解

#### 3.1 链上事件数据流 (On-chain Events)

**事件类型** (定义于 `protocol/indexer/events/`):
- `OrderFillEvent` - 订单成交
- `SubaccountUpdateEvent` - 子账户更新 (仓位/余额变化)
- `TransferEvent` - 转账事件
- `FundingEvent` - 资金费率事件
- `MarketEvent` - 市场参数变化
- `LiquidityTierEvent` - 流动性层级变化
- `StatefulOrderEvent` - 有状态订单事件

**数据流程**:
```
Protocol (链上)
    │
    │ 1. 在 ABCI 生命周期中生成事件
    │    - AddTxnEvent() / AddBlockEvent()
    │    - 存储在 TransientStore
    ▼
IndexerEventManager
    │
    │ 2. EndBlocker 时调用 ProduceBlock()
    │    - 收集所有事件
    │    - 创建 IndexerTendermintBlock
    ▼
IndexerMessageSenderKafka
    │
    │ 3. SendOnchainData()
    │    - 序列化 protobuf
    │    - 发送到 Kafka "to-ender" topic
    ▼
Ender 服务
    │
    │ 4. 消费 Kafka 消息
    │    - 反序列化事件
    │    - 按事件类型分发到 Handler
    ▼
事件 Handlers
    │
    │ 5. 处理具体事件类型
    │    - OrderHandler: 订单成交
    │    - SubaccountUpdateHandler: 仓位更新
    │    - FundingHandler: 资金费率
    ▼
PostgreSQL + Kafka (WebSocket topics)
```

#### 3.2 链下订单数据流 (Off-chain Updates)

**更新类型** (定义于 `protocol/indexer/off_chain_updates/`):
- `OrderPlaceV1` - 订单放置
- `OrderRemoveV1` - 订单移除
- `OrderUpdateV1` - 订单更新 (部分成交)
- `OrderReplaceV1` - 订单替换

**数据流程**:
```
Protocol (链上 CheckTx/DeliverTx)
    │
    │ 1. 订单操作时生成 Off-chain Update
    │    - CreateOrderPlaceMessage()
    │    - CreateOrderRemoveMessage()
    │    - CreateOrderUpdateMessage()
    ▼
IndexerEventManager
    │
    │ 2. SendOffchainData()
    │    - 序列化 protobuf
    │    - 发送到 Kafka "to-vulcan" topic
    ▼
Vulcan 服务
    │
    │ 3. 消费 Kafka 消息
    │    - 反序列化更新
    │    - 按类型分发到 Handler
    ▼
Order Handlers
    │
    │ 4. 更新 Redis 缓存
    │    - OrdersCache: 订单数据
    │    - OrderbookLevelsCache: 价格层级
    │    - SubaccountOrderIdsCache: 账户订单映射
    ▼
Redis + Kafka (to-websockets-subaccounts)
```

### 4. 数据存储设计

#### 4.1 PostgreSQL 数据模型 (持久化存储)

**核心表结构** (定义于 `indexer/packages/postgres/src/models/`):

| 表名 | 用途 | 索引类型数据 |
|-----|------|-------------|
| `blocks` | 区块信息 | 区块高度、时间 |
| `orders` | 订单记录 | 订单详情、状态、成交量 |
| `fills` | 成交记录 | 成交价格、数量、费用 |
| `perpetual_positions` | 永续仓位 | 仓位大小、入场价格 |
| `asset_positions` | 资产仓位 | 余额 |
| `subaccounts` | 子账户 | 账户信息 |
| `candles` | K线数据 | OHLCV |
| `funding_index_updates` | 资金费率 | 历史费率 |
| `oracle_prices` | 预言机价格 | 历史价格 |
| `perpetual_markets` | 永续市场 | 市场配置 |
| `transfers` | 转账记录 | 转账历史 |
| `trading_rewards` | 交易奖励 | 奖励数据 |
| `pnl_ticks` | PnL快照 | 损益历史 |

#### 4.2 Redis 缓存结构 (高性能访问)

**订单簿缓存** (`orderbook-levels-cache.ts`):
```
Key: v4/orderbookLevels/{ticker}/{side}
Type: HSET
Value: { [price]: quantums }

Key: v4/orderbookLevels/{ticker}/{side}/lastUpdated
Type: HSET
Value: { [price]: timestamp }
```

**订单缓存** (`orders-cache.ts`, `orders-data-cache.ts`):
```
Key: v4/orders/{orderId}
Type: STRING
Value: RedisOrder (protobuf)

Key: v4/ordersData/{orderId}
Type: STRING
Value: Order metadata
```

**子账户订单映射** (`subaccount-order-ids-cache.ts`):
```
Key: v4/subaccountOrderIds/{subaccountId}
Type: SET
Value: [orderId, ...]
```

**其他缓存**:
- `CanceledOrdersCache` - 取消的订单
- `StatefulOrderUpdatesCache` - 有状态订单更新
- `OrderExpiryCache` - 订单过期时间
- `OrderbookMidPricesCache` - 订单簿中间价

### 5. 高性能设计

#### 5.1 Kafka 消息队列

**Topic 设计**:
```
to-ender              链上事件 (区块级批量处理)
to-vulcan             链下订单更新 (实时处理)
to-websockets-orderbooks    订单簿更新推送
to-websockets-subaccounts   子账户更新推送
to-websockets-trades        成交推送
to-websockets-markets       市场信息推送
to-websockets-candles       K线推送
to-websockets-block-height  区块高度推送
```

**性能优化**:
- 异步生产者 (`sarama.AsyncProducer`)
- 分区策略 (JVM Compatible Partitioner)
- 批量处理 (`BATCH_PROCESSING_ENABLED`)
- 消息压缩

#### 5.2 Redis 性能优化

**Lua 脚本原子操作** (`scripts.ts`):
- `incrementOrderbookLevelScript` - 原子更新价格层级
- `deleteZeroPriceLevelScript` - 原子删除零值层级
- `deleteStalePriceLevelScript` - 删除过期层级

**数据结构优化**:
- HSET 存储价格层级 (O(1) 访问)
- SET 存储子账户订单 ID
- 整数存储 quantums 避免浮点误差

#### 5.3 数据库优化

**索引策略**:
- 复合索引 (subaccountId + status + goodTilBlock)
- 部分索引 (仅活跃订单)
- 物化视图 (Vault hourly view)

**连接池**:
- PostgreSQL 连接池
- 读写分离 (Read-only client)

#### 5.4 缓存策略

**内存缓存** (`caches/`):
- `perpetualMarketRefresher` - 永续市场配置
- `assetRefresher` - 资产配置
- `liquidityTierRefresher` - 流动性层级
- `blockHeightRefresher` - 区块高度

**定时刷新**:
```typescript
wrapBackgroundTask(perpetualMarketRefresher.start(), true, 'startUpdatePerpetualMarkets');
wrapBackgroundTask(blockHeightRefresher.start(), true, 'startUpdateBlockHeight');
```

### 6. 客户端连接方式

#### 6.1 REST API (Comlink 服务)

**端点示例**:
```
GET /v4/addresses/{address}/subaccountNumber/{subaccountNumber}
GET /v4/orders?address={address}&subaccountNumber={subaccountNumber}&status={status}
GET /v4/orderbooks/perpetualMarket/{ticker}
GET /v4/candles/perpetualMarkets/{ticker}?resolution={resolution}
GET /v4/trades/perpetualMarket/{ticker}
GET /v4/perpetualMarkets
GET /v4/height
```

**数据来源**:
- PostgreSQL: 历史数据、订单记录、仓位等
- Redis: 实时订单簿 (`OrderbookLevelsCache.getOrderBookLevels()`)

#### 6.2 WebSocket (Socks 服务)

**订阅频道**:
| 频道 | ID 格式 | 数据内容 |
|-----|--------|---------|
| `v4_accounts` | `{address}/{subaccountNumber}` | 子账户更新、订单状态 |
| `v4_parent_accounts` | `{address}/{parentSubaccountNumber}` | 父账户更新 |
| `v4_orderbook` | `{ticker}` | 订单簿增量更新 |
| `v4_trades` | `{ticker}` | 成交推送 |
| `v4_candles` | `{ticker}/{resolution}` | K线更新 |
| `v4_markets` | (无) | 市场参数更新 |
| `v4_block_height` | (无) | 区块高度更新 |

**订阅流程**:
```
客户端                    Socks                      Comlink
   │                        │                           │
   │ 1. 发送订阅消息         │                           │
   │ {"type":"subscribe",   │                           │
   │  "channel":"v4_orderbook",                         │
   │  "id":"BTC-USD"}       │                           │
   ├───────────────────────►│                           │
   │                        │ 2. 获取初始数据            │
   │                        ├──────────────────────────►│
   │                        │◄──────────────────────────┤
   │ 3. 返回初始快照         │                           │
   │◄───────────────────────┤                           │
   │                        │                           │
   │ 4. 后续增量更新         │                           │
   │ (从 Kafka 消费推送)     │                           │
   │◄───────────────────────┤                           │
```

**消息转发机制** (`message-forwarder.ts`):
- 从 Kafka WebSocket topics 消费消息
- 根据订阅关系分发到对应连接
- 支持批量消息发送 (`batched` 参数)

#### 6.3 连接目标总结

**客户端应该连接 Indexer 服务，而非直接连接节点**:

| 需求 | 连接目标 | 原因 |
|-----|---------|-----|
| 历史数据查询 | Comlink (REST) | 数据持久化在 PostgreSQL |
| 实时订单簿 | Comlink (REST) + Socks (WS) | Redis 缓存 + 实时推送 |
| 账户状态 | Comlink (REST) + Socks (WS) | 数据库查询 + 实时更新 |
| 下单/取消订单 | 节点 gRPC/REST | 交易需要上链 |

### 7. 完整数据流图

```
                                    用户/客户端
                                         │
                    ┌────────────────────┼────────────────────┐
                    │                    │                    │
                    ▼                    ▼                    ▼
              ┌──────────┐        ┌──────────┐         ┌──────────┐
              │   下单   │        │ REST API │         │WebSocket │
              │ (节点)   │        │(Comlink) │         │ (Socks)  │
              └────┬─────┘        └────┬─────┘         └────┬─────┘
                   │                   │                    │
                   │                   │ 查询               │ 订阅
                   │                   ▼                    │
                   │            ┌──────────────┐           │
                   │            │  PostgreSQL  │           │
                   │            │    Redis     │           │
                   │            └──────────────┘           │
                   │                   ▲                    │
                   │                   │                    │
                   ▼                   │                    │
            ┌──────────┐               │              ┌─────┴─────┐
            │  Full    │               │              │   Kafka   │
            │  Node    │               │              │ (WS Topics)│
            └────┬─────┘               │              └─────▲─────┘
                 │                     │                    │
      ┌──────────┼──────────┐         │                    │
      │          │          │         │                    │
      ▼          ▼          ▼         │                    │
┌──────────┐ ┌───────┐ ┌────────┐    │                    │
│to-ender  │ │to-    │ │Off-chain│   │                    │
│(on-chain)│ │vulcan │ │Updates │    │                    │
└────┬─────┘ └───┬───┘ └────────┘    │                    │
     │           │                    │                    │
     ▼           ▼                    │                    │
┌──────────┐ ┌───────┐               │                    │
│  Ender   │ │Vulcan │───────────────┼────────────────────┘
│          │ │       │               │
└────┬─────┘ └───┬───┘               │
     │           │                    │
     └───────────┴────────────────────┘
                 │
                 ▼
          写入数据库/缓存
```

### 8. K线数据索引机制

**K线缓存** (`candle-cache.ts`):
```typescript
// 内存缓存当前K线
let candlesMap: CandlesMap = {};

// 初始化: 从数据库加载最新K线
await CandleTable.findCandlesMap(tickers, latestBlockTime);

// 更新: 成交事件触发K线更新
updateCandleCacheWithCandle(candle);
```

**K线计算流程**:
1. Ender 处理 OrderFill 事件
2. 根据成交价格/数量更新 K线数据
3. 写入 PostgreSQL `candles` 表
4. 推送到 Kafka `to-websockets-candles` topic

### 9. 关键源码文件

**Protocol 层 (事件生成)**:
- `protocol/indexer/indexer_manager/event_manager.go` - 事件管理器
- `protocol/indexer/indexer_manager/events.go` - 事件存储和生产
- `protocol/indexer/msgsender/msgsender_kafka.go` - Kafka 消息发送
- `protocol/indexer/off_chain_updates/off_chain_updates.go` - 链下更新
- `protocol/indexer/events/order_fill.go` - 订单成交事件
- `protocol/indexer/events/subaccount_update.go` - 子账户更新事件

**Indexer 服务**:
- `indexer/services/ender/` - 链上事件处理服务
- `indexer/services/vulcan/` - 链下订单处理服务
- `indexer/services/comlink/` - REST API 服务
- `indexer/services/socks/` - WebSocket 服务

**数据层**:
- `indexer/packages/postgres/src/models/` - 数据库模型
- `indexer/packages/redis/src/caches/` - Redis 缓存
- `indexer/packages/kafka/` - Kafka 配置

---

## 执行计划

将上述分析内容写入 `sui/mynotes/dex/analyst/dydx-indexer-analyst.md` 文件。
