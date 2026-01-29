# Sui 原生 Rust 代码发出 Event 并被 sui-indexer 索引的可行性分析

## 1. 执行摘要

### 问题
PerpVM（原生 Rust DEX 引擎）能否通过构造 `Event` 对象并添加到 `ExecutionResultsV2.user_events`，使事件自然流入 sui-indexer？

### 答案
**技术上可行，但需要解决以下关键问题：**

| 问题 | 难度 | 解决方案 |
|------|------|----------|
| Event 需要 StructTag (Move 类型) | 中 | 注册虚拟 Move 类型或使用通用包装器 |
| 获取 ExecutionResultsV2 引用 | 中 | 修改执行流程，传递 mutable 引用 |
| 事件影响共识 (events_digest) | 高 | 必须确保所有验证器产生相同事件 |
| sui-indexer 解析事件内容 | 低 | 自定义解析逻辑 |

**结论**: 可行，但需要对 Sui 执行层进行适度修改。

---

## 2. 事件数据结构分析

### 2.1 Event 结构

```rust
// sui-types/src/event.rs:106-113
pub struct Event {
    pub package_id: ObjectID,        // 发出事件的 package ID
    pub transaction_module: Identifier, // 模块名称
    pub sender: SuiAddress,          // 交易发送者
    pub type_: StructTag,            // 事件类型 (Move 类型)
    pub contents: Vec<u8>,           // BCS 序列化的事件内容
}

impl Event {
    pub fn new(
        package_id: &AccountAddress,
        module: &IdentStr,
        sender: SuiAddress,
        type_: StructTag,
        contents: Vec<u8>,
    ) -> Self { ... }
}
```

### 2.2 事件流程

```
┌─────────────────────────────────────────────────────────────────────────┐
│                            事件生成流程                                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Move 执行                                                               │
│  ────────                                                                │
│  sui::event::emit<T>()                                                   │
│       │                                                                  │
│       ▼                                                                  │
│  ObjectRuntime.state.events: Vec<(StructTag, Value)>                    │
│       │                                                                  │
│       │ take_user_events()                                               │
│       ▼                                                                  │
│  ExecutionResultsV2.user_events: Vec<Event>                             │
│       │                                                                  │
│       │ TemporaryStore.into_inner()                                      │
│       ▼                                                                  │
│  InnerTemporaryStore.events: TransactionEvents                          │
│       │                                                                  │
│       │ into_effects()                                                   │
│       ▼                                                                  │
│  TransactionEffects.events_digest: Option<TransactionEventsDigest>      │
│       │                                                                  │
│       │ CheckpointBuilder                                                │
│       ▼                                                                  │
│  CheckpointData.transactions[].events                                   │
│       │                                                                  │
│       │ gRPC 订阅                                                        │
│       ▼                                                                  │
│  sui-indexer (写入数据库)                                                │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.3 关键代码位置

| 组件 | 文件 | 说明 |
|------|------|------|
| Event 结构 | `sui-types/src/event.rs:106-113` | 事件数据结构 |
| ExecutionResultsV2 | `sui-types/src/execution.rs:82-105` | 执行结果，包含 user_events |
| TemporaryStore | `sui-execution/latest/sui-adapter/src/temporary_store.rs:37-84` | 临时存储 |
| into_inner() | `temporary_store.rs:160-176` | 转换为 InnerTemporaryStore |
| TransactionEvents | `sui-types/src/effects/mod.rs` | 事件集合 |

---

## 3. 可行性分析

### 3.1 方案概述

用户提出的方案：

```rust
// 在 PerpVM 中直接构造 Event
let event = Event::new(
    &package_id,        // 虚拟 package ID
    &module_name,       // 模块名称 (如 "perp_vm")
    sender,             // 交易发送者
    event_type,         // StructTag (事件类型)
    bcs_serialized_contents,  // BCS 序列化内容
);

// 添加到 ExecutionResultsV2.user_events
execution_results.user_events.push(event);
```

### 3.2 技术可行性评估

#### 问题 1: Event 需要 StructTag (Move 类型)

**现状**:
- `Event.type_` 是 `StructTag`，需要 Move 类型信息
- PerpVM 事件是 Rust 结构体，没有对应的 Move 类型

**解决方案**:

**方案 A: 创建虚拟 Move Package**
```move
// 0xDEX::perp_events
module perp_events {
    struct FillEvent has copy, drop {
        order_id: u128,
        price: u64,
        quantity: u64,
        // ... 其他字段
    }

    struct PositionEvent has copy, drop { ... }
    struct BalanceEvent has copy, drop { ... }
}
```

然后在 Rust 中构造对应的 StructTag：
```rust
let event_type = StructTag {
    address: DEX_PACKAGE_ADDRESS,
    module: ident_str!("perp_events").into(),
    name: ident_str!("FillEvent").into(),
    type_params: vec![],
};
```

**方案 B: 通用事件包装器**
```move
// 0xDEX::generic_event
module generic_event {
    struct DexEvent has copy, drop {
        event_type: vector<u8>,  // 事件类型标识
        data: vector<u8>,        // BCS 编码的事件数据
    }
}
```

**推荐**: 方案 A，因为 sui-indexer 可以更好地解析结构化事件。

---

#### 问题 2: 获取 ExecutionResultsV2 引用

**现状**:
- `ExecutionResultsV2` 存储在 `TemporaryStore.execution_results`
- `TemporaryStore` 在 `execute_transaction_to_effects` 中创建
- PerpVM 需要在执行过程中访问它

**解决方案**:

**方案 A: 传递 mutable 引用**

修改 PerpVM 执行接口：

```rust
// 修改 PerpVM 接口
pub trait PerpVmExecutor {
    fn execute(
        &self,
        tx: &DexTransaction,
        execution_results: &mut ExecutionResultsV2,  // 传入引用
    ) -> Result<(), ExecutionError>;
}

// 在 execute_transaction 中调用
fn execute_transaction<Mode: ExecutionMode>(
    // ... 其他参数
    temporary_store: &mut TemporaryStore<'_>,
    // ...
) {
    // 执行 PerpVM
    perp_vm.execute(tx, &mut temporary_store.execution_results)?;
}
```

**方案 B: 执行后回调**

```rust
// 在 execute_transaction 完成后，into_inner() 调用前
fn execute_transaction_to_effects<Mode: ExecutionMode>(...) {
    // ... 正常执行

    // 注入 PerpVM 事件
    let perp_events = perp_vm.take_events();
    temporary_store.execution_results.user_events.extend(perp_events);

    // 转换为 effects
    let (inner, effects) = temporary_store.into_effects(...);
}
```

**推荐**: 方案 A，更清晰的接口设计。

---

#### 问题 3: 事件影响共识 (events_digest)

**现状**:
- `TransactionEffects` 包含 `events_digest`
- 所有验证器必须产生相同的 effects 才能达成共识
- 如果 PerpVM 在不同验证器上产生不同事件，会导致共识失败

```rust
// temporary_store.rs:292-296
if inner.events.data.is_empty() {
    None
} else {
    Some(inner.events.digest())  // 事件 digest 进入 effects
}
```

**解决方案**:

**必要条件**: PerpVM 必须是**确定性**的
- 相同输入 → 相同输出
- 相同输入 → 相同事件

**验证机制**:
```rust
impl PerpVmExecutor {
    fn execute(&self, tx: &DexTransaction, results: &mut ExecutionResultsV2) {
        // 确定性执行
        let (state_changes, events) = self.deterministic_execute(tx);

        // 事件必须与状态变化完全对应
        results.user_events.extend(events);
    }
}
```

**风险**: 如果 PerpVM 有任何非确定性行为（如浮点运算、随机数、时间依赖），会导致分叉。

**推荐**: 使用 Sui 的时间戳和随机数源，避免本地状态依赖。

---

#### 问题 4: sui-indexer 解析事件内容

**现状**:
- sui-indexer 存储事件的 BCS 编码内容
- 解析时需要 Move 类型布局

**解决方案**:

**方案 A: 注册类型布局**

在 sui-indexer 中注册 PerpVM 事件类型的解析器：

```rust
// 自定义事件解析器
fn parse_perp_event(event: &Event) -> Result<PerpEvent, Error> {
    if event.type_.module.as_str() == "perp_events" {
        match event.type_.name.as_str() {
            "FillEvent" => {
                let fill: FillEvent = bcs::from_bytes(&event.contents)?;
                Ok(PerpEvent::Fill(fill))
            }
            // ... 其他事件类型
        }
    }
}
```

**方案 B: 使用 JSON 编码**

在事件内容中使用 JSON 而非纯 BCS：
```rust
let contents = serde_json::to_vec(&fill_event)?;
```

**推荐**: 方案 A，保持与 Sui 生态一致。

---

## 4. 实现方案

### 4.1 修改点清单

| 组件 | 修改内容 | 影响范围 |
|------|----------|----------|
| sui-types | 添加 PerpVM 事件类型定义 | 低 |
| sui-execution | 修改 execute_transaction 接口 | 中 |
| PerpVM | 实现事件生成逻辑 | 低 |
| sui-indexer | 添加事件解析器（可选） | 低 |

### 4.2 代码修改示例

**Step 1: 定义事件类型**

```rust
// crates/sui-types/src/perp_events.rs
use move_core_types::language_storage::StructTag;

pub const PERP_PACKAGE_ID: ObjectID = ObjectID::from_address(/* 0xDEX */);
pub const PERP_MODULE: &str = "perp_events";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillEvent {
    pub order_id: u128,
    pub market_id: u64,
    pub side: u8,
    pub price: u64,
    pub quantity: u64,
    pub maker: SuiAddress,
    pub taker: SuiAddress,
    pub timestamp: u64,
}

impl FillEvent {
    pub fn struct_tag() -> StructTag {
        StructTag {
            address: PERP_PACKAGE_ID.into(),
            module: ident_str!(PERP_MODULE).into(),
            name: ident_str!("FillEvent").into(),
            type_params: vec![],
        }
    }

    pub fn to_event(&self, sender: SuiAddress) -> Event {
        Event::new(
            &PERP_PACKAGE_ID.into(),
            ident_str!(PERP_MODULE),
            sender,
            Self::struct_tag(),
            bcs::to_bytes(self).unwrap(),
        )
    }
}
```

**Step 2: 修改执行引擎接口**

```rust
// sui-execution/latest/sui-adapter/src/execution_engine.rs

pub trait PerpVmHook {
    fn on_transaction_executed(
        &self,
        tx_digest: &TransactionDigest,
        execution_results: &mut ExecutionResultsV2,
    ) -> Result<(), ExecutionError>;
}

fn execute_transaction<Mode: ExecutionMode>(
    // ... 现有参数
    perp_vm_hook: Option<&dyn PerpVmHook>,  // 新增参数
) -> (...) {
    // ... 正常 Move 执行

    // 调用 PerpVM hook
    if let Some(hook) = perp_vm_hook {
        hook.on_transaction_executed(
            &tx_ctx.borrow().digest(),
            &mut temporary_store.execution_results,
        )?;
    }

    // ... 继续处理
}
```

**Step 3: PerpVM 实现**

```rust
// perp-vm/src/event_emitter.rs

pub struct PerpVmEventEmitter;

impl PerpVmHook for PerpVmEventEmitter {
    fn on_transaction_executed(
        &self,
        tx_digest: &TransactionDigest,
        execution_results: &mut ExecutionResultsV2,
    ) -> Result<(), ExecutionError> {
        // 从 PerpVM 状态获取待发送事件
        let events = PERP_VM_STATE.take_pending_events();

        for event in events {
            execution_results.user_events.push(event);
        }

        Ok(())
    }
}
```

---

## 5. 风险评估

### 5.1 共识风险

| 风险 | 严重程度 | 缓解措施 |
|------|----------|----------|
| PerpVM 非确定性执行 | 致命 | 严格的确定性测试 |
| 事件顺序不一致 | 高 | 使用确定性排序 |
| 事件内容不一致 | 高 | 完整的状态复现 |

### 5.2 兼容性风险

| 风险 | 严重程度 | 缓解措施 |
|------|----------|----------|
| 虚拟 Move 类型与真实类型冲突 | 中 | 使用保留地址空间 |
| sui-indexer 解析失败 | 低 | 自定义解析逻辑 |
| 第三方工具不兼容 | 低 | 文档说明 |

---

## 6. 替代方案对比

| 方案 | 复杂度 | 索引兼容性 | 共识安全 |
|------|--------|-----------|----------|
| **A: 注入 ExecutionResultsV2** | 中 | 高 (原生支持) | 需要确定性 |
| B: 独立 gRPC 事件流 | 低 | 无 (自建索引) | 无风险 |
| C: 写入 Accumulator | 高 | 中 | 需要确定性 |
| D: 伪造 Move 调用 | 高 | 高 | 复杂 |

**推荐**: 方案 A，因为：
1. 与 Sui 原生事件系统完全兼容
2. sui-indexer 无需修改（或仅需小幅修改）
3. 实现复杂度适中

---

## 7. 结论

### 7.1 可行性判定

**技术上可行**，PerpVM 可以通过以下方式发出被 sui-indexer 索引的事件：

1. 创建虚拟 Move 包定义事件类型
2. 修改执行引擎，在 Move 执行后注入 PerpVM 事件
3. 确保 PerpVM 执行完全确定性
4. （可选）在 sui-indexer 中添加自定义解析逻辑

### 7.2 实现建议

1. **Phase 1**: 在测试网验证确定性
2. **Phase 2**: 实现事件注入机制
3. **Phase 3**: 集成 sui-indexer 解析
4. **Phase 4**: 主网部署

### 7.3 代码引用

| 文件 | 行号 | 说明 |
|------|------|------|
| `sui-types/src/event.rs` | 106-129 | Event 结构和构造函数 |
| `sui-types/src/execution.rs` | 82-105 | ExecutionResultsV2 定义 |
| `sui-execution/latest/sui-adapter/src/temporary_store.rs` | 160-176 | into_inner() 事件转换 |
| `sui-execution/latest/sui-adapter/src/execution_engine.rs` | 264-272 | into_effects() 调用点 |
