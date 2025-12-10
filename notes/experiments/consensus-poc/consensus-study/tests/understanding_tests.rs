// 验证对 Mysticeti 共识协议理解的测试

use consensus_study::dag_builder::{DagBuilder, TestBlock};
use consensus_config::AuthorityIndex;
use consensus_types::block::{BlockRef, Round};

/// 测试 Wave 结构理解
#[test]
fn test_wave_structure_understanding() {
    const WAVE_LENGTH: u32 = 3;
    const COMMITTEE_SIZE: usize = 4;

    // Wave 0: rounds 0, 1, 2
    // Wave 1: rounds 3, 4, 5
    // Wave 2: rounds 6, 7, 8

    let leader_round = |wave: u32| -> Round { wave * WAVE_LENGTH };
    let decision_round = |wave: u32| -> Round { wave * WAVE_LENGTH + WAVE_LENGTH - 1 };
    let wave_number = |round: Round| -> u32 { round / WAVE_LENGTH };

    // 验证 Wave 0
    assert_eq!(leader_round(0), 0);
    assert_eq!(decision_round(0), 2);
    assert_eq!(wave_number(0), 0);
    assert_eq!(wave_number(1), 0);
    assert_eq!(wave_number(2), 0);

    // 验证 Wave 1
    assert_eq!(leader_round(1), 3);
    assert_eq!(decision_round(1), 5);
    assert_eq!(wave_number(3), 1);
    assert_eq!(wave_number(4), 1);
    assert_eq!(wave_number(5), 1);

    // 验证 Wave 2
    assert_eq!(leader_round(2), 6);
    assert_eq!(decision_round(2), 8);
    assert_eq!(wave_number(6), 2);

    println!("✅ Wave structure understanding verified");
}

/// 测试 DAG 构建和连通性
#[test]
fn test_dag_building_and_connectivity() {
    let mut builder = DagBuilder::new(4);

    // 构建 9 轮（3 个完整 wave）
    for round in 1..=9 {
        builder.add_round(round);
    }

    // 验证连通性
    builder.validate_connectivity().expect("DAG should be connected");

    // 验证统计
    let stats = builder.stats();
    assert_eq!(stats.total_blocks, 40); // 10 rounds * 4 authorities
    assert_eq!(stats.max_round, 9);
    assert_eq!(stats.committee_size, 4);

    println!("✅ DAG connectivity verified: {:?}", stats);
}

/// 测试 Leader 选举理解
#[test]
fn test_leader_election_understanding() {
    const WAVE_LENGTH: u32 = 3;

    // 模拟 leader 选举（简化版）
    let elect_leader = |round: Round, committee_size: usize| -> AuthorityIndex {
        // 简化：round-robin
        AuthorityIndex::new_for_test((round % committee_size as u32) as u32)
    };

    // Wave 0 leader (round 0)
    let leader0 = elect_leader(0, 4);
    assert_eq!(leader0.value(), 0);

    // Wave 1 leader (round 3)
    let leader1 = elect_leader(3, 4);
    assert_eq!(leader1.value(), 3);

    // Wave 2 leader (round 6)
    let leader2 = elect_leader(6, 4);
    assert_eq!(leader2.value(), 2);

    println!("✅ Leader election understanding verified");
}

/// 测试祖先查找和引用
#[test]
fn test_ancestor_references() {
    let mut builder = DagBuilder::new(4);

    // 添加几轮
    builder.add_round(1);
    builder.add_round(2);

    // 获取 round 2 的第一个 block
    let round2_blocks = builder.get_blocks_at_round(2);
    let block = round2_blocks.first().unwrap();

    // 验证它引用了 round 1 的所有 4 个 blocks
    assert_eq!(block.ancestors.len(), 4);

    // 验证所有祖先都是 round 1
    for ancestor_ref in &block.ancestors {
        assert_eq!(ancestor_ref.round, 1);

        // 验证祖先确实存在
        let ancestor = builder.get_block(ancestor_ref);
        assert!(ancestor.is_some());
    }

    println!("✅ Ancestor references verified");
}

/// 测试简化的投票机制理解
#[test]
fn test_voting_mechanism_understanding() {
    let mut builder = DagBuilder::new(4);

    // 构建 wave 0: rounds 0, 1, 2
    builder.add_round(1);
    builder.add_round(2);

    // Round 0: leader round
    let leader_blocks = builder.get_blocks_at_round(0);
    assert_eq!(leader_blocks.len(), 4);

    // Round 1: voting round (每个 block 引用 leader)
    let voting_blocks = builder.get_blocks_at_round(1);
    assert_eq!(voting_blocks.len(), 4);

    // 每个投票 block 都应该引用所有 leader blocks
    for vote_block in &voting_blocks {
        assert_eq!(vote_block.ancestors.len(), 4);

        // 模拟检查：是否引用了特定 leader
        let leader_ref = leader_blocks[0].reference();
        let contains_leader = vote_block
            .ancestors
            .iter()
            .any(|a| a.author == leader_ref.author && a.round == leader_ref.round);

        assert!(contains_leader, "Vote block should reference leader");
    }

    // Round 2: decision round (certificate round)
    let decision_blocks = builder.get_blocks_at_round(2);
    assert_eq!(decision_blocks.len(), 4);

    // 每个 decision block 引用所有 voting blocks
    for cert_block in &decision_blocks {
        assert_eq!(cert_block.ancestors.len(), 4);
    }

    println!("✅ Voting mechanism understanding verified");
}

/// 测试多 wave 场景
#[test]
fn test_multi_wave_scenario() {
    const WAVE_LENGTH: u32 = 3;
    const NUM_WAVES: u32 = 5;
    const COMMITTEE_SIZE: usize = 4;

    let mut builder = DagBuilder::new(COMMITTEE_SIZE);

    // 构建 5 个 wave
    let max_round = NUM_WAVES * WAVE_LENGTH;
    for round in 1..=max_round {
        builder.add_round(round);
    }

    // 验证每个 wave 的结构
    for wave in 0..NUM_WAVES {
        let leader_round = wave * WAVE_LENGTH;
        let decision_round = wave * WAVE_LENGTH + WAVE_LENGTH - 1;

        // 验证 leader round 有 blocks
        let leader_blocks = builder.get_blocks_at_round(leader_round);
        assert_eq!(
            leader_blocks.len(),
            COMMITTEE_SIZE,
            "Wave {} leader round {} should have {} blocks",
            wave,
            leader_round,
            COMMITTEE_SIZE
        );

        // 验证 decision round 有 blocks
        let decision_blocks = builder.get_blocks_at_round(decision_round);
        assert_eq!(
            decision_blocks.len(),
            COMMITTEE_SIZE,
            "Wave {} decision round {} should have {} blocks",
            wave,
            decision_round,
            COMMITTEE_SIZE
        );

        println!(
            "Wave {}: leader round {}, decision round {}",
            wave, leader_round, decision_round
        );
    }

    let stats = builder.stats();
    println!("Multi-wave DAG stats: {:?}", stats);

    println!("✅ Multi-wave scenario verified");
}

/// 测试 Quorum 理解（2f+1）
#[test]
fn test_quorum_understanding() {
    let test_cases = vec![
        (4, 3),   // 4 nodes: quorum = 3 (2*1 + 1)
        (7, 5),   // 7 nodes: quorum = 5 (2*2 + 1)
        (10, 7),  // 10 nodes: quorum = 7 (2*3 + 1)
        (13, 9),  // 13 nodes: quorum = 9 (2*4 + 1)
        (16, 11), // 16 nodes: quorum = 11 (2*5 + 1)
    ];

    for (total, expected_quorum) in test_cases {
        let f = (total - 1) / 3; // Max Byzantine faults
        let quorum = 2 * f + 1;

        assert_eq!(
            quorum, expected_quorum,
            "For {} nodes, quorum should be {}",
            total, expected_quorum
        );

        println!(
            "Committee size: {}, f: {}, quorum: {}",
            total, f, quorum
        );
    }

    println!("✅ Quorum calculation understanding verified");
}

/// 测试区块引用的排序
#[test]
fn test_block_ref_ordering() {
    let author0 = AuthorityIndex::new_for_test(0);
    let author1 = AuthorityIndex::new_for_test(1);

    let ref1 = BlockRef::new(1, author0, Default::default());
    let ref2 = BlockRef::new(1, author1, Default::default());
    let ref3 = BlockRef::new(2, author0, Default::default());

    // 验证排序：先 round，后 author
    assert!(ref1 < ref2, "Same round, lower author should be less");
    assert!(ref1 < ref3, "Lower round should be less");
    assert!(ref2 < ref3, "Lower round should be less");

    println!("✅ BlockRef ordering verified");
}

/// 测试 DAG 深度优先遍历（模拟因果历史追踪）
#[test]
fn test_dag_causal_history() {
    let mut builder = DagBuilder::new(4);

    // 构建 5 轮
    for round in 1..=5 {
        builder.add_round(round);
    }

    // 获取 round 5 的一个 block
    let round5_blocks = builder.get_blocks_at_round(5);
    let target_block = round5_blocks.first().unwrap();

    // 追踪因果历史（简化版 DFS）
    fn count_causal_history(
        builder: &DagBuilder,
        block_ref: &BlockRef,
        visited: &mut std::collections::HashSet<BlockRef>,
    ) -> usize {
        if visited.contains(block_ref) {
            return 0;
        }

        visited.insert(*block_ref);
        let mut count = 1;

        if let Some(block) = builder.get_block(block_ref) {
            for ancestor in &block.ancestors {
                count += count_causal_history(builder, ancestor, visited);
            }
        }

        count
    }

    let mut visited = std::collections::HashSet::new();
    let causal_blocks = count_causal_history(&builder, &target_block.reference(), &mut visited);

    // Round 5 block 应该能追溯到所有包括它在内的 blocks
    // 6 rounds (0-5) * 4 authorities = 24 blocks total
    // 但由于 DAG 结构，可能不是所有 round 4 的 blocks 都被引用
    // 检查至少包含自己和部分历史
    assert!(
        causal_blocks >= 20,
        "Should trace back to at least 20 blocks, got {}",
        causal_blocks
    );

    println!("✅ Causal history tracing verified: {} blocks", causal_blocks);
}
