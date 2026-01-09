# Precompile 测试指南

本指南说明如何在本地节点上测试 Demo Precompile 和普通交易。

## 前置条件

1. **编译 Sui 二进制 (启用 dex-demo feature)**:
   ```bash
   cargo build --features dex-demo -p sui
   ```

2. **确保所有依赖已编译**:
   ```bash
   cargo build --features dex-demo
   ```

## 测试步骤

### 步骤 1: 启动本地节点 (终端 1)

```bash
# 生成配置 (只需运行一次)
export SUI_CHAIN_DIR="$PWD/local-network-config"
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 60000

# 启动节点
RUST_LOG="off,sui_node=info,sui_core=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9000
```

**关键日志**:
- 看到 `Fullnode RPC URL: http://127.0.0.1:9000` 表示节点已启动
- 看到 `Started faucet server` 表示 Faucet 已启动

### 步骤 2: 运行测试脚本 (终端 2)

在**另一个终端**运行测试脚本:

```bash
cd /Users/renshiwei/code/company/DEX/sui
cargo run --example test_precompile_on_node --features dex-demo
```

## 测试脚本功能

测试脚本会自动执行以下操作:

### 1. 连接节点
- 连接到 `http://127.0.0.1:9000`
- 验证连接成功

### 2. 获取 Gas
- 从本地 Faucet (`http://127.0.0.1:9123/gas`) 请求 gas
- 等待 gas 到账
- 验证 gas 对象可用

### 3. 构建并提交 Demo 交易 (Precompile)
- 构建调用 `DEMO_PACKAGE_ID::counter::increment` 的交易
- 验证交易被正确识别为 Demo 交易
- 提交交易到节点
- **验证结果**:
  - `computation_cost = 1000` (固定值, Demo 标识)
  - 交易执行成功
  - 节点日志包含: `"Detected demo transaction, routing to DemoEngine"`

### 4. 构建并提交普通交易 (标准 Move VM)
- 构建普通的 SUI 转账交易
- 验证交易**不是** Demo 交易
- 提交交易到节点
- **验证结果**:
  - `computation_cost != 1000` (动态计算)
  - 交易执行成功
  - 无特殊日志标识

### 5. 对比总结
- 显示 Demo 交易和普通交易的对比
- 验证 Precompile 机制正常工作

## 验证要点

### Demo 交易验证

✅ **成功标志**:
- `computation_cost = 1000` (固定值)
- 交易执行成功
- 节点日志包含: `"Detected demo transaction, routing to DemoEngine (bypassing Move VM)"`

❌ **失败标志**:
- `computation_cost != 1000`
- 交易执行失败
- 无特殊日志

### 普通交易验证

✅ **成功标志**:
- `computation_cost != 1000` (动态值)
- 交易执行成功
- 无特殊日志

❌ **失败标志**:
- `computation_cost = 1000` (不应该发生)
- 交易执行失败

## 查看节点日志

在**终端 1** (运行节点的终端) 查看日志:

```bash
# 应该看到类似日志:
# INFO sui_adapter_latest::execution_engine: Detected demo transaction, routing to DemoEngine (bypassing Move VM)
# INFO sui_adapter_latest::execution_engine: tx_digest=TransactionDigest(...)
```

## 常见问题

### Q: Faucet 请求失败

**原因**: Faucet 服务未启动或端口不对

**解决**:
1. 确保启动命令包含 `--with-faucet=0.0.0.0:9123`
2. 检查端口 9123 是否被占用: `lsof -i :9123`
3. 手动从 faucet 获取 gas:
   ```bash
   curl -X POST http://127.0.0.1:9123/gas \
     -H "Content-Type: application/json" \
     -d '{"FixedAmountRequest": {"recipient": "YOUR_ADDRESS"}}'
   ```

### Q: 没有 gas 对象

**原因**: Faucet 请求成功但 gas 还未到账

**解决**:
1. 等待更长时间 (脚本默认等待 3 秒)
2. 手动检查 gas:
   ```bash
   sui client gas --address YOUR_ADDRESS
   ```

### Q: Demo 交易 computation_cost 不是 1000

**原因**: 
1. 节点未使用 `--features dex-demo` 编译
2. Feature 传递配置不正确

**解决**:
1. 重新编译: `cargo build --features dex-demo -p sui`
2. 检查 `sui-execution/Cargo.toml` 是否包含 `dex-demo = ["sui-adapter-latest/dex-demo"]`
3. 重启节点

### Q: 交易执行失败

**原因**: 
1. Gas 不足
2. 交易格式错误
3. 节点未完全启动

**解决**:
1. 检查 gas 余额
2. 查看节点日志中的错误信息
3. 确保节点完全启动后再提交交易

## 手动测试 (不使用脚本)

如果不想使用测试脚本,可以手动构建和提交交易:

### 1. 构建 Demo 交易

```rust
use dex_demo::DEMO_PACKAGE_ID;
use sui_types::programmable_transaction_builder::ProgrammableTransactionBuilder;

let mut ptb = ProgrammableTransactionBuilder::new();
ptb.programmable_move_call(
    DEMO_PACKAGE_ID,
    "counter".parse().unwrap(),
    "increment".parse().unwrap(),
    vec![],
    vec![],
);
```

### 2. 提交交易

```rust
let response = client
    .quorum_driver_api()
    .execute_transaction_block(
        signed_tx,
        SuiTransactionBlockResponseOptions::full_content(),
        Some(ExecuteTransactionRequestType::WaitForLocalExecution),
    )
    .await?;
```

### 3. 验证结果

```rust
if let Some(effects) = response.effects {
    let gas_summary = effects.gas_cost_summary();
    assert_eq!(gas_summary.computation_cost, 1000); // Demo 标识
}
```

## 参考

- 测试脚本: `examples/test_precompile_on_node.rs`
- 核心逻辑: `src/lib.rs`
- README: `README.md`

