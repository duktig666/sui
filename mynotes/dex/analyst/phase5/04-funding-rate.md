# 04 资金费率完善

> 日期：2026-02-25
> 依赖：部分无依赖（Redis 发布），部分依赖工程师 B（cumFunding、预测费率）
> 优先级：P0（Redis 发布）/ P1（cumFunding）

---

## 一、当前状态

### 1.1 已实现

**事件**：`FundingSettlementEvent` 已在 `dex_events.rs` 定义

```rust
pub struct FundingSettlementEvent {
    pub perpetual_id: u32,
    pub subaccount: Vec<u8>,        // 36 bytes
    pub funding_payment: i128,      // 正=收到, 负=支付
    pub funding_rate: i64,          // scaled by 1e18
    pub position_size: i64,
    pub timestamp_ms: u64,
}
```

**Handler**：`funding_payments.rs` 已实现
- 处理 `FundingSettlementEvent` → 写入 `dex_funding_payments` 表
- **不发 Redis Stream**（其他所有 handler 都发）

**数据库表**：`dex_funding_payments`
```
cp_sequence_number, tx_sequence_number, event_index, tx_digest,
account_address, subaccount_number, perpetual_id,
funding_payment (BYTEA/i128), funding_rate (BIGINT/i64), position_size (BIGINT),
timestamp_ms
```

**API 端点**（已实现）：
- `userFunding` — 查询用户资金费历史，JOIN `dex_perpetuals` 获取 ticker
- `fundingHistory` — 查询全局资金费历史（按 perpetual_id）

### 1.2 未实现

| 缺失项 | 说明 |
|--------|------|
| Redis Stream 发布 | funding_payments.rs 的 commit() 不发 Redis |
| WS userFundings 频道 | 无实时资金费推送 |
| cumFunding 字段 | dex_positions 表无累计资金费列 |
| 预测资金费率 | 无预测下一次结算的费率（需 mark price） |
| clearinghouseState.cumFunding | 返回中无累计资金费信息 |

---

## 二、Hyperliquid 规范

### 2.1 userFunding 端点

```json
{
  "type": "userFunding",
  "user": "0x...",
  "startTime": 1700000000000,
  "endTime": 1700100000000
}
```

响应：
```json
[
  {
    "time": 1700000000000,
    "coin": "BTC",
    "usdc": "12.345678",       // 资金费支付 (正=收到)
    "szi": "0.5",              // 结算时持仓
    "fundingRate": "0.0001",   // 资金费率
    "hash": "0x..."            // tx digest (Hyperliquid 有)
  }
]
```

当前 DEX 实现基本匹配，字段名不同：
- DEX: `usdc` → `funding_payment` (i128 decimal string)
- DEX: `szi` → `position_size` (i64 string)
- DEX: 有 `perpetual_id` 字段（Hyperliquid 只有 `coin`）

### 2.2 clearinghouseState 中的 cumFunding

```json
{
  "assetPositions": [{
    "position": {
      "coin": "BTC",
      "cumFunding": {
        "sinceOpen": "-12.5",    // 自开仓以来累计
        "sinceChange": "-5.2",   // 自上次变更以来
        "allTime": "-100.0"      // 账户总累计
      }
    }
  }]
}
```

### 2.3 metaAndAssetCtxs 中的资金费率

```json
{
  "funding": "0.00003125",       // 当前预测费率（8小时一次）
  "premium": "0.0001"            // 溢价
}
```

这个"预测费率"来自 mark price 与 index price 的偏差，不是 FundingSettlementEvent 的历史费率。

### 2.4 WS userFundings 频道

Hyperliquid 没有独立的 `userFundings` WS 频道。资金费变更通过 `user:{address}` 频道推送。但我们可以提供更好的体验。

---

## 三、Part 1 工作：FundingPayments Redis 发布（无依赖）

### 3.1 修改 funding_payments.rs

当前 commit() 方法只做 PG INSERT，需增加 Redis publish：

```rust
// funding_payments.rs - commit()

// 现有代码：PG INSERT
diesel::insert_into(dex_funding_payments::table)
    .values(values)
    .on_conflict_do_nothing()
    .execute(conn)?;

// 新增：Redis Stream 发布（参照 balances.rs 模式）
#[cfg(feature = "redis-publish")]
if let Some(redis) = &self.redis {
    let mut redis = redis.clone();
    let messages: Vec<String> = values.iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();

    tokio::spawn(async move {
        for msg in messages {
            let _: Result<String, _> = redis::cmd("XADD")
                .arg("dex:stream:funding_settlements")
                .arg("MAXLEN")
                .arg("~")
                .arg("100000")
                .arg("*")
                .arg("data")
                .arg(&msg)
                .query_async(&mut redis)
                .await;
        }
    });
}
```

### 3.2 Redis Stream Key

```
dex:stream:funding_settlements
```

**消息格式**（与 StoredFundingPayment 序列化一致）：
```json
{
  "cp_sequence_number": 12345,
  "tx_sequence_number": 0,
  "event_index": 0,
  "tx_digest": "base64...",
  "account_address": "0xabc...",
  "subaccount_number": 0,
  "perpetual_id": 0,
  "funding_payment": "base64_i128...",
  "funding_rate": 100000000000000,
  "position_size": 500,
  "timestamp_ms": 1700000000000
}
```

### 3.3 consumer.rs 新增消费

```rust
// stream_keys
pub const FUNDING_SETTLEMENTS: &str = "dex:stream:funding_settlements";

// broadcast 路由：推送到 user:{address} 频道
fn handle_funding_settlement_message(data: &str, ws_state: &WsState) {
    if let Ok(msg) = serde_json::from_str::<Value>(data) {
        if let Some(address) = msg.get("account_address").and_then(|v| v.as_str()) {
            let channel = format!("user:{}", address);
            let server_msg = ServerMessage::ChannelData {
                channel,
                data: json!({
                    "updateType": "funding",
                    "data": msg,
                }),
            };
            ws_state.broadcast_to_channel(&channel, &server_msg);
        }
    }
}
```

---

## 四、Part 2 工作：cumFunding 维护（依赖引擎/DB migration）

### 4.1 概述

cumFunding 需要在 `dex_positions` 表中维护两个累计字段：
- `cum_funding_since_open` — 自开仓以来累计资金费
- `cum_funding_all_time` — 账户在该市场的总累计资金费

### 4.2 更新时机

在 `funding_payments.rs` 的 `commit()` 中，INSERT 资金费记录后，UPDATE positions 表：

```rust
// 对每条 FundingSettlementEvent，更新对应 position 的 cumFunding
for payment in values {
    diesel::update(dex_positions::table)
        .filter(dex_positions::account_address.eq(&payment.account_address))
        .filter(dex_positions::subaccount_number.eq(payment.subaccount_number))
        .filter(dex_positions::perpetual_id.eq(payment.perpetual_id))
        .set((
            dex_positions::cum_funding_since_open.eq(
                dex_positions::cum_funding_since_open + payment.funding_payment_i64()
            ),
            dex_positions::cum_funding_all_time.eq(
                dex_positions::cum_funding_all_time + payment.funding_payment_i64()
            ),
        ))
        .execute(conn)?;
}
```

**注意**：`cum_funding_since_open` 应在开仓时重置为 0。这个重置由 `positions.rs` handler 在检测到新仓位（size 从 0 变为非 0）时处理。

### 4.3 positions.rs 配合修改

```rust
// positions.rs - commit() 中
// 当 UPSERT 检测到仓位从 0 变为非 0（新开仓）时
// 重置 cum_funding_since_open = 0
if position.size != 0 && previous_size == 0 {
    // 新开仓，重置
    diesel::update(dex_positions::table)
        .filter(/* ... */)
        .set(dex_positions::cum_funding_since_open.eq(0))
        .execute(conn)?;
}
```

### 4.4 clearinghouseState 返回 cumFunding

```rust
// handlers.rs - query_clearinghouse_state()
// AssetPosition 增加 cumFunding 字段

pub struct PositionInfo {
    // 现有字段...
    pub cum_funding: Option<CumFunding>,
}

pub struct CumFunding {
    pub since_open: String,   // i64 as decimal string
    pub all_time: String,
}
```

---

## 五、预测资金费率

### 5.1 来源

预测资金费率来自 `MarkPriceUpdateEvent.funding_rate` 字段（详见 03-mark-price.md）。

### 5.2 展示位置

| 端点/频道 | 字段 | 说明 |
|----------|------|------|
| `metaAndAssetCtxs` | `funding` | 当前预测费率 |
| `activeAssetCtx` WS | `funding` | 实时推送 |
| `clearinghouseState` | 无直接对应 | 通过 cumFunding 间接展示 |

### 5.3 前端展示

前端通常展示：
- "下一次资金费" + 倒计时（距离下一个 8h 结算）
- "预测费率"（正=多头支付，负=空头支付）
- 历史费率图表

---

## 六、文件清单

### Part 1（无依赖）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-indexer/src/handlers/funding_payments.rs` | 修改 | commit() 增加 Redis publish |
| `dex-api/src/ws/consumer.rs` | 修改 | 新增 FUNDING_SETTLEMENTS stream 消费 |

### Part 2（依赖 M2 migration + 引擎）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-indexer/src/handlers/funding_payments.rs` | 修改 | UPDATE dex_positions.cumFunding |
| `dex-indexer/src/handlers/positions.rs` | 修改 | 新开仓重置 cum_funding_since_open |
| `dex-indexer/src/schema/mod.rs` | 修改 | StoredPosition 新字段 |
| `dex-types/src/api/responses.rs` | 修改 | CumFunding 类型 + PositionInfo 扩展 |
| `dex-api/src/handlers.rs` | 修改 | clearinghouseState 返回 cumFunding |

---

## 七、依赖关系

```
Part 1（立即可做）：
    funding_payments.rs → Redis Stream
    consumer.rs → 广播到 WS

Part 2：
    M2 migration (positions 增加 cumFunding 列)
        ↓
    funding_payments.rs → UPDATE dex_positions
    positions.rs → 新开仓重置
        ↓
    clearinghouseState → 返回 cumFunding

    MarkPriceUpdateEvent (工程师 B)
        ↓
    mark_prices handler → Redis Hash funding_rate
        ↓
    metaAndAssetCtxs → 展示预测费率
    activeAssetCtx WS → 实时推送
```
