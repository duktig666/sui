# Prices 模块数据结构文档

## 模块概述

Prices (价格预言机) 模块负责管理市场价格参数、存储预言机价格数据和维护货币对映射。本模块集成了 Slinky Oracle,通过 Vote Extensions 机制实现去中心化价格更新。

**模块路径**: `protocol/x/prices`
**Store Key**: `prices`
**核心功能**: 价格市场配置、预言机价格存储、Slinky 集成

---

## 一、StateStore (链上持久化存储)

### 1.1 市场参数配置

**存储键**: `MarketParamKeyPrefix` + `MarketId` = `"Param:" + <uint32>`

**数据结构**: `MarketParam`

```go
type MarketParam struct {
    Id uint32                  // 市场 ID
    Pair string                // 交易对,如 "BTC/USD"
    Exponent int32             // 价格指数
    MinExchanges uint32        // 最小交易所数量
    MinPriceChangePpm uint32   // 最小价格变化(PPM)
    ExchangeConfigJson string  // 交易所配置 JSON
}
```

**业务含义**:
- **Pair**: 货币对标识,如 "BTC/USD", "ETH/USD"
- **Exponent**: 价格精度,例如 `-5` 表示价格 = 链上值 × 10^(-5)
- **MinExchanges**: Slinky 聚合时需要的最少交易所数据源
- **MinPriceChangePpm**: 价格更新的最小变化阈值,过滤噪音
- **ExchangeConfigJson**: Slinky 配置,指定数据源和权重

**使用场景**:
- **价格验证**: 检查价格更新是否满足最小变化要求
- **Slinky 配置**: 预言机读取配置,从指定交易所获取价格
- **价格计算**: 根据 Exponent 将链上价格转换为实际价格

**数据访问**:
```go
keeper.GetMarketParam(ctx, marketId) -> MarketParam, found
keeper.SetMarketParam(ctx, marketParam)
```

**关键文件**: `protocol/x/prices/types/market_param.pb.go`

---

### 1.2 市场价格数据

**存储键**: `MarketPriceKeyPrefix` + `MarketId` = `"Price:" + <uint32>`

**数据结构**: `MarketPrice`

```go
type MarketPrice struct {
    Id uint32          // 市场 ID
    Exponent int32     // 价格指数(与 MarketParam 一致)
    Price uint64       // 当前价格(链上表示)
    BlockTimestamp *Timestamp  // 价格更新时间戳
}
```

**业务含义**:
- **Price**: 链上存储的价格值,实际价格 = Price × 10^(Exponent)
- **BlockTimestamp**: 价格最后更新的区块时间
- 由 Slinky Oracle 通过 Vote Extensions 更新

**使用场景**:
- **抵押品计算**: Subaccounts 模块读取价格计算账户净值
- **清算判断**: 检查账户是否水下
- **资金费率计算**: Perpetuals 模块计算溢价

**数据访问**:
```go
keeper.GetMarketPrice(ctx, marketId) -> MarketPrice, found
keeper.UpdateMarketPrices(ctx, updates []MarketPrice)
```

**关键文件**: `protocol/x/prices/types/market_price.pb.go`

---

### 1.3 货币对 ID 映射

**存储键**: `CurrencyPairIDPrefix` + `CurrencyPair` = `"CurrencyPairID:" + <string>`

**数据结构**: `uint32` (Market ID)

**业务含义**:
- 建立货币对字符串到市场 ID 的映射
- 例如: "BTC/USD" → MarketId = 0
- 便于通过货币对名称查找市场

**使用场景**:
- **Slinky 集成**: 通过货币对名称查找市场 ID
- **API 查询**: 用户查询特定货币对的价格

**数据访问**:
```go
keeper.GetMarketIdFromCurrencyPair(ctx, "BTC/USD") -> uint32, found
keeper.SetCurrencyPairIDToMarketID(ctx, "BTC/USD", marketId)
```

**关键文件**: `protocol/x/prices/keeper/currency_pair.go`

---

### 1.4 下一个市场 ID

**存储键**: `NextMarketIDKey` = `"NextMarketID"`

**数据结构**: `uint32`

**业务含义**:
- 存储下一个可用的市场 ID
- 每次创建新市场时自动递增

**数据访问**:
```go
keeper.GetNextMarketID(ctx) -> uint32
keeper.IncrementNextMarketID(ctx)
```

---

## 二、Slinky Oracle 集成

### 2.1 价格更新流程

```
Vote Extensions (每个区块):
1. ExtendVote: 验证器从 Slinky 获取最新价格
2. VerifyVoteExtension: 验证价格数据有效性
3. PrepareProposal: Proposer 聚合所有验证器的价格
4. ProcessProposal: 验证聚合后的价格
5. FinalizeBlock: 更新 MarketPrice
```

### 2.2 价格验证机制

**Towards 条件**:
- 新价格必须朝向预言机价格方向移动
- 防止价格被恶意操纵向错误方向变化

**Crossing 条件**:
- 如果新价格跨越预言机价格,必须在合理范围内
- 防止价格剧烈波动

---

## 三、总结

**数据结构特点**:
- **简洁高效**: 仅 4 个核心数据结构
- **Slinky 集成**: 通过 Vote Extensions 实现去中心化价格更新
- **精度控制**: Exponent 机制支持灵活的价格精度

**关键功能**:
- 预言机价格存储
- 市场配置管理
- 货币对映射

---

**文档版本**: v1.0
**最后更新**: 2025-12-31
