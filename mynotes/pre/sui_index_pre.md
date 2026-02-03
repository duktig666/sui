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

## 启动节点

```sh
cargo build --bin sui 

# 清空所有表
sudo docker exec dex-indexer-db psql -U dex -d dex_indexer -c "TRUNCATE TABLE dex_balances, dex_fills, dex_watermarks, watermarks CASCADE;"

# 1. 编译 sui 二进制
cargo build -p sui -p dex-indexer -p dex-api -p dex-node-test  

sudo rm -rf local-network-config/**
rm -rf ~/.sui/sui_config/network.yaml
rm -rf ~/.sui/sui_config/authorities_db/
rm -rf ~/.sui/sui_config/consensus_db/

# 2. 初始化本地网络配置
export SUI_CHAIN_DIR="$PWD/local-network-config"
sudo ./target/debug/sui genesis \
  --with-faucet \
  --committee-size 1 \
  --working-dir "$SUI_CHAIN_DIR" \
  --force \
  --epoch-duration-ms 30000
  
# 3. 启动本地验证者节点 (带水龙头)
export SUI_CHAIN_DIR="$PWD/local-network-config"
sudo RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start \
    --network.config "$SUI_CHAIN_DIR" \
    --with-faucet=0.0.0.0:9123 \
    --fullnode-rpc-port 9001

# 临时启动节点  
RUST_LOG="off,sui_node=info" \
  ./target/debug/sui start --with-faucet --force-regenesis --fullnode-rpc-port 9000
  
  
cargo run -p dex-node-test --example place_order -- --fullnode-url http://127.0.0.1:9001  
```

```text
启动顺序:                                                                                               
  终端: 1                                                                                                 
  服务: Sui 节点                                                                                          
  命令: sui start ...                                                                                     
  类型: 长期运行                                                                                          
  ────────────────────────────────────────                                                                
  终端: 2                                                                                                 
  服务: dex-indexer                                                                                       
  命令: ./target/debug/dex-indexer --database-url ... --rpc-api-url http://127.0.0.1:9001                 
  --first-checkpoint                                                                                      
    0                                                                                                     
  类型: 长期运行      
  RUST_LOG=dex_indexer=debug,sui_indexer_alt_framework::pipeline::logging=info ./target/debug/dex-indexer --database-url postgres://dex:dex123@localhost:5432/dex_indexer --rpc-api-url http://127.0.0.1:9001 --first-checkpoint 0                                                                                     
  ────────────────────────────────────────                                                                
  终端: 3                                                                                                 
  服务: dex-api                                                                                           
  命令: ./target/debug/dex-api --database-url ... --api-listen-address 0.0.0.0:3000                       
  类型: 长期运行          
  ./target/debug/dex-api --database-url postgres://dex:dex123@localhost:5432/dex_indexer --api-listen-address 0.0.0.0:3000   
  ────────────────────────────────────────                                                                
  终端: 4                                                                                                 
  服务: dex-node-test                                                                                     
  命令: cargo run -p dex-node-test --example place_order -- --fullnode-url http://127.0.0.1:9001          
  类型: 按需执行                                                                                          
  前提条件:                                                                                               
  - PostgreSQL 需要先启动（docker-compose up -d）                                                         
  - Sui 节点需要先启动并稳定运行                                                                          
  - dex-indexer 启动后会开始处理 checkpoint
  
  -----------------------------------------
  
# 运行 API E2E 测试 (simtest)                                                       
cargo simtest -p dex-indexer-e2e-test --test api_balance_tests                      
cargo simtest -p dex-indexer-e2e-test --test api_perpetual_tests                    
                                                                                  
# 运行所有 E2E 测试                                                                 
cargo simtest -p dex-indexer-e2e-test                                                
                                                                                         
# 运行 dex-node-test 测试                                                              
cargo simtest -p dex-node-test
  
cargo run -p dex-node-test --example place_order -- --fullnode-url http://127.0.0.1:9001 
curl -s http://127.0.0.1:3000/info -X POST -H "Content-Type: application/json" -d '{"type": "userBalances", "subaccount":"0x80ba1c742a3d4a02d0275046d1e01a31cffb341da49f14e66932c66cee05110d", "limit": 10}' | jq                                                                              
  
cargo run -p dex-node-test --example fill_and_verify -- --fullnode-url http://127.0.0.1:9001
curl -X POST http://127.0.0.1:3000/info \                                                     
-H "Content-Type: application/json" \                                                       
-d '{"type": "userFills", "subaccount": "0x...", "limit": 10}'   

# 市场创建事件验证                                                                                     
cargo run -p dex-node-test --example perpetual_and_verify -- --fullnode-url http://127.0.0.1:9001      
                                                                                                         
# 持仓变化事件验证                                                                                     
cargo run -p dex-node-test --example position_and_verify -- --fullnode-url http://127.0.0.1:9001

perpetual_and_verify API 验证                                                                          
                                                                                                         
  # 查询市场元数据（验证 PerpetualCreatedEvent）                                                         
  curl -s http://127.0.0.1:3000/info -X POST -H "Content-Type: application/json" -d '{"type": "meta"}' | jq                                                                           
                                                                                                         
  position_and_verify API 验证                                                                           
                                                                                                         
  # 1. 查询最近成交（验证 FillEvent）                                                                    
  curl -s http://127.0.0.1:3000/info -X POST \                                                           
    -H "Content-Type: application/json" \                                                                
    -d '{"type": "recentFills", "perpetualId": 0, "limit": 10}' | jq                                     
                                                                                                         
  # 2. 查询用户持仓和保证金（验证 PositionUpdateEvent）                                                  
  # 替换 <SUBACCOUNT_ID> 为示例输出中的子账户 ID                                                         
  curl -s http://127.0.0.1:3000/info -X POST \                                                           
    -H "Content-Type: application/json" \                                                                
    -d '{"type": "clearinghouseState", "subaccount": "<SUBACCOUNT_ID>"}' | jq                            
                                                                                                         
  # 3. 查询用户成交历史                                                                                  
  curl -s http://127.0.0.1:3000/info -X POST \                                                           
    -H "Content-Type: application/json" \                                                                
    -d '{"type": "userFills", "subaccount": "<SUBACCOUNT_ID>", "limit": 10}' | jq                        
                                                                                                         
  # 4. 查询用户余额变化（验证 BalanceUpdateEvent）                                                       
  curl -s http://127.0.0.1:3000/info -X POST \                                                           
    -H "Content-Type: application/json" \                                                                
    -d '{"type": "userBalances", "subaccount": "<SUBACCOUNT_ID>", "limit": 10}' | jq 
```



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