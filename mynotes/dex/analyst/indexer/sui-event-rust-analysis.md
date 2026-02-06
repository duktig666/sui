# Sui 原生 Rust 代码发出 Event 可行性分析

> 分析日期: 2026-01-28
> 分析目标: 评估是否可以修改 Sui 代码，使原生 Rust 代码能够发出可被 sui-indexer-alt 索引的 Event

## 1. 执行摘要

### 结论

**技术上可行，但代价极高，不推荐实施。**

修改 Sui 使原生 Rust 代码发出事件需要：
- 修改 Sui 核心协议
- 触发协议升级（影响所有验证器）
- 重新设计 Event 数据结构
- 更新所有索引器和 RPC 服务

### 推荐方案

对于原生 Rust DEX 引擎，**自建事件索引系统**是更实际的选择（见第 5 节）。如果确实需要修改 Sui，**方案 B（合成事件包）** 是最具可行性的路径。

---

## 2. 当前事件系统分析

### 2.1 事件流程概览

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              事件发出流程                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Move 代码层                                                                 │
│  ───────────                                                                │
│  sui::event::emit<T>(event_data)                                            │
│         │                                                                    │
│         ▼                                                                    │
│  Native 实现层                                                               │
│  ─────────────                                                               │
│  emit() [sui-execution/latest/sui-move-natives/src/event.rs:43-54]          │
│         │                                                                    │
│         │  依赖: NativeContext (Move VM 上下文)                              │
│         │                                                                    │
│         ▼                                                                    │
│  ObjectRuntime.emit_event(tag, value)                                       │
│  [sui-execution/latest/sui-move-natives/src/object_runtime/mod.rs:359-365]  │
│         │                                                                    │
│         ▼                                                                    │
│  state.events.push((tag, value))  ← Vec<(StructTag, Value)>                 │
│         │                                                                    │
├─────────┼────────────────────────────────────────────────────────────────────┤
│         │                                                                    │
│  执行完成后                                                                  │
│  ─────────                                                                   │
│  take_user_events() → (ModuleId, StructTag, Vec<u8>)                        │
│  [context.rs:318-366]                                                        │
│         │                                                                    │
│         ▼                                                                    │
│  Event::new(package_id, module, sender, type_, contents)                    │
│  [sui-types/src/event.rs:116-130]                                           │
│         │                                                                    │
│         ▼                                                                    │
│  TransactionEvents { data: Vec<Event> }                                     │
│  [sui-types/src/effects/mod.rs:421-423]                                     │
│         │                                                                    │
│         ▼                                                                    │
│  InnerTemporaryStore.events → Checkpoint → sui-indexer-alt                  │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 关键代码引用

#### Native emit 函数

> 源码: [`sui-execution/latest/sui-move-natives/src/event.rs:43-54`](../../../../sui-execution/latest/sui-move-natives/src/event.rs)

```rust
pub fn emit(
    context: &mut NativeContext,  // Move VM 执行上下文
    mut ty_args: Vec<Type>,       // Move 类型参数
    mut args: VecDeque<Value>,    // Move 值参数
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.len() == 1);
    debug_assert!(args.len() == 1);

    let ty = ty_args.pop().unwrap();
    let event_value = args.pop_back().unwrap();
    emit_impl(context, ty, event_value, None)
}
```

**关键依赖**:
- `NativeContext`: 只在 Move VM 执行期间存在
- `Type`: Move 类型系统的运行时表示
- `Value`: Move 值的运行时表示

#### ObjectRuntime 事件存储

> 源码: [`sui-execution/latest/sui-move-natives/src/object_runtime/mod.rs:359-365`](../../../../sui-execution/latest/sui-move-natives/src/object_runtime/mod.rs)

```rust
pub fn emit_event(&mut self, tag: StructTag, event: Value) -> PartialVMResult<()> {
    if self.state.events.len() >= (self.protocol_config.max_num_event_emit() as usize) {
        return Err(max_event_error(self.protocol_config.max_num_event_emit()));
    }
    self.state.events.push((tag, event));
    Ok(())
}
```

#### Event 数据结构

> 源码: [`crates/sui-types/src/event.rs:106-130`](../../../../crates/sui-types/src/event.rs)

```rust
pub struct Event {
    pub package_id: ObjectID,         // Move 包 ID（必须是有效的链上包）
    pub transaction_module: Identifier, // Move 模块名
    pub sender: SuiAddress,
    pub type_: StructTag,             // Move 结构体类型标识
    pub contents: Vec<u8>,            // BCS 序列化的 Move 数据
}

impl Event {
    pub fn new(
        package_id: &AccountAddress,  // ← 来自 ModuleId.address()
        module: &IdentStr,            // ← 来自 ModuleId.name()
        sender: SuiAddress,
        type_: StructTag,
        contents: Vec<u8>,
    ) -> Self { /* ... */ }
}
```

### 2.3 核心约束

| 字段 | 来源 | 约束 |
|------|------|------|
| `package_id` | `ModuleId.address()` | **必须是链上已发布的 Move 包地址** |
| `transaction_module` | `ModuleId.name()` | **必须是该包中存在的模块名** |
| `type_` | `StructTag` | **必须是有效的 Move 结构体类型** |
| `contents` | BCS 序列化 | **必须是 `type_` 对应类型的有效 BCS 编码** |

**核心问题**: Event 数据结构的每个字段都与 Move 类型系统紧密耦合。原生 Rust 代码没有 Move 包、模块或类型。

---

## 3. 修改方案详解

### 3.1 方案 A: 扩展 ObjectRuntime API

**思路**: 在 `ObjectRuntime` 中添加新方法，允许直接注入事件，绕过 Move VM。

**修改点**:

1. **`object_runtime/mod.rs`** - 添加原生事件 API
```rust
// 伪代码示例
pub fn emit_native_event(
    &mut self,
    event_type: NativeEventType,  // 新的事件类型枚举
    contents: Vec<u8>,
) -> PartialVMResult<()> {
    // 验证事件大小限制
    // 存储到新的 native_events 字段
    self.state.native_events.push(NativeEvent { event_type, contents });
    Ok(())
}
```

2. **`context.rs`** - 处理原生事件
```rust
// 需要在 take_user_events 后添加 take_native_events
// 将原生事件合并到 TransactionEvents
```

3. **`sui-types/src/event.rs`** - 扩展 Event 结构
```rust
pub enum EventSource {
    Move { package_id: ObjectID, module: Identifier },
    Native { source_id: NativeSourceId },  // 新增
}

pub struct Event {
    pub source: EventSource,  // 替换 package_id + module
    pub sender: SuiAddress,
    pub type_: EventType,     // 扩展为支持原生类型
    pub contents: Vec<u8>,
}
```

4. **`sui-core/`** - 验证器需要支持新事件类型

**复杂度**: 中
**协议升级**: 需要
**兼容性影响**: 现有索引器需要更新

### 3.2 方案 B: 合成事件包（推荐修改方案）

**思路**: 部署一个特殊的 Move "占位符" 包，专门用于原生 Rust 事件。所有原生事件使用这个包的 ID 和预定义类型。

**优势**:
- 复用现有 Event 结构
- 索引器无需特殊处理
- 通过类型区分事件来源

**实现步骤**:

1. **部署系统级 Move 包** (`0xNATIVE_EVENTS`)
```move
module native_events::dex {
    /// DEX 引擎原生事件占位类型
    public struct NativeOrderEvent has copy, drop {
        event_type: u8,      // 事件子类型
        payload: vector<u8>, // 原始事件数据
    }

    public struct NativeTradeEvent has copy, drop {
        event_type: u8,
        payload: vector<u8>,
    }
}
```

2. **修改 ObjectRuntime** - 使用占位包 ID 创建事件
```rust
pub fn emit_native_event(&mut self, event_data: Vec<u8>) -> PartialVMResult<()> {
    let tag = StructTag {
        address: NATIVE_EVENTS_PACKAGE_ID,  // 固定的系统包 ID
        module: Identifier::new("dex").unwrap(),
        name: Identifier::new("NativeOrderEvent").unwrap(),
        type_params: vec![],
    };
    // 包装成 Move Value 格式
    let value = /* ... */;
    self.state.events.push((tag, value));
    Ok(())
}
```

3. **调用入口** - 创建新的交易类型或扩展现有 PTB

**复杂度**: 中高
**协议升级**: 需要
**兼容性影响**: 低（复用现有 Event 结构）

### 3.3 方案 C: 事件扩展系统

**思路**: 在 Move 事件系统之外，创建并行的原生事件通道。

```
                    ┌──────────────┐
Move VM ─────────► │ Move Events  │ ──────┐
                    └──────────────┘       │
                                           ▼
                                    ┌──────────────┐
Native Rust ─────► │Native Events │ ────► │ Event Merger │ ──► Checkpoint
                    └──────────────┘       └──────────────┘
```

**修改点**:
1. 新增 `NativeEvent` 类型和存储结构
2. 在 `InnerTemporaryStore` 中添加 `native_events` 字段
3. 在 checkpoint 创建时合并两种事件
4. 索引器需要处理新的事件类型

**复杂度**: 高
**协议升级**: 需要
**兼容性影响**: 高（需要更新所有相关组件）

### 3.4 方案 D: 新交易类型

**思路**: 创建专门用于原生事件的新交易类型 `NativeEventTransaction`。

```rust
pub enum TransactionKind {
    ProgrammableTransaction(ProgrammableTransaction),
    // ... 其他类型
    NativeEventTransaction(NativeEventTransaction),  // 新增
}

pub struct NativeEventTransaction {
    pub events: Vec<NativeEvent>,
    pub sender: SuiAddress,
    pub signature: /* ... */,
}
```

**复杂度**: 非常高
**协议升级**: 需要
**兼容性影响**: 非常高（共识层需要处理新交易类型）

---

## 4. 方案对比与推荐

### 4.1 对比表

| 维度 | 方案 A | 方案 B | 方案 C | 方案 D |
|------|--------|--------|--------|--------|
| 实施复杂度 | 中 | 中高 | 高 | 非常高 |
| 协议升级范围 | 执行层 | 执行层+系统包 | 全栈 | 共识+全栈 |
| 索引器改动 | 需要 | 最小 | 需要 | 需要 |
| 向后兼容 | 中 | 高 | 低 | 很低 |
| 实施周期 | 2-3 月 | 3-4 月 | 4-6 月 | 6+ 月 |
| Mysten 接受度 | 低 | 中 | 低 | 很低 |

### 4.2 推荐路径

如果**必须**修改 Sui 来支持原生事件，推荐 **方案 B（合成事件包）**：

1. **与现有系统兼容**: 复用 Event 结构，索引器改动最小
2. **类型安全**: 通过 Move 类型系统区分事件
3. **可扩展**: 可以定义多种原生事件类型
4. **审计友好**: 所有事件仍然有明确的类型标识

但是，任何修改 Sui 的方案都面临：
- **协议升级**: 需要 Mysten Labs 和验证器社区同意
- **测试成本**: 全面的安全审计和测试
- **维护负担**: 长期维护分叉代码

---

## 5. 替代方案：为何自建索引更实际

### 5.1 自建索引架构

对于原生 Rust DEX 引擎，**自建事件索引系统**是更实际的选择：

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        链下 DEX 引擎 (Rust)                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐            │
│  │  撮合引擎    │ ──► │  gRPC/事件流 │ ──► │  自建索引器  │            │
│  │  (Matching)  │     │  (tonic)     │     │  (Rust)      │            │
│  └──────────────┘     └──────┬───────┘     └──────┬───────┘            │
│                              │                    │                     │
│                              ▼                    ▼                     │
│                       ┌──────────────┐     ┌──────────────┐            │
│                       │  WebSocket   │     │  PostgreSQL  │            │
│                       │  (实时推送)  │     │  (持久化)    │            │
│                       └──────────────┘     └──────────────┘            │
└─────────────────────────────────────────────────────────────────────────┘
                              │
                              │ 结算交易
                              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                          Sui 区块链                                      │
│  ┌──────────────┐                        ┌──────────────┐               │
│  │  结算合约    │ ───── Move Event ────► │  sui-indexer │               │
│  │  (Settlement)│                        │  (链上数据)  │               │
│  └──────────────┘                        └──────────────┘               │
└─────────────────────────────────────────────────────────────────────────┘
```

### 5.2 优势分析

| 维度 | 自建索引 | 修改 Sui |
|------|----------|----------|
| 实施周期 | 1-2 周 | 3-6 月 |
| 依赖 Mysten | 无 | 完全依赖 |
| 性能控制 | 完全自主 | 受限于 Sui |
| 维护成本 | 低 | 高（分叉维护） |
| 灵活性 | 高 | 受限于协议 |

### 5.3 混合方案

**推荐架构**：链下事件自建索引 + 链上结算事件用 sui-indexer-alt

```rust
// 链下事件定义（gRPC proto）
message OrderPlacedEvent {
    uint64 order_id = 1;
    uint64 market_id = 2;
    uint64 price = 3;
    uint64 quantity = 4;
    Side side = 5;
    uint64 timestamp = 6;
}

// 链上结算事件（Move）
module dex::settlement {
    public struct SettlementExecuted has copy, drop {
        batch_id: u64,
        market_id: ID,
        total_volume: u64,
    }
}
```

---

## 6. 结论

### 6.1 技术可行性

**可以**修改 Sui 代码使原生 Rust 发出事件，但：
- 需要协议升级（所有验证器同步）
- 需要修改多个核心组件
- 需要 Mysten Labs 的支持和配合

### 6.2 实际建议

| 场景 | 推荐方案 |
|------|----------|
| 原生 Rust DEX 引擎 | **自建索引** + 链上结算事件桥接 |
| 需要完全链上可验证 | **Move 合约**（放弃原生 Rust） |
| 有资源长期维护 Sui 分叉 | **方案 B（合成事件包）** |

### 6.3 最终结论

对于 DEX 项目，**不建议**修改 Sui 代码来支持原生 Rust 事件。原因：

1. **投入产出比低**: 修改 Sui 需要 3-6 个月，自建索引只需 1-2 周
2. **依赖风险高**: 需要 Mysten Labs 配合，且需要持续跟进 Sui 版本
3. **设计耦合**: Event 结构与 Move 深度耦合，强行分离会引入复杂性
4. **替代方案成熟**: gRPC 事件流 + 自建索引是业界成熟方案（dYdX、Hyperliquid）

**推荐路径**: 参考 `sui-indexer-alt-analyst.md` 第 12.5 节的混合架构方案。
