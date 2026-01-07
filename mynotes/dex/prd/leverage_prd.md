# Leverage 模块产品需求文档 (PRD)

## 1. 产品概述

### 1.1 产品定位

Leverage (杠杆) 功能是 Hermes DEX 的**杠杆倍数管理系统**,允许用户自定义交易杠杆,灵活控制风险和收益。

**注意**: Leverage 不是独立模块,而是集成在 CLOB 模块中的功能,数据存储在 CLOB,逻辑分散在 CLOB 和 Subaccounts 模块。

### 1.2 核心价值

**对交易者**:
- 自由选择杠杆倍数 (如 5x, 10x, 20x)
- 高杠杆放大收益,低杠杆降低风险
- 灵活调整,适应不同市场环境
- 透明的保证金要求

**对系统**:
- 风险可控,保证金要求明确
- 防止过度杠杆,保护系统安全
- 用户自主选择,降低监管风险

### 1.3 目标用户

- **保守交易者**: 使用低杠杆 (2x-5x),降低风险
- **专业交易者**: 使用中等杠杆 (10x-20x),平衡收益和风险
- **高风险交易者**: 使用高杠杆 (50x-100x),追求高收益

---

## 2. 功能需求

### FR-1: 杠杆倍数设置

**需求描述**: 用户可以为子账户设置杠杆倍数,影响保证金要求。

#### FR-1.1: 杠杆设置流程

**功能点**:
- 用户提交杠杆调整请求
- 指定子账户和新杠杆倍数
- 系统验证杠杆合法性和保证金充足性
- 设置成功后立即生效

**杠杆范围**:

| 杠杆范围      | 适用场景              | 风险等级  |
|-------------|---------------------|----------|
| 1x - 5x     | 保守交易,降低风险     | 低       |
| 5x - 20x    | 标准交易,平衡收益风险 | 中       |
| 20x - 50x   | 激进交易,追求高收益   | 高       |
| 50x - 100x  | 超高风险交易         | 极高     |

**计算公式**:

**保证金要求计算**:

数学表达式:
```
InitialMarginRate (IMR) = 1 / Leverage
MarginRequirement = PositionValue × IMR
```

变量说明:
- **InitialMarginRate (IMR)**: 初始保证金率 (小数形式)
- **Leverage**: 杠杆倍数 (例如 20 表示 20x)
- **MarginRequirement**: 保证金要求 (USDC)
- **PositionValue**: 仓位价值 (USDC)
  - PositionValue = |PositionSize| × OraclePrice
  - PositionSize: 持仓数量 (BTC)
  - OraclePrice: 预言机价格 (USDC/BTC)

计算示例 (20x 杠杆):
- 假设 Leverage = 20x
- IMR = 1 / 20 = 0.05 = 5%
- 假设仓位价值 = 100,000 USDC
- MarginRequirement = 100,000 × 5% = 5,000 USDC

计算示例 (5x 杠杆):
- 假设 Leverage = 5x
- IMR = 1 / 5 = 0.2 = 20%
- 假设仓位价值 = 100,000 USDC
- MarginRequirement = 100,000 × 20% = 20,000 USDC

业务解释:
- 杠杆倍数越高,保证金要求越低
- 20x 杠杆只需 5% 保证金,可以用 5,000 USDC 控制 100,000 USDC 仓位
- 5x 杠杆需要 20% 保证金,更安全但收益有限
- 杠杆倍数与保证金率互为倒数

**业务规则**:
- 杠杆倍数必须 > 0
- 杠杆倍数不能超过最大限制 (通过流动性层级配置,例如 100x)
- 调整杠杆时必须验证账户保证金充足性
- 杠杆设置立即生效,影响新订单和现有仓位

**验收标准**:
- 用户可以成功设置杠杆
- 杠杆验证正确 (范围检查)
- 保证金充足性验证正确
- 杠杆设置立即生效

**用户场景**:

场景 1: 设置标准杠杆
- 参与者: 交易者 Alice
- 前置条件:
  - Alice 有新子账户,默认杠杆 20x
  - Alice 希望使用更保守的 10x 杠杆
- 流程:
  1. Alice 提交杠杆调整请求:
     - SubaccountId: Alice 的子账户
     - NewLeverage: 10x
  2. 系统验证:
     - 10x 在允许范围内 (1x - 100x) ✅
     - Alice 账户无持仓,无需验证保证金 ✅
  3. 系统更新杠杆: Alice 子账户杠杆 = 10x
  4. Alice 后续交易使用 10x 杠杆:
     - 初始保证金率 = 10%
     - 持仓 10,000 USDC 需要 1,000 USDC 保证金
- 后置条件: Alice 杠杆设置为 10x

---

#### FR-1.2: IMF (初始保证金率) 精度与表示

**需求描述**: 系统内部使用 IMF (Initial Margin Factor) 以 PPM (Parts Per Million) 表示杠杆,保证高精度计算。

**技术实现**:
- IMF 使用 PPM (Parts Per Million) 表示,精度为百万分之一
- `custom_imf_ppm` 范围: (0, 1_000,000]
- 用户设置的"杠杆倍数"会转换为内部的 `custom_imf_ppm`

**转换公式**:

数学表达式:
```
custom_imf_ppm = 1,000,000 / Leverage
Leverage = 1,000,000 / custom_imf_ppm
```

变量说明:
- **custom_imf_ppm**: 内部存储的初始保证金率 (PPM 格式)
- **Leverage**: 用户理解的杠杆倍数
- **1,000,000**: PPM 转换常数 (100%)

**转换示例**:

| 杠杆倍数 | custom_imf_ppm | IMR (百分比) | 说明 |
|---------|---------------|-------------|------|
| 20x     | 50,000        | 5%          | 标准杠杆 |
| 10x     | 100,000       | 10%         | 保守杠杆 |
| 50x     | 20,000        | 2%          | 高杠杆 |
| 100x    | 10,000        | 1%          | 极高杠杆 |
| 5x      | 200,000       | 20%         | 低杠杆 |
| 2x      | 500,000       | 50%         | 极低杠杆 |

**精度优势**:
- PPM 格式支持 6 位小数精度 (1 ppm = 0.0001%)
- 避免浮点数误差
- 支持非整数杠杆 (如 15.5x = 64,516 ppm)

**业务规则**:
- `custom_imf_ppm` 必须 > 0 (不允许零保证金)
- `custom_imf_ppm` 必须 <= 1,000,000 (不允许低于 1x 杠杆)
- API 层面接受杠杆倍数,内部自动转换为 PPM

**验收标准**:
- 杠杆倍数与 PPM 转换正确
- 精度满足业务需求
- 边界值验证正确 (如 1x, 100x)

**用户影响**:
- 用户无需理解 PPM 概念,仅设置杠杆倍数即可
- 系统内部使用 PPM 保证精度
- API 返回时转换回杠杆倍数,用户友好

---

#### FR-1.3: 多永续合约杠杆配置

**需求描述**: 用户可以为同一子账户的不同永续合约设置不同的杠杆倍数。

**功能点**:
- 一个子账户可以持有多个永续合约的仓位
- 每个永续合约可以设置独立的杠杆
- 杠杆设置支持增量更新 (只修改指定的合约)

**数据结构**:

API 请求格式:
```json
{
  "subaccount_id": "alice/0",
  "clob_pair_leverage": [
    {
      "clob_pair_id": 0,        // BTC-USD
      "custom_imf_ppm": 50000   // 20x 杠杆
    },
    {
      "clob_pair_id": 1,        // ETH-USD
      "custom_imf_ppm": 100000  // 10x 杠杆
    }
  ]
}
```

内部存储格式:
```
Key: "Lev:" + SubaccountId
Value: {
  perpetual_id_0: custom_imf_ppm_0,
  perpetual_id_1: custom_imf_ppm_1,
  ...
}
```

**业务规则**:
- API 使用 `clob_pair_id`,内部转换为 `perpetual_id`
- 增量更新: 只修改请求中指定的永续合约,其他保持不变
- 首次设置: 未指定的永续合约使用流动性层级默认值

**更新逻辑**:

场景 1: 首次设置杠杆
- 请求: 为 BTC-USD (clob_pair_id=0) 设置 20x
- 结果: perpetual_id_0 = 50,000 ppm,其他永续合约未设置

场景 2: 增量更新
- 初始状态: perpetual_id_0 = 50,000 ppm (20x)
- 请求: 为 ETH-USD (clob_pair_id=1) 设置 10x
- 结果:
  - perpetual_id_0 = 50,000 ppm (保持不变)
  - perpetual_id_1 = 100,000 ppm (新增)

场景 3: 修改现有杠杆
- 初始状态:
  - perpetual_id_0 = 50,000 ppm (20x)
  - perpetual_id_1 = 100,000 ppm (10x)
- 请求: 修改 BTC-USD 为 50x
- 结果:
  - perpetual_id_0 = 20,000 ppm (更新)
  - perpetual_id_1 = 100,000 ppm (保持不变)

**验收标准**:
- 可以为不同合约设置不同杠杆
- 增量更新逻辑正确
- `clob_pair_id` 与 `perpetual_id` 转换正确

**用户场景**:

场景: 多合约差异化杠杆策略
- 参与者: 专业交易者 Frank
- 前置条件:
  - Frank 同时交易 BTC-USD 和 ETH-USD
  - Frank 认为 BTC 波动较小,ETH 波动较大
- 流程:
  1. Frank 设置 BTC-USD 杠杆为 20x (custom_imf_ppm = 50,000)
  2. Frank 设置 ETH-USD 杠杆为 10x (custom_imf_ppm = 100,000)
  3. Frank 在 BTC-USD 上开 20x 杠杆多头
  4. Frank 在 ETH-USD 上开 10x 杠杆多头
  5. BTC 需要 5% 保证金,ETH 需要 10% 保证金
  6. Frank 根据不同资产波动性优化风险
- 后置条件: 不同合约使用差异化杠杆

**业务价值**:
- 灵活的风险管理
- 根据资产特性优化杠杆
- 提高资金利用率

---

### FR-2: 杠杆调整验证

**需求描述**: 系统验证杠杆调整的合法性,防止用户在保证金不足时随意调整杠杆。

#### FR-2.1: 保证金充足性验证

**功能点**:
- 调整杠杆时检查账户净资产
- 验证新杠杆下保证金是否充足
- 拒绝导致保证金不足的杠杆调整

**计算公式**:

**杠杆调整验证**:

数学表达式:
```
NewMarginRequirement = PositionValue × (1 / NewLeverage)
验证: AccountNetCollateral >= NewMarginRequirement
```

变量说明:
- **NewMarginRequirement**: 新杠杆下的保证金要求 (USDC)
- **PositionValue**: 当前仓位价值 (USDC)
- **NewLeverage**: 新杠杆倍数
- **AccountNetCollateral**: 账户净资产 (USDC)
  - AccountNetCollateral = USDCBalance + UnrealizedPnL
  - USDCBalance: USDC 余额
  - UnrealizedPnL: 未实现盈亏

计算示例 (提高杠杆,通过):
- 假设仓位价值 = 50,000 USDC
- 假设 OldLeverage = 20x (IMR = 5%)
- 假设 NewLeverage = 50x (IMR = 2%)
- OldMarginReq = 50,000 × 5% = 2,500 USDC
- NewMarginReq = 50,000 × 2% = 1,000 USDC
- 假设账户净资产 = 3,000 USDC
- 验证: 3,000 >= 1,000 ✅ 通过 (提高杠杆降低保证金要求)

计算示例 (降低杠杆,失败):
- 假设仓位价值 = 50,000 USDC
- 假设 OldLeverage = 50x (IMR = 2%)
- 假设 NewLeverage = 10x (IMR = 10%)
- OldMarginReq = 50,000 × 2% = 1,000 USDC
- NewMarginReq = 50,000 × 10% = 5,000 USDC
- 假设账户净资产 = 3,000 USDC
- 验证: 3,000 < 5,000 ❌ 失败 (降低杠杆提高保证金要求,资金不足)

业务解释:
- 提高杠杆(降低保证金要求):通常可以通过验证
- 降低杠杆(提高保证金要求):需要账户有足够净资产
- 防止用户在保证金不足时随意调整杠杆
- 保护系统安全,避免风险累积

**业务规则**:
- 调整杠杆时必须验证保证金充足性
- 提高杠杆一般允许 (降低风险)
- 降低杠杆需要资金支持 (提高风险)
- 验证失败时拒绝调整,保持旧杠杆

**验收标准**:
- 保证金充足时调整成功
- 保证金不足时调整失败
- 验证逻辑正确

**用户场景**:

场景 2: 保证金不足拒绝调整
- 参与者: 交易者 Bob
- 前置条件:
  - Bob 持有 2 BTC 多头,仓位价值 100,000 USDC
  - Bob 当前杠杆 50x (IMR = 2%)
  - Bob 账户净资产 3,000 USDC
  - Bob 希望降低杠杆到 10x 以降低风险
- 流程:
  1. Bob 提交杠杆调整请求: NewLeverage = 10x
  2. 系统计算新保证金要求:
     - NewIMR = 1 / 10 = 10%
     - NewMarginReq = 100,000 × 10% = 10,000 USDC
  3. 系统验证保证金:
     - 账户净资产 = 3,000 USDC
     - 3,000 < 10,000 ❌ 不足
  4. 系统拒绝调整请求
  5. 返回错误:"保证金不足,无法降低杠杆。需要至少 10,000 USDC,当前只有 3,000 USDC"
  6. Bob 保持 50x 杠杆,或选择入金后再调整
- 后置条件: 杠杆调整失败,Bob 保持 50x 杠杆

---

#### FR-2.2: 流动性层级最大杠杆限制

**需求描述**: 每个永续合约通过流动性层级定义最大杠杆限制,用户设置的杠杆不能超过此限制。

**功能点**:
- 每个永续合约关联一个流动性层级 (Liquidity Tier)
- 流动性层级定义该合约的 `InitialMarginPpm` (最小初始保证金率)
- 最大杠杆 = 1,000,000 / InitialMarginPpm

**业务规则**:
- 用户设置的 `custom_imf_ppm` 必须 >= `InitialMarginPpm`
- 等价于: 用户杠杆 <= 最大杠杆
- 违反限制时拒绝设置,返回错误

**流动性层级配置示例**:

| 层级 ID | 合约示例 | InitialMarginPpm | 最大杠杆 | 说明 |
|--------|---------|-----------------|---------|------|
| 0      | BTC-USD | 20,000          | 50x     | 主流币,高流动性 |
| 1      | ETH-USD | 50,000          | 20x     | 主流币,中等流动性 |
| 2      | 小币种   | 100,000         | 10x     | 山寨币,低流动性 |

**验证逻辑**:

数学表达式:
```
验证: custom_imf_ppm >= LiquidityTier.InitialMarginPpm

等价于: Leverage <= (1,000,000 / LiquidityTier.InitialMarginPpm)
```

变量说明:
- **LiquidityTier.InitialMarginPpm**: 流动性层级定义的最小保证金率 (PPM)
- **custom_imf_ppm**: 用户设置的保证金率 (PPM)
- **Leverage**: 用户设置的杠杆倍数

**验证示例**:

示例 1: 通过验证 (BTC-USD, 最大 50x)
- 流动性层级: InitialMarginPpm = 20,000 (最大 50x)
- 用户设置: 20x 杠杆 (custom_imf_ppm = 50,000)
- 验证: 50,000 >= 20,000 ✅ 通过
- 结果: 允许设置 20x

示例 2: 验证失败 (BTC-USD, 超过 50x)
- 流动性层级: InitialMarginPpm = 20,000 (最大 50x)
- 用户设置: 100x 杠杆 (custom_imf_ppm = 10,000)
- 验证: 10,000 < 20,000 ❌ 失败
- 结果: 拒绝设置,返回错误:"杠杆不能超过 50x (流动性层级限制)"

示例 3: 边界值 (设置最大杠杆)
- 流动性层级: InitialMarginPpm = 20,000 (最大 50x)
- 用户设置: 50x 杠杆 (custom_imf_ppm = 20,000)
- 验证: 20,000 >= 20,000 ✅ 通过
- 结果: 允许设置 50x (刚好达到最大值)

**不同合约的最大杠杆**:

根据流动性层级配置,不同合约有不同的最大杠杆:

**高流动性合约** (如 BTC-USD):
- InitialMarginPpm = 20,000
- 最大杠杆 = 1,000,000 / 20,000 = 50x
- 原因: 流动性高,价格稳定,支持高杠杆

**中等流动性合约** (如 ETH-USD):
- InitialMarginPpm = 50,000
- 最大杠杆 = 1,000,000 / 50,000 = 20x
- 原因: 流动性适中,波动较大,限制杠杆

**低流动性合约** (如小币种):
- InitialMarginPpm = 100,000
- 最大杠杆 = 1,000,000 / 100,000 = 10x
- 原因: 流动性差,波动极大,严格限制杠杆

**验收标准**:
- 杠杆超过流动性层级限制时拒绝
- 错误消息明确说明最大杠杆
- 边界值正确验证 (如刚好 50x)

**用户场景**:

场景: 杠杆超过流动性层级限制
- 参与者: 交易者 George
- 前置条件:
  - George 交易 ETH-USD (流动性层级 1)
  - ETH-USD 的 InitialMarginPpm = 50,000 (最大 20x)
  - George 希望设置 50x 杠杆
- 流程:
  1. George 提交杠杆设置: 50x (custom_imf_ppm = 20,000)
  2. 系统查询 ETH-USD 的流动性层级
  3. 系统读取 InitialMarginPpm = 50,000
  4. 系统验证: 20,000 < 50,000 ❌ 失败
  5. 系统拒绝请求
  6. 返回错误:"ETH-USD 的最大杠杆为 20x (流动性层级限制),您设置的 50x 超出限制"
  7. George 调整为 20x 杠杆
  8. 系统验证: 50,000 >= 50,000 ✅ 通过
  9. 杠杆设置成功
- 后置条件: George 使用 20x 杠杆交易 ETH-USD

**业务价值**:
- 根据合约流动性差异化风险管理
- 保护系统免受低流动性合约的高杠杆风险
- 用户理解不同合约的风险等级
- 防止在高波动合约上过度杠杆

**错误码**:

| 错误码 | 消息 | 原因 |
|-------|------|------|
| `ErrInitialMarginPpmIsZero` | "流动性层级配置错误: InitialMarginPpm 为 0" | 系统配置问题 |
| `ErrInvalidLeverage` | "杠杆超出流动性层级限制" | 用户设置杠杆过高 |

---

### FR-3: 杠杆与保证金计算集成

**需求描述**: 杠杆设置影响订单放置和持仓的保证金计算。

#### FR-3.1: 订单保证金验证

**功能点**:
- 用户放置订单时,系统使用用户设置的杠杆计算保证金要求
- 保证金不足时拒绝订单
- 保证金充足时允许订单

**计算公式**:

**订单保证金计算** (完整版,包含 OI 缩放):

数学表达式:
```
# 第 1 步: 计算订单价值
OrderValue = OrderPrice × OrderSize

# 第 2 步: 获取用户自定义 IMF
Custom_IMF = 1,000,000 / UserLeverage

# 第 3 步: 获取流动性层级基础 IMF
Base_IMF = LiquidityTier.InitialMarginPpm

# 第 4 步: 根据当前市场 OI 计算调整后 IMF
if OI <= LowerCap:
    OI_Adjusted_IMF = Base_IMF
elif OI >= UpperCap:
    OI_Adjusted_IMF = 1,000,000
else:
    OI_Adjusted_IMF = Base_IMF + ((OI - LowerCap) / (UpperCap - LowerCap)) × (1,000,000 - Base_IMF)

# 第 5 步: 取三者最大值
Effective_IMF = max(Base_IMF, OI_Adjusted_IMF, Custom_IMF)

# 第 6 步: 计算保证金要求
OrderMarginRequirement = OrderValue × (Effective_IMF / 1,000,000)
```

变量说明:
- **OrderMarginRequirement**: 订单保证金要求 (USDC)
- **OrderValue**: 订单价值 (USDC)
- **OrderPrice**: 订单价格 (USDC/BTC)
- **OrderSize**: 订单数量 (BTC)
- **UserLeverage**: 用户设置的杠杆倍数
- **Custom_IMF**: 用户自定义初始保证金率 (PPM)
- **Base_IMF**: 流动性层级基础保证金率 (PPM)
- **OI_Adjusted_IMF**: OI 调整后的保证金率 (PPM)
- **Effective_IMF**: 最终有效保证金率 (PPM)
- **OI**: 当前市场未平仓合约量 (USDC)

**计算示例 1: 低 OI,用户杠杆生效**:
- 用户杠杆 = 20x (Custom_IMF = 50,000 ppm)
- 订单: 买入 1 BTC @ 50,000 USDC
- 市场 OI = 50M (< LowerCap 100M)
- 流动性层级: Base_IMF = 20,000
- 计算:
  - OrderValue = 50,000 × 1 = 50,000 USDC
  - OI_Adjusted_IMF = 20,000 (OI 低)
  - Effective_IMF = max(20,000, 20,000, 50,000) = **50,000**
  - OrderMarginRequirement = 50,000 × 5% = **2,500 USDC**
- 结果: 用户获得预期的 20x 杠杆

**计算示例 2: 高 OI,系统降杠杆**:
- 用户杠杆 = 20x (Custom_IMF = 50,000 ppm)
- 订单: 买入 1 BTC @ 50,000 USDC
- 市场 OI = 550M (处于 100M 和 1000M 之间)
- 流动性层级: Base_IMF = 20,000, LowerCap = 100M, UpperCap = 1000M
- 计算:
  - OrderValue = 50,000 USDC
  - OI_Adjusted_IMF = 20,000 + ((550M - 100M) / (1000M - 100M)) × (1,000,000 - 20,000)
  - OI_Adjusted_IMF = 20,000 + 0.5 × 980,000 = **510,000**
  - Effective_IMF = max(20,000, 510,000, 50,000) = **510,000**
  - OrderMarginRequirement = 50,000 × 51% = **25,500 USDC**
- 结果: 用户设置 20x,实际只有约 2x 杠杆

业务解释:
- **低 OI 市场**: 用户杠杆正常生效,保证金要求符合预期
- **高 OI 市场**: 系统自动提高保证金要求,降低实际杠杆
- 保证金验证确保用户有足够资金支持订单
- OI 缩放是动态的,随市场实时变化

**业务规则**:
- 每个订单放置时读取用户杠杆设置
- 根据杠杆计算保证金要求
- 保证金不足时拒绝订单

**验收标准**:
- 订单保证金计算正确
- 使用用户设置的杠杆
- 保证金验证准确

---

#### FR-3.2: OI 缩放机制 (Open Interest Scaling)

**需求描述**: 系统根据市场未平仓合约量 (OI) 动态调整保证金要求,实际杠杆可能低于用户设置值。

**什么是 OI (未平仓合约量)**:
- OI (Open Interest) = 市场上所有未平仓合约的总价值
- OI 越高,表示市场单边持仓风险越大
- OI 过高时可能导致系统性清算风险

**为什么需要 OI 缩放**:
- **风险管理**: 高 OI 意味着市场单边风险增加
- **系统保护**: 防止大规模清算导致系统不稳定
- **动态调整**: 根据市场实时状态调整杠杆,而非固定限制

**核心机制**:

用户设置的杠杆只是**最小值**,实际保证金要求由以下**三个因素的最大值**决定:

1. **流动性层级基础 IMF** (Base_IMF): 合约自身风险
2. **OI 调整后 IMF** (OI_Adjusted_IMF): 市场风险
3. **用户自定义 IMF** (Custom_IMF): 用户风险偏好

最终公式:
```
Effective_IMF = max(Base_IMF, OI_Adjusted_IMF, Custom_IMF)
```

**OI 调整后 IMF 的计算**:

流动性层级配置了 3 个关键参数:
- **InitialMarginPpm** (Base_IMF): 基础保证金率
- **OpenInterestLowerCap** (LowerCap): OI 下限
- **OpenInterestUpperCap** (UpperCap): OI 上限

计算逻辑:
```
if OI <= LowerCap:
    OI_Adjusted_IMF = Base_IMF
elif OI >= UpperCap:
    OI_Adjusted_IMF = 1,000,000 ppm (100% 保证金,相当于 1x 杠杆)
else:
    # 线性插值
    OI_Adjusted_IMF = Base_IMF + ((OI - LowerCap) / (UpperCap - LowerCap)) × (1,000,000 - Base_IMF)
```

**流动性层级配置示例** (BTC-USD):
- Base_IMF = 20,000 ppm (50x 杠杆)
- LowerCap = 100,000,000 USDC
- UpperCap = 1,000,000,000 USDC

**实际案例分析**:

**案例 1: 低 OI,用户杠杆生效**
- 市场状态: OI = 50,000,000 USDC (< LowerCap)
- 用户设置: 10x 杠杆 (Custom_IMF = 100,000 ppm)
- 计算:
  - OI_Adjusted_IMF = 20,000 (OI 低,使用基础值)
  - Effective_IMF = max(20,000, 100,000) = **100,000 ppm**
  - 实际杠杆 = 1,000,000 / 100,000 = **10x** ✅
- 结果: **用户获得预期的 10x 杠杆**

**案例 2: 中等 OI,杠杆部分降低**
- 市场状态: OI = 550,000,000 USDC (处于 LowerCap 和 UpperCap 之间)
- 用户设置: 10x 杠杆 (Custom_IMF = 100,000 ppm)
- 计算:
  - OI_Adjusted_IMF = 20,000 + ((550M - 100M) / (1000M - 100M)) × (1,000,000 - 20,000)
  - OI_Adjusted_IMF = 20,000 + (450M / 900M) × 980,000
  - OI_Adjusted_IMF = 20,000 + 490,000 = **510,000 ppm**
  - Effective_IMF = max(20,000, 510,000, 100,000) = **510,000 ppm**
  - 实际杠杆 = 1,000,000 / 510,000 ≈ **1.96x** ⚠️
- 结果: **用户设置 10x,实际只有约 2x 杠杆** (OI 缩放导致)

**案例 3: 高 OI,强制最低杠杆**
- 市场状态: OI = 1,200,000,000 USDC (> UpperCap)
- 用户设置: 10x 杠杆 (Custom_IMF = 100,000 ppm)
- 计算:
  - OI_Adjusted_IMF = 1,000,000 (OI 超过上限,100% 保证金)
  - Effective_IMF = max(20,000, 1,000,000, 100,000) = **1,000,000 ppm**
  - 实际杠杆 = 1,000,000 / 1,000,000 = **1x** ❌
- 结果: **用户设置 10x,实际只有 1x 杠杆** (OI 过高,系统强制降杠杆)

**用户影响**:
- ✅ **低 OI 市场**: 用户杠杆正常生效
- ⚠️ **中等 OI 市场**: 实际杠杆低于设置值
- ❌ **高 OI 市场**: 实际杠杆可能仅 1x (无杠杆)

**查询接口**:
用户可以查询:
- 设置的杠杆 (Custom_IMF)
- 当前市场 OI
- 当前实际有效杠杆 (Effective_IMF)

**业务规则**:
- OI 缩放自动生效,用户无需手动调整
- 用户设置的杠杆是**最小保证金要求**,而非**最终杠杆**
- 系统总是选择**最保守的保证金率**,确保安全

**验收标准**:
- OI 缩放计算正确
- 不同 OI 场景下保证金要求准确
- 查询接口返回实际有效杠杆
- 用户理解实际杠杆可能低于设置

**用户场景**:

场景: OI 缩放导致实际杠杆降低
- 参与者: 交易者 Henry
- 前置条件:
  - Henry 交易 BTC-USD
  - 流动性层级: Base_IMF = 20,000, LowerCap = 100M, UpperCap = 1000M
  - 当前市场 OI = 550,000,000 USDC (中等水平)
  - Henry 设置杠杆为 10x (Custom_IMF = 100,000)
- 流程:
  1. Henry 查询当前杠杆设置: 10x
  2. Henry 准备开仓 1 BTC @ 50,000 USDC
  3. Henry 预期保证金 = 50,000 × 10% = 5,000 USDC
  4. 系统计算实际保证金:
     - OI_Adjusted_IMF = 510,000 (根据 OI 550M 计算)
     - Effective_IMF = max(20,000, 510,000, 100,000) = 510,000
     - 实际保证金 = 50,000 × 51% = 25,500 USDC
  5. Henry 发现需要 25,500 USDC,而非预期的 5,000 USDC
  6. Henry 查询实际有效杠杆:
     - 系统返回: "设置杠杆 10x,当前实际杠杆约 2x (OI 缩放影响)"
  7. Henry 理解这是正常的风险管理机制
  8. Henry 选择:
     - 选项 A: 减小开仓数量 (如 0.2 BTC)
     - 选项 B: 等待市场 OI 降低
     - 选项 C: 增加保证金
- 后置条件: Henry 理解 OI 缩放机制,调整交易策略

**业务价值**:
- **透明化**: 用户理解为什么实际杠杆低于设置
- **风险管理**: 系统根据市场风险自动调整杠杆
- **用户信任**: 明确说明机制,避免"欺骗"感
- **系统安全**: 防止高 OI 市场的系统性风险

**重要提示**:

⚠️ **用户必须理解的关键点**:
1. 设置的杠杆是**你愿意承担的最小保证金率**
2. 实际杠杆取决于**市场风险**和**合约风险**
3. 高 OI 市场下,系统会**自动降低杠杆**以保护安全
4. 这**不是系统错误**,而是**风险管理机制**

---

### FR-4: 杠杆查询

**需求描述**: 用户可以查询自己的杠杆设置。

#### FR-4.1: 杠杆查询接口

**功能点**:
- 查询指定子账户的当前杠杆
- 查询杠杆对应的保证金率

**返回信息**:
- 子账户 ID
- 当前杠杆倍数
- 初始保证金率 (IMR)
- 最大杠杆限制 (从流动性层级获取)

**验收标准**:
- 查询返回准确的杠杆设置
- 查询延迟 < 100ms

---

## 3. 非功能需求

### NFR-1: 性能要求

**杠杆设置性能**:
- 杠杆设置延迟 < 区块时间
- 验证延迟 < 200ms

**查询性能**:
- 杠杆查询延迟 < 100ms

**验收标准**:
- 性能测试达到目标
- 杠杆设置不影响交易

### NFR-2: 准确性要求

**保证金计算精度**:
- IMR 计算精度 >= 6 位小数
- 保证金要求计算精度 >= 2 位小数 (USDC 分)

**验证准确性**:
- 保证金验证 100% 准确
- 无误判风险

**验收标准**:
- 精度测试通过
- 验证准确性测试通过

### NFR-3: 安全性要求

**风险控制**:
- 最大杠杆限制,防止过度风险
- 保证金验证,防止杠杆绕过风控
- 实时验证,不允许保证金不足的操作

**验收标准**:
- 风险控制测试通过
- 无法通过杠杆调整绕过风控

---

## 4. 用户场景

### 场景 3: 保守交易者使用低杠杆

**参与者**: 保守交易者 Carol

**前置条件**:
- Carol 是新手交易者,希望降低风险
- Carol 有 10,000 USDC

**流程**:
1. Carol 设置杠杆为 5x (IMR = 20%)
2. Carol 放置订单: 买入 0.2 BTC @ 50,000 USDC
3. 系统计算保证金要求:
   - OrderValue = 50,000 × 0.2 = 10,000 USDC
   - MarginReq = 10,000 × 20% = 2,000 USDC
4. Carol 账户余额 10,000 >= 2,000 ✅ 通过
5. 订单成功,Carol 持有 0.2 BTC 多头
6. BTC 价格下跌到 45,000 USDC (-10%)
7. Carol 亏损 = 0.2 × (45,000 - 50,000) = -1,000 USDC
8. Carol 净资产 = 10,000 - 1,000 = 9,000 USDC
9. 维持保证金要求 = 10,000 × 10% (假设) = 1,000 USDC
10. 9,000 >> 1,000,Carol 仍然安全,未被清算

**后置条件**: 低杠杆保护 Carol 免受清算

**业务价值**:
- 低杠杆降低风险,适合新手
- 保证金充足,抵抗价格波动
- 用户自主选择风险水平

### 场景 4: 高风险交易者使用高杠杆

**参与者**: 高风险交易者 Dave

**前置条件**:
- Dave 是经验丰富的交易者
- Dave 有 5,000 USDC,希望放大收益

**流程**:
1. Dave 设置杠杆为 50x (IMR = 2%)
2. Dave 放置订单: 买入 5 BTC @ 50,000 USDC
3. 系统计算保证金要求:
   - OrderValue = 50,000 × 5 = 250,000 USDC
   - MarginReq = 250,000 × 2% = 5,000 USDC
4. Dave 账户余额 5,000 = 5,000 ✅ 刚好满足
5. 订单成功,Dave 持有 5 BTC 多头 (仓位价值 250,000 USDC)
6. BTC 价格上涨到 51,000 USDC (+2%)
7. Dave 盈利 = 5 × (51,000 - 50,000) = 5,000 USDC
8. Dave 净资产 = 5,000 + 5,000 = 10,000 USDC
9. Dave 收益率 = 5,000 / 5,000 = 100% (翻倍!)
10. 但如果价格下跌 2%,Dave 亏损 5,000,全部损失

**后置条件**: 高杠杆放大收益,但风险极高

**业务价值**:
- 高杠杆满足专业交易者需求
- 收益和风险成正比
- 用户自主承担风险

### 场景 5: 杠杆调整优化保证金使用

**参与者**: 交易者 Eve

**前置条件**:
- Eve 持有 1 BTC 多头,仓位价值 50,000 USDC
- Eve 当前杠杆 10x (IMR = 10%)
- Eve 保证金要求 = 5,000 USDC
- Eve 账户净资产 = 10,000 USDC

**流程**:
1. Eve 发现自己保证金过剩 (10,000 >> 5,000)
2. Eve 决定提高杠杆到 20x,释放保证金
3. Eve 提交杠杆调整: NewLeverage = 20x
4. 系统计算新保证金要求:
   - NewIMR = 1 / 20 = 5%
   - NewMarginReq = 50,000 × 5% = 2,500 USDC
5. 系统验证: 10,000 >= 2,500 ✅ 通过
6. 杠杆更新为 20x
7. Eve 释放保证金 = 5,000 - 2,500 = 2,500 USDC
8. Eve 可以使用释放的 2,500 USDC 进行其他交易

**后置条件**: Eve 优化保证金使用,提高资金效率

**业务价值**:
- 灵活杠杆调整,优化资金使用
- 提高资金利用率
- 用户自主管理风险

---

## 5. 业务指标

### 5.1 关键指标 (KPI)

**杠杆使用指标**:
- 平均杠杆倍数
- 杠杆分布 (低/中/高)
- 杠杆调整频率

**风险指标**:
- 高杠杆用户占比
- 高杠杆账户清算率
- 平均保证金覆盖率

### 5.2 监控指标

**杠杆健康度**:
- 杠杆调整成功率
- 杠杆验证失败率
- 极端杠杆使用情况

**用户行为**:
- 杠杆调整趋势
- 不同杠杆用户的交易行为
- 杠杆与盈亏相关性

---

## 6. 术语表

| 术语              | 英文                  | 定义                                                    |
|------------------|----------------------|--------------------------------------------------------|
| 杠杆              | Leverage             | 放大交易规模的倍数                                       |
| 杠杆倍数          | Leverage Ratio       | 仓位价值与保证金的比例 (如 20x)                          |
| 初始保证金率      | Initial Margin Rate (IMR) | 开仓所需保证金占仓位价值的比例                      |
| 保证金            | Margin               | 用于支持仓位的资金                                       |
| 净资产            | Net Collateral       | 账户总资产 (余额 + 未实现盈亏)                           |
| 低杠杆            | Low Leverage         | 1x - 5x,保守交易                                        |
| 中杠杆            | Medium Leverage      | 5x - 20x,标准交易                                       |
| 高杠杆            | High Leverage        | 20x - 100x,高风险交易                                   |
| IMF               | Initial Margin Factor | 初始保证金因子,用 PPM 表示                               |
| PPM               | Parts Per Million    | 百万分之一,精度单位 (1 ppm = 0.0001%)                   |
| OI                | Open Interest        | 未平仓合约量,市场总持仓价值                              |
| OI 缩放           | OI Scaling           | 根据 OI 动态调整保证金的机制                             |
| Base IMF          | Base Initial Margin Factor | 流动性层级定义的基础保证金率                       |
| Custom IMF        | Custom Initial Margin Factor | 用户自定义的保证金率                               |
| Effective IMF     | Effective Initial Margin Factor | 最终有效保证金率 (三者最大值)                   |
| 流动性层级        | Liquidity Tier       | 定义合约风险参数和保证金要求的配置                        |

---

## 7. 附录

### 7.1 错误码参考

| 错误码 | 消息 | 触发场景 | 用户操作建议 |
|-------|------|---------|------------|
| `ErrInvalidLeverage` | "杠杆设置无效" | 1. `custom_imf_ppm` 不在 (0, 1,000,000] 范围<br>2. 杠杆超出流动性层级限制 | 1. 检查杠杆倍数是否 >= 1x<br>2. 查询该合约的最大杠杆限制<br>3. 调整为允许范围内的杠杆 |
| `ErrInitialMarginPpmIsZero` | "流动性层级配置错误: InitialMarginPpm 为 0" | 流动性层级配置异常 (系统错误) | 联系系统管理员,这是配置问题 |
| `ErrInsufficientMargin` | "保证金不足" | 调整杠杆后保证金不满足要求 | 1. 增加保证金<br>2. 提高杠杆 (降低保证金要求)<br>3. 减少仓位 |
| `ErrClobPairNotFound` | "交易对不存在" | clob_pair_id 无效 | 检查 clob_pair_id 是否正确 |

**错误处理示例**:

场景: 用户设置杠杆超过流动性层级限制
- 请求: 为 ETH-USD 设置 50x 杠杆
- 系统检查: ETH-USD 最大杠杆 20x
- 返回错误:
  ```json
  {
    "code": "ErrInvalidLeverage",
    "message": "杠杆超出流动性层级限制",
    "details": {
      "requested_leverage": "50x",
      "max_leverage": "20x",
      "clob_pair_id": 1,
      "perpetual_id": 1
    }
  }
  ```
- 用户操作: 调整为 20x 或更低

---

### 7.2 CLI 使用示例

**命令**: `hermesd tx clob update-leverage`

**功能**: 更新子账户的杠杆设置

**语法**:
```bash
hermesd tx clob update-leverage [subaccount_id] [leverage_entries_json] [flags]
```

**参数说明**:
- `subaccount_id`: 子账户 ID (格式: "owner/number")
- `leverage_entries_json`: JSON 数组,指定 CLOB 交易对和杠杆

**示例 1: 为单个合约设置杠杆**
```bash
hermesd tx clob update-leverage \
  "alice/0" \
  '[{"clob_pair_id": 0, "custom_imf_ppm": 50000}]' \
  --from alice \
  --chain-id hermes-1 \
  --gas auto
```
- 设置 BTC-USD (clob_pair_id=0) 为 20x 杠杆 (50,000 ppm)

**示例 2: 为多个合约设置不同杠杆**
```bash
hermesd tx clob update-leverage \
  "bob/0" \
  '[
    {"clob_pair_id": 0, "custom_imf_ppm": 50000},
    {"clob_pair_id": 1, "custom_imf_ppm": 100000}
  ]' \
  --from bob \
  --chain-id hermes-1 \
  --gas auto
```
- BTC-USD: 20x 杠杆 (50,000 ppm)
- ETH-USD: 10x 杠杆 (100,000 ppm)

**示例 3: 设置极低杠杆 (保守策略)**
```bash
hermesd tx clob update-leverage \
  "carol/0" \
  '[{"clob_pair_id": 0, "custom_imf_ppm": 500000}]' \
  --from carol
```
- BTC-USD: 2x 杠杆 (500,000 ppm)

**示例 4: 设置极高杠杆 (激进策略)**
```bash
hermesd tx clob update-leverage \
  "dave/0" \
  '[{"clob_pair_id": 0, "custom_imf_ppm": 20000}]' \
  --from dave
```
- BTC-USD: 50x 杠杆 (20,000 ppm,假设流动性层级允许)

**杠杆倍数与 PPM 转换表**:

| 杠杆倍数 | custom_imf_ppm | CLI 值 |
|---------|---------------|--------|
| 1x      | 1,000,000     | `1000000` |
| 2x      | 500,000       | `500000` |
| 5x      | 200,000       | `200000` |
| 10x     | 100,000       | `100000` |
| 20x     | 50,000        | `50000` |
| 50x     | 20,000        | `20000` |
| 100x    | 10,000        | `10000` |

**查询杠杆设置**:
```bash
hermesd query clob leverage alice/0
```

**常见错误**:

1. **杠杆超出限制**:
   ```
   Error: leverage exceeds liquidity tier limit
   ```
   解决: 查询最大杠杆限制,调整为允许范围

2. **保证金不足**:
   ```
   Error: insufficient margin for new leverage
   ```
   解决: 增加保证金或提高杠杆

3. **无效的 clob_pair_id**:
   ```
   Error: clob pair not found
   ```
   解决: 检查 clob_pair_id 是否存在

---

## 7. 参考资料

### 架构文档
- [Leverage 模块架构设计](../../architecture/leverage.md)

### 数据结构文档
- [Leverage 模块数据结构](../data_structure/leverage.md)

---

**文档版本**: v2.0
**最后更新**: 2025-12-31
**文档作者**: Claude Sonnet 4.5
**文档状态**: ✅ 完成 (已补充 OI 缩放机制等核心内容)

**更新日志**:

**v2.0 (2025-12-31)**:
- ✅ 新增 FR-1.2: IMF 精度与 PPM 表示
- ✅ 新增 FR-1.3: 多永续合约杠杆配置
- ✅ 新增 FR-2.2: 流动性层级最大杠杆限制
- ✅ 新增 FR-3.2: OI 缩放机制 (最重要的补充)
- ✅ 更新 FR-3.1: 订单保证金计算公式 (包含 OI 缩放)
- ✅ 扩展术语表 (新增 IMF, PPM, OI 等术语)
- ✅ 新增附录 7.1: 错误码参考
- ✅ 新增附录 7.2: CLI 使用示例

**v1.0 (2025-12-31)**:
- 初始版本,包含基础杠杆功能 (FR-1 到 FR-4)
