# 去杠杆机制 (Deleveraging Mechanism) 详解

## 概述

去杠杆 (Deleveraging) 是 Hermes DEX 的高级风险管理机制,用于处理两种极端场景:
1. **负净抵押品 (Negative TNC)**: 账户资不抵债,清算无法覆盖损失
2. **最终结算 (Final Settlement)**: 市场永久关闭,所有持仓必须强制平仓

去杠杆通过**强制匹配反向持仓的账户**来平仓,按特定价格结算,确保系统整体安全。

---

## 1. 代码调用流程及详细分析

### 1.1 完整调用链路

```
PrepareCheckState (每个区块)
    ↓
clob/keeper/prepare_check_state.go:PrepareCheckStateWithClobPairId()
    ├─ 步骤1: 处理清算 (LiquidateSubaccountsAgainstOrderbook)
    ├─ 步骤2: 处理未完成清算的去杠杆
    │   ↓
    │   keeper/deleveraging.go:DeleverageSubaccounts()
    │       ├─ 遍历未完成清算的账户
    │       └─ 调用 MaybeDeleverageSubaccount()
    │           ↓
    │           keeper/deleveraging.go:MaybeDeleverageSubaccount()
    │               ├─ 调用 CanDeleverageSubaccount() 检查触发条件
    │               ├─ 获取账户完整持仓
    │               └─ 调用 MemClob.DeleverageSubaccount()
    │                   ↓
    │                   memclob/memclob.go:DeleverageSubaccount()
    │                       ├─ 调用 OffsetSubaccountPerpetualPosition() 查找对手方
    │                       └─ 生成 MatchPerpetualDeleveraging 操作
    │
    └─ 步骤3: 处理最终结算市场的去杠杆
        ↓
        keeper/deleveraging.go:GetSubaccountsWithPositionsInFinalSettlementMarkets()
            ├─ 遍历所有 CLOB 交易对
            ├─ 查找状态为 FINAL_SETTLEMENT 的市场
            └─ 返回持仓账户列表
        ↓
        keeper/deleveraging.go:DeleverageSubaccounts()
            └─ 同上流程

FinalizeBlock (执行去杠杆成交)
    ↓
DeliverTx(MsgProposedOperations)
    ↓
keeper/process_operations.go:ProcessProposerMatches()
    ├─ 遍历 Operations
    └─ 处理 Operation_DeleveragingMatch
        ↓
        keeper/deleveraging.go:ProcessDeleveraging()
            ├─ 调用 getDeleveragingQuoteQuantumsDelta() 计算价格
            ├─ 生成双方账户更新 (Update)
            └─ 调用 subaccountsKeeper.UpdateSubaccounts()
```

### 1.2 关键函数详细分析

#### 函数 1: `CanDeleverageSubaccount()` - 检查去杠杆触发条件

**位置**: `protocol/x/clob/keeper/deleveraging.go:151-195`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `subaccountId satypes.SubaccountId`: 子账户 ID
- `perpetualId uint32`: 永续合约 ID

**输出**:
- `shouldDeleverageAtBankruptcyPrice bool`: 是否以破产价格去杠杆
- `shouldDeleverageAtOraclePrice bool`: 是否以预言机价格去杠杆
- `err error`: 错误信息

**核心逻辑**:

```go
// 步骤1: 获取账户净抵押品和保证金要求
risk, err := k.subaccountsKeeper.GetNetCollateralAndMarginRequirements(
    ctx,
    satypes.Update{SubaccountId: subaccountId},
)

// 步骤2: 检查净抵押品是否为负
if risk.NC.Sign() == -1 {
    // 情况1: 负TNC → 以破产价格去杠杆
    return true, false, nil
}

// 步骤3: 如果NC >= 0,检查市场是否在最终结算状态
clobPairId, err := k.GetClobPairIdForPerpetual(ctx, perpetualId)
clobPair := k.mustGetClobPair(ctx, clobPairId)

// 步骤4: 判断市场状态
if clobPair.Status == types.ClobPair_STATUS_FINAL_SETTLEMENT {
    // 情况2: 非负TNC + 最终结算 → 以预言机价格去杠杆
    return false, true, nil
}

// 情况3: 非负TNC + 正常市场 → 不需要去杠杆
return false, false, nil
```

**业务解释**:
- **负 TNC**: 账户已破产,使用破产价格确保被去杠杆账户归零
- **最终结算**: 市场关闭,使用预言机价格公平结算所有账户

---

#### 函数 2: `OffsetSubaccountPerpetualPosition()` - 查找对手方并匹配

**位置**: `protocol/x/clob/keeper/deleveraging.go:295-466`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `liquidatedSubaccountId satypes.SubaccountId`: 被去杠杆账户 ID
- `perpetualId uint32`: 永续合约 ID
- `deltaQuantumsTotal *big.Int`: 需要平仓的总数量 (持仓的相反数)
- `isFinalSettlement bool`: 是否为最终结算

**输出**:
- `fills []types.MatchPerpetualDeleveraging_Fill`: 去杠杆成交列表
- `deltaQuantumsRemaining *big.Int`: 未能去杠杆的剩余数量

**核心逻辑**:

```go
// 步骤1: 查找反向持仓的账户
isDeleveragingLong := deltaQuantumsTotal.Sign() == -1
subaccountsWithOpenPositions := k.DaemonLiquidationInfo.GetSubaccountsWithOpenPositionsOnSide(
    perpetualId,
    !isDeleveragingLong, // 查找相反方向
)

// 步骤2: 从随机位置开始遍历 (防止偏向性)
pseudoRand := k.GetPseudoRand(ctx)
indexOffset := pseudoRand.Intn(numSubaccounts)

// 步骤3: 迭代对手方账户,尝试匹配
for i := 0; i < numSubaccountsToIterate && deltaQuantumsRemaining.Sign() != 0; i++ {
    index := (i + indexOffset) % numSubaccounts
    subaccountId := subaccountsWithOpenPositions[index]

    offsettingSubaccount := k.subaccountsKeeper.GetSubaccount(ctx, subaccountId)
    offsettingPosition, _ := offsettingSubaccount.GetPerpetualPositionForId(perpetualId)

    // 确定本次去杠杆数量 (取较小值)
    var deltaBaseQuantums *big.Int
    if deltaQuantumsRemaining.CmpAbs(offsettingPosition) > 0 {
        deltaBaseQuantums = offsettingPosition // 对手方全部平仓
    } else {
        deltaBaseQuantums = deltaQuantumsRemaining // 只需部分平仓
    }

    // 步骤4: 计算去杠杆价格 (破产价格或预言机价格)
    deltaQuoteQuantums, err := k.getDeleveragingQuoteQuantumsDelta(
        ctx, perpetualId, liquidatedSubaccountId, deltaBaseQuantums, isFinalSettlement,
    )

    // 步骤5: 执行去杠杆
    if err := k.ProcessDeleveraging(...); err == nil {
        deltaQuantumsRemaining.Sub(deltaQuantumsRemaining, deltaBaseQuantums)
        fills = append(fills, ...)
    }
}
```

**业务解释**:
- **对手方选择**: 从随机位置开始遍历,避免总是选择相同账户
- **数量确定**: 取被去杠杆账户剩余量和对手方持仓量的较小值
- **部分成功**: 即使无法完全去杠杆,部分成功的成交也会被保留

---

#### 函数 3: `getDeleveragingQuoteQuantumsDelta()` - 计算去杠杆价格

**位置**: `protocol/x/clob/keeper/deleveraging.go:471-490`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `perpetualId uint32`: 永续合约 ID
- `subaccountId satypes.SubaccountId`: 被去杠杆账户 ID
- `deltaQuantums *big.Int`: 去杠杆数量
- `isFinalSettlement bool`: 是否为最终结算

**输出**:
- `deltaQuoteQuantums *big.Int`: 去杠杆的报价金额变动
- `err error`: 错误信息

**核心逻辑**:

```go
// 情况1: 最终结算 → 使用预言机价格
if isFinalSettlement {
    return k.perpetualsKeeper.GetNetNotional(
        ctx,
        perpetualId,
        new(big.Int).Neg(deltaQuantums), // 取相反数
    )
}

// 情况2: 负TNC去杠杆 → 使用破产价格
return k.GetBankruptcyPriceInQuoteQuantums(
    ctx,
    subaccountId,
    perpetualId,
    deltaQuantums,
)
```

**业务解释**:
- **最终结算**: 所有账户公平按预言机价格结算
- **负 TNC**: 按破产价格结算,确保被去杠杆账户归零

---

#### 函数 4: `ProcessDeleveraging()` - 执行去杠杆成交并更新账户

**位置**: `protocol/x/clob/keeper/deleveraging.go:502-642`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `liquidatedSubaccountId satypes.SubaccountId`: 被去杠杆账户 ID
- `offsettingSubaccountId satypes.SubaccountId`: 对手方账户 ID
- `perpetualId uint32`: 永续合约 ID
- `deltaBaseQuantums *big.Int`: 去杠杆数量
- `deltaQuoteQuantums *big.Int`: 去杠杆报价金额

**输出**:
- `err error`: 错误信息

**核心逻辑**:

```go
// 步骤1: 获取双方账户和持仓
liquidatedSubaccount := k.subaccountsKeeper.GetSubaccount(ctx, liquidatedSubaccountId)
offsettingSubaccount := k.subaccountsKeeper.GetSubaccount(ctx, offsettingSubaccountId)

// 步骤2: 验证 deltaQuantums 的有效性
// - deltaQuantums 必须与被去杠杆账户持仓方向相反
// - deltaQuantums 必须与对手方持仓方向相同
// - deltaQuantums 数量不能超过双方持仓
if liquidatedPositionQuantums.Sign()*deltaBaseQuantums.Sign() != -1 ||
   liquidatedPositionQuantums.CmpAbs(deltaBaseQuantums) == -1 ||
   offsettingPositionQuantums.Sign()*deltaBaseQuantums.Sign() != 1 ||
   offsettingPositionQuantums.CmpAbs(deltaBaseQuantums) == -1 {
    return ErrInvalidPerpetualPositionSizeDelta
}

// 步骤3: 计算双方账户变动
deleveragedSubaccountQuoteBalanceDelta := deltaQuoteQuantums
offsettingSubaccountQuoteBalanceDelta := new(big.Int).Neg(deltaQuoteQuantums)
deleveragedSubaccountPerpetualQuantumsDelta := deltaBaseQuantums
offsettingSubaccountPerpetualQuantumsDelta := new(big.Int).Neg(deltaBaseQuantums)

// 步骤4: 构建账户更新
updates := []satypes.Update{
    {
        AssetUpdates: []satypes.AssetUpdate{
            {AssetId: USDC, BigQuantumsDelta: deleveragedSubaccountQuoteBalanceDelta},
        },
        PerpetualUpdates: []satypes.PerpetualUpdate{
            {PerpetualId: perpetualId, BigQuantumsDelta: deleveragedSubaccountPerpetualQuantumsDelta},
        },
        SubaccountId: liquidatedSubaccountId,
    },
    {
        AssetUpdates: []satypes.AssetUpdate{
            {AssetId: USDC, BigQuantumsDelta: offsettingSubaccountQuoteBalanceDelta},
        },
        PerpetualUpdates: []satypes.PerpetualUpdate{
            {PerpetualId: perpetualId, BigQuantumsDelta: offsettingSubaccountPerpetualQuantumsDelta},
        },
        SubaccountId: offsettingSubaccountId,
    },
}

// 步骤5: 应用更新
success, successPerUpdate, err := k.subaccountsKeeper.UpdateSubaccounts(ctx, updates, satypes.Match)
if err != nil || !success {
    return err
}

// 步骤6: 发出去杠杆事件
ctx.EventManager().EmitEvent(
    types.NewCreateMatchEvent(..., IsDeleverage: true),
)
```

**业务解释**:
- **双向更新**: 被去杠杆账户和对手方同时更新余额和持仓
- **原子性**: 要么双方都更新成功,要么都失败回滚
- **事件记录**: 发出链上事件,供索引器和前端使用

---

### 1.3 提现门控机制 (Withdrawal Gating)

#### 函数: `GateWithdrawalsIfNegativeTncSubaccountSeen()` - 负 TNC 时冻结提现

**位置**: `protocol/x/clob/keeper/deleveraging.go:200-260`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `negativeTncSubaccountIds []satypes.SubaccountId`: 疑似负 TNC 的账户列表

**输出**:
- `err error`: 错误信息

**核心逻辑**:

```go
// 步骤1: 验证账户是否真的负 TNC
for _, subaccountId := range negativeTncSubaccountIds {
    risk, err := k.subaccountsKeeper.GetNetCollateralAndMarginRequirements(ctx, ...)
    if risk.NC.Sign() == -1 {
        foundNegativeTncSubaccount = true
        break
    }
}

// 步骤2: 如果发现负 TNC,插入零成交去杠杆操作
if foundNegativeTncSubaccount {
    subaccount := k.subaccountsKeeper.GetSubaccount(ctx, negativeTncSubaccountId)
    perpetualId := subaccount.PerpetualPositions[0].PerpetualId

    // 插入零成交去杠杆操作到队列
    k.MemClob.InsertZeroFillDeleveragingIntoOperationsQueue(
        negativeTncSubaccountId,
        perpetualId,
    )
}
```

**业务解释**:
- **零成交去杠杆**: 不实际执行去杠杆,只是标记有负 TNC 账户存在
- **提现检测**: 提现逻辑会检测到队列中的零成交去杠杆操作,拒绝或暂停提现
- **风险隔离**: 防止负 TNC 账户提现,避免损失扩大

---

## 2. 触发条件分析

### 2.1 触发条件 1: 负净抵押品 (Negative TNC)

**数学公式**:

```
TNC = Account Balance + Unrealized PnL

触发去杠杆 if: TNC < 0
```

**变量说明**:
- **TNC (Total Net Collateral)**: 总净抵押品
- **Account Balance**: 账户 USDC 余额
- **Unrealized PnL**: 所有持仓的未实现盈亏总和

**计算示例**:

```
假设:
- 用户 USDC 余额 = 2,000 USDC
- 用户持有 10 BTC 多头,开仓价 50,000 USDC
- 当前 BTC 价格 = 49,700 USDC (暴跌)

计算:
- Unrealized PnL = (49,700 - 50,000) × 10 = -3,000 USDC
- TNC = 2,000 + (-3,000) = -1,000 USDC (负数!)

结论: 触发负 TNC 去杠杆
```

**业务解释**:
- 账户总资产为负,清算无法覆盖损失
- 必须强制平仓,并让对手方承担部分损失
- 去杠杆价格为**破产价格**

---

### 2.2 触发条件 2: 最终结算 (Final Settlement)

**数学公式**:

```
触发去杠杆 if: Market.Status == FINAL_SETTLEMENT AND HasOpenPosition == true
```

**变量说明**:
- **Market.Status**: 市场状态
  - `FINAL_SETTLEMENT`: 市场永久关闭,停止交易
- **HasOpenPosition**: 账户是否有未平仓持仓

**计算示例**:

```
假设:
- BTC-USD 市场被决定永久关闭
- 市场状态变为 FINAL_SETTLEMENT
- 用户 Alice 持有 5 BTC 多头
- 预言机最终价格 = 50,000 USDC

结论: 触发最终结算去杠杆,所有持仓强制平仓
```

**业务解释**:
- 市场关闭时所有持仓必须强制平仓
- 使用**预言机价格**作为公平结算价
- 即使账户健康也必须去杠杆

---

## 3. 价格计算机制

### 3.1 破产价格计算 (Bankruptcy Price)

**数学公式**:

```
多头破产价格:
BankruptcyPrice = EntryPrice - (AccountBalance / PositionSize)

空头破产价格:
BankruptcyPrice = EntryPrice + (AccountBalance / PositionSize)
```

**变量说明**:
- **BankruptcyPrice**: 破产价格,使账户净资产恰好为 0 的价格
- **EntryPrice**: 开仓价格
- **AccountBalance**: 账户 USDC 余额 (正数)
- **PositionSize**: 持仓数量 (绝对值)

**计算示例 (多头)**:

```
假设:
- 用户持有 10 BTC 多头,开仓价 50,000 USDC
- 账户余额 2,000 USDC

计算:
BankruptcyPrice = 50,000 - (2,000 / 10) = 50,000 - 200 = 49,800 USDC

验证:
- 当价格 = 49,800 时:
  - Unrealized PnL = (49,800 - 50,000) × 10 = -2,000 USDC
  - TNC = 2,000 + (-2,000) = 0 USDC ✅

结论: 破产价格 = 49,800 USDC
```

**业务解释**:
- 破产价格是账户"生死线"
- 跌破破产价格,账户进入负资产状态
- 去杠杆时使用破产价格确保被去杠杆账户归零

---

### 3.2 预言机价格 (Oracle Price)

**应用场景**: 最终结算时使用

**数学公式**:

```
QuoteQuantums = GetNetNotional(perpetualId, -deltaQuantums)
              = -deltaQuantums × OraclePrice
```

**变量说明**:
- **OraclePrice**: 预言机提供的市场价格
- **deltaQuantums**: 去杠杆数量
- **QuoteQuantums**: 报价金额变动

**计算示例**:

```
假设:
- 最终结算,预言机价格 = 50,000 USDC
- 用户 A 需要去杠杆 5 BTC 多头

计算:
QuoteQuantums = -(-5) × 50,000 = 250,000 USDC

结算:
- 用户 A 卖出 5 BTC,获得 250,000 USDC
- 对手方买入 5 BTC,支付 250,000 USDC

结论: 双方按预言机价格公平结算
```

**业务解释**:
- 预言机价格公平可信
- 所有账户按相同价格结算
- 避免主观定价争议

---

## 4. 对手方选择逻辑

### 4.1 盈利率排序 (PnL Ratio Sorting)

**数学公式**:

```
PnL_Ratio = UnrealizedPnL / PositionValue

对手方排序: 按 PnL_Ratio 降序排列 (盈利率最高的优先)
```

**变量说明**:
- **PnL_Ratio**: 盈利率,盈利占仓位价值的比例
- **UnrealizedPnL**: 未实现盈亏
- **PositionValue**: 仓位价值 = |PositionSize| × CurrentPrice

**计算示例**:

```
假设需要去杠杆 10 BTC 空头 (卖出 10 BTC),寻找多头对手方:
当前 BTC 价格 = 50,000 USDC

对手方候选:
┌────────┬───────────┬───────────┬───────────┬───────────┬────────────┬───────────┬────────────┐
│ 账户   │ 持仓量    │ 开仓价    │ 当前价    │ PnL       │ 仓位价值   │ PnL Ratio │ 去杠杆顺序 │
├────────┼───────────┼───────────┼───────────┼───────────┼────────────┼───────────┼────────────┤
│ A      │ +5 BTC    │ 48,000    │ 50,000    │ +10,000   │ 250,000    │ 4%        │ 1st ⭐     │
│ B      │ +3 BTC    │ 49,000    │ 50,000    │ +3,000    │ 150,000    │ 2%        │ 2nd        │
│ C      │ +8 BTC    │ 49,500    │ 50,000    │ +4,000    │ 400,000    │ 1%        │ 3rd        │
└────────┴───────────┴───────────┴───────────┴───────────┴────────────┴───────────┴────────────┘

去杠杆分配:
1. 账户 A: 去杠杆 5 BTC (盈利率最高 4%)
2. 账户 B: 去杠杆 3 BTC (盈利率次高 2%)
3. 账户 C: 去杠杆 2 BTC (剩余量)
总计: 5 + 3 + 2 = 10 BTC ✅
```

**业务解释**:
- **公平性**: 盈利最多的账户优先承担去杠杆
- **能力**: 盈利率高的账户有能力承受损失
- **随机性**: 从随机位置开始遍历,避免偏向性

---

### 4.2 部分去杠杆处理

**核心逻辑**:

```go
// 对每个对手方,确定本次去杠杆数量
var deltaBaseQuantums *big.Int
if deltaQuantumsRemaining.CmpAbs(offsettingPositionQuantums) > 0 {
    // 剩余量 > 对手方持仓 → 对手方全部平仓
    deltaBaseQuantums = offsettingPositionQuantums
} else {
    // 剩余量 <= 对手方持仓 → 部分平仓
    deltaBaseQuantums = deltaQuantumsRemaining
}
```

**示例**:

```
被去杠杆账户需要平仓 10 BTC:

第1个对手方 (持仓 3 BTC):
- 剩余量 = 10 BTC > 对手方持仓 3 BTC
- 本次去杠杆 = 3 BTC (对手方全部平仓)
- 剩余量 = 10 - 3 = 7 BTC

第2个对手方 (持仓 8 BTC):
- 剩余量 = 7 BTC < 对手方持仓 8 BTC
- 本次去杠杆 = 7 BTC (对手方部分平仓)
- 剩余量 = 7 - 7 = 0 BTC ✅ 完成
```

---

## 5. 社会化损失 (Socialized Loss)

### 5.1 社会化损失的产生

**定义**: 当被去杠杆账户的损失无法由保险基金完全覆盖时,损失会分摊给对手方,这部分损失称为**社会化损失**。

**数学公式**:

```
被去杠杆账户实际亏损 = |TNC| (负数取绝对值)
保险基金可用余额 = InsuranceFundBalance

if 实际亏损 > 保险基金余额:
    社会化损失 = 实际亏损 - 保险基金余额
else:
    社会化损失 = 0
```

**计算示例**:

```
假设:
- 被去杠杆账户 TNC = -5,000 USDC (亏损 5,000)
- 保险基金余额 = 3,000 USDC
- 对手方账户盈利 = 8,000 USDC

正常情况 (无去杠杆):
- 对手方应得盈利 = 8,000 USDC

实际去杠杆:
- 保险基金支付 = 3,000 USDC
- 社会化损失 = 5,000 - 3,000 = 2,000 USDC
- 对手方实际盈利 = 8,000 - 2,000 = 6,000 USDC

结论:
- 对手方承担了 2,000 USDC 的社会化损失
- 原本应得 8,000,实际只得 6,000
```

**业务解释**:
- 社会化损失是系统性风险的最后防线
- 对手方被迫承担部分损失,但避免了系统崩溃
- 保险基金优先吸收损失,不足部分才社会化

---

### 5.2 社会化损失的体现

**代码体现**: `ProcessDeleveraging()` 中的价格计算

```go
// 破产价格计算
deltaQuoteQuantums := k.GetBankruptcyPriceInQuoteQuantums(
    ctx,
    liquidatedSubaccountId,
    perpetualId,
    deltaQuantums,
)

// 被去杠杆账户按破产价格结算 (归零)
deleveragedSubaccountQuoteBalanceDelta := deltaQuoteQuantums

// 对手方按相同价格结算 (承担损失)
offsettingSubaccountQuoteBalanceDelta := -deltaQuoteQuantums
```

**示例**:

```
被去杠杆账户:
- 持仓: 10 BTC 多头,开仓价 50,000 USDC
- 余额: 1,000 USDC
- 破产价格: 50,000 - (1,000 / 10) = 49,900 USDC
- 当前价格: 49,700 USDC (实际市场价)

对手方账户:
- 持仓: 10 BTC 空头,开仓价 52,000 USDC
- 预期盈利 (按市场价): (52,000 - 49,700) × 10 = 23,000 USDC

去杠杆执行:
- 成交价格: 49,900 USDC (破产价格,非市场价)
- 被去杠杆账户: 卖出 10 BTC @ 49,900,获得 499,000 USDC
  - 余额变动: 1,000 + 499,000 - 500,000 (开仓成本) = 0 USDC ✅ 归零
- 对手方账户: 买入 10 BTC @ 49,900,支付 499,000 USDC
  - 盈利: (52,000 - 49,900) × 10 = 21,000 USDC

社会化损失:
- 对手方预期盈利: 23,000 USDC
- 对手方实际盈利: 21,000 USDC
- 社会化损失: 23,000 - 21,000 = 2,000 USDC

结论: 对手方承担了 2,000 USDC 的社会化损失
```

**业务解释**:
- 破产价格 > 市场价时,对手方盈利减少 (社会化损失)
- 破产价格 < 市场价时,对手方盈利增加 (无社会化损失)
- 社会化损失是价格差异导致的,体现在成交价格上

---

## 6. 去杠杆与清算的对比

| 对比项          | 清算 (Liquidation)                  | 去杠杆 (Deleveraging)              |
|--------------|-----------------------------------|---------------------------------|
| **触发条件**     | TNC < MMR (维持保证金不足)              | TNC < 0 或市场最终结算                |
| **执行方式**     | 通过订单簿匹配                           | 强制匹配对手方                         |
| **成交价格**     | Fillable Price (考虑 ABR)           | Bankruptcy Price 或 Oracle Price |
| **对手方**      | 订单簿中的普通订单                         | 系统选择的反向持仓账户                     |
| **保险基金**     | 保险基金吸收差价                          | 社会化损失分摊给对手方                     |
| **风险程度**     | 中等风险 (仍有抵押品)                      | 高风险 (已破产或市场关闭)                  |
| **执行优先级**    | 先执行清算                             | 清算失败后执行去杠杆                      |
| **提现影响**     | 不影响提现                             | 负 TNC 时冻结提现                     |
| **ABCI 阶段**  | PrepareCheckState + FinalizeBlock | PrepareCheckState + FinalizeBlock |
| **代码入口**     | `LiquidateSubaccounts()`          | `DeleverageSubaccounts()`       |

---

## 7. 完整示例解析

### 示例 1: 账户破产触发去杠杆

**场景**: 用户 Eve 持有多头,价格暴跌导致账户破产

**前置条件**:
```
Eve 账户:
- 持仓: 10 BTC 多头,开仓价 50,000 USDC
- 余额: 2,000 USDC
- 当前价格: 49,800 USDC

Frank 账户:
- 持仓: 15 BTC 空头,开仓价 52,000 USDC
- 盈利率: (52,000 - 49,800) / (15 × 49,800) = 2.95% (高盈利)
```

**执行流程**:

**步骤 1**: 价格下跌触发负 TNC
```
价格跌到 49,700 USDC:
Eve TNC = 2,000 + (49,700 - 50,000) × 10 = 2,000 - 3,000 = -1,000 USDC (负数!)

触发条件: CanDeleverageSubaccount() 返回 (true, false)
→ 以破产价格去杠杆
```

**步骤 2**: 计算破产价格
```
调用: GetBankruptcyPriceInQuoteQuantums()
破产价格 = 50,000 - (2,000 / 10) = 49,800 USDC
```

**步骤 3**: 查找对手方
```
调用: OffsetSubaccountPerpetualPosition()
- 查找空头对手方
- 发现 Frank (15 BTC 空头,盈利率 2.95%)
- 选择 Frank 作为对手方
```

**步骤 4**: 执行去杠杆
```
调用: ProcessDeleveraging()

Eve 账户更新:
- 持仓变动: -10 BTC (多头平仓)
- 报价变动: +498,000 USDC (卖出 10 BTC @ 49,800)
- 新余额: 2,000 + 498,000 - 500,000 (开仓成本) = 0 USDC ✅ 归零

Frank 账户更新:
- 持仓变动: +10 BTC (空头部分平仓,剩余 5 BTC 空头)
- 报价变动: -498,000 USDC (买入 10 BTC @ 49,800)
- 新盈利: (52,000 - 49,800) × 10 = 22,000 USDC
```

**步骤 5**: 社会化损失分析
```
Frank 预期盈利 (按市场价 49,700):
(52,000 - 49,700) × 10 = 23,000 USDC

Frank 实际盈利 (按破产价 49,800):
(52,000 - 49,800) × 10 = 22,000 USDC

社会化损失:
23,000 - 22,000 = 1,000 USDC (Frank 承担)
```

**后置条件**:
```
Eve:
- 持仓: 0 BTC
- 余额: 0 USDC
- 状态: 账户归零,离场

Frank:
- 持仓: 5 BTC 空头 (原 15 BTC,去杠杆 10 BTC)
- 盈利: 22,000 USDC (减少 1,000 USDC 社会化损失)
- 状态: 继续交易

系统:
- 风险解除: Eve 的负 TNC 账户已清零
- 损失分摊: Frank 承担了 1,000 USDC 损失,避免系统性风险
```

---

### 示例 2: 市场最终结算去杠杆

**场景**: BTC-USD 市场永久关闭,所有持仓强制平仓

**前置条件**:
```
市场状态:
- BTC-USD 市场状态: FINAL_SETTLEMENT
- 预言机最终价格: 50,000 USDC

Alice 账户:
- 持仓: 5 BTC 多头,开仓价 48,000 USDC
- 余额: 10,000 USDC
- TNC: 10,000 + (50,000 - 48,000) × 5 = 20,000 USDC (健康!)

Bob 账户:
- 持仓: 3 BTC 空头,开仓价 52,000 USDC
- 余额: 8,000 USDC
- TNC: 8,000 + (52,000 - 50,000) × 3 = 14,000 USDC (健康!)
```

**执行流程**:

**步骤 1**: 扫描最终结算市场
```
调用: GetSubaccountsWithPositionsInFinalSettlementMarkets()
- 遍历所有 CLOB 交易对
- 发现 BTC-USD 状态为 FINAL_SETTLEMENT
- 返回持仓账户: [Alice, Bob, ...]
```

**步骤 2**: 检查触发条件
```
调用: CanDeleverageSubaccount(Alice)
- Alice TNC = 20,000 > 0 (非负)
- Market Status = FINAL_SETTLEMENT
- 返回: (false, true) → 以预言机价格去杠杆

调用: CanDeleverageSubaccount(Bob)
- Bob TNC = 14,000 > 0 (非负)
- Market Status = FINAL_SETTLEMENT
- 返回: (false, true) → 以预言机价格去杠杆
```

**步骤 3**: 计算预言机价格
```
调用: getDeleveragingQuoteQuantumsDelta()
- isFinalSettlement = true
- 使用预言机价格 50,000 USDC

Alice 去杠杆:
QuoteQuantums = -(-5) × 50,000 = 250,000 USDC

Bob 去杠杆:
QuoteQuantums = -(3) × 50,000 = -150,000 USDC
```

**步骤 4**: 执行去杠杆
```
调用: ProcessDeleveraging(Alice, Bob, 3 BTC)

Alice 账户更新:
- 持仓变动: -3 BTC (多头部分平仓,剩余 2 BTC 待去杠杆)
- 报价变动: +150,000 USDC (卖出 3 BTC @ 50,000)
- 新余额: 10,000 + 150,000 = 160,000 USDC

Bob 账户更新:
- 持仓变动: +3 BTC (空头全部平仓)
- 报价变动: -150,000 USDC (买入 3 BTC @ 50,000)
- 新余额: 8,000 - 150,000 + (52,000 - 50,000) × 3 = -136,000 USDC
  → 实际: 8,000 + 盈利 6,000 = 14,000 USDC
```

**步骤 5**: 剩余持仓处理
```
Alice 剩余 2 BTC 多头:
- 寻找其他空头对手方
- 继续去杠杆直到全部平仓
```

**后置条件**:
```
Alice:
- 持仓: 0 BTC (全部平仓)
- 余额: 10,000 + (50,000 - 48,000) × 5 = 20,000 USDC
- 盈利: 10,000 USDC

Bob:
- 持仓: 0 BTC (全部平仓)
- 余额: 8,000 + (52,000 - 50,000) × 3 = 14,000 USDC
- 盈利: 6,000 USDC

市场:
- 状态: 已关闭
- 所有持仓: 已清零
- 结算: 按预言机价格公平结算
```

---

## 8. 关键代码路径总结

### 8.1 负 TNC 去杠杆路径

```
1. PrepareCheckState
   └─ LiquidateSubaccountsAgainstOrderbook() (清算失败)
       └─ DeleverageSubaccounts(negativeTncSubaccounts)
           └─ MaybeDeleverageSubaccount()
               ├─ CanDeleverageSubaccount() → (true, false)
               ├─ MemClob.DeleverageSubaccount()
               │   └─ OffsetSubaccountPerpetualPosition()
               │       ├─ 查找空头/多头对手方
               │       ├─ getDeleveragingQuoteQuantumsDelta() → 破产价格
               │       └─ ProcessDeleveraging() → 更新双方账户
               └─ GateWithdrawalsIfNegativeTncSubaccountSeen() → 冻结提现

2. FinalizeBlock
   └─ DeliverTx(MsgProposedOperations)
       └─ ProcessProposerMatches()
           └─ ProcessDeleveraging() → 最终状态更新
```

### 8.2 最终结算去杠杆路径

```
1. PrepareCheckState
   └─ GetSubaccountsWithPositionsInFinalSettlementMarkets()
       ├─ 遍历 CLOB 交易对
       ├─ 查找 FINAL_SETTLEMENT 状态市场
       └─ 返回持仓账户列表
   └─ DeleverageSubaccounts(finalSettlementSubaccounts)
       └─ MaybeDeleverageSubaccount()
           ├─ CanDeleverageSubaccount() → (false, true)
           ├─ MemClob.DeleverageSubaccount()
           │   └─ OffsetSubaccountPerpetualPosition()
           │       ├─ 查找反向持仓对手方
           │       ├─ getDeleveragingQuoteQuantumsDelta() → 预言机价格
           │       └─ ProcessDeleveraging() → 更新双方账户
           └─ 无需冻结提现 (TNC 非负)

2. FinalizeBlock
   └─ DeliverTx(MsgProposedOperations)
       └─ ProcessProposerMatches()
           └─ ProcessDeleveraging() → 最终状态更新
```

---

## 9. 业务规则总结

### 9.1 去杠杆执行规则

1. **触发优先级**: 清算失败 → 负 TNC 去杠杆 → 最终结算去杠杆
2. **价格选择**:
   - 负 TNC → 破产价格 (使被去杠杆账户归零)
   - 最终结算 → 预言机价格 (公平结算)
3. **对手方选择**: 按盈利率降序排列,盈利最高的优先
4. **部分成功**: 允许部分去杠杆,剩余部分继续尝试
5. **提现冻结**: 负 TNC 时插入零成交去杠杆,冻结提现

### 9.2 社会化损失规则

1. **损失顺序**: 保险基金 → 社会化损失 (对手方)
2. **损失计算**: 破产价格与市场价的差异
3. **损失分摊**: 所有去杠杆对手方按比例承担
4. **透明性**: 链上事件记录,用户可查询

### 9.3 安全保障

1. **原子性**: 去杠杆要么完全成功,要么完全回滚
2. **确定性**: 所有节点按相同逻辑选择对手方
3. **公平性**: 盈利率高的账户优先承担损失
4. **风险隔离**: 提现冻结防止损失扩大

---

## 10. 相关文件索引

| 文件路径 | 关键函数 | 行号 |
|---------|---------|------|
| `protocol/x/clob/keeper/deleveraging.go` | `MaybeDeleverageSubaccount()` | 35-140 |
| `protocol/x/clob/keeper/deleveraging.go` | `CanDeleverageSubaccount()` | 151-195 |
| `protocol/x/clob/keeper/deleveraging.go` | `GateWithdrawalsIfNegativeTncSubaccountSeen()` | 200-260 |
| `protocol/x/clob/keeper/deleveraging.go` | `OffsetSubaccountPerpetualPosition()` | 295-466 |
| `protocol/x/clob/keeper/deleveraging.go` | `getDeleveragingQuoteQuantumsDelta()` | 471-490 |
| `protocol/x/clob/keeper/deleveraging.go` | `ProcessDeleveraging()` | 502-642 |
| `protocol/x/clob/keeper/deleveraging.go` | `GetSubaccountsWithPositionsInFinalSettlementMarkets()` | 654-687 |
| `protocol/x/clob/keeper/deleveraging.go` | `DeleverageSubaccounts()` | 691-720 |
| `protocol/x/clob/keeper/liquidations.go` | `GetBankruptcyPriceInQuoteQuantums()` | 656-733 |
| `notes/business/prd/clob_prd.md` | 去杠杆业务需求 (FR-4.3) | 699-903 |

---

**文档版本**: v1.0
**创建时间**: 2026-01-06
**作者**: Claude Sonnet 4.5
**状态**: ✅ 完成
