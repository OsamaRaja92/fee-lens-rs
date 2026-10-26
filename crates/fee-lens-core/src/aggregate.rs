//! Aggregation shared by the slot and account views.
//!
//! Kept separate from both so the two cannot drift apart: "what did this block
//! cost" and "what has this account been paying" are the same arithmetic over a
//! different set of transactions.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::builtins::MICRO_LAMPORTS_PER_LAMPORT;
use crate::types::{PriorityFeeSource, TransactionAnalysis};

/// Counts of transactions by how their priority fee was expressed.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct FeeSourceCounts {
    /// Paid an absolute total declared in a v1 header.
    pub v1_header: usize,
    /// Paid a rate per compute unit (legacy and v0).
    pub compute_unit_price: usize,
    /// Paid no priority fee at all.
    pub none: usize,
}

/// Nearest-rank percentiles over a set of values.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct PercentileTable {
    /// Median.
    pub p50: Option<u64>,
    /// 75th percentile.
    pub p75: Option<u64>,
    /// 90th percentile.
    pub p90: Option<u64>,
    /// 99th percentile.
    pub p99: Option<u64>,
    /// The largest value present.
    pub max: Option<u64>,
}

/// Nearest-rank percentile over a sorted ascending slice.
///
/// Nearest-rank rather than an interpolating definition because every value
/// returned is then a figure some transaction actually paid, which is what makes
/// the output usable as a reference point.
#[must_use]
pub fn percentile(sorted_ascending: &[u64], fraction: f64) -> Option<u64> {
    if sorted_ascending.is_empty() {
        return None;
    }
    if fraction <= 0.0 {
        return sorted_ascending.first().copied();
    }
    if fraction >= 1.0 {
        return sorted_ascending.last().copied();
    }
    let count = sorted_ascending.len();
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    let rank = (fraction * count as f64).ceil() as usize;
    sorted_ascending.get(rank.saturating_sub(1)).copied()
}

fn build_percentiles(values: &mut [u64]) -> PercentileTable {
    values.sort_unstable();
    PercentileTable {
        p50: percentile(values, 0.50),
        p75: percentile(values, 0.75),
        p90: percentile(values, 0.90),
        p99: percentile(values, 0.99),
        max: percentile(values, 1.0),
    }
}

/// Aggregate cost statistics over a set of transactions.
#[derive(Debug, Clone, Serialize)]
pub struct CostAggregate {
    /// How many transactions were aggregated.
    pub transaction_count: usize,
    /// How many executed without error.
    pub succeeded: usize,
    /// How many failed, having paid their fee anyway.
    pub failed: usize,
    /// Transactions per version, keyed by `legacy`, `v0`, `v1`.
    pub version_counts: BTreeMap<String, usize>,
    /// How the priority fees were expressed.
    pub fee_sources: FeeSourceCounts,

    /// Protocol fees charged, in lamports.
    pub total_fee_lamports: u64,
    /// The priority part of those fees.
    pub total_priority_lamports: u64,
    /// Payments for inclusion, which the protocol fee excludes.
    pub total_tip_lamports: u64,
    /// How many transactions paid one.
    pub tip_paying_transactions: usize,

    /// Compute units declared, summed.
    pub total_compute_budgeted: u64,
    /// Compute units actually used, summed.
    pub total_compute_consumed: u64,
    /// Budgeted but unused, as a fraction of what was budgeted.
    pub compute_wasted_fraction: Option<f64>,
    /// Lamports an exactly fitted compute unit limit would have saved, summed.
    pub total_overpaid_lamports: u64,

    /// Priority fee per transaction, over transactions that paid one.
    pub priority_lamports_percentiles: PercentileTable,
    /// Priority fee per compute unit consumed, in micro-lamports. More
    /// comparable across transactions than a per-transaction total, which
    /// scales with size.
    pub priority_per_compute_unit_percentiles: PercentileTable,

    /// How many transactions the fee model failed to reproduce exactly.
    ///
    /// A self-test that runs on every real transaction the tool reads. It should
    /// be zero; if it is not, the model is out of date with the runtime.
    pub model_mismatches: usize,
}

/// Micro-lamports of priority fee per compute unit consumed, if derivable.
fn price_per_compute_unit(analysis: &TransactionAnalysis) -> Option<u64> {
    let consumed = analysis.compute.consumed?;
    if analysis.fee.priority_lamports == 0 || consumed == 0 {
        return None;
    }
    let scaled = u128::from(analysis.fee.priority_lamports) * MICRO_LAMPORTS_PER_LAMPORT;
    u64::try_from(scaled / u128::from(consumed)).ok()
}

/// Reduces a set of analyses to one set of totals.
#[must_use]
pub fn aggregate(analyses: &[TransactionAnalysis]) -> CostAggregate {
    let mut version_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut fee_sources = FeeSourceCounts::default();
    let mut succeeded = 0;
    let mut total_fee_lamports = 0u64;
    let mut total_priority_lamports = 0u64;
    let mut total_tip_lamports = 0u64;
    let mut tip_paying_transactions = 0;
    let mut total_compute_budgeted = 0u64;
    let mut total_compute_consumed = 0u64;
    let mut total_overpaid_lamports = 0u64;
    let mut model_mismatches = 0;
    let mut priority_values = Vec::new();
    let mut per_unit_values = Vec::new();

    for analysis in analyses {
        *version_counts
            .entry(analysis.version.to_string())
            .or_insert(0) += 1;

        if analysis.succeeded {
            succeeded += 1;
        }
        if analysis.fee.model_residual_lamports != 0 {
            model_mismatches += 1;
        }

        match analysis.fee.priority_source {
            PriorityFeeSource::V1Header => fee_sources.v1_header += 1,
            PriorityFeeSource::ComputeUnitPrice => fee_sources.compute_unit_price += 1,
            PriorityFeeSource::None => fee_sources.none += 1,
        }

        total_fee_lamports = total_fee_lamports.saturating_add(analysis.fee.observed_fee_lamports);
        total_priority_lamports =
            total_priority_lamports.saturating_add(analysis.fee.priority_lamports);
        total_tip_lamports = total_tip_lamports.saturating_add(analysis.fee.tip_lamports);
        total_overpaid_lamports =
            total_overpaid_lamports.saturating_add(analysis.compute.overpaid_lamports);
        if analysis.fee.tip_lamports > 0 {
            tip_paying_transactions += 1;
        }

        total_compute_budgeted =
            total_compute_budgeted.saturating_add(u64::from(analysis.compute.effective_limit));
        total_compute_consumed =
            total_compute_consumed.saturating_add(analysis.compute.consumed.unwrap_or(0));

        if analysis.fee.priority_lamports > 0 {
            priority_values.push(analysis.fee.priority_lamports);
            if let Some(per_unit) = price_per_compute_unit(analysis) {
                per_unit_values.push(per_unit);
            }
        }
    }

    #[allow(clippy::cast_precision_loss)]
    let compute_wasted_fraction = (total_compute_budgeted > 0).then(|| {
        (total_compute_budgeted - total_compute_consumed.min(total_compute_budgeted)) as f64
            / total_compute_budgeted as f64
    });

    CostAggregate {
        transaction_count: analyses.len(),
        succeeded,
        failed: analyses.len() - succeeded,
        version_counts,
        fee_sources,
        total_fee_lamports,
        total_priority_lamports,
        total_tip_lamports,
        tip_paying_transactions,
        total_compute_budgeted,
        total_compute_consumed,
        compute_wasted_fraction,
        total_overpaid_lamports,
        priority_lamports_percentiles: build_percentiles(&mut priority_values),
        priority_per_compute_unit_percentiles: build_percentiles(&mut per_unit_values),
        model_mismatches,
    }
}

#[cfg(test)]
mod tests {
    use super::percentile;

    #[test]
    fn uses_nearest_rank_so_every_result_is_real_data() {
        let values: Vec<u64> = (1..=10).collect();
        assert_eq!(percentile(&values, 0.50), Some(5));
        assert_eq!(percentile(&values, 0.90), Some(9));
        assert_eq!(percentile(&values, 1.0), Some(10));
    }

    #[test]
    fn clamps_out_of_range_fractions() {
        let values: Vec<u64> = (1..=10).collect();
        assert_eq!(percentile(&values, 0.0), Some(1));
        assert_eq!(percentile(&values, -1.0), Some(1));
        assert_eq!(percentile(&values, 2.0), Some(10));
    }

    #[test]
    fn handles_empty_and_single_element_input() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[42], 0.5), Some(42));
    }
}
