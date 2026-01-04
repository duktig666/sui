# Hybrid Transaction Design Review (v1.2)

> **Reviewer**: Gemini
> **Date**: 2025-12-31
> **Subject**: Move <-> Native Hybrid Transaction Model (Synchronous Callback)
> **Reference**: `notes/dex_l1/docs/07-MOVE-INTEGRATION-DESIGN.md` (v1.2)

---

## 1. Executive Summary

The v1.2 design introduces a significant architectural shift from "Event Listeners" to a **"Synchronous Callback"** model for handling hybrid transactions (Deposit/Withdraw). This change fundamentally improves the atomicity guarantees by binding the DEX state transitions to the Move transaction execution lifecycle.

**Verdict**: **Conditional Approval**. The conceptual model of "Synchronous Callbacks" combined with "Atomic Locks" is sound and formally verifiable. However, a critical implementation risk remains regarding the *timing* of the Commit phase within the Sui transaction lifecycle (Signing vs. Execution).

---

## 2. Correctness Verification

### 2.1 Synchronous Callback vs. Event Listener
The move to synchronous callbacks addresses the "Gap Problem" inherent in event-based systems:
- **Old Model (Events)**: Move Commit -> Event Emitted -> (Gap) -> DEX Indexer picks up -> DEX State Update.
    - *Risk*: Chain reorgs (rare in Sui but possible), Indexer lag, or crash during the gap could lead to state drift.
- **New Model (Sync Callback)**: Move Execution -> DEX Callback -> Merge Effects -> Commit.
    - *Benefit*: The DEX state change is calculated *as part of* the transaction execution. The resulting `Effects` commit (or abort) the state changes for both systems simultaneously from the user's perspective.

### 2.2 Formal Model Soundness
The formal definitions provided in Section 6.7 are logically sound:
- **State Space**: Properly captures the dual-state nature ($M$ for Move, $D$ for DEX) and the auxiliary states ($W$ for WAL, $L$ for Locks).
- **Invariants**: The "Balance Conservation" invariant ($M_{custody} = D_{total}$) is the correct correctness condition.
- **Transition Rules**: The definition of `withdraw` using a 5-step process (Lock -> Move -> Unlock/Commit) correctly models a 2-Phase Commit (2PC) simplified for this specific context.

---

## 3. Deep Dive: Atomicity & Safety

### 3.1 Deposit Atomicity (The "Merge" Assumption)
The design assumes that `Effects::merge` guarantees atomicity.
- **Mechanism**: `execute_deposit` runs Move logic, then calls DEX `credit_balance`, then merges effects.
- **Safety**:
    - If Move fails: Transaction aborts, DEX is never called. Safe.
    - If DEX fails: Function returns error, entire transaction aborts. Safe.
    - **Risk**: The DEX must *not* persist the credit to WAL/Disk during the `credit_balance` call *unless* it can rollback if the transaction fails later (e.g., during signature aggregation).
    - **Recommendation**: Ensure `credit_balance` only modifies *transient* memory state or generates a *pending* Effect. The permanent WAL write must happen only when the Transaction Certificate is executed.

### 3.2 Withdraw Atomicity (The Locking Mechanism)
The Withdraw flow uses a "Lock -> Execute -> Commit" pattern.
- **Locking (Step 1)**: `lock_for_withdraw` writes `LockCreated` to WAL immediately.
    - *Analysis*: This is safe because it only *reserves* funds. If the node crashes or the transaction fails, the TTL mechanism (`cleanup_expired_locks`) will eventually release the funds.
- **Move Execution (Step 2)**: Standard Move execution.
- **Commit/Rollback (Step 3)**:
    - *Code Snippet*: `commit_withdraw` is called immediately after `move_executor.execute` returns success.
    - **CRITICAL RISK (The "Pre-Commit" Flaw)**:
        - In Sui, `execute_transaction` often runs during the **Signing Phase** (to generate effects for signing) *before* a Certificate is formed.
        - If `commit_withdraw` (which permanently deducts balance and removes the lock) runs during the Signing Phase:
            1. Validator A executes, signs, and **commits DEX withdrawal**.
            2. The client fails to gather enough signatures (e.g., network partition).
            3. The transaction **never happens on-chain**.
            4. **Result**: User loses funds on DEX (balance deducted) but gets nothing on Move.
    - **Fix Requirement**: `commit_withdraw` must be deferred. The `execute_withdraw` function should only return an *Effect* (e.g., `WithdrawEffect`). The actual application of this effect (modifying DEX balance + WAL `WithdrawCommitted`) must occur only when the **Certificate is executed** (finalized).

### 3.3 Timeout & Concurrency
- **Concurrency**: The `DashMap` entry API usage (`entry.or_insert`) correctly handles local concurrency (multiple threads on one validator).
- **Distributed Race Conditions**:
    - If User sends Request A to Validator 1 and Request B to Validator 2 simultaneously (double spend attempt):
    - Both create Locks.
    - Move VM (on-chain) will sequence them. Only one will succeed (Nonce/Object Version check).
    - The failed one triggers `Rollback` (or timeout). Safe.
- **Timeouts**: The 30s TTL is reasonable. The `cleanup_expired_locks` loop ensures liveness.

---

## 4. Recommendations & Required Changes

1.  **Defer State Commit**:
    - Modify `HybridExecutor::execute_withdraw`: Do **not** call `commit_withdraw` directly. Instead, return a `DexEffect::Withdraw({lock_id})`.
    - Implement a `DexEffectApplicator` that runs *only* when the Authority processes a verified Certificate.
    - **Why**: This aligns the DEX state commit point with the Move state commit point.

2.  **Refine Deposit WAL Logic**:
    - Similarly, `credit_balance` should not write `BalanceIncreased` to WAL immediately. It should write a tentative entry or return an Effect.
    - The permanent WAL entry should be written upon Certificate execution.

3.  **Idempotency**:
    - Ensure that re-playing the same Certificate (e.g., node restart) doesn't double-credit or double-withdraw.
    - The WAL/Effect application must check if the `tx_digest` has already been processed.

## 5. Conclusion

The v1.2 design is a major improvement and provides a solid theoretical foundation for safe hybrid transactions. The Formal Model accurately describes the intended behavior. However, the implementation must strictly separate **Execution (Simulation/Signing)** from **Commit (Certificate Application)** to prevent fund loss during consensus failures.

With the **"Defer State Commit"** fix applied, the system will satisfy the Safety Theorems defined in the formal model.

---

## 6. 设计响应 / Design Response (2026-01-01)

**评审状态**: ✅ 设计已完整解决所有问题

| 评审建议 | 设计响应 | 位置 |
|----------|----------|------|
| Defer State Commit | ✅ 两阶段执行模型 | 07:674-872 |
| Refine Deposit WAL | ✅ PendingDexEffect | 07:687-699 |
| Idempotency | ✅ tx_digest 去重 | 07:816-821 |

**确认人**: Claude (2026-01-01)
