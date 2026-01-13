# Sui Crates 模块依赖关系完整参考

本文档梳理了 Sui 项目 `crates/` 目录下所有 114+ 个 crate 模块的依赖关系,为理解 Sui 架构和进行模块选择提供参考。

---

## 一、总体架构概述

### 模块数量统计

- **总模块数**: 114+ 个 Rust crate
- **核心模块**: ~15 个(sui-core, sui-types, sui-storage 等)
- **API/RPC 模块**: ~15 个(JSON-RPC, GraphQL, 索引器等)
- **工具支持模块**: ~40 个(测试、监控、构建工具等)
- **上游依赖模块**: ~10 个(mysten-*, shared-*, typed-store 等)

### 依赖层级结构

Sui 代码库采用分层架构,从基础类型到应用服务共分为 5 层:

```
┌─────────────────────────────────────────────────────────────────┐
│ Layer 0: 基础类型层 (Foundation Layer)                            │
├─────────────────────────────────────────────────────────────────┤
│ - sui-types (核心类型定义)                                        │
│ - sui-protocol-config (协议配置)                                 │
│ - move-core-types (Move 基础类型)                                │
│ - shared-crypto (加密原语)                                        │
│                                                                  │
│ 特点: 无向上依赖,被所有上层模块依赖                                │
└─────────────────────────────────────────────────────────────────┘
                            ↑
┌─────────────────────────────────────────────────────────────────┐
│ Layer 1: 工具层 (Utility Layer)                                  │
├─────────────────────────────────────────────────────────────────┤
│ - sui-json (JSON 序列化/反序列化)                                 │
│ - sui-json-rpc-types (RPC 类型定义)                              │
│ - sui-transaction-builder (交易构建器)                           │
│ - sui-indexer-alt-schema (索引器 schema)                         │
│ - sui-framework (Move 标准库)                                    │
│                                                                  │
│ 特点: 依赖基础层,提供通用工具和类型转换                             │
└─────────────────────────────────────────────────────────────────┘
                            ↑
┌─────────────────────────────────────────────────────────────────┐
│ Layer 2: 存储与数据层 (Storage & Data Layer)                     │
├─────────────────────────────────────────────────────────────────┤
│ - sui-storage (对象存储、检查点管理)                               │
│ - typed-store (类型化 RocksDB 封装)                              │
│ - sui-pg-db (PostgreSQL 接口)                                   │
│ - sui-data-ingestion-core (数据摄取)                             │
│ - sui-indexer-alt-framework (索引器框架)                         │
│                                                                  │
│ 特点: 管理持久化数据,不涉及业务逻辑                                │
└─────────────────────────────────────────────────────────────────┘
                            ↑
┌─────────────────────────────────────────────────────────────────┐
│ Layer 3: 执行与核心层 (Execution & Core Layer)                   │
├─────────────────────────────────────────────────────────────────┤
│ - sui-execution (Move VM 执行引擎,版本化)                         │
│ - sui-core (权限管理、交易处理、共识集成)                          │
│ - consensus/ (Mysticeti 共识协议)                                │
│ - sui-authority-aggregation (Authority 聚合)                     │
│                                                                  │
│ 特点: 重型模块,包含完整的验证器逻辑                                │
└─────────────────────────────────────────────────────────────────┘
                            ↑
┌─────────────────────────────────────────────────────────────────┐
│ Layer 4: 应用与服务层 (Application & Service Layer)              │
├─────────────────────────────────────────────────────────────────┤
│ - sui-node (验证器节点)                                           │
│ - sui-json-rpc (JSON-RPC 服务器)                                 │
│ - sui-graphql-rpc (GraphQL 服务器)                               │
│ - sui-indexer-alt (新索引器)                                      │
│ - sui-deepbook-indexer (DeepBook 索引器)                         │
│ - sui-sdk (Rust SDK)                                             │
│                                                                  │
│ 特点: 面向最终用户的服务和工具                                     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 二、模块分类详细列表

### 1. 核心架构 Crates (Core Architecture)

这些是 Sui 区块链的核心模块,实现了共识、执行、状态管理等基础功能。

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-core** | 区块链核心逻辑:权限管理、交易处理、证书聚合、对象状态管理 | `crates/sui-core/Cargo.toml` | Layer 3 |
| **sui-types** | 核心类型定义:Object、Transaction、Event、Signature、Effects 等 | `crates/sui-types/Cargo.toml` | Layer 0 |
| **sui-node** | 验证器节点实现,集成共识、执行、存储、RPC 服务 | `crates/sui-node/Cargo.toml` | Layer 4 |
| **sui-framework** | Move 系统包和标准库,提供 Sui 框架 Move 模块 | `crates/sui-framework/Cargo.toml` | Layer 1 |
| **sui-execution** | Move VM 执行层的版本化多路复用器(v0/v1/v2/latest) | `sui-execution/Cargo.toml` | Layer 3 |
| **sui-config** | 节点配置管理,包括 genesis、网络、权限配置 | `crates/sui-config/Cargo.toml` | Layer 1 |
| **sui-protocol-config** | 协议参数配置,支持协议版本升级 | `crates/sui-protocol-config/Cargo.toml` | Layer 0 |

**关键依赖关系**:
```
sui-node
├─ sui-core
│  ├─ sui-execution
│  ├─ sui-framework
│  ├─ sui-storage
│  ├─ consensus-core
│  └─ sui-types
├─ sui-json-rpc
└─ sui-graphql-rpc
```

---

### 2. 存储层 Crates (Storage Layer)

管理持久化数据、对象存储、检查点、数据库接口。

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-storage** | 持久化存储:对象存储、检查点管理、事务日志、状态同步 | `crates/sui-storage/Cargo.toml` | Layer 2 |
| **typed-store** | 类型化 KV 存储抽象,封装 RocksDB | `crates/typed-store/Cargo.toml` | Layer 2 |
| **typed-store-derive** | typed-store 的 derive macros | `crates/typed-store-derive/Cargo.toml` | Layer 2 |
| **sui-pg-db** | PostgreSQL 数据库接口,用于索引器 | `crates/sui-pg-db/Cargo.toml` | Layer 2 |
| **sui-data-store** | 数据存储框架,提供统一的数据访问接口 | `crates/sui-data-store/Cargo.toml` | Layer 2 |
| **sui-kvstore** | 键值存储实现,基于 RocksDB 或其他后端 | `crates/sui-kvstore/Cargo.toml` | Layer 2 |

**关键依赖关系**:
```
sui-storage
├─ typed-store (RocksDB 封装)
├─ sui-types (核心类型)
└─ sui-protocol-config
```

---

### 3. RPC 和 API Crates (API & RPC Layer)

提供 JSON-RPC、GraphQL、KV-RPC 等 API 服务。

#### 3.1 JSON-RPC API

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-json-rpc** | JSON-RPC API 服务器实现,提供完整的查询和交易接口 | `crates/sui-json-rpc/Cargo.toml` | Layer 4 |
| **sui-json-rpc-api** | JSON-RPC API 定义(trait 和接口) | `crates/sui-json-rpc-api/Cargo.toml` | Layer 3 |
| **sui-json-rpc-types** | JSON-RPC 类型定义,用于序列化 | `crates/sui-json-rpc-types/Cargo.toml` | Layer 1 |
| **sui-json-rpc-tests** | JSON-RPC E2E 测试 | `crates/sui-json-rpc-tests/Cargo.toml` | - |
| **sui-rpc-api** | 新一代 RPC API 定义 | `crates/sui-rpc-api/Cargo.toml` | Layer 1 |

**关键依赖关系**:
```
sui-json-rpc
├─ sui-core (⚠️ 重型依赖)
│  ├─ sui-execution
│  └─ sui-storage
├─ sui-storage
├─ sui-types
└─ sui-transaction-builder
```

#### 3.2 GraphQL API

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-graphql-rpc** | GraphQL API 服务器,提供灵活的查询接口 | `crates/sui-graphql-rpc/Cargo.toml` | Layer 4 |
| **sui-graphql-rpc-client** | GraphQL RPC 客户端库 | `crates/sui-graphql-rpc-client/Cargo.toml` | - |
| **sui-graphql-rpc-headers** | GraphQL RPC 头部处理 | `crates/sui-graphql-rpc-headers/Cargo.toml` | - |
| **sui-graphql-e2e-tests** | GraphQL E2E 测试 | `crates/sui-graphql-e2e-tests/Cargo.toml` | - |

#### 3.3 其他 API

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-kv-rpc** | 键值 RPC 接口 | `crates/sui-kv-rpc/Cargo.toml` |
| **sui-json** | JSON 序列化/反序列化工具 | `crates/sui-json/Cargo.toml` |
| **sui-open-rpc** | OpenRPC 文档自动生成 | `crates/sui-open-rpc/Cargo.toml` |

---

### 4. 索引器 Crates (Indexer Layer)

索引器负责从区块链数据中提取、转换、存储结构化数据,供快速查询。

#### 4.1 新一代索引器 (sui-indexer-alt 系列)

采用模块化设计,职责清晰,支持灵活扩展。

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-indexer-alt** | 新索引器主程序,协调各组件 | `crates/sui-indexer-alt/Cargo.toml` | Layer 4 |
| **sui-indexer-alt-framework** | 索引器框架核心,提供数据摄取和处理基础设施 | `crates/sui-indexer-alt-framework/Cargo.toml` | Layer 2 |
| **sui-indexer-alt-framework-store-traits** | 存储 trait 定义 | `crates/sui-indexer-alt-framework-store-traits/Cargo.toml` | Layer 2 |
| **sui-indexer-alt-schema** | 数据库 schema 定义(Diesel ORM) | `crates/sui-indexer-alt-schema/Cargo.toml` | Layer 1 |
| **sui-indexer-alt-graphql** | GraphQL 接口模块 | `crates/sui-indexer-alt-graphql/Cargo.toml` | Layer 4 |
| **sui-indexer-alt-jsonrpc** | JSON-RPC 接口模块(兼容旧接口) | `crates/sui-indexer-alt-jsonrpc/Cargo.toml` | Layer 4 |
| **sui-indexer-alt-consistent-api** | 一致性 API 实现 | `crates/sui-indexer-alt-consistent-api/Cargo.toml` | Layer 3 |
| **sui-indexer-alt-consistent-store** | 一致性存储实现 | `crates/sui-indexer-alt-consistent-store/Cargo.toml` | Layer 2 |
| **sui-indexer-alt-object-store** | 对象存储模块 | `crates/sui-indexer-alt-object-store/Cargo.toml` | Layer 2 |
| **sui-indexer-alt-reader** | 数据读取器,支持复杂查询 | `crates/sui-indexer-alt-reader/Cargo.toml` | Layer 3 |
| **sui-indexer-alt-restorer** | 恢复机制模块 | `crates/sui-indexer-alt-restorer/Cargo.toml` | Layer 3 |
| **sui-indexer-alt-metrics** | 索引器指标收集 | `crates/sui-indexer-alt-metrics/Cargo.toml` | - |
| **sui-indexer-alt-e2e-tests** | 索引器 E2E 测试 | `crates/sui-indexer-alt-e2e-tests/Cargo.toml` | - |

**架构优势**:
- 模块化设计,职责清晰
- 不依赖 `sui-core`,避免重型依赖
- 支持 PostgreSQL,查询性能优秀
- 支持多种接口(GraphQL, JSON-RPC)

**关键依赖关系**:
```
sui-indexer-alt
├─ sui-indexer-alt-framework
│  ├─ sui-storage
│  │  └─ sui-types
│  └─ sui-indexer-alt-framework-store-traits
├─ sui-indexer-alt-schema
│  └─ sui-types
└─ sui-indexer-alt-jsonrpc
   ├─ sui-indexer-alt-reader
   └─ sui-json-rpc-types
```

#### 4.2 旧索引器

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-indexer** | 原索引器实现(逐步被 sui-indexer-alt 替代) | `crates/sui-indexer/Cargo.toml` |
| **sui-indexer-builder** | 索引器构建框架 | `crates/sui-indexer-builder/Cargo.toml` |

#### 4.3 特化索引器

针对特定用途的索引器实现。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-deepbook-indexer** | DeepBook DEX 专用索引器,索引订单簿事件和交易 | `crates/sui-deepbook-indexer/Cargo.toml` |
| **sui-analytics-indexer** | 分析数据索引器,用于数据分析和统计 | `crates/sui-analytics-indexer/Cargo.toml` |
| **sui-bridge-indexer** | 跨链桥索引器(旧) | `crates/sui-bridge-indexer/Cargo.toml` |
| **sui-bridge-indexer-alt** | 跨链桥索引器(新架构) | `crates/sui-bridge-indexer-alt/Cargo.toml` |
| **sui-checkpoint-blob-indexer** | 检查点 blob 索引器 | `crates/sui-checkpoint-blob-indexer/Cargo.toml` |
| **suins-indexer** | SuiNS 域名服务索引器 | `crates/suins-indexer/Cargo.toml` |

---

### 5. 数据摄入 Crates (Data Ingestion)

负责从区块链数据源(检查点、交易流)中摄取数据。

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-data-ingestion-core** | 数据摄入核心库,提供检查点读取和订阅 | `crates/sui-data-ingestion-core/Cargo.toml` | Layer 2 |
| **sui-data-ingestion** | 数据摄入实现,支持多种数据源 | `crates/sui-data-ingestion/Cargo.toml` | Layer 3 |
| **sui-synthetic-ingestion** | 合成数据摄入(用于测试和模拟) | `crates/sui-synthetic-ingestion/Cargo.toml` | - |

---

### 6. 交易构建和处理 Crates (Transaction Layer)

用于构建、验证、处理交易。

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-transaction-builder** | 交易构建器,构建 Programmable Transactions (PTB) | `crates/sui-transaction-builder/Cargo.toml` | Layer 1 |
| **sui-transaction-checks** | 交易验证检查(Gas、签名、对象锁等) | `crates/sui-transaction-checks/Cargo.toml` | Layer 1 |
| **sui-test-transaction-builder** | 测试用交易构建器 | `crates/sui-test-transaction-builder/Cargo.toml` | - |

**关键特性**:
- `sui-transaction-builder` 是轻量级模块,只依赖 `sui-types`
- 支持构建复杂的 Programmable Transaction Blocks
- 用于客户端和测试环境

---

### 7. 客户端和 SDK Crates (Client/SDK Layer)

提供 SDK 和客户端工具。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-sdk** | 官方 Sui Rust SDK,提供完整的客户端功能 | `crates/sui-sdk/Cargo.toml` |
| **sui-sdk-types** | SDK 类型定义 | `crates/sui-sdk-types/Cargo.toml` |
| **sui-keys** | 密钥管理工具 | `crates/sui-keys/Cargo.toml` |
| **sui-move** | Move 工具链 CLI | `crates/sui-move/Cargo.toml` |
| **sui-move-build** | Move 编译构建工具 | `crates/sui-move-build/Cargo.toml` |
| **sui-move-lsp** | Move 语言服务器(IDE 支持) | `crates/sui-move-lsp/Cargo.toml` |

---

### 8. 网络和通信 Crates (Networking)

实现 P2P 网络、节点通信、权限聚合等功能。

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **sui-network** | P2P 网络层,实现节点间通信 | `crates/sui-network/Cargo.toml` | Layer 3 |
| **sui-authority-aggregation** | Authority 权限聚合,收集 2f+1 签名 | `crates/sui-authority-aggregation/Cargo.toml` | Layer 3 |
| **mysten-network** | Mysten Labs 网络库(通用网络框架) | `crates/mysten-network/Cargo.toml` | Layer 0 |
| **mysten-service** | 网络服务框架 | `crates/mysten-service/Cargo.toml` | Layer 0 |

---

### 9. 测试和验证 Crates (Testing & Validation)

提供各种测试框架和工具。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-cluster-test** | 集群测试框架,测试多节点场景 | `crates/sui-cluster-test/Cargo.toml` |
| **sui-e2e-tests** | 端到端测试 | `crates/sui-e2e-tests/Cargo.toml` |
| **sui-graphql-e2e-tests** | GraphQL E2E 测试 | `crates/sui-graphql-e2e-tests/Cargo.toml` |
| **sui-indexer-alt-e2e-tests** | 索引器 E2E 测试 | `crates/sui-indexer-alt-e2e-tests/Cargo.toml` |
| **sui-test-validator** | 测试验证器节点,用于本地开发 | `crates/sui-test-validator/Cargo.toml` |
| **sui-transactional-test-runner** | 交易式测试运行器(Move 测试) | `crates/sui-transactional-test-runner/Cargo.toml` |
| **test-cluster** | 通用测试集群工具 | `crates/test-cluster/Cargo.toml` |
| **transaction-fuzzer** | 交易模糊测试器 | `crates/transaction-fuzzer/Cargo.toml` |

---

### 10. 性能和监控 Crates (Monitoring & Performance)

提供性能基准测试、指标收集、遥测等功能。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-benchmark** | 基准测试框架,压测工具 | `crates/sui-benchmark/Cargo.toml` |
| **sui-single-node-benchmark** | 单节点性能基准测试 | `crates/sui-single-node-benchmark/Cargo.toml` |
| **sui-rpc-benchmark** | RPC 接口基准测试 | `crates/sui-rpc-benchmark/Cargo.toml` |
| **sui-rpc-loadgen** | RPC 负载生成器 | `crates/sui-rpc-loadgen/Cargo.toml` |
| **sui-metrics-push-client** | 指标推送客户端 | `crates/sui-metrics-push-client/Cargo.toml` |
| **mysten-metrics** | 指标收集库(Prometheus) | `crates/mysten-metrics/Cargo.toml` |
| **sui-metric-checker** | 指标检查工具 | `crates/sui-metric-checker/Cargo.toml` |
| **telemetry-subscribers** | 遥测订阅者(Tracing) | `crates/telemetry-subscribers/Cargo.toml` |
| **sui-telemetry** | 遥测数据收集 | `crates/sui-telemetry/Cargo.toml` |
| **sui-cost** | Gas 成本计算工具 | `crates/sui-cost/Cargo.toml` |

---

### 11. 基础设施和运维 Crates (Infrastructure & DevOps)

用于节点部署、配置、集群管理等。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-swarm** | 本地 Sui 网络模拟(Swarm),用于本地开发 | `crates/sui-swarm/Cargo.toml` |
| **sui-swarm-config** | Swarm 配置管理 | `crates/sui-swarm-config/Cargo.toml` |
| **sui-genesis-builder** | 创世块构建工具 | `crates/sui-genesis-builder/Cargo.toml` |
| **sui-default-config** | 默认配置生成 | `crates/sui-default-config/Cargo.toml` |
| **sui-simulator** | 事件驱动模拟器(用于测试共识) | `crates/sui-simulator/Cargo.toml` |
| **sui-aws-orchestrator** | AWS 部署编排工具 | `crates/sui-aws-orchestrator/Cargo.toml` |
| **sui-tool** | 命令行工具集(数据库操作、对象检查等) | `crates/sui-tool/Cargo.toml` |
| **sui-snapshot** | 检查点快照管理工具 | `crates/sui-snapshot/Cargo.toml` |

---

### 12. 杂项和支持 Crates (Utilities & Support)

提供各种辅助功能。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-faucet** | 水龙头服务,发放测试币 | `crates/sui-faucet/Cargo.toml` |
| **sui-display** | 对象显示格式标准 | `crates/sui-display/Cargo.toml` |
| **sui-name-service** | SuiNS 域名服务实现 | `crates/sui-name-service/Cargo.toml` |
| **sui-oracle** | 预言机实现 | `crates/sui-oracle/Cargo.toml` |
| **sui-light-client** | 轻客户端实现 | `crates/sui-light-client/Cargo.toml` |
| **sui-http** | HTTP 服务支持 | `crates/sui-http/Cargo.toml` |
| **sui-tls** | TLS/SSL 支持 | `crates/sui-tls/Cargo.toml` |
| **sui-macros** | 过程宏集合 | `crates/sui-macros/Cargo.toml` |
| **sui-enum-compat-util** | 枚举兼容性工具 | `crates/sui-enum-compat-util/Cargo.toml` |
| **sui-field-count** | 字段计数工具(derive macro) | `crates/sui-field-count/Cargo.toml` |
| **sui-rpc** | RPC 基础设施 | `crates/sui-rpc/Cargo.toml` |
| **sui-futures** | Future 工具扩展 | `crates/sui-futures/Cargo.toml` |

---

### 13. 跨链和第三方集成 Crates (Cross-Chain & Integration)

实现跨链桥和外部集成。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-bridge** | 跨链桥核心逻辑 | `crates/sui-bridge/Cargo.toml` |
| **sui-bridge-cli** | 桥 CLI 工具 | `crates/sui-bridge-cli/Cargo.toml` |
| **sui-bridge-indexer** | 桥事件索引器(旧) | `crates/sui-bridge-indexer/Cargo.toml` |
| **sui-bridge-indexer-alt** | 桥事件索引器(新架构) | `crates/sui-bridge-indexer-alt/Cargo.toml` |
| **sui-bridge-schema** | 桥数据 schema 定义 | `crates/sui-bridge-schema/Cargo.toml` |

---

### 14. 特殊用途 Crates (Specialized)

用于特定场景的模块。

| Crate | 功能描述 | Cargo.toml 路径 |
|-------|---------|----------------|
| **sui-replay** | 交易重放工具(旧) | `crates/sui-replay/Cargo.toml` |
| **sui-replay-2** | 交易重放工具(新版本) | `crates/sui-replay-2/Cargo.toml` |
| **sui-rosetta** | Rosetta API 实现(区块链标准接口) | `crates/sui-rosetta/Cargo.toml` |
| **sui-package-alt** | Move 包管理(替代实现) | `crates/sui-package-alt/Cargo.toml` |
| **sui-package-resolver** | 包解析器,解析链上 Move 包 | `crates/sui-package-resolver/Cargo.toml` |
| **sui-rpc-resolver** | RPC 解析器 | `crates/sui-rpc-resolver/Cargo.toml` |
| **sui-source-validation** | 源代码验证,验证链上代码与源码一致性 | `crates/sui-source-validation/Cargo.toml` |

---

### 15. 上游依赖 Crates (External/Upstream)

Mysten Labs 的通用库,被 Sui 项目使用但不特定于 Sui。

| Crate | 功能描述 | Cargo.toml 路径 | 层级 |
|-------|---------|----------------|------|
| **mysten-common** | 通用工具库(日志、错误处理等) | `crates/mysten-common/Cargo.toml` | Layer 0 |
| **shared-crypto** | 加密原语库(fastcrypto 封装) | `crates/shared-crypto/Cargo.toml` | Layer 0 |
| **bin-version** | 版本管理工具 | `crates/bin-version/Cargo.toml` | - |
| **anemo-benchmark** | Anemo 网络基准测试 | `crates/anemo-benchmark/Cargo.toml` | - |

---

## 三、核心模块依赖关系详解

### 1. sui-types (基础类型层)

**层级**: Layer 0 (最底层)

**直接依赖的 sui crates**:
- `sui-protocol-config`
- `sui-macros`
- `sui-enum-compat-util`
- `sui-sdk-types`
- `sui-rpc`

**直接依赖的 Move crates**:
- `move-binary-format`
- `move-bytecode-utils`
- `move-core-types`
- `move-trace-format`
- `move-vm-test-utils`
- `move-vm-profiler`

**直接依赖的共识 crates**:
- `consensus-config`
- `consensus-types`

**传递依赖深度**: 0-1 层(这是基础模块)

**特点**: 几乎所有 sui crate 都依赖它,定义了核心数据结构。

---

### 2. sui-core (核心执行层)

**层级**: Layer 3 (执行核心层)

**直接依赖的 sui crates** (验证自 `crates/sui-core/Cargo.toml`):
- `sui-execution` ⚠️
- `sui-framework` ⚠️
- `sui-storage`
- `sui-types`
- `sui-config`
- `sui-authority-aggregation`
- `sui-network`
- `sui-protocol-config`
- `sui-transaction-checks`
- `sui-simulator`
- `sui-swarm-config`
- `sui-genesis-builder`
- `sui-json-rpc-types`
- `sui-macros`
- `sui-tls`

**直接依赖的共识 crates**:
- `consensus-core` ⚠️
- `consensus-config` ⚠️
- `consensus-types` ⚠️

**传递依赖深度**: 3-4 层

**特点**: 这是最重型的模块,几乎依赖所有核心子系统,包含完整的验证器逻辑。

**依赖链示例**:
```
sui-core
├─ sui-execution
│  ├─ sui-protocol-config
│  ├─ sui-types
│  └─ sui-adapter-{latest,v0,v1,v2}
├─ sui-framework
│  └─ sui-types
├─ sui-storage
│  └─ sui-types
└─ consensus-core
   ├─ consensus-config
   └─ consensus-types
```

---

### 3. sui-execution (执行层)

**层级**: Layer 3

**直接依赖的 sui crates** (验证自 `sui-execution/Cargo.toml`):
- `sui-protocol-config`
- `sui-types`
- `sui-adapter-{latest,v0,v1,v2}` (版本化适配器)
- `sui-move-natives-{latest,v0,v1,v2}` (版本化原生函数)
- `sui-verifier-{latest,v0,v1,v2}` (版本化验证器)

**直接依赖的 Move crates**:
- `move-binary-format`
- `move-bytecode-verifier-meter`
- `move-trace-format`
- `move-vm-config`
- `move-vm-runtime-{latest,v0,v1,v2}` (版本化 VM 运行时)
- `move-bytecode-verifier-{latest,v0,v1,v2}` (版本化字节码验证)
- `move-abstract-interpreter-{latest,v2}`
- `move-vm-types-{latest,v0,v1,v2}`

**传递依赖深度**: 1-2 层

**特点**:
- 版本化执行环境,支持协议升级而不分叉
- 通过 protocol version 在运行时选择执行版本
- 所有 Authority 代码必须通过 `sui-execution` 访问 Move VM,不得直接依赖

---

### 4. sui-storage (存储层)

**层级**: Layer 2

**直接依赖的 sui crates**:
- `sui-types`
- `sui-json-rpc-types`
- `sui-protocol-config`
- `sui-config`
- `typed-store` (RocksDB 封装)

**传递依赖深度**: 1-2 层

**特点**:
- 存储层模块,不依赖 `sui-core` 或执行层
- 保持了良好的分层架构
- 管理对象存储、检查点、事务日志

---

### 5. sui-framework (Move 框架)

**层级**: Layer 1

**直接依赖的 sui crates** (验证自 `crates/sui-framework/Cargo.toml`):
- `sui-types`

**直接依赖的 Move crates**:
- `move-binary-format`
- `move-core-types`

**传递依赖深度**: 1 层(非常轻量)

**特点**: 只依赖核心类型,是基础框架模块,提供 Sui Move 标准库。

---

### 6. sui-json-rpc (RPC 服务)

**层级**: Layer 4

**直接依赖的 sui crates**:
- `sui-core` ⚠️ (重型依赖)
- `sui-display`
- `sui-storage`
- `sui-types`
- `sui-json`
- `sui-json-rpc-api`
- `sui-name-service`
- `sui-protocol-config`
- `sui-config`
- `sui-json-rpc-types`
- `sui-transaction-builder`

**传递依赖深度**: 2-4 层

**特点**:
- 这是重型模块,直接依赖 `sui-core`
- 意味着它可以访问完整的验证器逻辑和执行层
- 适合全节点 RPC 服务,不适合轻量级客户端

---

### 7. sui-transaction-builder (交易构建)

**层级**: Layer 1

**直接依赖的 sui crates**:
- `sui-json-rpc-types`
- `sui-types`
- `sui-json`
- `sui-protocol-config`

**直接依赖的 Move crates**:
- `move-binary-format`
- `move-core-types`

**传递依赖深度**: 1-2 层(非常轻量)

**特点**:
- 轻量级模块,主要依赖核心类型
- 没有依赖重型模块如 `sui-core` 或 `sui-storage`
- 适合客户端和轻量级应用

---

### 8. sui-indexer-alt-framework (索引器框架)

**层级**: Layer 2

**直接依赖的 sui crates**:
- `sui-field-count`
- `sui-futures`
- `sui-indexer-alt-framework-store-traits`
- `sui-indexer-alt-metrics`
- `sui-rpc`
- `sui-sdk-types`
- `sui-rpc-api`
- `sui-storage`
- `sui-types`
- `sui-pg-db` (可选)

**传递依赖深度**: 2-3 层

**特点**:
- 框架层,提供索引器的通用基础设施
- 不依赖 `sui-core`,避免重型依赖
- 模块化设计,易于扩展

---

## 四、依赖层级可视化

### 完整依赖树(简化版)

```
Layer 0: Foundation
├─ sui-types
├─ sui-protocol-config
├─ move-core-types
├─ shared-crypto
└─ mysten-common

Layer 1: Utilities
├─ sui-json (← sui-types)
├─ sui-json-rpc-types (← sui-types)
├─ sui-transaction-builder (← sui-types)
├─ sui-indexer-alt-schema (← sui-types)
└─ sui-framework (← sui-types)

Layer 2: Storage & Data
├─ sui-storage (← sui-types)
├─ typed-store
├─ sui-pg-db
├─ sui-data-ingestion-core (← sui-storage)
└─ sui-indexer-alt-framework (← sui-storage, sui-types)

Layer 3: Execution & Core
├─ sui-execution (← sui-types, sui-protocol-config)
├─ sui-core (← sui-execution, sui-framework, sui-storage, consensus-*)
├─ consensus/ (consensus-core, consensus-config, consensus-types)
└─ sui-indexer-alt-reader (← sui-indexer-alt-framework)

Layer 4: Applications & Services
├─ sui-node (← sui-core)
├─ sui-json-rpc (← sui-core, sui-storage)
├─ sui-graphql-rpc (← sui-storage)
├─ sui-indexer-alt (← sui-indexer-alt-framework)
├─ sui-indexer-alt-jsonrpc (← sui-indexer-alt-reader)
└─ sui-sdk (← sui-json-rpc-types, sui-types)
```

---

## 五、快速查找索引

### 按功能查找模块

| 功能需求 | 推荐模块 |
|---------|---------|
| 构建交易 | `sui-transaction-builder` |
| 查询对象状态 | `sui-json-rpc` 或 `sui-graphql-rpc` |
| 索引区块链数据 | `sui-indexer-alt` 系列 |
| 存储对象数据 | `sui-storage`, `typed-store` |
| 执行 Move 合约 | `sui-execution` |
| 运行验证器节点 | `sui-node`, `sui-core` |
| 开发 SDK | `sui-sdk`, `sui-types` |
| 密钥管理 | `sui-keys`, `shared-crypto` |
| 性能测试 | `sui-benchmark`, `sui-single-node-benchmark` |
| 本地开发环境 | `sui-test-validator`, `sui-swarm` |

### 按依赖深度查找模块

| 依赖深度 | 模块列表 |
|---------|---------|
| **0-1 层(轻量)** | `sui-types`, `sui-protocol-config`, `sui-framework`, `sui-transaction-builder`, `sui-indexer-alt-schema` |
| **1-2 层(中等)** | `sui-storage`, `sui-json-rpc-types`, `sui-execution`, `sui-data-ingestion-core` |
| **2-3 层(较重)** | `sui-indexer-alt-framework`, `sui-indexer-alt-reader`, `sui-authority-aggregation` |
| **3-4 层(重型)** | `sui-core`, `sui-json-rpc`, `sui-node` |

---

## 六、总结

Sui 项目采用清晰的分层架构:

1. **基础层 (Layer 0)**: 提供核心类型和加密原语,无向上依赖
2. **工具层 (Layer 1)**: 提供类型转换、交易构建等轻量级工具
3. **存储层 (Layer 2)**: 管理持久化数据,不涉及业务逻辑
4. **执行核心层 (Layer 3)**: 实现共识、执行、权限管理等核心功能
5. **应用服务层 (Layer 4)**: 提供面向用户的 RPC、索引器、SDK 等服务

**关键设计原则**:
- 模块职责单一,边界清晰
- 避免循环依赖,保持单向依赖流
- 版本化执行层,支持协议升级
- 新架构(如 sui-indexer-alt)采用更模块化的设计

这份文档为开发者提供了完整的模块索引,可快速定位所需模块及其依赖关系。
