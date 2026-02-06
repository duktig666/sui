# Phase 2 WebSocket 协议规范

## 概述

本文档定义 dex-ws WebSocket 服务的协议规范，包括连接管理、订阅机制、消息格式和心跳机制。协议设计参考 Hyperliquid WebSocket API。

---

## 1. 连接规范

### 1.1 连接地址

```
ws://localhost:3001/ws
wss://api.dex.example.com/ws  # 生产环境
```

### 1.2 连接参数

| 参数 | 类型 | 说明 | 默认值 |
|------|------|------|--------|
| 无需认证 | - | 公开市场数据无需认证 | - |

### 1.3 连接限制

| 参数 | 值 | 说明 |
|------|-----|------|
| 最大连接数 | 10000 | 单实例并发连接上限 |
| 单 IP 连接数 | 100 | 防止单 IP 过载 |
| 消息速率 | 10 msg/s | 客户端发送速率限制 |

---

## 2. 消息格式

### 2.1 请求消息

所有客户端请求使用统一格式：

```json
{
    "method": "subscribe" | "unsubscribe",
    "subscription": { ... }
}
```

### 2.2 响应/推送消息

所有服务端消息使用统一格式：

```json
{
    "channel": "频道名称",
    "data": { ... }
}
```

---

## 3. 订阅 API

### 3.1 订阅请求

**请求格式**：

```json
{
    "method": "subscribe",
    "subscription": {
        "type": "频道类型",
        ...其他参数
    }
}
```

**响应格式**：

```json
{
    "channel": "subscriptionResponse",
    "data": {
        "method": "subscribe",
        "subscription": { ... },
        "success": true
    }
}
```

**错误响应**：

```json
{
    "channel": "subscriptionResponse",
    "data": {
        "method": "subscribe",
        "subscription": { ... },
        "success": false,
        "error": "错误信息"
    }
}
```

---

### 3.2 取消订阅请求

**请求格式**：

```json
{
    "method": "unsubscribe",
    "subscription": {
        "type": "频道类型",
        ...其他参数
    }
}
```

**响应格式**：

```json
{
    "channel": "subscriptionResponse",
    "data": {
        "method": "unsubscribe",
        "subscription": { ... },
        "success": true
    }
}
```

---

## 4. 频道定义

### 4.1 trades - 成交频道

**订阅请求**：

```json
{
    "method": "subscribe",
    "subscription": {
        "type": "trades",
        "perpetualId": 0
    }
}
```

**推送消息**：

```json
{
    "channel": "trades",
    "data": {
        "perpetualId": 0,
        "trades": [
            {
                "id": "7xK9Abc123-0",
                "price": "95000.0",
                "size": "1.0",
                "side": "Buy",
                "timestamp": 1707123456789,
                "takerFee": "0.05",
                "makerFee": "-0.025"
            }
        ]
    }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `perpetualId` | number | 永续合约 ID |
| `trades` | array | 成交记录数组 |
| `trades[].id` | string | 事件 ID |
| `trades[].price` | string | 成交价格（格式化后） |
| `trades[].size` | string | 成交数量（格式化后） |
| `trades[].side` | string | Taker 方向：Buy/Sell |
| `trades[].timestamp` | number | 时间戳（毫秒） |
| `trades[].takerFee` | string | Taker 手续费 |
| `trades[].makerFee` | string | Maker 手续费 |

---

### 4.2 l2Book - 订单簿频道

**订阅请求**：

```json
{
    "method": "subscribe",
    "subscription": {
        "type": "l2Book",
        "perpetualId": 0
    }
}
```

**初始快照**（订阅后立即推送）：

```json
{
    "channel": "l2Book",
    "data": {
        "perpetualId": 0,
        "type": "snapshot",
        "bids": [
            ["95000.0", "10.5"],
            ["94999.0", "5.2"]
        ],
        "asks": [
            ["95001.0", "8.3"],
            ["95002.0", "12.1"]
        ],
        "timestamp": 1707123456789
    }
}
```

**增量更新**：

```json
{
    "channel": "l2Book",
    "data": {
        "perpetualId": 0,
        "type": "delta",
        "bids": [
            ["95000.0", "15.5"],
            ["94998.0", "0"]
        ],
        "asks": [
            ["95001.0", "0"]
        ],
        "timestamp": 1707123456790
    }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `type` | string | snapshot（全量）/ delta（增量） |
| `bids` | array | 买盘 [价格, 数量]，数量为 0 表示删除 |
| `asks` | array | 卖盘 [价格, 数量] |
| `timestamp` | number | 订单簿时间戳 |

---

### 4.3 candle - K 线频道

**订阅请求**：

```json
{
    "method": "subscribe",
    "subscription": {
        "type": "candle",
        "perpetualId": 0,
        "interval": "1m"
    }
}
```

**支持的 interval**：

| interval | 说明 |
|----------|------|
| `1m` | 1 分钟 |
| `5m` | 5 分钟 |
| `15m` | 15 分钟 |
| `1h` | 1 小时 |
| `4h` | 4 小时 |
| `1d` | 1 天 |

**推送消息**：

```json
{
    "channel": "candle",
    "data": {
        "perpetualId": 0,
        "interval": "1m",
        "candle": {
            "openTime": 1707123420000,
            "open": "95000.0",
            "high": "95100.0",
            "low": "94900.0",
            "close": "95050.0",
            "volume": "1234.56",
            "quoteVolume": "117234560.0",
            "tradeCount": 156
        }
    }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `openTime` | number | K 线开始时间（毫秒） |
| `open` | string | 开盘价 |
| `high` | string | 最高价 |
| `low` | string | 最低价 |
| `close` | string | 收盘价（当前价） |
| `volume` | string | 成交量（基础货币） |
| `quoteVolume` | string | 成交额（报价货币） |
| `tradeCount` | number | 成交笔数 |

---

### 4.4 userFills - 用户成交频道

**订阅请求**：

```json
{
    "method": "subscribe",
    "subscription": {
        "type": "userFills",
        "subaccount": "0x1234567890abcdef..."
    }
}
```

**推送消息**：

```json
{
    "channel": "userFills",
    "data": {
        "subaccount": "0x1234567890abcdef...",
        "fills": [
            {
                "perpetualId": 0,
                "orderId": "0xabc123...",
                "side": "Buy",
                "price": "95000.0",
                "size": "1.0",
                "fee": "0.05",
                "isMaker": false,
                "timestamp": 1707123456789
            }
        ]
    }
}
```

---

### 4.5 userOrders - 用户订单频道

**订阅请求**：

```json
{
    "method": "subscribe",
    "subscription": {
        "type": "userOrders",
        "subaccount": "0x1234567890abcdef..."
    }
}
```

**订单状态推送**：

```json
{
    "channel": "userOrders",
    "data": {
        "subaccount": "0x1234567890abcdef...",
        "orders": [
            {
                "orderId": "0xabc123...",
                "perpetualId": 0,
                "side": "Buy",
                "price": "95000.0",
                "size": "5.0",
                "filled": "2.0",
                "status": "open",
                "timestamp": 1707123456789
            }
        ]
    }
}
```

| status | 说明 |
|--------|------|
| `open` | 挂单中 |
| `filled` | 完全成交 |
| `cancelled` | 已取消 |
| `expired` | 已过期 |

---

### 4.6 userPositions - 用户持仓频道

**订阅请求**：

```json
{
    "method": "subscribe",
    "subscription": {
        "type": "userPositions",
        "subaccount": "0x1234567890abcdef..."
    }
}
```

**推送消息**：

```json
{
    "channel": "userPositions",
    "data": {
        "subaccount": "0x1234567890abcdef...",
        "positions": [
            {
                "perpetualId": 0,
                "side": "Long",
                "size": "10.0",
                "avgEntryPrice": "95500.0",
                "unrealizedPnl": "500.0",
                "marginUsed": "9550.0"
            }
        ]
    }
}
```

---

## 5. 心跳机制

### 5.1 客户端心跳

客户端应定期发送 ping 消息：

```json
{"method": "ping"}
```

服务端响应：

```json
{"channel": "pong"}
```

### 5.2 服务端心跳

服务端每秒发送 WebSocket ping frame（协议层），客户端应响应 pong frame。

### 5.3 超时断连

| 参数 | 值 | 说明 |
|------|-----|------|
| 心跳间隔 | 1 秒 | 服务端发送间隔 |
| 超时时间 | 5 秒 | 无响应则断开 |
| 客户端建议 | 30 秒 | 建议客户端 30s 发一次 ping |

---

## 6. 错误处理

### 6.1 错误消息格式

```json
{
    "channel": "error",
    "data": {
        "code": "ERROR_CODE",
        "message": "错误描述"
    }
}
```

### 6.2 错误码定义

| 错误码 | 说明 | 处理建议 |
|--------|------|----------|
| `INVALID_MESSAGE` | 消息格式错误 | 检查 JSON 格式 |
| `INVALID_METHOD` | 不支持的方法 | 使用 subscribe/unsubscribe |
| `INVALID_SUBSCRIPTION` | 订阅参数错误 | 检查必填参数 |
| `UNKNOWN_CHANNEL` | 未知频道类型 | 检查 type 字段 |
| `RATE_LIMITED` | 请求过于频繁 | 降低请求频率 |
| `INTERNAL_ERROR` | 服务器内部错误 | 稍后重试 |

---

## 7. 连接生命周期

### 7.1 连接建立

```
Client                          Server
   |-------- WebSocket Connect ------->|
   |<-------- 101 Switching ---------|
   |                                   |
   |-------- Subscribe Request ------->|
   |<------ Subscription Response -----|
   |<-------- Initial Snapshot --------|
   |<-------- Updates (streaming) -----|
```

### 7.2 断线重连

**客户端重连策略**：

| 参数 | 值 |
|------|-----|
| 初始延迟 | 1 秒 |
| 最大延迟 | 30 秒 |
| 退避乘数 | 2 |

**重连后操作**：
1. 重新发送所有订阅请求
2. l2Book 频道会收到新的完整快照
3. 其他频道从当前状态开始推送

---

## 8. 示例：完整交互流程

### 8.1 订阅订单簿

```
# 1. 建立连接
ws://localhost:3001/ws

# 2. 发送订阅
{"method": "subscribe", "subscription": {"type": "l2Book", "perpetualId": 0}}

# 3. 收到响应
{"channel": "subscriptionResponse", "data": {"method": "subscribe", "subscription": {"type": "l2Book", "perpetualId": 0}, "success": true}}

# 4. 收到快照
{"channel": "l2Book", "data": {"perpetualId": 0, "type": "snapshot", "bids": [["95000.0", "10.5"]], "asks": [["95001.0", "8.3"]], "timestamp": 1707123456789}}

# 5. 收到增量更新（持续）
{"channel": "l2Book", "data": {"perpetualId": 0, "type": "delta", "bids": [["95000.0", "15.5"]], "asks": [], "timestamp": 1707123456790}}
```

### 8.2 多频道订阅

```
# 同时订阅多个频道
{"method": "subscribe", "subscription": {"type": "trades", "perpetualId": 0}}
{"method": "subscribe", "subscription": {"type": "l2Book", "perpetualId": 0}}
{"method": "subscribe", "subscription": {"type": "candle", "perpetualId": 0, "interval": "1m"}}
```

---

## 9. 与 Hyperliquid 对比

| 特性 | Hyperliquid | 本项目 | 说明 |
|------|-------------|--------|------|
| 订阅格式 | `{"method": "subscribe", "subscription": {...}}` | 相同 | 保持兼容 |
| 频道命名 | trades, l2Book, candle | 相同 | 保持兼容 |
| 快照/增量 | snapshot + delta | 相同 | l2Book 模式一致 |
| 心跳 | ping/pong JSON | 相同 | 保持兼容 |
| 认证 | 签名认证 | 暂无 | 公开数据无需认证 |

---

## 附录：参考资料

- [Hyperliquid WebSocket API](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket)
- `04-design-decisions.md` - 设计决策记录
- `10-redis-message-spec.md` - Redis Stream 消息规范
