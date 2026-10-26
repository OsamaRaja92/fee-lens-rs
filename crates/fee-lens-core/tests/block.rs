//! The whole-block test, and the cross-implementation check.
//!
//! This is the strongest claim the project makes: for every transaction in a
//! real mainnet block, the fee predicted from the transaction's own directives
//! equals the fee the chain charged, to the lamport. The block mixes all three
//! live transaction versions and every way of expressing a priority fee, so it
//! covers the model's branches with real data rather than with cases chosen to
//! pass.
//!
//! The totals asserted below were produced by the independent TypeScript
//! implementation at <https://github.com/OsamaRaja92/solana-fee-lens> over the
//! same fixture. Two implementations, written against the same specification in
//! different languages, agreeing on 574 real transactions is a much stronger
//! statement than either makes alone — and if one drifts, this test says so.

mod common;

use common::{RECORDED_BLOCK, load_block, options};
use fee_lens_core::analyse_slot;

#[test]
fn reproduces_the_charged_fee_for_every_transaction() {
    let analysis = analyse_slot(&load_block(), &options());

    let off_by: Vec<_> = analysis
        .transactions
        .iter()
        .filter(|tx| tx.fee.model_residual_lamports != 0)
        .map(|tx| {
            (
                tx.signature.clone(),
                tx.version,
                tx.fee.model_residual_lamports,
            )
        })
        .collect();

    assert!(
        off_by.is_empty(),
        "transactions the model could not reproduce: {off_by:#?}"
    );
    assert_eq!(analysis.totals.model_mismatches, 0);
    assert_eq!(analysis.totals.transaction_count, 574);
    assert_eq!(analysis.slot, RECORDED_BLOCK.1);
}

#[test]
fn agrees_with_the_typescript_implementation() {
    let a = analyse_slot(&load_block(), &options()).totals;

    assert_eq!(a.transaction_count, 574);
    assert_eq!(a.succeeded, 431);
    assert_eq!(a.failed, 143);

    assert_eq!(a.version_counts.get("legacy"), Some(&104));
    assert_eq!(a.version_counts.get("v0"), Some(&190));
    assert_eq!(a.version_counts.get("v1"), Some(&280));

    assert_eq!(a.fee_sources.v1_header, 241);
    assert_eq!(a.fee_sources.compute_unit_price, 177);
    assert_eq!(a.fee_sources.none, 156);

    assert_eq!(a.total_fee_lamports, 8_935_933);
    assert_eq!(a.total_priority_lamports, 5_960_933);
    assert_eq!(a.total_tip_lamports, 13_842_684);
    assert_eq!(a.tip_paying_transactions, 86);

    assert_eq!(a.total_compute_budgeted, 74_154_954);
    assert_eq!(a.total_compute_consumed, 19_825_530);
    assert_eq!(a.total_overpaid_lamports, 790_028);

    assert_eq!(a.priority_lamports_percentiles.p50, Some(324));
    assert_eq!(a.priority_lamports_percentiles.p90, Some(10_161));
    assert_eq!(a.priority_lamports_percentiles.max, Some(1_000_000));
    assert_eq!(a.priority_per_compute_unit_percentiles.p50, Some(98_119));
    assert_eq!(
        a.priority_per_compute_unit_percentiles.max,
        Some(162_613_718)
    );
}

#[test]
fn totals_are_the_sum_of_the_parts() {
    let analysis = analyse_slot(&load_block(), &options());
    let summed: u64 = analysis
        .transactions
        .iter()
        .map(|tx| tx.fee.observed_fee_lamports)
        .sum();

    assert_eq!(analysis.totals.total_fee_lamports, summed);
    assert_eq!(
        analysis.totals.succeeded + analysis.totals.failed,
        analysis.totals.transaction_count
    );
}

#[test]
fn counts_only_the_tips_a_successful_transaction_actually_paid() {
    let analysis = analyse_slot(&load_block(), &options());

    // A failed transaction's transfers are rolled back, so the tips it
    // instructed never move. Counting them inflated this block's tips from
    // 13,842,684 to 67,345,751 lamports, nearly five times over.
    assert_eq!(analysis.totals.total_tip_lamports, 13_842_684);
    assert_eq!(analysis.totals.tip_paying_transactions, 86);

    let instructed_on_failures: u64 = analysis
        .transactions
        .iter()
        .filter(|tx| !tx.succeeded)
        .map(|tx| tx.fee.tip_lamports_instructed)
        .sum();
    assert_eq!(instructed_on_failures, 53_503_067);

    for tx in analysis.transactions.iter().filter(|tx| !tx.succeeded) {
        assert_eq!(tx.fee.tip_lamports, 0, "{}", tx.signature);
        assert_eq!(
            tx.fee.total_cost_lamports, tx.fee.observed_fee_lamports,
            "{}",
            tx.signature
        );
    }

    // Every tip-paying transaction must still reproduce exactly: a tip is a
    // transfer, so it must never appear in the charged fee.
    for tx in analysis
        .transactions
        .iter()
        .filter(|tx| tx.fee.tip_lamports > 0)
    {
        assert_eq!(tx.fee.model_residual_lamports, 0, "{}", tx.signature);
    }
}

#[test]
fn measures_compute_budgeted_but_never_used() {
    let a = analyse_slot(&load_block(), &options()).totals;

    assert!(a.total_compute_budgeted > a.total_compute_consumed);
    let fraction = a.compute_wasted_fraction.expect("a fraction");
    assert!(fraction > 0.0 && fraction < 1.0, "fraction was {fraction}");
}
