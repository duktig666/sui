复用sui的一些结论：

1. 第一阶段部分功能还是要使用MoveVM，例如：充提（Coin Object）
2. FastPath 机制
    - DEX 订单交易：开发 DEX Precompile (预编译钩子)，拦截MoveVM调用，路由到CustomEngine（兼容Move接口）
    - 存取款交易：需要 Move VM 处理，但仍走 Sequencer 保证顺序
3. 执行调度器
    * DEX 订单：Sequencer 已排序，无需调度
    * 非 DEX 交易：继续使用原有调度器
4. 复用 Sui 的 Tonic Network，主从交易同步
    - Tonic (gRPC)
    - Anemo (P2P 网络)
5. 扩展 Sui JSON-RPC API 添加 DEX 专用接口（下单、撤单、查询订单簿等）
6. 持久化存储
    * **RocksDB**: 通过 `typed-store` 存储 DEX 状态
    * **WAL**: 使用 Sui 的 WAL 机制保证持久化
    * **Checkpoint**: 定期创建快照，支持快速恢复
7. Move 框架集成
* 创建 `dex-framework` Move 包
* 定义 `place_order`, `cancel_order`, `deposit`, `withdraw` 等函数
* Precompile 拦截这些调用，路由到原生引擎
8. 事件系统 (Event System）
* 复用 Sui 的事件机制
* 发布订单创建、成交、撤单等事件
* 支持索引器订阅
9. Gas
* DEX 订单：使用固定Gas、按撮合次数计费、无Gas
* 存取款：使用标准 Sui Gas 计算
* 非 DEX 交易：使用原有 Gas 机制

---

上述方式 DEX Precompile 实现有问题，自定义程序不能被sui识别，考虑Move智能合约。