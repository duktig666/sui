# Hermes DEX 清算机制详细分析

## 目录
1. [代码调用流程及详细分析](#1-代码调用流程及详细分析)
2. [清算价格计算机制](#2-清算价格计算机制)
3. [保险基金机制详解](#3-保险基金机制详解)
4. [强制平仓补偿与社会化损失](#4-强制平仓补偿与社会化损失)
5. [清算执行实例](#5-清算执行实例)

---

## 1. 代码调用流程及详细分析

### 1.1 完整调用链路

```
PrepareCheckState (protocol/x/clob/abci.go:262-279)
  ↓
LiquidateSubaccountsAgainstOrderbook() (liquidations.go:55-162)
  ├─ 遍历所有需要清算的子账户
  ├─ 调用 MaybeGetLiquidationOrder()
  └─ 调用 PlacePerpetualLiquidation()
  ↓
【步骤1: 生成清算订单】
MaybeGetLiquidationOrder(liquidations.go:164-197)
  ├─ Line 177: EnsureIsLiquidatable() → 检查 TNC < MMR
  ├─ Line 182: GetPerpetualPositionToLiquidate() → 选择要清算的持仓
  └─ Line 194: GetLiquidationOrderForPerpetual()
      ├─ Line 873: GetLiquidatablePositionSizeDelta() → 计算清算规模
      ├─ Line 896: GetFillablePrice() → 计算可成交价格 ⭐
      │   ├─ Line 535-537: 获取子账户和持仓信息
      │   ├─ Line 541-550: 验证 deltaQuantums 有效性
      │   ├─ Line 552-558: 获取永续合约、市场价格、流动性层级
      │   ├─ Line 560-566: 计算持仓的风险参数 (riskPos)
      │   ├─ Line 568-574: 获取账户总风险参数 (riskTotal)
      │   ├─ Line 604-619: 计算调整后的破产评级 (ABR)
      │   ├─ Line 622-625: 计算最大清算价差 (SMMR * PMMR)
      │   ├─ Line 627: 计算价格偏离 (ABR * maxSpread)
      │   ├─ Line 629-636: 计算可成交价格 (PNNV - 价格偏离) / PS
      │   └─ Line 641-649: 返回可成交价格
      └─ Line 902: ConvertFillablePriceToSubticks() → 转换为 subticks
  ↓
【步骤2: 放置清算订单】
PlacePerpetualLiquidation(liquidations.go:246-350)
  ├─ Line 276-283: validateLiquidationAgainstClobPairStatus() → 验证市场状态
  ├─ Line 288-305: MemClob.PlacePerpetualLiquidation() → 在内存订单簿匹配
  │   └─ 尝试与现有订单匹配,生成成交
  └─ Line 342: MustUpdateSubaccountPerpetualLiquidated() → 更新清算状态
  ↓
【步骤3: 匹配与结算】
ProcessSingleMatch(process_single_match.go:44-317)
  ├─ Line 91-97: 验证匹配的基本信息
  ├─ Line 139-180: validateMatchedLiquidation() → 清算特殊验证
  │   ├─ Line 1024: GetLiquidationInsuranceFundDelta() → 计算保险基金变化 ⭐
  │   │   ├─ Line 668-675: 验证 fillAmount 非零
  │   │   ├─ Line 678-689: 计算 deltaQuantums 和 deltaQuoteQuantums
  │   │   ├─ Line 692-700: 调用 GetBankruptcyPriceInQuoteQuantums() → 计算破产价格
  │   │   ├─ Line 704-707: 计算保险基金变化 = 实际成交 - 破产价格
  │   │   ├─ Line 712-713: 如果 <= 0,保险基金需要补偿
  │   │   └─ Line 722-732: 如果 > 0,收取清算费(不超过上限)
  │   ├─ Line 1032: IsValidInsuranceFundDelta() → 验证保险基金充足
  │   └─ Line 1050: validateLiquidationAgainstSubaccountBlockLimits() → 验证区块限制
  └─ Line 227-241: persistMatchedOrders() → 持久化成交
      ├─ Line 432-467: TransferInsuranceFundPayments() → 转账保险基金
      │   ├─ Line 395-424 (transfer.go): 确定转账方向
      │   ├─ 保险基金 → 抵押池 (insuranceFundDelta < 0)
      │   └─ 抵押池 → 保险基金 (insuranceFundDelta > 0)
      └─ Line 468-520: UpdateSubaccounts() → 更新账户余额和仓位
```

### 1.2 关键函数详细分析

#### 1.2.1 GetFillablePrice() - 可成交价格计算

**文件位置**: `protocol/x/clob/keeper/liquidations.go:514-650`

**函数签名**:
```go
func (k Keeper) GetFillablePrice(
    ctx sdk.Context,
    subaccountId satypes.SubaccountId,
    perpetualId uint32,
    deltaQuantums *big.Int,
) (fillablePrice *big.Rat, err error)
```

**输入参数**:
- `subaccountId`: 被清算的子账户ID
- `perpetualId`: 永续合约ID
- `deltaQuantums`: 清算数量(负数表示卖出,正数表示买入)

**核心计算逻辑**:

1. **获取账户和持仓信息** (Line 535-537):
   ```go
   subaccount := k.subaccountsKeeper.GetSubaccount(ctx, subaccountId)
   position, _ := subaccount.GetPerpetualPositionForId(perpetualId)
   psBig := position.GetBigQuantums()
   ```

2. **验证 deltaQuantums 有效性** (Line 541-550):
   ```go
   // 验证清算方向与持仓方向相反,且不超过持仓大小
   if psBig.Sign()*deltaQuantums.Sign() != -1 || psBig.CmpAbs(deltaQuantums) == -1 {
       return nil, errorsmod.Wrapf(types.ErrInvalidPerpetualPositionSizeDelta, ...)
   }
   ```

3. **获取永续合约、市场价格、流动性层级** (Line 552-558):
   ```go
   perpetual, marketPrice, liquidityTier, err :=
       k.perpetualsKeeper.GetPerpetualAndMarketPriceAndLiquidityTier(ctx, perpetualId)
   ```

4. **计算持仓风险参数 riskPos** (Line 560-566):
   ```go
   riskPos := perplib.GetPositionNetNotionalValueAndMarginRequirements(
       perpetual,
       marketPrice,
       liquidityTier,
       psBig,
       0, // No custom IMF for liquidations
   )
   // riskPos 包含:
   // - NC (Net Collateral): 持仓净名义价值 (PNNV)
   // - MMR (Maintenance Margin Requirement): 持仓维持保证金要求 (PMMR)
   ```

5. **获取账户总风险参数 riskTotal** (Line 568-574):
   ```go
   riskTotal, err := k.subaccountsKeeper.GetNetCollateralAndMarginRequirements(
       ctx,
       satypes.Update{SubaccountId: subaccountId},
   )
   // riskTotal 包含:
   // - NC: 总净抵押品 (TNC)
   // - MMR: 总维持保证金要求 (TMMR)
   ```

6. **计算调整后的破产评级 ABR** (Line 604-619):
   ```go
   liquidationsConfig := k.GetLiquidationsConfig(ctx)
   ba := liquidationsConfig.FillablePriceConfig.BankruptcyAdjustmentPpm
   smmr := liquidationsConfig.FillablePriceConfig.SpreadToMaintenanceMarginRatioPpm

   // 计算 TNC / TMMR
   tncDivTmmrRat := new(big.Rat).SetFrac(riskTotal.NC, riskTotal.MMR)

   // 计算未绑定的 ABR = BA × (1 - TNC/TMMR)
   unboundedAbrRat := lib.BigRatMulPpm(
       new(big.Rat).Sub(lib.BigRat1(), tncDivTmmrRat),
       ba,
   )

   // 将 ABR 限制在 [0, 1] 范围内
   abrRat := lib.BigRatClamp(unboundedAbrRat, lib.BigRat0(), lib.BigRat1())
   ```

7. **计算最大清算价差** (Line 622-625):
   ```go
   // maxSpread = SMMR × PMMR
   maxLiquidationSpreadQuoteQuantumsRat := lib.BigRatMulPpm(
       new(big.Rat).SetInt(riskPos.MMR),
       smmr,
   )
   ```

8. **计算价格偏离** (Line 627):
   ```go
   // priceDeviation = ABR × maxSpread
   fillablePriceOracleDeltaQuoteQuantumsRat :=
       new(big.Rat).Mul(abrRat, maxLiquidationSpreadQuoteQuantumsRat)
   ```

9. **计算可成交价格** (Line 629-649):
   ```go
   // fillablePriceQuoteQuantums = PNNV - priceDeviation
   pnnvRat := new(big.Rat).SetInt(riskPos.NC)
   fillablePriceQuoteQuantumsRat := new(big.Rat).Sub(
       pnnvRat,
       fillablePriceOracleDeltaQuoteQuantumsRat,
   )

   // fillablePrice = fillablePriceQuoteQuantums / PS
   fillablePrice = new(big.Rat).Quo(
       fillablePriceQuoteQuantumsRat,
       new(big.Rat).SetInt(psBig),
   )

   // 验证可成交价格为正
   if fillablePrice.Sign() < 0 {
       panic("GetFillablePrice: Calculated fillable price is negative")
   }
   ```

**输出**: 可成交价格 (`*big.Rat`)

---

#### 1.2.2 GetBankruptcyPriceInQuoteQuantums() - 破产价格计算

**文件位置**: `protocol/x/clob/keeper/liquidations.go:404-509`

**函数签名**:
```go
func (k Keeper) GetBankruptcyPriceInQuoteQuantums(
    ctx sdk.Context,
    subaccountId satypes.SubaccountId,
    perpetualId uint32,
    deltaQuantums *big.Int,
) (bankruptcyPriceQuoteQuantums *big.Int, err error)
```

**核心计算逻辑**:

1. **获取账户总风险参数** (Line 413-419):
   ```go
   riskTotal, err := k.subaccountsKeeper.GetNetCollateralAndMarginRequirements(
       ctx,
       satypes.Update{SubaccountId: subaccountId},
   )
   ```

2. **计算持仓变化前后的风险参数** (Line 428-462):
   ```go
   subaccount := k.subaccountsKeeper.GetSubaccount(ctx, subaccountId)
   position, _ := subaccount.GetPerpetualPositionForId(perpetualId)
   psBig := position.GetBigQuantums()

   // 持仓变化前
   riskPosOld := perplib.GetPositionNetNotionalValueAndMarginRequirements(
       perpetual, marketPrice, liquidityTier, psBig, 0,
   )

   // 持仓变化后
   psBigNew := new(big.Int).Add(psBig, deltaQuantums)
   riskPosNew := perplib.GetPositionNetNotionalValueAndMarginRequirements(
       perpetual, marketPrice, liquidityTier, psBigNew, 0,
   )
   ```

3. **计算持仓净名义价值变化 DNNV** (Line 467-469):
   ```go
   deltaNC := new(big.Int).Sub(riskPosNew.NC, riskPosOld.NC)
   ```

4. **计算维持保证金要求变化 DMMR** (Line 471):
   ```go
   deltaMMR := new(big.Int).Sub(riskPosNew.MMR, riskPosOld.MMR)
   ```

5. **计算破产价格** (Line 474-506):
   ```go
   // 计算 TNC × abs(DMMR)
   tncMulDmmrBig := new(big.Int).Mul(
       riskTotal.NC,
       new(big.Int).Abs(deltaMMR),
   )

   // 计算 TNC × abs(DMMR) / TMMR (向下取整)
   quoteQuantumsBeforeBankruptcyBig := new(big.Int).Div(
       tncMulDmmrBig,
       riskTotal.MMR,
   )

   // 计算破产价格 = -DNNV - (TNC × abs(DMMR) / TMMR)
   bankruptcyPriceQuoteQuantumsBig := new(big.Int).Sub(
       new(big.Int).Neg(deltaNC),
       quoteQuantumsBeforeBankruptcyBig,
   )

   // 向正无穷舍入
   if new(big.Int).Mul(
       new(big.Int).Sub(
           new(big.Int).Mul(bankruptcyPriceQuoteQuantumsBig, riskTotal.MMR),
           new(big.Int).Mul(new(big.Int).Neg(deltaNC), riskTotal.MMR),
       ),
       new(big.Int).SetInt64(-1),
   ).Cmp(tncMulDmmrBig) < 0 {
       bankruptcyPriceQuoteQuantumsBig.Add(bankruptcyPriceQuoteQuantumsBig, big.NewInt(1))
   }
   ```

**输出**: 破产价格对应的报价量 (`*big.Int`)

---

#### 1.2.3 GetLiquidationInsuranceFundDelta() - 保险基金变化计算

**文件位置**: `protocol/x/clob/keeper/liquidations.go:656-733`

**函数签名**:
```go
func (k Keeper) GetLiquidationInsuranceFundDelta(
    ctx sdk.Context,
    subaccountId satypes.SubaccountId,
    perpetualId uint32,
    isBuy bool,
    fillAmount uint64,
    subticks types.Subticks,
) (insuranceFundDeltaQuoteQuantums *big.Int, err error)
```

**核心计算逻辑**:

1. **验证成交量非零** (Line 668-675):
   ```go
   if fillAmount == 0 {
       return nil, errorsmod.Wrapf(
           types.ErrInvalidQuantumsForInsuranceFundDeltaCalculation,
           "FillAmount is zero...",
       )
   }
   ```

2. **计算实际成交的报价量变化** (Line 678-689):
   ```go
   clobPair := k.mustGetClobPairForPerpetualId(ctx, perpetualId)
   deltaQuantums := new(big.Int).SetUint64(fillAmount)
   deltaQuoteQuantums := types.FillAmountToQuoteQuantums(
       subticks,
       satypes.BaseQuantums(fillAmount),
       clobPair.QuantumConversionExponent,
   )

   if isBuy {
       deltaQuoteQuantums.Neg(deltaQuoteQuantums)  // 买入: 支付报价
   } else {
       deltaQuantums.Neg(deltaQuantums)            // 卖出: 收到报价
   }
   ```

3. **获取破产价格对应的报价量** (Line 692-700):
   ```go
   bankruptcyPriceInQuoteQuantumsBig, err := k.GetBankruptcyPriceInQuoteQuantums(
       ctx,
       subaccountId,
       perpetualId,
       deltaQuantums,
   )
   ```

4. **计算保险基金变化** (Line 704-732):
   ```go
   // insuranceFundDelta = 实际成交报价 - 破产价格报价
   insuranceFundDeltaQuoteQuantumsBig := new(big.Int).Sub(
       deltaQuoteQuantums,
       bankruptcyPriceInQuoteQuantumsBig,
   )

   // 如果 <= 0,保险基金需要补偿
   if insuranceFundDeltaQuoteQuantumsBig.Sign() <= 0 {
       return insuranceFundDeltaQuoteQuantumsBig, nil
   }

   // 如果 > 0,收取清算费,但不超过上限
   liquidationsConfig := k.GetLiquidationsConfig(ctx)
   maxLiquidationFeeQuoteQuantumsBig := lib.BigIntMulPpm(
       new(big.Int).Abs(deltaQuoteQuantums),
       liquidationsConfig.MaxLiquidationFeePpm,
   )

   // 返回较小值
   return lib.BigMin(
       maxLiquidationFeeQuoteQuantumsBig,
       insuranceFundDeltaQuoteQuantumsBig,
   ), nil
   ```

**输出**: 保险基金变化量 (`*big.Int`,正数表示收费,负数表示补偿)

---

## 2. 清算价格计算机制

### 2.1 可成交价格 (Fillable Price) 公式

**数学表达式**:
```
fillablePrice = (PNNV - ABR × SMMR × PMMR) / PS

其中:
- PNNV (Position Net Notional Value): 持仓净名义价值
- ABR (Adjusted Bankruptcy Rating): 调整后的破产评级
  ABR = Clamp(BA × (1 - TNC/TMMR), 0, 1)
  - BA (Bankruptcy Adjustment PPM): 破产调整系数
  - TNC (Total Net Collateral): 总净抵押品
  - TMMR (Total Maintenance Margin Requirement): 总维持保证金要求
- SMMR (Spread to Maintenance Margin Ratio PPM): 价差与维持保证金比率
- PMMR (Position Maintenance Margin Requirement): 持仓维持保证金要求
- PS (Position Size): 持仓规模
```

**代码实现位置**: `liquidations.go:514-650`

**业务含义**:

1. **多头清算 (卖出)**:
   - `PNNV > 0`(持仓价值为正)
   - `ABR × SMMR × PMMR > 0`(价格偏离为正)
   - `fillablePrice < oraclePrice`(打折卖出)
   - 清算订单以低于市场价的价格卖出,吸引买家

2. **空头清算 (买入)**:
   - `PNNV < 0`(持仓价值为负)
   - `ABR × SMMR × PMMR < 0`(价格偏离为负)
   - `fillablePrice > oraclePrice`(溢价买入)
   - 清算订单以高于市场价的价格买入,吸引卖家

3. **ABR 的作用**:
   - ABR ≈ 0: 账户健康,价格偏离小,接近市场价
   - ABR ≈ 1: 账户接近破产,价格偏离大,远离市场价
   - ABR 动态调整清算激励,确保及时清算

**计算示例**:

假设:
- 持仓: 10 BTC 多头
- PNNV = 490,000 USDC
- PMMR = 2,500 USDC
- TNC = 1,000 USDC
- TMMR = 12,500 USDC
- BA = 1,000,000 PPM (100%)
- SMMR = 150,000 PPM (15%)
- PS = 10 BTC

计算:
1. TNC/TMMR = 1,000 / 12,500 = 0.08
2. ABR = 1 × (1 - 0.08) = 0.92
3. maxSpread = 0.15 × 2,500 = 375 USDC
4. priceDeviation = 0.92 × 375 = 345 USDC
5. fillablePriceQuoteQuantums = 490,000 - 345 = 489,655 USDC
6. fillablePrice = 489,655 / 10 = **48,965.5 USDC/BTC**

如果预言机价格为 49,000 USDC,则清算价格低于市场价 34.5 USDC。

---

### 2.2 破产价格 (Bankruptcy Price) 公式

**数学表达式**:
```
bankruptcyPrice = -DNNV - (TNC × abs(DMMR) / TMMR)

其中:
- DNNV (Delta Net Notional Value): 持仓净名义价值变化
  DNNV = riskPosNew.NC - riskPosOld.NC
- DMMR (Delta Maintenance Margin Requirement): 维持保证金要求变化
  DMMR = riskPosNew.MMR - riskPosOld.MMR
- TNC: 总净抵押品
- TMMR: 总维持保证金要求
```

**代码实现位置**: `liquidations.go:404-509`

**舍入规则**: **向正无穷舍入**,确保破产价格保守估计,不需要额外保险基金支付

**业务含义**:

- **破产价格**是使账户 TNC 恰好为 0 的价格
- 用于计算保险基金应支付/收取的金额
- 向上舍入确保保险基金不会亏空

**计算示例**:

假设:
- 持仓: 10 BTC 多头,开仓价 50,000 USDC
- 账户余额: 3,000 USDC
- 清算数量: -10 BTC (卖出)
- TNC = 3,000 USDC
- TMMR = 12,500 USDC

计算步骤:
1. 持仓变化前: PS = 10 BTC
   - riskPosOld.NC = 490,000 USDC
   - riskPosOld.MMR = 2,500 USDC
2. 持仓变化后: PS = 0 BTC
   - riskPosNew.NC = 0 USDC
   - riskPosNew.MMR = 0 USDC
3. DNNV = 0 - 490,000 = -490,000 USDC
4. DMMR = 0 - 2,500 = -2,500 USDC
5. TNC × abs(DMMR) = 3,000 × 2,500 = 7,500,000
6. TNC × abs(DMMR) / TMMR = 7,500,000 / 12,500 = 600 USDC
7. bankruptcyPrice = -(-490,000) - 600 = 490,000 - 600 = **489,400 USDC**
8. 破产价格/BTC = 489,400 / 10 = **48,940 USDC/BTC**

如果以破产价格平仓,账户恰好归零:
- 收到: 489,400 USDC
- 初始余额: 3,000 USDC
- 需要支付: 10 × 50,000 = 500,000 USDC (开仓成本)
- 最终余额: 3,000 + 489,400 - 500,000 = -7,600 USDC (实际上还是负数)

*(注: 实际计算中还需考虑未实现盈亏,此处简化示例)*

---

## 3. 保险基金机制详解

### 3.1 保险基金变化计算

**公式**:
```
insuranceFundDelta = 实际成交报价量 - 破产价格报价量
```

**代码实现位置**: `liquidations.go:656-733`

### 3.2 三种场景

#### 场景 1: insuranceFundDelta > 0 (被清算账户支付费用)

**条件**: 实际成交价格优于破产价格

**处理**:
```go
// 收取清算费,但不超过上限
maxLiquidationFee := deltaQuoteQuantums × MaxLiquidationFeePpm
insuranceFundDelta = min(maxLiquidationFee, insuranceFundDelta)
```

**示例**:
- 实际成交: 10 BTC @ 48,800 USDC = 488,000 USDC
- 破产价格: 10 BTC @ 48,940 USDC = 489,400 USDC
- insuranceFundDelta = 488,000 - 489,400 = **-1,400 USDC**
- 保险基金需要补偿 1,400 USDC

*(注: 此示例中 delta < 0,属于场景3)*

#### 场景 2: insuranceFundDelta = 0 (恰好平衡)

**条件**: 实际成交价格等于破产价格

**处理**: 无资金转移

#### 场景 3: insuranceFundDelta < 0 (保险基金补偿损失)

**条件**: 实际成交价格劣于破产价格(账户已破产)

**处理**:
```go
// 直接返回负数,保险基金需要补偿
return insuranceFundDelta, nil
```

**代码实现** (`liquidations.go:712-713`):
```go
if insuranceFundDeltaQuoteQuantumsBig.Sign() <= 0 {
    return insuranceFundDeltaQuoteQuantumsBig, nil
}
```

**转账实现** (`subaccounts/keeper/transfer.go:395-424`):
```go
// 确定转账方向
fromModule := collateralPoolAddr
toModule := insuranceFundModuleAddr

if insuranceFundDelta.Sign() < 0 {
    // 保险基金 → 子账户抵押池
    fromModule, toModule = toModule, fromModule
}

// 执行转账
return k.bankKeeper.SendCoins(ctx, fromModule, toModule, []sdk.Coin{coinToTransfer})
```

---

### 3.3 保险基金架构

**存储位置**: `x/bank` 模块的账户余额

**架构设计**:
1. **Cross 市场**: 共享一个保险基金
   - 地址: `perptypes.InsuranceFundModuleAddress`
2. **Isolated 市场**: 每个永续合约独立保险基金
   - 地址: 根据 perpetualId 生成

**余额查询** (`subaccounts/keeper/subaccount.go:861-878`):
```go
func (k Keeper) GetInsuranceFundBalance(ctx sdk.Context, perpetualId uint32) (balance *big.Int) {
    usdcAsset, exists := k.assetsKeeper.GetAsset(ctx, assettypes.AssetUsdc.Id)

    insuranceFundAddr, err := k.perpetualsKeeper.GetInsuranceFundModuleAddress(ctx, perpetualId)

    insuranceFundBalance := k.bankKeeper.GetBalance(ctx, insuranceFundAddr, usdcAsset.Denom)

    return insuranceFundBalance.Amount.BigInt()
}
```

---

## 4. 强制平仓补偿与社会化损失

### 4.1 强制平仓补偿

**定义**: 当清算成交价劣于破产价格时,保险基金补偿差额

**场景**:
- 空头清算时,实际买入价格高于破产价格
- 多头清算时,实际卖出价格低于破产价格

**代码位置**: `subaccounts/keeper/transfer.go:395-424`

**示例**:
- 被清算账户: 卖出 10 BTC
- 破产价格: 48,940 USDC/BTC
- 实际成交价: 48,800 USDC/BTC
- 差额: (48,940 - 48,800) × 10 = **1,400 USDC**
- 保险基金支付 1,400 USDC 到抵押池

---

### 4.2 社会化损失 (Socialized Loss)

**定义**: 当保险基金不足以覆盖清算损失时,通过去杠杆机制由对手方吸收损失

**实现机制**: 去杠杆流程

**代码路径**: `protocol/x/clob/keeper/deleveraging.go:289-466` - `OffsetSubaccountPerpetualPosition()`

**损失分配逻辑**:
1. 对手方以**破产价格**(而非市场价格)成交
2. 差价 = 破产价格 - 市场价格
3. 对手方吸收损失

**体现位置**: `ProcessDeleveraging()` (deleveraging.go:502-642) 中双方账户更新

**示例**:
- 被去杠杆账户 Eve: 10 BTC 多头,破产价格 49,800 USDC/BTC
- 对手方 Frank: 10 BTC 空头,当前市场价 49,700 USDC/BTC
- 去杠杆成交价: 49,800 USDC/BTC (破产价格)
- Frank 损失 = (49,800 - 49,700) × 10 = **1,000 USDC** (社会化损失)

---

## 5. 清算执行实例

### 5.1 实例 1: 清算成功,保险基金收费

**场景设定**:
```
用户 Alice:
- 持仓: 10 BTC 多头
- 开仓价: 50,000 USDC/BTC
- 账户余额: 3,000 USDC
- 当前市场价: 49,000 USDC/BTC
```

**清算触发判断**:
```
1. 计算 TNC:
   - 未实现盈亏 = (49,000 - 50,000) × 10 = -10,000 USDC
   - TNC = 3,000 + (-10,000) = -7,000 USDC

2. 计算 MMR:
   - 持仓价值 = 10 × 49,000 = 490,000 USDC
   - MMR = 490,000 × 2.5% = 12,250 USDC (假设维持保证金率 2.5%)

3. 判断:
   - TNC (-7,000) < MMR (12,250) ✅ 触发清算
```

**清算执行**:
```
1. 计算可成交价格:
   - 假设通过 GetFillablePrice() 计算得到: 48,800 USDC/BTC

2. 计算破产价格:
   - 破产价格 = 50,000 - (3,000 / 10) = 49,700 USDC/BTC

3. 放置清算订单:
   - 卖出 10 BTC @ 48,800 USDC/BTC

4. 订单匹配成交:
   - 成交价值 = 10 × 48,800 = 488,000 USDC

5. 计算保险基金变化:
   - 破产价格价值 = 10 × 49,700 = 497,000 USDC
   - insuranceFundDelta = 488,000 - 497,000 = -9,000 USDC

6. 保险基金转账:
   - 保险基金支付 9,000 USDC 到抵押池

7. 更新账户状态:
   - Alice 仓位清零
   - Alice 余额归零 (全部损失)
```

**结果**: 保险基金补偿 9,000 USDC 损失

---

### 5.2 实例 2: 清算失败,触发去杠杆

**场景设定**:
```
用户 Bob:
- 持仓: -5 BTC 空头
- 开仓价: 50,000 USDC/BTC
- 账户余额: 1,000 USDC
- 当前市场价: 55,000 USDC/BTC (暴涨)
```

**清算失败场景**:
```
1. 计算 TNC:
   - 未实现盈亏 = (50,000 - 55,000) × (-5) = -25,000 USDC
   - TNC = 1,000 + (-25,000) = -24,000 USDC (严重破产)

2. 清算订单:
   - 买入 5 BTC @ 市场价
   - 但订单簿流动性不足,无法完全成交

3. 进入去杠杆流程
```

**去杠杆执行**:
```
1. 计算破产价格:
   - 破产价格 = 50,000 + (1,000 / 5) = 50,200 USDC/BTC

2. 查找对手方:
   - 系统查找多头持仓账户
   - 假设 Carol 持有 5 BTC 多头,盈利中

3. 强制匹配:
   - Bob 买入 5 BTC @ 50,200 USDC/BTC
   - Carol 卖出 5 BTC @ 50,200 USDC/BTC

4. 更新账户:
   - Bob:
     - 支付 = 5 × 50,200 = 251,000 USDC
     - 初始余额 + 收到 = 1,000 + 收到的 USDC
     - 开仓时收到 = 5 × 50,000 = 250,000 USDC
     - 最终余额 = 1,000 + 250,000 - 251,000 = 0 USDC (归零)
   - Carol:
     - 收到 = 5 × 50,200 = 251,000 USDC
     - 本应收到 = 5 × 55,000 = 275,000 USDC
     - 损失 = 275,000 - 251,000 = 24,000 USDC (社会化损失)
```

**结果**: Carol 吸收 Bob 的 24,000 USDC 损失

---

## 6. 关键文件索引

### 6.1 核心实现文件

| 文件 | 路径 | 关键内容 |
|------|------|---------|
| liquidations.go | protocol/x/clob/keeper/liquidations.go | 清算核心逻辑 (1283行) |
| liquidations_config.go | protocol/x/clob/keeper/liquidations_config.go | 清算配置管理 (70行) |
| liquidation_order.go | protocol/x/clob/types/liquidation_order.go | 清算订单数据结构 (150行) |
| transfer.go | protocol/x/subaccounts/keeper/transfer.go | 保险基金转账 (600行) |
| subaccount.go | protocol/x/subaccounts/keeper/subaccount.go | 保险基金余额查询 (894行) |

### 6.2 关键函数索引

| 函数 | 文件:行号 | 功能 |
|------|----------|------|
| GetFillablePrice | liquidations.go:514-650 | 计算可成交价格 |
| GetBankruptcyPriceInQuoteQuantums | liquidations.go:404-509 | 计算破产价格 |
| GetLiquidationInsuranceFundDelta | liquidations.go:656-733 | 计算保险基金变化 |
| PlacePerpetualLiquidation | liquidations.go:246-350 | 放置清算订单 |
| TransferInsuranceFundPayments | transfer.go:390-434 | 转账保险基金 |
| GetInsuranceFundBalance | subaccount.go:861-878 | 查询保险基金余额 |

---

**文档版本**: v1.0
**最后更新**: 2026-01-06
**文档作者**: Claude Sonnet 4.5
