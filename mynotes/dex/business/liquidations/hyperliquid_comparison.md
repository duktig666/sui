# Hyperliquid vs Hermes 清算与去杠杆机制对比分析

## 概述

本文档对比分析 **Hyperliquid** 和 **Hermes DEX** 的清算 (Liquidation) 和去杠杆 (Deleveraging/ADL) 机制,揭示两个去中心化永续合约交易所在风险管理方面的设计差异。

---

## 1. Hyperliquid 清算机制详解

### 1.1 清算触发条件

**触发时机**: 当账户净值 (Account Equity) 低于维持保证金 (Maintenance Margin) 时触发清算。

**维持保证金计算**:

```
Maintenance Margin = Initial Margin × 50%
                   = Position Value / Max Leverage × 50%
```

**不同杠杆的维持保证金率**:

| 最大杠杆 | 初始保证金率 | 维持保证金率 | 示例资产 |
|---------|------------|------------|---------|
| 3x      | 33.3%      | 16.7%      | 低流动性资产 |
| 5x      | 20%        | 10%        | 中等流动性资产 |
| 20x     | 5%         | 2.5%       | 主流资产 |
| 40x     | 2.5%       | 1.25%      | BTC, ETH 等 |

**示例计算**:

```
假设:
- 用户持仓: 10 BTC 多头
- 开仓价格: 50,000 USDC
- 当前价格: 49,000 USDC
- 最大杠杆: 20x
- 账户余额: 30,000 USDC

计算:
Position Value = 10 × 49,000 = 490,000 USDC
Initial Margin = 490,000 / 20 = 24,500 USDC
Maintenance Margin = 24,500 × 50% = 12,250 USDC
Unrealized PnL = (49,000 - 50,000) × 10 = -10,000 USDC
Account Equity = 30,000 - 10,000 = 20,000 USDC

判断: 20,000 > 12,250 → 未触发清算 ✅
```

---

### 1.2 三层清算流程 (Three-Tier Liquidation)

Hyperliquid 采用**三层风险瀑布** (Risk Waterfall) 机制:

#### 层级 1: 订单簿清算 (Order Book Liquidation)

**机制**:
- 将被清算账户的持仓以**市价单**形式发送到订单簿
- 任何用户都可以参与竞价清算流程
- 被清算用户保留剩余保证金 (如果有)

**特殊规则**:
- 对于 > 100,000 USDC 的大额持仓,**仅清算 20%** 进入订单簿
- 目的: 避免单笔大额清算冲击市场

**价格**:
- 使用 **Mark Price** (标记价格),而非即时订单簿价格
- Mark Price = 外部 CEX 价格 + Hyperliquid 订单簿状态的加权平均

**优势**:
- 被清算用户可能保留部分资金
- 无清算手续费 (Unlike CEXs)
- 透明,公平竞争

---

#### 层级 2: HLP 保底清算 (Backstop Liquidation via HLP)

**触发条件**: 当账户净值 < 维持保证金 × 2/3 时触发。

**HLP 简介**:
- HLP (Hyperliquid Liquidity Provider) 是 Hyperliquid 的**流动性提供者金库**
- 类似传统 CEX 的保险基金,但由社区流动性提供者共同承担风险
- 所有清算利润流入 HLP,损失由 HLP 吸收

**机制**:
- HLP 金库接管被清算持仓
- 按市场价格平仓
- 盈亏由 HLP 承担

**示例**:

```
假设:
- 用户持仓: 10 BTC 多头,开仓价 50,000 USDC
- 账户余额: 5,000 USDC
- 当前价格: 49,500 USDC
- Maintenance Margin: 12,250 USDC

计算:
Account Equity = 5,000 + (49,500 - 50,000) × 10 = 0 USDC
触发条件: 0 < 12,250 × 2/3 = 8,167 USDC ✅ 触发 HLP 清算

HLP 操作:
1. HLP 接管 10 BTC 多头持仓
2. 按市场价 49,500 USDC 卖出 10 BTC
3. 收入: 495,000 USDC
4. 原开仓成本: 500,000 USDC
5. HLP 损失: 500,000 - 495,000 = 5,000 USDC (由 HLP 吸收)
```

---

#### 层级 3: 自动去杠杆 (Auto-Deleveraging, ADL)

**触发条件**: 当 HLP 无法覆盖剩余损失,且账户净值为负时触发。

**历史事件**: 2025 年 10 月,Hyperliquid **首次触发跨仓 ADL**,运行两年多来的第一次。

**ADL 机制**:
- 系统自动对**对手方盈利账户**进行强制平仓
- 按**盈利率 × 杠杆**排序,优先平仓盈利最高且杠杆最高的账户
- 平仓价格: **Mark Price**

**排序公式**:

```
ADL Priority = Unrealized PnL × Leverage Used
```

**示例**:

```
被清算账户:
- 持仓: 50 BTC 多头,开仓价 50,000 USDC
- 账户净值: -10,000 USDC (负数!)
- 当前价格: 48,000 USDC

对手方候选 (空头):
┌────────┬───────────┬───────────┬───────────┬─────────┬──────────────┬────────────┐
│ 账户   │ 持仓量    │ 开仓价    │ 当前价    │ 杠杆    │ Unrealized PnL │ ADL Priority │
├────────┼───────────┼───────────┼───────────┼─────────┼──────────────┼────────────┤
│ A      │ -20 BTC   │ 52,000    │ 48,000    │ 10x     │ +80,000      │ 800,000 ⭐  │
│ B      │ -15 BTC   │ 51,000    │ 48,000    │ 5x      │ +45,000      │ 225,000     │
│ C      │ -30 BTC   │ 50,500    │ 48,000    │ 3x      │ +75,000      │ 225,000     │
└────────┴───────────┴───────────┴───────────┴─────────┴──────────────┴────────────┘

ADL 执行:
1. 账户 A 优先被 ADL (Priority 最高)
2. 强制平仓 20 BTC 空头 @ 48,000 (Mark Price)
3. 账户 A 实际盈利: 80,000 USDC (提前实现盈利)
4. 剩余 30 BTC 继续 ADL 账户 B 和 C
```

---

### 1.3 破产价格 (Bankruptcy Price)

**定义**: 破产价格是账户净值恰好为 0 的价格点。

**公式**:

```
多头破产价格:
Bankruptcy Price = Entry Price - (Initial Margin / Position Size)

空头破产价格:
Bankruptcy Price = Entry Price + (Initial Margin / Position Size)
```

**示例**:

```
假设:
- 持仓: 10 BTC 多头,开仓价 50,000 USDC
- 初始保证金: 25,000 USDC

计算:
Bankruptcy Price = 50,000 - (25,000 / 10) = 50,000 - 2,500 = 47,500 USDC

验证:
- 当价格 = 47,500 时:
  - Unrealized PnL = (47,500 - 50,000) × 10 = -25,000 USDC
  - Account Equity = 25,000 - 25,000 = 0 USDC ✅
```

**业务意义**:
- 破产价格以下,损失由 HLP/保险基金承担
- 清算价格 > 破产价格,确保有缓冲空间

---

## 2. Hermes DEX 清算机制详解

### 2.1 清算触发条件

**触发时机**: 当账户总净抵押品 (TNC) < 维持保证金要求 (MMR) 时触发清算。

**公式**:

```
TNC = Account Balance + Unrealized PnL

MMR = Position Value × Maintenance Margin Rate

触发清算 if: TNC < MMR
```

**示例** (与 Hyperliquid 对比):

```
假设 (相同场景):
- 用户持仓: 10 BTC 多头
- 开仓价格: 50,000 USDC
- 当前价格: 49,000 USDC
- 杠杆: 20x (MMR = 2.5%)
- 账户余额: 30,000 USDC

Hermes 计算:
Position Value = 10 × 49,000 = 490,000 USDC
MMR = 490,000 × 2.5% = 12,250 USDC
Unrealized PnL = (49,000 - 50,000) × 10 = -10,000 USDC
TNC = 30,000 - 10,000 = 20,000 USDC

判断: 20,000 > 12,250 → 未触发清算 ✅
```

**差异**: Hermes 和 Hyperliquid 的触发条件本质相同,但命名不同。

---

### 2.2 Hermes 清算流程

#### 步骤 1: 生成清算订单

**机制**:
- 系统生成**清算限价单** (Liquidation Limit Order)
- 清算价格 = **Fillable Price** (可成交价格)

**Fillable Price 计算**:

```
Fillable Price = (PNNV - ABR × SMMR × PMMR) / PS

其中:
- PNNV: 持仓净名义价值 (Position Net Notional Value)
- ABR: 调整后破产评级 (Adjusted Bankruptcy Rating),范围 [0, 1]
- SMMR: 子账户维持保证金要求 (Subaccount MMR)
- PMMR: 持仓维持保证金要求 (Position MMR)
- PS: 持仓数量 (Position Size)
```

**ABR 公式**:

```
ABR = BA × (1 - TNC / TMMR)

Clamped to [0, 1]

其中:
- BA: 破产调整因子 (Bankruptcy Adjustment Factor),配置参数
- TNC: 总净抵押品
- TMMR: 总维持保证金要求
```

**示例**:

```
假设:
- 持仓: 10 BTC 多头,开仓价 50,000 USDC
- 当前价格: 49,000 USDC
- TNC: 5,000 USDC
- TMMR: 12,250 USDC
- BA: 1.0

计算 ABR:
ABR = 1.0 × (1 - 5,000 / 12,250) = 1.0 × 0.592 = 0.592

计算 Fillable Price (简化):
Fillable Price ≈ 49,000 - (0.592 × buffer) = ~48,700 USDC

业务解释:
- ABR 越高,清算价格越不利于被清算者
- ABR = 0: 清算价格 = 市场价
- ABR = 1: 清算价格 = 破产价格附近
```

---

#### 步骤 2: 订单簿匹配

**机制**:
- 清算订单进入订单簿,与普通订单竞争匹配
- 优先级: 清算订单优先于普通订单

**保险基金差价** (Insurance Fund Delta):

```
Insurance Fund Delta = (Bankruptcy Price - Fill Price) × Fill Amount

正数: 保险基金收入
负数: 保险基金支出
```

**示例**:

```
清算执行:
- 清算订单: 卖出 10 BTC @ 48,700 USDC
- 成交价格: 48,700 USDC (Fillable Price)
- 破产价格: 49,500 USDC

Insurance Fund Delta = (49,500 - 48,700) × 10 = +8,000 USDC

结果: 保险基金收入 8,000 USDC ✅
```

---

#### 步骤 3: 去杠杆 (Deleveraging)

**触发条件**: 当 TNC < 0 (负净抵押品) 时触发去杠杆。

**机制**:
- 系统查找反向持仓的对手方
- 按**盈利率**排序,盈利最高的优先被去杠杆
- 去杠杆价格 = **破产价格** (Bankruptcy Price)

**示例** (详见 `deleveraging_mechanism.md`)

---

## 3. Hyperliquid vs Hermes 对比总结

### 3.1 清算机制对比

| 对比项 | Hyperliquid | Hermes DEX |
|-------|-------------|------------|
| **触发条件** | Account Equity < Maintenance Margin | TNC < MMR |
| **本质差异** | 无 (命名不同,逻辑相同) | 无 |
| **清算方式** | 市价单 (Market Order) | 限价单 (Limit Order,Fillable Price) |
| **清算价格** | Mark Price (外部 CEX + 订单簿加权) | Fillable Price (考虑 ABR) |
| **大额持仓** | > 100k USDC 仅清算 20% | 全部清算 (无特殊规则) |
| **手续费** | ✅ 无清算手续费 | ❌ 清算订单支付手续费 |
| **剩余保证金** | ✅ 被清算用户保留 (如果有) | ✅ 被清算用户保留 (如果有) |
| **保险基金** | HLP 金库 (社区流动性提供者) | 系统保险基金 (独立池) |
| **保险基金角色** | 主动参与清算 (层级 2) | 被动吸收差价 |

---

### 3.2 去杠杆 (ADL) 机制对比

| 对比项 | Hyperliquid ADL | Hermes Deleveraging |
|-------|----------------|---------------------|
| **触发条件** | HLP 无法覆盖损失 + 账户净值为负 | TNC < 0 或市场最终结算 |
| **对手方选择** | 盈利率 × 杠杆 (Priority 最高优先) | 盈利率 (PnL Ratio 最高优先) |
| **去杠杆价格** | Mark Price (标记价格) | Bankruptcy Price (破产价格) 或 Oracle Price |
| **历史触发** | 2025 年 10 月首次触发跨仓 ADL | 未公开具体触发历史 |
| **透明度** | ✅ 用户可查询 ADL 排名 | ✅ 链上事件记录 |
| **社会化损失** | 对手方按 Priority 承担 | 对手方按盈利率承担 |

---

### 3.3 保险基金机制对比

| 对比项 | Hyperliquid HLP | Hermes Insurance Fund |
|-------|-----------------|----------------------|
| **性质** | 社区流动性提供者金库 | 系统独立保险基金池 |
| **资金来源** | 流动性提供者存款 | 清算盈余、系统收入 |
| **盈利分配** | ✅ 所有清算利润流入 HLP | ❌ 保险基金独立核算 |
| **损失承担** | HLP 持有者共同承担 | 保险基金吸收,不足时社会化 |
| **主动性** | 主动参与清算 (层级 2) | 被动吸收差价 |
| **透明度** | ✅ HLP 净值公开可查 | ✅ 保险基金余额链上公开 |

---

### 3.4 破产价格与清算价格对比

| 对比项 | Hyperliquid | Hermes DEX |
|-------|-------------|------------|
| **破产价格定义** | Account Equity = 0 的价格 | TNC = 0 的价格 |
| **破产价格公式** | Entry Price ± (Initial Margin / Position Size) | Entry Price ± (Account Balance / Position Size) |
| **清算价格** | Mark Price (订单簿清算) | Fillable Price (考虑 ABR) |
| **价格关系** | Liquidation Price > Bankruptcy Price | Fillable Price ≈ Bankruptcy Price (ABR 影响) |
| **缓冲空间** | Maintenance Margin - Initial Margin | TNC - MMR |

---

## 4. 设计理念对比

### 4.1 Hyperliquid 设计理念

**核心思想**: 多层风险隔离 + 社区共担风险

**特点**:
1. **三层风险瀑布**: 订单簿 → HLP → ADL,逐级吸收风险
2. **社区化保险基金**: HLP 由流动性提供者组成,利润共享、风险共担
3. **透明无手续费**: 无清算手续费,被清算用户保留最大资金
4. **Mark Price 机制**: 结合外部 CEX 价格,防止单一价格操纵

**优势**:
- 用户友好,保留更多资金
- 社区利益一致
- 透明度高

**劣势**:
- HLP 承担较高风险
- 极端行情可能触发 ADL (2025 年 10 月首次触发)

---

### 4.2 Hermes DEX 设计理念

**核心思想**: 精确价格控制 + 保险基金缓冲 + 确定性去杠杆

**特点**:
1. **Fillable Price 机制**: 通过 ABR 精确控制清算价格,平衡被清算用户与保险基金利益
2. **独立保险基金**: 系统层面统一管理,不依赖外部流动性提供者
3. **确定性去杠杆**: 严格按破产价格执行,确保被去杠杆账户归零
4. **多场景支持**: 支持负 TNC 和最终结算两种去杠杆场景

**优势**:
- 风险可控,保险基金独立
- 去杠杆价格公平 (破产价格)
- 适合复杂市场场景 (最终结算)

**劣势**:
- 清算价格可能不如 Mark Price 优惠
- 保险基金压力较大 (无 HLP 缓冲)

---

## 5. 真实案例分析: 2025 年 Hyperliquid ADL 事件

### 5.1 事件背景

**时间**: 2025 年 10 月

**事件**: Hyperliquid **首次触发跨仓 ADL** (Auto-Deleveraging),运行两年多来的第一次。

**原因**: 极端行情导致大量清算,HLP 金库无法完全覆盖损失。

---

### 5.2 事件时间线

```
第1阶段: 市场暴跌
- 某资产价格短时间内暴跌 30%
- 大量高杠杆多头账户触发清算

第2阶段: 订单簿清算 (层级 1)
- 清算订单以市价单形式进入订单簿
- 部分清算成功,部分因流动性不足未完全成交

第3阶段: HLP 保底清算 (层级 2)
- HLP 金库接管剩余清算持仓
- HLP 净值开始下降,吸收损失

第4阶段: HLP 资金不足
- 清算损失超过 HLP 可承受范围
- 部分被清算账户净值为负

第5阶段: ADL 触发 (层级 3)
- 系统自动触发 ADL 机制
- 对空头盈利账户按 Priority 排序
- 强制平仓高 Priority 账户

第6阶段: 市场恢复
- ADL 执行完毕,系统恢复正常
- 被 ADL 账户提前实现盈利 (但盈利减少)
- HLP 净值止损
```

---

### 5.3 影响分析

**被 ADL 用户**:
- **正面**: 提前实现盈利,锁定收益
- **负面**: 盈利减少 (本可以赚更多)
- **情绪**: 部分高级交易者震惊和愤怒 (CoindDesk 报道)

**HLP 持有者**:
- **正面**: ADL 保护了 HLP 净值,避免更大损失
- **负面**: 仍然吸收了部分损失,净值下降

**系统整体**:
- **正面**: 系统保持偿付能力,未出现坏账
- **负面**: ADL 机制首次触发,引发市场关注和讨论

---

### 5.4 社区反应

**正面评价**:
- 系统设计有效,三层风险瀑布成功防止系统性风险
- ADL 机制透明,用户可提前查询自己的 ADL 排名
- 比传统 CEX 的社会化损失机制更公平

**负面评价**:
- 高级交易者认为 ADL 是"惩罚赢家"
- 部分用户质疑 HLP 的风险承受能力
- 建议增加 HLP 资金池深度

---

## 6. Hermes DEX 是否会面临类似情况?

### 6.1 风险评估

**场景 1: 极端行情导致大量清算**

Hermes 应对:
1. **Fillable Price 机制**: 清算价格更接近破产价格,保险基金收益更多
2. **独立保险基金**: 不依赖外部流动性提供者,资金更稳定
3. **去杠杆机制**: 负 TNC 时立即触发去杠杆,及时切断损失

**结论**: Hermes 的设计更保守,保险基金压力可能较 HLP 更小。

---

**场景 2: 保险基金耗尽**

Hermes 应对:
1. **去杠杆机制**: 保险基金不足时,强制对手方按破产价格去杠杆
2. **社会化损失**: 对手方承担部分损失,确保系统偿付能力
3. **提现门控**: 负 TNC 时冻结提现,防止损失扩大

**结论**: Hermes 有完善的后备机制,但极端情况下仍会触发去杠杆。

---

### 6.2 优化建议

**对 Hermes**:
1. 增加保险基金储备,提高抗风险能力
2. 考虑引入类似 HLP 的社区流动性提供者机制
3. 优化 Fillable Price 算法,平衡用户体验和保险基金收益
4. 增加去杠杆透明度,让用户提前了解自己的去杠杆优先级

**对 Hyperliquid**:
1. 增加 HLP 资金池深度,减少 ADL 触发频率
2. 优化 ADL 排序算法,考虑用户持仓时长等因素
3. 为 HLP 持有者提供更多风险对冲工具
4. 增强市场监控,预警极端行情

---

## 7. 关键差异总结表

| 维度 | Hyperliquid | Hermes DEX | 优劣对比 |
|-----|-------------|------------|---------|
| **清算方式** | 市价单 (3 层瀑布) | 限价单 (订单簿匹配) | Hyperliquid 更灵活 |
| **清算价格** | Mark Price | Fillable Price (ABR) | Hyperliquid 对用户更优 |
| **保险基金** | HLP (社区流动性提供者) | 独立保险基金 | Hermes 更稳定 |
| **去杠杆触发** | HLP 不足 + 负净值 | TNC < 0 | Hermes 更严格 |
| **去杠杆价格** | Mark Price | Bankruptcy Price | Hermes 更公平 (被去杠杆账户归零) |
| **对手方排序** | 盈利率 × 杠杆 | 盈利率 | Hyperliquid 考虑杠杆因素 |
| **透明度** | ✅ 高 (ADL 排名可查) | ✅ 高 (链上事件记录) | 相当 |
| **手续费** | ✅ 无清算手续费 | ❌ 有清算手续费 | Hyperliquid 更优 |
| **历史事件** | 2025 年 10 月首次 ADL | 未公开 | Hyperliquid 已经过压力测试 |
| **社区反馈** | 褒贬不一 (ADL 争议) | 未知 (未大规模运行) | - |

---

## 8. 结论

### 8.1 Hyperliquid 优势

1. **用户友好**: 无清算手续费,保留更多资金
2. **社区共治**: HLP 机制让社区共享利润、共担风险
3. **多层保护**: 三层风险瀑布逐级吸收风险
4. **已验证**: 2025 年 ADL 事件证明系统设计有效

### 8.2 Hermes 优势

1. **风险可控**: 独立保险基金,不依赖外部流动性提供者
2. **价格公平**: 破产价格去杠杆,确保被去杠杆账户归零
3. **确定性强**: 严格的去杠杆逻辑,适合复杂场景 (最终结算)
4. **保险基金收益**: Fillable Price 机制增加保险基金收入

### 8.3 总体评价

- **Hyperliquid**: 更注重**用户体验**和**社区参与**,适合追求低手续费和透明度的用户
- **Hermes DEX**: 更注重**系统稳定性**和**风险控制**,适合追求确定性和公平性的用户

两者各有优劣,Hermes 可以学习 Hyperliquid 的 HLP 机制和无手续费设计,Hyperliquid 可以学习 Hermes 的精确价格控制和确定性去杠杆逻辑。

---

## 9. 参考资料

### Hyperliquid 官方文档
- [Liquidations | Hyperliquid Docs](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/liquidations)
- [Auto-deleveraging | Hyperliquid Docs](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/auto-deleveraging)
- [Liquidations | Hyperliquid Wiki](https://hyperliquid-co.gitbook.io/wiki/architecture/hypercore/dex/clearinghouse/liquidations)

### 行业分析文章
- [Hyperliquid Activates Cross-Margin Auto-Deleveraging for the First Time](https://wublock.substack.com/p/hyperliquid-activates-cross-margin)
- [How Auto-Deleveraging Works on Crypto Perp Platforms](https://www.coindesk.com/markets/2025/10/11/how-adl-on-crypto-perp-trading-platforms-can-shock-and-anger-even-advanced-traders)
- [Liquidation Alchemy Part 2: From BitMEX to Hyperliquid & Beyond](https://www.blockhead.co/2025/12/05/liquidation-alchemy-part-2-from-bitmex-to-hyperliquid-beyond/)
- [ADL Mechanism on Crypto Exchanges](https://thecoinomist.com/learn/adl-mechanism-crypto-exchanges-liquidation/)

### 技术对比
- [Technical Architecture Comparison: Hyperliquid, dYdX, and Lighter.xyz](https://medium.com/@gwrx2005/technical-architecture-comparison-hyperliquid-dydx-and-lighter-xyz-2fd005854a7e)
- [Hyperliquid Liquidation | Udit Samani's Website](https://uditsamani.com/hype-liquidation/)

### Hermes DEX 内部文档
- `notes/business/liquidations/liquidation_mechanism.md` - Hermes 清算机制详解
- `notes/business/liquidations/deleveraging_mechanism.md` - Hermes 去杠杆机制详解
- `notes/business/prd/clob_prd.md` - CLOB 模块产品需求文档

---

**文档版本**: v1.0
**创建时间**: 2026-01-06
**作者**: Claude Sonnet 4.5
**状态**: ✅ 完成
