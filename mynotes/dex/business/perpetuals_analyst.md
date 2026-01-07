`x/perpetuals` 负责管理永续合约（Perpetual Contracts）的创建、配置、资金费率计算和未平仓合约量跟踪。该模块为去中心化交易所提供永续合约的基础设施支持。

### 1.1 模块职责

- 永续合约的创建和生命周期管理
- 资金费率（Funding Rate）的计算和更新，溢价（Premium）投票和采样机制
- 未平仓合约量（Open Interest）的跟踪和管理
- 流动性层级（Liquidity Tier）配置
- 保证金计算

## 2. 数据模型

### 2.1 核心数据结构

#### 2.1.1 Perpetual（永续合约）

```Plain
type Perpetual struct {
    // 永续合约参数
    Params PerpetualParams
    
    // 资金费率指数，表示累计的资金费率历史
    FundingIndex SerializableInt
    
    // 未平仓合约量，以基础资产量子为单位
    OpenInterest SerializableInt
}
```

**字段说明：**

- `Params`: 永续合约的配置参数
- `FundingIndex`: 资金费率指数，用于计算持仓的资金费用
- `OpenInterest`: 当前未平仓合约量总量（多头和空头的净敞口）

#### 2.1.2 PerpetualParams（永续合约参数）

```Go
type PerpetualParams struct {
    Id                uint32              // 唯一标识符，顺序生成
    Ticker            string              // 交易对名称，如 "BTC-USD"
    MarketId          uint32              // 关联的价格市场ID（用于获取Oracle价格）
    AtomicResolution  int32               // 原子精度，用于数量转换
    DefaultFundingPpm int32               // 默认资金费率（8小时，PPM单位）
    LiquidityTier     uint32              // 关联的流动性层级ID
    MarketType        PerpetualMarketType // 市场类型：CROSS（全仓）或 ISOLATED（逐仓）
}
```

**字段说明：**

- `Id`: 永续合约的唯一标识，按顺序递增
- `Ticker`: 交易对符号，用于显示和识别
- `MarketId`: 关联到 `x/prices` 模块的市场ID，用于获取Oracle价格
- `AtomicResolution`: 基础资产的精度，例如 -8 表示 1e8 原子单位 = 1 个完整币
- `DefaultFundingPpm`: 默认资金费率，当没有溢价时使用（PPM = parts per million）
- `LiquidityTier`: 流动性层级ID，决定保证金要求和资金费率限制
- `MarketType`: 市场类型，影响保证金计算和保险基金分配

#### 2.1.3 LiquidityTier（流动性层级）

```Go
type LiquidityTier struct {
    Id                        uint32  // 唯一标识符
    Name                      string  // 层级名称
    InitialMarginPpm          uint32  // 初始保证金率（PPM）
    MaintenanceFractionPpm    uint32  // 维持保证金比例（PPM）
    BasePositionNotional      uint64  // 基础仓位名义价值（USDC，已废弃）
    ImpactNotional             uint64  // 影响名义价值，用于溢价计算
    OpenInterestLowerCap       uint64  // 未平仓合约量下限（USDC，报价资产量子）
    OpenInterestUpperCap       uint64  // 未平仓合约量上限（USDC，报价资产量子）
}
```

**字段说明：**

- `InitialMarginPpm`: 初始保证金要求（例如 50000 PPM = 5%），表示开仓所需的最小保证金比例
- `MaintenanceFractionPpm`: 维持保证金相对于初始保证金的比例（例如 600000 PPM = 60%），表示维持保证金 = 初始保证金 × 60%
- `ImpactNotional`: 用于计算价格溢价的影响名义价值（报价资产量子），用于计算订单簿的影响价格
- `OpenInterestLowerCap`: 未平仓合约量下限（报价资产量子），当未平仓合约量低于此值时，IMF不调整
- `OpenInterestUpperCap`: 未平仓合约量上限（报价资产量子），当未平仓合约量达到此值时，IMF调整为100%

**关键方法：**

- `GetMaintenanceMarginPpm()`: 计算维持保证金率 = InitialMarginPpm × MaintenanceFractionPpm
- `GetMaxAbsFundingClampPpm()`: 计算资金费率的最大绝对值限制 = ClampFactor × (InitialMargin - MaintenanceMargin)
- `GetAdjustedInitialMarginPpm()`: 根据当前未平仓合约量计算调整后的初始保证金率（OIMF）
- `GetInitialMarginQuoteQuantums()`: 计算初始保证金要求（报价资产量子）= 持仓名义价值 × 调整后的IMF

**LiquidityTier 的作用：**

1. **集中管理风险参数**：为多个永续合约提供统一的保证金和风险参数配置
2. **定义保证金要求**：通过 `InitialMarginPpm` 和 `MaintenanceFractionPpm` 定义初始和维持保证金
3. **控制资金费率**：通过 `GetMaxAbsFundingClampPpm` 限制资金费率的绝对值
4. **动态调整保证金**：通过 OIMF 机制根据未平仓合约量动态调整初始保证金要求

#### 2.1.4 PremiumStore（溢价存储）

```Go
type PremiumStore struct {
    NumPremiums       uint32           // 当前存储的溢价样本数量
    AllMarketPremiums []MarketPremiums // 所有市场的溢价列表
}

type MarketPremiums struct {
    PerpetualId uint32  // 永续合约ID
    Premiums    []int32 // 溢价值列表（PPM）
}
```

**用途：**

- `PremiumVotesKey`: 存储当前 `funding-sample` 周期内的溢价投票
- `PremiumSamplesKey`: 存储当前 `funding-tick` 周期内的溢价样本

#### 2.1.5 Params（模块参数）

```Go
type Params struct {
    FundingRateClampFactorPpm  uint32 // 资金费率限制因子（PPM）
    PremiumVoteClampFactorPpm   uint32 // 溢价投票限制因子（PPM）
    MinNumVotesPerSample        uint32 // 每个样本的最小投票数
}
```

**默认值：**

- `FundingRateClampFactorPpm`: 6,000,000 (600%)
- `PremiumVoteClampFactorPpm`: 60,000,000 (6000%)
- `MinNumVotesPerSample`: 15

### 2.2 存储结构

#### 2.2.1 持久化存储（StoreKey）

| Key前缀           | 说明             | 存储内容             |
| ----------------- | ---------------- | -------------------- |
| `Perp:`           | 永续合约         | `Perpetual` 对象     |
| `LiqTier:`        | 流动性层级       | `LiquidityTier` 对象 |
| `Params`          | 模块参数         | `Params` 对象        |
| `NextPerpetualID` | 下一个永续合约ID | `uint32`             |
| `PremVotes`       | 溢价投票         | `PremiumStore` 对象  |
| `PremSamples`     | 溢价样本         | `PremiumStore` 对象  |

#### 2.2.2 临时存储（TransientStoreKey）

| Key前缀     | 说明               | 存储内容                     |
| ----------- | ------------------ | ---------------------------- |
| `UpdatedOI` | 更新的未平仓合约量 | `map[uint32]SerializableInt` |

## 3. 核心业务场景

### 3.1 永续合约管理

#### 3.1.1 创建永续合约

**消息类型：** `MsgCreatePerpetual`

**流程：**

1. 验证权限（Authority）
2. 验证参数有效性（stateless validation）
3. 检查永续合约ID是否已存在
4. 验证关联的 `MarketId` 是否存在（通过 PricesKeeper）
5. 验证关联的 `LiquidityTier` 是否存在
6. 创建 `Perpetual` 对象，初始化：
   1. `FundingIndex = 0`
   2. `OpenInterest = 0`
7. 存储到状态
8. 初始化空的溢价投票和样本存储
9. 发送 Indexer 事件

**关键验证：**

- Ticker 不能为空
- DefaultFundingPpm 绝对值不能超过 1,000,000 (100%)
- MarketType 必须是 CROSS 或 ISOLATED

#### 3.1.2 修改永续合约

**消息类型：** `MsgUpdatePerpetualParams`

**可修改字段：**

- `Ticker`
- `MarketId`
- `DefaultFundingPpm`
- `LiquidityTier`

**不可修改字段：**

- `Id`
- `AtomicResolution`
- `MarketType`（CROSS 类型不能修改，ISOLATED 可以修改为 CROSS）

**流程：**

1. 验证权限
2. 获取现有永续合约
3. 更新可修改字段
4. 执行状态验证（包括验证新的 MarketId 和 LiquidityTier）
5. 存储更新后的永续合约
6. 发送 Indexer 事件

### 3.2 资金费率机制

#### 3.2.1 资金费率计算流程

资金费率计算采用**两层周期机制**：

1. **Funding Sample Epoch（资金采样周期）**
   1. 周期：通常为 1 分钟（在分钟的第 30 秒触发）
   2. 操作：收集溢价投票，聚合成溢价样本
2. **Funding Tick Epoch（资金费率周期）**
   1. 周期：通常为 8 小时（每小时整点触发）
   2. 操作：处理溢价样本，计算最终资金费率，更新资金费率指数

#### 3.2.2 溢价投票（Premium Votes）

**消息类型：** `MsgAddPremiumVotes`

**数据提供者：** 区块提议者（Proposer）在 `PrepareProposal` 阶段

**流程：**

1. 区块提议者调用 `GetAddPremiumVotes` 计算溢价
2. 对每个永续合约：
   1. 获取 Oracle 价格（索引价格）
   2. 从订单簿获取影响价格（Impact Price）
   3. 计算溢价 = (影响价格 - Oracle价格) / Oracle价格
3. 验证器提交溢价投票（每个永续合约一个投票值）
4. 验证投票的有效性：
   1. 投票必须按 PerpetualId 排序
   2. 不能有重复的 PerpetualId
   3. 溢价值必须在限制范围内
5. 存储到 `PremiumVotesKey`

**溢价计算公式：**

```Plain
溢价 = (Max(0, Impact Bid - Index Price) - Max(0, Index Price - Impact Ask)) / Index Price
```

简化：

- 如果 `Index Price < Impact Bid`：`溢价 = Impact Bid / Index Price - 1`（正溢价，多头支付）
- 如果 `Impact Bid ≤ Index Price ≤ Impact Ask`：`溢价 = 0`
- 如果 `Index Price > Impact Ask`：`溢价 = Impact Ask / Index Price - 1`（负溢价，空头支付）

**影响价格（Impact Price）：**

- `Impact Bid`: 买入 `ImpactNotional` 数量订单的平均价格
- `Impact Ask`: 卖出 `ImpactNotional` 数量订单的平均价格
- 如果 `ImpactNotional = 0`，则使用最佳买卖价（Best Bid/Ask）

**溢价值限制：**

- 受 `PremiumVoteClampFactorPpm` 限制
- 最大绝对值 = `ClampFactor × (InitialMargin - MaintenanceMargin)`

**实现位置：**

- 计算溢价：`x/clob/memclob/memclob.go::GetPricePremium`
- 生成投票：`keeper/perpetual.go::sampleAllPerpetuals`
- 提交投票：`app/prepare/prepare_proposal.go::GetAddPremiumVotesTx`

#### 3.2.3 溢价采样聚合（Premium Sample Aggregation）

**触发时机：** `funding-sample` 周期开始时（每分钟的第30秒）

**数据来源：** 多个验证器节点在 `PrepareProposal` 阶段提交的溢价投票

**为什么需要聚合？**

- 多个验证器会提交不同的溢价投票（订单簿状态、网络延迟、计算时机不同）
- 需要达成共识，得到一致的溢价值
- 使用中位数可以抵抗异常值（恶意或错误的投票）

**流程：**

1. 检查是否是新周期的开始
2. 读取所有溢价投票（`PremiumVotesKey`）
3. 对每个永续合约的投票进行聚合：
   1. 使用中位数（median）方法聚合投票
   2. 如果投票数不足 `MinNumVotesPerSample`，用零填充
   3. 不进行尾部移除（使用 identity filter）
4. 生成溢价样本，存储到 `PremiumSamplesKey`
5. 清空溢价投票存储
6. 发送 Indexer 事件

**聚合方法：**

- **使用中位数而非平均值**：提高抗异常值能力
  - 示例：投票值 [200, 210, 195, 205, 1000]（异常值）
  - 平均值：(200 + 210 + 195 + 205 + 1000) / 5 = 362 PPM ❌ 被异常值影响
  - 中位数：205 PPM ✅ 不受异常值影响

**数据结构：**

- `PremiumStore`: 存储所有市场的溢价
- `MarketPremiums`: 每个永续合约的溢价值列表（只存储非零值）
- `FundingPremium`: 单个溢价值（PerpetualId + PremiumPpm）

**实现位置：** `keeper/perpetual.go::processPremiumVotesIntoSamples`

#### 3.2.4 资金费率计算（Funding Rate Calculation）

**触发时机：** `funding-tick` 周期开始时（每小时整点）

**数据来源：** 过去1小时的溢价样本（`PremiumSamplesKey`，通常60个样本）

**为什么需要聚合样本？**

- 过去1小时有60个样本（每分钟一个）
- 需要平滑波动，反映整体市场趋势
- 使用平均值可以平滑时间序列的波动

**流程：**

1. 检查是否是新周期的开始
2. 读取所有溢价样本（过去1小时的样本）
3. 对每个永续合约：
   1. **聚合样本**（使用平均值）
      - 如果样本数不足（少于60个），用零填充
      - 应用尾部移除过滤（tail removal）：移除排序后的最大值和最小值
      - 计算平均值 = sum(样本值) / 样本数
   2. **计算资金费率** = 溢价 + 默认资金费率
      - ```Plain
        FundingRate = PremiumPpm + DefaultFundingPpm
        ```
   3. **应用资金费率限制（clamp）**：
      - ```Plain
        |FundingRate| <= ClampFactor × (InitialMargin - MaintenanceMargin)
        ```
   4. **更新资金费率指数**（如果资金费率非零）：
      - ```Plain
        FundingIndexDelta = FundingRate × (1小时 / 8小时) × Price
        ```
4. 更新 `Perpetual.FundingIndex`
5. 清空溢价样本存储
6. 发送 Indexer 事件

**尾部移除（Tail Removal）说明：**

- **不是移除"最后一个"样本**（按时间顺序）
- **而是移除排序后的最大值和最小值**（按数值大小）
- 目的：抵抗异常值，提高资金费率计算的稳定性
- 当前配置：`RemovedTailSampleRatioPpm = 0`（不移除任何样本）
- 实现位置：`keeper/perpetual.go::GetRemoveSampleTailsFunc`

**示例：**

```Plain
过去1小时的样本：[200, 205, 198, 210, 202, ..., 208]  // 60个样本

排序后：[195, 198, 200, 202, 205, ..., 210, 215, 220]
         ↑最小值                          ↑最大值

如果移除10%（6个最小 + 6个最大）：
移除后：[200, 202, 205, ..., 208]  // 48个稳定样本

平均值 = (200 + 202 + 205 + ... + 208) / 48 = 203 PPM
```

**资金费率的组成：**

- **溢价（Premium）**：动态计算，反映市场供需
- **默认资金费率（DefaultFundingPpm）**：静态配置，永续合约参数
- 示例：资金费率 = 300 PPM（溢价）+ 500 PPM（默认）= 800 PPM

**实现位置：** `keeper/perpetual.go::MaybeProcessNewFundingTickEpoch`

**资金费率指数更新公式：**

```Plain
FundingIndexDelta = FundingRatePpm × (timeSinceLastFunding / 8hours) × quoteQuantumsPerBaseQuantum
```

其中：

- `FundingRatePpm`: 8小时资金费率（PPM），表示如果持续8小时会产生多少资金费率
- `timeSinceLastFunding`: 自上次更新以来的时间（秒），通常为3600秒（1小时）
- `quoteQuantumsPerBaseQuantum`: 每个基础资产量子对应的报价资产量子数 = Price × 10^(marketExponent + quoteAtomicResolution - baseAtomicResolution)

**为什么乘以 (1小时 / 8小时)？**

- 资金费率以8小时为基准，但系统每小时更新一次
- 需要按实际时间比例计算，确保8次每小时更新 = 1次8小时更新
- 例如：800 PPM × (3600秒 / 28800秒) = 800 × 0.125 = 100 PPM（1小时的效果）

**FundingIndex 的用途：**

- 存储在 `Perpetual.FundingIndex` 中：永续合约的累计资金费率指数
- 存储在 `PerpetualPosition.FundingIndex` 中：持仓上次结算时的资金费率指数
- 计算资金费用：`资金费用 = -(Perpetual.FundingIndex - PerpetualPosition.FundingIndex) × 持仓数量`
- 实现位置：`funding/funding.go::GetFundingIndexDelta` 和 `lib/lib.go::GetSettlementPpmWithPerpetual`

### 3.3 未平仓合约量管理

#### 3.3.1 未平仓合约量（Open Interest, OI）的定义

**定义：** 未平仓合约量表示永续合约市场中所有未平仓持仓的总量

**计算公式：**

```Plain
OI = |所有多头持仓总和 - 所有空头持仓总和|
```

**实际实现：**

- OI 只跟踪多头持仓的变化（delta long）
- 当订单成交时，计算两个账户的多头持仓变化，然后相加得到 OI delta
- 实现位置：`x/subaccounts/lib/oimf.go::GetDeltaOpenInterestFromUpdates`

**存储单位：** 基础资产的量子（base quantums）

**示例：**

```Plain
假设市场中有以下持仓：
- 账户A：+1 BTC（多头）
- 账户B：-0.5 BTC（空头）
- 账户C：+0.3 BTC（多头）

OI = |(1 + 0.3) - 0.5| = |1.3 - 0.5| = 0.8 BTC
```

#### 3.3.2 未平仓合约量更新

**方法：** `ModifyOpenInterest`

**调用方：**

- `x/subaccounts` 模块：在订单成交时（Match update type）更新
- 清算模块：在清算时更新

**更新时机：**

- 仅在订单成交（Match）时更新
- 其他更新类型（PlaceOrder、CancelOrder等）不更新 OI

**更新流程：**

1. 计算 OI Delta：
   1. 对于每个参与成交的账户，计算多头持仓的变化
   2. `Delta Long = Max(0, 新持仓) - Max(0, 旧持仓)`
   3. OI Delta = 所有账户的 Delta Long 之和
2. 应用增量：
   1. `新 OI = 旧 OI + OI Delta`
   2. 正数表示增加（开仓）
   3. 负数表示减少（平仓）
3. 验证结果不能为负
4. 更新存储到 `Perpetual.OpenInterest`

**实现位置：**

- 计算 Delta：`x/subaccounts/lib/oimf.go::GetDeltaOpenInterestFromUpdates`
- 更新 OI：`keeper/perpetual.go::ModifyOpenInterest`
- 调用位置：`x/subaccounts/keeper/subaccount.go::UpdateSubaccounts`

**示例：**

```Plain
场景：两个账户成交订单
- 账户A：从 0 BTC 变为 +1 BTC（开多仓）
- 账户B：从 0 BTC 变为 -1 BTC（开空仓）

计算：
- 账户A的 Delta Long = Max(0, 1) - Max(0, 0) = 1
- 账户B的 Delta Long = Max(0, 0) - Max(0, 0) = 0
- OI Delta = 1 + 0 = 1 BTC

更新：
- 新 OI = 旧 OI + 1 BTC
```

**注意事项：**

- OI 以基础资产的量子为单位（不是报价资产）
- OI 只跟踪多头持仓的变化，空头持仓不影响 OI
- OI 不能为负（验证失败会返回错误）
- OI 影响初始保证金率（通过 OIMF 机制）

**未平仓合约量的用途：**

1. **动态调整初始保证金率（OIMF机制）**
   1. 当 OI 超过 `OpenInterestLowerCap` 时，IMF 会根据 OI 线性增加
   2. 当 OI 达到 `OpenInterestUpperCap` 时，IMF 达到 100%（1:1 保证金要求）
   3. 用于风险管理，防止市场过度杠杆化
   4. 实现位置：`types/liquidity_tier.go::GetAdjustedInitialMarginPpm`
2. **风险监控**
   1. OI 反映了市场的总敞口
   2. 高 OI 表示市场风险较高，需要更高的保证金要求
   3. 系统通过 OIMF 机制自动调整保证金要求
3. **市场分析**
   1. OI 可以用于分析市场情绪和趋势
   2. OI 增加表示市场活跃度增加，新开仓增加
   3. OI 减少表示市场平仓增加，持仓减少

#### 3.3.3 未平仓合约量调整的初始保证金（OIMF）

**目的：** 当未平仓合约量过大时，提高初始保证金要求，降低系统风险

**机制：** Open Interest-adjusted Initial Margin Fraction（OIMF）

**计算公式：**

```Plain
如果 OpenInterestUpperCap == 0 或 OI <= LowerCap:
    AdjustedIMF = BaseIMF  // 不调整
    
如果 OI >= UpperCap:
    AdjustedIMF = 1.0 (100%)  // 1:1 保证金要求
    
否则（LowerCap < OI < UpperCap）:
    ScalingFactor = (OI - LowerCap) / (UpperCap - LowerCap)
    IMFIncrease = ScalingFactor × (1,000,000 - BaseIMF)
    AdjustedIMF = BaseIMF + IMFIncrease
```

**示例：**

- `BaseIMF = 50,000 PPM`（5%）
- `LowerCap = $10,000,000`
- `UpperCap = $50,000,000`
- 当前 `OI = $30,000,000`

计算：

```Plain
ScalingFactor = ($30M - $10M) / ($50M - $10M) = 0.5
IMFIncrease = 0.5 × (1,000,000 - 50,000) = 475,000 PPM
AdjustedIMF = 50,000 + 475,000 = 525,000 PPM（52.5%）
```

**特点：**

- 线性插值，平滑过渡
- 防止突然的保证金要求变化
- 当未平仓合约量达到上限时，要求100%保证金（1:1）

**实现位置：** `types/liquidity_tier.go::GetAdjustedInitialMarginPpm`

**使用场景：**

- 计算初始保证金要求时，使用调整后的IMF
- 实现位置：`types/liquidity_tier.go::GetInitialMarginQuoteQuantums`

### 3.4 保证金计算

#### 3.4.1 初始保证金率（Initial Margin Fraction, IMF）

**定义：** 开仓所需的最小保证金比例

**存储位置：** `LiquidityTier.InitialMarginPpm`

**单位：** PPM（Parts Per Million，百万分之一）

**示例：**

- `InitialMarginPpm = 50,000` 表示 5% 的初始保证金率
- `InitialMarginPpm = 100,000` 表示 10% 的初始保证金率

**计算初始保证金要求（IMR）：**

```Plain
IMR = |持仓名义价值| × IMF
```

**实现位置：** `types/liquidity_tier.go::GetInitialMarginQuoteQuantums`

**动态调整（OIMF）：**

- 当未平仓合约量（OI）超过 `OpenInterestLowerCap` 时，IMF 会根据 OI 线性增加
- 当 OI 达到 `OpenInterestUpperCap` 时，IMF 达到 100%（1:1 保证金要求）
- 实现位置：`types/liquidity_tier.go::GetAdjustedInitialMarginPpm`

#### 3.4.2 维持保证金比例（Maintenance Margin Fraction）

**定义：** 维持保证金相对于初始保证金的比例

**存储位置：** `LiquidityTier.MaintenanceFractionPpm`

**单位：** PPM（Parts Per Million）

**示例：**

- `MaintenanceFractionPpm = 600,000` 表示维持保证金 = 初始保证金 × 60%
- 如果 `InitialMarginPpm = 50,000`（5%），则 `MaintenanceMarginPpm = 30,000`（3%）

**计算维持保证金要求（MMR）：**

```Plain
MMR = IMR × MaintenanceFractionPpm
```

**实现位置：** `types/liquidity_tier.go::GetMaintenanceMarginPpm`

**清算条件：**

```Plain
Net Collateral < MMR
```

**说明：**

- 维持保证金总是小于或等于初始保证金
- 当账户的 `Net Collateral` 低于 `MMR` 时，账户会被清算
- 用于风险管理和保护系统免受损失

#### 3.4.3 净名义价值（Net Notional）

**方法：** `GetNetNotionalInQuoteQuantums`

**用途：** 计算持仓的名义价值（以报价资产为单位）

**公式：**

```Plain
NetNotional = quantums / 10^baseAtomicResolution × marketPrice × 10^marketExponent × 10^quoteAtomicResolution
```

**说明：**

- 多头持仓为正数
- 空头持仓为负数
- 用于计算保证金要求
- 实现位置：`lib/lib.go::GetNetNotionalInQuoteQuantums`

#### 3.4.2 净抵押品（Net Collateral）

**方法：** `GetNetCollateralAndMarginRequirements`

**用途：** 计算持仓的净抵押品价值，用于判断账户是否可清算

**计算公式：**

```Plain
Net Collateral (NC) = Net Notional + Quote Balance
```

其中：

- `Net Notional`: 持仓的名义价值（报价资产量子）
  - 多头持仓：正数
  - 空头持仓：负数
  - 计算公式：`quantums / 10^baseAtomicResolution × marketPrice × 10^marketExponent × 10^quoteAtomicResolution`
- `Quote Balance`: 持仓的资金费用余额（报价资产量子）
  - 通过 `FundingIndex` 差值计算得出
  - 多头持仓：如果 `FundingIndex` 增加，`Quote Balance` 减少（支付资金费用）
  - 空头持仓：如果 `FundingIndex` 增加，`Quote Balance` 增加（收取资金费用）

**实现位置：** `lib/lib.go::GetNetCollateralAndMarginRequirements`

**清算条件：**

```Plain
Net Collateral < Maintenance Margin Requirement (MMR)
```

**说明：**

- `Net Collateral` 是实时计算的值，不存储在状态中
- 在子账户模块中，会聚合所有资产和永续持仓的 `Net Collateral`
- 用于判断账户是否可清算，以及计算可用保证金

### 3.5 流动性层级管理

#### 3.5.1 设置流动性层级

**消息类型：** `MsgSetLiquidityTier`

**流程：**

1. 验证权限
2. 验证参数有效性：
   1. InitialMarginPpm <= 1,000,000 (100%)
   2. MaintenanceFractionPpm <= 1,000,000 (100%)
   3. ImpactNotional > 0
   4. OpenInterestLowerCap <= OpenInterestUpperCap
3. 存储或更新流动性层级
4. 发送 Indexer 事件

**用途：**

- 为永续合约配置保证金要求
- 配置资金费率限制
- 配置未平仓合约量调整参数

## 4. 上下游系统依赖

### 4.1 上游依赖（输入）

#### 4.1.1 x/prices 模块

**依赖接口：** `PricesKeeper`

**用途：**

- 获取 Oracle 价格（`GetMarketPrice`）
- 获取有效的索引价格映射（`GetMarketIdToValidIndexPrice`）

**使用场景：**

- 创建永续合约时验证 MarketId 存在
- 计算资金费率指数时获取价格
- 计算净名义价值和抵押品时获取价格

#### 4.1.2 x/epochs 模块

**依赖接口：** `EpochsKeeper`

**用途：**

- 获取 `funding-sample` 周期信息
- 获取 `funding-tick` 周期信息
- 检查是否是新周期的开始

**使用场景：**

- `MaybeProcessNewFundingSampleEpoch`: 检查并处理新的采样周期
- `MaybeProcessNewFundingTickEpoch`: 检查并处理新的资金费率周期

#### 4.1.3 x/clob 模块

**依赖接口：** `PerpetualsClobKeeper`

**用途：**

- 获取价格溢价（`GetPricePremiumForPerpetual`）
- 检查永续合约是否活跃（`IsPerpetualClobPairActive`）

**使用场景：**

- 验证器节点计算溢价投票时获取订单簿溢价
- 验证永续合约是否可用于交易

**注意：** 这是一个双向依赖关系，通过 `SetClobKeeper` 方法在初始化后设置。

### 4.2 下游依赖（输出）

#### 4.2.1 x/subaccounts 模块

**提供接口：** `PerpetualsKeeper`

**被调用方法：**

- `GetNetNotional`: 计算持仓的名义价值
- `GetNetCollateral`: 计算持仓的净抵押品
- `GetPerpetual`: 获取永续合约信息
- `GetLiquidityTier`: 获取流动性层级信息
- `GetPerpetualAndMarketPrice`: 获取永续合约和价格信息

**使用场景：**

- 子账户的保证金计算
- 持仓的抵押品评估
- 风险检查

#### 4.2.2 x/clob 模块

**提供接口：** `PerpetualsKeeper`

**被调用方法：**

- `ModifyOpenInterest`: 更新未平仓合约量
- `GetPerpetual`: 获取永续合约信息
- `IsIsolatedPerpetual`: 检查是否为逐仓市场
- `GetInsuranceFundName`: 获取保险基金账户名

**使用场景：**

- 订单成交后更新未平仓合约量
- 验证订单参数
- 确定保险基金账户

#### 4.2.3 Indexer 事件

**事件类型：**

- `FundingValues`: 资金费率更新事件
- `UpdatePerpetual`: 永续合约更新事件

**事件内容：**

- 资金费率值
- 资金费率指数
- 溢价样本值
- 永续合约参数变更

**用途：** 供外部系统（如前端、API）订阅和查询

### 4.3 模块账户

#### 4.3.1 保险基金（Insurance Fund）

**账户命名规则：**

- 全仓市场：`insurance_fund`
- 逐仓市场：`insurance_fund:<perpetualId>`

**用途：**

- 存储保险基金资金
- 用于清算和风险覆盖

**管理：** 由其他模块（如清算模块）管理资金，perpetuals 模块仅提供账户地址查询。

## 5. 关键技术实现

### 5.1 资金费率指数计算

**实现位置：** `funding/funding.go::GetFundingIndexDelta`

**算法：**

```Go
fundingIndexDelta = fundingRatePpm × (timeSinceLastFunding / 8hours) × quoteQuantumsPerBaseQuantum
```

**关键点：**

- 先乘后除，避免精度损失
- 使用截断除法（towards zero）
- 8小时周期硬编码（TODO: 可配置化）

**精度处理：**

- 使用 `math/big.Int` 进行大数运算
- 价格转换使用 `BaseToQuoteQuantums` 辅助函数

### 5.2 溢价采样聚合

**实现位置：** `keeper/perpetual.go::processStoredPremiums`

**算法步骤：**

1. 读取所有溢价样本
2. 对每个永续合约：
   1. 如果样本数不足，用零填充
   2. 应用尾部移除过滤（tail removal）
   3. 使用聚合函数（平均值或中位数）计算最终值

**聚合函数：**

- `funding-sample`: 使用中位数（median）
- `funding-tick`: 使用平均值（average）

**尾部移除：**

- 移除一定比例的头部和尾部样本（按数值大小排序后）
- 默认移除比例：`RemovedTailSampleRatioPpm`（当前为 0%，不移除任何样本）
- 实现逻辑：
  - 排序样本（从小到大）
  - 计算移除数量：`totalRemoval = len(samples) × tailRemovalRatePpm × 2`
  - 移除底部：`bottomRemoval = totalRemoval / 2`（最小值）
  - 移除顶部：`topRemoval = totalRemoval - bottomRemoval`（最大值）
  - 返回中间部分：`samples[bottomRemoval:len(samples)-topRemoval]`
- 实现位置：`keeper/perpetual.go::GetRemoveSampleTailsFunc`

### 5.3 资金费率限制（Clamping）

**实现位置：** `keeper/perpetual.go::MaybeProcessNewFundingTickEpoch`

**限制公式：**

```Plain
|FundingRate| <= ClampFactor × (InitialMargin - MaintenanceMargin)
```

**目的：**

- 防止资金费率过大导致系统风险
- 确保资金费率在合理范围内

**实现：**

```Go
fundingRateUpperBoundPpm := liquidityTier.GetMaxAbsFundingClampPpm(params.FundingRateClampFactorPpm)
bigFundingRatePpm = lib.BigIntClamp(
    bigFundingRatePpm,
    new(big.Int).Neg(fundingRateUpperBoundPpm),
    fundingRateUpperBoundPpm,
)
```

### 5.4 Net Collateral 计算

**实现位置：** `lib/lib.go::GetNetCollateralAndMarginRequirements`

**计算流程：**

1. 计算持仓的名义价值（Net Notional）
2. 计算资金费用余额（Quote Balance）
3. 相加得到 Net Collateral

**详细公式：**

```Plain
Net Collateral = Net Notional + Quote Balance

其中：
- Net Notional = quantums × price × 10^(marketExponent + quoteAtomicResolution - baseAtomicResolution)
- Quote Balance = -(Perpetual.FundingIndex - PerpetualPosition.FundingIndex) × quantums
```

**Quote Balance 的计算：**

- 通过比较 `Perpetual.FundingIndex`（当前索引）和 `PerpetualPosition.FundingIndex`（持仓上次结算时的索引）
- 差值表示自上次结算以来的资金费率累计
- 资金费用 = -差值 × 持仓数量
  - 如果 `FundingIndex` 增加：多头支付资金费用（Quote Balance 减少），空头收取（Quote Balance 增加）
  - 如果 `FundingIndex` 减少：多头收取资金费用（Quote Balance 增加），空头支付（Quote Balance 减少）

**实现位置：** `lib/lib.go::GetSettlementPpmWithPerpetual`

**在子账户中的使用：**

- `x/subaccounts` 模块会聚合所有资产和永续持仓的 `Net Collateral`
- 用于判断账户是否可清算：`Net Collateral < Maintenance Margin Requirement`
- 实现位置：`x/subaccounts/lib/updates.go::GetRiskForSubaccount`

### 5.5 未平仓合约量调整的初始保证金（OIMF）

**实现位置：** `types/liquidity_tier.go::GetAdjustedInitialMarginPpm`

**算法：**

```Go
if OI <= LowerCap:
    return BaseIMF
if OI >= UpperCap:
    return 1.0 (100%)
else:
    scalingFactor = (OI - LowerCap) / (UpperCap - LowerCap)
    imfIncrease = scalingFactor × (1,000,000 - BaseIMF)
    return BaseIMF + imfIncrease
```

**特点：**

- 线性插值
- 平滑过渡
- 防止突然的保证金要求变化

### 5.6 保证金计算流程

**实现位置：** `lib/lib.go::GetMarginRequirementsInQuoteQuantums`

**计算步骤：**

1. **计算持仓名义价值（绝对值）**
   1. ```Plain
      QuoteQuantums = |quantums| × price × 10^(marketExponent + quoteAtomicResolution - baseAtomicResolution)
      ```
2. **计算基础初始保证金要求（Base IMR）**
   1. ```Plain
      Base IMR = QuoteQuantums × BaseIMF
      ```

   2. 使用 `OpenInterest = 0` 计算，得到基础保证金要求
3. **计算维持保证金要求（MMR）**
   1. ```Plain
      MMR = Base IMR × MaintenanceFractionPpm
      ```

   2. 维持保证金基于基础 IMR，不受 OI 影响
4. **计算调整后的初始保证金要求（Adjusted IMR）**
   1. ```Plain
      Adjusted IMR = QuoteQuantums × AdjustedIMF(OI)
      ```

   2. 使用当前未平仓合约量（OI）计算调整后的 IMF
   3. 如果 `custom_imf_ppm > 0`，使用 `max(AdjustedIMF, custom_imf_ppm)`

**完整示例：**

```Plain
假设：
- 持仓：1 BTC（多头）
- 价格：$50,000
- BaseIMF：5% (50,000 PPM)
- MaintenanceFraction：60% (600,000 PPM)
- OI：$30,000,000
- LowerCap：$10,000,000
- UpperCap：$50,000,000

计算：
1. QuoteQuantums = 1 × $50,000 = $50,000
2. Base IMR = $50,000 × 5% = $2,500
3. MMR = $2,500 × 60% = $1,500
4. AdjustedIMF = 52.5% (根据 OI 计算)
5. Adjusted IMR = $50,000 × 52.5% = $26,250
```

**清算判断：**

```Plain
如果 Net Collateral < MMR：
    账户可被清算
```

### 5.7 状态验证

#### 5.5.1 Stateless Validation

**位置：** `types/perpetual.go::Validate`

**检查项：**

- MarketType 有效性
- Ticker 非空
- DefaultFundingPpm 范围

#### 5.5.2 Stateful Validation

**位置：** `keeper/perpetual.go::ValidateAndSetPerpetual`

**检查项：**

- MarketId 存在性（通过 PricesKeeper）
- LiquidityTier 存在性
- 原子精度一致性

### 5.8 EndBlock 处理

**实现位置：** `abci.go::EndBlocker`

**执行顺序：**

1. `MaybeProcessNewFundingSampleEpoch`: 处理新的采样周期
2. `MaybeProcessNewFundingTickEpoch`: 处理新的资金费率周期

**注意：** 两个周期通常不会在同一区块触发，但如果触发，采样周期先处理。

### 5.9 事件发送

**事件类型：**

- `FundingValues`: 资金费率更新
- `UpdatePerpetual`: 永续合约更新

**发送时机：**

- 资金费率更新时（每个 funding-tick 周期）
- 溢价样本生成时（每个 funding-sample 周期）
- 永续合约创建/修改时

**事件格式：** Protobuf 编码，通过 IndexerEventManager 发送

## 6. 消息和查询接口

### 6.1 消息类型（Messages）

| 消息类型                   | 说明             | 权限要求  |
| -------------------------- | ---------------- | --------- |
| `MsgCreatePerpetual`       | 创建永续合约     | Authority |
| `MsgUpdatePerpetualParams` | 更新永续合约参数 | Authority |
| `MsgAddPremiumVotes`       | 添加溢价投票     | Validator |
| `MsgSetLiquidityTier`      | 设置流动性层级   | Authority |
| `MsgUpdateParams`          | 更新模块参数     | Authority |

### 6.2 查询接口（Queries）

| 查询类型                 | 说明               |
| ------------------------ | ------------------ |
| `QueryPerpetual`         | 查询单个永续合约   |
| `QueryAllPerpetuals`     | 查询所有永续合约   |
| `QueryPremiumVotes`      | 查询当前溢价投票   |
| `QueryPremiumSamples`    | 查询当前溢价样本   |
| `QueryParams`            | 查询模块参数       |
| `QueryAllLiquidityTiers` | 查询所有流动性层级 |
| `QueryLiquidityTier`     | 查询单个流动性层级 |

## 7. 错误处理

### 7.1 常见错误类型

| 错误代码                                    | 说明                     |
| ------------------------------------------- | ------------------------ |
| `ErrPerpetualDoesNotExist`                  | 永续合约不存在           |
| `ErrPerpetualAlreadyExists`                 | 永续合约已存在           |
| `ErrInvalidMarketType`                      | 无效的市场类型           |
| `ErrTickerEmptyString`                      | Ticker 为空              |
| `ErrDefaultFundingPpmMagnitudeExceedsMax`   | 默认资金费率超出最大值   |
| `ErrInitialMarginPpmExceedsMax`             | 初始保证金率超出最大值   |
| `ErrMaintenanceFractionPpmExceedsMax`       | 维持保证金比例超出最大值 |
| `ErrImpactNotionalIsZero`                   | 影响名义价值为零         |
| `ErrOpenInterestLowerCapLargerThanUpperCap` | 未平仓合约量下限大于上限 |
| `ErrFundingRateClampFactorPpmIsZero`        | 资金费率限制因子为零     |
| `ErrPremiumVoteClampFactorPpmIsZero`        | 溢价投票限制因子为零     |
| `ErrMinNumVotesPerSampleIsZero`             | 最小投票数为零           |
| `ErrInvalidAddPremiumVotes`                 | 无效的溢价投票           |