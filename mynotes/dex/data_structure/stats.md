# Stats 模块数据结构文档

## 模块概述

Stats (统计) 模块负责追踪用户交易量、计算手续费分级和支持联盟费用分配。使用 30 天滚动窗口统计机制。

**模块路径**: `protocol/x/stats`
**Store Key**: `stats`
**核心功能**: 用户交易统计、Epoch 统计、手续费分级

---

## 一、StateStore (链上持久化存储)

### 1.1 用户统计数据

**存储键**: `UserStatsKeyPrefix` + `UserAddress` = `"User:" + <address>`

**数据结构**: `UserStats`

```go
type UserStats struct {
    TakerNotional uint64   // Taker 累计成交额(quote quantums)
    MakerNotional uint64   // Maker 累计成交额(quote quantums)
}
```

**业务含义**:
- 追踪用户过去 30 天的累计交易量
- 区分 Taker(吃单)和 Maker(挂单)成交量
- 用于手续费分级和联盟费用计算

---

### 1.2 Epoch 统计数据

**存储键**: `EpochStatsKeyPrefix` + `EpochNumber` = `"Epoch:" + <uint32>`

**数据结构**: `EpochStats`

```go
type EpochStats struct {
    EpochNumber uint32           // Epoch 编号
    Stats []UserEpochStats       // 该 Epoch 内的用户统计
}

type UserEpochStats struct {
    User string            // 用户地址
    TakerNotional uint64   // Taker 成交额
    MakerNotional uint64   // Maker 成交额
}
```

**业务含义**:
- 每个 Epoch(时间窗口)的统计数据
- 30 天滚动窗口:保留最近 30 个 Epoch
- 旧 Epoch 自动删除

---

### 1.3 全局统计

**存储键**: `GlobalStatsKey` = `"Global"`

**数据结构**: `GlobalStats`

```go
type GlobalStats struct {
    TotalNotional uint64  // 全局累计成交额
}
```

---

### 1.4 统计元数据

**存储键**: `StatsMetadataKey` = `"Metadata"`

**数据结构**: `StatsMetadata`

```go
type StatsMetadata struct {
    OldestEpochRetained uint32  // 最早保留的 Epoch 编号
    LatestEpochNumber uint32    // 最新 Epoch 编号
}
```

---

## 二、数据流

### 2.1 统计更新流程

```
订单成交 → 更新 UserStats
         → 更新 EpochStats
         → 更新 GlobalStats

每个 Epoch 结束:
- 删除旧 Epoch (> 30 天)
- 更新元数据
```

---

## 三、总结

**数据结构特点**:
- **滚动窗口**: 30 天自动清理机制
- **双重统计**: 用户级 + Epoch 级
- **手续费分级**: 支持动态费率调整

---

**文档版本**: v1.0
**最后更新**: 2025-12-31
