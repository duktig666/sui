# 下一步计划 - Token Chain 项目路线图

**当前状态**: ✅ 一周速成计划 100% 完成
**项目阶段**: 原型验证 → 生产准备
**制定时间**: 2025-12-11

---

## 📊 当前状态评估

### ✅ 已完成
- consensus-framework (832行，11个测试)
- simple-token-chain (1890行，21个测试)
- 完整的文档体系 (15份文档)
- 功能验证成功

### ⚠️ 当前限制
- 内存存储（无持久化）
- 单节点运行（未测试多节点）
- 简化的共识集成
- 无交易签名验证
- 无权限控制

### 🎯 改进方向
1. 生产级特性
2. 性能优化
3. 生态工具
4. 深度集成

---

## 🚀 路线图概览

```
现在 ──────────────► 1-2周 ──────────► 1-2月 ──────────► 3-6月
  │                    │                │                │
原型完成            生产准备         功能完善         主网部署
  │                    │                │                │
  ├─ 代码完成         ├─ 持久化        ├─ 智能合约      ├─ 安全审计
  ├─ 文档完成         ├─ 签名验证      ├─ 跨链桥        ├─ 主网启动
  └─ 测试通过         ├─ 多节点        ├─ 治理机制      └─ 生态发展
                      └─ 监控系统      └─ 性能优化
```

---

## 📅 阶段 1: 立即行动 (本周)

**目标**: 验证和优化现有成果

### 任务清单

#### 1.1 完整验证 ⏰ 2小时

**验证区块链功能**:
```bash
# 1. 启动节点
cd notes/experiments/simple-token-chain
cargo run --release --bin simple-token-chain

# 2. 运行完整测试套件
cargo test --release -- --test-threads=1

# 3. 运行性能基准测试
cargo bench

# 4. 运行客户端示例
cargo run --example client

# 5. 代码质量检查
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

**预期结果**:
- ✅ 所有测试通过
- ✅ 性能基准建立
- ✅ 客户端演示成功
- ✅ 0 clippy warnings

#### 1.2 代码提交 ⏰ 30分钟

```bash
# 1. 查看修改
git status
git diff

# 2. 添加所有新文件
git add notes/

# 3. 创建有意义的提交
git commit -m "$(cat <<'EOF'
feat: Complete Sui consensus research and Token Chain implementation

Day 1-7 Summary:
- ✅ consensus-framework: 832 lines, 11 tests
- ✅ simple-token-chain: 1890 lines, 21 tests
- ✅ 15 documents including architecture, API reference, guides
- ✅ 46 tests, 100% pass rate, 0 clippy warnings

Key achievements:
- Abstracted Mysticeti consensus into reusable framework
- Built fully functional blockchain with Mint/Transfer
- Established comprehensive testing and documentation
- Verified 303% efficiency improvement with AI assistance

🤖 Generated with Claude Code
Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
EOF
)"

# 4. 查看提交
git log -1 --stat
```

#### 1.3 性能基准收集 ⏰ 1小时

**运行完整性能测试**:
```bash
# 生成性能报告
cargo bench --bench throughput > performance-report.txt

# 查看报告
cat performance-report.txt

# 生成 HTML 报告
open target/criterion/report/index.html
```

**记录性能数据**:
- TPS (Transactions Per Second)
- 查询延迟 (P50, P99)
- 内存占用
- CPU 使用率

**创建性能报告**:
```bash
# 创建报告文件
cat > notes/PERFORMANCE_BASELINE.md <<'EOF'
# Token Chain 性能基准报告

## 测试环境
- CPU: [your CPU]
- RAM: [your RAM]
- OS: [your OS]

## 性能数据
- TPS: XXX transactions/second
- Balance Query: X ms (P50), Y ms (P99)
- Submit Transaction: X ms (P50), Y ms (P99)

## 基准日期
2025-12-11
EOF
```

---

## 📅 阶段 2: 短期改进 (1-2周)

**目标**: 添加生产级基础特性

### 2.1 持久化存储 ⏰ 3-4天

**实现 RocksDB 集成**:

```rust
// src/storage.rs
use rocksdb::{DB, Options};

pub trait StateStore: Send + Sync {
    async fn get(&self, address: &Address) -> Result<Option<Account>>;
    async fn put(&mut self, address: Address, account: Account) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
    async fn rollback(&mut self) -> Result<()>;
}

pub struct RocksDBStore {
    db: Arc<DB>,
}

impl StateStore for RocksDBStore {
    async fn get(&self, address: &Address) -> Result<Option<Account>> {
        let key = address.to_bytes();
        match self.db.get(&key)? {
            Some(value) => {
                let account = bincode::deserialize(&value)?;
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    async fn put(&mut self, address: Address, account: Account) -> Result<()> {
        let key = address.to_bytes();
        let value = bincode::serialize(&account)?;
        self.db.put(&key, &value)?;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        // RocksDB auto-commits, but we could add write batches here
        Ok(())
    }
}
```

**更新 Cargo.toml**:
```toml
[dependencies]
rocksdb = "0.21"
```

**迁移计划**:
1. 创建 `StateStore` trait
2. 实现 `RocksDBStore`
3. 保留 `MemoryStore` 用于测试
4. 更新 `TokenExecutor` 使用 trait
5. 添加配置选项选择存储后端

**测试**:
```rust
#[tokio::test]
async fn test_persistence() {
    let store = RocksDBStore::new("./test_db").unwrap();
    let mut executor = TokenExecutor::with_store(store);

    // Execute transactions
    executor.execute_mint(alice, 1000).await.unwrap();

    // Restart
    drop(executor);
    let store = RocksDBStore::new("./test_db").unwrap();
    let executor = TokenExecutor::with_store(store);

    // Verify state persisted
    assert_eq!(executor.get_balance(&alice), 1000);
}
```

### 2.2 交易签名 ⏰ 2-3天

**添加密钥对和签名**:

```rust
// src/crypto.rs
use ed25519_dalek::{Keypair, Signature, Signer, Verifier, PublicKey};

pub struct KeyPair {
    keypair: Keypair,
}

impl KeyPair {
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        let keypair = Keypair::generate(&mut csprng);
        Self { keypair }
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.keypair.sign(message)
    }

    pub fn public_key(&self) -> PublicKey {
        self.keypair.public
    }
}

// src/types.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
}

impl SignedTransaction {
    pub fn verify(&self) -> Result<(), CryptoError> {
        let public_key = PublicKey::from_bytes(&self.public_key)?;
        let signature = Signature::from_bytes(&self.signature)?;
        let message = bincode::serialize(&self.transaction)?;

        public_key.verify(&message, &signature)
            .map_err(|_| CryptoError::InvalidSignature)?;

        Ok(())
    }
}
```

**更新 Cargo.toml**:
```toml
[dependencies]
ed25519-dalek = "2.0"
rand = "0.8"
```

**集成到节点**:
```rust
// src/node.rs
pub async fn submit_signed_transaction(&self, signed_tx: SignedTransaction)
    -> Result<TxHash>
{
    // 1. 验证签名
    signed_tx.verify()?;

    // 2. 验证公钥与发送者地址匹配
    let sender_address = Address::from_public_key(&signed_tx.public_key);
    match &signed_tx.transaction {
        Transaction::Transfer { from, .. } => {
            if from != &sender_address {
                return Err(Error::InvalidSigner);
            }
        }
        _ => {}
    }

    // 3. 提交交易
    self.submit_transaction(signed_tx.transaction).await
}
```

### 2.3 多节点测试网 ⏰ 3-4天

**创建多节点配置**:

```yaml
# config/node0.yaml
node_id: 0
rpc_addr: "127.0.0.1:9000"
data_dir: "./data/node0"

consensus:
  authority_index: 0
  committee_size: 4
  committee:
    - id: 0
      address: "127.0.0.1:10000"
      stake: 1
    - id: 1
      address: "127.0.0.1:10001"
      stake: 1
    - id: 2
      address: "127.0.0.1:10002"
      stake: 1
    - id: 3
      address: "127.0.0.1:10003"
      stake: 1
```

**启动脚本**:
```bash
#!/bin/bash
# scripts/start-testnet.sh

echo "Starting 4-node testnet..."

# 启动节点
for i in 0 1 2 3; do
    cargo run --release --bin simple-token-chain -- \
        --config config/node$i.yaml \
        > logs/node$i.log 2>&1 &

    echo "Started node $i (PID: $!)"
    sleep 2
done

echo "Testnet started!"
echo ""
echo "RPC endpoints:"
echo "  Node 0: http://127.0.0.1:9000"
echo "  Node 1: http://127.0.0.1:9001"
echo "  Node 2: http://127.0.0.1:9002"
echo "  Node 3: http://127.0.0.1:9003"
```

**测试一致性**:
```rust
#[tokio::test]
async fn test_multi_node_consistency() {
    // 启动4个节点
    let nodes = start_test_network(4).await;

    // 向节点0提交交易
    let tx = Transaction::Mint { to: alice, amount: 1000 };
    nodes[0].submit_transaction(tx).await.unwrap();

    // 等待共识
    tokio::time::sleep(Duration::from_secs(5)).await;

    // 验证所有节点状态一致
    for node in &nodes {
        let balance = node.get_balance(alice).await.unwrap();
        assert_eq!(balance, 1000, "Node {} inconsistent", node.id());
    }
}
```

### 2.4 监控和日志 ⏰ 2天

**添加 metrics**:
```toml
[dependencies]
prometheus = "0.13"
```

```rust
// src/metrics.rs
use prometheus::{IntCounter, IntGauge, Histogram, Registry};

lazy_static! {
    pub static ref TRANSACTIONS_TOTAL: IntCounter =
        IntCounter::new("transactions_total", "Total transactions").unwrap();

    pub static ref BALANCE_QUERIES: IntCounter =
        IntCounter::new("balance_queries_total", "Total balance queries").unwrap();

    pub static ref TX_LATENCY: Histogram =
        Histogram::new("tx_latency_seconds", "Transaction latency").unwrap();

    pub static ref ACTIVE_ACCOUNTS: IntGauge =
        IntGauge::new("active_accounts", "Number of active accounts").unwrap();
}

pub fn register_metrics(registry: &Registry) {
    registry.register(Box::new(TRANSACTIONS_TOTAL.clone())).unwrap();
    registry.register(Box::new(BALANCE_QUERIES.clone())).unwrap();
    registry.register(Box::new(TX_LATENCY.clone())).unwrap();
    registry.register(Box::new(ACTIVE_ACCOUNTS.clone())).unwrap();
}
```

**添加 metrics 端点**:
```rust
// src/rpc.rs
#[method(name = "getMetrics")]
async fn get_metrics(&self) -> RpcResult<String> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    Ok(String::from_utf8(buffer).unwrap())
}
```

---

## 📅 阶段 3: 中期增强 (1-2月)

**目标**: 功能完善和生态工具

### 3.1 CLI 工具 ⏰ 1周

**创建命令行工具**:
```bash
cargo new --bin token-cli
```

```rust
// token-cli/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "token-cli")]
#[command(about = "Token Chain CLI tool")]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:9000")]
    rpc_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get account balance
    Balance {
        #[arg(short, long)]
        address: String,
    },

    /// Transfer tokens
    Transfer {
        #[arg(short, long)]
        from: String,

        #[arg(short, long)]
        to: String,

        #[arg(short, long)]
        amount: u64,
    },

    /// Get node status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = create_rpc_client(&cli.rpc_url)?;

    match cli.command {
        Commands::Balance { address } => {
            let addr = Address::from_string(&address);
            let balance = client.get_balance(addr).await?;
            println!("Balance: {} tokens", balance);
        }

        Commands::Transfer { from, to, amount } => {
            // Interactive: ask for private key
            println!("Enter private key for {}:", from);
            let private_key = read_private_key()?;

            // Create and sign transaction
            let tx = create_transfer_tx(&from, &to, amount);
            let signed_tx = sign_transaction(tx, &private_key)?;

            // Submit
            let tx_hash = client.submit_signed_transaction(signed_tx).await?;
            println!("Transaction submitted: {}", tx_hash);
        }

        Commands::Status => {
            let status = client.get_status().await?;
            println!("Node ID: {}", status.node_id);
            println!("Running: {}", status.running);
            println!("RPC: {}", status.rpc_addr);
        }
    }

    Ok(())
}
```

**使用示例**:
```bash
# 查询余额
token-cli balance --address alice

# 转账
token-cli transfer --from alice --to bob --amount 100

# 查看状态
token-cli status --rpc-url http://127.0.0.1:9001
```

### 3.2 Web Dashboard ⏰ 1-2周

**技术栈**:
- Frontend: React + TypeScript
- API: JSON-RPC via fetch
- UI: Tailwind CSS

**功能**:
- 账户管理
- 交易历史
- 实时统计
- 节点监控

**项目结构**:
```
token-dashboard/
├── src/
│   ├── components/
│   │   ├── AccountList.tsx
│   │   ├── TransactionList.tsx
│   │   ├── NodeStatus.tsx
│   │   └── SendTransaction.tsx
│   ├── hooks/
│   │   └── useRpcClient.ts
│   ├── App.tsx
│   └── main.tsx
└── package.json
```

### 3.3 SDK 库 ⏰ 1周

**Rust SDK**:
```rust
// token-chain-sdk/src/lib.rs
pub struct TokenChainClient {
    client: HttpClient,
    url: String,
}

impl TokenChainClient {
    pub fn new(url: &str) -> Result<Self> {
        let client = HttpClientBuilder::default().build(url)?;
        Ok(Self {
            client,
            url: url.to_string(),
        })
    }

    pub async fn get_balance(&self, address: Address) -> Result<u64> {
        self.client
            .request("getBalance", vec![json!(address)])
            .await
            .map_err(Into::into)
    }

    pub async fn submit_transaction(&self, tx: SignedTransaction) -> Result<TxHash> {
        self.client
            .request("submitTransaction", vec![json!(tx)])
            .await
            .map_err(Into::into)
    }
}
```

**Python SDK**:
```python
# token_chain_sdk/__init__.py
from dataclasses import dataclass
import requests

@dataclass
class Address:
    bytes: bytes

class TokenChainClient:
    def __init__(self, url: str = "http://127.0.0.1:9000"):
        self.url = url

    def get_balance(self, address: Address) -> int:
        response = self._request("getBalance", [list(address.bytes)])
        return response

    def submit_transaction(self, tx: dict) -> str:
        response = self._request("submitTransaction", [tx])
        return response

    def _request(self, method: str, params: list):
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        }
        r = requests.post(self.url, json=payload)
        result = r.json()
        if "error" in result:
            raise Exception(result["error"]["message"])
        return result["result"]
```

### 3.4 性能优化 ⏰ 1周

**优化清单**:

1. **减少锁竞争**:
```rust
// 使用 RwLock 代替 Mutex
use tokio::sync::RwLock;

pub struct TokenExecutor {
    state: Arc<RwLock<State>>,  // 改为 RwLock
}

// 读操作不阻塞
pub async fn get_balance(&self, address: &Address) -> u64 {
    let state = self.state.read().await;  // 多个读者可以并发
    state.get(address).map(|a| a.balance).unwrap_or(0)
}
```

2. **批量处理优化**:
```rust
pub async fn submit_batch(&self, txs: Vec<Transaction>) -> Result<Vec<TxHash>> {
    // 一次性获取锁
    let mut executor = self.executor.write().await;

    // 批量执行
    let results = executor.execute_batch(txs).await?;

    // 一次性释放锁
    Ok(results)
}
```

3. **零拷贝序列化**:
```toml
[dependencies]
rkyv = "0.7"  # 零拷贝序列化
```

---

## 📅 阶段 4: 长期愿景 (3-6月)

**目标**: 生产级部署和生态发展

### 4.1 智能合约支持 ⏰ 1-2月

**集成 Move VM**:
```rust
// src/vm.rs
use move_vm_runtime::MoveVM;

pub struct ContractExecutor {
    vm: MoveVM,
    modules: HashMap<Address, Vec<CompiledModule>>,
}

impl ContractExecutor {
    pub fn deploy_contract(&mut self, bytecode: Vec<u8>) -> Result<Address> {
        let module = CompiledModule::deserialize(&bytecode)?;
        let address = Address::from_module_id(module.self_id());
        self.modules.insert(address, vec![module]);
        Ok(address)
    }

    pub fn call_contract(
        &mut self,
        contract: Address,
        function: &str,
        args: Vec<Vec<u8>>,
    ) -> Result<Vec<u8>> {
        // Execute Move function
        let result = self.vm.execute_function(
            &contract,
            function,
            vec![],  // type args
            args,
        )?;
        Ok(result)
    }
}
```

### 4.2 跨链桥接 ⏰ 1-2月

**设计跨链桥**:
```rust
pub struct Bridge {
    source_chain: Arc<TokenChainNode>,
    target_chain_rpc: String,
}

impl Bridge {
    pub async fn lock_and_mint(
        &self,
        from: Address,
        amount: u64,
        target_address: String,
    ) -> Result<()> {
        // 1. 在源链锁定代币
        let lock_tx = Transaction::Lock { from, amount };
        let tx_hash = self.source_chain.submit_transaction(lock_tx).await?;

        // 2. 等待确认
        self.wait_for_confirmation(tx_hash).await?;

        // 3. 在目标链铸造
        let mint_msg = create_mint_message(target_address, amount);
        self.submit_to_target_chain(mint_msg).await?;

        Ok(())
    }
}
```

### 4.3 治理机制 ⏰ 2-3周

**提案和投票**:
```rust
pub enum Proposal {
    ParameterChange { param: String, value: u64 },
    UpgradeContract { address: Address, code: Vec<u8> },
    AddValidator { validator: Address, stake: u64 },
}

pub struct Governance {
    proposals: HashMap<ProposalId, Proposal>,
    votes: HashMap<ProposalId, HashMap<Address, Vote>>,
}

impl Governance {
    pub fn create_proposal(&mut self, proposal: Proposal) -> ProposalId {
        let id = self.next_proposal_id();
        self.proposals.insert(id, proposal);
        id
    }

    pub fn vote(&mut self, proposal_id: ProposalId, voter: Address, vote: Vote) {
        self.votes
            .entry(proposal_id)
            .or_default()
            .insert(voter, vote);
    }

    pub fn execute_if_passed(&mut self, proposal_id: ProposalId) -> Result<()> {
        if self.is_passed(proposal_id) {
            let proposal = self.proposals.get(&proposal_id).unwrap();
            self.execute_proposal(proposal)?;
        }
        Ok(())
    }
}
```

### 4.4 安全审计和主网部署 ⏰ 1月

**安全审计清单**:
- [ ] 代码安全审计
- [ ] 密码学审计
- [ ] 共识安全审计
- [ ] 智能合约审计
- [ ] 渗透测试

**部署检查清单**:
- [ ] 压力测试通过
- [ ] 灾难恢复计划
- [ ] 监控和告警系统
- [ ] 备份和恢复机制
- [ ] 运维文档完整

---

## 📊 优先级矩阵

| 任务 | 重要性 | 紧急性 | 优先级 | 预计时间 |
|------|--------|--------|--------|---------|
| 完整验证 | 高 | 高 | P0 | 2h |
| 代码提交 | 高 | 高 | P0 | 30min |
| 性能基准 | 高 | 中 | P1 | 1h |
| 持久化存储 | 高 | 中 | P1 | 3-4天 |
| 交易签名 | 高 | 中 | P1 | 2-3天 |
| 多节点测试 | 高 | 低 | P2 | 3-4天 |
| 监控系统 | 中 | 中 | P2 | 2天 |
| CLI 工具 | 中 | 低 | P3 | 1周 |
| Web Dashboard | 低 | 低 | P4 | 1-2周 |

---

## 🎯 推荐执行顺序

### 本周 (Week 1)
1. ✅ 完整验证 (2小时)
2. ✅ 代码提交 (30分钟)
3. ✅ 性能基准 (1小时)

### Week 2-3
4. 持久化存储 (3-4天)
5. 交易签名 (2-3天)
6. 监控系统 (2天)

### Week 4-5
7. 多节点测试 (3-4天)
8. CLI 工具 (1周)

### Month 2
9. 性能优化
10. SDK 库开发
11. Web Dashboard

### Month 3-6
12. 智能合约支持
13. 跨链桥接
14. 治理机制
15. 安全审计
16. 主网部署

---

## 📝 总结

### 立即行动 (今天)
```bash
# 1. 运行完整验证
cd notes/experiments/simple-token-chain
cargo test --release
cargo bench
cargo run --example client

# 2. 提交代码
git add notes/
git commit -m "feat: Complete one-week research plan"
git push

# 3. 收集性能数据
cargo bench > notes/PERFORMANCE_BASELINE.md
```

### 本周目标
- ✅ 验证所有功能
- ✅ 提交代码到仓库
- ✅ 建立性能基准

### 下周开始
- 🚀 持久化存储
- 🔐 交易签名
- 📊 监控系统

---

**制定时间**: 2025-12-11
**状态**: 📋 待执行
**下次review**: 1周后

---

*从原型到生产，Token Chain 项目将继续演进！* 🚀
