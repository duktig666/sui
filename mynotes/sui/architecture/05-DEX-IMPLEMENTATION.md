# Sui DEX 实现专项

> **文档用途**: DEX 开发者的完整实施指南
>
> **适合人群**: DEX 开发者、运维工程师、架构师

---

## 目录

- [概述](#概述)
- [DEX 模块依赖清单](#dex-模块依赖清单)
- [最小 DEX 节点配置](#最小-dex-节点配置)
- [节点抽离方案对比](#节点抽离方案对比)
- [DeepBook 源码分析](#deepbook-源码分析)
- [DEX 开发实施路径](#dex-开发实施路径)
- [性能优化建议](#性能优化建议)

---

## 概述

### DEX 在 Sui 上的优势

1. **低延迟**:
   - 拥有对象交易 ~200ms (FastPath)
   - 共享对象交易 ~400ms (Mysticeti 共识)

2. **高吞吐**:
   - 理论 TPS: 200,000+ (简单转账)
   - 实际 DEX TPS: 5,000-20,000 (取决于订单簿复杂度)

3. **对象级并行**:
   - 不同交易对的订单簿可并行执行
   - 同一订单簿的订单按共识排序串行执行

4. **Move 语言优势**:
   - 资源安全 (防止双花)
   - 类型安全 (编译时检查)
   - Gas 可预测

### DEX 架构选择

**选项 1: 链上 CLOB (Central Limit Order Book)**
- 代表: DeepBook
- 优点: 去中心化、透明、可组合
- 缺点: Gas 成本、延迟较高

**选项 2: 链下撮合 + 链上结算**
- 代表: dYdX v4
- 优点: 超低延迟、高吞吐
- 缺点: 中心化风险、复杂度高

**选项 3: 混合模式**
- 链下订单簿 + 链上 AMM
- 平衡延迟和去中心化

本文档主要关注**选项 1 (链上 CLOB)**,基于 Sui 的 DeepBook 实现。

---

## DEX 模块依赖清单

### 完整依赖 (40-50 个 crate)

#### 1. Move 合约层 (核心)

**DeepBook 核心**:
```
sui-framework/packages/deepbook/sources/
├── clob_v2.move          # 中央限价订单簿
├── custodian_v2.move     # 资金托管 (账户余额管理)
├── critbit.move          # Critbit Tree (价格级别索引)
├── math.move             # 定点数运算 (手续费、价格计算)
└── order_query.move      # 订单查询接口
```

**sui-framework 基础模块**:
```
sui-framework/packages/sui-framework/sources/
├── balance.move          # 余额抽象
├── coin.move             # 代币标准
├── table.move            # 动态键值存储
├── linked_table.move     # 链表 (订单队列)
├── clock.move            # 时间戳 (订单过期)
├── event.move            # 事件发射
├── object.move           # 对象系统
├── transfer.move         # 所有权转移
└── tx_context.move       # 交易上下文
```

**为什么需要这些模块?**
- `balance/coin`: 代币充值、提现
- `table/linked_table`: 存储订单队列和价格级别
- `clock`: 订单过期时间检查
- `event`: 订单成交、取消等事件
- `critbit`: O(log n) 查找最优价格

#### 2. 链上交互层 (8 个 crate)

| Crate | 职责 | 重要性 |
|-------|------|--------|
| **sui-json-rpc** | 交易提交、状态查询 | ⭐⭐⭐⭐⭐ |
| **sui-transaction-builder** | 构建订单交易 | ⭐⭐⭐⭐⭐ |
| sui-json-rpc-types | RPC 类型转换 | ⭐⭐⭐⭐ |
| sui-rpc-api | 新 RPC 框架 | ⭐⭐⭐ |
| sui-json | JSON 序列化 | ⭐⭐⭐ |
| sui-keys | 密钥管理 | ⭐⭐⭐ |

**客户端集成示例**:
```rust
use sui_sdk::SuiClient;
use sui_transaction_builder::TransactionBuilder;

// 下单交易
let tx = TransactionBuilder::new()
    .move_call(
        deepbook_package_id,
        "clob_v2",
        "place_limit_order",
        type_args: vec![base_coin_type, quote_coin_type],
        call_args: vec![
            pool_id,
            price,
            quantity,
            side, // BUY or SELL
            expiration_timestamp,
        ],
        gas_budget,
    )
    .build();

let response = sui_client.execute_transaction_block(tx, signatures).await?;
```

**事件订阅** (订单成交通知):
```rust
let mut event_stream = sui_client
    .event_api()
    .subscribe_event(EventFilter::MoveEventType(
        "deepbook::clob_v2::OrderFilled".to_string()
    ))
    .await?;

while let Some(event) = event_stream.next().await {
    // 处理成交事件
}
```

#### 3. 执行层 (15 个 crate)

| Crate | 职责 | 为什么 DEX 需要? |
|-------|------|----------------|
| **sui-execution** | Move VM 执行层多路复用 | 执行订单匹配逻辑 |
| **sui-adapter** | Move 到 Sui 适配器 | Gas 计量、对象管理 |
| **move-vm-runtime** | VM 核心 | 执行 Move 字节码 |
| **move-stdlib** | Move 标准库 | 基础数据结构 |
| **consensus-core** | Mysticeti 共识 | **关键**: 共享对象(订单簿)排序 |
| consensus-config | 共识配置 | 性能调优 |
| consensus-types | 共识类型 | - |
| move-binary-format | 字节码格式 | - |
| move-bytecode-verifier | 字节码验证 | - |
| move-core-types | 类型系统 | - |
| fastcrypto | 密码学 | 签名验证 |

**为什么订单簿需要共识?**
```
订单簿是共享对象 (Shared Object):
- 多个用户同时下单
- 需要确定性的执行顺序
- 防止竞态条件 (Front-running)

共识保证:
- 订单按全局顺序执行
- 所有验证者看到相同的订单序列
- 成交价格确定性
```

**订单簿并发处理**:
```
不同交易对 (BTC/USDC vs ETH/USDC):
  → 完全并行执行 ✅

同一交易对 (BTC/USDC):
  → 共识排序 → 串行执行 ⚠️
```

#### 4. 存储层 (4 个 crate)

| Crate | 职责 | DEX 用途 |
|-------|------|---------|
| **sui-storage** | 存储抽象 | 订单簿状态缓存 |
| **typed-store** | RocksDB 封装 | 持久化订单簿 |
| authority_store | 持久化存储 | 交易历史 |
| sharded_lru | 分片缓存 | 热点订单簿缓存 |

**订单簿存储**:
```
内存缓存 (sharded_lru):
  → BTC/USDC 订单簿 (热点)
  → ETH/USDC 订单簿 (热点)
  → 缓存命中率 > 95%

持久化 (RocksDB):
  → 历史订单
  → 成交记录
  → 账户余额快照
```

#### 5. 索引层 (可选, 3-5 个 crate)

| Crate | 职责 | DEX 用途 |
|-------|------|---------|
| **sui-indexer-alt** | 新一代索引器 | **推荐**: 订单历史查询 |
| sui-indexer-alt-jsonrpc | JSON-RPC 接口 | API 查询 |
| sui-deepbook-indexer | DeepBook 专用 | 市场深度、K线 |

**为什么需要索引器?**
```
没有索引器:
  ❌ 无法查询历史订单
  ❌ 无法查询成交记录
  ❌ 无法统计交易量
  ❌ 无法生成 K 线图

有索引器:
  ✅ 实时订单历史
  ✅ 账户交易记录
  ✅ 市场深度数据
  ✅ 价格图表数据
```

**索引器数据流**:
```
sui-node
  ↓ Checkpoint 数据
sui-data-ingestion-core
  ↓ 解析交易和事件
sui-deepbook-indexer
  ↓ 写入 PostgreSQL
  ├─ orders 表 (订单历史)
  ├─ trades 表 (成交记录)
  ├─ balances 表 (账户余额)
  └─ klines 表 (K 线数据)
```

### DEX 依赖关系图

```
┌─────────────────────────────────────────────┐
│          DEX Frontend / Trading Bot        │
└──────────────────┬──────────────────────────┘
                   │
        ┌──────────┼──────────┐
        ▼          ▼          ▼
┌─────────────┐ ┌──────────┐ ┌─────────────┐
│ JSON-RPC    │ │ GraphQL  │ │ WebSocket   │
│ API         │ │ API      │ │ (Events)    │
└──────┬──────┘ └─────┬────┘ └──────┬──────┘
       │              │              │
       └──────────────┴──────────────┘
                      │
        ┌─────────────┴─────────────┐
        ▼                           ▼
┌──────────────┐            ┌──────────────┐
│ sui-node     │            │ sui-indexer  │
│ (实时执行)   │            │ (历史查询)   │
└──────┬───────┘            └──────┬───────┘
       │                           │
       ▼                           ▼
┌──────────────┐            ┌──────────────┐
│ sui-core     │            │ PostgreSQL   │
│ + consensus  │            │ (订单历史)   │
└──────┬───────┘            └──────────────┘
       │
       ▼
┌──────────────┐
│ sui-execution│
│ + Move VM    │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ DeepBook     │
│ Move 合约    │
│ (clob_v2)    │
└──────────────┘
```

---

## 最小 DEX 节点配置

### 节点类型选择

#### 选项 1: 全节点模式 (推荐开发测试)

**特点**:
- 不参与共识
- 同步区块链状态
- 提供 RPC 接口
- 可查询最新状态

**需要模块**: 约 33-40 个 crate

**适用场景**:
- DEX 开发测试
- 只读 API 节点
- 索引器数据源

**配置示例**:
```yaml
# dex-fullnode.yaml
consensus_config: null  # 不参与共识

json_rpc_address: "0.0.0.0:9000"
enable_index_processing: false  # 禁用内置索引

# 数据库路径
db_path: "/data/sui-fullnode"

# 网络配置
p2p_config:
  listen_address: "0.0.0.0:8080"
  external_address: "your.domain.com:8080"
```

#### 选项 2: 验证者模式 (生产环境)

**特点**:
- 参与共识
- 签名和验证
- 需要质押 SUI
- 高可用要求

**需要模块**: 约 40-50 个 crate

**适用场景**:
- DEX 主网部署
- 需要控制延迟
- 专属验证者集合

**配置示例**:
```yaml
# dex-validator.yaml
protocol_key_pair: "validator_key.key"
network_key_pair: "network_key.key"

consensus_config:
  address: "0.0.0.0:8081"
  db_path: "/data/consensus-db"

  # Mysticeti 参数调优
  narwhal_config:
    batch_size: 500000
    max_pending_transactions: 100000
```

### 最小模块清单 (33-40 个 crate)

#### 一级核心 (15 个, 绝对必需)

| Crate | 路径 | 职责 | 二进制影响 |
|-------|------|------|-----------|
| sui-node | crates/sui-node/ | 节点主程序 | ⭐⭐⭐⭐⭐ |
| sui-core | crates/sui-core/ | 核心逻辑 | ⭐⭐⭐⭐⭐ |
| sui-types | crates/sui-types/ | 类型定义 | ⭐⭐⭐⭐⭐ |
| sui-storage | crates/sui-storage/ | 存储层 | ⭐⭐⭐⭐⭐ |
| sui-config | crates/sui-config/ | 配置管理 | ⭐⭐⭐⭐⭐ |
| sui-protocol-config | crates/sui-protocol-config/ | 协议配置 | ⭐⭐⭐⭐⭐ |
| sui-execution | sui-execution/latest/ | 执行层 | ⭐⭐⭐⭐⭐ |
| sui-framework | crates/sui-framework/ | Move 框架 | ⭐⭐⭐⭐⭐ |
| sui-network | crates/sui-network/ | P2P 网络 | ⭐⭐⭐⭐⭐ |
| consensus-core | consensus/core/ | 共识 | ⭐⭐⭐⭐⭐ |
| consensus-config | consensus/config/ | 共识配置 | ⭐⭐⭐⭐ |
| consensus-types | consensus/types/ | 共识类型 | ⭐⭐⭐⭐ |
| typed-store | crates/typed-store/ | RocksDB | ⭐⭐⭐⭐⭐ |
| mysten-network | crates/mysten-network/ | 网络基础 | ⭐⭐⭐⭐⭐ |
| mysten-metrics | crates/mysten-metrics/ | 监控 | ⭐⭐⭐⭐ |

#### 二级核心 (10 个, Move 执行)

| Crate | 路径 | 职责 |
|-------|------|------|
| move-vm-runtime | external-crates/move/crates/move-vm-runtime/ | VM 运行时 |
| move-vm-types | external-crates/move/crates/move-vm-types/ | VM 类型 |
| move-binary-format | external-crates/move/crates/move-binary-format/ | 字节码 |
| move-bytecode-utils | external-crates/move/crates/move-bytecode-utils/ | 工具 |
| move-bytecode-verifier | external-crates/move/crates/move-bytecode-verifier/ | 验证器 |
| move-core-types | external-crates/move/crates/move-core-types/ | 核心类型 |
| move-stdlib | external-crates/move/crates/move-stdlib/ | 标准库 |
| move-vm-config | external-crates/move/crates/move-vm-config/ | VM 配置 |
| fastcrypto | external-crates/fastcrypto/ | 密码学 |
| fastcrypto-zkp | external-crates/fastcrypto/fastcrypto-zkp/ | 零知识证明 |

#### 三级核心 (8 个, RPC 接口)

| Crate | 路径 | 职责 |
|-------|------|------|
| sui-json-rpc | crates/sui-json-rpc/ | JSON-RPC 服务 |
| sui-json-rpc-api | crates/sui-json-rpc-api/ | API 定义 |
| sui-json-rpc-types | crates/sui-json-rpc-types/ | RPC 类型 |
| sui-rpc-api | crates/sui-rpc-api/ | 新 RPC 框架 |
| sui-transaction-builder | crates/sui-transaction-builder/ | 交易构建 |
| sui-tls | crates/sui-tls/ | TLS 支持 |
| anemo | external-crates/anemo/ | 网络框架 |
| anemo-tower | external-crates/anemo/crates/anemo-tower/ | 中间件 |

#### 四级扩展 (可选, 索引查询)

| Crate | 路径 | 职责 | 必需性 |
|-------|------|------|--------|
| sui-indexer-alt | crates/sui-indexer-alt/ | 索引器主程序 | ⭐⭐⭐ (强烈推荐) |
| sui-indexer-alt-jsonrpc | crates/sui-indexer-alt-jsonrpc/ | 索引 RPC | ⭐⭐⭐ |
| sui-deepbook-indexer | crates/sui-deepbook-indexer/ | DeepBook 索引 | ⭐⭐⭐ (DEX 专用) |
| PostgreSQL | 外部依赖 | 数据库 | ⭐⭐⭐ |

### 可删除模块 (约 70%, 94 个 crate)

#### 确定可删除 (不影响 DEX)

**跨链和特定功能** (12 个):
- sui-bridge, sui-bridge-cli, sui-bridge-indexer
- sui-bridge-indexer-alt, sui-bridge-schema, sui-bridge-watchdog
- sui-axelar-cgp
- sui-name-service, suins-indexer
- sui-oracle
- sui-rosetta
- sui-light-client

**测试和开发工具** (30+ 个):
- sui-benchmark, sui-cluster-test, sui-e2e-tests
- sui-graphql-e2e-tests, sui-json-rpc-tests
- sui-rpc-benchmark, sui-rpc-loadgen
- sui-simulator, sui-single-node-benchmark
- sui-swarm, sui-swarm-config, test-cluster
- transaction-fuzzer, simulacrum
- sui-test-transaction-builder, sui-test-validator
- sui-adapter-transactional-tests
- sui-framework-tests
- sui-upgrade-compatibility-transactional-tests
- sui-verifier-transactional-tests

**可选服务** (10+ 个):
- sui-graphql-rpc (可用 JSON-RPC 替代)
- sui-snapshot
- sui-telemetry
- sui-security-watchdog
- sui-metric-checker
- sui-aws-orchestrator
- sui-analytics-indexer
- sui-authority-aggregation (全节点聚合,单节点不需要)

**辅助工具** (20+ 个):
- sui-replay, sui-replay-2
- sui-package-dump
- sui-source-validation
- sui-surfer
- sui-futures
- sui-enum-compat-util
- sui-field-count, sui-field-count-derive
- 各种 -derive, -macros crates

---

## 节点抽离方案对比

### 方案 A: 配置裁剪 ⭐⭐⭐⭐⭐ (强烈推荐)

#### 实施步骤

1. **创建 DEX 专用配置**:
```yaml
# dex-node.yaml
# 禁用不需要的功能
enable_index_processing: false  # 使用外部索引器
consensus_config: null          # 全节点模式 (或配置验证者)

# 仅启用 HTTP JSON-RPC
jsonrpc_server_type: "http"
json_rpc_address: "0.0.0.0:9000"

# 限制资源使用
grpc_concurrency_limit: 50000
authority_store_pruning_config:
  num_epochs_to_retain: 2  # 仅保留最近 2 个 epoch

# 数据库路径
db_path: "/data/sui-dex-node"
```

2. **部署外部索引器**:
```bash
# 单独运行索引器进程
sui-indexer-alt \
  --rpc-url http://sui-node:9000 \
  --db-url postgresql://user:pass@localhost/sui_indexer \
  --reset-database
```

3. **裁剪 Move 包** (可选):
```bash
# 仅部署必需的 Move 包
sui move build --path sui-framework/packages/sui-framework/
sui move build --path sui-framework/packages/deepbook/

# 不部署 bridge, sui-system (如果不需要质押功能)
```

#### 优点
- ✅ **不修改源码**: 跟随官方升级,维护成本低
- ✅ **快速实施**: 1-2 天即可部署
- ✅ **兼容性好**: 与官方节点完全兼容
- ✅ **灵活调整**: 随时修改配置

#### 缺点
- ⚠️ 二进制文件仍包含未使用代码 (约 1GB)
- ⚠️ 内存优化有限 (约 4-8GB 内存占用)

#### 适用场景
- DEX 开发和测试
- 小规模生产部署 (<10,000 用户)
- 快速迭代和原型验证

#### 预期效果
- 启动时间: 无显著变化
- 内存占用: 减少 10-20% (通过配置优化)
- DEX TPS: 无影响 (瓶颈在共识和执行)

---

### 方案 B: 源码裁剪 ⭐⭐

#### 实施步骤

1. **Fork Sui 仓库**:
```bash
git clone https://github.com/MystenLabs/sui.git sui-dex
cd sui-dex
git checkout -b dex-optimized
```

2. **删除无关 crate**:
```bash
# 删除跨链桥
rm -rf crates/sui-bridge/
rm -rf crates/sui-bridge-cli/
rm -rf crates/sui-bridge-indexer/
rm -rf crates/sui-bridge-schema/

# 删除域名服务
rm -rf crates/sui-name-service/
rm -rf crates/suins-indexer/

# 删除快照、遥测
rm -rf crates/sui-snapshot/
rm -rf crates/sui-telemetry/

# 删除 GraphQL (使用 JSON-RPC)
rm -rf crates/sui-graphql-rpc/

# 删除测试工具 (30+ crates)
rm -rf crates/sui-benchmark/
rm -rf crates/sui-cluster-test/
# ... (见上面的可删除列表)
```

3. **修改 Cargo.toml**:
```toml
# sui/Cargo.toml
[workspace]
members = [
    # 仅保留必需的 33-40 个 crates
    "crates/sui-node",
    "crates/sui-core",
    # ... (一级、二级、三级核心模块)
]

exclude = [
    # 排除已删除的 crates
]
```

4. **修改 sui-node 依赖**:
```toml
# crates/sui-node/Cargo.toml
[dependencies]
# 移除可选依赖
# sui-bridge = { ... }  # 注释掉
# sui-graphql-rpc = { ... }  # 注释掉
```

5. **精简 sui-framework**:
```bash
cd crates/sui-framework/packages/
# 仅保留必需包
rm -rf bridge/
rm -rf sui-system/  # 如果不需要质押
```

6. **编译优化版本**:
```bash
cargo build --release --bin sui-node
# 输出: target/release/sui-node (约 300-400MB,原版约 500MB)
```

#### 优点
- ✅ **显著减少二进制大小**: 30-40% (500MB → 300MB)
- ✅ **降低内存占用**: 20-30% (8GB → 5-6GB)
- ✅ **加快编译速度**: 40-50% (删除大量 crate)
- ✅ **启动时间**: 减少 10-20%

#### 缺点
- ❌ **需要维护 fork**: 每次升级需要手动合并
- ❌ **升级困难**: 可能破坏依赖关系
- ❌ **测试成本高**: 需要重新验证所有功能
- ❌ **团队开销**: 需要专职人员维护

#### 适用场景
- 大规模生产部署 (>100,000 用户)
- 有专职区块链团队
- 长期运营 (>2 年)
- 对资源成本敏感 (节省云服务费用)

#### 风险评估
- ⚠️ **兼容性风险**: 可能无法连接到官方网络
- ⚠️ **安全风险**: 手动合并可能引入漏洞
- ⚠️ **功能缺失**: 误删重要依赖导致崩溃

---

### 方案 C: 容器化微服务 ⭐⭐⭐⭐

#### 架构设计

```
                    ┌─────────────────┐
                    │   Load Balancer  │
                    │   (HAProxy/Nginx)│
                    └────────┬─────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
         ▼                   ▼                   ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│  RPC Node 1     │ │  RPC Node 2     │ │  RPC Node 3     │
│  (无状态)       │ │  (无状态)       │ │  (无状态)       │
│  sui-node       │ │  sui-node       │ │  sui-node       │
│  + JSON-RPC     │ │  + JSON-RPC     │ │  + JSON-RPC     │
└────────┬────────┘ └────────┬────────┘ └────────┬────────┘
         │                   │                   │
         └───────────────────┼───────────────────┘
                             │
         ┌───────────────────┴───────────────────┐
         │                                       │
         ▼                                       ▼
┌─────────────────┐                    ┌─────────────────┐
│  Indexer Node   │                    │  Validator Pool │
│  sui-indexer    │                    │  (3-7 nodes)    │
│  + PostgreSQL   │                    │  sui-node       │
│                 │                    │  + consensus    │
└─────────────────┘                    └────────┬────────┘
                                                │
                                                ▼
                                       ┌─────────────────┐
                                       │  Shared Storage │
                                       │  (RocksDB / S3) │
                                       └─────────────────┘
```

#### 服务划分

**1. 共识集群 (Validator Pool)**
- 节点数: 3-7 个 (BFT 容错)
- 配置: 完整 sui-node + consensus
- 资源: 高 CPU, 高内存 (16GB+)
- 作用: 达成共识,签名区块

**2. RPC 节点池 (API Gateway)**
- 节点数: 3-10 个 (根据负载)
- 配置: sui-node (全节点模式, 无共识)
- 资源: 中等 CPU, 中等内存 (8GB)
- 作用: 处理 API 请求,无状态可水平扩展

**3. 索引服务 (Indexer)**
- 节点数: 1-2 个 (主备)
- 配置: sui-indexer-alt + PostgreSQL
- 资源: 高存储, 中等 CPU
- 作用: 历史数据查询

**4. 共享存储 (可选)**
- RocksDB 集群 或 S3
- 减少重复存储
- 加快状态同步

#### Docker Compose 示例

```yaml
# docker-compose.yml
version: '3.8'

services:
  # 验证者节点 1
  validator-1:
    image: mysten/sui-node:latest
    volumes:
      - ./configs/validator-1.yaml:/etc/sui/node.yaml
      - validator-1-data:/data
    ports:
      - "8081:8081"  # Consensus
    deploy:
      resources:
        limits:
          cpus: '8'
          memory: 16G

  # RPC 节点 1
  rpc-1:
    image: mysten/sui-node:latest
    volumes:
      - ./configs/rpc-1.yaml:/etc/sui/node.yaml
      - rpc-1-data:/data
    ports:
      - "9001:9000"  # JSON-RPC

  # RPC 节点 2
  rpc-2:
    image: mysten/sui-node:latest
    volumes:
      - ./configs/rpc-2.yaml:/etc/sui/node.yaml
      - rpc-2-data:/data
    ports:
      - "9002:9000"

  # 索引器
  indexer:
    image: mysten/sui-indexer-alt:latest
    environment:
      - RPC_URL=http://rpc-1:9000
      - DB_URL=postgresql://sui:password@postgres:5432/sui_indexer
    depends_on:
      - postgres
      - rpc-1

  # PostgreSQL
  postgres:
    image: postgres:15
    environment:
      - POSTGRES_DB=sui_indexer
      - POSTGRES_USER=sui
      - POSTGRES_PASSWORD=password
    volumes:
      - postgres-data:/var/lib/postgresql/data

  # 负载均衡
  haproxy:
    image: haproxy:2.8
    ports:
      - "9000:9000"  # 统一入口
    volumes:
      - ./haproxy.cfg:/usr/local/etc/haproxy/haproxy.cfg

volumes:
  validator-1-data:
  rpc-1-data:
  rpc-2-data:
  postgres-data:
```

#### Kubernetes 部署 (生产级)

```yaml
# k8s/validator-statefulset.yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: sui-validator
spec:
  serviceName: sui-validator
  replicas: 3  # 3 个验证者
  selector:
    matchLabels:
      app: sui-validator
  template:
    metadata:
      labels:
        app: sui-validator
    spec:
      containers:
      - name: sui-node
        image: mysten/sui-node:latest
        resources:
          requests:
            memory: "16Gi"
            cpu: "8"
          limits:
            memory: "32Gi"
            cpu: "16"
        volumeMounts:
        - name: data
          mountPath: /data
  volumeClaimTemplates:
  - metadata:
      name: data
    spec:
      accessModes: ["ReadWriteOnce"]
      storageClassName: "fast-ssd"
      resources:
        requests:
          storage: 1Ti

---
# k8s/rpc-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: sui-rpc
spec:
  replicas: 5  # 5 个 RPC 节点
  selector:
    matchLabels:
      app: sui-rpc
  template:
    metadata:
      labels:
        app: sui-rpc
    spec:
      containers:
      - name: sui-node
        image: mysten/sui-node:latest
        resources:
          requests:
            memory: "8Gi"
            cpu: "4"
        ports:
        - containerPort: 9000

---
# k8s/service.yaml
apiVersion: v1
kind: Service
metadata:
  name: sui-rpc
spec:
  selector:
    app: sui-rpc
  ports:
  - protocol: TCP
    port: 9000
    targetPort: 9000
  type: LoadBalancer
```

#### 优点
- ✅ **水平扩展**: RPC 节点可动态增减
- ✅ **故障隔离**: 单节点故障不影响整体
- ✅ **高可用**: 多活架构,无单点故障
- ✅ **资源优化**: 按需分配 CPU/内存
- ✅ **易于维护**: 滚动更新,无停机

#### 缺点
- ⚠️ **运维复杂**: 需要 DevOps 团队
- ⚠️ **成本较高**: 多节点部署费用
- ⚠️ **网络开销**: 跨服务通信延迟

#### 适用场景
- 高性能 DEX 生产环境 (>50,000 用户)
- 需要 99.99% SLA
- 有 DevOps 团队
- 云原生基础设施 (K8s)

---

### 方案对比总结

| 维度 | 方案 A (配置) | 方案 B (源码) | 方案 C (微服务) |
|-----|--------------|--------------|----------------|
| **实施难度** | ⭐ (简单) | ⭐⭐⭐⭐ (困难) | ⭐⭐⭐ (中等) |
| **维护成本** | ⭐ (低) | ⭐⭐⭐⭐ (高) | ⭐⭐⭐ (中) |
| **二进制大小** | 500MB | 300MB (-40%) | 500MB (每节点) |
| **内存占用** | 6-8GB | 4-6GB (-30%) | 8GB (RPC), 16GB (Validator) |
| **升级兼容性** | ✅ 完美 | ❌ 困难 | ✅ 良好 |
| **水平扩展** | ❌ 单节点 | ❌ 单节点 | ✅ 支持 |
| **高可用** | ❌ 单点 | ❌ 单点 | ✅ 多活 |
| **性能** | 良好 | 优秀 | 优秀 |
| **成本** | 低 | 中 | 高 |
| **适用规模** | <10K 用户 | >100K 用户 | >50K 用户 |

**推荐路线图**:
```
开发阶段 → 方案 A (配置裁剪)
  ↓ 用户增长
小规模生产 → 方案 A + 外部索引器
  ↓ 用户增长
中等规模 → 方案 C (微服务)
  ↓ 用户增长
大规模优化 → 方案 B (可选,需专职团队)
```

---

## DeepBook 源码分析

### 核心模块详解

#### 1. clob_v2.move (订单簿核心)

**文件**: `sui-framework/packages/deepbook/sources/clob_v2.move`

**核心数据结构**:
```move
/// 订单簿池 (每个交易对一个)
struct Pool<BaseAsset, QuoteAsset> has key {
    id: UID,

    // 买单队列 (价格从高到低)
    bids: CritbitTree<TickLevel>,

    // 卖单队列 (价格从低到高)
    asks: CritbitTree<TickLevel>,

    // 下一个订单ID
    next_order_id: u64,

    // 用户账户余额
    usr_open_orders: Table<address, LinkedTable<u64, Order>>,

    // 手续费配置
    taker_fee_rate: u64,  // 吃单手续费
    maker_rebate_rate: u64,  // 挂单返佣

    // 最新成交价
    last_trade_price: u64,
}

/// 价格级别 (同一价格的所有订单)
struct TickLevel has store {
    price: u64,
    open_orders: LinkedTable<u64, Order>,  // 订单队列 (FIFO)
}

/// 订单
struct Order has store, drop {
    order_id: u64,
    client_order_id: u64,
    price: u64,
    original_quantity: u64,
    quantity: u64,  // 剩余数量
    is_bid: bool,   // true = 买单, false = 卖单
    owner: address,
    expire_timestamp: u64,  // 过期时间
}
```

**关键函数 - 下单**:
```move
/// 下限价单
public entry fun place_limit_order<BaseAsset, QuoteAsset>(
    pool: &mut Pool<BaseAsset, QuoteAsset>,
    client_order_id: u64,
    price: u64,
    quantity: u64,
    side: u8,  // 0 = BUY, 1 = SELL
    expire_timestamp: u64,
    restriction: u8,  // 0 = NO_RESTRICTION, 1 = IMMEDIATE_OR_CANCEL, 2 = FILL_OR_KILL, 3 = POST_ONLY
    clock: &Clock,
    account_cap: &AccountCap,
    ctx: &mut TxContext
) {
    // 1. 验证过期时间
    assert!(expire_timestamp > clock::timestamp_ms(clock), EInvalidExpireTimestamp);

    // 2. 分配订单ID
    let order_id = pool.next_order_id;
    pool.next_order_id = order_id + 1;

    // 3. 创建订单
    let order = Order {
        order_id,
        client_order_id,
        price,
        original_quantity: quantity,
        quantity,
        is_bid: (side == 0),
        owner: account_cap_owner(account_cap),
        expire_timestamp,
    };

    // 4. 尝试撮合
    let (base_quantity_filled, quote_quantity_filled, is_fully_filled) =
        match_order(pool, &mut order, clock);

    // 5. 如果未完全成交且不是 IOC/FOK,加入订单簿
    if (!is_fully_filled && restriction != IMMEDIATE_OR_CANCEL && restriction != FILL_OR_KILL) {
        if (order.is_bid) {
            insert_bid(pool, order);
        } else {
            insert_ask(pool, order);
        }
    }

    // 6. 发射事件
    event::emit(OrderPlaced {
        pool_id: object::id(pool),
        order_id,
        client_order_id,
        price,
        quantity: order.quantity,
        is_bid: order.is_bid,
    });
}
```

**关键函数 - 撮合**:
```move
/// 订单撮合逻辑
fun match_order<BaseAsset, QuoteAsset>(
    pool: &mut Pool<BaseAsset, QuoteAsset>,
    taker_order: &mut Order,
    clock: &Clock,
): (u64, u64, bool) {
    let base_filled = 0u64;
    let quote_filled = 0u64;

    // 选择对手盘 (买单匹配卖单,卖单匹配买单)
    let book = if (taker_order.is_bid) { &mut pool.asks } else { &mut pool.bids };

    // 遍历价格级别 (从最优价格开始)
    while (taker_order.quantity > 0) {
        // 获取最优价格
        let (best_price, tick_level) = critbit::min(book);

        // 检查价格是否匹配
        if (taker_order.is_bid) {
            // 买单: taker 价格 >= maker 价格
            if (taker_order.price < best_price) break;
        } else {
            // 卖单: taker 价格 <= maker 价格
            if (taker_order.price > best_price) break;
        }

        // 遍历该价格级别的所有订单 (FIFO)
        let orders = &mut tick_level.open_orders;
        while (taker_order.quantity > 0 && !linked_table::is_empty(orders)) {
            let (maker_order_id, maker_order) = linked_table::front(orders);

            // 检查订单是否过期
            if (maker_order.expire_timestamp < clock::timestamp_ms(clock)) {
                linked_table::pop_front(orders);  // 移除过期订单
                continue;
            }

            // 计算成交数量 (取两者较小值)
            let match_quantity = min(taker_order.quantity, maker_order.quantity);
            let match_price = maker_order.price;  // 使用 maker 价格

            // 更新订单
            taker_order.quantity = taker_order.quantity - match_quantity;
            maker_order.quantity = maker_order.quantity - match_quantity;

            // 累计成交量
            base_filled = base_filled + match_quantity;
            quote_filled = quote_filled + match_quantity * match_price;

            // 如果 maker 订单完全成交,移除
            if (maker_order.quantity == 0) {
                linked_table::pop_front(orders);
            }

            // 发射成交事件
            event::emit(OrderFilled {
                pool_id: object::id(pool),
                taker_order_id: taker_order.order_id,
                maker_order_id,
                price: match_price,
                quantity: match_quantity,
            });
        }

        // 如果该价格级别无订单,移除
        if (linked_table::is_empty(orders)) {
            critbit::remove(book, best_price);
        }
    }

    let is_fully_filled = (taker_order.quantity == 0);
    (base_filled, quote_filled, is_fully_filled)
}
```

**Critbit Tree 索引**:
- O(log n) 插入、删除、查找
- 自动按价格排序
- 买单: 从高到低 (Max Heap)
- 卖单: 从低到高 (Min Heap)

---

#### 2. custodian_v2.move (资金托管)

**文件**: `sui-framework/packages/deepbook/sources/custodian_v2.move`

**核心数据结构**:
```move
/// 账户能力 (Account Capability)
struct AccountCap has key, store {
    id: UID,
    owner: address,
}

/// 用户账户 (托管资金)
struct Account<Asset> has store {
    available_balance: Balance<Asset>,  // 可用余额
    locked_balance: Balance<Asset>,     // 锁定余额 (未成交订单)
}
```

**关键函数**:
```move
/// 充值
public entry fun deposit<Asset>(
    account_cap: &AccountCap,
    coin: Coin<Asset>,
    account: &mut Account<Asset>,
) {
    let amount = coin::value(&coin);
    balance::join(&mut account.available_balance, coin::into_balance(coin));
}

/// 提现
public entry fun withdraw<Asset>(
    account_cap: &AccountCap,
    amount: u64,
    account: &mut Account<Asset>,
    ctx: &mut TxContext
): Coin<Asset> {
    assert!(balance::value(&account.available_balance) >= amount, EInsufficientBalance);
    coin::from_balance(balance::split(&mut account.available_balance, amount), ctx)
}

/// 下单时锁定资金
fun lock_balance<Asset>(
    account: &mut Account<Asset>,
    amount: u64,
) {
    let locked = balance::split(&mut account.available_balance, amount);
    balance::join(&mut account.locked_balance, locked);
}

/// 成交后释放资金
fun unlock_balance<Asset>(
    account: &mut Account<Asset>,
    amount: u64,
) {
    let unlocked = balance::split(&mut account.locked_balance, amount);
    balance::join(&mut account.available_balance, unlocked);
}
```

---

#### 3. critbit.move (价格索引)

**Critbit Tree 原理**:
- 二叉搜索树的变种
- 每个内部节点表示一个关键位 (Critical Bit)
- 查找、插入、删除: O(log n)
- 自动排序

**为什么不用 BTreeMap?**
- Move 标准库的 `Table` 无序
- `LinkedTable` 是链表,查找 O(n)
- Critbit Tree 提供 O(log n) 有序查找

**数据结构** (简化版):
```move
struct CritbitTree<V> has store {
    root: u64,  // 根节点索引
    internal_nodes: vector<InternalNode>,
    leaves: Table<u64, Leaf<V>>,  // key → value
    min_leaf: u64,  // 最小key (缓存)
    max_leaf: u64,  // 最大key (缓存)
}

struct InternalNode has store {
    mask: u64,       // 关键位掩码
    left_child: u64,
    right_child: u64,
}

struct Leaf<V> has store {
    key: u64,
    value: V,
    parent: u64,
}
```

---

### DeepBook 性能分析

#### 订单簿操作复杂度

| 操作 | 时间复杂度 | 说明 |
|-----|-----------|------|
| 下单 (无撮合) | O(log n) | Critbit Tree 插入 |
| 下单 (完全成交) | O(m) | m = 匹配的订单数 |
| 取消订单 | O(log n) | Critbit Tree 删除 |
| 查询最优价格 | O(1) | Critbit 缓存 min/max |
| 查询用户订单 | O(1) | Table 直接索引 |

#### Gas 消耗估算

| 操作 | Gas 消耗 | 说明 |
|-----|---------|------|
| 下单 (无成交) | ~10,000 | 仅插入订单簿 |
| 下单 (1笔成交) | ~15,000 | 撮合 + 资金转移 |
| 下单 (5笔成交) | ~40,000 | 多笔撮合 |
| 取消订单 | ~8,000 | 删除 + 解锁资金 |
| 查询订单 | 0 (读操作) | 不消耗 Gas |

#### 性能瓶颈

1. **共享对象锁竞争**:
   - 同一订单簿串行执行
   - TPS 受共识延迟限制 (~2,500 TPS/Pool)

2. **大订单撮合**:
   - 需要遍历多个价格级别
   - Gas 消耗随成交笔数线性增长

3. **过期订单清理**:
   - 撮合时检查过期,增加开销
   - 建议使用链下 Keeper 批量清理

---

## DEX 开发实施路径

### 阶段 1: 本地开发 (1-2 周)

#### 目标
- 搭建本地测试环境
- 熟悉 DeepBook API
- 实现基础交易功能

#### 步骤

**1. 安装 Sui CLI**:
```bash
cargo install --locked --git https://github.com/MystenLabs/sui.git --branch devnet sui
```

**2. 启动本地测试网**:
```bash
sui-test-validator
```

**3. 部署 DeepBook**:
```bash
cd sui-framework/packages/deepbook
sui move build
sui client publish --gas-budget 100000000
# 记录 Package ID
```

**4. 创建交易对**:
```rust
use sui_sdk::SuiClient;

let sui = SuiClientBuilder::default().build_localnet().await?;

// 创建 BTC/USDC 池
let tx = TransactionBuilder::new()
    .move_call(
        deepbook_package_id,
        "clob_v2",
        "create_pool",
        type_args: vec![btc_coin_type, usdc_coin_type],
        call_args: vec![
            tick_size,  // 价格精度 (如 0.01 USDC)
            lot_size,   // 数量精度 (如 0.0001 BTC)
            taker_fee_rate,  // 0.1% = 10000 (basis points)
            maker_rebate_rate,  // 0.05% = 5000
        ],
        gas_budget: 10000000,
    )
    .build();
```

**5. 测试下单**:
```rust
// 下买单: 以 50,000 USDC 买 1 BTC
let tx = TransactionBuilder::new()
    .move_call(
        deepbook_package_id,
        "clob_v2",
        "place_limit_order",
        type_args: vec![btc_coin_type, usdc_coin_type],
        call_args: vec![
            pool_id,
            client_order_id,
            price: 50_000_000_000,  // 50,000 USDC (scaled by 10^6)
            quantity: 100_000_000,  // 1 BTC (scaled by 10^8)
            side: 0,  // BUY
            expiration: u64::MAX,  // 永不过期
            restriction: 0,  // NO_RESTRICTION
            clock,
            account_cap,
        ],
        gas_budget: 20000000,
    )
    .build();
```

---

### 阶段 2: 单节点优化 (1 周)

#### 目标
- 部署专用节点
- 集成外部索引器
- 性能基准测试

#### 步骤

**1. 部署 sui-node** (方案 A):
```bash
# 下载官方二进制
wget https://github.com/MystenLabs/sui/releases/download/mainnet-v1.64.0/sui-node

# 创建配置
cat > dex-node.yaml <<EOF
db_path: "/data/sui-node"
network_address: "/ip4/0.0.0.0/tcp/8080"
json_rpc_address: "0.0.0.0:9000"
enable_index_processing: false
consensus_config: null  # 全节点
EOF

# 启动节点
./sui-node --config-path dex-node.yaml
```

**2. 部署索引器**:
```bash
# 安装 PostgreSQL
docker run -d \
  -e POSTGRES_USER=sui \
  -e POSTGRES_PASSWORD=password \
  -e POSTGRES_DB=sui_indexer \
  -p 5432:5432 \
  postgres:15

# 运行索引器
sui-indexer-alt \
  --rpc-url http://localhost:9000 \
  --db-url postgresql://sui:password@localhost:5432/sui_indexer \
  --reset-database
```

**3. 性能基准测试**:
```bash
# 使用 sui-benchmark
sui-benchmark \
  --target-qps 100 \
  --num-workers 10 \
  --duration 60s \
  --transaction-type move-call \
  --move-package-id <deepbook_package> \
  --move-module clob_v2 \
  --move-function place_limit_order
```

---

### 阶段 3: 微服务架构 (2-3 周)

#### 目标
- 3-7 个验证者节点
- RPC 节点池 (3+ 节点)
- 独立索引服务
- 负载均衡和监控

#### 架构部署

**1. 使用 Docker Compose** (见上面方案 C)

**2. 或使用 Kubernetes**:
```bash
# 部署验证者 StatefulSet
kubectl apply -f k8s/validator-statefulset.yaml

# 部署 RPC Deployment
kubectl apply -f k8s/rpc-deployment.yaml

# 部署负载均衡
kubectl apply -f k8s/service.yaml

# 部署索引器
kubectl apply -f k8s/indexer-deployment.yaml
```

**3. 监控和告警**:
```bash
# Prometheus + Grafana
helm install prometheus prometheus-community/kube-prometheus-stack

# 配置 Sui 指标抓取
kubectl apply -f k8s/servicemonitor.yaml
```

---

### 阶段 4: 深度优化 (可选, 2-4 周)

#### 目标
- 考虑方案 B (源码裁剪)
- 自定义撮合引擎优化
- 链下订单簿 (如需极致性能)

#### 高级优化方向

**1. 自定义 Move 合约优化**:
- 批量下单 (减少交易数)
- 订单簿分片 (多个 Pool 对象)
- Gas 优化 (减少存储操作)

**2. 链下撮合 + 链上结算**:
```
链下撮合引擎 (Rust):
  - 内存订单簿 (超低延迟)
  - 定期批量结算到链上

链上结算合约:
  - 验证链下撮合结果
  - 批量更新账户余额
  - 发射成交事件
```

**3. Layer 2 方案**:
- 使用 Sui 作为 DA 层
- 自定义执行层 (专用 DEX 链)

---

## 性能优化建议

### 1. 共识层优化

**调整 Mysticeti 参数**:
```yaml
# consensus-config.yaml
narwhal_config:
  batch_size: 500000  # 增大批次 (提高吞吐)
  max_pending_transactions: 100000

  # 减少延迟 (牺牲吞吐)
  batch_size: 100000
  max_batch_delay_ms: 50  # 更快批次生成
```

**权衡**:
- 大批次: 高吞吐,高延迟
- 小批次: 低延迟,低吞吐
- DEX 建议: 中等批次 (200,000-300,000)

---

### 2. 执行层优化

**并行交易对**:
```move
// 不同交易对的订单簿是独立对象,可并行
Pool<BTC, USDC>  // 可与下面并行
Pool<ETH, USDC>  // 可与上面并行
Pool<SOL, USDC>  // 可与上面并行
```

**订单簿分片** (高级):
```move
// 将订单簿分为多个价格区间
Pool<BTC, USDC, Range1>  // 价格 0-50,000
Pool<BTC, USDC, Range2>  // 价格 50,000-100,000

// 不同区间可并行处理
```

---

### 3. 存储层优化

**RocksDB 调优**:
```toml
# rocksdb.conf
[default]
# 写缓冲
write_buffer_size = 256MB
max_write_buffer_number = 6

# 块缓存 (读性能)
block_cache_size = 8GB

# 压缩
compression = "lz4"
```

**SSD 优化**:
- 使用 NVMe SSD (>500K IOPS)
- 启用 TRIM
- 定期碎片整理

---

### 4. 网络层优化

**QUIC 参数调优**:
```yaml
# mysten-network config
quic_config:
  max_idle_timeout_ms: 30000
  keep_alive_interval_ms: 5000
  max_concurrent_bidi_streams: 10000
```

**CDN 加速** (RPC 节点):
- Cloudflare / AWS CloudFront
- 地理分布式部署
- 减少客户端延迟

---

### 5. 应用层优化

**客户端批量操作**:
```rust
// 批量下单 (减少签名验证开销)
let tx = TransactionBuilder::new()
    .move_call(...)  // 订单 1
    .move_call(...)  // 订单 2
    .move_call(...)  // 订单 3
    .build();
```

**WebSocket 事件推送**:
```rust
// 实时推送成交,避免轮询
let mut event_stream = sui_client
    .event_api()
    .subscribe_event(EventFilter::MoveEventType(
        "deepbook::clob_v2::OrderFilled".to_string()
    ))
    .await?;
```

---

### 性能指标预期

| 场景 | 预期 TPS | 延迟 | 瓶颈 |
|-----|---------|------|------|
| 单交易对 (无冲突) | 200,000+ | ~200ms | 网络 |
| 单交易对 (高频交易) | 2,000-5,000 | ~400ms | 共识 |
| 多交易对 (10个) | 20,000-50,000 | ~400ms | 执行 |
| 大订单撮合 (100笔) | 100-500 | ~1s | Gas |

---

**返回**: [架构文档首页](README.md) | **相关**: [关键模块详解](02-KEY-MODULES.md)
