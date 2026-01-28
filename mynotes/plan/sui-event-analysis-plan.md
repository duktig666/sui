# Sui 原生 Rust 代码发出 Event 可行性分析计划

## 目标
分析是否可以修改 Sui 代码，使原生 Rust 代码能够发出可被索引的 Event。
输出文档: `sui/mynotes/dex/analyst/sui-event-rust-analysis.md`

## 研究发现

### 1. 当前事件流程

```
Move 代码                    Native 层                      Sui 类型层
─────────                    ──────────                     ──────────
sui::event::emit<T>()  ──►  emit() native fn  ──►  ObjectRuntime.emit_event()
                                  │                         │
                                  ▼                         ▼
                            NativeContext              state.events: Vec<(StructTag, Value)>
                            ObjectRuntime                   │
                                                           ▼
                                                    take_user_events()
                                                           │
                                                           ▼
                                                    Event { package_id, module, sender, type_, contents }
                                                           │
                                                           ▼
                                                    TransactionEvents → checkpoint
```

### 2. 关键代码位置

| 组件 | 文件 | 行号 | 作用 |
|------|------|------|------|
| Native emit | `sui-execution/latest/sui-move-natives/src/event.rs` | 43-54 | Move emit 的 native 实现 |
| ObjectRuntime | `sui-execution/latest/sui-move-natives/src/object_runtime/mod.rs` | 359-365 | 事件存储 |
| Event 提取 | `sui-execution/latest/sui-adapter/src/programmable_transactions/context.rs` | 318-366 | 转换为 Event struct |
| Event 创建 | 同上 | 1522-1533 | 最终 Event 构造 |
| Event 结构 | `crates/sui-types/src/event.rs` | 106-113 | Event 数据结构 |
| 存储结构 | `crates/sui-types/src/inner_temporary_store.rs` | 27-39 | InnerTemporaryStore |

### 3. Event 结构约束

```rust
pub struct Event {
    pub package_id: ObjectID,         // 必须是有效的 Move 包 ID
    pub transaction_module: Identifier, // 必须是有效的 Move 模块名
    pub sender: SuiAddress,
    pub type_: StructTag,             // 必须是有效的 Move 类型
    pub contents: Vec<u8>,            // BCS 序列化数据
}
```

**核心问题**: Event 的设计假设事件来自 Move 合约，每个字段都与 Move 类型系统紧密耦合。

### 4. 修改方案对比

| 方案 | 复杂度 | 协议升级 | 兼容性 | 可行性 |
|------|--------|----------|--------|--------|
| A. 扩展 ObjectRuntime API | 中 | 需要 | 中 | ⭐⭐⭐ |
| B. 合成事件包 | 中高 | 需要 | 高 | ⭐⭐⭐ |
| C. 事件扩展系统 | 高 | 需要 | 中 | ⭐⭐ |
| D. 新交易类型 | 非常高 | 需要 | 低 | ⭐ |

## 文档输出计划

### 文档结构
```
1. 执行摘要
   - 结论：技术上可行，但需要协议升级
   - 推荐方案

2. 当前事件系统分析
   - 事件流程图
   - 关键代码引用
   - 约束条件

3. 修改方案详解
   3.1 方案 A: 扩展 ObjectRuntime API
   3.2 方案 B: 合成事件包
   3.3 方案 C: 事件扩展系统
   3.4 方案 D: 新交易类型

4. 方案对比与推荐
   - 实施难度
   - 兼容性影响
   - 推荐路径

5. 替代方案
   - 为何自建索引更实际
   - 与链上事件桥接的混合方案

6. 结论
```

### 关键修改点（方案 A 详解）

**需要修改的文件**:
1. `sui-execution/latest/sui-move-natives/src/object_runtime/mod.rs`
   - 添加 `emit_native_event()` 方法

2. `sui-execution/latest/sui-adapter/src/programmable_transactions/context.rs`
   - 添加原生事件处理逻辑

3. `sui-types/src/event.rs`
   - 可能需要添加事件类型标识

4. `sui-core/` - 交易处理逻辑
   - 验证器需要接受新的事件类型

## 验证方式
- 分析完成后输出完整的中文分析文档
- 所有代码引用包含文件路径和行号
- 包含具体的代码修改示例（伪代码）
