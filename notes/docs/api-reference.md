# Token Chain - API 参考文档

本文档提供 Token Chain JSON-RPC API 的完整参考。

---

## 📡 API 概述

### 基本信息

- **协议**: JSON-RPC 2.0
- **传输**: HTTP/HTTPS
- **默认端点**: `http://127.0.0.1:9000`
- **内容类型**: `application/json`

### 通用请求格式

所有 RPC 请求遵循 JSON-RPC 2.0 规范：

```json
{
  "jsonrpc": "2.0",
  "method": "<method_name>",
  "params": [<param1>, <param2>, ...],
  "id": 1
}
```

### 通用响应格式

**成功响应**：
```json
{
  "jsonrpc": "2.0",
  "result": <result_value>,
  "id": 1
}
```

**错误响应**：
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": <error_code>,
    "message": "<error_message>"
  },
  "id": 1
}
```

---

## 🔗 API 方法列表

| 方法 | 功能 | 类别 |
|------|------|------|
| [submitTransaction](#submittransaction) | 提交交易 | 交易 |
| [getBalance](#getbalance) | 查询余额 | 查询 |
| [getNonce](#getnonce) | 查询 nonce | 查询 |
| [getStatus](#getstatus) | 查询节点状态 | 节点 |
| [getTransaction](#gettransaction) | 查询交易信息 | 查询 |

---

## 📝 详细 API 文档

### submitTransaction

提交一笔交易到区块链。

#### 方法名
`submitTransaction`

#### 参数

| 参数名 | 类型 | 必需 | 描述 |
|--------|------|------|------|
| transaction | Transaction | ✅ | 交易对象（Mint 或 Transfer） |

#### Transaction 类型

**Mint Transaction** - 铸造新代币：
```typescript
{
  "Mint": {
    "to": Address,      // 接收地址（32字节数组）
    "amount": number    // 铸造数量（u64）
  }
}
```

**Transfer Transaction** - 转账：
```typescript
{
  "Transfer": {
    "from": Address,    // 发送地址（32字节数组）
    "to": Address,      // 接收地址（32字节数组）
    "amount": number,   // 转账数量（u64）
    "nonce": number     // 发送者当前 nonce（u64）
  }
}
```

#### 返回值

| 类型 | 描述 |
|------|------|
| string | 交易哈希（十六进制字符串，带 0x 前缀） |

#### 示例

**请求 - Mint 代币**：
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "submitTransaction",
    "params": [{
      "Mint": {
        "to": [97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "amount": 1000
      }
    }],
    "id": 1
  }'
```

**响应**：
```json
{
  "jsonrpc": "2.0",
  "result": "0xabcd1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab",
  "id": 1
}
```

**请求 - 转账**：
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "submitTransaction",
    "params": [{
      "Transfer": {
        "from": [97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "to": [98,111,98,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "amount": 300,
        "nonce": 0
      }
    }],
    "id": 2
  }'
```

**响应**：
```json
{
  "jsonrpc": "2.0",
  "result": "0xef567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12",
  "id": 2
}
```

#### 错误情况

| 错误代码 | 错误消息 | 原因 |
|---------|---------|------|
| 1 | Invalid nonce: expected X, got Y | Nonce 不匹配 |
| 1 | Insufficient balance: has X, needs Y | 余额不足 |
| 1 | Node is not running | 节点未运行 |
| 1 | Execution error: ... | 执行失败 |

**错误响应示例**：
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": 1,
    "message": "Insufficient balance: has 500, needs 1000"
  },
  "id": 2
}
```

---

### getBalance

查询指定地址的代币余额。

#### 方法名
`getBalance`

#### 参数

| 参数名 | 类型 | 必需 | 描述 |
|--------|------|------|------|
| address | Address | ✅ | 账户地址（32字节数组） |

#### 返回值

| 类型 | 描述 |
|------|------|
| number | 余额（u64） |

#### 示例

**请求**：
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getBalance",
    "params": [[97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
    "id": 3
  }'
```

**响应**：
```json
{
  "jsonrpc": "2.0",
  "result": 700,
  "id": 3
}
```

#### 注意事项

- 如果地址不存在，返回 `0`
- 余额始终为非负数（u64）
- 最大值: 18,446,744,073,709,551,615

---

### getNonce

查询指定地址的当前 nonce 值。

#### 方法名
`getNonce`

#### 参数

| 参数名 | 类型 | 必需 | 描述 |
|--------|------|------|------|
| address | Address | ✅ | 账户地址（32字节数组） |

#### 返回值

| 类型 | 描述 |
|------|------|
| number | 当前 nonce（u64） |

#### 示例

**请求**：
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getNonce",
    "params": [[97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]],
    "id": 4
  }'
```

**响应**：
```json
{
  "jsonrpc": "2.0",
  "result": 2,
  "id": 4
}
```

#### Nonce 说明

- **初始值**: 新地址的 nonce 为 `0`
- **递增规则**: 每次成功的 Transfer 交易后 +1
- **用途**: 防止交易重放攻击
- **验证**: 提交交易时，nonce 必须等于当前值

**正确使用**：
```
1. 查询当前 nonce: getNonce(alice) → 0
2. 提交交易: Transfer(..., nonce: 0)
3. 查询新 nonce: getNonce(alice) → 1
4. 下次交易: Transfer(..., nonce: 1)
```

---

### getStatus

查询节点运行状态。

#### 方法名
`getStatus`

#### 参数

无

#### 返回值

| 字段 | 类型 | 描述 |
|------|------|------|
| node_id | number | 节点 ID |
| running | boolean | 节点是否运行中 |
| rpc_addr | string | RPC 服务地址 |

#### 示例

**请求**：
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getStatus",
    "params": [],
    "id": 5
  }'
```

**响应**：
```json
{
  "jsonrpc": "2.0",
  "result": {
    "node_id": 0,
    "running": true,
    "rpc_addr": "127.0.0.1:9000"
  },
  "id": 5
}
```

#### 使用场景

- 健康检查
- 监控节点状态
- 确认连接成功

---

### getTransaction

根据交易哈希查询交易详情。

#### 方法名
`getTransaction`

#### 参数

| 参数名 | 类型 | 必需 | 描述 |
|--------|------|------|------|
| hash | string | ✅ | 交易哈希（十六进制字符串） |

#### 返回值

| 类型 | 描述 |
|------|------|
| TransactionInfo \| null | 交易信息对象，如果不存在则返回 null |

#### TransactionInfo 结构

```typescript
{
  "tx_hash": string,           // 交易哈希
  "success": boolean,          // 是否成功
  "error": string | null,      // 错误信息（如果失败）
  "state_changes": [           // 状态变更列表
    {
      "address": Address,      // 受影响的地址
      "old_balance": number,   // 旧余额
      "new_balance": number,   // 新余额
      "old_nonce": number,     // 旧 nonce
      "new_nonce": number      // 新 nonce
    }
  ]
}
```

#### 示例

**请求**：
```bash
curl -X POST http://127.0.0.1:9000 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "getTransaction",
    "params": ["0xabcd1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab"],
    "id": 6
  }'
```

**响应 - 成功的交易**：
```json
{
  "jsonrpc": "2.0",
  "result": {
    "tx_hash": "0xabcd1234...",
    "success": true,
    "error": null,
    "state_changes": [
      {
        "address": [97,108,105,99,101,...],
        "old_balance": 1000,
        "new_balance": 700,
        "old_nonce": 0,
        "new_nonce": 1
      },
      {
        "address": [98,111,98,...],
        "old_balance": 0,
        "new_balance": 300,
        "old_nonce": 0,
        "new_nonce": 0
      }
    ]
  },
  "id": 6
}
```

**响应 - 不存在的交易**：
```json
{
  "jsonrpc": "2.0",
  "result": null,
  "id": 6
}
```

---

## 🔢 数据类型参考

### Address

32字节的账户地址，表示为数字数组。

**格式**：
```json
[byte0, byte1, byte2, ..., byte31]
```

**示例**：
```json
[97,108,105,99,101,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
```

**辅助方法**（仅在 Rust 客户端中可用）：
```rust
// 从字符串创建地址（用于测试）
let alice = Address::from_string("alice");
// 结果: [97,108,105,99,101,0,0,0,0,...]
```

**Python 示例**：
```python
def string_to_address(s: str) -> list:
    """将字符串转换为 Address 格式"""
    bytes_list = list(s.encode('utf-8'))
    # 填充到 32 字节
    return bytes_list + [0] * (32 - len(bytes_list))

alice_addr = string_to_address("alice")
# [97, 108, 105, 99, 101, 0, 0, ..., 0]
```

**JavaScript 示例**：
```javascript
function stringToAddress(s) {
    const bytes = new TextEncoder().encode(s);
    const address = new Array(32).fill(0);
    for (let i = 0; i < Math.min(bytes.length, 32); i++) {
        address[i] = bytes[i];
    }
    return address;
}

const aliceAddr = stringToAddress("alice");
// [97, 108, 105, 99, 101, 0, 0, ..., 0]
```

---

## 🔐 认证与安全

### 当前版本

- ❌ **无认证**: 当前版本不需要认证
- ❌ **无加密**: 默认使用 HTTP（非 HTTPS）
- ⚠️ **仅限测试**: 不建议在生产环境使用

### 未来版本计划

- ✅ JWT 认证
- ✅ TLS/HTTPS 支持
- ✅ 交易签名验证
- ✅ IP 白名单

---

## 📊 速率限制

### 当前版本

- **无速率限制**: 当前版本没有请求速率限制
- **并发支持**: 支持并发请求
- **建议**: 客户端自行实现速率控制

### 推荐实践

**客户端限流示例** (Python):
```python
import time
from functools import wraps

def rate_limit(max_per_second):
    min_interval = 1.0 / max_per_second
    last_called = [0.0]

    def decorator(func):
        @wraps(func)
        def wrapper(*args, **kwargs):
            elapsed = time.time() - last_called[0]
            left_to_wait = min_interval - elapsed
            if left_to_wait > 0:
                time.sleep(left_to_wait)
            ret = func(*args, **kwargs)
            last_called[0] = time.time()
            return ret
        return wrapper
    return decorator

@rate_limit(100)  # 最多 100 req/s
def submit_transaction(tx):
    # 提交交易
    pass
```

---

## 🐛 错误处理

### 错误代码列表

| 代码 | 名称 | 描述 |
|------|------|------|
| -32700 | Parse error | JSON 解析错误 |
| -32600 | Invalid Request | 请求格式错误 |
| -32601 | Method not found | 方法不存在 |
| -32602 | Invalid params | 参数无效 |
| -32603 | Internal error | 内部错误 |
| 1 | Execution error | 交易执行错误 |

### 常见错误及解决方案

#### 1. Invalid nonce

**错误消息**：
```json
{
  "code": 1,
  "message": "Invalid nonce: expected 2, got 0"
}
```

**原因**: 提交的 nonce 与账户当前 nonce 不匹配

**解决**：
```bash
# 1. 查询当前 nonce
curl ... -d '{"method": "getNonce", "params": [<address>], ...}'

# 2. 使用正确的 nonce 提交交易
curl ... -d '{"method": "submitTransaction", "params": [{
  "Transfer": { ..., "nonce": <correct_nonce> }
}], ...}'
```

#### 2. Insufficient balance

**错误消息**：
```json
{
  "code": 1,
  "message": "Insufficient balance: has 500, needs 1000"
}
```

**原因**: 账户余额不足

**解决**：
```bash
# 1. 查询当前余额
curl ... -d '{"method": "getBalance", "params": [<address>], ...}'

# 2. 调整转账金额或先mint代币
```

#### 3. Node is not running

**错误消息**：
```json
{
  "code": 1,
  "message": "Node is not running"
}
```

**原因**: 节点未启动或已停止

**解决**：
```bash
# 启动节点
cargo run --bin simple-token-chain
```

#### 4. Method not found

**错误消息**：
```json
{
  "code": -32601,
  "message": "Method not found"
}
```

**原因**: 方法名拼写错误

**解决**: 检查方法名是否正确（区分大小写）

---

## 💡 最佳实践

### 1. Nonce 管理

**问题**: 并发提交交易时 nonce 冲突

**解决方案**：
```python
import threading

class NonceManager:
    def __init__(self, address, rpc_client):
        self.address = address
        self.rpc = rpc_client
        self.lock = threading.Lock()
        self.current_nonce = self.rpc.get_nonce(address)

    def get_next_nonce(self):
        with self.lock:
            nonce = self.current_nonce
            self.current_nonce += 1
            return nonce

    def reset(self):
        with self.lock:
            self.current_nonce = self.rpc.get_nonce(self.address)

# 使用
nonce_mgr = NonceManager(alice, rpc_client)
tx1_nonce = nonce_mgr.get_next_nonce()  # 0
tx2_nonce = nonce_mgr.get_next_nonce()  # 1
```

### 2. 错误重试

**指数退避重试**：
```python
import time

def retry_with_backoff(func, max_retries=3):
    for i in range(max_retries):
        try:
            return func()
        except Exception as e:
            if i == max_retries - 1:
                raise
            wait_time = (2 ** i) * 0.1  # 0.1s, 0.2s, 0.4s
            time.sleep(wait_time)
```

### 3. 批量查询

**并行查询多个余额**：
```python
import asyncio

async def get_balances_async(addresses, rpc_client):
    tasks = [rpc_client.get_balance_async(addr) for addr in addresses]
    return await asyncio.gather(*tasks)

# 使用
balances = asyncio.run(get_balances_async([alice, bob, charlie], rpc))
```

### 4. 交易确认

**等待交易确认**：
```python
def wait_for_confirmation(tx_hash, rpc_client, timeout=30):
    start_time = time.time()
    while time.time() - start_time < timeout:
        tx_info = rpc_client.get_transaction(tx_hash)
        if tx_info is not None:
            return tx_info
        time.sleep(0.5)
    raise TimeoutError(f"Transaction {tx_hash} not confirmed")
```

---

## 📚 SDK 和库

### Rust

使用 `jsonrpsee` 客户端：

```rust
use jsonrpsee::http_client::{HttpClientBuilder, HttpClient};
use jsonrpsee::core::client::ClientT;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = HttpClientBuilder::default()
        .build("http://127.0.0.1:9000")?;

    // 查询余额
    let balance: u64 = client
        .request("getBalance", vec![json!(alice)])
        .await?;

    println!("Balance: {}", balance);
    Ok(())
}
```

### Python

使用 `requests` 库：

```python
import requests
import json

class TokenChainClient:
    def __init__(self, url="http://127.0.0.1:9000"):
        self.url = url
        self.id_counter = 0

    def _request(self, method, params):
        self.id_counter += 1
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": self.id_counter
        }
        response = requests.post(self.url, json=payload)
        result = response.json()

        if "error" in result:
            raise Exception(result["error"]["message"])

        return result["result"]

    def get_balance(self, address):
        return self._request("getBalance", [address])

    def submit_transaction(self, tx):
        return self._request("submitTransaction", [tx])

# 使用
client = TokenChainClient()
balance = client.get_balance(alice_address)
print(f"Balance: {balance}")
```

### JavaScript/TypeScript

使用 `fetch` API：

```typescript
class TokenChainClient {
    constructor(private url: string = "http://127.0.0.1:9000") {}

    async request(method: string, params: any[]): Promise<any> {
        const response = await fetch(this.url, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                jsonrpc: '2.0',
                method,
                params,
                id: 1
            })
        });

        const result = await response.json();

        if (result.error) {
            throw new Error(result.error.message);
        }

        return result.result;
    }

    async getBalance(address: number[]): Promise<number> {
        return this.request('getBalance', [address]);
    }

    async submitTransaction(tx: any): Promise<string> {
        return this.request('submitTransaction', [tx]);
    }
}

// 使用
const client = new TokenChainClient();
const balance = await client.getBalance(aliceAddress);
console.log(`Balance: ${balance}`);
```

---

## 🔗 相关资源

- **快速开始**: [getting-started.md](getting-started.md)
- **架构文档**: [architecture.md](architecture.md)
- **源代码**: [simple-token-chain](../experiments/simple-token-chain/)
- **JSON-RPC 2.0 规范**: https://www.jsonrpc.org/specification

---

**文档版本**: 1.0
**最后更新**: 2025-12-11
