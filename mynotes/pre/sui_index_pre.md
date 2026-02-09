# Other

## Claude.me 优化
dex目录下 主要用来分析前沿的区块链和DEX技术，实现低延迟高吞吐的DEX。
几个项目介绍：
- dex-sui 基于sui开发低延迟高吞吐的DEX，主要用来项目开发的工程。
- dex-ui DEX项目的前端工程
- dydx-v4-chain dydx的dex，用来分析和参考dex的一些设计和实现
- sui 区块链sui的源代码
notes和mynotes文件夹下是一些分析、架构和技术方案设计的文档。

将上述内容简洁地总结和整理到 dex/CLAUDE.md

回答问题和总结也使用中文 整理到 dex/CLAUDE.md

---
dex-sui是基于sui开发的低延迟高吞吐的DEX，目前正在开发阶段。 这个介绍整理到dex/CLAUDE.md
dex-sui/v4-chain 是dydx的实现和dydx-v4-chain项目一样，在进行实现分析时，可选择性忽略。 这个整理到dex/CLAUDE.md

---
mkdir open touch 等基础命令如果遇到权限问题，可以尝试加上sudo执行 这个整理到dex/CLAUDE.md

DEX API主要参考hyperliquid，事件的设计，包括OnChainUpdates和OffChainUpdates，主要参考dydx。
dex-indexer和dex-api的技术方案和实施计划要参看最新版本的文件。
这个整理到dex/CLAUDE.md

DEX API主要参考hyperliquid，hyperliquid的api文件在dex-ui/notes/hyperliquid/http 这个整理到dex/CLAUDE.md

sui/mynotes/draft 下是历史文档，代码实现不用参考 这个整理到dex/CLAUDE.md

---
sui/mynotes/dex/arch/dex-indexer-structure-latest.md 移动到 dex-sui/docs/indexer/arch 下
sui/mynotes/dex/plan/dex-indexer-implementation-plan-latest.md 移动到 dex-sui/docs/indexer/plan 下
sui/mynotes/dex/tech/dex-indexer-tech-latest.md 移动到 dex-sui/docs/indexer/tech 下
dex-ui/notes/hyperliquid/http 复制到 dex-sui/docs/indexer/hyperliquid/http 下

并更新dex-indexer任务相关的执行计划，修改CLAUDE.md 相关的文件引用

---
dex/CLAUDE.md 和dex-sui相关的内容 整理添加到 dex-sui/CLAUDE.md 其他git有关项目内容的规范暂不添加。

psql 优先使用docker方式去调试 这个整理到dex/CLAUDE.md
测试阶段 psql 使用docker compose 启动的  这个整理到dex/CLAUDE.md

测试阶段 psql以docker compose方式 安装在 ~/code/infra/psql 这个整理到dex/CLAUDE.md

---
回答和文档总结用总分总的格式 整理到dex/CLAUDE.md

---
如果有决策问题或者疑问点，及时弹出问题和询问，让我做出决策 整理到dex/CLAUDE.md
DEX核心逻辑使用rust编写自定义引擎，而不是使用move合约开发，这个理解误区 整理到dex/CLAUDE.md

有多任务的情况下，合理展示todo及完成情况 整理到dex/CLAUDE.md

---
如果任务中既有更新文档又有更新代码，根据情况更新文档的优先级大于更新代码。整理到dex/CLAUDE.md
定义简洁的rust规范 考虑加入到Claude.md 中，等我review后再加入

---
dydx-indexer-analyst.md dydx的indexer索引设计参考 整理到dex/CLAUDE.md

---
如果是多个阶段的连续性任务，每个阶段完成后进行总结，然后给我下一步或者下一阶段的实施建议。整理到dex/CLAUDE.md

## 多Claude code 的Git问题
多Claude code同时编写一个git仓库，可能遇到git冲突或者编译问题。 Claude的superpower或者git的worktree是否有解决的办法，给出最佳实践，并输出到一个文件。
https://github.com/obra/superpowers 这是superpower的代码仓库

## DEX UI
https://agents.md/ 对claude code和cursor的使用有什么帮助，建议的最佳实践
那么一个思路是 规范、建议和约束等 可以写在 AGENTS.md 中，CLAUDE.md和.cursorrules引用AGENTS.md，这样可以跨AI工具保持相同的规范，这样是否可行

那么建议的AGENTS.md CLAUDE.md和.cursorrules 都分别放什么内容合适？                 
基于dex-ui给出一个示例 先不要写具体内容



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

---
1. dydx OffChainUpdates和OnChainUpdates分别是在什么时机发出事件的？ 结论更新到dydx-indexer-analyst.md
2. cosmos有智能合约和事件吗？DYDX是否用了cosmos的事件


# Sui Event
分析是否可以修改sui的代码，兼容原生rust代码去发出Event，结论输出到 sui/mynotes/dex/analyst/sui-event-rust-analysis.md

# sui-indexer-alt
1. sui-indexer-alt 是链下的单独服务吗？部署架构是怎样的？
2. sui-indexer-alt 是如何将数据写入的？
   完善到 sui-indexer-alt-analyst.md

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

---
梳理下当阶段和任务完成的进展，推荐接下来该做什么事                                                    
什么时候可以进行阶段性的启动节点，发送交易，索引和api查询验证

---

sui-index-alt 的部署架构是怎样的，连接全节点还是验证者节点？每个节点都需要sui-index-alt吗，然后整体提供rpc服务，还是其他的方案？
结论输出到 sui/mynotes/sui 文件夹下


# Rust Indexer by DYDX
## 方案设计
DYDX的双通道索引机制，OffChainUpdates在订单薄更新后发出事件，解决了DEX订单薄等数据的低延迟问题，OnChainUpdates经过区块共识后发出事件，解决最终一致性和历史数据查询的问题。
如果采用 原生Rust自定义DEX引擎，OffChainUpdates在订单薄更新后发出事件与DYDX一致。
但是OnChainUpdates的实现背景有一定的区别，DYDX每次共识即区块确定后，即可发出OnChainUpdates的事件。但是sui不像传统区块链，没有区块的概念，对应的是CheckPoint机制，
CheckPoint的延迟相对是较高的，OnChainUpdates的实现有什么推荐的方案

针对上述背景和内容进行分析，结论输出到 sui/mynotes/dex/analyst/dex-indexer-full-by-dydx-analysis.md

---
1. 推荐架构那里 OffChainUpdates 成交记录和持仓变化应该不能更新，因为没有经过最终确认，可以再完善下 OnChainUpdates 更新哪些东西。
2. 如果采用双层 OnChainUpdates，分别都更新哪些事件
3. 双层 OnChainUpdates 与只在Checkpoint更新OnChainUpdates 有什么区别，进行利弊分析。
可以参看dydx的分析 dydx-indexer-analyst.md
完善执行计划

---
根据dex-indexer-full-by-dydx-analysis.md确认只使用CheckPoint处理OnChainUpdates。如果借鉴dydx自定义实现OnChainUpdates的话，sui-indexer-alt有可以直接借鉴的地方吗？
dydx的架构中还加入了kafka，对比sui-indexer-alt 还有现在的方案中是否要引入kafka

---
根据dex-indexer-full-by-dydx-analysis.md确认只使用CheckPoint处理OnChainUpdates。
第一阶段实现OnChainUpdates，第二阶段实现OffChainUpdates。
dex-ui/notes/hyperliquid/http 文件夹下是hyperliquid的api，最终暴露出去的api和hyperliquid的类似，主要是数据模型和端点。端点设计要符合hyperliquid的风格
部分分析可以参看 dex-indexer-tech-v2.md，先不引入kafka
在传统的技术方案基础上，重点包括 事件的定义，api的端点，数据模型，存储及表的定义 等等，可以再进行头脑风暴和发散，还要包括哪些重点内容，帮助写出更完善的技术方案。
再写一份技术方案，输出到 sui/mynotes/dex/tech/dex-indexer-tech-v3.md。

---
dex-indexer-tech-v3.md 结合sui的项目模块结构分析，要对哪些模块进行更改，新增哪些模块
借鉴内容的复用，是采用引入代码的方式，还是重新去实现
dex-sui 是fork sui的仓库增加dex的实现，dex-indexer也要将在这个仓库实现，那么这个角度来看，是引入sui-indexer-alt的部分代码去实现，还是重新开发新的模块。
结论输出到文件dex-indexer-tech-v3.md

dex-indexer-module-analysis.md 模块设计总结后输出到 dex-indexer-tech-v3.md

## 实施规划
总结性简要分析 dex-sui 中如何基于sui实现dex，都做了哪些改动？ 输出到 sui/mynotes/dex-sui/analysis/dex-sui-summary.analysis.md

---
dex-indexer-tech-v3.md 是dex-indexer实现的方案，在dex-sui下进行实现。
dex-sui/crates/sui-e2e-tests/tests/dex_order_tests.rs 是dex下单的一个测试
dex-sui/crates/sui-e2e-tests/tests/dex_subaccount_tests.rs 是dex账户相关的一个测试
细粒度进行实施规划，每个小阶段要进行测试，规划可以详细些。

每个阶段完成后，要进行测试和review，完成后再进入下一个阶段。阶段完成后要更新dex-indexer-implementation-plan.md的任务完成状态

事件如何再DEX引擎发出？CheckPoint什么标志可以代表DEX的事件？DEX-index如何解析CheckPoint的DEX事件？  
这个进行问题先进行回答，结论输出到dex-indexer-tech-v3.md，然后看是否要再完善实施计划

每个阶段完成后再进行一个总结，总结输出到sui/mynotes/dex/summary下的一个文档。

---
----------------- 更换方案 使用sui事件+sui-indexer扩展 ----------------------------
dex-indexer-tech-v4.md 内容不够详细，参看 dex-indexer-tech-v3.md 内容以及格式 进行完善。
v4与v3的主要区别是OnChainUpdates部分复用sui-indexer-alt而不是自建。

---
dex-indexer-tech-v4.md 是dex-indexer实现的方案，在dex-sui下进行实现。
dex-sui/crates/sui-e2e-tests/tests/dex_order_tests.rs 是dex下单的一个测试
dex-sui/crates/sui-e2e-tests/tests/dex_subaccount_tests.rs 是dex账户相关的一个测试
细粒度进行实施规划，每个小阶段要进行测试，规划可以详细些。实施计划输出到 dex-indexer-implementation-plan-v2.md
阶段完成后要更新 dex-indexer-implementation-plan-v2.md的任务完成状态
每个阶段完成后再进行一个总结，总结输出到sui/mynotes/dex/summary下的一个文档。

发出的事件是否可以被indexer正确索引，可以在中间阶段先使用下单进行测试验证，然后再完善整个事件索引部分。
先输出执行计划，然后再执行。

---
每个阶段执行完别忘了进行总结输出到sui/mynotes/dex/summary下的一个文档。
dex-indexer-implementation-plan-v2.md 实施计划中每个阶段的完成状态要更新，如果有遗留问题也总结进去。
如果需要真实安装和连接psql可以考虑写入实施计划，考虑通过docker-compose的方式运行。

分析是否需要额外安装psql，sui-indexer-alt 也是需要连接psql的原来是如何做的？先分析解决这个问题，如果有必要可以更新技术文档和实施计划文档。 然后再继续向后执行。
psql我已经在其他地方启动，可以直接连接。继续后续任务

---
继续实现 PositionsHandler

---
simtest 事件测试代码在哪里？测试了什么？是使用sdk发送交易出现了问题吗？
当前实施计划执行到了哪个阶段？
什么时候可以运行节点，发送真实交易的方式去验证dex事件的发出，CheckPoint对DEX交易的打包，index对dex交易的解析并存储，API的查询等链路的真实测试。
这样可以先进行完善的阶段性测试，论证通过后在进行下一步的实施。

dex-node-test 是之前写的测试，目的是通过真实的下单，发送给真实的节点去执行，然后去验证。
如下是节点运行和验证的命令。dex-node-test不过是早期写的，不一定符合现在的开发设计情况，分析验证和修改，接下来第9阶段不通过e2e测试，而是要真实性对节点测试。
同时考虑index相关程序如何运行的问题

这是节点执行的相关命令 是否有需要修改的 启动节点是否需要添加dex相关的日志标志



# dex-sui

## 分析
分析dex-sui，dex引擎是自定义开发，现在是否可以启动节点，发送对应的交易？ 结论输出到sui/mynotes/dex-sui/analysis的一个新文件

dex-sui切换了添加dex自定义事件的分支，分析都在哪里添加了自定义的事件，分析事件添  
加到CheckPoint是否正确？目前sui-indexer-alt还没有改造解析dex的自定义事件，分析可  
行性 结论输出到sui/mynotes/dex-sui/analysis的一个新文件 

根据 dex-event-implementation-analysis.md 对dex-sui进行修复验证和测试

---
当前dex-sui的dex是否可以启动节点方式，发送dex下单交易？ 是否可以写出针对节点的下单交易的示例 结论在sui/mynotes/dex-sui/analysis输出新的文件
在dex-sui/crates下新建一个模块 dex-node-test 用rust写测试调用启动的节点发送下单交易（考虑怎么下单成功，是否需要币对，充值等前置问题）

---
cargo run -p dex-node-test --example place_order -- --fullnode-url http://127.0.0.1:9001 失败         
=== Step 4: Placing limit order ===
Error: ErrorObject { code: InternalError, message: "Internal error", data: None }

Caused by:
    ErrorObject { code: InternalError, message: "Internal error", data: None }
如果Placing limit order实现有问题可以考虑先解决，很难解决考虑Creating perpetualmarket和Deposit添加一个用来临时测试的事件 用来验证全流程。   

---
BalanceUpdateEvent 是否有和hyperliquid info相关的api，如果没有 写个测试的api，测试下api服务的可用性

有时需要重新部署节点从0开始运行，并初始化indexer数据库的状态，帮我写出执行命令到一个sui/mynotes/dex下的一个文档。如下是已知现在的执行命令

---
dex-indexer的开发当前已经有了阶段性成果，BalanceUpdateEvent实现了全流程的验证，place_order在创建subaccount和deposit后，
BalanceUpdateEvent可以被正常的索引，以及userBalances可以被api访问。
现在帮我整理输出一个文档，介绍如下内容：
1. 当前阶段dex-indexer是如何实现的，都改了哪些内容？
2. 关键设计和实现的介绍和分析
整理一个文档到 dex-sui/docs/indexer 下

---
dex-indexer 是一个新的服务，和sui-indexer-alt的关系是什么？
Watermark 机制 每个事件是独立的进度还是全局进度，dex-indexer会和sui-indexer-alt 共享进度吗？
dex-indexer中每个事件为什么要独立Watermark进度？为什么不共享？

---
拉取了新的代码placeOrder e2e可以测试通过，下单仍有问题
=== Step 4: Placing limit order ===
Error: ErrorObject { code: InternalError, message: "Internal error", data: None }

Caused by:
ErrorObject { code: InternalError, message: "Internal error", data: None }
再去进行分析

---
DEX API主要参考hyperliquid，事件的设计，包括OnChainUpdates和OffChainUpdates，主要参考dydx。  

dex-indexer 和 dex-api 可以模块分离，考虑在 crates 下新建模块，秉承 模块职责分离的思想 看实施计划各阶段的模块文件夹如何设计调整？包括 双通道的dex-indexer、ws阶段和dex-api
根据上述分析内容 修改技术方案和实施计划

--- 
dex-node-test 下编写发送交易给节点的真实交易 完善现有功能事件真实交易的测试
再建一个模块 dex-indexer-e2e-test 参考 sui-e2e-tests 和 sui-indexer-alt-e2e-tests 对已实现功能进行e2e测试（除了成交相关的事件，现在成交有问题） 
sui-e2e-tests下如果有dex-indexer的测试也迁移到 dex-indexer-e2e-test

---
dex-indexer新加的Event的在 dex-node-test添加真实发送给节点的交易示例，并给出如何验证事件索引和api查询的命令 完善文档到 sui/mynotes/dex/e2e/fresh-deploy-guide.md

是否因为事件定义做了修改，与数据库的表结构不对应了，可以和分支feat-dex-indexer分支的代码作对比。以及API的查询是否还与之前的方式对应进行审查。 

---
dydx和hyperliquid 如何设计主账户与子账户 在不同场景下api查询即数据存储的问题的 

现在的subaccount是根据dydx进行设计的，导致索引的数据和api不兼容，dex-indexer和dex-api的设计需要一些兼容。当然api的设计尽可能和hyperliquid保持一致。
数据库字段 subaccount 可以拆分为 account_address 和 subaccount_number。
审查merge后的代码事件、api、数据库存储的设计是否合理？另外api中哪些需要根据account_address查询，那么些需要根据account_address+subaccount_number查询？
推荐account_address+subaccount_number查询的字段设计。

技术方案和实施计划进行修改，审查后进行代码开发

# DEX Write API


# DEX Indexer Phase2
dex-indexer-tech-latest.md 技术方案中对于Phase2的设计是否够完善？先进行分析不要更改内容

---
参看hyperliquid的文档，类似dex-sui/docs/indexer/hyperliquid/http的方式梳理ws的请求方式，参数响应等内容需要解释。输出到dex-sui/docs/indexer/hyperliquid/ws的一个新文件

---

dex-realtime 缺失内容问题：
1. 订阅 API 选择，sui_subscribeEvent 还是 sui_subscribeTransaction 有什么区别？
2. 事件过滤条件的MoveEventType 过滤器和 Package ID？ dex-realtime 主要参考dydx，解决订单薄，最新成交等需要低延迟快速访问的数据，这个问题还需要提供什么？
3. 断线重连策略 参看dydx是否有对应的机制
4. 事件反序列化 是否还需要用move的事件
5. 节点订阅 dydx是有专门索引的全节点发出事件，那么我们推荐怎么做？
6. Redis Stream 发布 的问题 都可以先参看dydx是怎么做的？是否有问题或者优化 再让我来决策
7. dex-ws  订阅频道管理 Redis 消费  缺失内容 也先参看dydx。另外分析下 当前的设计和dydx是否有重大差异，是否有需要改进的
根据上述缺失内容，我提出了一些新的问题和建议，在进行分析和梳理，将问题缺失项和结论，或者需要我再做决策的，输出到sui/mynotes/dex/analyst/phase2_realtime下的一个新文件
另外 dex-realtime 需要处理的事件以及设计在dex-sui/docs/indexer/tech/dex-indexer-tech-latest.md文档下并不完善，再进行方案版本文件过度的时候有大量方案设计缺失，
可以参看sui/mynotes/dex/analyst/phase2_realtime下的文件进行补充完善，尤其是dex-indexer-tech-v4.md，但是之前废弃的方案和设计不要重复引入。尤其是双通道各自需要处理的事件，现在并不明确。
如果有需要我进行决策的及时给我选项，让我选择。

---
三个关键问题：
1. 多用户并发下单 - 大量用户同时下单，保证实时数据推送的正确性
2. 多 realtime 实例部署 - 高可用部署，避免单点故障
3. Sui 节点连接 - 连接单个节点是否能获取全量事件？
4. dex-realtime 连接单节点在没有CheckPoint前，应该并不知道别节点的事件，然后dex-realtime才是解决dex高频信息如订单薄，最近成交等数据的关键，这点是否会有重大影响。

---
全面审查indexer现在的架构设计、技术方案、实施计划。看有什么地方需要优化，未决策问题还有哪些？先不要进入开发                            

主文档节点连接策略 使用 Validator+索引节点
当前阶段延迟目标500ms左右
Validator 同步方案 内网RPC
多实例去重方案 幂等发布
补充实现细节
DEX Engine 乐观事件 暂不实现

相应改动也需要同步改动主文件

---
针对链下订单薄推送的问题，现在有两个方案
1. 保持当前方案，链上发出事件，dealtime服务实时构建订单薄
2. 链上内存订单薄，250ms推送一次给链下服务，每次推送完整订单簿。避免链下重建订单薄的繁琐流程
针对这两种方案进行分析

---
现在indexer dealtime实现中sui_subscribeEvent在验证者节点的共识之后，CheckPoint之前就可以把事件推送出去，存在了一定的疑问，可以通过怎样的方式进行一下验证。

---
修改源码启用 Validator RPC（需要注释掉那3行检查代码）
使用方案1 但是验证成功后要及时提醒我撤销启用 Validator RPC的修改 

---
redis的缓存有哪些服务维护，机制是什么？整体架构是怎样的？redis要写入哪些事件或数据？
先更新文档不要修改代码，代码等我文档review后再执行
现在的技术方案、架构、实施计划在sui/mynotes/draft/indexer合适的文件夹下做一下备份，文件名记得有版本后缀
现在Phase2整体的实施计划是怎样的？

---
当前方案已经足够开始开发。上述补充内容可以在开发过程中逐步完善：
1. 可立即开始：dex-indexer Redis 发布功能开发
2. 并行准备：部署和运维方案（可以边开发边准备）
3. 开发中完善：错误处理策略（遇到问题时记录）
4. 上线前补充：性能测试、安全审查

---

phase2阶段我该如何测试和验证，给我一个测试列表。并且我更关注运行节点、indexer、api 然后真实测试。
dex-node-test 是否需要给我增加更多的真实交易验证的测试用例
评估测试命令 dex-sui/docs/indexer/test/index-test.md 是否需要修改和完善
phase2_realtime 生成一个详细的测试指南

# DEX API
对标Hyperliquid，梳理现在indexer和API已实现的功能，还缺少的功能（如果是因为dex没有实现而没实现的标记出来）。 总结到phase2_realtime文件夹下


# DEX Indexer Test Deploy
dex-sui/docker/dex-indexer/docker-compose.yml 完善redis的compose配置，psql和redis的配置修改为当前目录下新建文件夹的映射存储。

# DEX Verify Demo
dex indexer 和 api 已经完成阶段性的开发，想要一个demo ui来进行完整性的功能测试，以及方便演示。
我又几个思路：
1. 开发一个命令行ui工具，dex核心功能可以在终端展现。
2. dex-ui项目的目的是复刻Based UI，与HyperLiquid API打通，提前完善DEX前端，等正在开发DEX完善后对接。 可以考虑在其基础上新建分支开发验证，但是其比较复杂，是否可以快速完成测试验证，需要分析。
3. 新建一个项目，快速开发一个前端demo用来验证。
4. 新的方式你可以帮我补充
方案推荐

---
indexer k线 成交 时间的字段是否有问题 包括dex-test-panel的展示

---
按照dex-sui/docs/indexer/hyperliquid下，整理的hyperliquid api和ws规范，将我们现在实现的api和ws规范整理到dex-sui/docs/indexer/api_docs文件夹下。
api的介绍，参数和响应的返回值，都要有一定的说明。 也定义新的环境变量。