## DEX L1 需求与详细设计评审（GPT）— 2025-12-31

### 0. 评审范围与输入

- **评审范围**：`notes/dex_l1/` 目录下的“需求”和“详细设计”（含系列文档与汇总设计文档），聚焦 Phase 1/Phase 2（现货 + 核心基础设施）可落地性与一致性。
- **评审目标**：发现需求/设计之间的缺口与矛盾、识别关键风险与未决策点、给出可执行的改进建议与下一步清单。

#### 0.1 评审材料清单

- **需求**
  - `notes/dex_l1/docs/01-REQUIREMENTS.md`（v1.0，Draft，最后更新 2025-01-01）
- **详细设计（汇总）**
  - `notes/dex_l1/dex-l1-detailed-design.md`（v1.0.0，最后更新 2025-01-XX，约 4k+ 行，含网络/API/安全/性能章节）
- **详细设计（分册）**
  - `notes/dex_l1/docs/02-ARCHITECTURE-OVERVIEW.md`
  - `notes/dex_l1/docs/03-ABSTRACTION-DESIGN.md`
  - `notes/dex_l1/docs/04-SEQUENCER-DESIGN.md`
  - `notes/dex_l1/docs/05-MATCHING-ENGINE-DESIGN.md`
  - `notes/dex_l1/docs/06-STORAGE-DESIGN.md`
  - `notes/dex_l1/docs/07-MOVE-INTEGRATION-DESIGN.md`
  - `notes/dex_l1/docs/08-SPOT-OVERVIEW.md`（最后更新 2024-01）
  - `notes/dex_l1/docs/09-PERPETUAL-OVERVIEW.md`（最后更新 2024-01）
  - `notes/dex_l1/docs/10-PERFORMANCE-DESIGN.md`（最后更新 2024-01）
- **实现状态/约束**
  - `notes/dex_l1/CLAUDE.md`（“编码宪法”：禁止 `unwrap/expect`、关键路径禁止同步 I/O、性能红线等）
  - `notes/dex_l1/DEX_L1_IMPLEMENTATION_STATUS.md`
  - `notes/dex_l1/dex-plan.md`

---

### 1. 总体评价（结论摘要）

#### 1.1 亮点

- **文档体系完整**：需求（FR/NFR/用例/追踪矩阵/风险）+ 分模块设计（Sequencer/撮合/存储/Move 集成/性能）+ 汇总详细设计（含网络/API/安全/性能）。
- **架构方向清晰**：Fast Path（Sequencer + Native Engine）/ 标准 Sui Path（Mysticeti + Move VM）/ Hybrid Path（存取款等）。
- **复用原则明确**：强调复用 `typed-store`、`mysten-network`、`shared-crypto`、`mysten-metrics` 等，符合 Fork 可维护性目标。

#### 1.2 主要结论

- **需求与设计“方向可行”，但“口径/一致性不足”**：多处出现相互冲突的指标定义、执行路径、Sui 集成策略与持久化策略描述，导致无法严格判断 Phase 1 是否满足 P0 需求与验收标准。
- **阻断项（P0）集中在**：指标口径统一、持久化/RPO 定义、Hybrid 原子性闭环、Sui Core 修改策略选择、安全模型补齐。
- **建议**：先做一次“文档收敛/对齐”的小迭代（1~2 天），把关键决策与不一致点统一后再推进实现与评审，否则后续实现会不断返工。

---

### 2. 关键不一致与缺失（按优先级）

> 说明：P0 = 会阻断上线/落地；P1 = 重要但可后续迭代；P2 = 建议优化。

#### 2.1 P0（阻断项）

- **P0-01 指标口径/目标值在多文档不一致**
  - **吞吐量**：`01-REQUIREMENTS.md`（峰值 ≥200K TPS、持续 ≥100K TPS） vs `dex-l1-detailed-design.md`（吞吐量 100K TPS） vs `DEX_L1_IMPLEMENTATION_STATUS.md`（目标 100K TPS） vs `notes/dex_l1/CLAUDE.md`（目标 TPS 200K）。
  - **软/硬确认延迟**：`01-REQUIREMENTS.md`（软 <50ms，硬 <100ms） vs `DEX_L1_IMPLEMENTATION_STATUS.md`（软 <100ms，硬 <500ms） vs 其他设计文档（多处写 <100ms）。
  - **撮合延迟定义**：有的写“单次撮合 <10μs”（算法操作级），有的写“撮合延迟 P99 <50ms”（请求级，从接收订单到撮合完成），需要明确“测量点/口径/分位数”。
  - **建议**：在 `01-REQUIREMENTS.md` 中固化“指标字典”（metric name + measurement point + pXX + 条件），并在所有设计文档引用同一套定义（可在每篇文档顶部写“引用指标：NFR-PERF-xxx”）。

- **P0-02 “关键路径禁止同步 I/O”与“WAL/持久化保证(RPO=0)”描述冲突**
  - `notes/dex_l1/CLAUDE.md`：**禁止关键路径同步 I/O**。
  - `01-REQUIREMENTS.md`：RPO=0，机制写“WAL + 同步复制”。
  - `06-STORAGE-DESIGN.md`：设计原则写“异步持久化，不阻塞主路径”，但 WAL 伪代码又出现 `write_all + sync_data()`（同步刷盘）。
  - **必须先澄清的决策**（建议写成 ADR）：
    - 软确认是否允许 **异步持久化**（存在少量丢单风险）？如果允许，需要把风险写进需求并定义“软确认语义”。
    - 硬确认是否要求 **durable（fsync/复制完成）** 才返回？如果要求，硬确认延迟目标是否仍可达 <100ms？
  - **建议**：把“软确认/硬确认”分别绑定到“持久化等级”，形成清晰的语义层级（例如：Soft = replicated in-memory quorum；Hard = fsync+quorum；Final = checkpoint）。

- **P0-03 Sui 集成策略存在互斥方案且未收敛**
  - `dex-plan.md` 与 `DEX_L1_IMPLEMENTATION_STATUS.md`：倾向**直接修改** `crates/sui-core/src/authority.rs`、`sui-execution/latest/...` 等。
  - `03-ABSTRACTION-DESIGN.md`：提出“**依赖注入，不修改 authority.rs 源码**”并新增 `sui-core-ext` 等扩展 crate。
  - 两条路线都会影响 Fork 维护成本与实现复杂度，必须择一或明确混合策略（哪些必须改 upstream，哪些可扩展实现）。
  - **建议**：补一个“改动面清单 + 维护策略”并成为单一真相来源（建议放在 `dex-l1-detailed-design.md` 或新增 ADR 文档）。

- **P0-04 Hybrid（存取款）原子性闭环描述不一致**
  - `07-MOVE-INTEGRATION-DESIGN.md` 同时出现“**事件监听更新余额**”与“**DEX 更新失败可通过 Effects 回滚**”两种表述。
  - 若依赖“链下事件监听”，则无法保证与 Move 转账在同一事务内原子；若在 Authority 内“Move 执行后回调 DEX、合并 Effects”，则需要明确：
    - 回调触发点（在哪个执行阶段？）
    - 如何生成/合并 `TransactionEffects` 的关键字段（对象版本、事件、gas 等）
    - 失败回滚模型（Move 成功但 DEX 更新失败时，如何回滚？是否必须使整个交易失败？）
  - **建议**：把存款/取款做成“单事务内的 Hybrid 执行协议”并给出状态机与失败矩阵（Success/Fail 组合）。

- **P0-05 安全模型覆盖不足（目前仅覆盖签名与限流的“薄层”）**
  - `dex-l1-detailed-design.md` 的“安全性设计”目前主要是：签名验证 + 速率限制。
  - 需求/架构里已出现的关键风险未形成可执行设计：Sequencer 作恶（排序操纵/审查/双花式软确认回滚）、重放与 nonce 语义、MEV/抢跑防护、密钥管理、管理员权限模型、多签/治理、审计与回滚策略、DoS（含内存/磁盘/网络）等。
  - **建议**：补齐“威胁建模（Threat Model）+ 安全不变量（Invariants）+ 缓解措施落点（代码/配置/运营）”，至少覆盖 Phase 1 的 P0 风险。

#### 2.2 P1（重要改进项）

- **P1-01 订单簿索引与撤单复杂度/正确性风险**
  - 多处设计使用 `PriceLevel.orders: VecDeque<Order>` + `OrderLocation { index }` 的组合；当 `pop_front()` 等操作发生时，`index` 的稳定性容易失效（或需要高成本修正）。
  - 建议明确撤单的目标复杂度（O(1)/O(log n)/摊还）与可接受实现（链表节点、slot-map、arena + stable handle、或每 price level 使用 intrusive list）。

- **P1-02 数值精度/溢出与舍入规则需统一**
  - `05-MATCHING-ENGINE-DESIGN.md` 使用 `u64` 计算 `notional = quantity * price`；在高精度与大额场景存在溢出风险。
  - `08-SPOT-OVERVIEW.md` 定义了舍入规则（手续费向上、输入向下等），但其他文档/伪代码未一致引用。
  - 建议统一为“定点数类型 + 明确的舍入策略 + 全链路约束（tick/step/min_notional）”。

- **P1-03 API/RPC/WebSocket 需要补齐工程化要素**
  - `dex-l1-detailed-design.md` 已给出 REST/OpenAPI 雏形与 WS 消息类型，但缺少：鉴权（钱包签名/会话）、幂等（clientOrderId 语义）、分页与游标、错误码统一、版本化策略、限流策略与返回码、兼容 Sui JSON-RPC 的边界说明。

- **P1-04 观测与指标：示例代码与“编码宪法”冲突**
  - 多篇文档的 metrics 示例使用 `unwrap()/expect()`（即使是示例，也与 `notes/dex_l1/CLAUDE.md` 的“绝对禁令”冲突，容易在实现时被照搬）。
  - 建议：文档示例代码要么遵循禁令，要么明确标注“伪代码/示例，不可直接复制到生产代码”。

#### 2.3 P2（可选优化项）

- **P2-01 文档版本/更新时间不一致**
  - `08/09/10` 为 2024-01；其余为 2025-01-01 / 2025-12-30；建议补“变更历史”并标注哪些内容已过期/被 `dex-l1-detailed-design.md` 覆盖。

---

### 3. 逐主题评审意见（高信号）

#### 3.1 需求文档（`01-REQUIREMENTS.md`）

- **优点**
  - FR/NFR 分层清晰，有验收标准与优先级。
  - 有“需求追踪矩阵”，能落到设计文档与模块。
  - 约束/假设/风险表较完整，有利于后续决策对齐。
- **建议补强**
  - **验收标准可测试化**：为 P0 NFR 给出明确的压测方法、环境、流量模型与统计方式（例如：P99 的窗口、采样、剔除规则）。
  - **确认语义定义**：Soft/Hard confirmation 的一致性、可回滚性、与持久化/复制的关系需要写入需求层，避免在设计层争论。
  - **管理/权限模型**：FR-ADMIN 目前较粗，建议明确管理员身份与权限授予/撤销、审计日志需求、紧急暂停影响面。

#### 3.2 Sequencer（`04-SEQUENCER-DESIGN.md` + 汇总设计）

- **需要明确的关键点**
  - **确定性**：Standby 节点重放批次时的确定性来源（同输入同输出）依赖哪些不变量（时钟、随机数、价格源等）。
  - **Leader 作恶模型**：Leader 可通过排序/审查获利，硬确认如何约束其行为？需要更清晰的“可证伪/可惩罚/可恢复”设计（至少：检测 equivocation、拒绝非法 batch、强制切换）。
  - **状态同步**：切换时“从 DA 层获取最后确认序列号”的语义需和持久化策略对齐（DA 是必须组件还是可选？与 Sui checkpoint 的关系？）。

#### 3.3 撮合引擎（`05-MATCHING-ENGINE-DESIGN.md`）

- **正确性与边界条件**
  - **撤单/修改单**：索引结构需要保证在频繁撮合、pop_front、批量撮合时撤单定位不失效。
  - **自成交/风控**：`08-SPOT-OVERVIEW.md` 提及“自成交拒绝”等风控点，但撮合引擎详细设计中未落地（需要明确发生在撮合前还是撮合后、如何处理 maker/taker 同账户等）。
  - **手续费与结算**：建议统一资产扣费规则（从 quote 扣还是 base 扣），并明确 rounding 与最小单位。

#### 3.4 存储（`06-STORAGE-DESIGN.md` + 汇总设计）

- **关键风险**
  - **异步持久化与一致性**：如果软确认先返回而 WAL 未落盘，崩溃恢复将出现“已对用户确认但未持久化”的不一致，需要明确定义这是否允许，以及如何通过硬确认/客户端重试/补偿处理。
  - **快照流程“暂停写入”**：暂停窗口多大、是否会影响 50ms SLA、如何做增量快照避免长暂停，需要补工程细节。

#### 3.5 Move 集成（`07-MOVE-INTEGRATION-DESIGN.md`）

- **需要补齐**
  - **DEX 包地址/版本治理**：`DEX_PACKAGE_ID` 作为识别依据，升级/迁移如何处理？是否允许多个版本并存？
  - **Gas/费用模型**：DEX 原生路径如何计费与限流，避免免费 spam。
  - **Hybrid 原子性**：建议将“事件监听”表述替换为“读取 Move Effects 后在同一执行上下文内回调 DEX”，并明确失败回滚机制。

#### 3.6 API/网络（汇总设计第 9/10 章）

- **API 设计建议**
  - 把 REST/WS 与“链上交易提交（PTB/JSON-RPC）”的职责边界讲清楚：哪些是撮合指令（必须签名上链/走 authority），哪些是纯查询（可走 fullnode/索引服务）。
  - 增加：错误码与 HTTP 状态码映射、幂等语义（clientOrderId）、分页/游标、版本控制（`/api/v1` 的升级策略）。

---

### 4. 建议的“对齐/收敛”行动清单（可直接执行）

#### 4.1 P0（本周内建议完成）

- **A1 统一指标字典**：在 `01-REQUIREMENTS.md` 增加“指标口径表”（测量点、统计方式、条件），并在其它设计文档引用该表。
- **A2 明确确认语义与持久化等级**：补 ADR（或在汇总设计中新增小节）定义 Soft/Hard 对应的 durability/replication 条件。
- **A3 选定 Sui 集成路线**：明确“必须修改的 upstream 文件清单” vs “可扩展实现”，并与 `03-ABSTRACTION-DESIGN.md`/`dex-plan.md`/实现状态文档对齐。
- **A4 定义 Hybrid 原子性协议**：输出存取款状态机 + 失败矩阵 + Effects 合并规则。
- **A5 安全建模补齐**：最少完成 Sequencer 作恶/审查/重放/DoS 的威胁建模与缓解落点。

#### 4.2 P1（两周内建议完成）

- **B1 撤单索引方案落地**：明确订单句柄/索引的稳定性设计，补复杂度与内存成本评估。
- **B2 精度与溢出策略统一**：定义统一定点数类型与 rounding；所有 notional/fee 使用安全宽位计算（如 `u128`）。
- **B3 API 工程化补齐**：幂等/分页/错误码/鉴权/版本策略。
- **B4 文档示例与“编码宪法”一致**：将示例中的 `unwrap/expect` 替换为可传播错误或明确标注“不可直接复制”。

---

### 5. 需要产品/架构决策确认的问题（开放问题清单）

- **Q1**：软确认是否允许在“未 durable 落盘”前返回？如果允许，最大可接受丢失窗口是多少？客户端如何感知与补偿？
- **Q2**：硬确认的定义到底是“2f+1 内存确认”还是“2f+1 + durable（fsync/复制）”？对应的延迟目标是否需要调整？
- **Q3**：Sequencer 的中心化程度与治理方案：是否计划多 Sequencer、轮换、惩罚、或提交加密（anti-front-run）？
- **Q4**：DEX 原生路径的 gas/费用/限流模型：如何避免免费高频 spam，同时保持与 Sui 兼容？
- **Q5**：存取款托管账户（`@dex_custody`）的权限与管理：多签？治理？升级/迁移如何进行？

---

### 6. 结语

整体设计方向（Sui Fork + Native Engine + Sequencer + 双确认）是清晰且有可行路径的；当前最大的风险并非“缺少模块”，而是**关键语义与目标口径尚未收敛**，会导致实现与测试阶段频繁返工。建议优先按本评审的 P0 清单做一次文档对齐与决策落盘，再进入下一轮实现与正式评审。


