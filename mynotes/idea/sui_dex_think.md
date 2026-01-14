
针对一些问题思考：

1. ✅ 实现高TPS、低时延的DEX，哪些Sui的机制可以复用？
2. ✅ 开发DEX时，Sui是以sdk方式去使用，还是fork sui代码进行DEX开发
3. ✅ Tonic/Anemo Network - 验证者 P2P 通信，是否基于此实现主节点（或主节点轮换）定序的功能之一：从节点请求/tx同步给主节点。
4. 依赖Sui开发最小化的DEX应用链，直接启动Sui很庞大，可能影响性能，是否可以进行功能裁剪。可以从Fork SUI作为SDK+DEX方式是否可行。
5. 是否要兼容ABCI
6. ❌开发Demo - 验证DEX Precompile可以过滤dex交易到CustomEngine执行

DEX执行层如何落地？

Sui
1. 对象模型
2. ✅ 模块依赖
3. ✅ 节点运行 - 使用方式
4. Matemask - EVM Address
5. 实现路径


细节问题：
1. ✅ Object所有权管理，不用moveVM在外部是否可以管理，即是否可以使用Object的特性和并行执行？
2. ✅ sui-transaction-builder模块 前端还是服务端或节点内部调用
    Rust 客户端侧的“交易构建工具库”，sui-transaction-builder 既可能是客户端用，也可能被 RPC 服务端用来提供“构建交易”接口。
3. sui-indexer 数据来源、数据流向以及索引内容
4. ✅ fastpath 客户端提交Certificate - 验证者调用执行层，为什么是客户端在收集签名和提交证书？
5. sui-adapter
6. sui-authority-aggregation
