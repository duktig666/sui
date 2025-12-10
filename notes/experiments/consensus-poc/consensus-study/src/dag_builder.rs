// DAG Builder - 用于测试和可视化的简化 DAG 构建器

use consensus_types::block::{BlockDigest, BlockRef, Round};
use consensus_config::AuthorityIndex;
use std::collections::HashMap;

/// 简化的 Block 表示（用于测试）
#[derive(Debug, Clone)]
pub struct TestBlock {
    pub block_ref: BlockRef,
    pub ancestors: Vec<BlockRef>,
    pub timestamp_ms: u64,
}

impl TestBlock {
    pub fn new(round: Round, author: AuthorityIndex, ancestors: Vec<BlockRef>) -> Self {
        // 简化的摘要计算（测试用）
        let digest_bytes = Self::compute_test_digest(round, author, &ancestors);
        let digest = BlockDigest(digest_bytes);

        Self {
            block_ref: BlockRef::new(round, author, digest),
            ancestors,
            timestamp_ms: 0,
        }
    }

    /// 测试用的简化摘要计算
    fn compute_test_digest(round: Round, author: AuthorityIndex, ancestors: &[BlockRef]) -> [u8; 32] {
        let mut digest = [0u8; 32];

        // 简单混合：round + author + ancestors count
        digest[0..4].copy_from_slice(&round.to_le_bytes());
        digest[4..8].copy_from_slice(&(author.value() as u32).to_le_bytes());
        digest[8..12].copy_from_slice(&(ancestors.len() as u32).to_le_bytes());

        // 添加第一个 ancestor 的部分摘要
        if let Some(ancestor) = ancestors.first() {
            digest[12..20].copy_from_slice(&ancestor.digest.0[0..8]);
        }

        digest
    }

    pub fn reference(&self) -> BlockRef {
        self.block_ref
    }
}

/// 简化的 DAG 构建器
pub struct DagBuilder {
    /// 所有已添加的 blocks
    blocks: HashMap<BlockRef, TestBlock>,

    /// 按轮次索引
    blocks_by_round: HashMap<Round, Vec<BlockRef>>,

    /// 按 authority 索引
    blocks_by_authority: HashMap<AuthorityIndex, Vec<BlockRef>>,

    /// 委员会大小
    committee_size: usize,
}

impl DagBuilder {
    pub fn new(committee_size: usize) -> Self {
        let mut builder = Self {
            blocks: HashMap::new(),
            blocks_by_round: HashMap::new(),
            blocks_by_authority: HashMap::new(),
            committee_size,
        };

        // 添加创世区块（round 0）
        for authority in 0..committee_size {
            let author = AuthorityIndex::new_for_test(authority as u32);
            let genesis_block = TestBlock::new(0, author, vec![]);
            builder.add_block(genesis_block);
        }

        builder
    }

    /// 添加一个 block 到 DAG
    pub fn add_block(&mut self, block: TestBlock) {
        let block_ref = block.reference();
        let round = block_ref.round;
        let author = block_ref.author;

        // 验证祖先存在
        for ancestor in &block.ancestors {
            assert!(
                self.blocks.contains_key(ancestor),
                "Ancestor {:?} not found in DAG",
                ancestor
            );
        }

        // 添加到索引
        self.blocks_by_round
            .entry(round)
            .or_insert_with(Vec::new)
            .push(block_ref);

        self.blocks_by_authority
            .entry(author)
            .or_insert_with(Vec::new)
            .push(block_ref);

        self.blocks.insert(block_ref, block);
    }

    /// 为指定轮次的所有 authority 创建 blocks
    /// 每个 block 引用上一轮的所有 blocks
    pub fn add_round(&mut self, round: Round) {
        assert!(round > 0, "Cannot add genesis round");

        let prev_round = round - 1;

        // 收集上一轮的 BlockRefs（避免借用冲突）
        let ancestors: Vec<BlockRef> = self.get_blocks_at_round(prev_round)
            .iter()
            .map(|b| b.reference())
            .collect();

        for authority in 0..self.committee_size {
            let author = AuthorityIndex::new_for_test(authority as u32);
            let block = TestBlock::new(round, author, ancestors.clone());
            self.add_block(block);
        }
    }

    /// 获取指定轮次的所有 blocks
    pub fn get_blocks_at_round(&self, round: Round) -> Vec<&TestBlock> {
        self.blocks_by_round
            .get(&round)
            .map(|refs| refs.iter().filter_map(|r| self.blocks.get(r)).collect())
            .unwrap_or_default()
    }

    /// 获取指定 authority 的所有 blocks
    pub fn get_blocks_by_authority(&self, author: AuthorityIndex) -> Vec<&TestBlock> {
        self.blocks_by_authority
            .get(&author)
            .map(|refs| refs.iter().filter_map(|r| self.blocks.get(r)).collect())
            .unwrap_or_default()
    }

    /// 获取特定 block
    pub fn get_block(&self, block_ref: &BlockRef) -> Option<&TestBlock> {
        self.blocks.get(block_ref)
    }

    /// 获取所有 blocks
    pub fn all_blocks(&self) -> Vec<&TestBlock> {
        self.blocks.values().collect()
    }

    /// 获取最高轮次
    pub fn highest_round(&self) -> Round {
        *self.blocks_by_round.keys().max().unwrap_or(&0)
    }

    /// 检查 DAG 的连通性（所有非创世 blocks 都有祖先）
    pub fn validate_connectivity(&self) -> Result<(), String> {
        for (block_ref, block) in &self.blocks {
            if block_ref.round == 0 {
                if !block.ancestors.is_empty() {
                    return Err(format!("Genesis block {:?} should have no ancestors", block_ref));
                }
            } else {
                if block.ancestors.is_empty() {
                    return Err(format!("Non-genesis block {:?} must have ancestors", block_ref));
                }

                // 验证祖先轮次 < 当前轮次
                for ancestor in &block.ancestors {
                    if ancestor.round >= block_ref.round {
                        return Err(format!(
                            "Block {:?} has ancestor {:?} with round >= current round",
                            block_ref, ancestor
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// 计算统计信息
    pub fn stats(&self) -> DagStats {
        let total_blocks = self.blocks.len();
        let rounds = self.blocks_by_round.len();
        let max_round = self.highest_round();

        let mut total_ancestors = 0;
        for block in self.blocks.values() {
            total_ancestors += block.ancestors.len();
        }

        let avg_ancestors = if total_blocks > 0 {
            total_ancestors as f64 / total_blocks as f64
        } else {
            0.0
        };

        DagStats {
            total_blocks,
            rounds,
            max_round,
            avg_ancestors,
            committee_size: self.committee_size,
        }
    }
}

#[derive(Debug)]
pub struct DagStats {
    pub total_blocks: usize,
    pub rounds: usize,
    pub max_round: Round,
    pub avg_ancestors: f64,
    pub committee_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_builder_genesis() {
        let builder = DagBuilder::new(4);

        // 应该有 4 个创世区块
        let genesis_blocks = builder.get_blocks_at_round(0);
        assert_eq!(genesis_blocks.len(), 4);

        // 每个创世区块应该没有祖先
        for block in genesis_blocks {
            assert_eq!(block.ancestors.len(), 0);
            assert_eq!(block.block_ref.round, 0);
        }
    }

    #[test]
    fn test_dag_builder_add_round() {
        let mut builder = DagBuilder::new(4);

        // 添加 round 1
        builder.add_round(1);

        let round1_blocks = builder.get_blocks_at_round(1);
        assert_eq!(round1_blocks.len(), 4);

        // 每个 block 应该引用 4 个创世区块
        for block in round1_blocks {
            assert_eq!(block.ancestors.len(), 4);
            assert_eq!(block.block_ref.round, 1);
        }
    }

    #[test]
    fn test_dag_builder_multiple_rounds() {
        let mut builder = DagBuilder::new(4);

        // 构建 10 轮
        for round in 1..=10 {
            builder.add_round(round);
        }

        assert_eq!(builder.highest_round(), 10);

        let stats = builder.stats();
        assert_eq!(stats.total_blocks, 44); // 11 rounds * 4 authorities
        assert_eq!(stats.rounds, 11); // 0..=10
        assert_eq!(stats.committee_size, 4);

        // 验证连通性
        builder.validate_connectivity().unwrap();
    }

    #[test]
    fn test_get_blocks_by_authority() {
        let mut builder = DagBuilder::new(4);

        for round in 1..=5 {
            builder.add_round(round);
        }

        let author0 = AuthorityIndex::new_for_test(0);
        let blocks = builder.get_blocks_by_authority(author0);

        // 应该有 6 个 blocks (rounds 0..=5)
        assert_eq!(blocks.len(), 6);

        // 验证都是 author 0 的
        for block in blocks {
            assert_eq!(block.block_ref.author, author0);
        }
    }

    #[test]
    fn test_dag_stats() {
        let mut builder = DagBuilder::new(4);

        for round in 1..=5 {
            builder.add_round(round);
        }

        let stats = builder.stats();
        println!("DAG Stats: {:?}", stats);

        assert_eq!(stats.total_blocks, 24); // 6 rounds * 4 authorities
        assert_eq!(stats.max_round, 5);
        assert!(stats.avg_ancestors > 0.0);
    }
}
