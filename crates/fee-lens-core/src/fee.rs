//! The fee model.
//!
//! A Solana transaction fee has two parts. The base fee is 5,000 lamports for
//! each signature the message requires. The priority fee is whatever the sender
//! bid on top, and how that is expressed depends on the transaction version:
//!
//! - **legacy, v0** — `SetComputeUnitPrice` sets a RATE in micro-lamports per
//!   compute unit, and the fee is that rate times the declared compute unit
//!   limit. Declaring more compute therefore costs more lamports.
//! - **v1** — the message header carries an ABSOLUTE lamport total (SIMD-0385).
//!   The compute unit limit does not enter the fee at all.
//!
//! Treating the two as the same number is the easiest way to misread a v1
//! transaction, and the error is large: a rate is out by a factor of the limit
//! over a million.
//!
//! The base fee is charged per signature, and that includes signatures verified
//! by the Ed25519 and secp256 precompiles, not only the ones the message header
//! requires. See [`crate::precompiles`].
//!
//! Tips are not fees. They are ordinary SOL transfers to a block builder or
//! relay, so they never appear in the charged fee.
//!
//! A failed transaction pays none of them. The chain rolls its instructions back
//! and keeps only the fee, so a tip a failed transaction instructed never leaves
//! the payer's account. Counting it inflates what landing cost: in the recorded
//! block it would overstate the tips paid by a factor of nearly five. They are counted separately
//! and added only into the total cost to land.

use crate::builtins::{LAMPORTS_PER_SIGNATURE, MICRO_LAMPORTS_PER_LAMPORT};
use crate::compute::ResolvedComputeLimit;
use crate::precompiles::count_precompile_signatures;
use crate::types::{
    ComputeBudgetDirectives, FeeBreakdown, NormalisedTransaction, PriorityFeeSource, TipTransfer,
    TransactionVersion,
};

/// The priority fee, and where it came from.
#[derive(Debug, Clone, Copy)]
pub struct PriorityFee {
    /// The fee in lamports, however it was expressed on the wire.
    pub lamports: u64,
    /// Where the figure came from.
    pub source: PriorityFeeSource,
}

/// The base fee: one flat charge per charged signature.
#[must_use]
pub fn base_fee(signatures_charged: u32) -> u64 {
    LAMPORTS_PER_SIGNATURE.saturating_mul(u64::from(signatures_charged))
}

/// Lamports charged for `units` at `price_micro_lamports` per unit, rounded up.
///
/// Computed in `u128` because the product of a `u64` price and a realistic limit
/// overflows `u64`. The result saturates back into `u64`: the chain cannot charge
/// more lamports than exist, so saturation is unreachable for any transaction
/// that could be submitted, and is preferable to a wrapping surprise.
#[must_use]
pub fn charge_for(price_micro_lamports: u64, units: u64) -> u64 {
    let product = u128::from(price_micro_lamports) * u128::from(units);
    let charged = product.div_ceil(MICRO_LAMPORTS_PER_LAMPORT);
    u64::try_from(charged).unwrap_or(u64::MAX)
}

/// The priority fee implied by the transaction's own directives.
///
/// Derived from what the sender asked for, not from what was charged, which is
/// what makes it worth comparing against the charged fee.
#[must_use]
pub fn priority_fee(
    transaction: &NormalisedTransaction,
    directives: &ComputeBudgetDirectives,
    limit: ResolvedComputeLimit,
) -> PriorityFee {
    if transaction.version == TransactionVersion::V1 {
        return match transaction
            .transaction_config
            .and_then(|config| config.priority_fee_lamports)
        {
            Some(lamports) => PriorityFee {
                lamports,
                source: PriorityFeeSource::V1Header,
            },
            None => PriorityFee {
                lamports: 0,
                source: PriorityFeeSource::None,
            },
        };
    }

    match directives.compute_unit_price_micro_lamports {
        Some(price) if price > 0 => PriorityFee {
            lamports: charge_for(price, u64::from(limit.effective_limit)),
            source: PriorityFeeSource::ComputeUnitPrice,
        },
        _ => PriorityFee {
            lamports: 0,
            source: PriorityFeeSource::None,
        },
    }
}

/// Assembles the breakdown and checks the model against reality.
///
/// `model_residual_lamports` is the point of this function. The model predicts
/// the fee from the transaction's directives; the metadata says what was
/// charged. The two are independent, so their difference is a running self-test.
/// It is reported rather than smoothed over: a non-zero residual means the model
/// is missing something, and the caller should know that instead of trusting a
/// confident wrong answer.
#[must_use]
pub fn build_fee_breakdown(
    transaction: &NormalisedTransaction,
    priority: PriorityFee,
    tips: &[TipTransfer],
) -> FeeBreakdown {
    let precompile_signatures = count_precompile_signatures(&transaction.instructions);
    let signatures_charged = transaction.num_required_signatures + precompile_signatures;
    let base_lamports = base_fee(signatures_charged);
    let modelled_fee_lamports = base_lamports.saturating_add(priority.lamports);
    let observed_fee_lamports = transaction.fee_lamports;

    let succeeded = transaction.err.is_none();
    let tip_lamports_instructed: u64 = tips.iter().map(|tip| tip.lamports).sum();
    let tip_lamports = if succeeded {
        tip_lamports_instructed
    } else {
        0
    };
    let labelled_tip_lamports: u64 = if succeeded {
        tips.iter()
            .filter(|tip| tip.service.is_some())
            .map(|tip| tip.lamports)
            .sum()
    } else {
        0
    };

    FeeBreakdown {
        base_lamports,
        signatures_charged,
        precompile_signatures,
        priority_lamports: priority.lamports,
        priority_source: priority.source,
        tip_lamports,
        tip_lamports_instructed,
        labelled_tip_lamports,
        modelled_fee_lamports,
        observed_fee_lamports,
        model_residual_lamports: i128::from(observed_fee_lamports)
            - i128::from(modelled_fee_lamports),
        implied_priority_lamports: observed_fee_lamports.saturating_sub(base_lamports),
        total_cost_lamports: observed_fee_lamports.saturating_add(tip_lamports),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_the_charge_up() {
        assert_eq!(charge_for(1, 1), 1);
        assert_eq!(charge_for(1, 999_999), 1);
        assert_eq!(charge_for(1, 1_000_001), 2);
    }

    #[test]
    fn is_exact_when_the_division_is_exact() {
        assert_eq!(charge_for(1, 2_000_000), 2);
        assert_eq!(charge_for(0, 1_400_000), 0);
        assert_eq!(charge_for(57_000, 600), 35);
    }

    #[test]
    fn computes_in_u128_so_a_full_width_price_does_not_wrap() {
        // u64::MAX micro-lamports over the compute ceiling. The product exceeds
        // u64 entirely; computing in u64 would wrap and report a small number.
        let charged = charge_for(u64::MAX, u64::from(crate::builtins::MAX_COMPUTE_UNIT_LIMIT));
        assert_eq!(
            charged,
            u64::MAX,
            "the charge saturates rather than wrapping"
        );
    }

    #[test]
    fn base_fee_is_per_required_signature() {
        assert_eq!(base_fee(0), 0);
        assert_eq!(base_fee(1), 5_000);
        assert_eq!(base_fee(2), 10_000);
    }
}
