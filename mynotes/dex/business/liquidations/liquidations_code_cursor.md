# 清算与去杠杆机制代码分析

本文档基于代码实现，详细分析 Hermes DEX 的清算（Liquidation）和去杠杆（Deleveraging）业务机制。

## 目录

1. [概述](#1-概述)
2. [清算机制](#2-清算机制)
3. [去杠杆机制](#3-去杠杆机制)
4. [清算价格计算](#4-清算价格计算)
5. [破产价格计算](#5-破产价格计算)
6. [保险基金机制](#6-保险基金机制)
7. [强制平仓补偿与社会化损失](#7-强制平仓补偿与社会化损失)
8. [完整流程示例](#8-完整流程示例)
9. [代码调用链](#9-代码调用链)

---

## 1. 概述

### 1.1 基本概念

**清算（Liquidation）**：当账户的净抵押品（Net Collateral）低于维持保证金要求（Maintenance Margin Requirement）时，系统强制平仓部分或全部持仓。

**去杠杆（Deleveraging）**：当清算无法完全平仓，或账户已资不抵债（负净值）时，系统强制匹配相反方向的持仓来平仓。

### 1.2 触发条件

#### 清算触发条件

```go
// protocol/x/clob/keeper/liquidations.go:357
func (k Keeper) IsLiquidatable(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
) (bool, error) {
	risk, err := k.subaccountsKeeper.GetNetCollateralAndMarginRequirements(
		ctx,
		satypes.Update{SubaccountId: subaccountId},
	)
	if err != nil {
		return false, err
	}
	return risk.IsLiquidatable(), nil
}
```

**清算条件**：
```
NetCollateral < MaintenanceMarginRequirement
```

#### 去杠杆触发条件

```go
// protocol/x/clob/keeper/deleveraging.go:142
func (k Keeper) CanDeleverageSubaccount(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
	perpetualId uint32,
) (shouldDeleverageAtBankruptcyPrice bool, shouldDeleverageAtOraclePrice bool, err error)
```

**去杠杆条件**：
1. **负净值去杠杆**：`NetCollateral < 0`（资不抵债）
2. **最终结算去杠杆**：市场处于最终结算状态（Final Settlement）

---

## 2. 清算机制

### 2.1 清算流程

#### 主入口函数

```go
// protocol/x/clob/keeper/liquidations.go:38
func (k Keeper) LiquidateSubaccountsAgainstOrderbook(
	ctx sdk.Context,
	subaccountIds []satypes.SubaccountId,
) (subaccountsToDeleverage []subaccountToDeleverage, err error)
```

**流程步骤**：

1. **获取可清算账户列表**
   - 从清算守护进程（Liquidation Daemon）获取可清算账户
   - 限制每个区块最多处理 `MaxLiquidationAttemptsPerBlock` 个账户

2. **生成清算订单**
   ```go
   // protocol/x/clob/keeper/liquidations.go:168
   func (k Keeper) MaybeGetLiquidationOrder(
       ctx sdk.Context,
       subaccountId satypes.SubaccountId,
   ) (*types.LiquidationOrder, error)
   ```

3. **排序清算订单**
   - 按与预言机价格的偏差百分比降序排序
   - 最资不抵债的账户优先清算

4. **下单并匹配**
   ```go
   // protocol/x/clob/keeper/liquidations.go:246
   func (k Keeper) PlacePerpetualLiquidation(
       ctx sdk.Context,
       liquidationOrder types.LiquidationOrder,
   ) (optimisticallyFilledQuantums satypes.BaseQuantums, offchainUpdates *types.OffchainUpdates, err error)
   ```

5. **处理未完全成交的订单**
   - 如果清算订单未完全成交，进入去杠杆流程

### 2.2 清算订单生成

#### 获取清算持仓

```go
// protocol/x/clob/keeper/liquidations.go:735
func (k Keeper) GetPerpetualPositionToLiquidate(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
) (perpetualId uint32, err error)
```

**逻辑**：
- 随机选择一个未清算过的永续合约持仓
- 避免同一区块内重复清算同一持仓

#### 计算清算数量

```go
// protocol/x/clob/keeper/liquidations.go:773
func (k Keeper) GetLiquidatablePositionSizeDelta(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
	perpetualId uint32,
) (deltaQuantums *big.Int, err error)
```

**限制条件**：
- `MinPositionNotionalLiquidated`：最小清算名义价值
- `MaxPositionPortionLiquidatedPpm`：最大清算比例（PPM）
- `MaxPositionNotionalLiquidated`：最大清算名义价值

#### 计算可成交价格（Fillable Price）

```go
// protocol/x/clob/keeper/liquidations.go:514
func (k Keeper) GetFillablePrice(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
	perpetualId uint32,
	deltaQuantums *big.Int,
) (fillablePrice *big.Rat, err error)
```

**公式**：
```
FillablePrice = (PNNV - ABR * SMMR * PMMR) / PS

其中：
- PNNV: Position Net Notional Value（持仓净名义价值）
- ABR: Adjusted Bankruptcy Rating（调整后破产评级）
  ABR = BA * (1 - TNC / TMMR)，限制在 [0, 1]
- SMMR: Spread to Maintenance Margin Ratio（价差与维持保证金比率）
- PMMR: Position Maintenance Margin Requirement（持仓维持保证金要求）
- PS: Position Size（持仓数量）
- BA: Bankruptcy Adjustment PPM（破产调整系数）
- TNC: Total Net Collateral（总净抵押品）
- TMMR: Total Maintenance Margin Requirement（总维持保证金要求）
```

**代码实现**：

```go
// protocol/x/clob/keeper/liquidations.go:608-649
// 计算 ABR
tncDivTmmrRat := new(big.Rat).SetFrac(riskTotal.NC, riskTotal.MMR)
unboundedAbrRat := lib.BigRatMulPpm(
    new(big.Rat).Sub(
        lib.BigRat1(),
        tncDivTmmrRat,
    ),
    ba,
)
abrRat := lib.BigRatClamp(unboundedAbrRat, lib.BigRat0(), lib.BigRat1())

// 计算最大清算价差
maxLiquidationSpreadQuoteQuantumsRat := lib.BigRatMulPpm(
    new(big.Rat).SetInt(riskPos.MMR),
    smmr,
)

// 计算可成交价格
fillablePriceOracleDeltaQuoteQuantumsRat := new(big.Rat).Mul(abrRat, maxLiquidationSpreadQuoteQuantumsRat)
pnnvRat := new(big.Rat).SetInt(riskPos.NC)
fillablePriceQuoteQuantumsRat := new(big.Rat).Sub(pnnvRat, fillablePriceOracleDeltaQuoteQuantumsRat)
fillablePrice = new(big.Rat).Quo(
    fillablePriceQuoteQuantumsRat,
    new(big.Rat).SetInt(psBig),
)
```

### 2.3 清算订单匹配

清算订单是 **IOC（Immediate or Cancel）** 订单，立即与订单簿匹配：

- **做多清算**：下卖单，与订单簿买单匹配
- **做空清算**：下买单，与订单簿卖单匹配

匹配成功后，生成 `MatchPerpetualLiquidation` 事件。

---

## 3. 去杠杆机制

### 3.1 去杠杆流程

#### 主入口函数

```go
// protocol/x/clob/keeper/deleveraging.go:35
func (k Keeper) MaybeDeleverageSubaccount(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
	perpetualId uint32,
) (quantumsDeleveraged *big.Int, err error)
```

**流程步骤**：

1. **检查去杠杆条件**
   ```go
   shouldDeleverageAtBankruptcyPrice, shouldDeleverageAtOraclePrice, err := k.CanDeleverageSubaccount(
       ctx,
       subaccountId,
       perpetualId,
   )
   ```

2. **查找对手方持仓**
   ```go
   // protocol/x/clob/keeper/deleveraging.go:289
   func (k Keeper) OffsetSubaccountPerpetualPosition(
       ctx sdk.Context,
       liquidatedSubaccountId satypes.SubaccountId,
       perpetualId uint32,
       deltaQuantumsTotal *big.Int,
       isFinalSettlement bool,
   ) (fills []types.MatchPerpetualDeleveraging_Fill, deltaQuantumsRemaining *big.Int)
   ```

3. **匹配对手方账户**
   - 从清算守护进程获取持有相反方向持仓的账户列表
   - 随机选择起始位置，最多迭代 `MaxDeleveragingSubaccountsToIterate` 个账户
   - 验证破产价格是否重叠（非最终结算场景）

4. **执行去杠杆**
   ```go
   // protocol/x/clob/keeper/deleveraging.go:502
   func (k Keeper) ProcessDeleveraging(
       ctx sdk.Context,
       liquidatedSubaccountId satypes.SubaccountId,
       offsettingSubaccountId satypes.SubaccountId,
       perpetualId uint32,
       deltaBaseQuantums *big.Int,
       deltaQuoteQuantums *big.Int,
   ) error
   ```

### 3.2 去杠杆价格

#### 标准去杠杆（负净值）

使用**破产价格（Bankruptcy Price）**：

```go
// protocol/x/clob/keeper/deleveraging.go:471
func (k Keeper) getDeleveragingQuoteQuantumsDelta(
	ctx sdk.Context,
	perpetualId uint32,
	liquidatedSubaccountId satypes.SubaccountId,
	deltaBaseQuantums *big.Int,
	isFinalSettlement bool,
) (*big.Int, error) {
	if isFinalSettlement {
		// 最终结算：使用预言机价格
		return k.getOraclePriceQuoteQuantumsDelta(...)
	}
	// 标准去杠杆：使用破产价格
	return k.GetBankruptcyPriceInQuoteQuantums(
		ctx,
		liquidatedSubaccountId,
		perpetualId,
		deltaBaseQuantums,
	)
}
```

#### 最终结算去杠杆

使用**预言机价格（Oracle Price）**：

```go
// 最终结算时，使用预言机价格
oraclePriceQuoteQuantums := k.getOraclePriceQuoteQuantumsDelta(...)
```

### 3.3 去杠杆匹配逻辑

```go
// protocol/x/clob/keeper/deleveraging.go:295
func (k Keeper) OffsetSubaccountPerpetualPosition(...)
```

**匹配规则**：

1. **方向匹配**：对手方持仓必须与被清算账户持仓方向相反
2. **数量限制**：每次匹配的数量不超过对手方持仓数量
3. **破产价格重叠**：标准去杠杆需要破产价格重叠（最终结算除外）

**匹配过程**：

```go
for i := 0; i < numSubaccountsToIterate && deltaQuantumsRemaining.Sign() != 0; i++ {
    // 1. 获取对手方账户
    offsettingSubaccount := k.subaccountsKeeper.GetSubaccount(ctx, subaccountId)
    
    // 2. 计算匹配数量
    if deltaQuantumsRemaining.CmpAbs(bigOffsettingPositionQuantums) > 0 {
        deltaBaseQuantums = new(big.Int).Set(bigOffsettingPositionQuantums)
    } else {
        deltaBaseQuantums = new(big.Int).Set(deltaQuantumsRemaining)
    }
    
    // 3. 计算报价数量（破产价格或预言机价格）
    deltaQuoteQuantums, err := k.getDeleveragingQuoteQuantumsDelta(...)
    
    // 4. 执行去杠杆
    if err := k.ProcessDeleveraging(...); err == nil {
        // 成功：更新剩余数量
        deltaQuantumsRemaining.Sub(deltaQuantumsRemaining, deltaBaseQuantums)
    }
}
```

---

## 4. 清算价格计算

### 4.1 Fillable Price（可成交价格）

**定义**：清算订单可以成交的价格，用于在订单簿上下单。

**计算公式**：

```go
// protocol/x/clob/keeper/liquidations.go:523
// FillablePrice = (PNNV - ABR * SMMR * PMMR) / PS

// 其中：
// ABR = BA * (1 - TNC / TMMR)，限制在 [0, 1]
```

**参数说明**：

- **PNNV（Position Net Notional Value）**：持仓净名义价值
  - 做多：`PNNV = PositionSize × Price`（正数）
  - 做空：`PNNV = PositionSize × Price`（负数）

- **ABR（Adjusted Bankruptcy Rating）**：调整后破产评级
  - 反映账户的健康程度
  - `TNC / TMMR` 越小，ABR 越大，清算价格越差

- **SMMR（Spread to Maintenance Margin Ratio）**：价差与维持保证金比率
  - 配置参数，控制清算价差上限

- **PMMR（Position Maintenance Margin Requirement）**：持仓维持保证金要求

**计算示例**：

假设：
- 持仓：10 BTC（做多）
- BTC 价格：$50,000
- PNNV = 10 × 50,000 = 500,000 USDC
- PMMR = 50,000 USDC（10% 维持保证金）
- TNC = 30,000 USDC
- TMMR = 100,000 USDC
- BA = 1,000,000 PPM（100%）
- SMMR = 100,000 PPM（10%）

计算：
```
ABR = 100% × (1 - 30,000 / 100,000) = 70%
最大价差 = 10% × 50,000 = 5,000 USDC
价差调整 = 70% × 5,000 = 3,500 USDC
FillablePrice = (500,000 - 3,500) / 10 = 49,650 USDC
```

**价格限制**：

```go
// protocol/x/clob/keeper/liquidations.go:1019
func (k Keeper) ConvertFillablePriceToSubticks(...) types.Subticks {
    // 限制在 [minSubticks, maxSubticks] 范围内
    // minSubticks = oraclePrice * (1 - maxSpread)
    // maxSubticks = oraclePrice * (1 + maxSpread)
}
```

---

## 5. 破产价格计算

### 5.1 破产价格定义

**破产价格（Bankruptcy Price）**：平仓后账户净值刚好为 0 的价格。

### 5.2 计算公式

```go
// protocol/x/clob/keeper/liquidations.go:413
// 破产价格 = -DNNV - (TNC * abs(DMMR) / TMMR)
```

**参数说明**：

- **DNNV（Delta Net Notional Value）**：持仓净名义价值变化
  - `DNNV = PNNV_after - PNNV_before`
  - `PNNV_after`：平仓后的持仓净名义价值（通常为 0）
  - `PNNV_before`：平仓前的持仓净名义价值

- **DMMR（Delta Maintenance Margin Requirement）**：维持保证金要求变化
  - `DMMR = MMR_after - MMR_before`
  - 平仓后 MMR 减少，所以 DMMR 为负数

- **TNC（Total Net Collateral）**：总净抵押品
- **TMMR（Total Maintenance Margin Requirement）**：总维持保证金要求

**代码实现**：

```go
// protocol/x/clob/keeper/liquidations.go:463-508
// 计算平仓前后的风险指标
riskPosOld := perplib.GetPositionNetNotionalValueAndMarginRequirements(
    perpetual, marketPrice, liquidityTier, psBig, 0,
)
riskPosNew := perplib.GetPositionNetNotionalValueAndMarginRequirements(
    perpetual, marketPrice, liquidityTier,
    new(big.Int).Add(psBig, deltaQuantums), 0,
)

// 计算变化量
deltaNC := new(big.Int).Sub(riskPosNew.NC, riskPosOld.NC)  // DNNV
deltaMMR := new(big.Int).Sub(riskPosNew.MMR, riskPosOld.MMR)  // DMMR

// 计算 TNC * abs(DMMR) / TMMR
tncMulDmmrBig := new(big.Int).Mul(riskTotal.NC, new(big.Int).Abs(deltaMMR))
quoteQuantumsBeforeBankruptcyBig := new(big.Int).Div(tncMulDmmrBig, riskTotal.MMR)

// 计算破产价格
bankruptcyPriceQuoteQuantumsBig := new(big.Int).Sub(
    new(big.Int).Neg(deltaNC),
    quoteQuantumsBeforeBankruptcyBig,
)
```

### 5.3 计算示例

**场景**：做多 10 BTC，需要平仓

假设：
- 当前价格：$50,000
- 持仓：10 BTC（做多）
- TNC = 30,000 USDC
- TMMR = 100,000 USDC（包含其他持仓）
- 该持仓的 MMR = 50,000 USDC（10% 维持保证金）

计算：
```
平仓前：
- PNNV_before = 10 × 50,000 = 500,000 USDC
- MMR_before = 50,000 USDC

平仓后（deltaQuantums = -10）：
- PNNV_after = 0
- MMR_after = 0

变化量：
- DNNV = 0 - 500,000 = -500,000 USDC
- DMMR = 0 - 50,000 = -50,000 USDC

破产价格计算：
破产价格 = -(-500,000) - (30,000 × 50,000 / 100,000)
         = 500,000 - 15,000
         = 485,000 USDC

单位价格 = 485,000 / 10 = 48,500 USDC/BTC
```

**含义**：如果以 $48,500 的价格平仓 10 BTC，账户净值将刚好为 0。

---

## 6. 保险基金机制

### 6.1 保险基金概述

**保险基金（Insurance Fund）**：用于覆盖清算和去杠杆过程中的损失，保护系统稳定性。

**基金类型**：
- **Cross Margin 市场**：共享保险基金 `insurance_fund`
- **Isolated Margin 市场**：独立保险基金 `insurance_fund:<perpetualId>`

### 6.2 保险基金地址

```go
// protocol/x/perpetuals/keeper/perpetual.go:47
func (k Keeper) GetInsuranceFundName(ctx sdk.Context, perpetualId uint32) (string, error) {
    perpetual, err := k.GetPerpetual(ctx, perpetualId)
    if err != nil {
        return "", err
    }
    if perpetual.Params.MarketType == types.PerpetualMarketType_PERPETUAL_MARKET_TYPE_ISOLATED {
        return types.InsuranceFundName + ":" + lib.UintToString(perpetualId), nil
    }
    return types.InsuranceFundName, nil
}
```

### 6.3 保险基金变化计算

#### 计算公式

```go
// protocol/x/clob/keeper/liquidations.go:656
func (k Keeper) GetLiquidationInsuranceFundDelta(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
	perpetualId uint32,
	isBuy bool,
	fillAmount uint64,
	subticks types.Subticks,
) (insuranceFundDeltaQuoteQuantums *big.Int, err error)
```

**公式**：
```
保险基金变化 = 实际成交价值 - 破产价格成交价值
```

**代码实现**：

```go
// protocol/x/clob/keeper/liquidations.go:704-732
// 1. 计算实际成交价值
deltaQuoteQuantums := types.FillAmountToQuoteQuantums(
    subticks, satypes.BaseQuantums(fillAmount), clobPair.QuantumConversionExponent,
)

// 2. 计算破产价格成交价值
bankruptcyPriceInQuoteQuantumsBig, err := k.GetBankruptcyPriceInQuoteQuantums(
    ctx, subaccountId, perpetualId, deltaQuantums,
)

// 3. 计算保险基金变化
insuranceFundDeltaQuoteQuantumsBig := new(big.Int).Sub(
    deltaQuoteQuantums,
    bankruptcyPriceInQuoteQuantumsBig,
)

// 4. 如果为负，保险基金需要支付
if insuranceFundDeltaQuoteQuantumsBig.Sign() <= 0 {
    return insuranceFundDeltaQuoteQuantumsBig, nil
}

// 5. 如果为正，用户需要支付清算费用（上限为 MaxLiquidationFeePpm）
maxLiquidationFeeQuoteQuantumsBig := lib.BigIntMulPpm(
    new(big.Int).Abs(deltaQuoteQuantums),
    liquidationsConfig.MaxLiquidationFeePpm,
)
return lib.BigMin(
    maxLiquidationFeeQuoteQuantumsBig,
    insuranceFundDeltaQuoteQuantumsBig,
), nil
```

### 6.4 保险基金变化含义

#### 负值（保险基金支付）

```
保险基金变化 < 0
```

**含义**：实际成交价格低于破产价格，保险基金需要补偿差额。

**示例**：
- 破产价格：$48,500
- 实际成交价格：$48,000
- 平仓数量：10 BTC
- 保险基金变化 = (48,000 - 48,500) × 10 = -5,000 USDC

保险基金需要支付 5,000 USDC 来覆盖损失。

#### 正值（用户支付清算费）

```
保险基金变化 > 0
```

**含义**：实际成交价格高于破产价格，用户需要支付清算费用（上限为 MaxLiquidationFeePpm）。

**示例**：
- 破产价格：$48,500
- 实际成交价格：$49,000
- 平仓数量：10 BTC
- 保险基金变化 = (49,000 - 48,500) × 10 = 5,000 USDC
- MaxLiquidationFeePpm = 50,000 PPM（5%）
- 最大清算费 = 49,000 × 10 × 5% = 24,500 USDC
- 实际清算费 = min(5,000, 24,500) = 5,000 USDC

用户支付 5,000 USDC 清算费给保险基金。

### 6.5 保险基金验证

```go
// protocol/x/clob/keeper/liquidations.go:1125
if !k.IsValidInsuranceFundDelta(ctx, insuranceFundDelta, perpetualId) {
    return nil, errorsmod.Wrapf(
        types.ErrInsuranceFundHasInsufficientFunds,
        "Liquidation order %v, insurance fund delta %v",
        order,
        insuranceFundDelta.String(),
    )
}
```

**验证逻辑**：

```go
// protocol/x/clob/keeper/deleveraging.go:280
func (k Keeper) IsValidInsuranceFundDelta(
	ctx sdk.Context,
	insuranceFundDelta *big.Int,
	perpetualId uint32,
) bool {
	currentInsuranceFundBalance, err := k.perpetualsKeeper.GetInsuranceFundBalance(
		ctx, perpetualId,
	)
	if err != nil {
		return false
	}
	// 验证：当前余额 + 变化量 >= 0
	return new(big.Int).Add(currentInsuranceFundBalance, insuranceFundDelta).Sign() >= 0
}
```

**含义**：保险基金余额必须足够支付损失，否则清算/去杠杆无法执行。

### 6.6 保险基金转账

```go
// protocol/x/subaccounts/keeper/transfer.go:390
func (k Keeper) TransferInsuranceFundPayments(
	ctx sdk.Context,
	insuranceFundDelta *big.Int,
	perpetualId uint32,
) error {
	if insuranceFundDelta.Sign() == 0 {
		return nil
	}

	// 确定发送方和接收方
	fromModule, err := k.GetCollateralPoolFromPerpetualId(ctx, perpetualId)
	toModule, err := k.perpetualsKeeper.GetInsuranceFundModuleAddress(ctx, perpetualId)

	if insuranceFundDelta.Sign() < 0 {
		// 保险基金支付：从保险基金转到子账户模块
		fromModule, toModule = toModule, fromModule
	}

	// 执行转账
	return k.bankKeeper.SendCoins(ctx, fromModule, toModule, []sdk.Coin{coinToTransfer})
}
```

---

## 7. 强制平仓补偿与社会化损失

### 7.1 强制平仓补偿

**定义**：当清算/去杠杆以低于破产价格成交时，保险基金补偿差额。

**触发条件**：
```
实际成交价格 < 破产价格
```

**补偿金额**：
```
补偿金额 = (破产价格 - 实际成交价格) × 平仓数量
```

**代码位置**：

```go
// protocol/x/clob/keeper/liquidations.go:709-714
if insuranceFundDeltaQuoteQuantumsBig.Sign() <= 0 {
    // 保险基金需要覆盖损失
    return insuranceFundDeltaQuoteQuantumsBig, nil
}
```

### 7.2 社会化损失（Socialized Loss）

#### 定义

**社会化损失**：当保险基金余额不足以覆盖损失时，损失会分摊给对手方账户。

#### 产生条件

1. **保险基金余额不足**
   ```go
   // 如果保险基金余额 < 所需支付金额
   if !k.IsValidInsuranceFundDelta(ctx, insuranceFundDelta, perpetualId) {
       // 清算/去杠杆无法执行，或需要社会化损失
   }
   ```

2. **去杠杆场景中的价格差异**

在去杠杆过程中，如果破产价格与市场价格的差异无法完全由保险基金覆盖，对手方会承担部分损失。

#### 社会化损失的体现

**在去杠杆中**：

```go
// protocol/x/clob/keeper/deleveraging.go:502
func (k Keeper) ProcessDeleveraging(
	ctx sdk.Context,
	liquidatedSubaccountId satypes.SubaccountId,
	offsettingSubaccountId satypes.SubaccountId,
	perpetualId uint32,
	deltaBaseQuantums *big.Int,
	deltaQuoteQuantums *big.Int,  // 这是破产价格
) error {
	// 被清算账户：按破产价格平仓
	deleveragedSubaccountQuoteBalanceDelta := deltaQuoteQuantums
	
	// 对手方账户：按破产价格平仓（可能低于市场价格）
	offsettingSubaccountQuoteBalanceDelta := new(big.Int).Neg(deltaQuoteQuantums)
	
	// 更新账户
	updates := []satypes.Update{
		{
			// 被清算账户更新
			AssetUpdates: []satypes.AssetUpdate{{
				AssetId: assettypes.AssetUsdc.Id,
				BigQuantumsDelta: deleveragedSubaccountQuoteBalanceDelta,
			}},
			PerpetualUpdates: []satypes.PerpetualUpdate{{
				PerpetualId: perpetualId,
				BigQuantumsDelta: deltaBaseQuantums,  // 减少持仓
			}},
		},
		{
			// 对手方账户更新
			AssetUpdates: []satypes.AssetUpdate{{
				AssetId: assettypes.AssetUsdc.Id,
				BigQuantumsDelta: offsettingSubaccountQuoteBalanceDelta,  // 按破产价格结算
			}},
			PerpetualUpdates: []satypes.PerpetualUpdate{{
				PerpetualId: perpetualId,
				BigQuantumsDelta: new(big.Int).Neg(deltaBaseQuantums),  // 减少持仓
			}},
		},
	}
	
	return k.subaccountsKeeper.UpdateSubaccounts(ctx, updates, satypes.Match)
}
```

**关键点**：
- 对手方按**破产价格**结算，而不是市场价格
- 如果破产价格 < 市场价格，对手方承担损失（社会化损失）
- 如果破产价格 > 市场价格，对手方获得额外收益

#### 社会化损失计算示例

**场景**：做多账户被去杠杆，匹配做空对手方

假设：
- 被清算账户：做多 10 BTC，破产价格 = $48,500
- 对手方账户：做空 10 BTC，市场价格 = $50,000
- 保险基金余额：0 USDC（不足以覆盖）

去杠杆执行：
```
被清算账户：
- 减少持仓：-10 BTC
- 增加余额：+485,000 USDC（按破产价格）

对手方账户：
- 减少持仓：-10 BTC（做空减少 = 平仓）
- 减少余额：-485,000 USDC（按破产价格）

对手方损失计算：
- 如果按市场价格平仓，应获得：50,000 × 10 = 500,000 USDC
- 实际按破产价格获得：485,000 USDC
- 社会化损失：500,000 - 485,000 = 15,000 USDC
```

**结论**：对手方承担了 15,000 USDC 的社会化损失。

### 7.3 社会化损失的防护机制

#### 1. 保险基金优先

```go
// 保险基金优先覆盖损失
if insuranceFundDeltaQuoteQuantumsBig.Sign() <= 0 {
    // 保险基金支付
    return insuranceFundDeltaQuoteQuantumsBig, nil
}
```

#### 2. 破产价格重叠检查

在标准去杠杆中，系统会检查破产价格是否重叠：

```go
// protocol/x/clob/keeper/deleveraging.go:387
if err := k.ProcessDeleveraging(...); err != nil {
    // 如果破产价格不重叠，去杠杆失败
    // 这避免了对手方承担过大损失
}
```

#### 3. 限制每个账户的保险基金损失

```go
// protocol/x/clob/keeper/liquidations.go:927
func (k Keeper) GetSubaccountMaxInsuranceLost(
	ctx sdk.Context,
	subaccountId satypes.SubaccountId,
	perpetualId uint32,
) (*big.Int, error) {
	// 限制每个账户每个区块的最大保险基金损失
	bigInsuranceFundLostBlockLimit := new(big.Int).SetUint64(
		liquidationConfig.SubaccountBlockLimits.MaxQuantumsInsuranceLost,
	)
	// ...
}
```

---

## 8. 完整流程示例

### 8.1 清算流程示例

#### 场景设置

- **账户 A**：做多 10 BTC
- BTC 价格：$50,000
- 账户净值：$30,000
- 维持保证金要求：$50,000
- **触发清算**：30,000 < 50,000

#### 步骤 1：生成清算订单

```go
// 1. 检查是否可清算
risk := GetNetCollateralAndMarginRequirements(...)
// risk.NC = 30,000, risk.MMR = 50,000
// IsLiquidatable() = true

// 2. 选择持仓
perpetualId = 0  // BTC-PERP

// 3. 计算清算数量
// 假设清算 50%：5 BTC

// 4. 计算可成交价格
// FillablePrice = (500,000 - ABR * SMMR * 25,000) / 10
// 假设 = $49,500
```

#### 步骤 2：下单并匹配

```go
// 下卖单：5 BTC @ $49,500（IOC）
// 与订单簿买单匹配，假设成交价格 = $49,600
```

#### 步骤 3：计算保险基金变化

```go
// 1. 计算破产价格
// 假设破产价格 = $48,500
bankruptcyPrice = 48,500 × 5 = 242,500 USDC

// 2. 计算实际成交价值
actualValue = 49,600 × 5 = 248,000 USDC

// 3. 计算保险基金变化
insuranceFundDelta = 248,000 - 242,500 = 5,500 USDC（正值）

// 4. 计算清算费
maxLiquidationFee = 248,000 × 5% = 12,400 USDC
actualFee = min(5,500, 12,400) = 5,500 USDC
```

#### 步骤 4：更新账户状态

```
账户 A：
- 持仓：10 BTC → 5 BTC
- 余额：30,000 + 248,000 - 5,500 = 272,500 USDC
- 净值：重新计算，应该 >= 维持保证金要求

保险基金：
- 余额：+5,500 USDC（收到清算费）
```

### 8.2 去杠杆流程示例

#### 场景设置

- **账户 B**：做多 10 BTC
- BTC 价格：$50,000
- 账户净值：-$10,000（负净值）
- **触发去杠杆**：NetCollateral < 0

#### 步骤 1：查找对手方

```go
// 查找做空账户列表
subaccountsWithOpenPositions := GetSubaccountsWithOpenPositionsOnSide(
    perpetualId, isShort=true,
)
// 假设找到账户 C：做空 15 BTC
```

#### 步骤 2：计算去杠杆价格

```go
// 计算账户 B 的破产价格
bankruptcyPrice = GetBankruptcyPriceInQuoteQuantums(...)
// 假设 = $48,500
deltaQuoteQuantums = 48,500 × 10 = 485,000 USDC
```

#### 步骤 3：执行去杠杆

```go
// 匹配 10 BTC
ProcessDeleveraging(
    liquidatedSubaccountId = B,
    offsettingSubaccountId = C,
    deltaBaseQuantums = -10,  // 账户 B 减少 10 BTC
    deltaQuoteQuantums = 485,000,  // 按破产价格
)
```

#### 步骤 4：更新账户状态

```
账户 B（被清算）：
- 持仓：10 BTC → 0 BTC
- 余额：-10,000 + 485,000 = 475,000 USDC
- 净值：475,000（刚好为 0，破产价格的定义）

账户 C（对手方）：
- 持仓：-15 BTC → -5 BTC（做空减少 10 BTC = 平仓 10 BTC）
- 余额：原有余额 - 485,000 USDC
- 净值：如果市场价格是 $50,000，账户 C 承担了社会化损失
```

#### 步骤 5：社会化损失分析

```
如果市场价格 = $50,000：

账户 C 的损失：
- 按市场价格平仓应获得：50,000 × 10 = 500,000 USDC
- 实际按破产价格获得：485,000 USDC
- 社会化损失：500,000 - 485,000 = 15,000 USDC

如果保险基金有余额：
- 保险基金应支付：15,000 USDC
- 但如果保险基金余额不足，账户 C 承担全部损失
```

### 8.3 保险基金不足场景

#### 场景设置

- **账户 D**：做多 10 BTC，净值 = -$20,000
- 破产价格：$48,000
- 市场价格：$50,000
- 保险基金余额：$5,000

#### 去杠杆执行

```go
// 1. 计算所需保险基金
requiredInsuranceFund = (50,000 - 48,000) × 10 = 20,000 USDC

// 2. 检查保险基金余额
currentBalance = 5,000 USDC
if currentBalance < requiredInsuranceFund {
    // 保险基金不足，但去杠杆仍会执行
    // 对手方将承担部分损失
}

// 3. 执行去杠杆（按破产价格）
ProcessDeleveraging(..., deltaQuoteQuantums = 480,000 USDC)
```

#### 结果分析

```
账户 D（被清算）：
- 持仓：10 BTC → 0 BTC
- 余额：-20,000 + 480,000 = 460,000 USDC
- 净值：460,000（刚好为 0）

账户 E（对手方）：
- 持仓：-10 BTC → 0 BTC
- 余额：原有余额 - 480,000 USDC
- 按市场价格应获得：500,000 USDC
- 实际获得：480,000 USDC
- 损失：20,000 USDC

保险基金：
- 余额：5,000 USDC（全部支付）
- 仍不足：20,000 - 5,000 = 15,000 USDC

社会化损失：
- 账户 E 承担：15,000 USDC
- 保险基金承担：5,000 USDC
```

---

## 9. 代码调用链

### 9.1 清算调用链

```
PrepareCheckState
  └─ LiquidateSubaccountsAgainstOrderbook
      ├─ MaybeGetLiquidationOrder
      │   ├─ EnsureIsLiquidatable
      │   │   └─ IsLiquidatable
      │   │       └─ GetNetCollateralAndMarginRequirements
      │   ├─ GetPerpetualPositionToLiquidate
      │   └─ GetLiquidationOrderForPerpetual
      │       ├─ GetLiquidatablePositionSizeDelta
      │       │   └─ GetMaxAndMinPositionNotionalLiquidatable
      │       └─ GetFillablePrice
      │           └─ GetPositionNetNotionalValueAndMarginRequirements
      ├─ SortLiquidationOrders
      └─ PlacePerpetualLiquidation
          └─ MemClob.PlacePerpetualLiquidation
              └─ (匹配订单)
                  └─ ProcessMatches
                      └─ validateMatchedLiquidation
                          └─ GetLiquidationInsuranceFundDelta
                              └─ GetBankruptcyPriceInQuoteQuantums
```

### 9.2 去杠杆调用链

```
PrepareCheckState
  └─ MaybeDeleverageSubaccount
      ├─ CanDeleverageSubaccount
      │   └─ GetNetCollateralAndMarginRequirements
      └─ MemClob.DeleverageSubaccount
          └─ OffsetSubaccountPerpetualPosition
              ├─ getDeleveragingQuoteQuantumsDelta
              │   └─ GetBankruptcyPriceInQuoteQuantums（标准去杠杆）
              │   └─ getOraclePriceQuoteQuantumsDelta（最终结算）
              └─ ProcessDeleveraging
                  └─ UpdateSubaccounts
```

### 9.3 关键函数说明

#### IsLiquidatable

```go
// protocol/x/clob/keeper/liquidations.go:357
func (k Keeper) IsLiquidatable(ctx, subaccountId) (bool, error)
```

**功能**：检查账户是否可清算

**判断条件**：
```go
risk.IsLiquidatable()  // MMR > 0 && MMR > NC
```

#### GetBankruptcyPriceInQuoteQuantums

```go
// protocol/x/clob/keeper/liquidations.go:404
func (k Keeper) GetBankruptcyPriceInQuoteQuantums(
	ctx, subaccountId, perpetualId, deltaQuantums,
) (*big.Int, error)
```

**功能**：计算破产价格（报价数量）

**公式**：
```
破产价格 = -DNNV - (TNC * abs(DMMR) / TMMR)
```

#### GetFillablePrice

```go
// protocol/x/clob/keeper/liquidations.go:514
func (k Keeper) GetFillablePrice(
	ctx, subaccountId, perpetualId, deltaQuantums,
) (*big.Rat, error)
```

**功能**：计算清算订单的可成交价格

**公式**：
```
FillablePrice = (PNNV - ABR * SMMR * PMMR) / PS
```

#### GetLiquidationInsuranceFundDelta

```go
// protocol/x/clob/keeper/liquidations.go:656
func (k Keeper) GetLiquidationInsuranceFundDelta(
	ctx, subaccountId, perpetualId, isBuy, fillAmount, subticks,
) (*big.Int, error)
```

**功能**：计算保险基金变化

**公式**：
```
保险基金变化 = 实际成交价值 - 破产价格成交价值
```

#### ProcessDeleveraging

```go
// protocol/x/clob/keeper/deleveraging.go:502
func (k Keeper) ProcessDeleveraging(
	ctx, liquidatedSubaccountId, offsettingSubaccountId,
	perpetualId, deltaBaseQuantums, deltaQuoteQuantums,
) error
```

**功能**：执行去杠杆操作，更新两个账户的状态

**关键点**：
- 按破产价格结算
- 双方持仓同时减少
- 报价余额按破产价格调整

---

## 10. 总结

### 10.1 清算 vs 去杠杆

| 特性 | 清算（Liquidation） | 去杠杆（Deleveraging） |
|------|---------------------|------------------------|
| **触发条件** | MMR > NC | NC < 0 或最终结算 |
| **成交方式** | 订单簿匹配 | 系统强制匹配 |
| **成交价格** | 订单簿价格（Fillable Price） | 破产价格或预言机价格 |
| **对手方** | 订单簿上的自愿交易者 | 系统选择的对手方账户 |
| **费用** | 清算费（用户支付） | 无费用（可能承担社会化损失） |
| **保险基金** | 可能支付或收取 | 可能支付（如果破产价格 < 市场价格） |

### 10.2 关键公式总结

1. **清算触发**：`NetCollateral < MaintenanceMarginRequirement`
2. **可成交价格**：`FillablePrice = (PNNV - ABR * SMMR * PMMR) / PS`
3. **破产价格**：`BankruptcyPrice = -DNNV - (TNC * abs(DMMR) / TMMR)`
4. **保险基金变化**：`InsuranceFundDelta = 实际成交价值 - 破产价格成交价值`
5. **社会化损失**：`SocializedLoss = 市场价格成交价值 - 破产价格成交价值 - 保险基金支付`

### 10.3 风险控制机制

1. **保险基金优先**：优先使用保险基金覆盖损失
2. **破产价格重叠检查**：标准去杠杆需要破产价格重叠
3. **限制每个账户的损失**：限制每个账户每个区块的最大保险基金损失
4. **价格限制**：清算价格限制在合理范围内
5. **部分清算**：避免一次性清算全部持仓

---

## 附录：相关代码文件

- `protocol/x/clob/keeper/liquidations.go` - 清算核心逻辑
- `protocol/x/clob/keeper/deleveraging.go` - 去杠杆核心逻辑
- `protocol/x/clob/keeper/liquidations_state.go` - 清算状态管理
- `protocol/x/subaccounts/keeper/transfer.go` - 保险基金转账
- `protocol/x/perpetuals/keeper/perpetual.go` - 保险基金地址管理
- `protocol/lib/margin/risk.go` - 风险指标计算

---

*文档生成时间：2024年*
*基于代码版本：hermes protocol*

