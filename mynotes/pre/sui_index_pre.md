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