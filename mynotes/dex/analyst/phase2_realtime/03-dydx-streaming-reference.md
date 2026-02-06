# dYdX 实时通道参考分析

## 概述

本文档分析 dYdX v4 的实时数据推送架构，作为 dex-realtime 和 dex-ws 设计的参考。重点关注 FullNodeStreamingManager、MemClob 订单簿内存结构、快照与增量推送机制，以及 Redis 在 Indexer 层的缓存角色。

---

## 1. dYdX 实时推送架构

### 1.1 整体架构

```
┌──────────────────────────────────────────────────────────────┐
│                     dYdX v4 Architecture                      │
├──────────────────────────────────────────────────────────────┤
│  Application Chain (Cosmos SDK + CometBFT)                    │
│  ┌──────────────────────────────────────────────────────────┐│
│  │  MemClob (内存订单簿)                                      ││
│  │  - 订单状态管理                                           ││
│  │  - 撮合执行                                               ││
│  │  - 事件生成                                               ││
│  └─────────────────────┬────────────────────────────────────┘│
│                        │                                      │
│  ┌─────────────────────▼────────────────────────────────────┐│
│  │  FullNodeStreamingManager                                 ││
│  │  - 订阅 MemClob 变更                                      ││
│  │  - 生成 StreamUpdates                                     ││
│  │  - 管理客户端连接                                         ││
│  └─────────────────────┬────────────────────────────────────┘│
│                        │                                      │
├────────────────────────┼─────────────────────────────────────┤
│  gRPC Stream           │                                      │
│  ┌─────────────────────▼────────────────────────────────────┐│
│  │  Indexer Service                                          ││
│  │  - 接收 StreamUpdates                                     ││
│  │  - 写入 PostgreSQL                                        ││
│  │  - 更新 Redis 缓存                                        ││
│  │  - 推送 WebSocket                                         ││
│  └──────────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────┘
```

### 1.2 数据流向

```
MemClob 状态变更
      │
      ▼
FullNodeStreamingManager
      │
      ├─→ StreamOrderbookUpdates (订单簿增量)
      ├─→ StreamOrderUpdates (订单状态)
      └─→ StreamFills (成交记录)
      │
      ▼
gRPC Stream → Indexer
      │
      ├─→ PostgreSQL (持久化)
      ├─→ Redis (缓存)
      └─→ WebSocket (实时推送)
```

---

## 2. MemClob 订单簿内存结构

### 2.1 核心数据结构

**文件**: `protocol/x/clob/memclob/memclob.go`

```go
// MemClobKeeper 管理内存中的订单簿状态
type MemClobKeeper struct {
    // 按 ClobPairId 索引的订单簿
    orderbooks map[types.ClobPairId]*Orderbook

    // 活跃订单索引（按 SubaccountId）
    subaccountOpenOrders map[types.SubaccountId]map[types.OrderId]*types.Order

    // 订单哈希索引
    orderIdToOrder map[types.OrderId]*types.Order
}

// Orderbook 单个市场的订单簿
type Orderbook struct {
    // 买盘（按价格降序）
    Bids *skiplist.SkipList
    // 卖盘（按价格升序）
    Asks *skiplist.SkipList
    // 订单簿元数据
    ClobPairId types.ClobPairId
}

// 价格档位
type PriceLevel struct {
    Price    types.Subticks
    Quantity types.BaseQuantums
    Orders   []*types.Order  // 同价格的订单列表
}
```

### 2.2 订单生命周期

```
下单请求
    │
    ▼
PlaceOrder()
    │
    ├─→ 验证订单参数
    ├─→ 检查保证金
    ├─→ 尝试撮合（Match）
    │      ├─→ 完全成交 → 发送 FillEvent
    │      └─→ 部分成交/未成交 → 进入订单簿
    │
    ▼
订单进入订单簿
    │
    ├─→ 更新 Bids/Asks
    ├─→ 更新 subaccountOpenOrders
    ├─→ 发送 OrderPlaceEvent
    └─→ 触发 StreamOrderbookUpdate
```

### 2.3 与本项目的对应关系

| dYdX 组件 | 本项目对应 | 说明 |
|-----------|-----------|------|
| MemClob | sui-execution 撮合引擎 | 订单簿和撮合逻辑 |
| FullNodeStreamingManager | 无直接对应（使用链上事件） | 事件分发 |
| Indexer Redis | dex-realtime Redis | 缓存层 |

---

## 3. FullNodeStreamingManager

### 3.1 架构设计

**文件**: `protocol/streaming/full_node_streaming_manager.go`

```go
type FullNodeStreamingManager struct {
    // gRPC 服务器
    grpcServer *grpc.Server

    // 订阅者管理
    subscribers map[uint32]*Subscriber

    // 更新缓冲区
    updateBuffer *UpdateBuffer

    // 批处理配置
    batchIntervalMs int64  // 默认 10ms
    maxBatchSize    int    // 默认 100
}

// Subscriber 表示一个订阅客户端
type Subscriber struct {
    id           uint32
    stream       StreamingServiceServer
    clobPairIds  []types.ClobPairId
    subaccountIds []types.SubaccountId
}
```

### 3.2 更新类型

```protobuf
// StreamUpdate 消息类型
message StreamUpdate {
    oneof update_message {
        StreamOrderbookUpdate orderbook_update = 1;
        StreamOrderUpdate order_update = 2;
        StreamFill fill = 3;
        StreamTakerOrder taker_order = 4;
        StreamSubaccountUpdate subaccount_update = 5;
    }
    uint32 block_height = 6;
    uint32 exec_mode = 7;
}

// StreamOrderbookUpdate 订单簿增量更新
message StreamOrderbookUpdate {
    uint32 clob_pair_id = 1;
    repeated OffchainUpdate updates = 2;  // 增量变更列表
    bool snapshot = 3;                      // 是否为快照
}

// OffchainUpdate 单个订单簿变更
message OffchainUpdate {
    oneof update_message {
        OffchainUpdateOrder order_place = 1;
        OffchainUpdateOrder order_update = 2;
        OrderRemove order_remove = 3;
    }
}
```

### 3.3 批处理机制

```go
// SendUpdates 批量发送更新
func (m *FullNodeStreamingManager) SendUpdates() {
    ticker := time.NewTicker(time.Duration(m.batchIntervalMs) * time.Millisecond)

    for {
        select {
        case <-ticker.C:
            m.flushBuffer()
        case update := <-m.updateBuffer:
            m.buffer = append(m.buffer, update)
            if len(m.buffer) >= m.maxBatchSize {
                m.flushBuffer()
            }
        }
    }
}

func (m *FullNodeStreamingManager) flushBuffer() {
    if len(m.buffer) == 0 {
        return
    }

    batch := m.buffer
    m.buffer = nil

    for _, sub := range m.subscribers {
        sub.Send(batch)
    }
}
```

**批处理配置**：
- 默认间隔：10ms
- 最大批大小：100 条更新
- 目的：减少网络开销，平滑推送

---

## 4. 快照与增量推送机制

### 4.1 推送模式

```
新客户端连接
      │
      ▼
发送全量快照（Snapshot）
      │
      ├─→ 完整订单簿状态
      ├─→ 所有活跃订单
      └─→ 当前持仓和余额
      │
      ▼
切换到增量模式
      │
      └─→ 仅发送变更（Delta）
            ├─→ OrderPlace
            ├─→ OrderUpdate
            ├─→ OrderRemove
            └─→ Fill
```

### 4.2 快照生成

```go
// GetOrderbookSnapshot 获取订单簿快照
func (o *Orderbook) GetOrderbookSnapshot() *StreamOrderbookUpdate {
    update := &StreamOrderbookUpdate{
        ClobPairId: o.ClobPairId,
        Snapshot:   true,  // 标记为快照
        Updates:    make([]*OffchainUpdate, 0),
    }

    // 遍历买盘
    for it := o.Bids.Iterator(); it.Valid(); it.Next() {
        level := it.Value().(*PriceLevel)
        for _, order := range level.Orders {
            update.Updates = append(update.Updates, &OffchainUpdate{
                UpdateMessage: &OffchainUpdateOrder{
                    OrderPlace: order,
                },
            })
        }
    }

    // 遍历卖盘
    for it := o.Asks.Iterator(); it.Valid(); it.Next() {
        level := it.Value().(*PriceLevel)
        for _, order := range level.Orders {
            update.Updates = append(update.Updates, &OffchainUpdate{
                UpdateMessage: &OffchainUpdateOrder{
                    OrderPlace: order,
                },
            })
        }
    }

    return update
}
```

### 4.3 增量更新

```go
// OnOrderPlaced 订单放置时生成增量
func (m *FullNodeStreamingManager) OnOrderPlaced(order *types.Order) {
    update := &StreamUpdate{
        OrderbookUpdate: &StreamOrderbookUpdate{
            ClobPairId: order.ClobPairId,
            Snapshot:   false,  // 增量更新
            Updates: []*OffchainUpdate{{
                UpdateMessage: &OffchainUpdateOrder{
                    OrderPlace: order,
                },
            }},
        },
    }
    m.updateBuffer <- update
}

// OnOrderRemoved 订单移除时生成增量
func (m *FullNodeStreamingManager) OnOrderRemoved(orderId types.OrderId, reason RemovalReason) {
    update := &StreamUpdate{
        OrderbookUpdate: &StreamOrderbookUpdate{
            ClobPairId: orderId.ClobPairId,
            Snapshot:   false,
            Updates: []*OffchainUpdate{{
                UpdateMessage: &OrderRemove{
                    OrderId:       orderId,
                    RemovalReason: reason,
                },
            }},
        },
    }
    m.updateBuffer <- update
}
```

### 4.4 本项目的快照机制设计

```
dex-realtime 启动
      │
      ▼
从 PostgreSQL 恢复订单簿状态
(SELECT * FROM dex_orders WHERE status = 'open')
      │
      ▼
构建内存订单簿
      │
      ▼
监听 Sui RPC 事件
      │
      ├─→ OrderPlacedEventV1 → 添加到订单簿
      ├─→ OrderRemovedEventV1 → 从订单簿移除
      └─→ FillEventV1 → 更新订单数量
      │
      ▼
定期快照到 Redis（~100ms）
      │
      ▼
dex-ws 新连接
      │
      ├─→ 从 Redis 读取快照
      └─→ 订阅 Redis Stream 接收增量
```

---

## 5. OrderUpdateV1 与 OrderFill 的关系

### 5.1 dYdX 的设计

```protobuf
// OrderUpdateV1 订单状态更新
message OrderUpdateV1 {
    IndexerOrderId order_id = 1;

    oneof update_type {
        // 订单数量更新（部分成交后剩余量）
        uint64 total_filled_quantums = 2;
        // 订单状态变更
        OrderStatus new_status = 3;
    }
}

// OrderFillEventV1 成交事件
message OrderFillEventV1 {
    IndexerOrderId maker_order_id = 1;
    IndexerOrderId taker_order_id = 2;
    uint64 fill_amount = 3;
    uint64 price = 4;
    // ...
}
```

### 5.2 事件触发关系

```
下单请求
    │
    ▼
撮合引擎处理
    │
    ├─→ 无匹配 → OrderPlaceEvent（订单进入簿）
    │
    └─→ 有匹配
          │
          ├─→ OrderFillEvent（每次成交）
          │
          ├─→ OrderUpdateV1（更新 maker 剩余量）
          │
          └─→ 完全成交？
                ├─→ 是 → OrderRemoveEvent
                └─→ 否 → 订单保留在簿中
```

### 5.3 本项目的简化设计

由于当前 DEX 不支持订单修改（OrderUpdate），设计简化为：

| 事件 | 触发时机 | 包含信息 |
|------|----------|----------|
| OrderPlacedEventV1 | 订单进入订单簿 | 完整订单信息 |
| FillEventV1 | 每次订单匹配 | 成交价、成交量、双方信息 |
| OrderRemovedEventV1 | 订单移除 | 订单 ID、移除原因、剩余量 |

**简化点**：
- 无 OrderUpdateV1（不支持订单修改）
- FillEventV1 已包含成交信息，无需额外的 OrderUpdate
- 通过 OrderRemovedEventV1.remaining_quantity 可知部分成交情况

---

## 6. 心跳/限流/缓冲机制

### 6.1 心跳机制

```go
// dYdX 心跳实现
type HeartbeatConfig struct {
    IntervalMs   int64  // 心跳间隔，默认 1000ms
    TimeoutMs    int64  // 超时时间，默认 5000ms
}

func (s *Subscriber) StartHeartbeat() {
    ticker := time.NewTicker(time.Duration(s.heartbeatInterval) * time.Millisecond)

    for {
        select {
        case <-ticker.C:
            s.Send(&StreamUpdate{
                Heartbeat: &Heartbeat{
                    Timestamp: time.Now().UnixMilli(),
                },
            })
        case <-s.done:
            return
        }
    }
}
```

### 6.2 限流机制

```go
type RateLimitConfig struct {
    // 每秒最大消息数
    MaxMessagesPerSecond int
    // 每秒最大字节数
    MaxBytesPerSecond    int
    // 突发容量
    BurstSize            int
}

func (s *Subscriber) Send(update *StreamUpdate) error {
    if !s.rateLimiter.Allow() {
        // 丢弃或缓冲
        return ErrRateLimited
    }
    return s.stream.Send(update)
}
```

### 6.3 缓冲机制

```go
type BufferConfig struct {
    // 缓冲区大小
    Size int
    // 溢出策略：丢弃旧消息 / 丢弃新消息 / 阻塞
    OverflowPolicy OverflowPolicy
}

type UpdateBuffer struct {
    updates chan *StreamUpdate
    config  BufferConfig
}

func (b *UpdateBuffer) Push(update *StreamUpdate) error {
    select {
    case b.updates <- update:
        return nil
    default:
        switch b.config.OverflowPolicy {
        case DropOldest:
            <-b.updates  // 丢弃最旧的
            b.updates <- update
        case DropNewest:
            return ErrBufferFull
        case Block:
            b.updates <- update  // 阻塞等待
        }
    }
    return nil
}
```

### 6.4 本项目的建议配置

| 机制 | 配置 | 说明 |
|------|------|------|
| 批处理间隔 | 10ms | 平衡延迟和吞吐 |
| 批大小上限 | 100 | 防止单批过大 |
| 心跳间隔 | 1000ms | 检测连接存活 |
| 心跳超时 | 5000ms | 判定连接断开 |
| 缓冲区大小 | 1000 | 应对突发流量 |
| 溢出策略 | DropOldest | 保证最新数据 |

---

## 7. Redis 在 Indexer 层的缓存角色

### 7.1 dYdX Indexer 的 Redis 使用

```
┌─────────────────────────────────────────────────────────────┐
│  dYdX Indexer Redis 架构                                     │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────┐    ┌──────────────────┐               │
│  │  Orderbook Cache │    │  Market Cache    │               │
│  │  - L2 聚合快照   │    │  - 中间价        │               │
│  │  - 最优买卖价   │    │  - 24h 统计      │               │
│  └──────────────────┘    └──────────────────┘               │
│                                                              │
│  ┌──────────────────┐    ┌──────────────────┐               │
│  │  Candle Cache    │    │  Trades Cache    │               │
│  │  - 实时 K 线    │    │  - 最近成交      │               │
│  │  - 多周期       │    │  - 按市场索引    │               │
│  └──────────────────┘    └──────────────────┘               │
│                                                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  Redis Pub/Sub (消息分发)                                ││
│  │  - orderbook:{clobPairId}                               ││
│  │  - trades:{clobPairId}                                  ││
│  │  - subaccount:{subaccountId}                            ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### 7.2 Redis 键设计（dYdX 风格）

```
# 订单簿聚合缓存
v4_orderbook:{clobPairId}            → Hash { bids, asks, updated_at }
v4_orderbook_depth:{clobPairId}:{depth} → Hash { bids, asks }

# 市场数据缓存
v4_market:{clobPairId}               → Hash { mid_price, oracle_price, ... }
v4_market_stats:{clobPairId}         → Hash { volume_24h, open_interest, ... }

# K 线缓存
v4_candle:{clobPairId}:{interval}    → ZSet { timestamp → OHLCV JSON }
v4_candle_latest:{clobPairId}:{interval} → Hash { current candle }

# 最近成交
v4_trades:{clobPairId}               → ZSet { timestamp → trade JSON }

# Pub/Sub 频道
channel:orderbook:{clobPairId}       → 订单簿更新
channel:trades:{clobPairId}          → 成交更新
channel:subaccount:{subaccountId}    → 账户更新
```

### 7.3 本项目 Redis 设计对照

| dYdX 键 | 本项目键 | 说明 |
|---------|----------|------|
| v4_orderbook:* | dex:orderbook:{perpetual_id} | 订单簿快照 |
| v4_market:* | dex:market:{perpetual_id} | 市场数据 |
| v4_candle:* | dex:candles:{perpetual_id}:{interval} | K 线数据 |
| v4_trades:* | dex:trades:{perpetual_id} | 最近成交 |
| channel:* | dex:stream:* (Redis Stream) | 使用 Stream 替代 Pub/Sub |

### 7.4 Redis Stream vs Pub/Sub

| 特性 | Redis Stream | Redis Pub/Sub |
|------|--------------|---------------|
| 消息持久化 | ✓ 支持 | ✗ 不支持 |
| 消费者组 | ✓ 支持 | ✗ 不支持 |
| 消息确认 | ✓ 支持 | ✗ 不支持 |
| 历史消息 | ✓ 可回溯 | ✗ 错过即丢失 |
| 推荐场景 | dex-realtime | 简单通知 |

**选择 Redis Stream 的理由**：
1. 消息持久化，支持断线重连后继续消费
2. 消费者组支持多实例部署
3. 消息确认机制，确保处理完成
4. 可回溯历史消息，便于调试

---

## 8. 关键设计启示

### 8.1 从 dYdX 学到的

1. **内存优先**：订单簿主数据在内存，Redis 是查询缓存
2. **批处理推送**：10ms 间隔批量推送，减少网络开销
3. **快照+增量**：新连接发快照，后续发增量
4. **分层缓存**：不同数据不同 TTL 和更新频率

### 8.2 本项目的差异化设计

1. **链上事件替代 Off-chain**：使用 Sui RPC 订阅替代 gRPC Stream
2. **Redis Stream 替代 Pub/Sub**：更可靠的消息分发
3. **启动恢复从 PostgreSQL**：无 FullNode 内存状态，从持久化层恢复
4. **统一事件结构**：dex-indexer 和 dex-realtime 处理相同事件

### 8.3 需要注意的风险

| 风险 | dYdX 方案 | 本项目方案 |
|------|-----------|-----------|
| 状态不一致 | MemClob 为准 | PostgreSQL + Redis 双写 |
| 消息丢失 | 内存通道 | Redis Stream 持久化 |
| 启动恢复 | 内存重建 | 从 PostgreSQL 加载 |
| 性能瓶颈 | 内存操作 | 依赖 Redis 和 RPC |
