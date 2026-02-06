# Phase 2 Redis Stream 消息格式规范

## 概述

本文档定义 dex-realtime 发布到 Redis Stream 的消息格式规范，包括 Stream 命名、消息结构、去重机制和生命周期管理。

---

## 1. Stream 命名规范

### 1.1 命名格式

```
dex:stream:{event_type}
```

### 1.2 已定义 Stream

| Stream 名称 | 事件类型 | 发布来源 | 消费者 |
|-------------|----------|----------|--------|
| `dex:stream:fills` | 成交事件 | dex-realtime, dex-indexer | dex-ws |
| `dex:stream:orders` | 订单事件 | dex-realtime, dex-indexer | dex-ws |
| `dex:stream:positions` | 持仓事件 | dex-realtime, dex-indexer | dex-ws |
| `dex:stream:liquidations` | 清算事件 | dex-realtime, dex-indexer | dex-ws |

---

## 2. 消息结构规范

### 2.1 通用字段

所有消息包含以下通用字段：

| 字段 | 类型 | 说明 | 示例 |
|------|------|------|------|
| `event_id` | string | Sui 事件唯一标识 | `"7xK9...abc-0"` |
| `event_type` | string | 事件类型 | `"FillEventV1"` |
| `perpetual_id` | string | 永续合约 ID | `"0"` |
| `timestamp` | string | 事件时间戳（毫秒） | `"1707123456789"` |
| `data` | string | JSON 序列化的事件数据 | `"{...}"` |

### 2.2 XADD 命令格式

```bash
XADD dex:stream:fills MAXLEN ~ 10000 * \
    event_id "7xK9...abc-0" \
    event_type "FillEventV1" \
    perpetual_id "0" \
    timestamp "1707123456789" \
    data '{"taker_subaccount":"0x...","maker_subaccount":"0x...","side":"Buy","price":"95000000000","quantity":"100000000","taker_fee":"50000","maker_fee":"-25000"}'
```

---

## 3. 事件数据格式

### 3.1 FillEventV1 (成交事件)

**Stream**: `dex:stream:fills`

**data 字段内容**：

```json
{
    "taker_subaccount": "0x1234567890abcdef...",
    "maker_subaccount": "0xfedcba0987654321...",
    "taker_order_id": "0xabc123...",
    "maker_order_id": "0xdef456...",
    "side": "Buy",
    "price": "95000000000",
    "quantity": "100000000",
    "taker_fee": "50000",
    "maker_fee": "-25000"
}
```

| 字段 | 类型 | 说明 | 精度 |
|------|------|------|------|
| `taker_subaccount` | hex string | Taker 子账户地址 | - |
| `maker_subaccount` | hex string | Maker 子账户地址 | - |
| `taker_order_id` | hex string | Taker 订单 ID | - |
| `maker_order_id` | hex string | Maker 订单 ID | - |
| `side` | string | Taker 方向 | "Buy" / "Sell" |
| `price` | string | 成交价格 | 10^9 (9 位小数) |
| `quantity` | string | 成交数量 | 10^8 (8 位小数) |
| `taker_fee` | string | Taker 手续费 | 10^6 (6 位小数) |
| `maker_fee` | string | Maker 手续费（负数为返佣） | 10^6 |

---

### 3.2 OrderPlacedEventV1 (订单挂单事件)

**Stream**: `dex:stream:orders`

**data 字段内容**：

```json
{
    "order_id": "0xabc123...",
    "subaccount": "0x1234567890abcdef...",
    "side": "Buy",
    "price": "95000000000",
    "quantity": "500000000",
    "order_type": "Limit",
    "reduce_only": false,
    "client_order_id": "12345"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `order_id` | hex string | 订单唯一 ID |
| `subaccount` | hex string | 子账户地址 |
| `side` | string | 订单方向 |
| `price` | string | 订单价格 |
| `quantity` | string | 订单数量（剩余数量，非原始数量） |
| `order_type` | string | 订单类型：Limit, PostOnly |
| `reduce_only` | boolean | 是否仅减仓 |
| `client_order_id` | string | 客户端订单 ID（可选） |

---

### 3.3 OrderRemovedEventV1 (订单移除事件)

**Stream**: `dex:stream:orders`

**data 字段内容**：

```json
{
    "order_id": "0xabc123...",
    "subaccount": "0x1234567890abcdef...",
    "reason": "Filled",
    "total_filled_quantity": "500000000",
    "remaining_quantity": "0"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `order_id` | hex string | 订单唯一 ID |
| `subaccount` | hex string | 子账户地址 |
| `reason` | string | 移除原因：Filled, Cancelled, Expired, Liquidation |
| `total_filled_quantity` | string | 累计成交数量 |
| `remaining_quantity` | string | 移除时剩余数量 |

---

### 3.4 PositionUpdateEventV1 (持仓更新事件)

**Stream**: `dex:stream:positions`

**data 字段内容**：

```json
{
    "subaccount": "0x1234567890abcdef...",
    "side": "Long",
    "size": "1000000000",
    "avg_entry_price": "95500000000",
    "unrealized_pnl": "50000000"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `subaccount` | hex string | 子账户地址 |
| `side` | string | 持仓方向：Long, Short, None |
| `size` | string | 持仓数量（绝对值） |
| `avg_entry_price` | string | 平均入场价格 |
| `unrealized_pnl` | string | 未实现盈亏 |

---

### 3.5 LiquidationEventV1 (清算事件)

**Stream**: `dex:stream:liquidations`

**data 字段内容**：

```json
{
    "subaccount": "0x1234567890abcdef...",
    "liquidator": "0xfedcba0987654321...",
    "side": "Long",
    "quantity": "500000000",
    "price": "94000000000",
    "bankruptcy_price": "93500000000"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `subaccount` | hex string | 被清算子账户 |
| `liquidator` | hex string | 清算人地址 |
| `side` | string | 被清算持仓方向 |
| `quantity` | string | 清算数量 |
| `price` | string | 清算价格 |
| `bankruptcy_price` | string | 破产价格 |

---

## 4. 去重机制

### 4.1 去重键格式

```
dex:event:seen:{tx_digest}-{event_seq}
```

**示例**：
```
dex:event:seen:7xK9Abc123DefGhi456Jkl789Mno012Pqr345Stu678Vwx-0
```

### 4.2 去重流程

```rust
// 1. 生成去重键
let event_id = format!("{}-{}", tx_digest, event_seq);
let dedup_key = format!("dex:event:seen:{}", event_id);

// 2. 原子检查并设置
let is_new: bool = redis::cmd("SET")
    .arg(&dedup_key)
    .arg("1")
    .arg("NX")           // 仅键不存在时设置
    .arg("EX")
    .arg(3600)           // 1 小时过期
    .query_async(&mut conn)
    .await?;

// 3. 仅新事件发布
if is_new {
    // XADD to stream
}
```

### 4.3 去重键 TTL

| 参数 | 值 | 说明 |
|------|-----|------|
| TTL | 3600 秒（1 小时） | 覆盖重试窗口和网络延迟 |
| 清理方式 | Redis 自动过期 | 无需手动清理 |

---

## 5. Stream 生命周期管理

### 5.1 长度限制

使用 `MAXLEN ~` 近似修剪，避免精确修剪的性能开销：

```bash
XADD dex:stream:fills MAXLEN ~ 10000 * ...
```

| Stream | MAXLEN | 说明 |
|--------|--------|------|
| `dex:stream:fills` | ~10000 | 约 10 分钟高频交易数据 |
| `dex:stream:orders` | ~10000 | 订单事件 |
| `dex:stream:positions` | ~5000 | 持仓更新相对较少 |
| `dex:stream:liquidations` | ~1000 | 清算事件相对稀少 |

### 5.2 消费者组

每个 Stream 创建消费者组用于 dex-ws 实例负载均衡：

```bash
# 创建消费者组（从最新消息开始）
XGROUP CREATE dex:stream:fills dex-ws-group $ MKSTREAM

# 或从头开始（用于恢复）
XGROUP CREATE dex:stream:fills dex-ws-group 0 MKSTREAM
```

### 5.3 消息确认

dex-ws 消费消息后必须确认：

```bash
XACK dex:stream:fills dex-ws-group <message_id>
```

**未确认消息处理**：
- Pending 消息超过 60 秒自动重新投递
- 使用 `XPENDING` 监控积压情况

---

## 6. 序列化规范

### 6.1 选择 JSON

| 格式 | 优点 | 缺点 | 决策 |
|------|------|------|------|
| **JSON** | 可读性好，调试方便 | 体积较大 | ✅ 选择 |
| BCS | 体积小，性能好 | 不可读，调试困难 | 不选择 |
| Protobuf | 跨语言，体积小 | 需要 schema 定义 | 不选择 |

### 6.2 JSON 编码规则

1. **数字使用字符串**：避免 JavaScript 大数精度问题
2. **地址使用十六进制**：0x 前缀
3. **枚举使用字符串**：如 "Buy"/"Sell"、"Limit"/"Market"
4. **时间戳使用毫秒**：Unix 时间戳 * 1000

---

## 7. 监控指标

### 7.1 关键指标

| 指标 | 获取方式 | 告警阈值 |
|------|----------|----------|
| Stream 长度 | `XLEN dex:stream:fills` | > 8000（接近 MAXLEN） |
| Pending 消息数 | `XPENDING dex:stream:fills dex-ws-group` | > 1000 |
| 消费延迟 | 最老 Pending 消息时间 | > 30 秒 |
| 去重键数量 | `DBSIZE` 采样 | 无告警，仅监控 |

### 7.2 监控命令

```bash
# 查看 Stream 信息
XINFO STREAM dex:stream:fills

# 查看消费者组状态
XINFO GROUPS dex:stream:fills

# 查看 Pending 消息
XPENDING dex:stream:fills dex-ws-group

# 查看消费者信息
XINFO CONSUMERS dex:stream:fills dex-ws-group
```

---

## 8. 示例：完整消息流

### 8.1 成交事件发布

```bash
# dex-realtime 发布成交事件
XADD dex:stream:fills MAXLEN ~ 10000 * \
    event_id "7xK9Abc123-0" \
    event_type "FillEventV1" \
    perpetual_id "0" \
    timestamp "1707123456789" \
    data '{"taker_subaccount":"0x1234...","maker_subaccount":"0x5678...","side":"Buy","price":"95000000000","quantity":"100000000","taker_fee":"50000","maker_fee":"-25000"}'
```

### 8.2 dex-ws 消费

```bash
# 消费者读取
XREADGROUP GROUP dex-ws-group consumer-1 COUNT 10 BLOCK 1000 STREAMS dex:stream:fills >

# 处理后确认
XACK dex:stream:fills dex-ws-group 1707123456789-0
```

---

## 附录：参考资料

- [Redis Streams 官方文档](https://redis.io/docs/data-types/streams/)
- `04-design-decisions.md` - 设计决策记录
- `07-multi-node-consistency.md` - 多节点一致性分析
