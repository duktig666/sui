# DEX与SUI结合

**开发DEX和Sui可以结合的一些点**

参看notes和mynotes下的一些prd、数据结构、sui机制、dex机制和设计预想等文件，需求是想要用rust基于或者结合sui开发一个dex。dex的架构设想主要参看文件notes/dex_l1/drafts/dex-plan.md，是第一阶段使用中心化定序器（后续扩展为多节点轮换定序），不使用sui的共识，等到第二阶段再开发类似HyperEVM那样的使用sui共识。先回答第一阶段，该如何与sui结合，会使用sui的哪些模块、特性和内容？并给出如何实现这个dex的思路。  先写plan，输出到mynotes/plan/dex_use_sui_plan.md

补充1：

1. mynotes/dex/prd/DEX完整业务需求.md 文件是dex的完整需求，参看后完善看还有哪些是可以和sui结合，哪些需要自行rust开发。
2. Tonic Network - 验证者 P2P 通信 主要解决什么问题，sui网络层结合可以干什么？因为第一阶段目前主要是 中心化主节点定序，从节点只接受订单和tx同步给主节点处理。
3. 如果开发是新项目将sui以sdk方式引入（必要可以修改fork sui修改部分代码，但还是sdk引入使用的方式）。还是说直接在sui仓库基于sui开发。这两种集成方式哪种合适？

根据上述完善plan，输出到mynotes/plan/dex_use_sui_plan.md


补充2：

1. mynotes/dex/arch/DYDX_Cosmos_DEX架构.docx 是dydx+cosmos的dex架构设计，可以进行架构设计参考。
2. 根据dex_use_sui_plan.md plan，使用architect进行架构设计，输出到mynotes/dex/arch/sui_dex_arch.md
3. 根据dex_use_sui_plan.md plan，使用architect进行技术方案设计，输出到mynotes/dex/tech/sui_dex_tech.md

---

# DEX执行层如何落地
使用rust如何实现一个高性能、低延迟的dex，对标HyperLiquid。这是一个调研、设计架构和方案的话题。mynotes/dex/prd/DEX完整业务需求.md 是实现DEX的基本需求，可以分哪些模块，该用什么技术栈实现，技术路径是怎样的？最终目的是调研、架构设计和方案设计三步走并生成文件。

前期调研：
深入研究了DYDX基于Cosmos的实现，一些问题导致不可复用：
1. DEX流程建立在ABCI基础上，区块之前执行有PrepareCheckStatus阶段，在每个区块执行的期间需要重置状态，并且此阶段需要处理大量复杂业务，导致其TPS性能很差。
2. DYDX的其他ABCI仍需处理大量复杂业务，区块链执行层效率低下。
3. Cosmos的共识层效率不理想，但最主要的瓶颈仍然是执行层。

当前阶段的思考（重点）：Rust完全实现DEX是一项庞大复杂的工程，第一阶段先单节点实现DEX的执行层，后续再考虑是使用ZK的方式，还是结合sui的dag共识。

sui的一些思考和补充：
1. Sui的Object模型和FastPath以及并行执行应当可以大大提升DEX执行层的TPS。
2. Object模型和FastPath在第一阶段是否可以使用？哪些模块可以使用？怎么使用可以兼容第二阶段的zk或共识（Object模型好像可以sui共识有绑定，第一阶段可以使用吗？）
3. Sui虽然有DeepBook，可以借鉴其思想，但是其TPS和生态仍不满足需求，但是可以解释其机制和借鉴的点。
4. Sui还有哪些其他的特性可以用于DEX基本需求和模块的实现。

Reth：
1. Reth是EVM rust的高性能实现，应当有很多区块链设计可以供DEX实现去借鉴，但是我还没深入研究过Reth。
2. 更倾向优先借鉴和使用sui的特性，Reth机制作为备选，方便兼容后续接入sui的共识。

结合以上内容，Sui 和 Reth 有什么可以参考的点，使用Rust实现一个高性能、低延迟的dex，怎么实现？调研、架构设计和方案设计三步走并生成文件（不要进行代码开发）。

---

梳理SU1项目下各个模块的职责以及相互依赖和调用的关系（每个 rust模块都要梳理)， 必要时使用图来表示。
plan中再关注问题：
1. 实现dex需要用到哪些模块？
2. 如果想在sui中抽离模块实现dex，需要抽离哪些模块才可以把节点跑起来

---

基于Sui自研dex方式分析和推荐：
1. fork sui 开发自定义的dex引擎，dex交易走dex引擎处理，其他交易仍使用moveVm处理
2. 将sui当做依赖使用（必要时fork修改部分代码，仍作为依赖），创建新项目开发DEX专用链
3. 直接copy sui代码到新仓库，创建新项目开发DEX专用链。
最后要启动节点，符合区块链特征，符合DEX的tps和延迟的标准。分析并生成分析文件到 mynotes/sui/architecture。