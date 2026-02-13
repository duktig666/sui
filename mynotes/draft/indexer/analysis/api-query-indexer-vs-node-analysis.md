DEX 查询 API 数据源分析：查链 vs 查 Indexer

核心区别

Hyperliquid 是自己的 L1，所有"实时状态"查询都直接读取验证者节点内存，零延迟。这相当于查链。

DEX 在 Sui 上，"查链"= 通过 sui_getObject RPC 读取链上共享对象。

逐个 API 分类
#: 1
API: meta
当前数据源: PG
Hyperliquid 做法: 节点内存
推荐数据源: PG (当前即可)
理由: 市场列表变化极少，indexer 够用
────────────────────────────────────────
#: 2
API: l2Book
当前数据源: Redis
Hyperliquid 做法: 节点内存
推荐数据源: 查链 ⚠️
理由: 订单簿要求实时，当前 Redis 是 indexer 聚合的快照，有延迟
────────────────────────────────────────
#: 3
API: candleSnapshot
当前数据源: Redis+PG
Hyperliquid 做法: 节点不存
推荐数据源: Redis+PG (当前即可)
理由: K 线是聚合数据，链上不存在，只能 indexer 计算
────────────────────────────────────────
#: 4
API: recentFills
当前数据源: Redis+PG
Hyperliquid 做法: 节点内存
推荐数据源: PG (当前即可)
理由: 历史数据，indexer 的强项
────────────────────────────────────────
#: 5
API: marketStats
当前数据源: Redis
Hyperliquid 做法: 节点内存
推荐数据源: Redis (当前即可)
理由: 聚合统计，链上不存在，indexer 计算合理
────────────────────────────────────────
#: 6
API: allMids
当前数据源: Redis
Hyperliquid 做法: 节点内存
推荐数据源: 查链 ⚠️
理由: 中间价应从链上订单簿实时计算
────────────────────────────────────────
#: 7
API: clearinghouseState
当前数据源: PG+Redis
Hyperliquid 做法: 节点内存
推荐数据源: 查链 ⚠️
理由: 上条分析过，仓位/余额应查链
────────────────────────────────────────
#: 8
API: userFills
当前数据源: PG
Hyperliquid 做法: indexer
推荐数据源: PG (当前即可)
理由: 纯历史记录
────────────────────────────────────────
#: 9
API: userBalances
当前数据源: PG
Hyperliquid 做法: indexer
推荐数据源: PG (当前即可)
理由: 余额变动历史
────────────────────────────────────────
#: 10
API: userTransfers
当前数据源: PG
Hyperliquid 做法: indexer
推荐数据源: PG (当前即可)
理由: 转账历史
────────────────────────────────────────
#: 11
API: openOrders
当前数据源: PG
Hyperliquid 做法: 节点内存
推荐数据源: 查链 ⚠️
理由: 当前挂单应实时，链上 PerpetualState.orderbook 有
────────────────────────────────────────
#: 12
API: historicalOrders
当前数据源: PG
Hyperliquid 做法: indexer
推荐数据源: PG (当前即可)
理由: 纯历史记录
────────────────────────────────────────
#: 13
API: orderStatus
当前数据源: PG
Hyperliquid 做法: 节点内存
推荐数据源: 查链+PG
理由: 活跃订单查链，已完结订单查 PG
────────────────────────────────────────
#: 14
API: subAccounts
当前数据源: PG
Hyperliquid 做法: 节点内存
推荐数据源: 查链 ⚠️
理由: 链上 GlobalAccounts 有完整子账户列表
────────────────────────────────────────
#: 15
API: userNonFundingLedgerUpdates
当前数据源: PG
Hyperliquid 做法: indexer
推荐数据源: PG (当前即可)
理由: 纯历史流水
────────────────────────────────────────
#: 16
API: userRateLimit
当前数据源: 内存
Hyperliquid 做法: 节点内存
推荐数据源: 内存 (当前即可)
理由: API 层自己维护
分类总结

推荐查链（实时状态类）              推荐查 Indexer（历史/聚合类）
─────────────────────              ──────────────────────────
clearinghouseState  ⚠️              meta
openOrders          ⚠️              candleSnapshot
l2Book              ⚠️              recentFills
allMids             ⚠️              marketStats
subAccounts         ⚠️              userFills
orderStatus (活跃)  ⚠️              userBalances
userTransfers
historicalOrders
orderStatus (已完结)
userNonFundingLedgerUpdates
userRateLimit

判断标准
┌─────────────┬──────────────────────────┬──────────────────────────────┐
│    特征     │           查链           │          查 Indexer          │
├─────────────┼──────────────────────────┼──────────────────────────────┤
│ 数据性质    │ 当前快照（"现在是什么"） │ 历史记录（"过去发生了什么"） │
├─────────────┼──────────────────────────┼──────────────────────────────┤
│ 用户期望    │ 交易后立即看到变化       │ 可接受几秒延迟               │
├─────────────┼──────────────────────────┼──────────────────────────────┤
│ 链上存在    │ 存储在共享对象中         │ 链上不存在（聚合/计算数据）  │
├─────────────┼──────────────────────────┼──────────────────────────────┤
│ Hyperliquid │ 直接读节点内存           │ 用 indexer                   │
└─────────────┴──────────────────────────┴──────────────────────────────┘
链上数据位置

GlobalAccounts (1个共享对象)
└── accounts[SubaccountId] → AccountState
├── quote_balance        → clearinghouseState.totalRawUsd
├── locked_balance       → 挂单冻结金额
└── realized_pnl         → indexer 没存的字段

PerpetualState (每市场1个共享对象)
├── positions[SubaccountId] → Position
│     ├── quantums           → clearinghouseState.assetPositions[i].szi
│     ├── entry_price        → entryPx
│     └── funding_index      → indexer 没存
└── orderbook → InlineOrderbook
├── bids[]             → l2Book.bids + openOrders
└── asks[]             → l2Book.asks + openOrders

实施优先级建议
┌────────┬────────────────────┬────────────────────────────────────────────────┐
│ 优先级 │        API         │                      原因                      │
├────────┼────────────────────┼────────────────────────────────────────────────┤
│ P0     │ clearinghouseState │ 用户最关心仓位准确性，交易后立即查看           │
├────────┼────────────────────┼────────────────────────────────────────────────┤
│ P0     │ openOrders         │ 挂单管理（撤单前需看到最新状态）               │
├────────┼────────────────────┼────────────────────────────────────────────────┤
│ P1     │ l2Book             │ 交易页核心组件，直接查链上订单簿最准           │
├────────┼────────────────────┼────────────────────────────────────────────────┤
│ P1     │ allMids            │ 可从 l2Book 查链的结果中派生                   │
├────────┼────────────────────┼────────────────────────────────────────────────┤
│ P2     │ orderStatus        │ 混合模式：先查链（活跃），miss 时查 PG（历史） │
├────────┼────────────────────┼────────────────────────────────────────────────┤
│ P2     │ subAccounts        │ 不频繁使用，优先级低                           │
└────────┴────────────────────┴────────────────────────────────────────────────┘