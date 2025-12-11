# Sui 交易执行时序图详解

**日期**: 2025年12月11日  
**版本**: v1.0  

---

## 目录

1. [简单交易时序图](#1-简单交易时序图)
2. [共享对象交易时序图](#2-共享对象交易时序图)
3. [对比分析](#3-对比分析)
4. [关键差异](#4-关键差异)

---

## 1. 简单交易时序图

### 场景：Alice 转账 100 SUI 给 Bob

**网络拓扑**:
- 验证者: Validator A, B, C, D (各 25% 权重)
- 全节点: FullNode1 (用户连接), FullNode2 (备用)
- 客户端: Alice 的钱包

**交易内容**:
```rust
Transaction {
    sender: Alice (0x123...),
    recipient: Bob (0x456...),
    coin: ObjectRef(0xAAA, version: 42, digest: hash_v42),
    amount: 100 SUI,
    gas_budget: 1000,
}
```

### 完整时序图

```
时间轴      Alice钱包    FullNode1    ValidatorA    ValidatorB    ValidatorC    ValidatorD    FullNode2
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

T=0ms      [构造交易]
           TransactionData {
             sender: Alice
             input: Coin(0xAAA, v42)
             recipient: Bob
             amount: 100 SUI
           }
               ↓
           [签名]
           sign_secure(tx_data)
               ↓
               
T=5ms      [发送] ──────→ [收到交易]
                           QuorumDriver
                               ↓
                           [分类交易]
                           classify: Owned ✓
                               ↓
                           [并发广播]
                           broadcast_to_validators
                               ├────────────→ [收到]         [收到]        [收到]        [收到]
                               │              RPC            RPC           RPC           RPC
                               │              T=10ms         T=10ms        T=10ms        T=10ms
                               │
T=10ms                         │              ↓              ↓             ↓             ↓
                               │           [验证签名]      [验证签名]    [验证签名]    [验证签名]
                               │           verify_sig()    verify_sig()  verify_sig()  verify_sig()
                               │              ✓              ✓             ✓             ✓
                               │
T=12ms                         │           [检查版本]      [检查版本]    [检查版本]    [检查版本]
                               │           obj.version     obj.version   obj.version   obj.version
                               │           == 42 ✓         == 42 ✓       == 42 ✓       == 42 ✓
                               │
T=15ms                         │           [获取锁]        [获取锁]      [获取锁]      [获取锁]
                               │           lock(0xAAA,v42) lock(...)     lock(...)     lock(...)
                               │           本地数据库 ✓    本地DB ✓      本地DB ✓      本地DB ✓
                               │              ↓              ↓             ↓             ↓
T=25ms                         │           [执行VM]        [执行VM]      [执行VM]      [执行VM]
                               │           Move::execute   Move::exec    Move::exec    Move::exec
                               │           generate effects              (并行执行)
                               │              ↓              ↓             ↓             ↓
T=35ms                         │           [写数据库]      [写数据库]    [写数据库]    [写数据库]
                               │           v42 -> v43      v42 -> v43    v42 -> v43    v42 -> v43
                               │           commit_effects  commit        commit        commit
                               │              ✓              ✓             ✓             ✓
                               │
T=40ms                         │           [签名]          [签名]        [签名]        [签名]
                               │           sign(effects)   sign(effects) sign(effects) sign(effects)
                               │           sig_A           sig_B         sig_C         sig_D
                               │              ↓              ↓             ↓             ↓
                               │              └──────────────┴─────────────┴─────────────┘
                               │                             ↓
T=50ms                     [收集签名中]
                           received: []
                           stake: 0%
                               ↓
                           [收到 sig_A]
                           verify(sig_A) ✓
                           received: [A]
                           stake: 25%
                               ↓
T=55ms                     [收到 sig_B]
                           verify(sig_B) ✓
                           received: [A, B]
                           stake: 50% ✅
                               ↓
                           [达到法定人数]
                           quorum_reached!
                           threshold: 50%
                           actual: 50%
                               ↓
                           [构造证书]
                           CertifiedTx {
                             transaction,
                             signatures: [
                               (A, sig_A),
                               (B, sig_B)
                             ],
                             total_stake: 50%
                           }
                               ↓
T=60ms                     [验证证书]
                           verify_quorum() ✓
                               ↓
                           [写入本地]
                           commit_to_local_db
                           effects ✓
                           objects ✓
                               ↓
T=65ms                     [返回证书] ←─────────────────── 🎉 交易最终确认！
                               ↓
           [收到证书] ←────────┘
           显示: "✅ 转账成功"
           latency: 65ms
           

T=70ms                     [收到 sig_C]
                           (已有证书，忽略)
                           
T=75ms                     [收到 sig_D]
                           (已有证书，忽略)


// ================== 后台异步流程 ==================

T=2000ms                   [Checkpoint 构建器]
                           collect_executed_txs(
                             from: T=0ms,
                             to: T=2000ms
                           )
                               ↓
                           build_checkpoint {
                             epoch: 100,
                             sequence: 1000,
                             txs: [本交易, ...],
                             state_root: 0xABC...,
                             timestamp: 2000
                           }
                               ↓
                           [广播 Checkpoint]
                               ├────────────→ [收到CKP]    [收到CKP]     [收到CKP]     [收到CKP]
                               │              verify ✓     verify ✓      verify ✓      verify ✓
                               │
T=2100ms                       │              [签名CKP]    [签名CKP]     [签名CKP]     [签名CKP]
                               │              ckp_sig_A    ckp_sig_B     ckp_sig_C     ckp_sig_D
                               │                  ↓            ↓             ↓             ↓
                               │                  └────────────┴─────────────┴─────────────┘
                               │                               ↓
                           [收集 CKP 签名]
                           quorum: 50% ✓
                               ↓
T=2200ms                   [认证 Checkpoint]
                           CertifiedCheckpoint {
                             checkpoint,
                             signatures: [A,B,C,D],
                             stake: 100%
                           }
                               ↓
                           [广播到全节点] ───────────────────────────────────────────────→ [收到CKP]
                                                                                            T=2200ms
                                                                                               ↓
T=2250ms                                                                                   [下载交易]
                                                                                           download_tx
                                                                                           from_peers
                                                                                               ↓
T=2300ms                                                                                   [执行交易]
                                                                                           execute_tx
                                                                                           commit_state
                                                                                               ↓
T=2350ms                                                                                   [验证状态根]
                                                                                           local_root
                                                                                           == ckp_root ✓
                                                                                               ↓
                                                                                           [同步完成] ✅
```

### 关键时间点

| 时间 | 事件 | 节点 | 说明 |
|-----|------|------|------|
| T=0ms | 构造交易 | Alice 钱包 | 用户发起转账 |
| T=5ms | 广播到验证者 | FullNode1 | 开始收集签名 |
| T=10ms | 验证者收到交易 | 所有验证者 | 并行开始执行 |
| T=15ms | 获取对象锁 | 所有验证者 | 本地锁定，无协调 |
| T=35ms | 状态更新 | 所有验证者 | v42 -> v43 |
| T=40ms | 生成签名 | 所有验证者 | 独立签名 |
| T=55ms | 达到法定人数 | FullNode1 | 收集到 50% 权重 |
| T=65ms | **交易最终确认** | 客户端 | **总延迟 65ms** |
| T=2200ms | Checkpoint 认证 | FullNode1 | 额外的最终性 |
| T=2300ms | 状态同步 | FullNode2 | 延迟同步 |

---

## 2. 共享对象交易时序图

### 场景：Alice 在 DEX 中交易 100 SUI -> USDC

**网络拓扑**: 同上

**交易内容**:
```rust
Transaction {
    sender: Alice (0x123...),
    shared_objects: [
        Pool(0xPOOL, version: 1000) // 共享对象！
    ],
    function: "swap_exact_input",
    args: [100 SUI, min_output: 95 USDC],
    gas_budget: 1000,
}
```

### 完整时序图

```
时间轴      Alice钱包    FullNode1    ConsensusAdapter    Mysticeti       ValidatorA    ValidatorB    ValidatorC    ValidatorD
═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════

T=0ms      [构造交易]
           TransactionData {
             sender: Alice
             shared: Pool(0xPOOL)
             function: swap
             args: [100, 95]
           }
               ↓
           [签名]
               ↓
               
T=5ms      [发送] ──────→ [收到交易]
                           QuorumDriver
                               ↓
                           [分类交易]
                           classify: Shared! ⚠️
                               ↓
                           [转发到共识层]
                           forward_to_consensus
                               ├────────────→ [收到]         [DAG]
                               │              submit_tx      consensus
                               │              to_mysticeti   engine
                               │                  ↓             ↓
T=10ms                         │              [构造共识交易] [接收]
                               │              ConsensusTransaction {
                               │                tx: Alice's tx,
                               │                tracking_id: 0x111
                               │              }
                               │                  ↓             ↓
T=20ms                         │              [提交到DAG] ──→ [添加到DAG]
                               │              submit()        Block {
                               │                                round: 1000,
                               │                                author: A,
                               │                                txs: [0x111],
                               │                                parents: [...]
                               │                              }
                               │                                  ↓
T=50ms                         │                             [DAG 共识中]
                               │                             Mysticeti Algorithm:
                               │                             ┌─────────────────┐
                               │                             │ Round 1000      │
                               │                             │ Block_A [tx]    │
                               │                             │   ↓             │
                               │                             │ Round 1001      │
                               │                             │ Block_B, C, D   │
                               │                             │   ↓             │
                               │                             │ 达成共识顺序     │
                               │                             └─────────────────┘
                               │                                  ↓
T=500ms                        │                             [共识完成]
                               │                             consensus_output {
                               │                               sequence: 1000,
                               │                               txs: [Alice's tx],
                               │                               order: deterministic
                               │                             }
                               │                                  ↓
                               │                             [分发到验证者]
                               │              ┌───────────────────┴──────────────────┬──────────────┬─────────────┐
                               │              ↓                   ↓                  ↓              ↓             ↓
T=510ms                        │          [共识输出]          [共识输出]         [共识输出]     [共识输出]    [共识输出]
                               │          consensus_seq:     consensus_seq:      consensus_seq:  consensus_seq: consensus_seq:
                               │          1000               1000                1000           1000           1000
                               │              ↓                   ↓                  ↓              ↓             ↓
T=515ms                        │          [获取共享锁]        [获取共享锁]        [获取共享锁]  [获取共享锁]  [获取共享锁]
                               │          lock_shared(        lock_shared(        按共识序号     按共识序号     按共识序号
                               │            Pool, seq:1000      Pool, seq:1000    锁定 ✓         锁定 ✓         锁定 ✓
                               │          )                   )
                               │              ↓                   ↓                  ↓              ↓             ↓
T=525ms                        │          [验证版本]          [验证版本]          [验证版本]    [验证版本]    [验证版本]
                               │          Pool.version       Pool.version        Pool.version  Pool.version  Pool.version
                               │          == 1000 ✓          == 1000 ✓           == 1000 ✓     == 1000 ✓     == 1000 ✓
                               │              ↓                   ↓                  ↓              ↓             ↓
T=550ms                        │          [执行VM]            [执行VM]            [执行VM]      [执行VM]      [执行VM]
                               │          Move::execute      Move::execute       Move::exec    Move::exec    Move::exec
                               │          swap(100 SUI)      swap(100 SUI)       swap(...)     swap(...)     swap(...)
                               │              ↓                   ↓                  ↓              ↓             ↓
                               │          Alice: +95 USDC   Alice: +95 USDC     确定性执行     确定性执行     确定性执行
                               │          Pool: 更新储备     Pool: 更新储备       相同结果      相同结果      相同结果
                               │              ↓                   ↓                  ↓              ↓             ↓
T=600ms                        │          [写数据库]          [写数据库]          [写数据库]    [写数据库]    [写数据库]
                               │          Pool v1000->v1001 Pool v1000->v1001  commit ✓      commit ✓      commit ✓
                               │          commit_effects    commit_effects
                               │              ✓                   ✓                  ✓              ✓             ✓
                               │              ↓                   ↓                  ↓              ↓             ↓
T=610ms                        │          [签名]              [签名]              [签名]        [签名]        [签名]
                               │          sign(effects)      sign(effects)       sign(effects) sign(effects) sign(effects)
                               │          sig_A              sig_B               sig_C         sig_D         sig_E
                               │              ↓                   ↓                  ↓              ↓             ↓
                               │              └───────────────────┴──────────────────┴──────────────┴─────────────┘
                               │                                          ↓
T=620ms                    [收集签名中]
                           received: []
                           stake: 0%
                               ↓
                           [收到 sig_A]
                           verify(sig_A) ✓
                           received: [A]
                           stake: 25%
                               ↓
T=625ms                    [收到 sig_B]
                           verify(sig_B) ✓
                           received: [A, B]
                           stake: 50% ✅
                               ↓
                           [达到法定人数]
                           quorum_reached!
                               ↓
                           [构造证书]
                           CertifiedTx {
                             transaction,
                             signatures: [A, B],
                             total_stake: 50%,
                             consensus_seq: 1000 ✓
                           }
                               ↓
T=630ms                    [验证证书]
                           verify_quorum() ✓
                           verify_consensus() ✓
                               ↓
                           [写入本地]
                           commit_effects ✓
                               ↓
T=635ms                    [返回证书] ←─────────────────────── 🎉 交易最终确认！
                               ↓
           [收到证书] ←────────┘
           显示: "✅ 交易成功"
           latency: 635ms


T=640ms                    [收到 sig_C]
                           (已有证书，忽略)
                           
T=645ms                    [收到 sig_D]
                           (已有证书，忽略)


// ================== Checkpoint 流程（同简单交易）==================

T=2000ms                   [构建 Checkpoint]
                           ...（与简单交易相同）
```

### 关键时间点

| 时间 | 事件 | 组件 | 说明 |
|-----|------|------|------|
| T=0ms | 构造交易 | Alice 钱包 | 包含共享对象 |
| T=5ms | 转发到共识 | FullNode1 | 检测到共享对象 |
| T=10ms | 提交到 Mysticeti | ConsensusAdapter | 进入 DAG 共识 |
| T=20ms | 添加到 DAG | Mysticeti | 创建共识区块 |
| T=500ms | **共识完成** | Mysticeti | **关键步骤！** |
| T=510ms | 分发共识输出 | Mysticeti | 发送到所有验证者 |
| T=515ms | 获取共享锁 | 所有验证者 | 使用共识序号 |
| T=550ms | 执行交易 | 所有验证者 | 按共识顺序 |
| T=600ms | 状态更新 | 所有验证者 | v1000 -> v1001 |
| T=625ms | 达到法定人数 | FullNode1 | 50% 权重 |
| T=635ms | **交易最终确认** | 客户端 | **总延迟 635ms** |

### Mysticeti DAG 共识详解

```
Mysticeti DAG 内部流程 (T=20ms - T=500ms):

Round 1000:
    ValidatorA 提议 Block_A1000
    ├─ transactions: [Alice's swap tx]
    ├─ parents: [B999, C999, D999]
    └─ signature: sig_A

Round 1001:
    ValidatorB 提议 Block_B1001
    ├─ parents: [A1000, C999, D999]  // 引用 A1000
    └─ signature: sig_B
    
    ValidatorC 提议 Block_C1001
    ├─ parents: [A1000, B999, D999]  // 引用 A1000
    └─ signature: sig_C
    
    ValidatorD 提议 Block_D1001
    ├─ parents: [A1000, B999, C999]  // 引用 A1000
    └─ signature: sig_D

Round 1002:
    所有验证者都已引用 A1000
    └─> A1000 达到"提交"状态
        └─> 交易顺序确定！
            sequence: 1000
            tx: Alice's swap

共识输出:
    ConsensusOutput {
        committed_blocks: [A1000],
        transactions: [Alice's swap],
        order: deterministic,
        sequence: 1000
    }
    
分发到所有验证者按顺序执行 ✓
```

---

## 3. 对比分析

### 3.1 流程对比表

| 阶段 | 简单交易 | 共享对象交易 | 差异 |
|-----|---------|-------------|------|
| **1. 交易提交** | T=0-5ms | T=0-5ms | 相同 |
| **2. 交易分类** | T=5ms (Owned) | T=5ms (Shared) | 分类不同 |
| **3. 共识排序** | ❌ 跳过 | ✅ T=10-500ms | **核心差异** |
| **4. 获取对象锁** | T=15ms (版本锁) | T=515ms (共识序号锁) | 锁定机制不同 |
| **5. 执行交易** | T=25ms | T=550ms | 相同逻辑 |
| **6. 提交状态** | T=35ms | T=600ms | 相同逻辑 |
| **7. 签名返回** | T=40ms | T=610ms | 相同逻辑 |
| **8. 收集签名** | T=50-55ms | T=620-625ms | 相同流程 |
| **9. 构造证书** | T=55ms | T=625ms | 相同流程 |
| **10. 最终确认** | T=65ms | T=635ms | 延迟差异大 |

### 3.2 延迟分析

```rust
/// 延迟分解
pub struct LatencyBreakdown {
    simple_transaction: {
        client_to_fullnode: "5ms",
        fullnode_broadcast: "5ms",
        validators_execute: "30ms (并行)",
        collect_signatures: "15ms",
        total: "65ms ✅",
    },
    
    shared_transaction: {
        client_to_fullnode: "5ms",
        fullnode_to_consensus: "5ms",
        consensus_ordering: "490ms ⚠️", // 主要开销
        validators_execute: "90ms",
        collect_signatures: "15ms",
        total: "635ms ⚠️",
    },
    
    difference: {
        absolute: "570ms",
        reason: "Mysticeti DAG 共识排序",
        percentage: "共识占总延迟的 77%",
    },
}
```

### 3.3 吞吐量对比

```rust
/// 吞吐量分析
pub struct ThroughputAnalysis {
    simple_transaction: {
        single_validator: "~2,500 TPS (40ms/tx)",
        all_validators_parallel: "~250,000 TPS",
        real_world: "~100,000 TPS",
        bottleneck: "网络带宽和签名验证",
    },
    
    shared_transaction: {
        single_shared_object: "~2,000 TPS (500ms共识)",
        multiple_shared_objects: "~10,000 TPS (并行)",
        real_world: "~5,000 TPS",
        bottleneck: "Mysticeti 共识吞吐量",
    },
    
    ratio: {
        simple_vs_shared: "20:1",
        conclusion: "简单交易性能远超共享对象交易",
    },
}
```

---

## 4. 关键差异

### 4.1 执行路径差异

```rust
/// 简单交易路径（无共识）
简单交易:
    客户端 
      ↓ (5ms)
    FullNode1 
      ↓ (并发广播)
    验证者 A, B, C, D
      ↓ (独立执行, 30ms)
    FullNode1 收集签名
      ↓ (15ms)
    证书返回客户端
    
    总计: ~65ms
    并行度: 完全并行
    瓶颈: 网络往返

/// 共享对象路径（需要共识）
共享对象交易:
    客户端
      ↓ (5ms)
    FullNode1
      ↓ (5ms)
    Mysticeti 共识
      ↓ (490ms) ⚠️ 瓶颈！
    验证者 A, B, C, D
      ↓ (按顺序执行, 90ms)
    FullNode1 收集签名
      ↓ (15ms)
    证书返回客户端
    
    总计: ~635ms
    并行度: 受共识限制
    瓶颈: Mysticeti 共识延迟
```

### 4.2 对象锁定差异

```rust
/// 简单交易: 对象版本锁
impl AuthorityPerEpochStore {
    fn acquire_owned_lock(
        &self,
        object_id: ObjectID,
        version: Version, // 交易声明的版本
        tx_digest: TransactionDigest,
    ) -> Result<()> {
        // 纯本地操作，无需协调
        let lock_key = ObjectKey(object_id, version);
        
        // 检查版本锁
        if self.locks.contains_key(&lock_key) {
            return Err(SuiError::ObjectVersionLocked);
        }
        
        // 锁定特定版本
        self.locks.insert(lock_key, tx_digest);
        Ok(())
        
        // 关键：每个验证者独立锁定
        // 依赖确定性执行保证一致性
    }
}

/// 共享对象: 共识序号锁
impl AuthorityPerEpochStore {
    fn acquire_shared_lock(
        &self,
        object_id: ObjectID,
        consensus_sequence: u64, // 共识确定的序号
        tx_digest: TransactionDigest,
    ) -> Result<()> {
        // 使用共识序号作为锁定依据
        let shared_version = self.get_version_at_sequence(
            object_id,
            consensus_sequence,
        )?;
        
        let lock_key = ObjectKey(object_id, shared_version);
        self.locks.insert(lock_key, tx_digest);
        
        Ok(())
        
        // 关键：共识已经确定了全局顺序
        // 所有验证者按相同顺序锁定
    }
}
```

### 4.3 状态一致性保证

```rust
/// 简单交易的一致性
pub struct SimpleTransactionConsistency {
    mechanism: "对象版本号 + 拜占庭法定人数",
    
    guarantee: {
        // 场景：Alice 双花 Coin v42
        // tx1 和 tx2 都使用 v42
        
        // 验证者 A 先执行 tx1:
        step1: "检查 Coin 版本 = v42 ✓",
        step2: "锁定 v42",
        step3: "执行 tx1",
        step4: "Coin 版本更新为 v43",
        step5: "签名 tx1",
        
        // 验证者 A 收到 tx2:
        step6: "检查 Coin 版本 = v43 (已更新)",
        step7: "tx2 声明使用 v42",
        step8: "v43 != v42",
        step9: "拒绝 tx2 ✗",
        
        conclusion: "版本号自动防止双花",
    },
    
    safety: "2/3+ 诚实验证者的一致状态",
}

/// 共享对象的一致性
pub struct SharedObjectConsistency {
    mechanism: "Mysticeti 共识排序 + 确定性执行",
    
    guarantee: {
        // 场景：Alice 和 Bob 同时在 Pool 交易
        // tx1: Alice swap 100 SUI
        // tx2: Bob swap 200 SUI
        
        // Mysticeti 确定顺序:
        step1: "共识输出: [tx1(seq:1000), tx2(seq:1001)]",
        
        // 所有验证者收到相同顺序:
        step2: "验证者 A 执行: tx1 -> tx2",
        step3: "验证者 B 执行: tx1 -> tx2",
        step4: "验证者 C 执行: tx1 -> tx2",
        step5: "验证者 D 执行: tx1 -> tx2",
        
        // 确定性保证相同结果:
        step6: "所有验证者的 Pool 状态一致",
        
        conclusion: "共识序号保证全局一致",
    },
    
    safety: "Mysticeti BFT 共识 + 确定性执行",
}
```

### 4.4 性能权衡

```rust
/// 简单交易的优势
pub struct SimpleTransactionAdvantages {
    latency: "~65ms (极低)",
    throughput: "~100k TPS (极高)",
    scalability: "水平扩展（增加验证者）",
    parallelism: "完全并行（无依赖）",
    
    use_cases: [
        "P2P 转账",
        "NFT 转移",
        "个人钱包操作",
        "单人游戏进度",
    ],
}

/// 共享对象的权衡
pub struct SharedObjectTradeoffs {
    latency: "~635ms (10倍慢)",
    throughput: "~5k TPS (20倍低)",
    necessity: "需要全局排序",
    parallelism: "受共识限制",
    
    use_cases: [
        "DEX 交易",
        "NFT 拍卖",
        "多人游戏",
        "DAO 投票",
    ],
    
    optimization: "Mysticeti 已是最快的 BFT 共识之一",
}
```

---

## 5. 实战示例代码

### 5.1 简单交易完整示例

```rust
/// 简单交易的完整执行流程
#[tokio::main]
async fn simple_transaction_example() -> Result<()> {
    let start = Instant::now();
    
    // T=0ms: 构造交易
    println!("📱 T=0ms: 构造交易");
    let tx = TransactionData::new_transfer(
        alice_address,
        ObjectRef(0xAAA, 42, hash_v42),
        bob_address,
        100_000_000, // 100 SUI
        1000,
    );
    
    // T=2ms: 签名
    let signature = keystore.sign_secure(&tx, alice_address)?;
    let signed_tx = Transaction::from_data(tx, signature);
    
    // T=5ms: 发送到全节点
    println!("🔵 T=5ms: 发送到 FullNode1");
    let cert = fullnode1
        .quorum_driver()
        .execute_transaction_block(signed_tx)
        .await?;
    
    // T=65ms: 收到证书
    println!("✅ T={}ms: 交易确认", start.elapsed().as_millis());
    println!("Certificate: {:?}", cert);
    
    // 验证结果
    assert!(start.elapsed() < Duration::from_millis(100));
    
    Ok(())
}
```

### 5.2 共享对象交易完整示例

```rust
/// 共享对象交易的完整执行流程
#[tokio::main]
async fn shared_transaction_example() -> Result<()> {
    let start = Instant::now();
    
    // T=0ms: 构造 DEX swap 交易
    println!("📱 T=0ms: 构造 DEX 交易");
    let tx = TransactionData::new_move_call(
        alice_address,
        dex_package,
        module!("amm"),
        function!("swap_exact_input"),
        vec![],
        vec![
            CallArg::Object(ObjectArg::SharedObject {
                id: pool_id,
                initial_shared_version: SequenceNumber::from_u64(1000),
                mutable: true,
            }),
            CallArg::Pure(bcs::to_bytes(&100_000_000u64)?), // 100 SUI
            CallArg::Pure(bcs::to_bytes(&95_000_000u64)?),  // min 95 USDC
        ],
        1000,
    );
    
    // T=2ms: 签名
    let signature = keystore.sign_secure(&tx, alice_address)?;
    let signed_tx = Transaction::from_data(tx, signature);
    
    // T=5ms: 发送到全节点
    println!("🔵 T=5ms: 发送到 FullNode1");
    println!("🔵 T=5ms: 检测到共享对象，转发到共识层");
    
    // T=10-500ms: Mysticeti 共识
    println!("⏳ T=10ms: 进入 Mysticeti 共识...");
    
    let cert = fullnode1
        .quorum_driver()
        .execute_transaction_block(signed_tx)
        .await?;
    
    // T=635ms: 收到证书
    println!("✅ T={}ms: 交易确认", start.elapsed().as_millis());
    println!("Certificate: {:?}", cert);
    
    // 验证结果
    assert!(start.elapsed() > Duration::from_millis(500));
    assert!(start.elapsed() < Duration::from_millis(1000));
    
    Ok(())
}
```

---

## 6. 总结

### 6.1 核心要点

1. **简单交易**:
   - 跳过共识，直接执行
   - 延迟 ~65ms
   - 吞吐量 ~100k TPS
   - 完全并行

2. **共享对象交易**:
   - 需要 Mysticeti 共识排序
   - 延迟 ~635ms (主要是共识开销)
   - 吞吐量 ~5k TPS
   - 受共识限制

3. **关键差异**:
   - 只在共识阶段不同
   - 执行、提交、签名逻辑完全相同
   - 对象锁定机制不同（版本锁 vs 共识序号锁）

4. **性能差异**:
   - 延迟差异: 10倍
   - 吞吐量差异: 20倍
   - 根本原因: 共识排序开销

### 6.2 设计哲学

```rust
/// Sui 的设计哲学
pub struct SuiDesignPhilosophy {
    principle: "只在必要时使用共识",
    
    rationale: {
        simple_transactions: "90%+ 交易是简单交易",
        optimization: "为常见情况优化（无共识）",
        necessity: "只有真正需要排序时才用共识",
    },
    
    result: {
        best_case: "简单交易极快 (~65ms)",
        worst_case: "共享对象仍然很快 (~635ms)",
        comparison: "比传统链快 10-100 倍",
    },
}
```

---

**文档结束**

*本文档详细描述了 Sui 区块链中简单交易和共享对象交易的完整执行时序，包括所有节点的行为和时间点。*
