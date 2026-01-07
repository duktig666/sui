# Perpetuals 模块产品需求文档 (PRD)

## 1. 产品概述

### 1.1 产品定位

Perpetuals (永续合约) 模块是 Hermes DEX 的**永续合约配置中心**,负责定义永续合约的参数、计算资金费率、管理流动性层级,确保永续合约市场的稳定运行。

### 1.2 核心价值

**对交易者**:

- 无到期日的合约,可以长期持有
- 资金费率机制,价格锚定现货
- 多档杠杆选择,灵活风险管理
- 透明的费率计算,无隐藏成本

**对系统**:

- 动态保证金调整 (OIMF),防止大额持仓操纵
- 资金费率平衡多空,维持价格稳定
- 流动性层级管理,风险分级

### 1.3 目标用户

- **普通交易者**: 使用标准杠杆交易永续合约
- **专业交易者**: 利用资金费率套利
- **大户**: 受 OIMF 影响,需要更多保证金
- **做市商**: 提供流动性,赚取价差和资金费率

---

## 2. 功能需求

### FR-1: 永续合约定义

**需求描述**: 系统支持创建和管理多个永续合约,每个合约关联一个交易对和一组配置参数。

#### FR-1.1: 合约基本信息

**功能点**:
- 每个永续合约有唯一的合约 ID
- 关联一个价格市场 (Market)
- 定义合约的交易对名称 (如 BTC-USD)
- 指定合约的精度和最小变动单位

**合约属性**:

| 属性              | 说明                          | 示例                          |
|------------------|------------------------------|------------------------------|
| 合约 ID           | 唯一标识符                    | 1, 2, 3, ...                 |
| Ticker           | 交易对名称                    | BTC-USD, ETH-USD             |
| 市场 ID           | 关联的价格市场                | 1 (BTC 市场)                 |
| 原子精度          | 最小价格/数量单位             | -10 (表示 10^-10)            |
| 流动性层级        | 关联的流动性层级 ID            | 0, 1, 2 (对应不同风险等级)    |
| 市场类型          | 交叉保证金或独立保证金         | CROSS, ISOLATED              |

**验收标准**:
- 可以成功创建新永续合约
- 合约参数正确存储和读取
- 每个合约有唯一 ID

**用户场景**:

场景 1: 创建新的 BTC 永续合约
- 参与者: 系统管理员或治理流程
- 前置条件: BTC 价格市场已创建
- 流程:
  1. 提交创建合约请求
  2. 指定 Ticker = "BTC-USD"
  3. 指定市场 ID = 1 (BTC 市场)
  4. 指定流动性层级 = 0 (大盘币,低风险)
  5. 系统分配合约 ID = 1
  6. 合约创建成功,可以开始交易
- 后置条件: BTC-USD 永续合约可用

#### FR-1.2: 流动性层级配置

**需求描述**: 系统支持多个流动性层级,不同层级有不同的保证金要求和风险参数。

**功能点**:

- 定义多个流动性层级 (例如 Large-Cap, Mid-Cap, Small-Cap)
- 每个层级有独立的保证金参数
- 不同合约可以使用不同的流动性层级

**流动性层级参数**:

| 参数              | 说明                          | 示例 (Large-Cap)  | 示例 (Small-Cap)  |
|------------------|------------------------------|------------------|------------------|
| 层级 ID           | 唯一标识符                    | 0                | 2                |
| 层级名称          | 描述性名称                    | Large-Cap        | Small-Cap        |
| 初始保证金率      | 开仓所需保证金比例 (PPM)       | 50,000 (5%)      | 200,000 (20%)    |
| 维持保证金比例    | 维持保证金占初始保证金比例      | 600,000 (60%)    | 500,000 (50%)    |
| 影响名义价值      | OIMF 计算基准                 | 10,000,000 USDC  | 1,000,000 USDC   |
| 未平仓合约下限    | OIMF 触发下限                 | 10 BTC           | 1 BTC            |
| 未平仓合约上限    | OIMF 最大值触发点             | 100 BTC          | 10 BTC           |

**验收标准**:
- 可以创建和修改流动性层级
- 不同合约使用不同层级的参数
- 参数正确应用于保证金计算

**用户场景**:

场景 2: 查看不同合约的保证金要求
- 参与者: 交易者 Alice
- 前置条件: BTC 使用 Large-Cap,SHIB 使用 Small-Cap
- 流程:
  1. Alice 查询 BTC-USD 合约信息
     - 初始保证金率: 5% (20x 杠杆)
  2. Alice 查询 SHIB-USD 合约信息
     - 初始保证金率: 20% (5x 杠杆)
  3. Alice 发现 SHIB 需要更多保证金,风险更高
- 后置条件: Alice 理解不同合约的风险差异

---

### FR-2: 资金费率机制

**需求描述**: 系统自动计算和结算资金费率,使永续合约价格锚定现货价格。

 ⏰ 时间周期总览

| 机制                        | 时间间隔         | 描述                |
| --------------------------- | ---------------- | ------------------- |
| Premium Sample (溢价采样)   | 60 秒            | 每 1 分钟次价格溢价 |
| Funding Tick (资金费率结算) | 3600 秒 (1 小时) | 每 1 小时次资金费率 |
| 标准化周期                  | 8 小时           | 资金费率按准化显示  |

 - 资金费率以 8 小时为基准 显示
  - 实际结算是 每小时 1/8



#### FR-2.1: 资金费率计算

**功能点**:
- 每 1 分钟采样一次溢价 (Premium)
- 每 1 小时聚合溢价样本,计算资金费率
- 资金费率在多头和空头之间转移

**计算公式**:

**步骤 1: 溢价计算 (每 1 分钟)**

数学表达式:
```
Premium = (IndexPrice - OraclePrice) / OraclePrice
```

变量说明:
- **Premium**: 溢价 (Funding Premium),表示永续合约价格与现货价格的偏差
- **IndexPrice**: 指数价格,基于订单簿中间价计算
  - IndexPrice = (BestBidPrice + BestAskPrice) / 2
  - BestBidPrice: 订单簿最佳买价
  - BestAskPrice: 订单簿最佳卖价
- **OraclePrice**: 预言机价格 (现货价格)

计算示例 (溢价为正):
- 假设 BTC 预言机价格 = 50,000 USDC (现货价格)
- 假设订单簿最佳买价 = 50,100 USDC
- 假设订单簿最佳卖价 = 50,200 USDC
- IndexPrice = (50,100 + 50,200) / 2 = 50,150 USDC
- Premium = (50,150 - 50,000) / 50,000 = 0.003 = 0.3%

计算示例 (溢价为负):
- 假设 BTC 预言机价格 = 50,000 USDC
- 假设订单簿最佳买价 = 49,800 USDC
- 假设订单簿最佳卖价 = 49,900 USDC
- IndexPrice = (49,800 + 49,900) / 2 = 49,850 USDC
- Premium = (49,850 - 50,000) / 50,000 = -0.003 = -0.3%

业务解释:
- 溢价为正: 永续合约价格高于现货,多头需要支付资金费给空头
- 溢价为负: 永续合约价格低于现货,空头需要支付资金费给多头
- 溢价机制促使价格回归现货

**步骤 2: 溢价样本聚合 (每 1 分钟存储)**

数学表达式:
```
PremiumSamples[i] = Median(PremiumVotes[])
```

变量说明:
- **PremiumSamples[i]**: 第 i 个 1 分钟周期的溢价样本
- **PremiumVotes[]**: 所有验证器节点提交的溢价投票
- **Median()**: 中位数函数,选择中位数作为最终样本

计算示例:
- 假设 5 个验证器提交溢价投票: [0.3%, 0.28%, 0.32%, 0.29%, 0.31%]
- 排序: [0.28%, 0.29%, 0.3%, 0.31%, 0.32%]
- 中位数: 0.3%
- PremiumSamples[0] = 0.3%

业务解释:
- 使用中位数而非平均值,防止异常值操纵
- 每个验证器独立计算溢价,去中心化
- 1 分钟采样,快速响应市场变化

**步骤 3: 资金费率计算 (每 1 小时)**

数学表达式:
```
AvgPremium = Mean(PremiumSamples[0..59])
FundingRate = Clamp(
  8 × AvgPremium,
  DefaultFundingPpm - MaxPremiumPpm,
  DefaultFundingPpm + MaxPremiumPpm
)
```

变量说明:
- **AvgPremium**: 过去 1 小时 (60 个样本) 的平均溢价
- **FundingRate**: 资金费率 (PPM,百万分之一)
- **DefaultFundingPpm**: 默认资金费率,通常为 0 (8 小时为 0%)
- **MaxPremiumPpm**: 最大溢价限制 (PPM)
- **8**: 放大系数,因为资金费率通常表示为 8 小时费率
- **Clamp()**: 钳制函数,限制结果在范围内

> 没有Clamp 的资金费率计算：`资金费率 = 溢价（Premium）+ 默认资金费率（Default Funding）`

**Clamp()** 解释：

Clamp 将资金费率限制在合理范围内，避免极端值。

计算示例:

- 假设过去 60 分钟的溢价样本平均值 = 0.05% = 500 PPM
- 假设 DefaultFundingPpm = 0
- 假设 MaxPremiumPpm = 6,000 PPM (0.6%)
- FundingRate = 8 × 500 = 4,000 PPM
- 检查范围: -6,000 <= 4,000 <= 6,000 ✅ 在范围内
- 最终资金费率 = 4,000 PPM = 0.4% (每 8 小时)

计算示例 (超出上限):
- 假设过去 60 分钟的溢价样本平均值 = 1% = 10,000 PPM
- FundingRate = 8 × 10,000 = 80,000 PPM
- 检查范围: 80,000 > 6,000 → 超出上限
- Clamp 到上限: FundingRate = 6,000 PPM = 0.6% (每 8 小时)

业务解释:
- 资金费率与平均溢价成正比
- 溢价越大,资金费率越高,促使价格回归
- 上下限保护,防止极端费率
- 8 倍系数:因为费率通常表示为 8 小时周期,但每 1 小时结算

**验收标准**:
- 溢价计算正确
- 资金费率计算正确
- 费率在合理范围内

**用户场景**:

场景 3: 理解资金费率
- 参与者: 交易者 Bob
- 前置条件: Bob 持有 1 BTC 多头仓位
- 流程:
  1. Bob 查看当前资金费率: 0.01% (每 8 小时)
  2. Bob 计算资金费用:
     - 仓位价值 = 1 BTC × 50,000 USDC = 50,000 USDC
     - 资金费用 = 50,000 × 0.01% = 5 USDC (每 8 小时)
     - 每天费用 = 5 × 3 = 15 USDC
  3. Bob 理解:如果持有多头 1 天,需要支付 15 USDC 给空头
- 后置条件: Bob 了解持仓成本

#### FR-2.2: 资金费率结算

**功能点**:
- 每 1 小时自动结算一次资金费率
- 多头支付给空头 (费率为正时) 或空头支付给多头 (费率为负时)
- 资金费用从账户余额扣除或增加

**计算公式**:

**资金费用计算**:

数学表达式:
```
FundingPayment = PositionSize × IndexPrice × (FundingRate / 1,000,000) × (1小时 / 8小时)
```

变量说明:
- **FundingPayment**: 资金费用 (USDC),正数表示支付,负数表示收取
- **PositionSize**: 持仓数量 (BTC),正数为多头,负数为空头
- **IndexPrice**: 指数价格 (USDC/BTC)
- **FundingRate**: 资金费率 (PPM,8 小时费率)
- **(1小时 / 8小时)**: 因为结算周期是 1 小时,但费率是 8 小时

计算示例 (多头支付):
- 假设用户持有 2 BTC 多头 (PositionSize = +2)
- 假设 IndexPrice = 50,000 USDC
- 假设 FundingRate = 4,000 PPM (0.4% 每 8 小时)
- FundingPayment = 2 × 50,000 × (4,000 / 1,000,000) × (1/8)
               = 2 × 50,000 × 0.004 × 0.125
               = 50 USDC
- 用户账户扣除 50 USDC

计算示例 (空头收取):
- 假设用户持有 2 BTC 空头 (PositionSize = -2)
- 假设 IndexPrice = 50,000 USDC
- 假设 FundingRate = 4,000 PPM (0.4% 每 8 小时)
- FundingPayment = -2 × 50,000 × (4,000 / 1,000,000) × (1/8)
               = -50 USDC
- 用户账户增加 50 USDC (收取资金费)

业务解释:
- 多头支付,空头收取 (费率为正时)
- 资金费在多空之间转移,系统不赚取
- 每小时结算,持续平衡多空

**业务规则**:
- 每个区块检查是否到达 Funding Tick Epoch (1 小时)
- 结算时遍历所有持仓账户,计算资金费用
- 资金费用直接从账户余额扣除或增加

**验收标准**:
- 资金费用计算正确
- 多空资金费总和为 0 (忽略舍入误差)
- 结算及时执行

**用户场景**:

场景 4: 资金费率结算
- 参与者: 多头 Alice (1 BTC),空头 Bob (1 BTC)
- 前置条件: 资金费率 = 0.4% (每 8 小时)
- 流程:
  1. 1 小时 Epoch 到达,系统开始结算
  2. 计算 Alice 的资金费用:
     - FundingPayment = 1 × 50,000 × 0.004 × (1/8) = 25 USDC
     - Alice 账户扣除 25 USDC
  3. 计算 Bob 的资金费用:
     - FundingPayment = -1 × 50,000 × 0.004 × (1/8) = -25 USDC
     - Bob 账户增加 25 USDC
  4. 系统验证: Alice 支付 25 = Bob 收取 25 ✅
- 后置条件: Alice 余额减少 25,Bob 余额增加 25

---

#### FR-2.3: 资金费率 Clamp 机制 (上下限保护)

**需求描述**: 资金费率和溢价投票都受流动性层级参数限制,防止极端费率损害用户。

**功能点**:
- 资金费率受动态上限约束
- 上限基于流动性层级的保证金参数计算
- 不同市场有不同的资金费率上限

**Clamp 公式**:

数学表达式:
```
# 计算资金费率上限 = Clamp系数 × (初始保证金率 - 维持保证金率)
MaxAbsFundingRatePpm = FundingRateClampFactorPpm × (InitialMarginPpm - MaintenanceMarginPpm)

# 应用 Clamp
FundingRate = Clamp(
  8 × AvgPremium + DefaultFundingPpm,
  -MaxAbsFundingRatePpm,
  +MaxAbsFundingRatePpm
)
```

变量说明:
- **MaxAbsFundingRatePpm**: 资金费率的绝对值上限 (PPM)
- **FundingRateClampFactorPpm**: Clamp 系数,默认 6,000,000 (600%)
- **InitialMarginPpm**: 流动性层级的初始保证金率
- **MaintenanceMarginPpm**: 维持保证金率 = InitialMarginPpm × MaintenanceFractionPpm
- **DefaultFundingPpm**: 默认资金费率,通常为 0
- **Clamp()**: 限制函数,将值限制在 [-Max, +Max] 范围内

**溢价投票 Clamp**:

数学表达式:
```
MaxAbsPremiumVotePpm = PremiumVoteClampFactorPpm × (InitialMarginPpm - MaintenanceMarginPpm)

PremiumVote = Clamp(
  RawPremium,
  -MaxAbsPremiumVotePpm,
  +MaxAbsPremiumVotePpm
)
```

变量说明:
- **PremiumVoteClampFactorPpm**: 溢价投票 Clamp 系数,默认 60,000,000 (6000%)
- **RawPremium**: 原始溢价计算值
- 溢价投票上限远高于资金费率上限 (60x vs 6x)

**系统参数** (默认值):

| 参数 | 默认值 | 说明 |
|------|-------|------|
| FundingRateClampFactorPpm | 6,000,000 (600%) | 资金费率 Clamp 系数 |
| PremiumVoteClampFactorPpm | 60,000,000 (6000%) | 溢价投票 Clamp 系数 |
| MinNumVotesPerSample | 15 | 最小溢价投票数 |

**计算示例 1: Large-Cap 市场** (低保证金,高杠杆):
- 流动性层级: Large-Cap
- InitialMarginPpm = 50,000 (5%)
- MaintenanceFractionPpm = 600,000 (60%)
- MaintenanceMarginPpm = 50,000 × 60% = 30,000 (3%)
- 计算资金费率上限:
  - MaxAbsFundingRatePpm = 6,000,000 × (50,000 - 30,000) / 1,000,000
  - MaxAbsFundingRatePpm = 6 × 0.02 = 0.12 = **12%** (每 8 小时)
- 计算溢价投票上限:
  - MaxAbsPremiumVotePpm = 60,000,000 × (50,000 - 30,000) / 1,000,000
  - MaxAbsPremiumVotePpm = 60 × 0.02 = 1.2 = **120%** (每分钟)

**计算示例 2: Small-Cap 市场** (高保证金,低杠杆):
- 流动性层级: Small-Cap
- InitialMarginPpm = 200,000 (20%)
- MaintenanceFractionPpm = 500,000 (50%)
- MaintenanceMarginPpm = 200,000 × 50% = 100,000 (10%)
- 计算资金费率上限:
  - MaxAbsFundingRatePpm = 6,000,000 × (200,000 - 100,000) / 1,000,000
  - MaxAbsFundingRatePpm = 6 × 0.10 = 0.60 = **60%** (每 8 小时)
- 计算溢价投票上限:
  - MaxAbsPremiumVotePpm = 60,000,000 × (200,000 - 100,000) / 1,000,000
  - MaxAbsPremiumVotePpm = 60 × 0.10 = 6.0 = **600%** (每分钟)

**业务解释**:
- **动态上限**: 资金费率上限不是固定值,基于流动性层级动态计算
- **低保证金市场 (Large-Cap)**: 上限较低 (12%),因为高杠杆市场波动风险小
- **高保证金市场 (Small-Cap)**: 上限较高 (60%),因为低杠杆市场需要更大资金费率平衡多空
- **溢价投票宽松**: 溢价投票上限是资金费率的 10 倍,允许更大波动捕捉市场信号
- **防极端费率**: Clamp 机制防止异常市场条件下资金费率失控

**验收标准**:
- 资金费率不超过动态计算的上限
- 不同流动性层级有不同的上限
- 溢价投票正确 Clamp
- 极端市场条件下费率受控

**用户场景**:

场景: 极端市场条件下的费率保护
- 参与者: 多头交易者 Charlie
- 前置条件:
  - BTC 市场 (Large-Cap,12% 资金费率上限)
  - 极端行情,原始溢价 = 5% (非常高)
- 流程:
  1. 系统计算原始资金费率:
     - RawFundingRate = 8 × 5% = 40% (每 8 小时)
  2. 系统应用 Clamp:
     - MaxAbsFundingRatePpm = 12%
     - FundingRate = Clamp(40%, -12%, +12%) = **12%** (被限制)
  3. Charlie 的资金费用:
     - 仓位价值 = 50,000 USDC
     - 每 8 小时支付 = 50,000 × 12% = 6,000 USDC
     - 而非 50,000 × 40% = 20,000 USDC
  4. Clamp 保护了 Charlie,避免极端费率
- 后置条件: 资金费率受控,用户损失有限

**业务价值**:
- **用户保护**: 防止极端费率导致大额损失
- **市场稳定**: 避免资金费率失控引发恐慌
- **风险管理**: 基于保证金参数动态调整上限,匹配市场风险

---

### FR-3: 动态保证金调整 (OIMF)

**需求描述**: 系统根据未平仓合约量 (Open Interest) 动态调整保证金要求,防止大额持仓操纵市场。

#### FR-3.1: OIMF 计算

**功能点**:
- 监控每个永续合约的总未平仓合约量
- 当未平仓合约量超过阈值时,提高保证金要求
- 线性插值计算保证金调整系数

**计算公式**:

**OIMF 系数计算** (完整版):

数学表达式:
```
# 特殊情况 1: UpperCap = 0,OIMF 机制完全禁用
if OpenInterestUpperCap == 0:
    OI_Adjusted_IMF = Base_IMF
    (不缩放,用于不需要 OIMF 的市场,如稳定币)

# 情况 2: OI <= LowerCap,不触发 OIMF
elif OI <= OpenInterestLowerCap:
    OI_Adjusted_IMF = Base_IMF
    (无调整)

# 情况 3: OI >= UpperCap,OIMF 最大缩放
elif OI >= OpenInterestUpperCap:
    OI_Adjusted_IMF = 1,000,000 ppm (100% 保证金,相当于 1:1 抵押)

# 情况 4: LowerCap < OI < UpperCap,线性插值
else:
    ScalingFactor = (OI - LowerCap) / (UpperCap - LowerCap)
    IMF_Increase = ScalingFactor × (1,000,000 - Base_IMF)
    OI_Adjusted_IMF = Base_IMF + IMF_Increase
```

变量说明:
- **OI_Adjusted_IMF**: OI 调整后的初始保证金率 (PPM)
- **Base_IMF**: 流动性层级的基础初始保证金率 (PPM)
- **OI**: Open Interest,当前未平仓合约总量 (QuoteQuantums,USDC 计价)
- **OpenInterestLowerCap**: OI 下限,低于此值无调整 (QuoteQuantums)
- **OpenInterestUpperCap**: OI 上限,高于此值 IMF = 100% (QuoteQuantums)
- **ScalingFactor**: 缩放因子 (0 ~ 1)
- **IMF_Increase**: IMF 增量 (从 Base_IMF 到 100% 的增量)

**注意**: OI 以 QuoteQuantums (USDC) 计价,而非 BaseQuantums (BTC)

计算示例 (无调整):
- 假设 LowerCap = 1,000 BTC
- 假设 UpperCap = 10,000 BTC
- 假设当前 OI = 500 BTC
- 500 <= 1,000 → OIMF = 0%

计算示例 (线性插值):
- 假设 LowerCap = 1,000 BTC
- 假设 UpperCap = 10,000 BTC
- 假设当前 OI = 5,500 BTC
- 1,000 < 5,500 < 10,000 → 线性插值
- OIMF = (5,500 - 1,000) / (10,000 - 1,000) × 100%
       = 4,500 / 9,000 × 100%
       = 50%

计算示例 (最大调整):
- 假设 LowerCap = 1,000 BTC
- 假设 UpperCap = 10,000 BTC
- 假设当前 OI = 15,000 BTC
- 15,000 >= 10,000 → OIMF = 100%

**调整后保证金率计算**:

数学表达式:
```
AdjustedIMR = BaseIMR + OIMF × (BaseIMR - MaintenanceMarginRate)
```

变量说明:
- **AdjustedIMR**: 调整后初始保证金率
- **BaseIMR**: 基础初始保证金率 (从流动性层级获取,例如 5%)
- **MaintenanceMarginRate**: 维持保证金率 (例如 3%)
- **OIMF**: OIMF 系数 (0% - 100%)

计算示例 (OIMF = 0%):
- 假设 BaseIMR = 10% (10x 杠杆)
- 假设 MaintenanceMarginRate = 5%
- 假设 OIMF = 0%
- AdjustedIMR = 10% + 0% × (10% - 5%) = 10%
- 杠杆: 10x (无调整)

计算示例 (OIMF = 50%):
- 假设 BaseIMR = 10%
- 假设 MaintenanceMarginRate = 5%
- 假设 OIMF = 50%
- AdjustedIMR = 10% + 50% × (10% - 5%)
             = 10% + 50% × 5%
             = 10% + 2.5%
             = 12.5%
- 杠杆: 1 / 0.125 = 8x (杠杆降低)

计算示例 (OIMF = 100%):
- 假设 BaseIMR = 10%
- 假设 MaintenanceMarginRate = 5%
- 假设 OIMF = 100%
- AdjustedIMR = 10% + 100% × (10% - 5%)
             = 10% + 5%
             = 15%
- 杠杆: 1 / 0.15 = 6.67x (最大调整)

业务解释:
- 未平仓合约量越高,保证金要求越高
- OIMF 增加保证金,降低杠杆,减少风险
- 防止大户用高杠杆操纵市场
- 小额持仓不受影响 (OIMF = 0%)

**验收标准**:
- OIMF 计算正确
- 保证金要求随 OI 动态调整
- 新订单验证保证金时使用调整后的 IMR

**用户场景**:

场景 5: 大户受 OIMF 影响
- 参与者: 大户 Whale
- 前置条件: BTC 未平仓合约量 = 8,000 BTC (接近上限 10,000)
- 流程:
  1. Whale 想开 100 BTC 多头仓位
  2. 系统计算 OIMF:
     - OI = 8,000 BTC
     - OIMF = (8,000 - 1,000) / (10,000 - 1,000) = 77.78%
  3. 系统计算调整后保证金率:
     - AdjustedIMR = 10% + 77.78% × 5% = 13.89%
  4. 保证金要求:
     - PositionValue = 100 × 50,000 = 5,000,000 USDC
     - MarginRequired = 5,000,000 × 13.89% = 694,500 USDC
  5. Whale 需要 694,500 USDC 保证金,而不是标准的 500,000 USDC
- 后置条件: OIMF 增加了 Whale 的保证金负担

---

#### FR-3.2: 自定义初始保证金率 (Custom IMF)

**需求描述**: 系统允许为特定仓位设置自定义初始保证金率,覆盖流动性层级的默认值。

**功能点**:

- 支持为特定账户或仓位设置 Custom IMF
- Custom IMF 只能提高保证金要求,不能降低
- 与 OI 缩放机制协同工作

**计算逻辑**:

数学表达式:
```
# 第 1 步: 获取基于 OI 调整后的 IMF
OI_Adjusted_IMF = GetAdjustedInitialMarginPpm(OI)

# 第 2 步: 如果有自定义 IMF,取两者的最大值
if Custom_IMF > 0:
    Effective_IMF = max(OI_Adjusted_IMF, Custom_IMF)
else:
    Effective_IMF = OI_Adjusted_IMF

# 第 3 步: 计算保证金要求
Margin_Requirement = Position_Value × (Effective_IMF / 1,000,000)
```

变量说明:
- **Custom_IMF**: 用户/系统为特定仓位设置的自定义初始保证金率 (PPM)
- **Effective_IMF**: 最终有效的初始保证金率 (PPM)
- **OI_Adjusted_IMF**: 基于 OI 缩放后的保证金率 (PPM)

**业务规则**:
- Custom IMF 必须 >= Base IMF (不能低于流动性层级的最低要求)
- Custom IMF 与 OI-Adjusted IMF 取最大值,确保风险控制
- Custom IMF = 0 表示未设置,使用默认值

**使用场景**:

**场景 1: 风险控制 - 对高风险账户提高保证金**

- 系统检测到某账户频繁清算
- 系统为该账户设置 Custom IMF = 200,000 ppm (20%)
- 该账户开仓时,即使 Base IMF = 5%,也需要 20% 保证金

**场景 2: VIP 账户 - 特殊保证金规则**

- VIP 账户申请更高的保证金率以换取其他优惠
- 系统为 VIP 账户设置 Custom IMF = 150,000 ppm (15%)
- 该账户始终使用 15% 保证金,不受 OI 缩放影响 (如果 OI-Adjusted < 15%)

**计算示例**:

示例 1: Custom IMF 高于 OI-Adjusted IMF
- Base IMF = 50,000 ppm (5%)
- OI-Adjusted IMF = 100,000 ppm (10%,因为 OI 中等)
- Custom IMF = 200,000 ppm (20%,账户设置)
- Effective IMF = max(100,000, 200,000) = **200,000 ppm (20%)**
- 结果: 使用 Custom IMF

示例 2: OI-Adjusted IMF 高于 Custom IMF
- Base IMF = 50,000 ppm (5%)
- OI-Adjusted IMF = 800,000 ppm (80%,因为 OI 极高)
- Custom IMF = 200,000 ppm (20%,账户设置)
- Effective IMF = max(800,000, 200,000) = **800,000 ppm (80%)**
- 结果: 使用 OI-Adjusted IMF (系统风险优先)

**验收标准**:
- Custom IMF 正确应用于保证金计算
- Custom IMF 与 OI-Adjusted IMF 取最大值
- Custom IMF = 0 时使用默认值

**业务价值**:
- **风险管理**: 对高风险账户提高保证金要求
- **灵活性**: 支持特殊账户的自定义保证金规则
- **安全优先**: 始终选择最保守的保证金要求

---

#### FR-3.3: OIMF 机制禁用条件

**需求描述**: 某些市场不需要 OI 缩放机制,系统支持禁用 OIMF。

**功能点**:
- 通过设置 `OpenInterestUpperCap = 0` 禁用 OIMF
- 禁用后,始终使用 Base IMF
- 适用于不需要 OI 缩放的市场 (如稳定币市场)

**业务规则**:
```
if OpenInterestUpperCap == 0:
    OIMF 机制完全禁用
    Effective_IMF = Base_IMF (不受 OI 影响)
```

**使用场景**:

**场景: 稳定币永续合约**
- 合约: USDC/USDT 永续合约
- 特点: 价格波动极小,无需 OI 缩放
- 配置: OpenInterestUpperCap = 0
- 结果: 无论 OI 多大,始终使用 Base IMF

**验收标准**:
- UpperCap = 0 时 OIMF 禁用
- Effective IMF = Base IMF
- 保证金计算不受 OI 影响

---

### FR-4: 未平仓合约量追踪

**需求描述**: 系统实时追踪每个永续合约的总未平仓合约量,用于 OIMF 计算和风险监控。

#### FR-4.1: 未平仓合约量更新

**功能点**:
- 订单成交时更新未平仓合约量
- 开仓时增加,平仓时减少
- 多空抵消,只计算净持仓

**计算公式**:

**未平仓合约量变化**:

数学表达式:
```
ΔOI = |NewNetPosition| - |OldNetPosition|
NewOI = OldOI + ΔOI
```

变量说明:
- **ΔOI**: 未平仓合约量变化 (BTC)
- **NewNetPosition**: 成交后的净持仓 (多头为正,空头为负)
- **OldNetPosition**: 成交前的净持仓
- **NewOI**: 更新后的未平仓合约总量
- **OldOI**: 更新前的未平仓合约总量

计算示例 (开多仓):
- 假设用户成交前净持仓 = 0 BTC
- 假设用户成交 1 BTC 多头
- NewNetPosition = +1 BTC
- ΔOI = |1| - |0| = 1 BTC
- NewOI = OldOI + 1

计算示例 (平多仓):
- 假设用户成交前净持仓 = +2 BTC (多头)
- 假设用户卖出 1 BTC (平仓)
- NewNetPosition = +1 BTC
- ΔOI = |1| - |2| = -1 BTC
- NewOI = OldOI - 1

计算示例 (多空转换):
- 假设用户成交前净持仓 = +1 BTC (多头)
- 假设用户卖出 3 BTC (平仓 + 开空)
- NewNetPosition = -2 BTC (空头)
- ΔOI = |-2| - |1| = 1 BTC
- NewOI = OldOI + 1

业务解释:
- 未平仓合约量只计算净持仓,多空抵消
- 开仓增加 OI,平仓减少 OI
- OI 反映市场总风险敞口

**业务规则**:
- 每笔成交后立即更新 OI
- OI 不能为负数
- OI 用于 OIMF 计算和风险监控

**验收标准**:
- OI 更新及时准确
- 开平仓正确影响 OI
- OI 用于保证金计算

---

### FR-5: 保证金计算机制

**需求描述**: 系统计算两种保证金率:初始保证金率 (IMR) 和维持保证金率 (MMR),用于不同场景。

#### FR-5.1: 初始保证金率 (IMR) 计算

**功能点**:
- IMR 用于开仓时的保证金验证
- IMR 受 OI 缩放影响
- IMR 受 Custom IMF 影响

**完整计算步骤**:

数学表达式:
```
# 第 1 步: 获取基础 IMR (从流动性层级)
Base_IMF = LiquidityTier.InitialMarginPpm

# 第 2 步: 根据 OI 缩放 IMR
if UpperCap == 0:
    OI_Adjusted_IMF = Base_IMF
elif OI <= LowerCap:
    OI_Adjusted_IMF = Base_IMF
elif OI >= UpperCap:
    OI_Adjusted_IMF = 1,000,000
else:
    OI_Adjusted_IMF = Base_IMF + ((OI - LowerCap) / (UpperCap - LowerCap)) × (1,000,000 - Base_IMF)

# 第 3 步: 如果有 Custom IMF,取最大值
if Custom_IMF > 0:
    Effective_IMF = max(OI_Adjusted_IMF, Custom_IMF)
else:
    Effective_IMF = OI_Adjusted_IMF

# 第 4 步: 计算初始保证金要求
Initial_Margin = Position_Value × (Effective_IMF / 1,000,000)
```

**计算示例**:

- Base IMF = 50,000 ppm (5%)
- OI = 550,000,000 USDC (LowerCap = 100M, UpperCap = 1000M)
- OI_Adjusted_IMF = 50,000 + 0.5 × 950,000 = 525,000 ppm (52.5%)
- Custom IMF = 0 (未设置)
- Effective IMF = 525,000 ppm (52.5%)
- Position Value = 100,000 USDC
- Initial Margin = 100,000 × 52.5% = **52,500 USDC**

---

#### FR-5.2: 维持保证金率 (MMR) 计算

**功能点**:
- MMR 用于清算判断
- MMR **不受 OI 缩放影响**,始终基于 Base IMR
- MMR < IMR (维持保证金 < 初始保证金)

**关键区别**: MMR 不受 OIMF 影响

**计算公式**:

数学表达式:
```
# 第 1 步: 计算基础初始保证金 (不考虑 OI 缩放)
Base_Initial_Margin = Position_Value × (Base_IMF / 1,000,000)

# 第 2 步: 计算维持保证金
Maintenance_Margin = Base_Initial_Margin × (MaintenanceFractionPpm / 1,000,000)

# 简化公式
Maintenance_Margin_Ppm = Base_IMF × MaintenanceFractionPpm / 1,000,000
```

变量说明:
- **Base_IMF**: 流动性层级的基础初始保证金率 (PPM)
- **MaintenanceFractionPpm**: 维持保证金占初始保证金的比例 (PPM)
- **Maintenance_Margin_Ppm**: 维持保证金率 (PPM)

**计算示例**:
- Base IMF = 100,000 ppm (10%)
- OI-Scaled IMF = 500,000 ppm (50%,因为 OI 高)
- MaintenanceFractionPpm = 600,000 (60%)
- 计算:
  - **IMR** = 500,000 ppm (50%,受 OI 影响)
  - **MMR** = 100,000 × 600,000 / 1,000,000 = **60,000 ppm (6%,不受 OI 影响)**
- 结果: IMR = 50%, MMR = 6%

**为什么 MMR 不受 OI 影响?**

**原因**:
1. **开仓门槛 vs 清算门槛**: IMR 是开仓门槛,需要随市场风险动态调整;MMR 是清算门槛,应保持稳定
2. **防止级联清算**: 如果 MMR 也受 OI 影响,OI 上升时会触发大量清算,进一步推高 OI,形成恶性循环
3. **公平性**: 用户开仓时的 MMR 应保持不变,不应因后续市场 OI 变化而改变

**计算对比示例**:

场景: 用户开仓时 OI 低,持仓期间 OI 上升

| 时刻 | OI 状态 | Base IMF | OI-Scaled IMF | Effective IMF | MMR |
|------|---------|----------|--------------|--------------|-----|
| T1 (开仓) | 低 OI | 10% | 10% | 10% | 6% |
| T2 (持仓) | 高 OI | 10% | 50% | 50% | **6%** (不变) |

**说明**:
- 在 T1 时刻,用户用 10% 保证金开仓,MMR = 6%
- 在 T2 时刻,市场 OI 上升,新开仓需要 50% 保证金,但用户的 MMR 仍是 6%
- 这确保了用户不会因市场 OI 变化而被意外清算

**验收标准**:
- IMR 正确受 OI 缩放影响
- MMR 不受 OI 缩放影响
- MMR 始终基于 Base IMR 计算
- MMR < IMR

**用户场景**:

场景: OI 上升不触发清算
- 参与者: 交易者 David
- 前置条件:
  - David 在 OI 低时开仓,IMR = 10%, MMR = 6%
  - David 保证金 = 8% (高于 MMR 6%,但低于当前 IMR 10%)
- 流程:
  1. 市场 OI 大幅上升,新订单 IMR = 50%
  2. 系统计算 David 的清算阈值:
     - MMR = 6% (不变,基于开仓时的 Base IMR)
     - David 保证金 = 8% > 6% ✅ 安全
  3. David 不被清算,可以继续持仓
  4. 如果 MMR 也受 OI 影响升至 30%,David 会被清算 ❌
- 后置条件: David 持仓安全,MMR 稳定性保护了用户

**业务价值**:
- **稳定性**: MMR 不受市场 OI 波动影响,用户清算阈值稳定
- **公平性**: 用户不会因后续市场变化被意外清算
- **防级联**: 避免 OI 上升触发清算,进一步推高 OI 的恶性循环

---

## 3. 非功能需求

### NFR-1: 性能要求

**资金费率计算**:
- 溢价采样延迟 < 1 秒 (每分钟一次)
- 资金费率计算延迟 < 5 秒 (每小时一次)
- 资金费率结算延迟 < 区块时间

**OIMF 计算**:
- OI 更新延迟 < 订单成交延迟
- OIMF 计算延迟 < 100ms
- 保证金验证延迟 < 200ms

**验收标准**:
- 性能测试达到目标
- 结算不影响订单匹配

### NFR-2: 准确性要求

**资金费率精度**:
- 溢价计算精度 >= 6 位小数
- 资金费率计算精度 >= 6 位小数
- 资金费用计算精度 >= 2 位小数 (USDC 分)

**OIMF 精度**:
- OIMF 系数精度 >= 4 位小数
- 保证金率精度 >= 6 位小数

**验收标准**:
- 精度测试通过
- 舍入误差可接受 (< 1 USDC)

### NFR-3: 安全性要求

**防操纵**:
- 溢价使用中位数,防止单个节点操纵
- OIMF 限制大额持仓,防止市场操纵
- 资金费率上下限,防止极端费率

**风险控制**:
- 及时更新 OI,准确计算风险敞口
- 动态调整保证金,降低系统风险

**验收标准**:
- 操纵攻击测试无法成功
- 风险监控准确

---

## 4. 用户场景

### 场景 6: 资金费率套利

**参与者**: 套利者 Arbitrager

**前置条件**:
- BTC 永续合约资金费率 = 0.1% (每 8 小时)
- 现货市场可借贷 BTC

**流程**:
1. Arbitrager 发现资金费率机会 (多头支付 0.1%)
2. Arbitrager 开 1 BTC 空头永续合约
3. Arbitrager 在现货市场买入 1 BTC (对冲)
4. 持有 1 天,收取资金费:
   - 每 8 小时收取 50,000 × 0.1% = 50 USDC
   - 每天收取 50 × 3 = 150 USDC
5. 资金费率回归后,Arbitrager 平仓
6. 套利收益 = 资金费收入 - 借贷成本 - 手续费

**后置条件**: 资金费率套利,促使费率回归

**业务价值**:
- 套利者获得收益
- 资金费率保持合理水平
- 永续合约价格锚定现货

### 场景 7: OIMF 保护机制

**参与者**: 大户 Whale,普通用户 Alice

**前置条件**:
- BTC 未平仓合约量 = 500 BTC (低于下限 1,000)

**流程**:
1. Whale 尝试开 10,000 BTC 多头 (远超市场容量)
2. 系统计算 OIMF:
   - 如果 Whale 成交,OI = 500 + 10,000 = 10,500 BTC
   - OIMF = 100% (超过上限)
3. 系统计算 Whale 保证金要求:
   - AdjustedIMR = 10% + 100% × 5% = 15%
   - MarginRequired = 10,000 × 50,000 × 15% = 75,000,000 USDC
4. Whale 保证金不足,订单被拒绝
5. Whale 只能分批开仓,或准备更多保证金
6. Alice 的小额订单不受影响 (OIMF = 0%)

**后置条件**: OIMF 限制了 Whale 的大额持仓,保护市场

**业务价值**:
- 防止大户操纵市场
- 保护普通用户利益
- 降低系统风险

---

## 5. 业务指标

### 5.1 关键指标 (KPI)

**资金费率指标**:
- 平均资金费率 (每天)
- 资金费率波动范围
- 资金费率异常次数

**未平仓合约量指标**:
- 总未平仓合约量 (每个合约)
- OI 增长率
- OI 占比 (不同合约)

**OIMF 触发指标**:
- OIMF > 0% 的时间占比
- OIMF = 100% 的时间占比
- 受 OIMF 影响的订单占比

### 5.2 监控指标

**资金费率健康度**:
- 溢价异常检测
- 资金费率趋势
- 多空资金费平衡

**市场风险**:
- 未平仓合约量预警
- 大额持仓监控
- 保证金覆盖率

---

## 6. 术语表

| 术语              | 英文                          | 定义                                                    |
|------------------|------------------------------|--------------------------------------------------------|
| 永续合约          | Perpetual Contract           | 无到期日的期货合约                                       |
| 资金费率          | Funding Rate                 | 多空之间转移的费用,使合约价格锚定现货                    |
| 溢价              | Premium                      | 永续合约价格与现货价格的偏差                             |
| 未平仓合约量      | Open Interest (OI)           | 市场上所有未平仓合约的总量                               |
| OIMF             | Open Interest-adjusted IMF   | 基于未平仓合约量调整的初始保证金比例                     |
| 流动性层级        | Liquidity Tier               | 风险等级分类,不同层级有不同保证金要求                    |
| 指数价格          | Index Price                  | 基于订单簿计算的合约价格                                 |
| 预言机价格        | Oracle Price                 | 外部价格源提供的现货价格                                 |
| 初始保证金率      | Initial Margin Rate (IMR)    | 开仓所需保证金占仓位价值的比例                           |
| 维持保证金率      | Maintenance Margin Rate (MMR) | 维持仓位所需保证金占仓位价值的比例                       |
| Base IMF          | Base Initial Margin Factor   | 流动性层级定义的基础初始保证金率                         |
| Custom IMF        | Custom Initial Margin Factor | 为特定账户/仓位设置的自定义初始保证金率                  |
| Effective IMF     | Effective Initial Margin Factor | 最终有效的初始保证金率 (多因素取最大值)               |
| Clamp            | Clamp / 钳制                  | 限制函数,将值限制在指定范围内                            |
| PPM              | Parts Per Million            | 百万分之一,精度单位 (1 ppm = 0.0001%)                   |
| QuoteQuantums    | Quote Quantums               | USDC 计价的数量单位                                     |
| BaseQuantums     | Base Quantums                | BTC 等基础资产的数量单位                                 |
| Funding Sample   | Funding Sample               | 1 分钟溢价样本,多个样本聚合为资金费率                    |
| Funding Tick     | Funding Tick                 | 1 小时资金费率结算周期                                   |
| 中位数聚合        | Median Aggregation           | 溢价投票聚合方式,防止异常值操纵                          |
| 平均值聚合        | Average Aggregation          | 溢价样本聚合方式,计算资金费率                            |

---

## 7. 参考资料

### 架构文档
- [Perpetuals 模块架构设计](../../architecture/perpetuals.md)

### 数据结构文档
- [Perpetuals 模块数据结构](../data_structure/perpetuals.md)

### 技术分析文档
- [Perpetuals 模块技术分析](../business/perpetuals_analyst.md)

---

**文档版本**: v2.0
**最后更新**: 2025-12-31
**文档作者**: Claude Sonnet 4.5
**文档状态**: ✅ 完成 (已补充资金费率 Clamp、OIMF 完整机制、保证金计算详解)

**更新日志**:

**v2.0 (2025-12-31)**:
- ✅ 新增 FR-2.3: 资金费率 Clamp 机制 (动态上限保护)
- ✅ 更新 FR-3.1: OIMF 完整公式 (包含 UpperCap=0 等边界条件)
- ✅ 新增 FR-3.2: 自定义初始保证金率 (Custom IMF)
- ✅ 新增 FR-3.3: OIMF 机制禁用条件
- ✅ 新增 FR-5: 保证金计算机制 (IMR vs MMR 详解)
- ✅ 扩展术语表 (新增 11 个术语)

**v1.0 (2025-12-31)**:
- 初始版本,包含基础功能 (FR-1 到 FR-4)
