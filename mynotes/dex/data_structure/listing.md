# Listing 模块数据结构文档

## 模块概述

Listing (市场上市) 模块负责管理无权限市场上市流程、市场数量硬上限和上市 Vault 存款参数。本文档描述了 Listing 模块中的所有数据结构。

**模块路径**: `protocol/x/listing`
**Store Key**: `listing`
**核心功能**: 无权限市场创建、市场硬上限管理、上市存款参数配置

---

## 一、StateStore (链上持久化存储)

### 1.1 市场硬上限

**存储键**: `HardCapForMarketsKey` = `"HardCapForMarkets"`

**数据结构**: `uint32`

**业务含义**:
- 限制可创建的市场总数上限
- 防止无限制创建市场导致系统资源耗尽
- 通过治理提案调整

**使用场景**:
- **市场创建验证**: 检查当前市场数量是否超过硬上限
- **容量规划**: 评估系统容量和扩展需求

**数据访问**:
```go
// 读取
hardCap := keeper.GetMarketsHardCap(ctx)

// 写入(通过治理)
keeper.SetMarketsHardCap(ctx, newCap)
```

**关键文件**:
- `protocol/x/listing/keeper/hard_cap.go`

---

### 1.2 上市 Vault 存款参数

**存储键**: `ListingVaultDepositParamsKey` = `"ListingVaultDepositParams"`

**数据结构**: `ListingVaultDepositParams`

```go
type ListingVaultDepositParams struct {
    NewMarketMinQuoteQuantums uint64  // 新市场最小报价数量(quote quantums)
    MainVaultMinDepositPpm uint32     // 主 Vault 最小存款比例(PPM)
}
```

**业务含义**:

**NewMarketMinQuoteQuantums 新市场最小报价数量**:
- 创建新市场时必须向 Vault 存入的最小金额
- 单位: quote quantums (通常是 USDC)
- 目的: 确保新市场有足够初始流动性

**MainVaultMinDepositPpm 主 Vault 最小存款比例**:
- 存入主 Vault 的最小比例
- 单位: PPM (parts-per-million)
- 例如: 500000 = 50%,即总存款的 50% 必须存入主 Vault

**使用场景**:
- **无权限市场创建**: 验证存款金额是否满足要求
- **Vault 分配**: 计算主 Vault 和子 Vault 的存款比例

**数据访问**:
```go
// 读取
params := keeper.GetListingVaultDepositParams(ctx)

// 写入(通过治理)
keeper.SetListingVaultDepositParams(ctx, params)
```

**关键文件**:
- `protocol/x/listing/types/params.pb.go`
- `protocol/x/listing/keeper/params.go`

---

## 二、TransientStore

Listing 模块没有使用 TransientStore。

---

## 三、MemStore

Listing 模块没有专门的 MemStore 数据。

---

## 四、数据结构关系总览

### 4.1 无权限市场创建流程

```
用户 → MsgCreateMarketPermissionless
  ├─> 验证:当前市场数 < HardCapForMarkets
  ├─> 验证:存款金额 >= NewMarketMinQuoteQuantums
  ├─> 计算:主 Vault 存款 = 总金额 × MainVaultMinDepositPpm
  ├─> 创建:新的 Perpetual、ClobPair、Market
  └─> 存款:转账到 Vault
```

### 4.2 模块间数据交互

```
Listing 模块触发:
  ├─> Perpetuals: 创建新永续合约
  ├─> CLOB: 创建新交易对
  ├─> Prices: 创建新价格市场
  └─> Vault: 转移初始流动性存款
```

---

## 五、总结

**数据结构特点**:
- **极简设计**: 仅 2 个配置参数,专注核心功能
- **治理驱动**: 所有参数通过治理调整,确保去中心化
- **容量限制**: 硬上限防止系统过载

**关键功能**:
- 无权限市场创建
- 市场数量控制
- 初始流动性要求

---

**文档版本**: v1.0
**最后更新**: 2025-12-31
**作者**: Claude Sonnet 4.5 + Hermes DEX Team
