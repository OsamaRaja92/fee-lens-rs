//! Signature-verifying precompiles.
//!
//! The base fee is charged per signature, and the signatures a precompile
//! verifies are billed alongside the ones the message header requires. A
//! transaction with one required signature and one Ed25519 precompile signature
//! pays 10,000 lamports, not 5,000.
//!
//! Counting only `num_required_signatures` understates the base fee by 5,000
//! lamports per precompile signature. Nothing in the recorded block exercises
//! this — mainnet precompile transactions are rare, roughly one per block in the
//! slots sampled — which is exactly why the model is checked against the charged
//! fee rather than against a corpus that happens to agree with it.
//!
//! Each precompile's instruction data begins with a one-byte count of the
//! signatures it verifies.

use crate::types::NormalisedInstruction;

/// Verifies Ed25519 signatures supplied in its instruction data.
pub const ED25519_PROGRAM: &str = "Ed25519SigVerify111111111111111111111111111";
/// Verifies secp256k1 signatures, recovering the signer from a Keccak hash.
pub const SECP256K1_PROGRAM: &str = "KeccakSecp256k11111111111111111111111111111";
/// Verifies secp256r1 (P-256) signatures, as used by passkeys.
pub const SECP256R1_PROGRAM: &str = "Secp256r1SigVerify1111111111111111111111111";

/// Precompiles whose verified signatures are billed like message signatures.
pub const SIGNATURE_PRECOMPILES: &[&str] = &[ED25519_PROGRAM, SECP256K1_PROGRAM, SECP256R1_PROGRAM];

/// How many signatures the transaction's precompile instructions verify.
///
/// Only top-level instructions count: a precompile cannot be invoked through CPI.
#[must_use]
pub fn count_precompile_signatures(instructions: &[NormalisedInstruction]) -> u32 {
    instructions
        .iter()
        .filter(|instruction| SIGNATURE_PRECOMPILES.contains(&instruction.program_address.as_str()))
        .map(|instruction| u32::from(instruction.data.first().copied().unwrap_or(0)))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction(program: &str, data: Vec<u8>) -> NormalisedInstruction {
        NormalisedInstruction {
            program_address: program.to_owned(),
            account_addresses: Vec::new(),
            data,
            stack_height: 1,
        }
    }

    #[test]
    fn reads_the_count_from_the_first_data_byte() {
        assert_eq!(
            count_precompile_signatures(&[instruction(ED25519_PROGRAM, vec![1, 0])]),
            1
        );
        assert_eq!(
            count_precompile_signatures(&[instruction(ED25519_PROGRAM, vec![7, 0])]),
            7
        );
    }

    #[test]
    fn counts_every_signature_verifying_precompile() {
        let instructions = [
            instruction(ED25519_PROGRAM, vec![1, 0]),
            instruction(SECP256K1_PROGRAM, vec![2, 0]),
            instruction(SECP256R1_PROGRAM, vec![3, 0]),
        ];
        assert_eq!(count_precompile_signatures(&instructions), 6);
    }

    #[test]
    fn ignores_other_programs() {
        let ix = [instruction(crate::builtins::SYSTEM_PROGRAM, vec![9])];
        assert_eq!(count_precompile_signatures(&ix), 0);
    }

    #[test]
    fn treats_empty_data_as_zero() {
        assert_eq!(
            count_precompile_signatures(&[instruction(ED25519_PROGRAM, Vec::new())]),
            0
        );
    }
}
