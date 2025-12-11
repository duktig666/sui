//! Throughput and performance benchmarks for Token Chain
//!
//! These benchmarks measure:
//! - Transaction submission throughput
//! - Execution latency
//! - Concurrent transaction handling

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use simple_token_chain::{Address, NodeConfig, TokenChainNode, Transaction};
use tokio::runtime::Runtime;
use std::time::Duration;

/// Helper to create runtime
fn create_runtime() -> Runtime {
    Runtime::new().unwrap()
}

/// Helper to create and start a test node
async fn setup_node() -> TokenChainNode {
    let config = NodeConfig::default_for_node(0);
    let node = TokenChainNode::new(config).unwrap();
    node.start().await.unwrap();
    node
}

/// Benchmark transaction submission rate
fn bench_submit_transactions(c: &mut Criterion) {
    let rt = create_runtime();
    let node = rt.block_on(setup_node());
    let alice = Address::from_string("alice_bench");

    // Pre-mint tokens
    rt.block_on(async {
        node.submit_transaction(Transaction::Mint {
            to: alice,
            amount: u64::MAX / 2,
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let mut group = c.benchmark_group("submit_transactions");

    for batch_size in [1, 10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &size| {
                let mut nonce = 0u64;
                b.iter(|| {
                    rt.block_on(async {
                        for _ in 0..size {
                            let bob = Address::from_string(&format!("bob_{}", nonce));
                            let tx = Transaction::Transfer {
                                from: alice,
                                to: bob,
                                amount: 1,
                                nonce,
                            };
                            nonce += 1;
                            let _ = black_box(node.submit_transaction(tx).await);
                        }
                    });
                });
            },
        );
    }

    group.finish();
    rt.block_on(node.stop()).unwrap();
}

/// Benchmark mint operation throughput
fn bench_mint_throughput(c: &mut Criterion) {
    let rt = create_runtime();
    let node = rt.block_on(setup_node());

    c.bench_function("mint_1000_accounts", |b| {
        let mut counter = 0;
        b.iter(|| {
            let _ = rt.block_on(async {
                let addr = Address::from_string(&format!("account_{}", counter));
                counter += 1;
                black_box(
                    node.submit_transaction(Transaction::Mint {
                        to: addr,
                        amount: 1000,
                    })
                    .await,
                )
            });
        });
    });

    rt.block_on(node.stop()).unwrap();
}

/// Benchmark balance queries
fn bench_balance_queries(c: &mut Criterion) {
    let rt = create_runtime();
    let node = rt.block_on(setup_node());
    let alice = Address::from_string("alice_query");

    // Setup: mint tokens
    rt.block_on(async {
        node.submit_transaction(Transaction::Mint {
            to: alice,
            amount: 1000,
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    c.bench_function("get_balance", |b| {
        b.iter(|| {
            let _ = rt.block_on(async {
                black_box(node.get_balance(alice).await)
            });
        });
    });

    rt.block_on(node.stop()).unwrap();
}

/// Benchmark nonce queries
fn bench_nonce_queries(c: &mut Criterion) {
    let rt = create_runtime();
    let node = rt.block_on(setup_node());
    let alice = Address::from_string("alice_nonce");

    // Setup: mint tokens
    rt.block_on(async {
        node.submit_transaction(Transaction::Mint {
            to: alice,
            amount: 1000,
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    c.bench_function("get_nonce", |b| {
        b.iter(|| {
            let _ = rt.block_on(async {
                black_box(node.get_nonce(alice).await)
            });
        });
    });

    rt.block_on(node.stop()).unwrap();
}

/// Benchmark sequential transfers
fn bench_sequential_transfers(c: &mut Criterion) {
    let rt = create_runtime();
    let node = rt.block_on(setup_node());
    let alice = Address::from_string("alice_seq");

    // Pre-mint very large amount for warmup iterations and measurements
    rt.block_on(async {
        node.submit_transaction(Transaction::Mint {
            to: alice,
            amount: 100_000_000_000, // 100 billion tokens
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let mut group = c.benchmark_group("sequential_transfers");

    for count in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            count,
            |b, &n| {
                b.iter(|| {
                    rt.block_on(async {
                        // Query current nonce at start of each iteration
                        let current_nonce = node.get_nonce(alice).await.unwrap();
                        let mut nonce = current_nonce;

                        for i in 0..n {
                            let bob = Address::from_string(&format!("bob_seq_{}", i));
                            node.submit_transaction(Transaction::Transfer {
                                from: alice,
                                to: bob,
                                amount: 100,
                                nonce,
                            })
                            .await
                            .unwrap();
                            nonce += 1;
                        }
                    });
                });
            },
        );
    }

    group.finish();
    rt.block_on(node.stop()).unwrap();
}

/// Stress test: high transaction volume
fn bench_high_volume_stress(c: &mut Criterion) {
    let rt = create_runtime();
    let node = rt.block_on(setup_node());

    let mut group = c.benchmark_group("stress_test");
    group.sample_size(10); // Reduce sample size for stress tests
    group.measurement_time(Duration::from_secs(30));

    // Setup multiple accounts with funds
    let accounts: Vec<_> = (0..10)
        .map(|i| Address::from_string(&format!("stress_account_{}", i)))
        .collect();

    rt.block_on(async {
        for addr in &accounts {
            node.submit_transaction(Transaction::Mint {
                to: *addr,
                amount: 10_000_000_000, // 10 billion tokens per account
            })
            .await
            .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    });

    group.bench_function("submit_1000_transactions", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Query current nonces for all accounts at start of each iteration
                let mut nonce_map = Vec::new();
                for addr in &accounts {
                    let current_nonce = node.get_nonce(*addr).await.unwrap();
                    nonce_map.push(current_nonce);
                }

                for i in 0..1000 {
                    let from_idx = i % accounts.len();
                    let to_idx = (i + 1) % accounts.len();

                    let tx = Transaction::Transfer {
                        from: accounts[from_idx],
                        to: accounts[to_idx],
                        amount: 10,
                        nonce: nonce_map[from_idx],
                    };

                    nonce_map[from_idx] += 1;

                    let _ = black_box(node.submit_transaction(tx).await);
                }
            });
        });
    });

    group.finish();
    rt.block_on(node.stop()).unwrap();
}

criterion_group!(
    benches,
    bench_submit_transactions,
    bench_mint_throughput,
    bench_balance_queries,
    bench_nonce_queries,
    bench_sequential_transfers,
    bench_high_volume_stress,
);
criterion_main!(benches);
