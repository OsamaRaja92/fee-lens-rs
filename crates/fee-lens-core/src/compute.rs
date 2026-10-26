//! Resolving the compute unit limit a transaction was charged against, and
//! measuring how much of it went unused.

use crate::builtins::{MAX_COMPUTE_UNIT_LIMIT, reserved_compute_units};
use crate::fee::charge_for;
use crate::types::{
    ComputeAnalysis, ComputeBudgetDirectives, ComputeLimitSource, NormalisedTransaction,
    TransactionVersion,
};

/// The limit, and where it came from.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedComputeLimit {
    /// Compute units the runtime charged against.
    pub effective_limit: u32,
    /// Which of the three places that limit came from.
    pub limit_source: ComputeLimitSource,
}

/// The limit the runtime derives when a transaction declares none: the units
/// reserved for each instruction, summed, capped at the per-transaction ceiling.
#[must_use]
pub fn runtime_default_compute_limit(transaction: &NormalisedTransaction) -> u32 {
    transaction
        .instructions
        .iter()
        .map(|instruction| u64::from(reserved_compute_units(&instruction.program_address)))
        .sum::<u64>()
        .min(u64::from(MAX_COMPUTE_UNIT_LIMIT)) as u32
}

/// Works out which limit applies, and says where it came from.
///
/// The order matters. On a version 1 transaction the header wins outright: any
/// `ComputeBudget` instruction present is inert, and reading it instead would
/// report the wrong limit — often zero, since a limit of 0 is a common no-op
/// payload.
#[must_use]
pub fn resolve_compute_limit(
    transaction: &NormalisedTransaction,
    directives: &ComputeBudgetDirectives,
) -> ResolvedComputeLimit {
    if transaction.version == TransactionVersion::V1 {
        return ResolvedComputeLimit {
            effective_limit: transaction
                .transaction_config
                .and_then(|config| config.compute_unit_limit)
                .unwrap_or(0),
            limit_source: ComputeLimitSource::V1Header,
        };
    }

    if let Some(declared) = directives.compute_unit_limit {
        return ResolvedComputeLimit {
            effective_limit: declared.min(MAX_COMPUTE_UNIT_LIMIT),
            limit_source: ComputeLimitSource::ComputeBudgetInstruction,
        };
    }

    ResolvedComputeLimit {
        effective_limit: runtime_default_compute_limit(transaction),
        limit_source: ComputeLimitSource::RuntimeDefault,
    }
}

/// Compute waste, and what an exactly-fitted limit would have saved in lamports.
///
/// The saving is zero on a version 1 transaction by construction: its priority
/// fee is an absolute lamport total, so declaring more compute than you use no
/// longer costs anything in fees. It still costs scheduling priority, because
/// the declared limit feeds the block's cost accounting.
#[must_use]
pub fn analyse_compute(
    transaction: &NormalisedTransaction,
    directives: &ComputeBudgetDirectives,
    limit: ResolvedComputeLimit,
) -> ComputeAnalysis {
    let effective = u64::from(limit.effective_limit);
    let consumed = transaction.compute_units_consumed;
    let wasted = consumed.map(|used| effective.saturating_sub(used));
    // The compute unit ceiling is 1,400,000, far inside f64's exact range, so the
    // conversion cannot lose a digit that matters for a displayed fraction.
    #[allow(clippy::cast_precision_loss)]
    let wasted_fraction = match (wasted, limit.effective_limit) {
        (Some(unused), limit) if limit > 0 => Some(unused as f64 / f64::from(limit)),
        _ => None,
    };

    let mut overpaid_lamports = 0;
    if transaction.version != TransactionVersion::V1 {
        if let (Some(price), Some(used)) = (directives.compute_unit_price_micro_lamports, consumed)
        {
            if price > 0 {
                overpaid_lamports =
                    charge_for(price, effective).saturating_sub(charge_for(price, used));
            }
        }
    }

    ComputeAnalysis {
        effective_limit: limit.effective_limit,
        limit_source: limit.limit_source,
        consumed,
        wasted,
        wasted_fraction,
        overpaid_lamports,
    }
}
