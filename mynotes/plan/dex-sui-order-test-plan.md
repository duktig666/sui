# 创建 dex-node-test 测试模块

## 任务目标

在 `dex-sui/crates/` 下创建新的 `dex-node-test` 模块，用 Rust 编写测试代码，连接到已启动的节点发送 DEX 下单交易。

## 设计决策

### 模块定位

参考 `sui-cluster-test` 模式，创建一个可以：
1. 连接到已运行的远程节点（通过 RPC URL）
2. 或启动本地测试集群
3. 执行 DEX 下单测试（包含完整前置条件）

### 下单成功的前置条件

根据 DEX 执行流程分析，下单需要以下前置步骤：

```
1. Subaccount (子账户)
   └── 创建 CreateSubaccount
   └── 存款 DepositSubaccount (充值保证金)

2. Perpetual (永续合约市场)
   └── 创建 CreatePerpetual (如果市场不存在)

3. Order (订单)
   └── 下单 PlaceOrder
```

---

## 实施计划

### 目录结构

```
dex-sui/crates/dex-node-test/
├── Cargo.toml
├── src/
│   ├── lib.rs           # 主库，导出模块
│   ├── config.rs        # 配置（RPC URL, Faucet 等）
│   ├── client.rs        # DEX 客户端封装
│   ├── test_context.rs  # 测试上下文
│   └── tests/
│       ├── mod.rs
│       ├── subaccount_test.rs   # 子账户测试
│       ├── perpetual_test.rs    # 永续合约市场测试
│       └── order_test.rs        # 下单测试
└── examples/
    └── place_order.rs   # 下单示例脚本
```

### 关键文件内容

#### 1. Cargo.toml

```toml
[package]
name = "dex-node-test"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
# 核心依赖
anyhow.workspace = true
async-trait.workspace = true
tokio = { workspace = true, features = ["full"] }
clap.workspace = true
tracing.workspace = true

# Sui 依赖
sui-sdk.workspace = true
sui-types.workspace = true
sui-keys.workspace = true
sui-config.workspace = true
sui-json-rpc-types.workspace = true
shared-crypto.workspace = true

# 测试依赖
tempfile.workspace = true

[dev-dependencies]
test-cluster.workspace = true
sui-macros.workspace = true

[[example]]
name = "place_order"
path = "examples/place_order.rs"
```

#### 2. config.rs - 连接配置

```rust
use clap::Parser;

#[derive(Parser, Clone, Debug)]
pub struct DexTestConfig {
    /// Fullnode RPC URL
    #[clap(long, default_value = "http://127.0.0.1:9000")]
    pub fullnode_url: String,

    /// Faucet URL (optional, for getting test coins)
    #[clap(long, default_value = "http://127.0.0.1:9123/gas")]
    pub faucet_url: Option<String>,

    /// Use local test cluster instead of remote node
    #[clap(long)]
    pub use_local_cluster: bool,
}
```

#### 3. client.rs - DEX 客户端

封装 DEX 交易构建和发送逻辑：

```rust
pub struct DexClient {
    sui_client: SuiClient,
    keystore: FileBasedKeystore,
    sender: SuiAddress,
}

impl DexClient {
    // 创建子账户
    pub async fn create_subaccount(&self, number: u32) -> Result<ObjectID>;

    // 存款
    pub async fn deposit(&self, subaccount_id: ObjectID, amount: u128) -> Result<()>;

    // 创建永续合约市场
    pub async fn create_perpetual(&self, perpetual_id: u32) -> Result<ObjectID>;

    // 下限价单
    pub async fn place_limit_order(&self, params: PlaceOrderParams) -> Result<ObjectID>;

    // 下市价单
    pub async fn place_market_order(&self, params: PlaceOrderParams) -> Result<ObjectID>;

    // 撤单
    pub async fn cancel_order(&self, order_id: ObjectID) -> Result<()>;
}
```

#### 4. tests/order_test.rs - 下单测试

```rust
/// 完整下单流程测试
#[tokio::test]
async fn test_place_limit_order_full_flow() {
    // 1. 创建子账户
    let subaccount_id = client.create_subaccount(0).await?;

    // 2. 存款 (充值保证金)
    client.deposit(subaccount_id, 100000).await?;

    // 3. 创建永续合约市场 (如果不存在)
    let perpetual_id = client.create_perpetual(0).await?;

    // 4. 下单
    let order_id = client.place_limit_order(PlaceOrderParams {
        subaccount_id,
        perpetual_id: 0,
        side: Side::Buy,
        quantums: 100,
        subticks: 50000,
        ..Default::default()
    }).await?;

    // 5. 验证订单状态
    let order = client.get_order(order_id).await?;
    assert_eq!(order.status, OrderStatus::Open);
}
```

#### 5. examples/place_order.rs - 命令行示例

```rust
/// 连接到已运行节点，执行下单
///
/// 使用方式:
/// cargo run --example place_order -- --fullnode-url http://127.0.0.1:9000
#[tokio::main]
async fn main() -> Result<()> {
    let config = DexTestConfig::parse();
    let client = DexClient::connect(&config).await?;

    // 执行完整下单流程
    println!("1. Creating subaccount...");
    let subaccount_id = client.create_subaccount(0).await?;

    println!("2. Depositing funds...");
    client.deposit(subaccount_id, 100000).await?;

    println!("3. Creating perpetual market...");
    let perpetual_obj_id = client.create_perpetual(0).await?;

    println!("4. Placing limit order...");
    let order_id = client.place_limit_order(...).await?;

    println!("Order placed successfully! ID: {}", order_id);
    Ok(())
}
```

---

## 关键文件路径

| 文件 | 作用 |
|-----|------|
| `dex-sui/Cargo.toml` | 需要添加 `dex-node-test` 到 workspace members |
| `dex-sui/crates/dex-node-test/Cargo.toml` | 新模块配置 |
| `dex-sui/crates/dex-node-test/src/lib.rs` | 模块入口 |
| `dex-sui/crates/dex-node-test/src/client.rs` | DEX 客户端封装 |
| `dex-sui/crates/dex-node-test/examples/place_order.rs` | 命令行示例 |

### 参考现有代码

| 参考文件 | 作用 |
|---------|------|
| `crates/sui-cluster-test/` | 测试框架模式 |
| `crates/sui-types/src/dex.rs` | DEX 类型定义 |
| `crates/sui-types/src/dex_builder.rs` | 交易构建器 |
| `crates/sui-e2e-tests/tests/dex_order_tests.rs` | E2E 测试示例 |
| `crates/sui/src/dex_commands.rs` | CLI 命令实现 |

---

## 验证方式

### 1. 编译验证

```bash
cd dex-sui
cargo build -p dex-node-test
```

### 2. 运行示例（需要先启动节点）

```bash
# 终端 1: 启动节点
RUST_LOG="off,sui_node=info" ./target/debug/sui start --with-faucet --force-regenesis --fullnode-rpc-port 9000

# 终端 2: 运行测试
cargo run -p dex-node-test --example place_order -- --fullnode-url http://127.0.0.1:9000
```

### 3. 运行单元测试（本地集群模式）

```bash
cargo test -p dex-node-test
```

---

## 实施步骤

1. **创建 crate 目录结构**
   - 创建 `dex-sui/crates/dex-node-test/` 目录
   - 添加 Cargo.toml

2. **更新 workspace**
   - 在 `dex-sui/Cargo.toml` 的 `members` 中添加 `"crates/dex-node-test"`

3. **实现核心模块**
   - `config.rs` - 配置解析
   - `client.rs` - DEX 客户端
   - `lib.rs` - 模块导出

4. **编写测试用例**
   - `tests/subaccount_test.rs`
   - `tests/order_test.rs`

5. **创建示例脚本**
   - `examples/place_order.rs`

6. **验证**
   - 编译通过
   - 运行示例成功
