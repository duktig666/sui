# Hermes DEX 清算与去杠杆机制完整文档

## 📚 文档目录

本目录包含 Hermes DEX 清算 (Liquidation) 和去杠杆 (Deleveraging) 机制的完整技术分析文档,涵盖代码实现、业务逻辑、价格计算、对比分析等多个方面。

---

## 📄 文档列表

### 1️⃣ [清算机制详解](liquidation_mechanism.md) ⭐核心

**内容概要**:
- ✅ 完整代码调用流程分析 (PrepareCheckState → FinalizeBlock)
- ✅ 清算价格计算详解 (Fillable Price, Bankruptcy Price)
- ✅ 保险基金机制 (Insurance Fund Delta)
- ✅ 强制平仓补偿与社会化损失
- ✅ 2 个详细实例解析

**关键知识点**:
- **清算触发条件**: TNC < MMR (总净抵押品 < 维持保证金要求)
- **Fillable Price 公式**: `(PNNV - ABR × SMMR × PMMR) / PS`
- **ABR (调整后破产评级)**: `BA × (1 - TNC / TMMR)`,范围 [0, 1]
- **Bankruptcy Price 公式**: `EntryPrice ± AccountBalance / PositionSize`
- **Insurance Fund Delta**: 破产价格与实际成交价的差额

**适合人群**: 需要深入理解清算机制的开发者和分析师

---

### 2️⃣ [去杠杆机制详解](deleveraging_mechanism.md) ⭐核心

**内容概要**:
- ✅ 完整代码调用流程分析 (PrepareCheckState → DeleverageSubaccounts)
- ✅ 两种触发条件详解 (负 TNC + 最终结算)
- ✅ 破产价格与预言机价格计算
- ✅ 对手方选择逻辑 (盈利率排序)
- ✅ 社会化损失机制
- ✅ 提现门控 (Withdrawal Gating)
- ✅ 2 个完整示例 (负 TNC + 最终结算)

**关键知识点**:
- **触发条件 1**: TNC < 0 (负净抵押品) → 破产价格去杠杆
- **触发条件 2**: 市场最终结算 (FINAL_SETTLEMENT) → 预言机价格去杠杆
- **对手方选择**: `PnL_Ratio = UnrealizedPnL / PositionValue`,按降序排列
- **社会化损失**: 保险基金不足时,对手方承担损失
- **提现冻结**: 负 TNC 时插入零成交去杠杆,冻结提现

**适合人群**: 需要理解极端风险场景处理的开发者和风控人员

---

### 3️⃣ [OrderFill 更新机制详解](orderfill_update.md) ⭐技术细节

**内容概要**:
- ✅ ABCI 阶段识别: OrderFill 在哪个阶段更新?
- ✅ OrderFillState 数据结构详解
- ✅ 存储机制 (KVStore 键值设计)
- ✅ 更新时机与调用链 (DeliverTx → ProcessSingleMatch)
- ✅ 清理机制 (EndBlock → PruneOrdersForBlockHeight)
- ✅ 完整示例: Short-Term 订单生命周期
- ✅ 常见问题解答 (FAQ)

**关键知识点**:
- **更新阶段**: **DeliverTx** (处理 MsgProposedOperations 时)
- **数据结构**: `OrderFillState{FillAmount, PrunableBlockHeight}`
- **存储键**: `OrderAmountFilledKeyPrefix + OrderId.ToStateKey()`
- **清理时机**: **EndBlock** (pruneableBlockHeight <= currentBlockHeight)
- **清理规则**:
  - Short-Term: `GoodTilBlock + ShortBlockWindow` 后清理
  - Stateful: `math.MaxUint32` (永不清理)
  - Liquidation (Taker): 不跟踪 OrderFill

**适合人群**: 需要理解订单状态管理的开发者

---

### 4️⃣ [Hyperliquid 对比分析](hyperliquid_comparison.md) ⭐行业对比

**内容概要**:
- ✅ Hyperliquid 三层清算流程 (订单簿 → HLP → ADL)
- ✅ HLP (社区流动性提供者) 机制
- ✅ ADL (自动去杠杆) 排序算法
- ✅ 2025 年 10 月首次 ADL 事件分析
- ✅ Hermes vs Hyperliquid 详细对比表
- ✅ 设计理念对比与优化建议

**关键知识点**:
- **Hyperliquid 三层瀑布**:
  1. 订单簿清算 (市价单,> 100k USDC 仅清算 20%)
  2. HLP 保底清算 (账户净值 < MMR × 2/3)
  3. ADL 去杠杆 (HLP 不足 + 账户净值为负)
- **ADL Priority**: `Unrealized PnL × Leverage Used`
- **HLP 机制**: 社区流动性提供者共担风险,清算利润流入 HLP
- **无清算手续费**: Hyperliquid 无清算手续费,用户友好

**适合人群**: 需要了解行业竞品设计的产品经理和架构师

---

## 🎯 快速导航

### 按主题导航

**清算机制**:
- [清算触发条件](liquidation_mechanism.md#2-清算触发条件)
- [清算价格计算](liquidation_mechanism.md#3-清算价格计算机制)
- [保险基金机制](liquidation_mechanism.md#4-保险基金机制)
- [完整清算示例](liquidation_mechanism.md#6-完整示例解析)

**去杠杆机制**:
- [触发条件分析](deleveraging_mechanism.md#2-触发条件分析)
- [价格计算机制](deleveraging_mechanism.md#3-价格计算机制)
- [对手方选择逻辑](deleveraging_mechanism.md#4-对手方选择逻辑)
- [社会化损失](deleveraging_mechanism.md#5-社会化损失-socialized-loss)

**OrderFill 机制**:
- [ABCI 阶段识别](orderfill_update.md#1-abci-阶段识别-orderfill-在哪个阶段更新)
- [数据结构详解](orderfill_update.md#2-数据结构详解)
- [更新时机详解](orderfill_update.md#4-更新时机详解)
- [清理机制详解](orderfill_update.md#5-清理机制详解)

**对比分析**:
- [Hyperliquid 清算机制](hyperliquid_comparison.md#1-hyperliquid-清算机制详解)
- [Hyperliquid ADL 机制](hyperliquid_comparison.md#12-三层清算流程-three-tier-liquidation)
- [对比总结表](hyperliquid_comparison.md#3-hyperliquid-vs-hermes-对比总结)
- [真实案例分析](hyperliquid_comparison.md#5-真实案例分析-2025-年-hyperliquid-adl-事件)

---

### 按角色导航

**开发者**:
1. 先阅读 [OrderFill 更新机制](orderfill_update.md) 了解技术实现
2. 再阅读 [清算机制](liquidation_mechanism.md) 的代码调用流程部分
3. 最后阅读 [去杠杆机制](deleveraging_mechanism.md) 的代码调用流程部分

**产品经理/分析师**:
1. 先阅读 [清算机制](liquidation_mechanism.md) 的业务逻辑和示例
2. 再阅读 [去杠杆机制](deleveraging_mechanism.md) 的业务规则
3. 最后阅读 [Hyperliquid 对比](hyperliquid_comparison.md) 了解竞品

**风控人员**:
1. 重点阅读 [清算机制](liquidation_mechanism.md) 的保险基金部分
2. 重点阅读 [去杠杆机制](deleveraging_mechanism.md) 的社会化损失部分
3. 参考 [Hyperliquid 对比](hyperliquid_comparison.md) 的风险评估

---

## 🔑 核心概念速查

### 清算相关

| 概念 | 定义 | 公式/说明 |
|-----|------|----------|
| **TNC** | 总净抵押品 (Total Net Collateral) | `Account Balance + Unrealized PnL` |
| **MMR** | 维持保证金要求 (Maintenance Margin Requirement) | `Position Value × MMR Rate` |
| **ABR** | 调整后破产评级 (Adjusted Bankruptcy Rating) | `BA × (1 - TNC / TMMR)`,范围 [0, 1] |
| **Fillable Price** | 可成交价格 (清算订单价格) | `(PNNV - ABR × SMMR × PMMR) / PS` |
| **Bankruptcy Price** | 破产价格 (TNC = 0 的价格) | `EntryPrice ± AccountBalance / PositionSize` |
| **Insurance Fund Delta** | 保险基金变动 | `(Bankruptcy Price - Fill Price) × Fill Amount` |

### 去杠杆相关

| 概念 | 定义 | 公式/说明 |
|-----|------|----------|
| **负 TNC** | 账户资不抵债 | `TNC < 0` |
| **盈利率** | 未实现盈亏占仓位价值的比例 | `PnL_Ratio = UnrealizedPnL / PositionValue` |
| **社会化损失** | 对手方承担的损失 | 保险基金不足时,损失分摊给对手方 |
| **提现门控** | 负 TNC 时冻结提现 | 插入零成交去杠杆,阻止提现 |
| **最终结算** | 市场永久关闭 | 所有持仓强制平仓,按预言机价格结算 |

### OrderFill 相关

| 概念 | 定义 | 说明 |
|-----|------|------|
| **OrderFillState** | 订单履行状态 | 记录累计成交量和可清理高度 |
| **pruneableBlockHeight** | 可清理区块高度 | Short-Term: `GoodTilBlock + ShortBlockWindow`<br>Stateful: `math.MaxUint32` |
| **DeliverTx** | 交易执行阶段 | OrderFill 状态在此阶段更新 |
| **EndBlock** | 区块结束阶段 | OrderFill 状态在此阶段清理 |

---

## 📊 对比总结表

### Hermes vs Hyperliquid 清算机制

| 对比项 | Hermes DEX | Hyperliquid | 优劣 |
|-------|------------|-------------|-----|
| **清算方式** | 限价单 (Fillable Price) | 市价单 (三层瀑布) | Hyperliquid 更灵活 |
| **清算价格** | Fillable Price (考虑 ABR) | Mark Price (外部 CEX + 订单簿) | Hyperliquid 对用户更优 |
| **保险基金** | 独立保险基金 | HLP (社区流动性提供者) | Hermes 更稳定 |
| **手续费** | ❌ 有清算手续费 | ✅ 无清算手续费 | Hyperliquid 更优 |
| **去杠杆触发** | TNC < 0 | HLP 不足 + 负净值 | Hermes 更严格 |
| **去杠杆价格** | Bankruptcy Price (被去杠杆账户归零) | Mark Price | Hermes 更公平 |
| **对手方排序** | 盈利率 | 盈利率 × 杠杆 | Hyperliquid 考虑杠杆 |
| **历史验证** | 未公开 | 2025 年 10 月首次 ADL | Hyperliquid 已压力测试 |

---

## 🛠️ 相关代码文件索引

### 清算相关代码

| 文件路径 | 关键函数 | 行号 |
|---------|---------|------|
| `protocol/x/clob/keeper/liquidations.go` | `MaybeGetLiquidationOrder()` | 19-208 |
| `protocol/x/clob/keeper/liquidations.go` | `GetFillablePrice()` | 514-650 |
| `protocol/x/clob/keeper/liquidations.go` | `GetBankruptcyPriceInQuoteQuantums()` | 656-733 |
| `protocol/x/clob/keeper/liquidations.go` | `GetLiquidationInsuranceFundDelta()` | 656-733 |
| `protocol/x/clob/keeper/process_operations.go` | `LiquidateSubaccountsAgainstOrderbook()` | - |

### 去杠杆相关代码

| 文件路径 | 关键函数 | 行号 |
|---------|---------|------|
| `protocol/x/clob/keeper/deleveraging.go` | `MaybeDeleverageSubaccount()` | 35-140 |
| `protocol/x/clob/keeper/deleveraging.go` | `CanDeleverageSubaccount()` | 151-195 |
| `protocol/x/clob/keeper/deleveraging.go` | `OffsetSubaccountPerpetualPosition()` | 295-466 |
| `protocol/x/clob/keeper/deleveraging.go` | `getDeleveragingQuoteQuantumsDelta()` | 471-490 |
| `protocol/x/clob/keeper/deleveraging.go` | `ProcessDeleveraging()` | 502-642 |
| `protocol/x/clob/keeper/deleveraging.go` | `GateWithdrawalsIfNegativeTncSubaccountSeen()` | 200-260 |

### OrderFill 相关代码

| 文件路径 | 关键函数 | 行号 |
|---------|---------|------|
| `protocol/x/clob/keeper/order_state.go` | `SetOrderFillAmount()` | 69-95 |
| `protocol/x/clob/keeper/order_state.go` | `GetOrderFillAmount()` | 98-128 |
| `protocol/x/clob/keeper/order_state.go` | `RemoveOrderFillAmount()` | 262-283 |
| `protocol/x/clob/keeper/order_state.go` | `PruneOrdersForBlockHeight()` | 211-236 |
| `protocol/x/clob/keeper/process_single_match.go` | `ProcessSingleMatch()` | 44-317 |
| `protocol/x/clob/keeper/process_single_match.go` | `setOrderFillAmountsAndPruning()` | 741-784 |

---

## 🔗 相关文档链接

### 内部文档

- [CLOB 模块 PRD](../../prd/clob_prd.md) - 产品需求文档
- [Perpetuals 模块 PRD](../../prd/perpetuals_prd.md) - 永续合约需求文档
- [CLOB 数据结构](../../data_structure/clob.md) - 数据结构设计
- [CLOB 模块架构](../../architecture/clob.md) - 架构设计文档
- [CLOB CLAUDE 文档](../../../protocol/x/clob/CLAUDE.md) - 开发者指南

### 外部参考

- [Cosmos SDK 文档](https://docs.cosmos.network)
- [CometBFT 文档](https://docs.cometbft.com)
- [Hyperliquid 官方文档](https://hyperliquid.gitbook.io/hyperliquid-docs)

---

## 📝 文档维护

### 版本信息

- **文档版本**: v1.0
- **创建时间**: 2026-01-06
- **作者**: Claude Sonnet 4.5
- **状态**: ✅ 已完成

### 更新日志

| 日期 | 版本 | 更新内容 |
|------|------|---------|
| 2026-01-06 | v1.0 | 初始版本,包含 4 个详细技术文档 |

### 贡献指南

如需更新文档,请遵循以下规范:
1. 代码分析需包含具体文件路径和行号
2. 公式需提供完整变量说明和示例计算
3. 对比分析需提供数据来源和参考链接
4. 示例需包含完整的前置条件、执行流程和后置条件

---

## 💡 常见问题速查

### Q1: 清算和去杠杆有什么区别?

**答案**:
- **清算 (Liquidation)**: TNC < MMR,通过订单簿匹配平仓,保险基金吸收差价
- **去杠杆 (Deleveraging)**: TNC < 0 或市场最终结算,强制匹配对手方,社会化损失

**详见**: [去杠杆与清算的对比](deleveraging_mechanism.md#6-去杠杆与清算的对比)

---

### Q2: OrderFill 状态在哪个 ABCI 阶段更新?

**答案**: **DeliverTx 阶段**,具体是处理 `MsgProposedOperations` 消息时。

**详见**: [ABCI 阶段识别](orderfill_update.md#1-abci-阶段识别-orderfill-在哪个阶段更新)

---

### Q3: Hyperliquid 的 HLP 机制是什么?

**答案**: HLP (Hyperliquid Liquidity Provider) 是社区流动性提供者金库,类似保险基金,但由社区共同承担风险并分享清算利润。

**详见**: [Hyperliquid HLP 机制](hyperliquid_comparison.md#层级-2-hlp-保底清算-backstop-liquidation-via-hlp)

---

### Q4: 什么是社会化损失?

**答案**: 当保险基金无法覆盖清算损失时,损失会分摊给对手方盈利账户,这部分损失称为社会化损失。

**详见**: [社会化损失详解](deleveraging_mechanism.md#5-社会化损失-socialized-loss)

---

### Q5: 为什么清算订单不跟踪 OrderFill?

**答案**: 清算订单 (Liquidation Order) 是一次性订单,只能在账户可清算时放置,无法重放,因此不需要跟踪累计成交量。

**详见**: [OrderFill FAQ](orderfill_update.md#9-常见问题解答-faq)

---

## 🎓 学习路径推荐

### 初学者路径 (3-5 小时)

1. **第一步**: 阅读本 README 文档,了解整体结构 (15 分钟)
2. **第二步**: 阅读 [清算机制](liquidation_mechanism.md) 的"业务流程概述"和"完整示例" (1 小时)
3. **第三步**: 阅读 [去杠杆机制](deleveraging_mechanism.md) 的"触发条件"和"完整示例" (1 小时)
4. **第四步**: 阅读 [Hyperliquid 对比](hyperliquid_comparison.md) 的"对比总结表" (30 分钟)
5. **第五步**: 回顾核心概念和公式 (30 分钟)

### 开发者路径 (5-8 小时)

1. **第一步**: 阅读 [OrderFill 更新机制](orderfill_update.md) 完整文档 (2 小时)
2. **第二步**: 阅读 [清算机制](liquidation_mechanism.md) 的"代码调用流程"部分 (2 小时)
3. **第三步**: 阅读 [去杠杆机制](deleveraging_mechanism.md) 的"代码调用流程"部分 (2 小时)
4. **第四步**: 查看相关代码文件,对照文档理解实现 (2 小时)

### 产品/架构师路径 (4-6 小时)

1. **第一步**: 阅读本 README 文档和核心概念 (30 分钟)
2. **第二步**: 阅读 [清算机制](liquidation_mechanism.md) 完整文档 (1.5 小时)
3. **第三步**: 阅读 [去杠杆机制](deleveraging_mechanism.md) 完整文档 (1.5 小时)
4. **第四步**: 重点阅读 [Hyperliquid 对比](hyperliquid_comparison.md) 完整文档 (2 小时)
5. **第五步**: 思考优化方向和改进建议 (1 小时)

---

## 📧 联系方式

如有疑问或建议,请联系:
- 文档作者: Claude Sonnet 4.5
- 项目团队: Hermes DEX Development Team
- 问题反馈: 通过 GitHub Issues 提交

---

**最后更新**: 2026-01-06
**文档状态**: ✅ 完整
**总字数**: 约 50,000 字 (4 个文档合计)
