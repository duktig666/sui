# tx-gateway 技术方案

> 版本: V1
> 日期: 2026-02-09
> 状态: 设计阶段

## 1. 背景与目标

### 1.1 问题

dex-test-panel 目前只有只读功能（查询 dex-api），无法执行交易操作（下单、撤单、充值等）。

### 1.2 为什么需要 Rust 网关

DEX 使用自定义的 `ProgrammableDexTransaction`（`TransactionKind::ProgrammableDex`），这是 Sui fork 扩展的非标准交易类型。TypeScript Sui SDK 只支持标准 `ProgrammableTransaction`，无法直接构建此类交易。

因此需要创建 Rust 薄网关层，复用现有 `DexClient` 代码，将前端 JSON 请求转换为链上交易。

### 1.3 演进路径

```
Phase 1（本方案）：
  前端 → tx-gateway (独立 Axum 服务, :3200) → Sui Node
  简单 JSON 格式，快速验证

Phase 2（未来）：
  前端 → dex-api /exchange (Hyperliquid 格式) → Sui Node
  tx-gateway 逻辑迁入 dex-api，tx-gateway 退役
```

---

## 2. 架构设计

### 2.1 系统拓扑

```
dex-test-panel (React, :3100)
    ├─ GET/POST → dex-api (:9100)        # 只读查询（已有）
    └─ POST     → tx-gateway (:3200)     # 交易操作（新建）
                      ↓
                  DexClient
                      ↓
                  Sui Node (:9000)
```

### 2.2 部署位置

tx-gateway 作为 `dex-node-test` crate 的新 binary target，与现有 examples 共享 `DexClient` 代码：

```
dex-sui/crates/dex-node-test/
├── src/
│   ├── lib.rs          # +pub mod gateway
│   ├── client.rs       # DexClient（已有，复用）
│   ├── config.rs       # DexTestConfig（已有，复用）
│   ├── gateway.rs      # 网关模块（新建）
│   └── bin/
│       └── tx_gateway.rs  # 网关入口（新建）
└── Cargo.toml          # +axum, +tower-http, +[[bin]]
```

### 2.3 为什么放在 dex-node-test 而非独立 crate

- `DexClient` 已实现全部交易操作（faucet、deposit、withdraw、order、cancel）
- 网关仅是 HTTP ↔ DexClient 的薄转换层，无独立业务逻辑
- 避免重复依赖 `sui-sdk`、`sui-types` 等重量级 crate
- Phase 2 迁入 dex-api 时可整体删除，无遗留代码

---

## 3. Rust 网关详细设计

### 3.1 核心数据结构

```rust
/// 网关共享状态
pub struct GatewayState {
    client: Arc<DexClient>,
    shared_objects: Arc<RwLock<SharedObjects>>,
    config: GatewayConfig,
}

/// 缓存的共享对象 ID（链上创建后记录）
pub struct SharedObjects {
    global_accounts_id: Option<ObjectID>,
    perpetual_states: HashMap<u32, ObjectID>,  // perpetual_id → ObjectID
}

/// 网关配置
pub struct GatewayConfig {
    faucet_url: String,
}
```

**设计要点**：
- `DexClient` 用 `Arc` 共享（内部 `SuiClient` 线程安全，`Keystore` 签名方法是 `&self`）
- `SharedObjects` 用 `RwLock` 保护写入：`setup` 写入，`order/deposit/withdraw` 读取
- 共享对象 ID 仅缓存 ObjectID，每次交易前通过 `get_shared_object_version()` 获取最新 `initial_shared_version`

### 3.2 路由设计

| 路由 | 方法 | 功能 | 复用 DexClient 方法 |
|------|------|------|---------------------|
| `/tx/faucet` | POST | 请求 SUI 测试币 | `request_faucet()` |
| `/tx/setup` | POST | 创建 GlobalAccounts + Perpetual | `create_global_accounts()` + `create_perpetual()` |
| `/tx/deposit` | POST | DEX 账户充值 | `deposit()` |
| `/tx/withdraw` | POST | DEX 账户提款 | `withdraw()` |
| `/tx/order` | POST | 下单（限价/市价） | `place_limit_order()` / `place_market_order()` |
| `/tx/cancel` | POST | 撤单 | `cancel_order()` |
| `/tx/status` | GET | 查看网关状态 | `sender()` + SharedObjects |
| `/tx/address` | GET | 查看钱包地址 | `sender()` |

### 3.3 请求/响应格式

#### 通用响应

```json
{
  "success": true,
  "message": "Deposit successful",
  "digest": "0xabc123...",       // 交易哈希（成功时）
  "data": { ... }               // 附加数据（可选）
}
```

#### `/tx/faucet` — 请求测试币

```
POST /tx/faucet
// 无请求体

响应:
{ "success": true, "message": "Faucet request successful" }
```

#### `/tx/setup` — 初始化环境

```json
// 请求
{
  "perpetual_id": 0,           // 可选，默认 0
  "liquidity_tier_id": 0,     // 可选，默认 0
  "atomic_resolution": -10     // 可选，默认 -10
}

// 响应
{
  "success": true,
  "message": "Setup complete",
  "data": {
    "global_accounts_id": "0x...",
    "perpetual_state_id": "0x...",
    "perpetual_id": 0
  }
}
```

**幂等性**：如果 `global_accounts_id` 已存在则跳过创建，只创建新的 perpetual。

#### `/tx/deposit` — 充值

```json
// 请求
{
  "subaccount_number": 0,
  "amount": "1000000000"       // 字符串，避免 JS u128 精度问题
}

// 响应
{ "success": true, "message": "Deposit successful", "digest": "0x..." }
```

#### `/tx/withdraw` — 提款

```json
// 请求
{
  "subaccount_number": 0,
  "amount": "500000000"
}
```

#### `/tx/order` — 下单

```json
// 请求
{
  "perpetual_id": 0,
  "side": "buy",               // "buy" | "sell"
  "quantity": "100",           // 字符串（u64 quantums）
  "price": "50000",           // 字符串（u64 subticks），限价单必填
  "order_type": "limit"       // "limit" | "market"
}

// 响应
{
  "success": true,
  "message": "Limit buy order placed",
  "digest": "0x...",
  "data": {
    "client_id": 1738999999,   // 自动生成（时间戳低32位）
    "order_id": "..."
  }
}
```

**client_id 生成策略**：`SystemTime::now().duration_since(UNIX_EPOCH).as_millis() as u32`，满足单用户测试场景。

#### `/tx/cancel` — 撤单

```json
// 请求
{
  "perpetual_id": 0,
  "client_id": 1738999999,
  "subaccount_number": 0
}
```

#### `/tx/status` — 网关状态

```
GET /tx/status

// 响应
{
  "address": "0x73a6...adc0",
  "global_accounts_id": "0xabc...",
  "perpetual_states": {
    "0": "0xdef..."
  }
}
```

#### `/tx/address` — 钱包地址

```
GET /tx/address

// 响应
{ "address": "0x73a6...adc0" }
```

### 3.4 共享对象版本管理

DEX 交易需要两个共享对象：`GlobalAccounts` 和 `PerpetualState`。每次交易前必须查询最新的 `initial_shared_version`：

```
每次 deposit/withdraw/order/cancel 操作：
1. 从 SharedObjects 缓存读取 ObjectID
2. 调用 get_shared_object_version(object_id) 获取 initial_shared_version
3. 用最新版本构建交易
```

这是因为共享对象的 `initial_shared_version` 在对象首次变为共享状态时确定，之后不变。但必须在交易构建时指定正确值。

### 3.5 密钥管理

- 使用 `DexClient::connect_deterministic(config, index)` 连接
- `index=0` 始终产生相同地址（跨重启稳定）
- 密钥存储在 `TempDir` 中，进程退出销毁
- 链上余额（SUI Gas + DEX margin）不受网关重启影响
- CLI 参数 `--sender-index` 控制使用的密钥索引，默认 0

### 3.6 二进制入口

```
tx-gateway [OPTIONS]

OPTIONS:
    --fullnode-url <URL>       Sui 节点 RPC [默认: http://127.0.0.1:9000]
    --faucet-url <URL>         Faucet 地址 [默认: http://127.0.0.1:9123]
    --listen-address <ADDR>    监听地址 [默认: 0.0.0.0:3200]
    --sender-index <N>         密钥索引 [默认: 0]
    --gas-budget <N>           Gas 预算 [默认: 10000000]
```

复用 `DexTestConfig`（clap Parser），仅新增 `--listen-address` 和 `--sender-index`。

### 3.7 错误处理

所有错误统一返回：
```json
{
  "success": false,
  "message": "Transaction failed: insufficient gas"
}
```

HTTP 状态码：
- 成功操作：200
- 请求参数错误：400（JSON 解析失败、缺少必要字段）
- 链上交易失败：500（Gas 不足、余额不够、权限错误等）

### 3.8 跨域 (CORS)

使用 `tower_http::cors::CorsLayer::permissive()` 允许所有来源访问（前端 :3100 → 网关 :3200）。

---

## 4. 前端集成设计

### 4.1 新增文件

```
dex-test-panel/src/
├── api/
│   ├── txTypes.ts        # 请求/响应类型定义
│   └── txClient.ts       # 网关 API 客户端
└── components/panels/
    ├── FaucetPanel.tsx    # 水龙头 + 充值面板
    └── OrderEntryPanel.tsx # 下单面板（嵌入 TradingPanel）
```

### 4.2 API 客户端层

#### txTypes.ts — 类型定义

```typescript
// 请求类型
interface SetupRequest {
  perpetual_id?: number;
  liquidity_tier_id?: number;
  atomic_resolution?: number;
}

interface DepositRequest {
  subaccount_number: number;
  amount: string;  // u128 字符串
}

interface WithdrawRequest {
  subaccount_number: number;
  amount: string;
}

interface OrderRequest {
  perpetual_id: number;
  side: "buy" | "sell";
  quantity: string;  // u64 字符串
  price?: string;    // 限价单必填
  order_type: "limit" | "market";
}

interface CancelRequest {
  perpetual_id: number;
  client_id: number;
  subaccount_number: number;
}

// 响应类型
interface TxResponse {
  success: boolean;
  message: string;
  digest?: string;
  data?: Record<string, unknown>;
}

interface StatusResponse {
  address: string;
  global_accounts_id?: string;
  perpetual_states: Record<string, string>;
}
```

#### txClient.ts — API 客户端

- 基础 URL：从 `localStorage("dex-gateway-url")` 读取，默认 `http://localhost:3200`
- 7 个导出函数：`requestFaucet()`, `setupAccounts()`, `deposit()`, `withdraw()`, `placeOrder()`, `cancelOrder()`, `getStatus()`
- 统一错误处理：解析 JSON 中的 `success` 字段，失败时抛出 `message`

### 4.3 FaucetPanel

独立面板，通过 Sidebar 导航访问。包含三个区域：

```
┌─────────────────────────────────────┐
│  Account Status                     │
│  Address: 0x73a6...adc0            │
│  GlobalAccounts: 0xabc... (或 N/A)  │
│  Perpetual 0: 0xdef... (或 N/A)    │
├─────────────────────────────────────┤
│  Quick Actions                      │
│  [1. Request SUI from Faucet     ] │
│  [2. Setup Accounts              ] │
│  [3. Deposit]  Amount: [________] │
│  [4. Withdraw] Amount: [________] │
├─────────────────────────────────────┤
│  操作日志                           │
│  ✅ Faucet request successful       │
│  ✅ Setup complete: GA=0x..., PS=0x.│
│  ❌ Deposit failed: insufficient gas│
└─────────────────────────────────────┘
```

**交互流程**：
1. 进入面板自动调用 `getStatus()` 显示当前状态
2. 顺序执行：Faucet → Setup → Deposit
3. 每步操作完成后刷新状态
4. 操作日志滚动显示所有结果

### 4.4 OrderEntryPanel

嵌入 TradingPanel 右侧，作为下单组件：

```
┌──────────────────────┐
│  [Buy]    [Sell]     │  ← Side 选择器
│  [Limit]  [Market]   │  ← Order Type
│                      │
│  Price:  [50000   ]  │  ← 限价单才显示
│  Qty:    [100     ]  │
│                      │
│  [Place BUY Order ]  │  ← 颜色跟随 side
│                      │
│  ✅ Order placed      │
│  digest: 0xabc...    │
└──────────────────────┘
```

**样式规则**：
- Buy 选中：`bg-bid` (#26a69a 绿)；Sell 选中：`bg-ask` (#ef5350 红)
- 提交按钮颜色跟随 side
- Market 模式下隐藏 Price 输入

### 4.5 TradingPanel 布局调整

将现有 4 列网格改为 5 列，右侧加入 OrderEntryPanel：

```
当前 (grid-cols-4):
  K线(3) + OrderBook(1)
  Fills(4)

调整后 (grid-cols-5):
  K线(3) + OrderBook(1) + OrderEntry(1)
  Fills(5)
```

### 4.6 Sidebar 变更

1. **PanelId 新增** `"faucet"`
2. **Trading 组** 新增导航项 `{ id: "faucet", label: "Faucet & Deposit" }`
3. **Settings 区域** 新增 Gateway URL 输入框（`localStorage("dex-gateway-url")`），与现有 API URL 设置并列

### 4.7 Settings 存储

| key | 默认值 | 用途 |
|-----|--------|------|
| `dex-api-url` | `/api`（已有） | dex-api 查询 URL |
| `dex-gateway-url` | `http://localhost:3200`（新增） | tx-gateway 交易 URL |
| `dex-user-address` | 空（已有） | 查询用户地址 |

---

## 5. 文件变更清单

### 新建文件 (6)

| 文件 | 说明 | 预估行数 |
|------|------|----------|
| `dex-sui/crates/dex-node-test/src/gateway.rs` | 网关路由 + 处理器 | ~400 |
| `dex-sui/crates/dex-node-test/src/bin/tx_gateway.rs` | 网关二进制入口 | ~60 |
| `dex-test-panel/src/api/txTypes.ts` | 交易请求/响应类型 | ~50 |
| `dex-test-panel/src/api/txClient.ts` | 网关 API 客户端 | ~60 |
| `dex-test-panel/src/components/panels/FaucetPanel.tsx` | 水龙头 + 充值面板 | ~130 |
| `dex-test-panel/src/components/panels/OrderEntryPanel.tsx` | 下单面板 | ~120 |

### 修改文件 (5)

| 文件 | 变更内容 |
|------|----------|
| `dex-sui/crates/dex-node-test/Cargo.toml` | +`axum.workspace = true`、+`tower-http.workspace = true`、+`[[bin]]` |
| `dex-sui/crates/dex-node-test/src/lib.rs` | +`pub mod gateway;` |
| `dex-test-panel/src/App.tsx` | +FaucetPanel 导入和注册 |
| `dex-test-panel/src/components/layout/Sidebar.tsx` | +`"faucet"` PanelId、+导航项、+Gateway URL 设置 |
| `dex-test-panel/src/components/panels/TradingPanel.tsx` | +OrderEntryPanel 导入、grid-cols-4 → grid-cols-5 |

---

## 6. 关键实现细节

### 6.1 DexClient 线程安全分析

```rust
pub struct DexClient {
    sui_client: SuiClient,         // Arc<RpcClient> 内部，线程安全
    keystore: Keystore,            // sign_secure 是 &self
    sender: SuiAddress,            // Copy
    gas_budget: u64,               // Copy
    temp_dir: Option<TempDir>,     // 不需要并发访问
}
```

**问题**：`DexClient` 的 `execute_dex_transaction` 方法是 `&self`，但需要查询 gas coin + 签名 + 发送，多个并发请求可能使用同一个 gas coin 导致冲突。

**解决方案**：网关层加 `Mutex` 序列化交易执行，单用户测试场景下不影响体验：

```rust
pub struct GatewayState {
    client: Arc<DexClient>,
    tx_lock: Arc<Mutex<()>>,  // 序列化交易执行
    shared_objects: Arc<RwLock<SharedObjects>>,
    config: GatewayConfig,
}
```

### 6.2 Setup 幂等性

```
POST /tx/setup { perpetual_id: 0 }

逻辑：
1. 检查 SharedObjects.global_accounts_id
   - None → 创建 GlobalAccounts，存储 ID
   - Some → 跳过
2. 检查 SharedObjects.perpetual_states[perpetual_id]
   - 不存在 → 创建 PerpetualState，存储 ID
   - 存在 → 跳过
3. 返回所有 ID
```

### 6.3 金额传输安全

- DEX 使用 `u128` 金额（充值/提款），超过 JS `Number.MAX_SAFE_INTEGER`
- 前端用字符串传输，网关解析为 `u128`
- 下单数量 (`quantums`) 和价格 (`subticks`) 是 `u64`，同样用字符串传输以保持一致

### 6.4 共享对象 ID 持久化

当前方案中 `SharedObjects` 仅在内存中缓存，网关重启后丢失。这意味着重启后需要重新执行 `/tx/setup`。

**可接受原因**：
- 测试场景通常 force-regenesis 重建链，旧对象不存在
- Setup 幂等，重复调用安全
- Phase 2 迁入 dex-api 后可用数据库持久化

---

## 7. 验证方法

### 7.1 编译检查

```bash
# Rust 编译
cargo check -p dex-node-test

# 前端编译
cd dex-test-panel && pnpm build
```

### 7.2 启动环境

```bash
# T1: Sui 节点 + Faucet
./target/debug/sui start --with-faucet --force-regenesis --fullnode-rpc-port 9000

# T2: tx-gateway
cargo run -p dex-node-test --bin tx-gateway -- \
  --fullnode-url http://127.0.0.1:9000 \
  --faucet-url http://127.0.0.1:9123

# T3: 前端
cd dex-test-panel && pnpm dev
```

### 7.3 curl 测试

```bash
# 查看地址
curl http://localhost:3200/tx/address

# 请求测试币
curl -X POST http://localhost:3200/tx/faucet

# 初始化环境
curl -X POST http://localhost:3200/tx/setup \
  -H "Content-Type: application/json" \
  -d '{"perpetual_id": 0}'

# 充值
curl -X POST http://localhost:3200/tx/deposit \
  -H "Content-Type: application/json" \
  -d '{"subaccount_number": 0, "amount": "1000000000"}'

# 下单
curl -X POST http://localhost:3200/tx/order \
  -H "Content-Type: application/json" \
  -d '{"perpetual_id": 0, "side": "buy", "quantity": "100", "price": "50000", "order_type": "limit"}'

# 查看状态
curl http://localhost:3200/tx/status
```

### 7.4 前端测试流程

1. 打开 Faucet & Deposit 面板
2. 点击 "Request SUI from Faucet" → 确认成功
3. 点击 "Setup Accounts" → 确认 GlobalAccounts + Perpetual 创建
4. 输入金额 → 点击 "Deposit" → 确认成功
5. 切换到 Trading View → 使用 OrderEntry 下单
6. 在 Open Orders 面板确认订单存在

---

## 8. 风险与约束

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Gas coin 并发冲突 | 多个快速请求可能失败 | `Mutex` 序列化交易 |
| 网关重启丢失 SharedObjects | 需要重新 setup | 测试场景可接受；setup 幂等 |
| 单一签名者 | 无法模拟多用户 | `--sender-index` 支持不同密钥 |
| 无认证机制 | 任何人可调用 | 仅用于本地测试环境 |

---

## 9. 与 Phase 2 的关系

本方案是 Phase 1 快速验证方案。Phase 2 将：

1. 在 `dex-api` 中添加 `/exchange` 端点（Hyperliquid 格式）
2. 支持 EIP-712 或自定义签名验证
3. 支持多用户（每个用户持有自己的密钥）
4. tx-gateway 退役

Phase 1 代码全部在 `dex-node-test` crate 中，退役时直接删除 `gateway.rs` + `bin/tx_gateway.rs` + 移除依赖即可，不影响核心代码。
