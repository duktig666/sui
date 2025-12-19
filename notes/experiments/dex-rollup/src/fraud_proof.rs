use crate::error::{DexError, DexResult};
use crate::types::{ExecutionBatch, FraudProof};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudProofVerifier {
    challenge_period_blocks: u64,
}

impl FraudProofVerifier {
    pub fn new(challenge_period_blocks: u64) -> Self {
        Self {
            challenge_period_blocks,
        }
    }

    pub fn verify_fraud_proof(
        &self,
        batch: &ExecutionBatch,
        proof: &FraudProof,
    ) -> DexResult<bool> {
        if batch.index != proof.batch_index {
            return Err(DexError::InvalidFraudProof(
                "Batch index mismatch".to_string(),
            ));
        }

        if batch.state_root_after != proof.claimed_state_root {
            return Err(DexError::InvalidFraudProof(
                "State root mismatch".to_string(),
            ));
        }

        Ok(proof.correct_state_root != proof.claimed_state_root)
    }

    pub fn challenge_period_blocks(&self) -> u64 {
        self.challenge_period_blocks
    }
}

impl Default for FraudProofVerifier {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DexTransaction;
    use sui_types::base_types::SuiAddress;

    #[test]
    fn test_fraud_proof_verifier() {
        let verifier = FraudProofVerifier::new(100);
        assert_eq!(verifier.challenge_period_blocks(), 100);

        let batch = ExecutionBatch {
            index: 1,
            transactions: vec![],
            trades: vec![],
            state_root_before: [0u8; 32],
            state_root_after: [1u8; 32],
            timestamp: 0,
        };

        let proof = FraudProof {
            batch_index: 1,
            claimed_state_root: [1u8; 32],
            correct_state_root: [2u8; 32],
            invalid_transaction: Box::new(DexTransaction::Deposit {
                user: SuiAddress::random_for_testing_only(),
                amount: 100,
                nonce: 0,
            }),
            proof_data: vec![],
        };

        let result = verifier.verify_fraud_proof(&batch, &proof);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }
}
