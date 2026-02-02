# dex-sui 自定义事件实现分析

## 分析目标

分析 dex-sui 项目 `feature-match-engine-event-indexer` 分支的自定义事件实现：
1. 都在哪里添加了自定义事件
2. 事件添加到 Checkpoint 是否正确
3. sui-indexer-alt 解析 DEX 自定义事件的可行性

## 分支信息

- **分支**: `feature-match-engine-event-indexer`
- **最新提交**: `b00f897ee6 event & indexer`
- **核心改动**: 19 files changed, +1106/-541 lines

## 关键发现

### 1. 严重问题：dex_events.rs 文件缺失

**问题描述**:

```
crates/sui-types/src/lib.rs:50
    pub mod dex_events;   ← 声明了模块

crates/sui-types/src/dex_events.rs
    文件不存在!           ← 但文件缺失
```

**影响范围**:
- `sui-execution/src/dex.rs:25-28` 导入会失败：
  ```rust
  use sui_types::dex_events::{
      CancelReason, DexEvent, OrderCanceledEvent, OrderFilledEvent, OrderPlacedEvent,
      SubaccountCreatedEvent, SubaccountDeletedEvent, SubaccountDepositEvent,
      SubaccountWithdrawEvent, TradeEvent,
  };
  ```
- 代码无法编译
- 所有事件相关功能无法工作

**结论**: 这是一个提交遗漏问题，需要补充 `dex_events.rs` 文件。

---

## 2. 事件发射位置分析

**文件**: `sui-execution/src/dex.rs`

代码使用 `ctx.emit_event(DexEvent::...)` 模式发射事件：

### 2.1 子账户事件

| 行号 | 操作 | 事件类型 | 触发条件 |
|------|------|----------|----------|
| 1135 | 创建子账户 | `SubaccountCreatedEvent` | `execute_subaccount_create` |
| 1206 | 子账户存款 | `SubaccountDepositEvent` | `execute_subaccount_deposit` |
| 1290 | 子账户提款 | `SubaccountWithdrawEvent` | `execute_subaccount_withdraw` |
| 1372 | 删除子账户 | `SubaccountDeletedEvent` | `execute_subaccount_delete` |

### 2.2 订单事件

| 行号 | 操作 | 事件类型 | 触发条件 |
|------|------|----------|----------|
| 1575 | Maker 订单成交 | `OrderFilledEvent` | 撮合过程中 Maker 方 |
| 1635 | 成交记录 | `TradeEvent` | 每笔撮合生成 |
| 1805 | 下单 | `OrderPlacedEvent` | `execute_place_order` |
| 1818 | Taker 订单成交 | `OrderFilledEvent` | Taker 有成交时 |
| 1837 | 订单取消 | `OrderCanceledEvent` | IOC/PostOnly 剩余取消 |
| 2013 | 手动取消订单 | `OrderCanceledEvent` | `execute_cancel_order` |

### 2.3 PlaceOrderV2 事件

| 行号 | 操作 | 事件类型 | 触发条件 |
|------|------|----------|----------|
| 2400 | 下单 | `OrderPlacedEvent` | `execute_place_order_v2` |
| 2498 | 订单取消 | `OrderCanceledEvent` | `execute_cancel_order_v2` |

### 事件类型汇总

```
DexEvent
├── SubaccountCreated(SubaccountCreatedEvent)
├── SubaccountDeposit(SubaccountDepositEvent)
├── SubaccountWithdraw(SubaccountWithdrawEvent)
├── SubaccountDeleted(SubaccountDeletedEvent)
├── OrderPlaced(OrderPlacedEvent)
├── OrderFilled(OrderFilledEvent)
├── OrderCanceled(OrderCanceledEvent)
└── Trade(TradeEvent)
```

---

## 3. 事件收集与转换架构

```
┌─────────────────────────────────────────────────────────────────┐
│                     事件流程架构                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. 事件发射 (dex.rs)                                           │
│     ctx.emit_event(DexEvent::OrderPlaced(...))                  │
│         │                                                        │
│         ▼                                                        │
│  2. 事件收集 (DexExecutionContext, line 2701)                   │
│     struct DexExecutionContext {                                 │
│         events: Vec<DexEvent>,  // 收集所有事件                  │
│     }                                                            │
│         │                                                        │
│         ▼                                                        │
│  3. 事件转换 (build_temporary_store, line 2613-2619)            │
│     TransactionEvents {                                          │
│         data: dex_events.into_iter()                             │
│             .map(|e| e.to_event(transaction_signer))             │
│             .collect()                                           │
│     }                                                            │
│         │                                                        │
│         ▼                                                        │
│  4. 写入 InnerTemporaryStore (line 2627)                        │
│     InnerTemporaryStore {                                        │
│         events: TransactionEvents,  // 传入 Checkpoint 流程     │
│         ...                                                      │
│     }                                                            │
│         │                                                        │
│         ▼                                                        │
│  5. 效果生成 (build_effects, line 2659-2664)                    │
│     events_digest = if events.data.is_empty() {                  │
│         None                                                     │
│     } else {                                                     │
│         Some(events.digest())  // 计算事件摘要                   │
│     }                                                            │
│         │                                                        │
│         ▼                                                        │
│  6. 包含在 TransactionEffects (line 2676)                       │
│     TransactionEffects::new_from_execution_v2(                   │
│         ...,                                                     │
│         events_digest,  // 事件摘要写入效果                      │
│         ...                                                      │
│     )                                                            │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 4. 事件到 Checkpoint 的正确性分析

### 4.1 设计正确性评估

**结论**: ✅ 架构设计正确

当 `dex_events.rs` 补齐后，事件流程符合 Sui 标准：

1. **事件收集**: `DexExecutionContext.events: Vec<DexEvent>` 正确收集所有事件
2. **事件转换**: `to_event()` 将 DEX 事件转为 Sui 标准 `Event` 结构
3. **事件存储**: 写入 `InnerTemporaryStore.events`
4. **摘要计算**: `events.digest()` 正确计算并写入 `TransactionEffects`
5. **Checkpoint 包含**: 事件随交易效果进入 Checkpoint

### 4.2 缺失的 `to_event()` 实现

需要在 `dex_events.rs` 中实现：

```rust
use sui_types::base_types::{ObjectID, SuiAddress};
use sui_types::dex::DEX_ADDRESS;
use sui_types::event::Event;
use move_core_types::ident_str;
use move_core_types::identifier::Identifier;
use move_core_types::language_storage::StructTag;
use serde::{Deserialize, Serialize};

/// DEX 事件枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DexEvent {
    SubaccountCreated(SubaccountCreatedEvent),
    SubaccountDeposit(SubaccountDepositEvent),
    SubaccountWithdraw(SubaccountWithdrawEvent),
    SubaccountDeleted(SubaccountDeletedEvent),
    OrderPlaced(OrderPlacedEvent),
    OrderFilled(OrderFilledEvent),
    OrderCanceled(OrderCanceledEvent),
    Trade(TradeEvent),
}

impl DexEvent {
    /// 转换为 Sui 标准 Event 结构
    pub fn to_event(&self, sender: SuiAddress) -> Event {
        Event {
            package_id: ObjectID::from(DEX_ADDRESS),  // 0x0
            transaction_module: Identifier::from(ident_str!("dex")),
            sender,
            type_: self.struct_tag(),
            contents: bcs::to_bytes(self).expect("DexEvent serialization failed"),
        }
    }

    /// 获取事件的 StructTag
    fn struct_tag(&self) -> StructTag {
        let name = match self {
            DexEvent::SubaccountCreated(_) => "SubaccountCreatedEvent",
            DexEvent::SubaccountDeposit(_) => "SubaccountDepositEvent",
            DexEvent::SubaccountWithdraw(_) => "SubaccountWithdrawEvent",
            DexEvent::SubaccountDeleted(_) => "SubaccountDeletedEvent",
            DexEvent::OrderPlaced(_) => "OrderPlacedEvent",
            DexEvent::OrderFilled(_) => "OrderFilledEvent",
            DexEvent::OrderCanceled(_) => "OrderCanceledEvent",
            DexEvent::Trade(_) => "TradeEvent",
        };
        StructTag {
            address: DEX_ADDRESS,
            module: Identifier::from(ident_str!("dex")),
            name: Identifier::new(name).unwrap(),
            type_params: vec![],
        }
    }
}
```

### 4.3 事件结构体示例

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubaccountCreatedEvent {
    pub subaccount_object_id: ObjectID,
    pub subaccount_id: SubaccountId,
    pub owner: SuiAddress,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderPlacedEvent {
    pub order_id: ObjectID,
    pub subaccount_id: SubaccountId,
    pub perpetual_id: u32,
    pub side: Side,
    pub price: u64,
    pub size: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    pub maker_order_id: ObjectID,
    pub taker_order_id: ObjectID,
    pub maker_subaccount_id: SubaccountId,
    pub taker_subaccount_id: SubaccountId,
    pub perpetual_id: u32,
    pub price: u64,
    pub size: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CancelReason {
    User,
    IOC,
    PostOnly,
    Expired,
}
```

---

## 5. sui-indexer-alt 解析可行性分析

### 5.1 可行性评估

**结论**: ✅ 完全可行

sui-indexer-alt 使用标准的事件处理机制，可以轻松扩展支持 DEX 事件。

### 5.2 现有事件处理机制

**参考文件**: `crates/sui-indexer-alt/src/handlers/ev_emit_mod.rs`

```rust
impl Processor for EvEmitMod {
    async fn process(&self, checkpoint: &Arc<Checkpoint>) -> Result<Vec<Self::Value>> {
        for (i, tx) in transactions.iter().enumerate() {
            values.extend(
                tx.events
                    .iter()
                    .flat_map(|evs| &evs.data)
                    .map(|ev| StoredEvEmitMod {
                        package: ev.package_id.to_vec(),
                        module: ev.transaction_module.to_string(),
                        tx_sequence_number: (first_tx + i) as i64,
                        sender: ev.sender.to_vec(),
                    }),
            );
        }
        Ok(values.into_iter().collect())
    }
}
```

### 5.3 DEX 事件解析方案

```rust
// 新建 crates/sui-indexer-alt/src/handlers/dex_events.rs

use sui_indexer_alt_framework::pipeline::Processor;
use sui_types::dex_events::*;

pub(crate) struct DexEventsHandler;

#[async_trait]
impl Processor for DexEventsHandler {
    const NAME: &'static str = "dex_events";
    type Value = StoredDexEvent;

    async fn process(&self, checkpoint: &Arc<Checkpoint>) -> Result<Vec<Self::Value>> {
        let mut values = vec![];
        let first_tx = checkpoint.summary.network_total_transactions as usize
            - checkpoint.transactions.len();

        for (i, tx) in checkpoint.transactions.iter().enumerate() {
            let tx_seq = (first_tx + i) as i64;
            let cp_seq = checkpoint.summary.sequence_number as i64;

            for ev in tx.events.iter().flat_map(|e| &e.data) {
                // 过滤 DEX 事件: package_id == 0x0, module == "dex"
                if ev.package_id.inner() == &[0u8; 32]
                   && ev.transaction_module.as_str() == "dex" {

                    let stored = match ev.type_.name.as_str() {
                        "SubaccountCreatedEvent" => {
                            let event: SubaccountCreatedEvent =
                                bcs::from_bytes(&ev.contents)?;
                            StoredDexEvent::SubaccountCreated {
                                cp_sequence_number: cp_seq,
                                tx_sequence_number: tx_seq,
                                subaccount_id: event.subaccount_id.to_bytes(),
                                owner: event.owner.to_vec(),
                                timestamp: event.timestamp as i64,
                            }
                        }
                        "OrderPlacedEvent" => {
                            let event: OrderPlacedEvent =
                                bcs::from_bytes(&ev.contents)?;
                            StoredDexEvent::OrderPlaced {
                                cp_sequence_number: cp_seq,
                                tx_sequence_number: tx_seq,
                                order_id: event.order_id.to_vec(),
                                subaccount_id: event.subaccount_id.to_bytes(),
                                perpetual_id: event.perpetual_id as i32,
                                side: event.side as i16,
                                price: event.price as i64,
                                size: event.size as i64,
                                timestamp: event.timestamp as i64,
                            }
                        }
                        "TradeEvent" => {
                            let event: TradeEvent =
                                bcs::from_bytes(&ev.contents)?;
                            StoredDexEvent::Trade {
                                cp_sequence_number: cp_seq,
                                tx_sequence_number: tx_seq,
                                maker_order_id: event.maker_order_id.to_vec(),
                                taker_order_id: event.taker_order_id.to_vec(),
                                perpetual_id: event.perpetual_id as i32,
                                price: event.price as i64,
                                size: event.size as i64,
                                timestamp: event.timestamp as i64,
                            }
                        }
                        // ... 其他事件类型
                        _ => continue,
                    };
                    values.push(stored);
                }
            }
        }
        Ok(values)
    }
}
```

### 5.4 数据库 Schema 设计

```sql
-- DEX 子账户事件表
CREATE TABLE dex_subaccount_events (
    cp_sequence_number BIGINT NOT NULL,
    tx_sequence_number BIGINT NOT NULL,
    event_type TEXT NOT NULL,  -- 'created', 'deposit', 'withdraw', 'deleted'
    subaccount_id BYTEA NOT NULL,
    owner BYTEA NOT NULL,
    amount BIGINT,
    timestamp BIGINT NOT NULL,
    PRIMARY KEY (tx_sequence_number, subaccount_id, event_type)
);

-- DEX 订单事件表
CREATE TABLE dex_order_events (
    cp_sequence_number BIGINT NOT NULL,
    tx_sequence_number BIGINT NOT NULL,
    event_type TEXT NOT NULL,  -- 'placed', 'filled', 'canceled'
    order_id BYTEA NOT NULL,
    subaccount_id BYTEA NOT NULL,
    perpetual_id INTEGER NOT NULL,
    side SMALLINT,
    price BIGINT,
    size BIGINT,
    filled_size BIGINT,
    cancel_reason TEXT,
    timestamp BIGINT NOT NULL,
    PRIMARY KEY (tx_sequence_number, order_id, event_type)
);

-- DEX 成交事件表
CREATE TABLE dex_trades (
    cp_sequence_number BIGINT NOT NULL,
    tx_sequence_number BIGINT NOT NULL,
    maker_order_id BYTEA NOT NULL,
    taker_order_id BYTEA NOT NULL,
    maker_subaccount_id BYTEA NOT NULL,
    taker_subaccount_id BYTEA NOT NULL,
    perpetual_id INTEGER NOT NULL,
    price BIGINT NOT NULL,
    size BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    PRIMARY KEY (tx_sequence_number, maker_order_id, taker_order_id)
);

-- 索引
CREATE INDEX idx_dex_trades_perpetual ON dex_trades(perpetual_id, timestamp);
CREATE INDEX idx_dex_trades_maker ON dex_trades(maker_subaccount_id, timestamp);
CREATE INDEX idx_dex_trades_taker ON dex_trades(taker_subaccount_id, timestamp);
CREATE INDEX idx_dex_orders_subaccount ON dex_order_events(subaccount_id, timestamp);
```

---

## 6. 总结

### 状态评估

| 组件 | 状态 | 说明 |
|------|------|------|
| 事件发射代码 | ✅ 已实现 | 12 处 `ctx.emit_event()` 调用完整 |
| 事件类型定义 | ❌ 缺失 | `dex_events.rs` 文件不存在 |
| 事件转换逻辑 | ❌ 缺失 | `to_event()` 方法未实现 |
| 事件摘要计算 | ✅ 正确 | `events.digest()` 正确调用 |
| Checkpoint 集成 | ✅ 正确 | `InnerTemporaryStore.events` 正确传递 |
| sui-indexer-alt 解析 | ⚠️ 未实现 | 需新增 handler |

### 修复步骤

1. **创建 `crates/sui-types/src/dex_events.rs`**
   - 定义 `DexEvent` 枚举
   - 定义各事件结构体
   - 实现 `to_event()` 方法
   - 实现 `CancelReason` 枚举

2. **验证编译**
   ```bash
   cargo check -p sui-types -p sui-execution
   ```

3. **运行 E2E 测试验证事件**
   ```bash
   cargo simtest -p sui-e2e-tests --test dex_order_tests
   cargo simtest -p sui-e2e-tests --test dex_subaccount_tests
   ```

4. **(后续) 添加 sui-indexer-alt handler**
   - 创建 `handlers/dex_events.rs`
   - 添加数据库 migration
   - 注册到 indexer pipeline

---

## 关键代码引用

| 组件 | 文件 | 行号 |
|------|------|------|
| 模块声明 (缺失) | `sui-types/src/lib.rs` | 50 |
| 事件导入 | `sui-execution/src/dex.rs` | 25-28 |
| 事件发射 | `sui-execution/src/dex.rs` | 1135, 1206, 1290, 1372, 1575, 1635, 1805, 1818, 1837, 2013, 2400, 2498 |
| 事件收集结构 | `sui-execution/src/dex.rs` | 2698-2702 |
| emit_event 方法 | `sui-execution/src/dex.rs` | 2722-2725 |
| 事件转换 | `sui-execution/src/dex.rs` | 2613-2619 |
| 摘要计算 | `sui-execution/src/dex.rs` | 2659-2664 |
| Effects 生成 | `sui-execution/src/dex.rs` | 2676 |
| Event 结构定义 | `sui-types/src/event.rs` | 106-113 |
| 索引器事件处理示例 | `sui-indexer-alt/src/handlers/ev_emit_mod.rs` | 40-52 |

---

## 结论

1. **当前状态**: 事件发射代码架构完整，但因 `dex_events.rs` 文件缺失导致无法编译
2. **Checkpoint 集成**: 设计正确，补齐文件后事件将正确流入 Checkpoint
3. **sui-indexer-alt**: 解析完全可行，遵循现有 handler 模式即可实现
4. **优先级**: 首先补充 `dex_events.rs`，然后验证编译和 E2E 测试
