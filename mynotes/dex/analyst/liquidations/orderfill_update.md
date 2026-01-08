# OrderFill 订单履行状态更新机制详解

## 概述

**OrderFill State** (订单履行状态) 是 Hermes DEX 用于跟踪订单累计成交量的持久化状态机制。该机制记录每个订单的已成交数量,并支持自动清理过期订单的状态数据。

---

## 1. ABCI 阶段识别: OrderFill 在哪个阶段更新?

### 答案: **DeliverTx 阶段**

OrderFill 状态的更新发生在 **FinalizeBlock → DeliverTx** 阶段,具体是在处理 `MsgProposedOperations` 消息时。

### 完整时序图

```
区块 N-1 已提交
    ↓
区块 N 共识开始
    ↓
PrepareProposal (Proposer 节点)
    ├─ 从 MemClob 获取待提议的操作 (operationsToPropose)
    ├─ 生成 MsgProposedOperations 消息
    └─ 组装区块提案
    ↓
ProcessProposal (所有验证节点)
    ├─ 验证 MsgProposedOperations 的有效性
    └─ 返回 ACCEPT 或 REJECT
    ↓
FinalizeBlock (所有验证节点)
    ├─ PreBlock
    │   └─ 初始化模块状态
    │
    ├─ BeginBlock
    │   └─ 初始化区块状态
    │
    ├─ DeliverTx[0]: UpdateMarketPrices
    │   └─ 更新预言机价格
    │
    ├─ DeliverTx[1]: AddPremiumVotes
    │   └─ 添加资金费率投票
    │
    ├─ DeliverTx[2]: AcknowledgeBridges
    │   └─ 确认桥接事件
    │
    ├─ DeliverTx[3]: MsgProposedOperations ⭐关键阶段
    │   ├─ ProcessProposerMatches()
    │   │   ├─ 遍历 Operations (订单成交、清算、去杠杆)
    │   │   └─ 对每个订单成交调用 ProcessSingleMatch()
    │   │       ├─ 计算新的累计成交量
    │   │       ├─ 更新账户余额和持仓
    │   │       └─ 调用 setOrderFillAmountsAndPruning()
    │   │           ├─ 计算 pruneableBlockHeight
    │   │           └─ 调用 SetOrderFillAmount() ⭐更新 OrderFill 状态
    │   │               └─ 写入 KVStore (持久化)
    │   └─ ✅ OrderFill 状态在此阶段更新
    │
    ├─ DeliverTx[4...N]: 其他用户交易
    │   └─ 执行普通交易 (转账、质押等)
    │
    └─ EndBlock
        ├─ PruneStateFillAmountsForShortTermOrders() ⭐清理过期 OrderFill
        │   └─ 调用 PruneOrdersForBlockHeight()
        │       ├─ 读取 pruneableBlockHeight <= currentBlockHeight 的订单
        │       └─ 调用 RemoveOrderFillAmount() 删除 OrderFill 状态
        └─ 更新 MemClob
    ↓
Commit
    ├─ 提交 KVStore 到数据库 (包括 OrderFill 状态)
    ├─ 更新 AppHash
    └─ 保存区块
    ↓
区块 N 已提交
```

---

## 2. 数据结构详解

### 2.1 OrderFillState 定义

**位置**: `protocol/x/clob/types/order.proto`

```protobuf
message OrderFillState {
  // 订单已成交的累计数量 (base quantums)
  uint64 fill_amount = 1;

  // 该订单填充状态可以被清理的区块高度
  uint32 prunable_block_height = 2;
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|-----|------|------|
| `fill_amount` | `uint64` | 订单累计成交量 (base quantums),每次成交后累加 |
| `prunable_block_height` | `uint32` | 可清理区块高度,到达此高度后订单填充状态可被删除 |

**示例**:

```go
OrderFillState{
    FillAmount: 5000,           // 已成交 5000 base quantums (如 0.5 BTC)
    PrunableBlockHeight: 12345, // 在区块高度 12345 时可清理
}
```

---

### 2.2 OrderIdFillState 结构

**位置**: `protocol/x/clob/keeper/order_state.go:28-31`

```go
type OrderIdFillState struct {
    types.OrderFillState // 继承 OrderFillState
    OrderId types.OrderId // 订单 ID
}
```

**用途**: 在应用启动时用于"hydration"(将持久化状态恢复到内存 MemClob)

**示例**:

```go
OrderIdFillState{
    OrderFillState: OrderFillState{
        FillAmount: 5000,
        PrunableBlockHeight: 12345,
    },
    OrderId: OrderId{
        SubaccountId: SubaccountId{Owner: "dydx1...", Number: 0},
        ClientId: 42,
        OrderFlags: 0,
        ClobPairId: 1,
    },
}
```

---

## 3. 存储机制详解

### 3.1 存储键 (State Key)

**存储位置**: Cosmos SDK KVStore

**键前缀**: `OrderAmountFilledKeyPrefix = "OrderFillAmount/"`

**完整键格式**:

```
OrderAmountFilledKeyPrefix + OrderId.ToStateKey()
```

**OrderId.ToStateKey() 编码规则**:

```
[SubaccountId.Owner (bech32)] + [SubaccountId.Number (varint)] + [ClientId (varint)] + [OrderFlags (varint)] + [ClobPairId (varint)]
```

**示例**:

```
键:   "OrderFillAmount/dydx1abc...xyz:0:42:0:1"
值:   Protobuf(OrderFillState{FillAmount: 5000, PrunableBlockHeight: 12345})
```

---

### 3.2 存储操作函数

#### 函数 1: `SetOrderFillAmount()` - 写入 OrderFill 状态

**位置**: `protocol/x/clob/keeper/order_state.go:69-95`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `orderId types.OrderId`: 订单 ID
- `fillAmount satypes.BaseQuantums`: 累计成交量
- `prunableBlockHeight uint32`: 可清理区块高度

**核心逻辑**:

```go
func (k Keeper) SetOrderFillAmount(
    ctx sdk.Context,
    orderId types.OrderId,
    fillAmount satypes.BaseQuantums,
    prunableBlockHeight uint32,
) {
    // 步骤1: 构建 OrderFillState 对象
    var orderFillState = types.OrderFillState{
        FillAmount:          uint64(fillAmount),
        PrunableBlockHeight: prunableBlockHeight,
    }

    // 步骤2: 序列化为 Protobuf 字节
    orderFillStateBytes := k.cdc.MustMarshal(&orderFillState)

    // 步骤3: 获取 KVStore 实例
    store := prefix.NewStore(
        ctx.KVStore(k.storeKey),
        []byte(types.OrderAmountFilledKeyPrefix),
    )

    // 步骤4: 写入 KVStore (持久化)
    store.Set(
        orderId.ToStateKey(),
        orderFillStateBytes,
    )
}
```

**业务解释**:
- 每次订单成交时调用,更新累计成交量
- 覆盖写入 (如果键已存在,覆盖旧值)
- 在 Commit 阶段持久化到数据库

---

#### 函数 2: `GetOrderFillAmount()` - 读取 OrderFill 状态

**位置**: `protocol/x/clob/keeper/order_state.go:98-128`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `orderId types.OrderId`: 订单 ID

**输出**:
- `exists bool`: 是否存在
- `fillAmount satypes.BaseQuantums`: 累计成交量
- `prunableBlockHeight uint32`: 可清理区块高度

**核心逻辑**:

```go
func (k Keeper) GetOrderFillAmount(
    ctx sdk.Context,
    orderId types.OrderId,
) (exists bool, fillAmount satypes.BaseQuantums, prunableBlockHeight uint32) {
    // 步骤1: 获取 KVStore 实例
    store := ctx.KVStore(k.storeKey)
    prefixStore := prefix.NewStore(store, []byte(types.OrderAmountFilledKeyPrefix))

    // 步骤2: 读取 OrderFillState 字节
    orderFillStateBytes := prefixStore.Get(orderId.ToStateKey())

    // 步骤3: 如果不存在,返回默认值
    if orderFillStateBytes == nil {
        return false, 0, 0
    }

    // 步骤4: 反序列化 Protobuf 字节
    var orderFillState types.OrderFillState
    k.cdc.MustUnmarshal(orderFillStateBytes, &orderFillState)

    // 步骤5: 返回结果
    return true, satypes.BaseQuantums(orderFillState.FillAmount), orderFillState.PrunableBlockHeight
}
```

**业务解释**:
- 在订单成交前调用,获取当前累计成交量
- 用于验证订单是否超量成交
- 首次成交的订单返回 `exists = false`

---

#### 函数 3: `RemoveOrderFillAmount()` - 删除 OrderFill 状态

**位置**: `protocol/x/clob/keeper/order_state.go:262-283`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `orderId types.OrderId`: 订单 ID

**核心逻辑**:

```go
func (k Keeper) RemoveOrderFillAmount(ctx sdk.Context, orderId types.OrderId) {
    // 步骤1: 获取 KVStore 实例
    orderAmountFilledStore := prefix.NewStore(
        ctx.KVStore(k.storeKey),
        []byte(types.OrderAmountFilledKeyPrefix),
    )

    // 步骤2: 删除 OrderFill 状态
    orderAmountFilledStore.Delete(orderId.ToStateKey())

    // 步骤3: 如果启用了 gRPC 流,发送零填充量更新
    if k.GetFullNodeStreamingManager().Enabled() {
        allUpdates := types.NewOffchainUpdates()
        if message, success := off_chain_updates.CreateOrderUpdateMessage(
            ctx,
            orderId,
            0, // 总填充量为零 (已清理)
        ); success {
            allUpdates.AddUpdateMessage(orderId, message)
        }
        k.SendOrderbookUpdates(ctx, allUpdates)
    }
}
```

**业务解释**:
- 在 EndBlock 阶段调用,清理过期订单状态
- 释放存储空间,防止状态膨胀
- 通知订阅者订单已清理

---

## 4. 更新时机详解

### 4.1 何时更新 OrderFill?

OrderFill 状态在 **DeliverTx** 阶段,处理 `MsgProposedOperations` 消息时更新。

**完整调用链**:

```
FinalizeBlock
    ↓
DeliverTx(MsgProposedOperations)
    ↓
keeper/process_operations.go:ProcessProposerMatches()
    ├─ 遍历 Operations (订单成交、清算、去杠杆)
    └─ 对每个成交调用 ProcessSingleMatch()
        ↓
        keeper/process_single_match.go:ProcessSingleMatch()
            ├─ 步骤1: 获取当前 OrderFill 状态
            │   └─ GetOrderFillAmount(takerOrderId) → (exists, curTakerFillAmount, curTakerPruneableBlockHeight)
            │   └─ GetOrderFillAmount(makerOrderId) → (exists, curMakerFillAmount, curMakerPruneableBlockHeight)
            │
            ├─ 步骤2: 计算新的累计成交量
            │   └─ newTakerTotalFillAmount = curTakerFillAmount + fillAmount
            │   └─ newMakerTotalFillAmount = curMakerFillAmount + fillAmount
            │
            ├─ 步骤3: 验证不超量成交
            │   └─ if newTotalFillAmount > orderBaseQuantums: return error
            │
            ├─ 步骤4: 更新账户余额和持仓
            │   └─ persistMatchedOrders() → UpdateSubaccounts()
            │
            └─ 步骤5: 更新 OrderFill 状态 (持久化)
                ├─ setOrderFillAmountsAndPruning(takerOrder, newTakerTotalFillAmount, curTakerPruneableBlockHeight)
                │   ├─ 计算 pruneableBlockHeight
                │   │   └─ Short-Term: GoodTilBlock + ShortBlockWindow
                │   │   └─ Stateful: math.MaxUint32 (永不清理)
                │   ├─ AddOrdersForPruning(orderId, pruneableBlockHeight)
                │   └─ SetOrderFillAmount(orderId, newTotalFillAmount, pruneableBlockHeight) ⭐写入 KVStore
                │
                └─ setOrderFillAmountsAndPruning(makerOrder, newMakerTotalFillAmount, curMakerPruneableBlockHeight)
```

---

### 4.2 具体代码分析

#### 步骤 1: 获取当前 OrderFill 状态

**位置**: `protocol/x/clob/keeper/process_single_match.go:183-212`

```go
// Taker 订单
if !takerMatchableOrder.IsLiquidation() {
    // 获取 Taker 订单的当前成交量
    _, curTakerFillAmount, curTakerPruneableBlockHeight = k.GetOrderFillAmount(
        ctx,
        matchWithOrders.TakerOrder.MustGetOrder().OrderId,
    )

    // 计算新的总成交量
    newTakerTotalFillAmount, err = getUpdatedOrderFillAmount(
        matchWithOrders.TakerOrder.MustGetOrder().OrderId,
        matchWithOrders.TakerOrder.GetBaseQuantums(),
        curTakerFillAmount,
        fillAmount,
    )
}

// Maker 订单 (总是跟踪,包括清算订单)
_, curMakerFillAmount, curMakerPruneableBlockHeight = k.GetOrderFillAmount(
    ctx,
    matchWithOrders.MakerOrder.MustGetOrder().OrderId,
)

newMakerTotalFillAmount, err = getUpdatedOrderFillAmount(
    matchWithOrders.MakerOrder.MustGetOrder().OrderId,
    matchWithOrders.MakerOrder.GetBaseQuantums(),
    curMakerFillAmount,
    fillAmount,
)
```

**业务解释**:
- **清算订单 (Taker)**: 不跟踪 OrderFill (因为清算订单不可重放)
- **普通订单 (Taker/Maker)**: 需要跟踪 OrderFill,防止超量成交

---

#### 步骤 2: 更新 OrderFill 状态

**位置**: `protocol/x/clob/keeper/process_single_match.go:287-301`

```go
// Taker 订单 (非清算订单)
if !matchWithOrders.TakerOrder.IsLiquidation() {
    k.setOrderFillAmountsAndPruning(
        ctx,
        matchWithOrders.TakerOrder.MustGetOrder(),
        newTakerTotalFillAmount,
        curTakerPruneableBlockHeight,
    )
}

// Maker 订单 (总是更新)
k.setOrderFillAmountsAndPruning(
    ctx,
    matchWithOrders.MakerOrder.MustGetOrder(),
    newMakerTotalFillAmount,
    curMakerPruneableBlockHeight,
)
```

**核心函数: `setOrderFillAmountsAndPruning()`**

**位置**: `protocol/x/clob/keeper/process_single_match.go:741-784`

```go
func (k Keeper) setOrderFillAmountsAndPruning(
    ctx sdk.Context,
    order types.Order,
    newTotalFillAmount satypes.BaseQuantums,
    curPruneableBlockHeight uint32,
) {
    // 步骤1: 计算可清理区块高度
    pruneableBlockHeight := uint32(math.MaxUint32) // 默认: 永不清理 (Stateful 订单)

    if !order.IsStatefulOrder() {
        // Short-Term 订单: GoodTilBlock + ShortBlockWindow
        pruneableBlockHeight = lib.Max(
            order.GetGoodTilBlock() + types.ShortBlockWindow,
            curPruneableBlockHeight,
        )

        // 步骤2: 添加到清理队列
        k.AddOrdersForPruning(ctx, []types.OrderId{order.OrderId}, pruneableBlockHeight)
    }

    // 步骤3: 更新 OrderFill 状态 (持久化)
    k.SetOrderFillAmount(
        ctx,
        order.OrderId,
        newTotalFillAmount,
        pruneableBlockHeight,
    )
}
```

**业务解释**:
- **Short-Term 订单**: 在 `GoodTilBlock + ShortBlockWindow` 后清理
- **Stateful 订单**: 永不自动清理 (`math.MaxUint32`)
- 每次成交都会更新 `pruneableBlockHeight`,保留最新的过期时间

---

## 5. 清理机制详解

### 5.1 清理时机

OrderFill 状态在 **EndBlock** 阶段清理。

**调用链**:

```
EndBlock
    ↓
clob/abci.go:EndBlocker()
    ↓
keeper/keeper.go:PruneStateFillAmountsForShortTermOrders()
    ├─ 获取当前区块高度 blockHeight
    └─ 调用 PruneOrdersForBlockHeight(blockHeight)
        ↓
        keeper/order_state.go:PruneOrdersForBlockHeight()
            ├─ 步骤1: 获取 pruneableBlockHeight == blockHeight 的所有订单
            ├─ 步骤2: 遍历订单,验证 prunableBlockHeight <= blockHeight
            ├─ 步骤3: 调用 RemoveOrderFillAmount() 删除 OrderFill 状态
            └─ 步骤4: 从清理队列删除订单 ID
```

---

### 5.2 清理逻辑详解

#### 函数: `PruneOrdersForBlockHeight()` - 清理指定高度的订单

**位置**: `protocol/x/clob/keeper/order_state.go:211-236`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `blockHeight uint32`: 当前区块高度

**输出**:
- `prunedOrderIds []types.OrderId`: 已清理的订单 ID 列表

**核心逻辑**:

```go
func (k Keeper) PruneOrdersForBlockHeight(ctx sdk.Context, blockHeight uint32) (prunedOrderIds []types.OrderId) {
    // 步骤1: 获取 pruneableBlockHeight == blockHeight 的订单存储
    potentiallyPrunableOrdersStore := k.GetPruneableOrdersStore(ctx, blockHeight)
    it := potentiallyPrunableOrdersStore.Iterator(nil, nil)
    defer it.Close()

    // 步骤2: 遍历所有潜在可清理订单
    for ; it.Valid(); it.Next() {
        var orderId types.OrderId
        k.cdc.MustUnmarshal(it.Value(), &orderId)

        // 步骤3: 获取订单的实际 prunableBlockHeight
        exists, _, prunableBlockHeight := k.GetOrderFillAmount(ctx, orderId)

        // 步骤4: 验证是否可以清理
        if exists && prunableBlockHeight <= blockHeight {
            // 删除 OrderFill 状态
            k.RemoveOrderFillAmount(ctx, orderId)
            prunedOrderIds = append(prunedOrderIds, orderId)

            // 异常检测: prunableBlockHeight 不应小于 blockHeight
            if prunableBlockHeight < blockHeight {
                log.ErrorLog(ctx,
                    "prunableBlockHeight is less than blockHeight, this should never happen.",
                    "prunableBlockHeight", prunableBlockHeight,
                )
            }
        }

        // 步骤5: 从清理队列删除订单 ID
        potentiallyPrunableOrdersStore.Delete(it.Key())
    }

    return prunedOrderIds
}
```

**业务解释**:
- 每个区块只清理 `prunableBlockHeight <= currentBlockHeight` 的订单
- 清理后释放 KVStore 空间,防止状态膨胀
- 通过 gRPC 流通知订阅者订单已清理

---

### 5.3 清理队列管理

#### 函数: `AddOrdersForPruning()` - 添加到清理队列

**位置**: `protocol/x/clob/keeper/order_state.go:141-149`

**输入参数**:
- `ctx sdk.Context`: 区块链上下文
- `orderIds []types.OrderId`: 订单 ID 列表
- `prunableBlockHeight uint32`: 可清理区块高度

**核心逻辑**:

```go
func (k Keeper) AddOrdersForPruning(ctx sdk.Context, orderIds []types.OrderId, prunableBlockHeight uint32) {
    // 获取指定高度的清理队列存储
    store := k.GetPruneableOrdersStore(ctx, prunableBlockHeight)

    // 将订单 ID 写入清理队列
    for _, orderId := range orderIds {
        store.Set(
            orderId.ToStateKey(),
            k.cdc.MustMarshal(&orderId),
        )
    }
}
```

**存储键格式**:

```
<PrunableOrdersKeyPrefix><height>:<order_id>
```

**示例**:

```
键:   "PrunableOrders/12345:dydx1abc...xyz:0:42:0:1"
值:   Protobuf(OrderId)
```

**业务解释**:
- 按区块高度组织清理队列,提高清理效率
- 允许同一订单多次添加 (覆盖写入)
- 在清理时一次性处理所有该高度的订单

---

## 6. 完整示例解析

### 示例: Short-Term 订单的 OrderFill 生命周期

**前置条件**:
```
用户 Alice:
- 账户余额: 10,000 USDC
- 当前区块: 1000

订单参数:
- 订单类型: Short-Term Limit Order
- 方向: 买入
- 数量: 10 BTC
- 价格: 50,000 USDC
- GoodTilBlock: 1010 (10 个区块后过期)
- ClientId: 42
```

**时间线**:

---

#### 时刻 1: 区块 1000 - 订单放置

```
用户提交订单:
OrderId{
    SubaccountId: {Owner: "dydx1alice...", Number: 0},
    ClientId: 42,
    OrderFlags: 0,
    ClobPairId: 1,
}

此时:
- OrderFill 状态不存在 (GetOrderFillAmount() → exists = false)
- 订单进入 MemClob,等待匹配
```

---

#### 时刻 2: 区块 1005 - 部分成交 (第一次)

```
匹配成交:
- fillAmount = 3 BTC (30% 成交)
- Maker: Bob

执行流程 (DeliverTx 阶段):

步骤1: GetOrderFillAmount(Alice订单)
  → (exists = false, curFillAmount = 0, curPruneableBlockHeight = 0)

步骤2: 计算新成交量
  → newTotalFillAmount = 0 + 3 = 3 BTC

步骤3: 计算 pruneableBlockHeight
  → pruneableBlockHeight = GoodTilBlock + ShortBlockWindow
  → pruneableBlockHeight = 1010 + 20 = 1030

步骤4: 更新 OrderFill 状态 (写入 KVStore)
  → SetOrderFillAmount(Alice订单, fillAmount = 3 BTC, pruneableBlockHeight = 1030)

步骤5: 添加到清理队列
  → AddOrdersForPruning([Alice订单], 1030)

KVStore 状态:
键: "OrderAmountFilledKeyPrefix/dydx1alice...:0:42:0:1"
值: OrderFillState{FillAmount: 3, PrunableBlockHeight: 1030}
```

---

#### 时刻 3: 区块 1008 - 部分成交 (第二次)

```
匹配成交:
- fillAmount = 5 BTC (累计 80% 成交)
- Maker: Carol

执行流程 (DeliverTx 阶段):

步骤1: GetOrderFillAmount(Alice订单)
  → (exists = true, curFillAmount = 3 BTC, curPruneableBlockHeight = 1030)

步骤2: 计算新成交量
  → newTotalFillAmount = 3 + 5 = 8 BTC

步骤3: 验证不超量
  → 8 BTC <= 10 BTC ✅ 通过

步骤4: 计算 pruneableBlockHeight
  → pruneableBlockHeight = max(1010 + 20, 1030) = 1030 (不变)

步骤5: 更新 OrderFill 状态 (覆盖写入)
  → SetOrderFillAmount(Alice订单, fillAmount = 8 BTC, pruneableBlockHeight = 1030)

KVStore 状态:
键: "OrderAmountFilledKeyPrefix/dydx1alice...:0:42:0:1"
值: OrderFillState{FillAmount: 8, PrunableBlockHeight: 1030} (已更新)
```

---

#### 时刻 4: 区块 1010 - 订单过期

```
订单到达 GoodTilBlock:
- GoodTilBlock = 1010
- 订单从 MemClob 移除 (不再匹配)
- OrderFill 状态仍保留 (pruneableBlockHeight = 1030 未到)

KVStore 状态:
键: "OrderAmountFilledKeyPrefix/dydx1alice...:0:42:0:1"
值: OrderFillState{FillAmount: 8, PrunableBlockHeight: 1030} (未清理)
```

---

#### 时刻 5: 区块 1030 - 清理 OrderFill 状态

```
EndBlock 阶段:

步骤1: 调用 PruneStateFillAmountsForShortTermOrders()
  → blockHeight = 1030

步骤2: 调用 PruneOrdersForBlockHeight(1030)
  ├─ 从清理队列读取 pruneableBlockHeight = 1030 的订单
  ├─ 发现 Alice 订单 (pruneableBlockHeight = 1030)
  ├─ 验证: 1030 <= 1030 ✅ 可清理
  └─ 调用 RemoveOrderFillAmount(Alice订单)

步骤3: 删除 OrderFill 状态
  → orderAmountFilledStore.Delete("dydx1alice...:0:42:0:1")

步骤4: 发送 gRPC 流更新 (如果启用)
  → CreateOrderUpdateMessage(orderId, fillAmount = 0) (已清理)

KVStore 状态:
键: "OrderAmountFilledKeyPrefix/dydx1alice...:0:42:0:1"
值: (已删除)

结果:
- OrderFill 状态已清理
- 释放存储空间
- 订单生命周期结束
```

---

## 7. 不同订单类型的 OrderFill 处理

| 订单类型 | 是否跟踪 OrderFill | pruneableBlockHeight | 清理时机 |
|---------|------------------|----------------------|---------|
| **Short-Term** | ✅ 是 | `GoodTilBlock + ShortBlockWindow` | EndBlock (到达清理高度) |
| **Long-Term** | ✅ 是 | `math.MaxUint32` | 永不自动清理 |
| **Conditional** | ✅ 是 | `math.MaxUint32` | 永不自动清理 |
| **Liquidation (Taker)** | ❌ 否 | N/A | N/A (不跟踪) |
| **Liquidation (Maker)** | ✅ 是 | 取决于 Maker 订单类型 | 取决于 Maker 订单类型 |

**业务解释**:
- **Short-Term 订单**: 过期后 20 个区块清理,防止状态膨胀
- **Stateful 订单**: 永久保留,用于跨区块成交验证
- **清算订单 (Taker)**: 不跟踪 (一次性订单,不可重放)

---

## 8. 关键代码路径总结

### 8.1 OrderFill 更新路径

```
FinalizeBlock
    ↓
DeliverTx(MsgProposedOperations)
    ↓
keeper/process_operations.go:ProcessProposerMatches()
    ↓
keeper/process_single_match.go:ProcessSingleMatch()
    ├─ GetOrderFillAmount() (行 187, 209)
    ├─ getUpdatedOrderFillAmount() (行 193, 215)
    ├─ persistMatchedOrders() (行 227)
    └─ setOrderFillAmountsAndPruning() (行 288, 296, 741-784)
        ├─ AddOrdersForPruning() (行 773)
        └─ SetOrderFillAmount() (行 778) ⭐写入 KVStore
```

### 8.2 OrderFill 清理路径

```
EndBlock
    ↓
clob/abci.go:EndBlocker()
    ↓
keeper/keeper.go:PruneStateFillAmountsForShortTermOrders()
    ↓
keeper/order_state.go:PruneOrdersForBlockHeight() (行 211-236)
    ├─ GetPruneableOrdersStore() (行 212)
    ├─ GetOrderFillAmount() (行 219)
    └─ RemoveOrderFillAmount() (行 221) ⭐删除 KVStore
```

---

## 9. 常见问题解答 (FAQ)

### Q1: 为什么清算订单 (Taker) 不跟踪 OrderFill?

**答案**: 清算订单是一次性订单,只能在账户可清算时放置,无法重放。因此不需要跟踪累计成交量。

**代码体现** (`process_single_match.go:182`):

```go
if !takerMatchableOrder.IsLiquidation() {
    // 只有非清算订单才跟踪 OrderFill
}
```

---

### Q2: Stateful 订单的 OrderFill 何时清理?

**答案**: Stateful 订单 (Long-Term, Conditional) 的 OrderFill 永不自动清理 (`pruneableBlockHeight = math.MaxUint32`)。

**原因**: Stateful 订单可以跨多个区块成交,需要持久跟踪累计成交量。

---

### Q3: 同一订单在不同区块多次成交,OrderFill 如何更新?

**答案**: 每次成交都会**覆盖写入** OrderFill 状态,累加成交量。

**示例**:
```
区块 1000: fillAmount = 3 BTC
区块 1005: fillAmount = 3 + 5 = 8 BTC (覆盖写入)
区块 1010: fillAmount = 8 + 2 = 10 BTC (覆盖写入)
```

---

### Q4: pruneableBlockHeight 会更新吗?

**答案**: 会。每次成交时,`pruneableBlockHeight` 会更新为 `max(新计算值, 旧值)`。

**代码体现** (`process_single_match.go:753`):

```go
pruneableBlockHeight = lib.Max(
    order.GetGoodTilBlock() + types.ShortBlockWindow,
    curPruneableBlockHeight,
)
```

**业务解释**: 如果订单被替换 (Replace),新的 `GoodTilBlock` 可能更晚,需要延长清理时间。

---

### Q5: OrderFill 状态占用多少存储空间?

**答案**: 每个 OrderFill 状态占用约 **50-100 字节**。

**计算**:
```
OrderId (键): ~40 字节 (bech32 地址 + varint)
OrderFillState (值): ~12 字节 (Protobuf: uint64 + uint32)
总计: ~50-100 字节 (含索引开销)
```

**影响**: 假设 100 万个活跃订单,占用约 **50-100 MB** 存储。

---

## 10. 相关文件索引

| 文件路径 | 关键函数/内容 | 行号 |
|---------|-------------|------|
| `protocol/x/clob/keeper/order_state.go` | `SetOrderFillAmount()` | 69-95 |
| `protocol/x/clob/keeper/order_state.go` | `GetOrderFillAmount()` | 98-128 |
| `protocol/x/clob/keeper/order_state.go` | `RemoveOrderFillAmount()` | 262-283 |
| `protocol/x/clob/keeper/order_state.go` | `AddOrdersForPruning()` | 141-149 |
| `protocol/x/clob/keeper/order_state.go` | `PruneOrdersForBlockHeight()` | 211-236 |
| `protocol/x/clob/keeper/order_state.go` | `GetAllOrderFillStates()` | 37-65 |
| `protocol/x/clob/keeper/process_single_match.go` | `ProcessSingleMatch()` | 44-317 |
| `protocol/x/clob/keeper/process_single_match.go` | `setOrderFillAmountsAndPruning()` | 741-784 |
| `protocol/x/clob/keeper/keeper.go` | `PruneStateFillAmountsForShortTermOrders()` | 287-294 |
| `protocol/x/clob/abci.go` | `EndBlocker()` | - |
| `protocol/x/clob/types/order.proto` | `OrderFillState` 定义 | - |

---

**文档版本**: v1.0
**创建时间**: 2026-01-06
**作者**: Claude Sonnet 4.5
**状态**: ✅ 完成
