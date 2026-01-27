# DYDX v4 Indexer 机制详细分析

> 基于 dydx-v4-chain 源码分析，涵盖数据索引方式、高性能设计、客户端连接方式和完整数据流。

## 1. 整体架构概览

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

## 2. 核心服务组件

| 服务 | 功能 | 数据来源 | 数据输出 |
|-----|------|---------|---------|
| **Ender** | 处理链上事件，写入数据库 | Kafka (to-ender) | PostgreSQL, Kafka (WebSocket topics) |
| **Vulcan** | 处理链下订单更新，维护订单簿 | Kafka (to-vulcan) | Redis, Kafka (WebSocket topics) |
| **Comlink** | REST API 服务 | PostgreSQL, Redis | HTTP 响应 |
| **Socks** | WebSocket 服务 | Kafka (WebSocket topics), Comlink | WebSocket 推送 |
| **Auxo** | 辅助服务 (PnL计算等) | PostgreSQL | PostgreSQL |
| **Bazooka** | 批量操作服务 | Redis | Redis |

---

## 3. 数据流详解

### 3.1 链上事件数据流 (On-chain Events)

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

**核心代码路径**:

```go
// protocol/indexer/indexer_manager/event_manager.go
type IndexerEventManager interface {
    AddTxnEvent(ctx sdk.Context, subType string, version uint32, dataBytes []byte)
    AddBlockEvent(ctx sdk.Context, subType string, blockEvent IndexerTendermintEvent_BlockEvent, ...)
    SendOnchainData(block *IndexerTendermintBlock)
    ProduceBlock(ctx sdk.Context) *IndexerTendermintBlock
}
```

```go
// protocol/indexer/indexer_manager/events.go
func produceBlock(ctx sdk.Context, storeKey storetypes.StoreKey) *IndexerTendermintBlock {
    // 从 TransientStore 收集所有事件
    events := getIndexerEvents(noGasCtx, storeKey)
    // 按交易和区块事件分类
    // 创建 IndexerTendermintBlock
    return &IndexerTendermintBlock{
        Height:   blockHeight,
        Time:     blockTime,
        Events:   allEvents,
        TxHashes: txHashes,
    }
}
```

### 3.2 链下订单数据流 (Off-chain Updates)

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

**核心代码**:

```go
// protocol/indexer/off_chain_updates/off_chain_updates.go
func CreateOrderPlaceMessage(ctx sdk.Context, order clobtypes.Order) (msgsender.Message, bool) {
    orderIdHash, _ := GetOrderIdHash(order.OrderId)
    update, _ := NewOrderPlaceMessage(order)
    return msgsender.Message{Key: orderIdHash, Value: update}, true
}

func NewOrderPlaceMessage(order clobtypes.Order) ([]byte, error) {
    indexerOrder := v1.OrderToIndexerOrder(order)
    update := ocutypes.OffChainUpdateV1{
        UpdateMessage: &ocutypes.OffChainUpdateV1_OrderPlace{
            OrderPlace: &ocutypes.OrderPlaceV1{
                Order: &indexerOrder,
                PlacementStatus: ocutypes.OrderPlaceV1_ORDER_PLACEMENT_STATUS_BEST_EFFORT_OPENED,
            },
        },
    }
    return proto.Marshal(&update)
}
```

---

## 4. 数据存储设计

### 4.1 PostgreSQL 数据模型 (持久化存储)

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

### 4.2 Redis 缓存结构 (高性能访问)

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

---

## 5. 高性能设计

### 5.1 Kafka 消息队列

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

```go
// protocol/indexer/msgsender/msgsender_kafka.go
func NewIndexerMessageSenderKafka(...) (*IndexerMessageSenderKafka, error) {
    config.Producer.Return.Errors = true
    config.Producer.Return.Successes = true
    config.Producer.Retry.Max = indexerFlags.MaxRetries
    config.Producer.Retry.Backoff = 1000 * time.Millisecond
    config.Producer.MaxMessageBytes = 4194304 // 4MB
    config.Producer.RequiredAcks = sarama.WaitForAll
    config.Producer.Partitioner = kafkautil.NewJVMCompatiblePartitioner
    producer, _ := sarama.NewAsyncProducer(indexerFlags.KafkaAddrs, config)
    // ...
}
```

### 5.2 Redis 性能优化

**Lua 脚本原子操作** (`scripts.ts`):
- `incrementOrderbookLevelScript` - 原子更新价格层级
- `deleteZeroPriceLevelScript` - 原子删除零值层级
- `deleteStalePriceLevelScript` - 删除过期层级

```typescript
// indexer/packages/redis/src/caches/orderbook-levels-cache.ts
export async function updatePriceLevel(
  ticker: string,
  side: OrderSide,
  humanPrice: string,
  sizeDeltaInQuantums: string,
  client: RedisClient,
): Promise<number> {
  // 使用 Lua 脚本原子更新
  const updatedQuantums = await incrementOrderbookLevel(
    ticker, side, humanPrice, sizeDeltaInQuantums, client,
  );
  // 处理负数量情况 (并发竞态)
  if (updatedQuantums < 0) {
    // 重置为 0
  }
  return updatedQuantums;
}
```

**数据结构优化**:
- HSET 存储价格层级 (O(1) 访问)
- SET 存储子账户订单 ID
- 整数存储 quantums 避免浮点误差

### 5.3 数据库优化

**索引策略**:
- 复合索引 (subaccountId + status + goodTilBlock)
- 部分索引 (仅活跃订单)
- 物化视图 (Vault hourly view)

**连接池**:
- PostgreSQL 连接池
- 读写分离 (Read-only client)

### 5.4 缓存策略

**内存缓存** (`caches/`):
- `perpetualMarketRefresher` - 永续市场配置
- `assetRefresher` - 资产配置
- `liquidityTierRefresher` - 流动性层级
- `blockHeightRefresher` - 区块高度

**定时刷新**:
```typescript
// indexer/services/ender/src/index.ts
await Promise.all([
  perpetualMarketRefresher.updatePerpetualMarkets(),
  assetRefresher.updateAssets(),
  liquidityTierRefresher.updateLiquidityTiers(),
]);
```

---

## 6. 客户端连接方式

### 6.1 REST API (Comlink 服务)

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

```typescript
// indexer/services/comlink/src/controllers/api/v4/orderbook-controller.ts
async getPerpetualMarket(ticker: string): Promise<OrderbookResponseObject> {
  const perpetualMarket = perpetualMarketRefresher.getPerpetualMarketFromTicker(ticker);

  // 从 Redis 获取订单簿
  const orderbookLevels = await OrderbookLevelsCache.getOrderBookLevels(
    ticker,
    redisReadOnlyClient,
    {
      sortSides: true,
      uncrossBook: true,  // 解决订单簿交叉问题
      limitPerSide: config.API_ORDERBOOK_LEVELS_PER_SIDE_LIMIT,
    },
  );

  return OrderbookLevelsToResponseObject(orderbookLevels, perpetualMarket);
}
```

### 6.2 WebSocket (Socks 服务)

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

**初始数据获取**:
```typescript
// indexer/services/socks/src/lib/subscription.ts
private getInitialEndpointForSubscription(channel: Channel, id?: string): string | undefined {
  switch (channel) {
    case (Channel.V4_BLOCK_HEIGHT):
      return `${COMLINK_URL}/v4/height`;
    case (Channel.V4_MARKETS):
      return `${COMLINK_URL}/v4/perpetualMarkets`;
    case (Channel.V4_TRADES):
      return `${COMLINK_URL}/v4/trades/perpetualMarket/${id}`;
    case (Channel.V4_ORDERBOOK):
      return `${COMLINK_URL}/v4/orderbooks/perpetualMarket/${id}`;
    case (Channel.V4_CANDLES):
      const { ticker, resolution } = this.parseCandleChannelId(id);
      return `${COMLINK_URL}/v4/candles/perpetualMarkets/${ticker}?resolution=${resolution}`;
    // ...
  }
}
```

**消息转发机制** (`message-forwarder.ts`):
- 从 Kafka WebSocket topics 消费消息
- 根据订阅关系分发到对应连接
- 支持批量消息发送 (`batched` 参数)

### 6.3 连接目标总结

**客户端应该连接 Indexer 服务，而非直接连接节点**:

| 需求 | 连接目标 | 原因 |
|-----|---------|-----|
| 历史数据查询 | Comlink (REST) | 数据持久化在 PostgreSQL |
| 实时订单簿 | Comlink (REST) + Socks (WS) | Redis 缓存 + 实时推送 |
| 账户状态 | Comlink (REST) + Socks (WS) | 数据库查询 + 实时更新 |
| 下单/取消订单 | **节点 gRPC/REST** | 交易需要上链 |

---

## 7. 完整数据流图

### 7.1 系统架构全景图

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                                      客户端层                                            │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐              │
│  │  交易前端   │    │  做市机器人  │    │   API 用户  │    │  数据服务   │              │
│  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘    └──────┬──────┘              │
│         │                  │                  │                  │                      │
│         │    下单/取消     │                  │    查询/订阅     │                      │
│         └────────┬─────────┘                  └────────┬─────────┘                      │
│                  │                                     │                                │
└──────────────────┼─────────────────────────────────────┼────────────────────────────────┘
                   │                                     │
                   ▼                                     ▼
┌──────────────────────────────┐          ┌─────────────────────────────────────────────┐
│         验证节点层            │          │                  Indexer 服务层              │
│  ┌────────────────────────┐  │          │  ┌─────────────┐        ┌─────────────┐    │
│  │      Full Node         │  │          │  │   Comlink   │        │    Socks    │    │
│  │  ┌──────────────────┐  │  │          │  │  (REST API) │        │ (WebSocket) │    │
│  │  │   ABCI 应用      │  │  │          │  └──────┬──────┘        └──────┬──────┘    │
│  │  │  ┌────────────┐  │  │  │          │         │                      │           │
│  │  │  │  MemClob   │  │  │  │          │         │ 查询                 │ 推送      │
│  │  │  │ (内存订单簿)│  │  │  │          │         ▼                      ▼           │
│  │  │  └────────────┘  │  │  │          │  ┌─────────────────────────────────────┐   │
│  │  │  ┌────────────┐  │  │  │          │  │            数据存储层               │   │
│  │  │  │ Indexer    │  │  │  │          │  │  ┌───────────┐    ┌───────────┐    │   │
│  │  │  │ EventMgr   │  │  │  │          │  │  │PostgreSQL │    │   Redis   │    │   │
│  │  │  └────────────┘  │  │  │          │  │  │ (持久化)  │    │  (缓存)   │    │   │
│  │  └──────────────────┘  │  │          │  │  │           │    │           │    │   │
│  └────────────────────────┘  │          │  │  │ • 订单历史│    │ • 订单簿  │    │   │
│              │                │          │  │  │ • 成交记录│    │ • 活跃订单│    │   │
│   ┌──────────┴──────────┐    │          │  │  │ • 仓位    │    │ • 订单状态│    │   │
│   │                     │    │          │  │  │ • K线     │    │           │    │   │
│   ▼                     ▼    │          │  │  └─────▲─────┘    └─────▲─────┘    │   │
│ On-chain           Off-chain │          │  └────────┼────────────────┼──────────┘   │
│ Events             Updates   │          │           │                │              │
└───┬─────────────────────┬────┘          │           │                │              │
    │                     │               │  ┌────────┴────────────────┴────────┐    │
    │                     │               │  │              处理服务层           │    │
    │                     │               │  │  ┌───────────┐    ┌───────────┐  │    │
    │                     │               │  │  │   Ender   │    │  Vulcan   │  │    │
    │                     │               │  │  │(链上事件) │    │(链下更新) │  │    │
    │                     │               │  │  └─────▲─────┘    └─────▲─────┘  │    │
    │                     │               │  └────────┼────────────────┼────────┘    │
    │                     │               │           │                │              │
    │                     │               └───────────┼────────────────┼──────────────┘
    │                     │                           │                │
    │                     │               ┌───────────┴────────────────┴───────────┐
    │                     │               │              Kafka 消息队列             │
    │                     │               │  ┌─────────────────────────────────┐   │
    │                     └──────────────►│  │  to-vulcan (订单更新)            │   │
    │                                     │  └─────────────────────────────────┘   │
    └────────────────────────────────────►│  ┌─────────────────────────────────┐   │
                                          │  │  to-ender (链上事件)             │   │
                                          │  └─────────────────────────────────┘   │
                                          │  ┌─────────────────────────────────┐   │
                                          │  │  to-websockets-* (推送数据)     │◄──┼───┐
                                          │  └─────────────────────────────────┘   │   │
                                          └────────────────────────────────────────┘   │
                                                                                       │
                          Ender/Vulcan 处理后推送到 WebSocket Topics ─────────────────┘
```

### 7.2 双通道数据流详解

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              Full Node (验证节点)                                    │
│                                                                                      │
│   用户交易 ──────────────────────────────────────────────────────────────────┐      │
│       │                                                                      │      │
│       ▼                                                                      │      │
│   ┌────────────────────────────────────────────────────────────────────┐    │      │
│   │                         CheckTx 阶段                                │    │      │
│   │   ┌──────────────────────────────────────────────────────────┐    │    │      │
│   │   │  短期订单处理 (PlaceShortTermOrder/CancelShortTermOrder)  │    │    │      │
│   │   │                                                          │    │    │      │
│   │   │   1. 验证订单                                            │    │    │      │
│   │   │   2. MemClob.PlaceOrder() / MemClob.CancelOrder()        │    │    │      │
│   │   │   3. 生成 OffchainUpdates                                │    │    │      │
│   │   │   4. SendOffchainData() ──────────────────────────────────┼────┼────┼──┐   │
│   │   └──────────────────────────────────────────────────────────┘    │    │  │   │
│   └────────────────────────────────────────────────────────────────────┘    │  │   │
│                                                                              │  │   │
│   ┌────────────────────────────────────────────────────────────────────┐    │  │   │
│   │                        DeliverTx 阶段                               │    │  │   │
│   │                                                                     │    │  │   │
│   │   ┌─────────────────────────────────────────────────────────┐     │    │  │   │
│   │   │  有状态订单处理 (MsgPlaceOrder/MsgCancelOrder)           │     │    │  │   │
│   │   │  • 长期订单 (Long-Term)                                  │     │    │  │   │
│   │   │  • 条件订单 (Conditional)                                │     │    │  │   │
│   │   │  • TWAP 订单                                             │     │    │  │   │
│   │   │                                                          │     │    │  │   │
│   │   │  AddTxnEvent(SubtypeStatefulOrder, ...) ─────────────────┼─────┼────┼──┼─┐ │
│   │   └─────────────────────────────────────────────────────────┘     │    │  │ │ │
│   │                                                                     │    │  │ │ │
│   │   ┌─────────────────────────────────────────────────────────┐     │    │  │ │ │
│   │   │  订单匹配处理 (ProcessProposerOperations)                │     │    │  │ │ │
│   │   │  • MatchOrders: 普通成交                                 │     │    │  │ │ │
│   │   │  • MatchPerpetualLiquidation: 清算成交                   │     │    │  │ │ │
│   │   │  • MatchPerpetualDeleveraging: 去杠杆                    │     │    │  │ │ │
│   │   │                                                          │     │    │  │ │ │
│   │   │  AddTxnEvent(SubtypeOrderFill, ...) ─────────────────────┼─────┼────┼──┼─┤ │
│   │   │  AddTxnEvent(SubtypeDeleveraging, ...) ──────────────────┼─────┼────┼──┼─┤ │
│   │   └─────────────────────────────────────────────────────────┘     │    │  │ │ │
│   │                                                                     │    │  │ │ │
│   │   ┌─────────────────────────────────────────────────────────┐     │    │  │ │ │
│   │   │  子账户更新 (UpdateSubaccounts)                          │     │    │  │ │ │
│   │   │  • 仓位变化                                              │     │    │  │ │ │
│   │   │  • 余额变化                                              │     │    │  │ │ │
│   │   │                                                          │     │    │  │ │ │
│   │   │  AddTxnEvent(SubtypeSubaccountUpdate, ...) ──────────────┼─────┼────┼──┼─┤ │
│   │   └─────────────────────────────────────────────────────────┘     │    │  │ │ │
│   │                                                                     │    │  │ │ │
│   │   ┌─────────────────────────────────────────────────────────┐     │    │  │ │ │
│   │   │  转账处理 (Transfer/Deposit/Withdraw)                    │     │    │  │ │ │
│   │   │                                                          │     │    │  │ │ │
│   │   │  AddTxnEvent(SubtypeTransfer, ...) ──────────────────────┼─────┼────┼──┼─┤ │
│   │   └─────────────────────────────────────────────────────────┘     │    │  │ │ │
│   └────────────────────────────────────────────────────────────────────┘    │  │ │ │
│                                                                              │  │ │ │
│   ┌────────────────────────────────────────────────────────────────────┐    │  │ │ │
│   │                        EndBlocker 阶段                              │    │  │ │ │
│   │                                                                     │    │  │ │ │
│   │   ┌─────────────────────────────────────────────────────────┐     │    │  │ │ │
│   │   │  资金费率计算                                            │     │    │  │ │ │
│   │   │  AddBlockEvent(SubtypeFundingValues, ...) ───────────────┼─────┼────┼──┼─┤ │
│   │   └─────────────────────────────────────────────────────────┘     │    │  │ │ │
│   │                                                                     │    │  │ │ │
│   │   ┌─────────────────────────────────────────────────────────┐     │    │  │ │ │
│   │   │  ProduceBlock() - 收集所有事件                           │     │    │  │ │ │
│   │   │  SendOnchainData(IndexerTendermintBlock) ────────────────┼─────┼────┼──┼─┘ │
│   │   └─────────────────────────────────────────────────────────┘     │    │  │   │
│   └────────────────────────────────────────────────────────────────────┘    │  │   │
│                                                                              │  │   │
└──────────────────────────────────────────────────────────────────────────────┘  │   │
                                                                                   │   │
    ┌──────────────────────────────────────────────────────────────────────────────┘   │
    │                                                                                   │
    │  Off-chain Updates (实时)                                                        │
    │  ┌───────────────────────────────────────────────────────────────────────────────┘
    │  │
    ▼  ▼
┌──────────────────────────────────────────────────────────────────────────────────────┐
│                                 Kafka 消息队列                                        │
│                                                                                       │
│   ┌───────────────────────┐              ┌───────────────────────┐                  │
│   │      to-vulcan        │              │       to-ender        │                  │
│   │   (链下订单更新)       │              │    (链上区块事件)      │                  │
│   │                       │              │                       │                  │
│   │ • OrderPlaceV1        │              │ • IndexerTendermint-  │                  │
│   │ • OrderUpdateV1       │              │   Block               │                  │
│   │ • OrderRemoveV1       │              │   - order_fill        │                  │
│   │ • OrderReplaceV1      │              │   - subaccount_update │                  │
│   │                       │              │   - transfer          │                  │
│   │ 延迟: 毫秒级          │              │   - funding_values    │                  │
│   │ 状态: 乐观 (可回滚)   │              │   - stateful_order    │                  │
│   │                       │              │   - deleveraging      │                  │
│   └───────────┬───────────┘              │   - ...               │                  │
│               │                          │                       │                  │
│               │                          │ 延迟: 区块时间 (~1-2s)│                  │
│               │                          │ 状态: 最终确定        │                  │
│               │                          └───────────┬───────────┘                  │
│               │                                      │                              │
└───────────────┼──────────────────────────────────────┼──────────────────────────────┘
                │                                      │
                ▼                                      ▼
┌───────────────────────────────┐    ┌───────────────────────────────┐
│           Vulcan              │    │            Ender              │
│       (链下更新处理)          │    │        (链上事件处理)          │
│                               │    │                               │
│  ┌─────────────────────────┐ │    │  ┌─────────────────────────┐ │
│  │    OrderHandler         │ │    │  │   OrderFillHandler      │ │
│  │  • 更新订单状态          │ │    │  │  • 写入 fills 表        │ │
│  │  • 更新订单簿层级        │ │    │  │  • 更新 orders 状态     │ │
│  └─────────────────────────┘ │    │  │  • 计算 K线数据         │ │
│               │               │    │  └─────────────────────────┘ │
│               ▼               │    │               │               │
│  ┌─────────────────────────┐ │    │  ┌─────────────────────────┐ │
│  │    写入 Redis            │ │    │  │ SubaccountUpdateHandler │ │
│  │  • OrdersCache          │ │    │  │  • 更新仓位              │ │
│  │  • OrderbookLevelsCache │ │    │  │  • 更新余额              │ │
│  │  • SubaccountOrderIds   │ │    │  └─────────────────────────┘ │
│  └─────────────────────────┘ │    │               │               │
│               │               │    │  ┌─────────────────────────┐ │
│               ▼               │    │  │   StatefulOrderHandler  │ │
│  ┌─────────────────────────┐ │    │  │  • 长期订单状态更新      │ │
│  │ 推送到 Kafka WS Topics  │ │    │  └─────────────────────────┘ │
│  │ to-websockets-          │ │    │               │               │
│  │   subaccounts           │ │    │               ▼               │
│  └─────────────────────────┘ │    │  ┌─────────────────────────┐ │
│                               │    │  │   写入 PostgreSQL       │ │
└───────────────────────────────┘    │  │  • orders, fills        │ │
                                     │  │  • perpetual_positions  │ │
                                     │  │  • candles              │ │
                                     │  │  • funding_index_updates│ │
                                     │  └─────────────────────────┘ │
                                     │               │               │
                                     │               ▼               │
                                     │  ┌─────────────────────────┐ │
                                     │  │ 推送到 Kafka WS Topics  │ │
                                     │  │ to-websockets-          │ │
                                     │  │   orderbooks/trades/    │ │
                                     │  │   candles/markets       │ │
                                     │  └─────────────────────────┘ │
                                     └───────────────────────────────┘
```

### 7.3 客户端数据获取流程

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                               客户端数据获取流程                                      │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │                          1. 下单/取消订单                                     │  │
│   │                                                                              │  │
│   │   客户端 ──► Full Node (gRPC/REST) ──► 交易上链                             │  │
│   │                                                                              │  │
│   │   • MsgPlaceOrder (有状态订单)                                               │  │
│   │   • 短期订单通过交易字节传输                                                 │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │                        2. 查询实时订单簿                                      │  │
│   │                                                                              │  │
│   │   客户端 ──► Comlink (REST) ──► Redis (OrderbookLevelsCache)                │  │
│   │        GET /v4/orderbooks/perpetualMarket/{ticker}                          │  │
│   │                                                                              │  │
│   │   数据来源: Vulcan 处理 Off-chain Updates 后写入 Redis                       │  │
│   │   延迟: 毫秒级                                                               │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │                        3. 查询历史订单/成交                                   │  │
│   │                                                                              │  │
│   │   客户端 ──► Comlink (REST) ──► PostgreSQL                                  │  │
│   │        GET /v4/orders?address={}&status={}                                  │  │
│   │        GET /v4/fills?address={}&subaccountNumber={}                         │  │
│   │                                                                              │  │
│   │   数据来源: Ender 处理 On-chain Events 后写入 PostgreSQL                     │  │
│   │   延迟: 区块确认后 (~1-2s)                                                   │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │                        4. 订阅实时更新 (WebSocket)                            │  │
│   │                                                                              │  │
│   │   客户端 ◄──► Socks (WebSocket) ◄── Kafka (to-websockets-*)                 │  │
│   │                                                                              │  │
│   │   ┌─────────────────────────────────────────────────────────────────────┐   │  │
│   │   │ 订阅频道                数据来源              更新频率                │   │  │
│   │   ├─────────────────────────────────────────────────────────────────────┤   │  │
│   │   │ v4_accounts            Vulcan (Redis)         实时 (毫秒级)          │   │  │
│   │   │ v4_orderbook           Vulcan (Redis)         实时 (毫秒级)          │   │  │
│   │   │ v4_trades              Ender (PostgreSQL)     区块确认后             │   │  │
│   │   │ v4_candles             Ender (PostgreSQL)     区块确认后             │   │  │
│   │   │ v4_markets             Ender (PostgreSQL)     参数变更时             │   │  │
│   │   │ v4_block_height        Ender                  每个区块               │   │  │
│   │   └─────────────────────────────────────────────────────────────────────┘   │  │
│   │                                                                              │  │
│   │   订阅流程:                                                                  │  │
│   │   1. 客户端发送 subscribe 消息                                              │  │
│   │   2. Socks 从 Comlink 获取初始快照                                          │  │
│   │   3. Socks 消费 Kafka 消息并推送增量更新                                    │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### 7.4 数据一致性保证

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                              数据一致性设计                                          │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  Off-chain Updates (乐观更新)                                                │  │
│   │                                                                              │  │
│   │  状态: BEST_EFFORT_OPENED / BEST_EFFORT_CANCELED                            │  │
│   │  含义: 订单状态是乐观的，可能被后续链上确认覆盖                               │  │
│   │                                                                              │  │
│   │  场景:                                                                       │  │
│   │  1. 用户下单 → Off-chain: OrderPlace (BEST_EFFORT_OPENED)                   │  │
│   │  2. 交易上链 → On-chain: OrderFill / StatefulOrder (最终确认)               │  │
│   │  3. 如果链上拒绝 → Off-chain: OrderRemove (状态修正)                        │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  On-chain Events (最终确定)                                                  │  │
│   │                                                                              │  │
│   │  状态: FILLED / CANCELED (最终状态)                                         │  │
│   │  含义: 链上确认的最终状态，不可回滚                                          │  │
│   │                                                                              │  │
│   │  数据权威性:                                                                 │  │
│   │  • PostgreSQL 数据 > Redis 数据 (发生冲突时以链上为准)                       │  │
│   │  • Ender 处理会触发 Redis 状态修正                                          │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   时间线示例:                                                                        │
│                                                                                      │
│   T0: 用户下单                                                                      │
│       │                                                                              │
│   T1: CheckTx → Off-chain: OrderPlace (BEST_EFFORT_OPENED)                         │
│       │         → Redis: 订单簿更新                                                 │
│       │         → WebSocket: v4_orderbook 推送                                      │
│       │                                                                              │
│   T2: 区块确认 (1-2s 后)                                                            │
│       │                                                                              │
│   T3: DeliverTx → On-chain: OrderFill (如果成交)                                   │
│       │         → PostgreSQL: 写入成交记录                                          │
│       │         → WebSocket: v4_trades 推送                                         │
│       │                                                                              │
│   T4: EndBlocker → On-chain: SubaccountUpdate                                       │
│                  → PostgreSQL: 更新仓位                                             │
│                  → WebSocket: v4_accounts 推送                                      │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. K线数据索引机制

**K线缓存** (`candle-cache.ts`):
```typescript
// 内存缓存当前K线
let candlesMap: CandlesMap = {};

// 初始化: 从数据库加载最新K线
export async function startCandleCache(txId?: number): Promise<void> {
  const latestBlock = await BlockTable.getLatest({ txId });
  const perpetualMarkets = await PerpetualMarketTable.findAll({}, [], { txId });
  const tickers = _.map(perpetualMarkets, PerpetualMarketColumns.ticker);

  candlesMap = await CandleTable.findCandlesMap(tickers, latestBlock.time);
}

// 更新: 成交事件触发K线更新
export function updateCandleCacheWithCandle(candle: CandleFromDatabase): void {
  if (!(candle.ticker in candlesMap)) {
    candlesMap[candle.ticker] = {};
  }
  candlesMap[candle.ticker][candle.resolution] = candle;
}
```

**K线计算流程**:
1. Ender 处理 OrderFill 事件
2. 根据成交价格/数量更新 K线数据
3. 写入 PostgreSQL `candles` 表
4. 推送到 Kafka `to-websockets-candles` topic

**支持的时间周期** (CandleResolution):
- 1分钟, 5分钟, 15分钟, 30分钟
- 1小时, 4小时
- 1天

---

## 9. 关键源码文件路径

### Protocol 层 (事件生成)
| 文件 | 功能 |
|-----|------|
| `protocol/indexer/indexer_manager/event_manager.go` | 事件管理器接口和实现 |
| `protocol/indexer/indexer_manager/events.go` | 事件存储和 ProduceBlock |
| `protocol/indexer/msgsender/msgsender_kafka.go` | Kafka 消息发送 |
| `protocol/indexer/off_chain_updates/off_chain_updates.go` | 链下订单更新消息 |
| `protocol/indexer/events/order_fill.go` | 订单成交事件 |
| `protocol/indexer/events/subaccount_update.go` | 子账户更新事件 |

### Indexer 服务
| 目录 | 功能 |
|-----|------|
| `indexer/services/ender/` | 链上事件处理服务 |
| `indexer/services/vulcan/` | 链下订单处理服务 |
| `indexer/services/comlink/` | REST API 服务 |
| `indexer/services/socks/` | WebSocket 服务 |
| `indexer/services/auxo/` | 辅助计算服务 |
| `indexer/services/bazooka/` | 批量操作服务 |

### 数据层
| 目录 | 功能 |
|-----|------|
| `indexer/packages/postgres/src/models/` | 数据库模型 |
| `indexer/packages/redis/src/caches/` | Redis 缓存 |
| `indexer/packages/kafka/` | Kafka 配置 |

---

## 10. 设计亮点总结

### 10.1 双通道数据流
- **On-chain Events**: 区块级批量处理，保证最终一致性
- **Off-chain Updates**: 实时订单更新，低延迟响应

### 10.2 多级缓存架构
```
请求 → 内存缓存 (ms) → Redis (1-10ms) → PostgreSQL (10-100ms)
```

### 10.3 订单簿解耦
- 链上只存储订单哈希和成交结果
- 完整订单簿维护在 Redis
- 支持订单簿交叉自动修正 (uncrossBook)

### 10.4 WebSocket 扇出
- 单一 Kafka 消费者
- 根据订阅关系分发到多个连接
- 支持批量消息发送减少网络开销

### 10.5 性能优化
- Lua 脚本保证 Redis 原子操作
- 异步 Kafka 生产者
- 数据库读写分离
- 定时刷新内存缓存

---

---

## 11. 代码级别详细分析: On-chain Events vs Off-chain Updates

> 基于 `dydx-v4-chain/protocol/` 源码的深入分析，详细说明每种数据类型的触发位置和场景。

### 11.1 On-chain Events (链上事件)

On-chain Events 通过 `AddTxnEvent()` 或 `AddBlockEvent()` 在 DeliverTx 期间添加到 TransientStore，然后在 EndBlocker 时通过 `ProduceBlock()` 批量发送到 Kafka `to-ender` topic。

#### 事件类型定义 (`protocol/indexer/events/constants.go:46-62`)

```go
var OnChainEventSubtypes = []string{
    SubtypeOrderFill,          // "order_fill"
    SubtypeSubaccountUpdate,   // "subaccount_update"
    SubtypeTransfer,           // "transfer"
    SubtypeMarket,             // "market"
    SubtypeFundingValues,      // "funding_values"
    SubtypeStatefulOrder,      // "stateful_order"
    SubtypeAsset,              // "asset"
    SubtypePerpetualMarket,    // "perpetual_market"
    SubtypeLiquidityTier,      // "liquidity_tier"
    SubtypeUpdatePerpetual,    // "update_perpetual"
    SubtypeUpdateClobPair,     // "update_clob_pair"
    SubtypeDeleveraging,       // "deleveraging"
    SubtypeTradingReward,      // "trading_reward"
    SubtypeRegisterAffiliate,  // "register_affiliate"
    SubtypeUpsertVault,        // "upsert_vault"
}
```

#### 详细触发场景

| 事件类型 | 触发场景 | 代码位置 | 触发时机 |
|---------|---------|---------|---------|
| **order_fill** | 订单成交 (maker-taker 匹配) | `process_operations.go:552-572` | DeliverTx 处理 MatchOrders |
| **order_fill** | 清算成交 | `process_operations.go:667-684` | DeliverTx 处理 MatchPerpetualLiquidation |
| **subaccount_update** | 仓位/余额变化 | `subaccount.go:445-460` | DeliverTx 中 UpdateSubaccounts |
| **transfer** | 子账户间转账 | `transfer.go:51` | DeliverTx 处理 MsgCreateTransfer |
| **transfer** | 充值 (存款) | `transfer.go:116` | DeliverTx 处理 MsgDepositToSubaccount |
| **transfer** | 提款 | `transfer.go:180` | DeliverTx 处理 MsgWithdrawFromSubaccount |
| **funding_values** | Premium Samples | `perpetual.go:405` | EndBlocker 添加 Premium Samples |
| **funding_values** | 资金费率更新 | `perpetual.go:786` | EndBlocker 计算资金费率 |
| **stateful_order** | 长期订单放置 | `msg_server_place_order.go:143-152` | DeliverTx 处理 MsgPlaceOrder (Long-Term) |
| **stateful_order** | 条件订单放置 | `msg_server_place_order.go:117-126` | DeliverTx 处理 MsgPlaceOrder (Conditional) |
| **stateful_order** | TWAP 订单放置 | `msg_server_place_order.go:131-141` | DeliverTx 处理 MsgPlaceOrder (TWAP) |
| **stateful_order** | 订单取消/移除 | `msg_server_cancel_orders.go:106-116` | DeliverTx 处理 MsgCancelOrder |
| **stateful_order** | 订单移除 (Operations) | `process_operations.go:445-457` | DeliverTx 处理 OrderRemoval |
| **deleveraging** | 去杠杆事件 | `process_operations.go:855-870` | DeliverTx 处理 MatchPerpetualDeleveraging |
| **perpetual_market** | 永续市场创建 | `perpetual_market_create.go:11-39` | DeliverTx 创建永续市场 |
| **liquidity_tier** | 流动性层级变更 | `liquidity_tier.go` | DeliverTx 修改流动性配置 |
| **market** | 市场价格更新 | `market.go` | 市场参数变更 |
| **trading_reward** | 交易奖励 | `rewards/keeper/keeper.go` | 奖励分发 |
| **register_affiliate** | 联盟注册 | `affiliates/keeper/keeper.go` | 联盟商注册 |
| **upsert_vault** | Vault 操作 | `vault/keeper/params.go` | Vault 创建/更新 |

#### 核心代码示例

**订单成交事件** (`process_operations.go:552-572`):
```go
k.GetIndexerEventManager().AddTxnEvent(
    ctx,
    indexerevents.SubtypeOrderFill,
    indexerevents.OrderFillEventVersion,
    indexer_manager.GetBytes(
        indexerevents.NewOrderFillEvent(
            matchWithOrders.MakerOrder.MustGetOrder(),
            matchWithOrders.TakerOrder.MustGetOrder(),
            matchWithOrders.FillAmount,
            matchWithOrders.MakerFee,
            matchWithOrders.TakerFee,
            // ...
        ),
    ),
)
```

**子账户更新事件** (`subaccount.go:445-460`):
```go
k.GetIndexerEventManager().AddTxnEvent(
    ctx,
    indexerevents.SubtypeSubaccountUpdate,
    indexerevents.SubaccountUpdateEventVersion,
    indexer_manager.GetBytes(
        indexerevents.NewSubaccountUpdateEvent(
            u.SettledSubaccount.Id,
            salib.GetUpdatedPerpetualPositions(u, fundingPayments),
            salib.GetUpdatedAssetPositions(u),
            fundingPayments,
        ),
    ),
)
```

---

### 11.2 Off-chain Updates (链下更新)

Off-chain Updates 通过 `SendOffchainData()` 实时发送到 Kafka `to-vulcan` topic，不等待区块确认。主要用于短期订单的实时状态更新。

#### 更新类型定义 (`protocol/indexer/off_chain_updates/types/`)

| 类型 | 消息结构 | 用途 |
|-----|---------|------|
| **OrderPlaceV1** | `OffChainUpdateV1_OrderPlace` | 订单放置 |
| **OrderRemoveV1** | `OffChainUpdateV1_OrderRemove` | 订单移除 |
| **OrderUpdateV1** | `OffChainUpdateV1_OrderUpdate` | 订单更新 (部分成交) |
| **OrderReplaceV1** | `OffChainUpdateV1_OrderReplace` | 订单替换 |

#### 详细触发场景

| 更新类型 | 触发场景 | 代码位置 | 触发时机 |
|---------|---------|---------|---------|
| **OrderPlace** | 短期订单放置成功 | `memclob.go:559` | CheckTx 订单进入订单簿 |
| **OrderPlace** | 订单替换时移除旧订单后新增 | `memclob.go:552` | CheckTx 替换操作 |
| **OrderUpdate** | 订单部分成交 (Taker) | `memclob.go:655, 710` | CheckTx/匹配过程 |
| **OrderUpdate** | 订单部分成交 (Maker) | `memclob.go:2072` | 匹配过程中 Maker 被成交 |
| **OrderRemove** | 订单取消成功 | `memclob.go:152` | CheckTx 取消订单 |
| **OrderRemove** | 订单替换时移除旧订单 | `memclob.go:552` | CheckTx 替换操作 |
| **OrderRemove** | 订单完全成交 | `memclob.go:674` | 匹配完成后移除 |
| **OrderRemove** | Post-Only 订单被拒绝 | `memclob.go:595` | 会交叉对手盘时移除 |
| **OrderRemove** | IOC/FOK 订单未完全成交 | `memclob.go:617` | 立即执行条件不满足 |
| **OrderRemove** | Reduce-Only 订单失效 | `memclob.go:2204` | 仓位变化导致 RO 无效 |
| **OrderRemove** | Replay 失败时移除 | `memclob.go:1205, 1339, 1359` | 订单重放失败 |

#### 核心代码流程

**订单放置流程** (`orders.go:175-236` + `memclob.go:504-710`):

```go
// 1. keeper/orders.go - PlaceShortTermOrder
func (k Keeper) PlaceShortTermOrder(ctx sdk.Context, msg *types.MsgPlaceOrder) (...) {
    // 调用 MemClob.PlaceOrder
    orderSizeOptimisticallyFilledFromMatchingQuantums, orderStatus, offchainUpdates, err := k.MemClob.PlaceOrder(
        ctx,
        msg.Order,
    )
    // 发送 off-chain updates
    k.sendOffchainMessagesWithTxHash(
        offchainUpdates,
        tmhash.Sum(ctx.TxBytes()),
        metrics.SendPlaceOrderOffchainUpdates,
    )
    // ...
}

// 2. memclob/memclob.go - PlaceOrder
func (m *MemClobPriceTimePriority) PlaceOrder(...) (..., offchainUpdates *types.OffchainUpdates, ...) {
    offchainUpdates = types.NewOffchainUpdates()

    // 生成订单放置消息
    if m.generateOffchainUpdates {
        if message, success := off_chain_updates.CreateOrderPlaceMessage(ctx, order); success {
            offchainUpdates.AddPlaceMessage(order.OrderId, message)
        }
    }

    // 执行匹配
    takerOrderStatus, takerOffchainUpdates, _, err := m.matchOrder(ctx, &order)
    offchainUpdates.Append(takerOffchainUpdates)

    // 根据订单状态生成不同的 off-chain 消息
    if order was fully filled {
        // 生成 OrderUpdate (最终成交量)
        offchainUpdates.AddUpdateMessage(order.OrderId, message)
    } else if order rests on book {
        // 保持 OrderPlace 消息
    } else if post-only crosses {
        // 生成 OrderRemove
        offchainUpdates.AddRemoveMessage(order.OrderId, message)
    }
    // ...
}
```

**订单取消流程** (`orders.go:133-164` + `memclob.go:107-153`):

```go
// 1. keeper/orders.go - CancelShortTermOrder
func (k Keeper) CancelShortTermOrder(ctx sdk.Context, msgCancelOrder *types.MsgCancelOrder) error {
    // 调用 MemClob.CancelOrder
    offchainUpdates, err := k.MemClob.CancelOrder(ctx, msgCancelOrder)
    // 发送 off-chain updates
    k.sendOffchainMessagesWithTxHash(
        offchainUpdates,
        tmhash.Sum(ctx.TxBytes()),
        metrics.SendCancelOrderOffchainUpdates,
    )
    return nil
}

// 2. memclob/memclob.go - CancelOrder
func (m *MemClobPriceTimePriority) CancelOrder(...) (*types.OffchainUpdates, error) {
    offchainUpdates = types.NewOffchainUpdates()

    if m.generateOffchainUpdates {
        if message, success := off_chain_updates.CreateOrderRemoveMessageWithReason(
            ctx,
            orderIdToCancel,
            indexershared.OrderRemovalReason_ORDER_REMOVAL_REASON_USER_CANCELED,
            ocutypes.OrderRemoveV1_ORDER_REMOVAL_STATUS_BEST_EFFORT_CANCELED,
        ); success {
            offchainUpdates.AddRemoveMessage(orderIdToCancel, message)
        }
    }
    return offchainUpdates, nil
}
```

---

### 11.3 对比总结

| 维度 | On-chain Events | Off-chain Updates |
|-----|-----------------|-------------------|
| **数据类型** | 15 种事件类型 | 4 种更新类型 |
| **触发时机** | DeliverTx / EndBlocker | CheckTx / 内存匹配 |
| **发送方式** | 区块批量发送 (ProduceBlock) | 实时单条发送 |
| **Kafka Topic** | `to-ender` | `to-vulcan` |
| **处理服务** | Ender | Vulcan |
| **存储目标** | PostgreSQL | Redis |
| **延迟** | 区块时间 (~1-2s) | 毫秒级 |
| **数据特点** | 最终确定、持久化 | 乐观更新、可能回滚 |

#### 数据分类详情

**On-chain Events 包含**:
```
├── 交易相关
│   ├── order_fill          - 订单成交 (最终确认)
│   └── deleveraging        - 去杠杆事件
├── 账户相关
│   ├── subaccount_update   - 仓位/余额变化
│   └── transfer            - 充值/提款/转账
├── 有状态订单
│   └── stateful_order      - 长期/条件/TWAP 订单放置/取消/移除
├── 市场配置
│   ├── perpetual_market    - 永续市场创建
│   ├── liquidity_tier      - 流动性层级
│   ├── market              - 市场参数
│   ├── update_perpetual    - 永续更新
│   └── update_clob_pair    - CLOB 对更新
├── 费率相关
│   └── funding_values      - 资金费率
├── 其他
│   ├── asset               - 资产配置
│   ├── trading_reward      - 交易奖励
│   ├── register_affiliate  - 联盟注册
│   └── upsert_vault        - Vault 操作
```

**Off-chain Updates 包含**:
```
├── 订单生命周期
│   ├── OrderPlace    - 短期订单放置 (乐观)
│   ├── OrderUpdate   - 订单部分成交
│   ├── OrderRemove   - 订单移除/取消/失效
│   └── OrderReplace  - 订单替换
```

---

### 11.4 设计哲学

1. **最终一致性 vs 实时性**:
   - On-chain Events 保证最终一致性，但延迟较高 (区块时间)
   - Off-chain Updates 提供实时响应，但可能因区块回滚而需要修正

2. **数据分层**:
   - 持久化数据 (仓位、成交记录) → On-chain → PostgreSQL
   - 实时状态 (订单簿、活跃订单) → Off-chain → Redis

3. **乐观更新**:
   - 短期订单使用 `BEST_EFFORT_OPENED` 状态
   - 取消使用 `BEST_EFFORT_CANCELED` 状态
   - 表示该状态是乐观的，可能被后续链上确认覆盖

4. **消息 Key 设计**:
   - Off-chain Updates 使用 `OrderIdHash` 作为 Kafka 消息 Key
   - 确保同一订单的所有更新发送到同一分区，保证顺序性

---

## 12. 存储层详细分析: On-chain Events vs Off-chain Updates

> 解答关键问题: On-chain Events 和 Off-chain Updates 最终都会存储到 PostgreSQL 吗？

### 12.1 核心结论

| 数据通道 | 处理服务 | 存储目标 | 是否写入 PostgreSQL |
|---------|---------|---------|-------------------|
| **On-chain Events** | Ender | PostgreSQL + Redis (部分) | ✅ **是** |
| **Off-chain Updates** | Vulcan | Redis 仅缓存 | ❌ **否** |

**关键发现**:
- **Off-chain Updates (Vulcan) 只写入 Redis，不写入 PostgreSQL**
- **On-chain Events (Ender) 写入 PostgreSQL，同时可能更新部分 Redis 缓存**
- PostgreSQL 中的数据是链上确认后的最终状态，Redis 中的数据是乐观的实时状态

### 12.2 数据流存储路径详解

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                            存储层数据流详解                                           │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   On-chain Events (链上事件)                                                         │
│   ════════════════════════                                                          │
│                                                                                      │
│   Full Node                                                                          │
│       │                                                                              │
│       │ 1. DeliverTx/EndBlocker 生成事件                                            │
│       ▼                                                                              │
│   Kafka (to-ender)                                                                   │
│       │                                                                              │
│       │ 2. Ender 服务消费                                                            │
│       ▼                                                                              │
│   ┌─────────────────────────────────────────────────────────────────┐               │
│   │                      Ender 服务                                  │               │
│   │                                                                  │               │
│   │   OrderFillHandler ──────┬──► PostgreSQL (orders, fills)        │               │
│   │                          │                                       │               │
│   │                          └──► Redis (StateFilledQuantumsCache)  │               │
│   │                                                                  │               │
│   │   SubaccountUpdateHandler ──► PostgreSQL (perpetual_positions,  │               │
│   │                               asset_positions, subaccounts)     │               │
│   │                                                                  │               │
│   │   StatefulOrderHandler ──► PostgreSQL (orders)                  │               │
│   │                                                                  │               │
│   │   FundingHandler ──► PostgreSQL (funding_index_updates)         │               │
│   │                                                                  │               │
│   │   TransferHandler ──► PostgreSQL (transfers)                    │               │
│   │                                                                  │               │
│   │   CandleHandler ──► PostgreSQL (candles)                        │               │
│   └─────────────────────────────────────────────────────────────────┘               │
│                                                                                      │
│   ──────────────────────────────────────────────────────────────────────────────    │
│                                                                                      │
│   Off-chain Updates (链下更新)                                                       │
│   ════════════════════════════                                                      │
│                                                                                      │
│   Full Node                                                                          │
│       │                                                                              │
│       │ 1. CheckTx 时生成订单更新                                                    │
│       ▼                                                                              │
│   Kafka (to-vulcan)                                                                  │
│       │                                                                              │
│       │ 2. Vulcan 服务消费                                                           │
│       ▼                                                                              │
│   ┌─────────────────────────────────────────────────────────────────┐               │
│   │                      Vulcan 服务                                 │               │
│   │                                                                  │               │
│   │   OrderPlaceHandler ──┬──► Redis (OrdersCache)                  │               │
│   │                       │                                          │               │
│   │                       ├──► Redis (OrderbookLevelsCache)         │               │
│   │                       │                                          │               │
│   │                       └──► Redis (SubaccountOrderIdsCache)      │               │
│   │                                                                  │               │
│   │   OrderUpdateHandler ──► Redis (OrdersCache - 更新成交量)       │               │
│   │                                                                  │               │
│   │   OrderRemoveHandler ──► Redis (移除订单/更新订单簿)             │               │
│   │                                                                  │               │
│   │   ⚠️ 注意: Vulcan 不写入任何 PostgreSQL 表                       │               │
│   │                                                                  │               │
│   └─────────────────────────────────────────────────────────────────┘               │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### 12.3 代码证据

#### Vulcan 只写 Redis (不写 PostgreSQL)

**代码位置**: `indexer/services/vulcan/src/handlers/order-place-handler.ts`

```typescript
// vulcan/src/handlers/order-place-handler.ts:200-250
export async function handleOrderPlace(update: OffChainUpdateV1): Promise<void> {
  // ...

  // 只写入 Redis 缓存
  const placeOrderResult: PlaceOrderResult = await placeOrder({
    redisOrder: order,
    client: redisClient,
  });

  // placeOrder 内部调用:
  // - OrdersCache.setOrder() → Redis
  // - OrderbookLevelsCache.updatePriceLevel() → Redis
  // - SubaccountOrderIdsCache.addOrderId() → Redis

  // ❌ 没有任何 PostgreSQL 写入操作
}
```

**Redis 写入实现** (`indexer/packages/redis/src/caches/`):

```typescript
// orders-cache.ts
export async function setOrder(orderId: string, order: RedisOrder, client: RedisClient) {
  await client.set(`v4/orders/${orderId}`, order.toBuffer());
}

// orderbook-levels-cache.ts
export async function updatePriceLevel(
  ticker: string,
  side: OrderSide,
  price: string,
  sizeDelta: string,
  client: RedisClient,
) {
  // 使用 Lua 脚本原子更新
  await incrementOrderbookLevel(ticker, side, price, sizeDelta, client);
}
```

#### Ender 写入 PostgreSQL

**代码位置**: `indexer/services/ender/src/handlers/order-fills/order-handler.ts`

```typescript
// ender/src/handlers/order-fills/order-handler.ts:150-200
export async function handleOrderFill(event: OrderFillEvent): Promise<void> {
  // 1. 写入 fills 表 (PostgreSQL)
  await FillTable.create(fill, { txId });

  // 2. 更新 orders 表 (PostgreSQL)
  await OrderTable.update({
    id: orderId,
    totalFilled: newTotalFilled,
    status: newStatus,
  }, { txId });

  // 3. 更新仓位表 (PostgreSQL)
  await PerpetualPositionTable.upsert(position, { txId });

  // 4. 同时更新 Redis 缓存 (用于数据一致性)
  await StateFilledQuantumsCache.updateStateFilledQuantums(
    orderId,
    filledQuantums,
    redisClient,
  );

  // 5. 通知 Vulcan 更新 Redis 订单状态
  // 发送消息到 Kafka，Vulcan 会更新 Redis
}
```

### 12.4 PostgreSQL 表结构设计

#### 核心表分类

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                          PostgreSQL 表结构分类                                        │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  交易核心表 (Trading Core)                                                    │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  orders                │ 订单记录                │ stateful_order, order_fill│  │
│   │  fills                 │ 成交记录                │ order_fill                │  │
│   │  candles               │ K线数据                 │ order_fill (聚合计算)     │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  账户仓位表 (Account & Position)                                              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  subaccounts           │ 子账户信息              │ subaccount_update         │  │
│   │  perpetual_positions   │ 永续合约仓位            │ subaccount_update         │  │
│   │  asset_positions       │ 资产仓位 (保证金余额)   │ subaccount_update         │  │
│   │  pnl_ticks             │ 盈亏快照                │ 定时计算                  │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  资金与转账表 (Funding & Transfer)                                            │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  funding_index_updates │ 资金费率历史            │ funding_values            │  │
│   │  transfers             │ 转账记录                │ transfer                  │  │
│   │  trading_rewards       │ 交易奖励                │ trading_reward            │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  市场配置表 (Market Configuration)                                            │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源事件              │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  perpetual_markets     │ 永续市场配置            │ perpetual_market          │  │
│   │  liquidity_tiers       │ 流动性层级              │ liquidity_tier            │  │
│   │  assets                │ 资产配置                │ asset                     │  │
│   │  markets               │ 市场参数                │ market                    │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  │
│   │  区块与事件表 (Block & Events)                                                │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  表名                   │ 用途                    │ 数据来源                  │  │
│   ├──────────────────────────────────────────────────────────────────────────────┤  │
│   │  blocks                │ 区块信息                │ 每个区块                  │  │
│   │  tendermint_events     │ Tendermint 事件        │ 每个交易                  │  │
│   │  transactions          │ 交易记录                │ 每个交易                  │  │
│   └──────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

#### 详细表结构

##### 1. orders 表 (订单记录)

```sql
CREATE TABLE orders (
    id UUID PRIMARY KEY,
    subaccount_id UUID NOT NULL,          -- 关联 subaccounts 表
    client_id TEXT NOT NULL,              -- 客户端订单 ID
    clob_pair_id INTEGER NOT NULL,        -- 交易对 ID
    side TEXT NOT NULL,                   -- 'BUY' / 'SELL'
    size TEXT NOT NULL,                   -- 订单数量 (quantums)
    total_filled TEXT NOT NULL DEFAULT '0', -- 已成交数量
    price TEXT NOT NULL,                  -- 订单价格 (subticks)
    type TEXT NOT NULL,                   -- 'LIMIT' / 'MARKET' / 'STOP_LIMIT' 等
    status TEXT NOT NULL,                 -- 'OPEN' / 'FILLED' / 'CANCELED' / 'BEST_EFFORT_CANCELED'
    time_in_force TEXT NOT NULL,          -- 'GTT' / 'IOC' / 'FOK' / 'POST_ONLY'
    reduce_only BOOLEAN NOT NULL DEFAULT FALSE,
    order_flags INTEGER NOT NULL,         -- 订单类型标志 (Short-Term/Long-Term/Conditional)
    good_til_block INTEGER,               -- 短期订单过期区块
    good_til_block_time TIMESTAMP,        -- 长期订单过期时间
    created_at_height TEXT NOT NULL,      -- 创建区块高度
    client_metadata INTEGER DEFAULT 0,
    trigger_price TEXT,                   -- 条件订单触发价
    updated_at TIMESTAMP NOT NULL,
    updated_at_height TEXT NOT NULL,

    -- 索引
    INDEX idx_orders_subaccount_status (subaccount_id, status),
    INDEX idx_orders_clob_pair_status (clob_pair_id, status),
    INDEX idx_orders_good_til_block (good_til_block) WHERE good_til_block IS NOT NULL
);
```

##### 2. fills 表 (成交记录)

```sql
CREATE TABLE fills (
    id UUID PRIMARY KEY,
    subaccount_id UUID NOT NULL,          -- 关联 subaccounts 表
    side TEXT NOT NULL,                   -- 'BUY' / 'SELL'
    liquidity TEXT NOT NULL,              -- 'MAKER' / 'TAKER'
    type TEXT NOT NULL,                   -- 'LIMIT' / 'LIQUIDATED' / 'LIQUIDATION' / 'DELEVERAGED' / 'OFFSETTING'
    clob_pair_id INTEGER NOT NULL,        -- 交易对 ID
    order_id UUID NOT NULL,               -- 关联 orders 表
    size TEXT NOT NULL,                   -- 成交数量
    price TEXT NOT NULL,                  -- 成交价格
    quote_amount TEXT NOT NULL,           -- 成交金额
    fee TEXT NOT NULL,                    -- 手续费
    affiliate_rev_share TEXT,             -- 联盟分成
    transaction_hash TEXT NOT NULL,       -- 交易哈希
    created_at TIMESTAMP NOT NULL,
    created_at_height TEXT NOT NULL,
    event_id BYTEA NOT NULL,              -- 关联 tendermint_events

    -- 索引
    INDEX idx_fills_subaccount (subaccount_id, created_at DESC),
    INDEX idx_fills_order (order_id),
    INDEX idx_fills_clob_pair (clob_pair_id, created_at DESC)
);
```

##### 3. perpetual_positions 表 (永续仓位)

```sql
CREATE TABLE perpetual_positions (
    id UUID PRIMARY KEY,
    subaccount_id UUID NOT NULL,          -- 关联 subaccounts 表
    perpetual_id INTEGER NOT NULL,        -- 永续合约 ID
    side TEXT NOT NULL,                   -- 'LONG' / 'SHORT'
    status TEXT NOT NULL,                 -- 'OPEN' / 'CLOSED'
    size TEXT NOT NULL,                   -- 仓位大小
    max_size TEXT NOT NULL,               -- 历史最大仓位
    entry_price TEXT,                     -- 入场价格
    exit_price TEXT,                      -- 出场价格
    sum_open TEXT NOT NULL,               -- 累计开仓
    sum_close TEXT NOT NULL,              -- 累计平仓
    created_at TIMESTAMP NOT NULL,
    closed_at TIMESTAMP,
    created_at_height TEXT NOT NULL,
    closed_at_height TEXT,
    settled_funding TEXT NOT NULL,        -- 已结算资金费

    -- 索引
    INDEX idx_positions_subaccount (subaccount_id, status),
    INDEX idx_positions_perpetual (perpetual_id, status)
);
```

##### 4. candles 表 (K线数据)

```sql
CREATE TABLE candles (
    id UUID PRIMARY KEY,
    started_at TIMESTAMP NOT NULL,        -- K线开始时间
    ticker TEXT NOT NULL,                 -- 交易对代码 (如 'BTC-USD')
    resolution TEXT NOT NULL,             -- 时间周期: '1MIN' / '5MINS' / '15MINS' / '30MINS' / '1HOUR' / '4HOURS' / '1DAY'
    low TEXT NOT NULL,                    -- 最低价
    high TEXT NOT NULL,                   -- 最高价
    open TEXT NOT NULL,                   -- 开盘价
    close TEXT NOT NULL,                  -- 收盘价
    base_token_volume TEXT NOT NULL,      -- 基础代币成交量
    usd_volume TEXT NOT NULL,             -- USD 成交额
    trades INTEGER NOT NULL,              -- 成交笔数
    starting_open_interest TEXT NOT NULL, -- 开始时持仓量

    -- 复合主键
    UNIQUE (ticker, resolution, started_at),

    -- 索引
    INDEX idx_candles_ticker_time (ticker, resolution, started_at DESC)
);
```

##### 5. funding_index_updates 表 (资金费率)

```sql
CREATE TABLE funding_index_updates (
    id UUID PRIMARY KEY,
    perpetual_id INTEGER NOT NULL,        -- 永续合约 ID
    event_id BYTEA NOT NULL,              -- 关联 tendermint_events
    rate TEXT NOT NULL,                   -- 资金费率 (如 "0.0001" 表示 0.01%)
    oracle_price TEXT NOT NULL,           -- 预言机价格
    funding_index TEXT NOT NULL,          -- 累计资金指数
    effective_at TIMESTAMP NOT NULL,      -- 生效时间
    effective_at_height TEXT NOT NULL,    -- 生效区块高度

    -- 索引
    INDEX idx_funding_perpetual_time (perpetual_id, effective_at DESC)
);
```

##### 6. subaccounts 表 (子账户)

```sql
CREATE TABLE subaccounts (
    id UUID PRIMARY KEY,
    address TEXT NOT NULL,                -- 钱包地址
    subaccount_number INTEGER NOT NULL,   -- 子账户编号 (0-127)
    updated_at TIMESTAMP NOT NULL,
    updated_at_height TEXT NOT NULL,

    -- 唯一约束
    UNIQUE (address, subaccount_number),

    -- 索引
    INDEX idx_subaccounts_address (address)
);
```

##### 7. asset_positions 表 (资产仓位/保证金)

```sql
CREATE TABLE asset_positions (
    id UUID PRIMARY KEY,
    subaccount_id UUID NOT NULL,          -- 关联 subaccounts 表
    asset_id INTEGER NOT NULL,            -- 资产 ID (通常 0 = USDC)
    size TEXT NOT NULL,                   -- 余额大小
    is_long BOOLEAN NOT NULL DEFAULT TRUE, -- 多头/空头 (USDC 通常为 true)

    -- 唯一约束
    UNIQUE (subaccount_id, asset_id)
);
```

##### 8. transfers 表 (转账记录)

```sql
CREATE TABLE transfers (
    id UUID PRIMARY KEY,
    sender_subaccount_id UUID,            -- 发送方子账户 (可为空)
    recipient_subaccount_id UUID,         -- 接收方子账户 (可为空)
    sender_wallet_address TEXT,           -- 发送方钱包 (可为空)
    recipient_wallet_address TEXT,        -- 接收方钱包 (可为空)
    asset_id INTEGER NOT NULL,            -- 资产 ID
    size TEXT NOT NULL,                   -- 转账金额
    event_id BYTEA NOT NULL,              -- 关联 tendermint_events
    transaction_hash TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    created_at_height TEXT NOT NULL,

    -- 索引
    INDEX idx_transfers_sender (sender_subaccount_id, created_at DESC),
    INDEX idx_transfers_recipient (recipient_subaccount_id, created_at DESC)
);
```

##### 9. blocks 表 (区块信息)

```sql
CREATE TABLE blocks (
    block_height TEXT PRIMARY KEY,        -- 区块高度 (字符串以支持大数)
    time TIMESTAMP NOT NULL,              -- 区块时间

    -- 索引
    INDEX idx_blocks_time (time DESC)
);
```

### 12.5 Redis 缓存结构详解

| 缓存名称 | Redis Key 格式 | 数据类型 | 用途 | 写入来源 |
|---------|---------------|---------|------|---------|
| **OrdersCache** | `v4/orders/{orderId}` | STRING | 订单完整数据 | Vulcan |
| **OrderbookLevelsCache** | `v4/orderbookLevels/{ticker}/{side}` | HSET | 价格层级聚合 | Vulcan |
| **SubaccountOrderIdsCache** | `v4/subaccountOrderIds/{subaccountId}` | SET | 账户订单映射 | Vulcan |
| **OrdersDataCache** | `v4/ordersData/{orderId}` | STRING | 订单元数据 | Vulcan |
| **CanceledOrdersCache** | `v4/canceledOrders/{orderId}` | STRING | 已取消订单 | Vulcan |
| **StateFilledQuantumsCache** | `v4/stateFilledQuantums/{orderId}` | STRING | 链上确认成交量 | Ender |

### 12.6 数据一致性设计

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                           数据一致性保证机制                                          │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   时间线:                                                                            │
│   ───────                                                                            │
│                                                                                      │
│   T0: 用户提交订单                                                                   │
│        │                                                                             │
│   T1: CheckTx 验证通过                                                               │
│        │                                                                             │
│        ├──► Off-chain: OrderPlace (BEST_EFFORT_OPENED)                              │
│        │    └──► Vulcan ──► Redis (OrdersCache, OrderbookLevelsCache)               │
│        │                                                                             │
│   T2: 交易进入区块                                                                   │
│        │                                                                             │
│   T3: DeliverTx 执行                                                                 │
│        │                                                                             │
│        ├──► On-chain: OrderFill (如果成交)                                          │
│        │    └──► Ender ──► PostgreSQL (orders, fills)                               │
│        │                   Redis (StateFilledQuantumsCache)                         │
│        │                                                                             │
│        └──► On-chain: StatefulOrder (如果是有状态订单)                              │
│             └──► Ender ──► PostgreSQL (orders)                                      │
│                                                                                      │
│   T4: Ender 通知 Vulcan 更新 Redis                                                   │
│        │                                                                             │
│        └──► Vulcan ──► Redis (更新订单状态为 FILLED/CANCELED)                       │
│                                                                                      │
│   ──────────────────────────────────────────────────────────────────────────────    │
│                                                                                      │
│   冲突解决策略:                                                                       │
│   ═════════════                                                                      │
│                                                                                      │
│   1. PostgreSQL 数据权威性 > Redis                                                   │
│      - 发生冲突时以链上确认的 PostgreSQL 数据为准                                    │
│                                                                                      │
│   2. StateFilledQuantumsCache 作为桥梁                                               │
│      - Ender 写入 Redis 中已确认的成交量                                             │
│      - Vulcan 读取此缓存判断订单实际状态                                             │
│                                                                                      │
│   3. 乐观状态回滚                                                                    │
│      - 如果链上拒绝订单，Ender 会发送 OrderRemove 事件                               │
│      - Vulcan 收到后从 Redis 中移除该订单                                            │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### 12.7 设计原理总结

| 设计决策 | 原因 |
|---------|------|
| **Off-chain 不写 PostgreSQL** | 短期订单生命周期极短 (单区块)，写入 PostgreSQL 会造成大量无效 I/O；Redis 内存操作延迟低，适合高频更新 |
| **On-chain 写 PostgreSQL** | 链上确认的数据是最终状态，需要持久化存储用于历史查询、审计和恢复 |
| **Redis 作为实时层** | 订单簿需要毫秒级响应，PostgreSQL 无法满足；Redis 支持复杂数据结构 (HSET 用于价格层级) |
| **双缓存同步** | Ender 更新 StateFilledQuantumsCache 通知 Vulcan 实际链上状态，实现最终一致性 |
| **PostgreSQL 读写分离** | 查询操作使用只读副本，减轻主库压力 |

---

## 参考资料

- DYDX v4 Chain 源码: `dydx-v4-chain/`
- Indexer 服务: `dydx-v4-chain/indexer/`
- Protocol Indexer: `dydx-v4-chain/protocol/indexer/`
- 核心代码文件:
  - `protocol/indexer/events/constants.go` - 事件类型定义
  - `protocol/indexer/off_chain_updates/off_chain_updates.go` - 链下更新消息创建
  - `protocol/x/clob/keeper/process_operations.go` - 订单匹配和链上事件
  - `protocol/x/clob/keeper/orders.go` - 订单操作和链下更新发送
  - `protocol/x/clob/memclob/memclob.go` - 内存订单簿和链下更新生成
  - `protocol/x/subaccounts/keeper/subaccount.go` - 子账户更新事件
