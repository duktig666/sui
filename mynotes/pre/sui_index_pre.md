# DYDX Indexer
详细分析DYDX的Index机制。包括如何将DEX的订单簿、k线数据、订单数据、仓位数据等数据索引到链下？如何保持高性能？客户端应该如何进行连接，是连接的节点还是索引服务，总结出数据流。
可以参看 dydx-v4-chain/mynotes下和dydx-v4-chain/notes的部分资料
结论最后输出到 sui/mynotes/dex/analyst/dydx-indexer-analyst.md

---

1. 结合代码，详细分析dydx，哪些数据是On-chain Events，那些数据Off-chain Updates，结论补充道sui/mynotes/dex/analyst/dydx-indexer-analyst.md 
2. DYDX的数据流图不是很清晰，完善到文档 dydx-indexer-analyst.md 
3. DYDX On-chain Events和Off-chain Updates最终都会存储到psql吗？存储层的表和结构是如何设计的？ 完善到文档 dydx-indexer-analyst.md 
4. dydx-indexer-analyst.md 文档中 第6部分客户端连接方式梳理的api如果不全进行完善，每个api或ws标明下从什么存储中查询的。
5. dydx的索引服务是统一启动一份，还是每个节点配套启动一份？ 完善到文档 dydx-indexer-analyst.md
6. dydx区块回滚存储如何回滚？ 完善到文档 dydx-indexer-analyst.md
7. 第13部分，index专用全节点和普通节点什么区别？如何辨别


# sui-indexer-alt
1. sui-indexer-alt 是链下的单独服务吗？部署架构是怎样的？
2. sui-indexer-alt 是如何将数据写入的？
   完善到 sui-indexer-alt-analyst.md
---


# Sui Indexer
sui-indexer-alt 可以将链上数据按照Checkpoint维度索引到链下（机制参看 sui/mynotes/sui/analysis/sui_indexer_data_flow.md）。
dydx的indexer机制可以作为参考，参看文件 sui/mynotes/dex/analyst/dydx-indexer-analyst.md

Checkpoint是秒级约3s，对于高性能DEX来说，需要很多场景的数据延迟太高，比如说订单簿、k线数据、订单数据、仓位数据等等。
DEX链上部分如何将DEX需要的历史和实时数据发送给链下，是否要使用sui-indexer-alt的方式？
链下索引的数据库使用什么比较合适，要支持链上数据的高频写入，数据流支持亿级别，并支持高频查询？
考虑进行架构和方案设计

备注： dex基于sui开发，回答问题和设计方案，多倾向于sui
结论最后输出到 sui/mynotes/dex/analyst/dex-indexer-analyst.md

---

dydx-indexer-analyst.md 更新了大量的内容，结合dydx-indexer-analyst.md分析，并优化dex-indexer-analyst.md的设计

--- 

一些问题：
1. FastPath Listener 如何将数据存储到链下 
2. dex-indexer-analyst.md分析中 k线数据从psql替换为TimescaleDB，历史存储从psql替换为ClickHouse 有多大提升？如何还使用 psql进行分析利弊
3. 如果复用 sui-indexer-alt 服务，它用的什么存储？如果替换存储工作量评估，给出建议是否还要替换存储 完善到文档 dex-storage-vs-analysis.md
4. 链上层没有move合约，而是自定义的DEX引擎，dex-indexer-analyst.md 修改完善相关部分和架构部分等。 
---

详细分析sui-indexer-alt，并将结论输出到sui/mynotes/dex/analyst
基于dex-indexer-analyst.md 画出一张完善的结构图，输出到 sui/mynotes/dex/analyst

1. 架构图中客户端层连接的节点应该是Full Node，Index Full Node向Full Node同步数据，并发出索引事件
2. Move Events 改为 OnChainUpdate
3. 架构图中缺少对 数据具体存储那个存储 不清晰
完善架构图 dex-indexer-structure.md

---
dex-indexer-structure.md 中FastPath的存储路径和sui的FastPath机制没有关系吧

dex-indexer-analyst.md方案中TimescaleDB和ClickHouse该用psql，设计新的方案，输出到dex-indexer-analyst-psql-v2.md。dex-indexer-analyst.md保持不变。
dex-indexer-structure.md中TimescaleDB和ClickHouse该用psql，设计新的架构图，输出到dex-indexer-structure-psql-v2.md。dex-indexer-structure.md保持不变。

---

dex-indexer-structure-psql-v2.md 的双通道设计似乎可行，但是太过复杂。实时通道用途: 订单簿实时更新、活跃订单状态，数据缓存在Redis，不存储db。
先实现批量通道 (Checkpoint)，是否也可以完整实现功能，只是延迟较大。进行分析
---
第一阶段先实现Checkpoint-Only架构，主要进行功能性验证。
第二阶段实现双通道方案。
写出v3版本的方案，并输出到新的文件。
---
dex-ui/notes/hyperliquid/http 文件夹下是hyperliquid的api。
结合hyperliquid的api 完善 dex-indexer-structure-v3.md的架构设计。
最终暴露出去的api和hyperliquid的类似，主要是数据模型和端点。 架构文档可以没有这些。
再写一份技术方案，输出到 sui/mynotes/dex/tech/dex-indexer-tech.md。
在传统的技术方案基础上，重点包括 事件的定义，api的端点，数据模型，存储及表的定义 等等，可以再进行头脑风暴和发散，还要包括哪些重点内容，帮助写出更完善的技术方案。
技术方案中，还要扩展 sui-indexer-alt 的功能，如何解析dex定义的事件等。
先写计划，输出到 sui/mynotes/plan/dex-indexer-plan.md，计划评审后在写技术方案。
---
第一阶段的api可以考虑先使用sui-indexer的jsonrpc，后续再考虑是否使用restful。
API 端点设计 是否符合Hyperliquid的形式
---
计划使用原生rust开发DEX引擎，事件使用原生rust发出，这样sui-indexer-alt好像并不能直接索引，考虑这个问题如何解决，完善到计划文档。


# Sui Event
分析是否可以修改sui的代码，兼容原生rust代码去发出Event，结论输出到 sui/mynotes/dex/analyst/sui-event-rust-analysis.md