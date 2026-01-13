# 抽离 Sui 模块实现 DEX：最小可跑节点依赖集

> 你问的是两层意思：
> 1) **“在 Sui 工程里裁剪/抽离哪些模块，能把节点跑起来？”**（工程裁剪）
> 2) **“为了做 DEX，哪些模块必须保留？”**（DEX 运行时依赖）
>
> 这里给一个非常务实的结论：如果目标是“最少改动就能跑起 Sui node（fullnode/validator）”，**几乎不能大裁剪**，因为 `sui-node` 是强依赖很多子系统（RPC、存储、state-sync、checkpoint 等）的聚合体。
> 如果目标是“做一个 DEX 执行层节点”，建议 **新建一个最小二进制（dex-node）**，复用 Sui 的类型/存储/网络/签名/监控，而不是裁剪 `sui-node`。

## 1. 方案 A：保持 `sui-node`，最少改动跑起节点（推荐用于对齐/调试）

### 1.1 你真正需要“跑起来”的最小形态

- **单机 fullnode 模式**：不参与共识出块，但仍要加载 genesis、存储、RPC、state-sync 等。
- **单机 validator 模式**：需要共识（`consensus-core`）/checkpoint/交易处理等完整链路。

### 1.2 `sui-node` 启动链路里“硬依赖”的核心 crate（基本无法移除）

- **入口与编排**：`crates/sui-node`
- **核心逻辑/状态机**：`crates/sui-core`（AuthorityState、execution_scheduler、checkpoints、consensus_adapter、transaction_orchestrator…）
- **类型体系**：`crates/sui-types`（Object/Tx/Effects/Events/Committee…）
- **执行层**：`sui-execution`（选择 execution version、PTB/MoveVM 执行适配）
- **存储层**：`crates/sui-storage` + `crates/typed-store`（RocksDB、cache、object store、pending tx log…）
- **配置**：`crates/sui-config` + `crates/sui-protocol-config`
- **网络**：`crates/sui-network` + `crates/mysten-network` + `crates/sui-tls` + `crates/sui-http`
- **签名与加密**：`crates/shared-crypto` + `fastcrypto`（workspace依赖）
- **可观测性**：`crates/mysten-metrics` + `crates/telemetry-subscribers` + `crates/mysten-service` + `crates/mysten-common`

### 1.3 可选/可替换的模块（“能跑起来”不一定需要）

- **Indexer 系列**：`crates/sui-indexer*`（对外数据服务，不是节点最小闭环）
- **GraphQL RPC**：`crates/sui-graphql-rpc*`（可选）
- **Bridge 系列**：`crates/sui-bridge*`（可选）
- **Rosetta/KV-RPC/Proxy**：`crates/sui-rosetta` / `crates/sui-kv-rpc` / `crates/sui-proxy`（可选）
- **Faucet**：`crates/sui-faucet`（本地测试常用，但不是节点必须）

> 但注意：即使你不“运行”这些模块，很多仍然会被 workspace 依赖链带进编译产物里。

## 2. 方案 B：新建 `dex-node`（推荐用于 Phase 1 单节点 DEX 执行层）

### 2.1 目标

- 启动一个**单节点 DEX 执行层**，提供下单/撤单/查询/行情推送接口
- 复用 Sui 的：类型、存储、签名、监控、（可选）网络
- **不引入** Sui 的：共识、checkpoint、MoveVM 热路径（只在存取款等场景使用）

### 2.2 `dex-node` 需要抽离/复用的最小 crate 集合（建议）

- **必须复用**
  - `crates/sui-types`：Object/Tx/Effects/Event 数据结构
  - `crates/typed-store`：RocksDB typed tables
  - `crates/sui-storage`：cache/object-store/pending log（可按需选用子模块）
  - `crates/shared-crypto`：Intent/签名域
  - `crates/mysten-metrics` + `crates/telemetry-subscribers`：指标+日志
- **建议复用**
  - `crates/sui-config` + `crates/sui-protocol-config`：参数/配置体系
  - `crates/mysten-common`：通用 backoff/async once cell 等
- **Phase 2 才需要**
  - `crates/sui-network` + `crates/mysten-network`：P2P/validator 网络
  - `consensus/core`：Mysticeti DAG 共识

### 2.3 为什么“不建议裁剪 sui-node”来做 Phase 1

- `sui-node` 在启动时会初始化大量与 Phase 1 无关但强绑定的子系统：checkpoint、state-sync、reconfig、validator service、backpressure、global state hasher 等。
- 你为了性能要绕过 MoveVM（撮合/风控/清算），这会在 `sui-core`/`sui-execution` 路径引入大量条件分支和兼容成本。
- 新建 `dex-node` 反而更清晰：把 Sequencer + MatchingEngine + RiskEngine + Storage 做成一个确定性状态机；Sui 只作为“可复用基础设施库”。

## 3. 与你当前问题的直接回答

### 3.1 实现 DEX 需要用到哪些模块？

- 见 `mynotes/dex/exec_layer/04_sui_modules_for_dex.md`，核心是：`sui-types`、`typed-store`、`sui-storage`、`shared-crypto`、`mysten-metrics`、`telemetry-subscribers`，以及（Phase 2）`sui-network`/`consensus-core`。

### 3.2 如果想在 Sui 中抽离模块实现 DEX，需要抽离哪些模块才可以把节点跑起来？

- **如果你指的是“保持 sui-node 形态，能跑起来”**：核心依赖基本就是上面的 **1.2**（`sui-node` + `sui-core` + `sui-types` + `sui-execution` + `sui-storage` + `typed-store` + 网络/配置/指标/加密）。
- **如果你指的是“做 DEX 节点，能跑起来”**：不要裁剪 `sui-node`，而是走 **方案 B（dex-node）**，抽离/复用最小集合见 **2.2**。
