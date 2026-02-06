# DYDX v4 Indexer 机制详细分析

> 基于 dydx-v4-chain 源码分析，涵盖数据索引方式、高性能设计、客户端连接方式和完整数据流。

## 1. 整体架构概览

### 1.1 系统架构全景图

```
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                              DYDX v4 Indexer 完整架构                                         │
├─────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────────────────────┐   │
│  │                              客户端层 (Client Layer)                                  │   │
│  │                                                                                       │   │
│  │   ┌───────────┐   ┌───────────┐   ┌───────────┐   ┌───────────┐   ┌───────────┐    │   │
│  │   │  交易前端  │   │ 做市机器人 │   │  API 用户  │   │  数据分析  │   │ 第三方服务 │    │   │
│  │   └─────┬─────┘   └─────┬─────┘   └─────┬─────┘   └─────┬─────┘   └─────┬─────┘    │   │
│  │         │               │               │               │               │           │   │
│  │         └───────────────┴───────┬───────┴───────────────┴───────────────┘           │   │
│  │                                 │                                                    │   │
│  │                    下单/取消    │    查询(REST) / 订阅(WebSocket)                     │   │
│  │                                 │                                                    │   │
│  └─────────────────────────────────┼────────────────────────────────────────────────────┘   │
│                                    │                                                        │
│          ┌─────────────────────────┼─────────────────────────┐                             │
│          │                         │                         │                             │
│          ▼                         ▼                         ▼                             │
│  ┌───────────────┐        ┌───────────────┐        ┌───────────────┐                      │
│  │   验证器节点   │        │   Comlink     │        │    Socks      │                      │
│  │  (Validator)  │        │  (REST API)   │        │  (WebSocket)  │                      │
│  │               │        │               │        │               │                      │
│  │  • 接收交易   │        │  • 历史查询   │        │  • 实时推送   │                      │
│  │  • 参与共识   │        │  • 订单查询   │        │  • 订单簿更新 │                      │
│  │  • 出块       │        │  • 仓位查询   │        │  • 成交更新   │                      │
│  └───────┬───────┘        └───────┬───────┘        └───────┬───────┘                      │
│          │                        │                        │                               │
│          │ CometBFT P2P           │ PostgreSQL + Redis     │ Kafka (to-websockets-*)      │
│          ▼                        ▼                        ▼                               │
│  ════════════════════════════════════════════════════════════════════════════════════     │
│                                                                                            │
│  ┌─────────────────────────────────────────────────────────────────────────────────────┐  │
│  │                         Indexer 专用全节点 (Full Node)                               │  │
│  │                                                                                      │  │
│  │   --indexer-kafka-conn-str=kafka:9092  --indexer-send-offchain-data=true           │  │
│  │                                                                                      │  │
│  │   ┌─────────────────────────────────────────────────────────────────────────────┐  │  │
│  │   │                           ABCI 应用层                                        │  │  │
│  │   │                                                                              │  │  │
│  │   │   ┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐    │  │  │
│  │   │   │   CheckTx 阶段  │      │  DeliverTx 阶段 │      │ EndBlocker 阶段 │    │  │  │
│  │   │   │                 │      │                 │      │                 │    │  │  │
│  │   │   │ • 短期订单验证  │      │ • 有状态订单处理│      │ • 资金费率计算  │    │  │  │
│  │   │   │ • 订单簿更新    │      │ • 订单匹配执行  │      │ • 收集区块事件  │    │  │  │
│  │   │   │                 │      │ • 仓位更新      │      │ • ProduceBlock  │    │  │  │
│  │   │   │   MemClob       │      │ • 转账处理      │      │                 │    │  │  │
│  │   │   └────────┬────────┘      └────────┬────────┘      └────────┬────────┘    │  │  │
│  │   │            │                        │                        │             │  │  │
│  │   │            │ Off-chain Updates      │ On-chain Events        │             │  │  │
│  │   │            │ (乐观状态,可回滚)       │ (最终确定状态)          │             │  │  │
│  │   │            │                        │                        │             │  │  │
│  │   └────────────┼────────────────────────┼────────────────────────┼─────────────┘  │  │
│  │                │                        │                        │                │  │
│  │                ▼                        └────────────┬───────────┘                │  │
│  │   ┌────────────────────┐                            │                             │  │
│  │   │ SendOffchainData() │                            ▼                             │  │
│  │   │ (实时,毫秒级)       │               ┌────────────────────┐                    │  │
│  │   └─────────┬──────────┘               │ SendOnchainData()  │                    │  │
│  │             │                          │ (区块级,1-2秒)      │                    │  │
│  │             │                          └─────────┬──────────┘                    │  │
│  │             │                                    │                               │  │
│  └─────────────┼────────────────────────────────────┼───────────────────────────────┘  │
│                │                                    │                                   │
│                ▼                                    ▼                                   │
│  ┌─────────────────────────────────────────────────────────────────────────────────────┐│
│  │                                Kafka 消息队列                                        ││
│  │                                                                                      ││
│  │  ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────────┐  ││
│  │  │      to-vulcan       │  │      to-ender        │  │   to-websockets-*        │  ││
│  │  │   (链下订单更新)      │  │   (链上区块事件)      │  │   (WebSocket 推送)       │  ││
│  │  │                      │  │                      │  │                          │  ││
│  │  │  • OrderPlaceV1      │  │  • OrderFillEvent    │  │  • orderbooks            │  ││
│  │  │  • OrderUpdateV1     │  │  • SubaccountUpdate  │  │  • trades                │  ││
│  │  │  • OrderRemoveV1     │  │  • TransferEvent     │  │  • subaccounts           │  ││
│  │  │  • OrderReplaceV1    │  │  • FundingEvent      │  │  • candles               │  ││
│  │  │                      │  │  • StatefulOrder     │  │  • markets               │  ││
│  │  │  延迟: 毫秒级        │  │  延迟: ~1-2秒        │  │  • block-height          │  ││
│  │  │  状态: 乐观          │  │  状态: 最终确定      │  │                          │  ││
│  │  └──────────┬───────────┘  └──────────┬───────────┘  └────────────┬─────────────┘  ││
│  │             │                         │                           │                ││
│  └─────────────┼─────────────────────────┼───────────────────────────┼────────────────┘│
│                │                         │                           │                  │
│                ▼                         ▼                           │                  │
│  ┌─────────────────────────────────────────────────────────────────────────────────────┐│
│  │                           Indexer 服务集群 (统一部署)                                ││
│  │                                                                                      ││
│  │  ┌─────────────────────┐         ┌─────────────────────┐                           ││
│  │  │       Vulcan        │         │        Ender        │                           ││
│  │  │   (链下更新处理)     │         │    (链上事件处理)    │                           ││
│  │  │                     │         │                     │                           ││
│  │  │  • 更新订单状态     │         │  • 解析区块事件     │                           ││
│  │  │  • 更新订单簿层级   │         │  • 写入成交记录     │                           ││
│  │  │  • 乐观填充状态同步 │         │  • 更新订单状态     │        ┌──────────────┐   ││
│  │  │                     │         │  • 更新仓位/余额    │        │  Roundtable  │   ││
│  │  │       │             │         │  • 计算 K线数据     │        │  (定时任务)   │   ││
│  │  │       ▼             │         │                     │        │              │   ││
│  │  │  ┌─────────┐        │         │       │             │        │ • PnL 计算   │   ││
│  │  │  │  Redis  │        │         │       ▼             │        │ • 排行榜     │   ││
│  │  │  └─────────┘        │         │  ┌──────────┐       │        │ • 数据聚合   │   ││
│  │  │   • OrdersCache     │         │  │PostgreSQL│       │        └──────────────┘   ││
│  │  │   • OrderbookLevels │         │  └──────────┘       │                           ││
│  │  │   • SubaccountOrders│         │   • orders          │                           ││
│  │  │                     │         │   • fills           │                           ││
│  │  │       │             │         │   • perpetual_      │                           ││
│  │  │       ▼             │         │     positions       │                           ││
│  │  │  推送到 Kafka       │         │   • candles         │                           ││
│  │  │  to-websockets-*    │─────────│   • funding_index   │                           ││
│  │  │                     │         │   • transfers       │                           ││
│  │  └─────────────────────┘         └─────────────────────┘                           ││
│  │                                                                                      ││
│  └──────────────────────────────────────────────────────────────────────────────────────┘│
│                                                                                          │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

### 1.2 双通道数据流对比

| 特性 | Off-chain Updates (链下更新) | On-chain Events (链上事件) |
|-----|------------------------------|---------------------------|
| **触发阶段** | CheckTx / PrepareProposal | DeliverTx / EndBlocker |
| **数据类型** | 短期订单状态 (放置/取消/更新) | 成交/仓位/转账/资金费率 |
| **Kafka Topic** | to-vulcan | to-ender |
| **处理服务** | Vulcan | Ender |
| **存储目标** | Redis (内存缓存) | PostgreSQL (持久化) |
| **延迟** | 毫秒级 | 区块时间 (~1-2秒) |
| **状态性质** | 乐观状态 (可回滚) | 最终确定状态 |
| **典型用途** | 实时订单簿显示 | 历史查询/审计 |

### 1.3 关键设计特点

1. **双通道分离**: 链上最终状态与链下乐观状态分开处理，兼顾实时性与一致性
2. **Kafka 解耦**: 全节点与 Indexer 服务通过消息队列异步通信，支持削峰填谷
3. **存储分层**: Redis 处理高频读写，PostgreSQL 保障数据持久化
4. **统一部署**: 所有 Indexer 服务共享同一套 Kafka/PostgreSQL/Redis，降低运维复杂度

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

### 6.1 REST API 完整列表与数据来源 (Comlink 服务)

基于对 `dydx-v4-chain/indexer/services/comlink/src/controllers/api/v4/` 目录下所有控制器的分析：

#### 订单相关 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/orders` | **PostgreSQL + Redis** | OrderTable + OrdersCache/SubaccountOrderIdsCache 合并 |
| `GET /v4/orders/parentSubaccountNumber` | **PostgreSQL + Redis** | 同上，父账户视角 |
| `GET /v4/orders/:orderId` | **PostgreSQL + Redis** | OrderTable.findById + OrdersCache.getOrder |

> **关键代码** (`orders-controller.ts`): 先从 Redis 获取活跃订单，再从 PostgreSQL 查询历史订单，最后合并两者数据。PostgreSQL 被视为权威数据源 (source of truth)。

#### 成交相关 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/fills` | **PostgreSQL** | FillTable |
| `GET /v4/fills/parentSubaccountNumber` | **PostgreSQL** | FillTable (父账户视角) |
| `GET /v4/trades/perpetualMarket/:ticker` | **PostgreSQL** | FillTable (过滤 Liquidity.TAKER) |

#### 仓位相关 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/perpetualPositions` | **PostgreSQL** | PerpetualPositionTable + FundingIndexUpdatesTable |
| `GET /v4/perpetualPositions/parentSubaccountNumber` | **PostgreSQL** | 同上 (父账户视角) |
| `GET /v4/assetPositions` | **PostgreSQL** | AssetPositionTable |

#### 账户相关 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/addresses/:address` | **PostgreSQL** | SubaccountTable + 多表联查 |
| `GET /v4/addresses/:address/subaccountNumber/:num` | **PostgreSQL** | 同上 |
| `GET /v4/addresses/:address/parentSubaccountNumber/:num` | **PostgreSQL** | 同上 |

#### 市场数据 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/orderbooks/perpetualMarket/:ticker` | **Redis** | OrderbookLevelsCache (实时订单簿) |
| `GET /v4/perpetualMarkets` | **PostgreSQL + 内存缓存** | PerpetualMarketTable + perpetualMarketRefresher + liquidityTierRefresher |
| `GET /v4/candles/perpetualMarkets/:ticker` | **PostgreSQL** | CandleTable |
| `GET /v4/sparklines` | **PostgreSQL** | CandleTable (聚合计算) |

#### 资金费率 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/historicalFunding/:ticker` | **PostgreSQL** | FundingIndexUpdatesTable |
| `GET /v4/fundingPayments` | **PostgreSQL** | FundingIndexUpdatesTable |

#### 转账相关 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/transfers` | **PostgreSQL** | TransferTable |
| `GET /v4/transfers/parentSubaccountNumber` | **PostgreSQL** | TransferTable |
| `GET /v4/transfers/between` | **PostgreSQL** | TransferTable |

#### 其他 API
| API | 数据来源 | 说明 |
|-----|---------|------|
| `GET /v4/height` | **PostgreSQL** | BlockTable.getLatest() |
| `GET /v4/time` | **内存** | 服务器时间 |
| `GET /v4/historicalPnl` | **PostgreSQL** | PnlTicksTable |
| `GET /v4/historicalBlockTradingRewards/:address` | **PostgreSQL** | TradingRewardAggregationTable |
| `GET /v4/historicalTradingRewardAggregations/:address` | **PostgreSQL** | TradingRewardAggregationTable |
| `GET /v4/vault/megavault/positions` | **PostgreSQL** | VaultTable |
| `GET /v4/affiliates/metadata` | **PostgreSQL** | AffiliateInfoTable |

**订单簿查询示例代码**:
```typescript
// indexer/services/comlink/src/controllers/api/v4/orderbook-controller.ts
async getPerpetualMarket(ticker: string): Promise<OrderbookResponseObject> {
  const perpetualMarket = perpetualMarketRefresher.getPerpetualMarketFromTicker(ticker);

  // 从 Redis 获取订单簿 (实时数据)
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

### 6.2 WebSocket 订阅频道与数据来源 (Socks 服务)

| 频道 | ID 格式 | 初始数据来源 | 增量更新来源 | Kafka Topic |
|-----|--------|------------|------------|-------------|
| `v4_orderbook` | `{ticker}` | Comlink REST → **Redis** | Kafka | `to-websockets-orderbooks` |
| `v4_trades` | `{ticker}` | Comlink REST → **PostgreSQL** | Kafka | `to-websockets-trades` |
| `v4_candles` | `{ticker}/{resolution}` | Comlink REST → **PostgreSQL** | Kafka | `to-websockets-candles` |
| `v4_markets` | (无) | Comlink REST → **PostgreSQL** | Kafka | `to-websockets-markets` |
| `v4_accounts` | `{address}/{subaccountNumber}` | Comlink REST → **PostgreSQL** | Kafka | `to-websockets-subaccounts` |
| `v4_parent_accounts` | `{address}/{parentSubaccountNumber}` | Comlink REST → **PostgreSQL** | Kafka | `to-websockets-subaccounts` |
| `v4_block_height` | (无) | Comlink REST → **PostgreSQL** | Kafka | `to-websockets-block-height` |

**订阅流程**:
```
客户端                    Socks                      Comlink               Kafka
   │                        │                           │                    │
   │ 1. 发送订阅消息         │                           │                    │
   │ {"type":"subscribe",   │                           │                    │
   │  "channel":"v4_orderbook",                         │                    │
   │  "id":"BTC-USD"}       │                           │                    │
   ├───────────────────────►│                           │                    │
   │                        │ 2. 获取初始数据            │                    │
   │                        │  (调用 REST API)          │                    │
   │                        ├──────────────────────────►│                    │
   │                        │     (从 Redis 查询)        │                    │
   │                        │◄──────────────────────────┤                    │
   │ 3. 返回初始快照         │                           │                    │
   │◄───────────────────────┤                           │                    │
   │                        │                           │                    │
   │                        │ 4. 持续消费 Kafka          │                    │
   │                        │◄───────────────────────────────────────────────┤
   │ 5. 推送增量更新         │                           │                    │
   │◄───────────────────────┤                           │                    │
```

**初始数据获取**:
```typescript
// indexer/services/socks/src/lib/subscription.ts
private getInitialEndpointForSubscription(channel: Channel, id?: string): string | undefined {
  switch (channel) {
    case (Channel.V4_BLOCK_HEIGHT):
      return `${COMLINK_URL}/v4/height`;            // → PostgreSQL
    case (Channel.V4_MARKETS):
      return `${COMLINK_URL}/v4/perpetualMarkets`;  // → PostgreSQL + 内存缓存
    case (Channel.V4_TRADES):
      return `${COMLINK_URL}/v4/trades/perpetualMarket/${id}`;  // → PostgreSQL
    case (Channel.V4_ORDERBOOK):
      return `${COMLINK_URL}/v4/orderbooks/perpetualMarket/${id}`;  // → Redis
    case (Channel.V4_CANDLES):
      const { ticker, resolution } = this.parseCandleChannelId(id);
      return `${COMLINK_URL}/v4/candles/perpetualMarkets/${ticker}?resolution=${resolution}`;  // → PostgreSQL
    // ...
  }
}
```

**消息转发机制** (`message-forwarder.ts`):
- 从 Kafka WebSocket topics 消费消息 (由 Ender/Vulcan 写入)
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

### 11.4 事件发送时机详解

> **核心问题**: OffChainUpdates 和 OnChainUpdates 分别在什么时机发出事件？

#### 11.4.1 CometBFT 交易处理阶段

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CometBFT 交易处理生命周期                              │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   客户端提交交易                                                              │
│         │                                                                    │
│         ▼                                                                    │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                    阶段 1: CheckTx (交易验证)                         │   │
│   │                                                                      │   │
│   │   • 验证交易格式、签名、余额                                          │   │
│   │   • 短期订单: 进入 MemClob 订单簿，执行乐观匹配                        │   │
│   │   • ★ 触发 OffChainUpdates (OrderPlace/Update/Remove)               │   │
│   │   • 交易进入 mempool 等待打包                                         │   │
│   │                                                                      │   │
│   │   延迟: 毫秒级 (收到交易后立即处理)                                    │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│         │                                                                    │
│         │ (等待区块打包, ~1-2秒)                                             │
│         ▼                                                                    │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                    阶段 2: DeliverTx (交易执行)                        │   │
│   │                                                                      │   │
│   │   • 执行交易逻辑 (匹配、转账、仓位更新)                                │   │
│   │   • 生成 Indexer 事件，添加到 TransientStore                          │   │
│   │   • ★ 收集 OnChainEvents (order_fill, subaccount_update, ...)        │   │
│   │   • 状态持久化到链上                                                  │   │
│   │                                                                      │   │
│   │   触发时机: 区块打包后，每笔交易逐个执行                               │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│         │                                                                    │
│         ▼                                                                    │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                    阶段 3: EndBlocker (区块结束)                       │   │
│   │                                                                      │   │
│   │   • 处理区块级逻辑 (资金费率计算、清算等)                              │   │
│   │   • 调用 ProduceBlock(): 从 TransientStore 收集所有事件               │   │
│   │   • ★ 批量发送 OnChainEvents 到 Kafka to-ender topic                 │   │
│   │                                                                      │   │
│   │   延迟: 区块时间 (~1-2秒)                                             │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### 11.4.2 OffChainUpdates 发送时机

**触发阶段**: `CheckTx` (交易验证阶段)

**触发时机**:
1. **客户端提交短期订单后立即触发** - 不等待区块确认
2. **订单进入 MemClob 订单簿时** - 乐观执行匹配逻辑
3. **匹配过程中产生部分成交时** - 实时更新订单状态

**代码调用链**:
```
客户端提交 MsgPlaceOrder (短期订单)
    │
    ▼
ABCI CheckTx()
    │
    ▼
keeper.PlaceShortTermOrder()
    │
    ▼
memclob.PlaceOrder()
    │
    ├── 生成 OrderPlaceV1 消息
    ├── 执行乐观匹配
    ├── 生成 OrderUpdateV1 (如有部分成交)
    └── 生成 OrderRemoveV1 (如完全成交/取消/失效)
    │
    ▼
keeper.sendOffchainMessagesWithTxHash()
    │
    ▼
indexerMessageSender.SendOffchainData()  ──→  Kafka: to-vulcan
```

**具体触发场景**:

| 时机 | 事件类型 | 代码位置 | 说明 |
|------|---------|---------|------|
| 订单进入订单簿 | OrderPlace | `memclob.go:559` | 短期订单验证通过后 |
| 乐观匹配中 Taker 成交 | OrderUpdate | `memclob.go:655,710` | Taker 部分/全部成交 |
| 乐观匹配中 Maker 被吃 | OrderUpdate | `memclob.go:2072` | Maker 被匹配成交 |
| 订单取消 | OrderRemove | `memclob.go:152` | 取消订单请求 |
| 订单完全成交 | OrderRemove | `memclob.go:674` | 从订单簿移除 |
| Post-Only 失败 | OrderRemove | `memclob.go:595` | 会吃单时拒绝 |
| IOC/FOK 未满足 | OrderRemove | `memclob.go:617` | 立即取消未成交部分 |
| 替换订单 | OrderReplace | `memclob.go:552` | 先 Remove 再 Place |

**特点**:
- ⚡ **延迟极低**: 毫秒级 (交易到达节点后立即处理)
- ⚠️ **乐观状态**: 可能因区块回滚而失效
- 📌 **仅短期订单**: 长期订单走 OnChain 流程

---

#### 11.4.3 OnChainUpdates 发送时机

**触发阶段**: `DeliverTx` (交易执行) + `EndBlocker` (区块结束)

**触发时机**:
1. **区块被提议并进入共识后** - 等待 2/3+ 验证者确认
2. **DeliverTx 逐笔执行交易时** - 收集事件到 TransientStore
3. **EndBlocker 时批量发送** - 调用 `ProduceBlock()` 发送到 Kafka

**代码调用链**:
```
区块进入 Consensus 阶段
    │
    ▼
ABCI BeginBlock()
    │
    ▼
ABCI DeliverTx() (逐笔执行)
    │
    ├── process_operations.go: 执行订单匹配
    │   └── AddTxnEvent(order_fill, subaccount_update, ...)
    │
    ├── transfer.go: 执行转账
    │   └── AddTxnEvent(transfer, ...)
    │
    └── stateful_orders.go: 执行长期订单
        └── AddTxnEvent(stateful_order, ...)
    │
    ▼
ABCI EndBlocker()
    │
    ├── 处理资金费率
    │   └── AddBlockEvent(funding_values, ...)
    │
    └── indexerManager.ProduceBlock()
        │
        ▼
        从 TransientStore 收集所有事件
        │
        ▼
        indexerMessageSender.SendOnchainData()  ──→  Kafka: to-ender
```

**具体触发场景**:

| 时机 | 事件类型 | 触发位置 | 说明 |
|------|---------|---------|------|
| 订单成交确认 | order_fill | DeliverTx | MatchOrders 执行成功后 |
| 仓位/余额变化 | subaccount_update | DeliverTx | UpdateSubaccounts 调用后 |
| 子账户转账 | transfer | DeliverTx | MsgCreateTransfer 执行后 |
| 充值确认 | transfer | DeliverTx | MsgDepositToSubaccount 执行后 |
| 提款确认 | transfer | DeliverTx | MsgWithdrawFromSubaccount 执行后 |
| 长期订单放置 | stateful_order | DeliverTx | MsgPlaceOrder (Long-Term) |
| 条件订单放置 | stateful_order | DeliverTx | MsgPlaceOrder (Conditional) |
| 订单取消确认 | stateful_order | DeliverTx | MsgCancelOrder 执行后 |
| 资金费率更新 | funding_values | EndBlocker | 每个资金费率周期结束 |
| 清算事件 | deleveraging | DeliverTx | MatchPerpetualDeleveraging |
| 永续市场创建 | perpetual_market | DeliverTx | 创建新市场 |

**特点**:
- 🔒 **最终确定**: 事件代表链上已确认状态
- ⏱️ **延迟较高**: 区块时间 (~1-2秒)
- 📦 **批量发送**: EndBlocker 时一次性发送整个区块的所有事件

---

#### 11.4.4 时序对比图

```
时间轴 ───────────────────────────────────────────────────────────────────────→

T0: 客户端提交订单
│
│   ┌────────────────────────────────────────────────────────────────────┐
│   │                    CheckTx 阶段 (~10-50ms)                          │
│   │                                                                     │
│   │  T0+10ms: MemClob.PlaceOrder() 开始                                │
│   │  T0+15ms: OrderPlace 事件生成                                       │
│   │  T0+20ms: 乐观匹配执行                                              │
│   │  T0+25ms: OrderUpdate 事件生成 (如有成交)                           │
│   │  T0+30ms: ★ SendOffchainData() → Kafka:to-vulcan                   │
│   │           │                                                         │
│   │           │ (Vulcan 处理 → Redis → WebSocket)                       │
│   │           ▼                                                         │
│   │  T0+50ms: 客户端收到订单状态更新 (WebSocket)                         │
│   │                                                                     │
│   └────────────────────────────────────────────────────────────────────┘
│
│   (等待区块打包 ~1-2秒)
│
T1: 区块 N 开始执行 (T0 + ~1500ms)
│
│   ┌────────────────────────────────────────────────────────────────────┐
│   │                    DeliverTx 阶段 (~100-500ms)                      │
│   │                                                                     │
│   │  T1+50ms: 执行订单匹配 (确认)                                       │
│   │  T1+60ms: AddTxnEvent(order_fill) 添加到 TransientStore            │
│   │  T1+70ms: AddTxnEvent(subaccount_update)                           │
│   │                                                                     │
│   └────────────────────────────────────────────────────────────────────┘
│
│   ┌────────────────────────────────────────────────────────────────────┐
│   │                    EndBlocker 阶段 (~50-200ms)                      │
│   │                                                                     │
│   │  T1+500ms: ProduceBlock() 收集事件                                  │
│   │  T1+510ms: ★ SendOnchainData() → Kafka:to-ender                    │
│   │            │                                                        │
│   │            │ (Ender 处理 → PostgreSQL → Kafka:to-websockets)        │
│   │            ▼                                                        │
│   │  T1+600ms: 成交记录写入数据库                                       │
│   │  T1+700ms: 客户端收到成交确认 (WebSocket)                           │
│   │                                                                     │
│   └────────────────────────────────────────────────────────────────────┘
│
T2: 区块 N 完成 (T0 + ~2000ms)
```

---

#### 11.4.5 关键差异总结

| 维度 | OffChainUpdates | OnChainUpdates |
|------|----------------|----------------|
| **触发阶段** | CheckTx (交易验证) | DeliverTx + EndBlocker |
| **触发时机** | 交易到达节点后立即 | 区块确认后 |
| **延迟** | 10-50ms | 1000-2000ms (区块时间) |
| **状态性质** | 乐观 (可回滚) | 最终确定 |
| **Kafka Topic** | to-vulcan | to-ender |
| **处理服务** | Vulcan | Ender |
| **最终存储** | Redis (热数据) | PostgreSQL (持久化) |
| **订单类型** | 仅短期订单 | 所有类型 |
| **事件内容** | 订单状态变化 | 成交、仓位、转账等 |

---

#### 11.4.6 为什么需要两种机制？

**OffChainUpdates 存在的必要性**:
1. **低延迟交易体验**: 用户下单后毫秒级看到订单状态
2. **订单簿实时更新**: 做市商需要实时订单簿深度
3. **高频交易支持**: 短期订单不需要等待区块确认

**OnChainUpdates 存在的必要性**:
1. **数据持久化**: 成交记录、仓位变化需要永久保存
2. **最终一致性**: 防止区块回滚导致的数据不一致
3. **审计需求**: 链上确认的数据具有法律效力
4. **有状态订单**: 长期订单、条件订单需要链上记录

**两者协作**:
```
用户体验:
  T0+50ms: 看到订单进入订单簿 (OffChain, 乐观)
  T0+2s:   看到成交确认 (OnChain, 最终)

数据一致性:
  OffChain: 提供即时反馈，可能被覆盖
  OnChain:  最终确认，覆盖 OffChain 的乐观状态
```

---

### 11.5 设计哲学

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

## 13. 索引服务部署架构分析

### 13.1 核心结论

**DYDX 索引服务是统一部署一套，不是每个验证器节点配套启动一份。**

### 13.2 架构图

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                         DYDX 索引服务部署架构                                         │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                          验证器网络层                                        │   │
│   │  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐               │   │
│   │  │ Validator │  │ Validator │  │ Validator │  │ Validator │               │   │
│   │  │    #1     │  │    #2     │  │    #3     │  │    #N     │               │   │
│   │  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘               │   │
│   │        │              │              │              │                       │   │
│   │        └──────────────┴──────────────┴──────────────┘                       │   │
│   │                           │                                                  │   │
│   │                           │ CometBFT P2P 共识                                │   │
│   │                           ▼                                                  │   │
│   └─────────────────────────────────────────────────────────────────────────────┘   │
│                               │                                                      │
│                               │ 区块同步                                             │
│                               ▼                                                      │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                    Indexer 专用全节点 (Full Node)                            │   │
│   │                                                                              │   │
│   │   配置参数:                                                                   │   │
│   │   --indexer-kafka-addrs=kafka:9092                                          │   │
│   │   --indexer-send-offchain-data=true                                         │   │
│   │                                                                              │   │
│   │   ┌─────────────────────────────┐    ┌─────────────────────────────┐        │   │
│   │   │  On-chain Event Manager     │    │  Off-chain Update Manager   │        │   │
│   │   │  (DeliverTx/EndBlocker)    │    │  (CheckTx/PrepareProposal)  │        │   │
│   │   └────────────┬────────────────┘    └────────────┬────────────────┘        │   │
│   │                │                                  │                          │   │
│   │                │ to-ender                         │ to-vulcan                │   │
│   │                ▼                                  ▼                          │   │
│   └────────────────┼──────────────────────────────────┼──────────────────────────┘   │
│                    │                                  │                              │
│                    └────────────────┬─────────────────┘                              │
│                                     │                                                │
│                                     ▼                                                │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                              Kafka                                           │   │
│   │  Topics: to-ender, to-vulcan, to-websockets-*, ...                          │   │
│   └─────────────────────────────────┬───────────────────────────────────────────┘   │
│                                     │                                                │
│                                     ▼                                                │
│   ┌─────────────────────────────────────────────────────────────────────────────┐   │
│   │                   Indexer 服务集群 (统一部署)                                 │   │
│   │                                                                              │   │
│   │   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐     │   │
│   │   │  Ender   │  │  Vulcan  │  │ Comlink  │  │  Socks   │  │Roundtable│     │   │
│   │   │ (on-chain│  │(off-chain│  │ (REST    │  │(WebSocket│  │ (定时    │     │   │
│   │   │  events) │  │ updates) │  │   API)   │  │  server) │  │  任务)   │     │   │
│   │   └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘     │   │
│   │        │              │              │              │              │          │   │
│   │        └──────────────┴──────────────┴──────────────┴──────────────┘          │   │
│   │                                     │                                         │   │
│   └─────────────────────────────────────┼─────────────────────────────────────────┘   │
│                                         │                                            │
│                            ┌────────────┴────────────┐                               │
│                            ▼                         ▼                               │
│                      ┌───────────┐             ┌───────────┐                         │
│                      │PostgreSQL │             │   Redis   │                         │
│                      │ (持久化)   │             │ (实时缓存) │                         │
│                      └───────────┘             └───────────┘                         │
│                                                                                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

### 13.3 关键代码证据

#### 全节点配置 (`protocol/indexer/flags.go`)
```go
// Indexer 相关命令行参数
--indexer-kafka-addrs      // 指定 Kafka 地址
--indexer-send-offchain-data  // 启用链下数据发送
```

#### 消息发送 (`protocol/indexer/msgsender/msgsender_kafka.go`)
```go
// 全节点通过 Kafka 发送事件到 Indexer
type IndexerMessageSenderKafka struct {
    producer   sarama.AsyncProducer
    // ...
}

// SendOnchainData 发送链上数据到 to-ender topic
func (msgSender *IndexerMessageSenderKafka) SendOnchainData(message Message) {
    msgSender.send(&sarama.ProducerMessage{
        Topic:   ON_CHAIN_KAFKA_TOPIC,  // "to-ender"
        // ...
    })
}

// SendOffchainData 发送链下数据到 to-vulcan topic
func (msgSender *IndexerMessageSenderKafka) SendOffchainData(message Message) {
    msgSender.send(&sarama.ProducerMessage{
        Topic:   OFF_CHAIN_KAFKA_TOPIC,  // "to-vulcan"
        // ...
    })
}
```

#### Indexer 服务部署 (`indexer/docker-compose-local-deployment.yml`)
```yaml
services:
  kafka:        # 共享 Kafka 实例
  postgres:     # 共享 PostgreSQL 实例
  redis:        # 共享 Redis 实例

  ender:        # 链上事件处理服务
  vulcan:       # 链下更新处理服务
  comlink:      # REST API 服务
  socks:        # WebSocket 服务
  roundtable:   # 定时任务服务
```

### 13.4 设计原理

| 特性 | 设计 | 原因 |
|-----|------|------|
| **统一部署** | 一套 Indexer 服务 | 降低运维复杂度，保证数据一致性 |
| **专用全节点** | 独立于验证器 | 不影响验证器性能，专注于事件收集 |
| **Kafka 解耦** | 消息队列 | 异步处理，削峰填谷，保证可靠性 |
| **水平扩展** | 多实例 Comlink/Socks | 支持高并发 API 请求 |

### 13.5 节点类型详解与辨别方法

#### 三种节点类型对比

```
┌─────────────────────────────────────────────────────────────────┐
│                      DYDX 节点类型                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    Validator (验证器)                    │    │
│  │  --non-validating-full-node=false                       │    │
│  │  --indexer-kafka-conn-str=<空>                          │    │
│  │                                                          │    │
│  │  功能: 参与共识、出块、运行 Oracle/Bridge Daemon          │    │
│  │  Indexer: ❌ 不发送事件                                   │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │               Regular Full Node (普通全节点)             │    │
│  │  --non-validating-full-node=true                        │    │
│  │  --indexer-kafka-conn-str=<空>                          │    │
│  │                                                          │    │
│  │  功能: 同步区块链状态、提供 RPC 服务                      │    │
│  │  Indexer: ❌ 不发送事件                                   │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │            Indexer Full Node (Indexer 专用全节点)        │    │
│  │  --non-validating-full-node=true                        │    │
│  │  --indexer-kafka-conn-str=kafka:9092                    │    │
│  │  --indexer-send-offchain-data=true                      │    │
│  │                                                          │    │
│  │  功能: 同步区块 + 发送链上/链下事件到 Kafka               │    │
│  │  Indexer: ✅ 发送事件到 Kafka                            │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### 关键配置参数对比

| 参数 | Validator | 普通全节点 | Indexer 全节点 |
|-----|-----------|-----------|---------------|
| `--non-validating-full-node` | false | true | true |
| `--indexer-kafka-conn-str` | 空 | 空 | kafka:9092 |
| `--indexer-send-offchain-data` | N/A | N/A | true |

#### 代码判断逻辑

**1. MessageSender 初始化** (`protocol/app/app.go:2078-2113`)

```go
func getIndexerFromOptions(appOpts, logger) (msgsender.IndexerMessageSender, indexer.IndexerFlags) {
    indexerFlags := indexer.GetIndexerFlagValuesFromOptions(appOpts)

    var indexerMessageSender msgsender.IndexerMessageSender
    if len(indexerFlags.KafkaAddrs) == 0 {
        // 普通节点：使用 Noop 实现，不发送任何消息
        indexerMessageSender = msgsender.NewIndexerMessageSenderNoop()
    } else {
        // Indexer 节点：使用 Kafka 实现，发送消息到 Kafka
        indexerMessageSender, _ = msgsender.NewIndexerMessageSenderKafka(indexerFlags, nil, logger)
    }
    return indexerMessageSender, indexerFlags
}
```

**2. Enabled() 检查** (`protocol/indexer/indexer_manager/event_manager.go:46-48`)

```go
func (i *indexerEventManagerImpl) Enabled() bool {
    return i.indexerMessageSender.Enabled()
}
```

**3. 条件执行** (`protocol/x/clob/keeper/orders.go:221`)

```go
// Off-chain update messages should be only be returned if the `IndexerMessageSender`
// is enabled (`msgSender.Enabled()` returns true).
```

#### 如何辨别节点类型

**方法1: 检查启动日志**
```
[INFO] Parsed Indexer flags Flags={KafkaAddrs:[kafka:9092] MaxRetries:20 SendOffchainData:true}
```
- 如果 `KafkaAddrs` 不为空 → Indexer 节点
- 如果 `KafkaAddrs` 为空 → 普通节点

**方法2: 检查启动命令**
```bash
# Indexer 全节点
dydxprotocold start --non-validating-full-node=true --indexer-kafka-conn-str=kafka:9092

# 普通全节点
dydxprotocold start --non-validating-full-node=true

# 验证器
dydxprotocold start --non-validating-full-node=false
```

**方法3: 代码中判断**
```go
if k.indexerEventManager.Enabled() {
    // 只有 Indexer 节点会执行这里的代码
    k.SendOffchainData(...)
}
```

#### Indexer 节点额外工作

| 工作 | 触发时机 | 数据流向 |
|-----|---------|---------|
| **链上事件** | DeliverTx/EndBlocker | → to-ender → Ender → PostgreSQL |
| **链下更新** | CheckTx/PrepareProposal | → to-vulcan → Vulcan → Redis |

### 13.6 扩展性设计

```
                    ┌─────────────────┐
                    │   Full Node #1  │
                    │  (Primary)      │──┐
                    └─────────────────┘  │
                                         │
                    ┌─────────────────┐  │    ┌───────────┐
                    │   Full Node #2  │──┼───▶│   Kafka   │
                    │  (Backup)       │  │    └─────┬─────┘
                    └─────────────────┘  │          │
                                         │          ▼
                    ┌─────────────────┐  │    ┌───────────┐
                    │   Full Node #N  │──┘    │  Indexer  │
                    │  (Geo-replica)  │       │  Services │
                    └─────────────────┘       └───────────┘
```

- **多全节点**: 可部署多个全节点提高数据源可用性
- **Kafka 分区**: 支持消费者组扩展处理能力
- **服务副本**: Comlink/Socks 可水平扩展应对高并发

### 13.7 区块回滚处理机制

#### 核心发现：DYDX Indexer 没有自动区块回滚机制

DYDX Indexer 基于 CometBFT 的即时最终性假设，**不处理区块链 Reorg（重组）**。

#### Ender 区块处理流程

```
┌─────────────────────────────────────────────────────────────────┐
│                    Ender 区块处理流程                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   收到 Kafka 消息 (IndexerTendermintBlock)                       │
│                    │                                             │
│                    ▼                                             │
│   ┌────────────────────────────────────┐                        │
│   │     shouldSkipBlock(blockHeight)   │                        │
│   │  检查: block.height <= 当前高度?    │                        │
│   └────────────────┬───────────────────┘                        │
│         是 │              │ 否                                   │
│            ▼              ▼                                      │
│   ┌─────────────┐  ┌─────────────────────────┐                  │
│   │  跳过处理    │  │ Transaction.start()     │                  │
│   │  (已处理过)  │  │ 开始 PostgreSQL 事务     │                  │
│   └─────────────┘  └───────────┬─────────────┘                  │
│                                │                                 │
│                                ▼                                 │
│                    ┌───────────────────────┐                    │
│                    │   BlockProcessor      │                    │
│                    │   处理区块事件         │                    │
│                    └───────────┬───────────┘                    │
│                      成功 │         │ 失败                       │
│                          ▼         ▼                             │
│           ┌─────────────────┐  ┌─────────────────┐              │
│           │Transaction.commit│  │Transaction.rollback│           │
│           │ 提交事务         │  │ 回滚事务          │              │
│           │ 更新 blockCache  │  │ 刷新数据缓存      │              │
│           └─────────────────┘  └─────────────────┘              │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### 代码证据 (`indexer/services/ender/src/caches/block-cache.ts:42-93`)

```typescript
/**
 * If block.height <= currentBlockHeight, then we can skip processing the block.
 * If block.height == currentBlockHeight + 1, then we should process the block.
 * If block.height > currentBlockHeight + 1, then refresh the cache and...
 */
export async function shouldSkipBlock(blockHeight: string): Promise<boolean> {
  if (blockAlreadyProcessed(blockHeight)) {
    // 如果区块高度 <= 当前已处理高度，直接跳过
    stats.increment(`${config.SERVICE_NAME}.block_already_parsed`, 1);
    return true;
  }
  // ...
}

function blockAlreadyProcessed(blockHeight: string): boolean {
  return Big(currentBlockHeight).gte(blockHeight);
}
```

#### 处理逻辑说明

| 情况 | 处理方式 | 原因 |
|-----|---------|------|
| **正常区块** (height = current+1) | 正常处理，提交事务 | 顺序处理 |
| **重复区块** (height <= current) | 跳过 | 幂等性保证 |
| **处理失败** | 事务回滚，抛出错误，Kafka 不 ack | 可重试 |
| **区块链 Reorg** | ⚠️ 无自动处理 | CometBFT 提供即时最终性 |

#### 为什么没有 Reorg 处理？

1. **CometBFT 最终性**: DYDX 使用 CometBFT 共识，区块一旦提交具有即时最终性，不会发生 Reorg
2. **单一数据源**: 只有一个 Indexer 全节点发送事件，不会出现分叉数据
3. **设计假设**: 从全节点收到的事件被认为是最终的、不可逆的

#### 手动恢复机制 (Bazooka 服务)

如果确实需要回滚（如升级、数据修复），可通过 Bazooka 服务手动操作：

```typescript
// indexer/services/bazooka/src/index.ts
interface BazookaEventJson {
  migrate: boolean,           // 运行数据库迁移
  rollback: boolean,          // 回滚最新一批数据库迁移
  clear_db: boolean,          // 清空 PostgreSQL 数据（保留表结构）
  reset_db: boolean,          // 重置数据库和所有迁移
  clear_kafka_topics: boolean, // 清空 Kafka topics
  clear_redis: boolean,       // 清空 Redis 缓存
  force: boolean,             // 在 testnet/mainnet 执行破坏性操作需要此标志
}
```

**使用场景**:
- Indexer 升级后数据格式变化
- 数据不一致需要重新同步
- 测试环境重置

**操作示例**:
```bash
# 清空所有数据并重新同步
bazooka --clear_db=true --clear_redis=true --clear_kafka_topics=true --force=true
```

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
