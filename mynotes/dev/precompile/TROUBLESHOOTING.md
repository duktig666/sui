# DEX Demo Precompile 故障排除指南

## 问题: Request timeout 错误

### 症状

```
Step 8: Executing transaction...
Error: Request timeout
```

### 可能原因和解决方案

#### 1. 节点未启用 dex-demo feature

**检查方法**:
```bash
# 查看编译的二进制是否包含 dex-demo feature
./target/debug/sui --version
```

**解决方案**:
```bash
# 重新编译,确保包含 dex-demo feature
cargo build --bin sui --features dex-demo

# 验证编译产物
ls -lh ./target/debug/sui
```

#### 2. 节点日志级别不正确

**检查方法**:
查看节点启动命令是否包含正确的 RUST_LOG 设置

**解决方案**:
```bash
# 确保包含 dex_demo=debug
RUST_LOG="off,sui_node=info,dex_demo=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9000
```

#### 3. 交易验证失败 (虚拟对象引用问题)

**已修复**: 最新版本的 `send_demo_tx.rs` 已移除虚拟对象引用,改用无参数函数。

**如果使用旧版本**:
```bash
# 更新代码
git pull  # 或手动更新 send_demo_tx.rs
```

#### 4. 节点配置问题

**检查方法**:
```bash
# 检查网络配置是否存在
ls -la $SUI_CHAIN_DIR

# 检查节点配置文件
cat $SUI_CHAIN_DIR/network.yaml
```

**解决方案**:
```bash
# 重新初始化网络配置
rm -rf $SUI_CHAIN_DIR
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000
```

#### 5. RPC 端点连接问题

**检查方法**:
```bash
# 测试 RPC 端点是否可达
curl http://127.0.0.1:9000 -d '{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "sui_getLatestCheckpointSequenceNumber",
  "params": []
}'
```

**预期响应**:
```json
{"jsonrpc":"2.0","result":"...","id":1}
```

**如果连接失败**:
- 检查节点是否正在运行: `ps aux | grep sui`
- 检查端口是否被占用: `lsof -i :9000`
- 检查防火墙设置

#### 6. 交易超时 (节点共识问题)

**检查方法**:
```bash
# 查看节点日志中是否有共识相关错误
tail -f <节点日志文件> | grep -E "(consensus|epoch|checkpoint)"
```

**解决方案**:
```bash
# 增加 epoch 持续时间
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 60000  # 增加到 60 秒
```

## 调试步骤

### 步骤 1: 验证节点状态

```bash
# 1. 检查节点是否运行
ps aux | grep sui

# 2. 检查 RPC 端口
curl http://127.0.0.1:9000

# 3. 检查水龙头端口
curl http://127.0.0.1:9123/v2/status
```

### 步骤 2: 查看节点日志

启动节点时将日志输出到文件:
```bash
RUST_LOG="off,sui_node=info,dex_demo=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9000 \
  2>&1 | tee sui_node.log
```

然后查看日志:
```bash
tail -f sui_node.log
```

### 步骤 3: 测试标准交易

如果 Demo 交易超时,尝试标准交易:
```bash
cargo run --example send_normal_tx -p dex-demo
```

**如果标准交易也超时**:
- 问题在节点本身,不是 dex-demo feature
- 检查节点配置和共识设置

**如果标准交易成功,Demo 交易超时**:
- 问题可能在 dex-demo 实现
- 检查节点是否包含 dex-demo feature
- 查看节点日志中是否有 "Detected demo transaction"

### 步骤 4: 增加超时时间

修改示例代码,增加超时时间:

编辑 `send_demo_tx.rs`:
```rust
// 在 execute_transaction_block 调用前添加
use std::time::Duration;

// 创建自定义 client 配置
let sui_client = SuiClientBuilder::default()
    .request_timeout(Duration::from_secs(60))  // 增加到 60 秒
    .build("http://127.0.0.1:9000")
    .await?;
```

### 步骤 5: 简化交易

如果问题持续,尝试最简单的 Demo 交易:

修改 `send_demo_tx.rs`:
```rust
// 使用最简单的 PTB (只包含一个 move call,无参数)
let mut ptb = ProgrammableTransactionBuilder::new();
ptb.move_call(
    dex_demo::DEMO_PACKAGE_ID,
    Identifier::new("counter")?,
    Identifier::new("get_value")?,
    vec![],  // no type args
    vec![],  // no args
)?;
```

## 常见错误消息

### "Connection refused"

**原因**: 节点未启动或端口错误

**解决方案**:
1. 确认节点正在运行
2. 确认端口号正确 (9000 for RPC, 9123 for faucet)
3. 检查防火墙设置

### "Invalid transaction"

**原因**: 交易构造错误或签名错误

**解决方案**:
1. 检查交易参数是否正确
2. 验证 gas coin 是否有效
3. 确认签名逻辑正确

### "Insufficient gas"

**原因**: Gas budget 不足

**解决方案**:
```rust
// 增加 gas budget
let gas_budget = 50_000_000;  // 从 10M 增加到 50M
```

### "Module resolution failed"

**原因**: DEMO_PACKAGE_ID 对应的 module 不存在

**说明**: 这是正常的,因为 Demo Precompile 会拦截这个交易,不会真正在 Move VM 中查找 module。

**如果交易被拒绝**: 检查 dex-demo feature 是否启用。

## 验证 Precompile 是否工作

### 方法 1: 检查日志

**预期日志**:
```
[INFO  sui_node] Detected demo transaction, routing to DemoEngine
[DEBUG dex_demo] Executing demo transaction
```

**如果没有这些日志**:
- dex-demo feature 未启用
- 交易未被识别为 Demo 交易

### 方法 2: 检查 Gas 消耗

**Demo 交易**: `computation_cost = 1000` (固定)
**标准交易**: `computation_cost ≈ 700-1500` (动态)

如果 Demo 交易的 `computation_cost != 1000`,说明未走 Precompile 路径。

### 方法 3: 运行单元测试

```bash
# 运行 dex-demo 单元测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p dex-demo

# 运行 E2E 测试
SUI_SKIP_SIMTESTS=1 cargo nextest run -p sui-e2e-tests --features dex-demo test_demo
```

如果测试通过,说明 Precompile 逻辑正确。

## 获取帮助

如果以上步骤都无法解决问题,请提供以下信息:

1. **节点日志** (最后 100 行):
   ```bash
   tail -100 sui_node.log
   ```

2. **编译信息**:
   ```bash
   cargo build --bin sui --features dex-demo -vv 2>&1 | grep dex-demo
   ```

3. **示例程序输出**:
   ```bash
   cargo run --example send_demo_tx -p dex-demo 2>&1 | tee demo_tx_output.log
   ```

4. **系统信息**:
   ```bash
   rustc --version
   cargo --version
   uname -a
   ```

5. **完整错误堆栈** (如果有):
   设置 `RUST_BACKTRACE=1` 运行:
   ```bash
   RUST_BACKTRACE=1 cargo run --example send_demo_tx -p dex-demo
   ```

---

**最后更新**: 2026-01-08
**维护者**: DEX 团队
