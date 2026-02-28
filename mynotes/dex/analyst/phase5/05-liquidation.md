# 05 清算适配

> 日期：2026-02-25
> 依赖：Part 1 无依赖（handler 实现），Part 2 依赖工程师 B（清算价计算）
> 优先级：P0（handler）/ P1（清算价）

---

## 一、当前状态

### 1.1 LiquidationEvent 已定义

```rust
// dex_events.rs
pub struct LiquidationEvent {
    pub perpetual_id: u32,
    pub liquidated_subaccount: Vec<u8>,  // 36 bytes
    pub liquidator_subaccount: Vec<u8>,  // 36 bytes
    pub size_liquidated: u64,
    pub liquidation_price: u64,
    pub insurance_payout: u64,
    pub timestamp_ms: u64,
}
```

### 1.2 缺失部分

| 缺失项 | 说明 |
|--------|------|
| Handler | 无 `liquidations.rs`，LiquidationEvent 被忽略 |
| DB 表 | 无 `dex_liquidations` 表 |
| Redis Stream | 无 `dex:stream:liquidations` |
| API 端点 | 无清算记录查询端点 |
| WS 推送 | 无清算事件推送 |
| liquidationPx | `clearinghouseState` 中 `liquidation_px` 始终 None |

### 1.3 相关联的已有实现

- `PositionUpdateEvent.reason = 1` (Liquidation) — positions handler 已处理
- `OrderRemovedEventV1.reason = 3` (Liquidation) — order_removals handler 已处理
- `userNonFundingLedgerUpdates` — 目前不包含 liquidation 类型

---

## 二、Hyperliquid 规范

### 2.1 清算相关端点

Hyperliquid 不提供独立的 "查询清算记录" 端点。清算信息通过以下方式暴露：

1. **userFills** — 清算成交会出现在 fills 中，标记为 `liquidation: true`
2. **userNonFundingLedgerUpdates** — 清算导致的余额变化，`type: "liquidation"`
3. **clearinghouseState** — `liquidationPx` 字段显示当前持仓的预估清算价

### 2.2 liquidationPx 计算

Hyperliquid 的清算价计算：

```
对于全仓保证金（Cross Margin）:
  多头: liquidation_px = entry_px * (1 - 1/leverage + maintenance_margin)
  空头: liquidation_px = entry_px * (1 + 1/leverage - maintenance_margin)

更精确的版本（考虑所有仓位和余额）:
  liquidation_px = mark_price at which account_value <= maintenance_margin_required

  其中:
    account_value = quote_balance + sum(unrealized_pnl_i)
    maintenance_margin_required = sum(|position_value_i| * maintenance_margin_ppm_i / 1e6)
```

### 2.3 WS 推送

清算事件不通过专门的 WS 频道推送。被清算用户会收到：
- `user:{address}` 频道的 position update（仓位归零或减少）
- `user:{address}` 频道的 balance update（保证金扣除）

---

## 三、Part 1：Liquidation Handler 实现（无依赖）

### 3.1 新建 liquidations.rs

```rust
// dex-indexer/src/handlers/liquidations.rs

use crate::schema::{dex_liquidations, StoredLiquidation};
use diesel::prelude::*;
use sui_indexer_alt_framework::pipeline::{Handler, Processor};
use sui_types::dex_events::LiquidationEvent;

pub struct Liquidations;

impl Processor for Liquidations {
    const NAME: &'static str = "dex_liquidations";
    type Value = StoredLiquidation;

    fn process(checkpoint: &CheckpointData) -> Result<Vec<Self::Value>> {
        let mut values = vec![];

        for tx in &checkpoint.transactions {
            for (event_index, event) in tx.events.iter().enumerate() {
                if let Some(liq) = LiquidationEvent::try_from_event(event) {
                    let (liquidated_addr, liquidated_sub) =
                        split_subaccount(&liq.liquidated_subaccount);
                    let (liquidator_addr, liquidator_sub) =
                        split_subaccount(&liq.liquidator_subaccount);

                    values.push(StoredLiquidation {
                        cp_sequence_number: checkpoint.sequence_number as i64,
                        tx_sequence_number: tx.sequence_number as i64,
                        event_index: event_index as i32,
                        tx_digest: tx.digest().to_vec(),
                        perpetual_id: liq.perpetual_id as i32,
                        liquidated_account_address: liquidated_addr,
                        liquidated_subaccount_number: liquidated_sub,
                        liquidator_account_address: liquidator_addr,
                        liquidator_subaccount_number: liquidator_sub,
                        size_liquidated: liq.size_liquidated as i64,
                        liquidation_price: liq.liquidation_price as i64,
                        insurance_payout: liq.insurance_payout as i64,
                        timestamp_ms: liq.timestamp_ms as i64,
                    });
                }
            }
        }

        Ok(values)
    }
}

impl Handler for Liquidations {
    async fn commit(values: &[Self::Value], conn: &mut PgConnection) -> Result<usize> {
        // PG INSERT
        let count = diesel::insert_into(dex_liquidations::table)
            .values(values)
            .on_conflict_do_nothing()
            .execute(conn)?;

        // Redis publish
        #[cfg(feature = "redis-publish")]
        if let Some(redis) = &self.redis {
            let mut redis = redis.clone();
            let messages: Vec<String> = values.iter()
                .map(|v| serde_json::to_string(v).unwrap())
                .collect();

            tokio::spawn(async move {
                for msg in messages {
                    let _: Result<String, _> = redis::cmd("XADD")
                        .arg("dex:stream:liquidations")
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

        Ok(count)
    }
}
```

### 3.2 StoredLiquidation

```rust
// schema/mod.rs

diesel::table! {
    dex_liquidations (cp_sequence_number, tx_sequence_number, event_index) {
        cp_sequence_number -> Int8,
        tx_sequence_number -> Int8,
        event_index -> Int4,
        tx_digest -> Bytea,
        perpetual_id -> Int4,
        liquidated_account_address -> Text,
        liquidated_subaccount_number -> Int4,
        liquidator_account_address -> Text,
        liquidator_subaccount_number -> Int4,
        size_liquidated -> Int8,
        liquidation_price -> Int8,
        insurance_payout -> Int8,
        timestamp_ms -> Int8,
    }
}

#[derive(Insertable, Queryable, Serialize)]
#[diesel(table_name = dex_liquidations)]
pub struct StoredLiquidation {
    pub cp_sequence_number: i64,
    pub tx_sequence_number: i64,
    pub event_index: i32,
    pub tx_digest: Vec<u8>,
    pub perpetual_id: i32,
    pub liquidated_account_address: String,
    pub liquidated_subaccount_number: i32,
    pub liquidator_account_address: String,
    pub liquidator_subaccount_number: i32,
    pub size_liquidated: i64,
    pub liquidation_price: i64,
    pub insurance_payout: i64,
    pub timestamp_ms: i64,
}
```

### 3.3 注册 Handler

```rust
// handlers/mod.rs
pub mod liquidations;

// lib.rs 或 main.rs 中
indexer.add_handler::<Liquidations>();
```

### 3.4 Redis Stream 消费

```rust
// consumer.rs
pub const LIQUIDATIONS: &str = "dex:stream:liquidations";

// 广播路由
fn handle_liquidation_message(data: &str, ws_state: &WsState) {
    if let Ok(msg) = serde_json::from_str::<Value>(data) {
        // 推送给被清算用户
        if let Some(addr) = msg.get("liquidated_account_address").and_then(|v| v.as_str()) {
            ws_state.broadcast_to_channel(
                &format!("user:{}", addr),
                &ServerMessage::ChannelData {
                    channel: format!("user:{}", addr),
                    data: json!({"updateType": "liquidation", "data": msg}),
                },
            );
        }
    }
}
```

---

## 四、Part 1：userNonFundingLedgerUpdates 包含清算

### 4.1 当前实现

`query_user_non_funding_ledger_updates()` 查询 `dex_balances` + `dex_transfers`，不包含清算记录。

### 4.2 扩展

增加从 `dex_liquidations` 查询，合并到 ledger updates 中：

```rust
// handlers.rs - query_user_non_funding_ledger_updates()

// 新增：查询清算记录
let liquidations = dex_liquidations::table
    .filter(dex_liquidations::liquidated_account_address.eq(&req.user))
    .filter(/* 时间范围 */)
    .order(dex_liquidations::timestamp_ms.desc())
    .load::<StoredLiquidation>(conn)?;

// 转换为 LedgerUpdate
for liq in liquidations {
    updates.push(LedgerUpdate {
        time: liq.timestamp_ms,
        hash: format!("0x{}", hex::encode(&liq.tx_digest)),
        delta: format!("-{}", liq.insurance_payout),  // 保险金扣除
        update_type: "liquidation".to_string(),
        perpetual_id: Some(liq.perpetual_id),
        subaccount_number: Some(liq.liquidated_subaccount_number),
    });
}
```

---

## 五、Part 2：liquidationPx 计算（依赖工程师 B）

### 5.1 概述

`clearinghouseState` 中的 `liquidation_px` 需要根据持仓、保证金模式、维持保证金率来计算。

### 5.2 全仓模式清算价

```
给定：
  P = 持仓数量 (szi)
  E = 入场价 (entry_px)
  B = 可用余额 (quote_balance + sum(unrealized_pnl from other positions))
  M_maint = 维持保证金率 (maintenance_margin_ppm / 1e6)

多头 (P > 0):
  liquidation_px = E - (B - |P| * E * M_maint) / |P|
  简化: liquidation_px = E - B/|P| + E * M_maint

空头 (P < 0):
  liquidation_px = E + (B - |P| * E * M_maint) / |P|
  简化: liquidation_px = E + B/|P| - E * M_maint
```

### 5.3 逐仓模式清算价

```
给定：
  P = 持仓数量
  E = 入场价
  IM = 逐仓保证金 (isolated_margin)
  M_maint = 维持保证金率

多头: liquidation_px = E * (1 - IM/(|P|*E) + M_maint)
空头: liquidation_px = E * (1 + IM/(|P|*E) - M_maint)
```

### 5.4 实现位置

```rust
// handlers.rs - query_clearinghouse_state()

fn calculate_liquidation_price(
    position: &StoredPosition,
    market_config: &MarketConfig,
    account_equity: i128,  // 全仓模式需要
) -> Option<i64> {
    if position.size == 0 {
        return None;
    }

    let maint_margin = market_config.maintenance_margin_ppm as f64 / 1_000_000.0;
    let entry_px = position.avg_entry_price as f64;
    let abs_size = position.size.unsigned_abs() as f64;

    match position.margin_mode {
        0 => {
            // Cross margin
            let available = account_equity as f64;
            if position.size > 0 {
                // 多头
                let liq = entry_px - available / abs_size + entry_px * maint_margin;
                Some(liq.max(0.0) as i64)
            } else {
                // 空头
                let liq = entry_px + available / abs_size - entry_px * maint_margin;
                Some(liq as i64)
            }
        }
        1 => {
            // Isolated margin
            let isolated = i128::from_le_bytes(
                position.isolated_margin.as_ref()?.try_into().ok()?
            ) as f64;
            if position.size > 0 {
                let liq = entry_px * (1.0 - isolated / (abs_size * entry_px) + maint_margin);
                Some(liq.max(0.0) as i64)
            } else {
                let liq = entry_px * (1.0 + isolated / (abs_size * entry_px) - maint_margin);
                Some(liq as i64)
            }
        }
        _ => None,
    }
}
```

**前提条件**：
- `maintenance_margin_ppm` 需要从 DB 获取（M4 migration）
- `margin_mode` 和 `isolated_margin` 需要从 DB 获取（M2 migration）
- mark price 可用时，清算价计算更精确

---

## 六、Part 1：userFills 增加 is_liquidation 标记

### 6.1 问题

清算成交会产生 FillEvent，但当前无法区分普通成交和清算成交。

### 6.2 方案

**方案 A**：引擎在 FillEvent 中增加 `is_liquidation` 字段
- 优点：精确
- 缺点：需要引擎修改

**方案 B**：通过时间窗口关联 LiquidationEvent 和 FillEvent
- 同一 checkpoint 内，同一 subaccount + perpetual 的 LiquidationEvent 和 FillEvent 可关联
- 优点：不需引擎修改
- 缺点：可能有误匹配

**推荐方案 A**，与工程师 B 讨论。

---

## 七、文件清单

### Part 1（无依赖）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-indexer/migrations/2026-02-25-000001_*/up.sql` | 新建 | M1: dex_liquidations 表 |
| `dex-indexer/src/handlers/liquidations.rs` | 新建 | Liquidation handler |
| `dex-indexer/src/handlers/mod.rs` | 修改 | 注册 handler |
| `dex-indexer/src/schema/mod.rs` | 修改 | table! + StoredLiquidation |
| `dex-api/src/handlers.rs` | 修改 | userNonFundingLedgerUpdates 包含清算 |
| `dex-api/src/ws/consumer.rs` | 修改 | 清算 stream 消费 |

### Part 2（依赖引擎）

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `dex-api/src/handlers.rs` | 修改 | liquidationPx 计算 |
| `sui-types/src/dex_events.rs` | **引擎修改** | FillEvent 增加 is_liquidation（可选） |

---

## 八、依赖关系

```
Part 1（立即可做）：
    M1 (dex_liquidations 表)
        ↓
    liquidations.rs handler
        ├─→ dex_liquidations (PG)
        ├─→ dex:stream:liquidations (Redis)
        └─→ userNonFundingLedgerUpdates 包含清算

Part 2：
    M2 (positions 增加 margin_mode, isolated_margin)
    M4 (perpetuals 增加 maintenance_margin_ppm)
        ↓
    liquidationPx 计算
        ↓
    clearinghouseState 返回 liquidation_px
```
