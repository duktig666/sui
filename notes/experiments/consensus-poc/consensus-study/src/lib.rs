// Consensus Study - 验证对 Sui Mysticeti 共识协议的理解

pub mod dag_builder;

/// Re-export commonly used types for convenience
pub use consensus_config::{AuthorityIndex, Committee, Parameters};
pub use consensus_types::block::{BlockDigest, BlockRef, Round};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_imports_work() {
        // 验证基础导入正常
        let _round: Round = 1;
        let _author: AuthorityIndex = AuthorityIndex::new_for_test(0);
        let _digest = BlockDigest::default();
        let block_ref = BlockRef::new(_round, _author, _digest);
        assert_eq!(block_ref.round, 1);
    }
}
