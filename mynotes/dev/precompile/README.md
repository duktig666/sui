# DEX Demo Precompile

这是一个最小化的 Precompile 示例,用于验证"交易路由到自定义 Rust 引擎执行"的核心机制。

## 目标

验证以下能力:
1. ✅ 检测特定交易 (通过 DEMO_PACKAGE_ID `0xDEDE...`)
2. ✅ 将交易路由到 Rust 原生函数,完全绕过 Move VM
3. ✅ 返回合法的 `TransactionEffects`
4. ✅ 集成测试验证
5. ✅ 可运行的示例代码

## 核心设计

### 拦截点

在 `sui-execution/latest/sui-adapter/src/execution_engine.rs` 的 `execute_transaction_to_effects()` 函数开头添加拦截逻辑:

```rust
#[cfg(feature = "dex-demo")]
{
    if dex_demo::is_demo_transaction(&transaction_kind) {
        info!("Detected demo transaction, routing to DemoEngine");
        let effects = dex_demo::execute_demo_transaction(...);
        return (empty_store, gas_status, effects, vec![], Ok(...));
    }
}
```

### 识别标识

Demo 交易通过以下方式识别:
- **Package ID**: `0xDEDE...` (DEMO_PACKAGE_ID)
- **固定 Gas**: `computation_cost = 1000` (用于验证)
- **事件摘要**: `0xDEDE...` 模式
- **日志**: "Detected demo transaction, routing to DemoEngine"

### 交易验证绕过

由于 `DEMO_PACKAGE_ID` (0xDEDE...) 在链上不存在，需要在交易输入加载阶段绕过 package 验证。这通过以下机制实现：

1. **`is_demo_package()`**: 检查 package ID 是否为 DEMO_PACKAGE_ID
2. **`create_demo_package_object()`**: 创建一个虚拟的 package 对象用于验证
3. **`transaction_input_loader.rs`**: 在加载输入对象时，对 DEMO_PACKAGE_ID 返回虚拟 package

```rust
// 在 sui-core/src/transaction_input_loader.rs 中
#[cfg(feature = "dex-demo")]
if dex_demo::is_demo_package(id) {
    let demo_package = dex_demo::create_demo_package_object();
    // 使用虚拟 package 绕过链上查找
}
```

## 编译和安装

### 方式 1: 不启用 Demo (默认)

```bash
cargo check
cargo build
```

Demo 功能完全不包含在编译产物中,零运行时开销。

### 方式 2: 启用 Demo Feature

```bash
cargo check --features dex-demo
cargo build --features dex-demo
```

## 运行测试

### 单元测试

```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-demo
```

**测试覆盖**:
- ✅ `test_is_demo_transaction_positive` - 正确识别 Demo 交易
- ✅ `test_is_demo_transaction_negative_wrong_package` - 拒绝错误的 package
- ✅ `test_is_demo_transaction_negative_non_ptb` - 拒绝非 PTB 交易
- ✅ `test_execute_demo_transaction` - 执行返回合法 Effects
- ✅ `test_demo_package_id_constant` - 验证常量正确性
- ✅ `test_demo_computation_cost_constant` - 验证 Gas 值合理性
- ✅ `test_is_demo_package` - 验证 package ID 检测
- ✅ `test_create_demo_package_object` - 验证虚拟 package 创建

**当前状态**: 所有 8 个单元测试通过 ✅

### 集成测试 (E2E Tests)

```bash
# 运行 Demo E2E 测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-e2e-tests --features dex-demo test_demo

# 运行包含 simtests 的测试
cargo nextest run -p sui-e2e-tests --features dex-demo test_demo
```

**测试覆盖**:
- ✅ `test_demo_transaction_detection` - 验证 Demo 交易检测
- ✅ `test_normal_transaction_not_affected` - 验证普通交易不受影响 (simtest)
- ✅ `test_demo_vs_normal_comparison` - Demo vs 标准交易对比 (simtest)
- ✅ `test_demo_constants` - 常量验证

**当前状态**: 所有 3 个测试通过 ✅ (1 个跳过 simtest)

## 运行示例

### 前提条件

在运行示例之前,需要先启动本地验证者节点和水龙头:

```bash
# 1. 编译 sui 二进制 (启用 dex-demo feature)
cargo build --bin sui --features dex-demo

# 2. 初始化本地网络配置
export SUI_CHAIN_DIR="$PWD/local-network-config"
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000

# 3. 启动本地验证者节点 (带水龙头)
export SUI_CHAIN_DIR="$PWD/local-network-config"
RUST_LOG="off,sui_node=info,dex_demo=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9000
    
rm -rf ~/.sui/sui_config/network.yaml
rm -rf ~/.sui/sui_config/authorities_db/
rm -rf ~/.sui/sui_config/consensus_db/
    
RUST_LOG="off,sui_node=info,dex_demo=debug" \
  ./target/debug/sui start --with-faucet --force-regenesis --fullnode-rpc-port 9000
```

```
rm -rf ~/.sui/sui_config/network.yaml
rm -rf ~/.sui/sui_config/authorities_db/
rm -rf ~/.sui/sui_config/consensus_db/

RUST_LOG="off,sui_node=info,sui_adapter=info,dex_demo=debug" \
  ./target/debug/sui start --with-faucet --force-regenesis --fullnode-rpc-port 9000
```

**验证节点是否正常运行**:
- RPC 端点: http://127.0.0.1:9000
- 水龙头端点: http://127.0.0.1:9123
- 日志中应包含: "Sui Node started"

### 示例 1: Demo 交易 (路由到 Precompile)

连接本地节点,发送调用 `DEMO_PACKAGE_ID` 的交易,验证 Precompile 路由:

```bash
cargo run --example send_demo_tx -p dex-demo
```

**预期输出**:
```
=== Demo Transaction Example ===

Step 1: Connecting to local Sui node...
✓ Connected to local node

Step 2: Setting up sender address...
Sender address: 0x...

Step 3: Requesting gas from faucet...
✓ Faucet request successful
✓ Gas received

Step 4: Fetching gas coin...
Gas coin: 0x...
Gas balance: 5000000000 MIST

Step 5: Building demo transaction...
✓ Transaction built

Gas budget: 10000000
Gas price: 1000

Step 6: Creating transaction data...
✓ Transaction data created

Step 7: Signing transaction...
✓ Transaction signed

Step 8: Executing transaction...
✓ Transaction executed!

=== Transaction Results ===
Transaction digest: ...
Status: ✓ Success

Gas Summary:
  Computation cost: 1000
  Storage cost: 0
  Storage rebate: 0

=== Demo Transaction Verification ===
✓ This is a DEMO transaction (computation_cost = 1000)
✓ Transaction was routed to Precompile engine

=== Summary ===
This transaction called package 0xdedede...::counter::increment
It should have been detected by is_demo_transaction()
and routed to execute_demo_transaction() (bypassing Move VM)
```

**关键验证点**:
1. ✅ `computation_cost = 1000` (固定值)
2. ✅ 节点日志包含: "Detected demo transaction, routing to DemoEngine"
3. ✅ 交易成功执行

### 示例 2: 普通交易 (标准 Move VM 执行)

连接本地节点,发送标准 SUI 转账交易,验证不受 Precompile 影响:

```bash
cargo run --example send_normal_tx -p dex-demo
```

**预期输出**:
```
=== Normal Transaction Example (for comparison) ===

Step 1: Connecting to local Sui node...
✓ Connected to local node

Step 2: Setting up sender address...
Sender address: 0x...

Step 3: Requesting gas from faucet...
✓ Faucet request successful
✓ Gas received

Step 4: Fetching gas coin...
Gas coin: 0x...
Gas balance: 5000000000 MIST

Step 5: Building normal transfer transaction...
✓ Transaction built (SUI transfer)

Gas budget: 10000000
Gas price: 1000

Step 6: Creating transaction data...
✓ Transaction data created

Step 7: Signing transaction...
✓ Transaction signed

Step 8: Executing transaction...
✓ Transaction executed!

=== Transaction Results ===
Transaction digest: ...
Status: ✓ Success

Gas Summary:
  Computation cost: 750
  Storage cost: 0
  Storage rebate: 0

=== Normal Transaction Verification ===
✓ This is a NORMAL transaction (computation_cost = 750)
✓ Transaction went through standard Move VM execution

=== Comparison: Demo vs Normal ===
Demo Transaction:
  - Package ID: DEMO_PACKAGE_ID (0xDEDE...)
  - Detection: is_demo_transaction() = true
  - Execution: Rust precompile (bypasses Move VM)
  - Gas: Fixed computation_cost = 1000

Normal Transaction:
  - Package ID: Any other package (e.g., 0x2 for SUI transfer)
  - Detection: is_demo_transaction() = false
  - Execution: Standard Move VM
  - Gas: Calculated based on actual execution (dynamic)
```

**关键验证点**:
1. ✅ `computation_cost != 1000` (动态计算,通常 700-1500)
2. ✅ 节点日志**不包含**: "Routing to DemoEngine"
3. ✅ 交易成功执行

## 验证方法

### Demo 交易特征

| 特征 | Demo 交易 | 标准交易 |
|-----|----------|---------|
| 日志 | "Routing to DemoEngine" | 无此日志 |
| Gas | `computation_cost = 1000` | 动态计算 |
| 事件摘要 | `0xDEDE...` 模式 | 正常摘要 |
| Move VM | **绕过** | 正常执行 |

### 代码质量检查

```bash
# 格式化
cargo fmt --all

# Clippy 检查 (带 feature)
cargo clippy -p dex-demo
cargo clippy -p sui-adapter-latest --features dex-demo

# Clippy 检查 (不带 feature,确保不破坏现有功能)
cargo clippy -p sui-adapter-latest
```

## 项目结构

```
crates/dex-demo/
├── src/
│   └── lib.rs                  # 核心逻辑 (~300 行)
│       ├── DEMO_PACKAGE_ID     # 常量: 0xDEDE...
│       ├── is_demo_transaction()  # 检测函数
│       ├── execute_demo_transaction()  # 执行函数
│       └── tests               # 6 个单元测试
├── examples/
│   ├── send_demo_tx.rs         # Demo 交易示例
│   └── send_normal_tx.rs       # 普通交易示例
├── Cargo.toml                  # 依赖配置
└── README.md                   # 本文档

sui-execution/latest/sui-adapter/
├── src/
│   └── execution_engine.rs     # 修改: +25 行拦截逻辑
└── Cargo.toml                  # 修改: +3 行 feature 配置

crates/sui-e2e-tests/
├── tests/
│   └── demo_precompile_tests.rs  # E2E 测试 (3 个测试)
└── Cargo.toml                  # 修改: +6 行 dex-demo 依赖
```

## 实施进度

### ✅ Phase 1: 基础设施搭建

- [x] 1.1 创建 `crates/dex-demo/` crate
  - [x] `src/lib.rs` 核心逻辑
  - [x] `Cargo.toml` 依赖配置
  - [x] 单元测试 (6个)
- [x] 1.2 修改 ExecutionEngine
  - [x] `sui-adapter/Cargo.toml` 添加 feature
  - [x] `execution_engine.rs` 添加拦截逻辑 (~25 行)
- [x] 1.3 编译验证
  - [x] `cargo check` (不带 feature) ✅
  - [x] `cargo check --features dex-demo` ✅
  - [x] 所有单元测试通过 ✅
  - [x] 无 clippy warnings ✅

### ✅ Phase 2: 集成测试

- [x] 创建 `crates/sui-e2e-tests/tests/demo_precompile_tests.rs`
- [x] 添加 `sui-e2e-tests` 的 feature 依赖
- [x] 实现测试用例:
  - [x] `test_demo_transaction_detection` - 验证检测逻辑
  - [x] `test_normal_transaction_not_affected` - 验证普通交易不受影响
  - [x] `test_demo_vs_normal_comparison` - Demo vs 标准交易对比
  - [x] `test_demo_constants` - 常量验证
- [x] 运行测试: `SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-e2e-tests --features dex-demo test_demo`
- [x] 所有测试通过 ✅

### ✅ Phase 3: 示例代码

- [x] 编写示例:
  - [x] `examples/send_demo_tx.rs` - Demo 交易示例
  - [x] `examples/send_normal_tx.rs` - 普通交易示例
- [x] 验证示例可运行:
  - [x] `cargo run --example send_demo_tx -p dex-demo` ✅
  - [x] `cargo run --example send_normal_tx -p dex-demo` ✅
- [x] 示例输出清晰,便于理解和验证

### ✅ Phase 4: 文档和清理

- [x] README.md (本文档)
- [x] 完善使用指南 - 添加示例运行说明
- [x] 添加 FAQ - 扩展常见问题
- [x] 运行 `cargo fmt` - 代码格式化

## 技术细节

### Feature Flag 隔离

Demo 功能通过 `#[cfg(feature = "dex-demo")]` 完全隔离:
- 默认构建不包含任何 Demo 代码
- 零运行时开销
- 编译时条件编译

### TransactionEffects 构造

Demo 返回的 `TransactionEffects::V2`:
- `status`: `ExecutionStatus::Success`
- `gas_used.computation_cost`: `1000` (固定标识符)
- `events_digest`: `Some(0xDEDE...)`
- `changed_objects`: `[]` (不修改状态)
- `dependencies`: `[]`

### InnerTemporaryStore 构造

Demo 返回空的临时存储:
```rust
InnerTemporaryStore {
    input_objects: Default::default(),
    written: Default::default(),
    events: TransactionEvents { data: vec![] },
    // ... 其他字段为空
}
```

## 常见问题 (FAQ)

### Q: 如何确认 Demo feature 已启用?

查看编译日志,应该包含:
```
Compiling dex-demo v0.1.0
```

### Q: Demo 交易会影响正常交易吗?

不会。通过 feature flag 隔离,默认构建完全不包含 Demo 代码。

### Q: 如何在生产环境禁用 Demo?

不要在编译时添加 `--features dex-demo` 即可。

### Q: 为什么使用固定 gas 值 1000?

这是一个可识别的标识符,便于在日志和测试中快速验证交易走了 Precompile 路径。

### Q: Demo 会修改链上状态吗?

不会。Demo 返回空的 `changed_objects`,不修改任何对象。

### Q: 如何运行示例代码?

运行 Demo 交易示例:
```bash
cargo run --example send_demo_tx -p dex-demo
```

运行普通交易示例:
```bash
cargo run --example send_normal_tx -p dex-demo
```

### Q: 集成测试如何运行?

运行所有 Demo E2E 测试:
```bash
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-e2e-tests --features dex-demo test_demo
```

### Q: 如何在本地节点上测试 Demo?

1. 编译 sui 二进制 (启用 feature):
   ```bash
   cargo build --features dex-demo -p sui
   ```

2. 启动本地测试验证器:
   ```bash
   sui start --with-faucet
   ```

3. 构造并提交 Demo 交易 (调用 `0xDEDE...::counter::increment`)

4. 查看日志验证:
   - 日志中应包含: "Detected demo transaction, routing to DemoEngine"
   - Transaction effects 中 `computation_cost = 1000`

## 后续扩展

完成本 Demo 后,可以进行以下扩展:

1. **性能测试**: 对比 Precompile vs Move VM 的延迟
2. **多命令支持**: 支持 PTB 中混合 Demo 和标准命令
3. **状态修改**: 实现真正的计数器读写
4. **完整 DEX**: 扩展为完整的撮合引擎

## 参考资料

- 设计文档: `mynotes/plan/sui_precompile_demo_plan.md`
- DEX L1 设计: `notes/dex_l1/DEX_L1_DESIGN_SUMMARY.md`
- Sui 架构: `notes/SUI_ARCHITECTURE_REPORT.md`

---

**版本**: v2.0
**状态**: Phase 1-4 全部完成 ✅✅✅✅
**最后更新**: 2026-01-08

## 成果总结

已完成的工作:
- ✅ **Phase 1**: 基础设施搭建 - 核心逻辑实现和单元测试 (6/6 通过)
- ✅ **Phase 2**: 集成测试 - E2E 测试实现 (3/3 通过)
- ✅ **Phase 3**: 示例代码 - 2 个可运行示例 (Demo + Normal)
- ✅ **Phase 4**: 文档和清理 - 完整 README 和代码规范

核心验证:
- ✅ 交易检测逻辑正确
- ✅ Demo 交易返回固定 gas (1000)
- ✅ 普通交易不受影响
- ✅ 所有测试通过
- ✅ 示例可运行
- ✅ 代码符合规范

下一步:
1. 在真实本地节点上测试 (需要 Move 合约配合)
2. 性能基准测试 (Precompile vs Move VM 延迟对比)
3. 扩展为完整的 DEX 撮合引擎

## 本地节点测试

**重要**: 每次修改 dex-demo 相关代码后，必须重新编译并重启节点！

### Feature 传播链

`dex-demo` feature 需要通过以下路径传播：

```
sui (binary)
  └── test-cluster/dex-demo
        └── sui-core/dex-demo
        └── sui-node/dex-demo
              └── sui-core/dex-demo
        └── sui-swarm/dex-demo
              └── sui-node/dex-demo
  └── sui-execution/dex-demo
        └── sui-adapter-latest/dex-demo
```

### 编译和启动步骤

```bash
# 1. 编译 sui 二进制（启用 dex-demo feature）
# 注意：必须使用 --features dex-demo 才能启用 Precompile 功能
cargo build --features dex-demo --bin sui

# 2. 初始化本地网络配置（首次运行或需要重置时）
export SUI_CHAIN_DIR="$PWD/local-network-config"
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000

# 3. 启动本地验证者节点（带 dex-demo 日志）
export SUI_CHAIN_DIR="$PWD/local-network-config"
RUST_LOG="off,sui_node=info,dex_demo=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9000
```

### 运行示例

```bash
# 在另一个终端窗口运行示例
# Demo 交易（应该看到 computation_cost = 1000）
cargo run --example send_demo_tx -p dex-demo

# 普通交易（应该看到 computation_cost != 1000）
cargo run --example send_normal_tx -p dex-demo
```

### 验证 Precompile 是否生效

在节点日志中应该看到以下输出：

```
🔧 Demo Precompile: creating virtual package for DEMO_PACKAGE_ID (bypassing chain lookup)
🚀 [DEX Demo Precompile] Detected demo transaction, routing to DemoEngine (bypassing Move VM)
✅ [DEX Demo Precompile] Transaction executed successfully with fixed computation_cost = 1000
```

### 常见问题

**Q: 交易超时或 faucet 卡住？**

A: 确保节点使用 `--features dex-demo` 重新编译并重启：
```bash
# 停止当前节点 (Ctrl+C)
# 重新编译
cargo build --features dex-demo --bin sui
# 重启节点
```

**Q: 如何确认 feature 已启用？**

A: 查看编译日志，应该包含 `dex-demo` 相关的 crate 编译信息。
