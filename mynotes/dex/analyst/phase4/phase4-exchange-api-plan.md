# Phase 4: Exchange Write API 实施计划

## Context

前三阶段（indexer pipeline、query API、实时聚合）已完成。Phase 4 核心目标：实现基于 Hyperliquid 风格的 Exchange 写交易 API，让用户通过 EIP-712 签名下单，无需 Sui 钱包。

**核心流程**: 用户 EIP712 签名 → `POST /exchange` → dex-api 验证签名 → 构建 Sui 交易 → 提交到 Sui 节点

**已有基础**: EIP712 类型系统完整（`sui-types/src/dex/eip712.rs` 1441行）、`PlaceOrderWithEip712` 命令、签名验证、builder 方法全部就绪。

---

## Step 1: 数据库 — dex_sui_objects 表 + 迁移

存储 Sui 共享对象的 ID 和 initial_shared_version，供 exchange API 构建交易时查询。

### 1.1 新建 migration

**文件**: `crates/dex-indexer/migrations/2026-02-11-000001_dex_sui_objects/up.sql`

```sql
CREATE TABLE dex_sui_objects (
    -- 对象类型标识: "global_accounts", "perpetual_state"
    object_type     TEXT NOT NULL,
    -- 关联 ID（如 perpetual_id），global_accounts 为 0
    type_id         INT NOT NULL DEFAULT 0,
    -- Sui Object ID (32 bytes)
    object_id       BYTEA NOT NULL,
    -- initial_shared_version（构建交易需要）
    initial_shared_version  BIGINT NOT NULL,
    -- 最后更新的 checkpoint
    updated_at_cp   BIGINT NOT NULL DEFAULT 0,
    -- 时间戳
    timestamp_ms    BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (object_type, type_id)
);

CREATE INDEX idx_dex_sui_objects_type ON dex_sui_objects (object_type);
```

**文件**: `crates/dex-indexer/migrations/2026-02-11-000001_dex_sui_objects/down.sql`
```sql
DROP TABLE IF EXISTS dex_sui_objects;
```

### 1.2 Schema 和模型

**修改**: `crates/dex-indexer/src/schema/mod.rs` — 添加 diesel table 定义 + `StoredSuiObject` struct

### 1.3 数据填充方案

修改现有 handler 在处理 `PerpetualCreatedEvent` 时同步写入 `dex_sui_objects` 表。对于 `GlobalAccounts`，在首次处理 checkpoint 时从事件中提取。

**修改**: `crates/dex-indexer/src/handlers/perpetuals.rs` — commit() 时同时 upsert `dex_sui_objects`

---

## Step 2: Exchange 类型定义 (dex-types)

### 2.1 请求类型

**新文件**: `crates/dex-types/src/api/exchange.rs`

```rust
/// Exchange API 请求 — 对标 Hyperliquid POST /exchange
#[derive(Deserialize)]
pub struct ExchangeRequest {
    pub action: ExchangeAction,
    pub nonce: u64,
    pub signature: SignatureData,
    #[serde(rename = "vaultAddress")]
    pub vault_address: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExchangeAction {
    Order(OrderAction),
    Cancel(CancelAction),
    CancelByCloid(CancelByCloidAction),
}

/// 下单 — 对标 Hyperliquid order action
#[derive(Deserialize)]
pub struct OrderAction {
    pub orders: Vec<OrderWire>,
    pub grouping: String, // "na" | "normalTpsl" | "positionTpsl"
}

/// 单个订单 — 字段命名对标 Hyperliquid
#[derive(Deserialize)]
pub struct OrderWire {
    pub a: u32,           // asset index (perpetual_id)
    pub b: bool,          // is_buy
    pub p: String,        // price (subticks as string)
    pub s: String,        // size (quantums as string)
    pub r: bool,          // reduce_only
    pub t: OrderType,     // order type object
    pub c: Option<String>, // client order ID (hex)
    // DEX 扩展字段
    #[serde(rename = "subaccountNumber")]
    pub subaccount_number: Option<u32>,
}

#[derive(Deserialize)]
pub struct OrderType {
    pub limit: Option<LimitOrderType>,
    pub trigger: Option<TriggerOrderType>,
}

#[derive(Deserialize)]
pub struct LimitOrderType {
    pub tif: String, // "Gtc" | "Ioc" | "Alo"
}

/// EIP-712 签名数据
#[derive(Deserialize)]
pub struct SignatureData {
    pub r: String,   // "0x..." hex
    pub s: String,   // "0x..." hex
    pub v: u8,       // 27 or 28
}

/// 撤单
#[derive(Deserialize)]
pub struct CancelAction {
    pub cancels: Vec<CancelWire>,
}

#[derive(Deserialize)]
pub struct CancelWire {
    pub a: u32,    // asset index
    pub o: u64,    // order ID
}

/// 按 client ID 撤单
#[derive(Deserialize)]
pub struct CancelByCloidAction {
    pub cancels: Vec<CancelByCloidWire>,
}

#[derive(Deserialize)]
pub struct CancelByCloidWire {
    pub asset: u32,
    pub cloid: String,
}
```

### 2.2 响应类型

```rust
/// Exchange API 统一响应
#[derive(Serialize)]
pub struct ExchangeResponse {
    pub status: String,        // "ok" | "err"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<ExchangeResponseData>,
}

#[derive(Serialize)]
pub struct ExchangeResponseData {
    #[serde(rename = "type")]
    pub response_type: String, // "order" | "cancel"
    pub data: serde_json::Value,
}
```

**修改**: `crates/dex-types/src/api/mod.rs` — 添加 `pub mod exchange;`

---

## Step 3: dex-api Exchange Handler

### 3.1 AppState 扩展

**修改**: `crates/dex-api/src/server.rs`

```rust
pub struct AppState {
    pub db: Arc<Db>,
    pub redis: Option<redis::aio::MultiplexedConnection>,
    pub cache: Option<Arc<crate::cache::RedisCache>>,
    pub ws_state: Option<ws::WsState>,
    // 新增
    pub exchange: Option<Arc<ExchangeState>>,
}
```

### 3.2 ExchangeState

**新文件**: `crates/dex-api/src/exchange/mod.rs`

```rust
pub struct ExchangeState {
    pub sui_client: SuiClient,
    pub keystore: Mutex<Keystore>,
    pub sender: SuiAddress,
    /// 缓存: object_type -> (ObjectID, initial_shared_version)
    pub object_cache: RwLock<HashMap<String, (ObjectID, SequenceNumber)>>,
}
```

关键方法:
- `new(fullnode_url, db)` — 用种子生成密钥对（复用 dex-node-test 的 `index=0` 种子模式），从数据库加载 object 信息
- `get_global_accounts()` → `(ObjectID, SequenceNumber)` — 缓存优先，miss 时查 DB
- `get_perpetual_state(perpetual_id)` → `(ObjectID, SequenceNumber)` — 同上
- `submit_eip712_order(signature, params)` → 构建 + 签名 + 提交 Sui 交易
- `refresh_cache(db)` — 定时从 DB 刷新 object versions

### 3.3 Exchange Handler

**新文件**: `crates/dex-api/src/exchange/handlers.rs`

核心处理流程:
1. 解析 `ExchangeRequest`
2. 转换 `SignatureData` → `Eip712Signature`
3. 转换 `OrderWire` → `Eip712PlaceOrderParams`
4. 调用 `verify_place_order_with_eip712()` 预验证（复用 `sui-types/src/dex/eip712.rs`）
5. 查询 object versions（从缓存/DB）
6. 构建 `ProgrammableDexTransaction`（复用 `sui-types/src/dex_builder.rs`）
7. 创建 `TransactionData::new_dex()` + 签名 + 提交
8. 返回 `ExchangeResponse`

### 3.4 路由

**修改**: `crates/dex-api/src/server.rs`
```rust
// 在 create_router 中添加
if state.exchange.is_some() {
    router = router.route("/exchange", post(exchange_handler));
}
```

### 3.5 main.rs 扩展

**修改**: `crates/dex-api/src/main.rs` — 新增 CLI 参数:
- `--fullnode-url` / `SUI_FULLNODE_URL` — Sui 节点 RPC 地址
- exchange 功能仅在提供 fullnode-url 时启用

### 3.6 subaccountVersion 常量

**新文件或修改**: `crates/dex-api/src/exchange/constants.rs`

```rust
/// Subaccount 版本常量 — 此值不变，某些交易构建需要
pub const SUBACCOUNT_VERSION: u64 = 1;
```

### 3.7 依赖添加

**修改**: `crates/dex-api/Cargo.toml` — 添加:
- `sui-sdk` (for SuiClient)
- `sui-keys` (for Keystore)
- `sui-types` (已有，确认 eip712 feature)
- `shared-crypto` (for Intent signing)
- `fastcrypto` (for secp256k1)

---

## Step 4: Indexer — 写入 dex_sui_objects

### 4.1 perpetuals handler 扩展

**修改**: `crates/dex-indexer/src/handlers/perpetuals.rs`

在 `commit()` 方法中，处理 `PerpetualCreatedEvent` 后，额外 upsert `dex_sui_objects` 表:
- `object_type = "perpetual_state"`, `type_id = perpetual_id`
- `object_id` = 从事件中获取
- `initial_shared_version` = 从事件的 checkpoint 数据中获取

### 4.2 GlobalAccounts 写入

**方案**: 在首次处理到 `GlobalAccountsCreatedEvent`（或类似事件）时写入。如果没有专门事件，可在 API 启动时通过 RPC 查询后写入 DB（fallback）。

需检查事件定义中是否有 GlobalAccounts 创建事件。若无，则在 dex-api 的 exchange 初始化时，通过 `--global-accounts-id` 参数传入并写入 DB。

---

## Step 5: dex-node-test example2 — Exchange API 测试

**新文件**: `crates/dex-node-test/examples/exchange_api_test.rs`

流程:
1. 创建 secp256k1 测试密钥对（复用 e2e 测试中的 `create_eip712_signature` 模式）
2. 构建 `Eip712PlaceOrderParams`（nonce = 当前时间戳毫秒）
3. 计算 signing_hash → 签名 → 生成 `{r, s, v}`
4. 组装 Hyperliquid 格式的 JSON 请求
5. `POST` 到 `http://localhost:9100/exchange`
6. 验证响应 `status: "ok"`
7. 通过 `/info` 查询 `openOrders` 确认订单已上链

**关键复用**:
- `sui-types/src/dex/eip712.rs` 中的 `Eip712Domain::default()`, `signing_hash()`, `Eip712PlaceOrderParams`
- `fastcrypto::secp256k1::Secp256k1KeyPair` 签名
- `dex-node-test/src/api_client.rs` 查询验证

---

## Step 6: dex-test-panel — TradeView 切换按钮

**修改**: `dex-test-panel/src/components/panels/OrderEntryPanel.tsx`

添加 "SDK 模式 / Exchange API 模式" 切换:
- **SDK 模式**（默认）: 现有逻辑，POST 到 tx-gateway `/tx/order`
- **Exchange API 模式**:
  1. 用内置测试私钥（hardcoded secp256k1 seed）构建 EIP712 params
  2. 在浏览器端计算 signing_hash + 签名（需要 ethers.js 或 noble-secp256k1）
  3. POST 到 dex-api `/exchange`

**新增依赖**: `ethers`（或 `@noble/secp256k1` + `@noble/hashes`）

**新文件**: `dex-test-panel/src/api/exchangeClient.ts` — Exchange API 客户端
- `signAndPlaceOrder(params)` → 签名 + POST /exchange
- EIP712 domain/type 定义（TypeScript 版）
- 测试私钥签名逻辑

**UI 变更**:
- 在 OrderEntryPanel 顶部添加 toggle 按钮: `[SDK] [Exchange API]`
- Exchange API 模式下复用相同的表单字段
- 提交时调用 `exchangeClient.signAndPlaceOrder()` 替代 `txClient.placeOrder()`

---

## Step 7: dex-test-panel — MetaMask 页面

**新文件**: `dex-test-panel/src/components/panels/MetaMaskPanel.tsx`

功能:
1. **连接 MetaMask**: `window.ethereum.request({ method: 'eth_requestAccounts' })`
2. **显示钱包信息**: ETH 地址、对应 Sui 地址
3. **下单表单**: perpetual_id, side, price, quantity, subaccount_number
4. **EIP712 签名**: 使用 `eth_signTypedData_v4` 让 MetaMask 签名
5. **提交**: POST 签名结果到 `/exchange`
6. **结果展示**: 交易状态、digest、订单信息

**新文件**: `dex-test-panel/src/utils/eip712.ts`
- EIP712 domain 和 type 定义
- `buildPlaceOrderTypedData(params)` → 构建 MetaMask `eth_signTypedData_v4` 参数
- `parseSignature(sig)` → 拆分为 `{r, s, v}`

**修改**: `dex-test-panel/src/App.tsx` — 添加 MetaMask panel 到路由

**依赖**: 无额外 npm 依赖，直接使用 `window.ethereum` provider

---

## 文件变更清单

### 新文件
| 文件 | 说明 |
|------|------|
| `crates/dex-indexer/migrations/2026-02-11-000001_dex_sui_objects/up.sql` | 新表 migration |
| `crates/dex-indexer/migrations/2026-02-11-000001_dex_sui_objects/down.sql` | down migration |
| `crates/dex-types/src/api/exchange.rs` | Exchange 请求/响应类型 |
| `crates/dex-api/src/exchange/mod.rs` | ExchangeState + 路由 |
| `crates/dex-api/src/exchange/handlers.rs` | Exchange handler 逻辑 |
| `crates/dex-api/src/exchange/constants.rs` | subaccountVersion 等常量 |
| `crates/dex-node-test/examples/exchange_api_test.rs` | Exchange API 测试用例 |
| `dex-test-panel/src/api/exchangeClient.ts` | Exchange API 前端客户端 |
| `dex-test-panel/src/utils/eip712.ts` | EIP712 工具函数 |
| `dex-test-panel/src/components/panels/MetaMaskPanel.tsx` | MetaMask 下单页面 |

### 修改文件
| 文件 | 变更 |
|------|------|
| `crates/dex-indexer/src/schema/mod.rs` | 添加 dex_sui_objects 表定义 + StoredSuiObject |
| `crates/dex-indexer/src/handlers/perpetuals.rs` | commit() 时写入 dex_sui_objects |
| `crates/dex-types/src/api/mod.rs` | 添加 `pub mod exchange` |
| `crates/dex-api/src/lib.rs` | 添加 `pub mod exchange` |
| `crates/dex-api/src/server.rs` | AppState 扩展 + `/exchange` 路由 |
| `crates/dex-api/src/main.rs` | 新增 fullnode-url 参数 + exchange 初始化 |
| `crates/dex-api/Cargo.toml` | 添加 sui-sdk, sui-keys 等依赖 |
| `dex-test-panel/src/App.tsx` | 添加 MetaMask panel 路由 |
| `dex-test-panel/src/components/panels/OrderEntryPanel.tsx` | 添加模式切换 |
| `dex-test-panel/package.json` | 添加 ethers 依赖（用于 Exchange API 模式签名） |

### 关键复用
| 已有代码 | 复用位置 |
|---------|---------|
| `sui-types/src/dex/eip712.rs` → `verify_place_order_with_eip712()` | exchange handler 预验证 |
| `sui-types/src/dex/eip712.rs` → `Eip712PlaceOrderParams`, `Eip712Signature` | 类型转换 |
| `sui-types/src/dex_builder.rs` → `place_order_with_eip712()` | 交易构建 |
| `dex-node-test/src/client.rs` → `execute_dex_transaction()` 模式 | dex-api 交易提交 |
| `dex-node-test/src/client.rs` → 种子密钥生成模式 | dex-api gas sponsor 密钥 |
| `sui-e2e-tests/tests/dex_order_tests.rs` → `create_eip712_signature()` | example2 测试签名 |

---

## 实施顺序

```
Step 1 (DB migration)
  ↓
Step 2 (类型定义)           ← 可与 Step 1 并行
  ↓
Step 4 (indexer 写入)       ← 依赖 Step 1
  ↓
Step 3 (exchange handler)   ← 依赖 Step 1, 2
  ↓
Step 5 (example2 测试)      ← 依赖 Step 3
  ↓
Step 6 (前端切换按钮)       ← 依赖 Step 3
  ↓
Step 7 (MetaMask 页面)      ← 依赖 Step 3, 可与 Step 6 并行
```

---

## 验证方案

### 编译验证
```bash
cd dex-sui && cargo build -p dex-api -p dex-indexer -p dex-types -p dex-node-test
```

### 单元测试
```bash
cargo nextest run -p dex-api -p dex-types -p dex-indexer
```

### 数据库迁移
```bash
cd crates/dex-indexer
diesel migration run --database-url postgres://dex:dex123@localhost:5432/dex_indexer
docker exec -it dex-indexer-db psql -U dex -d dex_indexer -c '\d dex_sui_objects'
```

### E2E 测试
1. 启动环境: `docker compose up -d` (PostgreSQL + Redis)
2. 启动 Sui 节点 + indexer + dex-api (带 --fullnode-url)
3. 通过 tx-gateway 创建市场 + 充值
4. 运行 `cargo run --example exchange_api_test` 验证 exchange API
5. 打开 dex-test-panel，测试 SDK/Exchange API 模式切换
6. 安装 MetaMask，测试 MetaMask 页面签名下单

### curl 手动验证
```bash
# Exchange API 下单
curl -X POST http://localhost:9100/exchange \
  -H "Content-Type: application/json" \
  -d '{
    "action": {"type": "order", "orders": [...], "grouping": "na"},
    "nonce": 1707580800000,
    "signature": {"r": "0x...", "s": "0x...", "v": 27}
  }'

# 查询确认
curl -X POST http://localhost:9100/info \
  -d '{"type": "openOrders", "user": "0x..."}'
```
