//! Decoder for the `ComputeBudget` program's instructions.
//!
//! The instruction data is a one-byte discriminant followed by a little-endian
//! payload. Decoding it directly, rather than relying on an RPC's `jsonParsed`
//! output, keeps the tool working against any endpoint and makes the parsing
//! testable without a network call.

use crate::builtins::COMPUTE_BUDGET_PROGRAM;
use crate::types::{ComputeBudgetDirectives, NormalisedInstruction};

/// Deprecated, superseded by `SetComputeUnitLimit`.
const REQUEST_UNITS_DEPRECATED: u8 = 0;
const REQUEST_HEAP_FRAME: u8 = 1;
const SET_COMPUTE_UNIT_LIMIT: u8 = 2;
const SET_COMPUTE_UNIT_PRICE: u8 = 3;
const SET_LOADED_ACCOUNTS_DATA_SIZE_LIMIT: u8 = 4;

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    data.get(offset..offset + 8)?
        .try_into()
        .ok()
        .map(u64::from_le_bytes)
}

/// Reads every `ComputeBudget` directive out of a transaction's top-level instructions.
///
/// A truncated payload leaves its field `None` rather than being guessed at.
#[must_use]
pub fn decode_compute_budget(
    instructions: &[NormalisedInstruction],
    is_version_1: bool,
) -> ComputeBudgetDirectives {
    let mut directives = ComputeBudgetDirectives::default();

    for instruction in instructions {
        if instruction.program_address != COMPUTE_BUDGET_PROGRAM {
            continue;
        }
        directives.instruction_count += 1;

        let data = &instruction.data;
        let Some(&discriminant) = data.first() else {
            continue;
        };

        match discriminant {
            SET_COMPUTE_UNIT_LIMIT | REQUEST_UNITS_DEPRECATED => {
                directives.compute_unit_limit =
                    read_u32_le(data, 1).or(directives.compute_unit_limit);
            }
            SET_COMPUTE_UNIT_PRICE => {
                directives.compute_unit_price_micro_lamports =
                    read_u64_le(data, 1).or(directives.compute_unit_price_micro_lamports);
            }
            SET_LOADED_ACCOUNTS_DATA_SIZE_LIMIT => {
                directives.loaded_accounts_data_size_limit =
                    read_u32_le(data, 1).or(directives.loaded_accounts_data_size_limit);
            }
            REQUEST_HEAP_FRAME => {
                directives.heap_frame_bytes = read_u32_le(data, 1).or(directives.heap_frame_bytes);
            }
            _ => {}
        }
    }

    directives.inert_on_v1 = is_version_1 && directives.instruction_count > 0;
    directives
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

    fn with_u32(discriminant: u8, value: u32) -> Vec<u8> {
        let mut data = vec![discriminant];
        data.extend_from_slice(&value.to_le_bytes());
        data
    }

    fn with_u64(discriminant: u8, value: u64) -> Vec<u8> {
        let mut data = vec![discriminant];
        data.extend_from_slice(&value.to_le_bytes());
        data
    }

    #[test]
    fn reads_a_compute_unit_limit() {
        let ix = [instruction(COMPUTE_BUDGET_PROGRAM, with_u32(2, 300_000))];
        let directives = decode_compute_budget(&ix, false);
        assert_eq!(directives.compute_unit_limit, Some(300_000));
        assert_eq!(directives.instruction_count, 1);
    }

    #[test]
    fn reads_a_price_at_the_full_width_of_u64() {
        let ix = [instruction(COMPUTE_BUDGET_PROGRAM, with_u64(3, u64::MAX))];
        let directives = decode_compute_budget(&ix, false);
        assert_eq!(directives.compute_unit_price_micro_lamports, Some(u64::MAX));
    }

    #[test]
    fn reads_the_data_size_limit_and_heap_frame() {
        let ix = [
            instruction(COMPUTE_BUDGET_PROGRAM, with_u32(4, 65_536)),
            instruction(COMPUTE_BUDGET_PROGRAM, with_u32(1, 262_144)),
        ];
        let directives = decode_compute_budget(&ix, false);
        assert_eq!(directives.loaded_accounts_data_size_limit, Some(65_536));
        assert_eq!(directives.heap_frame_bytes, Some(262_144));
        assert_eq!(directives.instruction_count, 2);
    }

    #[test]
    fn ignores_other_programs() {
        let ix = [instruction(
            crate::builtins::SYSTEM_PROGRAM,
            with_u32(2, 300_000),
        )];
        let directives = decode_compute_budget(&ix, false);
        assert_eq!(directives.compute_unit_limit, None);
        assert_eq!(directives.instruction_count, 0);
    }

    #[test]
    fn leaves_a_truncated_payload_unread_rather_than_guessing() {
        let ix = [instruction(COMPUTE_BUDGET_PROGRAM, vec![2, 0, 1])];
        let directives = decode_compute_budget(&ix, false);
        assert_eq!(directives.compute_unit_limit, None);
        assert_eq!(directives.instruction_count, 1);
    }

    #[test]
    fn tolerates_empty_data() {
        let ix = [instruction(COMPUTE_BUDGET_PROGRAM, Vec::new())];
        let directives = decode_compute_budget(&ix, false);
        assert_eq!(directives.compute_unit_limit, None);
        assert_eq!(directives.compute_unit_price_micro_lamports, None);
    }

    #[test]
    fn flags_directives_as_inert_on_v1() {
        let ix = [instruction(COMPUTE_BUDGET_PROGRAM, with_u32(2, 0))];
        assert!(decode_compute_budget(&ix, true).inert_on_v1);
        assert!(!decode_compute_budget(&ix, false).inert_on_v1);
        assert!(!decode_compute_budget(&[], true).inert_on_v1);
    }

    #[test]
    fn accepts_the_deprecated_request_units_instruction() {
        let ix = [instruction(COMPUTE_BUDGET_PROGRAM, with_u32(0, 120_000))];
        assert_eq!(
            decode_compute_budget(&ix, false).compute_unit_limit,
            Some(120_000)
        );
    }
}
