# Sui 模块完整索引

> **文档用途**: 全部 134 个 Rust crates 的快速查询索引
>
> **使用方法**: 使用 Ctrl+F / Cmd+F 搜索模块名称

---

## 📊 模块统计

| 类别 | 数量 | 占比 |
|-----|------|------|
| **核心协议层** | 28 | 21% |
| **服务层** | 18 | 13% |
| **基础设施层** | 35 | 26% |
| **应用层** | 12 | 9% |
| **测试工具** | 22 | 16% |
| **索引器系列** | 13 | 10% |
| **其他工具** | 6 | 5% |
| **总计** | **134** | **100%** |

---

## 目录

- [按架构层分类](#按架构层分类)
  - [基础设施层 (35个)](#基础设施层-35个)
  - [核心协议层 (28个)](#核心协议层-28个)
  - [服务层 (18个)](#服务层-18个)
  - [应用层 (12个)](#应用层-12个)
- [测试与工具 (41个)](#测试与工具-41个)
- [按字母排序完整列表](#按字母排序完整列表)

---

## 按架构层分类

### 基础设施层 (35个)

#### 类型系统 (2个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-types** | crates/sui-types/ | 核心类型定义 (ObjectID, TransactionDigest等) | ⭐⭐⭐⭐⭐ |
| sui-json | crates/sui-json/ | JSON 序列化/反序列化 | ⭐⭐⭐ |

#### 网络通信 (5个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **mysten-network** | crates/mysten-network/ | P2P 网络抽象层 | ⭐⭐⭐⭐⭐ |
| sui-network | crates/sui-network/ | Sui 特定网络协议 | ⭐⭐⭐⭐ |
| sui-tls | crates/sui-tls/ | TLS 证书管理 | ⭐⭐⭐ |
| anemo-benchmark | crates/anemo-benchmark/ | 网络性能基准测试 | ⭐⭐ |
| sui-http | crates/sui-http/ | HTTP 客户端工具 | ⭐⭐ |

#### 存储系统 (6个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **typed-store** | crates/typed-store/ | RocksDB 类型安全封装 | ⭐⭐⭐⭐⭐ |
| **sui-storage** | crates/sui-storage/ | 存储抽象层 (缓存, 对象存储) | ⭐⭐⭐⭐⭐ |
| sui-data-store | crates/sui-data-store/ | 数据存储工具 | ⭐⭐⭐ |
| typed-store-derive | crates/typed-store-derive/ | typed-store 宏派生 | ⭐⭐⭐ |
| typed-store-error | crates/typed-store-error/ | typed-store 错误类型 | ⭐⭐ |
| typed-store-workspace-hack | crates/typed-store-workspace-hack/ | Workspace 优化 hack | ⭐ |

#### 密码学 (2个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **shared-crypto** | crates/shared-crypto/ | 共享加密工具 (Ed25519, BLS) | ⭐⭐⭐⭐⭐ |
| sui-keys | crates/sui-keys/ | 密钥管理 | ⭐⭐⭐ |

#### 配置管理 (4个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-config** | crates/sui-config/ | 节点配置管理 | ⭐⭐⭐⭐⭐ |
| **sui-protocol-config** | crates/sui-protocol-config/ | 协议参数配置 (版本门控) | ⭐⭐⭐⭐⭐ |
| sui-default-config | crates/sui-default-config/ | 默认配置生成 | ⭐⭐⭐ |
| sui-protocol-config-macros | crates/sui-protocol-config-macros/ | 协议配置宏 | ⭐⭐ |

#### 监控与日志 (5个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **mysten-metrics** | crates/mysten-metrics/ | Prometheus 指标收集 | ⭐⭐⭐⭐ |
| telemetry-subscribers | crates/telemetry-subscribers/ | 日志订阅器 | ⭐⭐⭐ |
| sui-telemetry | crates/sui-telemetry/ | 遥测上报 | ⭐⭐ |
| sui-metrics-push-client | crates/sui-metrics-push-client/ | 指标推送客户端 | ⭐⭐ |
| prometheus-closure-metric | crates/prometheus-closure-metric/ | Prometheus 闭包指标 | ⭐ |

#### 通用工具 (11个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **mysten-common** | crates/mysten-common/ | 通用工具函数 | ⭐⭐⭐⭐ |
| mysten-service | crates/mysten-service/ | 服务基础设施 | ⭐⭐⭐ |
| sui-macros | crates/sui-macros/ | Sui 宏定义 | ⭐⭐⭐ |
| sui-proc-macros | crates/sui-proc-macros/ | 过程宏 | ⭐⭐⭐ |
| sui-enum-compat-util | crates/sui-enum-compat-util/ | 枚举兼容性工具 | ⭐⭐ |
| sui-field-count | crates/sui-field-count/ | 字段计数宏 | ⭐⭐ |
| sui-field-count-derive | crates/sui-field-count-derive/ | 字段计数派生宏 | ⭐⭐ |
| sui-futures | crates/sui-futures/ | Future 工具 | ⭐⭐ |
| mysten-service-boilerplate | crates/mysten-service-boilerplate/ | 服务样板代码 | ⭐ |
| bin-version | crates/bin-version/ | 二进制版本管理 | ⭐ |
| x | crates/x/ | Workspace 工具 | ⭐ |

---

### 核心协议层 (28个)

#### 共识机制 (4个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **consensus-core** | consensus/core/ | Mysticeti 共识核心实现 | ⭐⭐⭐⭐⭐ |
| **consensus-config** | consensus/config/ | 共识配置 | ⭐⭐⭐⭐ |
| **consensus-types** | consensus/types/ | 共识类型定义 | ⭐⭐⭐⭐ |
| consensus-simtests | consensus/simtests/ | 共识模拟测试 | ⭐⭐ |

#### 核心逻辑 (3个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-core** | crates/sui-core/ | 验证者核心逻辑 (Authority, 执行调度) | ⭐⭐⭐⭐⭐ |
| sui-authority-aggregation | crates/sui-authority-aggregation/ | 全节点验证者聚合 | ⭐⭐⭐⭐ |
| sui-transaction-checks | crates/sui-transaction-checks/ | 交易合法性检查 | ⭐⭐⭐⭐ |

#### 执行层 (7个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-execution** | sui-execution/latest/ | Move VM 执行层 (多版本) | ⭐⭐⭐⭐⭐ |
| **sui-adapter** | sui-execution/latest/sui-adapter/ | Move 到 Sui 适配器 | ⭐⭐⭐⭐⭐ |
| **sui-verifier** | sui-execution/latest/sui-verifier/ | Move 字节码验证器 | ⭐⭐⭐⭐ |
| **sui-move-natives** | sui-execution/latest/sui-move-natives/ | Sui 原生 Move 函数 | ⭐⭐⭐⭐ |
| sui-adapter-transactional-tests | crates/sui-adapter-transactional-tests/ | 适配器事务测试 | ⭐⭐ |
| sui-verifier-transactional-tests | crates/sui-verifier-transactional-tests/ | 验证器事务测试 | ⭐⭐ |
| sui-transactional-test-runner | crates/sui-transactional-test-runner/ | 事务测试运行器 | ⭐⭐ |

#### Move 框架 (4个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-framework** | crates/sui-framework/ | Sui Move 标准库和系统包 | ⭐⭐⭐⭐⭐ |
| sui-framework-snapshot | crates/sui-framework-snapshot/ | 框架快照 (协议升级) | ⭐⭐⭐⭐ |
| sui-framework-tests | crates/sui-framework-tests/ | 框架测试 | ⭐⭐ |
| sui-genesis-builder | crates/sui-genesis-builder/ | 创世区块生成器 | ⭐⭐⭐ |

#### 包管理 (4个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| sui-package-management | crates/sui-package-management/ | Move 包管理 | ⭐⭐⭐⭐ |
| sui-package-resolver | crates/sui-package-resolver/ | 包依赖解析 | ⭐⭐⭐ |
| sui-package-alt | crates/sui-package-alt/ | 替代包管理实现 | ⭐⭐ |
| sui-package-dump | crates/sui-package-dump/ | 包导出工具 | ⭐⭐ |

#### 数据摄取 (3个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-data-ingestion-core** | crates/sui-data-ingestion-core/ | 数据摄取核心 | ⭐⭐⭐⭐ |
| sui-data-ingestion | crates/sui-data-ingestion/ | 数据摄取服务 | ⭐⭐⭐ |
| sui-checkpoint-blob-indexer | crates/sui-checkpoint-blob-indexer/ | Checkpoint Blob 索引 | ⭐⭐⭐ |

#### 其他协议组件 (3个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| sui-display | crates/sui-display/ | 对象显示标准 | ⭐⭐⭐ |
| sui-cost | crates/sui-cost/ | Gas 成本计算 | ⭐⭐⭐ |
| sui-source-validation | crates/sui-source-validation/ | 源码验证 | ⭐⭐ |

---

### 服务层 (18个)

#### 节点服务 (2个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-node** | crates/sui-node/ | 验证者/全节点主程序 | ⭐⭐⭐⭐⭐ |
| sui-proxy | crates/sui-proxy/ | 代理服务 | ⭐⭐ |

#### RPC 服务 (6个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-json-rpc** | crates/sui-json-rpc/ | JSON-RPC API 服务器 | ⭐⭐⭐⭐⭐ |
| **sui-json-rpc-api** | crates/sui-json-rpc-api/ | JSON-RPC API 定义 | ⭐⭐⭐⭐ |
| **sui-json-rpc-types** | crates/sui-json-rpc-types/ | JSON-RPC 类型转换 | ⭐⭐⭐⭐ |
| **sui-graphql-rpc** | crates/sui-graphql-rpc/ | GraphQL API 服务器 | ⭐⭐⭐⭐ |
| sui-graphql-rpc-client | crates/sui-graphql-rpc-client/ | GraphQL 客户端 | ⭐⭐⭐ |
| sui-graphql-rpc-headers | crates/sui-graphql-rpc-headers/ | GraphQL 请求头处理 | ⭐⭐ |

#### 索引器 (10个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-indexer-alt** | crates/sui-indexer-alt/ | 新一代索引器主程序 | ⭐⭐⭐⭐⭐ |
| sui-indexer-alt-framework | crates/sui-indexer-alt-framework/ | 索引器框架 | ⭐⭐⭐⭐ |
| sui-indexer-alt-schema | crates/sui-indexer-alt-schema/ | 数据库 Schema | ⭐⭐⭐⭐ |
| sui-indexer-alt-consistent-store | crates/sui-indexer-alt-consistent-store/ | 一致性存储 | ⭐⭐⭐ |
| sui-indexer-alt-graphql | crates/sui-indexer-alt-graphql/ | 索引器 GraphQL 接口 | ⭐⭐⭐ |
| sui-indexer-alt-jsonrpc | crates/sui-indexer-alt-jsonrpc/ | 索引器 JSON-RPC 接口 | ⭐⭐⭐ |
| sui-indexer-alt-object-store | crates/sui-indexer-alt-object-store/ | 对象存储索引 | ⭐⭐⭐ |
| sui-indexer-alt-reader | crates/sui-indexer-alt-reader/ | 索引器读取器 | ⭐⭐⭐ |
| sui-indexer | crates/sui-indexer/ | 传统索引器 | ⭐⭐⭐ |
| sui-analytics-indexer | crates/sui-analytics-indexer/ | 分析索引器 | ⭐⭐ |

---

### 应用层 (12个)

#### SDK (2个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-sdk** | crates/sui-sdk/ | Rust SDK | ⭐⭐⭐⭐⭐ |
| sui-transaction-builder | crates/sui-transaction-builder/ | 交易构建器 | ⭐⭐⭐⭐ |

#### Move 开发工具 (3个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-move** | crates/sui-move/ | Move 开发 CLI 工具 | ⭐⭐⭐⭐ |
| sui-move-build | crates/sui-move-build/ | Move 编译工具 | ⭐⭐⭐⭐ |
| sui-move-lsp | crates/sui-move-lsp/ | Move LSP 服务器 | ⭐⭐⭐ |

#### 特定功能 (7个)

| Crate | 路径 | 职责 | 重要性 |
|-------|------|------|--------|
| **sui-bridge** | crates/sui-bridge/ | 跨链桥实现 | ⭐⭐⭐⭐ |
| sui-bridge-cli | crates/sui-bridge-cli/ | 跨链桥 CLI | ⭐⭐⭐ |
| sui-bridge-indexer | crates/sui-bridge-indexer/ | 跨链桥索引器 | ⭐⭐⭐ |
| sui-faucet | crates/sui-faucet/ | 测试网水龙头 | ⭐⭐⭐ |
| sui-name-service | crates/sui-name-service/ | 域名服务 | ⭐⭐⭐ |
| sui-oracle | crates/sui-oracle/ | 预言机 | ⭐⭐⭐ |
| sui-rosetta | crates/sui-rosetta/ | Rosetta API 实现 | ⭐⭐ |

---

## 测试与工具 (41个)

### 测试工具 (22个)

| Crate | 路径 | 职责 |
|-------|------|------|
| sui-benchmark | crates/sui-benchmark/ | 性能基准测试 |
| sui-cluster-test | crates/sui-cluster-test/ | 集群测试 |
| sui-e2e-tests | crates/sui-e2e-tests/ | 端到端测试 |
| sui-graphql-e2e-tests | crates/sui-graphql-e2e-tests/ | GraphQL E2E 测试 |
| sui-json-rpc-tests | crates/sui-json-rpc-tests/ | JSON-RPC 测试 |
| sui-rpc-benchmark | crates/sui-rpc-benchmark/ | RPC 基准测试 |
| sui-rpc-loadgen | crates/sui-rpc-loadgen/ | RPC 负载生成器 |
| sui-simulator | crates/sui-simulator/ | 网络模拟器 |
| sui-single-node-benchmark | crates/sui-single-node-benchmark/ | 单节点基准测试 |
| sui-swarm | crates/sui-swarm/ | 测试网络集群 |
| sui-swarm-config | crates/sui-swarm-config/ | Swarm 配置 |
| sui-test-transaction-builder | crates/sui-test-transaction-builder/ | 测试交易构建器 |
| sui-test-validator | crates/sui-test-validator/ | 测试验证者 |
| test-cluster | crates/test-cluster/ | 测试集群 |
| transaction-fuzzer | crates/transaction-fuzzer/ | 交易模糊测试 |
| sui-synthetic-ingestion | crates/sui-synthetic-ingestion/ | 合成数据摄取 |
| simulacrum | crates/simulacrum/ | 模拟环境 |
| sui-indexer-alt-e2e-tests | crates/sui-indexer-alt-e2e-tests/ | 索引器 E2E 测试 |
| sui-metric-checker | crates/sui-metric-checker/ | 指标检查器 |
| sui-upgrade-compatibility-transactional-tests | crates/sui-upgrade-compatibility-transactional-tests/ | 升级兼容性测试 |
| sui-indexer-alt-consistent-api | crates/sui-indexer-alt-consistent-api/ | 索引器一致性 API |
| sui-indexer-alt-restorer | crates/sui-indexer-alt-restorer/ | 索引器恢复工具 |

### 工具与辅助 (19个)

| Crate | 路径 | 职责 |
|-------|------|------|
| **sui** | crates/sui/ | Sui CLI 主程序 |
| **sui-tool** | crates/sui-tool/ | 调试和管理工具 |
| sui-replay | crates/sui-replay/ | 交易重放工具 |
| sui-replay-2 | crates/sui-replay-2/ | 交易重放工具 v2 |
| sui-snapshot | crates/sui-snapshot/ | 快照服务 |
| sui-aws-orchestrator | crates/sui-aws-orchestrator/ | AWS 编排工具 |
| sui-security-watchdog | crates/sui-security-watchdog/ | 安全监视器 |
| sui-bridge-watchdog | crates/sui-bridge-watchdog/ | 跨链桥监视器 |
| sui-deepbook-indexer | crates/sui-deepbook-indexer/ | DeepBook 索引器 |
| suins-indexer | crates/suins-indexer/ | SuiNS 域名索引器 |
| sui-open-rpc | crates/sui-open-rpc/ | OpenRPC 规范生成 |
| sui-open-rpc-macros | crates/sui-open-rpc-macros/ | OpenRPC 宏 |
| sui-rpc-api | crates/sui-rpc-api/ | 新 RPC API 框架 |
| sui-rpc-resolver | crates/sui-rpc-resolver/ | RPC 解析器 |
| sui-kv-rpc | crates/sui-kv-rpc/ | KV RPC 服务 |
| sui-kvstore | crates/sui-kvstore/ | KV 存储 |
| sui-pg-db | crates/sui-pg-db/ | PostgreSQL 数据库工具 |
| sui-sql-macro | crates/sui-sql-macro/ | SQL 宏 |
| sui-light-client | crates/sui-light-client/ | 轻客户端 |

### 专项工具 (不常用)

| Crate | 路径 | 职责 |
|-------|------|------|
| sui-axelar-cgp | crates/sui-axelar-cgp/ | Axelar 跨链网关 |
| sui-bridge-schema | crates/sui-bridge-schema/ | 跨链桥数据库 Schema |
| sui-bridge-indexer-alt | crates/sui-bridge-indexer-alt/ | 跨链桥索引器 Alt |
| sui-analytics-indexer-derive | crates/sui-analytics-indexer-derive/ | 分析索引器宏 |
| sui-indexer-builder | crates/sui-indexer-builder/ | 索引器构建器 |
| sui-indexer-alt-metrics | crates/sui-indexer-alt-metrics/ | 索引器指标 |
| sui-indexer-alt-framework-store-traits | crates/sui-indexer-alt-framework-store-traits/ | 索引器存储 Traits |
| sui-surfer | crates/sui-surfer/ | Sui Surfer 工具 |

---

## 按字母排序完整列表

| # | Crate | 层级 | 类别 | 重要性 |
|---|-------|------|------|--------|
| 1 | anemo-benchmark | 基础设施层 | 网络 | ⭐⭐ |
| 2 | bin-version | 基础设施层 | 工具 | ⭐ |
| 3 | consensus-config | 核心协议层 | 共识 | ⭐⭐⭐⭐ |
| 4 | consensus-core | 核心协议层 | 共识 | ⭐⭐⭐⭐⭐ |
| 5 | consensus-simtests | 测试工具 | 共识测试 | ⭐⭐ |
| 6 | consensus-types | 核心协议层 | 共识 | ⭐⭐⭐⭐ |
| 7 | mysten-common | 基础设施层 | 工具 | ⭐⭐⭐⭐ |
| 8 | mysten-metrics | 基础设施层 | 监控 | ⭐⭐⭐⭐ |
| 9 | mysten-network | 基础设施层 | 网络 | ⭐⭐⭐⭐⭐ |
| 10 | mysten-service | 基础设施层 | 工具 | ⭐⭐⭐ |
| 11 | mysten-service-boilerplate | 基础设施层 | 工具 | ⭐ |
| 12 | prometheus-closure-metric | 基础设施层 | 监控 | ⭐ |
| 13 | shared-crypto | 基础设施层 | 密码学 | ⭐⭐⭐⭐⭐ |
| 14 | simulacrum | 测试工具 | 模拟 | ⭐⭐ |
| 15 | sui | 应用层 | CLI | ⭐⭐⭐⭐⭐ |
| 16 | sui-adapter | 核心协议层 | 执行 | ⭐⭐⭐⭐⭐ |
| 17 | sui-adapter-transactional-tests | 测试工具 | 执行测试 | ⭐⭐ |
| 18 | sui-analytics-indexer | 服务层 | 索引 | ⭐⭐ |
| 19 | sui-analytics-indexer-derive | 工具 | 宏 | ⭐ |
| 20 | sui-authority-aggregation | 核心协议层 | 核心 | ⭐⭐⭐⭐ |
| 21 | sui-aws-orchestrator | 工具 | 部署 | ⭐ |
| 22 | sui-axelar-cgp | 应用层 | 跨链 | ⭐ |
| 23 | sui-benchmark | 测试工具 | 基准测试 | ⭐⭐⭐ |
| 24 | sui-bridge | 应用层 | 跨链桥 | ⭐⭐⭐⭐ |
| 25 | sui-bridge-cli | 应用层 | 跨链桥 | ⭐⭐⭐ |
| 26 | sui-bridge-indexer | 应用层 | 跨链桥索引 | ⭐⭐⭐ |
| 27 | sui-bridge-indexer-alt | 应用层 | 跨链桥索引 | ⭐⭐ |
| 28 | sui-bridge-schema | 应用层 | 跨链桥 | ⭐⭐ |
| 29 | sui-bridge-watchdog | 工具 | 监控 | ⭐⭐ |
| 30 | sui-checkpoint-blob-indexer | 核心协议层 | 数据摄取 | ⭐⭐⭐ |
| 31 | sui-cluster-test | 测试工具 | 集群测试 | ⭐⭐ |
| 32 | sui-config | 基础设施层 | 配置 | ⭐⭐⭐⭐⭐ |
| 33 | sui-core | 核心协议层 | 核心 | ⭐⭐⭐⭐⭐ |
| 34 | sui-cost | 核心协议层 | Gas | ⭐⭐⭐ |
| 35 | sui-data-ingestion | 核心协议层 | 数据摄取 | ⭐⭐⭐ |
| 36 | sui-data-ingestion-core | 核心协议层 | 数据摄取 | ⭐⭐⭐⭐ |
| 37 | sui-data-store | 基础设施层 | 存储 | ⭐⭐⭐ |
| 38 | sui-deepbook-indexer | 工具 | DEX索引 | ⭐⭐⭐ |
| 39 | sui-default-config | 基础设施层 | 配置 | ⭐⭐⭐ |
| 40 | sui-display | 核心协议层 | 标准 | ⭐⭐⭐ |
| 41 | sui-e2e-tests | 测试工具 | E2E测试 | ⭐⭐ |
| 42 | sui-enum-compat-util | 基础设施层 | 工具 | ⭐⭐ |
| 43 | sui-execution | 核心协议层 | 执行 | ⭐⭐⭐⭐⭐ |
| 44 | sui-faucet | 应用层 | 水龙头 | ⭐⭐⭐ |
| 45 | sui-field-count | 基础设施层 | 宏 | ⭐⭐ |
| 46 | sui-field-count-derive | 基础设施层 | 宏 | ⭐⭐ |
| 47 | sui-framework | 核心协议层 | Move框架 | ⭐⭐⭐⭐⭐ |
| 48 | sui-framework-snapshot | 核心协议层 | Move框架 | ⭐⭐⭐⭐ |
| 49 | sui-framework-tests | 测试工具 | 框架测试 | ⭐⭐ |
| 50 | sui-futures | 基础设施层 | 工具 | ⭐⭐ |
| 51 | sui-genesis-builder | 核心协议层 | 创世 | ⭐⭐⭐ |
| 52 | sui-graphql-e2e-tests | 测试工具 | GraphQL测试 | ⭐⭐ |
| 53 | sui-graphql-rpc | 服务层 | RPC | ⭐⭐⭐⭐ |
| 54 | sui-graphql-rpc-client | 服务层 | RPC | ⭐⭐⭐ |
| 55 | sui-graphql-rpc-headers | 服务层 | RPC | ⭐⭐ |
| 56 | sui-http | 基础设施层 | 网络 | ⭐⭐ |
| 57 | sui-indexer | 服务层 | 索引 | ⭐⭐⭐ |
| 58 | sui-indexer-alt | 服务层 | 索引 | ⭐⭐⭐⭐⭐ |
| 59 | sui-indexer-alt-consistent-api | 服务层 | 索引 | ⭐⭐ |
| 60 | sui-indexer-alt-consistent-store | 服务层 | 索引 | ⭐⭐⭐ |
| 61 | sui-indexer-alt-e2e-tests | 测试工具 | 索引测试 | ⭐⭐ |
| 62 | sui-indexer-alt-framework | 服务层 | 索引 | ⭐⭐⭐⭐ |
| 63 | sui-indexer-alt-framework-store-traits | 服务层 | 索引 | ⭐⭐ |
| 64 | sui-indexer-alt-graphql | 服务层 | 索引 | ⭐⭐⭐ |
| 65 | sui-indexer-alt-jsonrpc | 服务层 | 索引 | ⭐⭐⭐ |
| 66 | sui-indexer-alt-metrics | 服务层 | 索引 | ⭐⭐ |
| 67 | sui-indexer-alt-object-store | 服务层 | 索引 | ⭐⭐⭐ |
| 68 | sui-indexer-alt-reader | 服务层 | 索引 | ⭐⭐⭐ |
| 69 | sui-indexer-alt-restorer | 工具 | 索引 | ⭐⭐ |
| 70 | sui-indexer-alt-schema | 服务层 | 索引 | ⭐⭐⭐⭐ |
| 71 | sui-indexer-builder | 工具 | 索引 | ⭐⭐ |
| 72 | sui-json | 基础设施层 | 序列化 | ⭐⭐⭐ |
| 73 | sui-json-rpc | 服务层 | RPC | ⭐⭐⭐⭐⭐ |
| 74 | sui-json-rpc-api | 服务层 | RPC | ⭐⭐⭐⭐ |
| 75 | sui-json-rpc-tests | 测试工具 | RPC测试 | ⭐⭐ |
| 76 | sui-json-rpc-types | 服务层 | RPC | ⭐⭐⭐⭐ |
| 77 | sui-keys | 基础设施层 | 密钥 | ⭐⭐⭐ |
| 78 | sui-kv-rpc | 工具 | RPC | ⭐⭐ |
| 79 | sui-kvstore | 工具 | 存储 | ⭐⭐ |
| 80 | sui-light-client | 工具 | 轻客户端 | ⭐⭐ |
| 81 | sui-macros | 基础设施层 | 宏 | ⭐⭐⭐ |
| 82 | sui-metric-checker | 测试工具 | 指标检查 | ⭐⭐ |
| 83 | sui-metrics-push-client | 基础设施层 | 监控 | ⭐⭐ |
| 84 | sui-move | 应用层 | Move工具 | ⭐⭐⭐⭐ |
| 85 | sui-move-build | 应用层 | Move工具 | ⭐⭐⭐⭐ |
| 86 | sui-move-lsp | 应用层 | Move工具 | ⭐⭐⭐ |
| 87 | sui-move-natives | 核心协议层 | 执行 | ⭐⭐⭐⭐ |
| 88 | sui-name-service | 应用层 | 域名 | ⭐⭐⭐ |
| 89 | sui-network | 基础设施层 | 网络 | ⭐⭐⭐⭐ |
| 90 | sui-node | 服务层 | 节点 | ⭐⭐⭐⭐⭐ |
| 91 | sui-open-rpc | 工具 | RPC规范 | ⭐⭐ |
| 92 | sui-open-rpc-macros | 工具 | RPC规范 | ⭐⭐ |
| 93 | sui-oracle | 应用层 | 预言机 | ⭐⭐⭐ |
| 94 | sui-package-alt | 核心协议层 | 包管理 | ⭐⭐ |
| 95 | sui-package-dump | 核心协议层 | 包管理 | ⭐⭐ |
| 96 | sui-package-management | 核心协议层 | 包管理 | ⭐⭐⭐⭐ |
| 97 | sui-package-resolver | 核心协议层 | 包管理 | ⭐⭐⭐ |
| 98 | sui-pg-db | 工具 | 数据库 | ⭐⭐⭐ |
| 99 | sui-proc-macros | 基础设施层 | 宏 | ⭐⭐⭐ |
| 100 | sui-protocol-config | 基础设施层 | 配置 | ⭐⭐⭐⭐⭐ |
| 101 | sui-protocol-config-macros | 基础设施层 | 配置 | ⭐⭐ |
| 102 | sui-proxy | 服务层 | 代理 | ⭐⭐ |
| 103 | sui-replay | 工具 | 重放 | ⭐⭐ |
| 104 | sui-replay-2 | 工具 | 重放 | ⭐⭐ |
| 105 | sui-rosetta | 应用层 | Rosetta | ⭐⭐ |
| 106 | sui-rpc-api | 工具 | RPC | ⭐⭐⭐ |
| 107 | sui-rpc-benchmark | 测试工具 | RPC基准 | ⭐⭐ |
| 108 | sui-rpc-loadgen | 测试工具 | RPC负载 | ⭐⭐ |
| 109 | sui-rpc-resolver | 工具 | RPC | ⭐⭐ |
| 110 | sui-sdk | 应用层 | SDK | ⭐⭐⭐⭐⭐ |
| 111 | sui-security-watchdog | 工具 | 监控 | ⭐⭐ |
| 112 | sui-simulator | 测试工具 | 模拟器 | ⭐⭐ |
| 113 | sui-single-node-benchmark | 测试工具 | 基准测试 | ⭐⭐ |
| 114 | sui-snapshot | 工具 | 快照 | ⭐⭐ |
| 115 | sui-source-validation | 核心协议层 | 验证 | ⭐⭐ |
| 116 | sui-sql-macro | 工具 | SQL | ⭐⭐ |
| 117 | sui-storage | 基础设施层 | 存储 | ⭐⭐⭐⭐⭐ |
| 118 | sui-surfer | 工具 | 工具 | ⭐ |
| 119 | sui-swarm | 测试工具 | 测试网络 | ⭐⭐⭐ |
| 120 | sui-swarm-config | 测试工具 | 测试网络 | ⭐⭐ |
| 121 | sui-synthetic-ingestion | 测试工具 | 合成数据 | ⭐⭐ |
| 122 | sui-telemetry | 基础设施层 | 遥测 | ⭐⭐ |
| 123 | sui-test-transaction-builder | 测试工具 | 测试 | ⭐⭐ |
| 124 | sui-test-validator | 测试工具 | 测试 | ⭐⭐⭐ |
| 125 | sui-tls | 基础设施层 | TLS | ⭐⭐⭐ |
| 126 | sui-tool | 工具 | CLI工具 | ⭐⭐⭐ |
| 127 | sui-transaction-builder | 应用层 | 交易构建 | ⭐⭐⭐⭐ |
| 128 | sui-transaction-checks | 核心协议层 | 交易验证 | ⭐⭐⭐⭐ |
| 129 | sui-transactional-test-runner | 测试工具 | 测试 | ⭐⭐ |
| 130 | sui-types | 基础设施层 | 类型 | ⭐⭐⭐⭐⭐ |
| 131 | sui-upgrade-compatibility-transactional-tests | 测试工具 | 升级测试 | ⭐⭐ |
| 132 | sui-verifier | 核心协议层 | 验证器 | ⭐⭐⭐⭐ |
| 133 | sui-verifier-transactional-tests | 测试工具 | 验证器测试 | ⭐⭐ |
| 134 | suins-indexer | 工具 | 域名索引 | ⭐⭐ |
| 135 | telemetry-subscribers | 基础设施层 | 日志 | ⭐⭐⭐ |
| 136 | test-cluster | 测试工具 | 测试集群 | ⭐⭐ |
| 137 | transaction-fuzzer | 测试工具 | 模糊测试 | ⭐⭐ |
| 138 | typed-store | 基础设施层 | 存储 | ⭐⭐⭐⭐⭐ |
| 139 | typed-store-derive | 基础设施层 | 存储宏 | ⭐⭐⭐ |
| 140 | typed-store-error | 基础设施层 | 存储错误 | ⭐⭐ |
| 141 | typed-store-workspace-hack | 基础设施层 | Workspace | ⭐ |
| 142 | x | 基础设施层 | Workspace工具 | ⭐ |

---

## 重要性说明

| 星级 | 含义 | 说明 |
|-----|------|------|
| ⭐⭐⭐⭐⭐ | 核心必需 | 运行节点或开发应用的绝对核心模块 |
| ⭐⭐⭐⭐ | 重要 | 大多数场景需要的重要模块 |
| ⭐⭐⭐ | 常用 | 特定功能需要的常用模块 |
| ⭐⭐ | 辅助 | 测试、工具或特定场景的辅助模块 |
| ⭐ | 可选 | 内部工具或很少使用的模块 |

---

## DEX 开发者快速索引

如果你正在开发 DEX,以下模块最相关:

### 必需模块 (⭐⭐⭐⭐⭐)
- sui-types, sui-core, sui-execution, sui-framework
- sui-storage, typed-store
- consensus-core, mysten-network
- sui-node, sui-json-rpc, sui-sdk

### 推荐模块 (⭐⭐⭐⭐)
- sui-indexer-alt (订单历史查询)
- sui-transaction-builder (订单交易构建)
- sui-deepbook-indexer (DeepBook 专用索引)

### 可选模块 (⭐⭐⭐)
- sui-graphql-rpc (如果需要 GraphQL)
- sui-bridge (如果需要跨链)

> **详细分析**: [05-DEX-IMPLEMENTATION.md](05-DEX-IMPLEMENTATION.md)

---

**返回**: [架构文档首页](README.md) | **下一步**: [关键模块详解](02-KEY-MODULES.md)
