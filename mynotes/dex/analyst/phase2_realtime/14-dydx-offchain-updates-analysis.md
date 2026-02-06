# dYdX OffChainUpdates 功能分析

> 分析 dYdX OffChainUpdates 事件的设计目的、触发时机和实现功能

## 1. 概述

OffChainUpdates 是 dYdX 双通道事件架构中的**低延迟通道**，提供 10-50ms 级别的订单状态推送，与 OnChainUpdates（1-2秒，区块确认后）形成互补。

## 2. 事件类型

| 类型 | Protobuf 结构 | 用途 |
|------|--------------|------|
| `OrderPlaceV1` | `OffChainUpdateV1_OrderPlace` | 订单放置到订单簿 |
| `OrderRemoveV1` | `OffChainUpdateV1_OrderRemove` | 订单移除（取消/完全成交/Post-Only 失败/IOC-FOK） |
| `OrderUpdateV1` | `OffChainUpdateV1_OrderUpdate` | 订单部分成交，更新剩余数量 |
| `OrderReplaceV1` | `OffChainUpdateV1_OrderReplace` | 订单替换 |

**代码位置**: `protocol/indexer/off_chain_updates/`

## 3. 触发时机

### 3.1 触发阶段：CheckTx

OffChainUpdates 在 **CheckTx 阶段**（交易验证）触发，而非区块确认后：

```
交易到达 → CheckTx → MemClob.PlaceOrder() → 立即生成 OffChainUpdates
                                           ↓
                                    sendOffchainMessagesWithTxHash()
                                           ↓
                                    Kafka (to-vulcan topic)
```

### 3.2 详细触发场景

| 更新类型 | 触发场景 | 代码位置 |
|---------|---------|---------|
| **OrderPlace** | 短期订单进入 MemClob | `memclob/memclob.go:PlaceOrder` |
| **OrderUpdate** | Taker 部分成交 | `memclob/memclob.go:matchOrder` |
| **OrderUpdate** | Maker 被吃单 | `memclob/memclob.go:matchOrder` |
| **OrderRemove** | 用户取消订单 | `memclob/memclob.go:CancelOrder` |
| **OrderRemove** | 订单完全成交 | `memclob/memclob.go:matchOrder` |
| **OrderRemove** | Post-Only 订单会吃单被拒 | `memclob/memclob.go:PlaceOrder` |
| **OrderRemove** | IOC/FOK 未完全成交 | `memclob/memclob.go:PlaceOrder` |

### 3.3 代码示例

```go
// memclob/memclob.go - PlaceOrder
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

    // 根据订单状态生成不同的消息
    if order was fully filled {
        offchainUpdates.AddUpdateMessage(order.OrderId, message)  // 最终成交量
    } else if post-only crosses {
        offchainUpdates.AddRemoveMessage(order.OrderId, message)  // 被拒绝
    }
}
```

## 4. 主要实现功能

### 4.1 实时订单簿维护

**核心功能**：维护 Redis 中的内存订单簿状态

```
OrderPlaceV1  → HSET orderbook → 将订单添加到 Redis 订单簿
OrderRemoveV1 → HDEL orderbook → 从 Redis 订单簿移除订单
OrderUpdateV1 → HSET orderbook → 更新订单剩余数量
```

Vulcan 服务处理这些事件后，Redis 中始终保持最新的订单簿快照。

### 4.2 WebSocket 实时推送

**推送通道**：
- `v4_orderbook/{market}` - 订单簿深度变化
- `v4_orders/{subaccount}` - 用户订单状态变更
- `v4_trades/{market}` - 实时成交信息

**推送内容**：
- 订单状态变更（挂单/成交/取消）
- 订单簿深度增量更新
- Taker/Maker 双方的成交通知

### 4.3 乐观 UI 更新

| 用户操作 | OffChain 事件 | UI 立即显示 |
|---------|--------------|------------|
| 下限价单 | `OrderPlaceV1` | "订单已挂单" |
| 订单部分成交 | `OrderUpdateV1` | "已成交 50%" |
| 取消订单 | `OrderRemoveV1` | "订单已取消" |
| Post-Only 被拒 | `OrderRemoveV1` | "订单被拒绝" |

### 4.4 做市商数据源

做市商依赖 OffChainUpdates 获取：
- **自身订单状态** - 判断是否需要调整报价
- **市场深度变化** - 实时调整做市策略
- **成交反馈** - 毫秒级知道自己被吃单

## 5. 数据流架构

```
┌─────────────────────────────────────────────────────────────────┐
│                      OffChainUpdates 数据流                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐  │
│  │ CheckTx  │ → │  MemClob  │ → │  Kafka   │ → │  Vulcan  │  │
│  │ 交易验证  │    │ 订单匹配  │    │ to-vulcan│    │ 事件处理 │  │
│  └──────────┘    └──────────┘    └──────────┘    └────┬─────┘  │
│                                                        │        │
│                                                        ▼        │
│                                                  ┌──────────┐   │
│                                                  │  Redis   │   │
│                                                  │ 订单簿缓存│   │
│                                                  └────┬─────┘   │
│                                                        │        │
│                    ┌──────────────────────────────────┼────┐   │
│                    │                                   │    │   │
│                    ▼                                   ▼    │   │
│              ┌──────────┐                       ┌──────────┐│   │
│              │ REST API │                       │WebSocket ││   │
│              │ /orderbook│                       │ 实时推送 ││   │
│              └──────────┘                       └──────────┘│   │
│                                                              │   │
└──────────────────────────────────────────────────────────────────┘
```

## 6. 与 OnChainUpdates 对比

| 维度 | OffChainUpdates | OnChainUpdates |
|------|-----------------|----------------|
| **触发阶段** | CheckTx（交易验证） | DeliverTx + EndBlocker |
| **触发时机** | 交易到达节点后立即 | 区块确认后 |
| **延迟** | 10-50ms | 1000-2000ms |
| **状态性质** | 乐观（可回滚） | 最终确定 |
| **存储目标** | Redis（热数据） | PostgreSQL（持久化） |
| **Kafka Topic** | `to-vulcan` | `to-ender` |
| **处理服务** | Vulcan | Ender |
| **适用场景** | 短期订单、实时 UI | 资金变动、历史记录 |

## 7. 消息累积与优化

### 7.1 OffchainUpdates 结构

```go
// protocol/x/clob/types/offchain_updates.go
type OffchainUpdates struct {
    Messages []OffchainUpdateMessage
}

type OffchainUpdateMessage struct {
    Type    OffchainUpdateMessageType  // Place/Remove/Update/Replace
    OrderId types.OrderId
    Message msgsender.Message
}
```

### 7.2 消息压缩（Replay 场景）

`CondenseMessagesForReplay()` 方法优化重放场景的消息批次：
- 删除所有 `Place` 消息（indexer 已经有）
- 每个 OrderId 只保留最后一条消息
- 中间的 `Update` 消息被跳过（只关心最终成交量）

## 8. 设计哲学

### 8.1 为什么需要 OffChainUpdates

1. **低延迟交易体验** - 用户下单后毫秒级看到订单状态
2. **订单簿实时更新** - 做市商需要实时订单簿深度
3. **高频交易支持** - 短期订单不需要等待区块确认

### 8.2 乐观更新的风险与处理

OffChainUpdates 是**乐观状态**，可能被回滚：
- 交易最终未被区块包含
- 执行结果与乐观匹配不同

**处理方式**：OnChainUpdates 的最终状态会覆盖 OffChainUpdates 的乐观状态

```
用户体验:
  T0+50ms: 看到订单进入订单簿 (OffChain, 乐观)
  T0+2s:   看到成交确认 (OnChain, 最终)

数据一致性:
  OffChain: 提供即时反馈，可能被覆盖
  OnChain:  最终确认，覆盖 OffChain 的乐观状态
```

## 9. 对 DEX 项目的启示

### 9.1 可借鉴的设计

1. **双通道分离** - 实时推送与最终确认分离
2. **事件类型精简** - 只有 4 种订单相关事件
3. **Redis 热数据** - 订单簿状态不写数据库
4. **消息批次优化** - 压缩冗余消息

### 9.2 Sui 实现差异

| dYdX | Sui DEX |
|------|---------|
| CheckTx 阶段触发 | 交易执行后事件 |
| 应用链可控时机 | 依赖 Sui 事件订阅 |
| Kafka 消息队列 | Redis Streams |
| 专用 Vulcan 服务 | dex-realtime 服务 |

## 10. 参考资料

- 代码位置: `dydx-v4-chain/protocol/indexer/off_chain_updates/`
- Protobuf 定义: `dydx-v4-chain/proto/dydxprotocol/indexer/off_chain_updates/`
- 详细分析: `sui/mynotes/dex/analyst/indexer/dydx-indexer-analyst.md` 第 11 节
