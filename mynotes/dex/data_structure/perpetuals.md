# Perpetuals 模块数据结构文档

## 模块概述

Perpetuals (永续合约) 模块负责管理永续合约的定义、资金费率计算、未平仓合约量(Open Interest)跟踪和流动性层级(Liquidity Tier)管理。本文档详细描述了 Perpetuals 模块中的所有数据结构,按存储类型分类组织。

**模块路径**: `protocol/x/perpetuals`
**Store Key**: `perpetuals`
**核心功能**: 永续合约配置、资金费率机制、OIMF 动态保证金调整

---

## 一、StateStore (链上持久化存储)

StateStore 存储的数据会在区块间持久化,是链上状态的核心。这些数据在节点重启后依然存在,通过共识保证所有节点数据一致。

### 1.1 永续合约定义

**存储键**: `PerpetualKeyPrefix` + `PerpetualId` = `"Perp:" + <uint32>`

**数据结构**: `Perpetual`

```go
type Perpetual struct {
    Params PerpetualParams       // 永续合约参数
    FundingIndex SerializableInt  // 资金费率指数(累计值)
    OpenInterest SerializableInt  // 未平仓合约量(base quantums)
}

type PerpetualParams struct {
    Id uint32                     // 唯一标识符,顺序递增
    Ticker string                 // 交易对名称,如 "BTC-USD"
    MarketId uint32               // 关联的价格市场 ID(Prices 模块)
    AtomicResolution int32        // 原子精度指数,如 -8 表示 1e8 = 1 BTC
    DefaultFundingPpm int32       // 默认资金费率(8小时),PPM 表示
    LiquidityTier uint32          // 关联的流动性层级 ID
    MarketType PerpetualMarketType // 市场类型(CROSS/ISOLATED)
}

// 市场类型枚举
type PerpetualMarketType int32
const (
    PERPETUAL_MARKET_TYPE_UNSPECIFIED = 0  // 未指定
    PERPETUAL_MARKET_TYPE_CROSS = 1        // 全仓保证金模式
    PERPETUAL_MARKET_TYPE_ISOLATED = 2     // 逐仓保证金模式
)
```

**业务含义**:

**Perpetual 结构**:
- 定义一个永续合约的所有配置和状态
- `FundingIndex`: 累计资金费率指数,用于计算持仓的资金费用
  - 每次资金费率结算时更新
  - 公式: `新 FundingIndex = 旧 FundingIndex + 资金费率`
  - 用户资金费用 = (当前 FundingIndex - 开仓时 FundingIndex) × 持仓数量
- `OpenInterest`: 市场总未平仓合约量,用于 OIMF 计算和风险监控
  - 买方开多仓时增加,平多仓时减少
  - 卖方开空仓时增加,平空仓时减少

**PerpetualParams 参数解释**:
- `Ticker`: 人类可读的交易对名称,如 "BTC-USD", "ETH-USD"
- `MarketId`: 关联到 Prices 模块的市场 ID,用于获取预言机价格
  - 预言机价格用于计算抵押品价值、保证金要求、资金费率
- `AtomicResolution`: 数量精度转换
  - 例如 `-8`: 链上 `size=100000000` 表示 1 BTC
  - 例如 `-6`: 链上 `size=1000000` 表示 1 ETH
- `DefaultFundingPpm`: 默认 8 小时资金费率
  - 例如 `10000` = 1%(PPM = parts-per-million)
  - 当市场溢价为 0 时使用此值
- `LiquidityTier`: 流动性层级决定保证金要求
  - 不同层级有不同的初始保证金率(IMR)和维持保证金率(MMR)
  - 高风险资产使用更高的保证金要求
- `MarketType`:
  - **CROSS(全仓)**: 所有仓位共享账户全部资产作为保证金
  - **ISOLATED(逐仓)**: 每个仓位独立使用固定保证金,互不影响

**使用场景**:
- **创建新合约**: 通过治理提案或无权限上市创建新的永续合约
- **订单验证**: CLOB 模块查询合约参数,验证订单价格和数量精度
- **保证金计算**: Subaccounts 模块根据流动性层级和 OIMF 计算保证金要求
- **资金费率结算**: 每 8 小时更新 `FundingIndex`,收取/支付资金费用
- **风险监控**: 监控 `OpenInterest`,评估市场系统性风险

**数据访问**:
```go
// 读取
keeper.GetPerpetual(ctx, perpetualId) -> Perpetual, found

// 写入/更新
keeper.SetPerpetual(ctx, perpetual)

// 更新资金费率指数
perpetual.FundingIndex = perpetual.FundingIndex.Add(fundingRate)
keeper.SetPerpetual(ctx, perpetual)

// 更新未平仓合约量
perpetual.OpenInterest = perpetual.OpenInterest.Add(delta)
keeper.SetPerpetual(ctx, perpetual)
```

**关键文件**:
- `protocol/x/perpetuals/types/perpetual.pb.go:59-107` (Perpetual)
- `protocol/x/perpetuals/types/perpetual.pb.go:111-213` (PerpetualParams)
- `protocol/x/perpetuals/keeper/perpetual.go`

---

### 1.2 流动性层级 (Liquidity Tier)

**存储键**: `LiquidityTierKeyPrefix` + `LiquidityTierId` = `"LiqTier:" + <uint32>`

**数据结构**: `LiquidityTier`

```go
type LiquidityTier struct {
    Id uint32                    // 唯一标识符
    Name string                  // 层级名称,如 "Gold", "Silver"
    InitialMarginPpm uint32      // 初始保证金率(PPM)
    MaintenanceFractionPpm uint32 // 维持保证金率占初始保证金比例(PPM)
    BasePositionNotional uint64  // 已废弃(v3.x),保留字段
    ImpactNotional uint64        // 影响名义价值(quote quantums)
    OpenInterestLowerCap uint64  // 未平仓合约量下限(quote quantums)
    OpenInterestUpperCap uint64  // 未平仓合约量上限(quote quantums)
}
```

**业务含义**:

**保证金相关参数**:
- `InitialMarginPpm` (初始保证金率, IMR):
  - 开仓时需要的保证金占仓位名义价值的比例
  - 例如 `50000` = 5%,即开仓 1000 USD 仓位需要 50 USD 保证金
  - 杠杆倍数 = 1 / IMR,如 5% IMR → 20x 杠杆
- `MaintenanceFractionPpm` (维持保证金占比):
  - 维持保证金率(MMR)= IMR × MaintenanceFraction
  - 例如 IMR=5%, MaintenanceFraction=50% → MMR=2.5%
  - 当账户保证金低于 MMR 时触发清算

**OIMF (Open Interest Margin Fraction) 动态保证金调整**:
- `OpenInterestLowerCap` (下限):
  - 市场未平仓合约量 <= 下限时,保证金率不调整
  - 例如 `10000000` = 10000 USD
- `OpenInterestUpperCap` (上限):
  - 市场未平仓合约量接近上限时,保证金率线性增加到 100%
  - 例如 `100000000` = 100000 USD
  - 如果为 0,则禁用 OIMF 机制

**OIMF 计算公式**:
```
if OI <= LowerCap:
    AdjustedIMR = InitialMarginPpm
else if OI >= UpperCap:
    AdjustedIMR = 1000000 (100%)
else:
    AdjustedIMR = InitialMarginPpm + (OI - LowerCap) / (UpperCap - LowerCap) * (1000000 - InitialMarginPpm)
```

**ImpactNotional 影响名义价值**:
- 用于计算影响买卖价(Impact Bid/Ask Price)
- 推荐值 = 500 USDC / InitialMarginFraction
  - 例如 IMR=5%: ImpactNotional = 500 / 0.05 = 10000 USDC
- **影响买价**: 卖出 ImpactNotional 数量的平均成交价
- **影响卖价**: 买入 ImpactNotional 数量的平均成交价
- 用于资金费率计算和市场深度评估

**使用场景**:
- **创建永续合约**: 分配合适的流动性层级(高风险资产用高保证金)
- **保证金计算**: Subaccounts 模块根据层级计算开仓和维持保证金
- **OIMF 调整**: 根据市场未平仓合约量动态调整保证金要求
  - 目的:限制单个市场的系统性风险
  - 当市场持仓过大时,提高保证金要求,抑制进一步开仓
- **清算判断**: 检查账户保证金是否低于维持保证金率
- **资金费率计算**: 使用 ImpactNotional 计算影响价格,参与溢价计算

**流动性层级示例**:
```json
{
  "id": 0,
  "name": "Large-Cap",
  "initial_margin_ppm": 50000,         // 5% IMR → 20x 杠杆
  "maintenance_fraction_ppm": 600000,  // 60% → MMR = 3%
  "impact_notional": 10000000000,      // 10000 USDC
  "open_interest_lower_cap": 25000000000,  // 25000 USDC
  "open_interest_upper_cap": 50000000000   // 50000 USDC
}
```

**数据访问**:
```go
// 读取
keeper.GetLiquidityTier(ctx, liquidityTierId) -> LiquidityTier, found

// 写入/更新
keeper.SetLiquidityTier(ctx, liquidityTier)

// 获取所有层级
keeper.GetAllLiquidityTiers(ctx) -> []LiquidityTier
```

**关键文件**:
- `protocol/x/perpetuals/types/perpetual.pb.go:340-431` (LiquidityTier)
- `protocol/x/perpetuals/keeper/liquidity_tier.go`

---

### 1.3 溢价投票 (Premium Votes) - Funding Sample Epoch

**存储键**: `PremiumVotesKey` = `"PremVotes"`

**数据结构**: `PremiumStore`

```go
type PremiumStore struct {
    AllMarketPremiums []MarketPremiums  // 所有市场的溢价投票
    NumPremiums uint32                  // 投票轮数(样本总数)
}

type MarketPremiums struct {
    PerpetualId uint32   // 永续合约 ID
    Premiums []int32     // 溢价值列表(仅存储非零值)
}
```

**业务含义**:

**资金费率机制概述**:
Hermes DEX 使用两层周期机制计算资金费率:
1. **Funding Sample Epoch (1 分钟周期)**:
   - 验证器每分钟提交溢价投票(Premium Vote)
   - 聚合所有验证器的投票,计算溢价样本(Premium Sample)
2. **Funding Tick Epoch (1 小时周期)**:
   - 收集 60 个溢价样本(60 分钟)
   - 聚合计算 8 小时资金费率
   - 每 8 小时结算一次

**PremiumVotes 溢价投票**:
- 存储当前 1 分钟周期内验证器提交的溢价投票
- 每个验证器提交一次投票,记录订单簿溢价
- 周期结束时聚合所有投票,计算中位数作为溢价样本

**MarketPremiums 市场溢价**:
- `PerpetualId`: 标识哪个永续合约
- `Premiums`: 溢价值列表
  - 仅存储非零溢价,节省存储空间
  - 零溢价非常常见(市场稳定时),不存储
  - 单位: PPM (parts-per-million)

**溢价计算公式**:
```
Premium = (订单簿中间价 - 预言机价格) / 预言机价格
```

**NumPremiums 投票轮数**:
- 记录当前周期内已经添加了多少轮投票
- 用于计算平均值和中位数
- 周期结束后重置为 0

**使用场景**:
- **验证器提交**: 每分钟验证器提交 `MsgAddPremiumVotes`
  - 计算订单簿中间价和预言机价格的差异
  - 提交溢价投票到链上
- **周期聚合**: Funding Sample Epoch 结束时
  - 读取所有验证器的投票
  - 计算中位数作为溢价样本
  - 写入 `PremiumSamples` (下一个数据结构)
  - 清空 `PremiumVotes`,开始新周期

**数据访问**:
```go
// 读取
keeper.GetPremiumVotes(ctx) -> PremiumStore

// 添加投票
premiumVotes := keeper.GetPremiumVotes(ctx)
premiumVotes.AllMarketPremiums = append(premiumVotes.AllMarketPremiums, newVote)
premiumVotes.NumPremiums++
keeper.SetPremiumVotes(ctx, premiumVotes)

// 清空(周期结束)
keeper.SetPremiumVotes(ctx, types.PremiumStore{})
```

**关键文件**:
- `protocol/x/perpetuals/types/perpetual.pb.go:217-271` (MarketPremiums)
- `protocol/x/perpetuals/types/perpetual.pb.go:279-337` (PremiumStore)
- `protocol/x/perpetuals/keeper/premium_store.go`

---

### 1.4 溢价样本 (Premium Samples) - Funding Tick Epoch

**存储键**: `PremiumSamplesKey` = `"PremSamples"`

**数据结构**: `PremiumStore` (与 PremiumVotes 相同结构)

```go
type PremiumStore struct {
    AllMarketPremiums []MarketPremiums
    NumPremiums uint32  // 样本总数(通常为 60,即 60 分钟)
}
```

**业务含义**:

**PremiumSamples 溢价样本**:
- 存储当前 1 小时周期内的溢价样本
- 每 1 分钟添加一个样本(从 PremiumVotes 聚合得出)
- 周期内最多 60 个样本

**资金费率计算流程**:
```
1. 每 1 分钟 (Funding Sample Epoch 结束):
   a. 聚合 PremiumVotes,计算中位数
   b. 将中位数添加到 PremiumSamples
   c. NumPremiums++

2. 每 1 小时 (Funding Tick Epoch 结束):
   a. 读取 PremiumSamples 的所有样本
   b. 计算平均值: AvgPremium = sum(Premiums) / NumPremiums
   c. 计算 8 小时资金费率:
      FundingRate = Clamp(
          8 * AvgPremium,
          DefaultFundingPpm - MaxPremiumPpm,
          DefaultFundingPpm + MaxPremiumPpm
      )
   d. 更新 Perpetual.FundingIndex
   e. 清空 PremiumSamples,开始新周期
```

**资金费率计算公式**:
```
8小时资金费率 = Clamp(
    8 × 平均溢价,
    默认资金费率 - 最大溢价偏差,
    默认资金费率 + 最大溢价偏差
)

其中:
- 平均溢价 = sum(60个溢价样本) / 60
- 8倍系数:将 1 小时溢价转换为 8 小时资金费率
- Clamp:限制资金费率在合理范围内,防止极端值
```

**使用场景**:
- **样本累积**: 每分钟从 PremiumVotes 聚合一个样本,添加到 PremiumSamples
- **资金费率结算**: 每小时计算 8 小时资金费率
  - 更新所有永续合约的 FundingIndex
  - 用户持仓根据 FundingIndex 差值计算资金费用
- **市场分析**: 追踪溢价趋势,评估市场情绪

**数据访问**:
```go
// 读取
keeper.GetPremiumSamples(ctx) -> PremiumStore

// 添加样本(从 PremiumVotes 聚合)
samples := keeper.GetPremiumSamples(ctx)
medianPremium := calculateMedian(premiumVotes)
samples.AllMarketPremiums = append(samples.AllMarketPremiums, medianPremium)
samples.NumPremiums++
keeper.SetPremiumSamples(ctx, samples)

// 计算资金费率(周期结束)
samples := keeper.GetPremiumSamples(ctx)
avgPremium := calculateAverage(samples)
fundingRate := clamp(8 * avgPremium, min, max)
keeper.SetPremiumSamples(ctx, types.PremiumStore{})
```

**关键文件**:
- `protocol/x/perpetuals/keeper/premium_store.go`
- `protocol/x/perpetuals/abci.go` (EndBlocker 处理资金费率)

---

### 1.5 模块参数

**存储键**: `ParamsKey` = `"Params"`

**数据结构**: `Params`

```go
type Params struct {
    FundingRateClampFactorPpm uint32  // 资金费率钳位系数(PPM)
    PremiumVoteClampFactorPpm uint32  // 溢价投票钳位系数(PPM)
    MinNumVotesPerSample uint32       // 每个样本最小投票数
}
```

**业务含义**:

**FundingRateClampFactorPpm 资金费率钳位系数**:
- 限制资金费率的最大偏差范围
- 公式: `MaxDeviation = DefaultFundingPpm × ClampFactor`
- 例如:
  - DefaultFundingPpm = 10000 (1%)
  - ClampFactor = 6000000 (6x)
  - 资金费率范围: [-5%, 7%] (10倍偏差)

**PremiumVoteClampFactorPpm 溢价投票钳位系数**:
- 限制单个验证器提交的溢价投票范围
- 防止恶意验证器提交极端溢价值
- 超出范围的投票会被钳位到边界值

**MinNumVotesPerSample 最小投票数**:
- 聚合溢价样本时需要的最少验证器投票数
- 如果投票数不足,跳过此轮聚合
- 确保溢价样本的可靠性

**使用场景**:
- **资金费率计算**: 使用钳位系数限制资金费率范围
- **投票验证**: 检查溢价投票是否在合理范围内
- **样本聚合**: 检查投票数是否足够

**数据访问**:
```go
// 读取
keeper.GetParams(ctx) -> Params

// 写入(通过治理)
keeper.SetParams(ctx, params)
```

**关键文件**:
- `protocol/x/perpetuals/types/params.pb.go`
- `protocol/x/perpetuals/keeper/params.go`

---

### 1.6 下一个永续合约 ID

**存储键**: `NextPerpetualIDKey` = `"NextPerpetualID"`

**数据结构**: `uint32`

**业务含义**:
- 存储下一个可用的永续合约 ID
- 每次创建新合约时自动递增
- 确保合约 ID 的唯一性

**使用场景**:
- **创建新合约**: 读取当前 ID,分配给新合约,然后递增
- **无权限上市**: 自动生成新的合约 ID

**数据访问**:
```go
// 读取
keeper.GetNextPerpetualID(ctx) -> uint32

// 递增
keeper.IncrementNextPerpetualID(ctx)
```

**关键文件**:
- `protocol/x/perpetuals/keeper/perpetual.go`

---

## 二、TransientStore (瞬态存储,每个区块结束后清空)

TransientStore 的数据仅在单个区块内有效,区块结束后自动清空。用于存储临时状态和区块级作用域数据。

### 2.1 已更新的未平仓合约量

**存储键前缀**: `UpdatedOIKeyPrefix` + `PerpetualId` = `"UpdatedOI" + <uint32>`

**数据结构**: `OpenInterestDelta`

```go
type OpenInterestDelta struct {
    PerpetualId uint32        // 永续合约 ID
    BaseQuantumsDelta int64   // 未平仓合约量变化(可正可负)
}
```

**业务含义**:
- 记录当前区块内每个永续合约的未平仓合约量变化
- 用于批量更新,避免频繁读写 StateStore
- 区块结束时将所有变化聚合后一次性更新到 StateStore

**未平仓合约量 (Open Interest, OI) 更新场景**:
1. **开多仓**: OI 增加(买方开仓)
2. **平多仓**: OI 减少(买方平仓)
3. **开空仓**: OI 增加(卖方开仓)
4. **平空仓**: OI 减少(卖方平仓)
5. **对冲成交**: 一方开仓,另一方平仓,净变化取决于数量

**使用场景**:

**区块内累积**:
```
订单匹配(多次):
1. Match 1: 用户 A 开多 10 BTC
   → UpdatedOI[BTC-USD] += 10

2. Match 2: 用户 B 平空 5 BTC
   → UpdatedOI[BTC-USD] -= 5

3. Match 3: 用户 C 开空 8 BTC
   → UpdatedOI[BTC-USD] += 8

区块内净变化: UpdatedOI[BTC-USD] = +13 BTC
```

**区块结束聚合**:
```
EndBlocker:
1. 读取所有 UpdatedOI 记录
2. 更新 Perpetual.OpenInterest:
   perpetual.OpenInterest += updatedOI.BaseQuantumsDelta
3. 清空 TransientStore (自动)
```

**数据访问**:
```go
// 读取(区块内)
delta := keeper.GetUpdatedOpenInterest(ctx, perpetualId)

// 写入/累加(订单匹配时)
keeper.AddToOpenInterestDelta(ctx, perpetualId, quantumsDelta)

// 应用到 StateStore (EndBlocker)
deltas := keeper.GetAllUpdatedOpenInterest(ctx)
for _, delta := range deltas {
    perpetual := keeper.GetPerpetual(ctx, delta.PerpetualId)
    perpetual.OpenInterest = perpetual.OpenInterest.Add(delta.BaseQuantumsDelta)
    keeper.SetPerpetual(ctx, perpetual)
}
```

**性能优化**:
- 避免每次匹配都更新 StateStore
- 区块内批量聚合,减少状态写入次数
- TransientStore 自动清空,无需手动清理

**关键文件**:
- `protocol/x/perpetuals/types/types.go:125-130` (OpenInterestDelta)
- `protocol/x/perpetuals/keeper/open_interest.go`
- `protocol/x/perpetuals/abci.go:EndBlocker()`

---

## 三、MemStore (内存缓存,从 StateStore 同步)

Perpetuals 模块没有专门的 MemStore 数据,但会缓存一些常用数据在内存中以提高性能。

### 3.1 流动性层级缓存

**存储位置**: Keeper 内存缓存(可选)

**数据结构**: `map[uint32]LiquidityTier`

**业务含义**:
- 缓存所有流动性层级,避免频繁读取 StateStore
- 流动性层级变化很少,适合缓存
- 应用启动时加载,更新时同步

**使用场景**:
- **保证金计算**: 频繁访问流动性层级参数
- **订单验证**: 检查保证金要求

**关键文件**:
- `protocol/x/perpetuals/keeper/keeper.go` (可能包含缓存逻辑)

---

## 四、MemClob (纯内存,不属于 Perpetuals 模块)

Perpetuals 模块本身不包含纯内存数据结构,所有订单簿数据由 CLOB 模块的 MemClob 管理。Perpetuals 模块仅提供合约定义和资金费率计算。

---

## 五、数据结构关系总览

### 5.1 存储层级关系

```
┌─────────────────────────────────────────────────────────────┐
│                         应用层                                │
│  (Keeper, Message Handlers, ABCI Hooks)                     │
└────────────────────┬────────────────────────────────────────┘
                     │
        ┌────────────┼────────────┐
        │            │            │
        ▼            ▼            ▼
   StateStore   TransientStore  MemStore
   (持久化)     (区块级)        (缓存)
        │            │            │
        │            │            │
   ┌────┴────┐  ┌────┴────┐  ┌────┴────┐
   │Perpetual│  │UpdatedOI│  │缓存数据  │
   │LiqTier  │  │         │  │         │
   │Premium  │  │         │  │         │
   └─────────┘  └─────────┘  └─────────┘
```

### 5.2 资金费率计算数据流

```
1. 验证器提交溢价投票 (每分钟):
   Validator → MsgAddPremiumVotes
   ├─> 计算 Premium = (订单簿中间价 - 预言机价格) / 预言机价格
   └─> StateStore: PremiumVotes

2. Funding Sample Epoch 结束 (每分钟):
   PremiumVotes (所有验证器投票)
   ├─> 聚合:计算中位数
   ├─> 清空 PremiumVotes
   └─> 添加到 StateStore: PremiumSamples

3. Funding Tick Epoch 结束 (每小时):
   PremiumSamples (60 个样本)
   ├─> 聚合:计算平均值
   ├─> 计算 8 小时资金费率
   ├─> 更新 StateStore: Perpetual.FundingIndex
   └─> 清空 PremiumSamples

4. 用户资金费用计算 (开仓/平仓时):
   持仓 FundingIndex - 当前 FundingIndex
   └─> 计算资金费用,更新账户余额
```

### 5.3 未平仓合约量更新数据流

```
订单匹配 (每次成交):
   MatchOrders
   ├─> 判断开仓/平仓/对冲
   ├─> 计算 OI 变化量 (delta)
   └─> TransientStore: UpdatedOI (累加)

区块结束 (EndBlocker):
   TransientStore: UpdatedOI
   ├─> 读取所有 delta
   ├─> StateStore: Perpetual.OpenInterest (批量更新)
   └─> TransientStore 自动清空

OIMF 动态调整 (保证金计算时):
   StateStore: Perpetual.OpenInterest
   ├─> 读取当前 OI
   ├─> 结合 LiquidityTier 的 OI Cap
   └─> 计算调整后的保证金率
```

### 5.4 模块间数据依赖

```
Perpetuals 模块数据被其他模块使用:

CLOB 模块:
- 读取 Perpetual.Params.AtomicResolution (订单数量精度)
- 读取 Perpetual.Params.MarketId (预言机价格 ID)

Subaccounts 模块:
- 读取 LiquidityTier (计算保证金要求)
- 读取 Perpetual.FundingIndex (计算资金费用)
- 读取 Perpetual.OpenInterest (OIMF 调整)

Prices 模块:
- 提供预言机价格 (MarketId 映射)
- 用于资金费率溢价计算
```

---

## 六、性能与优化

### 6.1 存储性能特点

| 存储类型 | 读取频率 | 写入频率 | 持久化 | 优化策略 |
|---------|---------|---------|-------|----------|
| Perpetual | 高(每次订单) | 低(仅资金费率/OI更新) | 永久 | 内存缓存 |
| LiquidityTier | 极高(每次保证金计算) | 极低(仅治理更新) | 永久 | 强缓存 |
| PremiumVotes | 低(仅验证器) | 低(每分钟) | 永久 | 无需优化 |
| PremiumSamples | 低(仅周期) | 低(每分钟) | 永久 | 无需优化 |
| UpdatedOI | 中(每次匹配) | 高(每次匹配) | 区块级 | TransientStore |

### 6.2 数据压缩策略

**溢价数据压缩**:
- `MarketPremiums.Premiums` 仅存储非零溢价
- 大部分时间市场溢价接近 0,大幅节省存储空间
- 零溢价通过 `NumPremiums` 推断

**示例**:
```
60 个溢价样本,58 个为 0,2 个非零:
- 未压缩: [0, 0, 0, ..., 100, 0, ..., -50]  (60 个 int32)
- 压缩后: [100, -50]                          (2 个 int32)
- NumPremiums = 60
- 计算平均值时:sum([100, -50]) / 60 = 0.83
```

### 6.3 批量更新优化

**未平仓合约量批量更新**:
- 区块内多次匹配累积到 TransientStore
- EndBlocker 一次性批量更新 StateStore
- 减少 StateStore 写入次数,提高性能

**效果对比**:
```
优化前(每次匹配直接更新):
- 100 次匹配 → 100 次 StateStore 写入

优化后(区块结束批量更新):
- 100 次匹配 → 100 次 TransientStore 累加
- 1 次 EndBlocker → 1 次 StateStore 写入
```

---

## 七、关键文件索引

### 7.1 数据结构定义 (Protobuf 生成)

| 文件 | 主要结构 | 行数 |
|-----|---------|-----|
| `types/perpetual.pb.go` | Perpetual, PerpetualParams, LiquidityTier | ~800 |
| `types/perpetual.pb.go` | MarketPremiums, PremiumStore | ~200 |
| `types/params.pb.go` | Params | ~200 |
| `types/types.go` | OpenInterestDelta | ~150 |

### 7.2 Keeper 状态管理

| 文件 | 功能 | 关键方法 |
|-----|------|---------|
| `keeper/perpetual.go` | 永续合约管理 | Get/SetPerpetual |
| `keeper/liquidity_tier.go` | 流动性层级管理 | Get/SetLiquidityTier |
| `keeper/premium_store.go` | 溢价投票和样本管理 | Get/SetPremiumVotes/Samples |
| `keeper/open_interest.go` | 未平仓合约量管理 | AddToOpenInterestDelta |
| `keeper/params.go` | 模块参数管理 | Get/SetParams |

### 7.3 ABCI 生命周期集成

| 文件 | 功能 |
|-----|------|
| `abci.go` | EndBlocker - 处理资金费率结算和 OI 更新 |

---

## 八、常见问题 (FAQ)

### 8.1 为什么资金费率使用两层周期机制?

**原因**:
1. **精度与可靠性**:
   - 1 分钟采样:捕捉短期市场波动
   - 1 小时聚合:平滑噪音,避免极端值
2. **验证器共识**:
   - 每分钟所有验证器提交投票,通过共识
   - 使用中位数聚合,防止少数恶意验证器影响结果
3. **Gas 优化**:
   - 不是每分钟都结算资金费率,而是 1 小时聚合一次
   - 减少链上计算和状态更新

### 8.2 为什么溢价用中位数聚合,资金费率用平均值?

**原因**:
- **溢价样本 (1 分钟)**:
  - 使用中位数:抗异常值,防止单个验证器错误投票影响结果
  - 更鲁棒,适合多方投票聚合
- **资金费率 (1 小时)**:
  - 使用平均值:反映整体市场趋势
  - 已经过中位数过滤,异常值已被剔除
  - 平均值更平滑,符合资金费率的经济意义

### 8.3 OIMF 机制的目的是什么?

**目的**:
- **限制系统性风险**:防止单个市场持仓过大导致系统性清算
- **动态风险调整**:市场持仓越大,风险越高,保证金要求越高
- **抑制过度投机**:高 OI 时提高保证金,自然限制新开仓

**工作原理**:
```
市场 OI 低 → 保证金正常 → 允许自由开仓
市场 OI 接近上限 → 保证金提高到 100% → 事实上禁止新开仓
```

### 8.4 为什么未平仓合约量要存储在 TransientStore?

**原因**:
- **性能优化**:避免每次订单匹配都更新 StateStore
- **批量写入**:区块内累积变化,EndBlocker 一次性更新
- **自动清理**:TransientStore 区块结束自动清空,无需手动管理

### 8.5 Cross 和 Isolated 保证金模式的区别?

**Cross (全仓)**:
- 所有仓位共享账户全部余额作为保证金
- 一个仓位爆仓可能导致其他仓位被清算
- 资金利用率高,但风险集中

**Isolated (逐仓)**:
- 每个仓位独立使用固定保证金
- 仓位爆仓只损失该仓位的保证金,不影响其他仓位
- 风险隔离,但资金利用率低

**使用场景**:
- Cross:专业交易者,精细风险管理
- Isolated:保守交易者,限制单个仓位风险

---

## 九、数据访问模式参考

### 9.1 常见读取模式

```go
// 读取永续合约
perpetual, found := keeper.GetPerpetual(ctx, perpetualId)

// 读取流动性层级
liquidityTier, found := keeper.GetLiquidityTier(ctx, liquidityTierId)

// 读取溢价样本
premiumSamples := keeper.GetPremiumSamples(ctx)

// 读取模块参数
params := keeper.GetParams(ctx)
```

### 9.2 常见写入模式

```go
// 更新资金费率指数
perpetual := keeper.GetPerpetual(ctx, perpetualId)
perpetual.FundingIndex = perpetual.FundingIndex.Add(fundingRate)
keeper.SetPerpetual(ctx, perpetual)

// 更新未平仓合约量 (区块内累加)
keeper.AddToOpenInterestDelta(ctx, perpetualId, quantumsDelta)

// 添加溢价样本
samples := keeper.GetPremiumSamples(ctx)
samples.AllMarketPremiums = append(samples.AllMarketPremiums, newSample)
samples.NumPremiums++
keeper.SetPremiumSamples(ctx, samples)
```

### 9.3 周期性任务模式

```go
// EndBlocker: 资金费率结算 (每小时)
if isFundingTickEpochEnd(ctx) {
    samples := keeper.GetPremiumSamples(ctx)
    fundingRate := calculateFundingRate(samples)

    // 更新所有永续合约
    perpetuals := keeper.GetAllPerpetuals(ctx)
    for _, p := range perpetuals {
        p.FundingIndex = p.FundingIndex.Add(fundingRate)
        keeper.SetPerpetual(ctx, p)
    }

    // 清空样本
    keeper.SetPremiumSamples(ctx, types.PremiumStore{})
}

// EndBlocker: OI 批量更新 (每个区块)
deltas := keeper.GetAllUpdatedOpenInterest(ctx)
for _, delta := range deltas {
    perpetual := keeper.GetPerpetual(ctx, delta.PerpetualId)
    perpetual.OpenInterest = perpetual.OpenInterest.Add(delta.BaseQuantumsDelta)
    keeper.SetPerpetual(ctx, perpetual)
}
```

---

## 十、监控与诊断

### 10.1 关键指标

**状态存储指标**:
- 永续合约数量
- 流动性层级数量
- 溢价样本数量 (应 <= 60)
- 溢价投票数量 (应 <= 验证器数量)

**资金费率指标**:
- 当前资金费率 (每个合约)
- 资金费率历史趋势
- 溢价异常检测 (极端值)

**未平仓合约量指标**:
- 每个合约的 OI
- OI / OI Cap 比率 (接近 1 时高风险)
- OI 增长速率

### 10.2 诊断工具

**查询接口**:
```bash
# 查询永续合约
hermesprotocold query perpetuals perpetual <perpetual-id>

# 查询流动性层级
hermesprotocold query perpetuals liquidity-tiers

# 查询溢价投票
hermesprotocold query perpetuals premium-votes

# 查询溢价样本
hermesprotocold query perpetuals premium-samples

# 查询模块参数
hermesprotocold query perpetuals params
```

**导出状态**:
```bash
# 导出创世状态
hermesprotocold export > genesis.json

# 分析永续合约数量
cat genesis.json | jq '.app_state.perpetuals.perpetuals | length'

# 分析未平仓合约量
cat genesis.json | jq '.app_state.perpetuals.perpetuals[] | {ticker: .params.ticker, oi: .open_interest}'
```

### 10.3 常见问题诊断

**问题 1: 资金费率异常**
```
诊断步骤:
1. 查询溢价样本:检查是否有异常高的溢价
2. 查询溢价投票:检查验证器投票是否正常
3. 检查模块参数:确认钳位系数配置正确
4. 分析订单簿:检查是否存在价格偏离
```

**问题 2: 未平仓合约量不更新**
```
诊断步骤:
1. 检查 TransientStore 的 UpdatedOI 是否有数据
2. 检查 EndBlocker 是否正常执行
3. 查看日志:是否有错误信息
4. 重启节点,检查恢复后是否正常
```

**问题 3: OIMF 调整不生效**
```
诊断步骤:
1. 查询流动性层级:检查 OI Cap 配置是否正确
2. 查询永续合约:检查 OpenInterest 值
3. 计算 OIMF:手动验证计算逻辑
4. 检查 Subaccounts 模块:确认保证金计算使用了正确的 IMR
```

---

## 十一、总结

Perpetuals 模块的数据结构设计体现了以下关键原则:

1. **分层聚合**: 两层周期机制(1分钟 + 1小时)确保资金费率的精度和可靠性
2. **性能优化**: TransientStore 批量更新 OI,减少 StateStore 写入
3. **数据压缩**: 仅存储非零溢价,大幅节省存储空间
4. **动态风险调整**: OIMF 机制根据市场 OI 动态调整保证金要求
5. **共识保证**: 溢价投票通过验证器共识,使用中位数聚合防止作恶

**关键数据流总结**:
- 资金费率: 投票 → 样本 → 聚合 → 结算
- 未平仓合约量: 匹配 → 累加 → 批量更新 → OIMF 调整
- 保证金计算: 流动性层级 + OI + OIMF → 动态保证金率

**设计亮点**:
- **中位数 + 平均值**: 两级聚合确保数据可靠性和平滑性
- **批量更新**: TransientStore 累积变化,减少 StateStore 写入
- **稀疏存储**: 仅存储非零溢价,大幅节省空间
- **动态调整**: OIMF 机制自动限制系统性风险

**未来优化方向**:
- 进一步优化溢价数据存储(可能使用压缩算法)
- 改进资金费率计算算法,更精准反映市场状况
- 增强 OIMF 机制,支持更灵活的风险调整曲线

---

**文档版本**: v1.0
**最后更新**: 2025-12-31
**作者**: Claude Sonnet 4.5 + Hermes DEX Team
**参考代码版本**: `protocol/x/perpetuals` (commit: 414ee78)
