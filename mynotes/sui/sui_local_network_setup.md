# Sui 本地网络开发环境搭建指南

> **版本**: v1.0  
> **日期**: 2025-01-XX  
> **参考**: Sui 代码仓库, Sui 官方文档

---

## 📋 目录

1. [概述](#1-概述)
2. [环境准备](#2-环境准备)
3. [方式一：CLI 启动（推荐用于开发）](#3-方式一cli-启动推荐用于开发)
4. [方式二：代码启动（推荐用于测试和调试）](#4-方式二代码启动推荐用于测试和调试)
5. [配置说明](#5-配置说明)
6. [Debug 调试配置](#6-debug-调试配置)
7. [修改代码并测试](#7-修改代码并测试)
8. [常见问题](#8-常见问题)

---

## 1. 概述

### 1.1 启动方式对比

| 方式 | 适用场景 | 优点 | 缺点 |
|-----|---------|------|------|
| **CLI (`sui start`)** | 日常开发、快速测试 | 简单易用、支持持久化 | 需要重新编译才能修改代码 |
| **代码启动 (`Swarm`/`TestCluster`)** | 单元测试、集成测试、Debug | 可编程控制、支持断点调试 | 需要编写代码 |
| **直接运行 Validator** | 深度定制、性能测试 | 完全控制、支持自定义配置 | 配置复杂 |

### 1.2 推荐方案

- **日常开发**: 使用 `sui start` + Debug 构建
- **代码测试**: 使用 `TestCluster` API
- **深度定制**: 直接运行 Validator 节点

---

## 2. 环境准备

### 2.1 构建 Sui CLI（Debug 模式）

**推荐使用 Debug 模式**，便于排错和调试：

```bash
# 进入 Sui 仓库根目录
cd /path/to/sui

# Debug 模式构建（推荐，便于调试）
cargo build --bin sui

# 验证构建成功
./target/debug/sui --version

# 或者安装到 PATH（可选）
cargo install --path crates/sui --bin sui --debug
```

### 2.2 Release 模式构建（可选）

如果需要更好的性能：

```bash
# Release 模式构建
cargo build --release --bin sui

# 使用 release 版本
./target/release/sui --version
```

### 2.3 检查依赖

确保已安装所有必要的依赖：

- **Rust**: 检查 `rust-toolchain.toml` 中的版本要求
- **PostgreSQL** (可选): 如果需要 Indexer 或 GraphQL 服务
  ```bash
  # macOS
  brew install postgresql
  
  # Ubuntu
  sudo apt-get install postgresql libpq-dev
  ```

---

## 3. 方式一：CLI 启动（推荐用于开发）

### 3.1 快速模式（不持久化）

每次启动都会生成新的 genesis，适合快速测试：

```bash
# 基本启动（1 个 validator + faucet）
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start --with-faucet --force-regenesis

# 启动 4 个 validator（推荐用于测试共识）
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start \
    --committee-size 4 \
    --with-faucet \
    --force-regenesis

# 指定临时目录（避免使用 /tmp）
mkdir -p ./tmp
TMPDIR="$PWD/tmp" RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start --with-faucet --force-regenesis
```

### 3.2 持久化模式（推荐用于开发）

保留状态，支持重启：

```bash
# 1. 生成 genesis 和配置文件
export SUI_CHAIN_DIR="$PWD/local-network-config"
./target/debug/sui genesis \
  --with-faucet \
  --committee-size 4 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force

# 查看生成的配置文件
ls -la "$SUI_CHAIN_DIR"

# 2. 启动网络（保留状态）
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet

# 3. 停止后重新启动（状态保留）
# Ctrl+C 停止，然后再次运行上述命令
```

### 3.3 启动选项详解

```bash
# 完整示例
RUST_LOG="off,sui_node=info,sui_core=debug" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --with-indexer \
    --with-graphql=0.0.0.0:9125 \
    --fullnode-rpc-port 9000 \
    --epoch-duration-ms 60000 \
    --committee-size 4
```

**关键参数**:
- `--network.config <DIR>`: 配置文件目录（持久化模式）
- `--force-regenesis`: 强制重新生成 genesis（快速模式）
- `--committee-size <N>`: Validator 数量（仅快速模式生效）
- `--with-faucet[=<HOST:PORT>]`: 启动 Faucet 服务（默认 `0.0.0.0:9123`）
- `--with-indexer[=<DB_URL>]`: 启动 Indexer（需要 PostgreSQL）
- `--with-graphql[=<HOST:PORT>]`: 启动 GraphQL 服务（需要 Indexer）
- `--fullnode-rpc-port <PORT>`: Fullnode RPC 端口（默认 `9000`）
- `--epoch-duration-ms <MS>`: Epoch 持续时间（默认 `60000` 毫秒）
- `--no-full-node`: 不启动 Fullnode（仅启动 Validators）

### 3.4 验证网络运行

```bash
# 检查 Fullnode RPC
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sui_getTotalTransactionBlocks",
    "params": []
  }'

# 连接客户端
./target/debug/sui client new-env --alias local --rpc http://127.0.0.1:9000
./target/debug/sui client switch --env local

# 领取测试币
./target/debug/sui client faucet

# 查看 Gas
./target/debug/sui client gas
```

---

## 4. 方式二：代码启动（推荐用于测试和调试）

### 4.1 使用 TestCluster API

这是**最推荐的方式**用于代码测试和调试，可以完全控制启动过程：

```rust
// examples/local_network/main.rs
use sui_test_cluster::TestClusterBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    let _guard = telemetry_subscribers::TelemetryConfig::new()
        .with_env()
        .init();

    // 构建测试集群
    let cluster = TestClusterBuilder::new()
        // 可选：自定义 validator 数量
        .with_num_validators(4)
        // 可选：自定义 epoch 持续时间
        .with_epoch_duration_ms(60000)
        // 可选：添加额外的 genesis 对象
        // .with_objects([/* objects */])
        .build()
        .await;

    println!("Fullnode RPC URL: {}", cluster.rpc_url());
    println!("Chain ID: {:?}", cluster.sui_client().chain_identifier());

    // 使用集群进行测试
    let wallet = cluster.wallet();
    let addresses = wallet.get_addresses();
    println!("Test addresses: {:?}", addresses);

    // 保持运行（在实际应用中，你需要保持集群运行）
    // 例如，可以使用 tokio::signal::ctrl_c() 来等待停止信号
    tokio::signal::ctrl_c().await?;
    println!("Shutting down...");

    Ok(())
}
```

### 4.2 使用 Swarm API（更底层）

如果需要更多控制，可以直接使用 `Swarm` API：

```rust
// examples/local_network/advanced.rs
use sui_swarm::memory::{Swarm, SwarmBuilder};
use sui_swarm_config::genesis_config::GenesisConfig;
use sui_config::NodeConfig;
use std::num::NonZeroUsize;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = telemetry_subscribers::TelemetryConfig::new()
        .with_env()
        .init();

    // 构建 Swarm
    let mut swarm = SwarmBuilder::default()
        // 设置 committee 大小
        .committee_size(NonZeroUsize::new(4).unwrap())
        // 设置 epoch 持续时间（毫秒）
        .with_epoch_duration_ms(60000)
        // 自定义 genesis 配置（可选）
        .with_genesis_config(GenesisConfig::custom_genesis(1, 100))
        .build();

    // 启动网络
    swarm.launch().await?;

    // 等待节点连接
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    println!("Network started!");
    println!("Validators: {:?}", swarm.validator_addresses());

    // 获取 Fullnode RPC 地址
    let fullnode_rpc = swarm.fullnode_rpc_address();
    println!("Fullnode RPC: http://{}", fullnode_rpc);

    // 保持运行
    tokio::signal::ctrl_c().await?;
    println!("Shutting down...");

    Ok(())
}
```

### 4.3 编写测试代码

创建测试文件 `tests/my_test.rs`:

```rust
use sui_test_cluster::TestClusterBuilder;
use sui_sdk::SuiClientBuilder;
use sui_types::base_types::SuiAddress;

#[tokio::test]
async fn test_my_feature() {
    // 初始化日志
    let _guard = telemetry_subscribers::TelemetryConfig::new()
        .with_env()
        .init();

    // 启动测试集群
    let cluster = TestClusterBuilder::new()
        .with_num_validators(4)
        .build()
        .await;

    // 获取客户端
    let client = cluster.sui_client();
    let wallet = cluster.wallet();

    // 执行测试逻辑
    let addresses = wallet.get_addresses();
    println!("Test addresses: {:?}", addresses);

    // 验证结果
    // assert_eq!(...);

    // 集群会在测试结束时自动清理
}
```

运行测试：

```bash
# 运行单个测试
cargo test --package sui-test-cluster my_test

# 运行所有测试
cargo test --package sui-test-cluster

# 显示测试输出
cargo test --package sui-test-cluster my_test -- --nocapture
```

---

## 5. 配置说明

### 5.1 配置文件结构

持久化模式会生成以下配置文件：

```
local-network-config/
├── genesis.blob              # Genesis 状态
├── network.yaml              # 网络配置（所有 validator 信息）
├── fullnode.yaml             # Fullnode 配置
├── client.yaml               # 客户端配置
├── sui.keystore             # Keystore（包含 faucet 密钥）
└── 127.0.0.1-<port>.yaml    # 各个 Validator 的配置
```

### 5.2 Network 配置（network.yaml）

```yaml
authorities:
  - address: /ip4/127.0.0.1/tcp/8080/http
    stake: 1
    voting_rights: 1
    host: "127.0.0.1"
    network_key: "0x..."
    protocol_key: "0x..."
    worker_key: "0x..."
    account_key: "0x..."
    gas_price: 1
```

### 5.3 Validator 配置示例

```yaml
# 127.0.0.1-8080.yaml
protocol-key-pair:
  path: /path/to/protocol.key
worker-key-pair:
  path: /path/to/worker.key
network-key-pair:
  path: /path/to/network.key
account-key-pair:
  path: /path/to/account.key

db-path: /path/to/authorities_db
network-address: /ip4/127.0.0.1/tcp/8080/http
metrics-address: 127.0.0.1:9184
admin-interface-port: 1337

genesis:
  genesis-file-location: /path/to/genesis.blob

consensus-config:
  db-path: /path/to/consensus_db

p2p-config:
  listen-address: 0.0.0.0:8084
  external-address: /dns/localhost/udp/8084
```

### 5.4 自定义配置

修改配置文件后，使用配置启动：

```bash
./target/debug/sui start --network.config ./my-custom-config
```

---

## 6. Debug 调试配置

### 6.1 IDE 配置（VS Code + CodeLLDB）

创建 `.vscode/launch.json`:

```json
{
    "version": "0.2.0",
    "configurations": [
        {
            "type": "lldb",
            "request": "launch",
            "name": "Debug sui start",
            "cargo": {
                "args": [
                    "build",
                    "--bin",
                    "sui"
                ],
                "filter": {
                    "name": "sui",
                    "kind": "bin"
                }
            },
            "args": [
                "start",
                "--network.config",
                "./local-network-config",
                "--with-faucet",
                "--force-regenesis"
            ],
            "cwd": "${workspaceFolder}",
            "env": {
                "RUST_LOG": "debug,sui_node=debug,sui_core=debug",
                "RUST_BACKTRACE": "1"
            }
        },
        {
            "type": "lldb",
            "request": "launch",
            "name": "Debug Test",
            "cargo": {
                "args": [
                    "test",
                    "--package",
                    "sui-test-cluster",
                    "--test",
                    "my_test"
                ],
                "filter": {
                    "name": "my_test",
                    "kind": "test"
                }
            },
            "cwd": "${workspaceFolder}",
            "env": {
                "RUST_LOG": "debug",
                "RUST_BACKTRACE": "1"
            }
        }
    ]
}
```

### 6.2 IDE 配置（IntelliJ IDEA + Rust Plugin）

创建运行配置：

1. **Run** → **Edit Configurations...**
2. 添加 **Cargo Command** 配置：
   - **Command**: `test`
   - **Arguments**: `--package sui-test-cluster --test my_test`
   - **Environment variables**: `RUST_LOG=debug;RUST_BACKTRACE=1`

或者直接运行：

```bash
# 使用 rust-lldb 调试
rust-lldb ./target/debug/sui -- start --with-faucet --force-regenesis

# 在 lldb 中设置断点
(lldb) breakpoint set --file sui_commands.rs --line 787
(lldb) run
```

### 6.3 日志级别配置

```bash
# 详细日志（推荐用于调试）
RUST_LOG="debug,sui_node=debug,sui_core=debug,sui_execution=debug" \
  ./target/debug/sui start --with-faucet --force-regenesis

# 仅关键日志（推荐用于正常运行）
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start --with-faucet --force-regenesis

# 所有模块详细日志（谨慎使用，输出量巨大）
RUST_LOG="trace" \
  ./target/debug/sui start --with-faucet --force-regenesis
```

### 6.4 断点调试技巧

1. **在代码中添加断点**:
   ```rust
   // 在代码中添加断点
   use std::panic;
   panic::set_hook(Box::new(|_| {
       // 触发断点的位置
   }));
   ```

2. **使用 `dbg!` 宏**:
   ```rust
   let result = some_function();
   dbg!(&result);  // 输出并继续执行
   ```

3. **使用 `tracing` 日志**:
   ```rust
   use tracing::{debug, info, warn, error};
   
   debug!("Debug message: {:?}", variable);
   info!("Info message");
   ```

---

## 7. 修改代码并测试

### 7.1 修改代码流程

1. **修改源代码**:
   ```bash
   # 例如修改 crates/sui/src/sui_commands.rs
   vim crates/sui/src/sui_commands.rs
   ```

2. **重新构建**:
   ```bash
   # 增量编译（快速）
   cargo build --bin sui

   # 完整重新编译（如果遇到问题）
   cargo clean
   cargo build --bin sui
   ```

3. **运行测试**:
   ```bash
   # 使用 CLI 测试
   ./target/debug/sui start --with-faucet --force-regenesis

   # 或使用代码测试
   cargo test --package sui-test-cluster my_test
   ```

### 7.2 热重载开发（推荐）

使用 `cargo watch` 自动重新构建：

```bash
# 安装 cargo-watch
cargo install cargo-watch

# 自动重新构建和运行
cargo watch -x "build --bin sui" -x "run --bin sui -- start --with-faucet --force-regenesis"
```

### 7.3 单元测试示例

```rust
// crates/sui/src/sui_commands.rs (示例)
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_start_network() {
        // 测试启动逻辑
        let result = start(
            None,
            Some("0.0.0.0:9123".to_string()),
            RpcArgs::default(),
            true,  // force_regenesis
            None,  // epoch_duration_ms
            9000,  // fullnode_rpc_port
            None,  // data_ingestion_dir
            false, // no_full_node
            Some(1), // committee_size
        ).await;

        assert!(result.is_ok());
    }
}
```

运行测试：

```bash
# 运行所有测试
cargo test --package sui

# 运行特定测试
cargo test --package sui test_start_network

# 显示测试输出
cargo test --package sui test_start_network -- --nocapture
```

---

## 8. 常见问题

### 8.1 端口冲突

**问题**: 启动时报错端口已被占用

**解决方案**:
```bash
# 查找占用端口的进程
lsof -i :9000  # RPC 端口
lsof -i :9123  # Faucet 端口

# 杀死进程
kill -9 <PID>

# 或使用不同的端口
./target/debug/sui start \
  --with-faucet=0.0.0.0:9124 \
  --fullnode-rpc-port 9001
```

### 8.2 编译错误

**问题**: 编译时出现依赖问题

**解决方案**:
```bash
# 更新依赖
cargo update

# 清理并重新构建
cargo clean
cargo build --bin sui

# 如果仍有问题，检查 rust-toolchain.toml
cat rust-toolchain.toml
```

### 8.3 数据库锁定

**问题**: 数据库文件被锁定（持久化模式）

**解决方案**:
```bash
# 确保没有其他 Sui 进程在运行
ps aux | grep sui

# 如果找不到进程但仍有锁定，删除数据库
rm -rf local-network-config/authorities_db
rm -rf local-network-config/consensus_db

# 重新生成 genesis
./target/debug/sui genesis --with-faucet --working-dir ./local-network-config --force
```

### 8.4 内存不足

**问题**: 启动时内存不足（特别是多个 validator）

**解决方案**:
```bash
# 减少 validator 数量
./target/debug/sui start --committee-size 1 --with-faucet --force-regenesis

# 或增加系统内存限制
ulimit -v 4194304  # 4GB（根据你的系统调整）
```

### 8.5 Debug 信息过多

**问题**: 日志输出太多，难以查看

**解决方案**:
```bash
# 只显示关键日志
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start --with-faucet --force-regenesis

# 过滤特定模块
RUST_LOG="off,sui_node=info,sui_core::authority::authority_store=debug" \
  ./target/debug/sui start --with-faucet --force-regenesis
```

### 8.6 测试集群无法启动

**问题**: `TestCluster` 启动失败

**解决方案**:
```rust
// 增加超时时间
let cluster = TestClusterBuilder::new()
    .with_num_validators(1)  // 先尝试 1 个 validator
    .with_epoch_duration_ms(120000)  // 增加 epoch 时间
    .build()
    .await;
```

---

## 9. 高级用法

### 9.1 自定义 Genesis

```rust
use sui_swarm_config::genesis_config::{GenesisConfig, ValidatorGenesisConfig};

let genesis_config = GenesisConfig::custom_genesis(
    1,    // num_validators
    100,  // gas_amount
);

let mut swarm = SwarmBuilder::default()
    .with_genesis_config(genesis_config)
    .build();

swarm.launch().await?;
```

### 9.2 多节点网络（跨机器）

需要在不同机器上运行：

1. **生成配置**:
   ```bash
   ./target/debug/sui genesis --working-dir ./network-config --force
   ```

2. **分发配置**:
   - 将 `network-config/` 复制到各个机器
   - 确保每个机器有自己的 validator 配置文件

3. **在各机器启动**:
   ```bash
   # 机器 1
   ./target/debug/sui-node --config-path ./network-config/validator-0.yaml

   # 机器 2
   ./target/debug/sui-node --config-path ./network-config/validator-1.yaml
   ```

### 9.3 性能分析

```bash
# 使用 perf（Linux）
perf record -g ./target/release/sui start --with-faucet --force-regenesis
perf report

# 使用 Instruments（macOS）
instruments -t "Time Profiler" -D trace.trace ./target/release/sui start --with-faucet --force-regenesis
```

---

## 10. 总结

### 10.1 推荐工作流程

1. **日常开发**:
   - 使用 `sui start` + 持久化配置
   - Debug 模式构建
   - 修改代码 → 重新构建 → 重启网络

2. **代码测试**:
   - 使用 `TestCluster` API
   - 编写单元测试/集成测试
   - 使用 `cargo test` 运行

3. **深度调试**:
   - 使用 IDE 调试器（VS Code/IntelliJ）
   - 设置断点
   - 查看变量和执行流程

### 10.2 关键要点

- ✅ 使用 Debug 模式构建便于调试
- ✅ 使用持久化配置保留状态
- ✅ 使用 `TestCluster` API 进行代码测试
- ✅ 配置适当的日志级别
- ✅ 使用 IDE 调试器设置断点

### 10.3 参考资源

- **代码位置**:
  - `crates/sui/src/sui_commands.rs` - CLI 命令实现
  - `crates/sui-swarm/src/memory/swarm.rs` - Swarm 实现
  - `crates/test-cluster/src/lib.rs` - TestCluster API
  - `crates/sui-node/src/lib.rs` - 节点启动逻辑

- **配置文件**:
  - `crates/sui-config/src/node.rs` - 节点配置定义
  - `crates/sui-config/src/genesis.rs` - Genesis 配置

- **文档**:
  - `docs/content/guides/developer/sui-101/local-network.mdx` - 官方文档
  - `notes/sui-fork-chain/README.md` - 私链启动指南

---

**文档版本**: v1.0  
**最后更新**: 2025-01-XX  
**维护者**: Sui 开发团队

