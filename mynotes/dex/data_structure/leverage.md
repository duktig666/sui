# Leverage 模块数据结构文档

## 模块概述

Leverage (杠杆) 模块是一个集成模块,没有独立的 Keeper。杠杆数据存储在 CLOB 模块,杠杆逻辑分散在 CLOB 和 Subaccounts 模块中。

**模块路径**: 无独立模块
**核心功能**: 杠杆倍数设置、保证金计算集成

---

## 一、数据存储 (在 CLOB 模块)

### 1.1 杠杆设置

**存储位置**: CLOB 模块 StateStore

**存储键**: `LeverageKeyPrefix` + `SubaccountId` = `"Leverage:" + <SubaccountId>`

**数据结构**: `Leverage`

```go
type Leverage struct {
    SubaccountId SubaccountId  // 子账户 ID
    Leverage uint32            // 杠杆倍数(例如 20 表示 20x)
}
```

**业务含义**:
- 设置子账户的杠杆倍数
- 影响保证金要求计算
- 杠杆越高,保证金要求越低,风险越大

---

## 二、消息处理 (在 CLOB 模块)

### 2.1 更新杠杆消息

**消息**: `MsgUpdateLeverage` (在 CLOB 模块定义)

**处理逻辑**:
1. 验证杠杆倍数合法性
2. 检查账户是否满足新杠杆的保证金要求
3. 更新杠杆设置
4. 触发保证金重新计算

---

## 三、集成设计

### 3.1 模块协作

```
用户更新杠杆:
  MsgUpdateLeverage (CLOB)
    ├─> CLOB: 验证和存储
    └─> Subaccounts: 重新计算保证金要求

保证金计算:
  Subaccounts 模块
    ├─> 读取 CLOB.Leverage
    ├─> 读取 Perpetuals.LiquidityTier
    └─> 计算 IM = 仓位价值 / 杠杆倍数
```

---

## 四、总结

**设计特点**:
- **集成设计**: 无独立模块,逻辑分散
- **数据在 CLOB**: 杠杆数据存储在 CLOB 模块
- **计算在 Subaccounts**: 保证金计算在 Subaccounts 模块

**为什么没有独立模块?**:
- 杠杆功能简单,不需要独立 Keeper
- 与订单和保证金紧密耦合
- 集成设计减少模块间通信开销

---

**文档版本**: v1.0
**最后更新**: 2025-12-31
