# Vault 模块数据结构文档

## 模块概述

Vault (金库/流动性池) 模块管理 Megavault 系统,提供自动化做市和流动性管理功能。使用份额机制跟踪存款和提款。

**模块路径**: `protocol/x/vault`
**Store Key**: `vault`
**核心功能**: Megavault 份额管理、Vault 参数配置、订单自动刷新

---

## 一、StateStore (链上持久化存储)

### 1.1 总份额

**存储键**: `TotalSharesKey` = `"TotalShares"`

**数据结构**: `NumShares`

```go
type NumShares struct {
    NumShares SerializableInt  // 总份额数量
}
```

**业务含义**:
- Megavault 的总份额数量
- 份额价值 = Vault 总净值 / 总份额
- 存款时增加份额,提款时减少份额

---

### 1.2 所有者份额

**存储键**: `OwnerSharesKeyPrefix` + `OwnerAddress` = `"Owner:" + <address>`

**数据结构**: `NumShares`

**业务含义**:
- 每个所有者持有的 Vault 份额
- 提款金额 = 份额数 × 份额价值

---

### 1.3 所有者份额解锁

**存储键**: `OwnerShareUnlocksKeyPrefix` + `OwnerAddress` + `UnlockIndex`

**数据结构**: `ShareUnlock`

```go
type ShareUnlock struct {
    Shares SerializableInt      // 解锁份额数量
    UnlockBlockHeight uint32    // 解锁区块高度
}
```

**业务含义**:
- 提款需要等待解锁期
- 存储未来解锁计划
- 到达解锁高度后可提款

---

### 1.4 Vault 参数

**存储键**: `VaultParamsKeyPrefix` + `VaultType` + `VaultNumber`

**数据结构**: `VaultParams`

```go
type VaultParams struct {
    VaultId VaultId              // Vault 标识
    Status VaultStatus           // 状态(Active/Deactivated等)
    QuotingParams QuotingParams  // 报价参数
    // ... 其他参数
}
```

**业务含义**:
- 配置 Vault 的做市参数
- 包括价差、订单大小、刷新频率等
- 通过治理或管理员更新

---

### 1.5 Vault 地址映射

**存储键**: `VaultAddressKeyPrefix` + `VaultType` + `VaultNumber`

**数据结构**: `VaultId`

**业务含义**:
- Vault 地址到 Vault ID 的映射

---

### 1.6 最近订单客户端 ID

**存储键**: `MostRecentClientIdsKeyPrefix` + `VaultId`

**数据结构**: `MostRecentClientIds`

**业务含义**:
- 跟踪 Vault 最近放置的订单 ID
- 用于订单刷新和替换逻辑
- 确保订单簿始终有 Vault 的流动性

---

## 二、Megavault 工作流程

### 2.1 存款流程

```
用户存款:
1. 转账 USDC 到 Vault 模块账户
2. 计算份额数 = 存款金额 / 份额价值
3. 增加 TotalShares
4. 增加 OwnerShares[用户地址]
```

### 2.2 提款流程

```
用户提款:
1. 提交提款请求
2. 创建 ShareUnlock (锁定期)
3. 到达解锁高度后:
   a. 计算提款金额 = 份额数 × 份额价值
   b. 转账 USDC 给用户
   c. 减少 TotalShares 和 OwnerShares
```

### 2.3 订单管理

```
EndBlocker 每个区块:
1. 检查 Vault 订单是否需要刷新
2. 取消旧订单
3. 根据 QuotingParams 生成新订单
4. 更新 MostRecentClientIds
```

---

## 三、总结

**数据结构特点**:
- **份额机制**: 公平分配收益和损失
- **解锁期**: 防止闪电贷攻击
- **自动做市**: Vault 自动管理订单簿

**关键功能**:
- Megavault 流动性池
- 自动化做市
- 份额化存提款

---

**文档版本**: v1.0
**最后更新**: 2025-12-31
