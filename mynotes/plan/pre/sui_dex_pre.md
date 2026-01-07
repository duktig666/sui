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
