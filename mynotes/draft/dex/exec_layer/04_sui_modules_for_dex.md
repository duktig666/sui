# DEX 在 Sui 体系下需要复用/借鉴哪些模块

> 聚焦：**Phase 1 单节点 DEX 执行层**（你的当前阶段），并标注 Phase 2（接入共识 / ZK）需要补齐的模块。

## 1. 结论先行：实现 DEX 最少需要哪些 Sui 模块？

### 1.1 Phase 1（单节点、性能优先）推荐复用

- **基础类型与序列化**：`crates/sui-types` + `bcs`
  - Object/Transaction/Effects/Event 等数据结构可直接复用（保持未来兼容）
- **存储与缓存基础设施**：`crates/typed-store` + `crates/sui-storage`
  - RocksDB + typed DBMap；package/object cache/sharded lru 等可借鉴
- **加密与签名**：`crates/shared-crypto` + `fastcrypto`（workspace依赖）
  - 交易签名、意图域（Intent）等可直接复用
- **可观测性**：`crates/mysten-metrics` + `crates/telemetry-subscribers` + `tracing`
  - 指标与日志体系直接复用，便于对齐 Sui 的监控方式
- **配置模型（可选但推荐）**：`crates/sui-config` + `crates/sui-protocol-config`
  - 复用 NodeConfig/参数体系，减少工程分叉的维护成本

### 1.2 Phase 1（单节点）建议“不直接复用/只借鉴思想”

- **共识层**：`consensus/core`（`consensus-core`）
  - 单节点阶段不需要 2f+1，也不需要 DAG 共识；但 Phase 2 若要兼容 Sui 共识要回到它
- **Move VM 执行**：`sui-execution` / `sui-adapter-*`
  - 你的目标是 HyperLiquid 级性能，撮合/风控/清算核心必须是原生 Rust 引擎；Move 仅保留在“存取款/资产桥接”场景
- **DeepBook 业务实现**：`crates/sui-framework/packages/deepbook`（Move 包）
  - 作为机制参考，不适合作为性能路径（Shared Object + 共识 + Checkpoint 等等待）

### 1.3 Phase 2（接入共识 or ZK）会新增/强化的 Sui 模块

- **网络与 P2P**：`crates/sui-network` + `crates/mysten-network`
- **共识**：`consensus/core`
- **检查点/状态同步**：主要在 `crates/sui-core` 的 checkpoints/state-sync 相关子模块（可选择复用或重做）

## 2. 按 DEX 业务模块（PRD 12 模块）映射到 Sui 可复用模块

> 这里的“复用”指工程层复用（crate/库），不是直接把业务逻辑照搬。

| PRD 模块 | DEX 执行层的核心诉求 | 建议复用/借鉴的 Sui 模块 | 说明 |
|---|---|---|---|
| 账户模块 | 账户/子账户/余额/仓位状态机 | `sui-types`（Object/Owner/Version） + `typed-store` | 用 Owned Object 表达账户状态，版本用 Lamport 算法 |
| 资产模块 | 资产定义/精度/单位换算 | `sui-types` + `sui-framework`（Coin 模型仅参考） | Phase 1 可自定义资产模型；存取款仍可与 Coin 对齐 |
| 风控模块 | 保证金、风险参数计算 | `sui-protocol-config`（参数治理思路） | 风控逻辑必须原生；参数/配置体系可借鉴 |
| 上币与市场 | 市场配置、参数管理 | `sui-config`/`sui-types`/`typed-store` | Immutable 配置对象 + DB 持久化 |
| 预言机 | 喂价、mark price、指数价 | `sui-types::event`（事件）+ networking（Phase2） | Phase 1 可先走链下服务；事件结构沿用 Sui 体系 |
| 合约模块 | perpetual/合约参数/仓位 | `sui-types` + `typed-store` | Owned Object/表结构表达仓位与参数 |
| 撮合结算 | CLOB/撮合/结算/事件 | 仅借鉴 `sui-core` 的调度/缓存思路 | 撮合必须内存态+确定性日志；Sui 的 shared-object 不适合热路径 |
| 资金费率 | funding rate 周期结算 | `typed-store` + metrics | 结算是批处理+可回放，存储/指标复用价值高 |
| 清算模块 | 触发、处置、ADL/保险基金 | 参考 `mynotes/dex/analyst/*` + 事件/存储 | 业务逻辑自研；基础设施复用 |
| 手续费与分成 | 费率、返佣、分成 | `sui-types`（BalanceChange/Event） | 可复用事件/变更表达方式 |
| 交易奖励 | 积分/空投/奖励发放 | `typed-store` + `sui-types` | 状态持久化+事件输出 |
| Vault | LP/金库/收益分配 | `typed-store` + `sui-types` | 与账户/资产同类状态机 |

## 3. 工程建议：在 Sui 里做 DEX（Phase 1）更像“抽执行层基础设施”

- **最推荐复用的层**：类型(`sui-types`) / 存储(`typed-store`,`sui-storage`) / 加密(`shared-crypto`) / 可观测性(`mysten-metrics`,`telemetry-subscribers`)
- **最不推荐直接复用的层**：Move VM 执行路径（性能目标冲突）、DeepBook 业务实现（Shared Object 路径）
