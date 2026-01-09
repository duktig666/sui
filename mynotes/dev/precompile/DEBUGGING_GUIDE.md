# Precompile 调试指南

> **目的**: 提供完整的日志配置和调试方法  
> **日期**: 2025-01-08

---

## 📋 日志级别说明

### 当前配置分析

```bash
RUST_LOG="off,sui_node=info,dex_demo=debug"
```

**问题**：
- ❌ `sui_adapter` 的执行流程日志看不到（需要 `debug` 级别）
- ❌ `sui_core` 的交易处理日志看不到（需要 `debug` 级别）
- ❌ Gas 处理的详细日志看不到（需要 `debug` 级别）
- ⚠️ Demo 交易的详细日志已改为 `debug!`，但仍需要 `sui_adapter=debug`

---

## 🔧 推荐的日志配置

### 配置 1: 基础调试（推荐用于开发）

```bash
RUST_LOG="off,sui_node=info,sui_adapter=debug,sui_core=debug,dex_demo=debug"
```

**包含的日志**：
- ✅ Demo 交易检测（`info!`）
- ✅ Demo 交易的详细日志（`debug!` - 已优化代码）
- ✅ 执行引擎的关键步骤（`#[instrument(level = "debug")]`）
- ✅ Gas 处理流程
- ✅ 交易执行流程

### 配置 2: 详细调试（用于深度排查）

```bash
RUST_LOG="off,sui_node=info,sui_adapter=trace,sui_core=debug,dex_demo=trace,gas_charger=debug"
```

**包含的日志**：
- ✅ 所有执行步骤的详细日志
- ✅ Gas 处理的每一步
- ✅ 对象状态变更
- ✅ 可能很冗长，但信息最全

### 配置 3: 最小调试（仅看关键信息）

```bash
RUST_LOG="off,sui_node=info,sui_adapter=info,dex_demo=info"
```

**包含的日志**：
- ✅ 仅关键信息
- ✅ Demo 交易检测
- ❌ 不包含详细执行流程

---

## 📝 关键日志位置

### 1. Demo 交易检测

**位置**: `execution_engine.rs:182`
```rust
info!(
    tx_digest = ?transaction_digest,
    "Detected demo transaction, routing to DemoEngine (bypassing Move VM)"
);
```

**需要的级别**: `info` ✅ (当前配置已包含)

### 2. Demo Gas 成本设置

**位置**: `execution_engine.rs:201`
```rust
debug!(
    tx_digest = ?transaction_digest,
    demo_computation_cost = gas_cost_summary.computation_cost,
    storage_cost = gas_cost_summary.storage_cost,
    storage_rebate = gas_cost_summary.storage_rebate,
    "Demo transaction: using fixed computation cost = {}",
    gas_cost_summary.computation_cost
);
```

**需要的级别**: `debug` ✅ (需要 `sui_adapter=debug`)

### 3. Gas 处理流程

**位置**: `gas_charger.rs`
- `smash_gas()` 的日志
- `charge_gas()` 的日志
- Gas 对象更新日志

**需要的级别**: `debug` 或 `trace`

### 4. Effects 创建

**位置**: `temporary_store.rs:into_effects()`
- 对象变更日志
- Effects 构建日志

**需要的级别**: `debug` 或 `trace`

---

## 🎯 针对 Precompile 调试的最佳配置

### 推荐配置（平衡信息量和可读性）

```bash
RUST_LOG="off,\
sui_node=info,\
sui_adapter=debug,\
sui_adapter::execution_engine=debug,\
sui_adapter::gas_charger=debug,\
sui_core=debug,\
dex_demo=debug"
```

### 如果还是看不到足够信息，使用：

```bash
RUST_LOG="off,\
sui_node=info,\
sui_adapter=trace,\
sui_core=debug,\
dex_demo=trace"
```

---

## 🔍 关键日志检查点

### 1. 交易接收
```
[INFO] sui_node: Received transaction
```

### 2. Demo 交易检测
```
[INFO] sui_adapter::execution_engine: Detected demo transaction, routing to DemoEngine
```

### 3. Gas 处理
```
[DEBUG] sui_adapter::gas_charger: Smashing gas coins
[DEBUG] sui_adapter::gas_charger: Charging gas
[TRACE] sui_adapter::execution_engine: Demo transaction: using fixed computation cost
```

### 4. Effects 创建
```
[DEBUG] sui_adapter::temporary_store: Creating effects
```

### 5. 交易完成
```
[INFO] sui_node: Transaction executed successfully
```

---

## 🛠️ 修改代码以提升日志可见性

如果 `trace!` 日志看不到，可以临时改为 `debug!`：

```rust
// 在 execution_engine.rs:201
#[skip_checked_arithmetic]
debug!(  // 改为 debug! 而不是 trace!
    tx_digest = ?transaction_digest,
    demo_computation_cost = gas_cost_summary.computation_cost,
    "Demo transaction: using fixed computation cost"
);
```

---

## 📊 日志级别对比

| 级别 | 当前配置 | 推荐配置 | 说明 |
|------|---------|---------|------|
| `error` | ✅ | ✅ | 错误信息 |
| `warn` | ✅ | ✅ | 警告信息 |
| `info` | ✅ | ✅ | 关键信息（Demo 检测） |
| `debug` | ⚠️ 部分 | ✅ | 详细执行流程 |
| `trace` | ❌ | ⚠️ 可选 | 最详细的日志 |

---

## 🚀 快速测试命令

```bash
# 使用推荐配置启动节点
RUST_LOG="off,sui_node=info,sui_adapter=debug,sui_core=debug,dex_demo=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9000

# 在另一个终端运行测试
cargo run --example send_demo_tx -p dex-demo
```

---

## 🔎 调试技巧

1. **过滤日志**：
   ```bash
   # 只看 Demo 相关日志
   RUST_LOG="..." ./target/debug/sui start ... | grep -i demo
   
   # 只看执行引擎日志
   RUST_LOG="..." ./target/debug/sui start ... | grep execution_engine
   ```

2. **保存日志到文件**：
   ```bash
   RUST_LOG="..." ./target/debug/sui start ... 2>&1 | tee sui.log
   ```

3. **实时查看关键日志**：
   ```bash
   # 使用 tail -f 实时查看
   tail -f sui.log | grep -E "(demo|Demo|DEMO|execution_engine)"
   ```

---

## 📚 相关模块日志

如果需要调试特定模块，可以添加：

- `sui_adapter::execution_engine=trace` - 执行引擎详细日志
- `sui_adapter::gas_charger=trace` - Gas 处理详细日志
- `sui_adapter::temporary_store=debug` - 临时存储日志
- `sui_core::authority=debug` - Authority 处理日志
- `sui_core::transaction_manager=debug` - 交易管理日志

