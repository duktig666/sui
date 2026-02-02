# DEX Indexer 技术方案 V4

> 版本: V4
> 日期: 2026-01-29
> 状态: 设计中
> 基于: V3 方案简化，采用原生 DEX 事件 + sui-indexer-alt-framework

---

## 1. 版本演进与设计决策

### 1.1 V3 → V4 核心变化

| 维度 | V3 方案 | V4 方案 | 变化原因 |
|------|---------|---------|----------|
| 事件传输 | 自建 gRPC Server + Client | Sui 原生 TransactionEvents | 复用 Sui 基础设施，减少代码量 |
| 索引框架 | 自建 Pipeline 框架 | sui-indexer-alt-framework | 成熟框架，无需重复造轮子 |
| 数据源 | gRPC 事件流 | Checkpoint 内的 Events | 数据一致性更高 |
| 代码量 | ~3600 行 | ~1000 行 | 减少 70%+ |

### 1.2 V4 架构核心理念

**"站在 Sui 的肩膀上"**：最大化复用 Sui 现有基础设施

1. **DEX 引擎**：在撮合时发出 `TransactionEvents`，复用 Sui 事件机制
2. **事件存储**：事件自动存入 Checkpoint，无需额外传输通道
3. **索引服务**：基于 `sui-indexer-alt-framework` 构建，复用其 Pipeline 管理
4. **API 层**：自定义 REST API，对标 Hyperliquid

### 1.3 设计决策总结

| 决策 | 选择 | 理由 |
|------|------|------|
| 事件机制 | 原生 DEX 事件 (方案 B) | 完整 Fill 明细，数据一致性高 |
| Package ID | **纯虚拟地址 (推荐)** | 无需部署，DEX 自有 Indexer 足够 |
| 索引框架 | sui-indexer-alt-framework | 成熟稳定，减少开发量 |
| 数据库 | PostgreSQL | 与 sui-indexer-alt 一致 |
| API 风格 | POST /info + POST /exchange | 对标 Hyperliquid |
| Phase 2 实时性 | gRPC 可选扩展 | 按需添加，不影响 Phase 1 |

### 1.4 Package ID 决策详细分析

#### 核心问题

Event 结构的 `package_id` 字段是否必须指向一个真实部署在链上的 Move Package？

#### 技术分析结论：**不需要**

通过代码分析确认：

```rust
// sui-types/src/event.rs - Event::new()
impl Event {
    pub fn new(
        package_id: &AccountAddress,  // ← 只是接收一个地址值
        module: &IdentStr,
        sender: SuiAddress,
        type_: StructTag,
        contents: Vec<u8>,
    ) -> Self {
        // ❗ 没有任何链上验证，直接存储传入的值
        Event {
            package_id: ObjectID::from(*package_id),
            transaction_module: Identifier::from(module),
            sender,
            type_,
            contents,
        }
    }
}
```

| 验证点 | 是否验证 Package 存在 | 说明 |
|--------|----------------------|------|
| `Event::new()` | ❌ 不验证 | 只存储值 |
| `events_digest` 计算 | ❌ 不验证 | 只计算数据哈希 |
| `TransactionEffects` 构建 | ❌ 不验证 | 直接包含 digest |
| Checkpoint 存储 | ❌ 不验证 | 直接存储事件 |
| sui-indexer-alt 索引 | ❌ 不验证 | 直接存储到数据库 |

#### 两种方案对比

| 维度 | 方案 A: 纯虚拟地址 | 方案 B: 部署占位 Package |
|------|-------------------|------------------------|
| 部署成本 | 0 | 需要部署 Move 包 |
| 链上状态 | 无 | ~1KB |
| 共识安全 | ✅ 安全 (所有节点用相同常量) | ✅ 安全 |
| 数据完整性 | ✅ BCS 完整保存 | ✅ BCS 完整保存 |
| 自定义 Indexer | ✅ 正常工作 | ✅ 正常工作 |
| sui-explorer 显示 | ⚠️ "Unknown Package" | ✅ 显示类型名 |
| GraphQL 类型查询 | ⚠️ 可能失败 | ✅ 正常 |
| 第三方工具 | ⚠️ 部分不兼容 | ✅ 完全兼容 |

#### 推荐决策：**方案 A - 纯虚拟地址**

**理由**：

1. **DEX 自有完整栈**
   - 自有 Indexer：自定义 Handler 直接用 package_id 常量过滤
   - 自有 Frontend：不依赖 sui-explorer 显示事件
   - 自有 API：不依赖 GraphQL 类型解析

2. **零部署复杂度**
   - 无需编写 Move 代码
   - 无需部署和管理 Package
   - 无需处理 Package 升级

3. **共识安全有保障**
   - 所有验证器使用相同的 `DEX_EVENTS_PACKAGE` 常量
   - 产生相同的 `events_digest`
   - 不影响共识

4. **必要时可迁移**
   - 如果未来需要生态兼容，可以部署 Package 并迁移
   - 迁移只需更新 `DEX_EVENTS_PACKAGE` 常量

#### 虚拟地址定义

```rust
/// DEX 事件虚拟 Package 地址
/// 使用系统保留地址空间的变体，确保不会与真实部署冲突
pub const DEX_EVENTS_PACKAGE: AccountAddress = AccountAddress::new([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x44, 0x45, 0x58, 0x00,  // "DEX\0" in hex
]);

// 地址: 0x0000000000000000000000000000000000000000000000000000000044455800
// 位于系统保留地址空间 (0x0 ~ 0xF)，不可能被用户部署占用
```

#### 如果选择方案 B（部署占位 Package）

仅在以下情况考虑：

1. **需要 sui-explorer 显示事件类型** - 例如面向普通用户的公开区块链浏览器
2. **需要 GraphQL 类型查询** - 例如第三方集成需要通过 GraphQL 查询事件
3. **需要第三方工具兼容** - 例如与其他生态系统工具集成

占位 Package 示例：

```move
// 仅需定义空结构体，不需要任何逻辑
module dex_events::events {
    public struct FillEvent has copy, drop { dummy: u8 }
    public struct PositionUpdateEvent has copy, drop { dummy: u8 }
    public struct BalanceUpdateEvent has copy, drop { dummy: u8 }
}
```

---

## 2. 系统架构

### 2.1 整体架构图

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Sui 验证器节点                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                        DEX 引擎 (dex.rs)                              │   │
│  │                                                                       │   │
│  │  ┌─────────────┐    ┌─────────────┐    ┌─────────────────────────┐   │   │
│  │  │ 订单撮合    │───►│ Fill 产生   │───►│ Event 发出              │   │   │
│  │  │ (Matching)  │    │             │    │ FillEvent, PositionEvent │   │   │
│  │  └─────────────┘    └─────────────┘    └───────────┬─────────────┘   │   │
│  │                                                    │                  │   │
│  └────────────────────────────────────────────────────┼──────────────────┘   │
│                                                       │                      │
│                                                       ▼                      │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                     TransactionEffects                                │   │
│  │  • events_digest: Some(hash)  ← 事件摘要进入共识                      │   │
│  └────────────────────────────────────────────────────────────────────── ┘   │
│                                                       │                      │
│                                                       ▼                      │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                     CheckpointTransaction                             │   │
│  │  • transaction: Transaction                                           │   │
│  │  • effects: TransactionEffects                                        │   │
│  │  • events: Some(TransactionEvents { data: [FillEvent, ...] })        │   │
│  │  • output_objects: [Order, Subaccount, ...]                          │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
└──────────────────────────────────────────┬──────────────────────────────────┘
                                           │
                                           │ 标准 Sui Checkpoint 订阅
                                           │ (gRPC/REST)
                                           ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            dex-indexer                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │              sui-indexer-alt-framework (复用)                         │   │
│  │                                                                       │   │
│  │  • Checkpoint 订阅与处理                                              │   │
│  │  • Pipeline 管理 (Concurrent/Sequential)                              │   │
│  │  • Watermark 断点续传                                                 │   │
│  │  • 数据库连接池                                                       │   │
│  │  • 背压控制                                                           │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                           │                                  │
│                                           ▼                                  │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                    DEX Handlers (自定义)                              │   │
│  │                                                                       │   │
│  │  ┌───────────────┐ ┌───────────────┐ ┌───────────────┐               │   │
│  │  │ FillsHandler  │ │PositionsHandler│ │BalancesHandler│               │   │
│  │  │               │ │               │ │               │               │   │
│  │  │ 解析 FillEvent│ │解析 Position  │ │解析 Balance   │               │   │
│  │  │ 写入 fills 表 │ │写入 positions │ │写入 balances  │               │   │
│  │  └───────┬───────┘ └───────┬───────┘ └───────┬───────┘               │   │
│  │          │                 │                 │                        │   │
│  │  ┌───────────────┐ ┌───────────────┐ ┌───────────────┐               │   │
│  │  │CandlesHandler │ │FundingHandler │ │MarketsHandler │               │   │
│  │  │               │ │               │ │               │               │   │
│  │  │ 聚合 K 线数据 │ │解析资金费率  │ │解析市场配置  │               │   │
│  │  │写入 candles 表│ │写入 funding  │ │写入 markets   │               │   │
│  │  └───────┬───────┘ └───────┬───────┘ └───────┬───────┘               │   │
│  │          │                 │                 │                        │   │
│  └──────────┼─────────────────┼─────────────────┼────────────────────────┘   │
│             │                 │                 │                            │
│             └─────────────────┼─────────────────┘                            │
│                               ▼                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                        PostgreSQL                                     │   │
│  │                                                                       │   │
│  │  fills │ positions │ balances │ candles │ funding_rates │ markets    │   │
│  │  orders │ transfers │ liquidations │ dex_watermarks                   │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                               │                                              │
│                               ▼                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                      REST API (Axum)                                  │   │
│  │                                                                       │   │
│  │  POST /info                          POST /exchange                   │   │
│  │  ├── type: "fills"                   ├── type: "order"                │   │
│  │  ├── type: "positions"               ├── type: "cancel"               │   │
│  │  ├── type: "balances"                ├── type: "withdraw"             │   │
│  │  ├── type: "candles"                 └── ...                          │   │
│  │  ├── type: "fundingHistory"                                           │   │
│  │  └── ...                                                              │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 数据流详解

```
时间轴:
────────────────────────────────────────────────────────────────────────────►

T0: 用户下单
    │
    ▼
T1: DEX 引擎撮合 (dex.rs)
    │
    ├─► 产生 Fill 结构体
    ├─► 创建 FillEvent
    ├─► 添加到 TransactionEvents
    │
    ▼
T2: 交易执行完成
    │
    ├─► build_effects() 计算 events_digest
    ├─► TransactionEffects 包含事件摘要
    │
    ▼
T3: 共识 (~400ms)
    │
    ├─► 验证器签名
    ├─► 达成共识
    │
    ▼
T4: Checkpoint 生成 (~700ms from T0)
    │
    ├─► CheckpointTransaction.events = TransactionEvents
    ├─► Checkpoint 包含完整事件数据
    │
    ▼
T5: dex-indexer 处理
    │
    ├─► 订阅 Checkpoint
    ├─► FillsHandler 解析 FillEvent
    ├─► 写入 PostgreSQL
    │
    ▼
T6: API 可查询 (~800ms from T0)
    │
    └─► GET /info { type: "fills" }
```

### 2.3 与 V3 架构对比

| 组件 | V3 (自建 gRPC) | V4 (原生事件) |
|------|---------------|---------------|
| DEX → Indexer 传输 | 自建 gRPC Server + Client | Sui 标准 Checkpoint 订阅 |
| 事件格式 | 自定义 Protobuf | Sui Event (BCS) |
| 断点续传 | 自建 Checkpoint 序号追踪 | framework Watermark |
| 重连逻辑 | 自建 | framework 内置 |
| 背压控制 | 自建 | framework Pipeline |
| 数据一致性 | 需要额外同步机制 | 天然一致 (来自 Checkpoint) |

---

## 3. DEX 事件设计

### 3.1 虚拟 Package 地址设计

基于 1.4 节的分析，采用**纯虚拟地址**方案，无需部署 Move Package。

**设计原则**：

1. 使用系统保留地址空间，确保不与真实 Package 冲突
2. 地址具有可读性，便于识别为 DEX 事件
3. 所有验证器使用相同常量，保证共识安全

**地址选择**：

```
0x0000000000000000000000000000000000000000000000000000000044455800
                                                        ^^^^^^^^
                                                        "DEX\0" (ASCII hex)
```

### 3.2 Rust 事件类型定义

```rust
// dex-sui/crates/sui-types/src/dex_events.rs

use move_core_types::language_storage::StructTag;
use move_core_types::identifier::Identifier;
use move_core_types::account_address::AccountAddress;
use serde::{Serialize, Deserialize};

/// DEX 事件虚拟 Package 地址
///
/// 技术说明：
/// - Event::new() 不验证 package_id 是否为真实链上 Package
/// - events_digest 仅计算数据哈希，不验证 Package 存在
/// - 所有验证器使用相同常量，共识安全有保障
///
/// 地址选择：
/// - 位于系统保留地址空间 (0x0 ~ 0xF)，不可能被用户部署占用
/// - 最后 4 字节 = "DEX\0" (0x44455800)，便于识别
pub const DEX_EVENTS_PACKAGE: AccountAddress = AccountAddress::new([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x44, 0x45, 0x58, 0x00,  // "DEX\0"
]);

pub const DEX_EVENTS_MODULE: &str = "dex_events";

// ==================== Fill Event ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillEvent {
    pub perpetual_id: u32,
    pub taker_order_id: Vec<u8>,
    pub maker_order_id: Vec<u8>,
    pub taker_subaccount: Vec<u8>,
    pub maker_subaccount: Vec<u8>,
    pub side: u8,
    pub price: u64,
    pub quantity: u64,
    pub taker_fee: u64,
    pub maker_fee: i64,
    pub timestamp_ms: u64,
}

impl FillEvent {
    pub fn struct_tag() -> StructTag {
        StructTag {
            address: DEX_EVENTS_PACKAGE,
            module: Identifier::new(DEX_EVENTS_MODULE).unwrap(),
            name: Identifier::new("FillEvent").unwrap(),
            type_params: vec![],
        }
    }

    pub fn to_sui_event(&self, sender: SuiAddress) -> Event {
        Event::new(
            &DEX_EVENTS_PACKAGE,
            ident_str!(DEX_EVENTS_MODULE),
            sender,
            Self::struct_tag(),
            bcs::to_bytes(self).expect("FillEvent serialization"),
        )
    }
}

// ==================== Position Update Event ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionUpdateEvent {
    pub subaccount: Vec<u8>,
    pub perpetual_id: u32,
    pub size: i64,
    pub entry_price: u64,
    pub realized_pnl: i64,
    pub timestamp_ms: u64,
}

impl PositionUpdateEvent {
    pub fn struct_tag() -> StructTag {
        StructTag {
            address: DEX_EVENTS_PACKAGE,
            module: Identifier::new(DEX_EVENTS_MODULE).unwrap(),
            name: Identifier::new("PositionUpdateEvent").unwrap(),
            type_params: vec![],
        }
    }
}

// ==================== Balance Update Event ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceUpdateEvent {
    pub subaccount: Vec<u8>,
    pub asset_id: u32,
    pub balance_before: i128,
    pub balance_after: i128,
    pub reason: u8,
    pub timestamp_ms: u64,
}

#[repr(u8)]
pub enum BalanceUpdateReason {
    Trade = 0,
    Funding = 1,
    Transfer = 2,
    Liquidation = 3,
    Fee = 4,
}

// ==================== Funding Settlement Event ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingSettlementEvent {
    pub perpetual_id: u32,
    pub funding_rate: i64,
    pub mark_price: u64,
    pub index_price: u64,
    pub timestamp_ms: u64,
}

// ==================== Liquidation Event ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidationEvent {
    pub subaccount: Vec<u8>,
    pub perpetual_id: u32,
    pub size: i64,
    pub price: u64,
    pub bankruptcy_price: u64,
    pub insurance_fund_delta: i64,
    pub timestamp_ms: u64,
}

// ==================== Transfer Event ====================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferEvent {
    pub transfer_type: u8,
    pub from_subaccount: Vec<u8>,
    pub to_subaccount: Vec<u8>,
    pub asset_id: u32,
    pub amount: u64,
    pub timestamp_ms: u64,
}

#[repr(u8)]
pub enum TransferType {
    Deposit = 0,
    Withdraw = 1,
    Internal = 2,
}
```

### 3.3 DEX 引擎事件发出

```rust
// dex-sui/sui-execution/src/dex.rs

use sui_types::dex_events::{FillEvent, PositionUpdateEvent, BalanceUpdateEvent};
use sui_types::event::Event;

impl DexExecutor {
    /// 执行下单并发出事件
    fn execute_place_order_v2(
        &mut self,
        ctx: &mut DexContext,
        params: PlaceOrderParams,
    ) -> Result<DexExecutionResult, ExecutionError> {
        // ... 撮合逻辑 ...

        let match_result = self.orderbook.match_order(
            order.side,
            order.quantums,
            order.subticks,
            params.worst_price,
        );

        // 收集事件
        let mut events = Vec::new();

        // 为每笔成交创建 FillEvent
        for fill in &match_result.fills {
            let fill_event = FillEvent {
                perpetual_id: self.perpetual_id,
                taker_order_id: order.id().to_vec(),
                maker_order_id: fill.maker_order_id.to_vec(),
                taker_subaccount: order.subaccount_id.to_vec(),
                maker_subaccount: fill.maker_subaccount_id.to_vec(),
                side: order.side as u8,
                price: fill.price,
                quantity: fill.quantity,
                taker_fee: fill.taker_fee,
                maker_fee: fill.maker_fee,
                timestamp_ms: ctx.timestamp_ms,
            };

            events.push(fill_event.to_sui_event(ctx.sender));
        }

        // 如果有持仓变化，创建 PositionUpdateEvent
        if let Some(position_delta) = &match_result.taker_position_delta {
            let position_event = PositionUpdateEvent {
                subaccount: order.subaccount_id.to_vec(),
                perpetual_id: self.perpetual_id,
                size: position_delta.new_size,
                entry_price: position_delta.new_entry_price,
                realized_pnl: position_delta.realized_pnl,
                timestamp_ms: ctx.timestamp_ms,
            };
            events.push(position_event.to_sui_event(ctx.sender));
        }

        // 返回包含事件的执行结果
        Ok(DexExecutionResult {
            written: written_objects,
            changed_objects,
            status: ExecutionStatus::Success,
            events,  // 新增：事件列表
        })
    }
}
```

### 3.4 build_effects 修改

```rust
// dex-sui/sui-execution/src/dex.rs

fn build_effects_and_events(
    result: &DexExecutionResult,
    input_objects: &CheckedInputObjects,
    transaction_digest: TransactionDigest,
    epoch_id: EpochId,
    lamport_version: SequenceNumber,
    gas_object_id: Option<ObjectID>,
) -> (TransactionEffects, TransactionEvents) {
    // 构建事件
    let events = TransactionEvents {
        data: result.events.clone(),
    };

    // 计算 events_digest (影响共识)
    let events_digest = if events.data.is_empty() {
        None
    } else {
        Some(events.digest())
    };

    // 构建 effects
    let effects = TransactionEffects::new_from_execution_v2(
        result.status.clone(),
        epoch_id,
        GasCostSummary::new(0, 0, 0, 0),
        result.shared_inputs.clone(),
        std::collections::BTreeSet::new(),
        transaction_digest,
        lamport_version,
        result.changed_objects.clone(),
        gas_object_id,
        events_digest,  // 现在有值了
        vec![],
    );

    (effects, events)
}
```

### 3.5 与 V3 事件传输对比

V4 相比 V3 最大的变化是**事件传输机制**的简化：

| 维度 | V3 方案 | V4 方案 |
|------|---------|---------|
| 事件定义 | 自定义 Protobuf | Sui 原生 Event (BCS) |
| 传输协议 | 自建 gRPC Server/Client | Sui 标准 Checkpoint 订阅 |
| 服务端实现 | DEX 引擎内嵌 gRPC Server | 无需（复用 Sui 基础设施）|
| 客户端实现 | 自建 DexEventStreamClient | sui-indexer-alt-framework |
| 断点续传 | 自建 Checkpoint 序号追踪 | framework Watermark |
| 重连逻辑 | 自建 | framework 内置 |
| 背压控制 | 自建 | framework Pipeline |
| 代码量 | ~1500 行（gRPC 相关） | 0 行（完全复用）|

**为什么不需要自定义 Proto**：

1. **性能相当**：Sui Event 的 BCS 编码与 Protobuf 性能相当
2. **结构统一**：事件内容通过 `contents: Vec<u8>` 字段携带 BCS 序列化的 Rust 结构体
3. **解析简单**：Indexer 直接 `bcs::from_bytes()` 反序列化，无需额外的 Proto 编解码层
4. **类型安全**：通过 `StructTag` 区分事件类型，与 Rust 类型一一对应

**数据一致性优势**：

```
V3: DEX 引擎 → gRPC Server → gRPC Client → Indexer
    ↓                                        ↓
    需要处理网络抖动、消息丢失、重复等问题

V4: DEX 引擎 → TransactionEvents → Checkpoint → Indexer
    ↓                                           ↓
    事件存储在 Checkpoint 中，天然保证一致性和有序性
```

---

## 4. dex-indexer 设计

### 4.1 项目结构

```
dex-indexer/
├── Cargo.toml
├── src/
│   ├── main.rs                 # 入口：启动 Indexer + API Server
│   ├── lib.rs
│   │
│   ├── handlers/               # Checkpoint 处理器
│   │   ├── mod.rs
│   │   ├── fills.rs            # FillEvent → fills 表
│   │   ├── positions.rs        # PositionUpdateEvent → positions 表
│   │   ├── balances.rs         # BalanceUpdateEvent → balances 表
│   │   ├── candles.rs          # 从 fills 聚合 K 线
│   │   ├── funding.rs          # FundingSettlementEvent → funding_rates 表
│   │   ├── liquidations.rs     # LiquidationEvent → liquidations 表
│   │   └── transfers.rs        # TransferEvent → transfers 表
│   │
│   ├── models/                 # 数据库模型
│   │   ├── mod.rs
│   │   ├── fill.rs
│   │   ├── position.rs
│   │   ├── balance.rs
│   │   ├── candle.rs
│   │   ├── funding_rate.rs
│   │   └── market.rs
│   │
│   ├── schema/                 # 数据库 Schema
│   │   ├── mod.rs
│   │   └── migrations/
│   │
│   └── api/                    # REST API
│       ├── mod.rs
│       ├── server.rs           # Axum server
│       ├── info.rs             # POST /info 处理
│       ├── exchange.rs         # POST /exchange 处理
│       └── types.rs            # API 请求/响应类型
│
├── proto/                      # (可选) Phase 2 gRPC 定义
│   └── dex_realtime.proto
│
└── migrations/                 # SQL 迁移文件
    ├── 001_create_fills.sql
    ├── 002_create_positions.sql
    └── ...
```

### 4.2 Pipeline 架构概览（复用 sui-indexer-alt-framework）

V4 最大的简化是直接复用 `sui-indexer-alt-framework`，无需自建 Pipeline 框架。

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Pipeline 架构                                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Checkpoint 订阅（sui-indexer-alt-framework 提供）                       │
│       │                                                                 │
│       ▼                                                                 │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Processor Layer（自定义）                   │      │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌────────────┐│      │
│  │  │FillsHandler│ │PositionHdl│ │CandleHandler│ │FundingHdl  ││      │
│  │  └─────┬──────┘ └─────┬──────┘ └─────┬──────┘ └─────┬──────┘│      │
│  │        │              │              │              │        │      │
│  └────────┼──────────────┼──────────────┼──────────────┼────────┘      │
│           │              │              │              │                │
│           ▼              ▼              ▼              ▼                │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Collector Layer（framework 提供）           │      │
│  │        批量收集处理结果，按 checkpoint 边界分组               │      │
│  └───────────────────────────┬──────────────────────────────────┘      │
│                              │                                          │
│                              ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Committer Layer（自定义 + framework）       │      │
│  │        批量写入数据库，保证幂等性                             │      │
│  └───────────────────────────┬──────────────────────────────────┘      │
│                              │                                          │
│                              ▼                                          │
│  ┌──────────────────────────────────────────────────────────────┐      │
│  │                    Watermark Layer（framework 提供）           │      │
│  │        更新断点续传标记，记录处理进度                         │      │
│  └──────────────────────────────────────────────────────────────┘      │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

**与 V3 自建 Pipeline 对比**：

| 组件 | V3 方案 | V4 方案 |
|------|---------|---------|
| Checkpoint 订阅 | 自建 gRPC Client | framework 提供 |
| Processor | 自建 Handler trait | 实现 framework Processor trait |
| Collector | 自建批量收集逻辑 | framework 内置 |
| Committer | 自建数据库写入 | 实现 framework Handler trait |
| Watermark | 自建断点续传 | framework 自动管理 |
| 重连/背压 | 自建 | framework 内置 |

### 4.3 Handler Trait 说明

sui-indexer-alt-framework 提供两种 Handler 模式：

```rust
/// Processor trait - 定义如何处理 Checkpoint
pub trait Processor: Send + Sync + 'static {
    /// Handler 名称（用于日志和 Watermark）
    const NAME: &'static str;
    /// 处理后的数据类型
    type Value: Send + Sync;

    /// 处理单个 Checkpoint，返回待写入的数据
    fn process(&self, checkpoint: &Arc<CheckpointData>) -> Result<Vec<Self::Value>>;
}

/// ConcurrentHandler - 并发写入（无顺序依赖）
#[async_trait]
pub trait ConcurrentHandler: Processor {
    /// 批量写入数据库（需保证幂等）
    async fn commit(values: &[Self::Value], conn: &mut AsyncPgConnection) -> Result<usize>;
}

/// SequentialHandler - 顺序写入（有状态依赖）
#[async_trait]
pub trait SequentialHandler: Processor {
    /// 顺序写入数据库
    async fn commit(&self, values: Vec<Self::Value>, conn: &mut AsyncPgConnection) -> Result<usize>;
}
```

**Handler 模式选择**：

| Handler | 模式 | 原因 |
|---------|------|------|
| FillsHandler | Concurrent | 成交记录无顺序依赖，可并行写入 |
| PositionsHandler | Sequential | 持仓 UPSERT 需要顺序保证 |
| BalancesHandler | Sequential | 余额 UPSERT 需要顺序保证 |
| CandlesHandler | Sequential | K线聚合有状态依赖 |
| FundingHandler | Concurrent | 资金费记录无顺序依赖 |
| LiquidationsHandler | Concurrent | 清算记录无顺序依赖 |
| TransfersHandler | Concurrent | 转账记录无顺序依赖 |

### 4.4 FillsHandler 实现示例

```rust
// dex-indexer/src/handlers/fills.rs

use sui_indexer_alt_framework::pipeline::{
    concurrent::Handler as ConcurrentHandler,
    Processor,
};
use sui_types::full_checkpoint_content::CheckpointData;
use sui_types::dex_events::{DEX_EVENTS_PACKAGE, FillEvent};
use sui_types::base_types::ObjectID;

pub struct FillsHandler;

/// 数据库存储结构
#[derive(Debug, Clone)]
pub struct StoredFill {
    pub fill_id: i64,               // 自增 ID
    pub perpetual_id: i32,
    pub taker_order_id: Vec<u8>,
    pub maker_order_id: Vec<u8>,
    pub taker_subaccount: Vec<u8>,
    pub maker_subaccount: Vec<u8>,
    pub side: String,               // "B" | "A"
    pub price: i64,                 // subticks
    pub quantity: i64,              // quantums
    pub taker_fee: i64,
    pub maker_fee: i64,
    pub timestamp_ms: i64,
    pub checkpoint_sequence: i64,
    pub tx_digest: Vec<u8>,
}

impl Processor for FillsHandler {
    const NAME: &'static str = "dex_fills";
    type Value = StoredFill;

    fn process(&self, checkpoint: &Arc<CheckpointData>) -> Result<Vec<Self::Value>> {
        let checkpoint_sequence = checkpoint.checkpoint_summary.sequence_number;
        let mut fills = Vec::new();

        for tx in &checkpoint.transactions {
            // 跳过没有事件的交易
            let events = match &tx.events {
                Some(e) => &e.data,
                None => continue,
            };

            let tx_digest = tx.transaction.digest().to_vec();

            for event in events {
                // 过滤：只处理 DEX 事件 Package 的事件
                if event.package_id != ObjectID::from(DEX_EVENTS_PACKAGE) {
                    continue;
                }

                // 过滤：只处理 FillEvent
                if event.type_.name.as_str() != "FillEvent" {
                    continue;
                }

                // 解析 BCS 内容
                let fill: FillEvent = bcs::from_bytes(&event.contents)
                    .map_err(|e| anyhow::anyhow!("Failed to parse FillEvent: {}", e))?;

                fills.push(StoredFill {
                    fill_id: 0,  // 由数据库自增
                    perpetual_id: fill.perpetual_id as i32,
                    taker_order_id: fill.taker_order_id,
                    maker_order_id: fill.maker_order_id,
                    taker_subaccount: fill.taker_subaccount,
                    maker_subaccount: fill.maker_subaccount,
                    side: if fill.side == 0 { "B".to_string() } else { "A".to_string() },
                    price: fill.price as i64,
                    quantity: fill.quantity as i64,
                    taker_fee: fill.taker_fee as i64,
                    maker_fee: fill.maker_fee,
                    timestamp_ms: fill.timestamp_ms as i64,
                    checkpoint_sequence: checkpoint_sequence as i64,
                    tx_digest,
                });
            }
        }

        Ok(fills)
    }
}

#[async_trait]
impl ConcurrentHandler for FillsHandler {
    async fn commit(values: &[Self::Value], conn: &mut AsyncPgConnection) -> Result<usize> {
        use diesel_async::RunQueryDsl;
        use crate::schema::fills;

        let result = diesel::insert_into(fills::table)
            .values(values)
            .on_conflict_do_nothing()  // 幂等写入
            .execute(conn)
            .await?;

        Ok(result)
    }
}
```

### 4.5 PositionsHandler 实现（Sequential 模式）

```rust
// dex-indexer/src/handlers/positions.rs

pub struct PositionsHandler;

#[derive(Debug, Clone)]
pub struct StoredPosition {
    pub subaccount: Vec<u8>,
    pub perpetual_id: i32,
    pub size: i64,
    pub entry_price: i64,
    pub realized_pnl: i64,
    pub timestamp_ms: i64,
    pub checkpoint_sequence: i64,
}

impl Processor for PositionsHandler {
    const NAME: &'static str = "dex_positions";
    type Value = StoredPosition;

    fn process(&self, checkpoint: &Arc<CheckpointData>) -> Result<Vec<Self::Value>> {
        let checkpoint_sequence = checkpoint.checkpoint_summary.sequence_number;
        let mut positions = Vec::new();

        for tx in &checkpoint.transactions {
            let events = match &tx.events {
                Some(e) => &e.data,
                None => continue,
            };

            for event in events {
                if event.package_id != ObjectID::from(DEX_EVENTS_PACKAGE) {
                    continue;
                }

                if event.type_.name.as_str() != "PositionUpdateEvent" {
                    continue;
                }

                let pos: PositionUpdateEvent = bcs::from_bytes(&event.contents)?;

                positions.push(StoredPosition {
                    subaccount: pos.subaccount,
                    perpetual_id: pos.perpetual_id as i32,
                    size: pos.size,
                    entry_price: pos.entry_price as i64,
                    realized_pnl: pos.realized_pnl,
                    timestamp_ms: pos.timestamp_ms as i64,
                    checkpoint_sequence: checkpoint_sequence as i64,
                });
            }
        }

        Ok(positions)
    }
}

#[async_trait]
impl SequentialHandler for PositionsHandler {
    async fn commit(&self, values: Vec<Self::Value>, conn: &mut AsyncPgConnection) -> Result<usize> {
        let mut count = 0;

        for pos in values {
            // UPSERT: 持仓状态为最新状态
            let result = sqlx::query(
                r#"
                INSERT INTO positions (
                    subaccount, perpetual_id, size, entry_price,
                    realized_pnl, timestamp_ms, checkpoint_sequence
                ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (subaccount, perpetual_id)
                DO UPDATE SET
                    size = EXCLUDED.size,
                    entry_price = EXCLUDED.entry_price,
                    realized_pnl = EXCLUDED.realized_pnl,
                    timestamp_ms = EXCLUDED.timestamp_ms,
                    checkpoint_sequence = EXCLUDED.checkpoint_sequence
                WHERE positions.checkpoint_sequence < EXCLUDED.checkpoint_sequence
                "#,
            )
            .bind(&pos.subaccount)
            .bind(pos.perpetual_id)
            .bind(pos.size)
            .bind(pos.entry_price)
            .bind(pos.realized_pnl)
            .bind(pos.timestamp_ms)
            .bind(pos.checkpoint_sequence)
            .execute(conn)
            .await?;

            count += result.rows_affected() as usize;
        }

        Ok(count)
    }
}
```

### 4.6 CandlesHandler 实现（K线聚合）

```rust
// dex-indexer/src/handlers/candles.rs

use std::collections::HashMap;

pub struct CandlesHandler {
    /// 内存中的 K 线缓存（按 perpetual_id + interval）
    candle_cache: tokio::sync::RwLock<HashMap<(u32, String), CandleAggregator>>,
}

impl CandlesHandler {
    pub fn new() -> Self {
        Self {
            candle_cache: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredCandle {
    pub perpetual_id: i32,
    pub resolution: String,
    pub open_time: i64,
    pub open_price: i64,
    pub high_price: i64,
    pub low_price: i64,
    pub close_price: i64,
    pub volume: i64,
    pub trades: i32,
    pub checkpoint_sequence: i64,
}

impl Processor for CandlesHandler {
    const NAME: &'static str = "dex_candles";
    type Value = StoredCandle;

    fn process(&self, checkpoint: &Arc<CheckpointData>) -> Result<Vec<Self::Value>> {
        let checkpoint_sequence = checkpoint.checkpoint_summary.sequence_number;
        let mut completed_candles = Vec::new();

        // 收集所有 FillEvent
        for tx in &checkpoint.transactions {
            let events = match &tx.events {
                Some(e) => &e.data,
                None => continue,
            };

            for event in events {
                if event.package_id != ObjectID::from(DEX_EVENTS_PACKAGE) {
                    continue;
                }

                if event.type_.name.as_str() != "FillEvent" {
                    continue;
                }

                let fill: FillEvent = bcs::from_bytes(&event.contents)?;

                // 更新各时间粒度的 K 线
                let intervals = ["1m", "5m", "15m", "1h", "4h", "1d"];
                let mut cache = self.candle_cache.blocking_write();

                for interval in intervals {
                    let key = (fill.perpetual_id, interval.to_string());
                    let aggregator = cache
                        .entry(key)
                        .or_insert_with(|| CandleAggregator::new(fill.perpetual_id, interval));

                    if let Some(candle) = aggregator.add_trade(
                        fill.price as i64,
                        fill.quantity as i64,
                        fill.timestamp_ms as i64,
                        checkpoint_sequence as i64,
                    ) {
                        completed_candles.push(candle);
                    }
                }
            }
        }

        Ok(completed_candles)
    }
}

/// K 线聚合器
struct CandleAggregator {
    perpetual_id: u32,
    interval: String,
    interval_ms: i64,
    current_candle: Option<StoredCandle>,
}

impl CandleAggregator {
    fn new(perpetual_id: u32, interval: &str) -> Self {
        let interval_ms = match interval {
            "1m" => 60_000,
            "5m" => 300_000,
            "15m" => 900_000,
            "1h" => 3_600_000,
            "4h" => 14_400_000,
            "1d" => 86_400_000,
            _ => 60_000,
        };

        Self {
            perpetual_id,
            interval: interval.to_string(),
            interval_ms,
            current_candle: None,
        }
    }

    /// 添加成交，返回已完成的 K 线（如果有）
    fn add_trade(
        &mut self,
        price: i64,
        quantity: i64,
        timestamp_ms: i64,
        checkpoint_sequence: i64,
    ) -> Option<StoredCandle> {
        let candle_start = (timestamp_ms / self.interval_ms) * self.interval_ms;

        // 检查是否需要关闭当前 K 线
        let completed = if let Some(ref current) = self.current_candle {
            if timestamp_ms >= current.open_time + self.interval_ms {
                Some(current.clone())
            } else {
                None
            }
        } else {
            None
        };

        // 更新或创建新 K 线
        match &mut self.current_candle {
            Some(current) if timestamp_ms < current.open_time + self.interval_ms => {
                // 更新当前 K 线
                current.high_price = current.high_price.max(price);
                current.low_price = current.low_price.min(price);
                current.close_price = price;
                current.volume += quantity;
                current.trades += 1;
                current.checkpoint_sequence = checkpoint_sequence;
            }
            _ => {
                // 新 K 线
                self.current_candle = Some(StoredCandle {
                    perpetual_id: self.perpetual_id as i32,
                    resolution: self.interval.clone(),
                    open_time: candle_start,
                    open_price: price,
                    high_price: price,
                    low_price: price,
                    close_price: price,
                    volume: quantity,
                    trades: 1,
                    checkpoint_sequence,
                });
            }
        }

        completed
    }
}

#[async_trait]
impl SequentialHandler for CandlesHandler {
    async fn commit(&self, values: Vec<Self::Value>, conn: &mut AsyncPgConnection) -> Result<usize> {
        let mut count = 0;

        for candle in values {
            // UPSERT: 更新或插入 K 线
            let result = sqlx::query(
                r#"
                INSERT INTO candles (
                    perpetual_id, resolution, open_time,
                    open_price, high_price, low_price, close_price,
                    volume, trades
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (perpetual_id, resolution, open_time)
                DO UPDATE SET
                    high_price = GREATEST(candles.high_price, EXCLUDED.high_price),
                    low_price = LEAST(candles.low_price, EXCLUDED.low_price),
                    close_price = EXCLUDED.close_price,
                    volume = candles.volume + EXCLUDED.volume,
                    trades = candles.trades + EXCLUDED.trades
                "#,
            )
            .bind(candle.perpetual_id)
            .bind(&candle.resolution)
            .bind(candle.open_time)
            .bind(candle.open_price)
            .bind(candle.high_price)
            .bind(candle.low_price)
            .bind(candle.close_price)
            .bind(candle.volume)
            .bind(candle.trades)
            .execute(conn)
            .await?;

            count += result.rows_affected() as usize;
        }

        Ok(count)
    }
}
```

### 4.7 主程序入口

```rust
// dex-indexer/src/main.rs

use sui_indexer_alt_framework::{Indexer, IndexerConfig};
use sui_indexer_alt_framework::pipeline::concurrent::ConcurrentConfig;

mod handlers;
mod api;

#[tokio::main]
async fn main() -> Result<()> {
    // 加载配置
    let config = IndexerConfig::from_env()?;

    // 创建数据库连接池
    let db_pool = create_db_pool(&config.database_url).await?;

    // 构建 Indexer
    let indexer = Indexer::new(config)
        // 注册 DEX Handlers
        .concurrent_pipeline::<handlers::FillsHandler>(ConcurrentConfig::default())
        .concurrent_pipeline::<handlers::PositionsHandler>(ConcurrentConfig::default())
        .concurrent_pipeline::<handlers::BalancesHandler>(ConcurrentConfig::default())
        .concurrent_pipeline::<handlers::CandlesHandler>(ConcurrentConfig::default())
        .concurrent_pipeline::<handlers::FundingHandler>(ConcurrentConfig::default())
        .concurrent_pipeline::<handlers::LiquidationsHandler>(ConcurrentConfig::default())
        .concurrent_pipeline::<handlers::TransfersHandler>(ConcurrentConfig::default())
        .build()?;

    // 创建 API Server
    let api_server = api::ApiServer::new(db_pool.clone());

    // 并行运行 Indexer 和 API Server
    tokio::select! {
        result = indexer.run() => {
            result?;
        }
        result = api_server.run(3000) => {
            result?;
        }
    }

    Ok(())
}
```

---

## 5. 数据库设计

### 5.1 核心表结构

```sql
-- ==================== fills 表 ====================
-- 存储所有成交记录
CREATE TABLE fills (
    id                  BIGSERIAL PRIMARY KEY,
    perpetual_id        INTEGER NOT NULL,
    taker_order_id      BYTEA NOT NULL,
    maker_order_id      BYTEA NOT NULL,
    taker_subaccount    BYTEA NOT NULL,
    maker_subaccount    BYTEA NOT NULL,
    side                CHAR(1) NOT NULL,           -- 'B' or 'A'
    price               BIGINT NOT NULL,            -- subticks
    quantity            BIGINT NOT NULL,            -- quantums
    taker_fee           BIGINT NOT NULL,
    maker_fee           BIGINT NOT NULL,            -- 可能为负
    timestamp_ms        BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,
    tx_digest           BYTEA NOT NULL,

    -- 索引
    CONSTRAINT fills_unique UNIQUE (tx_digest, taker_order_id, maker_order_id)
);

CREATE INDEX idx_fills_perpetual_time ON fills (perpetual_id, timestamp_ms DESC);
CREATE INDEX idx_fills_taker ON fills (taker_subaccount, timestamp_ms DESC);
CREATE INDEX idx_fills_maker ON fills (maker_subaccount, timestamp_ms DESC);
CREATE INDEX idx_fills_checkpoint ON fills (checkpoint_sequence);

-- ==================== positions 表 ====================
-- 存储当前持仓状态 (最新快照)
CREATE TABLE positions (
    id                  BIGSERIAL PRIMARY KEY,
    subaccount          BYTEA NOT NULL,
    perpetual_id        INTEGER NOT NULL,
    size                BIGINT NOT NULL,            -- 正=多头, 负=空头
    entry_price         BIGINT NOT NULL,
    realized_pnl        BIGINT NOT NULL,
    timestamp_ms        BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,

    CONSTRAINT positions_unique UNIQUE (subaccount, perpetual_id)
);

CREATE INDEX idx_positions_subaccount ON positions (subaccount);

-- ==================== position_history 表 ====================
-- 持仓历史变化
CREATE TABLE position_history (
    id                  BIGSERIAL PRIMARY KEY,
    subaccount          BYTEA NOT NULL,
    perpetual_id        INTEGER NOT NULL,
    size                BIGINT NOT NULL,
    entry_price         BIGINT NOT NULL,
    realized_pnl        BIGINT NOT NULL,
    timestamp_ms        BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL
);

CREATE INDEX idx_position_history_subaccount ON position_history (subaccount, timestamp_ms DESC);

-- ==================== balances 表 ====================
-- 当前余额状态 (最新快照)
CREATE TABLE balances (
    id                  BIGSERIAL PRIMARY KEY,
    subaccount          BYTEA NOT NULL,
    asset_id            INTEGER NOT NULL,
    balance             NUMERIC(39, 0) NOT NULL,    -- i128 range
    timestamp_ms        BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,

    CONSTRAINT balances_unique UNIQUE (subaccount, asset_id)
);

CREATE INDEX idx_balances_subaccount ON balances (subaccount);

-- ==================== candles 表 ====================
-- K 线数据
CREATE TABLE candles (
    id                  BIGSERIAL PRIMARY KEY,
    perpetual_id        INTEGER NOT NULL,
    resolution          VARCHAR(10) NOT NULL,       -- '1m', '5m', '15m', '1h', '4h', '1d'
    open_time           BIGINT NOT NULL,            -- 开盘时间戳 (ms)
    open_price          BIGINT NOT NULL,
    high_price          BIGINT NOT NULL,
    low_price           BIGINT NOT NULL,
    close_price         BIGINT NOT NULL,
    volume              BIGINT NOT NULL,            -- 成交量 (quantums)
    trades              INTEGER NOT NULL,           -- 成交笔数

    CONSTRAINT candles_unique UNIQUE (perpetual_id, resolution, open_time)
);

CREATE INDEX idx_candles_query ON candles (perpetual_id, resolution, open_time DESC);

-- ==================== funding_rates 表 ====================
-- 资金费率历史
CREATE TABLE funding_rates (
    id                  BIGSERIAL PRIMARY KEY,
    perpetual_id        INTEGER NOT NULL,
    funding_rate        BIGINT NOT NULL,            -- 8 小时费率 (scaled)
    mark_price          BIGINT NOT NULL,
    index_price         BIGINT NOT NULL,
    timestamp_ms        BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL,

    CONSTRAINT funding_rates_unique UNIQUE (perpetual_id, timestamp_ms)
);

CREATE INDEX idx_funding_rates_query ON funding_rates (perpetual_id, timestamp_ms DESC);

-- ==================== markets 表 ====================
-- 市场配置
CREATE TABLE markets (
    perpetual_id        INTEGER PRIMARY KEY,
    symbol              VARCHAR(20) NOT NULL,       -- 'BTC', 'ETH', ...
    base_asset          VARCHAR(20) NOT NULL,
    quote_asset         VARCHAR(20) NOT NULL,
    price_decimals      INTEGER NOT NULL,
    size_decimals       INTEGER NOT NULL,
    min_order_size      BIGINT NOT NULL,
    max_leverage        INTEGER NOT NULL,
    maker_fee_rate      INTEGER NOT NULL,           -- basis points
    taker_fee_rate      INTEGER NOT NULL,
    status              VARCHAR(20) NOT NULL,       -- 'active', 'suspended'
    updated_at          BIGINT NOT NULL
);

-- ==================== liquidations 表 ====================
CREATE TABLE liquidations (
    id                  BIGSERIAL PRIMARY KEY,
    subaccount          BYTEA NOT NULL,
    perpetual_id        INTEGER NOT NULL,
    size                BIGINT NOT NULL,
    price               BIGINT NOT NULL,
    bankruptcy_price    BIGINT NOT NULL,
    insurance_fund_delta BIGINT NOT NULL,
    timestamp_ms        BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL
);

CREATE INDEX idx_liquidations_subaccount ON liquidations (subaccount, timestamp_ms DESC);
CREATE INDEX idx_liquidations_perpetual ON liquidations (perpetual_id, timestamp_ms DESC);

-- ==================== transfers 表 ====================
CREATE TABLE transfers (
    id                  BIGSERIAL PRIMARY KEY,
    transfer_type       SMALLINT NOT NULL,          -- 0=Deposit, 1=Withdraw, 2=Internal
    from_subaccount     BYTEA NOT NULL,
    to_subaccount       BYTEA NOT NULL,
    asset_id            INTEGER NOT NULL,
    amount              BIGINT NOT NULL,
    timestamp_ms        BIGINT NOT NULL,
    checkpoint_sequence BIGINT NOT NULL
);

CREATE INDEX idx_transfers_from ON transfers (from_subaccount, timestamp_ms DESC);
CREATE INDEX idx_transfers_to ON transfers (to_subaccount, timestamp_ms DESC);

-- ==================== dex_watermarks 表 ====================
-- Watermark 追踪 (由 sui-indexer-alt-framework 管理)
CREATE TABLE dex_watermarks (
    pipeline            VARCHAR(255) PRIMARY KEY,
    epoch_hi_inclusive  BIGINT NOT NULL,
    checkpoint_hi_inclusive BIGINT NOT NULL,
    tx_hi               BIGINT NOT NULL,
    timestamp_ms_hi_inclusive BIGINT NOT NULL,
    reader_lo           BIGINT NOT NULL DEFAULT 0,
    pruner_hi           BIGINT NOT NULL DEFAULT 0
);
```

### 5.2 分区与保留策略

```sql
-- fills 表按时间分区 (月分区)
CREATE TABLE fills (
    -- ... 字段定义同上 ...
) PARTITION BY RANGE (timestamp_ms);

-- 创建分区
CREATE TABLE fills_2026_01 PARTITION OF fills
    FOR VALUES FROM (1735689600000) TO (1738368000000);  -- 2026-01

CREATE TABLE fills_2026_02 PARTITION OF fills
    FOR VALUES FROM (1738368000000) TO (1740787200000);  -- 2026-02

-- 保留策略：保留最近 90 天数据
-- 通过定时任务 DROP 旧分区实现
```

---

## 6. REST API 设计（对标 Hyperliquid）

### 6.1 API 概览

**设计原则**：采用 Hyperliquid 的 POST 模式，通过 `type` 字段区分请求类型，而非 RESTful 风格。

| 端点 | 用途 | 签名要求 | 阶段 |
|------|------|---------|------|
| `POST /info` | 查询数据（市场、订单簿、持仓等） | 无需 | Phase 1 |
| `POST /exchange` | 交易操作（下单、撤单等） | EIP-712 | Phase 1 |

### 6.2 Info API 完整列表（POST /info）

#### 6.2.1 市场数据

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `meta` | 无 | 永续合约元数据（杠杆、精度等） | ✓ |
| `metaAndAssetCtxs` | 无 | 永续元数据 + 实时市场数据 | ✓ |
| `spotMeta` | 无 | 现货代币元数据 | ✓ |
| `spotMetaAndAssetCtxs` | 无 | 现货元数据 + 实时数据 | ✓ |
| `allMids` | 无 | 所有交易对中间价 | ✓ |

#### 6.2.2 订单簿与行情

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `l2Book` | `coin` | 订单簿深度（买卖盘） | ✓ |
| `candleSnapshot` | `coin`, `interval`, `startTime`, `endTime` | K线数据 | ✓ |
| `recentTrades` | `coin` | 最近成交记录 | ✓ |

#### 6.2.3 用户账户

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `clearinghouseState` | `user` | 永续账户状态（保证金、持仓） | ✓ |
| `spotClearinghouseState` | `user` | 现货余额 | ✓ |
| `userVaultEquities` | `user` | Vault 权益 | ✓ |

#### 6.2.4 订单查询

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `openOrders` | `user` | 当前挂单（简化） | ✓ |
| `frontendOpenOrders` | `user` | 当前挂单（完整信息） | ✓ |
| `orderStatus` | `user`, `oid` | 单个订单状态 | ✓ |
| `historicalOrders` | `user` | 历史订单 | ✓ |

#### 6.2.5 成交与资金费

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `userFills` | `user` | 成交记录（默认最近100） | ✓ |
| `userFillsByTime` | `user`, `startTime`, `endTime` | 按时间范围成交 | ✓ |
| `userFunding` | `user`, `startTime`, `endTime` | 资金费记录 | ✓ |
| `fundingHistory` | `coin`, `startTime`, `endTime` | 市场资金费率历史 | ✓ |
| `predictedFundings` | 无 | 预测资金费率 | ✓ |

#### 6.2.6 Builder / 手续费

| type | 参数 | 说明 | 对标 Hyperliquid |
|------|------|------|-----------------|
| `maxBuilderFee` | `user`, `builder` | Builder 授权费率查询 | ✓ |
| `userFees` | `user` | 用户费率等级 | ✓ |

### 6.3 Exchange API 完整列表（POST /exchange）

**签名要求**：所有 Exchange 操作需要 EIP-712 签名。

#### 6.3.1 订单操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `order` | `orders[]`, `grouping` | 下单（支持批量） | signL1Action |
| `cancel` | `cancels[]` | 撤单（支持批量） | signL1Action |
| `cancelByCloid` | `cancels[]` | 按客户端ID撤单 | signL1Action |
| `modify` | `oid`, `order` | 修改订单 | signL1Action |
| `batchModify` | `modifies[]` | 批量修改订单 | signL1Action |

#### 6.3.2 账户操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `updateLeverage` | `asset`, `isCross`, `leverage` | 更新杠杆 | signL1Action |
| `updateIsolatedMargin` | `asset`, `isBuy`, `ntli` | 更新逐仓保证金 | signL1Action |

#### 6.3.3 资金操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `usdSend` | `destination`, `amount` | USDC 转账 | signUserSignedAction |
| `withdraw3` | `destination`, `amount` | 提现到 L1 | signUserSignedAction |
| `vaultDeposit` | `vaultAddress`, `amount` | 存入 Vault | signL1Action |
| `vaultWithdraw` | `vaultAddress`, `amount` | 取出 Vault | signL1Action |

#### 6.3.4 授权操作

| action.type | 参数 | 说明 | 签名方法 |
|-------------|------|------|---------|
| `approveBuilderFee` | `builder`, `maxFeeRate` | 授权 Builder 费率 | signUserSignedAction |

### 6.4 POST /info 请求类型

```json
// 查询成交记录
{
  "type": "userFills",
  "user": "0x...",
  "startTime": 1704067200000,
  "endTime": 1704153600000
}

// 查询持仓
{
  "type": "clearinghouseState",
  "user": "0x..."
}

// 查询 K 线
{
  "type": "candleSnapshot",
  "coin": "BTC",
  "interval": "1h",
  "startTime": 1704067200000,
  "endTime": 1704153600000
}

// 查询资金费率历史
{
  "type": "fundingHistory",
  "coin": "BTC",
  "startTime": 1704067200000,
  "endTime": 1704153600000
}

// 查询市场信息
{
  "type": "meta"
}

// 查询所有市场价格
{
  "type": "allMids"
}

// 查询订单簿深度
{
  "type": "l2Book",
  "coin": "BTC"
}
```

### 6.5 响应示例

#### 6.5.1 meta 响应

```json
{
  "universe": [
    {
      "name": "BTC",
      "szDecimals": 5,
      "maxLeverage": 50,
      "onlyIsolated": false
    },
    {
      "name": "ETH",
      "szDecimals": 4,
      "maxLeverage": 50,
      "onlyIsolated": false
    }
  ]
}
```

#### 6.5.2 l2Book 响应

```json
{
  "coin": "BTC",
  "time": 1706500000000,
  "levels": [
    [
      {"px": "42000.0", "sz": "1.5", "n": 3},
      {"px": "41999.5", "sz": "2.0", "n": 2}
    ],
    [
      {"px": "42000.5", "sz": "1.2", "n": 2},
      {"px": "42001.0", "sz": "3.0", "n": 5}
    ]
  ]
}
```

### 6.6 响应示例

```json
// userFills 响应
[
  {
    "coin": "BTC",
    "px": "42150.5",
    "sz": "0.1",
    "side": "B",
    "time": 1704067200000,
    "startPosition": "0.5",
    "dir": "Open Long",
    "closedPnl": "0",
    "hash": "0x...",
    "oid": 12345,
    "crossed": true,
    "fee": "0.42",
    "tid": 67890
  }
]

// clearinghouseState 响应
{
  "marginSummary": {
    "accountValue": "10000.00",
    "totalNtlPos": "5000.00",
    "totalRawUsd": "5000.00",
    "totalMarginUsed": "500.00",
    "withdrawable": "9500.00"
  },
  "crossMarginSummary": { ... },
  "assetPositions": [
    {
      "type": "oneWay",
      "position": {
        "coin": "BTC",
        "szi": "0.5",
        "leverage": {
          "type": "cross",
          "value": 10
        },
        "entryPx": "42000.0",
        "positionValue": "21000.0",
        "unrealizedPnl": "75.25",
        "returnOnEquity": "0.0358",
        "liquidationPx": "38000.0"
      }
    }
  ]
}
```

### 6.7 Exchange API 请求示例

**请求: order (下单)**
```json
{
  "action": {
    "type": "order",
    "orders": [
      {
        "a": 0,
        "b": true,
        "p": "42000.0",
        "s": "0.1",
        "r": false,
        "t": {
          "limit": {
            "tif": "Gtc"
          }
        },
        "c": "client-order-001"
      }
    ],
    "grouping": "na"
  },
  "nonce": 1706500000000,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  },
  "vaultAddress": null
}
```

**响应: order**
```json
{
  "status": "ok",
  "response": {
    "type": "order",
    "data": {
      "statuses": [
        {
          "resting": {
            "oid": 12345
          }
        }
      ]
    }
  }
}
```

**请求: cancel (撤单)**
```json
{
  "action": {
    "type": "cancel",
    "cancels": [
      {
        "a": 0,
        "o": 12345
      }
    ]
  },
  "nonce": 1706500000001,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

### 6.8 错误响应格式

```json
{
  "status": "err",
  "response": "Error message describing the issue"
}
```

**常见错误码**：

| 错误类型 | 响应示例 |
|---------|---------|
| 参数错误 | `{"status": "err", "response": "Invalid coin: UNKNOWN"}` |
| 用户不存在 | `{"status": "err", "response": "User not found"}` |
| 余额不足 | `{"status": "err", "response": "Insufficient margin"}` |
| 订单不存在 | `{"status": "err", "response": "Order not found"}` |
| 签名错误 | `{"status": "err", "response": "Invalid signature"}` |

### 6.9 Rust API 类型定义

```rust
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;

// ==================== Info API Request ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InfoRequest {
    // 市场数据
    Meta,
    MetaAndAssetCtxs,
    SpotMeta,
    SpotMetaAndAssetCtxs,
    AllMids,

    // 订单簿与行情
    L2Book { coin: String },
    CandleSnapshot { req: CandleRequest },
    RecentTrades { coin: String },

    // 用户账户
    ClearinghouseState { user: String },
    SpotClearinghouseState { user: String },
    UserVaultEquities { user: String },

    // 订单查询
    OpenOrders { user: String },
    FrontendOpenOrders { user: String },
    OrderStatus { user: String, oid: u64 },
    HistoricalOrders { user: String },

    // 成交与资金费
    UserFills { user: String },
    UserFillsByTime { user: String, start_time: u64, end_time: Option<u64> },
    UserFunding { user: String, start_time: u64, end_time: Option<u64> },
    FundingHistory { coin: String, start_time: u64, end_time: Option<u64> },
    PredictedFundings,

    // Builder / 手续费
    MaxBuilderFee { user: String, builder: String },
    UserFees { user: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandleRequest {
    pub coin: String,
    pub interval: String,
    pub start_time: u64,
    pub end_time: Option<u64>,
}

// ==================== Exchange API Request ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRequest {
    pub action: ExchangeAction,
    pub nonce: u64,
    pub signature: Signature,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExchangeAction {
    // 订单操作
    Order {
        orders: Vec<OrderRequest>,
        grouping: OrderGrouping,
    },
    Cancel {
        cancels: Vec<CancelRequest>,
    },
    CancelByCloid {
        cancels: Vec<CancelByCloidRequest>,
    },
    Modify {
        oid: u64,
        order: OrderRequest,
    },
    BatchModify {
        modifies: Vec<ModifyRequest>,
    },

    // 账户操作
    UpdateLeverage {
        asset: u32,
        is_cross: bool,
        leverage: u32,
    },
    UpdateIsolatedMargin {
        asset: u32,
        is_buy: bool,
        ntli: i64,
    },

    // 资金操作
    UsdSend {
        destination: String,
        amount: String,
    },
    Withdraw3 {
        destination: String,
        amount: String,
    },
    VaultDeposit {
        vault_address: String,
        amount: String,
    },
    VaultWithdraw {
        vault_address: String,
        amount: String,
    },

    // 授权操作
    ApproveBuilderFee {
        builder: String,
        max_fee_rate: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub a: u32,           // asset index
    pub b: bool,          // is_buy
    pub p: String,        // price
    pub s: String,        // size
    pub r: bool,          // reduce_only
    pub t: OrderType,     // order type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>, // client_order_id
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderType {
    Limit { tif: TimeInForce },
    Trigger {
        trigger_px: String,
        is_market: bool,
        tpsl: TpSlType,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TimeInForce {
    Gtc,  // Good Till Cancel
    Ioc,  // Immediate Or Cancel
    Alo,  // Add Liquidity Only (Post Only)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TpSlType {
    Tp,  // Take Profit
    Sl,  // Stop Loss
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderGrouping {
    Na,          // No grouping
    NormalTpsl,  // Normal with TP/SL
    PositionTpsl, // Position TP/SL
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRequest {
    pub a: u32,  // asset index
    pub o: u64,  // order_id
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelByCloidRequest {
    pub asset: u32,
    pub cloid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifyRequest {
    pub oid: u64,
    pub order: OrderRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub r: String,
    pub s: String,
    pub v: u8,
}

// ==================== API Response ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiResponse<T> {
    Ok { status: String, response: T },
    Err { status: String, response: String },
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        ApiResponse::Ok {
            status: "ok".to_string(),
            response: data,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        ApiResponse::Err {
            status: "err".to_string(),
            response: msg.into(),
        }
    }
}
```

### 6.10 API 实现示例

```rust
// dex-indexer/src/api/info.rs

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum InfoRequest {
    #[serde(rename = "userFills")]
    UserFills {
        user: String,
        #[serde(rename = "startTime")]
        start_time: Option<i64>,
        #[serde(rename = "endTime")]
        end_time: Option<i64>,
    },
    #[serde(rename = "clearinghouseState")]
    ClearinghouseState {
        user: String,
    },
    #[serde(rename = "candleSnapshot")]
    CandleSnapshot {
        coin: String,
        interval: String,
        #[serde(rename = "startTime")]
        start_time: i64,
        #[serde(rename = "endTime")]
        end_time: i64,
    },
    #[serde(rename = "fundingHistory")]
    FundingHistory {
        coin: String,
        #[serde(rename = "startTime")]
        start_time: i64,
        #[serde(rename = "endTime")]
        end_time: i64,
    },
    #[serde(rename = "meta")]
    Meta,
    #[serde(rename = "allMids")]
    AllMids,
}

pub async fn handle_info(
    State(state): State<AppState>,
    Json(request): Json<InfoRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match request {
        InfoRequest::UserFills { user, start_time, end_time } => {
            let fills = state.db.get_user_fills(&user, start_time, end_time).await?;
            Ok(Json(serde_json::to_value(fills)?))
        }
        InfoRequest::ClearinghouseState { user } => {
            let state = state.db.get_clearinghouse_state(&user).await?;
            Ok(Json(serde_json::to_value(state)?))
        }
        InfoRequest::CandleSnapshot { coin, interval, start_time, end_time } => {
            let candles = state.db.get_candles(&coin, &interval, start_time, end_time).await?;
            Ok(Json(serde_json::to_value(candles)?))
        }
        // ... 其他处理
    }
}
```

---

## 7. WebSocket API 设计（Phase 2）

### 7.1 连接端点

| 环境 | WebSocket URL |
|------|--------------|
| 主网 | `wss://api.dex.example.com/ws` |
| 测试网 | `wss://api-testnet.dex.example.com/ws` |

### 7.2 订阅类型

#### 7.2.1 公开数据订阅

| 订阅类型 | 说明 | 参数 | 推送频率 |
|---------|------|------|---------|
| `allMids` | 所有交易对中间价 | 无 | 实时 |
| `l2Book` | 订单簿深度 | `coin` | 实时（有变化时） |
| `trades` | 最新成交 | `coin` | 实时 |
| `candle` | K线更新 | `coin`, `interval` | 每秒/每根K线 |

#### 7.2.2 用户数据订阅（需认证）

| 订阅类型 | 说明 | 参数 | 推送频率 |
|---------|------|------|---------|
| `orderUpdates` | 订单状态变化 | `user` | 实时 |
| `userFills` | 用户成交推送 | `user` | 实时 |
| `userFunding` | 用户资金费结算 | `user` | 每8小时 |
| `webData2` | 用户综合数据 | `user` | 实时 |

### 7.3 消息格式

#### 7.3.1 订阅请求

```json
{
  "method": "subscribe",
  "subscription": {
    "type": "l2Book",
    "coin": "BTC"
  }
}
```

```json
{
  "method": "subscribe",
  "subscription": {
    "type": "orderUpdates",
    "user": "0x1234..."
  }
}
```

#### 7.3.2 取消订阅

```json
{
  "method": "unsubscribe",
  "subscription": {
    "type": "l2Book",
    "coin": "BTC"
  }
}
```

#### 7.3.3 推送消息格式

**allMids 推送**
```json
{
  "channel": "allMids",
  "data": {
    "mids": {
      "BTC": "42000.5",
      "ETH": "2500.0",
      "SOL": "95.5"
    },
    "time": 1706500000000
  }
}
```

**l2Book 推送**
```json
{
  "channel": "l2Book",
  "data": {
    "coin": "BTC",
    "time": 1706500000000,
    "levels": [
      [
        {"px": "42000.0", "sz": "1.5", "n": 3},
        {"px": "41999.5", "sz": "2.0", "n": 2}
      ],
      [
        {"px": "42000.5", "sz": "1.2", "n": 2},
        {"px": "42001.0", "sz": "3.0", "n": 5}
      ]
    ]
  }
}
```

**trades 推送**
```json
{
  "channel": "trades",
  "data": [
    {
      "coin": "BTC",
      "side": "B",
      "px": "42000.0",
      "sz": "0.1",
      "time": 1706500000000,
      "hash": "0xabc123...",
      "tid": 12345
    }
  ]
}
```

**candle 推送**
```json
{
  "channel": "candle",
  "data": {
    "t": 1706500000000,
    "T": 1706503600000,
    "s": "BTC",
    "i": "1h",
    "o": "42000.0",
    "c": "42500.0",
    "h": "42800.0",
    "l": "41800.0",
    "v": "150.5",
    "n": 1234
  }
}
```

**orderUpdates 推送**
```json
{
  "channel": "orderUpdates",
  "data": [
    {
      "order": {
        "coin": "BTC",
        "side": "B",
        "limitPx": "42000.0",
        "sz": "0.1",
        "oid": 12345,
        "timestamp": 1706500000000,
        "origSz": "0.1",
        "cloid": "client-order-001"
      },
      "status": "open",
      "statusTimestamp": 1706500000000
    }
  ]
}
```

**userFills 推送**
```json
{
  "channel": "userFills",
  "data": {
    "user": "0x1234...",
    "fills": [
      {
        "coin": "BTC",
        "px": "42000.0",
        "sz": "0.1",
        "side": "B",
        "time": 1706500000000,
        "startPosition": "0.4",
        "dir": "Open Long",
        "closedPnl": "0.0",
        "hash": "0xabc123...",
        "oid": 12345,
        "crossed": true,
        "fee": "2.1",
        "tid": 67890
      }
    ]
  }
}
```

### 7.4 心跳保活机制

**心跳请求（客户端发送）**
```json
{
  "method": "ping"
}
```

**心跳响应（服务端返回）**
```json
{
  "channel": "pong"
}
```

**超时规则**：
- 服务端 60 秒无活动发送 ping
- 客户端需在 10 秒内响应 pong
- 建议客户端每 30 秒主动发送 ping

### 7.5 重连策略

```rust
/// WebSocket 重连配置
pub struct ReconnectConfig {
    /// 初始重连延迟 (毫秒)
    pub initial_delay_ms: u64,      // 默认: 1000
    /// 最大重连延迟 (毫秒)
    pub max_delay_ms: u64,          // 默认: 30000
    /// 延迟增长倍数
    pub backoff_multiplier: f64,    // 默认: 2.0
    /// 最大重连次数 (0=无限)
    pub max_retries: u32,           // 默认: 0
    /// 添加随机抖动
    pub jitter: bool,               // 默认: true
}
```

**重连流程**：
1. 连接断开 → 等待 `initial_delay_ms`
2. 第 N 次重试 → 等待 `min(initial_delay_ms * backoff_multiplier^N, max_delay_ms)`
3. 重连成功 → 重新订阅所有频道
4. 重连失败达 `max_retries` → 通知上层应用

### 7.6 Rust WebSocket 类型定义

```rust
use serde::{Deserialize, Serialize};

// ==================== WebSocket 消息 ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum WsClientMessage {
    Subscribe { subscription: WsSubscription },
    Unsubscribe { subscription: WsSubscription },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WsSubscription {
    AllMids,
    L2Book { coin: String },
    Trades { coin: String },
    Candle { coin: String, interval: String },
    OrderUpdates { user: String },
    UserFills { user: String },
    UserFunding { user: String },
    WebData2 { user: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel", rename_all = "camelCase")]
pub enum WsServerMessage {
    // 系统消息
    Pong,
    Error { data: String },
    SubscriptionResponse { data: SubscriptionResult },

    // 公开数据
    AllMids { data: AllMidsData },
    L2Book { data: L2BookData },
    Trades { data: Vec<TradeData> },
    Candle { data: CandleData },

    // 用户数据
    OrderUpdates { data: Vec<OrderUpdateData> },
    UserFills { data: UserFillsData },
    UserFunding { data: UserFundingData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionResult {
    pub method: String,
    pub subscription: WsSubscription,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllMidsData {
    pub mids: std::collections::HashMap<String, String>,
    pub time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2BookData {
    pub coin: String,
    pub time: u64,
    pub levels: (Vec<Level>, Vec<Level>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    pub px: String,
    pub sz: String,
    pub n: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub coin: String,
    pub side: String,
    pub px: String,
    pub sz: String,
    pub time: u64,
    pub hash: String,
    pub tid: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandleData {
    pub t: u64,           // open time
    #[serde(rename = "T")]
    pub close_time: u64,  // close time
    pub s: String,        // symbol
    pub i: String,        // interval
    pub o: String,        // open
    pub c: String,        // close
    pub h: String,        // high
    pub l: String,        // low
    pub v: String,        // volume
    pub n: u64,           // trades count
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderUpdateData {
    pub order: OrderInfo,
    pub status: String,
    pub status_timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderInfo {
    pub coin: String,
    pub side: String,
    pub limit_px: String,
    pub sz: String,
    pub oid: u64,
    pub timestamp: u64,
    pub orig_sz: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFillsData {
    pub user: String,
    pub fills: Vec<FillInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillInfo {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub start_position: String,
    pub dir: String,
    pub closed_pnl: String,
    pub hash: String,
    pub oid: u64,
    pub crossed: bool,
    pub fee: String,
    pub tid: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFundingData {
    pub user: String,
    pub funding_payments: Vec<FundingPayment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingPayment {
    pub coin: String,
    pub usdc: String,
    pub szi: String,
    pub funding_rate: String,
    pub time: u64,
}
```

---

## 8. 数据模型

### 8.1 核心实体定义

#### 8.1.1 Market（市场/交易对）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpAsset {
    pub name: String,           // "BTC"
    pub sz_decimals: u32,       // 数量精度 (5 = 0.00001)
    pub max_leverage: u32,      // 最大杠杆 (50)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_isolated: Option<bool>,  // 是否仅支持逐仓
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotAsset {
    pub name: String,           // "USDC"
    pub sz_decimals: u32,       // 数量精度
    pub wei_decimals: u32,      // Wei 精度
    pub index: u32,             // 资产索引
    pub token_id: String,       // 代币 ID
    pub is_canonical: bool,     // 是否为规范代币
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetCtx {
    pub funding: String,        // 当前资金费率
    pub open_interest: String,  // 未平仓合约
    pub prev_day_px: String,    // 24h前价格
    pub day_ntl_vlm: String,    // 24h名义成交量
    pub premium: Option<String>, // 溢价率
    pub oracle_px: String,      // 预言机价格
    pub mark_px: String,        // 标记价格
    pub mid_px: Option<String>, // 中间价
    pub impact_pxs: Option<(String, String)>, // (买入/卖出冲击价)
}
```

#### 8.1.2 Order（订单）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOrder {
    pub coin: String,
    pub limit_px: String,
    pub oid: u64,
    pub side: String,           // "B" | "A"
    pub sz: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendOrder {
    pub coin: String,
    pub is_position_tpsl: bool,
    pub is_trigger: bool,
    pub limit_px: String,
    pub oid: u64,
    pub order_type: String,
    pub orig_sz: String,
    pub reduce_only: bool,
    pub side: String,
    pub sz: String,
    pub timestamp: u64,
    pub trigger_condition: String,
    pub trigger_px: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<String>,
    pub children: Option<Vec<FrontendOrder>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalOrder {
    pub order: FrontendOrder,
    pub status: String,
    pub status_timestamp: u64,
}
```

#### 8.1.3 Fill（成交）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFill {
    pub coin: String,
    pub px: String,
    pub sz: String,
    pub side: String,
    pub time: u64,
    pub start_position: String,
    pub dir: String,            // "Open Long" | "Close Long" | "Open Short" | "Close Short"
    pub closed_pnl: String,
    pub hash: String,
    pub oid: u64,
    pub crossed: bool,          // true=吃单, false=挂单
    pub fee: String,
    pub tid: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidation_mark_px: Option<String>,
}
```

#### 8.1.4 Position（持仓）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPosition {
    pub position: PositionInfo,
    #[serde(rename = "type")]
    pub position_type: String,  // "oneWay"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionInfo {
    pub coin: String,
    pub szi: String,            // 持仓数量 (正=多, 负=空)
    pub leverage: LeverageInfo,
    pub entry_px: Option<String>,
    pub position_value: String,
    pub unrealized_pnl: String,
    pub return_on_equity: String,
    pub liquidation_px: Option<String>,
    pub margin_used: String,
    pub max_trade_szs: (String, String),  // (可买, 可卖)
    pub cum_funding: CumFunding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeverageInfo {
    #[serde(rename = "type")]
    pub leverage_type: String,  // "cross" | "isolated"
    pub value: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_usd: Option<String>,  // 逐仓保证金
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CumFunding {
    pub all_time: String,
    pub since_open: String,
    pub since_change: String,
}
```

#### 8.1.5 Margin（保证金）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginSummary {
    pub account_value: String,
    pub total_ntl_pos: String,
    pub total_raw_usd: String,
    pub total_margin_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearinghouseState {
    pub margin_summary: MarginSummary,
    pub cross_margin_summary: MarginSummary,
    pub withdrawable: String,
    pub asset_positions: Vec<AssetPosition>,
    pub time: u64,
}
```

#### 8.1.6 Candle（K线）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub t: u64,           // 开盘时间
    #[serde(rename = "T")]
    pub close_time: u64,  // 收盘时间
    pub s: String,        // 交易对
    pub i: String,        // 时间周期 (1m, 5m, 15m, 1h, 4h, 1d)
    pub o: String,        // 开盘价
    pub c: String,        // 收盘价
    pub h: String,        // 最高价
    pub l: String,        // 最低价
    pub v: String,        // 成交量
    pub n: u64,           // 成交笔数
}
```

#### 8.1.7 Funding（资金费）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingHistory {
    pub coin: String,
    pub funding_rate: String,
    pub premium: String,
    pub time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFundingRecord {
    pub time: u64,
    pub coin: String,
    pub usdc: String,
    pub szi: String,
    pub funding_rate: String,
}
```

### 8.2 响应结构示例

#### 8.2.1 meta 响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaResponse {
    pub universe: Vec<PerpAsset>,
}
```

#### 8.2.2 metaAndAssetCtxs 响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaAndAssetCtxsResponse(pub MetaResponse, pub Vec<AssetCtx>);
```

#### 8.2.3 l2Book 响应

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2BookResponse {
    pub coin: String,
    pub time: u64,
    pub levels: (Vec<L2Level>, Vec<L2Level>),  // (bids, asks)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Level {
    pub px: String,
    pub sz: String,
    pub n: u32,
}
```

---

## 9. Phase 2 架构扩展：实时性增强

### 9.1 Phase 2 架构（可选）

如果 Phase 1 的 ~700ms 延迟无法满足需求，Phase 2 可添加 gRPC 实时通道：

```
┌─────────────────────────────────────────────────────────────────┐
│                     DEX 引擎                                     │
│                                                                  │
│  撮合完成 ─┬─► TransactionEvents (进入 Checkpoint，~700ms)      │
│            │                                                     │
│            └─► gRPC 实时推送 (可选，<10ms)                       │
└──────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              │                               │
              ▼                               ▼
       ┌─────────────┐               ┌─────────────┐
       │ Checkpoint  │               │ gRPC 实时流  │
       │ (权威源)    │               │ (预览)       │
       └──────┬──────┘               └──────┬──────┘
              │                              │
              │                              ▼
              │                       ┌─────────────┐
              │                       │   Redis     │
              │                       │ (实时缓存)  │
              │                       └──────┬──────┘
              │                              │
              └──────────────┬───────────────┘
                             │
                             ▼
                      ┌─────────────┐
                      │ 去重 & 确认 │
                      └─────────────┘
                             │
                             ▼
                      ┌─────────────┐
                      │ PostgreSQL  │
                      │ (历史数据)  │
                      └─────────────┘
```

### 7.2 Phase 2 事件去重

```rust
// Phase 2: 去重逻辑
struct EventDeduplicator {
    seen_events: DashSet<[u8; 32]>,  // tx_digest + event_index
}

impl EventDeduplicator {
    fn should_process(&self, event: &DexEvent) -> bool {
        let key = compute_event_key(event);
        self.seen_events.insert(key)  // 返回 true = 首次见到
    }

    fn mark_confirmed(&self, checkpoint_events: &[DexEvent]) {
        // Checkpoint 事件确认后，从去重集合中清理旧数据
        for event in checkpoint_events {
            self.seen_events.remove(&compute_event_key(event));
        }
    }
}
```

---

## 10. 测试策略

### 10.1 测试层次

| 层次 | 测试类型 | 覆盖范围 |
|------|----------|----------|
| 单元测试 | Handler 解析逻辑 | BCS 解码、数据转换 |
| 集成测试 | Pipeline 端到端 | Checkpoint → DB |
| E2E 测试 | 完整链路 | 下单 → 事件 → API |

### 10.2 测试用例示例

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fill_event_parsing() {
        // 构造 BCS 编码的 FillEvent
        let fill = FillEvent {
            perpetual_id: 1,
            taker_order_id: vec![1, 2, 3],
            maker_order_id: vec![4, 5, 6],
            // ...
        };
        let bcs_bytes = bcs::to_bytes(&fill).unwrap();

        // 模拟从 Event 解析
        let parsed: FillEvent = bcs::from_bytes(&bcs_bytes).unwrap();
        assert_eq!(parsed.perpetual_id, 1);
    }

    #[tokio::test]
    async fn test_fills_handler_process() {
        // 构造 mock Checkpoint
        let checkpoint = create_mock_checkpoint_with_fill_events();

        // 执行 Handler
        let handler = FillsHandler;
        let results = handler.process(&checkpoint).unwrap();

        // 验证结果
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].perpetual_id, 1);
    }
}
```

### 10.3 与现有测试集成

```rust
// dex-sui/crates/sui-e2e-tests/tests/dex_indexer_tests.rs

#[sim_test]
async fn test_dex_events_in_checkpoint() {
    let cluster = TestClusterBuilder::new().build().await;

    // 执行 DEX 交易
    let tx = cluster.execute_dex_place_order(...).await;

    // 等待 Checkpoint
    let checkpoint = cluster.wait_for_checkpoint(tx.digest()).await;

    // 验证事件存在
    let events = checkpoint.transactions[0].events.as_ref().unwrap();
    assert!(events.data.iter().any(|e|
        e.package_id == ObjectID::from(DEX_EVENTS_PACKAGE) &&
        e.type_.name.as_str() == "FillEvent"
    ));
}
```

---

## 11. 部署架构

### 11.1 单节点部署

```
┌─────────────────────────────────────────────────────────────┐
│                      服务器                                  │
│                                                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │ dex-indexer │───►│ PostgreSQL  │◄───│ REST API    │     │
│  │ (indexing)  │    │             │    │ (querying)  │     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
│         │                                     │              │
│         │         Checkpoint 订阅             │              │
│         ▼                                     ▼              │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   Sui Full Node                      │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### 11.2 Docker Compose

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: dex_indexer
      POSTGRES_USER: dex
      POSTGRES_PASSWORD: ${DB_PASSWORD}
    volumes:
      - postgres_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"

  dex-indexer:
    build: ./dex-indexer
    environment:
      DATABASE_URL: postgres://dex:${DB_PASSWORD}@postgres:5432/dex_indexer
      SUI_RPC_URL: http://sui-fullnode:9000
      RUST_LOG: info
    depends_on:
      - postgres
    ports:
      - "3000:3000"  # REST API

volumes:
  postgres_data:
```

---

## 12. 实施路线图

### 12.1 Phase 概览

| Phase | 名称 | 内容 | 预计工作量 |
|-------|------|------|-----------|
| **1** | DEX 事件发出 | 修改 dex.rs 发出事件 (无需部署 Package) | ~2 天 |
| **2** | Indexer 核心 | Handlers + Schema + Pipeline | ~5 天 |
| **3** | REST API | /info + /exchange 端点 | ~4 天 |
| **4** | 集成测试 | E2E 测试 + 性能验证 | ~3 天 |
| **5** | 部署运维 | Docker + 监控 | ~2 天 |
| **总计** | | | **~16 天 (~3 周)** |

### 12.2 里程碑

| 里程碑 | 完成标准 | 预计时间 |
|--------|----------|----------|
| **M1** | DEX 交易产生 FillEvent，可在 Checkpoint 中查看 | Week 1 |
| **M2** | dex-indexer 运行，fills 表有数据 | Week 2 |
| **M3** | REST API 可用，现有 DEX 测试通过 | Week 3 |
| **M4** | 生产就绪（Docker + 监控） | Week 3.5 |

### 12.3 与 V3 工作量对比

| 组件 | V3 估算 | V4 估算 | 节省 |
|------|---------|---------|------|
| gRPC Server (Engine) | 5 天 | 0 | 100% |
| gRPC Client (Indexer) | 5 天 | 0 | 100% |
| Pipeline 框架 | 5 天 | 0 (复用) | 100% |
| DEX 事件发出 | 3 天 | 3 天 | 0% |
| Handlers | 3 天 | 5 天 | -67% |
| REST API | 4 天 | 4 天 | 0% |
| 测试 | 5 天 | 3 天 | 40% |
| **总计** | **~30 天** | **~17 天** | **43%** |

---

## 13. 风险与缓解

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|----------|
| sui-indexer-alt-framework API 变化 | 低 | 中 | 锁定版本，关注上游更新 |
| 事件数据量过大 | 中 | 中 | 分区表 + 保留策略 |
| ~700ms 延迟不满足需求 | 中 | 中 | Phase 2 添加 gRPC 实时通道 |
| 虚拟 Package 生态兼容问题 | 低 | 低 | DEX 自有 Indexer，不依赖第三方工具 |

---

## 14. 决策待定项

请确认以下决策：

1. ~~**占位 Package 地址**~~: ✅ 已决策 - 使用纯虚拟地址 `0x...44455800` (无需部署)
2. **K 线粒度**: 支持哪些分辨率？ (建议: 1m, 5m, 15m, 1h, 4h, 1d)
3. **数据保留期**: fills 表保留多久？ (建议: 90 天)
4. **Phase 2 实时性**: 是否需要规划？何时启动？

---

## 附录 A: 代码量估算明细

| 文件/模块 | 估算行数 | 说明 |
|-----------|----------|------|
| `sui-types/src/dex_events.rs` | ~200 | 事件类型定义 + 虚拟 Package 常量 |
| `sui-execution/src/dex.rs` 修改 | ~100 | 事件发出逻辑 |
| `dex-indexer/src/handlers/*.rs` | ~400 | 6-7 个 Handler |
| `dex-indexer/src/schema/` | ~150 | 数据库迁移 SQL |
| `dex-indexer/src/api/*.rs` | ~300 | REST API 端点 |
| `dex-indexer/src/main.rs` | ~50 | 入口程序 |
| Move Package | 0 | **无需部署** |
| **总计** | **~1200 行** | 纯 Rust，无 Move |

## 附录 B: 参考文档

- `dex-event-parsing-analysis.md` - 事件解析方案详细分析
- `sui-indexer-alt-analyst.md` - sui-indexer-alt 框架分析
- `dex-indexer-tech-v3.md` - 前版本技术方案
