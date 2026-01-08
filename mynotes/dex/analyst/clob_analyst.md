# Clob模块分析

## 一、模块概述

CLOB (Central Limit Order Book) 是 dYdX v4 协议的**核心交易引擎模块**,实现了去中心化的限价订单簿。其创新点在于将**高频内存匹配**与**区块链共识**相结合,实现了接近中心化交易所性能的去中心化交易体验。

### 主要功能

功能模块描述订单管理支持多种订单类型的创建、取消、匹配撮合引擎价格-时间优先的订单匹配算法清算系统自动清算保证金不足的账户去杠杆机制当清算无法完成时的风险处理条件单触发基于 Oracle 价格的条件订单执行

### 模块文件结构

```Plain
protocol/x/clob/
├── module.go                    # Cosmos SDK 模块接口实现
├── abci.go                      # ABCI 生命周期处理 (最核心)
├── genesis.go                   # 创世状态
├── ante/                        # 交易前置处理器
│   └── clob.go                  # ClobDecorator - 订单处理入口
├── keeper/
│   ├── keeper.go                # Keeper 初始化
│   ├── orders.go                # 订单核心逻辑
│   ├── msg_server_place_order.go    # 下单 MsgServer
│   ├── msg_server_cancel_orders.go  # 取消 MsgServer
│   ├── msg_server_proposed_operations.go  # 操作队列处理
│   ├── process_operations.go    # 操作处理核心
│   ├── match_state.go           # 匹配状态管理
│   ├── liquidations.go          # 清算逻辑
│   ├── deleveraging.go          # 去杠杆逻辑
│   ├── conditional_orders.go    # 条件订单
│   ├── stateful_orders.go       # 状态订单管理
│   └── replay.go                # 订单重放
│   └── ...
├── memclob/                     # 内存订单簿
│   ├── memclob.go               # MemClob 实现
│   └── orderbook.go             # 订单簿数据结构
types/
    ├── order.go                 # 订单类型
    ├── order_id.go              # 订单 ID
    ├── operations_to_propose.go # 操作队列
    ├── matches.go               # 匹配类型
    └── liquidation_order.go     # 清算订单
│   ├── memclob.go               # MemClob 接口
│   └── ...
└── rate_limit/                  # 速率限制
```

### 订单类型

| 分类             | 类型        | 存储方式   | Gas 费用 | 过期机制     | 主要特点               | 适用场景             |
| ---------------- | ----------- | ---------- | -------- | ------------ | ---------------------- | -------------------- |
| **短期订单**     | Short-Term  | 仅存内存   | 零 Gas   | 区块高度过期 | 高吞吐、极低延迟       | 高频交易、网格、抢单 |
| **长期订单**     | Long-Term   | 链上持久化 | 标准 Gas | 时间戳过期   | 持久可靠、可查询       | 大额挂单、长期策略   |
| **条件订单**     | Conditional | 链上存储   | 标准 Gas | Oracle 触发  | 止损、止盈、条件单     | 风险控制、自动保护   |
| **时间加权订单** | TWAP        | 链上存储   | 标准 Gas | 分片时间执行 | 分批执行、减少市场冲击 | 大单拆分、防滑点执行 |

### 存储层次结构

```Go
┌──────────────────────────────────────────────────────────────────────────┐
│                           存储层次结构                                    │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  1. State Store (链上持久化存储 - storeKey)                               │
│     ├── 长期订单: "SO/P/L:{OrderId}"                                     │
│     ├── 未触发条件订单: "SO/U:{OrderId}"                                  │
│     ├── 已触发条件订单: "SO/P/T:{OrderId}"                                │
│     ├── TWAP订单: "TWAP:{OrderId}"                                       │
│     ├── 订单成交量: "Fill:{OrderId}"                                     │
│     ├── 过期时间切片: "Exp/{timestamp}:{OrderId}"                         │
│     └── ClobPair配置: "Clob:{ClobPairId}"                                │
│                                                                          │
│  2. Mem Store (内存存储 - memKey)                                         │
│     ├── ProcessProposerMatchesEvents (区块事件)                           │
│     └── StatefulOrderCount (状态化订单计数)                               │
│                                                                          │
│  3. Transient Store (瞬态存储 - transientStoreKey)                        │
│     ├── 未提交订单: "UncmtSO:{OrderId}"                                  │
│     ├── 未提交取消: "UncmtSOCxl:{OrderId}"                                │
│     ├── 下一个交易索引: "NextTxIdx"                                       │
│     └── 子账户清算信息: "SaLiqInfo:{SubaccountId}"                        │
│                                                                          │
│  4. MemClob (纯内存 - 不持久化)                                           │
│     ├── 订单簿 (Bids/Asks)                                               │
│     ├── 短期订单                                                          │
│     ├── 取消信息                                                          │
│     └── OperationsToPropose (待提议操作队列)                              │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────────────┐
│                   dYdX CLOB 存储架构全景图                           │
└─────────────────────────────────────────────────────────────────────┘

持久化存储 (StoreKey = "clob")
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
写入 IAVL Tree,永久保存,跨区块持久化

1. 交易对配置
   Clob:0 → ClobPair{...}
   Clob:1 → ClobPair{...}
   NextClobPairID → 2

2. 订单填充状态
   Fill:<OrderId_1> → OrderFillState{FillAmount: 1000000, PrunableBlockHeight: 12345}
   Fill:<OrderId_2> → OrderFillState{FillAmount: 500000, PrunableBlockHeight: MaxUint32}

3. 可清理订单索引
   PO/12345:<OrderId_1> → OrderId{...}
   PO/12346:<OrderId_2> → OrderId{...}

4. 长期订单
   SO/P/L:<OrderId_A> → LongTermOrderPlacement{Order: ..., PlacementIndex: ...}
   SO/P/L:<OrderId_B> → LongTermOrderPlacement{...}

5. 条件订单
   SO/U:<OrderId_C> → LongTermOrderPlacement{...}  // 未触发
   SO/P/T:<OrderId_D> → LongTermOrderPlacement{...}  // 已触发

6. 订单过期索引
   Exp/2024-12-17T10:30:00Z:<Order_1> → OrderId{...}
   Exp/2024-12-18T14:00:00Z:<Order_2> → OrderId{...}

7. 全局配置
   LiqCfg → LiquidationsConfig{...}
   EqTierCfg → EquityTierLimitConfiguration{...}
   RateLimCfg → BlockRateLimitConfiguration{...}


内存存储 (MemStoreKey = "mem_clob")
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
每个 Commit 后清空,用于区块内临时跟踪

1. 初始化标志
   MemstoreInit → true

2. 提议者撮合事件
   ProposerEvents → ProcessProposerMatchesEvents{
       PlacedLongTermOrderIds: [...]
       ExpiredStatefulOrderIds: [...]
       OrderIdsFilledInLastBlock: [...]
       ...
   }

3. 已交付订单索引
   DLTOIdx → 5
   DLTO:0 → OrderId{...}
   DLTO:1 → OrderId{...}
   ...
   DLTO:4 → OrderId{...}

   DCOIdx → 3
   DCIdx:0 → OrderId{...}
   ...

4. 已交付取消
   DCancel:<OrderId_X> → OrderId{...}

5. 状态化订单计数
   NumSO:<SubaccountId_Alice> → 10
   NumSO:<SubaccountId_Bob> → 5


临时存储 (TransientStoreKey = "tmp_clob")
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
每个区块结束清空,仅在 CheckTx 期间使用

1. 清算信息
   SaLiqInfo:<SubaccountId> → SubaccountLiquidationInfo{...}

2. 交易索引
   NextTxIdx → 42

3. 未提交订单 (CheckTx)
   UncmtSO:<OrderId_1> → LongTermOrderPlacement{...}
   UncmtSOCxl:<OrderId_2> → OrderId{...}
   NumUncmtSO:<SubaccountId> → 3

4. 价格范围
   MinTrade:0 → 1000000  // Perpetual 0 的最小交易价格
   MaxTrade:0 → 2000000  // Perpetual 0 的最大交易价格
```

### 模块关系

```Plain
┌─────────────────────────────────────────────┐
│                CLOB Module                  │
├─────────────────────────────────────────────┤
│                    │                        │
│    依赖            │      被依赖             │
│                    │                        │
│  • Subaccounts     │    • Indexer           │
│  • Perpetuals      │    • Full-node         │
│  • Prices (Oracle) │      Streaming         │
│  • Assets          │                        │
│  • Bank            │                        │
│                    │                        │
└─────────────────────────────────────────────┘
```

输入

消息类型功能MsgPlaceOrder下单MsgCancelOrder取消订单MsgBatchCancel批量取消MsgProposedOperations提案者提交的操作

输出

事件类型描述OrderFillEvent订单成交事件StatefulOrderEvent状态订单变更DeleveragingEvent去杠杆事件LiquidationEvent清算事件

### 核心文件

- protocol/x/clob/types/clob_keeper.go   `type ClobKeeper interface`
- protocol/x/clob/types/mem_clob_keeper.go `type MemClobKeeper interface`
- protocol/x/clob/types/memclob.go `type MemClob interface`
- protocol/x/clob/memclob/memclob.go `type MemClobPriceTimePriority struct`
- protocol/x/clob/memclob/orderbook.go `type Orderbook struct`
- protocol/x/clob/keeper/keeper.go `type Keeper struct`

## 二、核心数据结构详解

### OrderId - 订单唯一标识

**文件位置**: order_id.go

```Go
// OrderId 标志位定义
const (
    OrderIdFlags_ShortTerm    = uint32(0)    // 短期订单 - 仅存内存
    OrderIdFlags_Conditional  = uint32(32)   // 条件订单 (止损/止盈)
    OrderIdFlags_LongTerm     = uint32(64)   // 长期订单 - 链上持久化
    OrderIdFlags_Twap         = uint32(128)  // TWAP 订单
    OrderIdFlags_TwapSuborder = uint32(256)  // TWAP 子订单 (内部使用)
)

// OrderId 完整定义 (来自 protobuf)
type OrderId struct {
    SubaccountId types.SubaccountId  // 子账户ID (Owner + Number)
    ClientId     uint32              // 客户端订单ID
    OrderFlags   uint32              // 订单类型标志
    ClobPairId   uint32              // 交易对ID
}

// 关键方法

// IsShortTermOrder 判断是否为短期订单
func (o *OrderId) IsShortTermOrder() bool {
    return o.OrderFlags == OrderIdFlags_ShortTerm
}

// IsStatefulOrder 判断是否需要链上存储
func (o *OrderId) IsStatefulOrder() bool {
    return o.IsLongTermOrder() || o.IsConditionalOrder() || 
           o.IsTwapOrder() || o.IsTwapSuborder()
}
```

**设计说明**:

- **短期订单 (OrderFlags=0)**: 零 Gas 成本,仅存在于内存,通过 P2P 网络传播,20个区块后自动过期
- **长期订单 (OrderFlags=64)**: 需要 Gas,持久化到链上,可以设置较长的过期时间
- **条件订单 (OrderFlags=32)**: 触发价格达到后才会激活,支持止损/止盈
- **TWAP 订单 (OrderFlags=128)**: 时间加权平均价格订单,自动拆分为多个子订单

### Order - 订单完整结构

**文件位置**: order.go

```Go
// Order 核心结构 (简化版)
type Order struct {
    OrderId      OrderId
    Side         Order_Side       // SIDE_BUY 或 SIDE_SELL
    Quantums     uint64           // 订单数量 (base quantums)
    Subticks     uint64           // 价格 (subticks)
    
    // 有效期 (根据订单类型二选一)
    GoodTilBlock     uint32       // 短期订单: 有效至区块高度
    GoodTilBlockTime uint32       // 有状态订单: 有效至时间戳
    
    TimeInForce  Order_TimeInForce  // IOC, POST_ONLY, FOK 等
    ReduceOnly   bool               // 仅减仓标志
    ClientMetadata uint32          // 客户端元数据
    
    // 条件订单专用
    ConditionType Order_ConditionType       // STOP_LOSS 或 TAKE_PROFIT
    ConditionalOrderTriggerSubticks uint64  // 触发价格
}

// TimeInForce 类型
const (
    Order_TIME_IN_FORCE_UNSPECIFIED  // 默认: 挂单
    Order_TIME_IN_FORCE_IOC          // 立即成交或取消
    Order_TIME_IN_FORCE_POST_ONLY    // 仅做市商单
    Order_TIME_IN_FORCE_FILL_OR_KILL // 全部成交或取消 (已废弃)
)

// 关键方法

// GetOrderHash 计算订单的 SHA256 哈希
func (o *Order) GetOrderHash() OrderHash {
    orderBytes, _ := o.Marshal()
    return sha256.Sum256(orderBytes)
}

// IsBuy 判断是否为买单
func (o *Order) IsBuy() bool {
    return o.Side == Order_SIDE_BUY
}

// RequiresImmediateExecution 是否需要立即执行
func (o *Order) RequiresImmediateExecution() bool {
    return o.GetTimeInForce() == Order_TIME_IN_FORCE_IOC || 
           o.GetTimeInForce() == Order_TIME_IN_FORCE_FILL_OR_KILL
}
```

### ClobPair - 交易对配置

**文件位置**: clob_pair.go

```Go
// ClobPair_Status 交易对状态
const (
    ClobPair_STATUS_ACTIVE            // 正常交易
    ClobPair_STATUS_PAUSED            // 暂停
    ClobPair_STATUS_CANCEL_ONLY       // 仅允许取消
    ClobPair_STATUS_POST_ONLY         // 仅允许被动单
    ClobPair_STATUS_INITIALIZING      // 初始化中 (新市场)
    ClobPair_STATUS_FINAL_SETTLEMENT  // 最终结算
)

// ClobPair 结构
type ClobPair struct {
    Id                        uint32    // 交易对唯一ID
    Metadata                  oneof     // PerpetualClobMetadata 或 SpotClobMetadata
    StepBaseQuantums          uint64    // 最小订单量增量
    SubticksPerTick           uint32    // 价格精度 (subticks per tick)
    QuantumConversionExponent int32     // 数量转换指数
    Status                    ClobPair_Status
}

// 状态转换规则
var SupportedClobPairStatusTransitions = map[ClobPair_Status]map[ClobPair_Status]struct{}{
    ClobPair_STATUS_ACTIVE: {
        ClobPair_STATUS_FINAL_SETTLEMENT: struct{}{},
    },
    ClobPair_STATUS_INITIALIZING: {
        ClobPair_STATUS_ACTIVE:           struct{}{},
        ClobPair_STATUS_FINAL_SETTLEMENT: struct{}{},
    },
    ClobPair_STATUS_FINAL_SETTLEMENT: {
        ClobPair_STATUS_INITIALIZING: struct{}{},
    },
}
```

#### 状态转换规则

为什么 dYdX v4 要用这么“啰嗦”的写法？

核心原因只有一个：**在链上严格限制 CLOB 交易对（ClobPair）的状态只能按照预定义的路径迁移，杜绝非法状态跳跃，防止治理提案或恶意升级导致灾难性后果。**

```Go
ClobPair_STATUS_ACTIVE → ClobPair_STATUS_FINAL_SETTLEMENT   // 正常：交易对下市
ClobPair_STATUS_INITIALIZING → ClobPair_STATUS_ACTIVE       // 正常：初始化完成，正式上线
ClobPair_STATUS_INITIALIZING → ClobPair_STATUS_FINAL_SETTLEMENT  // 特殊：还没上线就被下市（比如参数错、风控触发）
ClobPair_STATUS_FINAL_SETTLEMENT → ClobPair_STATUS_INITIALIZING // 最关键！支持“复活”
```

**为什么允许 FINAL_SETTLEMENT → INITIALIZING（下市后还能重新上线）？**

这是 dYdX v4 的一个**天才设计**，解决了一个永恒难题：

> “交易对下市后，如果市场条件恢复、bug 修复、社区反悔，怎么重新上线？”

传统交易所（包括所有 EVM 链）下市后就彻底死了，合约地址作废，用户资金要迁出，极其麻烦。

dYdX v4 的设计是：

1. 治理通过提案把交易对状态改为 FINAL_SETTLEMENT
2. 所有持仓强制减仓（reduce-only），新订单被拒绝
3. 所有资金安全退出
4. 之后治理再发一个提案，把状态改回 INITIALIZING → ACTIVE
5. 同一个 ClobPair ID 复活！用户旧的订单簿、资金路径全部保留

这相当于**可逆下市**，极大提升了协议的韧性和治理灵活性。

为什么不用 bool 矩阵或 []Status，而是用 map[Status]map[Status]struct{}？

这是 Go 里写 **不可重复集合（set）**的惯用姿势，性能极高且可读性强。

写法内存占用查询速度是否支持重复推荐度map[Status][]Status高O(n)支持重复★☆☆☆☆map[Status]map[Status]bool中O(1)不支持★★★☆☆map[Status]map[Status]struct{}最低O(1)不支持★★★★★

struct{} 是 0 字节空结构体，Go 编译器会优化为只存 key，**内存占用最小**，常用于实现纯集合。

#### QuantumConversionExponent

它的核心作用是：**让 dYdX 在链上用纯整数运算，就能完美支持任意小数精度的价格和数量，同时保持所有订单簿、资金费率、强平逻辑 100% 确定性、无浮点误差**。

一句话总结它的意义： **QuantumConversionExponent 决定了「1 美元」在链上到底等于多少个最小整数单位（quantums）。**

交易对QuantumConversionExponent1 USD 在链上等于多少个 quantums能表达的最小价格变动（约）BTC-USD-91,000,000,000 (10⁹)0.000000001ETH-USD-91,000,000,0000.000000001SOL-USD-61,000,000 (10⁶)0.000001DOGE-USD-5100,000 (10⁵)0.00001PEPE-USD-2100 (10²)0.01

你会发现一个规律：**价格越低、波动越小的币，指数越大（越接近 0），最小精度反而越粗。**

这就是 QuantumConversionExponent 的终极目的：**在保证所有计算都是整数的前提下，用最少的存储空间，达到刚好够用的精度**。

dYdX v4 链上所有价格和数量都用三个“整数层”表示：

```Go
真实价格（美元）  
    = Subticks（价格整数）  
      × SubticksPerTick（价格精度）  
      × 10^QuantumConversionExponent  

真实数量（币的本位）  
    = BaseQuantums（数量整数）  
      × 10^(-10)  
      × StepBaseQuantums（最小数量增量）
      
PriceInUSD = Subticks × (SubticksPerTick) × 10^QuantumConversionExponent
```

BTC-USD

```Go
QuantumConversionExponent = -9
SubticksPerTick            = 1,000      // 1 tick = 0.001 USD
StepBaseQuantums           = 1          // 最小下单量 = 0.00001 BTC
```

- 你下单价格 60,000.50 USD → 链上 Subticks = 60,000,500,000 → 计算：60,000,500,000 × 1000 × 10⁻⁹ = 60,000.50 USD（完美还原）
- 你下单 0.001 BTC → 链上 BaseQuantums = 100,000 → 计算：100,000 × 10⁻¹⁰ × 1 = 0.001 BTC

全部都是 uint64 整数运算，零误差，零浮点。

### OrderBook

**文件位置：** memclob/orderbook.go

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=YjNjMzU5NTg3MTcxMjlkZWM0ODQyMGIwOTVjMTU5ZTFfVWhkR0JUSG02bDRkS2NBTFM2MWxaWHZTY09CRVlHN2tfVG9rZW46TnJrTWJyN1c5b2Y5bmx4VUVEVmxtNGlXZ3ZoXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

#### Orderbook 结构体

```Go
// 文件: memclob/orderbook.go

type Orderbook struct {
    // ═══════════════════ 价格档位核心结构 ═══════════════════
    
    // 买单价格档位映射: 价格(subticks) → Level
    Bids map[types.Subticks]*types.Level
    
    // 卖单价格档位映射: 价格(subticks) → Level  
    Asks map[types.Subticks]*types.Level
    
    // 最优买价 (subticks)，无买单时为 0
    BestBid types.Subticks
    
    // 最优卖价 (subticks)，无卖单时为 math.MaxUint64
    BestAsk types.Subticks
    
    // ═══════════════════ 配置参数 ═══════════════════
    
    // 每个 Tick 包含的 Subticks 数量（价格精度控制）
    SubticksPerTick types.SubticksPerTick
    
    // 订单的最小基础数量
    MinOrderBaseQuantums satypes.BaseQuantums
    
    // ═══════════════════ 订单索引结构 ═══════════════════
    
    // O(1) 订单查找: OrderId → LevelOrder 引用
    orderIdToLevelOrder map[types.OrderId]*types.LevelOrder
    
    // ═══════════════════ 子账户订单追踪 ═══════════════════
    
    // 子账户开放订单: SubaccountId → Side → OrderId → bool
    SubaccountOpenClobOrders map[satypes.SubaccountId]map[types.Order_Side]map[types.OrderId]bool
    
    // 子账户 Reduce-Only 订单追踪
    SubaccountOpenReduceOnlyOrders map[satypes.SubaccountId]map[types.OrderId]bool
    
    // ═══════════════════ 过期管理 ═══════════════════
    
    // 区块过期订单: BlockHeight → OrderId → bool
    blockExpirationsForOrders map[uint32]map[types.OrderId]bool
    
    // ═══════════════════ 取消订单追踪 ═══════════════════
    
    // 订单取消过期: OrderId → 过期区块
    orderIdToCancelExpiry map[types.OrderId]uint32
    
    // 区块取消过期: BlockHeight → OrderId → bool
    cancelExpiryToOrderIds map[uint32]map[types.OrderId]bool
    
    // ═══════════════════ 统计 ═══════════════════
    
    // 当前开放订单总数
    TotalOpenOrders uint
}
```

#### 价格档位层次结构

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=YzI3MzQwYTcyMzQ1N2E5Yzk3NDIyY2JhNjc3NDliMTJfVWVkMHl0NVhsMEJsNkk4eG45NWNCVzJPeFdiMGhBU0lfVG9rZW46RGx5eGJmTzNOb2RId1Z4U09iamxCRHJrZ3ZmXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

#### 索引结构示意

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=NzUxYzRhMjcxYTZlMTA2YzhjYzUyYmY4MGUzZWUxNTdfcXVmS2FhM091OU1YTkJHYkZrQ3k2dmZjdEJ0ZjNmdGpfVG9rZW46TjJ4b2JidmtFbzhIVEN4ZElmZmxGcUZRZ0pmXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=YjFlMmMwMjFhMjRkYWY5N2QzOWM3YTY5MmY3ODZhMzJfakRiakVOVENsVGl3NkZYNHpQZTcwOGd2NE1aNVZkU1VfVG9rZW46VU9mU2JKRHIzb2FBQlB4WlgyUmwzb3pSZ2tmXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

#### 时间复杂度

操作时间复杂度实现方式添加订单O(1)直接插入 map + 链表尾部删除订单O(1)通过 [orderIdToLevelOrder](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html) 直接定位查找订单O(1)通过 [orderIdToLevelOrder](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html) 直接查找获取最优价O(1)直接读取 [BestBid](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html) / [BestAsk](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)更新最优价O(n) 最坏删除最优价订单后需查找下一最优过期订单清理O(k)k = 该区块过期的订单数量

#### 价格单位系统

Subticks 概念

```Go
// 文件: types/orderbook.go

// Subticks 是订单簿的最小价格单位
type Subticks uint64

// SubticksPerTick 定义一个 Tick 包含多少 Subticks
type SubticksPerTick uint32
```

示例

```Go
假设 SubticksPerTick = 100

有效价格:   100, 200, 300, 400, ...  ✓
无效价格:   50, 150, 250, 350, ...   ✗ (不是 100 的倍数)

┌─────────────────────────────────────────────────────────────┐
│   Tick 1        Tick 2        Tick 3        Tick 4         │
│ ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐      │
│ │ 1-100   │   │ 101-200 │   │ 201-300 │   │ 301-400 │ ...  │
│ └─────────┘   └─────────┘   └─────────┘   └─────────┘      │
│    ↑                                                        │
│  订单只能挂在 100, 200, 300, 400 这些位置                    │
└─────────────────────────────────────────────────────────────┘
```

### OperationsToPropose - 待提议操作队列

**文件位置**: operations_to_propose.go

```Go
// OperationsToPropose 封装了区块提议所需的所有操作数据
type OperationsToPropose struct {
    // 有序的操作队列 - 将被提议到下一个区块
    OperationsQueue []InternalOperation
    
    // 已在操作队列中的订单哈希集合 (防止重复)
    OrderHashesInOperationsQueue map[OrderHash]bool
    
    // 短期订单哈希 -> 原始交易字节
    // 用于在 GetOperationsQueueRaw 中重建 MsgProposedOperations
    ShortTermOrderHashToTxBytes map[OrderHash][]byte
    
    // 已匹配的订单ID -> 订单对象映射
    // 注意: 同一ID可能有多个版本,保留"最大"的那个
    MatchedOrderIdToOrder map[OrderId]Order
    
    // 已在队列中的订单移除集合
    OrderRemovalsInOperationsQueue map[OrderId]bool
}

// NewOperationsToPropose 创建新实例
func NewOperationsToPropose() *OperationsToPropose {
    return &OperationsToPropose{
        OperationsQueue:                make([]InternalOperation, 0),
        OrderHashesInOperationsQueue:   make(map[OrderHash]bool),
        ShortTermOrderHashToTxBytes:    make(map[OrderHash][]byte),
        MatchedOrderIdToOrder:          make(map[OrderId]Order),
        OrderRemovalsInOperationsQueue: make(map[OrderId]bool),
    }
}

// MustAddShortTermOrderTxBytes 添加短期订单的交易字节
func (o *OperationsToPropose) MustAddShortTermOrderTxBytes(
    order Order,
    txBytes []byte,
) {
    order.OrderId.MustBeShortTermOrder()
    
    if len(txBytes) == 0 {
        panic("提供的交易字节为空")
    }
    
    orderHash := order.GetOrderHash()
    if _, exists := o.ShortTermOrderHashToTxBytes[orderHash]; exists {
        panic("订单已存在于 ShortTermOrderHashToTxBytes")
    }
    
    o.ShortTermOrderHashToTxBytes[orderHash] = txBytes
}

// MustAddMatchToOperationsQueue 添加匹配到操作队列
func (o *OperationsToPropose) MustAddMatchToOperationsQueue(
    takerMatchableOrder MatchableOrder,
    makerFillsWithOrders []MakerFillWithOrder,
) InternalOperation {
    // 验证所有参与订单都在队列中
    // 构建 ClobMatch
    // 添加到 OperationsQueue
    // ...
}
```

**设计说明**:

- `OperationsToPropose` 是验证者节点本地维护的操作队列
- 在 PrepareCheckState 中生成,在下一个区块被提议
- 通过 `MsgProposedOperations` 消息提交到共识

### InternalOperation - 内部操作类型

**文件位置**: internal_operation.go

```Go
 // InternalOperation 定义了可以在操作队列中的操作类型
 // InternalOperation 仅在 memclob 内部使用。
type InternalOperation struct {
    // operation represents the operation that occurred, which can be a match,
    // Short-Term order placement, or the placement of a pre-existing stateful
    // order.
    //
    // Types that are valid to be assigned to Operation:
    //  *InternalOperation_Match
    //  *InternalOperation_ShortTermOrderPlacement
    //  *InternalOperation_PreexistingStatefulOrder
    //  *InternalOperation_OrderRemoval
    Operation isInternalOperation_Operation  // oneof 类型
}

// 操作类型 (oneof)
// - ShortTermOrderPlacement: 短期订单放置
// - PreexistingStatefulOrder: 预存在的有状态订单引用
// - Match: 订单匹配 (ClobMatch)
// - OrderRemoval: 订单移除

// NewMatchOrdersInternalOperation 创建订单匹配操作
func NewMatchOrdersInternalOperation(
    takerOrder Order,
    makerFills []MakerFill,
) InternalOperation {
    if len(makerFills) == 0 {
        panic("无法创建没有 maker fills 的匹配操作")
    }
    
    return InternalOperation{
        Operation: &InternalOperation_Match{
            Match: &ClobMatch{
                Match: &ClobMatch_MatchOrders{
                    MatchOrders: &MatchOrders{
                        TakerOrderId: takerOrder.OrderId,
                        Fills:        makerFills,
                    },
                },
            },
        },
    }
}

// NewMatchPerpetualLiquidationInternalOperation 创建清算匹配操作
func NewMatchPerpetualLiquidationInternalOperation(
    takerLiquidationOrder MatchableOrder,
    makerFills []MakerFill,
) InternalOperation {
    if !takerLiquidationOrder.IsLiquidation() {
        panic("不是清算订单")
    }
    
    return InternalOperation{
        Operation: &InternalOperation_Match{
            Match: &ClobMatch{
                Match: &ClobMatch_MatchPerpetualLiquidation{
                    // ... 清算匹配数据
                },
            },
        },
    }
}
```

### LiquidationOrder - 清算订单

**文件位置**: liquidation_order.go

```Go
// LiquidationOrder 表示 IOC 类型的清算订单
type LiquidationOrder struct {
    perpetualLiquidationInfo PerpetualLiquidationInfo
    clobPairId               ClobPairId
    isBuy                    bool              // 清算空头则为 true
    quantums                 satypes.BaseQuantums
    subticks                 Subticks
}

// NewLiquidationOrder 创建清算订单
func NewLiquidationOrder(
    subaccountId satypes.SubaccountId,
    clobPair ClobPair,
    isBuy bool,
    quantums satypes.BaseQuantums,
    subticks Subticks,
) *LiquidationOrder {
    // 必须是永续合约 CLOB
    perpetualClobMetadata := clobPair.GetPerpetualClobMetadata()
    if perpetualClobMetadata == nil {
        panic("清算订单只能用于永续合约")
    }
    
    return &LiquidationOrder{
        perpetualLiquidationInfo: PerpetualLiquidationInfo{
            SubaccountId: subaccountId,
            PerpetualId:  perpetualClobMetadata.PerpetualId,
        },
        clobPairId: clobPair.GetClobPairId(),
        isBuy:      isBuy,
        quantums:   quantums,
        subticks:   subticks,
    }
}

// IsLiquidation 实现 MatchableOrder 接口
func (lo *LiquidationOrder) IsLiquidation() bool {
    return true  // 清算订单始终返回 true
}
```

## 三、Keeper 核心结构

### 3.1 Keeper 定义

**文件位置**: keeper.go

```Go
type Keeper struct {
    // ========== 存储键 ==========
    cdc               codec.BinaryCodec
    storeKey          storetypes.StoreKey     // 持久化状态存储
    memKey            storetypes.StoreKey     // 内存存储 (跨区块)
    transientStoreKey storetypes.StoreKey     // 临时存储 (单区块)
    
    // ========== 权限控制 ==========
    authorities       map[string]struct{}     // 治理授权地址
    
    // ========== 核心组件 ==========
    MemClob                 types.MemClob           // 内存订单簿
    PerpetualIdToClobPairId map[uint32][]types.ClobPairId  // 永续合约 -> 交易对映射
    
    // ========== 依赖的 Keepers ==========
    subaccountsKeeper types.SubaccountsKeeper  // 子账户管理
    assetsKeeper      types.AssetsKeeper       // 资产管理
    bankKeeper        types.BankKeeper         // 银行模块
    blockTimeKeeper   types.BlockTimeKeeper    // 区块时间
    feeTiersKeeper    types.FeeTiersKeeper     // 手续费层级
    perpetualsKeeper  types.PerpetualsKeeper   // 永续合约
    pricesKeeper      types.PricesKeeper       // 价格预言机
    statsKeeper       types.StatsKeeper        // 统计模块
    rewardsKeeper     types.RewardsKeeper      // 奖励模块
    affiliatesKeeper  types.AffiliatesKeeper   // 联盟返佣
    revshareKeeper    types.RevShareKeeper     // 收入分成
    accountPlusKeeper types.AccountPlusKeeper  // 账户增强
    
    // ========== 索引和流式传输 ==========
    indexerEventManager      indexer_manager.IndexerEventManager
    streamingManager         streamingtypes.FullNodeStreamingManager
    finalizeBlockEventStager finalizeblock.EventStager[*types.ClobStagedFinalizeBlockEvent]
    
    // ========== 状态标志 ==========
    inMemStructuresInitialized *atomic.Bool  // 内存结构是否已初始化
    
    // ========== 配置 ==========
    Flags              flags.ClobFlags
    mevTelemetryConfig MevTelemetryConfig
    
    // ========== 交易处理 ==========
    txDecoder   sdk.TxDecoder
    antehandler sdk.AnteHandler  // 在 BaseApp 设置后注入
    
    // ========== 速率限制 ==========
    placeCancelOrderRateLimiter rate_limit.RateLimiter[sdk.Msg]
    updateLeverageRateLimiter   rate_limit.RateLimiter[string]
    
    // ========== 清算守护进程 ==========
    DaemonLiquidationInfo *liquidationtypes.DaemonLiquidationInfo
}

// NewKeeper 创建新的 Keeper
func NewKeeper(
    cdc codec.BinaryCodec,
    storeKey storetypes.StoreKey,
    memKey storetypes.StoreKey,
    transientStoreKey storetypes.StoreKey,
    authorities []string,
    memClob types.MemClob,
    // ... 其他参数
) *Keeper {
    keeper := &Keeper{
        // ... 初始化字段
        inMemStructuresInitialized: &atomic.Bool{}, // 默认为 false
    }
    
    // 关键: 将 Keeper 提供给 MemClob
    // MemClob 需要 Keeper 来读取状态中的填充量
    memClob.SetClobKeeper(keeper)
    
    return keeper
}

// Initialize 初始化 Keeper 的内存数据结构
func (k Keeper) Initialize(ctx sdk.Context) {
    // 1. 初始化 memstore 中的订单填充量和有状态订单
    k.InitMemStore(ctx)
    
    // 2. 检查是否已初始化 (原子操作)
    alreadyInitialized := k.inMemStructuresInitialized.Swap(true)
    if alreadyInitialized {
        return
    }
    
    // 3. 分支 context 用于水合 (hydration)
    // 写入的匹配会被丢弃,避免破坏共识
    checkCtx, _ := ctx.CacheContext()
    checkCtx = checkCtx.WithIsCheckTx(true)
    
    // 4. 使用状态中的 ClobPairs 初始化 memclob 订单簿
    k.InitMemClobOrderbooks(checkCtx)
    
    // 5. 初始化所有已存在的有状态订单
    k.InitStatefulOrders(checkCtx)
    
    // 6. 水合 ClobPair 和 Perpetual 的映射
    k.HydrateClobPairAndPerpetualMapping(checkCtx)
}
```

### 3.2 依赖的接口

**文件位置**: expected_keepers.go

```Go
// SubaccountsKeeper 子账户管理接口
type SubaccountsKeeper interface {
    // 检查是否可以更新子账户
    CanUpdateSubaccounts(
        ctx sdk.Context,
        updates []satypes.Update,
        updateType satypes.UpdateType,
    ) (success bool, successPerUpdate []satypes.UpdateResult, err error)
    
    // 获取净抵押品和保证金要求
    GetNetCollateralAndMarginRequirements(
        ctx sdk.Context,
        update satypes.Update,
    ) (risk margin.Risk, err error)
    
    // 获取子账户
    GetSubaccount(ctx sdk.Context, id satypes.SubaccountId) satypes.Subaccount
    
    // 更新子账户
    UpdateSubaccounts(
        ctx sdk.Context,
        updates []satypes.Update,
        updateType satypes.UpdateType,
    ) (success bool, successPerUpdate []satypes.UpdateResult, err error)
    
    // 转移保险基金
    TransferInsuranceFundPayments(ctx sdk.Context, amount *big.Int, perpetualId uint32) error
    
    // 获取保险基金余额
    GetCrossInsuranceFundBalance(ctx sdk.Context) *big.Int
    
    // ... 更多方法
}

// PerpetualsKeeper 永续合约接口
type PerpetualsKeeper interface {
    GetNetNotional(ctx sdk.Context, id uint32, bigQuantums *big.Int) (*big.Int, error)
    GetPerpetual(ctx sdk.Context, perpetualId uint32) (perpetualsmoduletypes.Perpetual, error)
    GetMarginRequirements(ctx sdk.Context, id uint32, bigQuantums *big.Int) (*big.Int, *big.Int, error)
    // ...
}

// PricesKeeper 价格接口
type PricesKeeper interface {
    GetMarketPrice(ctx sdk.Context, id uint32) (pricestypes.MarketPrice, error)
}
```

## 四、内存订单簿 (MemClob) 详解

### 4.1 MemClob 接口定义

**文件位置**: memclob.go

```Go
// MemClob 封装了 CLOB 内存数据结构的所有读写操作
type MemClob interface {
    // ========== 初始化 ==========
    SetClobKeeper(keeper MemClobKeeper)
    CreateOrderbook(clobPair ClobPair)
    MaybeCreateOrderbook(clobPair ClobPair) bool
    
    // ========== 订单操作 ==========
    PlaceOrder(ctx sdk.Context, order Order) (
        satypes.BaseQuantums,   // 乐观匹配的数量
        OrderStatus,            // 订单状态
        *OffchainUpdates,       // 离链更新
        error,
    )
    CancelOrder(ctx sdk.Context, msgCancelOrder *MsgCancelOrder) (
        *OffchainUpdates, error,
    )
    GetOrder(orderId OrderId) (Order, bool)
    GetCancelOrder(orderId OrderId) (uint32, bool)
    
    // ========== 订单簿查询 ==========
    GetOrderFilledAmount(ctx sdk.Context, orderId OrderId) satypes.BaseQuantums
    GetOrderRemainingAmount(ctx sdk.Context, order Order) (satypes.BaseQuantums, bool)
    GetSubaccountOrders(clobPairId ClobPairId, subaccountId satypes.SubaccountId, side Order_Side) ([]Order, error)
    GetMidPrice(ctx sdk.Context, clobPairId ClobPairId) (Subticks, Order, Order, bool)
    
    // ========== 清算与去杠杆 ==========
    PlacePerpetualLiquidation(ctx sdk.Context, liquidationOrder LiquidationOrder) (
        satypes.BaseQuantums, OrderStatus, *OffchainUpdates, error,
    )
    DeleverageSubaccount(ctx sdk.Context, subaccountId satypes.SubaccountId, perpetualId uint32, deltaQuantums *big.Int, isFinalSettlement bool) (
        *big.Int, error,
    )
    
    // ========== 操作队列 ==========
    GetOperationsToReplay(ctx sdk.Context) ([]InternalOperation, map[OrderHash][]byte)
    GetOperationsRaw(ctx sdk.Context) []OperationRaw
    RemoveAndClearOperationsQueue(ctx sdk.Context, localValidatorOperationsQueue []InternalOperation)
    
    // ========== 状态管理 ==========
    PurgeInvalidMemclobState(ctx sdk.Context, fullyFilledOrderIds []OrderId, expiredStatefulOrderIds []OrderId, canceledStatefulOrderIds []OrderId, removedStatefulOrderIds []OrderId, existingOffchainUpdates *OffchainUpdates) *OffchainUpdates
    ReplayOperations(ctx sdk.Context, localOperations []InternalOperation, shortTermOrderTxBytes map[OrderHash][]byte, existingOffchainUpdates *OffchainUpdates, postOnlyFilter bool) *OffchainUpdates
    InsertZeroFillDeleveragingIntoOperationsQueue(subaccountId satypes.SubaccountId, perpetualId uint32)
    
    // ========== 价格溢价计算 ==========
    GetPricePremium(ctx sdk.Context, clobPair ClobPair, params perptypes.GetPricePremiumParams) (int32, error)
    
    // ========== 流式更新 ==========
    GetOffchainUpdatesForOrderbookSnapshot(ctx sdk.Context, clobPairId ClobPairId) *OffchainUpdates
    GenerateStreamOrderbookFill(ctx sdk.Context, clobMatch ClobMatch, takerOrder MatchableOrder, makerOrders []Order) StreamOrderbookFill
}
```

### 4.2 MemClobPriceTimePriority 实现

**文件位置**: memclob.go

```Go
// MemClobPriceTimePriority 实现价格-时间优先的订单匹配
type MemClobPriceTimePriority struct {
    // 所有订单簿,按 ClobPairId 索引
    orderbooks map[types.ClobPairId]*Orderbook
    
    // 待提议的操作队列
    operationsToPropose types.OperationsToPropose
    
    // Keeper 引用 (双向依赖)
    clobKeeper types.MemClobKeeper
    
    // 是否生成离链更新消息 (用于 Indexer)
    generateOffchainUpdates bool
    
    // 是否生成订单簿更新 (用于全节点流式传输)
    generateOrderbookUpdates bool
}

// NewMemClobPriceTimePriority 创建新实例
func NewMemClobPriceTimePriority(
    generateOffchainUpdates bool,
) *MemClobPriceTimePriority {
    return &MemClobPriceTimePriority{
        orderbooks:               make(map[types.ClobPairId]*Orderbook),
        operationsToPropose:      *types.NewOperationsToPropose(),
        generateOffchainUpdates:  generateOffchainUpdates,
        generateOrderbookUpdates: false,
    }
}

// CreateOrderbook 创建新订单簿
func (m *MemClobPriceTimePriority) CreateOrderbook(clobPair types.ClobPair) {
    clobPairId := clobPair.GetClobPairId()
    if _, exists := m.orderbooks[clobPairId]; exists {
        panic(fmt.Sprintf("订单簿 %d 已存在", clobPairId))
    }
    
    subticksPerTick := clobPair.GetClobPairSubticksPerTick()
    minOrderBaseQuantums := clobPair.GetClobPairMinOrderBaseQuantums()
    
    m.orderbooks[clobPairId] = &Orderbook{
        Asks:                           make(map[types.Subticks]*types.Level),
        BestAsk:                        math.MaxUint64,
        BestBid:                        0,
        Bids:                           make(map[types.Subticks]*types.Level),
        MinOrderBaseQuantums:           minOrderBaseQuantums,
        SubaccountOpenClobOrders:       make(map[satypes.SubaccountId]map[types.Order_Side]map[types.OrderId]bool),
        SubticksPerTick:                subticksPerTick,
        SubaccountOpenReduceOnlyOrders: make(map[satypes.SubaccountId]map[types.OrderId]bool),
        orderIdToLevelOrder:            make(map[types.OrderId]*types.LevelOrder),
        blockExpirationsForOrders:      make(map[uint32]map[types.OrderId]bool),
        orderIdToCancelExpiry:          make(map[types.OrderId]uint32),
        cancelExpiryToOrderIds:         make(map[uint32]map[types.OrderId]bool),
    }
}

// CancelOrder 取消短期订单
func (m *MemClobPriceTimePriority) CancelOrder(
    ctx sdk.Context,
    msgCancelOrder *types.MsgCancelOrder,
) (offchainUpdates *types.OffchainUpdates, err error) {
    lib.AssertCheckTxMode(ctx)  // 仅在 CheckTx 模式下执行
    
    orderbook := m.mustGetOrderbook(types.ClobPairId(msgCancelOrder.OrderId.GetClobPairId()))
    orderIdToCancel := msgCancelOrder.GetOrderId()
    orderIdToCancel.MustBeShortTermOrder()  // 必须是短期订单
    
    // 获取现有的取消记录
    oldCancellationGoodTilBlock, cancelAlreadyExists := orderbook.getCancel(orderIdToCancel)
    goodTilBlock := msgCancelOrder.GetGoodTilBlock()
    
    // 如果已存在相同或更大的 goodTilBlock,返回错误
    if cancelAlreadyExists && oldCancellationGoodTilBlock >= goodTilBlock {
        return nil, types.ErrMemClobCancelAlreadyExists
    }
    
    // 如果订单簿中存在该订单且 goodTilBlock >= 订单的 goodTilBlock,移除订单
    if levelOrder, orderExists := orderbook.orderIdToLevelOrder[orderIdToCancel]; orderExists &&
        goodTilBlock >= levelOrder.Value.Order.GetGoodTilBlock() {
        m.mustRemoveOrder(ctx, orderIdToCancel)
    }
    
    // 更新取消记录
    if cancelAlreadyExists {
        orderbook.mustRemoveCancel(orderIdToCancel)
    }
    orderbook.addShortTermCancel(orderIdToCancel, goodTilBlock)
    
    // 生成离链更新
    offchainUpdates = types.NewOffchainUpdates()
    if m.generateOffchainUpdates {
        if message, success := off_chain_updates.CreateOrderRemoveMessageWithReason(
            ctx, orderIdToCancel,
            indexersharedtypes.OrderRemovalReason_ORDER_REMOVAL_REASON_USER_CANCELED,
            ocutypes.OrderRemoveV1_ORDER_REMOVAL_STATUS_BEST_EFFORT_CANCELED,
        ); success {
            offchainUpdates.AddRemoveMessage(orderIdToCancel, message)
        }
    }
    
    return offchainUpdates, nil
}
```

 

### 4.3 Orderbook 订单簿结构

**文件位置**: orderbook.go

```Go
// Orderbook 保存特定 ClobPairId 的买卖单
type Orderbook struct {
    // ========== 价格精度 ==========
    SubticksPerTick types.SubticksPerTick
    
    // ========== 订单层级 ==========
    Bids    map[types.Subticks]*types.Level  // 买单: 价格 -> 层级
    Asks    map[types.Subticks]*types.Level  // 卖单: 价格 -> 层级
    BestBid types.Subticks                    // 最高买价 (0 表示无买单)
    BestAsk types.Subticks                    // 最低卖价 (MaxUint64 表示无卖单)
    
    // ========== 子账户订单追踪 ==========
    // 用于抵押品检查时快速查找某子账户在某侧的所有订单
    SubaccountOpenClobOrders map[satypes.SubaccountId]map[types.Order_Side]map[types.OrderId]bool
    
    // ========== 仅减仓订单追踪 ==========
    SubaccountOpenReduceOnlyOrders map[satypes.SubaccountId]map[types.OrderId]bool
    
    // ========== 最小订单量 ==========
    MinOrderBaseQuantums satypes.BaseQuantums
    
    // ========== 统计 ==========
    TotalOpenOrders uint
    
    // ========== 订单索引 ==========
    // O(1) 订单查找和删除
    orderIdToLevelOrder map[types.OrderId]*types.LevelOrder
    
    // ========== 过期管理 ==========
    // 区块高度 -> 该区块过期的订单集合
    blockExpirationsForOrders map[uint32]map[types.OrderId]bool
    
    // ========== 取消管理 ==========
    orderIdToCancelExpiry  map[types.OrderId]uint32           // 订单ID -> 取消过期区块
    cancelExpiryToOrderIds map[uint32]map[types.OrderId]bool  // 过期区块 -> 订单ID集合
}

// GetMidPrice 获取中间价格
func (ob *Orderbook) GetMidPrice() (midPrice types.Subticks, exists bool) {
    if ob.BestBid == 0 || ob.BestAsk == math.MaxUint64 {
        return 0, false
    }
    return ob.BestBid + (ob.BestAsk-ob.BestBid)/2, true
}

// findNextBestLevelOrder 查找下一个最优价格层级的订单
func (ob *Orderbook) findNextBestLevelOrder(
    levelOrder *types.LevelOrder,
) (nextBestLevelOrder *types.LevelOrder, foundOrder bool) {
    // 1. 尝试获取同层级的下一个订单 (时间优先)
    if levelOrder.Next != nil {
        return levelOrder.Next, true
    }
    
    // 2. 同层级没有更多订单,查找下一个价格层级
    order := levelOrder.Value.Order
    subticks := order.GetOrderSubticks()
    isBuy := order.IsBuy()
    
    nextBestSubticks, foundOrder := ob.findNextBestSubticks(subticks, isBuy)
    if !foundOrder {
        return nil, false
    }
    
    // 3. 获取该层级的第一个订单
    nextBestLevelOrder, foundOrder = ob.getFirstOrderAtSideAndSubticks(isBuy, nextBestSubticks)
    return nextBestLevelOrder, foundOrder
}

// findNextBestSubticks 查找下一个最优价格
func (ob *Orderbook) findNextBestSubticks(
    startingTicks types.Subticks,
    isBuy bool,
) (nextBestSubtick types.Subticks, found bool) {
    var curSubticks types.Subticks = startingTicks
    levels := ob.GetSide(isBuy)
    numLevels := len(levels)
    orderbookSubticksPerTick := types.Subticks(ob.SubticksPerTick)
    
    // 迭代查找,最多迭代 numLevels 次
    for i := 0; i < numLevels; i++ {
        if isBuy {
            curSubticks -= orderbookSubticksPerTick  // 买单往下找
        } else {
            curSubticks += orderbookSubticksPerTick  // 卖单往上找
        }
        
        if curLevel := levels[curSubticks]; curLevel != nil {
            return curSubticks, true
        }
    }
    
    // 未找到,回退到遍历所有层级
    // ... 完整实现省略
    return 0, false
}
```

## 五、ABCI 生命周期详解

**文件位置**: abci.go

这是理解 CLOB 模块执行流程的**最关键文件**。  

### 流程

```Go
区块 N-1 提交 ──▶ PreBlock ──▶ BeginBlock ──▶ DeliverTx ──▶ EndBlock ──▶ Precommit ──▶ PrepareCheckState ──▶ 区块 N 开始
┌─────────────────────────────────────────────────────────────────────────────────┐
│                        区块 N 的完整生命周期                                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  1. PreBlocker (每个区块开始前)                                                  │
│     └── keeper.Initialize(ctx)  // 初始化内存数据结构                            │
│                                                                                 │
│  2. BeginBlocker (区块开始)                                                      │
│     ├── 初始化 ProcessProposerMatchesEvents                                     │
│     └── 重置已交付订单ID                                                         │
│                                                                                 │
│  3. DeliverTx / FinalizeBlock (处理交易)                                         │
│     └── 处理区块中的所有交易 (MsgPlaceOrder, MsgCancelOrder, etc.)               │
│                                                                                 │
│  4. EndBlocker (区块结束)                                                        │
│     ├── 清理过期订单的成交量                                                     │
│     ├── 移除过期的状态化订单                                                     │
│     ├── 生成 TWAP 子订单                                                         │
│     ├── 触发条件订单                                                             │
│     └── 更新 ProcessProposerMatchesEvents                                       │
│                                                                                 │
│  5. Precommit (提交前)                                                           │
│     ├── 处理暂存的 FinalizeBlock 事件                                           │
│     └── 流式推送更新（如启用）                                                   │
│                                                                                 │
│  6. Commit (提交区块)                                                            │
│     └── 区块状态写入持久化存储                                                   │
│                                                                                 │
│  7. PrepareCheckState (准备检查状态 - 为下一区块 N+1 做准备)                       │
│     ├── 清理本地操作队列                                                         │
│     ├── 清除无效的 MemClob 状态                                                  │
│     ├── 放置上一区块的状态化订单                                                 │
│     ├── 放置触发的条件订单                                                       │
│     ├── 重放本地操作                                                             │
│     ├── 执行清算                                                                 │
│     ├── 执行去杠杆                                                               │
│     └── 检查负 TNC 子账户                                                        │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### 5.1 PreBlocker - 区块前初始化

```Go
// PreBlocker 在区块执行前调用,用于初始化
func PreBlocker(
    ctx sdk.Context,
    keeper types.ClobKeeper,
) {
    keeper.Initialize(ctx)  // 初始化内存数据结构
}
```

**执行时机**: 每个区块开始前 **作用**:

- 初始化 memstore
- 首次启动时水合订单簿和有状态订单

### 5.2 BeginBlocker - 区块开始

```Go
// BeginBlocker 在区块开始时执行
func BeginBlocker(
    ctx sdk.Context,
    keeper types.ClobKeeper,
) {
    ctx = log.AddPersistentTagsToLogger(ctx,
        log.Handler, log.BeginBlocker,
        log.BlockHeight, ctx.BlockHeight(),
    )
    
    // 重置 ProcessProposerMatchesEvents
    // 清除上一个区块的匹配事件
    keeper.MustSetProcessProposerMatchesEvents(
        ctx,
        types.ProcessProposerMatchesEvents{
            BlockHeight: lib.MustConvertIntegerToUint32(ctx.BlockHeight()),
        },
    )
    
    // 重置已交付的订单 ID
    keeper.ResetAllDeliveredOrderIds(ctx)
}
```

**作用**:

- 为新区块准备干净的匹配事件记录
- 清理上一个区块的订单交付记录

### 5.3 Precommit - 提交前处理

```Go
// Precommit 在状态提交前执行
func Precommit(
    ctx sdk.Context,
    keeper keeper.Keeper,
) {
    // 处理所有暂存的 FinalizeBlock 事件
    // 例如: 新建的订单簿需要在此时创建
    // 因为在 FinalizeBlock 期间不能修改 MemClob
    keeper.ProcessStagedFinalizeBlockEvents(ctx)
    
    // 流式传输更新
    if streamingManager := keeper.GetFullNodeStreamingManager(); !streamingManager.Enabled() {
        return
    }
    keeper.StreamBatchUpdatesAfterFinalizeBlock(ctx)
}
```

**关键点**:

- 必须在 PrepareCheckState 之前执行
- 处理需要延迟执行的副作用

### 5.4 EndBlocker - 区块结束

```Go
// EndBlocker 在区块结束时执行
func EndBlocker(
    ctx sdk.Context,
    keeper keeper.Keeper,
) {
    ctx = log.AddPersistentTagsToLogger(ctx,
        log.Handler, log.EndBlocker,
        log.BlockHeight, ctx.BlockHeight(),
    )
    
    processProposerMatchesEvents := keeper.GetProcessProposerMatchesEvents(ctx)
    
    // 1. 清理短期订单的过期填充量
    keeper.PruneStateFillAmountsForShortTermOrders(ctx)
    
    // 2. 移除过期的有状态订单
    expiredStatefulOrderIds := keeper.RemoveExpiredStatefulOrders(ctx, ctx.BlockTime())
    for _, orderId := range expiredStatefulOrderIds {
        // 清理填充量
        keeper.RemoveOrderFillAmount(ctx, orderId)
        // 删除订单放置记录
        keeper.DeleteLongTermOrderPlacement(ctx, orderId)
        
        // 发送 Indexer 事件
        keeper.GetIndexerEventManager().AddBlockEvent(
            ctx,
            indexerevents.SubtypeStatefulOrder,
            indexer_manager.IndexerTendermintEvent_BLOCK_EVENT_END_BLOCK,
            indexerevents.StatefulOrderEventVersion,
            indexer_manager.GetBytes(
                indexerevents.NewStatefulOrderRemovalEvent(
                    orderId,
                    indexershared.OrderRemovalReason_ORDER_REMOVAL_REASON_EXPIRED,
                ),
            ),
        )
    }
    
    // 3. 更新过期订单ID列表
    processProposerMatchesEvents.ExpiredStatefulOrderIds = expiredStatefulOrderIds
    
    // 4. 放置 TWAP 子订单
    keeper.GenerateAndPlaceTriggeredTwapSuborders(ctx)
    
    // 5. 触发条件订单
    triggeredConditionalOrderIds := keeper.MaybeTriggerConditionalOrders(ctx)
    processProposerMatchesEvents.ConditionalOrderIdsTriggeredInLastBlock = triggeredConditionalOrderIds
    
    // 6. 保存更新后的事件
    keeper.MustSetProcessProposerMatchesEvents(ctx, processProposerMatchesEvents)
    
    // 7. 发送保险基金余额指标
    metrics.SetGauge(
        metrics.InsuranceFundBalance,
        metrics.GetMetricValueFromBigInt(keeper.GetCrossInsuranceFundBalance(ctx)),
    )
}
```

### 5.5 PrepareCheckState - 准备检查状态 (最复杂)

```Go
// PrepareCheckState 在 Commit 后、下一个区块的 CheckTx 前执行
func PrepareCheckState(
    ctx sdk.Context,
    keeper *keeper.Keeper,
) {
    ctx = log.AddPersistentTagsToLogger(ctx,
        log.Handler, log.PrepareCheckState,
        log.BlockHeight, ctx.BlockHeight()+1,  // 为下一个区块准备
    )
    
    // ========== 阶段 0: 获取子账户快照 ==========
    var subaccountSnapshots map[satypes.SubaccountId]*satypes.StreamSubaccountUpdate
    if keeper.GetFullNodeStreamingManager().Enabled() {
        subaccountSnapshots = keeper.GetSubaccountSnapshotsForInitStreams(ctx)
    }
    
    // 清理速率限制
    keeper.PruneRateLimits(ctx)
    
    // 获取上一个区块的匹配事件
    processProposerMatchesEvents := keeper.GetProcessProposerMatchesEvents(ctx)
    
    // ========== 阶段 1: 获取并清空本地操作队列 ==========
    localValidatorOperationsQueue, shortTermOrderTxBytes := keeper.MemClob.GetOperationsToReplay(ctx)
    
    log.DebugLog(ctx, "清空本地操作队列",
        log.LocalValidatorOperationsQueue, types.GetInternalOperationsQueueTextString(localValidatorOperationsQueue),
    )
    
    keeper.MemClob.RemoveAndClearOperationsQueue(ctx, localValidatorOperationsQueue)
    
    // ========== 阶段 2: 清理无效的 MemClob 状态 ==========
    offchainUpdates := types.NewOffchainUpdates()
    offchainUpdates = keeper.MemClob.PurgeInvalidMemclobState(
        ctx,
        processProposerMatchesEvents.OrderIdsFilledInLastBlock,      // 已完全成交
        processProposerMatchesEvents.ExpiredStatefulOrderIds,        // 已过期
        keeper.GetDeliveredCancelledOrderIds(ctx),                   // 已取消
        processProposerMatchesEvents.RemovedStatefulOrderIds,        // 已移除
        offchainUpdates,
    )
    
    // ========== 阶段 3: 第一遍 - 仅放置 Post-Only 订单 ==========
    longTermOrderIds := keeper.GetDeliveredLongTermOrderIds(ctx)
    
    // 3.1 放置长期订单 (Post-Only)
    offchainUpdates = keeper.PlaceStatefulOrdersFromLastBlock(
        ctx, longTermOrderIds, offchainUpdates, true, /* postOnly */
    )
    
    // 3.2 放置触发的条件订单 (Post-Only)
    offchainUpdates = keeper.PlaceConditionalOrdersTriggeredInLastBlock(
        ctx, processProposerMatchesEvents.ConditionalOrderIdsTriggeredInLastBlock,
        offchainUpdates, true, /* postOnly */
    )
    
    // 3.3 重放本地操作 (Post-Only)
    replayUpdates := keeper.MemClob.ReplayOperations(
        ctx, localValidatorOperationsQueue, shortTermOrderTxBytes,
        offchainUpdates, true, /* postOnly */
    )
    if replayUpdates != nil {
        offchainUpdates = replayUpdates
    }
    
    // ========== 阶段 4: 第二遍 - 放置所有订单 ==========
    // 4.1 放置长期订单
    offchainUpdates = keeper.PlaceStatefulOrdersFromLastBlock(
        ctx, longTermOrderIds, offchainUpdates, false, /* postOnly */
    )
    
    // 4.2 放置触发的条件订单
    offchainUpdates = keeper.PlaceConditionalOrdersTriggeredInLastBlock(
        ctx, processProposerMatchesEvents.ConditionalOrderIdsTriggeredInLastBlock,
        offchainUpdates, false, /* postOnly */
    )
    
    // 4.3 重放本地操作
    replayUpdates = keeper.MemClob.ReplayOperations(
        ctx, localValidatorOperationsQueue, shortTermOrderTxBytes,
        offchainUpdates, false, /* postOnly */
    )
    if replayUpdates != nil {
        offchainUpdates = replayUpdates
    }
    
    // ========== 阶段 5: 清算 ==========
    liquidatableSubaccountIds := keeper.DaemonLiquidationInfo.GetLiquidatableSubaccountIds()
    subaccountsToDeleverage, err := keeper.LiquidateSubaccountsAgainstOrderbook(ctx, liquidatableSubaccountIds)
    if err != nil {
        panic(err)
    }
    
    // 添加最终结算市场中的仓位
    subaccountsToDeleverage = append(
        subaccountsToDeleverage,
        keeper.GetSubaccountsWithPositionsInFinalSettlementMarkets(ctx)...,
    )
    
    // ========== 阶段 6: 去杠杆化 ==========
    if err := keeper.DeleverageSubaccounts(ctx, subaccountsToDeleverage); err != nil {
        panic(err)
    }
    
    // ========== 阶段 7: 提款门控 ==========
    negativeTncSubaccountIds := keeper.DaemonLiquidationInfo.GetNegativeTncSubaccountIds()
    if err := keeper.GateWithdrawalsIfNegativeTncSubaccountSeen(ctx, negativeTncSubaccountIds); err != nil {
        panic(err)
    }
    
    // ========== 阶段 8: 发送离链更新 ==========
    keeper.SendOffchainMessages(offchainUpdates, nil, metrics.SendPrepareCheckStateOffchainUpdates)
    
    // 初始化新的流式连接
    keeper.InitializeNewStreams(ctx, subaccountSnapshots)
    
    // 设置指标
    keeper.MemClob.SetMemclobGauges(ctx)
}
```

**PrepareCheckState 的关键设计**:

1. **两遍订单放置**:
   1. 第一遍仅放置 Post-Only 订单,建立订单簿深度
   2. 第二遍放置所有订单,允许匹配
   3. 避免新订单与自己的 Post-Only 订单成交
2. **操作队列重放**: 将上一个区块产生的本地操作重新放入订单簿
3. **清算和去杠杆化**: 在新区块开始前处理风险头寸

### 共识流程

```Go
用户提交订单交易
    ↓
CheckTx（所有节点）
    ├─ 验证订单格式和签名
    ├─ 验证账户余额
    ├─ 放入 MemClob（内存订单簿）
    ├─ 乐观匹配（Optimistic Matching）
    └─ 操作加入 operationsToPropose
    ↓
Flood Gossip 传播交易
    ↓
PrepareProposal（仅 Proposer）【Propose阶段】
    ├─ 从 MemClob 获取 operationsToPropose
    ├─ 构建 MsgProposedOperations
    ├─ 生成价格更新交易（UpdateMarketPrices）
    ├─ 生成资金费率投票（AddPremiumVotes）
    ├─ 生成桥接确认（AcknowledgeBridges）
    └─ 组装区块提案（按字节限制组织交易）
    ↓
ProcessProposal（所有节点）【Prevote阶段】
    ├─ 解码交易
    ├─ 验证交易顺序
    ├─ 验证必需的应用注入消息
    ├─ 验证价格更新有效性
    └─ 返回 ACCEPT 或 REJECT
    ↓
FinalizeBlock（所有节点）
    ├─ PreBlock
    │   └─ 模块初始化（如 CLOB.Initialize）
    │
    ├─ BeginBlock
    │   ├─ 初始化 ProcessProposerMatchesEvents
    │   └─ 重置已交付订单 ID
    │
    ├─ DeliverTx（对每个交易顺序执行）
    │   ├─ DeliverTx[0]: UpdateMarketPrices
    │   │   └─ 更新市场价格
    │   │
    │   ├─ DeliverTx[1]: AddPremiumVotes
    │   │   └─ 添加资金费率投票
    │   │
    │   ├─ DeliverTx[2]: AcknowledgeBridges
    │   │   └─ 确认桥接事件
    │   │
    │   ├─ DeliverTx[3]: ProposedOperations ⭐核心
    │   │   ├─ 确定性匹配（Deterministic Matching）
    │   │   ├─ 验证匹配结果
    │   │   ├─ 更新账户余额
    │   │   ├─ 记录已成交订单
    │   │   └─ 更新订单簿状态
    │   │
    │   └─ DeliverTx[4...N]: 其他用户交易
    │       └─ 执行普通交易（转账、质押等）
    │
    └─ EndBlock
        ├─ 清理过期订单
        ├─ 修剪状态填充量
        └─ 更新 MemStore
    ↓
Precommit（所有节点）【Precommit阶段】
    ├─ ProcessStagedFinalizeBlockEvents
    │   └─ 处理 MemClob 订单簿创建等副作用
    ├─ ProduceBlock（索引器）
    │   └─ 生成索引器区块数据
    └─ SendOnchainData
        └─ 发送索引器数据到 Kafka
    ↓
Commit（所有节点）
    ├─ 提交状态到数据库
    ├─ 更新 AppHash
    └─ 保存区块到 BlockStore
    ↓
PrepareCheckState（所有节点）
    ├─ 清除本地操作队列（operationsToPropose）
    ├─ 清理无效状态
    ├─ 重放本地操作队列
    │   └─ 重新匹配未成交订单
    └─ 放置新订单到 MemClob
    ↓
下一个区块开始
```

## 六、Ante Handler - 交易入口

**文件位置**: clob.go

```Go
// ClobDecorator 负责:
// - 在 CheckTx 模式下将短期订单添加到内存订单簿
// - 在 CheckTx 和 ReCheckTx 模式下将有状态订单添加到状态
// - 在 DeliverTx 模式下不做处理 (由 MsgServer 处理)
type ClobDecorator struct {
    clobKeeper    types.ClobKeeper
    sendingKeeper sendingtypes.SendingKeeper
}

func (cd ClobDecorator) AnteHandle(
    ctx sdk.Context,
    tx sdk.Tx,
    simulate bool,
    next sdk.AnteHandler,
) (sdk.Context, error) {
    // DeliverTx 或模拟模式下直接跳过
    if lib.IsDeliverTxMode(ctx) || simulate {
        return next(ctx, tx, simulate)
    }
    
    // 验证交易格式
    if err := ValidateMsgsInClobTx(tx); err != nil {
        return ctx, err
    }
    
    // 检查 CLOB Keeper 是否已初始化
    if !cd.clobKeeper.IsInMemStructuresInitialized() {
        return ctx, errorsmod.Wrap(
            types.ErrClobNotInitialized,
            "clob keeper 未初始化,请等待下一个区块",
        )
    }
    
    msgs := tx.GetMsgs()
    
    var err error
    for _, msg := range msgs {
        switch msg := msg.(type) {
        // ========== 取消订单 ==========
        case *types.MsgCancelOrder:
            if msg.OrderId.IsStatefulOrder() {
                // 有状态订单取消 - 写入状态
                err = cd.clobKeeper.CancelStatefulOrder(ctx, msg)
            } else {
                // 短期订单取消 - ReCheckTx 跳过
                if ctx.IsReCheckTx() {
                    return next(ctx, tx, simulate)
                }
                // 内存取消
                err = cd.clobKeeper.CancelShortTermOrder(ctx, msg)
            }
            
        // ========== 放置订单 ==========
        case *types.MsgPlaceOrder:
            if msg.Order.OrderId.IsStatefulOrder() {
                // 有状态订单 - 写入状态并验证
                err = cd.clobKeeper.PlaceStatefulOrder(ctx, msg, false)
            } else {
                // 短期订单 - ReCheckTx 跳过
                if ctx.IsReCheckTx() {
                    return next(ctx, tx, simulate)
                }
                
                // HOTFIX: 验证 timeout height
                if timeoutHeight := GetTimeoutHeight(tx); timeoutHeight > 0 &&
                    timeoutHeight < uint64(msg.Order.GetGoodTilBlock()) && ctx.IsCheckTx() {
                    return ctx, errorsmod.Wrap(
                        sdkerrors.ErrInvalidRequest,
                        "timeout height 不能小于 goodTilBlock",
                    )
                }
                
                // 放置到内存订单簿并尝试匹配
                var orderSizeOptimisticallyFilledFromMatchingQuantums satypes.BaseQuantums
                var status types.OrderStatus
                orderSizeOptimisticallyFilledFromMatchingQuantums, status, err = 
                    cd.clobKeeper.PlaceShortTermOrder(ctx, msg)
                
                log.DebugLog(ctx, "收到新短期订单",
                    log.OrderHash, msg.Order.GetOrderHash(),
                    log.OrderStatus, status,
                    log.OrderSizeOptimisticallyFilledFromMatchingQuantums, orderSizeOptimisticallyFilledFromMatchingQuantums,
                )
            }
            
        // ========== 批量取消 ==========
        case *types.MsgBatchCancel:
            if ctx.IsReCheckTx() {
                return next(ctx, tx, simulate)
            }
            
            success, failures, err := cd.clobKeeper.BatchCancelShortTermOrder(ctx, msg)
            if len(success) == 0 && err == nil {
                err = errorsmod.Wrapf(
                    types.ErrBatchCancelFailed,
                    "没有成功的取消,失败: %+v", failures,
                )
            }
            
        // ========== 转账 (用于隔离仓位) ==========
        case *sendingtypes.MsgCreateTransfer:
            if err := cd.sendingKeeper.ProcessTransfer(ctx, msg.Transfer); err != nil {
                return ctx, err
            }
        }
    }
    
    if err != nil {
        return ctx, err
    }
    
    return next(ctx, tx, simulate)
}
```

## 七、订单匹配与撮合流程详解

### 订单流程

```Go
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                           订单处理流程对比                                           │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                     │
│  【短期订单 Short-Term Order】                                                       │
│  ══════════════════════════════                                                     │
│                                                                                     │
│   客户端 ──▶ CheckTx ──▶ ClobDecorator.AnteHandle ──▶ PlaceShortTermOrder           │
│                                    │                          │                     │
│                                    │                          ▼                     │
│                                    │                   MemClob.PlaceOrder           │
│                                    │                          │                     │
│                                    │                          ▼                     │
│                                    │               [仅存储在内存订单簿]              │
│                                    │                                                │
│                                    ▼                                                │
│                          ❌ 不进入 DeliverTx                                        │
│                          ❌ 不写入链上状态                                           │
│                                                                                     │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                     │
│  【长期订单 Long-Term Order / 条件订单 Conditional Order】                           │
│  ══════════════════════════════════════════════════════════                         │
│                                                                                     │
│   客户端 ──▶ CheckTx ──▶ ClobDecorator.AnteHandle ──▶ PlaceStatefulOrder            │
│                                    │                          │                     │
│                                    │                          ▼                     │
│                                    │              [写入 Uncommitted 瞬态存储]        │
│                                    │                                                │
│                                    ▼                                                │
│              DeliverTx ──▶ msg_server.PlaceOrder ──▶ HandleMsgPlaceOrder            │
│                                                              │                      │
│                                                              ▼                      │
│                                                    PlaceStatefulOrder               │
│                                                              │                      │
│                                                              ▼                      │
│                                                   [写入链上 KVStore 状态]            │
│                                                              │                      │
│                                                              ▼                      │
│                                               PrepareCheckState 中放入 MemClob      │
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────────────┐
│                              完整订单处理流程                                          │
├──────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                      │
│  用户提交 MsgPlaceOrder                                                               │
│         │                                                                            │
│         ▼                                                                            │
│  ┌─────────────────┐                                                                 │
│  │   ValidateBasic │  ← 无状态验证                                                   │
│  └────────┬────────┘                                                                 │
│           │                                                                          │
│           ▼                                                                          │
│  ┌─────────────────────────────────────────┐                                         │
│  │         ClobDecorator.AnteHandle        │  ← CheckTx 阶段                         │
│  │                                         │                                         │
│  │   if order.IsStatefulOrder() {          │                                         │
│  │       // 长期订单/条件订单               │                                         │
│  │       PlaceStatefulOrder()              │──▶ 写入 Uncommitted Transient Store    │
│  │   } else {                              │                                         │
│  │       // 短期订单                        │                                         │
│  │       PlaceShortTermOrder() ────────────│──▶ 直接放入 MemClob，撮合               │
│  │   }                                     │    ❌ 不进入 DeliverTx                  │
│  └────────┬────────────────────────────────┘                                         │
│           │                                                                          │
│           │ (仅状态化订单)                                                            │
│           ▼                                                                          │
│  ┌─────────────────────────────────────────┐                                         │
│  │     msgServer.PlaceOrder (DeliverTx)    │                                         │
│  │                                         │                                         │
│  │   HandleMsgPlaceOrder()                 │                                         │
│  │       │                                 │                                         │
│  │       ├─▶ PlaceStatefulOrder()          │──▶ 写入 KVStore (链上状态)              │
│  │       │                                 │                                         │
│  │       └─▶ AddDeliveredLongTermOrderId() │──▶ 记录已交付的订单ID                   │
│  └────────┬────────────────────────────────┘                                         │
│           │                                                                          │
│           ▼                                                                          │
│  ┌─────────────────────────────────────────┐                                         │
│  │          PrepareCheckState              │                                         │
│  │                                         │                                         │
│  │   PlaceStatefulOrdersFromLastBlock()    │──▶ 将链上长期订单放入 MemClob            │
│  │                                         │                                         │
│  │   PlaceConditionalOrdersTriggered...()  │──▶ 将触发的条件订单放入 MemClob          │
│  └─────────────────────────────────────────┘                                         │
│                                                                                      │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

短期订单：

特性描述处理阶段仅在 CheckTx 阶段处理存储位置仅存储在 [MemClob](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html) 内存中链上状态❌ 不写入 KVStore过期方式[GoodTilBlock](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html) - 指定区块高度共识方式通过 OperationsToPropose 队列，在区块提议时打包撮合结果DeliverTx❌ 不进入 DeliverTx 的 MsgServer

长期订单：

特性描述处理阶段CheckTx 验证 + DeliverTx 持久化存储位置KVStore (链上) + MemClob (内存)链上状态✅ 写入 KVStore过期方式[GoodTilBlockTime](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html) - 指定时间戳共识方式通过标准 Cosmos SDK 共识流程DeliverTx✅ 进入 MsgServer.PlaceOrder何时进入 MemClob[PrepareCheckState](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html) 阶段

### **五种订单类型的存储和流程详解**

#### 短期订单

**特征:**

- 有效期: GoodTilBlock (区块高度过期)
- 生命周期: 仅在当前区块内有效
- 速度优先: 不写入区块链存储

**存储架构:**

```Go
CheckTx阶段:
  └─> MemClob (纯内存订单簿)
       └─> Bids/Asks 双向链表

不存储在:
  ✗ KVStore (永久存储)
  ✗ TransientStore
```

**完整流程:**

**阶段1: 提交订单 (CheckTx)**

```Go
// 入口: ante/clob.go ClobDecorator.AnteHandle
用户提交 MsgPlaceOrder
  ↓
AnteHandler 检测到 ShortTerm 订单
  ↓
调用 PlaceShortTermOrder(ctx, msg) // CheckTx only
  ↓
基础验证: GoodTilBlock, ClobPair, Subaccount
  ↓
MemClob.PlaceOrder() // 直接放入内存订单簿 进行撮合
  ↓
订单进入 Bids/Asks 队列
```

**阶段2: 打包区块 (Proposer)**

```Go
// proposer 节点打包区块
// protocol/app/prepare/prepare_proposal.go `GetProposedOperationsTx`
GetOperations(ctx) 获取 OperationsToPropose 队列
  ↓
OperationsQueue 包含:
  - 短期订单撮合结果
  - Matches (成交记录)
  - OrderRemovals (取消/过期)
  ↓
proposer 将 OperationsQueue 打包进区块提案
```

**阶段3: 执行区块 (DeliverTx)**

```Go
// 所有验证节点执行 需要理解cosmos msgServer执行
ProcessProposerOperations(ctx, operations)
  ↓
重放 operations 中的所有撮合结果
  ↓
所有节点达成一致的 MemClob 状态
```

调用链路

```Go
CometBFT Consensus Engine
  ↓
ABCI FinalizeBlock (区块提交阶段)
  ↓
处理区块中的每一笔交易 (DeliverTx 模式)
  ↓
发现 MsgProposedOperations 交易
  ↓
app.BaseApp.DeliverTx()
  ↓
Router 路由到 CLOB 模块
  ↓
msgServer.ProposedOperations() [x/clob/keeper/msg_server_proposed_operations.go:13]
  ↓
keeper.ProcessProposerOperations() [x/clob/keeper/process_operations.go:47]
  ↓
keeper.ProcessInternalOperations() [x/clob/keeper/process_operations.go:133]
  ↓
处理三种操作类型:
  1. ShortTermOrderPlacement (验证)
  2. Match (调用 PersistMatchToState)
  3. OrderRemoval (调用 PersistOrderRemovalToState)
  ↓
更新状态: 子账户余额、仓位、订单成交量等
```

完整执行流程

```Go
┌─────────────────────────────────────────────────────────────────┐
│ 区块 N+1 已通过共识（PrepareProposal + ProcessProposal 完成）        │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 🔥 阶段3: DeliverTx 执行区块（所有验证节点）                          │
│                                                                  │
│ CometBFT FinalizeBlock 阶段                                      │
│ 依次处理区块中的所有交易                                            │
│                                                                  │
│ ┌──────────────────────────────────────────┐                    │
│ │ 交易 1: MsgUpdateMarketPrices (价格更新)  │                    │
│ │   → prices.MsgServer.UpdateMarketPrices()│                    │
│ │   → 更新 Oracle 价格到 KVStore              │                    │
│ └──────────────────────────────────────────┘                    │
│                                                                  │
│ ┌──────────────────────────────────────────┐                    │
│ │ 交易 2: MsgAddPremiumVotes (资金费率)     │                    │
│ │   → perpetuals.MsgServer.AddPremiumVotes │                    │
│ │   → 更新资金费率投票到 KVStore              │                    │
│ └──────────────────────────────────────────┘                    │
│                                                                  │
│ ┌──────────────────────────────────────────┐                    │
│ │ 交易 3: MsgAcknowledgeBridges (桥接事件)   │                    │
│ │   → bridge.MsgServer.AcknowledgeBridges  │                    │
│ │   → 确认桥接事件到 KVStore                  │                    │
│ └──────────────────────────────────────────┘                    │
│                                                                  │
│ ┌──────────────────────────────────────────┐                    │
│ │ 交易 4-N: Other Txs (用户提交的交易)        │                    │
│ │   → 状态订单放置                           │                    │
│ │   → 状态订单取消                           │                    │
│ │   → 转账交易等                             │                    │
│ └──────────────────────────────────────────┘                    │
│                                                                  │
│ ┌──────────────────────────────────────────────────────┐        │
│ │ 🔥 交易 N+1: MsgProposedOperations (CLOB 撮合结果)    │        │
│ │                                                      │        │
│ │ BaseApp.DeliverTx(txBytes)                          │        │
│ │   ↓                                                 │        │
│ │ Router → CLOB Module                                │        │
│ │   ↓                                                 │        │
│ │ msgServer.ProposedOperations()                      │        │
│ │   [msg_server_proposed_operations.go:13]            │        │
│ │   ↓                                                 │        │
│ │ keeper.ProcessProposerOperations()                  │        │
│ │   [process_operations.go:47]                        │        │
│ │                                                      │        │
│ │   步骤 1: ValidateAndTransformRawOperations()       │        │
│ │           无状态验证 + 解码 TX 字节                  │        │
│ │                                                      │        │
│ │   步骤 2: ProcessInternalOperations()               │        │
│ │           遍历所有操作:                              │        │
│ │           ┌────────────────────────────────┐       │        │
│ │           │ ShortTermOrderPlacement × N    │       │        │
│ │           │   → PerformStatefulValidation  │       │        │
│ │           │   → 添加到 placedOrders 映射     │       │        │
│ │           └────────────────────────────────┘       │        │
│ │           ┌────────────────────────────────┐       │        │
│ │           │ Match × M                      │       │        │
│ │           │   → PersistMatchToState()      │       │        │
│ │           │   → ProcessSingleMatch()       │       │        │
│ │           │       - 计算手续费              │       │        │
│ │           │       - 更新子账户余额          │       │        │
│ │           │       - 更新永续合约仓位        │       │        │
│ │           │       - 更新订单成交量          │       │        │
│ │           │       - 分配手续费收入          │       │        │
│ │           │       - 记录统计数据            │       │        │
│ │           │       - 发送索引器事件          │       │        │
│ │           └────────────────────────────────┘       │        │
│ │           ┌────────────────────────────────┐       │        │
│ │           │ OrderRemoval × K               │       │        │
│ │           │   → PersistOrderRemovalToState │       │        │
│ │           │   → 有状态验证移除原因          │       │        │
│ │           │   → MustRemoveStatefulOrder    │       │        │
│ │           └────────────────────────────────┘       │        │
│ │                                                      │        │
│ │   步骤 3: GenerateProcessProposerMatchesEvents()    │        │
│ │           收集成交订单 ID 列表                       │        │
│ │                                                      │        │
│ │   步骤 4: 移除完全成交的长期订单                      │        │
│ │           for orderId in filledOrders:              │        │
│ │             if fillAmount == orderSize:             │        │
│ │               MustRemoveStatefulOrder()             │        │
│ │                                                      │        │
│ │   步骤 5: MustSetProcessProposerMatchesEvents()     │        │
│ │           写入 MemStore (PrepareCheckState 使用)    │        │
│ │                                                      │        │
│ │   步骤 6: EmitStats() 发送统计指标                   │        │
│ │                                                      │        │
│ │ 返回: &MsgProposedOperationsResponse{}              │        │
│ └──────────────────────────────────────────────────────┘        │
│                                                                  │
│ 所有节点的状态现在完全一致:                                        │
│ ✅ 子账户余额已更新                                               │
│ ✅ 永续合约仓位已更新                                             │
│ ✅ 订单成交量已记录                                               │
│ ✅ 手续费已分配（Maker/Taker/Insurance/Revenue Share）            │
│ ✅ 完全成交的长期订单已从 KVStore 移除                              │
│ ✅ ProcessProposerMatchesEvents 已写入 MemStore                  │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ EndBlock 阶段                                                    │
│ - 触发条件订单                                                    │
│ - 生成 TWAP 子订单                                                │
│ - 清理过期订单                                                    │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ Commit 阶段                                                      │
│ - 将所有状态更改持久化到磁盘                                        │
│ - 计算新的 AppHash                                               │
│ - 区块 N+1 最终确认                                              │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ PrepareCheckState 阶段（区块 N+2 开始）                           │
│ - 清空 OperationsToPropose 队列                                  │
│ - 从 Orderbook 移除已处理的订单                                    │
│ - 重新放置状态订单                                                │
│ - 重放短期订单                                                    │
└─────────────────────────────────────────────────────────────────┘
```

**阶段4: 过期清理 (PrepareCheckState)**

```Go
BeginBlocker: 新区块开始
  ↓
短期订单过期 (GoodTilBlock <= currentBlock)
  ↓
PruneStateFillAmountsForShortTermOrders()
  ↓
从 MemClob 移除过期订单
```

#### 长期订单

**特征:**

- 有效期: GoodTilBlockTime (Unix时间戳过期)
- 持久化: 写入区块链存储
- 跨区块生命周期: 可存活多个区块

**存储架构:**

```Go
CheckTx阶段:
  TransientStore: "UncmtSO:" + OrderId
    └─> 防止区块内重复提交

DeliverTx阶段:
  KVStore (永久): "SO/P/L:" + OrderId
    └─> LongTermOrderPlacement {
          Order,
          PlacementIndex {BlockHeight, TransactionIndex}
        }
  
  MemStore: "DLTO:" + Index
    └─> 记录本区块新增的订单ID
  
  ExpirationsStore: "Exp/[time]:" + OrderId
    └─> 按过期时间索引

PrepareCheckState:
  MemClob: 从 KVStore 重新加载到内存订单簿
```

**完整流程:**

**阶段1: 提交订单 (CheckTx)**

```Go
// 入口: ante/clob.go ClobDecorator.AnteHandle
用户提交 MsgPlaceOrder (LongTerm)
  ↓
AnteHandler 检测到 Stateful 订单
  ↓
调用 PlaceStatefulOrder(ctx, msg) // CheckTx 模式
  ↓
验证:
  1. GoodTilBlockTime 有效性
  2. ClobPair 存在性
  3. OrderRouterAddress 有效性
  4. 抵押品充足性检查
  ↓
写入 TransientStore: "UncmtSO:" + OrderId
  └─> 目的: 防止同一区块内重复提交相同订单
  └─> 不持久化,区块结束后清空
```

**阶段2: 执行订单 (DeliverTx)**

```Go
// 入口: msg_server_place_order.go msgServer.PlaceOrder
区块打包后,验证节点执行
  ↓
HandleMsgPlaceOrder() 调用 PlaceStatefulOrder(ctx, msg) // DeliverTx 模式
  ↓
再次执行所有验证 (确保共识一致性)
  ↓
lib.IsDeliverTxMode(ctx) == true
  ↓
永久存储:
  1. SetLongTermOrderPlacement(ctx, order, blockHeight)
     └─> KVStore: "SO/P/L:" + OrderId
     └─> 存储 LongTermOrderPlacement {
           Order: 完整订单信息
           PlacementIndex: {
             BlockHeight: 当前区块高度
             TransactionIndex: 区块内交易序号 (保证时间优先级)
           }
         }
  
  2. AddStatefulOrderIdExpiration(ctx, goodTilBlockTime, orderId)
     └─> 按过期时间索引,方便 EndBlocker 清理
  
  3. MemStore: 记录 OrderId 到 "DLTO:" + Index
     └─> 供 PrepareCheckState 读取
```

**阶段3: 过期清理 (EndBlocker)**

```Go
EndBlocker(ctx, keeper)
  ↓
RemoveExpiredStatefulOrders(ctx, ctx.BlockTime())
  ↓
查询 ExpirationsStore: "Exp/[time]:" 前缀
  └─> 找到所有 GoodTilBlockTime <= 当前时间的订单
  ↓
遍历过期订单:
  1. RemoveOrderFillAmount(ctx, orderId)
  2. DeleteLongTermOrderPlacement(ctx, orderId)
     └─> 从 KVStore 删除
  3. 发送 Indexer 事件: ORDER_REMOVAL_REASON_EXPIRED
  ↓
更新 ProcessProposerMatchesEvents.ExpiredStatefulOrderIds
  ↓
PrepareCheckState 时从 MemClob 移除
```

**阶段4: 同步到内存 (PrepareCheckState)**

```Go
// 区块提交后,准备下一个区块的 CheckState
PrepareCheckState(ctx, keeper)
  ↓
从 MemStore 读取: GetDeliveredLongTermOrderIds(ctx)
  └─> 获取上一区块所有新增长期订单
  ↓
PlaceStatefulOrdersFromLastBlock(ctx, orderIds, ...)
  ↓
遍历每个 orderId:
  1. GetLongTermOrderPlacement(ctx, orderId) 从 KVStore 读取
  2. AddPreexistingStatefulOrder(ctx, order, MemClob)
     └─> 验证订单
     └─> MemClob.PlaceOrder() 放入内存订单簿
  ↓
所有节点的 MemClob 状态同步一致
```

#### 条件订单

**特征:**

- 触发条件: 止盈 (Take Profit) / 止损 (Stop Loss)
- 两阶段存储: Untriggered → Triggered
- Oracle价格驱动触发

**存储架构:**

```Go
未触发状态:
  KVStore: "SO/U:" + OrderId
    └─> UntriggeredConditionalOrderPlacement {
          Order { TriggerSubticks, ... },
          PlacementIndex
        }
  
  MemClob: UntriggeredConditionalOrders 内存结构
    └─> 按触发方向分类:
         - OrdersToTriggerWhenOraclePriceLTETriggerPrice (<=)
         - OrdersToTriggerWhenOraclePriceGTETriggerPrice (>=)

已触发状态:
  KVStore: "SO/P/T:" + OrderId
    └─> TriggeredConditionalOrderPlacement
  
  MemClob: 同长期订单,进入 Bids/Asks 队列
```

**完整流程:**

**阶段1: 提交订单 (CheckTx + DeliverTx)**

CheckTx:

```Go
// 与长期订单流程相同
CheckTx:
  └─> TransientStore: "UncmtSO:" + OrderId

DeliverTx:
  └─> PlaceStatefulOrder(ctx, msg)
      └─> SetLongTermOrderPlacement(ctx, order, blockHeight)
          └─> 检测到 IsConditionalOrder()
          └─> 写入 Untriggered Store: "SO/U:" + OrderId
          └─> 存储 LongTermOrderPlacement {
                Order { TriggerSubticks, ConditionalOrderTriggerSubticks },
                PlacementIndex
              }
```

**阶段2: 触发检测 (EndBlocker)**

```Markdown
EndBlocker(ctx, keeper)
  ↓
MaybeTriggerConditionalOrders(ctx)
  ↓
1. 从 KVStore 读取所有未触发条件订单
   GetAllUntriggeredConditionalOrders(ctx)
   └─> 遍历 "SO/U:" 前缀
  
2. 按 ClobPairId 分组组织
   OrganizeUntriggeredConditionalOrdersFromState()
   └─> 分为两个数组:
       - LTE 数组 (止盈买单 / 止损卖单)
       - GTE 数组 (止盈卖单 / 止损买单)
  
3. 获取 Oracle 价格
   oraclePrice := GetOraclePriceSubticksRat(ctx, clobPair)
  
4. 轮询触发订单
   PollTriggeredConditionalOrders(oraclePrice)
   └─> 遍历 LTE 数组: if oraclePrice <= TriggerSubticks
   └─> 遍历 GTE 数组: if oraclePrice >= TriggerSubticks
   └─> 返回 triggeredOrderIds[]
  
5. 状态迁移
   遍历 triggeredOrderIds:
     a. GetUntriggeredConditionalOrderPlacement(ctx, orderId)
     b. TriggerConditionalOrder(ctx, orderId)
        └─> 从 "SO/U:" 删除
        └─> 写入 "SO/P/T:" (Triggered Store)
     c. 发送 Indexer 事件: ConditionalOrderTriggered
  
6. 记录触发列表
   ProcessProposerMatchesEvents.ConditionalOrderIdsTriggeredInLastBlock = triggeredOrderIds
   └─> 供下一区块 PrepareCheckState 使用
```

**阶段3: 放入订单簿 (PrepareCheckState)**

```Go
PrepareCheckState(ctx, keeper)
  ↓
读取上一区块触发的条件订单
  triggeredIds := ProcessProposerMatchesEvents.ConditionalOrderIdsTriggeredInLastBlock
  ↓
PlaceConditionalOrdersTriggeredInLastBlock(ctx, triggeredIds, ...)
  ↓
遍历每个 orderId:
  1. GetTriggeredConditionalOrderPlacement(ctx, orderId) // 从 "SO/P/T:" 读取
  2. 执行抵押品检查 (现在才检查是否有足够保证金)
  3. MemClob.PlaceOrder() 放入内存订单簿
     └─> 如果抵押品不足,订单失败但不影响其他订单
```

**阶段4: 撮合与清理**

```Go
// 条件订单触发后,行为与长期订单相同
在 MemClob 中等待撮合
  ↓
过期时间到达: EndBlocker 清理
  ↓
DeleteLongTermOrderPlacement(ctx, orderId)
  └─> 从 "SO/P/T:" 删除
```

#### **TWAP订单**

**特征:**

- 时间加权平均价格订单
- 自动拆分: 父订单 → 多个子订单
- 定时触发: 每隔 `Interval` 秒生成一个子订单

**存储架构:**

```Go
父订单 (TWAP Parent):
  KVStore: "TWAP:" + ParentOrderId
    └─> TwapOrderPlacement {
          Order { TwapParameters: { TotalLegs, Interval } },
          RemainingLegs: 递减计数器
          RemainingQuantums: 剩余数量
        }

子订单触发器:
  KVStore: "TWAP/T:[timestamp][SuborderId]"
    └─> 按触发时间排序的触发队列
    └─> key: [8字节Unix时间戳][SuborderId编码]
    └─> value: 空 (仅用key存储信息)

子订单 (TWAP Suborder):
  行为完全同长期订单:
    - CheckTx: TransientStore
    - DeliverTx: "SO/P/L:" + SuborderId
    - PrepareCheckState: 放入 MemClob
```

**完整流程:**

**阶段1: 提交父订单 (CheckTx + DeliverTx)**

```Go
// 用户提交 TWAP 父订单
MsgPlaceOrder {
  Order {
    OrderFlags: OrderIdFlags_Twap,
    Quantums: 100_000_000_000, // 总数量
    TwapParameters: {
      TotalLegs: 5,          // 拆分5个子订单
      Interval: 60,          // 每60秒一个
      PriceTolerance: 50000  // 价格容忍度 5%
    }
  }
}

CheckTx:
  └─> TransientStore: "UncmtSO:" + ParentOrderId

DeliverTx:
  └─> PlaceStatefulOrder(ctx, msg)
      └─> order.IsTwapOrder() == true
      └─> SetTWAPOrderPlacement(ctx, order, blockHeight)
          └─> 计算 total_legs = order.TwapParameters.TotalLegs
          └─> 存储 TwapOrderPlacement:
              {
                Order: 完整父订单
                RemainingLegs: 5,      // 初始为 TotalLegs
                RemainingQuantums: 100_000_000_000
              }
          └─> KVStore: "TWAP:" + ParentOrderId
          
          └─> 立即创建第一个子订单触发器:
              AddSuborderToTriggerStore(ctx, suborderId, 0)
              └─> triggerTime = ctx.BlockTime().Unix() + 0 (立即触发)
              └─> SuborderId = {
                    SubaccountId: 同父订单
                    ClientId: 同父订单
                    OrderFlags: OrderIdFlags_TwapSuborder (256)
                    ClobPairId: 同父订单
                  }
              └─> triggerKey = [triggerTime(8字节)][SuborderId编码]
              └─> KVStore: "TWAP/T:" + triggerKey = []
```

**阶段2: 生成子订单 (EndBlocker)**

```Go
EndBlocker(ctx, keeper)
  ↓
GenerateAndPlaceTriggeredTwapSuborders(ctx)
  ↓
1. 遍历触发队列
   triggerStore := GetTWAPTriggerOrderPlacementStore(ctx)
   iterator := triggerStore.Iterator(nil, nil) // 按 key 排序,即按时间排序
   
2. 检查触发时间
   for iterator.Valid():
     triggerTime := TimeFromTriggerKey(iterator.Key()) // 解析前8字节
     if triggerTime > blockTime:
       break // 后续全是未来的触发,退出
     
     suborderId := 从 Key 解析 (跳过前8字节)
     
3. 获取父订单状态
   parentOrderId := ConvertToParentTwapId(suborderId)
   twapOrderPlacement := GetTwapOrderPlacement(ctx, parentOrderId)
   
   if !found:
     // 父订单已取消,删除触发器
     DeleteSuborderFromTriggerStore(ctx, iterator.Key())
     continue
   
4. 生成子订单
   order, isGenerated := GenerateSuborder(ctx, suborderId, twapOrderPlacement, blockTime)
   
   GenerateSuborder 内部逻辑:
     if twapOrderPlacement.RemainingLegs == 0:
       return nil, false // 父订单完成
     
     // 计算子订单数量
     quantumsPerLeg := twapOrderPlacement.RemainingQuantums / twapOrderPlacement.RemainingLegs
     
     // 补偿延迟 (Catchup机制)
     scheduledTime := 父订单开始时间 + (已执行Legs * Interval)
     delay := blockTime - scheduledTime
     if delay > 0:
       catchupLegs := min(delay / Interval, RemainingLegs)
       quantumsPerLeg *= (catchupLegs + 1)
       quantumsPerLeg = min(quantumsPerLeg, TWAP_MAX_SUBORDER_CATCHUP_MULTIPLE * 原始quantumsPerLeg)
     
     // 计算子订单价格
     subticks := calculateSuborderSubticks(ctx, clobPair, twapOrderPlacement)
       └─> 如果父订单 Subticks != 0: 使用固定价格
       └─> 否则: 使用 Oracle 价格 ± PriceTolerance
     
     // 构造子订单
     suborder := Order {
       OrderId: {
         SubaccountId: 同父订单
         ClientId: 同父订单
         OrderFlags: OrderIdFlags_TwapSuborder (256)
         ClobPairId: 同父订单
       },
       Side: 同父订单
       Quantums: quantumsPerLeg,
       Subticks: subticks,
       GoodTilOneof: GoodTilBlockTime (当前时间 + 3秒) // 短暂有效期
     }
     
     return suborder, true
   
5. 处理生成的子订单
   if !isGenerated:
     // 父订单完成
     DeleteTWAPOrderPlacement(ctx, parentOrderId)
     DeleteSuborderFromTriggerStore(ctx, triggerKey)
     continue
   
   // 更新父订单状态
   DecrementTwapOrderRemainingLegs(ctx, twapOrderPlacement)
     └─> RemainingLegs--
     └─> 更新 KVStore: "TWAP:" + ParentOrderId
   
   // 创建下一个子订单的触发器
   nextTriggerKey := AddSuborderToTriggerStore(
     ctx,
     suborder.OrderId,
     int64(twapOrderPlacement.Order.TwapParameters.Interval) // 例如 60秒后
   )
     └─> nextTriggerTime = ctx.BlockTime().Unix() + 60
     └─> KVStore: "TWAP/T:[nextTriggerTime][SuborderId]" = []
   
6. 放置子订单
   err := safeHandleMsgPlaceOrder(ctx, &MsgPlaceOrder{Order: suborder}, true)
     └─> 内部调用标准的 PlaceStatefulOrder 流程
     └─> 子订单被当作普通长期订单处理
     └─> CheckTx: TransientStore
     └─> DeliverTx: "SO/P/L:" + SuborderId
   
   if err != nil:
     // 子订单放置失败 (抵押品不足等)
     DeleteTWAPOrderPlacement(ctx, parentOrderId) // 取消整个 TWAP
     DeleteSuborderFromTriggerStore(ctx, nextTriggerKey)
     发送 Indexer 事件: TWAP Order Removal
```

**阶段3: 子订单撮合 (PrepareCheckState)**

```Go
// 子订单触发后,作为普通长期订单处理
PrepareCheckState(ctx, keeper)
  ↓
longTermOrderIds := GetDeliveredLongTermOrderIds(ctx)
  └─> 包含所有 OrderIdFlags_TwapSuborder 订单
  ↓
PlaceStatefulOrdersFromLastBlock(ctx, longTermOrderIds, ...)
  ↓
GetLongTermOrderPlacement(ctx, suborderId)
  └─> 从 "SO/P/L:" + SuborderId 读取
  ↓
MemClob.PlaceOrder(ctx, suborder)
  └─> 进入 Bids/Asks 队列等待撮合
```

**阶段4: 成交回调**

```Go
// 子订单成交时
ProcessSingleMatch(ctx, match)
  ↓
if orderId.IsTwapSuborder():
  ↓
  从 SuborderId 解析 ParentOrderId
  ↓
  UpdateTWAPOrderRemainingQuantityOnFill(ctx, parentOrderId, filledQuantums)
    └─> twapOrderPlacement.RemainingQuantums -= filledQuantums
    └─> 更新 KVStore: "TWAP:" + ParentOrderId
  
  如果 RemainingQuantums == 0:
    └─> 父订单提前完成
    └─> DeleteTWAPOrderPlacement(ctx, parentOrderId)
    └─> 后续子订单触发器被清理
```

#### **TWAP子订单**

**特征:**

- 内部使用,用户不直接创建
- 由父 TWAP 订单自动生成
- 行为完全等同长期订单

**存储架构:**

```Go
完全同长期订单:
  CheckTx: TransientStore: "UncmtSO:" + SuborderId
  DeliverTx: KVStore: "SO/P/L:" + SuborderId
  PrepareCheckState: MemClob
```

**流程:**

```Go
生成 (EndBlocker):
  └─> GenerateAndPlaceTriggeredTwapSuborders()

放置 (DeliverTx):
  └─> safeHandleMsgPlaceOrder(ctx, suborder)
      └─> PlaceStatefulOrder() // 标准流程

撮合 (PrepareCheckState):
  └─> PlaceStatefulOrdersFromLastBlock()
      └─> MemClob.PlaceOrder()

成交回调:
  └─> UpdateTWAPOrderRemainingQuantityOnFill()
      └─> 更新父订单 RemainingQuantums
```

### 订单撮合流程

```Plain
// 文件: memclob/memclob.go

PlaceOrder(order)
    │
    ├─→ validateNewOrder()     // 验证订单有效性
    │
    ├─→ matchOrder()           // 核心撮合逻辑
    │       │
    │       ├─→ 遍历对手方价格档位
    │       ├─→ 价格-时间优先匹配
    │       ├─→ 抵押品检查
    │       └─→ 生成成交记录
    │
    ├─→ 订单未完全成交?
    │       │
    │       ├─→ IOC: 取消剩余
    │       ├─→ FOK: 全部取消
    │       └─→ 其他: 挂单
    │
    └─→ mustAddOrderToOrderbook()  // 添加到订单簿
```

订单撮合示例

```Go
初始状态:
  Taker: Buy 100 BTC @ 50100 (用户提交)
  
  订单簿 Asks (卖单):
    Maker1: Sell 30 BTC @ 50000
    Maker2: Sell 40 BTC @ 50050
    Maker3: Sell 50 BTC @ 50080
    Maker4: Sell 20 BTC @ 50200

撮合过程:

循环 1:
  ├─ 找到 Maker1: Sell 30 @ 50000
  ├─ 价格检查: 50100 >= 50000 ✅
  ├─ 自成交检查: 不同子账户 ✅
  ├─ 成交数量: min(100, 30) = 30
  ├─ 抵押品检查: ✅
  ├─ 记录: newMakerFills = [{ Maker1, 30 }]
  └─ takerRemainingSize = 100 - 30 = 70

循环 2:
  ├─ 找到 Maker2: Sell 40 @ 50050
  ├─ 价格检查: 50100 >= 50050 ✅
  ├─ 成交数量: min(70, 40) = 40
  ├─ 抵押品检查: ✅
  ├─ 记录: newMakerFills = [{ Maker1, 30 }, { Maker2, 40 }]
  └─ takerRemainingSize = 70 - 40 = 30

循环 3:
  ├─ 找到 Maker3: Sell 50 @ 50080
  ├─ 价格检查: 50100 >= 50080 ✅
  ├─ 成交数量: min(30, 50) = 30
  ├─ 抵押品检查: ✅
  ├─ 记录: newMakerFills = [{ Maker1, 30 }, { Maker2, 40 }, { Maker3, 30 }]
  └─ takerRemainingSize = 30 - 30 = 0

退出条件:
  ✅ takerRemainingSize == 0 (Taker完全成交)

最终结果:
  - Taker Buy 100 BTC 完全成交
  - 成交价格: 
      30 BTC @ 50000
      40 BTC @ 50050
      30 BTC @ 50080
  - 平均成交价: (30*50000 + 40*50050 + 30*50080) / 100 = 50043
  - Maker1 完全成交，从订单簿移除
  - Maker2 完全成交，从订单簿移除
  - Maker3 部分成交，剩余 20 BTC 留在订单簿
```

**ProcessSingleMatch**

负责处理单笔订单撮合的所有逻辑。

**核心职责：**

1. ✅ **验证撮合有效性**（无状态 + 有状态验证）
2. ✅ **计算手续费**（Taker fee + Maker fee/rebate）
3. ✅ **更新子账户余额**（USDC + 持仓）
4. ✅ **检查抵押品充足性**（保证金要求）
5. ✅ **转移手续费**（Fee Collector + Revenue Share）
6. ✅ **更新成交量统计**（用户统计 + 奖励系统）
7. ✅ **更新订单填充量**（Fill Amount）
8. ✅ **发出事件**（链上事件 + 索引器）

### 条件订单触发流程

```Plain
┌─────────────────────────────────────────────────────────┐
│                  条件订单生命周期                         │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  1. 用户提交条件订单 (MsgPlaceOrder)                     │
│         │                                               │
│         ▼                                               │
│  ┌─────────────────┐                                    │
│  │ 存储为未触发状态 │  UntriggeredConditionalOrders     │
│  └────────┬────────┘                                    │
│           │                                             │
│           ▼                                             │
│  2. 每个区块检查触发条件                                  │
│     (PreBlocker / EndBlocker)                           │
│           │                                             │
│     ┌─────┴─────┐                                       │
│     │           │                                       │
│     ▼           ▼                                       │
│  未满足条件   满足条件                                    │
│  (继续等待)      │                                       │
│                 ▼                                       │
│  ┌─────────────────────────────────┐                    │
│  │ MaybeTriggerConditionalOrders() │                    │
│  │ • 检查 Oracle 价格              │                    │
│  │ • Stop-Loss: price ≤ trigger    │                    │
│  │ • Take-Profit: price ≥ trigger  │                    │
│  └───────────────┬─────────────────┘                    │
│                  │                                      │
│                  ▼                                      │
│  3. 触发后转为普通订单                                    │
│     ┌─────────────────────────────┐                     │
│     │ TriggeredConditionalOrders  │                     │
│     └──────────────┬──────────────┘                     │
│                    │                                    │
│                    ▼                                    │
│  4. 在 MemClob 中进行撮合                               │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### OperationsToPropose

```Go
时间轴: ─────────────────────────────────────────────────────────>

区块 N-1 Commit
    ↓
┌──────────────────────────────────────────┐
│ 区块 N CheckTx 阶段（持续 1-2 秒）          │
│                                          │
│ T0: 用户 A 提交 Order1                    │
│   → PlaceOrder()                        │
│   → matchOrder() 撮合                    │
│   → MustAddShortTermOrderTxBytes()      │ ← 添加 TxBytes
│   → MustAddShortTermOrderPlacement()    │ ← 添加 Placement
│   → MustAddMatchToOperationsQueue()     │ ← 添加 Match
│                                          │
│ T1: 用户 B 提交 Order2                    │
│   → 同样流程，累积到队列                   │
│                                          │
│ T2: Order3 GTB 过期                      │
│   → MustAddOrderRemovalToOperationsQueue│ ← 添加 Removal
│                                          │
│ ... 持续累积                              │
│                                          │
│ 🔥 此时 OperationsToPropose 包含:        │
│   - ShortTermOrderPlacement × N         │
│   - Match × M                           │
│   - OrderRemoval × K                    │
└──────────────────────────────────────────┘
    ↓
CometBFT 选中当前节点为 Proposer
    ↓
┌──────────────────────────────────────────┐
│ 区块 N+1 PrepareProposal（瞬时完成）       │
│                                          │
│ Proposer 节点执行:                        │
│   → PrepareProposalHandler()            │
│   → GetProposedOperationsTx()           │
│   → keeper.GetOperations()              │
│   → MemClob.GetOperationsRaw()          │
│   → operationsToPropose.GetOpsToPropose│
│                                          │
│ 🔥 读取完整的 OperationsToPropose 队列    │
│ 🔥 转换为 MsgProposedOperations          │
│ 🔥 编码为交易字节                         │
│ 🔥 打包进区块提案                         │
│                                          │
│ 返回: ResponsePrepareProposal            │
│   Txs: [价格, 资金, 桥接, Other,          │
│         MsgProposedOperations, ...]     │
└──────────────────────────────────────────┘
    ↓
区块 N+1 Consensus（投票、Commit）
    ↓
┌──────────────────────────────────────────┐
│ 区块 N+1 Commit 后 PrepareCheckState      │
│                                          │
│ 所有节点执行:                             │
│   → PrepareCheckState()                 │
│   → GetOperationsToReplay()             │ ← 复制队列
│   → RemoveAndClearOperationsQueue()     │
│   → ClearOperationsQueue()              │
│                                          │
│ 🔥 清空 OperationsQueue                  │
│ 🔥 清空 OrderHashesInOperationsQueue     │
│ 🔥 清空 MatchedOrderIdToOrder            │
│ 🔥 清空 OrderRemovalsInOperationsQueue   │
│                                          │
│ 🔥 从 Orderbook 移除已处理的订单           │
│ 🔥 重新放置状态订单                       │
│ 🔥 重放短期订单                           │
└──────────────────────────────────────────┘
    ↓
┌──────────────────────────────────────────┐
│ 区块 N+2 CheckTx 阶段                     │
│ 🔥 全新的 OperationsToPropose 队列开始累积 │
└──────────────────────────────────────────┘
```

## 八、清算机制详解

### 清算流程

```Go
 ┌─────────────────────────────────────────┐
  │ 1. 清算守护进程检测可清算账户             │
  │    IsLiquidatable(): MMR > NC && MMR > 0│
  └────────────────┬────────────────────────┘
                   │
                   ▼
  ┌─────────────────────────────────────────┐
  │ 2. 生成清算订单                          │
  │    - 选择清算的永续合约                   │
  │    - 计算清算数量（全部持仓）             │
  │    - 计算清算价格（折价/溢价）            │
  └────────────────┬────────────────────────┘
                   │
                   ▼
  ┌─────────────────────────────────────────┐
  │ 3. 提交清算订单                          │
  │    Keeper.PlacePerpetualLiquidation()   │
  └────────────────┬────────────────────────┘
                   │
                   ▼
  ┌─────────────────────────────────────────┐
  │ 4. MemClob订单匹配                       │
  │    matchOrder() 遍历订单簿               │
  └────────────────┬────────────────────────┘
                   │
       ┌───────────┴───────────┐
       │                       │
       ▼                       ▼
  ┌─────────┐           ┌──────────┐
  │价格交叉?│    NO     │ 匹配结束 │
  └────┬────┘  ────────>└──────────┘
       │ YES
       ▼
  ┌─────────────────────────────────────────┐
  │ 5. 单笔成交处理                          │
  │    ProcessSingleMatch()                 │
  │    ├─ 计算成交数量                       │
  │    ├─ 处理Reduce-Only逻辑                │
  │    ├─ 执行抵押品检查 ───┐                │
  │    └─ 更新子账户状态     │                │
  └──────────────────────────┼───────────────┘
                             │
                             ▼
            ┌────────────────────────────┐
            │ 6. 清算特殊验证             │
            │ validateMatchedLiquidation()│
            │ ├─ 保险基金变化计算         │
            │ ├─ 保险基金充足性验证       │
            │ └─ 区块限额验证             │
            └────────┬───────────────────┘
                     │
         ┌───────────┴──────────┐
         │                      │
         ▼                      ▼
     ┌────────┐           ┌──────────┐
     │ 成功   │           │  失败    │
     └───┬────┘           └────┬─────┘
         │                     │
         ▼                     ▼
  ┌──────────────┐    ┌─────────────────┐
  │记录成交       │    │移除Maker订单    │
  │更新订单簿     │    │(抵押品不足)     │
  └──────┬───────┘    └─────────────────┘
         │
         ▼
  ┌─────────────────────────────────────────┐
  │ 7. 循环匹配直到:                         │
  │    - 清算订单完全成交                    │
  │    - 无可匹配订单                        │
  │    - 无价格交叉                          │
  └────────────────┬────────────────────────┘
                   │
                   ▼
  ┌─────────────────────────────────────────┐
  │ 8. 状态更新                              │
  │    - 更新订单簿                          │
  │    - 更新子账户余额/持仓                 │
  │    - 记录清算信息（防止重复清算）         │
  │    - 发送链下消息                        │
  └─────────────────────────────────────────┘
         ┌─────────────────────────┐
         │  Liquidation Daemon     │  (链下守护进程)
         │  监控可清算账户          │
         └───────────┬─────────────┘
                     │ 提供可清算账户列表
                     ▼
┌─────────────────────────────────────────────────┐
│           PrepareCheckState                     │
├─────────────────────────────────────────────────┤
│                                                 │
│  LiquidateSubaccountsAgainstOrderbook()         │
│         │                                       │
│         ├─→ MaybeGetLiquidationOrder()          │
│         │      • 检查账户是否可清算               │
│         │      • 获取要清算的永续仓位             │
│         │      • 计算清算价格（破产价格）          │
│         │                                       │
│         ├─→ SortLiquidationOrders()             │
│         │      • 最水下账户优先清算               │
│         │                                       │
│         └─→ PlacePerpetualLiquidation()         │
│                • 作为 IOC 订单与订单簿匹配        │
│                • 成交则清算成功                  │
│                • 未成交则需要 Deleveraging       │
│                                                 │
└─────────────────────────────────────────────────┘
```

### 8.1 清算流程入口

**文件位置:** liquidations.go

```Go
// LiquidateSubaccountsAgainstOrderbook 接收子账户 ID 列表并针对订单簿进行清算
// 它会清算尽可能多的子账户，最多清算每个区块的最大清算数量
// 子账户通过伪随机生成的偏移量选择
func (k Keeper) LiquidateSubaccountsAgainstOrderbook(
    ctx sdk.Context,
    subaccountIds []satypes.SubaccountId,
) (
    subaccountsToDeleverage []subaccountToDeleverage,
    err error,
) {
    lib.AssertCheckTxMode(ctx)

    metrics.AddSample(
        metrics.LiquidationsLiquidatableSubaccountIdsCount,
        float32(len(subaccountIds)),
    )

    // 空列表早期返回
    numSubaccounts := len(subaccountIds)
    if numSubaccounts == 0 {
        return nil, nil
    }

    // 获取清算订单
    pseudoRand := k.GetPseudoRand(ctx)
    liquidationOrders := make([]types.LiquidationOrder, 0)
    numLiqOrders := lib.Min(numSubaccounts, int(k.Flags.MaxLiquidationAttemptsPerBlock))
    indexOffset := pseudoRand.Intn(numSubaccounts)  // 伪随机起始位置

    for i := 0; i < numLiqOrders; i++ {
        index := (i + indexOffset) % numSubaccounts
        subaccountId := subaccountIds[index]
        liquidationOrder, err := k.MaybeGetLiquidationOrder(ctx, subaccountId)
        if err != nil {
            if errors.Is(err, types.ErrSubaccountNotLiquidatable) {
                continue  // 子账户可能已不再需要清算
            }
            return nil, err
        }
        liquidationOrders = append(liquidationOrders, *liquidationOrder)
    }

    // 排序清算订单：最水下的账户优先清算
    k.SortLiquidationOrders(ctx, liquidationOrders)

    // 执行清算
    for _, subaccountId := range subaccountIdsToLiquidate {
        liquidationOrder, err := k.MaybeGetLiquidationOrder(ctx, subaccountId)
        if err != nil {
            if errors.Is(err, types.ErrSubaccountNotLiquidatable) {
                continue
            }
            return nil, err
        }

        optimisticallyFilledQuantums, _, err := k.PlacePerpetualLiquidation(ctx, *liquidationOrder)
        if err != nil && !errors.Is(err, types.ErrLiquidationConflictsWithClobPairStatus) {
            return nil, err
        }

        // 如果清算订单未成交，需要进行 Deleveraging
        if optimisticallyFilledQuantums == 0 {
            subaccountsToDeleverage = append(subaccountsToDeleverage, subaccountToDeleverage{
                SubaccountId: liquidationOrder.GetSubaccountId(),
                PerpetualId:  liquidationOrder.MustGetLiquidatedPerpetualId(),
            })
        }
    }

    return subaccountsToDeleverage, nil
}
```

### 8.2 生成清算订单

```Go
// 文件: liquidations.go

// MaybeGetLiquidationOrder 接收子账户 ID 并返回用于清算该子账户的清算订单
func (k Keeper) MaybeGetLiquidationOrder(
    ctx sdk.Context,
    subaccountId satypes.SubaccountId,
) (liquidationOrder *types.LiquidationOrder, err error) {
    // 检查是否可清算
    if err := k.EnsureIsLiquidatable(ctx, subaccountId); err != nil {
        return nil, err
    }

    // 获取要清算的永续合约仓位
    perpetualId, err := k.GetPerpetualPositionToLiquidate(ctx, subaccountId)
    if err != nil {
        return nil, err
    }

    return k.GetLiquidationOrderForPerpetual(ctx, subaccountId, perpetualId)
}

// GetLiquidationOrderForPerpetual 为指定子账户和永续合约生成清算订单
func (k Keeper) GetLiquidationOrderForPerpetual(
    ctx sdk.Context,
    subaccountId satypes.SubaccountId,
    perpetualId uint32,
) (liquidationOrder *types.LiquidationOrder, err error) {
    // 获取可清算的仓位大小变化量
    deltaQuantums, err := k.GetLiquidatablePositionSizeDelta(ctx, subaccountId, perpetualId)
    if err != nil {
        return nil, err
    }

    // 获取清算订单的可成交价格（以 subticks 为单位）
    fillablePriceRat, err := k.GetFillablePrice(ctx, subaccountId, perpetualId, deltaQuantums)
    if err != nil {
        return nil, err
    }

    // 计算可成交价格
    isLiquidatingLong := deltaQuantums.Sign() == -1
    clobPair := k.mustGetClobPairForPerpetualId(ctx, perpetualId)
    fillablePriceSubticks := k.ConvertFillablePriceToSubticks(
        ctx, fillablePriceRat, isLiquidatingLong, clobPair,
    )

    // 创建清算订单
    absBaseQuantums := deltaQuantums.Abs(deltaQuantums)
    liquidationOrder = types.NewLiquidationOrder(
        subaccountId,
        clobPair,
        !isLiquidatingLong,  // 清算多头时卖出，清算空头时买入
        satypes.BaseQuantums(absBaseQuantums.Uint64()),
        fillablePriceSubticks,
    )
    return liquidationOrder, nil
}
```

代码位置: protocol/x/clob/keeper/liquidations.go:511-647

```Go
清算方向判断:

  // 多头持仓（正数）-> 需要卖出平仓 -> 清算卖单
  // 空头持仓（负数）-> 需要买入平仓 -> 清算买单
  isBuy := positionSize < 0

  清算数量计算:

  // 计算需要清算的数量（通常是全部持仓）
  liquidationQuantums := GetLiquidatablePositionSizeDelta(...)

  清算价格计算:

  // 可成交价格（Fillable Price）
  // 多头清算: 低于市场价（折价卖出）
  // 空头清算: 高于市场价（溢价买入）
  fillablePrice = (PNNV - ABR × SMMR × PMMR) / PS

  // 其中:
  // ABR = 破产调整系数 × (1 - 净抵押品/维持保证金)
  // SMMR = 价差与维持保证金比率
```

### 8.3 清算订单执行

```Go
// PlacePerpetualLiquidation 将 IOC 清算订单放入订单簿进行匹配
func (k Keeper) PlacePerpetualLiquidation(
    ctx sdk.Context,
    liquidationOrder types.LiquidationOrder,
) (
    orderSizeOptimisticallyFilledFromMatchingQuantums satypes.BaseQuantums,
    orderStatus types.OrderStatus,
    err error,
) {
    lib.AssertCheckTxMode(ctx)

    // 验证清算订单与 ClobPair 状态的兼容性
    if err := k.validateLiquidationAgainstClobPairStatus(ctx, liquidationOrder); err != nil {
        return 0, 0, err
    }

    // 在 MemClob 中执行清算订单匹配
    orderSizeOptimisticallyFilledFromMatchingQuantums, orderStatus, offchainUpdates, err :=
        k.MemClob.PlacePerpetualLiquidation(ctx, liquidationOrder)
    if err != nil {
        return 0, 0, err
    }

    // 更新子账户的清算状态
    perpetualId := liquidationOrder.MustGetLiquidatedPerpetualId()
    k.MustUpdateSubaccountPerpetualLiquidated(ctx, liquidationOrder.GetSubaccountId(), perpetualId)

    return orderSizeOptimisticallyFilledFromMatchingQuantums, orderStatus, nil
}
```

### 清算触发条件

代码位置: protocol/lib/margin/risk.go:52-54

```Go
 func (risk *Risk) IsLiquidatable() bool {
      return risk.MMR.Sign() > 0 && risk.MMR.Cmp(risk.NC) > 0
  }
  
  type Risk struct {
      MMR *big.Int  // Maintenance Margin Requirement - 维持保证金
      IMR *big.Int  // Initial Margin Requirement - 初始保证金  
      NC  *big.Int  // Net Collateral - 净抵押品
  }
```

触发条件:

- MMR > 0: 账户有持仓（维持保证金要求 > 0）
- MMR > NC: 维持保证金要求 > 净抵押品

### 清算订单的特殊性

| 特性       | 清算订单                                 | 普通订单              |
| ---------- | ---------------------------------------- | --------------------- |
| 订单类型   | 强制IOC（立即成交或取消）                | GTC/IOC/POST_ONLY     |
| 能否挂单   | ❌ 只能作为Taker                          | ✅ 可作为Maker         |
| 交易费用   | ❌ 无需支付（已付清算费）                 | ✅ 支付taker/maker费用 |
| 保险基金   | ✅ 需验证保险基金变化                     | ❌ 不涉及              |
| 区块限额   | ✅ 受限（最大名义价值、最大保险基金损失） | ❌ 无限制              |
| 抵押品检查 | Taker免检查（被清算方）只检查Maker       | Taker和Maker都检查    |
| 重复清算   | ✅ 同一区块同一永续合约只能清算一次       | ❌ 无限制              |
| Maker回扣  | ≥ 0（防止费用收集器亏损）                | 可以为负              |

### 保证金机制

```Go
 位置: protocol/x/clob/keeper/liquidations.go:1090-1213

  // 保险基金变化 = 破产价格成交价值 - 清算价格成交价值
  insuranceFundDelta = GetLiquidationInsuranceFundDelta(...)

  // 验证保险基金充足
  if insuranceFundDelta < 0 && !IsValidInsuranceFundDelta(ctx, insuranceFundDelta) {
      return ErrInsuranceFundHasInsufficientFunds
  }
```

 保险基金作用:

- 当清算价格优于破产价格时，差额进入保险基金（正收益）
- 当清算价格劣于破产价格时，保险基金补贴差额（负收益）
- 保险基金不足时，触发强制减仓（Deleveraging）机制

### 相关文件

```Bash
 protocol/
  ├── x/clob/
  │   ├── types/
  │   │   └── liquidation_order.go          # 清算订单类型定义
  │   ├── keeper/
  │   │   ├── liquidations.go:246-347       # PlacePerpetualLiquidation入口
  │   │   ├── liquidations.go:511-647       # 清算价格计算
  │   │   ├── liquidations.go:1090-1213     # 保险基金验证
  │   │   ├── process_single_match.go:43-199# 抵押品检查
  │   │   └── orders.go                     # 订单处理通用逻辑
  │   └── memclob/
  │       ├── memclob.go:698-720            # MemClob清算入口
  │       ├── memclob.go:764-870            # 订单匹配核心逻辑
  │       └── memclob_place_perpetual_liquidation_test.go  # 测试文件
  └── lib/margin/
      └── risk.go:52-54                     # 清算条件判断
```

## 九、Deleveraging（去杠杆）机制详解

**Deleveraging（去杠杆）** 是永续合约交易中的**最后防线风控机制**，用于在 Liquidation（清算）失败时强制平掉爆仓账户的剩余仓位。

术语含义Liquidated Subaccount已爆仓的子账户（权益为负或不足以维持仓位）Offsetting Subaccount被选中用来对冲爆仓账户仓位的对手方账户Bankruptcy Price爆仓账户的破产价格（账户权益归零时的价格）Oracle Price预言机报价（用于 Final Settlement）

### **触发条件：何时发生 Deleveraging？**

正常交易 → 保证金不足 → Liquidation（清算）→ [失败] → Deleveraging（去杠杆）

**Liquidation 失败的场景**

1. **流动性枯竭**：订单簿上没有足够的对手盘来吃下清算单。
2. **价格剧烈波动**：短时间内价格跳空，清算单无法以合理价格成交。
3. **系统性风险**：大量账户同时爆仓，清算引擎无法及时处理。

### **Deleveraging vs Liquidation 的区别**

对比项Liquidation（清算）Deleveraging（去杠杆）触发时机保证金率低于维持水平Liquidation 失败后成交方式订单簿市价单或限价单系统直接强制撮合成交价格订单簿实时价格Bankruptcy Price（破产价）或 Oracle Price（最终结算）对手方订单簿上的自愿买卖方系统选择的高盈利反向持仓账户费用正常交易费 + 清算罚金通常无费用（社会化损失）链上操作[MatchPerpetualLiquidation](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)[MatchPerpetualDeleveraging](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)

### 杠杆（**`Leverage`**）相关概念

#### **IMF（Initial Margin Fraction）— 初始保证金率**

- **定义**：开仓时需要的最低保证金占仓位价值的比例（以 ppm 表示，即百万分之一）。
- **作用**：决定用户的**最大杠杆倍数**。
  - 公式：`最大杠杆 = 1 / IMF`
  - 例如：IMF = 10% (100,000 ppm) → 最大 10x 杠杆

#### **Custom IMF（自定义 IMF）**

- 用户可以为每个交易对（ClobPair）设置**高于系统默认值**的 IMF。
- **降低杠杆 = 提高 IMF**，从而降低风险和爆仓概率。
- **不允许低于系统最低值**（基于流动性层级的 `InitialMarginPpm`）。

#### 数据结构

**`LeverageEntry`****（从** **`tx.pb.go`** **定义）**

```Go
type LeverageEntry struct {
    ClobPairId   uint32  // CLOB 交易对 ID
    CustomImfPpm uint32  // 用户自定义的 IMF（百万分之一）
}
```

**`MsgUpdateLeverage`**

```Go
type MsgUpdateLeverage struct {
    SubaccountId     *SubaccountId  // 要更新杠杆的子账户
    ClobPairLeverage []*LeverageEntry  // 各交易对的杠杆设置列表
}
```

**示例**：

```Go
// 用户想为 BTC-USD (ClobPairId=1) 设置 5x 杠杆
MsgUpdateLeverage{
    SubaccountId: mySubaccountId,
    ClobPairLeverage: []*LeverageEntry{
        {
            ClobPairId: 1,                  // BTC-USD
            CustomImfPpm: 200_000,          // 20% IMF = 5x 杠杆
        },
    },
}
```

### 去杠杆流程

```Plain
清算订单未能成交
        │
        ▼
┌───────────────────────────────────────────┐
│      MaybeDeleverageSubaccount()          │
├───────────────────────────────────────────┤
│                                           │
│  1. CanDeleverageSubaccount()             │
│     ├─→ TNC < 0: 按破产价格去杠杆          │
│     └─→ TNC ≥ 0 且最终结算: 按 Oracle 价格 │
│                                           │
│  2. OffsetSubaccountPerpetualPosition()   │
│     ├─→ 查找持有相反仓位的账户             │
│     ├─→ 随机起点遍历避免偏向               │
│     └─→ 强制平仓对冲                       │
│                                           │
│  3. ProcessDeleveraging()                 │
│     ├─→ 更新双方仓位                       │
│     ├─→ 转移资金                           │
│     └─→ 发送 Indexer 事件                  │
│                                           │
└───────────────────────────────────────────┘
```

### Deleveraging 入口

**文件位置:** deleveraging.go

```Go
// MaybeDeleverageSubaccount 是去杠杆子账户的主入口
// 它尝试找到持有相反方向仓位的对手方，
// 以被清算仓位的破产价格进行对冲
func (k Keeper) MaybeDeleverageSubaccount(
    ctx sdk.Context,
    subaccountId satypes.SubaccountId,
    perpetualId uint32,
) (quantumsDeleveraged *big.Int, err error) {
    lib.AssertCheckTxMode(ctx)

    // 检查是否可以执行 deleveraging
    shouldDeleverageAtBankruptcyPrice, shouldDeleverageAtOraclePrice, err := k.CanDeleverageSubaccount(
        ctx, subaccountId, perpetualId,
    )
    if err != nil {
        return new(big.Int), err
    }

    // 不需要 deleveraging 的情况
    if !shouldDeleverageAtBankruptcyPrice && !shouldDeleverageAtOraclePrice {
        return new(big.Int), nil
    }

    // 获取需要去杠杆的完整仓位
    subaccount := k.subaccountsKeeper.GetSubaccount(ctx, subaccountId)
    position, exists := subaccount.GetPerpetualPositionForId(perpetualId)
    if !exists {
        return new(big.Int), nil
    }

    // 计算需要平掉的仓位数量（全部）
    deltaQuantums := new(big.Int).Neg(position.GetBigQuantums())
    
    // 在 MemClob 中执行 deleveraging
    quantumsDeleveraged, err = k.MemClob.DeleverageSubaccount(
        ctx,
        subaccountId,
        perpetualId,
        deltaQuantums,
        shouldDeleverageAtOraclePrice,
    )

    return quantumsDeleveraged, err
}
```

### Deleveraging 条件判断

```Go
// CanDeleverageSubaccount 返回子账户是否可以被去杠杆
// 返回两个布尔值:
// - shouldDeleverageAtBankruptcyPrice: 如果子账户 TNC 为负则为 true
// - shouldDeleverageAtOraclePrice: 如果子账户 TNC 非负且市场处于最终结算状态则为 true
func (k Keeper) CanDeleverageSubaccount(
    ctx sdk.Context,
    subaccountId satypes.SubaccountId,
    perpetualId uint32,
) (shouldDeleverageAtBankruptcyPrice bool, shouldDeleverageAtOraclePrice bool, err error) {
    // 获取净抵押品和保证金要求
    risk, err := k.subaccountsKeeper.GetNetCollateralAndMarginRequirements(
        ctx, satypes.Update{SubaccountId: subaccountId},
    )
    if err != nil {
        return false, false, err
    }

    // TNC 为负，按破产价格去杠杆
    if risk.NC.Sign() == -1 {
        return true, false, nil
    }

    // TNC 非负，检查市场是否处于最终结算状态
    clobPairId, err := k.GetClobPairIdForPerpetual(ctx, perpetualId)
    if err != nil {
        return false, false, err
    }
    clobPair := k.mustGetClobPair(ctx, clobPairId)

    // 最终结算状态下按 Oracle 价格去杠杆
    return false, clobPair.Status == types.ClobPair_STATUS_FINAL_SETTLEMENT, nil
}
```

### 对冲仓位查找

```Go
// OffsetSubaccountPerpetualPosition 遍历所有子账户，
// 使用持有相反方向仓位的账户来对冲被清算子账户的仓位
func (k Keeper) OffsetSubaccountPerpetualPosition(
    ctx sdk.Context,
    liquidatedSubaccountId satypes.SubaccountId,
    perpetualId uint32,
    deltaQuantumsTotal *big.Int,
    isFinalSettlement bool,
) (
    fills []types.MatchPerpetualDeleveraging_Fill,
    deltaQuantumsRemaining *big.Int,
) {
    deltaQuantumsRemaining = new(big.Int).Set(deltaQuantumsTotal)
    fills = make([]types.MatchPerpetualDeleveraging_Fill, 0)

    // 查找持有相反方向仓位的子账户
    isDeleveragingLong := deltaQuantumsTotal.Sign() == -1
    subaccountsWithOpenPositions := k.DaemonLiquidationInfo.GetSubaccountsWithOpenPositionsOnSide(
        perpetualId,
        !isDeleveragingLong,  // 寻找相反方向
    )

    numSubaccounts := len(subaccountsWithOpenPositions)
    if numSubaccounts == 0 {
        return fills, deltaQuantumsRemaining
    }

    // 从随机位置开始遍历
    pseudoRand := k.GetPseudoRand(ctx)
    indexOffset := pseudoRand.Intn(numSubaccounts)
    numSubaccountsToIterate := lib.Min(numSubaccounts, int(k.Flags.MaxDeleveragingSubaccountsToIterate))

    for i := 0; i < numSubaccountsToIterate && deltaQuantumsRemaining.Sign() != 0; i++ {
        index := (i + indexOffset) % numSubaccounts
        subaccountId := subaccountsWithOpenPositions[index]

        offsettingSubaccount := k.subaccountsKeeper.GetSubaccount(ctx, subaccountId)
        offsettingPosition, _ := offsettingSubaccount.GetPerpetualPositionForId(perpetualId)
        bigOffsettingPositionQuantums := offsettingPosition.GetBigQuantums()

        // 跳过同方向仓位
        if deltaQuantumsRemaining.Sign() != bigOffsettingPositionQuantums.Sign() {
            continue
        }

        // 计算可对冲数量
        var deltaBaseQuantums *big.Int
        if deltaQuantumsRemaining.CmpAbs(bigOffsettingPositionQuantums) > 0 {
            deltaBaseQuantums = new(big.Int).Set(bigOffsettingPositionQuantums)
        } else {
            deltaBaseQuantums = new(big.Int).Set(deltaQuantumsRemaining)
        }

        // 计算报价数量（破产价格或 Oracle 价格）
        deltaQuoteQuantums, err := k.getDeleveragingQuoteQuantumsDelta(
            ctx, perpetualId, liquidatedSubaccountId, deltaBaseQuantums, isFinalSettlement,
        )
        if err != nil {
            continue
        }

        // 执行去杠杆
        if err := k.ProcessDeleveraging(
            ctx,
            liquidatedSubaccountId,
            *offsettingSubaccount.Id,
            perpetualId,
            deltaBaseQuantums,
            deltaQuoteQuantums,
        ); err == nil {
            deltaQuantumsRemaining.Sub(deltaQuantumsRemaining, deltaBaseQuantums)
            fills = append(fills, types.MatchPerpetualDeleveraging_Fill{
                OffsettingSubaccountId: *offsettingSubaccount.Id,
                FillAmount:             new(big.Int).Abs(deltaBaseQuantums).Uint64(),
            })

            // 发送 Indexer 事件
            k.GetIndexerEventManager().AddTxnEvent(
                ctx,
                indexerevents.SubtypeDeleveraging,
                indexerevents.DeleveragingEventVersion,
                indexer_manager.GetBytes(
                    indexerevents.NewDeleveragingEvent(
                        liquidatedSubaccountId,
                        *offsettingSubaccount.Id,
                        perpetualId,
                        satypes.BaseQuantums(new(big.Int).Abs(deltaBaseQuantums).Uint64()),
                        satypes.BaseQuantums(deltaQuoteQuantums.Uint64()),
                        deltaBaseQuantums.Sign() > 0,
                        isFinalSettlement,
                    ),
                ),
            )
        }
    }

    return fills, deltaQuantumsRemaining
}
```

## 十、订单取消流程详解

### 10.1 MsgServer 处理

**文件位置:** msg_server_cancel_orders.go

```Go
// CancelOrder 执行状态订单的取消功能
func (k msgServer) CancelOrder(
    goCtx context.Context,
    msg *types.MsgCancelOrder,
) (resp *types.MsgCancelOrderResponse, err error) {
    ctx := lib.UnwrapSDKContext(goCtx, types.ModuleName)

    if err := k.Keeper.HandleMsgCancelOrder(ctx, msg); err != nil {
        return nil, err
    }

    return &types.MsgCancelOrderResponse{}, nil
}

// HandleMsgCancelOrder 处理 MsgCancelOrder:
// 1. 在链上持久化取消
// 2. 更新 ProcessProposerMatchesEvents
// 3. 添加链上 Indexer 事件
func (k Keeper) HandleMsgCancelOrder(
    ctx sdk.Context,
    msg *types.MsgCancelOrder,
) (err error) {
    lib.AssertDeliverTxMode(ctx)  // 确保在 DeliverTx 模式

    defer func() {
        metrics.IncrSuccessOrErrorCounter(err, types.ModuleName, metrics.CancelOrder, metrics.DeliverTx)
        if err != nil {
            // 优雅处理订单已从状态中移除的情况
            if errors.Is(err, types.ErrStatefulOrderDoesNotExist) {
                processProposerMatchesEvents := k.GetProcessProposerMatchesEvents(ctx)
                removedOrderIds := lib.UniqueSliceToSet(processProposerMatchesEvents.RemovedStatefulOrderIds)
                if _, found := removedOrderIds[msg.GetOrderId()]; found {
                    err = errorsmod.Wrapf(
                        types.ErrStatefulOrderCancellationFailedForAlreadyRemovedOrder,
                        "Error: %s", err.Error(),
                    )
                    return
                }
            }
        }
    }()

    // 1. 必须是状态订单
    msg.OrderId.MustBeStatefulOrder()

    // 2. 取消订单（验证 + 从状态和 memstore 中移除）
    if err := k.CancelStatefulOrder(ctx, msg); err != nil {
        return err
    }

    // 3. 更新 memstore
    k.AddDeliveredCancelledOrderId(ctx, msg.OrderId)

    // 4. 添加 Indexer 事件
    k.GetIndexerEventManager().AddTxnEvent(
        ctx,
        indexerevents.SubtypeStatefulOrder,
        indexerevents.StatefulOrderEventVersion,
        indexer_manager.GetBytes(
            indexerevents.NewStatefulOrderRemovalEvent(
                msg.OrderId,
                indexershared.OrderRemovalReason_ORDER_REMOVAL_REASON_USER_CANCELED,
            ),
        ),
    )

    return nil
}
```

## 十一、操作队列处理流程详解

### 11.1 ProposedOperations 处理

**文件位置:** msg_server_proposed_operations.go

```Go
// ProposedOperations 是 DeliverTx 阶段处理提案者操作的入口
func (k msgServer) ProposedOperations(
    goCtx context.Context,
    msg *types.MsgProposedOperations,
) (resp *types.MsgProposedOperationsResponse, err error) {
    ctx := lib.UnwrapSDKContext(goCtx, types.ModuleName)

    defer func() {
        metrics.IncrSuccessOrErrorCounter(err, types.ModuleName, metrics.ProposedOperations, metrics.DeliverTx)
    }()

    // 处理提案者的操作队列
    if err := k.Keeper.ProcessProposerOperations(ctx, msg.GetOperationsQueue()); err != nil {
        return nil, err
    }

    return &types.MsgProposedOperationsResponse{}, nil
}
```

### 11.2 ProcessProposerOperations 核心逻辑

**文件位置:** process_operations.go

```Go
// ProcessProposerOperations 处理提案者提交的操作队列
func (k Keeper) ProcessProposerOperations(
    ctx sdk.Context,
    rawOperations []types.OperationRaw,
) error {
    // 1. 验证并转换原始操作
    operations, err := types.ValidateAndTransformRawOperations(
        ctx, rawOperations, k.txDecoder, k.antehandler,
    )
    if err != nil {
        return err
    }

    // 2. 处理内部操作
    if err := k.ProcessInternalOperations(ctx, operations); err != nil {
        return err
    }

    // 3. 生成处理提案者匹配事件
    processProposerMatchesEvents := k.GenerateProcessProposerMatchesEvents(ctx, operations)

    // 4. 移除完全成交的订单，更新 memstore
    // ...

    return nil
}
```

### 11.3 匹配订单持久化

```Go
// PersistMatchOrdersToState 将 MatchOrders 对象写入状态
// 并为匹配发出链上 Indexer 事件
func (k Keeper) PersistMatchOrdersToState(
    ctx sdk.Context,
    matchOrders *types.MatchOrders,
    ordersMap map[types.OrderId]types.Order,
    affiliateOverrides map[string]bool,
    affiliateParameters affiliatetypes.AffiliateParameters,
) error {
    takerOrderId := matchOrders.GetTakerOrderId()
    
    // 从短期订单或状态中获取 Taker 订单
    takerOrder, err := k.FetchOrderFromOrderId(ctx, takerOrderId, ordersMap)
    if err != nil {
        return err
    }

    // Taker 订单不能是 Post-Only
    if takerOrder.GetTimeInForce() == types.Order_TIME_IN_FORCE_POST_ONLY {
        return errorsmod.Wrapf(types.ErrInvalidMatchOrder,
            "Taker order %+v cannot be post only.", takerOrder.GetOrderTextString())
    }

    // 需要立即执行的订单不能已有成交
    if takerOrder.RequiresImmediateExecution() {
        _, fillAmount, _ := k.GetOrderFillAmount(ctx, takerOrder.OrderId)
        if fillAmount != 0 {
            return errorsmod.Wrapf(types.ErrImmediateExecutionOrderAlreadyFilled,
                "Order %s", takerOrder.GetOrderTextString())
        }
    }

    // 处理每个 Maker 成交
    makerOrders := make([]types.Order, 0)
    makerFills := matchOrders.GetFills()
    for _, makerFill := range makerFills {
        makerOrder, err := k.FetchOrderFromOrderId(ctx, makerFill.MakerOrderId, ordersMap)
        if err != nil {
            return err
        }

        matchWithOrders := types.MatchWithOrders{
            TakerOrder: &takerOrder,
            MakerOrder: &makerOrder,
            FillAmount: satypes.BaseQuantums(makerFill.GetFillAmount()),
        }
        makerOrders = append(makerOrders, makerOrder)

        // 处理单笔匹配（计算费用、更新仓位等）
        _, _, _, affiliateRevSharesQuoteQuantums, err := k.ProcessSingleMatch(
            ctx, &matchWithOrders, affiliateOverrides, affiliateParameters,
        )
        if err != nil {
            return err
        }

        // 获取成交后的总成交量
        makerExists, totalFilledMaker, _ := k.GetOrderFillAmount(ctx, makerOrder.OrderId)
        takerExists, totalFilledTaker, _ := k.GetOrderFillAmount(ctx, takerOrder.OrderId)

        // 发送 Indexer 事件
        k.GetIndexerEventManager().AddTxnEvent(
            ctx,
            indexerevents.SubtypeOrderFill,
            indexerevents.OrderFillEventVersion,
            indexer_manager.GetBytes(
                indexerevents.NewOrderFillEvent(
                    makerOrder, takerOrder,
                    matchWithOrders.FillAmount,
                    matchWithOrders.MakerFee,
                    matchWithOrders.TakerFee,
                    matchWithOrders.MakerBuilderFee,
                    matchWithOrders.TakerBuilderFee,
                    totalFilledMaker,
                    totalFilledTaker,
                    affiliateRevSharesQuoteQuantums,
                    matchWithOrders.MakerOrderRouterFee,
                    matchWithOrders.TakerOrderRouterFee,
                ),
            ),
        )
    }

    // GRPC 流推送
    if streamingManager := k.GetFullNodeStreamingManager(); streamingManager.Enabled() {
        streamOrderbookFill := k.MemClob.GenerateStreamOrderbookFill(
            ctx,
            types.ClobMatch{Match: &types.ClobMatch_MatchOrders{MatchOrders: matchOrders}},
            &takerOrder, makerOrders,
        )
        k.GetFullNodeStreamingManager().SendOrderbookFillUpdate(
            streamOrderbookFill, ctx, k.PerpetualIdToClobPairId,
        )
    }

    return nil
}
```

## 十二、订单移除处理

### 12.1 订单移除原因枚举

```JavaScript
// 文件: types/order_removals.go

const (
    // 抵押品不足
    OrderRemoval_REMOVAL_REASON_UNDERCOLLATERALIZED
    // Post-Only 订单会与 Maker 订单成交
    OrderRemoval_REMOVAL_REASON_POST_ONLY_WOULD_CROSS_MAKER_ORDER
    // 无效的 Reduce-Only 订单
    OrderRemoval_REMOVAL_REASON_INVALID_REDUCE_ONLY
    // 无效的自成交
    OrderRemoval_REMOVAL_REASON_INVALID_SELF_TRADE
    // 条件单 FOK 无法完全成交
    OrderRemoval_REMOVAL_REASON_CONDITIONAL_FOK_COULD_NOT_BE_FULLY_FILLED
    // 条件单 IOC 会挂单
    OrderRemoval_REMOVAL_REASON_CONDITIONAL_IOC_WOULD_REST_ON_BOOK
    // 完全成交
    OrderRemoval_REMOVAL_REASON_FULLY_FILLED
    // 违反隔离子账户约束
    OrderRemoval_REMOVAL_REASON_VIOLATES_ISOLATED_SUBACCOUNT_CONSTRAINTS
)
```

### 12.2 订单移除持久化

```Go
// 文件: process_operations.go

// PersistOrderRemovalToState 处理订单移除
func (k Keeper) PersistOrderRemovalToState(
    ctx sdk.Context,
    orderRemoval types.OrderRemoval,
) error {
    orderIdToRemove := orderRemoval.GetOrderId()

    // 获取要移除的订单
    orderToRemove, found := k.GetLongTermOrderPlacement(ctx, orderIdToRemove)
    if !found {
        // 尝试从触发的条件订单中获取
        orderToRemove, found = k.GetTriggeredConditionalOrderPlacement(ctx, orderIdToRemove)
        if !found {
            return errorsmod.Wrapf(types.ErrStatefulOrderDoesNotExist,
                "Order Id %+v", orderRemoval.GetOrderId())
        }
    }

    // 根据移除原因进行验证
    switch orderRemoval.GetRemovalReason() {
    case types.OrderRemoval_REMOVAL_REASON_UNDERCOLLATERALIZED:
        // 抵押品不足 - 直接移除
        k.statUnverifiedOrderRemoval(ctx, orderRemoval)

    case types.OrderRemoval_REMOVAL_REASON_POST_ONLY_WOULD_CROSS_MAKER_ORDER:
        // Post-Only 验证
        k.statUnverifiedOrderRemoval(ctx, orderRemoval)
        if orderToRemove.TimeInForce != types.Order_TIME_IN_FORCE_POST_ONLY {
            return errorsmod.Wrap(types.ErrUnexpectedTimeInForce, "Order is not post-only")
        }

    case types.OrderRemoval_REMOVAL_REASON_INVALID_REDUCE_ONLY:
        // Reduce-Only 验证
        if !orderToRemove.IsReduceOnly() {
            return errorsmod.Wrapf(types.ErrInvalidOrderRemoval,
                "Order Removal (%+v) invalid. Order must be reduce only.", orderRemoval)
        }
        // 检查订单成交是否会增加仓位或改变方向
        currentPositionSize := k.GetStatePosition(ctx, orderIdToRemove.SubaccountId, orderToRemove.GetClobPairId())
        if currentPositionSize.Sign() != 0 {
            orderQuantumsToFill := orderToRemove.GetBigQuantums()
            orderFillWouldIncreasePositionSize := orderQuantumsToFill.Sign() == currentPositionSize.Sign()
            newPositionSize := new(big.Int).Add(currentPositionSize, orderQuantumsToFill)
            orderChangedSide := currentPositionSize.Sign()*newPositionSize.Sign() == -1
            if !orderFillWouldIncreasePositionSize && !orderChangedSide {
                return errorsmod.Wrapf(types.ErrInvalidOrderRemoval,
                    "Order fill must increase position size or change side.")
            }
        }

    case types.OrderRemoval_REMOVAL_REASON_FULLY_FILLED:
        // 完全成交不应该在操作队列中
        return errorsmod.Wrapf(types.ErrInvalidOrderRemovalReason,
            "Order removal reason fully filled should not be part of the operations queue.")
    }

    // 从状态中移除订单
    k.MustRemoveStatefulOrder(ctx, orderIdToRemove)

    // 发送 Indexer 事件
    k.GetIndexerEventManager().AddTxnEvent(
        ctx,
        indexerevents.SubtypeStatefulOrder,
        indexerevents.StatefulOrderEventVersion,
        indexer_manager.GetBytes(
            indexerevents.NewStatefulOrderRemovalEvent(
                orderIdToRemove,
                indexershared.ConvertOrderRemovalReasonToIndexerOrderRemovalReason(orderRemoval.RemovalReason),
            ),
        ),
    )

    return nil
}
```

## 十三、模块完整执行流程总结

### 13.1 完整订单生命周期

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=NzM3ZTgyZWY4MjI0MDVkNmI4OTdkZTY2NDIzMjUyNGVfYnV0NEs4amlyeUJyOGg5SnBtNDNSbEtUemFBTWVpZFFfVG9rZW46UWhHdWJCaHlGb1RkQ254cUF4OGwzWmw3Z0hiXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

```Plain
┌─────────────────────────────────────────────────────────────────────────────┐
│                           订单完整生命周期                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  1. 用户提交订单 (MsgPlaceOrder)                                             │
│         │                                                                   │
│         ▼                                                                   │
│  ┌─────────────────┐                                                        │
│  │   AnteHandler   │  ClobDecorator.AnteHandle()                           │
│  │   (CheckTx)     │  - 验证订单格式                                         │
│  │                 │  - 短期订单: PlaceShortTermOrder() → MemClob 匹配       │
│  │                 │  - 状态订单: PlaceStatefulOrder() → 写入临时存储         │
│  └────────┬────────┘                                                        │
│           │                                                                 │
│           ▼                                                                 │
│  ┌─────────────────┐                                                        │
│  │    MemClob      │  PlaceOrder()                                         │
│  │   订单匹配       │  - validateNewOrder() 验证                             │
│  │                 │  - matchOrder() 撮合                                   │
│  │                 │  - mustAddOrderToOrderbook() 挂单                      │
│  │                 │  - 生成 OperationsToPropose                            │
│  └────────┬────────┘                                                        │
│           │                                                                 │
│           ▼                                                                 │
│  ┌─────────────────┐                                                        │
│  │  PrepareProposal│  GetOperationsRaw()                                   │
│  │   区块构建       │  - 获取操作队列                                         │
│  │                 │  - 构建 MsgProposedOperations                          │
│  └────────┬────────┘                                                        │
│           │                                                                 │
│           ▼                                                                 │
│  ┌─────────────────┐                                                        │
│  │   DeliverTx     │  HandleMsgPlaceOrder() / ProposedOperations()         │
│  │   状态持久化     │  - ProcessProposerOperations()                         │
│  │                 │  - PersistMatchOrdersToState()                        │
│  │                 │  - 更新子账户仓位和余额                                  │
│  │                 │  - 发送 Indexer 事件                                   │
│  └────────┬────────┘                                                        │
│           │                                                                 │
│           ▼                                                                 │
│  ┌─────────────────┐                                                        │
│  │   EndBlocker    │  EndBlocker()                                         │
│  │                 │  - 处理过期订单                                         │
│  │                 │  - 触发条件订单                                         │
│  └────────┬────────┘                                                        │
│           │                                                                 │
│           ▼                                                                 │
│  ┌─────────────────┐                                                        │
│  │ PrepareCheckState│ PrepareCheckState()                                  │
│  │   状态准备       │  - 重放订单到 MemClob                                   │
│  │                 │  - 执行清算                                            │
│  │                 │  - 执行 Deleveraging                                   │
│  │                 │  - 清理过期订单                                         │
│  └─────────────────┘                                                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 13.2 关键数据流

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=Y2YzZTVmZWY2ZDgxMWM0YzAyYmE3YjdiMmQ4NjNjZWRfU09oMTdmZFpOUFBSdWtlWFM1ZFJtVzJMZkQzZkxQUjdfVG9rZW46RUN3RWJKWjFZb3NGSVZ4eE1sOWxxSlFpZ0VlXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

```Plain
┌─────────────────────────────────────────────────────────────────────────────┐
│                              数据流向图                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  输入层                                                                      │
│  ┌──────────┐ ┌──────────────┐ ┌─────────────┐                             │
│  │MsgPlace  │ │MsgCancel     │ │Liquidation  │                             │
│  │Order     │ │Order         │ │Daemon       │                             │
│  └────┬─────┘ └──────┬───────┘ └──────┬──────┘                             │
│       │              │                │                                     │
│       ▼              ▼                ▼                                     │
│  ┌─────────────────────────────────────────────────────────────────┐       │
│  │                       CLOB Keeper                               │       │
│  │  ┌─────────────────────────────────────────────────────────┐   │       │
│  │  │                    MemClob                               │   │       │
│  │  │  ┌────────────┐ ┌────────────────────┐ ┌────────────┐   │   │       │
│  │  │  │ Orderbook  │ │OperationsToPropose │ │OffchainUpd │   │   │       │
│  │  │  │ (内存)      │ │   (操作队列)        │ │ (Indexer)  │   │   │       │
│  │  │  └────────────┘ └────────────────────┘ └────────────┘   │   │       │
│  │  └─────────────────────────────────────────────────────────┘   │       │
│  │                                                                 │       │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────┐                  │       │
│  │  │ Stateful   │ │ Fill       │ │ Liquidation│                  │       │
│  │  │ Orders     │ │ Amounts    │ │ State      │                  │       │
│  │  │ (链上状态)  │ │ (链上状态)  │ │ (链上状态)  │                  │       │
│  │  └────────────┘ └────────────┘ └────────────┘                  │       │
│  └─────────────────────────────────────────────────────────────────┘       │
│       │              │                │                                     │
│       ▼              ▼                ▼                                     │
│  输出层                                                                      │
│  ┌──────────┐ ┌──────────────┐ ┌─────────────┐ ┌─────────────┐             │
│  │Subaccount│ │Indexer       │ │Full Node    │ │Insurance    │             │
│  │Updates   │ │Events        │ │Streaming    │ │Fund         │             │
│  └──────────┘ └──────────────┘ └─────────────┘ └─────────────┘             │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## 十四、核心设计总结

14.1 混合存储模式

存储类型数据内容生命周期用途MemClob (内存)订单簿、短期订单单个区块高性能撮合TransientStoreProcessProposerMatchesEvents单个区块跨回调状态传递KVStore (链上)状态订单、成交量、ClobPair持久化共识确认的状态

### 14.2 订单类型处理差异

订单类型存储位置Gas 消耗过期机制主要用途Short-Term仅内存零 GasGoodTilBlock高频交易Long-Term链上状态标准 GasGoodTilBlockTime大额订单Conditional链上状态标准 Gas触发后转换止损/止盈TWAP链上状态标准 Gas分批执行大额均价成交

### 14.3 安全机制

1. **抵押品检查**: 每笔匹配前后都进行抵押品验证
2. **清算机制**: 及时清算保证金不足的账户
3. **Deleveraging**: 当清算无法完成时的最后防线
4. **负 TNC 提款门控**: 发现负 TNC 账户时暂停提款

### 数据存储架构

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=YWNiNGQ5YThiOTUxMzMwNmMyNmE4NGJjMjA5ODcyNzNfeFRTM3FCdHhaZmxVbmV1UFVQUXZZOFdLNFVJNXJmT1VfVG9rZW46WWkzaWJqVUQxb3VYUlR4VlBTcWx0SHZvZ3lmXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

### 两阶段订单处理

```Plain
CheckTx 阶段                    DeliverTx 阶段
┌─────────────────┐            ┌─────────────────┐
│ • 订单验证       │                                │ • 状态持久化     │
│ • 内存撮合       │                 →              │ • 仓位更新       │
│ • 生成操作队列   │                                │ • 事件发送       │
└─────────────────┘            └─────────────────┘
```

## 十五、rate_limit 详解

rate_limit 模块用于**在 CheckTx 阶段对 CLOB 操作进行速率限制**，防止账户滥用网络资源，抵御 DDoS 攻击。

```Go
┌────────────────────────────────────────────────────────────┐
│                   Rate Limit 架构                           │
├────────────────────────────────────────────────────────────┤
│                                                            │
│  RateLimiter[K any] (泛型接口)                             │
│  ├── RateLimit(ctx, key) error                            │
│  ├── RateLimitIncrBy(ctx, key, incrBy) error              │
│  └── PruneRateLimits(ctx)                                 │
│                                                            │
│  实现类:                                                    │
│  ├── NoOpRateLimiter           (空操作，不限制)             │
│  ├── SingleBlockRateLimiter    (单区块限制)                │
│  ├── MultiBlockRateLimiter     (多区块滑动窗口限制)         │
│  └── PanicRateLimiter          (测试用，调用即 panic)       │
│                                                            │
│  顶层封装:                                                  │
│  ├── placeAndCancelOrderRateLimiter                       │
│  │   ├── checkStateShortTermOrderPlaceCancelRateLimiter   │
│  │   └── checkStateStatefulOrderRateLimiter               │
│  └── updateLeverageRateLimiter                            │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

限制类型：

限制类型字段名说明短期订单下单+取消MaxShortTermOrdersAndCancelsPerNBlocks限制短期订单的下单和取消操作总数长期订单下单MaxStatefulOrdersPerNBlocks限制长期订单（Long-Term/Conditional）下单次数杠杆更新MaxLeverageUpdatesPerNBlocks限制账户杠杆调整次数

## 十六、CLOB 模块代码执行流程梳理

### 模块初始化流程

#### 1.1 模块注册

**文件:** module.go

```Go
// AppModule 实现 Cosmos SDK 的 AppModule 接口
type AppModule struct {
    keeper        *keeper.Keeper
    accountKeeper types.AccountKeeper
    bankKeeper    types.BankKeeper
    // ...
}

// RegisterServices 注册 gRPC 服务
func (am AppModule) RegisterServices(cfg module.Configurator) {
    types.RegisterMsgServer(cfg.MsgServer(), keeper.NewMsgServerImpl(am.keeper))
    types.RegisterQueryServer(cfg.QueryServer(), am.keeper)
}
```

#### 1.2 Keeper 初始化

**文件:** keeper.go

```Go
// NewKeeper 创建 CLOB Keeper
func NewKeeper(
    cdc codec.BinaryCodec,
    storeKey storetypes.StoreKey,
    memKey storetypes.StoreKey,
    // ... 其他依赖
) *Keeper {
    return &Keeper{
        cdc:                 cdc,
        storeKey:            storeKey,
        memKey:              memKey,
        MemClob:             memclob.NewMemClobPriceTimePriority(...),
        // ...
    }
}
```

### 订单下单流程

#### 2.1 交易入口 - AnteHandler

**文件:** clob.go

```Go
用户提交交易
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ ClobDecorator.AnteHandle()                          │
│                                                     │
│ • 只在 CheckTx 模式下处理订单                         │
│ • DeliverTx 模式直接跳过                             │
└─────────────────────────────────────────────────────┘
      │
      ├─→ MsgPlaceOrder (短期订单)
      │        │
      │        ▼
      │   keeper.PlaceShortTermOrder()
      │        │
      │        ▼
      │   [keeper/orders.go]
      │
      ├─→ MsgPlaceOrder (状态订单)
      │        │
      │        ▼
      │   keeper.PlaceStatefulOrder()
      │        │
      │        ▼
      │   [keeper/orders.go]
      │
      └─→ MsgCancelOrder
               │
               ▼
          keeper.CancelShortTermOrder() 或
          keeper.CancelStatefulOrder()
// app/app.go
// setAnteHandler creates a new AnteHandler and sets it on the base app and clob keeper.
func (app *App) setAnteHandler(txConfig client.TxConfig) {
    anteHandler := app.buildAnteHandler(txConfig)
    // Prevent a cycle between when we create the clob keeper and the ante handler.
    app.ClobKeeper.SetAnteHandler(anteHandler)
    app.SetAnteHandler(anteHandler)
}

// app/ante.go
func (h *lockingAnteHandler) AnteHandle(ctx sdk.Context, tx sdk.Tx, simulate bool) (sdk.Context, error)
func (h *lockingAnteHandler) clobAnteHandle(ctx sdk.Context, tx sdk.Tx, simulate bool)

//  clob/ante/clob.go
func (cd ClobDecorator) AnteHandle(
    ctx sdk.Context,
    tx sdk.Tx,
    simulate bool,
    next sdk.AnteHandler,
) (sdk.Context, error){
    // No need to process during `DeliverTx` or simulation, call next `AnteHandler`.
    // 只在 CheckTx 模式下处理订单  DeliverTx 模式直接跳过  
    if lib.IsDeliverTxMode(ctx) || simulate {
        return next(ctx, tx, simulate)
    }
    
    // ……
}
```

#### 2.2 短期订单处理

**文件:** orders.go

```Go
PlaceShortTermOrder()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 1. 验证订单                                          │
│    - validateOrderAgainstClobPairStatus()           │
│    - 检查 Equity Tier 限制                           │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 2. MemClob.PlaceOrder()                             │
│    [memclob/memclob.go]                             │
└─────────────────────────────────────────────────────┘
```

#### 2.3 MemClob 撮合

**文件:** memclob.go

```Go
MemClob.PlaceOrder()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ validateNewOrder()                                  │
│ • 验证订单格式                                       │
│ • 检查重复订单                                       │
│ • 检查过期时间                                       │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ matchOrder()                                        │
│ [memclob/memclob.go - mustPerformTakerOrderMatching]│
│                                                     │
│ • 获取对手方最优价格                                  │
│ • 价格-时间优先匹配                                  │
│ • 抵押品检查 (PerformStatefulOrderValidation)       │
│ • 生成成交记录                                       │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 订单未完全成交?                                      │
│ • IOC/FOK: 取消剩余                                 │
│ • 其他: mustAddOrderToOrderbook()                   │
│         [memclob/memclob.go]                        │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 更新 OperationsToPropose                            │
│ [types/operations_to_propose.go]                    │
└─────────────────────────────────────────────────────┘
```

matchOrder 时序图

```Go
┌─────────────────────────────────────────────────────────────────────────────┐
│                         matchOrder 调用时序                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Caller (PlaceOrder / PlacePerpetualLiquidation)                           │
│      │                                                                      │
│      │  order (taker)                                                       │
│      ▼                                                                      │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │                      matchOrder()                                  │    │
│  ├────────────────────────────────────────────────────────────────────┤    │
│  │                                                                    │    │
│  │  1. ctx.CacheContext() ──────────────► branchedContext, writeCache │    │
│  │                                                                    │    │
│  │  2. mustPerformTakerOrderMatching(branchedContext, order)          │    │
│  │         │                                                          │    │
│  │         └──► newMakerFills                                         │    │
│  │              matchedOrderHashToOrder                               │    │
│  │              matchedMakerOrderIdToOrder                            │    │
│  │              makerOrdersToRemove                                   │    │
│  │              takerOrderStatus                                      │    │
│  │                                                                    │    │
│  │  3. [可选] SendTakerOrderStatus() ← 流推送                          │    │
│  │                                                                    │    │
│  │  4. 替换检查: 若同ID订单存在 → 加入 makerOrdersToRemove              │    │
│  │                                                                    │    │
│  │  5. Loop makerOrdersToRemove:                                      │    │
│  │         mustRemoveOrder(branchedContext, makerOrderId)             │    │
│  │         [stateful] → operationsToPropose.MustAddOrderRemoval...    │    │
│  │                                                                    │    │
│  │  6. 判断 matchingErr:                                               │    │
│  │         PostOnly冲突 → ErrPostOnlyWouldCrossMakerOrder             │    │
│  │         隔离约束违反 → ErrWouldViolateIsolatedSubaccountConstraints │    │
│  │                                                                    │    │
│  │  7. takerGeneratedValidMatches = (fills > 0) && (err == nil)       │    │
│  │         │                                                          │    │
│  │         ├─ true ─────────────────────────────────────────────┐     │    │
│  │         │   mustUpdateMemclobStateWithMatches()              │     │    │
│  │         │       └─► 更新 operationsToPropose                 │     │    │
│  │         │       └─► 生成 offchainUpdates                     │     │    │
│  │         │   writeCache() ✅ 提交分叉                          │     │    │
│  │         │                                                    │     │    │
│  │         └─ false ────────────────────────────────────────────┘     │    │
│  │             发送 reset updates (流一致性)                           │    │
│  │             分叉自动丢弃 ❌                                         │    │
│  │                                                                    │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│      │                                                                      │
│      │  return: takerOrderStatus, offchainUpdates,                         │
│      │          makerOrdersToRemove, matchingErr                           │
│      ▼                                                                      │
│  Caller                                                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### DeliverTx 阶段处理

#### 状态订单下单

**文件:** msg_server_place_order.go

```Go
MsgServer.PlaceOrder()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ HandleMsgPlaceOrder()                               │
│                                                     │
│ • 只处理状态订单 (Long-Term, Conditional)            │
│ • 写入链上状态                                       │
│ • 发送 Indexer 事件                                  │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ PlaceStatefulOrder()                                │
│ [keeper/orders.go]                                  │
│                                                     │
│ • SetLongTermOrderPlacement() 或                    │
│ • SetUntriggeredConditionalOrderPlacement()        │
└─────────────────────────────────────────────────────┘
```

#### 提案操作处理

**文件:** msg_server_proposed_operations.go

```Go
MsgServer.ProposedOperations()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ ProcessProposerOperations()                         │
│ [keeper/process_operations.go]                      │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ ValidateAndTransformRawOperations()                 │
│ [types/operations.go]                               │
│                                                     │
│ 解析操作类型:                                        │
│ • ShortTermOrderPlacement                           │
│ • MatchOrders                                       │
│ • MatchPerpetualLiquidation                         │
│ • MatchPerpetualDeleveraging                        │
│ • OrderRemoval                                      │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ ProcessInternalOperations()                         │
│ [keeper/process_operations.go]                      │
└─────────────────────────────────────────────────────┘
```

#### 匹配持久化

**文件:** process_operations.go

```Go
ProcessInternalOperations()
      │
      ├─→ Match Orders
      │        │
      │        ▼
      │   PersistMatchOrdersToState()
      │        │
      │        ▼
      │   ProcessSingleMatch()
      │   [keeper/match_state.go]
      │        │
      │        ▼
      │   • 更新订单成交量
      │   • 更新子账户仓位
      │   • 计算费用
      │   • 发送 Indexer 事件
      │
      ├─→ Liquidation Match
      │        │
      │        ▼
      │   PersistMatchLiquidationToState()
      │   [keeper/process_operations.go]
      │
      ├─→ Deleveraging Match
      │        │
      │        ▼
      │   PersistMatchDeleveragingToState()
      │   [keeper/process_operations.go]
      │
      └─→ Order Removal
               │
               ▼
          PersistOrderRemovalToState()
          [keeper/process_operations.go]
```

### ABCI 生命周期

#### PreBlocker

**文件:** abci.go

```Go
PreBlocker()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ MaybeTriggerConditionalOrders()                     │
│ [keeper/conditional_orders.go]                      │
│                                                     │
│ • 获取最新 Oracle 价格                               │
│ • 检查条件订单触发条件                               │
│ • 触发满足条件的订单                                 │
└─────────────────────────────────────────────────────┘
```

#### BeginBlocker

**文件:** abci.go

```Go
BeginBlocker()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 初始化区块状态                                       │
│ • 重置区块内计数器                                   │
│ • 初始化 ProcessProposerMatchesEvents               │
└─────────────────────────────────────────────────────┘
```

#### EndBlocker

**文件:** abci.go

```Go
EndBlocker()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 1. PruneExpiredStatefulOrders()                     │
│    [keeper/stateful_orders.go]                      │
│    • 移除过期的长期订单                              │
│    • 移除过期的条件订单                              │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 2. ProcessTwapOrders()                              │
│    [keeper/twap.go]                                 │
│    • 生成 TWAP 子订单                               │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 3. MaybeTriggerConditionalOrders()                  │
│    [keeper/conditional_orders.go]                   │
│    • 区块结束时再次检查触发                          │
└─────────────────────────────────────────────────────┘
```

#### PrepareCheckState

**文件:** abci.go

```Go
PrepareCheckState()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 1. 清理 MemClob                                     │
│    MemClob.ClearOperationsQueue()                   │
│    [memclob/memclob.go]                             │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 2. 重放订单 (两遍)                                   │
│    ReplayPlaceOrdersAndCancelOrders()               │
│    [keeper/replay.go]                               │
│                                                     │
│    第一遍: Post-Only 订单                            │
│    第二遍: 所有订单                                  │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 3. 执行清算                                          │
│    LiquidateSubaccountsAgainstOrderbook()           │
│    [keeper/liquidations.go]                         │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 4. 执行去杠杆                                        │
│    MaybeDeleverageSubaccount()                      │
│    [keeper/deleveraging.go]                         │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 5. 处理负 TNC 提款门控                               │
│    GateWithdrawalsIfNegativeTncSubaccountSeen()     │
│    [keeper/deleveraging.go]                         │
└─────────────────────────────────────────────────────┘
```

### 清算流程

#### 清算执行

**文件:** liquidations.go

```Go
LiquidateSubaccountsAgainstOrderbook()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 1. MaybeGetLiquidationOrder()                       │
│    • EnsureIsLiquidatable() - 检查是否可清算         │
│    • GetPerpetualPositionToLiquidate() - 获取仓位   │
│    • GetLiquidationOrderForPerpetual() - 生成订单   │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 2. SortLiquidationOrders()                          │
│    • 最水下账户优先                                  │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 3. PlacePerpetualLiquidation()                      │
│    │                                                │
│    ▼                                                │
│    MemClob.PlacePerpetualLiquidation()              │
│    [memclob/memclob.go]                             │
└─────────────────────────────────────────────────────┘
```

### 去杠杆流程

#### 去杠杆执行

**文件:** deleveraging.go

```Go
MaybeDeleverageSubaccount()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 1. CanDeleverageSubaccount()                        │
│    • TNC < 0: 按破产价格                            │
│    • TNC ≥ 0 且最终结算: 按 Oracle 价格             │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 2. MemClob.DeleverageSubaccount()                   │
│    [memclob/memclob.go]                             │
│         │                                           │
│         ▼                                           │
│    OffsetSubaccountPerpetualPosition()              │
│    [keeper/deleveraging.go]                         │
│    • 查找对手方仓位                                  │
│    • 计算对冲数量                                   │
│    • 执行强制平仓                                   │
└─────────────────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 3. ProcessDeleveraging()                            │
│    [keeper/deleveraging.go]                         │
│    • 更新双方仓位                                   │
│    • 发送 Indexer 事件                              │
└─────────────────────────────────────────────────────┘
```

### 订单取消流程

#### 取消订单

**文件:** msg_server_cancel_orders.go

```Go
MsgServer.CancelOrder()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ HandleMsgCancelOrder()                              │
│                                                     │
│ • 验证订单必须是状态订单                             │
│ • CancelStatefulOrder()                             │
│ • AddDeliveredCancelledOrderId()                    │
│ • 发送 Indexer 事件                                  │
└─────────────────────────────────────────────────────┘
```

**文件:** msg_server_batch_cancel.go

```Go
MsgServer.BatchCancel()
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ 批量取消短期订单                                     │
│ • 遍历订单列表                                       │
│ • MemClob.CancelOrder()                             │
└─────────────────────────────────────────────────────┘
```

### 关键数据类型

#### 订单相关

**文件:** order.go **文件:** order_id.go

#### 操作队列

**文件:** operations_to_propose.go

#### 匹配类型

**文件:** matches.go

#### 清算订单

**文件:** liquidation_order.go

## FAQ

### DeliverTx 和 CheckTx 在Clob模块 执行入口和流程 都做了些什么？

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=NDY0YzQ1NmNkNDA0MmJkYjUzZTMyNDM5MzFkODUzYTZfbDNhMk5pVlFYMmNJRzVvQ290ZzN1SmZ5VXRCSU9aTVZfVG9rZW46TnY2bGJUT2F5b3V2aGN4SjJ5N2xhM2lLZ3poXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

在 dYdX v4-chain 的 CLOB 模块中，CheckTx 和 DeliverTx 扮演着完全不同的角色：

阶段职责状态持久化CheckTx内存撮合 + 验证仅内存操作DeliverTx状态持久化 + 共识确认写入链上

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=MmVkMzM3ODZjNzY3OTU1Njg2YzZjYTMyNzBhNGMyYWFfMElyQlcwSXRxaURqeTU2R1ppNGpac3lqRG5xbmJVRXlfVG9rZW46UmxhZGJ2cFFSb3Axemh4Y0hsWmx6NDVnZ21oXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

关键文件位置总结

阶段文件函数CheckTx 入口[clob.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)ClobDecorator.AnteHandle()短期订单撮合[orders.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)PlaceShortTermOrder()短期订单取消[orders.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)CancelShortTermOrder()有状态订单验证[keeper/stateful_order_state.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)PlaceStatefulOrder()DeliverTx 订单入口[msg_server_place_order.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)PlaceOrder() / HandleMsgPlaceOrder()DeliverTx 操作队列[msg_server_proposed_operations.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)ProposedOperations()操作队列处理[process_operations.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)ProcessProposerOperations() / ProcessInternalOperations()DeliverTx 取消入口[msg_server_cancel_orders.go](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)CancelOrder() / HandleMsgCancelOrder()

### **ReCheckTx 详解**

`ReCheckTx` 是 Cosmos SDK 的一种特殊执行模式，用于在 **mempool 中已存在的交易** 需要被重新验证时执行。

**三种 Tx 模式对比：**

```Go
CheckTx     → 新交易首次进入 mempool 时验证
ReCheckTx   → mempool 中的交易被重新验证
DeliverTx   → 交易被打包进区块后执行
```

**ReCheckTx 何时执行？**

**触发时机：**

**场景1: 新区块提交后**

```Go
区块 N 提交完成
  ↓
Mempool 中剩余的未打包交易需要重新验证
  ↓
对每个 mempool 中的交易执行 ReCheckTx
  ↓
检查交易在新区块状态下是否仍然有效
```

**为什么需要？**

- 区块 N 的状态变更可能影响 mempool 中交易的有效性
- 例如：账户余额不足、nonce 过期、订单已成交等

**场景2: Mempool 清理**

```Go
Mempool 接近容量上限
  ↓
触发 Mempool Recheck
  ↓
清除无效交易，为新交易腾出空间
```

**执行的操作：**

订单类型CheckTxReCheckTx原因短期订单 (ShortTerm)✅ 处理⛔ 跳过已在 MemClob 中，无需重复处理长期订单 (LongTerm)✅ 处理✅ 处理需验证状态一致性条件订单 (Conditional)✅ 处理✅ 处理需验证状态一致性TWAP订单 (TWAP)✅ 处理✅ 处理需验证状态一致性批量取消 (BatchCancel)✅ 处理⛔ 跳过仅处理短期订单

**为什么短期订单跳过 ReCheckTx？**

```Go
// 原因分析
短期订单特性:
  1. 仅存在于 MemClob (内存中)
  2. 不写入 KVStore (不持久化)
  3. 生命周期短暂 (单个区块)
  4. 通过 OperationsQueue 共识

ReCheckTx 跳过短期订单的理由:
  ✓ 已经在 MemClob 中存在
  ✓ 新区块提交后会自动清理过期订单
  ✓ 重复处理会导致状态不一致
  ✓ 避免重复计入限速计数器
```

**为什么有状态订单需要 ReCheckTx？**

```Go
// 有状态订单在 ReCheckTx 时需要重新验证
PlaceStatefulOrder(ctx, msg, false) {
    // 1. 验证订单参数
    PerformStatefulOrderValidation(ctx, order, ...)
    
    // 2. 检查 ClobPair 是否存在
    GetClobPair(ctx, order.GetClobPairId())
    
    // 3. 检查抵押品是否充足 (关键!)
    AddOrderToOrderbookSubaccountUpdatesCheck(ctx, ...)
    
    // 4. 写入 TransientStore (防重复提交)
    if !lib.IsDeliverTxMode(ctx) {
        MustAddUncommittedStatefulOrderPlacement(ctx, msg)
    }
}

// ReCheckTx 的价值:
新区块可能改变:
  - 子账户余额 (有人提币/充值)
  - 持仓状态 (订单成交导致保证金变化)
  - ClobPair 配置 (治理提案修改参数)
  
ReCheckTx 确保:
  ✓ 订单在新状态下仍然有效
  ✓ 抵押品检查通过
  ✓ 无效订单从 mempool 移除
```

ReCheckTx 不限速

```Go
func (k *Keeper) ShouldRateLimit(ctx sdk.Context) bool {
    // 只在 CheckTx 时限速，ReCheckTx 不限速
    return ctx.IsCheckTx() && !ctx.IsReCheckTx()
}
```

### 有状态订单 为什么会执行两次的PlaceStatefulOrder

状态化订单在 **CheckTx** 和 **DeliverTx** 两个阶段都会调用 `PlaceStatefulOrder`，但它们的**目的不同**，**写入的存储也不同**：

```Go
┌──────────────────────────────────────────────────────────────────────────────────┐
│                    状态化订单的两次 PlaceStatefulOrder 调用                        │
├──────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  【第一次调用 - CheckTx 阶段】                                                    │
│  ═══════════════════════════════                                                 │
│  入口: ClobDecorator.AnteHandle (ante/clob.go)                                   │
│                                                                                  │
│  if msg.Order.OrderId.IsStatefulOrder() {                                        │
│      err = cd.clobKeeper.PlaceStatefulOrder(ctx, msg, false)  // ⬅️ 第一次       │
│  }                                                                               │
│                                                                                  │
│  目的:                                                                           │
│    ✅ 验证订单有效性                                                              │
│    ✅ 抵押品检查                                                                  │
│    ✅ 写入 Uncommitted 瞬态存储 (Transient Store)                                │
│    ❌ 不写入链上状态                                                              │
│                                                                                  │
│  代码路径 (orders.go 第476-485行):                                               │
│  if lib.IsDeliverTxMode(ctx) {                                                   │
│      // DeliverTx 模式：写入 KVStore                                             │
│  } else {                                                                        │
│      // CheckTx 模式：写入 Transient Store ⬅️                                   │
│      k.MustAddUncommittedStatefulOrderPlacement(ctx, msg)                        │
│  }                                                                               │
│                                                                                  │
├──────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  【第二次调用 - DeliverTx 阶段】                                                  │
│  ═══════════════════════════════                                                 │
│  入口: msgServer.PlaceOrder -> HandleMsgPlaceOrder (msg_server_place_order.go)   │
│                                                                                  │
│  if err := k.PlaceStatefulOrder(ctx, msg, isInternalOrder); err != nil {         │
│      return err  // ⬅️ 第二次                                                    │
│  }                                                                               │
│                                                                                  │
│  目的:                                                                           │
│    ✅ 再次验证订单有效性（共识层验证）                                             │
│    ✅ 写入链上 KVStore（永久状态）                                                │
│    ✅ 设置订单过期时间                                                            │
│    ✅ 发送 Indexer 事件                                                          │
│                                                                                  │
│  代码路径 (orders.go 第469-475行):                                               │
│  if lib.IsDeliverTxMode(ctx) {                                                   │
│      // DeliverTx 模式：写入 KVStore ⬅️                                          │
│      k.SetLongTermOrderPlacement(ctx, order, ...)                                │
│      k.AddStatefulOrderIdExpiration(ctx, ...)                                    │
│  } else {                                                                        │
│      // CheckTx 模式：写入 Transient Store                                       │
│  }                                                                               │
│                                                                                  │
└──────────────────────────────────────────────────────────────────────────────────┘
```

CheckTx 阶段的目的

- **快速验证**：在交易进入 mempool 之前验证其有效性
- **防止垃圾交易**：拒绝无效交易进入 mempool
- **临时记录**：写入 Uncommitted Store，防止重复提交
- **不影响链上状态**：即使验证通过，也不写入永久状态

DeliverTx 阶段的目的

- **共识验证**：所有节点执行相同的验证逻辑
- **状态持久化**：写入链上 KVStore，所有节点状态一致
- **触发事件**：发送 Indexer 事件，通知外部系统

### IOC liquidation order

**IOC = Immediate-or-Cancel** （即时成交否则取消）

在 dYdX v4（以及几乎所有专业永续合约交易所，包括 Binance、Bybit、OKX）里，**IOC 是清算单（liquidation order）的唯一允许订单类型**，而且是**强制性的**。

**清算单的唯一使命是“立刻把穿仓仓位砸到市场上，尽快止损”，绝不能挂在订单簿里等着慢慢成交，否则会让坏账继续扩大、威胁保险基金。**

#### IOC 在清算场景下的具体行为（dYdX v4 实际执行逻辑）

步骤行为如果无法满足会怎样1清算引擎检测到子账户 Margin Ratio ≤ 0（穿仓）触发强平2立刻生成一堆 Liquidation Orders，全部标记为 Post-Only = false + TimeInForce = IOC强制3这些 IOC 清算单被塞进匹配引擎开始匹配4立即以当时最优价格（best bid/ask）吃掉能吃的部分成交5任何没成交的部分立刻全部取消，绝不留在订单簿直接丢弃6如果还有剩余仓位未平，继续生成新的 IOC 清算单，直到仓位归零或保险基金接盘循环

#### 和其他 TimeInForce 的对比（为什么不能用别的）

订单类型缩写清算时是否允许如果用了会怎样IOCImmediate-or-Cancel允许（强制）部分成交后剩余立即取消，防止挂单拖延，确保坏账快速止损FOKFill-or-Kill不允许必须一次性全部成交，否则全单取消，太严苛，在深度不足时容易直接失败GTCGood-til-Canceled绝对禁止会长期挂在订单簿里慢慢成交，坏账持续扩大，可能把保险基金耗干Post-OnlyPost-Only绝对禁止只允许挂单不吃盘，清算单永远无法成交，等同于自杀，保险基金直接爆炸

### 已经有LiquidationOrder为什么还要StreamLiquidationOrder？

维度LiquidationOrder[StreamLiquidationOrder](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)用途内部撮合引擎使用gRPC/WebSocket 流式推送给客户端生命周期仅存在于区块处理期间（内存）序列化后通过网络传输数据表示Go 原生类型（强类型封装）Protobuf 生成类型（可序列化）接口实现实现 MatchableOrder 接口不实现任何接口，纯数据容器字段类型复杂类型（satypes.BaseQuantums, Subticks）简单类型（[uint64](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)）

### **`OffchainUpdates`** **作用**

这个文件实现了**链下更新消息的收集与管理**，用于将 CLOB 模块的订单变化**推送给 Indexer**（链下索引服务）。

```Go
链上订单操作 → OffchainUpdates 收集消息 → Kafka/gRPC → Indexer → 前端/API
```

这些类型对应 Indexer 的 Protobuf 定义，确保链上事件能被 Indexer 正确解析。

```Go
const (
    PlaceMessageType   // 下单
    RemoveMessageType  // 删除订单
    UpdateMessageType  // 更新订单（如部分成交）
    ReplaceMessageType // 替换订单
)
```

数据结构

```Go
// OffchainUpdateMessage — 单条消息
type OffchainUpdateMessage struct {
    Type    OffchainUpdateMessageType  // 消息类型
    OrderId OrderId                    // 订单 ID（用于去重/压缩）
    Message msgsender.Message          // 实际的序列化消息体
}

// OffchainUpdates — 消息容器
type OffchainUpdates struct {
    Messages []OffchainUpdateMessage  // 有序的消息列表
}
```

**与其他组件的关系**

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=ZmY2ZThhYTJjYTM0YjczNmE1MGE2NjdjZjkyNGQwYTRfM3pyQ1FxMW9VS1JyWXNUQjlORXNubHd2MXdqeUoxdjFfVG9rZW46WXU2dmJyd0xCbzNqakt4M0Uya2xGbmpPZ2VnXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

### "Hydrate" 在交易系统中的含义

Hydrate（水合/填充） 指从持久化存储（链上状态）读取数据，并加载到内存数据结构的过程，使内存结构“充满数据”以便使用。类比：像给干海绵“注水”，让内存结构从“空”变为“可用”。

### Authenticators

Authenticators（认证器） 是智能账户系统的认证组件，用于验证交易和消息的合法性。每个账户可配置多个 authenticators，实现灵活的认证策略。

### 短期订单  下单撮合是否会锁定余额？

短期订单的余额验证**不在下单时进行**，而是在**撮合时进行.**

**关键的余额检查发生在两个时机：**

**时机1：订单撮合时（matchOrder）**

- 在 `mustPerformTakerOrderMatching` 函数中，每次匹配后都会调用 `CanUpdateSubaccounts` 检查抵押品
- 如果余额不足，订单会被拒绝或部分成交

**时机2：订单加入订单簿时（mustAddOrderToOrderbook）**

- 未成交部分要加入订单簿前，会再次调用抵押品检查
- 通过 `CanUpdateSubaccounts` 验证订单全部成交后账户是否还满足保证金要求

**是否像CEX那样锁定余额？**

**❌ 不会锁定余额！** 这是与传统 CEX 的重要区别：

- **CEX 模式：** 下单时立即锁定对应的资金，未成交前资金不可用
- **dYdX 模式：** 不锁定余额，而是使用**抵押品检查（Collateralization Check）**机制：
  - 计算如果订单完全成交后的账户状态
  - 检查账户的总抵押品是否足够覆盖所有头寸和未平仓订单
  - 使用 `CanUpdateSubaccounts` 进行实时验证

CanUpdateSubaccounts 执行位置：

检查时机文件位置函数检查类型用途撮合时[x/clob/memclob/memclob.go:1777](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)mustPerformTakerOrderMatching → ProcessSingleMatch[satypes.Match](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)验证成交后双方余额是否合规入簿时[x/clob/keeper/orders.go:1139](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)AddOrderToOrderbookSubaccountUpdatesCheck[satypes.CollatCheck](vscode-file://vscode-app/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html)验证挂单时是否有足够保证金

### 短期订单过期时间 是否是当前区块

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=NjY1MzRiZDM4NTM2NTMzMGMxYWYzMWY1ZTQyNDUyOTNfQlJtQWZVZXlScTh3TVB6TDBBdFZmWTA5ZUN0ZEpEUnFfVG9rZW46UFViNGJUQVVGb3dGb2Z4VW40RGxnenZZZ3RnXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

过期的短期订单存储在Orderbook

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=OTYyMmZmYTBhMjRlZDQ1YTIzY2FhNmEyMGM4ZmI0NWZfZGh1ZnBzTno1OERaY1dJSDdXSHZLVERud21pMWV6alpfVG9rZW46VW5tQ2JuY1pxb3gyTEt4VEtJdmxHUXkzZ1VoXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

短期订单的过期 以GoodTilBlock为准

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=NmJmMDM5YjVhNTI4YjQ5MjVmNTgyNzIzMjJiYzA4NzhfYWlxMWZ6eGV1NnA4U1lvb1RQRUhOdG1TMjNpRXBLT0JfVG9rZW46STdHVGI3c09vb1Y3WGN4TW5mVGxuWmFBZ3loXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=ZDUyOGRmYmYzMjc0ZGEwMTIzODA2NWI5ZTdlMDVkYjVfS1hXdWFLVjJ5VEx0OTFvaEhPa2t2MGtUb1hLNVFYQUxfVG9rZW46V1B3Z2J5YnBHb3VOOUh4eTMwVGxIdGNyZ0NmXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=OWU1NzA0Y2VkMWNiNWY0YWRmZTQ4MWQwMDYyZGVhZWZfcERsRFVIS01tN3F6QlA4WWI1bEZ3c0JWWmt5VFB6TENfVG9rZW46WWJvcmIxMHMxb0ZENXd4b01YQ2xTMkl2Z2FiXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

PrepareCheckState 阶段清理过期订单

 

**所以短期订单的过期时间应该不是当前区块**

### PrepareCheckState 重建OrderBook流程

```Go
PrepareCheckState 订单簿重建完整流程
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

前置状态 (区块 N Commit 后):
┌─────────────────────────────────────────────────────────────────────────┐
│  DeliverState (已持久化)          MemClob (内存订单簿)                   │
├─────────────────────────────────────────────────────────────────────────┤
│  ✓ 订单 A (长期, N-5)             ✓ 订单 A (仍在订单簿上)               │
│  ✓ 订单 B (短期, N CheckTx)       ✓ 订单 B (在本地操作队列)             │
│  ✓ 订单 C (条件, N-2)             ✓ 订单 C (仍在订单簿上)               │
│  ✓ 订单 D (短期, GTB=N)           ✓ 订单 D (已过期但还在簿上)           │
│  ✓ 订单 E (长期, 已成交)          ✓ 订单 E (已成交但还在簿上)           │
│  ✓ 订单 F (短期, N CheckTx)       ✓ 订单 F (在本地操作队列)             │
│  ✓ 订单 G (长期, N DeliverTx)     ✗ 订单 G (还未放置到簿上)             │
└─────────────────────────────────────────────────────────────────────────┘

╔═════════════════════════════════════════════════════════════════════════╗
║  Step 1: 获取待清理数据                                                  ║
╚═════════════════════════════════════════════════════════════════════════╝

processProposerMatchesEvents := keeper.GetProcessProposerMatchesEvents(ctx)
├─ OrderIdsFilledInLastBlock: [E]         // 区块 N 完全成交的订单
├─ ExpiredStatefulOrderIds: []            // 区块 N 过期的长期订单
├─ ConditionalOrderIdsTriggeredInLastBlock: [] // 触发的条件订单
└─ RemovedStatefulOrderIds: []            // 强制移除的订单

localValidatorOperationsQueue := keeper.MemClob.GetOperationsToReplay(ctx)
├─ ShortTermOrderPlacement: 订单 B
├─ ShortTermOrderPlacement: 订单 F
└─ ... (本地验证器在区块 N CheckTx 接受的订单)

longTermOrderIds := keeper.GetDeliveredLongTermOrderIds(ctx)
└─ [订单 G]  // 只有区块 N DeliverTx 中新放置的长期订单

╔═════════════════════════════════════════════════════════════════════════╗
║  Step 2: 移除本地操作队列中的订单                                        ║
╚═════════════════════════════════════════════════════════════════════════╝

keeper.MemClob.RemoveAndClearOperationsQueue(ctx, localValidatorOperationsQueue)

操作:
├─ 清空 operationsToPropose (操作队列)
├─ 移除订单 B (在队列中) ✓
├─ 移除订单 F (在队列中) ✓
└─ 保留订单 A, C, D, E, G

当前 MemClob 状态:
┌─────────────────────────────────────────┐
│  订单 A ✓ (长期, 有效)                  │
│  订单 C ✓ (条件, 有效)                  │
│  订单 D ✓ (短期, 已过期)                │
│  订单 E ✓ (长期, 已成交)                │
└─────────────────────────────────────────┘

╔═════════════════════════════════════════════════════════════════════════╗
║  Step 3: 清理无效订单                                                    ║
╚═════════════════════════════════════════════════════════════════════════╝

keeper.MemClob.PurgeInvalidMemclobState(ctx, ...)

3.1 移除已成交订单:
    ├─ 检查订单 E: 完全成交 → 移除 ✓
    
3.2 移除已过期短期订单:
    ├─ 当前区块高度: N+1
    ├─ 检查 blockExpirationsForOrders[N+1]
    ├─ 发现订单 D (GTB=N < N+1) → 移除 ✓
    └─ 发送 ORDER_REMOVAL_REASON_EXPIRED 消息

3.3 移除过期的取消记录:
    └─ removeAllCancelsAtBlock(N+1)

当前 MemClob 状态:
┌─────────────────────────────────────────┐
│  订单 A ✓ (长期, 有效)                  │
│  订单 C ✓ (条件, 有效)                  │
└─────────────────────────────────────────┘

╔═════════════════════════════════════════════════════════════════════════╗
║  Step 4: 两遍放置订单 (Post-Only 优先)                                   ║
╚═════════════════════════════════════════════════════════════════════════╝

第一遍: postOnly = true (只放置 Post-Only 订单)
┌─────────────────────────────────────────────────────────────────────────┐
│  4.1 放置长期订单                                                        │
│      keeper.PlaceStatefulOrdersFromLastBlock(longTermOrderIds, true)    │
│      ├─ 从 State 读取订单 G 的完整数据                                  │
│      ├─ 检查: 订单 G 是 Post-Only? 是 → 放置 ✓                          │
│      └─ 订单 G 添加到 MemClob                                            │
│                                                                           │
│  4.2 放置触发的条件订单                                                   │
│      keeper.PlaceConditionalOrdersTriggeredInLastBlock([], true)        │
│      └─ 无触发的条件订单                                                 │
│                                                                           │
│  4.3 重放本地操作                                                         │
│      keeper.MemClob.ReplayOperations(localValidatorOperationsQueue, true)│
│      ├─ 重新验证订单 B (Post-Only?) 是 → 放置 ✓                         │
│      └─ 重新验证订单 F (Post-Only?) 否 → 跳过                           │
└─────────────────────────────────────────────────────────────────────────┘

当前 MemClob 状态 (第一遍后):
┌─────────────────────────────────────────┐
│  订单 A ✓ (长期, 保留)                  │
│  订单 B ✓ (短期, 重新放置, Post-Only)   │
│  订单 C ✓ (条件, 保留)                  │
│  订单 G ✓ (长期, 新放置, Post-Only)     │
└─────────────────────────────────────────┘

第二遍: postOnly = false (放置所有订单)
┌─────────────────────────────────────────────────────────────────────────┐
│  4.4 再次放置长期订单                                                     │
│      keeper.PlaceStatefulOrdersFromLastBlock(longTermOrderIds, false)   │
│      └─ 订单 G 已在订单簿上 → 跳过 (避免重复)                           │
│                                                                           │
│  4.5 再次放置条件订单                                                     │
│      keeper.PlaceConditionalOrdersTriggeredInLastBlock([], false)       │
│      └─ 无条件订单                                                       │
│                                                                           │
│  4.6 再次重放本地操作                                                     │
│      keeper.MemClob.ReplayOperations(localValidatorOperationsQueue, false)│
│      ├─ 订单 B 已在订单簿上 → 跳过                                       │
│      └─ 订单 F (非 Post-Only) → 放置 ✓                                  │
└─────────────────────────────────────────────────────────────────────────┘

最终 MemClob 状态:
┌─────────────────────────────────────────┐
│  订单 A ✓ (长期, 保留)                  │
│  订单 B ✓ (短期, 重新放置)              │
│  订单 C ✓ (条件, 保留)                  │
│  订单 F ✓ (短期, 重新放置)              │
│  订单 G ✓ (长期, 新放置)                │
└─────────────────────────────────────────┘

╔═════════════════════════════════════════════════════════════════════════╗
║  Step 5: 清算和去杠杆化                                                  ║
╚═════════════════════════════════════════════════════════════════════════╝

5.1 获取可清算子账户
    liquidatableSubaccountIds := keeper.DaemonLiquidationInfo.GetLiquidatableSubaccountIds()

5.2 执行清算
    keeper.LiquidateSubaccountsAgainstOrderbook(ctx, liquidatableSubaccountIds)

5.3 执行去杠杆化
    keeper.DeleverageSubaccounts(ctx, subaccountsToDeleverage)

5.4 提款管控
    keeper.GateWithdrawalsIfNegativeTncSubaccountSeen(ctx, negativeTncSubaccountIds)

╔═════════════════════════════════════════════════════════════════════════╗
║  Step 6: 完成 - 订单簿准备就绪                                           ║
╚═════════════════════════════════════════════════════════════════════════╝

最终状态:
┌─────────────────────────────────────────────────────────────────────────┐
│  MemClob 订单簿 (区块 N+1 CheckTx 准备就绪)                              │
├─────────────────────────────────────────────────────────────────────────┤
│  ✓ 订单 A: 长期订单, 区块 N-5 放置, 保留                                 │
│  ✓ 订单 B: 短期订单, 区块 N CheckTx, 重新放置                           │
│  ✓ 订单 C: 条件订单, 区块 N-2 触发, 保留                                 │
│  ✓ 订单 F: 短期订单, 区块 N CheckTx, 重新放置                           │
│  ✓ 订单 G: 长期订单, 区块 N DeliverTx, 新放置                           │
├─────────────────────────────────────────────────────────────────────────┤
│  订单簿状态: 一致、有效、准备接受新的 CheckTx                             │
└─────────────────────────────────────────────────────────────────────────┘

发送链外更新:
└─ keeper.SendOffchainMessages(offchainUpdates)
    ├─ 订单 D 过期移除通知
    ├─ 订单 E 成交移除通知
    └─ 订单 B, F, G 放置/更新通知
有效订单 (不需要移除):
├─ 已在订单簿上
├─ 没有状态变化
└─ 保留即可 ✓
无效订单 (需要移除):
├─ 已过期
├─ 已成交
├─ 已取消
└─ 必须移除 ✓
需要重新验证的订单:
├─ 本地操作队列中的订单
├─ 需要重新执行 CheckTx 验证
└─ 先移除后重新添加 ✓
```

**关键理解:**

- ❌ **不是** 清空订单簿
- ✅ **只移除** 无效的订单（已成交、已过期、已取消、已强制移除）
- ✅ 有效订单保留在订单簿上

GetDeliveredLongTermOrderIds **只返回当前区块（刚完成的区块）达成共识的长期订单，而不是历史区块的订单。**

**`GetDeliveredLongTermOrderIds`** **返回的是：**

✅ **只有当前区块（刚执行完的区块）达成共识的长期订单**

❌ **不包含历史区块的长期订单**

**原因：**

1. ✅ BeginBlocker 在每个区块开始时调用 `ResetAllDeliveredOrderIds()`
2. ✅ 清空上个区块记录的订单 ID 列表
3. ✅ `DeliverTx` 只添加当前区块的新订单
4. ✅ PrepareCheckState 读取的是当前区块的订单列表
5. ✅ 历史订单已经在 MemClob 订单簿上，不需要重新放置

### 多节点订单簿一致性

```Go
完整的多节点订单簿一致性保证机制 (详细时序图)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

场景设定:
• 3个验证器节点 (A, B, C)，节点 B 是当前区块的 Proposer
• Alice: 短期订单，买入 1 BTC @ $50,000
• Bob: 长期订单，卖出 2 BTC @ $51,000  
• Charlie: 短期订单，买入 0.5 BTC @ $49,000
• David: 短期订单，买入 0.3 BTC @ $50,500
• Eve: 短期订单，卖出 1 BTC @ $50,200

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

                    节点 A               节点 B (Proposer)      节点 C
                  (验证器)                 (验证器)            (验证器)
初始状态         ┌──────────┐            ┌──────────┐         ┌──────────┐
(区块 N-1 后)    │ MemClob  │            │ MemClob  │         │ MemClob  │
                 │ 历史订单:│            │ 历史订单:│         │ 历史订单:│
                 │ Order-1  │            │ Order-1  │         │ Order-1  │
                 │ Order-2  │            │ Order-2  │         │ Order-2  │
                 └────┬─────┘            └────┬─────┘         └────┬─────┘
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 1: 区块 N - CheckTx 阶段 (无共识，各节点独立)                        ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
   T1: Alice 订单到达  │                       │                     │
      (网络传播)      ├──>CheckTx             │                     │
                      │   ┌─────────────┐     │                     │
                      │   │验证订单      │     │                     │
                      │   │余额检查 ✓   │     │                     │
                      │   │放入 Mempool │     │                     │
                      │   └─────────────┘     │                     │
                      │   MemClob:            │                     │
                      │   [Alice]             │                     │
                      │   OTP: [Alice]        │                     │
                      │                       │                     │
   T2: Alice 订单到达  │                       ├──>CheckTx           │
      (稍晚)          │                       │   ┌─────────────┐   │
                      │                       │   │验证订单      │   │
                      │                       │   │余额检查 ✓   │   │
                      │                       │   │放入 Mempool │   │
                      │                       │   └─────────────┘   │
                      │                       │   MemClob:          │
                      │                       │   [Alice]           │
                      │                       │   OTP: [Alice]      │
                      │                       │                     │
   T3: Charlie 订单   ├──>CheckTx             │                     ├──>CheckTx
      到达            │   MemClob:            │                     │   MemClob:
                      │   [Alice, Charlie]    │                     │   [Charlie]
                      │   OTP: [Alice,        │                     │   OTP: [Charlie]
                      │         Charlie]      │                     │
                      │                       │                     │
   T4: Charlie 订单   │                       ├──>CheckTx           │
      到达 (节点B)    │                       │   MemClob:          │
                      │                       │   [Alice, Charlie]  │
                      │                       │   OTP: [Alice,      │
                      │                       │         Charlie]    │
                      │                       │                     │
   T5: Bob 订单       │                       │   ⚠️ 长期订单       │
      (长期订单到达)  │                       │   不在 CheckTx      │
                      │                       │   处理，等待        │
                      │                       │   DeliverTx         │
                      │                       │                     │
   T6: David 订单     ├──>CheckTx             │                     │
      到达 (节点A)    │   MemClob:            │                     │
                      │   [Alice, Charlie,    │                     │
                      │    David]             │                     │
                      │   OTP: [Alice,        │                     │
                      │         Charlie,      │                     │
                      │         David]        │                     │
                      │                       │                     │
   T7: Alice 订单     │                       │                     ├──>CheckTx
      到达 (节点C)    │                       │                     │   MemClob:
                      │                       │                     │   [Charlie,
                      │                       │                     │    Alice]
                      │                       │                     │   OTP: [Charlie,
                      │                       │                     │         Alice]
                      │                       │                     │
   T8: Eve 订单       │                       ├──>CheckTx           │
      到达 (节点B)    │                       │   MemClob:          │
                      │                       │   [Alice, Charlie,  │
                      │                       │    Eve]             │
                      │                       │   OTP: [Alice,      │
                      │                       │         Charlie,    │
                      │                       │         Eve]        │
                      ▼                       ▼                     ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  区块 N CheckTx 结束时状态 (各节点不同! 正常现象)                          │
├───────────────────────────────────────────────────────────────────────────┤
│  节点 A:                    节点 B (Proposer):          节点 C:             │
│  MemClob: [Alice,           MemClob: [Alice,            MemClob: [Charlie, │
│            Charlie,                   Charlie,                    Alice]    │
│            David]                     Eve]                                  │
│  OTP: [Alice, Charlie,      OTP: [Alice, Charlie,       OTP: [Charlie,     │
│        David]                     Eve]                        Alice]        │
│  余额 (CheckState):         余额 (CheckState):          余额 (CheckState):  │
│  Alice: -$50k               Alice: -$50k                Alice: -$50k        │
│  Charlie: -$49k             Charlie: -$49k              Charlie: -$49k      │
│  David: -$50.5k             Eve: +$50.2k                                    │
│                                                                              │
│  ⚠️ 注意差异:                                                                │
│  • 订单数量不同: A有3个, B有3个, C有2个                                      │
│  • 订单集合不同: A有David, B有Eve, C都没有                                  │
│  • 顺序不同: A和B都是[Alice, Charlie], C是[Charlie, Alice]                  │
└───────────────────────────────────────────────────────────────────────────┘
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 2: 共识阶段 - Propose (Proposer 生成区块提案)                        ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
                      │    节点 B 作为 Proposer 生成区块提案          │
                      │                       │                     │
                      │                       ├─────────────────────┤
                      │                       │ PrepareProposal     │
                      │                       │ ┌─────────────────┐ │
                      │                       │ │1. 从 Mempool    │ │
                      │                       │ │   选择交易      │ │
                      │                       │ │   ├─Alice ✓     │ │
                      │                       │ │   ├─Charlie ✓   │ │
                      │                       │ │   ├─Eve ✓       │ │
                      │                       │ │   └─Bob ✓(长期) │ │
                      │                       │ │                 │ │
                      │                       │ │2. 执行撮合      │ │
                      │                       │ │   MatchOrders() │ │
                      │                       │ │   ├─Match1: Alice│ │
                      │                       │ │   │  vs Order-X │ │
                      │                       │ │   └─Match2: Eve │ │
                      │                       │ │      vs Order-Y │ │
                      │                       │ │                 │ │
                      │                       │ │3. 生成         │ │
                      │                       │ │   MsgProposed  │ │
                      │                       │ │   Operations:  │ │
                      │                       │ │   ├─Alice订单   │ │
                      │                       │ │   ├─Match1     │ │
                      │                       │ │   ├─Charlie订单 │ │
                      │                       │ │   ├─Eve订单     │ │
                      │                       │ │   ├─Match2     │ │
                      │                       │ │   └─Bob订单     │ │
                      │                       │ └─────────────────┘ │
                      │                       │                     │
                      │  ⚠️ 注意:             │                     │
                      │  • David 订单不在提案中 (Proposer 没收到)    │
                      │  • 操作顺序由 Proposer 决定                  │
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 3: 共识阶段 - Prevote & Precommit (Tendermint 共识)                  ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
                      │  ◄────广播提案────────┤                     │
                      │                       ├────广播提案───────>│
                      │                       │                     │
                      ├──>ProcessProposal     │                     ├──>ProcessProposal
                      │   ┌─────────────────┐ │                     │   ┌─────────────────┐
                      │   │验证提案:        │ │                     │   │验证提案:        │
                      │   │├─订单有效性 ✓  │ │                     │   │├─订单有效性 ✓  │
                      │   │├─撮合正确性 ✓  │ │                     │   │├─撮合正确性 ✓  │
                      │   │├─顺序合法性 ✓  │ │                     │   │├─顺序合法性 ✓  │
                      │   │└─接受提案      │ │                     │   │└─接受提案      │
                      │   └─────────────────┘ │                     │   └─────────────────┘
                      │                       │                     │
                      ├──>Prevote (YES)       │                     ├──>Prevote (YES)
                      ├──────────────────────>│◄────────────────────┤
                      │                       │                     │
                      │  ◄────收集 Prevotes────┤                     │
                      │  (超过 2/3 投票)       ├────收集 Prevotes───>│
                      │                       │                     │
                      ├──>Precommit (YES)     │                     ├──>Precommit (YES)
                      ├──────────────────────>│◄────────────────────┤
                      │                       │                     │
                      │  ◄───收集 Precommits───┤                     │
                      │  (超过 2/3 投票)       ├───收集 Precommits──>│
                      │  ✓ 达成共识!           │  ✓ 达成共识!          │  ✓ 达成共识!
                      │                       │                     │
┌───────────────────────────────────────────────────────────────────────────┐
│  共识结果 (所有节点一致! ✓)                                                │
├───────────────────────────────────────────────────────────────────────────┤
│  共识的操作序列 (MsgProposedOperations):                                   │
│  1. Alice 订单 (短期)                                                      │
│  2. Match1 (Alice vs Order-X)                                             │
│  3. Charlie 订单 (短期)                                                    │
│  4. Eve 订单 (短期)                                                        │
│  5. Match2 (Eve vs Order-Y)                                               │
│  6. Bob 订单 (长期)                                                        │
│                                                                            │
│  ⚠️ 注意: David 订单不在共识中 (将在下个区块被 Proposer 选择)              │
└───────────────────────────────────────────────────────────────────────────┘
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 4: 区块 N - DeliverTx 阶段 (所有节点执行相同操作)                    ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
   ProcessProposerOperations(共识的操作序列)                         │
                      │                       │                     │
   1. PlaceOrder      ├──>State: Alice订单    ├──>State: Alice订单  ├──>State: Alice订单
      (Alice)         │   余额: -$50k         │   余额: -$50k       │   余额: -$50k
                      │   DeliverState ✓      │   DeliverState ✓    │   DeliverState ✓
                      │                       │                     │
   2. ProcessMatch1   ├──>执行撮合            ├──>执行撮合          ├──>执行撮合
      (Alice vs X)    │   更新余额            │   更新余额          │   更新余额
                      │   State ✓             │   State ✓           │   State ✓
                      │                       │                     │
   3. PlaceOrder      ├──>State: Charlie订单  ├──>State: Charlie订单├──>State: Charlie订单
      (Charlie)       │   余额: -$49k         │   余额: -$49k       │   余额: -$49k
                      │   DeliverState ✓      │   DeliverState ✓    │   DeliverState ✓
                      │                       │                     │
   4. PlaceOrder      ├──>State: Eve订单      ├──>State: Eve订单    ├──>State: Eve订单
      (Eve)           │   余额: +$50.2k       │   余额: +$50.2k     │   余额: +$50.2k
                      │   DeliverState ✓      │   DeliverState ✓    │   DeliverState ✓
                      │                       │                     │
   5. ProcessMatch2   ├──>执行撮合            ├──>执行撮合          ├──>执行撮合
      (Eve vs Y)      │   更新余额            │   更新余额          │   更新余额
                      │   State ✓             │   State ✓           │   State ✓
                      │                       │                     │
   6. PlaceOrder      ├──>State: Bob订单      ├──>State: Bob订单    ├──>State: Bob订单
      (Bob, 长期)     │   持久化 ✓            │   持久化 ✓          │   持久化 ✓
                      │   AddDelivered        │   AddDelivered      │   AddDelivered
                      │   LongTermOrderId     │   LongTermOrderId   │   LongTermOrderId
                      │   MemStore["DLTO:0"]  │   MemStore["DLTO:0"]│   MemStore["DLTO:0"]
                      │   = Bob OrderId ✓     │   = Bob OrderId ✓   │   = Bob OrderId ✓
                      │                       │                     │
                      ▼                       ▼                     ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  DeliverTx 执行后状态 (所有节点完全一致! ✓)                                │
├───────────────────────────────────────────────────────────────────────────┤
│  所有节点 DeliverState (持久化到 IAVL Tree):                               │
│  ├─ Alice 订单: 已成交                                                     │
│  ├─ Charlie 订单: 已放置到 State                                           │
│  ├─ Eve 订单: 已成交                                                       │
│  ├─ Bob 订单: 已放置到 State (长期)                                        │
│  ├─ 余额更新: 所有参与者余额一致                                           │
│  └─ MemStore["DLTO:0"] = Bob OrderId (所有节点相同) ✓                      │
│                                                                            │
│  所有节点 CheckState (仍然不一致，但即将被清空):                            │
│  节点 A: [Alice, Charlie, David]                                           │
│  节点 B: [Alice, Charlie, Eve]                                             │
│  节点 C: [Charlie, Alice]                                                  │
│  ⚠️ 这些差异将在 Commit 时消除                                              │
└───────────────────────────────────────────────────────────────────────────┘
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 5: Commit 阶段 (持久化并清空临时状态)                                 ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
   Commit()           ├──>DeliverState        ├──>DeliverState      ├──>DeliverState
                      │   .Write()            │   .Write()          │   .Write()
                      │   ┌─────────────────┐ │   ┌───────────────┐ │   ┌───────────────┐
                      │   │持久化到         │ │   │持久化到       │ │   │持久化到       │
                      │   │IAVL Tree:       │ │   │IAVL Tree:     │ │   │IAVL Tree:     │
                      │   │├─订单状态 ✓     │ │   │├─订单状态 ✓   │ │   │├─订单状态 ✓   │
                      │   │├─余额更新 ✓     │ │   │├─余额更新 ✓   │ │   │├─余额更新 ✓   │
                      │   │└─成交记录 ✓     │ │   │└─成交记录 ✓   │ │   │└─成交记录 ✓   │
                      │   └─────────────────┘ │   └───────────────┘ │   └───────────────┘
                      │                       │                     │
                      │   checkState = nil    │   checkState = nil  │   checkState = nil
                      │   ⚠️ 清空 CheckState   │   ⚠️ 清空 CheckState │   ⚠️ 清空 CheckState
                      │   (消除不一致) ✓      │   (消除不一致) ✓    │   (消除不一致) ✓
                      │                       │                     │
                      │   deliverState = nil  │   deliverState=nil  │   deliverState=nil
                      │                       │                     │
                      ▼                       ▼                     ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  Commit 后状态 (CheckState 差异被消除! ✓)                                  │
├───────────────────────────────────────────────────────────────────────────┤
│  所有节点相同:                                                              │
│  ├─ DeliverState: 已持久化到 IAVL Tree ✓                                  │
│  ├─ CheckState: null (下次 CheckTx 时基于 committed state 创建)            │
│  ├─ MemStore["DLTO:0"] = Bob OrderId (保留在内存) ✓                        │
│  └─ 所有临时差异被清除 ✓                                                   │
└───────────────────────────────────────────────────────────────────────────┘
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 6: BeginBlock N+1 (重置追踪列表)                                      ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
   BeginBlocker       ├──>Reset              ├──>Reset            ├──>Reset
                      │   DeliveredOrderIds  │   DeliveredOrderIds│   DeliveredOrderIds
                      │   MemStore清空:      │   MemStore清空:    │   MemStore清空:
                      │   "DLTO:*" → []      │   "DLTO:*" → []    │   "DLTO:*" → []
                      │   "DCDO:*" → []      │   "DCDO:*" → []    │   "DCDO:*" → []
                      │   "DCL:*" → []       │   "DCL:*" → []     │   "DCL:*" → []
                      │                       │                     │
                      │   ⚠️ 清空区块N的      │                     │
                      │   订单追踪列表        │                     │
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 7: PrepareCheckState (重建订单簿，恢复一致性)                         ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
   Step 1:            │                       │                     │
   获取本地操作队列    ├──>GetOperations      ├──>GetOperations    ├──>GetOperations
                      │   ToReplay()         │   ToReplay()       │   ToReplay()
                      │   返回: [Alice,       │   返回: [Alice,     │   返回: [Charlie,
                      │         Charlie,      │         Charlie,    │         Alice]
                      │         David]        │         Eve]        │
                      │   ⚠️ 各节点不同       │   ⚠️ 各节点不同    │   ⚠️ 各节点不同
                      │                       │                     │
   Step 2:            │                       │                     │
   移除本地操作        ├──>RemoveAndClear     ├──>RemoveAndClear   ├──>RemoveAndClear
                      │   OperationsQueue()  │   OperationsQueue() │   OperationsQueue()
                      │   MemClob清空本地    │   MemClob清空本地  │   MemClob清空本地
                      │   操作队列订单 ✓     │   操作队列订单 ✓   │   操作队列订单 ✓
                      │                       │                     │
   Step 3:            │                       │                     │
   清理无效订单        ├──>PurgeInvalid       ├──>PurgeInvalid     ├──>PurgeInvalid
                      │   MemclobState()     │   MemclobState()   │   MemclobState()
                      │   移除已成交、过期等 │   移除已成交、过期等│   移除已成交、过期等
                      │                       │                     │
   Step 4:            │                       │                     │
   获取长期订单        ├──>GetDelivered       ├──>GetDelivered     ├──>GetDelivered
                      │   LongTermOrderIds() │   LongTermOrderIds()│   LongTermOrderIds()
                      │   MemStore读取:      │   MemStore读取:    │   MemStore读取:
                      │   [Bob OrderId] ✓    │   [Bob OrderId] ✓  │   [Bob OrderId] ✓
                      │   ⚠️ 所有节点相同!   │   ⚠️ 所有节点相同! │   ⚠️ 所有节点相同!
                      │                       │                     │
   Step 5:            │                       │                     │
   从State读取订单     ├──>GetLongTerm        ├──>GetLongTerm      ├──>GetLongTerm
                      │   OrderPlacement     │   OrderPlacement   │   OrderPlacement
                      │   (Bob OrderId)      │   (Bob OrderId)    │   (Bob OrderId)
                      │   State读取:         │   State读取:       │   State读取:
                      │   Bob 完整订单数据 ✓ │   Bob 完整订单数据✓│   Bob 完整订单数据✓
                      │   ⚠️ 所有节点读取    │   ⚠️ 所有节点读取  │   ⚠️ 所有节点读取
                      │   相同的持久化数据   │   相同的持久化数据 │   相同的持久化数据
                      │                       │                     │
   Step 6:            │                       │                     │
   放置长期订单        ├──>PlaceStateful      ├──>PlaceStateful    ├──>PlaceStateful
   (两遍)             │   OrdersFromLast     │   OrdersFromLast   │   OrdersFromLast
                      │   Block()            │   Block()          │   Block()
                      │   第一遍: post-only  │   第一遍: post-only│   第一遍: post-only
                      │   第二遍: all orders │   第二遍: all orders│   第二遍: all orders
                      │   MemClob:           │   MemClob:         │   MemClob:
                      │   [Bob] ✓            │   [Bob] ✓          │   [Bob] ✓
                      │   ⚠️ 基础状态一致!   │   ⚠️ 基础状态一致! │   ⚠️ 基础状态一致!
                      │                       │                     │
   Step 7:            │                       │                     │
   重放本地操作        ├──>ReplayOperations   ├──>ReplayOperations ├──>ReplayOperations
   (两遍)             │   第一遍: post-only  │   第一遍: post-only│   第一遍: post-only
                      │   [Alice, Charlie,   │   [Alice, Charlie, │   [Charlie, Alice]
                      │    David]            │    Eve]            │
                      │   第二遍: all orders │   第二遍: all orders│   第二遍: all orders
                      │   MemClob:           │   MemClob:         │   MemClob:
                      │   [Bob, Alice,       │   [Bob, Alice,     │   [Bob, Charlie,
                      │    Charlie, David]   │    Charlie, Eve]   │    Alice]
                      │   ⚠️ 本地订单不同   │   ⚠️ 本地订单不同 │   ⚠️ 本地订单不同
                      │   但基于一致基础     │   但基于一致基础   │   但基于一致基础
                      │                       │                     │
                      ▼                       ▼                     ▼
┌───────────────────────────────────────────────────────────────────────────┐
│  PrepareCheckState 完成后状态 (基础一致性恢复! ✓)                          │
├───────────────────────────────────────────────────────────────────────────┤
│  ✅ 基础层 (长期订单) - 完全一致:                                           │
│     节点 A: [Bob] ← 从 State 读取                                          │
│     节点 B: [Bob] ← 从 State 读取                                          │
│     节点 C: [Bob] ← 从 State 读取                                          │
│     所有节点相同! ✓                                                        │
│                                                                            │
│  ⚠️ 临时层 (短期订单) - 可能不同:                                          │
│     节点 A: [Alice, Charlie, David]                                        │
│     节点 B: [Alice, Charlie, Eve]                                          │
│     节点 C: [Charlie, Alice]                                               │
│     • 订单数量不同: A有3个, B有3个, C有2个                                  │
│     • 订单集合不同: A有David, B有Eve, C都没有                              │
│     • 顺序不同: A和B都是[Alice, Charlie], C是[Charlie, Alice]              │
│                                                                            │
│  ✅ 关键理解:                                                               │
│     • 所有节点基于相同的基础状态 (长期订单、余额、历史成交) ✓               │
│     • 短期订单差异不影响基础一致性 ✓                                        │
│     • 下个区块通过共识统一 ✓                                                │
└───────────────────────────────────────────────────────────────────────────┘
                      │                       │                     │
╔═══════════════════════════════════════════════════════════════════════════╗
║  阶段 8: 区块 N+1 - CheckTx 阶段 (基于一致基础开始)                         ║
╚═══════════════════════════════════════════════════════════════════════════╝
                      │                       │                     │
   新订单到达          │                       │                     │
   (基于一致基础验证)  │                       │                     │
                      ├──>CheckTx            ├──>CheckTx          ├──>CheckTx
                      │   ✓ Bob订单在簿上    │   ✓ Bob订单在簿上  │   ✓ Bob订单在簿上
                      │   ✓ 余额一致         │   ✓ 余额一致       │   ✓ 余额一致
                      │   ✓ 历史成交一致     │   ✓ 历史成交一致   │   ✓ 历史成交一致
                      │   基础状态完全相同 ✓ │                     │
                      │                       │                     │
   David 订单          │                       ├──>CheckTx          │
   到达节点B           │                       │   (第一次收到)     │
                      │                       │   放入 Mempool ✓   │
                      │                       │                     │
                      ▼                       ▼                     ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  多节点订单簿一致性的核心机制                                             │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│  1️⃣ 短期订单差异是正常的:                                                 │
│     ├─ 各节点收到的订单可能不同 ✓                                        │
│     ├─ 订单数量可能不同 ✓                                                │
│     ├─ 订单顺序可能不同 ✓                                                │
│     └─ CheckTx 无共识,各节点独立验证 ✓                                   │
│                                                                           │
│  2️⃣ 共识消除差异:                                                         │
│     ├─ Proposer 选择订单集合 (从自己的 Mempool)                           │
│     ├─ 生成 MsgProposedOperations                                        │
│     ├─ Tendermint 共识 (>2/3 验证器同意)                                  │
│     └─ 所有节点执行相同的操作序列 ✓                                       │
│                                                                           │
│  3️⃣ DeliverTx 保证一致:                                                   │
│     ├─ 所有节点执行相同的交易                                             │
│     ├─ 写入相同的 DeliverState                                            │
│     ├─ 持久化到 IAVL Tree (Merkle 证明)                                  │
│     └─ 长期订单、余额、成交记录完全一致 ✓                                 │
│                                                                           │
│  4️⃣ Commit 清空临时差异:                                                  │
│     ├─ checkState = null                                                 │
│     ├─ 消除 CheckTx 阶段的所有差异 ✓                                      │
│     └─ 下次基于 committed state 创建 ✓                                   │
│                                                                           │
│  5️⃣ PrepareCheckState 重建基础一致性:                                     │
│     ├─ 从 MemStore 读取长期订单 ID (所有节点相同)                         │
│     ├─ 从 State 读取完整订单数据 (共识保证一致)                           │
│     ├─ 放置到 MemClob 订单簿 (基础层一致) ✓                               │
│     └─ 重放本地操作 (临时层可能不同,但不影响基础) ✓                        │
│                                                                           │
│  6️⃣ 最终一致性保证:                                                       │
│     ├─ 基础状态 (长期订单): 完全一致 ✓                                    │
│     ├─ 临时状态 (短期订单): 可能不同,但下个区块统一 ✓                     │
│     └─ 通过共识循环不断收敛到一致状态 ✓                                   │
│                                                                           │
└─────────────────────────────────────────────────────────────────────────┘
```

### 为什么需要重新执行 ReplayPlaceOrder

分析 `ReplayPlaceOrder` 的重新执行原因,我发现了关键的设计逻辑:

#### 1. **区块状态切换问题**

**位置**: `x/clob/abci.go:175-181` 和 `x/clob/keeper/orders.go:447-480`

在 `PrepareCheckState` 中,系统需要从刚提交的区块 `h` 准备下一个区块 `h+1` 的 `CheckState`:

```Go
// 步骤 1: 移除本地验证者操作队列中的所有订单
localValidatorOperationsQueue, shortTermOrderTxBytes := keeper.MemClob.GetOperationsToReplay(ctx)
keeper.MemClob.RemoveAndClearOperationsQueue(ctx, localValidatorOperationsQueue)

// 步骤 2: 清理 MemClob 中的无效状态
offchainUpdates = keeper.MemClob.PurgeInvalidMemclobState(ctx, ...)
```

#### 2. **状态不一致的根本原因**

**问题场景**:

- **区块 h 提交前**: 本地验证者在 `CheckTx` 阶段接收并验证了很多短期订单,这些订单被放入 `MemClob` 和 `localValidatorOperationsQueue`
- **区块 h 提交**: Proposer 的 `MsgProposedOperations` 可能只包含了部分订单(或顺序不同)
- **区块 h 提交后**: `MemClob` 中的订单状态与链上最终状态不一致

#### 3. **ReplayPlaceOrder 的核心作用**

**位置**: `x/clob/keeper/orders.go:447-480`

```Go
func (k Keeper) ReplayPlaceOrder(
    ctx sdk.Context,
    msg *types.MsgPlaceOrder,
) {
    order := msg.GetOrder()
    
    // 使用下一个区块的高度,检查订单在下一个区块是否有效
    nextBlockHeight := lib.MustConvertIntegerToUint32(ctx.BlockHeight() + 1)
    
    // 重新进行状态验证
    err = k.PerformStatefulOrderValidation(ctx, &order, nextBlockHeight, true)
    if err != nil {
        return 0, 0, nil, err
    }
    
    // 重新放置订单到 MemClob
    return k.MemClob.PlaceOrder(ctx, msg.Order)
}
```

#### 4. **为什么必须"重新"执行**

**关键点**(`x/clob/abci.go:207-262`):

```Go
// 步骤 3: 第一遍 - 仅放置 post-only 订单
offchainUpdates = keeper.PlaceStatefulOrdersFromLastBlock(ctx, longTermOrderIds, offchainUpdates, true) // post only
offchainUpdates = keeper.PlaceConditionalOrdersTriggeredInLastBlock(ctx, ..., true) // post only

replayUpdates := keeper.MemClob.ReplayOperations(
    ctx,
    localValidatorOperationsQueue,
    shortTermOrderTxBytes,
    offchainUpdates,
    true, // post only filter
)

// 步骤 6: 第二遍 - 放置非 post-only 订单
offchainUpdates = keeper.PlaceStatefulOrdersFromLastBlock(ctx, longTermOrderIds, offchainUpdates, false) // 非 post only
offchainUpdates = keeper.PlaceConditionalOrdersTriggeredInLastBlock(ctx, ..., false) // 非 post only

replayUpdates = keeper.MemClob.ReplayOperations(
    ctx,
    localValidatorOperationsQueue,
    shortTermOrderTxBytes,
    offchainUpdates,
    false, // 非 post only filter
)
```

#### 5. **"重新"的四层含义**

(1) **重新验证** - 状态已改变

```Go
// x/clob/keeper/orders.go:461-464
nextBlockHeight := lib.MustConvertIntegerToUint32(ctx.BlockHeight() + 1)
err = k.PerformStatefulOrderValidation(ctx, &order, nextBlockHeight, true)
```

- 原因: 区块高度 +1,短期订单的 `goodTilBlock` 可能已过期
- 余额状态可能因区块 h 的匹配而改变

(2) **重新匹配** - 订单簿已清空

```Go
// x/clob/abci.go:181
keeper.MemClob.RemoveAndClearOperationsQueue(ctx, localValidatorOperationsQueue)
```

- 原因: `PrepareCheckState` 开始时清空了整个 MemClob
- 必须按正确顺序重建订单簿

(3) **两阶段重新放置** - Post-only 优先

```Go
// x/clob/memclob/memclob.go:947-951
if postOnlyFilter != order.IsPostOnlyOrder() {
    continue
}
```

- **第一遍 (post-only=true)**: 只放置 post-only 订单,不会发生匹配
- **第二遍 (post-only=false)**: 放置可匹配订单,可能与 post-only 订单撮合

(4) **"第二次机会"机制**

```Go
// x/clob/memclob/memclob.go:1072-1115 - OrderRemoval 处理
case *types.InternalOperation_OrderRemoval:
    orderId := operation.GetOrderRemoval().OrderId
    
    // 如果之前已经作为 PreexistingStatefulOrder 放置成功,跳过删除
    if _, placedPreviously := placedPreexistingStatefulOrderIds[orderId]; placedPreviously {
        continue
    }
    
    // 否则尝试"第二次机会"重新放置
    if _, removedPreviously := placedOrderRemovalOrderIds[orderId]; !removedPreviously {
        // 尝试重新放置订单...
    }
```

#### 6. **设计目标总结**

**位置**: `x/clob/keeper/orders.go:438-446` 注释

```Go
// ReplayPlaceOrder 用于 ReplayOperations 流程,重新将短期订单和新放置的状态订单
// 放回到 memclob。这个方法不直接转发事件到 indexer,而是以 OffchainUpdates 的形式返回。
```

| 目标           | 实现方式                                     |
| -------------- | -------------------------------------------- |
| **状态一致性** | 清空 MemClob 后重建,确保与链上状态一致       |
| **订单有效性** | 使用 `nextBlockHeight` 重新验证,过滤过期订单 |
| **公平匹配**   | Post-only 先放置,避免被立即匹配              |
| **容错性**     | "第二次机会"机制,避免误删除有效订单          |
| **性能优化**   | 只重放本地验证者的操作,不重放全网所有操作    |

#### 7. **关键代码路径**

```Plain
PrepareCheckState (abci.go:146)
  ├─ 步骤 1: RemoveAndClearOperationsQueue (清空 MemClob)
  ├─ 步骤 2: PurgeInvalidMemclobState (清理无效状态)
  ├─ 步骤 3: ReplayOperations(postOnly=true) 
  │   └─ ReplayPlaceOrder (orders.go:447)
  │       ├─ PerformStatefulOrderValidation (重新验证)
  │       └─ MemClob.PlaceOrder (重新放置)
  ├─ 步骤 4: PlaceStatefulOrdersFromLastBlock(postOnly=false)
  └─ 步骤 6: ReplayOperations(postOnly=false)
      └─ ReplayPlaceOrder (再次重新放置非 post-only 订单)
```

#### **核心答案**

必须"重新"执行 `ReplayPlaceOrder` 因为:

1. **状态重置**: PrepareCheckState 清空了 MemClob,必须重建订单簿
2. **时间推进**: 区块高度 +1,需要用新高度重新验证订单有效性
3. **顺序保证**: 两阶段放置(post-only 先行)确保公平匹配
4. **最终一致**: 只重放本地操作,但基于已提交的全局状态重新执行,确保所有验证者的 MemClob 最终一致

### Operation 操作的状态改变 

**peration 的类型定义**

dYdX CLOB 中的 Operation 有两种表示形式:

**InternalOperation** (内部操作)

位于 internal_operation.go:

```Go
type InternalOperation struct {
    Operation isInternalOperation_Operation
}

// 具体类型:
- InternalOperation_ShortTermOrderPlacement     // 短期订单放置
- InternalOperation_PreexistingStatefulOrder    // 预存在的有状态订单
- InternalOperation_Match                       // 撮合
- InternalOperation_OrderRemoval                // 订单移除
```

**OperationRaw** (原始操作)

用于在区块提议中传输:

```Go
type OperationRaw struct {
    Operation isOperationRaw_Operation
}

// 具体类型:
- OperationRaw_ShortTermOrderPlacement  // 短期订单(TX字节)
- OperationRaw_Match                     // 撮合
- OperationRaw_OrderRemoval             // 订单移除
// 注意: 没有 PreexistingStatefulOrder (仅用于本地重放)
```

#### **OperationsToPropose 核心数据结构**

位于 operations_to_propose.go:

```Go
type OperationsToPropose struct {
    // 有序的操作队列
    OperationsQueue []InternalOperation
    
    // 已在队列中的订单哈希集合
    OrderHashesInOperationsQueue map[OrderHash]bool
    
    // 短期订单哈希到交易字节的映射
    ShortTermOrderHashToTxBytes map[OrderHash][]byte
    
    // 已撮合的订单ID到订单的映射
    MatchedOrderIdToOrder map[OrderId]Order
    
    // 已在队列中的订单移除集合
    OrderRemovalsInOperationsQueue map[OrderId]bool
}
```

#### **Operation 状态转换的完整生命周期**

```Plain
┌────────────────────────────────────────────────────────────────────────┐
│                   OPERATION 状态转换生命周期图                          │
└────────────────────────────────────────────────────────────────────────┘

阶段 1: CheckTx - 本地接收订单
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
用户提交订单
    ↓
CheckTx (AnteHandler)
    ↓
PlaceOrder() → matchOrder()
    ├─ 创建 branchedContext (临时缓存)
    ├─ 执行撮合
    └─ writeCache() 写入 CheckState
         ↓
    【状态变更 1】添加 ShortTermOrderTxBytes
         ↓
    m.operationsToPropose.MustAddShortTermOrderTxBytes(order, ctx.TxBytes())
         ↓
         ShortTermOrderHashToTxBytes[orderHash] = txBytes
         ↓
    【状态变更 2】添加订单放置到队列
         ↓
    m.operationsToPropose.MustAddShortTermOrderPlacementToOperationsQueue(order)
         ↓
         OperationsQueue = append(OperationsQueue, 
             NewShortTermOrderPlacementInternalOperation(order))
         OrderHashesInOperationsQueue[orderHash] = true
         ↓
    【状态变更 3】如果发生撮合,添加 Match 操作
         ↓
    m.operationsToPropose.MustAddMatchToOperationsQueue(takerOrder, makerFills)
         ↓
         OperationsQueue = append(OperationsQueue,
             NewMatchOrdersInternalOperation(takerOrder, makerFills))
         ↓
         MatchedOrderIdToOrder[orderId] = order

状态: OperationsQueue = [OrderPlacement, Match, OrderPlacement, ...]


阶段 2: PrepareProposal - Proposer 构建区块提议
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Proposer 节点被选中
    ↓
PrepareProposal()
    ↓
GetOperationsRaw()
    ↓
    【状态读取】GetOperationsToPropose()
    ↓
    遍历 OperationsQueue:
    ├─ ShortTermOrderPlacement
    │   ├─ 从 ShortTermOrderHashToTxBytes 获取 TX 字节
    │   └─ 转换为 OperationRaw_ShortTermOrderPlacement
    │
    ├─ Match
    │   └─ 转换为 OperationRaw_Match
    │
    ├─ PreexistingStatefulOrder
    │   └─ 跳过 (不包含在提议中)
    │
    └─ OrderRemoval
        └─ 转换为 OperationRaw_OrderRemoval
    ↓
    构建 MsgProposedOperations
    ↓
    返回 []OperationRaw

状态: 转换为网络传输格式


阶段 3: ProcessProposal - 验证者验证提议
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
所有验证者接收提议
    ↓
ProcessProposal()
    ↓
    验证 Operations 的顺序和有效性
    ↓
    Vote: Accept/Reject

状态: 只读验证,无状态变更


阶段 4: FinalizeBlock (DeliverTx) - 共识执行
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
共识达成
    ↓
FinalizeBlock()
    ↓
ProcessProposerOperations(operations)
    ↓
    【状态变更 4】生成 ProcessProposerMatchesEvents
    ↓
    GenerateProcessProposerMatchesEvents(operations)
    ├─ 扫描所有 Match 操作
    ├─ 提取 OrderIdsFilledInLastBlock
    ├─ 提取 PlacedLongTermOrderIds
    └─ 提取 ExpiredStatefulOrderIds
    ↓
    遍历 operations:
    │
    ├─ ShortTermOrderPlacement
    │   └─ DecodeTx → PlaceOrder → 执行订单
    │
    ├─ Match (ClobMatch)
    │   ├─ ProcessSingleMatch
    │   ├─ 更新余额 (DeliverState)
    │   ├─ 更新 FillAmount (持久化)
    │   └─ AddDeliveredLongTermOrderId (MemStore)
    │
    └─ OrderRemoval
        └─ 从状态移除订单
    ↓
    【状态变更 5】持久化到 MemStore
    ↓
    MustSetProcessProposerMatchesEvents(processProposerMatchesEvents)
    ↓
    写入 MemStore["PPME"]

状态: DeliverState 更新,持久化存储写入


阶段 5: Commit - 提交到磁盘
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Commit()
    ↓
    DeliverState → IAVL Tree
    ↓
    【状态变更 6】清空 CheckState
    ↓
    checkState = nil
    ↓
    【状态变更 7】清空 MemStore (transient)
    ↓
    MemStore 被清空 (但 ProcessProposerMatchesEvents 会在下个 BeginBlock 读取)

状态: 写入磁盘,CheckState 清空


阶段 6: BeginBlock - 新区块开始
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
BeginBlock(block N+1)
    ↓
    【状态变更 8】读取上个区块的 ProcessProposerMatchesEvents
    ↓
    processProposerMatchesEvents := k.GetProcessProposerMatchesEvents(ctx)
    ↓
    【状态变更 9】重置所有已交付订单跟踪
    ↓
    k.ResetAllDeliveredOrderIds(ctx)
    ├─ 清空 MemStore["DLTO:*"]  (长期订单)
    ├─ 清空 MemStore["DCDO:*"]  (条件订单)
    └─ 清空 MemStore["DDSLO:*"] (延迟消息订单)

状态: 清理上个区块的临时跟踪数据


阶段 7: PrepareCheckState - 重建 Memclob
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
PrepareCheckState() 在 BeginBlock 之后
    ↓
    【状态变更 10】获取本地操作队列
    ↓
    localValidatorOperationsQueue, shortTermTxBytes := 
        k.MemClob.GetOperationsToReplay(ctx)
    ↓
    返回 OperationsQueue (包含 PreexistingStatefulOrder)
    ↓
    【状态变更 11】移除并清空操作队列
    ↓
    k.MemClob.RemoveAndClearOperationsQueue(ctx, localValidatorOperationsQueue)
    ↓
    遍历 localValidatorOperationsQueue:
    ├─ ShortTermOrderPlacement
    │   ├─ 如果订单仍在 orderbook 且哈希匹配
    │   │   └─ mustRemoveOrder(orderId)
    │   └─ 否则
    │       └─ RemoveShortTermOrderTxBytes(order)
    │
    └─ PreexistingStatefulOrder
        └─ 如果订单仍在 orderbook
            └─ mustRemoveOrder(orderId)
    ↓
    【状态变更 12】清空操作队列
    ↓
    m.operationsToPropose.ClearOperationsQueue()
    ├─ OperationsQueue = []
    ├─ OrderHashesInOperationsQueue = {}
    ├─ MatchedOrderIdToOrder = {}
    └─ OrderRemovalsInOperationsQueue = {}
    ↓
    【状态变更 13】清理无效状态
    ↓
    k.MemClob.PurgeInvalidMemclobState(ctx, filledOrderIds, expiredOrderIds, ...)
    ├─ 移除已完全成交的订单
    ├─ 移除已过期的订单 (GoodTilBlock 检查)
    └─ 移除已取消的订单
    ↓
    【状态变更 14】重放操作 - 第一遍 (Post-Only)
    ↓
    k.MemClob.ReplayOperations(
        ctx, 
        localValidatorOperationsQueue,
        shortTermTxBytes,
        offchainUpdates,
        postOnlyFilter = true  // 只放置 post-only 订单
    )
    ↓
    遍历 localValidatorOperationsQueue:
    ├─ ShortTermOrderPlacement
    │   ├─ 如果是 post-only
    │   │   ├─ PlaceOrder(order, txBytes)
    │   │   ├─ 重新添加到 ShortTermOrderHashToTxBytes
    │   │   ├─ 重新添加到 OperationsQueue
    │   │   └─ 重新添加到 OrderHashesInOperationsQueue
    │   └─ 否则跳过
    │
    ├─ PreexistingStatefulOrder
    │   ├─ 从状态读取订单
    │   ├─ 如果是 post-only
    │   │   ├─ PlaceOrder(order)
    │   │   ├─ 重新添加到 OperationsQueue
    │   │   └─ 重新添加到 OrderHashesInOperationsQueue
    │   └─ 否则跳过
    │
    └─ Match / OrderRemoval
        └─ 跳过 (no-op)
    ↓
    【状态变更 15】重放有状态订单 - 从已交付列表
    ↓
    deliveredLongTermOrderIds := k.GetDeliveredLongTermOrderIds(ctx, clobPairId)
    ↓
    遍历 deliveredLongTermOrderIds:
    ├─ 如果订单是 post-only
    │   ├─ PlaceOrder(order)
    │   ├─ 添加到 OperationsQueue
    │   └─ 添加到 OrderHashesInOperationsQueue
    └─ 否则跳过
    ↓
    【状态变更 16】重放操作 - 第二遍 (Non Post-Only)
    ↓
    k.MemClob.ReplayOperations(
        ctx,
        localValidatorOperationsQueue,
        shortTermTxBytes,
        offchainUpdates,
        postOnlyFilter = false  // 放置非 post-only 订单
    )
    ↓
    遍历 localValidatorOperationsQueue:
    ├─ ShortTermOrderPlacement
    │   ├─ 如果不是 post-only
    │   │   ├─ PlaceOrder(order, txBytes)
    │   │   ├─ 重新添加到 ShortTermOrderHashToTxBytes
    │   │   ├─ 重新添加到 OperationsQueue
    │   │   └─ 重新添加到 OrderHashesInOperationsQueue
    │   └─ 否则跳过
    │
    └─ PreexistingStatefulOrder
        └─ 同上
    ↓
    【状态变更 17】重放有状态订单 - 第二遍
    ↓
    (同步骤 15,但处理非 post-only 订单)

状态: OperationsToPropose 重建完成,orderbook 恢复


阶段 8: 下一个 CheckTx - 循环开始
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
新的 CheckTx 到达
    ↓
    使用重建后的 OperationsToPropose
    ↓
    回到阶段 1

状态: 新一轮循环开始
```

#### **关键状态变更详解**

##### **添加操作到队列**

**短期订单放置** (位于 `memclob.go:384-390`):

```Go
// 步骤 1: 添加 TX 字节
m.operationsToPropose.MustAddShortTermOrderTxBytes(taker, ctx.TxBytes())

// 步骤 2: 添加订单放置操作
m.operationsToPropose.MustAddShortTermOrderPlacementToOperationsQueue(taker)
```

**有状态订单放置** (位于 `memclob.go:380-382`):

```Go
m.operationsToPropose.MustAddStatefulOrderPlacementToOperationsQueue(taker)
```

**撮合操作** (位于 `memclob.go:391`):

```Go
internalOperation := m.operationsToPropose.MustAddMatchToOperationsQueue(
    takerOrder, 
    makerFillWithOrders
)
```

##### **清空操作队列**

位于 `operations_to_propose.go:44-49`:

```Go
func (o *OperationsToPropose) ClearOperationsQueue() {
    o.OperationsQueue = make([]InternalOperation, 0)
    o.OrderHashesInOperationsQueue = make(map[OrderHash]bool, 0)
    o.MatchedOrderIdToOrder = make(map[OrderId]Order)
    o.OrderRemovalsInOperationsQueue = make(map[OrderId]bool, 0)
    // 注意: 不清空 ShortTermOrderHashToTxBytes
}
```

**为什么不清空** **`ShortTermOrderHashToTxBytes`****?**

- 因为在 `RemoveAndClearOperationsQueue` 中会选择性移除
- 只移除已在 orderbook 中的订单的 TX 字节
- 保留未在 orderbook 中的订单的 TX 字节供后续使用
  - 

##### **移除订单**

位于 `memclob.go:2006-2036`:

```Go
func (m *MemClobPriceTimePriority) mustRemoveOrder(
    ctx sdk.Context,
    orderId types.OrderId,
) {
    // 1. 从 orderbook 移除
    orderbook.mustRemoveOrder(levelOrder)
    
    // 2. 如果是短期订单且不在操作队列中,移除 TX 字节
    order := levelOrder.Value.Order
    if order.IsShortTermOrder() &&
        !m.operationsToPropose.IsOrderPlacementInOperationsQueue(order) {
        m.operationsToPropose.RemoveShortTermOrderTxBytes(order)
    }
    
    // 3. 发送 gRPC 更新
    if m.generateOrderbookUpdates {
        orderbookUpdate := m.GetOrderbookUpdatesForOrderRemoval(ctx, order.OrderId)
        m.clobKeeper.SendOrderbookUpdates(ctx, orderbookUpdate)
    }
}
```

**数据流转换关系**

```Plain
┌───────────────────────────────────────────────────────────────────┐
│                        数据结构转换关系                            │
└───────────────────────────────────────────────────────────────────┘

InternalOperation (本地使用)
    │
    ├─ ShortTermOrderPlacement
    │   ├─ Order
    │   └─ TxBytes (存储在 ShortTermOrderHashToTxBytes)
    │
    ├─ PreexistingStatefulOrder
    │   └─ OrderId (仅引用)
    │
    ├─ Match
    │   └─ ClobMatch
    │
    └─ OrderRemoval
        └─ OrderId + RemovalReason

                    ↓ GetOperationsToPropose()
                    
OperationRaw (网络传输)
    │
    ├─ ShortTermOrderPlacement
    │   └─ []byte (TX字节,从 ShortTermOrderHashToTxBytes 获取)
    │
    ├─ Match
    │   └─ ClobMatch
    │
    └─ OrderRemoval
        └─ OrderId + RemovalReason

    ⚠️ 注意: PreexistingStatefulOrder 不会转换为 OperationRaw
```

#### **总结: Operation 状态转换关键点**

1. **CheckTx 阶段**: 
   1. 订单添加到 `OperationsQueue`
   2. TX 字节添加到 `ShortTermOrderHashToTxBytes`
   3. 撮合添加到 `OperationsQueue`
      - 
2. **PrepareProposal 阶段**:
   1. `InternalOperation` → `OperationRaw` 转换
   2. `PreexistingStatefulOrder` 被过滤掉
      - 
3. **FinalizeBlock 阶段**:
   1. 执行所有操作
   2. 生成 `ProcessProposerMatchesEvents`
   3. 跟踪已交付的长期订单
      - 
4. **Commit 阶段**:
   1. CheckState 清空
   2. MemStore 清空
      - 
5. **BeginBlock 阶段**:
   1. 重置已交付订单跟踪
      - 
6. **PrepareCheckState 阶段**:
   1. 获取本地操作队列 (`GetOperationsToReplay` - 包含 `PreexistingStatefulOrder`)
   2. 移除旧订单
   3. 清空操作队列
   4. 两遍重放 (post-only 优先)
   5. 重建 `OperationsToPropose`
      - 
7. **下一个 CheckTx**:
   1. 使用重建后的 `OperationsToPropose`
   2. 循环继续
      - 

**关键设计理念**:

- ✅ `OperationsToPropose` 是 **本地状态**, 每个节点可以不同
- ✅ `MsgProposedOperations` 是 **共识状态**, 所有节点必须一致
- ✅ `PrepareCheckState` 是 **同步点**, 确保本地状态与共识状态一致
- ✅ `PreexistingStatefulOrder` 只用于本地重放, 不参与共识

### DeliverTx 修改用户余额是否有双花问题

双花（Double Spending）是指同一笔资金被多次使用的问题。在区块链交易中，常见的双花场景包括：

- **并发执行**：多个交易同时读取同一账户余额并扣款
- **状态不一致**：交易 A 和交易 B 都认为账户有 100 USDC，各自扣除 80 USDC
- **回滚失败**：部分更新成功，部分失败，导致状态不一致

```Go
FinalizeBlock (Consensus)
    ↓
ProcessProposerOperations()
    ├─ ValidateAndTransformRawOperations() [无状态验证]
    └─ ProcessInternalOperations()
        ↓
        遍历每个 Operation:
        ├─ ShortTermOrderPlacement → PlaceOrder()
        ├─ Match → ProcessSingleMatch()
        │   ├─ 构建 updates []satypes.Update
        │   └─ UpdateSubaccounts()
        │       ├─ getSettledUpdates() [读取当前余额]
        │       ├─ internalCanUpdateSubaccountsWithLeverage() [验证]
        │       ├─ CalculateUpdatedSubaccount() [计算新余额]
        │       └─ SetSubaccount() [写入新余额]
        └─ OrderRemoval → MustRemoveStatefulOrder()
func (k Keeper) ProcessSingleMatch(...) {
    // ... 计算费用、保险基金等 ...
    
    // 创建子账户更新
    updates := []satypes.Update{
        // Taker update
        {
            AssetUpdates: []satypes.AssetUpdate{
                {
                    AssetId:          assettypes.AssetUsdc.Id,
                    BigQuantumsDelta: bigTakerQuoteBalanceDelta,  // 净余额变化
                },
            },
            PerpetualUpdates: []satypes.PerpetualUpdate{
                {
                    PerpetualId:      perpetualId,
                    BigQuantumsDelta: bigTakerPerpetualQuantumsDelta, // 仓位变化
                },
            },
            SubaccountId: matchWithOrders.TakerOrder.GetSubaccountId(),
        },
        // Maker update
        {
            AssetUpdates: []satypes.AssetUpdate{
                {
                    AssetId:          assettypes.AssetUsdc.Id,
                    BigQuantumsDelta: bigMakerQuoteBalanceDelta,
                },
            },
            PerpetualUpdates: []satypes.PerpetualUpdate{
                {
                    PerpetualId:      perpetualId,
                    BigQuantumsDelta: bigMakerPerpetualQuantumsDelta,
                },
            },
            SubaccountId: matchWithOrders.MakerOrder.GetSubaccountId(),
        },
    }

    // 应用更新
    success, successPerUpdate, err := k.subaccountsKeeper.UpdateSubaccounts(
        ctx,
        updates,
        satypes.Match,
    )
    // ... 错误处理 ...
}
```

**关键点**:

- ✅ BigQuantumsDelta 是 **增量更新**, 不是绝对值
- ✅ 同一个 `ProcessSingleMatch` 调用中的 taker/maker 更新是 **原子的**
- ✅ 传递的是 updates 数组,包含两个账户的更新

```Go
func (k Keeper) UpdateSubaccounts(
    ctx sdk.Context,
    updates []types.Update,
    updateType types.UpdateType,
) (success bool, successPerUpdate []types.UpdateResult, err error) {
    
    // 1. 获取所有相关的永续合约信息
    perpInfos, err := k.GetAllRelevantPerpetuals(ctx, updates)
    if err != nil {
        return false, nil, err
    }

    // 2. 获取结算后的更新 (读取当前状态)
    settledUpdates, subaccountIdToFundingPayments, err := k.getSettledUpdates(
        ctx, 
        updates, 
        perpInfos, 
        true  // requireUniqueSubaccount: 必须唯一
    )
    if err != nil {
        return false, nil, err
    }

    // 3. 验证更新是否合法 (抵押品检查)
    success, successPerUpdate, err = k.internalCanUpdateSubaccountsWithLeverage(
        ctx,
        settledUpdates,
        updateType,
        perpInfos,
    )

    if !success || err != nil {
        return success, successPerUpdate, err  // ❌ 验证失败,不修改状态
    }

    // 4. 更新开放利益 (OI)
    perpOpenInterestDelta := salib.GetDeltaOpenInterestFromUpdates(settledUpdates, updateType)
    if perpOpenInterestDelta != nil {
        if err := k.perpetualsKeeper.ModifyOpenInterest(...); err != nil {
            return false, nil, err  // ❌ OI 更新失败,回滚
        }
    }

    // 5. 计算更新后的子账户状态
    for i := range settledUpdates {
        settledUpdates[i].SettledSubaccount = salib.CalculateUpdatedSubaccount(
            settledUpdates[i],
            perpInfos,
        )
    }

    // 6. 转移抵押品 (隔离市场)
    for _, settledUpdateWithUpdatedSubaccount := range settledUpdates {
        if err := k.computeAndExecuteCollateralTransfer(...); err != nil {
            return false, nil, err  // ❌ 转移失败,回滚
        }
    }

    // 7. 应用所有更新 (写入状态)
    for _, u := range settledUpdates {
        k.SetSubaccount(ctx, u.SettledSubaccount)  // ✅ 写入新状态
        // ... 发送事件 ...
    }

    return success, successPerUpdate, err
}
```

**关键防护机制**:

1. **唯一性检查** (requireUniqueSubaccount: true):
   1. ```Go
      if exists && requireUniqueSubaccount {
          return nil, nil, types.ErrNonUniqueUpdatesSubaccount
      }
      ```

   2. ✅ 同一个 SubaccountId 不能在同一批次更新中出现多次
2. **先验证后更新**:
   1. ✅ 所有验证在步骤 3 完成
   2. ✅ 只有验证通过后才执行步骤 4-7
   3. ✅ 任何步骤失败都会返回 error
3. **原子性保证**:
   1. ✅ 所有更新在同一个 ctx 中执行
   2. ✅ Cosmos SDK 的 `CacheContext` 机制保证原子性

```Go
func (k Keeper) getSettledUpdates(
    ctx sdk.Context,
    updates []types.Update,
    perpInfos perptypes.PerpInfos,
    requireUniqueSubaccount bool,
) (
    settledUpdates []types.SettledUpdate,
    subaccountIdToFundingPayments map[types.SubaccountId]map[uint32]dtypes.SerializableInt,
    err error,
) {
    var idToSettledSubaccount = make(map[types.SubaccountId]types.Subaccount)
    var idToLeverageMap = make(map[types.SubaccountId]map[uint32]uint32)
    settledUpdates = make([]types.SettledUpdate, len(updates))
    subaccountIdToFundingPayments = make(map[types.SubaccountId]map[uint32]dtypes.SerializableInt)

    // 遍历所有更新并查询相关的子账户
    for i, u := range updates {
        settledSubaccount, exists := idToSettledSubaccount[u.SubaccountId]
        var fundingPayments map[uint32]dtypes.SerializableInt
        var leverageMap map[uint32]uint32

        if exists && requireUniqueSubaccount {
            return nil, nil, types.ErrNonUniqueUpdatesSubaccount  // ❌ 重复的 SubaccountId
        }

        // 如果 SubaccountId 不在 map 中,获取并存储结算后的子账户状态
        if !exists {
            subaccount := k.GetSubaccount(ctx, u.SubaccountId)  // ✅ 从状态读取当前余额
            settledSubaccount, fundingPayments = salib.GetSettledSubaccountWithPerpetuals(
                subaccount, 
                perpInfos
            )

            // ... 获取杠杆配置 ...

            idToSettledSubaccount[u.SubaccountId] = settledSubaccount
            idToLeverageMap[u.SubaccountId] = leverageMap
            subaccountIdToFundingPayments[u.SubaccountId] = fundingPayments
        } else {
            // 重用缓存的杠杆映射
            leverageMap = idToLeverageMap[u.SubaccountId]
        }

        settledUpdate := types.SettledUpdate{
            SettledSubaccount: settledSubaccount,  // 当前余额
            AssetUpdates:      u.AssetUpdates,     // 增量变化
            PerpetualUpdates:  u.PerpetualUpdates, // 增量变化
            LeverageMap:       leverageMap,
        }

        settledUpdates[i] = settledUpdate
    }

    return settledUpdates, subaccountIdToFundingPayments, nil
}
```

**关键点**:

- ✅ 从 ctx.KVStore 读取当前余额
- ✅ 使用 idToSettledSubaccount map 缓存已读取的子账户
- ✅ 如果同一个 SubaccountId 出现多次,会检测到并返回错误

**❌ DeliverTx 不会发生双花问题**

**原因**:

1. **串行执行**: 所有 Operations 按顺序执行,不存在并发
2. **增量更新**: 使用 Delta 而不是绝对值,正确累加变化
3. **唯一性保证**: 同批次更新中每个账户只能出现一次
4. **抵押品验证**: 防止透支和过度杠杆
5. **共识保证**: 所有验证者执行相同的操作,结果一致

**可能的风险 (理论上)**:

- ⚠️ 如果 Cosmos SDK 或 Tendermint 有 bug,可能导致状态不一致
- ⚠️ 如果验证者作恶 (但需要 >1/3 验证者合谋)

### 余额不足的撮合结果，是否会失败

余额不足（准确说是**抵押品不足**）的撮合会被**拒绝，**系统会继续运行：

- ✅ **CheckTx 阶段**：抵押品不足的订单会被拒绝，不会进入 memclob
- ✅ **撮合阶段**：抵押品不足的 Maker 订单会被移除，Taker 继续撮合下一个订单
- ✅ **DeliverTx 阶段**：抵押品不足的撮合不会被持久化

```Go
┌───────────────────────────────────────────────────────────────────┐
│                    订单撮合抵押品检查完整流程                      │
└───────────────────────────────────────────────────────────────────┘

阶段 1: 订单放置 (CheckTx/DeliverTx)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
用户提交订单
    ↓
PlaceOrder()
    ↓
matchOrder() [在 memclob 中]
    ├─ branchedContext, writeCache := ctx.CacheContext()  // 创建临时缓存
    ├─ mustPerformTakerOrderMatching()
    │   ├─ 查找最优 Maker 订单
    │   ├─ 计算成交数量
    │   └─ ProcessSingleMatch()
    │       ├─ 计算费用
    │       ├─ 构建 updates []satypes.Update
    │       └─ UpdateSubaccounts()
    │           ├─ getSettledUpdates() [读取当前余额]
    │           ├─ internalCanUpdateSubaccountsWithLeverage() ✅ 抵押品检查
    │           │   ├─ GetRiskForSettledUpdate()
    │           │   │   ├─ NC (净抵押品)
    │           │   │   ├─ IMR (初始保证金要求)
    │           │   │   └─ MMR (维持保证金要求)
    │           │   └─ 判断抵押品充足性:
    │           │       ├─ NC >= IMR ? 可以开仓 ✅
    │           │       ├─ NC < IMR && NC >= MMR ? 只能减仓 ⚠️
    │           │       └─ NC < MMR ? 将被清算 ❌
    │           └─ if success:
    │               └─ SetSubaccount() [写入余额]
    │
    └─ if 抵押品检查通过:
        ├─ writeCache() ✅ 提交到 CheckState
        └─ 记录到 operationsToPropose
    else:
        ├─ 不调用 writeCache() ❌ 自动回滚
        ├─ 如果是 Maker 失败: 移除 Maker 订单,继续找下一个
        └─ 如果是 Taker 失败: 停止撮合,返回失败状态
```

**撮合中的抵押品检查**位于 memclob/memclob.go:1773-1832:

```Go
func (m *MemClobPriceTimePriority) mustPerformTakerOrderMatching(...) {
    // ... 前面的撮合逻辑 ...
    
    for {
        // 1. 找到下一个 Maker 订单
        makerLevelOrder, foundMakerOrder = orderbook.getBestOrderOnSide(!takerIsBuy)
        if !foundMakerOrder {
            break  // 没有更多 Maker 订单
        }
        
        // 2. 检查价格是否交叉
        if !takerOrderCrossesMakerOrder {
            break  // 价格不匹配
        }
        
        // 3. 计算成交数量
        matchedAmount = min(takerRemainingSize, makerRemainingSize)
        
        // 4. 执行抵押品检查
        matchWithOrders := types.MatchWithOrders{
            TakerOrder: newTakerOrder,
            MakerOrder: &makerOrder.Order,
            FillAmount: matchedAmount,
        }
        
        // ✅ 关键调用: 检查抵押品
        success, takerUpdateResult, makerUpdateResult, _, err := 
            m.clobKeeper.ProcessSingleMatch(ctx, &matchWithOrders, ...)
        
        // 5. 处理抵押品检查结果
        if !success {
            makerCollatOkay := updateResultToOrderStatus(makerUpdateResult).IsSuccess()
            takerCollatOkay := takerIsLiquidation || 
                              updateResultToOrderStatus(takerUpdateResult).IsSuccess()
            
            // Maker 抵押品不足
            if !makerCollatOkay {
                makerOrdersToRemove = append(makerOrdersToRemove, 
                    OrderWithRemovalReason{
                        Order:         makerOrder.Order,
                        RemovalReason: types.OrderRemoval_REMOVAL_REASON_UNDERCOLLATERALIZED,
                    },
                )
            }
            
            // Taker 抵押品不足 (非清算订单)
            if !takerCollatOkay {
                takerOrderStatus.OrderStatus = updateResultToOrderStatus(takerUpdateResult)
                break  // ❌ 停止撮合
            }
            
            // Taker OK, 继续找下一个 Maker
            continue
        }
        
        // 6. 抵押品检查通过,记录成交
        takerRemainingSize -= matchedAmount
        newMakerFills = append(newMakerFills, ...)
    }
    
    return newMakerFills, ..., takerOrderStatus
}
```

**关键行为**:

- ✅ **Maker 抵押品不足**: 标记移除,继续撮合下一个 Maker
- ❌ **Taker 抵押品不足**: 停止撮合,返回失败状态
- ✅ **清算订单**: Taker 永远不会失败抵押品检查

抵押品验证逻辑

位于 x/subaccounts/keeper/subaccount.go:665-726:

```Go
func (k Keeper) internalCanUpdateSubaccountsWithLeverage(...) (
    success bool,
    successPerUpdate []types.UpdateResult,
    err error,
) {
    // ... 前面的验证 ...
    
    // 遍历所有更新
    for i, u := range settledUpdates {
        // 1. 计算更新后的风险指标
        riskNew, err := salib.GetRiskForSettledUpdate(u, perpInfos)
        if err != nil {
            return false, nil, err
        }
        
        var result = types.Success
        
        // 2. 检查是否抵押品充足
        if !riskNew.IsInitialCollateralized() {
            // ❌ 抵押品不足: NC < IMR
            
            // 获取当前的风险指标 (更新前)
            riskCur, err := salib.GetRiskForSubaccount(
                u.SettledSubaccount,
                perpInfos,
                u.LeverageMap,
            )
            
            // 判断状态转换是否有效
            result = salib.IsValidStateTransitionForUndercollateralizedSubaccount(
                riskCur,
                riskNew,
            )
        }
        
        // 3. 记录结果
        if !result.IsSuccess() {
            success = false
        }
        successPerUpdate[i] = result
    }
    
    return success, successPerUpdate, nil
}
```

### 去杠杆和清算为什么在PrepareCheckState阶段执行

**PrepareCheckState的时序优势:**

1. **状态最新:** 区块 `h` 已提交,拥有最准确的账户/持仓状态
2. **影响CheckTx:** 清算结果更新MemClob,影响后续交易的验证逻辑
3. **提供门控:** 为负TNC账户设置提现门控,在CheckTx阶段拦截提现
4. **本地执行:** 不影响共识确定性,各验证者独立维护本地订单簿
5. **操作队列:** 清算结果进入本地队列,由Proposer选择性打包
6. **重新验证:** DeliverTx阶段重新生成清算单并执行,确保确定性

### **关键设计理念**

**乐观执行 + 确定性验证:**

- PrepareCheckState: **乐观清算**(可能因订单簿不同而结果不同)
- DeliverTx: **确定性清算**(基于区块中包含的操作重新执行)

**守护进程 + 链上执行:**

- Daemon: **链下扫描**可清算账户(异步,非确定性)
- PrepareCheckState: **链上执行**清算逻辑(同步,确定性)

这种设计既保证了**及时性**(尽早清算风险账户),又确保了**确定性**(共识不受影响)。

**为什么需要最新状态:**

```Go
// abci.go 第261-264行
// 7. 获取所有可能需要清算的子账户并尝试清算它们
liquidatableSubaccountIds := keeper.DaemonLiquidationInfo.GetLiquidatableSubaccountIds()
subaccountsToDeleverage, err := keeper.LiquidateSubaccountsAgainstOrderbook(ctx, liquidatableSubaccountIds)
```

- **清算守护进程(Liquidation Daemon)** 在后台持续扫描链下数据,计算可清算账户列表
- Daemon通过gRPC调用`LiquidateSubaccounts`接口更新DaemonLiquidationInfo
- PrepareCheckState从Daemon获取最新的可清算账户ID列表
- 这些ID基于区块 `h` 的**最终状态**计算,确保数据准确性

**为什么不在EndBlocker执行:**

```Go
// EndBlocker只处理:
// - 过期订单清理
// - 条件单触发检测  
// - 统计指标更新
// 不执行清算/去杠杆
```

**EndBlocker的限制:**

- EndBlocker在共识过程中执行,其输出必须**完全确定性**
- 清算依赖订单簿匹配,但订单簿状态可能因为:
  - 不同验证者的本地mempool内容不同
  - 网络传播延迟导致的订单到达顺序差异
  - 本地操作队列(LocalValidatorOperationsQueue)的差异
- 在EndBlocker执行清算可能导致**状态分叉**

### 长期订单什么时候撮合 

长期订单的撮合发生在两个关键阶段:

1. **PrepareCheckState阶段** - 放入内存订单簿(MemClob)
2. **CheckTx阶段(短期订单)** - 与新进入的短期订单撮合

长期订单的生命周期

```Go
用户提交 → DeliverTx(区块h) → 持久化到链上 → PrepareCheckState(h+1) → 放入MemClob → 撮合
```

关键代码：

```Go
// msg_server_place_order.go 第19-32行
// DeliverTx阶段:长期订单通过MsgPlaceOrder消息提交
func (k msgServer) PlaceOrder(goCtx context.Context, msg *types.MsgPlaceOrder) {
    ctx := lib.UnwrapSDKContext(goCtx, types.ModuleName)
    if err := k.Keeper.HandleMsgPlaceOrder(ctx, msg, false); err != nil {
        return nil, err
    }
    return &types.MsgPlaceOrderResponse{}, nil
}

// msg_server_place_order.go 第153-157行  
// DeliverTx执行后,订单ID被记录到memstore
k.AddDeliveredLongTermOrderId(ctx, order.OrderId)
```

PrepareCheckState阶段:订单簿重建

**执行时机:** 区块 `h` 提交后,准备区块 `h+1` 的CheckState时

```Go
// abci.go 第192-237行
// 步骤3-4: 第一轮放置(仅Post-Only订单)
longTermOrderIds := keeper.GetDeliveredLongTermOrderIds(ctx)
offchainUpdates = keeper.PlaceStatefulOrdersFromLastBlock(
    ctx,
    longTermOrderIds,
    offchainUpdates,
    true, // postOnlyFilter=true,只放Post-Only订单
)

// 步骤5: 第二轮放置(所有长期订单)
offchainUpdates = keeper.PlaceStatefulOrdersFromLastBlock(
    ctx,
    longTermOrderIds,
    offchainUpdates,
    false, // postOnlyFilter=false,放置所有订单
)
```

**两轮放置的原因:**

- **第一轮(Post-Only):** 确保被动挂单订单优先进入订单簿,避免与已有订单立即撮合
- **第二轮(All Orders):** 放置所有剩余订单,可能立即触发撮合

### 订单撮合理解

#### 撮合架构

```Go
┌──────────────────────────────────────────────────────────────────┐
│                    订单撮合系统架构                          │
└──────────────────────────────────────────────────────────────────┘

                         交易流程总览
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
        ▼                     ▼                     ▼
   ┌─────────┐          ┌─────────┐          ┌─────────┐
   │CheckTx  │          │PrepareP │          │DeliverTx│
   │阶段     │────────► │阶段     │────────► │阶段     │
   │乐观撮合 │          │提议操作 │          │确定执行 │
   └─────────┘          └─────────┘          └─────────┘
        │                     │                     │
        ▼                     ▼                     ▼
   MemClob              Operations           State Persist
   (内存订单簿)         Queue                (持久化状态)
        │                     │                     │
        └─────────────────────┴─────────────────────┘
                              │
                    核心撮合引擎 (MatchEngine)
┌────────────────────────────────────────────────────────────────┐
│                     撮合引擎组件图                              │
├────────────────────────────────────────────────────────────────┤
│                                                                │
│  ┌──────────────────────────────────────────────────────┐    │
│  │         MemClobPriceTimePriority (撮合引擎)          │    │
│  ├──────────────────────────────────────────────────────┤    │
│  │                                                      │    │
│  │  • PlaceOrder()         - 订单放置入口               │    │
│  │  • matchOrder()         - 撮合主流程                 │    │
│  │  • mustPerformTakerOrderMatching() - 核心撮合逻辑   │    │
│  │  • GetOrderRemainingAmount() - 订单剩余量           │    │
│  │                                                      │    │
│  └──────────────┬───────────────────────────────────────┘    │
│                 │                                             │
│                 ▼                                             │
│  ┌──────────────────────────────────────────────────────┐    │
│  │              Orderbook (订单簿)                      │    │
│  ├──────────────────────────────────────────────────────┤    │
│  │                                                      │    │
│  │  Bids: map[Subticks]*Level   (买单)                 │    │
│  │  Asks: map[Subticks]*Level   (卖单)                 │    │
│  │  BestBid / BestAsk           (最优价格缓存)          │    │
│  │  orderIdToLevelOrder         (O(1)订单索引)         │    │
│  │                                                      │    │
│  └──────────────┬───────────────────────────────────────┘    │
│                 │                                             │
│                 ▼                                             │
│  ┌──────────────────────────────────────────────────────┐    │
│  │         ClobKeeper (状态管理器)                      │    │
│  ├──────────────────────────────────────────────────────┤    │
│  │                                                      │    │
│  │  • ProcessSingleMatch()     - 单笔撮合处理          │    │
│  │  • UpdateSubaccounts()      - 更新账户余额          │    │
│  │  • GetOrderFillAmount()     - 获取已成交量          │    │
│  │  • SetOrderFillAmount()     - 设置已成交量          │    │
│  │                                                      │    │
│  └──────────────┬───────────────────────────────────────┘    │
│                 │                                             │
│                 ▼                                             │
│  ┌──────────────────────────────────────────────────────┐    │
│  │      SubaccountsKeeper (抵押品检查)                  │    │
│  ├──────────────────────────────────────────────────────┤    │
│  │                                                      │    │
│  │  • CanUpdateSubaccounts()   - 抵押品验证            │    │
│  │  • GetNetCollateralAndMarginRequirements()          │    │
│  │  • UpdateSubaccounts()      - 执行余额变更          │    │
│  │                                                      │    │
│  └──────────────────────────────────────────────────────┘    │
│                                                                │
└────────────────────────────────────────────────────────────────┘
```

#### 撮合的一些设计原则

**Price-Time Priority (价格-时间优先)**

```Go
价格优先 + 时间优先 = 公平撮合

买单侧 (Bids):
  价格从高到低排列
  同价格按时间先后 (FIFO)
  
  50000 → [Order1 (09:00)] → [Order2 (09:01)] → [Order3 (09:05)]
  49999 → [Order4 (09:02)]
  49998 → [Order5 (09:03)]

卖单侧 (Asks):
  价格从低到高排列
  同价格按时间先后 (FIFO)
  
  50001 → [Order6 (09:00)] → [Order7 (09:02)]
  50002 → [Order8 (09:01)]
  50003 → [Order9 (09:04)]

撮合规则:
  买单与最低卖价匹配
  卖单与最高买价匹配
```

**Taker 主动匹配 Maker (Taker-Driven)**

```Go
Maker (挂单方):
  - 订单静止在订单簿中等待
  - 提供流动性
  - 通常获得 Maker Fee 折扣

Taker (吃单方):
  - 主动进入撮合流程
  - 消耗流动性
  - 支付 Taker Fee

流程:
  Taker订单 → 扫描对侧订单簿 → 匹配 Maker订单
```

**乐观执行 + 抵押品验证 (Optimistic Execution)**

```Go
阶段1: CheckTx (乐观撮合)
  ┌──────────────────────────┐
  │ 1. 快速匹配订单          │
  │ 2. 检查抵押品            │
  │ 3. 更新 MemClob          │
  │ 4. 不写入区块链状态      │
  └──────────────────────────┘
       │
       ▼ (成功则进入)
阶段2: DeliverTx (确定性执行)
  ┌──────────────────────────┐
  │ 1. 重新执行撮合          │
  │ 2. 重新检查抵押品        │
  │ 3. 持久化到链上状态      │
  │ 4. 确保所有节点一致      │
  └──────────────────────────┘
```

**原子性保证 (Atomicity)**

```Go
单笔撮合原子性:
  ┌─────────────────────────────┐
  │ CacheContext (分支状态)     │
  ├─────────────────────────────┤
  │ 1. 计算成交量               │
  │ 2. 检查 Maker 抵押品        │
  │ 3. 检查 Taker 抵押品        │
  │ 4. 更新余额                 │
  │ 5. 记录成交量               │
  ├─────────────────────────────┤
  │ 全部成功 → writeCache()     │
  │ 任何失败 → 丢弃所有更新     │
  └─────────────────────────────┘
```

#### 撮合流程时序图

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=Njk5ZDFhN2JmMjQyZTQyNWNiNmU0ZWYwYjUyYmI3NzJfUHJMZUZRc0ozUFFkS283QUdZZVhJdHIyOTRVNUNPUFZfVG9rZW46V3FNNGI3dHNzb2ZtOXN4R3ZySWxFTWVLZ3ZkXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

```Go
用户/交易  MemClob        Orderbook      ClobKeeper    SubaccountsKeeper   State
  │           │              │               │                │              │
  │ PlaceOrder│              │               │                │              │
  ├──────────►│              │               │                │              │
  │           │              │               │                │              │
  │           │ matchOrder() │               │                │              │
  │           ├─────────────►│               │                │              │
  │           │              │               │                │              │
  │           │ CacheContext │               │                │              │
  │           │◄─────────────┤               │                │              │
  │           │ (分支状态)    │               │                │              │
  │           │              │               │                │              │
  │           │ getBestOrderOnSide(!takerIsBuy)              │              │
  │           ├─────────────►│               │                │              │
  │           │◄─────────────┤               │                │              │
  │           │ (返回最优Maker)               │                │              │
  │           │              │               │                │              │
  │           │ 检查订单簿交叉│               │                │              │
  │           │ (价格匹配？)  │               │                │              │
  │           │              │               │                │              │
  │           │ 计算成交数量  │               │                │              │
  │           │              │               │                │              │
  │           │              │ ProcessSingleMatch()          │              │
  │           ├──────────────┼──────────────►│                │              │
  │           │              │               │                │              │
  │           │              │               │ CanUpdateSubaccounts(Maker) │
  │           │              │               ├───────────────►│              │
  │           │              │               │◄───────────────┤              │
  │           │              │               │ (Maker抵押品OK?)              │
  │           │              │               │                │              │
  │           │              │               │ CanUpdateSubaccounts(Taker) │
  │           │              │               ├───────────────►│              │
  │           │              │               │◄───────────────┤              │
  │           │              │               │ (Taker抵押品OK?)              │
  │           │              │               │                │              │
  │           │              │               │ UpdateSubaccounts()          │
  │           │              │               ├───────────────►│              │
  │           │              │               │                │ Write State │
  │           │              │               │                ├────────────►│
  │           │              │               │                │              │
  │           │◄─────────────┼───────────────┤                │              │
  │           │ (成交结果)    │               │                │              │
  │           │              │               │                │              │
  │           │ 记录MakerFill│               │                │              │
  │           │              │               │                │              │
  │           │ 更新订单剩余量│               │                │              │
  │           │              │               │                │              │
  │           │ findNextBestLevelOrder()     │                │              │
  │           ├─────────────►│               │                │              │
  │           │◄─────────────┤               │                │              │
  │           │ (继续下一个Maker)             │                │              │
  │           │              │               │                │              │
  │           │ [循环直到Taker完全成交或无更多Maker]            │              │
  │           │              │               │                │              │
  │           │ writeCache() │               │                │              │
  │           │ (提交分支状态)│               │                │              │
  │           │              │               │                │              │
  │◄──────────┤              │               │                │              │
  │ (返回成交结果)            │               │                │              │
```

#### 核心撮合循环流程图

![img](https://chainupgroup.sg.larksuite.com/space/api/box/stream/download/asynccode/?code=Yzk5ZWU1NzE3YTg2YjZkOTE3MTY4ZWI1NzVkNzhiZDVfeVE3UTRadzhkTHVUSVR1MTJKWnlObXZ5bm1MN0FuVXFfVG9rZW46V2N1YWI5d1Jsb3hJZzN4UHg1Z2xuSXJzZzNlXzE3NjcxNDY5NDU6MTc2NzE1MDU0NV9WNA)

```Go
mustPerformTakerOrderMatching() 详细流程

开始
  │
  ▼
┌──────────────────────────┐
│ 初始化变量               │
│ - takerRemainingSize     │
│ - newMakerFills = []     │
│ - makerOrdersToRemove=[] │
└────────┬─────────────────┘
         │
         ▼
    ┌────────┐
    │撮合循环 │ ◄────────────────────────┐
    └────┬───┘                           │
         │                               │
         ▼                               │
┌─────────────────────┐                  │
│ Step 1: 获取Maker   │                  │
│ getBestOrderOnSide()│                  │
└────────┬────────────┘                  │
         │                               │
    ┌────┴─────┐                         │
    │找到了？   │                         │
    └────┬─────┘                         │
         │                               │
    ┌────┴─────┐                         │
   NO          YES                       │
    │           │                        │
    ▼           ▼                        │
  结束    ┌─────────────────┐            │
         │ Step 2: 价格检查 │            │
         │ 订单簿交叉？     │            │
         └────────┬────────┘            │
                  │                      │
             ┌────┴─────┐               │
            NO          YES              │
             │           │               │
             ▼           ▼               │
           结束    ┌─────────────────┐   │
                  │ Step 3: 自成交？ │   │
                  │ 同一子账户？     │   │
                  └────────┬────────┘   │
                           │             │
                      ┌────┴─────┐      │
                     YES         NO      │
                      │           │      │
                      ▼           ▼      │
                 标记移除    ┌──────────────────┐
                  Maker     │ Step 4: 计算成交量│
                      │      │ min(Taker剩余,   │
                      │      │     Maker剩余)   │
                      │      └────────┬─────────┘
                      │               │          │
                      │               ▼          │
                      │      ┌──────────────────┐│
                      │      │ Step 5: Reduce-  ││
                      │      │ Only订单调整     ││
                      │      └────────┬─────────┘│
                      │               │          │
                      │               ▼          │
                      │      ┌──────────────────┐│
                      │      │ Step 6: 抵押品检查││
                      │      │ ProcessSingleMatch││
                      │      └────────┬─────────┘│
                      │               │          │
                      │          ┌────┴────┐    │
                      │         成功      失败   │
                      │          │         │     │
                      │          ▼         ▼     │
                      │   ┌──────────┐ ┌────────┐│
                      │   │记录成交  │ │标记移除││
                      │   │MakerFill │ │(Maker或││
                      │   └────┬─────┘ │Taker)  ││
                      │        │       └────┬───┘│
                      │        ▼            │    │
                      │   ┌──────────────┐ │    │
                      │   │更新剩余数量  │ │    │
                      │   │takerRemaining│ │    │
                      │   └────┬─────────┘ │    │
                      │        │           │    │
                      │   ┌────┴─────┐     │    │
                      │  完全成交？    │    │    │
                      │   └────┬─────┘     │    │
                      │        │           │    │
                      │   ┌────┴─────┐     │    │
                      │  YES        NO     │    │
                      │   │          │     │    │
                      │   ▼          │     │    │
                      │ 结束         │     │    │
                      │              │     │    │
                      └──────────────┼─────┴────┘
                                     │
                                     │
                                     └─────────┘
                                      (继续循环)
```

#### 示例理解

```SQL
初始状态: BTC/USDC 永续合约订单簿

Orderbook 状态 (时间: 09:00:00)
┌────────────────────────────────────────────────────────┐
│                     订单簿快照                          │
├────────────────────────────────────────────────────────┤
│                                                        │
│ 卖单 (Asks) - 价格从低到高                             │
│ ────────────────────────────────────────              │
│  50002: [Order #7] Sell 0.5 BTC @50002 (Eve)         │
│  50001: [Order #5] Sell 1.0 BTC @50001 (Carol)       │
│         [Order #6] Sell 1.5 BTC @50001 (Dave)        │
│                                                        │
│         ─────────── 市场价差 ───────────              │
│                                                        │
│ 买单 (Bids) - 价格从高到低                             │
│ ────────────────────────────────────────              │
│  50000: [Order #1] Buy 1.0 BTC @50000 (Alice)        │
│         [Order #2] Buy 2.0 BTC @50000 (Bob)          │
│  49999: [Order #3] Buy 1.5 BTC @49999 (Frank)        │
│  49998: [Order #4] Buy 3.0 BTC @49998 (Grace)        │
│                                                        │
└────────────────────────────────────────────────────────┘

最优价格:
  BestBid = 50000
  BestAsk = 50001
  Spread = 1 subtick
```

市价买单撮合 (Taker Buy)

```YAML
事件: 时间 09:00:05
用户 Mike 提交市价买单: Buy 3.0 BTC (Market Order)

═══════════════════════════════════════════════════════
撮合过程详解
═══════════════════════════════════════════════════════

Step 1: 订单进入 PlaceOrder()
─────────────────────────────────
输入:
  Order: Buy 3.0 BTC (Market Order)
  SubaccountId: mike_0
  OrderId: {mike_0, ClientId: 100, ...}

内部处理:
  takerIsBuy = true
  takerRemainingSize = 3.0 BTC
  takerIsLiquidation = false


Step 2: 查找对侧最优价格
─────────────────────────────────
调用: orderbook.getBestOrderOnSide(false)  // false = 卖单侧
返回: makerLevelOrder → Order #5 (Carol)

当前订单簿状态:
  Asks:
    50001: [Carol: 1.0 BTC] ← 当前 Maker
           [Dave:  1.5 BTC]
    50002: [Eve:   0.5 BTC]


Step 3: 检查价格交叉
─────────────────────────────────
Taker 是买单,检查:
  Market Order 隐含价格 = ∞ (无限高)
  Maker 价格 = 50001
  
判断: ∞ >= 50001 → True (订单簿交叉,可成交)


Step 4: 计算成交数量
─────────────────────────────────
Carol 订单剩余: 1.0 BTC
Mike  订单剩余: 3.0 BTC

fillAmount = min(1.0, 3.0) = 1.0 BTC


Step 5: 处理 Reduce-Only (本例不涉及,跳过)
─────────────────────────────────


Step 6: 抵押品检查
─────────────────────────────────
调用: ProcessSingleMatch(Mike买单, Carol卖单, 1.0 BTC)

6.1 计算费用:
    成交金额 = 1.0 BTC × 50001 = 50,001 USDC
    Taker Fee (Mike) = 50,001 × 0.05% = 25.00 USDC
    Maker Fee (Carol) = 50,001 × -0.02% = -10.00 USDC (rebate)

6.2 子账户更新 (Delta):
    Mike (Taker):
      BTC:  +1.0 BTC
      USDC: -50,001 USDC (成交金额)
            -25 USDC (手续费)
      ────────────────────
      Total: +1.0 BTC, -50,026 USDC
    
    Carol (Maker):
      BTC:  -1.0 BTC
      USDC: +50,001 USDC (成交金额)
            +10 USDC (maker rebate)
      ────────────────────
      Total: -1.0 BTC, +50,011 USDC

6.3 检查 Carol 抵押品 (Maker):
    调用: CanUpdateSubaccounts(carol_0, updates)
    
    更新前:
      BTC Position: 2.0 BTC long
      USDC Balance: 100,000 USDC
      NC = 200,000 USDC
      IMR = 20,000 USDC
      
    更新后 (模拟):
      BTC Position: 1.0 BTC long (减少1.0)
      USDC Balance: 150,011 USDC (增加50,011)
      NC = 200,011 USDC (略增)
      IMR = 10,000 USDC (减少)
      
    结果: makerCollatOkay = true ✓

6.4 检查 Mike 抵押品 (Taker):
    调用: CanUpdateSubaccounts(mike_0, updates)
    
    更新前:
      BTC Position: 0 BTC
      USDC Balance: 200,000 USDC
      NC = 200,000 USDC
      IMR = 0
      
    更新后 (模拟):
      BTC Position: 1.0 BTC long
      USDC Balance: 149,974 USDC (减少50,026)
      NC = 199,974 USDC (略减)
      IMR = 10,000 USDC (增加)
      
    结果: takerCollatOkay = true ✓

6.5 执行余额更新:
    调用: UpdateSubaccounts(ctx, updates)
    写入 CacheContext (暂存,未提交)


Step 7: 记录成交
─────────────────────────────────
newMakerFills.append({
  MakerOrderId: Order #5 (Carol)
  FillAmount: 1.0 BTC
  MakerSubaccountId: carol_0
})

更新本地变量:
  takerRemainingSize = 3.0 - 1.0 = 2.0 BTC
  Carol订单完全成交 → 从订单簿移除


Step 8: 继续查找下一个 Maker
─────────────────────────────────
调用: orderbook.findNextBestLevelOrder(Carol)
返回: makerLevelOrder → Order #6 (Dave)

当前订单簿状态:
  Asks:
    50001: [Dave: 1.5 BTC] ← 当前 Maker (Carol已成交)
    50002: [Eve:  0.5 BTC]


Step 9: 重复 Step 3-7 (第二轮撮合)
─────────────────────────────────
Taker剩余: 2.0 BTC
Maker (Dave): 1.5 BTC

fillAmount = min(2.0, 1.5) = 1.5 BTC

抵押品检查:
  Dave (Maker) ✓
  Mike (Taker) ✓

成交:
  Mike: +1.5 BTC, -75,039 USDC (含手续费)
  Dave: -1.5 BTC, +75,017 USDC

更新:
  takerRemainingSize = 2.0 - 1.5 = 0.5 BTC
  Dave订单完全成交 → 从订单簿移除


Step 10: 继续第三轮撮合
─────────────────────────────────
调用: orderbook.findNextBestLevelOrder(Dave)
返回: makerLevelOrder → Order #7 (Eve)

当前订单簿状态:
  Asks:
    50002: [Eve: 0.5 BTC] ← 当前 Maker (价格跳变!)

价格检查:
  Market Order ∞ >= 50002 → True (仍可成交)

fillAmount = min(0.5, 0.5) = 0.5 BTC

抵押品检查:
  Eve (Maker) ✓
  Mike (Taker) ✓

成交:
  Mike: +0.5 BTC, -25,013 USDC
  Eve:  -0.5 BTC, +25,006 USDC

更新:
  takerRemainingSize = 0.5 - 0.5 = 0 BTC
  Mike订单完全成交 → 撮合结束


Step 11: 提交状态
─────────────────────────────────
调用: writeCache()
将 CacheContext 中的所有更新提交到实际状态

最终结果:
  Mike 成交汇总:
    BTC: +3.0 BTC
    USDC: -150,078 USDC (成交金额 + 手续费)
    平均成交价: 50,026 USDC/BTC
    
  Maker成交:
    Carol: 1.0 BTC @50001
    Dave:  1.5 BTC @50001
    Eve:   0.5 BTC @50002


最终订单簿状态 (时间: 09:00:05.001)
─────────────────────────────────
Asks:
  50002: [] (Eve完全成交,Level空)
  
Bids:
  50000: [Alice: 1.0 BTC]
         [Bob:   2.0 BTC]
  49999: [Frank: 1.5 BTC]
  49998: [Grace: 3.0 BTC]

最优价格更新:
  BestBid = 50000 (不变)
  BestAsk = ∞ (无卖单)
```

抵押品不足导致部分成交

```YAML
事件: 时间 09:01:00
用户 Tom 提交市价卖单: Sell 5.0 BTC (但保证金不足)

═══════════════════════════════════════════════════════
撮合过程 (抵押品失败场景)
═══════════════════════════════════════════════════════

初始订单簿:
  Bids:
    50000: [Alice: 1.0 BTC]
           [Bob:   2.0 BTC]
    49999: [Frank: 1.5 BTC]

Tom 账户状态:
  BTC Position: 0 BTC
  USDC Balance: 50,000 USDC (不足以开5.0 BTC空头)
  NC = 50,000 USDC
  IMR (如果开5 BTC空头) ≈ 250,000 USDC (假设50k/BTC, 10倍杠杆)


撮合流程:
─────────────────────────────────

Round 1: Tom vs Alice
  fillAmount = min(5.0, 1.0) = 1.0 BTC
  
  抵押品检查:
    Alice (Maker): ✓ (买入BTC,有足够USDC)
    Tom   (Taker): ✓ (首笔成交,保证金够)
    
  成交: 1.0 BTC @50000
  Tom剩余: 4.0 BTC


Round 2: Tom vs Bob
  fillAmount = min(4.0, 2.0) = 2.0 BTC
  
  抵押品检查:
    Bob (Maker): ✓
    Tom (Taker): ✗ (累计3.0 BTC空头,保证金不足!)
    
    计算结果:
      更新后 BTC Position: -3.0 BTC
      更新后 USDC: 150,000 USDC
      NC = -150,000 + 150,000 = 0
      IMR ≈ 150,000 USDC
      NC < IMR → 抵押品不足!
  
  处理:
    takerCollatOkay = false
    takerOrderStatus.OrderStatus = Undercollateralized
    停止撮合,丢弃本轮更新


最终结果:
─────────────────────────────────
Tom 实际成交: 1.0 BTC @50000 (部分成交)
未成交部分: 4.0 BTC (因抵押品不足被拒绝)

订单簿状态:
  Bids:
    50000: [Bob: 2.0 BTC] (Bob未被撮合)
    49999: [Frank: 1.5 BTC]

返回给 Tom:
  OrderStatus: Undercollateralized
  FilledQuantums: 1.0 BTC
  RemainingQuantums: 4.0 BTC (未成交)
```

自成交预防 (Self-Trade Prevention)

```YAML
事件: Alice 同时有买单和卖单
  买单: Buy 2.0 BTC @50000
  卖单: Sell 1.0 BTC @49999 (限价单,价格交叉)

═══════════════════════════════════════════════════════
自成交预防机制
═══════════════════════════════════════════════════════

订单簿状态:
  Asks:
    49999: [Alice: 1.0 BTC] ← Alice的卖单
  Bids:
    50000: [Alice: 2.0 BTC] ← Alice的买单

Alice 提交新市价买单: Buy 0.5 BTC

撮合流程:
─────────────────────────────────

Step 1: 找到最优 Maker
  getBestOrderOnSide(false) → Alice的卖单 @49999

Step 2: 价格检查
  Market ∞ >= 49999 → True (可成交)

Step 3: 自成交检查
  Taker SubaccountId: alice_0
  Maker SubaccountId: alice_0
  
  判断: alice_0 == alice_0 → 自成交!

Step 4: 处理自成交
  标记 Maker订单移除:
    makerOrdersToRemove.append({
      Order: Alice的卖单
      RemovalReason: SELF_TRADE_ERROR
    })
  
  继续查找下一个 Maker...

Step 5: 找到非自成交 Maker
  findNextBestLevelOrder() → Bob的卖单 @50001
  
  继续正常撮合 Alice vs Bob


最终结果:
─────────────────────────────────
Alice的卖单被移除 (防止自成交)
Alice买单 vs Bob卖单正常成交
```

#### **抵押品检查机制**

```YAML
抵押品检查流程图
┌─────────────────────────────────────────────────────┐
│          ProcessSingleMatch 抵押品验证              │
└─────────────────────────────────────────────────────┘

输入: Taker订单, Maker订单, fillAmount
  │
  ▼
┌──────────────────────────────┐
│ 1. 计算子账户更新 Delta      │
│                              │
│  Maker Updates:              │
│    Position: ±fillAmount     │
│    USDC: ±(fillAmount×price) │
│    Fee: makerFee             │
│                              │
│  Taker Updates:              │
│    Position: ±fillAmount     │
│    USDC: ∓(fillAmount×price) │
│    Fee: takerFee             │
└────────┬─────────────────────┘
         │
         ▼
┌──────────────────────────────┐
│ 2. Maker 抵押品检查          │
│                              │
│ CanUpdateSubaccounts(        │
│   makerSubaccountId,         │
│   makerUpdates,              │
│   UpdateType.DEFAULT         │
│ )                            │
└────────┬─────────────────────┘
         │
    ┌────┴────┐
   失败      成功
    │         │
    ▼         ▼
┌─────────┐ ┌──────────────────────────────┐
│ 返回失败│ │ 3. Taker 抵押品检查          │
│ Maker   │ │                              │
│ 标记移除│ │ CanUpdateSubaccounts(        │
└─────────┘ │   takerSubaccountId,         │
            │   takerUpdates,              │
            │   UpdateType.MATCH           │
            │ )                            │
            └────────┬─────────────────────┘
                     │
                ┌────┴────┐
               失败      成功
                │         │
                ▼         ▼
            ┌─────────┐ ┌──────────────────┐
            │ 返回失败│ │ 4. 执行更新      │
            │ Taker   │ │                  │
            │ 停止撮合│ │ UpdateSubaccounts│
            └─────────┘ │ (写入CacheContext)│
                        └────────┬─────────┘
                                 │
                                 ▼
                        ┌──────────────────┐
                        │ 5. 记录成交量    │
                        │                  │
                        │ SetOrderFillAmount│
                        └────────┬─────────┘
                                 │
                                 ▼
                        ┌──────────────────┐
                        │ 返回成功         │
                        └──────────────────┘

抵押品计算公式:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
NC  = Net Collateral (净抵押品)
    = USDC Balance + Σ(Position × OraclePrice)

IMR = Initial Margin Requirement (初始保证金)
    = Σ(|Position| × OraclePrice × IMF)
    IMF = Initial Margin Fraction (如10% for 10x)

MMR = Maintenance Margin Requirement (维持保证金)
    = Σ(|Position| × OraclePrice × MMF)
    MMF = Maintenance Margin Fraction (如5% for 20x)

验证条件:
  开仓: NC >= IMR
  持仓: NC >= MMR
  平仓: 允许 (即使NC < MMR,平仓降低风险)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

#### 订单状态流转图

```Go
订单生命周期状态机
┌────────────────────────────────────────────────────┐
│                                                    │
│  ┌─────────┐                                      │
│  │ 提交订单 │                                      │
│  └────┬────┘                                      │
│       │                                            │
│       ▼                                            │
│  ┌─────────────────┐                              │
│  │ CheckTx验证     │                              │
│  │ - 签名验证      │                              │
│  │ - 基本参数检查  │                              │
│  └────┬────────────┘                              │
│       │                                            │
│  ┌────┴─────┐                                     │
│ 失败       成功                                     │
│  │          │                                     │
│  ▼          ▼                                     │
│拒绝   ┌──────────────┐                            │
│      │ 进入MemClob  │                            │
│      │ (内存订单簿)  │                            │
│      └──────┬───────┘                            │
│             │                                     │
│        ┌────┴─────┐                              │
│      短期订单   长期订单                           │
│        │          │                              │
│        ▼          ▼                              │
│  ┌─────────┐ ┌─────────────┐                    │
│  │立即撮合 │ │写入链上状态 │                    │
│  └────┬────┘ └──────┬──────┘                    │
│       │             │                            │
│       │             ▼                            │
│       │      ┌─────────────────┐                │
│       │      │PrepareCheckState│                │
│       │      │重建订单簿       │                │
│       │      └──────┬──────────┘                │
│       │             │                            │
│       └─────────────┘                            │
│             │                                     │
│             ▼                                     │
│      ┌──────────────┐                            │
│      │ 撮合尝试     │                            │
│      └──────┬───────┘                            │
│             │                                     │
│    ┌────────┼────────┬────────┐                 │
│    │        │        │        │                 │
│    ▼        ▼        ▼        ▼                 │
│ ┌──────┐┌──────┐┌──────┐┌────────┐             │
│ │完全  ││部分  ││未成交││失败    │             │
│ │成交  ││成交  ││      ││        │             │
│ └──┬───┘└──┬───┘└──┬───┘└───┬────┘             │
│    │       │       │        │                   │
│    ▼       ▼       ▼        ▼                   │
│ ┌──────────────────────────────┐               │
│ │ DeliverTx 确定性执行         │               │
│ │ - 重新撮合                    │               │
│ │ - 重新检查抵押品              │               │
│ │ - 持久化状态                  │               │
│ └──────────┬───────────────────┘               │
│            │                                     │
│            ▼                                     │
│      ┌──────────┐                               │
│      │ 最终状态 │                               │
│      └──────────┘                               │
│                                                  │
└────────────────────────────────────────────────┘
```

#### 性能优化策略

```Go
优化点汇总
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. O(1) 最优价格访问
   ┌──────────────────────────────┐
   │ BestBid / BestAsk 缓存       │
   │ - 插入时更新: O(1)           │
   │ - 删除时智能扫描: O(m)       │
   │ - 读取: O(1)                 │
   └──────────────────────────────┘

2. O(1) 订单查找
   ┌──────────────────────────────┐
   │ orderIdToLevelOrder 索引     │
   │ - 通过OrderId直接定位        │
   │ - 删除订单: O(1)             │
   │ - 修改订单: O(1)             │
   └──────────────────────────────┘

3. 双向链表维护时间优先
   ┌──────────────────────────────┐
   │ Level 内使用双向链表         │
   │ - 插入尾部: O(1)             │
   │ - 删除节点: O(1)             │
   │ - 遍历: O(k) k=同价格订单数  │
   └──────────────────────────────┘

4. CacheContext 原子性
   ┌──────────────────────────────┐
   │ 分支状态                     │
   │ - 撮合失败: 丢弃所有更新     │
   │ - 撮合成功: writeCache()     │
   │ - 避免部分更新导致不一致     │
   └──────────────────────────────┘

5. 批量操作优化
   ┌──────────────────────────────┐
   │ 单次撮合处理多个Maker        │
   │ - 减少函数调用开销           │
   │ - 提高缓存命中率             │
   │ - 降低状态切换次数           │
   └──────────────────────────────┘
```

### 是否支持自成交——不支持

**自成交检测逻辑**

```Go
// memclob.go 第1666-1676行
// 撮合循环中的自成交检测
if makerSubaccountId == takerSubaccountId {
    // 同一子账户不能自成交
    makerOrdersToRemove = append(
        makerOrdersToRemove,
        OrderWithRemovalReason{
            Order:         makerOrder.Order,
            RemovalReason: types.OrderRemoval_REMOVAL_REASON_INVALID_SELF_TRADE,
        },
    )
    continue // 跳过该Maker,继续查找下一个订单
}
```

**检测条件:**

- 判断: `Taker订单的子账户ID` == `Maker订单的子账户ID`
- 如果相同 → 自成交,拒绝撮合
- 移除maker订单

为什么移除 Maker 而不是 Taker?

```Go
设计理念: 保护流动性提供者

┌─────────────────────────────────────────┐
│ 方案对比                                │
├─────────────────────────────────────────┤
│                                         │
│ ❌ 移除 Taker (不推荐):                │
│   - 新订单立即被拒绝                    │
│   - 用户体验差                          │
│   - 可能损失交易机会                    │
│                                         │
│ ✅ 移除 Maker (dYdX采用):              │
│   - Maker 是旧订单,可能已过时           │
│   - Taker 是新订单,代表最新意图         │
│   - Taker 继续撮合,最大化成交机会       │
│   - 避免用户需要手动取消旧订单          │
│                                         │
└─────────────────────────────────────────┘
```

### CEX vs DEX 撮合、结算、账户体系对比分析

#### 撮合

**CEX 撮合引擎架构**

```Go
┌─────────────────────── CEX 撮合引擎 ───────────────────────┐
│                                                              │
│  ┌──────────────┐      ┌──────────────┐                    │
│  │   API 网关    │─────▶│  订单路由器   │                    │
│  │  (Load Bal)  │      │ (Sequencer)  │                    │
│  └──────────────┘      └───────┬──────┘                    │
│                                │                            │
│                                ▼                            │
│                    ┌───────────────────┐                    │
│                    │  核心撮合引擎      │◀──┐               │
│                    │  (In-Memory)     │   │               │
│                    │  ┌──────────────┐│   │               │
│                    │  │  Orderbook   ││   │               │
│                    │  │  (RB-Tree/   ││   │               │
│                    │  │   Hash+List) ││   │ 高频         │
│                    │  └──────────────┘│   │ 读写         │
│                    │  ┌──────────────┐│   │               │
│                    │  │ Match Engine ││   │               │
│                    │  │ (Price-Time) ││   │               │
│                    │  └──────────────┘│   │               │
│                    └─────────┬─────────┘   │               │
│                              │              │               │
│                              ▼              │               │
│                    ┌──────────────────┐    │               │
│                    │   成交推送队列    │    │               │
│                    │  (Kafka/Redis)   │    │               │
│                    └─────────┬────────┘    │               │
│                              │              │               │
│        ┌─────────────────────┼──────────────┘              │
│        │                     │                              │
│        ▼                     ▼                              │
│  ┌──────────┐        ┌──────────────┐                     │
│  │ 持久化层  │        │  结算服务     │                     │
│  │ (MySQL)  │        │ (异步处理)    │                     │
│  └──────────┘        └──────────────┘                     │
│                                                              │
│  特点:                                                      │
│  • 单线程撮合 (避免锁竞争)                                  │
│  • 内存操作 (ns 级延迟)                                     │
│  • 异步持久化 (先撮合后落盘)                                │
│  • 水平扩展 (按交易对分片)                                  │
└──────────────────────────────────────────────────────────┘
```

**DEX (dYdX) 撮合引擎架构**

```Go
┌────────────────── DEX 撮合引擎 (链上 + 链下混合) ──────────────────┐
│                                                                      │
│  ┌──────────────┐                                                  │
│  │   用户钱包    │                                                  │
│  │  (MetaMask)  │                                                  │
│  └───────┬──────┘                                                  │
│          │ 签名交易                                                 │
│          ▼                                                          │
│  ┌──────────────┐      ┌──────────────┐                          │
│  │  RPC 节点     │─────▶│  内存池       │                          │
│  │  (Validator) │      │  (Mempool)   │                          │
│  └──────────────┘      └───────┬──────┘                          │
│                                 │                                  │
│                                 ▼                                  │
│              ┌──────────────────────────────┐                     │
│              │  ABCI 生命周期 (Cosmos SDK)  │                     │
│              └──────────────┬───────────────┘                     │
│                             │                                      │
│  ┌──────────────────────────┼──────────────────────────┐         │
│  │ PrepareProposal          │                           │         │
│  │ ┌────────────────────────▼─────────────────────┐   │         │
│  │ │  步骤1: PrepareCheckState (链下预处理)       │   │         │
│  │ │  ┌─────────────────────────────────────────┐ │   │         │
│  │ │  │  MemClob (内存订单簿)                  │ │   │         │
│  │ │  │  ┌───────────────┬─────────────────┐  │ │   │         │
│  │ │  │  │ Bids (Buy)    │ Asks (Sell)     │  │ │   │         │
│  │ │  │  │ map[price]    │ map[price]      │  │ │   │         │
│  │ │  │  │ -> LinkedList │ -> LinkedList   │  │ │   │         │
│  │ │  │  └───────────────┴─────────────────┘  │ │   │         │
│  │ │  │                                         │ │   │         │
│  │ │  │  • 清算订单生成                         │ │   │         │
│  │ │  │  • 去杠杆操作                           │ │   │         │
│  │ │  │  • 长期订单过期清理                     │ │   │         │
│  │ │  │  • 订单撮合 (Price-Time Priority)      │ │   │         │
│  │ │  └─────────────────────────────────────────┘ │   │         │
│  │ │                                               │   │         │
│  │ │  输出: Operations[] (订单操作序列)            │   │         │
│  │ └───────────────────────────────────────────────┘   │         │
│  │                             │                        │         │
│  │                             ▼                        │         │
│  │  步骤2: 构建区块提案                                  │         │
│  │  ┌─────────────────────────────────────────────┐   │         │
│  │  │  打包 Operations + User Txs                  │   │         │
│  │  │  (放入 Block.Data)                           │   │         │
│  │  └─────────────────────────────────────────────┘   │         │
│  └──────────────────────────┬────────────────────────┘         │
│                              │                                    │
│  ┌───────────────────────────┼──────────────────────────┐       │
│  │ ProcessProposal (验证节点) │                          │       │
│  │                           ▼                          │       │
│  │  ┌─────────────────────────────────────────────┐    │       │
│  │  │  验证 Operations 有效性                      │    │       │
│  │  │  • 签名验证                                  │    │       │
│  │  │  • 重放 MemClob 撮合 (确定性验证)           │    │       │
│  │  │  • 抵押品检查                                │    │       │
│  │  └─────────────────────────────────────────────┘    │       │
│  │                           │                          │       │
│  │                           ▼ 共识投票                 │       │
│  └───────────────────────────┼──────────────────────────┘       │
│                              │                                    │
│  ┌───────────────────────────┼──────────────────────────┐       │
│  │ DeliverTx (最终执行)       │                          │       │
│  │                           ▼                          │       │
│  │  ┌─────────────────────────────────────────────┐    │       │
│  │  │  确定性状态更新                              │    │       │
│  │  │  ┌────────────────────────────────────────┐ │    │       │
│  │  │  │ KVStore (LevelDB/RocksDB)              │ │    │       │
│  │  │  │ ┌────────────┬────────────┬──────────┐ │ │    │       │
│  │  │  │ │ Subaccount │ Positions  │ Orders   │ │ │    │       │
│  │  │  │ │ Balances   │ (Perp)     │ (State)  │ │ │    │       │
│  │  │  │ └────────────┴────────────┴──────────┘ │ │    │       │
│  │  │  │                                         │ │    │       │
│  │  │  │ • 更新子账户余额                        │ │    │       │
│  │  │  │ • 更新持仓信息                          │ │    │       │
│  │  │  │ • 更新订单状态                          │ │    │       │
│  │  │  │ • 触发事件 (Indexer)                   │ │    │       │
│  │  │  └────────────────────────────────────────┘ │    │       │
│  │  └─────────────────────────────────────────────┘    │       │
│  └──────────────────────────────────────────────────────┘       │
│                                                                   │
│  特点:                                                           │
│  • 确定性执行 (所有节点相同结果)                                 │
│  • 区块时间约束 (~1s)                                            │
│  • 状态证明 (Merkle Tree)                                        │
│  • 无法水平扩展撮合 (共识瓶颈)                                   │
└───────────────────────────────────────────────────────────────────┘
```

维度CEXDEX (dYdX)设计原因执行模型单线程异步确定性同步区块链需要所有节点执行结果一致延迟微秒级 (µs)秒级 (~1s)共识协议开销 + 区块确认时间吞吐量100万+ TPS~2000 TPS区块大小限制 + 计算资源约束扩展性按交易对水平扩展单链垂直扩展区块链状态必须全局一致持久化异步批量写入同步写入区块需要状态证明 (Merkle Root)回滚复杂 (需补偿)简单 (交易失败)CacheContext 原子提交

#### 结算

CEX 结算流程

```YAML
┌─────────────────── CEX 结算流程 (T+0 实时结算) ────────────────────┐
│                                                                      │
│  时间轴: ───────────▶                                               │
│                                                                      │
│  T0: 订单撮合成功                                                   │
│  ┌──────────────────────────────────────────────┐                  │
│  │  撮合引擎                                     │                  │
│  │  OrderA(买100 BTC @ 50000)                  │                  │
│  │  OrderB(卖100 BTC @ 50000)                  │                  │
│  │  ─────────────────────────                   │                  │
│  │  Match! 成交价 50000, 数量 100               │                  │
│  └────────────┬─────────────────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  T0+1ms: 生成成交记录                                               │
│  ┌──────────────────────────────────────────────┐                  │
│  │  Trade {                                      │                  │
│  │    id: "trade_12345",                        │                  │
│  │    buyer: "user_A",                          │                  │
│  │    seller: "user_B",                         │                  │
│  │    price: 50000,                             │                  │
│  │    quantity: 100,                            │                  │
│  │    timestamp: 1638360000000                  │                  │
│  │  }                                            │                  │
│  └────────────┬─────────────────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  T0+2ms: 更新账户余额 (内存中)                                      │
│  ┌──────────────────────┬───────────────────────┐                  │
│  │  User A (买方)        │  User B (卖方)         │                  │
│  │  ┌────────────────┐  │  ┌────────────────┐  │                  │
│  │  │ BTC: +100      │  │  │ BTC: -100      │  │                  │
│  │  │ USDT: -5000000 │  │  │ USDT: +5000000 │  │                  │
│  │  │ (扣手续费 50)  │  │  │ (扣手续费 50)  │  │                  │
│  │  └────────────────┘  │  └────────────────┘  │                  │
│  └──────────────────────┴───────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  T0+5ms: 推送消息                                                   │
│  ┌──────────────────────────────────────────────┐                  │
│  │  Kafka/Redis MQ                               │                  │
│  │  ├─▶ WebSocket (实时行情)                     │                  │
│  │  ├─▶ 风控系统 (头寸监控)                      │                  │
│  │  ├─▶ 报表系统 (交易统计)                      │                  │
│  │  └─▶ 审计日志                                 │                  │
│  └────────────┬─────────────────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  T0+100ms: 异步持久化 (批量写入)                                    │
│  ┌──────────────────────────────────────────────┐                  │
│  │  MySQL (主库)                                 │                  │
│  │  ┌────────────┬─────────────┬──────────────┐ │                  │
│  │  │ trades     │ balances    │ order_book   │ │                  │
│  │  │ INSERT     │ UPDATE      │ UPDATE       │ │                  │
│  │  └────────────┴─────────────┴──────────────┘ │                  │
│  │                                               │                  │
│  │  • 批量提交 (1000笔/批)                       │                  │
│  │  • 异步复制到从库                             │                  │
│  │  • 定期归档冷数据                             │                  │
│  └───────────────────────────────────────────────┘                  │
│                                                                      │
│  关键特性:                                                          │
│  ✓ 内存先行: 撮合和结算都在内存完成                                 │
│  ✓ 最终一致性: 异步持久化可能短暂不一致                             │
│  ✓ 高性能: 微秒级响应                                               │
│  ✗ 单点故障: 内存数据丢失需从备份恢复                               │
│  ✗ 信任依赖: 用户需信任交易所不作恶                                 │
└──────────────────────────────────────────────────────────────────────┘
```

DEX (dYdX) 结算流程

```YAML
┌────────────── DEX 结算流程 (原子化链上结算) ──────────────────────┐
│                                                                      │
│  时间轴: ───────────▶ (~1s per block)                              │
│                                                                      │
│  Block N: PrepareCheckState (链下预处理)                            │
│  ┌──────────────────────────────────────────────┐                  │
│  │  MemClob 撮合引擎                             │                  │
│  │  ┌──────────────────────────────────────┐   │                  │
│  │  │ 1. 加载最新状态到内存                 │   │                  │
│  │  │    • 所有子账户余额                   │   │                  │
│  │  │    • 所有持仓信息                     │   │                  │
│  │  │    • 挂单订单簿                       │   │                  │
│  │  └──────────────────────────────────────┘   │                  │
│  │  ┌──────────────────────────────────────┐   │                  │
│  │  │ 2. 执行清算和去杠杆                   │   │                  │
│  │  │    • 检查抵押品不足账户               │   │                  │
│  │  │    • 生成清算订单                     │   │                  │
│  │  │    • 自动去杠杆配对                   │   │                  │
│  │  └──────────────────────────────────────┘   │                  │
│  │  ┌──────────────────────────────────────┐   │                  │
│  │  │ 3. 订单撮合 (Price-Time Priority)    │   │                  │
│  │  │    OrderA(买100 BTC-PERP @ 50000)    │   │                  │
│  │  │    OrderB(卖100 BTC-PERP @ 50000)    │   │                  │
│  │  │    ─────────────────────────          │   │                  │
│  │  │    Match! 生成 Operations[]          │   │                  │
│  │  └──────────────────────────────────────┘   │                  │
│  │  ┌──────────────────────────────────────┐   │                  │
│  │  │ 4. 抵押品验证 (CacheContext)          │   │                  │
│  │  │    • 检查买方有足够 USDC margin       │   │                  │
│  │  │    • 检查卖方有足够 Collateral        │   │                  │
│  │  │    • 计算资金费率影响                 │   │                  │
│  │  │    ✗ 失败则回滚此笔交易               │   │                  │
│  │  └──────────────────────────────────────┘   │                  │
│  └────────────┬─────────────────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  输出: Operations[] (确定性操作序列)                                 │
│  ┌──────────────────────────────────────────────┐                  │
│  │  [                                            │                  │
│  │    {type: "OrderMatch", makerOrderId: "A",   │                  │
│  │     takerOrderId: "B", fillAmount: 100},     │                  │
│  │    {type: "PerpetualFill", ...},             │                  │
│  │    {type: "SubaccountUpdate", ...}           │                  │
│  │  ]                                            │                  │
│  └────────────┬─────────────────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  共识验证: 其他验证节点重放相同逻辑                                  │
│  ┌──────────────────────────────────────────────┐                  │
│  │  ProcessProposal (每个验证节点)               │                  │
│  │  ├─▶ 重放 MemClob 撮合                        │                  │
│  │  ├─▶ 验证 Operations 正确性                   │                  │
│  │  ├─▶ 检查签名有效性                           │                  │
│  │  └─▶ 投票 Accept/Reject                       │                  │
│  │                                               │                  │
│  │  • 67%+ 验证节点同意 → 区块确认               │                  │
│  │  • 拜占庭容错 (BFT)                           │                  │
│  └────────────┬─────────────────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  DeliverTx: 最终状态更新 (原子化提交)                                │
│  ┌──────────────────────────────────────────────┐                  │
│  │  KVStore 状态更新                             │                  │
│  │  ┌─────────────────────────────────────────┐ │                  │
│  │  │ BEGIN TRANSACTION                       │ │                  │
│  │  │                                         │ │                  │
│  │  │ Subaccount_A:                          │ │                  │
│  │  │   QuoteBalance: 1000000 → 950000       │ │                  │
│  │  │   PerpPosition[BTC]: 0 → +100 (Long)   │ │                  │
│  │  │                                         │ │                  │
│  │  │ Subaccount_B:                          │ │                  │
│  │  │   QuoteBalance: 500000 → 550000        │ │                  │
│  │  │   PerpPosition[BTC]: 0 → -100 (Short)  │ │                  │
│  │  │                                         │ │                  │
│  │  │ OrderBook:                              │ │                  │
│  │  │   Remove OrderA, OrderB from book      │ │                  │
│  │  │                                         │ │                  │
│  │  │ COMMIT (原子化)                         │ │                  │
│  │  │ ├─▶ 计算 Merkle Root Hash              │ │                  │
│  │  │ └─▶ 更新 App Hash (状态证明)           │ │                  │
│  │  └─────────────────────────────────────────┘ │                  │
│  └────────────┬─────────────────────────────────┘                  │
│               │                                                     │
│               ▼                                                     │
│  Indexer 事件: 异步索引 (不影响共识)                                 │
│  ┌──────────────────────────────────────────────┐                  │
│  │  • 发送到 Kafka/gRPC Stream                   │                  │
│  │  • 更新 PostgreSQL (查询优化)                 │                  │
│  │  • WebSocket 推送给用户                       │                  │
│  └───────────────────────────────────────────────┘                  │
│                                                                      │
│  关键特性:                                                          │
│  ✓ 强一致性: 所有节点状态完全一致                                   │
│  ✓ 状态证明: Merkle Tree 可验证                                     │
│  ✓ 原子化: 整个区块要么全部成功,要么全部失败                         │
│  ✓ 去信任: 用户可自行验证状态                                       │
│  ✗ 低吞吐: 受限于区块大小和共识速度                                 │
│  ✗ 高延迟: ~1s 确认时间                                             │
└──────────────────────────────────────────────────────────────────────┘
```

维度CEXDEX (dYdX)为什么这样设计一致性模型最终一致性强一致性区块链状态需要全局验证,不能有分叉原子性应用层保证 (补偿事务)协议层保证 (ACID)CacheContext 提供事务语义结算时间微秒级秒级 (~1s)需要共识确认 (BFT)回滚机制复杂 (需人工介入)简单 (交易失败自动回滚)CacheContext.Write() 失败即回滚状态证明无 (需信任 CEX)Merkle Root (可自证)去信任化核心需求并发控制乐观锁/悲观锁串行化执行确定性要求,不能有并发竞态

#### 账户

CEX 账户体系

```SQL
┌──────────────────── CEX 账户体系 (中心化托管) ────────────────────┐
│                                                                     │
│  用户视角:                                                         │
│  ┌────────────────────────────────────────────────────────┐       │
│  │  用户 (user_12345)                                      │       │
│  │  ├─ 邮箱/手机认证                                       │       │
│  │  ├─ 2FA 二次验证                                        │       │
│  │  └─ API Key (可选)                                      │       │
│  └───────────────────────┬────────────────────────────────┘       │
│                          │                                         │
│                          ▼                                         │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  账户层级结构                                             │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │  主账户 (Main Account)                             │  │    │
│  │  │  ├─ 资金账户 (Funding)                             │  │    │
│  │  │  │  ├─ BTC: 10.5                                  │  │    │
│  │  │  │  ├─ ETH: 50.0                                  │  │    │
│  │  │  │  └─ USDT: 100000                               │  │    │
│  │  │  │                                                 │  │    │
│  │  │  ├─ 现货账户 (Spot Trading)                        │  │    │
│  │  │  │  ├─ 可用余额: BTC 2.0, USDT 50000              │  │    │
│  │  │  │  └─ 冻结余额: BTC 0.5 (挂单占用)               │  │    │
│  │  │  │                                                 │  │    │
│  │  │  ├─ 合约账户 (Futures Trading)                     │  │    │
│  │  │  │  ├─ 账户权益: 100000 USDT                      │  │    │
│  │  │  │  ├─ 已用保证金: 20000 USDT                     │  │    │
│  │  │  │  ├─ 可用保证金: 80000 USDT                     │  │    │
│  │  │  │  ├─ 未实现盈亏: +5000 USDT                     │  │    │
│  │  │  │  └─ 持仓:                                       │  │    │
│  │  │  │     ├─ BTC-PERP: Long 10 BTC @ 50000          │  │    │
│  │  │  │     └─ ETH-PERP: Short 100 ETH @ 3000         │  │    │
│  │  │  │                                                 │  │    │
│  │  │  └─ 理财账户 (Earn)                                │  │    │
│  │  │     ├─ 活期: USDT 10000 (年化 5%)                 │  │    │
│  │  │     └─ 定期: BTC 1.0 (锁定 90天)                  │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  └──────────────────────────────────────────────────────────┘    │
│                          │                                         │
│                          ▼                                         │
│  数据库存储结构:                                                   │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  MySQL Sharding (按用户ID分片)                           │    │
│  │  ┌──────────────┬──────────────┬──────────────────────┐  │    │
│  │  │ accounts     │ balances     │ positions            │  │    │
│  │  ├──────────────┼──────────────┼──────────────────────┤  │    │
│  │  │ user_id (PK) │ user_id      │ user_id              │  │    │
│  │  │ account_type │ account_type │ symbol               │  │    │
│  │  │ status       │ asset_id     │ side (LONG/SHORT)    │  │    │
│  │  │ risk_level   │ available    │ quantity             │  │    │
│  │  │ ...          │ frozen       │ entry_price          │  │    │
│  │  │              │ updated_at   │ margin               │  │    │
│  │  │              │              │ unrealized_pnl       │  │    │
│  │  └──────────────┴──────────────┴──────────────────────┘  │    │
│  │                                                           │    │
│  │  • 主键: user_id + account_type + asset_id/symbol        │    │
│  │  • 索引: (user_id, account_type), (asset_id)             │    │
│  │  • 分片策略: user_id % 16 (按用户分散)                    │    │
│  └──────────────────────────────────────────────────────────┘    │
│                          │                                         │
│                          ▼                                         │
│  资金流转:                                                         │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  用户充值 (Deposit)                                       │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 1. 用户转账到 CEX 热钱包地址                       │  │    │
│  │  │ 2. 链上确认 (BTC 6确认, ETH 12确认)               │  │    │
│  │  │ 3. 更新数据库 balances 表                          │  │    │
│  │  │    UPDATE balances SET available = available + X  │  │    │
│  │  │ 4. 发送确认通知                                    │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  账户间转账 (Internal Transfer)                          │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 资金账户 → 合约账户:                               │  │    │
│  │  │ BEGIN TRANSACTION;                                 │  │    │
│  │  │   UPDATE balances SET available = available - X    │  │    │
│  │  │   WHERE user_id = ? AND account_type = 'funding';  │  │    │
│  │  │   UPDATE balances SET available = available + X    │  │    │
│  │  │   WHERE user_id = ? AND account_type = 'futures';  │  │    │
│  │  │ COMMIT;                                             │  │    │
│  │  │                                                     │  │    │
│  │  │ • 原子性保证: 数据库事务                           │  │    │
│  │  │ • 实时到账 (毫秒级)                                │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  用户提现 (Withdrawal)                                    │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 1. 用户提交提现请求                                │  │    │
│  │  │ 2. 风控审核 (反洗钱/大额审批)                      │  │    │
│  │  │ 3. 冻结金额:                                       │  │    │
│  │  │    UPDATE balances SET                             │  │    │
│  │  │      available = available - X,                    │  │    │
│  │  │      frozen = frozen + X                           │  │    │
│  │  │ 4. 冷钱包签名转账 (人工/多签)                      │  │    │
│  │  │ 5. 链上交易确认后扣除 frozen                       │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                     │
│  关键特性:                                                         │
│  ✓ 灵活性: 多账户类型,自由划转                                    │
│  ✓ 高性能: 内部转账毫秒级                                          │
│  ✓ 隔离性: 不同业务账户独立管理                                    │
│  ✗ 托管风险: 用户不掌握私钥                                        │
│  ✗ 透明度: 储备金证明依赖审计                                      │
│  ✗ 监管风险: 需遵守 KYC/AML                                        │
└─────────────────────────────────────────────────────────────────────┘
```

DEX (dYdX) 账户体系

```YAML
┌─────────────── DEX 账户体系 (非托管 + 链上证明) ──────────────────┐
│                                                                     │
│  用户视角:                                                         │
│  ┌────────────────────────────────────────────────────────┐       │
│  │  用户钱包 (0xABC...DEF)                                 │       │
│  │  ├─ 私钥由用户掌握 (MetaMask/Ledger)                   │       │
│  │  ├─ 主地址: 用于身份验证和充值                         │       │
│  │  └─ 签名能力: 签署交易和订单                           │       │
│  └───────────────────────┬────────────────────────────────┘       │
│                          │                                         │
│                          ▼                                         │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Subaccount 体系 (链上存储)                              │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │  Subaccount {                                      │  │    │
│  │  │    Owner: 0xABC...DEF,                            │  │    │
│  │  │    Number: 0,  // 子账户编号 (0-127)             │  │    │
│  │  │    AssetPositions: [                              │  │    │
│  │  │      {AssetId: 0, Quantums: 1000000000000}  // USDC │  │    │
│  │  │    ],                                              │  │    │
│  │  │    PerpetualPositions: [                          │  │    │
│  │  │      {                                             │  │    │
│  │  │        PerpetualId: 0,  // BTC-USD                │  │    │
│  │  │        Quantums: 1000000000,  // +10 BTC (Long)   │  │    │
│  │  │        FundingIndex: 500000                       │  │    │
│  │  │      },                                            │  │    │
│  │  │      {                                             │  │    │
│  │  │        PerpetualId: 1,  // ETH-USD                │  │    │
│  │  │        Quantums: -10000000000,  // -100 ETH (Short)│ │    │
│  │  │        FundingIndex: 300000                       │  │    │
│  │  │      }                                             │  │    │
│  │  │    ]                                               │  │    │
│  │  │  }                                                 │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  • Owner + Number 唯一确定子账户                          │    │
│  │  • 每个用户可创建 128 个子账户 (Number: 0-127)            │    │
│  │  • 资产和持仓聚合在子账户级别                             │    │
│  └──────────────────────────────────────────────────────────┘    │
│                          │                                         │
│                          ▼                                         │
│  链上存储结构 (KVStore):                                           │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Key-Value Store (Merkle Tree)                           │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ Key: "subaccount/{owner}/{number}"                 │  │    │
│  │  │ Value: Protobuf(Subaccount)                        │  │    │
│  │  │                                                     │  │    │
│  │  │ 示例:                                               │  │    │
│  │  │ Key: "subaccount/0xABC...DEF/0"                    │  │    │
│  │  │ Value: {                                            │  │    │
│  │  │   AssetPositions: [...],                           │  │    │
│  │  │   PerpetualPositions: [...]                        │  │    │
│  │  │ }                                                   │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  Merkle Tree 结构:                                        │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │           Root Hash (App Hash)                     │  │    │
│  │  │                 /         \                         │  │    │
│  │  │        Branch1              Branch2                │  │    │
│  │  │        /    \               /     \                │  │    │
│  │  │   Leaf1   Leaf2        Leaf3    Leaf4              │  │    │
│  │  │  (SA-1)   (SA-2)       (SA-3)   (SA-4)             │  │    │
│  │  │                                                     │  │    │
│  │  │  • 每个区块生成新 Root Hash                        │  │    │
│  │  │  • 用户可验证自己账户在链上                        │  │    │
│  │  │  • Merkle Proof 提供状态证明                       │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  └──────────────────────────────────────────────────────────┘    │
│                          │                                         │
│                          ▼                                         │
│  资金流转:                                                         │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  充值 (Deposit) - 跨链桥                                  │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 1. 用户在以太坊锁定 USDC 到智能合约:              │  │    │
│  │  │    Lock(0xABC...DEF, 10000 USDC)                   │  │    │
│  │  │                                                     │  │    │
│  │  │ 2. 中继器监听事件并在 dYdX 链提交证明:            │  │    │
│  │  │    MsgDepositToSubaccount {                        │  │    │
│  │  │      Sender: "bridge_module",                      │  │    │
│  │  │      Recipient: SubaccountId{                      │  │    │
│  │  │        Owner: "0xABC...DEF",                       │  │    │
│  │  │        Number: 0                                   │  │    │
│  │  │      },                                             │  │    │
│  │  │      AssetId: 0,  // USDC                          │  │    │
│  │  │      Quantums: 10000000000  // 10000 USDC         │  │    │
│  │  │    }                                                │  │    │
│  │  │                                                     │  │    │
│  │  │ 3. 链上更新子账户余额 (DeliverTx):                │  │    │
│  │  │    Subaccount.AssetPositions[0].Quantums += X     │  │    │
│  │  │                                                     │  │    │
│  │  │ • 跨链延迟: 以太坊 12确认 + dYdX 区块确认 (~3分钟) │  │    │
│  │  │ • 去信任: 智能合约锁定,无需信任交易所             │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  子账户间转账 (Internal Transfer)                        │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 交易消息 (用户签名):                               │  │    │
│  │  │ MsgTransfer {                                      │  │    │
│  │  │   Sender: SubaccountId{0xABC...DEF, 0},           │  │    │
│  │  │   Recipient: SubaccountId{0xABC...DEF, 1},        │  │    │
│  │  │   AssetId: 0,                                      │  │    │
│  │  │   Amount: 1000000000  // 1000 USDC                │  │    │
│  │  │ }                                                   │  │    │
│  │  │                                                     │  │    │
│  │  │ 链上执行 (原子化):                                 │  │    │
│  │  │ CanUpdateSubaccounts([                             │  │    │
│  │  │   {SubaccountId: SA-0, Delta: -1000},            │  │    │
│  │  │   {SubaccountId: SA-1, Delta: +1000}             │  │    │
│  │  │ ])                                                  │  │    │
│  │  │ → 检查 SA-0 抵押品充足                            │  │    │
│  │  │ → UpdateSubaccounts() 原子更新                    │  │    │
│  │  │                                                     │  │    │
│  │  │ • 实时到账: 1个区块 (~1s)                          │  │    │
│  │  │ • 手续费: Gas 费 (USDC 支付)                      │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  提现 (Withdrawal) - 跨链桥                              │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 1. 用户签名提现消息:                               │  │    │
│  │  │    MsgWithdrawFromSubaccount {                     │  │    │
│  │  │      Sender: SubaccountId{0xABC...DEF, 0},        │  │    │
│  │  │      Recipient: "0xABC...DEF",                     │  │    │
│  │  │      AssetId: 0,                                   │  │    │
│  │  │      Quantums: 5000000000  // 5000 USDC          │  │    │
│  │  │    }                                                │  │    │
│  │  │                                                     │  │    │
│  │  │ 2. 链上验证并扣款:                                 │  │    │
│  │  │    • 检查余额充足                                  │  │    │
│  │  │    • 检查未平仓持仓的保证金要求                    │  │    │
│  │  │    • 扣除 Subaccount.AssetPositions[0].Quantums   │  │    │
│  │  │                                                     │  │    │
│  │  │ 3. 桥接模块解锁以太坊资金:                         │  │    │
│  │  │    Unlock(0xABC...DEF, 5000 USDC)                  │  │    │
│  │  │                                                     │  │    │
│  │  │ • 提现延迟: dYdX确认 + 以太坊交易 (~2-5分钟)      │  │    │
│  │  │ • 无需审批: 智能合约自动执行                      │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  └──────────────────────────────────────────────────────────┘    │
│                          │                                         │
│                          ▼                                         │
│  抵押品池隔离 (Isolated vs Cross Margin):                          │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Cross Margin Pool (跨仓保证金)                           │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 模块账户: "subaccounts" (authtypes.ModuleAddress)  │  │    │
│  │  │ 用途: 存储所有跨仓市场的抵押品                     │  │    │
│  │  │                                                     │  │    │
│  │  │ 示例:                                               │  │    │
│  │  │ BTC-PERP (Cross), ETH-PERP (Cross)                │  │    │
│  │  │ → 所有用户的 USDC 存在此模块账户                  │  │    │
│  │  │ → 风险共享: 一个仓位爆仓可能影响其他仓位         │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  Isolated Margin Pool (逐仓保证金)                       │    │
│  │  ┌────────────────────────────────────────────────────┐  │    │
│  │  │ 模块账户: "subaccounts:{perpetual_id}"              │  │    │
│  │  │ 用途: 单独存储逐仓市场的抵押品                     │  │    │
│  │  │                                                     │  │    │
│  │  │ 示例:                                               │  │    │
│  │  │ DOGE-PERP (Isolated, ID=5)                        │  │    │
│  │  │ → 模块账户: "subaccounts:5"                        │  │    │
│  │  │ → 风险隔离: 爆仓只影响该市场,不波及其他仓位       │  │    │
│  │  └────────────────────────────────────────────────────┘  │    │
│  │                                                           │    │
│  │  设计原因:                                                │    │
│  │  • 高风险/长尾资产使用逐仓,防止系统性风险             │    │
│  │  • 蓝筹资产使用跨仓,提高资金利用率                     │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                     │
│  关键特性:                                                         │
│  ✓ 非托管: 用户控制私钥                                            │
│  ✓ 透明性: 所有状态可验证 (Merkle Proof)                           │
│  ✓ 原子化: 转账/交易要么全成功要么全失败                            │
│  ✓ 抗审查: 无法冻结用户资金 (协议层保护)                           │
│  ✗ 复杂性: 用户需管理私钥和 Gas 费                                 │
│  ✗ 跨链延迟: 充值/提现需等待跨链桥确认                             │
│  ✗ 灵活性: 无法实现某些 CEX 功能(如信用卡购买)                     │
└─────────────────────────────────────────────────────────────────────┘
```

维度CEXDEX (dYdX)为什么这样设计资产托管中心化托管 (热/冷钱包)用户自持 (钱包私钥)去信任化核心:用户掌控资产账户结构多层级 (主/子/理财)Subaccount (Owner+Number)简化链上存储,聚合计算数据存储SQL 数据库 (分片)KVStore + Merkle Tree状态证明需求,轻客户端验证充值速度快 (确认后立即可用)慢 (跨链桥延迟)跨链安全性 vs 速度权衡内部转账毫秒级 (数据库事务)秒级 (区块确认)共识开销提现审核需要 (人工/自动风控)无需 (智能合约自动)去中心化无需许可隐私性弱 (KYC/监管)强 (伪匿名地址)但链上透明,可追踪保证金模式灵活配置Cross/Isolated 二选一协议层硬编码,降低复杂度资产证明需第三方审计链上可验证 (Merkle Proof)透明化储备金

#### 数据流对比图

```YAML
┌────────────────────── CEX 完整交易流 (性能优先) ──────────────────────┐
│                                                                          │
│  时间线: T0 ────────────▶ T0+100ms                                      │
│                                                                          │
│  ① 用户下单                                                             │
│  ┌─────────────┐                                                        │
│  │   用户 A     │  POST /api/orders                                     │
│  │ (买100 BTC) │ ────────────────────────▶ ┌─────────────────────┐    │
│  └─────────────┘                           │   API Gateway       │    │
│                                             │  (Nginx/LB)         │    │
│  ┌─────────────┐                           └──────────┬──────────┘    │
│  │   用户 B     │  POST /api/orders                   │               │
│  │ (卖100 BTC) │ ────────────────────────────────────▶│               │
│  └─────────────┘                                      │               │
│                                                        │               │
│  ② 订单路由与序列化                                   ▼               │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │  Order Sequencer (单线程保证顺序)                            │    │
│  │  ┌────────────┬────────────┬────────────┐                    │    │
│  │  │ Seq: 1001  │ Seq: 1002  │ Seq: 1003  │                    │    │
│  │  │ OrderA     │ OrderB     │ OrderC     │                    │    │
│  │  │ (Buy)      │ (Sell)     │ (...)      │                    │    │
│  │  └────────────┴────────────┴────────────┘                    │    │
│  │  • 分配全局递增序列号                                         │    │
│  │  • 路由到对应交易对引擎                                       │    │
│  └────────────┬─────────────────────────────────────────────────┘    │
│               │                                                        │
│               ▼                                                        │
│  ③ 内存撮合引擎 (单线程/单交易对)                                     │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │  BTC-USDT Matching Engine (Lock-Free)                        │    │
│  │                                                                │    │
│  │  Orderbook (In-Memory):                                       │    │
│  │  ┌───────────────────┬───────────────────┐                   │    │
│  │  │ Bids (Buy Orders) │ Asks (Sell Orders)│                   │    │
│  │  ├───────────────────┼───────────────────┤                   │    │
│  │  │ 50000: [Order1]   │ 50000: [OrderB]   │ ◀─ 价格匹配      │    │
│  │  │ 49999: [Order3]   │ 50001: [Order5]   │                   │    │
│  │  │ 49998: [OrderA]   │ 50002: [Order7]   │ ◀─ 新订单插入    │    │
│  │  └───────────────────┴───────────────────┘                   │    │
│  │                                                                │    │
│  │  Matching Logic:                                              │    │
│  │  1. OrderA 进入 Bids at 50000                                │    │
│  │  2. OrderB 进入 Asks at 50000                                │    │
│  │  3. Match! 生成成交:                                          │    │
│  │     Trade {                                                    │    │
│  │       MakerOrderId: OrderA,                                   │    │
│  │       TakerOrderId: OrderB,                                   │    │
│  │       Price: 50000,                                           │    │
│  │       Quantity: 100,                                          │    │
│  │       Timestamp: now()                                        │    │
│  │     }                                                          │    │
│  └────────────┬─────────────────────────────────────────────────┘    │
│               │                                                        │
│               ▼                                                        │
│  ④ 发送到消息队列 (异步处理)                                          │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │  Kafka Topics                                                 │    │
│  │  ┌────────────┬────────────┬────────────┬────────────┐       │    │
│  │  │ trades     │ balances   │ notifications│ market_data│      │    │
│  │  │ (成交记录) │ (余额更新) │ (推送通知)   │ (行情数据) │      │    │
│  │  └──────┬─────┴──────┬─────┴──────┬──────┴──────┬─────┘       │    │
│  └─────────┼────────────┼────────────┼─────────────┼─────────────┘    │
│            │            │            │             │                  │
│  ⑤ 多消费者并行处理    │            │             │                  │
│            │            │            │             │                  │
│      ┌─────▼────┐ ┌────▼─────┐ ┌───▼────┐   ┌────▼─────┐            │
│      │ Balance  │ │Persistence││WebSocket│   │ Analytics│            │
│      │ Service  │ │ Service   ││ Service │   │ Service  │            │
│      └─────┬────┘ └────┬──────┘ └───┬────┘   └──────────┘            │
│            │           │            │                                 │
│            ▼           ▼            ▼                                 │
│  ┌──────────────┐ ┌─────────┐ ┌───────────┐                         │
│  │  Redis Cache │ │  MySQL  │ │ WebSocket │                         │
│  │ (实时余额)   │ │(持久化) │ │(实时推送) │                         │
│  │              │ │         │ │           │                         │
│  │ UserA:       │ │ trades  │ │ → 用户A:  │                         │
│  │  BTC: +100   │ │ INSERT  │ │   "成交!" │                         │
│  │  USDT: -5M   │ │         │ │           │                         │
│  │              │ │ balances│ │ → 用户B:  │                         │
│  │ UserB:       │ │ UPDATE  │ │   "成交!" │                         │
│  │  BTC: -100   │ │         │ │           │                         │
│  │  USDT: +5M   │ │         │ │           │                         │
│  └──────────────┘ └─────────┘ └───────────┘                         │
│                                                                        │
│  ⑥ 用户查询 (Redis优先)                                                │
│  ┌─────────────┐   GET /api/balance                                  │
│  │   用户 A     │ ──────────────────▶ Redis (Cache Hit) ──▶ 返回    │
│  └─────────────┘                     │                               │
│                                      │ Cache Miss                    │
│                                      ▼                                │
│                                    MySQL (Fallback)                   │
│                                                                        │
│  延迟对比:                                                            │
│  • 撮合完成: T0 + 0.1ms (内存操作)                                    │
│  • Redis更新: T0 + 2ms (网络 + 内存写)                                │
│  • 用户推送: T0 + 5ms (WebSocket)                                     │
│  • MySQL持久化: T0 + 100ms (批量异步写盘)                             │
│                                                                        │
│  优点: ⚡ 极致性能 (微秒级), 📊 灵活扩展 (按交易对分片)              │
│  缺点: ⚠️ 最终一致性, 🔒 需复杂补偿逻辑, 🏦 托管风险               │
└──────────────────────────────────────────────────────────────────────┘
┌─────────────── DEX 完整交易流 (一致性优先) ──────────────────────────┐
│                                                                          │
│  时间线: Block N ────────▶ Block N+1 (~1s)                              │
│                                                                          │
│  ① 用户签名订单 (链下)                                                  │
│  ┌─────────────┐                      ┌─────────────┐                  │
│  │   用户 A     │  私钥签名             │   用户 B     │                  │
│  │ (买100 BTC) │  PlaceOrder Tx       │ (卖100 BTC) │                  │
│  └──────┬──────┘                      └──────┬──────┘                  │
│         │                                    │                          │
│         │  gRPC / REST API                   │                          │
│         ▼                                    ▼                          │
│  ┌─────────────────────────────────────────────────────────────┐      │
│  │  Validator Node (全节点)                                     │      │
│  │  ┌───────────────────────────────────────────────────────┐  │      │
│  │  │  Mempool (未确认交易池)                               │  │      │
│  │  │  ┌──────────┬──────────┬──────────┬──────────┐       │  │      │
│  │  │  │ TxA      │ TxB      │ TxC      │ TxD      │       │  │      │
│  │  │  │ (买单)   │ (卖单)   │ (转账)   │ (...)    │       │  │      │
│  │  │  └──────────┴──────────┴──────────┴──────────┘       │  │      │
│  │  │  • 交易签名验证                                       │  │      │
│  │  │  • Gas 费检查                                         │  │      │
│  │  │  • 防重放 (Nonce)                                     │  │      │
│  │  └───────────────────────────────────────────────────────┘  │      │
│  └──────────────────────────┬──────────────────────────────────┘      │
│                             │                                          │
│  ② PrepareProposal (提议者构建区块)                                     │
│                             ▼                                          │
│  ┌─────────────────────────────────────────────────────────────┐      │
│  │  PrepareCheckState (链下预处理 - 在内存中)                  │      │
│  │  ┌───────────────────────────────────────────────────────┐  │      │
│  │  │ Step 1: 加载链上最新状态到 MemClob                   │  │      │
│  │  │  • 所有子账户 (Subaccounts)                          │  │      │
│  │  │  • 所有持仓 (Positions)                              │  │      │
│  │  │  • 所有挂单 (Open Orders)                            │  │      │
│  │  │  • 资金费率 (Funding Rates)                          │  │      │
│  │  └───────────────────────────────────────────────────────┘  │      │
│  │  ┌───────────────────────────────────────────────────────┐  │      │
│  │  │ Step 2-7: 清算和去杠杆 (自动风控)                    │  │      │
│  │  │  for each Subaccount:                                 │  │      │
│  │  │    if 抵押品不足(TNC < MaintenanceMargin):          │  │      │
│  │  │      生成清算订单(LiquidationOrder)                  │  │      │
│  │  │      放入 MemClob 撮合                                │  │      │
│  │  │    if 破产(TNC < 0):                                  │  │      │
│  │  │      触发去杠杆(Deleveraging)                        │  │      │
│  │  │      与对手方配对平仓                                │  │      │
│  │  └───────────────────────────────────────────────────────┘  │      │
│  │  ┌───────────────────────────────────────────────────────┐  │      │
│  │  │ Step 8: 订单撮合 (确定性执行)                        │  │      │
│  │  │                                                        │  │      │
│  │  │  MemClob Orderbook:                                   │  │      │
│  │  │  ┌─────────────────┬─────────────────┐               │  │      │
│  │  │  │ Bids            │ Asks            │               │  │      │
│  │  │  ├─────────────────┼─────────────────┤               │  │      │
│  │  │  │ 50000: [OrderA] │ 50000: [OrderB] │ ◀─ 匹配!    │  │      │
│  │  │  │ 49999: [...]    │ 50001: [...]    │               │  │      │
│  │  │  └─────────────────┴─────────────────┘               │  │      │
│  │  │                                                        │  │      │
│  │  │  Matching Process:                                    │  │      │
│  │  │  1. 处理 TxA (PlaceOrder - 买单)                     │  │      │
│  │  │     • 检查子账户抵押品充足                           │  │      │
│  │  │     • 加入 Bids at 50000                             │  │      │
│  │  │                                                        │  │      │
│  │  │  2. 处理 TxB (PlaceOrder - 卖单)                     │  │      │
│  │  │     • 检查子账户抵押品充足                           │  │      │
│  │  │     • 尝试与 Bids 撮合                               │  │      │
│  │  │     • Match! OrderA vs OrderB                        │  │      │
│  │  │                                                        │  │      │
│  │  │  3. 验证成交后状态 (CacheContext)                    │  │      │
│  │  │     CanUpdateSubaccounts([                            │  │      │
│  │  │       {SubaccountA: QuoteBalance -= 5000000,        │  │      │
│  │  │                    PerpPosition[BTC] += 100},       │  │      │
│  │  │       {SubaccountB: QuoteBalance += 5000000,        │  │      │
│  │  │                    PerpPosition[BTC] -= 100}        │  │      │
│  │  │     ])                                                │  │      │
│  │  │     ✓ 通过 → 生成 Operation                          │  │      │
│  │  │     ✗ 失败 → 回滚此笔交易,继续下一笔                │  │      │
│  │  └───────────────────────────────────────────────────────┘  │      │
│  │  ┌───────────────────────────────────────────────────────┐  │      │
│  │  │ Step 9: 生成 Operations[] (确定性操作序列)           │  │      │
│  │  │  [                                                     │  │      │
│  │  │    Operation{Type: ShortTermOrder, Data: TxA},       │  │      │
│  │  │    Operation{Type: ShortTermOrder, Data: TxB},       │  │      │
│  │  │    Operation{Type: Match, MakerOrderId: A,           │  │      │
│  │  │                      TakerOrderId: B, FillAmount: 100}│  │      │
│  │  │  ]                                                     │  │      │
│  │  └───────────────────────────────────────────────────────┘  │      │
│  │                                                              │      │
│  │  输出: 区块提案 (Block Proposal)                             │      │
│  │  ┌───────────────────────────────────────────────────────┐  │      │
│  │  │ Block.Data = Operations[] + UserTxs[]                 │  │      │
│  │  │ Block.Header.ProposerAddress = Validator_X            │  │      │
│  │  └───────────────────────────────────────────────────────┘  │      │
│  └─────────────────────────┬───────────────────────────────────┘      │
│                            │                                          │
│  ③ ProcessProposal (验证节点验证)                                      │
│                            │ 广播区块提案                             │
│          ┌─────────────────┼─────────────────┬──────────┐            │
│          │                 │                 │          │            │
│          ▼                 ▼                 ▼          ▼            │
│   ┌──────────┐      ┌──────────┐      ┌──────────┐ ┌──────────┐    │
│   │Validator1│      │Validator2│      │Validator3│ │   ...    │    │
│   └────┬─────┘      └────┬─────┘      └────┬─────┘ └────┬─────┘    │
│        │                 │                 │            │            │
│        │  各自独立重放撮合逻辑               │            │            │
│        ▼                 ▼                 ▼            ▼            │
│   ┌───────────────────────────────────────────────────────────┐    │
│   │  每个验证节点执行:                                         │    │
│   │  1. 重放 MemClob.matchOrders(Operations[])                │    │
│   │  2. 验证成交结果一致性                                     │    │
│   │  3. 验证签名和 Nonce                                       │    │
│   │  4. 验证 Gas 费充足                                        │    │
│   │                                                             │    │
│   │  投票: Accept or Reject                                    │    │
│   └───────────────────────────────────────────────────────────┘    │
│                            │                                          │
│                            │ 67%+ 投票通过                            │
│                            ▼                                          │
│  ④ Consensus (Tendermint BFT)                                        │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  区块确认 (Finalized)                                        │    │
│  │  • Block Height: N+1                                        │    │
│  │  • Block Hash: 0x1234...abcd                                │    │
│  │  • 不可逆 (67%+ 验证节点签名)                               │    │
│  └─────────────────────────┬───────────────────────────────────┘    │
│                            │                                          │
│  ⑤ DeliverTx (最终状态提交)                                          │
│                            ▼                                          │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  KVStore 原子更新 (IAVL Tree - Merkle)                      │    │
│  │  ┌───────────────────────────────────────────────────────┐  │    │
│  │  │ BEGIN STATE TRANSITION                                │  │    │
│  │  │                                                        │  │    │
│  │  │ Key: "subaccount/0xABC...DEF/0"                       │  │    │
│  │  │ Value.AssetPositions[0].Quantums:                     │  │    │
│  │  │   1000000000000 → 950000000000 (-5M USDC)            │  │    │
│  │  │ Value.PerpetualPositions[0]:                          │  │    │
│  │  │   Quantums: 0 → 10000000000 (+100 BTC-PERP)          │  │    │
│  │  │                                                        │  │    │
│  │  │ Key: "subaccount/0xXYZ...123/0"                       │  │    │
│  │  │ Value.AssetPositions[0].Quantums:                     │  │    │
│  │  │   500000000000 → 550000000000 (+5M USDC)             │  │    │
│  │  │ Value.PerpetualPositions[0]:                          │  │    │
│  │  │   Quantums: 0 → -10000000000 (-100 BTC-PERP)         │  │    │
│  │  │                                                        │  │    │
│  │  │ Key: "orderbook/BTC-PERP/asks/50000"                  │  │    │
│  │  │ Value: [] (移除 OrderB)                               │  │    │
│  │  │                                                        │  │    │
│  │  │ COMMIT → 生成 Merkle Root Hash (App Hash)            │  │    │
│  │  │ Old AppHash: 0xABCD...1234                            │  │    │
│  │  │ New AppHash: 0x5678...EFGH                            │  │    │
│  │  └───────────────────────────────────────────────────────┘  │    │
│  └─────────────────────────┬───────────────────────────────────┘    │
│                            │                                          │
│  ⑥ Indexer 事件推送 (异步,不影响共识)                                 │
│                            ▼                                          │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Indexer Manager                                             │    │
│  │  ┌─────────────┬─────────────┬─────────────────────────┐    │    │
│  │  │ Event Bus   │ Kafka Topic │ PostgreSQL (查询优化)   │    │    │
│  │  ├─────────────┼─────────────┼─────────────────────────┤    │    │
│  │  │ TradeFill   │ → trades    │ INSERT trade record     │    │    │
│  │  │ SubaccountUpdate│→positions│UPDATE positions table│    │    │
│  │  │ OrderRemoval│ → orders    │ UPDATE order status     │    │    │
│  │  └─────────────┴─────────────┴─────────────────────────┘    │    │
│  │                       │                                       │    │
│  │                       ▼                                       │    │
│  │  ┌────────────────────────────────────────────────────┐     │    │
│  │  │ WebSocket / gRPC Stream                             │     │    │
│  │  │ → 推送给订阅用户                                   │     │    │
│  │  │   • 用户A: "成交! +100 BTC-PERP @ 50000"           │     │    │
│  │  │   • 用户B: "成交! -100 BTC-PERP @ 50000"           │     │    │
│  │  └────────────────────────────────────────────────────┘     │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                                                                        │
│  ⑦ 用户查询 (Indexer 或全节点)                                         │
│  ┌─────────────┐   GET /api/v4/positions                             │
│  │   用户 A     │ ──────────────────▶ Indexer API (PostgreSQL)       │
│  └─────────────┘                     (毫秒级响应)                     │
│                                      OR                               │
│                                      Full Node (ABCI Query)           │
│                                      (需遍历 Merkle Tree,较慢)        │
│                                                                        │
│  延迟对比:                                                            │
│  • PrepareCheckState 完成: ~100ms (内存撮合)                          │
│  • 共识投票: ~400ms (网络通信 + 验证)                                 │
│  • 区块确认: ~1000ms (1秒/区块)                                       │
│  • 状态更新: ~50ms (Merkle Tree 计算)                                 │
│  • 用户推送: +50ms (Indexer 延迟)                                     │
│  总计: ~1.1s (从下单到最终确认)                                       │
│                                                                        │
│  优点: 🔒 强一致性, ✅ 状态可验证, 🛡️ 去信任化                      │
│  缺点: ⏱️ 高延迟 (~1s), 📉 低吞吐 (~2000 TPS), 🚫 难扩展            │
└──────────────────────────────────────────────────────────────────────┘
```

#### 设计权衡

```Go
┌────────────────── 不可能三角 (CAP 定理在交易系统的体现) ────────────────┐
│                                                                            │
│                         ⚡ 高性能 (Performance)                           │
│                        (低延迟 + 高吞吐量)                                │
│                              /        \                                   │
│                             /          \                                  │
│                            /            \                                 │
│                           /              \                                │
│                          /                \                               │
│                         /                  \                              │
│                        /                    \                             │
│                       /                      \                            │
│                      /                        \                           │
│                     /        不可能同时        \                          │
│                    /         满足三者           \                         │
│                   /                              \                        │
│                  /                                \                       │
│   🔒 强一致性 ◀────────────────────────────────────▶ 🌐 去中心化          │
│   (Consistency)                                    (Decentralization)     │
│                                                                            │
│  CEX 选择: 高性能 + 强一致性 → 牺牲去中心化                              │
│  • 单点控制: 交易所完全掌控                                               │
│  • 最终一致性可接受: 通过异步持久化                                       │
│  • 极致性能: 微秒级撮合,百万级TPS                                         │
│                                                                            │
│  DEX 选择: 强一致性 + 去中心化 → 牺牲性能                                │
│  • 共识开销: 67%节点验证,秒级延迟                                         │
│  • 状态一致: 所有节点完全同步                                             │
│  • 可扩展性受限: ~2000 TPS (单链瓶颈)                                     │
│                                                                            │
│  混合方案 (dYdX v4):                                                      │
│  • PrepareCheckState: 链下撮合 (高性能)                                   │
│  • ProcessProposal: 确定性验证 (一致性)                                   │
│  • Cosmos SDK: BFT 共识 (去中心化)                                        │
│  → 在去中心化前提下尽量提升性能                                           │
└────────────────────────────────────────────────────────────────────────────┘
```

#### 为什么 DEX 必须这样设计?

**确定性执行 (Deterministic Execution)**

```Go
CEX:
┌────────────────────────────────┐
│  撮合结果可以有微小差异         │
│  • 时间戳精度不同               │
│  • 浮点数计算误差               │
│  • 并发订单排序差异             │
│  ✓ 最终统一即可 (补偿机制)      │
└────────────────────────────────┘

DEX:
┌────────────────────────────────┐
│  撮合结果必须完全相同           │
│  • 所有节点执行相同逻辑         │
│  • 任何差异导致共识失败         │
│  • 必须使用整数运算             │
│  • 必须固定排序算法             │
│  ✓ 否则区块链分叉!              │
└────────────────────────────────┘
// dYdX 使用 big.Int (任意精度整数)
type Subaccount struct {
    AssetPositions []AssetPosition // Quantums (整数)
    PerpetualPositions []PerpetualPosition // BaseQuantums (整数)
}

// 禁止浮点数!避免不同 CPU 架构结果差异
// ✗ 不能用: float64 price = 50000.123
// ✓ 必须用: Subticks uint64 = 5000000000 (固定精度缩放)
```

**状态证明 (State Proof)**

```YAML
CEX:
┌────────────────────────────────────┐
│  用户需要信任交易所                │
│  • 无法自行验证余额                │
│  • 依赖储备金证明 (Proof of Reserves) │
│  • 审计机构介入                    │
│  ✗ 历史上多次暴雷 (Mt.Gox, FTX)    │
└────────────────────────────────────┘

DEX:
┌────────────────────────────────────┐
│  用户可自行验证状态                │
│  • Merkle Proof 证明账户存在       │
│  • App Hash 唯一确定全局状态       │
│  • 轻客户端验证 (无需全节点)       │
│  ✓ 去信任化 (Trustless)            │
└────────────────────────────────────┘

Merkle Proof 示例:
用户查询余额: "我的 Subaccount 有 10000 USDC?"

1. 全节点提供 Merkle Path:
   Root Hash: 0xABCD...
   ├─ Branch1: 0x1234...
   │  ├─ Branch1.1: 0x5678...
   │  └─ Branch1.2: 0x9ABC... ◀─ 你的账户在这
   └─ Branch2: 0xDEF0...

2. 本地验证:
   Hash(你的账户数据) = 0x9ABC... ✓
   Hash(Branch1.1 + 0x9ABC...) = 0x1234... ✓
   Hash(0x1234... + Branch2) = 0xABCD... ✓
   → 证明账户确实在链上!
```

**原子化结算 (Atomic Settlement)**

```Go
CEX 结算失败场景:
┌────────────────────────────────────────────────┐
│ 1. 撮合成功: OrderA ↔ OrderB                  │
│ 2. 更新 UserA 余额: ✓                          │
│ 3. 更新 UserB 余额: ✗ (数据库死锁/网络故障)   │
│ 4. 系统状态: 不一致!                           │
│    → 需要补偿事务回滚 UserA                    │
│    → 或人工介入修正                            │
└────────────────────────────────────────────────┘

DEX 原子化保证:
┌────────────────────────────────────────────────┐
│ CacheContext 机制:                             │
│                                                 │
│ 1. 创建状态快照 (CacheContext)                 │
│ 2. 在快照上执行所有操作                        │
│    • 更新 SubaccountA                          │
│    • 更新 SubaccountB                          │
│    • 更新 Orderbook                            │
│ 3. 验证所有约束                                │
│    • 抵押品充足?                               │
│    • 订单有效?                                 │
│ 4. 决策:                                       │
│    ✓ 全部成功 → ctx.Write() 提交到主状态       │
│    ✗ 任一失败 → 丢弃快照,状态回滚             │
│                                                 │
│ → 不存在"部分成功"的情况!                      │
└────────────────────────────────────────────────┘
// protocol/x/clob/memclob/memclob.go
func (m *MemClobPriceTimePriority) mustPerformTakerOrderMatching(...) {
    // 创建缓存上下文 (事务边界)
    cacheCtx := ctx.WithCacheMultiStore(ctx.MultiStore().CacheMultiStore())
    
    // 尝试撮合
    fills, err := m.attemptMatching(cacheCtx, takerOrder, makerOrders)
    
    // 验证抵押品
    success, _, err := k.CanUpdateSubaccounts(cacheCtx, updates)
    
    if success && err == nil {
        // ✓ 原子提交: 一次性更新所有状态
        cacheCtx.MultiStore().Write()
    } else {
        // ✗ 自动回滚: 丢弃所有更改
        return errors.Wrap(err, "collateral check failed")
    }
}
```

设计维度CEXDEX (dYdX)根本原因数据精度浮点数 (double)整数 (big.Int)浮点数在不同 CPU 架构结果不同,破坏确定性时间戳系统时钟 (ntp)区块高度 (Block.Height)系统时钟不同步导致排序差异订单排序灵活 (可人工调整)严格 Price-Time Priority必须保证所有节点排序一致状态存储SQL (行式存储)Merkle Tree (KV存储)需要生成状态证明 (Merkle Proof)错误处理异常重试 + 补偿交易失败回滚CacheContext 提供 ACID 语义Gas 费无 (交易所补贴)用户支付防止 DoS 攻击 + 激励验证节点升级方式停机维护链上治理投票去中心化升级,无单点控制监管合规KYC/AML 强制协议层不可审查去中心化特性决定

```Go
┌─────────────────────────── 设计哲学对比 ───────────────────────────┐
│                                                                      │
│  CEX 哲学: "Move Fast and Break Things"                            │
│  ┌──────────────────────────────────────────────────────────┐      │
│  │  • 追求极致性能: 微秒级延迟                              │      │
│  │  • 灵活应变: 发现问题立即回滚/人工修正                   │      │
│  │  • 用户体验优先: 即时到账,友好界面                       │      │
│  │  • 快速迭代: 每周发布新功能                              │      │
│  │  • 成本考量: 中心化降低运营成本                          │      │
│  └──────────────────────────────────────────────────────────┘      │
│  适用场景: 大众市场,追求流动性和用户体验                          │
│                                                                      │
│  DEX 哲学: "Code is Law, Trust is Minimized"                       │
│  ┌──────────────────────────────────────────────────────────┐      │
│  │  • 安全第一: 确定性执行,无单点故障                       │      │
│  │  • 透明化: 所有状态公开可验证                            │      │
│  │  • 抗审查: 协议层不可干预用户资产                        │      │
│  │  • 慢即是快: 牺牲性能换取一致性                          │      │
│  │  • 去信任: 用户无需信任任何中介                          │      │
│  └──────────────────────────────────────────────────────────┘      │
│  适用场景: 高价值资产,需要主权和隐私保护的用户                    │
│                                                                      │
│  dYdX v4 的折中:                                                    │
│  • PrepareCheckState: 链下高性能撮合 (CEX 优点)                    │
│  • ProcessProposal: 确定性共识验证 (DEX 保证)                      │
│  • Cosmos SDK: 模块化设计易于升级                                  │
│  → 在去中心化约束下尽量优化性能                                    │
└──────────────────────────────────────────────────────────────────────┘
```

**三个不可妥协的原则**

1. **确定性 (Determinism)** 所有验证节点必须得到完全相同的执行结果,否则共识失败。这要求:
   1. 整数运算 (避免浮点数误差)
   2. 固定排序 (Price-Time Priority 严格执行)
   3. 区块高度替代时间戳 (消除时钟偏移)
2. **可验证性 (Verifiability)** 用户和轻客户端必须能独立验证状态,无需信任。这要求:
   1. Merkle Tree 存储 (状态证明)
   2. App Hash 唯一确定全局状态
   3. 公开透明的执行日志
3. **原子性 (Atomicity)** 交易要么完全成功,要么完全失败,不存在中间状态。这要求:
   1. CacheContext 机制 (快照隔离)
   2. 链上验证所有约束 (抵押品、余额)
   3. 同步提交 (Write() 原子化)

**性能牺牲是必然代价**

- **共识开销**: 67% 验证节点同意才能确认,无法避免网络延迟
- **串行执行**: 不能像 CEX 一样并发处理,保证确定性
- **状态证明**: Merkle Tree 计算和验证增加计算开销

**未来优化方向**

- **Layer 2 扩容**: 如 StarkEx (状态通道/Rollup)
- **并行 EVM**: 如 Sei v2 (乐观并行执行)
- **应用链**: 如 dYdX v4 (专用链,减少通用链开销)

**DEX 的设计不是技术落后,而是在去中心化、安全性、透明性等核心价值观下的理性选择。**

# 问题

1. Orderbook 订单簿存储？
2. 怎么经过共识，所有节点的订单状态一致

1. 短期订单  余额验证是否改状态锁定余额？ ✅ 
2. Operation操作的状态改变 ✅ 
3. PrepareCheckState 内存订单簿状态一致性 ✅ 
4. 短期订单有区块高度是否在当前区块有效 ✅ 
5. DeliverTx 修改用户余额是否有双花问题  ✅
6. 余额不足的撮合结果，是否会失败 ✅
7. 去杠杆和清算为什么在PrepareCheckState阶段执行✅
8. 长期订单什么时候撮合 ✅

# Clob四周总结

Clob模块熟悉的三个阶段

1. 简单熟悉Cosmos(ABCI)，整体过Clob模块（挨个文件）熟悉代码，熟悉项目 —— 每个子模块/文件 做了什么 （1.5周）
2. 结合具体业务+ABCI+简单test+AI解答重要机制  —— ABCI即重要业务调用流程和机制，更进一步熟悉Clob （1.5周）
3. 按照重点测试项 debug+test+代码和机制注释  深入分析 —— 找出值得分享的内容（难点，新机制），找出不容易掌握的部分后续再进一步深入了解 （进行中）
   1. 很 耗时
4. 难点业务，机制，问题 —— 结合应用，专项梳理理解 （后续）

当前掌握情况：60-70%

Clob模块特点：代码分散，调用链路不清晰，代码复杂，代码量大

后续：

1. 参与后续需要做的内容 + 完成阶段3
2. 难点问题机制 深入理解
3. 产出：关键+新 机制分享    抛出非常难理解掌握的问题

# 重点测试

测试优先顺序：memclob -> keeper -> e2e -> rate_limit

## Memclob ✅

| 文件名                                      | 测试用例数 | 子测试数 | 总用例数 | 代码行数     | 简要测试功能介绍                                             | 分享内容                                                   |
| ------------------------------------------- | ---------- | -------- | -------- | ------------ | ------------------------------------------------------------ | ---------------------------------------------------------- |
| memclob_cancel_order_test.go                | 6          | 2        | 8        | 1343         | 测试短期订单的取消操作，包括取消已存在、GTB验证等            | ✅订单取消的区块窗口                                        |
| memclob_create_orderbook_test.go            | 6          | 0        | 6        | 116          | 测试订单簿的创建功能，包括永续合约订单簿、多订单簿等         |                                                            |
| memclob_get_impact_price_subticks_test.go   | 1          | 1        | 2        | 445          | 测试冲击价格计算，用于计算指定名义金额的加权平均价格         |                                                            |
| memclob_get_order_filled_amount_test.go     | 1          | 1        | 2        | 61           | 测试订单成交量查询功能                                       |                                                            |
| memclob_get_order_test.go                   | 2          | 0        | 2        | 41           | 测试订单查询功能，包括订单存在和不存在的情况                 |                                                            |
| memclob_get_premium_price_test.go           | 1          | 1        | 2        | 734          | 测试溢价价格计算，用于永续合约资金费率计算                   |                                                            |
| memclob_get_subaccount_orders_test.go       | 2          | 1        | 3        | 268          | 测试查询子账户的所有开放订单                                 |                                                            |
| memclob_grpc_streaming_test.go              | 2          | 0        | 2        | 95           | 测试gRPC流式更新功能，验证订单簿更新消息的发送               |                                                            |
| memclob_place_order_long_term_test.go       | 2          | 1        | 3        | 684          | 测试长期订单的下单、撮合、抵押检查失败处理和Post-only订单    |                                                            |
| memclob_place_order_reduce_only_test.go     | 2          | 2        | 4        | 1778         | 测试Reduce-Only订单（仅平仓订单）的下单和撮合                | ✅ 🤔Reduce-Only订单（普通订单极端行情仓位反转，未完全理解） |
| memclob_place_order_test.go                 | 14         | 7        | 21       | 4023         | 核心下单测试，覆盖空订单簿、订单撮合、部分/完全成交、价格滑点、Post-only等场景 | ✅订单替换，带BuilderCode订单                               |
| memclob_place_perpetual_liquidation_test.go | 2          | 2        | 4        | 884          | 测试清算订单的下单和撮合，包括成功场景和抵押检查失败         | ✅清算单                                                    |
| memclob_purge_invalid_memclob_state_test.go | 5          | 1        | 6        | 677          | 测试订单簿状态清理，包括过期订单、成交订单、取消订单等       | ✅移除操作 全局与本地状态一致性                             |
| memclob_remove_and_clear_test.go            | 1          | 1        | 2        | 202          | 测试移除并清空操作队列的功能                                 |                                                            |
| memclob_remove_order_test.go                | 3          | 2        | 5        | 667          | 测试订单移除功能，包括完全成交移除和强制移除两种场景         |                                                            |
| orderbook_cancels_test.go                   | 7          | 0        | 7        | 127          | 测试订单簿中取消操作的数据结构管理（双向索引）               |                                                            |
| **16个文件**                                | **57个**   | **23个** | **80个** | **12,088行** | **覆盖订单簿核心功能的完整测试**                             |                                                            |

## Keeper

| 文件名                                               | 测试用例数 | 子测试数 | 总用例数 | 代码行数   | 简要测试功能介绍                         | 是否重要 | 值得分享                   |
| ---------------------------------------------------- | ---------- | -------- | -------- | ---------- | ---------------------------------------- | -------- | -------------------------- |
| clob_pair_test.go                                    | 20         | 5        | 25       | 1209       | 测试CLOB配对（交建、更新、验证和状态管理 | ❌        | ❌                          |
| deleveraging_test.go                                 | 6          | 6        | 12       | 1593       | 测试去杠杆机制（自动减仓）的触发和执行   | ✅        | ✅ （需要进一步分析）       |
| get_price_premium_test.go                            | 1          | 1        | 2        | 235        | 测试永续合的计算（用于资金费率）         | ❌        | ❌（memclob已测试）         |
| grpc_query_block_rate_limit_configuration_test.go    | 1          | 1        | 2        | 62         | 测试查询区块级别速率限制配置的gRPC接口   | ❌        |                            |
| grpc_query_clob_pair_test.go                         | 2          | 5        | 7        | 144        | 测试查询B配对信息的gRPC接口              | ❌        |                            |
| grpc_query_equity_tier_limit_config_test.go          | 1          | 0        | 1        | 33         | 测试查询权益层级限制配置的gRPC接口       | ❌        |                            |
| grpc_query_liquidations_configuration_test.go        | 1          | 1        | 2        | 44         | 测试查询清算配置的gRPC接口               | ❌        |                            |
| grpc_query_mev_node_to_node_test.go                  | 1          | 1        | 2        | 137        | 测试MEV节点间查询的gRPC接口              | ✅        | ✅ （需要进一步分析）       |
| grpc_query_stateful_order_test.go                    | 1          | 0        | 1        | 53         | 测试询长期订单状态的gRPC接口             | ❌        |                            |
| keeper_test.go                                       | 3          | 0        | 3        | 82         | Keeper基础功能测试和初始化               | ❌        |                            |
| leverage_e2e_test.go                                 | 6          | 1        | 7        | 549        | 杠杆交易端到端金计算）                   |          |                            |
| liquidations_state_test.go                           | 4          | 1        | 5        | 351        | 测试清算理（清算记录、防重复清算等）     |          |                            |
| liquidations_test.go                                 | 16         | 14       | 30       | 5369       | 核心清算功括清算订单生成、匹配和结算     |          | ⭐                          |
| match_state_test.go                                  | 1          | 0        | 1        | 49         | 测试成交价格跟踪                         | ❌        | 最低最高成交价区块临时存储 |
| mev_test.go                                          | 3          | 3        | 6        | 1458       | 测试MEV（矿工可提取价值                  |          | ⭐                          |
| msg_server_cancel_orders_test.go                     | 6          | 2        | 8        | 261        | 测取消订单消息的处理逻辑                 |          |                            |
| msg_server_create_clob_pair_test.go                  | 1          | 1        | 2        | 173        | 测试创建CLOB配对消息的处理               |          |                            |
| msg_server_place_order_test.go                       | 4          | 3        | 7        | 626        | 测试消息的服务端处理逻辑                 |          |                            |
| msg_server_proposed_operations_test.go               | 1          | 1        | 2        | 82         | 测试提议操作消息的处理                   |          |                            |
| msg_server_update_block_rate_limit_config_test.go    | 1          | 0        | 1        | 79         | 测试更新区块速率限制配置消息             |          |                            |
| msg_server_update_clob_pair_test.go                  | 1          | 1        | 2        | 285        | 测试更新CLOB配对消息的处理               |          |                            |
| msg_server_update_equity_tier_limit_config_test.go   | 1          | 0        | 1        | 96         | 测试更新权益层级限制配置消息             |          |                            |
| msg_server_update_liquidations_config_test.go        | 1          | 1        | 2        | 75         | 测试更新清算配置消息的处理               |          |                            |
| order_cancellation_test.go                           | 6          | 1        | 7        | 455        | 测试订单种场景和边界条件                 |          |                            |
| order_state_test.go                                  | 6          | 4        | 10       | 729        | 测试订单状态的询                         |          |                            |
| orders_test.go                                       | 12         | 8        | 20       | 2629       | 订单核心功能测试，生命周期管理           |          | ⭐                          |
| process_operations_liquidations_test.go              | 5          | 4        | 9        | 2469       | 测试处理清算操作队列的逻辑               |          | ⭐                          |
| process_operations_long_term_test.go                 | 1          | 1        | 2        | 931        | 测试处理长期订单操作的逻辑               |          |                            |
| process_operations_stateful_validation_test.go       | 2          | 2        | 4        | 654        | 测试操作队列的状态验证逻辑               |          |                            |
| process_operations_test.go                           | 2          | 2        | 4        | 2912       | 核心操作试，包括订单匹配和状态更新       |          | ⭐                          |
| process_proposer_matches_events_test.go              | 4          | 1        | 5        | 254        | 测试提议者匹配事件的处理和验证           |          |                            |
| process_single_match_affiliate_stats_test.go         | 7          | 0        | 7        | 891        | 测试单笔匹配的联盟统计（推荐返佣）       |          |                            |
| process_single_match_isolated_insurance_fund_test.go | 1          | 0        | 1        | 318        | 测试隔离保险基金的单笔匹配处理           |          |                            |
| rate_limit_test.go                                   | 2          | 0        | 2        | 44         | 测试速率限制的基础功能                   |          |                            |
| stateful_order_expiration_migration_test.go          | 1          | 1        | 2        | 72         | 测试长期订单过期迁移逻辑                 |          |                            |
| stateful_order_state_test.go                         | 8          | 3        | 11       | 1069       | 测试订单状态的存储和管理                 |          |                            |
| twap_order_state_test.go                             | 5          | 2        | 7        | 713        | 测试TWAP（权平均价格）订单状态管理       |          |                            |
| untriggered_conditional_orders_test.go               | 5          | 4        | 9        | 525        | 测试未触发条件单的状态管理和触发机制     |          |                            |
| **38个文件**                                         | **150**    | **81**   | **231**  | **27,710** | **Keeper层核心业务逻辑测试**             |          | **6个推荐**                |

## e2e

| 文件名                             | 测试用例数 | 子测试数 | 总用例数 | 代码行数   | 简要测试功能介绍                                         | 值得分享    |
| ---------------------------------- | ---------- | -------- | -------- | ---------- | -------------------------------------------------------- | ----------- |
| app_test.go                        | 6          | 2        | 8        | 1116       | 应用层基础端到端测试，包括初始化和配置                   |             |
| batch_cancel_test.go               | 3          | 3        | 6        | 900        | 测试批量取消订单                                         |             |
| builder_code_test.go               | 1          | 1        | 2        | 155        | 测试构建器代码的关）                                     |             |
| conditional_orders_test.go         | 5          | 5        | 10       | 2764       | 条件单（损）的完整端到端测试，包括触发、撮合、取消、过期 | ⭐           |
| equity_tier_limit_test.go          | 3          | 3        | 6        | 715        | 测试权益层到端功能                                       |             |
| isolated_subaccount_orders_test.go | 1          | 1        | 2        | 679        | 测试隔离子账户订单的端到端流程                           |             |
| liquidation_deleveraging_test.go   | 2          | 2        | 4        | 1233       | 清和去杠杆的端到端集成测试                               | ⭐           |
| long_term_orders_test.go           | 8          | 4        | 12       | 2042       | 长期订单的周期端到端测试                                 | ⭐           |
| order_matches_test.go              | 1          | 1        | 2        | 220        | 订单撮合的端到端验证                                     |             |
| order_removal_test.go              | 4          | 3        | 7        | 906        | 订单移除（完全过期）的端到端测试                         |             |
| permissioned_keys_test.go          | 2          | 2        | 4        | 1540       | 权限密钥到端测试                                         |             |
| rate_limit_test.go                 | 9          | 6        | 15       | 997        | 速率限制的完整端                                         |             |
| reduce_only_orders_test.go         | 3          | 3        | 6        | 953        | Reduce-Only订单的端到端测试                              |             |
| short_term_orders_test.go          | 4          | 4        | 8        | 1573       | 短期订单命周期端到端测试                                 | ⭐           |
| twap_orders_test.go                | 8          | 0        | 8        | 967        | TWAP订单的端到端                                         |             |
| withdrawal_gating_test.go          | 1          | 1        | 2        | 553        | 提款门控机端测试                                         |             |
| **16个文件**                       | **61**     | **41**   | **102**  | **17,313** | **完整业务流程端到端测试**                               | **4个推荐** |

## rate_limit

| 文件名                            | 测试用例数 | 子测试数 | 总用例数 | 代码行数 | 简要测试功能介绍                | 值得分享    |
| --------------------------------- | ---------- | -------- | -------- | -------- | ------------------------------- | ----------- |
| multi_block_rate_limiter_test.go  | 2          | 0        | 2        | 105      | 测试区块速率限制器的功能        |             |
| noop_rate_limiter_test.go         | 1          | 0        | 1        | 19       | 测试空操作速无限制模式）        |             |
| panic_rate_limiter_test.go        | 1          | 0        | 1        | 23       | 测试panic速制器（用于测试环境） |             |
| single_block_rate_limiter_test.go | 2          | 0        | 2        | 50       | 测试区块速率限制器的功能        |             |
| **4个文件**                       | **6**      | **0**    | **6**    | **197**  | **速率限制器单元测试**          | **0个推荐** |