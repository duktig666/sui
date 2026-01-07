# CLOB 模块数据结构文档

## 模块概述

CLOB (中央限价订单簿) 模块是 Hermes DEX 的核心交易引擎,负责订单放置、匹配、取消、清算等关键功能。本文档详细描述了 CLOB 模块中的所有数据结构,按存储类型分类组织。

**模块路径**: `protocol/x/clob`
**Store Key**: `clob`
**代码规模**: ~10万+ 行(包含 memclob 匹配引擎)

---

## 一、StateStore (链上持久化存储)

StateStore 存储的数据会在区块间持久化,是链上状态的核心。这些数据在节点重启后依然存在,通过共识保证所有节点数据一致。

### 1.1 CLOB 交易对配置

**存储键**: `ClobPairKeyPrefix` + `ClobPairId` = `"Clob:" + <uint32>`

**数据结构**: `ClobPair`

```go
type ClobPair struct {
    Id uint32                   // 订单簿 ID,唯一标识符
    Metadata isClobPair_Metadata // 产品特定元数据(Perpetual/Spot)
    StepBaseQuantums uint64      // 订单数量最小步长(base quantums)
    SubticksPerTick uint32       // 每个 tick 包含的 subticks 数量
    QuantumConversionExponent int32  // 数量转换指数
    Status ClobPair_Status       // 交易对状态
}

// 元数据类型 - Perpetual CLOB
type PerpetualClobMetadata struct {
    PerpetualId uint32  // 关联的永续合约 ID
}

// 元数据类型 - Spot CLOB
type SpotClobMetadata struct {
    BaseAssetId uint32   // 基础资产 ID
    QuoteAssetId uint32  // 报价资产 ID
}

// 交易对状态枚举
type ClobPair_Status int32
const (
    STATUS_UNSPECIFIED      = 0  // 无效状态
    STATUS_ACTIVE           = 1  // 活跃状态,正常交易
    STATUS_PAUSED           = 2  // 暂停状态
    STATUS_CANCEL_ONLY      = 3  // 仅允许取消订单
    STATUS_POST_ONLY        = 4  // 仅允许挂单
    STATUS_INITIALIZING     = 5  // 初始化状态,仅接受短期+Post-Only订单
    STATUS_FINAL_SETTLEMENT = 6  // 最终结算,停止交易,平仓所有持仓
)
```

**业务含义**:
- 定义一个交易对的所有配置参数,决定订单簿的运行规则
- `StepBaseQuantums`: 订单数量必须是此值的整数倍,例如 100 表示最小 0.01 BTC
- `SubticksPerTick`: 价格精度,例如 100 表示价格最小变动单位是 1 tick = 100 subticks
- `QuantumConversionExponent`: 用于将数量从链上表示转换为人类可读格式
- `Status`: 控制交易对当前允许的操作类型,用于市场管理和风控

**使用场景**:
- **创建新交易对**: 通过治理提案或无权限上市(Listing)流程创建
- **订单验证**: 放置订单时读取配置,检查订单数量和价格是否符合步长要求
- **市场管理**: 更改 Status 可以控制市场行为(如紧急暂停、仅允许取消)
- **查询接口**: 前端查询交易对列表和配置

**数据访问**:
```go
// 读取
keeper.GetClobPair(ctx, clobPairId) -> ClobPair
keeper.GetAllClobPairs(ctx) -> []ClobPair

// 写入
keeper.SetClobPair(ctx, clobPair)
```

**关键文件**:
- `protocol/x/clob/types/clob_pair.pb.go:187-249`
- `protocol/x/clob/keeper/clob_pair.go`

---

### 1.2 Long-Term 订单 (长期订单)

**存储键**: `LongTermOrderPlacementKeyPrefix` + `OrderId` = `"SO/P/L:" + <OrderId>`

**数据结构**: `LongTermOrderPlacement`

```go
type LongTermOrderPlacement struct {
    Order Order                      // 订单详细信息
    PlacementIndex TransactionOrdering  // 订单放置的区块和交易索引
}

type Order struct {
    OrderId OrderId                // 订单唯一标识
    Side Order_Side                // BUY/SELL
    Quantums uint64                // 订单数量(base quantums)
    Subticks uint64                // 价格(subticks)
    GoodTilBlockTime uint32        // 过期时间(Unix时间戳,秒)
    TimeInForce Order_TimeInForce  // 时间有效性类型
    ReduceOnly bool                // 是否只能减仓
    ClientMetadata uint32          // 客户端元数据
    ConditionType Order_ConditionType  // 条件类型(对Long-Term通常是UNSPECIFIED)
    ConditionalOrderTriggerSubticks uint64  // 条件触发价格
}

type OrderId struct {
    SubaccountId types.SubaccountId  // 子账户 ID
    ClientId uint32                  // 客户端订单 ID
    OrderFlags uint32                // 订单标志(64=Long-Term)
    ClobPairId uint32                // 所属交易对 ID
}

type TransactionOrdering struct {
    BlockHeight uint32      // 区块高度
    TransactionIndex uint32 // 交易在区块中的索引
}

// 订单方向枚举
type Order_Side int32
const (
    SIDE_UNSPECIFIED = 0
    SIDE_BUY = 1         // 买单
    SIDE_SELL = 2        // 卖单
)

// 时间有效性枚举
type Order_TimeInForce int32
const (
    TIME_IN_FORCE_UNSPECIFIED = 0  // 默认:先匹配,剩余挂单
    TIME_IN_FORCE_IOC = 1          // 立即成交或取消(Immediate-Or-Cancel)
    TIME_IN_FORCE_POST_ONLY = 2    // 只能挂单,不能立即成交
)
```

**业务含义**:
- Long-Term 订单是跨区块有效的订单,类似于传统交易所的限价单
- 有明确的过期时间 (`GoodTilBlockTime`),到期后自动失效
- 需要持久化存储,因为订单可能在数小时、数天甚至更长时间内有效
- `PlacementIndex` 用于订单优先级排序(价格相同时,按放置时间排序)
- `ReduceOnly` 标志确保订单只能减少现有持仓,不能增加持仓(用于风控)

**使用场景**:
- **用户放置限价单**: 用户提交 MsgPlaceOrder,系统验证并存储
- **订单匹配**: 每个区块在 EndBlock 阶段从存储中读取活跃订单进行匹配
- **订单查询**: 用户查询自己的未成交订单
- **订单过期清理**: 系统定期扫描并删除过期订单

**数据访问**:
```go
// 读取
keeper.GetLongTermOrderPlacement(ctx, orderId) -> LongTermOrderPlacement
keeper.GetAllLongTermOrders(ctx) -> []LongTermOrderPlacement

// 写入
keeper.SetLongTermOrderPlacement(ctx, orderPlacement)

// 删除
keeper.RemoveLongTermOrderPlacement(ctx, orderId)
```

**与 MemClob 的关系**:
- 订单首次放置时同时存储到 StateStore 和 MemClob
- 应用启动时从 StateStore 加载所有 Long-Term 订单到 MemClob
- 订单成交或取消时从两处同时删除

**关键文件**:
- `protocol/x/clob/types/order.pb.go:448-505` (LongTermOrderPlacement)
- `protocol/x/clob/types/order.pb.go:637-847` (Order)
- `protocol/x/clob/keeper/long_term_order.go`

---

### 1.3 Conditional 订单 (条件订单)

#### 1.3.1 未触发的条件订单

**存储键**: `UntriggeredConditionalOrderKeyPrefix` + `OrderId` = `"SO/U:" + <OrderId>`

**数据结构**: `ConditionalOrderPlacement`

```go
type ConditionalOrderPlacement struct {
    Order Order                      // 订单详细信息
    PlacementIndex TransactionOrdering  // 订单放置的区块和交易索引
    TriggerIndex TransactionOrdering    // 订单触发的区块和交易索引(未触发时为空)
}

// 条件类型枚举
type Order_ConditionType int32
const (
    CONDITION_TYPE_UNSPECIFIED = 0   // 非条件订单
    CONDITION_TYPE_STOP_LOSS = 1     // 止损单
    CONDITION_TYPE_TAKE_PROFIT = 2   // 止盈单
)
```

**业务含义**:
- **止损单 (Stop Loss)**: 当预言机价格触及或突破触发价格时激活
  - 买单: 价格 >= 触发价格时激活(追涨)
  - 卖单: 价格 <= 触发价格时激活(止损)
- **止盈单 (Take Profit)**: 与止损单相反的触发逻辑
  - 买单: 价格 <= 触发价格时激活(低买)
  - 卖单: 价格 >= 触发价格时激活(止盈)
- 未触发的订单存储在此,不参与订单簿匹配
- 系统每个区块检查价格条件,满足时将订单移动到"已触发"存储

**使用场景**:
- **风险管理**: 交易者设置止损单,自动限制亏损
- **利润保护**: 交易者设置止盈单,自动锁定利润
- **自动化交易**: 无需盯盘,价格触发时自动执行

**触发逻辑**:
```
每个区块 EndBlock:
1. 读取当前市场价格(来自 Prices 模块)
2. 遍历未触发的条件订单
3. 检查触发条件是否满足
4. 如果满足,将订单从 "Untriggered" 移动到 "Triggered"
```

**数据访问**:
```go
// 读取
keeper.GetUntriggeredConditionalOrderPlacement(ctx, orderId) -> ConditionalOrderPlacement

// 写入
keeper.SetUntriggeredConditionalOrderPlacement(ctx, orderPlacement)

// 删除(触发时)
keeper.RemoveUntriggeredConditionalOrderPlacement(ctx, orderId)
```

**关键文件**:
- `protocol/x/clob/types/order.pb.go:571-635` (ConditionalOrderPlacement)
- `protocol/x/clob/keeper/conditional_order.go`

---

#### 1.3.2 已触发的条件订单

**存储键**: `TriggeredConditionalOrderKeyPrefix` + `OrderId` = `"SO/P/T:" + <OrderId>`

**数据结构**: `ConditionalOrderPlacement` (与未触发时相同结构)

**业务含义**:
- 条件订单触发后移动到此存储
- `TriggerIndex` 字段记录了订单触发的区块和交易索引
- 已触发的订单会被加载到 MemClob,参与订单簿匹配
- 与 Long-Term 订单的匹配逻辑相同

**使用场景**:
- 条件满足后自动进入订单簿进行匹配
- 用户查询已触发但未成交的订单
- 订单成交或过期后删除

**数据访问**:
```go
// 读取
keeper.GetTriggeredConditionalOrderPlacement(ctx, orderId) -> ConditionalOrderPlacement

// 写入(从未触发迁移)
keeper.SetTriggeredConditionalOrderPlacement(ctx, orderPlacement)

// 删除
keeper.RemoveTriggeredConditionalOrderPlacement(ctx, orderId)
```

**触发流程示例**:
```
1. 用户放置止损卖单: 触发价格=50000 USDT, 当前价格=51000 USDT
2. 存储到 UntriggeredConditionalOrder
3. 几个区块后,价格跌至 50000 USDT
4. 系统检测到触发条件满足
5. 将订单从 Untriggered 移动到 Triggered
6. 订单进入 MemClob 订单簿,开始匹配
7. 匹配成交后从 Triggered 存储中删除
```

**关键文件**:
- `protocol/x/clob/keeper/conditional_order.go`
- `protocol/x/clob/keeper/process_operations.go` (触发逻辑)

---

### 1.4 TWAP 订单 (时间加权平均价格订单)

#### 1.4.1 TWAP 主订单

**存储键**: `TWAPOrderKeyPrefix` + `OrderId` = `"TWAP:" + <OrderId>`

**数据结构**: `TwapOrderPlacement`

```go
type TwapOrderPlacement struct {
    Order Order                      // TWAP 母订单信息
    PlacementIndex TransactionOrdering  // 订单放置的区块和交易索引
    Duration uint32                   // 订单执行总时长(秒)
    Interval uint32                   // 每个子订单间隔(秒)
    PriceTolerance uint32             // 价格容差(ppm, 百万分率)
    RemainingLegs uint32              // 剩余待执行腿数
}
```

**业务含义**:
- TWAP (Time-Weighted Average Price) 订单用于将大额订单拆分成多个子订单,在指定时间内均匀执行
- 目的是减少市场冲击,避免单笔大额订单导致价格剧烈波动,获得接近平均价格的成交
- `Duration` 和 `Interval` 决定子订单的数量和执行频率
  - 子订单数 = Duration / Interval
  - 每个子订单数量 = 总订单数量 / 子订单数
- `PriceTolerance` 控制价格偏差,超出容差的子订单会被跳过
  - 例如 10000 ppm (1%) 表示允许价格偏差 ±1%
  - 如果市场价格超出目标价格 × (1 ± PriceTolerance/1000000),子订单不执行
- `RemainingLegs` 跟踪还有多少个子订单待执行,用于恢复执行进度

**参数约束**:
- Duration 范围: [300, 86400] 秒 (5分钟到24小时)
- Interval 范围: [30, 3600] 秒 (30秒到1小时)
- Duration 必须是 Interval 的倍数
- PriceTolerance 范围: [0, 1000000) ppm (0% 到 100%)

**使用场景**:
- **机构交易**: 大额订单需要平滑执行,避免滑点
- **算法交易**: 追求更优的平均执行价格
- **风控需求**: 避免单笔大额订单触发市场保护机制
- **价格保护**: 使用 PriceTolerance 避免在不利价格执行

**TWAP 执行流程**:
```
1. 用户提交 TWAP 订单:
   - 总量: 100 BTC
   - 目标价格: 50,000 USDC
   - Duration: 7200 秒 (2小时)
   - Interval: 600 秒 (10分钟)
   - PriceTolerance: 10000 ppm (1%)

2. 系统计算:
   - 子订单数 = 7200 / 600 = 12 个
   - 每个子订单数量 = 100 / 12 ≈ 8.33 BTC
   - RemainingLegs = 12

3. 系统在每个 EndBlocker 检查是否到达下一个执行时间
4. 到达执行时间时,生成一个子订单:
   - 价格: 50,000 USDC
   - 数量: 8.33 BTC
   - 如果当前市场价格在 [49,500, 50,500] 范围内,执行
   - 如果超出范围,跳过该子订单

5. RemainingLegs 减 1,更新 TWAP 订单状态
6. 重复步骤 3-5,直到 RemainingLegs = 0 或到达 Duration 结束
```

**数据访问**:
```go
// 读取
keeper.GetTwapOrderPlacement(ctx, orderId) -> TwapOrderPlacement
keeper.GetAllTwapOrders(ctx) -> []TwapOrderPlacement

// 写入
keeper.SetTwapOrderPlacement(ctx, twapPlacement)

// 更新剩余腿数
keeper.UpdateTwapRemainingLegs(ctx, orderId, remainingLegs)

// 删除
keeper.RemoveTwapOrderPlacement(ctx, orderId)
```

**与子订单的关系**:
- TWAP 母订单本身不进入订单簿匹配
- 系统生成的子订单是独立的 Short-Term 订单,进入 MemClob 进行匹配
- 子订单的 `OrderFlags` 包含 `TwapSuborder` 标志(256),标识其来源于 TWAP
- 子订单信息存储在下一节描述的 TwapSuborderInfo 中

**关键文件**:
- `protocol/x/clob/types/order.pb.go` (TwapOrderPlacement 定义)
- `protocol/x/clob/keeper/twap_order_state.go`

---

#### 1.4.2 TWAP 子订单信息

**存储键**: `TWAPTriggerOrderKeyPrefix` + `OrderId` + `<子订单索引>` = `"TWAP/T:" + <OrderId> + <Index>`

**数据结构**: `TwapSuborderInfo`

```go
type TwapSuborderInfo struct {
    SuborderIndex uint32      // 子订单索引(0, 1, 2, ...)
    ExecutionTime uint32      // 计划执行时间(Unix时间戳)
    Status TwapSuborderStatus // 子订单状态
    ExecutedQuantums uint64   // 已执行数量
}

type TwapSuborderStatus int32
const (
    TWAP_SUBORDER_STATUS_PENDING = 0    // 待执行
    TWAP_SUBORDER_STATUS_EXECUTED = 1   // 已执行
    TWAP_SUBORDER_STATUS_SKIPPED = 2    // 已跳过(价格超出容差)
    TWAP_SUBORDER_STATUS_CANCELLED = 3  // 已取消(主订单取消)
)
```

**业务含义**:

- 跟踪每个 TWAP 子订单的执行状态和进度
- 记录每个子订单的计划执行时间和实际执行结果
- 支持子订单级别的状态查询和审计
- `Status` 区分不同的执行结果:
  - `PENDING`: 尚未到达执行时间
  - `EXECUTED`: 已成功执行
  - `SKIPPED`: 价格超出容差,自动跳过
  - `CANCELLED`: 用户提前取消 TWAP 订单,后续子订单标记为取消

**使用场景**:
- **子订单生成**: 系统生成子订单时创建 `TwapSuborderInfo`,状态为 `PENDING`
- **子订单执行**: 执行后更新状态为 `EXECUTED` 或 `SKIPPED`,记录成交数量
- **状态查询**: 用户查询 TWAP 订单的详细执行情况,包括每个子订单的状态
- **审计与分析**: 系统管理员或用户分析 TWAP 订单的执行质量

**数据访问**:
```go
// 读取
keeper.GetTwapSuborderInfo(ctx, orderId, suborderIndex) -> TwapSuborderInfo

// 写入
keeper.SetTwapSuborderInfo(ctx, orderId, suborderIndex, info)

// 更新状态
keeper.UpdateTwapSuborderStatus(ctx, orderId, suborderIndex, status)

// 批量查询
keeper.GetAllTwapSuborderInfo(ctx, orderId) -> []TwapSuborderInfo
```

**执行流程示例**:
```
TWAP 订单: 12 个子订单

Suborder 0: ExecutionTime=T0, Status=EXECUTED, ExecutedQuantums=8.33 BTC
Suborder 1: ExecutionTime=T0+10min, Status=EXECUTED, ExecutedQuantums=8.33 BTC
Suborder 2: ExecutionTime=T0+20min, Status=SKIPPED (价格超出容差)
Suborder 3: ExecutionTime=T0+30min, Status=EXECUTED, ExecutedQuantums=8.33 BTC
...
Suborder 11: ExecutionTime=T0+110min, Status=PENDING (尚未到达)
```

**关键文件**:
- `protocol/x/clob/keeper/twap_order_state.go`

---

### 1.5 订单成交量记录

**存储键**: `OrderAmountFilledKeyPrefix` + `OrderId` = `"Fill:" + <OrderId>`

**数据结构**: `OrderFillState`

```go
type OrderFillState struct {
    FillAmount uint64          // 已成交数量(base quantums)
    PrunableBlockHeight uint32 // 可修剪的区块高度
}
```

**业务含义**:
- 记录订单的已成交数量,用于计算剩余未成交数量
- `FillAmount` 累加每次匹配的成交量
- `PrunableBlockHeight` 标记订单何时可以被清理(过期或完全成交后)
- 支持部分成交场景,订单可以分多次匹配成交

**使用场景**:
- **订单匹配**: 检查订单剩余可成交数量,避免超量成交
- **订单查询**: 用户查询订单当前成交状态
- **状态清理**: 订单完全成交或过期后,删除成交量记录

**成交量计算**:
```go
// 剩余可成交数量
remainingQuantums := order.Quantums - orderFillState.FillAmount

// 检查是否可以成交
if matchQuantums > remainingQuantums {
    return ErrOverfill
}

// 更新成交量
orderFillState.FillAmount += matchQuantums
```

**数据访问**:
```go
// 读取
keeper.GetOrderFillAmount(ctx, orderId) -> uint64

// 写入/更新
keeper.SetOrderFillAmount(ctx, orderId, fillState)

// 删除(订单完成后)
keeper.DeleteOrderFillAmount(ctx, orderId)
```

**修剪机制**:
- 订单完全成交或过期后,`PrunableBlockHeight` 设置为当前区块高度 + N
- 定期清理任务扫描并删除过期的成交量记录,防止状态膨胀

**关键文件**:
- `protocol/x/clob/types/order.pb.go:335-396` (OrderFillState)
- `protocol/x/clob/keeper/order_fill_state.go`

---

### 1.6 订单过期索引

**存储键**: `StatefulOrdersExpirationsKeyPrefix` + `GoodTilBlockTime` + `OrderId`
= `"Exp/<timestamp>:" + <OrderId>`

**数据结构**: `StatefulOrderTimeSliceValue`

```go
type StatefulOrderTimeSliceValue struct {
    OrderIds []OrderId  // 在此时间过期的订单 ID 列表
}
```

**业务含义**:
- 按过期时间索引订单,便于快速查找和清理过期订单
- 键中包含过期时间戳,值为过期时间相同的订单列表
- 支持批量清理,提高效率

**使用场景**:
- **订单过期清理**: 每个区块扫描当前时间之前的过期索引,批量删除订单
- **订单簿维护**: 确保订单簿中只包含有效订单
- **状态修剪**: 定期清理过期订单,减少状态存储

**清理流程**:
```
每个区块 EndBlock:
1. 获取当前区块时间戳 currentTime
2. 遍历所有 key 匹配 "Exp/<timestamp>:" 且 timestamp <= currentTime 的记录
3. 读取每个过期时间点的订单 ID 列表
4. 批量删除对应的订单(从 StateStore 和 MemClob)
5. 删除过期索引本身
```

**数据访问**:
```go
// 写入(订单放置时)
keeper.AddOrderToExpiration(ctx, orderId, goodTilBlockTime)

// 读取(清理时)
keeper.GetExpiredOrders(ctx, currentTime) -> []OrderId

// 删除(清理后)
keeper.RemoveExpiredOrders(ctx, expirationTime)
```

**性能优化**:
- 使用时间戳作为键前缀,支持高效的范围查询
- 批量处理同一时间过期的订单,减少数据库访问次数

**关键文件**:
- `protocol/x/clob/types/order.pb.go:398-446` (StatefulOrderTimeSliceValue)
- `protocol/x/clob/keeper/order_expiration.go`

---

### 1.7 TWAP 子订单触发索引

**存储键**: `TWAPTriggerOrderKeyPrefix` + `SuborderId` = `"TWAP/T:" + <OrderId>`

**数据结构**: 存储的是子订单的触发信息,具体结构待确认(可能是 TransactionOrdering 或订单状态)

**业务含义**:
- 存储 TWAP 子订单的触发记录
- 跟踪 TWAP 订单的执行进度
- 确保子订单按计划触发和执行

**使用场景**:
- TWAP 订单执行过程中,记录每个子订单的触发状态
- 系统重启后恢复 TWAP 执行进度
- 审计和查询 TWAP 订单的执行历史

**关键文件**:
- `protocol/x/clob/keeper/twap_order.go`

---

### 1.8 清算配置

**存储键**: `LiquidationsConfigKey` = `"LiqCfg"`

**数据结构**: `LiquidationsConfig`

```go
type LiquidationsConfig struct {
    MaxLiquidationFeePpm uint32         // 最大清算手续费(PPM,百万分率)
    PositionBlockLimits PositionBlockLimits    // 单个仓位的区块清算限制
    SubaccountBlockLimits SubaccountBlockLimits  // 单个子账户的区块清算限制
    FillablePriceConfig FillablePriceConfig    // 清算价格差价配置
}

type PositionBlockLimits struct {
    MinPositionNotionalLiquidated uint64  // 每次清算的最小名义价值(quote quantums)
    MaxPositionPortionLiquidatedPpm uint32  // 每个区块最大清算比例(PPM)
}

type SubaccountBlockLimits struct {
    MaxNotionalLiquidated uint64        // 单个区块最大清算名义价值
    MaxQuantumsInsuranceLost uint64     // 保险基金最大损失限额
}

type FillablePriceConfig struct {
    BankruptcyAdjustmentPpm uint32      // 破产调整系数(PPM)
    SpreadToMaintenanceMarginRatioPpm uint32  // 差价与维持保证金比率
}
```

**业务含义**:
- 全局清算参数配置,控制清算流程的关键参数
- `MaxLiquidationFeePpm`: 清算手续费上限,100% 进入保险基金
- `PositionBlockLimits`: 限制单个区块清算单个仓位的数量,避免市场冲击
  - `MinPositionNotionalLiquidated`: 最小清算金额,确保清算有意义
  - `MaxPositionPortionLiquidatedPpm`: 例如 100000 = 10%,每次最多清算仓位的 10%
- `SubaccountBlockLimits`: 限制单个账户在单个区块的清算量,防止连锁清算
- `FillablePriceConfig`: 控制清算订单的成交价格差价
  - 根据账户破产风险调整可成交价格范围
  - 避免清算订单以极端价格成交,损害市场公平性

**使用场景**:
- **清算执行**: 读取配置计算清算订单的数量和价格
- **风险管理**: 根据市场状况调整清算参数(通过治理)
- **保险基金保护**: 限制单个区块对保险基金的最大损失

**配置示例**:
```json
{
  "max_liquidation_fee_ppm": 50000,  // 5% 清算手续费
  "position_block_limits": {
    "min_position_notional_liquidated": 1000000,  // 最小清算 1000 USDT
    "max_position_portion_liquidated_ppm": 100000  // 每次最多清算 10%
  },
  "subaccount_block_limits": {
    "max_notional_liquidated": 10000000,  // 单个区块最多清算 10000 USDT
    "max_quantums_insurance_lost": 5000000  // 保险基金最大损失 5000 USDT
  }
}
```

**数据访问**:
```go
// 读取
keeper.GetLiquidationsConfig(ctx) -> LiquidationsConfig

// 写入(通过治理或初始化)
keeper.SetLiquidationsConfig(ctx, config)
```

**关键文件**:
- `protocol/x/clob/types/liquidations_config.pb.go:27-101`
- `protocol/x/clob/keeper/liquidations_config.go`

---

#### 1.8.2 去杠杆数据结构

去杠杆机制没有独立的持久化配置,而是通过操作队列中的 `MatchPerpetualDeleveraging` 数据结构来表示。

**数据结构**: `MatchPerpetualDeleveraging`

```protobuf
message MatchPerpetualDeleveraging {
  // 被去杠杆的子账户ID
  SubaccountId liquidated = 1;

  // 永续合约ID
  uint32 perpetual_id = 2;

  // 去杠杆成交列表
  repeated MatchPerpetualFill fills = 3;

  // 是否为最终结算触发的去杠杆
  // true: 使用预言机价格 (市场关闭)
  // false: 使用破产价格 (账户负资产)
  bool is_final_settlement = 4;
}

message MatchPerpetualFill {
  // 对手方子账户ID (承担去杠杆的账户)
  SubaccountId offsetting_subaccount_id = 1;

  // 成交数量 (base quantums)
  uint64 fill_amount = 2;
}
```

**业务含义**:

- **两种触发场景**:
  1. **负净抵押品 (Negative TNC)**: `is_final_settlement = false`
     - 账户资不抵债 (TNC < 0)
     - 使用破产价格结算
     - 被去杠杆账户最终余额归零
  2. **最终结算 (Final Settlement)**: `is_final_settlement = true`
     - 市场永久关闭 (Status = FINAL_SETTLEMENT)
     - 使用预言机价格结算
     - 所有持仓强制平仓

- **fills 列表**:
  - 一个被去杠杆账户可能与多个对手方成交
  - 对手方按盈利率从高到低排序
  - 每个 fill 记录对手方账户和成交数量

**数据验证规则**:

```go
// 验证逻辑 (protocol/x/clob/types/match_perpetual_deleveraging.go:13-40)
func (match *MatchPerpetualDeleveraging) Validate() error {
    // 1. 被去杠杆账户ID必须有效
    if err := liquidatedSubaccountId.Validate(); err != nil {
        return err
    }

    // 2. 允许零成交 (用于提现门控)
    // 零成交去杠杆用于标记负TNC账户,阻止提现

    // 3. 每个fill的成交量必须 > 0
    for _, fill := range fills {
        if fill.GetFillAmount() == 0 {
            return ErrZeroDeleveragingFillAmount
        }
    }

    // 4. 对手方账户ID不能与被去杠杆账户相同
    if offsettingSubaccountId == liquidatedSubaccountId {
        return ErrDeleveragingAgainstSelf
    }

    // 5. 对手方账户不能重复
    // 每个对手方账户只能出现一次

    return nil
}
```

**使用场景**:

1. **负TNC去杠杆流程**:
   ```
   1. keeper.CanDeleverageSubaccount(ctx, subaccountId, perpetualId)
      → 检查 TNC < 0
   2. keeper.MaybeDeleverageSubaccount(ctx, ...)
      → 调用 memclob.DeleverageSubaccount()
   3. memclob查找反向持仓账户,按盈利率排序
   4. 生成 MatchPerpetualDeleveraging (is_final_settlement=false)
   5. 添加到操作队列 (operationsToPropose)
   6. PrepareProposal时包含在 MsgProposedOperations 中
   7. DeliverTx时执行去杠杆,更新账户余额
   ```

2. **最终结算去杠杆流程**:
   ```
   1. 治理提案修改 ClobPair.Status = FINAL_SETTLEMENT
   2. PrepareCheckState时扫描所有持仓账户
   3. 对每个持仓调用 DeleverageSubaccount(is_final_settlement=true)
   4. 使用预言机价格生成去杠杆成交
   5. 所有持仓强制平仓
   ```

3. **提现门控 (零成交去杠杆)**:
   ```
   1. 清算/去杠杆后仍有负TNC账户
   2. 生成零成交去杠杆操作 (fills=[])
   3. 提现逻辑检测到未处理的去杠杆操作
   4. 阻止提现,直到负TNC账户解决
   ```

**数据访问**:

```go
// 去杠杆操作通过操作队列传递,不直接存储在KVStore
// 而是在操作队列中临时存在

// 检查是否可以去杠杆
shouldDeleverageAtBankruptcyPrice, shouldDeleverageAtOraclePrice, err :=
    keeper.CanDeleverageSubaccount(ctx, subaccountId, perpetualId)

// 执行去杠杆
quantumsDeleveraged, err := keeper.MaybeDeleverageSubaccount(
    ctx, subaccountId, perpetualId,
)

// 内部会调用 memclob.DeleverageSubaccount() 生成操作
```

**关键文件**:
- `protocol/x/clob/types/match_perpetual_deleveraging.go` - 数据结构定义和验证
- `protocol/x/clob/keeper/deleveraging.go` - 去杠杆业务逻辑
- `protocol/x/clob/memclob/memclob.go:722-758` - 去杠杆匹配引擎
- `protocol/x/clob/keeper/process_operations.go:724-850` - 去杠杆执行逻辑

---

### 1.9 权益层级限制配置

**存储键**: `EquityTierLimitConfigKey` = `"EqTierCfg"`

**数据结构**: `EquityTierLimitConfiguration`

```go
type EquityTierLimitConfiguration struct {
    ShortTermOrderEquityTiers []EquityTierLimit  // 短期订单权益层级限制
    StatefulOrderEquityTiers []EquityTierLimit   // 有状态订单权益层级限制
}

type EquityTierLimit struct {
    UsdTncRequired SerializableInt  // 所需的 USDC TNC(总净抵押品)
    Limit uint32                    // 对应的订单数量限制
}
```

**业务含义**:
- 根据账户净值(TNC = Total Net Collateral)限制订单数量
- 防止小额账户垃圾订单攻击,保护订单簿性能
- 激励用户存入更多资金,提高市场深度
- 区分短期订单和有状态订单(Long-Term/Conditional)的限制

**权益层级示例**:
```json
{
  "short_term_order_equity_tiers": [
    {"usd_tnc_required": "0", "limit": 10},      // 0-100 USDT: 最多 10 个短期订单
    {"usd_tnc_required": "100", "limit": 50},    // 100-1000 USDT: 最多 50 个
    {"usd_tnc_required": "1000", "limit": 200},  // 1000+ USDT: 最多 200 个
  ],
  "stateful_order_equity_tiers": [
    {"usd_tnc_required": "0", "limit": 5},       // 0-100 USDT: 最多 5 个有状态订单
    {"usd_tnc_required": "100", "limit": 20},    // 100-1000 USDT: 最多 20 个
    {"usd_tnc_required": "1000", "limit": 100},  // 1000+ USDT: 最多 100 个
  ]
}
```

**使用场景**:
- **订单验证**: 放置订单前检查账户权益层级,拒绝超限订单
- **防止垃圾订单**: 限制小额账户的订单数量,保护系统资源
- **激励机制**: 鼓励用户存入更多资金以获得更高订单限额

**验证逻辑**:
```go
// 放置订单时
accountTNC := subaccountsKeeper.GetNetCollateral(ctx, subaccountId)
tier := findEquityTier(accountTNC, config.ShortTermOrderEquityTiers)
currentOrderCount := keeper.GetOpenOrderCount(ctx, subaccountId)

if currentOrderCount >= tier.Limit {
    return ErrExceededOrderLimit
}
```

**数据访问**:
```go
// 读取
keeper.GetEquityTierLimitConfiguration(ctx) -> EquityTierLimitConfiguration

// 写入(通过治理)
keeper.SetEquityTierLimitConfiguration(ctx, config)
```

**关键文件**:
- `protocol/x/clob/types/equity_tier_limit_config.pb.go:29-83`
- `protocol/x/clob/keeper/equity_tier_limit.go`

---

### 1.10 区块速率限制配置

**存储键**: `BlockRateLimitConfigKey` = `"RateLimCfg"`

**数据结构**: `BlockRateLimitConfiguration`

```go
type BlockRateLimitConfiguration struct {
    // 已废弃,使用 MaxShortTermOrdersAndCancelsPerNBlocks 代替
    MaxShortTermOrdersPerNBlocks []MaxPerNBlocksRateLimit

    MaxStatefulOrdersPerNBlocks []MaxPerNBlocksRateLimit  // 有状态订单速率限制

    // 已废弃,使用 MaxShortTermOrdersAndCancelsPerNBlocks 代替
    MaxShortTermOrderCancellationsPerNBlocks []MaxPerNBlocksRateLimit

    // 短期订单和取消的合并速率限制(v5.x 起使用)
    MaxShortTermOrdersAndCancelsPerNBlocks []MaxPerNBlocksRateLimit

    MaxLeverageUpdatesPerNBlocks []MaxPerNBlocksRateLimit  // 杠杆更新速率限制
}

type MaxPerNBlocksRateLimit struct {
    NumBlocks uint32  // 统计窗口(区块数)
    Limit uint32      // 限制次数
}
```

**业务含义**:
- 限制账户在一定区块数内的操作频率,防止垃圾攻击和速率滥用
- 速率限制采用 **AND 逻辑**,订单必须通过所有配置的速率限制
- 支持多级速率限制,例如:
  - 每 1 个区块最多 5 次操作
  - 每 10 个区块最多 30 次操作
  - 每 100 个区块最多 200 次操作
- 限制包括成功和失败的操作,避免通过失败操作绕过限制

**配置示例**:
```json
{
  "max_short_term_orders_and_cancels_per_n_blocks": [
    {"num_blocks": 1, "limit": 5},      // 每个区块最多 5 次
    {"num_blocks": 10, "limit": 30},    // 每 10 个区块最多 30 次
    {"num_blocks": 100, "limit": 200}   // 每 100 个区块最多 200 次
  ],
  "max_stateful_orders_per_n_blocks": [
    {"num_blocks": 1, "limit": 2},
    {"num_blocks": 100, "limit": 50}
  ],
  "max_leverage_updates_per_n_blocks": [
    {"num_blocks": 1, "limit": 1},      // 每个区块最多更新 1 次杠杆
    {"num_blocks": 100, "limit": 10}
  ]
}
```

**使用场景**:
- **防止垃圾订单**: 限制高频订单提交,保护网络带宽和计算资源
- **防止市场操纵**: 限制单个账户短时间内大量订单操作
- **保护共识**: 避免单个账户占用过多区块空间

**验证逻辑**:
```go
// 放置订单前
for _, rateLimit := range config.MaxShortTermOrdersAndCancelsPerNBlocks {
    count := keeper.GetOperationCount(ctx, subaccountId, rateLimit.NumBlocks)
    if count >= rateLimit.Limit {
        return ErrRateLimitExceeded
    }
}

// 更新计数器
keeper.IncrementOperationCount(ctx, subaccountId)
```

**数据访问**:
```go
// 读取
keeper.GetBlockRateLimitConfiguration(ctx) -> BlockRateLimitConfiguration

// 写入(通过治理)
keeper.SetBlockRateLimitConfiguration(ctx, config)
```

**关键文件**:
- `protocol/x/clob/types/block_rate_limit_config.pb.go:27-67`
- `protocol/x/clob/rate_limit/rate_limiter.go`
- `protocol/x/clob/keeper/rate_limit.go`

---

### 1.11 下一个 CLOB 交易对 ID

**存储键**: `NextClobPairIDKey` = `"NextClobPairID"`

**数据结构**: `uint32`

**业务含义**:
- 存储下一个可用的 CLOB 交易对 ID
- 每次创建新交易对时自动递增
- 确保交易对 ID 的唯一性

**使用场景**:
- **创建新交易对**: 读取当前 ID,分配给新交易对,然后递增
- **无权限上市(Listing)**: 自动生成新的交易对 ID

**数据访问**:
```go
// 读取
keeper.GetNextClobPairID(ctx) -> uint32

// 递增
keeper.IncrementNextClobPairID(ctx)
```

**关键文件**:
- `protocol/x/clob/keeper/clob_pair.go`

---

### 1.12 杠杆设置 (存储在 CLOB 模块)

**存储键**: `LeverageKeyPrefix` + `SubaccountId` = `"Leverage:" + <SubaccountId>`

**数据结构**: `Leverage` (定义在 Subaccounts 模块)

```go
// 注意:具体结构定义在 subaccounts 模块
type Leverage struct {
    SubaccountId SubaccountId  // 子账户 ID
    Leverage uint32            // 杠杆倍数(例如 20 表示 20x)
}
```

**业务含义**:
- 尽管杠杆数据结构定义在 Subaccounts 模块,但存储在 CLOB 模块的 StateStore
- 这是一个跨模块数据存储的设计决策
- 杠杆倍数影响订单的保证金要求和风险管理

**使用场景**:
- **订单验证**: 放置订单时根据杠杆计算所需保证金
- **保证金计算**: 结合持仓和杠杆计算初始保证金(IM)和维持保证金(MM)
- **清算判断**: 杠杆影响清算价格阈值

**数据访问**:
```go
// 读取
keeper.GetLeverage(ctx, subaccountId) -> Leverage

// 写入/更新
keeper.SetLeverage(ctx, subaccountId, leverage)
```

**设计说明**:
- Leverage 模块在早期版本中是独立模块,后来被整合到 CLOB 和 Subaccounts 模块
- 数据存储在 CLOB,但逻辑分散在 CLOB 和 Subaccounts 模块
- 这是一个遗留设计,未来可能重构

**关键文件**:
- `protocol/x/clob/keeper/leverage.go`
- `protocol/x/subaccounts/types/leverage.go`

---

### 1.13 有状态订单计数

**存储键**: 不直接存储,通过迭代所有有状态订单计算

**业务含义**:
- 跟踪账户的有状态订单数量(Long-Term + Conditional)
- 用于权益层级限制验证
- 可能缓存在 MemStore 或 TransientStore 中以提高性能

**使用场景**:
- 验证账户是否超过有状态订单限额
- 查询账户当前活跃订单数量

**关键文件**:
- `protocol/x/clob/keeper/stateful_order.go`

---

## 二、MemStore (内存存储,从 StateStore 同步)

MemStore 是从 StateStore 同步的内存缓存,提供快速访问。数据在应用启动时从 StateStore 加载,在运行时保持同步更新。节点重启后会重新加载。

### 2.1 MemStore 初始化标志

**存储键**: `KeyMemstoreInitialized` = `"MemstoreInit"`

**数据结构**: `bool`

**业务含义**:
- 标记 MemStore 是否已经初始化
- 防止重复初始化导致数据不一致
- 应用启动时检查此标志,决定是否需要从 StateStore 加载数据

**使用场景**:

- **应用启动**: 检查标志,未初始化则从 StateStore 加载所有 CLOB 交易对和有状态订单
- **节点重启**: 重新加载数据到 MemStore

**初始化流程**:
```
应用启动:
1. 检查 KeyMemstoreInitialized
2. 如果未初始化:
   a. 从 StateStore 读取所有 ClobPair
   b. 从 StateStore 读取所有 Long-Term 和 Conditional 订单
   c. 加载到 MemClob 订单簿
   d. 设置 KeyMemstoreInitialized = true
3. 如果已初始化:跳过加载
```

**数据访问**:
```go
// 检查
isInitialized := keeper.IsMemStoreInitialized(ctx)

// 设置
keeper.SetMemStoreInitialized(ctx)
```

**关键文件**:
- `protocol/x/clob/keeper/keeper.go:Initialize()`
- `protocol/x/clob/abci.go:PreBlocker()`

---

### 2.2 提议者匹配事件

**存储键**: `ProcessProposerMatchesEventsKey` = `"ProposerEvents"`

**数据结构**: `ProcessProposerMatchesEvents` (具体结构待确认,可能包含匹配结果、成交量等)

**业务含义**:
- 存储当前区块提议者生成的匹配事件
- 用于 PrepareCheckState 阶段重放匹配结果,更新本地 MemClob 状态
- 确保所有节点的 MemClob 状态一致

**使用场景**:
- **FinalizeBlock**: 提议者执行匹配后,将匹配事件存储到 MemStore
- **PrepareCheckState**: 其他节点读取匹配事件,更新本地 MemClob
- **状态同步**: 确保所有节点的订单簿状态与链上一致

**数据流**:
```
FinalizeBlock (所有节点):
1. 执行 MsgProposedOperations
2. 生成匹配结果和事件
3. 存储匹配事件到 MemStore

PrepareCheckState (所有节点):
1. 读取 MemStore 中的匹配事件
2. 重放匹配结果到本地 MemClob
3. 更新订单簿状态(删除成交订单、更新部分成交)
```

**关键文件**:
- `protocol/x/clob/keeper/process_operations.go`
- `protocol/x/clob/abci.go:PrepareCheckState()`

---

### 2.3 已交付 Long-Term 订单索引

**存储键前缀**: `OrderedDeliveredLongTermOrderKeyPrefix` = `"DLTO:"`
**索引键**: `OrderedDeliveredLongTermOrderIndexKey` = `"DLTOIdx"`

**数据结构**:
- 索引: `uint32` (下一个可用索引)
- 订单: `LongTermOrderPlacement` (存储在 `DLTO:<index>`)

**业务含义**:
- 在 FinalizeBlock 阶段记录已交付(DeliverTx)的 Long-Term 订单
- 保持订单的交付顺序,用于 PrepareCheckState 阶段重放
- 确保订单簿状态与链上交易顺序一致

**使用场景**:
- **DeliverTx**: 记录新放置的 Long-Term 订单及其索引
- **PrepareCheckState**: 按索引顺序重放订单,重建 MemClob 状态

**数据访问**:
```go
// 写入(DeliverTx)
index := keeper.GetNextDeliveredLongTermOrderIndex(ctx)
keeper.SetDeliveredLongTermOrder(ctx, index, orderPlacement)
keeper.IncrementDeliveredLongTermOrderIndex(ctx)

// 读取(PrepareCheckState)
orders := keeper.GetAllDeliveredLongTermOrders(ctx)
```

**关键文件**:
- `protocol/x/clob/keeper/delivered_orders.go`

---

### 2.4 已交付 Conditional 订单索引

**存储键前缀**: `OrderedDeliveredConditionalOrderKeyPrefix` = `"DCIdx:"`
**索引键**: `OrderedDeliveredConditionalOrderIndexKey` = `"DCOIdx"`

**数据结构**:

- 索引: `uint32`
- 订单: `ConditionalOrderPlacement`

**业务含义**:
- 与 Long-Term 订单索引类似,记录已交付的 Conditional 订单
- 保持交付顺序,支持状态重放

**使用场景**:
- **DeliverTx**: 记录新触发的 Conditional 订单
- **PrepareCheckState**: 按顺序重放,更新 MemClob

**数据访问**:
```go
// 写入
index := keeper.GetNextDeliveredConditionalOrderIndex(ctx)
keeper.SetDeliveredConditionalOrder(ctx, index, orderPlacement)

// 读取
orders := keeper.GetAllDeliveredConditionalOrders(ctx)
```

**关键文件**:
- `protocol/x/clob/keeper/delivered_orders.go`

---

### 2.5 已交付订单取消

**存储键前缀**: `DeliveredCancelKeyPrefix` = `"DCancel:"`

**数据结构**: `OrderId` (被取消的订单 ID)

**业务含义**:
- 记录在 DeliverTx 阶段成功执行的订单取消操作
- 用于 PrepareCheckState 阶段重放,从 MemClob 删除订单

**使用场景**:
- **DeliverTx**: 记录取消的订单 ID
- **PrepareCheckState**: 重放取消操作,更新 MemClob

**数据访问**:
```go
// 写入
keeper.SetDeliveredCancel(ctx, orderId)

// 读取
cancelledOrderIds := keeper.GetAllDeliveredCancels(ctx)
```

**关键文件**:
- `protocol/x/clob/keeper/delivered_cancels.go`

---

### 2.6 有状态订单计数缓存

**存储键前缀**: `StatefulOrderCountPrefix` = `"NumSO:"`

**数据结构**: `uint32` (某个账户的有状态订单数量)

**业务含义**:
- 缓存账户的有状态订单数量,避免每次验证时遍历所有订单
- 提高订单验证性能,特别是在权益层级限制检查时

**使用场景**:
- **订单放置**: 快速检查账户是否超过订单限额
- **订单取消**: 减少计数器
- **订单成交**: 减少计数器

**数据访问**:
```go
// 读取
count := keeper.GetStatefulOrderCount(ctx, subaccountId)

// 递增
keeper.IncrementStatefulOrderCount(ctx, subaccountId)

// 递减
keeper.DecrementStatefulOrderCount(ctx, subaccountId)
```

**关键文件**:
- `protocol/x/clob/keeper/stateful_order_count.go`

---

## 三、TransientStore (瞬态存储,每个区块结束后清空)

TransientStore 的数据仅在单个区块内有效,区块结束后自动清空。用于存储临时状态和区块级作用域数据。

### 3.1 子账户清算信息

**存储键前缀**: `SubaccountLiquidationInfoKeyPrefix` + `SubaccountId` = `"SaLiqInfo:" + <SubaccountId>`

**数据结构**: `SubaccountLiquidationInfo`

```go
type SubaccountLiquidationInfo struct {
    PerpetualsLiquidated []PerpetualLiquidationInfo  // 被清算的永续合约列表
    NotionalLiquidated uint64                        // 已清算的名义价值
    QuantumsInsuranceLost uint64                     // 保险基金损失
}

type PerpetualLiquidationInfo struct {
    PerpetualId uint32        // 永续合约 ID
    NotionalLiquidated uint64 // 该合约已清算的名义价值
}
```

**业务含义**:
- 记录单个区块内对某个子账户的清算操作
- 跟踪清算的永续合约、清算金额和保险基金损失
- 用于执行区块级清算限制(SubaccountBlockLimits)
- 防止单个区块过度清算同一账户

**使用场景**:
- **清算执行**: 执行清算前检查是否超过区块限制
- **清算记录**: 每次清算后更新清算信息
- **风险控制**: 限制单个区块对单个账户的清算量

**清算限制检查**:
```go
// 执行清算前
liquidationInfo := keeper.GetSubaccountLiquidationInfo(ctx, subaccountId)
config := keeper.GetLiquidationsConfig(ctx)

if liquidationInfo.NotionalLiquidated + newLiquidationAmount > config.SubaccountBlockLimits.MaxNotionalLiquidated {
    return ErrExceededLiquidationLimit
}

// 执行清算后更新
liquidationInfo.NotionalLiquidated += newLiquidationAmount
keeper.SetSubaccountLiquidationInfo(ctx, subaccountId, liquidationInfo)
```

**数据访问**:
```go
// 读取
keeper.GetSubaccountLiquidationInfo(ctx, subaccountId) -> SubaccountLiquidationInfo

// 写入/更新
keeper.SetSubaccountLiquidationInfo(ctx, subaccountId, info)
```

**区块结束清空**:
- TransientStore 的特性自动清空,无需手动删除

**关键文件**:
- `protocol/x/clob/types/liquidations.pb.go:88-154`
- `protocol/x/clob/keeper/liquidations.go`

---

### 3.2 下一个有状态订单区块交易索引

**存储键**: `NextStatefulOrderBlockTransactionIndexKey` = `"NextTxIdx"`

**数据结构**: `uint32`

**业务含义**:
- 存储当前区块下一个可用的交易索引
- 用于为新放置的有状态订单分配 `TransactionOrdering`
- 确保同一区块内订单的放置顺序是确定性的

**使用场景**:
- **订单放置**: 分配交易索引给新订单
- **订单排序**: 价格相同时,按交易索引(时间优先)排序

**数据访问**:
```go
// 读取并递增
txIndex := keeper.GetAndIncrementNextStatefulOrderBlockTransactionIndex(ctx)
orderPlacement.PlacementIndex.TransactionIndex = txIndex
```

**区块结束清空**:
- 每个区块开始时自动重置为 0

**关键文件**:
- `protocol/x/clob/keeper/stateful_order.go`

---

### 3.3 未提交的有状态订单放置

**存储键前缀**: `UncommittedStatefulOrderPlacementKeyPrefix` + `OrderId` = `"UncmtSO:" + <OrderId>`

**数据结构**: `LongTermOrderPlacement` 或 `ConditionalOrderPlacement`

**业务含义**:
- 存储验证器本地知道的,但尚未提交到区块的有状态订单
- 用于 CheckTx 阶段的乐观处理,避免重复订单
- 提交到区块后移动到 StateStore,此处删除

**使用场景**:
- **CheckTx**: 订单通过验证后,临时存储到 TransientStore
- **DeliverTx**: 订单正式提交后,从 TransientStore 删除,写入 StateStore
- **订单去重**: 防止同一订单在 MemPool 中重复提交

**数据流**:
```
CheckTx (验证器本地):
1. 验证订单
2. 存储到 UncommittedStatefulOrderPlacement
3. 放入 MemPool

DeliverTx (所有节点):
1. 执行订单
2. 从 TransientStore 删除
3. 写入 StateStore
```

**数据访问**:
```go
// 写入(CheckTx)
keeper.SetUncommittedStatefulOrderPlacement(ctx, orderPlacement)

// 读取
keeper.GetUncommittedStatefulOrderPlacement(ctx, orderId) -> OrderPlacement

// 删除(DeliverTx)
keeper.DeleteUncommittedStatefulOrderPlacement(ctx, orderId)
```

**关键文件**:
- `protocol/x/clob/keeper/uncommitted_stateful_order.go`

---

### 3.4 未提交的有状态订单取消

**存储键前缀**: `UncommittedStatefulOrderCancellationKeyPrefix` + `OrderId` = `"UncmtSOCxl:" + <OrderId>`

**数据结构**: `OrderId`

**业务含义**:
- 存储验证器本地知道的,但尚未提交到区块的订单取消操作
- 用于 CheckTx 阶段避免取消同一订单多次
- 提交到区块后从 TransientStore 删除

**使用场景**:
- **CheckTx**: 取消订单通过验证后,临时记录
- **DeliverTx**: 取消操作正式提交后,删除记录
- **去重**: 防止同一取消操作在 MemPool 中重复

**数据访问**:
```go
// 写入(CheckTx)
keeper.SetUncommittedStatefulOrderCancellation(ctx, orderId)

// 检查
isUncommitted := keeper.HasUncommittedStatefulOrderCancellation(ctx, orderId)

// 删除(DeliverTx)
keeper.DeleteUncommittedStatefulOrderCancellation(ctx, orderId)
```

**关键文件**:
- `protocol/x/clob/keeper/uncommitted_stateful_order.go`

---

### 3.5 未提交的有状态订单计数

**存储键前缀**: `UncommittedStatefulOrderCountPrefix` + `SubaccountId` = `"NumUncmtSO:" + <SubaccountId>`

**数据结构**: `int32` (可以为负数,表示净放置数 = 放置 - 取消)

**业务含义**:
- 跟踪验证器本地未提交的有状态订单净变化
- 计数 = 未提交的放置数 - 未提交的取消数
- 用于 CheckTx 阶段验证订单限额,考虑未提交的订单

**使用场景**:
- **CheckTx 订单验证**: 计算实际订单数 = 已提交订单数 + 未提交订单计数
- **防止超限**: 即使订单尚未提交到区块,也要遵守限额

**计数逻辑**:
```go
// 放置订单(CheckTx)
keeper.IncrementUncommittedStatefulOrderCount(ctx, subaccountId)
uncommittedCount = 1

// 取消订单(CheckTx)
keeper.DecrementUncommittedStatefulOrderCount(ctx, subaccountId)
uncommittedCount = -1

// 验证订单限额
committedCount := keeper.GetStatefulOrderCount(ctx, subaccountId)
uncommittedCount := keeper.GetUncommittedStatefulOrderCount(ctx, subaccountId)
totalCount := committedCount + uncommittedCount

if totalCount >= limit {
    return ErrExceededOrderLimit
}
```

**数据访问**:
```go
// 读取
count := keeper.GetUncommittedStatefulOrderCount(ctx, subaccountId)

// 递增
keeper.IncrementUncommittedStatefulOrderCount(ctx, subaccountId)

// 递减
keeper.DecrementUncommittedStatefulOrderCount(ctx, subaccountId)
```

**区块结束清空**:
- 提交到区块后,未提交计数清零,转移到已提交计数

**关键文件**:
- `protocol/x/clob/keeper/uncommitted_stateful_order.go`

---

### 3.6 最低成交价格 (用于条件订单触发)

**存储键前缀**: `MinTradePricePrefix` + `PerpetualId` = `"MinTrade:" + <uint32>`

**数据结构**: `uint64` (subticks)

**业务含义**:
- 记录当前区块某个永续合约的**最低成交价格**
- 用于改进条件订单的触发逻辑,基于**实际成交价格**而非仅预言机价格
- 仅在区块内有效,区块结束后清空 (TransientStore 特性)

**为什么需要追踪成交价格?**

**问题**: 如果仅使用预言机价格触发条件订单,可能存在以下问题:
1. **延迟性**: 预言机价格更新频率可能低于区块内成交频率
2. **不准确性**: 预言机价格是市场平均价格,可能与实际成交价格有偏差
3. **错失触发**: 如果区块内价格短暂触及触发点,但预言机价格未更新,条件订单不会触发

**解决方案**: 结合预言机价格和区块内实际成交价格:
- **MinTradePrice**: 记录区块内最低成交价格,用于检测价格下跌触发
- **MaxTradePrice**: 记录区块内最高成交价格,用于检测价格上涨触发

**优势**:
1. **及时性**: 成交价格实时更新,触发更及时
2. **准确性**: 基于实际成交,而非估算价格
3. **公平性**: 如果区块内价格确实触及触发点,条件订单一定能触发

**使用场景**:
- **订单匹配时更新**: 每次订单匹配成交后,检查成交价格是否低于当前 MinTradePrice,如果是则更新
- **条件订单触发检查**: 在 `MaybeTriggerConditionalOrders` 函数中,同时检查预言机价格和 MinTradePrice
- **价格追踪**: 精确追踪区块内价格下限

**触发逻辑优化**:

**传统方式** (仅使用预言机价格):
```go
if oraclePrice <= stopLossPrice {
    触发止损单
}
```

**改进方式** (结合成交价格和预言机价格):
```go
minTradePrice := keeper.GetMinTradePrice(ctx, perpetualId)
oraclePrice := keeper.GetOraclePrice(ctx, perpetualId)

if minTradePrice <= stopLossPrice || oraclePrice <= stopLossPrice {
    触发止损单
}
```

**示例场景**:
```
区块 N:
- 预言机价格: 50,000 USDC (未更新)
- 区块内成交:
  - 成交 1: 49,800 USDC (更新 MinTradePrice = 49,800)
  - 成交 2: 50,200 USDC (不更新 MinTradePrice)
- 用户止损单: 触发价格 49,900 USDC

传统方式: 预言机价格 50,000 > 49,900,不触发 ❌
改进方式: MinTradePrice 49,800 < 49,900,触发 ✅
```

**数据访问**:
```go
// 写入/更新 (每次成交时)
if tradePrice < currentMinTradePrice {
    keeper.SetMinTradePrice(ctx, perpetualId, tradePrice)
}

// 读取 (触发检查时)
minPrice := keeper.GetMinTradePrice(ctx, perpetualId)
```

**区块结束清空**:
- TransientStore 自动清空,下个区块重新记录
- 每个区块开始时,MinTradePrice 初始化为最大值,MaxTradePrice 初始化为 0

**关键文件**:
- `protocol/x/clob/keeper/conditional_order_triggered.go`
- `protocol/x/clob/keeper/process_operations.go` (成交时更新)

---

### 3.7 最高成交价格 (用于条件订单触发)

**存储键前缀**: `MaxTradePricePrefix` + `PerpetualId` = `"MaxTrade:" + <uint32>`

**数据结构**: `uint64` (subticks)

**业务含义**:
- 记录当前区块某个永续合约的**最高成交价格**
- 与最低成交价格配合使用,覆盖区块内价格波动范围
- 提高条件订单触发的准确性和及时性

**使用场景**:
- **订单匹配时更新**: 每次订单匹配成交后,检查成交价格是否高于当前 MaxTradePrice,如果是则更新
- **条件订单触发检查**: 检查最高成交价格是否触发止盈单或买单止损
- **价格追踪**: 追踪区块内价格上限

**触发逻辑**:

**止盈单触发检查**:
```go
maxTradePrice := keeper.GetMaxTradePrice(ctx, perpetualId)
oraclePrice := keeper.GetOraclePrice(ctx, perpetualId)

if maxTradePrice >= takeProfitPrice || oraclePrice >= takeProfitPrice {
    触发止盈单
}
```

**示例场景**:
```
区块 N:
- 预言机价格: 50,000 USDC (未更新)
- 区块内成交:
  - 成交 1: 50,200 USDC (更新 MaxTradePrice = 50,200)
  - 成交 2: 49,800 USDC (不更新 MaxTradePrice)
- 用户止盈单: 触发价格 50,100 USDC

传统方式: 预言机价格 50,000 < 50,100,不触发 ❌
改进方式: MaxTradePrice 50,200 > 50,100,触发 ✅
```

**数据访问**:
```go
// 写入/更新 (每次成交时)
if tradePrice > currentMaxTradePrice {
    keeper.SetMaxTradePrice(ctx, perpetualId, tradePrice)
}

// 读取 (触发检查时)
maxPrice := keeper.GetMaxTradePrice(ctx, perpetualId)
```

**与 MinTradePrice 的配合**:
- **完整价格区间**: [MinTradePrice, MaxTradePrice] 覆盖区块内所有成交价格
- **双向触发**: MinTrade 用于下跌触发,MaxTrade 用于上涨触发
- **及时性保证**: 只要区块内价格触及触发点,条件订单必定触发

**性能优化**:
- 使用 TransientStore,自动清空,无需手动清理
- 只在区块内有效,避免状态膨胀
- 读取速度快 (内存访问)

**关键文件**:
- `protocol/x/clob/keeper/conditional_order_triggered.go`
- `protocol/x/clob/keeper/process_operations.go` (成交时更新)

---

### 3.8 FinalizeBlock 事件暂存

**存储键前缀**: `StagedEventsKeyPrefix` = `"StgEvt:"`
**事件计数键**: `StagedEventsCountKey` = `"StgEvtCnt"`

**数据结构**: `ClobStagedFinalizeBlockEvent`

**业务含义**:
- 在 FinalizeBlock 阶段暂存需要在 Precommit 阶段处理的事件
- 典型用途:MemClob 订单簿创建、索引器事件生成等副作用操作
- 分离确定性操作(FinalizeBlock)和非确定性副作用(Precommit)

**使用场景**:
- **FinalizeBlock**: 暂存事件到 TransientStore
- **Precommit**: 读取暂存事件并处理副作用

**事件类型示例**:
```go
type ClobStagedFinalizeBlockEvent struct {
    // 可能包含以下事件类型:
    // - MemClob 订单簿创建事件
    // - 索引器数据推送事件
    // - 流服务更新事件
}
```

**数据流**:
```
FinalizeBlock:
1. 执行确定性操作
2. 生成副作用事件
3. 暂存事件到 TransientStore

Precommit:
1. 读取暂存事件
2. 执行副作用操作(如创建 MemClob 订单簿)
3. 清空暂存事件(TransientStore 自动清空)
```

**数据访问**:
```go
// 暂存事件(FinalizeBlock)
keeper.StageFinalizeBlockEvent(ctx, event)

// 读取事件(Precommit)
events := keeper.GetStagedFinalizeBlockEvents(ctx)

// 处理事件
for _, event := range events {
    processEvent(event)
}
```

**关键文件**:
- `protocol/x/clob/abci.go:Precommit()`
- `protocol/finalizeblock/event_stager.go`

---

## 四、MemClob (纯内存订单簿,不持久化)

MemClob 是纯内存数据结构,存储在 Keeper 的内存中,不存储到任何 Store。应用重启后需要从 StateStore 重新加载。

### 4.1 MemClob 核心数据结构

**存储位置**: `keeper.MemClob` (Go 内存)

**核心接口**: `types.MemClob`

```go
type MemClob interface {
    // 订单簿操作
    CreateOrderbook(clobPair ClobPair) error
    PlaceOrder(ctx sdk.Context, order Order) (success bool, err error)
    CancelOrder(ctx sdk.Context, orderId OrderId) (success bool, err error)

    // 订单匹配
    MatchOrders(ctx sdk.Context, operations []Operation) (matches []Match, err error)

    // 状态查询
    GetOrder(orderId OrderId) (Order, bool)
    GetOrderbook(clobPairId ClobPairId) (Orderbook, bool)
    GetMidPrice(clobPairId ClobPairId) (Subticks, bool)

    // 状态管理
    SetClobKeeper(keeper ClobKeeper)
    PruneOrders(ctx sdk.Context, blockHeight uint32)
}
```

**主要实现**: `protocol/x/clob/memclob/memclob.go` (~10万+ 行)

**业务含义**:
- MemClob 是订单匹配引擎的核心,所有活跃订单都存储在内存订单簿中
- 实现了高性能的订单匹配算法(价格-时间优先)
- 不持久化,依赖 StateStore 恢复状态
- 确保匹配的确定性,相同输入必须产生相同输出

**数据结构组成**:
```go
type MemClobImpl struct {
    // 订单簿映射: ClobPairId -> Orderbook
    orderbooks map[ClobPairId]*Orderbook

    // 订单映射: OrderId -> Order (快速查找)
    orders map[OrderId]*Order

    // 待提议操作队列
    operationsToPropose OperationsQueue

    // Keeper 引用(用于读取 StateStore 成交量)
    clobKeeper ClobKeeper
}

type Orderbook struct {
    clobPairId ClobPairId

    // 买单订单簿(价格从高到低排序)
    bids *OrderbookSide

    // 卖单订单簿(价格从低到高排序)
    asks *OrderbookSide

    // 中间价格缓存
    midPrice Subticks
}

type OrderbookSide struct {
    // 价格层级: Price -> Level
    levels map[Subticks]*Level

    // 价格排序列表(支持快速遍历)
    sortedPrices []Subticks
}

type Level struct {
    price Subticks

    // 该价格层级的所有订单(按时间排序)
    orders []*Order

    // 总数量缓存
    totalQuantums uint64
}
```

**关键特性**:

1. **价格-时间优先匹配**:
   - 买单按价格从高到低排序,卖单从低到高排序
   - 同一价格层级内,订单按放置时间排序(先进先出)

2. **确定性保证**:
   - 使用确定性排序算法
   - 避免使用随机数、时间戳等不确定因素
   - 相同订单输入必须产生相同匹配结果

3. **性能优化**:
   - 订单快速查找: O(1)
   - 最优价格查找: O(1)
   - 订单匹配: O(N*M),N=买单数,M=卖单数

4. **内存管理**:
   - 订单成交或取消后立即从内存删除
   - 定期修剪过期订单
   - 限制订单簿深度,防止内存溢出

**使用场景**:
- **订单放置**: 验证通过后添加到订单簿
- **订单匹配**: EndBlock 阶段执行确定性匹配
- **订单取消**: 从订单簿移除订单
- **价格查询**: 获取订单簿中间价、最优买卖价

**与 StateStore 的关系**:
- **应用启动**: 从 StateStore 加载所有有状态订单到 MemClob
- **订单放置**: 同时写入 StateStore(持久化)和 MemClob(匹配)
- **订单成交**: 从 MemClob 删除,更新 StateStore 成交量
- **应用重启**: 从 StateStore 重新加载订单到 MemClob

**匹配算法示例**:
```
假设订单簿状态:
Bids:
  Price 100: [Order1(qty=10), Order2(qty=5)]
  Price 99:  [Order3(qty=20)]
Asks:
  Price 101: [Order4(qty=8)]
  Price 102: [Order5(qty=15)]

新卖单: Sell 12 @ 99 (市价单)

匹配过程:
1. 从最优买价(100)开始匹配
2. Order1 完全成交: 10 @ 100
3. Order2 部分成交: 2 @ 100 (剩余 3)
4. 卖单完全成交,匹配结束

匹配结果:
- Fills: [(Order1, 10), (Order2, 2)]
- 更新订单簿:移除 Order1,更新 Order2 剩余数量为 3
```

**关键文件**:
- `protocol/x/clob/memclob/memclob.go` (核心实现,~10万行)
- `protocol/x/clob/types/mem_clob.go` (接口定义)
- `protocol/x/clob/types/orderbook.go` (订单簿数据结构)

---

### 4.2 待提议操作队列

**存储位置**: `keeper.MemClob.OperationsToPropose` (Go 内存)

**数据结构**: `OperationsToPropose`

```go
type OperationsToPropose struct {
    // 短期订单放置操作
    ShortTermOrderPlacements []Order

    // 短期订单取消操作
    ShortTermOrderCancellations []OrderId

    // 有状态订单放置操作(Long-Term/Conditional)
    StatefulOrderPlacements []OrderId

    // 有状态订单取消操作
    StatefulOrderCancellations []OrderId

    // 清算订单
    Liquidations []LiquidationOrder

    // 去杠杆操作
    Deleverages []DeleverageOperation
}
```

**业务含义**:
- 验证器在 CheckTx 阶段接受的订单操作会加入此队列
- Proposer 在 PrepareProposal 阶段从队列中提取操作,构建 MsgProposedOperations
- 确保所有验证器知道的操作都有机会被包含到区块中

**使用场景**:
- **CheckTx**: 订单通过验证后,加入待提议队列
- **PrepareProposal**: Proposer 从队列中提取操作,组装区块提案
- **PrepareCheckState**: 区块确认后,清空队列,重放链上操作

**数据流**:
```
CheckTx (所有验证器):
1. 接收订单交易
2. 验证订单
3. 乐观匹配(更新本地 MemClob)
4. 添加到 OperationsToPropose

PrepareProposal (仅 Proposer):
1. 从 OperationsToPropose 读取操作
2. 构建 MsgProposedOperations
3. 组装区块提案

FinalizeBlock (所有验证器):
1. 执行 MsgProposedOperations
2. 确定性匹配

PrepareCheckState (所有验证器):
1. 清空本地 OperationsToPropose
2. 重放链上操作到本地 MemClob
```

**关键特性**:
- **乐观处理**: CheckTx 阶段乐观更新订单簿,提高用户体验
- **确定性执行**: FinalizeBlock 阶段确定性匹配,保证共识
- **状态同步**: PrepareCheckState 阶段同步本地状态与链上一致

**关键文件**:
- `protocol/x/clob/types/operations_to_propose.go`
- `protocol/x/clob/memclob/memclob.go` (操作队列管理)
- `protocol/app/prepare_proposal.go` (Proposer 逻辑)

---

### 4.3 订单簿快照 (用于查询和流服务)

**存储位置**: Go 内存,定期生成快照

**数据结构**: `Orderbook` (订单簿快照)

```go
type OrderbookSnapshot struct {
    ClobPairId uint32

    // 买单层级(价格从高到低)
    Bids []Level

    // 卖单层级(价格从低到高)
    Asks []Level

    // 快照时间戳
    Timestamp time.Time
}

type Level struct {
    Price Subticks      // 价格
    TotalQuantums uint64  // 总数量
    OrderCount uint32    // 订单数量
}
```

**业务含义**:
- 从 MemClob 生成订单簿快照,用于对外查询和流服务
- 不包含完整订单细节,仅包含价格层级聚合信息
- 减少数据传输量,提高查询性能

**使用场景**:
- **gRPC 查询**: 前端查询订单簿深度
- **WebSocket 流服务**: 实时推送订单簿更新
- **行情数据**: 生成市场行情和K线数据

**快照生成**:
```go
// 从 MemClob 生成快照
snapshot := memclob.GetOrderbookSnapshot(ctx, clobPairId)

// 聚合价格层级
for price, level := range orderbook.bids.levels {
    snapshot.Bids = append(snapshot.Bids, Level{
        Price: price,
        TotalQuantums: level.totalQuantums,
        OrderCount: len(level.orders),
    })
}
```

**关键文件**:
- `protocol/x/clob/types/orderbook.go`
- `protocol/x/clob/keeper/grpc_query_orderbook.go`

---

## 五、数据结构关系总览

### 5.1 存储层级关系

```
┌─────────────────────────────────────────────────────────────┐
│                         应用层                                │
│  (Keeper, Message Handlers, Query Handlers)                 │
└────────────────────┬────────────────────────────────────────┘
                     │
        ┌────────────┼────────────┐
        │            │            │
        ▼            ▼            ▼
   StateStore    MemStore   TransientStore    MemClob(内存)
   (持久化)     (内存缓存)   (区块级)         (纯内存)
        │            │            │               │
        │            │            │               │
   ┌────┴────┐  ┌────┴────┐  ┌────┴────┐    ┌────┴────┐
   │ ClobPair│  │MemInit  │  │Liquidation│   │Orderbook│
   │ Orders  │  │Delivered│  │Info      │   │Orders   │
   │ Configs │  │Orders   │  │MinMax    │   │Operations│
   └─────────┘  └─────────┘  └──────────┘   └─────────┘
```

### 5.2 订单生命周期数据流

```
1. 订单放置(CheckTx):
   User → MsgPlaceOrder
   ├─> 验证
   ├─> TransientStore: UncommittedStatefulOrderPlacement
   ├─> MemClob: 添加到订单簿
   └─> MemClob: OperationsToPropose 队列

2. 订单提议(PrepareProposal):
   MemClob.OperationsToPropose
   └─> MsgProposedOperations (区块提案)

3. 订单执行(FinalizeBlock):
   MsgProposedOperations
   ├─> StateStore: LongTermOrderPlacement
   ├─> StateStore: OrderFillState (成交量)
   ├─> MemStore: DeliveredOrders (已交付)
   └─> MemClob: 确定性匹配

4. 状态同步(PrepareCheckState):
   MemStore.DeliveredOrders
   └─> MemClob: 重放操作,更新订单簿

5. 订单过期(EndBlock):
   StateStore: StatefulOrdersExpirations
   ├─> 读取过期订单
   ├─> StateStore: 删除订单
   └─> MemClob: 删除订单
```

### 5.3 清算流程数据交互

```
清算守护进程(链下) → 检测水下账户
   ↓
MsgLiquidateSubaccount (用户交易)
   ↓
CheckTx:
   ├─> 验证账户确实水下(查询 Subaccounts 模块)
   ├─> 查询 StateStore: LiquidationsConfig
   ├─> 查询 TransientStore: SubaccountLiquidationInfo (区块限制)
   └─> MemClob: 添加清算订单

FinalizeBlock:
   ├─> 执行清算匹配
   ├─> 更新 TransientStore: SubaccountLiquidationInfo
   ├─> 更新 Subaccounts: 账户余额
   └─> 如果失败,触发去杠杆(Deleveraging)
```

### 5.4 条件订单触发数据流

```
EndBlock:
1. 读取 StateStore: UntriggeredConditionalOrders
2. 读取 Prices 模块: 预言机价格
3. 读取 TransientStore: MinTradePrice, MaxTradePrice
4. 检查触发条件:
   if (oraclePrice 或 tradePrice) 满足触发条件:
      a. 从 StateStore.Untriggered 删除
      b. 写入 StateStore.Triggered
      c. 添加到 MemClob 订单簿
      d. 参与下一轮匹配
```

---

## 六、性能与优化

### 6.1 存储性能特点

| 存储类型 | 读取性能 | 写入性能 | 持久化 | 容量 |
|---------|---------|---------|-------|------|
| StateStore | 中等(KVStore 查询) | 中等(共识延迟) | 永久 | 大 |
| MemStore | 快(内存读取) | 快(内存写入) | 区块间 | 中 |
| TransientStore | 快(内存读取) | 快(内存写入) | 仅区块内 | 小 |
| MemClob | 非常快(纯内存) | 非常快(纯内存) | 无 | 受限 |

### 6.2 数据修剪策略

**目的**: 防止状态膨胀,减少存储成本

**修剪对象**:
1. **过期订单**:
   - 扫描 `StatefulOrdersExpirations` 索引
   - 删除过期的 Long-Term 和 Conditional 订单
   - 删除对应的成交量记录(`OrderFillState`)

2. **完全成交的订单**:
   - 订单完全成交后立即删除
   - 保留成交量记录一段时间,然后修剪

3. **短期订单**:
   - 每个区块结束后自动清除(不存储到 StateStore)

**修剪时机**:
- EndBlock: 清理过期订单
- 定期任务: 清理旧的成交量记录
- PrepareCheckState: 清空 TransientStore(自动)

### 6.3 缓存与索引优化

**缓存策略**:
- MemStore 缓存热数据(CLOB Pairs, 已交付订单)
- MemClob 缓存订单簿和中间价格
- 有状态订单计数缓存,避免遍历

**索引优化**:
- 过期时间索引: 快速查找过期订单
- TWAP 子订单索引: 跟踪 TWAP 执行进度
- 价格层级索引: 快速定位最优价格

### 6.4 并发控制

**ABCI 生命周期中的并发**:
- 所有 ABCI 调用(CheckTx, FinalizeBlock 等)在同一线程执行,无需锁
- MemClob 不需要并发控制,因为只有一个 ABCI 线程访问

**查询并发**:
- gRPC 查询可能并发执行,但只读取 StateStore,不修改状态
- 使用 Cosmos SDK 的查询上下文,保证数据一致性

---

## 七、关键文件索引

### 7.1 数据结构定义 (Protobuf 生成)

| 文件 | 主要结构 | 行数 |
|-----|---------|-----|
| `types/clob_pair.pb.go` | ClobPair, PerpetualClobMetadata | ~1000 |
| `types/order.pb.go` | Order, OrderId, OrderFillState, LongTermOrderPlacement | ~2000 |
| `types/liquidations_config.pb.go` | LiquidationsConfig, PositionBlockLimits | ~500 |
| `types/equity_tier_limit_config.pb.go` | EquityTierLimitConfiguration | ~300 |
| `types/block_rate_limit_config.pb.go` | BlockRateLimitConfiguration | ~400 |
| `types/matches.pb.go` | ClobMatch, MatchOrders, MakerFill | ~800 |

### 7.2 Keeper 状态管理

| 文件 | 功能 | 关键方法 |
|-----|------|---------|
| `keeper/clob_pair.go` | CLOB 交易对管理 | Get/SetClobPair |
| `keeper/long_term_order.go` | 长期订单管理 | Get/SetLongTermOrderPlacement |
| `keeper/conditional_order.go` | 条件订单管理 | Get/SetTriggered/UntriggeredConditionalOrder |
| `keeper/order_fill_state.go` | 订单成交量管理 | Get/SetOrderFillAmount |
| `keeper/liquidations_config.go` | 清算配置管理 | Get/SetLiquidationsConfig |
| `keeper/stateful_order.go` | 有状态订单通用操作 | PlaceStatefulOrder, CancelStatefulOrder |
| `keeper/uncommitted_stateful_order.go` | 未提交订单管理 | Get/SetUncommittedStatefulOrderPlacement |

### 7.3 MemClob 实现

| 文件 | 功能 | 行数 |
|-----|------|-----|
| `memclob/memclob.go` | 核心匹配引擎 | ~100,000 |
| `types/orderbook.go` | 订单簿数据结构 | ~500 |
| `types/operations_to_propose.go` | 待提议操作队列 | ~300 |

### 7.4 ABCI 生命周期集成

| 文件 | 功能 |
|-----|------|
| `abci.go` | PreBlocker, BeginBlocker, EndBlocker, Precommit |
| `keeper/process_operations.go` | 处理 MsgProposedOperations,执行匹配 |
| `keeper/liquidations.go` | 清算逻辑 |

---

## 八、常见问题 (FAQ)

### 8.1 为什么 Long-Term 订单需要同时存储在 StateStore 和 MemClob?

**原因**:
- **StateStore**: 提供持久化存储,确保节点重启后订单不丢失
- **MemClob**: 提供高性能内存访问,支持快速订单匹配

### 8.2 TransientStore 与 MemStore 的区别是什么?

**区别**:
- **TransientStore**: 仅在单个区块内有效,区块结束后自动清空
- **MemStore**: 在区块间持久化,但仅存在于内存,节点重启后需从 StateStore 重新加载

### 8.3 为什么订单成交量要单独存储?

**原因**:
- 订单可能部分成交,需要跟踪已成交数量
- 分离存储避免频繁更新大的订单结构,提高性能
- 支持订单成交后延迟修剪,用于审计和查询

### 8.4 如何确保订单匹配的确定性?

**确定性保证机制**:
1. 使用确定性排序算法(价格-时间优先)
2. 避免随机数、系统时间戳等不确定因素
3. 使用 `TransactionOrdering` (区块高度 + 交易索引) 作为时间戳
4. 所有验证器执行相同的 `MsgProposedOperations`

### 8.5 MemClob 重启后如何恢复订单簿状态?

**恢复流程**:
```
应用启动(PreBlocker):
1. 检查 MemStoreInitialized 标志
2. 从 StateStore 读取所有 ClobPair
3. 为每个 ClobPair 创建 MemClob 订单簿
4. 从 StateStore 读取所有 Long-Term 和 Triggered Conditional 订单
5. 将订单加载到 MemClob 订单簿
6. 设置 MemStoreInitialized = true
```

### 8.6 为什么清算信息要存储在 TransientStore?

**原因**:
- 清算限制是区块级别的,不需要跨区块持久化
- 每个区块重新开始计算清算量,避免累积限制
- 使用 TransientStore 自动清空,无需手动清理

### 8.7 订单过期索引如何提高性能?

**性能优化**:
- **无索引**: 需要遍历所有订单,检查过期时间,复杂度 O(N)
- **有索引**: 按过期时间分组,只需查询当前时间之前的索引,复杂度 O(K),K << N

---

## 九、数据访问模式参考

### 9.1 常见读取模式

```go
// 读取 CLOB 交易对
clobPair, found := keeper.GetClobPair(ctx, clobPairId)

// 读取订单成交量
fillAmount := keeper.GetOrderFillAmount(ctx, orderId)

// 读取清算配置
liquidationsConfig := keeper.GetLiquidationsConfig(ctx)

// 读取订单簿快照
snapshot := keeper.MemClob.GetOrderbookSnapshot(ctx, clobPairId)
```

### 9.2 常见写入模式

```go
// 创建新交易对
keeper.SetClobPair(ctx, clobPair)
keeper.IncrementNextClobPairID(ctx)

// 放置 Long-Term 订单
keeper.SetLongTermOrderPlacement(ctx, orderPlacement)
keeper.AddOrderToExpiration(ctx, orderId, goodTilBlockTime)

// 更新订单成交量
fillState := keeper.GetOrderFillState(ctx, orderId)
fillState.FillAmount += matchQuantums
keeper.SetOrderFillState(ctx, orderId, fillState)
```

### 9.3 常见删除模式

```go
// 删除过期订单
expiredOrderIds := keeper.GetExpiredOrders(ctx, currentTime)
for _, orderId := range expiredOrderIds {
    keeper.RemoveLongTermOrderPlacement(ctx, orderId)
    keeper.DeleteOrderFillState(ctx, orderId)
}
keeper.RemoveExpiredOrders(ctx, currentTime)
```

---

## 十、监控与诊断

### 10.1 关键指标

**状态存储指标**:
- StateStore 大小(字节)
- Long-Term 订单数量
- Conditional 订单数量
- 订单成交量记录数量

**MemClob 指标**:
- 订单簿深度(每个价格层级的订单数)
- 内存占用(MB)
- 订单匹配延迟(毫秒)
- 每个区块匹配的订单数量

**性能指标**:
- StateStore 读写延迟
- MemClob 订单放置延迟
- 订单簿更新延迟
- PrepareCheckState 执行时间

### 10.2 诊断工具

**查询接口**:
```bash
# 查询 CLOB 交易对
hermesprotocold query clob clob-pair <clob-pair-id>

# 查询有状态订单
hermesprotocold query clob stateful-order <order-id>

# 查询清算配置
hermesprotocold query clob liquidations-configuration

# 查询权益层级限制
hermesprotocold query clob equity-tier-limit-configuration
```

**导出状态**:
```bash
# 导出创世状态(包含所有 StateStore 数据)
hermesprotocold export > genesis.json

# 分析订单数量
cat genesis.json | jq '.app_state.clob.long_term_order_placements | length'
```

### 10.3 常见问题诊断

**问题 1: 订单未成交**
```
诊断步骤:
1. 检查订单是否在 MemClob 订单簿中:查询 orderbook snapshot
2. 检查订单成交量:查询 OrderFillState
3. 检查订单是否过期:查询 GoodTilBlockTime
4. 检查价格是否匹配:比较订单价格与市场价格
```

**问题 2: 订单簿状态不一致**
```
诊断步骤:
1. 检查 MemStoreInitialized 标志
2. 比较 StateStore 订单数量与 MemClob 订单数量
3. 检查 PrepareCheckState 是否正确执行
4. 重启节点,重新加载订单簿
```

**问题 3: 清算失败**
```
诊断步骤:
1. 检查 TransientStore 清算信息(SubaccountLiquidationInfo)
2. 检查清算配置(LiquidationsConfig)
3. 检查账户是否确实水下(查询 Subaccounts 模块)
4. 检查清算订单是否进入 MemClob
```

---

## 十、已废弃的存储键

以下存储键已在历史版本中废弃,但在代码中保留以支持向后兼容性或数据迁移。新版本不再使用这些键,但可能在链升级过程中需要读取旧数据。

### 10.1 按区块高度的可修剪订单索引

**存储键前缀**: `StatefulOrdersHeightPrunableKeyPrefix` = `"ExpHt:"`

**废弃原因**: 替换为更高效的按时间戳的过期订单索引 (`"Exp/<timestamp>:"`)

**原用途**:
- 按区块高度索引可修剪的订单
- 用于清理过期订单

**新实现**:
- 使用 `StatefulOrdersExpirationsKeyPrefix` (`"Exp/<timestamp>:"`) 代替
- 按时间戳索引更直观,与 `GoodTilBlockTime` 语义一致
- 支持更精确的过期时间控制

**迁移说明**:
- 链升级时,可能需要从 `ExpHt:` 读取旧订单并迁移到新索引
- 迁移完成后,旧键可以被删除

**关键文件**:
- `protocol/x/clob/keeper/order_expiration.go` (包含迁移逻辑)

---

### 10.2 按时间的过期订单索引 (旧格式)

**存储键前缀**: `StatefulOrdersTimePrunableKeyPrefix` = `"ExpTm:"`

**废弃原因**: 替换为新格式 `"Exp/<timestamp>:"`,改进了键结构和查询效率

**原用途**:
- 按过期时间索引订单,与现在的 `"Exp/<timestamp>:"` 类似
- 可能使用不同的键结构或时间编码方式

**新实现**:
- 使用 `StatefulOrdersExpirationsKeyPrefix` (`"Exp/<timestamp>:"`)
- 键结构优化,支持更高效的范围查询
- 时间编码标准化,避免跨平台兼容性问题

**迁移说明**:
- 链升级时从 `ExpTm:` 读取旧数据,重新写入新格式
- 确保过期时间正确转换

**关键文件**:
- `protocol/x/clob/keeper/order_expiration.go`

---

### 10.3 已废弃键列表总结

| 已废弃键前缀 | 新替代键前缀 | 废弃时间 | 迁移状态 |
|------------|------------|---------|---------|
| `ExpHt:` | `Exp/<timestamp>:` | v4.x | ✅ 已完成 |
| `ExpTm:` | `Exp/<timestamp>:` | v4.x | ✅ 已完成 |

**使用建议**:
- 新功能开发不应使用已废弃的键前缀
- 读取历史数据时,可能需要同时检查新旧键
- 链升级时优先迁移这些数据,避免兼容性问题

**查询旧数据**:
```go
// 兼容性读取:先尝试新键,再尝试旧键
func GetOrderExpiration(ctx sdk.Context, orderId OrderId) (time.Time, bool) {
    // 优先读取新格式
    if expiration, found := keeper.GetOrderExpirationNew(ctx, orderId); found {
        return expiration, true
    }

    // 回退到旧格式 (废弃键)
    if expiration, found := keeper.GetOrderExpirationLegacy(ctx, orderId); found {
        // 可选:迁移到新格式
        keeper.SetOrderExpirationNew(ctx, orderId, expiration)
        keeper.DeleteOrderExpirationLegacy(ctx, orderId)
        return expiration, true
    }

    return time.Time{}, false
}
```

**清理策略**:
- 建议在下一次主要版本升级时移除对废弃键的支持
- 确保所有节点都完成数据迁移后再删除兼容性代码
- 在测试网上充分测试迁移流程

---

## 十一、总结

CLOB 模块的数据结构设计体现了以下关键原则:

1. **分层存储**: StateStore(持久化)、MemStore(缓存)、TransientStore(临时)、MemClob(纯内存)各司其职
2. **性能优化**: 多级缓存、索引优化、内存订单簿提供高性能匹配
3. **确定性保证**: 所有匹配操作确定性执行,保证共识
4. **状态修剪**: 定期清理过期数据,防止状态膨胀
5. **模块化设计**: 清晰的数据边界和访问模式,易于维护和扩展

**关键数据流总结**:
- 订单放置: User → CheckTx → TransientStore/MemClob → FinalizeBlock → StateStore
- 订单匹配: MemClob → MsgProposedOperations → FinalizeBlock → StateStore(成交量)
- 订单过期: StateStore(索引) → EndBlock → 删除订单
- 状态恢复: StateStore → 应用启动 → MemClob

**设计亮点**:
- **ABCI 生命周期集成**: 充分利用 CometBFT 的 ABCI 接口,分离乐观处理(CheckTx)和确定性执行(FinalizeBlock)
- **MemClob 架构**: 10 万+ 行的高性能匹配引擎,确保订单匹配的确定性和高效性
- **多级限制**: 权益层级限制、区块速率限制、清算区块限制,全方位防止垃圾攻击
- **灵活订单类型**: 支持 Short-Term、Long-Term、Conditional、TWAP 等多种订单类型,满足不同交易需求

**未来优化方向**:
- 进一步优化 MemClob 内存占用
- 改进订单过期清理策略,减少状态扫描
- 增强订单簿快照缓存,提高查询性能
- 考虑引入订单簿分片,支持更高并发

---

**文档版本**: v1.0
**最后更新**: 2025-12-31
**作者**: Claude Sonnet 4.5 + Hermes DEX Team
**参考代码版本**: `protocol/x/clob` (commit: 414ee78)
