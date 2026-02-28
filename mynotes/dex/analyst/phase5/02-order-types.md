# 02 新订单类型适配（TP/SL）

> 日期：2026-02-25
> 依赖：工程师 B（TP/SL 订单引擎实现）
> 优先级：P1

---

## 一、当前状态

### 1.1 已有订单类型

`OrderPlacedEventV1` 中 `order_type` 字段已定义 4 种类型：
- `0` = Limit
- `1` = Market
- `2` = StopLimit
- `3` = StopMarket

但 StopLimit / StopMarket 引擎层尚未实现，当前只有 Limit 和 Market 实际使用。

### 1.2 已有 Exchange API

`ExchangeAction::Order` 已支持 `OrderWire` 中的触发参数：

```rust
// exchange.rs - 已定义
pub struct OrderWire {
    pub t: OrderType,  // 包含 limit + trigger
    // ...
}

pub struct TriggerOrderType {
    pub trigger_px: String,
    pub tpsl: String,       // "tp" | "sl"
    pub is_market: bool,
}
```

说明前端下单接口已部分就绪，但引擎和 indexer 层尚未对接。

### 1.3 已有订单表

`dex_orders` 表当前缺少 TP/SL 相关字段：
- 无 `trigger_price`
- 无 `trigger_condition`
- 无 `parent_order_id`
- 无 `grouping`

M5 migration 将补充这些字段。

---

## 二、Hyperliquid 规范

### 2.1 frontendOpenOrders 端点

Hyperliquid 提供专门的 `frontendOpenOrders` 端点，返回比 `openOrders` 更丰富的信息：

```json
{
  "type": "frontendOpenOrders",
  "user": "0x..."
}
```

响应格式：
```json
[
  {
    "coin": "BTC",
    "limitPx": "90000.0",
    "oid": 12345,
    "side": "B",
    "sz": "0.1",
    "timestamp": 1700000000000,
    "triggerPx": "95000.0",       // TP/SL 触发价
    "triggerCondition": "tp",     // "tp" | "sl"
    "orderType": "Stop Market",   // 显示类型
    "children": [                 // 关联的子单
      {"oid": 12346, "triggerPx": "95000.0", "triggerCondition": "tp"},
      {"oid": 12347, "triggerPx": "85000.0", "triggerCondition": "sl"}
    ],
    "reduceOnly": true,
    "cloid": null
  }
]
```

### 2.2 下单 grouping 语义

| grouping 值 | 含义 | 订单数 |
|------------|------|-------|
| `"na"` | 普通单 | 1 |
| `"normalTpsl"` | 主单 + TP + SL | 1-3 |
| `"positionTpsl"` | 仓位级别的 TP/SL | 1-2 |

### 2.3 orderUpdates WS 扩展

Hyperliquid 的 `orderUpdates` 频道推送中包含 triggerPx 字段：
```json
{
  "order": {
    "coin": "BTC",
    "oid": 12345,
    "triggerPx": "95000.0",
    "triggerCondition": "tp",
    "orderType": "Take Profit Market"
  },
  "status": "open",
  "statusTimestamp": 1700000000000
}
```

---

## 三、需要引擎提供的支持

### 3.1 OrderPlacedEventV1 扩展

引擎需要在 `OrderPlacedEventV1` 中增加字段：

```rust
// dex_events.rs - 需要工程师 B 扩展
pub struct OrderPlacedEventV1 {
    // 现有字段...
    pub perpetual_id: u32,
    pub order_id: Vec<u8>,
    pub subaccount: Vec<u8>,
    pub side: u8,
    pub price: u64,
    pub quantity: u64,
    pub order_type: u8,
    pub time_in_force: u8,
    pub reduce_only: bool,
    pub client_id: u64,
    pub timestamp_ms: u64,

    // 新增字段
    pub trigger_price: u64,       // 0 表示无触发价
    pub trigger_condition: u8,    // 0=None, 1=TakeProfit, 2=StopLoss
    pub parent_order_id: Vec<u8>, // 空 vec 表示无父单
    pub grouping: u8,             // 0=na, 1=normalTpsl, 2=positionTpsl
}
```

### 3.2 OrderUpdateEvent 兼容

`OrderUpdateEvent` 无需修改 — 它是通用的订单状态追踪事件，TP/SL 订单的触发/取消也会通过它推送。

### 3.3 OrderRemovedEventV1 reason 扩展

可能需要新增 reason 值：
- `4` = TriggerActivated（触发激活后转为市价单）

---

## 四、Indexer 适配

### 4.1 orders.rs 修改

```rust
// 处理 OrderPlacedEventV1 新字段
let trigger_price = if event.trigger_price > 0 {
    Some(event.trigger_price as i64)
} else {
    None
};

let trigger_condition = match event.trigger_condition {
    1 => Some("tp".to_string()),
    2 => Some("sl".to_string()),
    _ => None,
};

let parent_order_id = if event.parent_order_id.is_empty() {
    None
} else {
    Some(event.parent_order_id.clone())
};

let grouping = match event.grouping {
    1 => "normalTpsl",
    2 => "positionTpsl",
    _ => "na",
};
```

**StoredOrder 结构体更新**：
```rust
pub struct StoredOrder {
    // 现有字段...
    pub trigger_price: Option<i64>,
    pub trigger_condition: Option<String>,
    pub parent_order_id: Option<Vec<u8>>,
    pub grouping: String,
}
```

### 4.2 order_updates.rs 修改

当 `OrderUpdateEvent` 触发 INSERT fallback（taker 订单未经 OrderPlacedEventV1）时，新字段设为默认值：
- `trigger_price`: None
- `trigger_condition`: None
- `parent_order_id`: None
- `grouping`: "na"

### 4.3 Redis 消息扩展

orders stream 消息增加新字段，WS consumer 广播时包含 triggerPx 等信息。

---

## 五、API 适配

### 5.1 OrderResponse 扩展

```rust
pub struct OrderResponse {
    // 现有字段...
    pub trigger_price: Option<i64>,
    pub trigger_condition: Option<String>,
    pub parent_order_id: Option<String>,   // hex
    pub grouping: String,
}
```

影响端点：`openOrders`, `historicalOrders`, `orderStatus`

### 5.2 新增 frontendOpenOrders 端点

```rust
// requests.rs
pub struct FrontendOpenOrdersRequest {
    pub user: String,
    pub subaccount_number: Option<u8>,
}

// handlers.rs
pub async fn query_frontend_open_orders(db, req) -> Result<Vec<FrontendOpenOrderResponse>> {
    // 1. 查询所有 open/partial 订单（同 openOrders）
    // 2. 对有 parent_order_id 的订单建立父子关系
    // 3. 返回富化的订单列表（含 children 数组）
}
```

**FrontendOpenOrderResponse**：
```rust
pub struct FrontendOpenOrderResponse {
    // OrderResponse 所有字段 +
    pub order_type_display: String,  // "Limit", "Take Profit Market", "Stop Loss Limit" 等
    pub children: Vec<ChildOrder>,
}

pub struct ChildOrder {
    pub order_id: String,
    pub trigger_price: Option<i64>,
    pub trigger_condition: Option<String>,
}
```

### 5.3 server.rs 路由

```rust
InfoRequest::FrontendOpenOrders(req) => handlers::query_frontend_open_orders(&state.db, req).await
```

---

## 六、WS 适配

### 6.1 orderUpdates 频道扩展

消息格式增加 triggerPx 字段（在 consumer.rs 广播时）：

```json
{
  "channel": "orderUpdates:0xabc...",
  "data": {
    "orderId": "0x...",
    "perpetualId": 0,
    "side": 0,
    "price": 90000,
    "quantity": 100,
    "status": 0,
    "triggerPrice": 95000,
    "triggerCondition": "tp",
    "grouping": "normalTpsl"
  }
}
```

### 6.2 openOrders 频道扩展

同步增加 triggerPx 字段。

---

## 七、Exchange 适配

### 7.1 handle_order 修改

当前 `handle_order` 已解析 `OrderWire.t.trigger`，但未传递给引擎。需要：

1. 从 `TriggerOrderType` 提取 `trigger_px`, `tpsl`, `is_market`
2. 传递给 `Eip712PlaceOrderParams`（需要引擎扩展此结构体）
3. 或新增 `Eip712PlaceTriggerOrderParams`

### 7.2 grouping 传递

当前 `OrderAction.grouping` 已存在，需要传递到引擎层。引擎需要支持批量下单（主单 + TP + SL 组合）。

---

## 八、文件清单

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `sui-types/src/dex_events.rs` | **引擎修改** | OrderPlacedEventV1 新增字段 |
| `dex-indexer/src/handlers/orders.rs` | 修改 | 解析新字段 |
| `dex-indexer/src/handlers/order_updates.rs` | 修改 | INSERT fallback 默认值 |
| `dex-indexer/src/schema/mod.rs` | 修改 | StoredOrder 新字段 |
| `dex-types/src/api/responses.rs` | 修改 | OrderResponse 新字段 + FrontendOpenOrderResponse |
| `dex-types/src/api/requests.rs` | 修改 | FrontendOpenOrdersRequest |
| `dex-api/src/handlers.rs` | 修改 | query_frontend_open_orders 实现 |
| `dex-api/src/server.rs` | 修改 | 路由注册 |
| `dex-api/src/ws/consumer.rs` | 修改 | 广播消息格式 |

---

## 九、依赖关系

```
M5 (DB migration)
    ↓
orders.rs / order_updates.rs (indexer)  ← 等待引擎 OrderPlacedEventV1 扩展
    ↓
OrderResponse 扩展 (API types)
    ↓
frontendOpenOrders (API handler)
    ↓
orderUpdates WS 消息格式 (WS)
```
