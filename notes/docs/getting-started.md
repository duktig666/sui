# Token Chain - 快速开始指南

本指南将帮助您在10分钟内启动并运行Token Chain区块链。

---

## 📋 前置要求

### 系统要求

- **操作系统**: Linux, macOS, 或 Windows (WSL2)
- **内存**: 至少 4GB RAM
- **磁盘空间**: 至少 2GB 可用空间

### 软件依赖

| 软件 | 最低版本 | 推荐版本 | 安装命令 |
|-----|---------|---------|---------|
| Rust | 1.75+ | 1.80+ | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Git | 2.0+ | 最新 | `apt-get install git` (Ubuntu) |
| jq | 1.6+ | 最新 | `apt-get install jq` (Ubuntu) |

### 验证安装

```bash
# 检查 Rust 版本
rustc --version  # 应该显示 rustc 1.75.0 或更高

# 检查 Cargo 版本
cargo --version  # 应该显示 cargo 1.75.0 或更高

# 检查 Git 版本
git --version    # 应该显示 git version 2.x.x
```

---

## 🚀 快速启动

### 步骤 1: 获取代码

```bash
# 克隆 Sui 仓库
git clone https://github.com/MystenLabs/sui.git
cd sui

# 进入 Token Chain 目录
cd notes/experiments/simple-token-chain
```

### 步骤 2: 构建项目

```bash
# 构建 Token Chain（开发模式）
cargo build

# 或者构建优化版本（推荐用于性能测试）
cargo build --release
```

**预期输出**：
```
   Compiling simple-token-chain v1.63.0
    Finished dev [unoptimized + debuginfo] target(s) in 45.23s
```

**构建时间**：
- 首次构建: 约 5-10 分钟（需要下载依赖）
- 后续构建: 约 10-30 秒

### 步骤 3: 启动节点

```bash
# 启动单节点（使用默认配置）
cargo run --bin simple-token-chain

# 或使用 release 模式（更快）
cargo run --release --bin simple-token-chain
```

**预期输出**：
```
2025-12-11T10:00:00.123Z  INFO simple_token_chain: Creating node 0
2025-12-11T10:00:00.456Z  INFO simple_token_chain: Starting node 0
2025-12-11T10:00:00.789Z  INFO simple_token_chain: Node 0 started successfully
2025-12-11T10:00:01.000Z  INFO simple_token_chain: 🚀 Token Chain node started at 127.0.0.1:9000
```

**节点信息**：
- **节点 ID**: 0
- **RPC 地址**: http://127.0.0.1:9000
- **状态**: 运行中

### 步骤 4: 验证节点运行

打开新的终端窗口，检查节点状态：

```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getStatus",
    "params": [],
    "id": 1
  }' | jq .
```

**预期响应**：
```json
{
  "jsonrpc": "2.0",
  "result": {
    "node_id": 0,
    "running": true,
    "rpc_addr": "127.0.0.1:9000"
  },
  "id": 1
}
```

✅ 如果看到上述输出，说明节点正常运行！

---

## 🎮 运行示例客户端

### 方式 1: 使用 Rust 客户端 (推荐)

在新终端中运行：

```bash
cd notes/experiments/simple-token-chain
cargo run --example client
```

**完整演示流程**：
```
🚀 Token Chain Client Demo

✅ Connected to Token Chain node at http://127.0.0.1:9000

👥 Created test addresses:
   Alice:   0x616c696365000000
   Bob:     0x626f62000000
   Charlie: 0x6368617269650000

📊 Step 1: Checking node status...
   Node status: { "node_id": 0, "running": true, "rpc_addr": "127.0.0.1:9000" }

💰 Step 2: Checking initial balances...
   Alice's balance: 0 tokens

🏦 Step 3: Minting 1000 tokens to Alice...
   Transaction hash: 0xabcd1234...
   ✅ Alice's new balance: 1000 tokens

💸 Step 4: Transferring 300 tokens from Alice to Bob...
   Transaction hash: 0xef567890...
   ✅ Alice's balance: 700 tokens
   ✅ Bob's balance: 300 tokens

💸 Step 5: Transferring 200 tokens from Alice to Charlie...
   Transaction hash: 0x12345678...

📊 Step 6: Final state of the blockchain:
   Alice:   500 tokens (nonce: 2)
   Bob:     300 tokens
   Charlie: 200 tokens
   Total:   1000 tokens

❌ Step 7: Testing invalid transaction (insufficient balance)...
   ✅ Expected error: Insufficient balance: has 300, needs 1000

🎉 Demo complete!

📝 Summary:
   - Created accounts for Alice, Bob, and Charlie
   - Minted 1000 tokens to Alice
   - Transferred 300 tokens to Bob
   - Transferred 200 tokens to Charlie
   - Verified nonce increments
   - Tested invalid transaction handling

✅ This is a working blockchain!
```

### 方式 2: 使用 curl 手动测试

#### 1. Mint 代币

```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "submitTransaction",
    "params": [{
      "Mint": {
        "to": [97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "amount": 1000
      }
    }],
    "id": 1
  }' | jq .
```

**响应**：
```json
{
  "jsonrpc": "2.0",
  "result": "0xabcd1234...",
  "id": 1
}
```

#### 2. 查询余额

```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getBalance",
    "params": [[97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
    "id": 2
  }' | jq .
```

**响应**：
```json
{
  "jsonrpc": "2.0",
  "result": 1000,
  "id": 2
}
```

#### 3. 转账

```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "submitTransaction",
    "params": [{
      "Transfer": {
        "from": [97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "to": [98,111,98,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "amount": 300,
        "nonce": 0
      }
    }],
    "id": 3
  }' | jq .
```

---

## 🧪 运行测试

### 运行所有测试

```bash
# 使用 cargo test（标准方式）
cargo test -- --test-threads=1

# 或使用 nextest（如果已安装，更快）
cargo nextest run
```

**预期输出**：
```
running 21 tests
test executor::tests::test_batch_execution ... ok
test executor::tests::test_insufficient_balance ... ok
test executor::tests::test_invalid_nonce ... ok
test executor::tests::test_mint ... ok
test executor::tests::test_transfer ... ok
test integration_tests::test_complete_token_workflow ... ok
test integration_tests::test_insufficient_balance ... ok
test integration_tests::test_large_transfer_amount ... ok
test integration_tests::test_multiple_accounts ... ok
test integration_tests::test_node_restart ... ok
test integration_tests::test_nonce_validation ... ok
test integration_tests::test_self_transfer ... ok
test integration_tests::test_sequential_transactions ... ok
test integration_tests::test_zero_amount_transfer ... ok
test node::tests::test_node_creation ... ok
test node::tests::test_node_start_stop ... ok
test node::tests::test_submit_transaction ... ok
test types::tests::test_account ... ok
test types::tests::test_address_creation ... ok
test types::tests::test_transaction_hash ... ok
test types::tests::test_transaction_serialization ... ok

test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 运行特定测试

```bash
# 只运行单元测试
cargo test --lib

# 只运行集成测试
cargo test --test integration_tests

# 运行特定测试
cargo test test_nonce_validation
```

### 运行性能基准测试

```bash
# 运行所有基准测试
cargo bench

# 运行特定基准测试
cargo bench --bench throughput -- submit_transactions

# 生成性能报告
cargo bench
open target/criterion/report/index.html
```

---

## 🔧 配置选项

### 使用自定义配置文件

创建配置文件 `config.yaml`：

```yaml
node_id: 0
rpc_addr: "127.0.0.1:9000"

consensus:
  authority_index: 0
  committee_size: 4
  wave_length: 3
  leader_timeout_ms: 2000
```

启动节点时指定配置：

```bash
cargo run --bin simple-token-chain -- --config config.yaml
```

### 配置参数说明

| 参数 | 说明 | 默认值 | 推荐值 |
|-----|------|--------|--------|
| `node_id` | 节点 ID | 0 | 0-3 |
| `rpc_addr` | RPC 监听地址 | 127.0.0.1:9000 | 127.0.0.1:9000-9003 |
| `authority_index` | 共识节点索引 | 0 | 0-3 |
| `committee_size` | 委员会大小 | 4 | 4, 7, 10 |
| `wave_length` | Wave 长度 | 3 | 2-5 |
| `leader_timeout_ms` | Leader 超时（毫秒） | 2000 | 1000-5000 |

---

## 📚 下一步

### 深入学习

1. **阅读架构文档**: [architecture.md](architecture.md)
   - 了解系统设计
   - 理解组件交互
   - 掌握数据流

2. **查看 API 参考**: [api-reference.md](api-reference.md)
   - 完整的 RPC API 文档
   - 请求/响应示例
   - 错误处理

3. **研究代码**: [simple-token-chain 源码](../experiments/simple-token-chain/src/)
   - 类型定义: `types.rs`
   - 执行引擎: `executor.rs`
   - 节点实现: `node.rs`

### 扩展功能

1. **添加新的交易类型**
   ```rust
   pub enum Transaction {
       Transfer { ... },
       Mint { ... },
       Burn { to: Address, amount: u64 },  // 新增
   }
   ```

2. **实现持久化存储**
   - 集成 RocksDB
   - 实现状态快照
   - 添加恢复机制

3. **添加交易签名**
   - 使用 ed25519 签名
   - 验证交易授权
   - 防止伪造交易

4. **部署多节点测试网**
   - 配置 4 个节点
   - 测试共识机制
   - 验证一致性

---

## ❓ 常见问题 (FAQ)

### Q1: 节点启动失败，显示 "Address already in use"

**A**: RPC 端口被占用。解决方案：
```bash
# 查找占用端口的进程
lsof -i :9000

# 杀死进程
kill -9 <PID>

# 或使用不同端口
cargo run --bin simple-token-chain -- --config custom-config.yaml
```

### Q2: 编译时间太长

**A**: 优化编译速度：
```bash
# 使用 sccache 缓存编译结果
cargo install sccache
export RUSTC_WRAPPER=sccache

# 使用 lld 链接器（Linux）
sudo apt-get install lld
export RUSTFLAGS="-C link-arg=-fuse-ld=lld"
```

### Q3: 测试失败

**A**: 检查以下几点：
1. 确保只运行一个节点实例
2. 等待节点完全启动（约1秒）
3. 使用 `--test-threads=1` 避免并发问题

```bash
cargo test -- --test-threads=1
```

### Q4: 如何重置区块链状态？

**A**: 当前版本使用内存存储，重启节点即可重置：
```bash
# 按 Ctrl+C 停止节点
# 重新启动
cargo run --bin simple-token-chain
```

### Q5: 支持哪些 RPC 方法？

**A**: 完整列表请查看 [API Reference](api-reference.md)，主要方法：
- `submitTransaction` - 提交交易
- `getBalance` - 查询余额
- `getNonce` - 查询 nonce
- `getStatus` - 查询节点状态
- `getTransaction` - 查询交易信息

### Q6: 如何查看日志？

**A**: 设置日志级别环境变量：
```bash
# 查看所有日志
RUST_LOG=debug cargo run --bin simple-token-chain

# 只查看 info 级别
RUST_LOG=info cargo run --bin simple-token-chain

# 只查看特定模块
RUST_LOG=simple_token_chain=debug cargo run --bin simple-token-chain
```

### Q7: 性能如何？

**A**: 单节点性能（参考值）：
- **吞吐量**: ~1000 TPS（开发模式），~5000 TPS（release 模式）
- **延迟**: <10ms（本地）
- **内存**: ~100MB

实际性能取决于硬件配置和网络条件。

### Q8: 如何贡献代码？

**A**: 欢迎贡献！步骤：
1. Fork 仓库
2. 创建特性分支: `git checkout -b feature/my-feature`
3. 提交更改: `git commit -am 'Add my feature'`
4. 推送分支: `git push origin feature/my-feature`
5. 创建 Pull Request

请确保：
- ✅ 所有测试通过: `cargo test`
- ✅ 代码格式正确: `cargo fmt`
- ✅ 无 clippy 警告: `cargo clippy -- -D warnings`

---

## 🆘 获取帮助

### 资源链接

- **文档**: [notes/docs/](.)
- **源码**: [simple-token-chain](../experiments/simple-token-chain/)
- **问题**: [GitHub Issues](https://github.com/MystenLabs/sui/issues)

### 社区支持

- **Discord**: Sui Developer Community
- **论坛**: https://forums.sui.io
- **Twitter**: @Mysten_Labs

---

## 🎉 总结

恭喜！您已经成功：

- ✅ 安装并配置了 Token Chain
- ✅ 启动了区块链节点
- ✅ 运行了示例客户端
- ✅ 执行了测试套件

**下一步建议**：
1. 阅读 [架构文档](architecture.md) 深入理解系统设计
2. 查看 [API 参考](api-reference.md) 学习完整的 RPC 接口
3. 修改代码添加自定义功能
4. 部署多节点测试网

祝您使用愉快！🚀

---

**文档版本**: 1.0
**最后更新**: 2025-12-11
