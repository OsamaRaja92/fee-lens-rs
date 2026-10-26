//! Fee model, case by case, against transactions recorded from mainnet.
//!
//! Every expected number here was charged on chain, and every one matches the
//! TypeScript implementation's assertions for the same fixtures. The figure that
//! matters in each case is the model residual: the model predicts the fee from
//! the transaction's own directives, the metadata says what was actually taken,
//! and a residual of zero means the two agree exactly.

mod common;

use common::analyse;
use fee_lens_core::{ComputeLimitSource, PriorityFeeSource};

#[test]
fn legacy_charges_price_times_explicit_limit() {
    let a = analyse("tx-legacy-explicit-limit");

    assert_eq!(a.directives.compute_unit_price_micro_lamports, Some(57_000));
    assert_eq!(a.directives.compute_unit_limit, Some(600));
    assert_eq!(
        a.compute.limit_source,
        ComputeLimitSource::ComputeBudgetInstruction
    );

    // ceil(57,000 micro-lamports x 600 units / 1e6) = 35 lamports.
    assert_eq!(a.fee.priority_lamports, 35);
    assert_eq!(a.fee.priority_source, PriorityFeeSource::ComputeUnitPrice);
    // Two required signatures, so the base fee is doubled.
    assert_eq!(a.fee.base_lamports, 10_000);
    assert_eq!(a.fee.observed_fee_lamports, 10_035);
    assert_eq!(a.fee.model_residual_lamports, 0);
}

#[test]
fn resolves_account_indices_through_lookup_tables() {
    let a = analyse("tx-v0-explicit-limit-with-alts");

    assert_eq!(a.address_table_lookup_count, 3);
    assert_eq!(a.compute.effective_limit, 707_059);
    assert_eq!(a.fee.priority_lamports, 111_703);
    assert_eq!(a.fee.model_residual_lamports, 0);
}

#[test]
fn derives_the_limit_when_none_is_declared() {
    let a = analyse("tx-v0-runtime-default-limit");

    assert_eq!(a.directives.compute_unit_limit, None);
    assert_eq!(a.compute.limit_source, ComputeLimitSource::RuntimeDefault);
    // Five non-builtin instructions at 200,000, plus a System instruction at
    // 3,000, plus a ComputeBudget instruction at 3,000.
    assert_eq!(a.compute.effective_limit, 1_006_000);
    assert_eq!(a.fee.priority_lamports, 10_161);
    assert_eq!(a.fee.model_residual_lamports, 0);
}

#[test]
fn derives_a_smaller_default_for_a_shorter_instruction_list() {
    let a = analyse("tx-legacy-runtime-default-limit");

    // Two non-builtin instructions at 200,000 plus ComputeBudget at 3,000.
    assert_eq!(a.compute.effective_limit, 403_000);
    assert_eq!(a.fee.priority_lamports, 4_030);
    assert_eq!(a.fee.model_residual_lamports, 0);
}

#[test]
fn reserves_three_thousand_units_for_a_builtin() {
    let a = analyse("tx-no-priority-fee");

    // A single System Program instruction and no declared limit.
    assert_eq!(a.compute.effective_limit, 3_000);
    assert_eq!(a.compute.consumed, Some(150));
    assert_eq!(a.fee.priority_source, PriorityFeeSource::None);
    assert_eq!(a.fee.priority_lamports, 0);
    assert_eq!(a.fee.model_residual_lamports, 0);
}

#[test]
fn v1_reads_an_absolute_total_from_the_header() {
    let a = analyse("tx-v1-header-priority-fee");

    assert_eq!(a.fee.priority_source, PriorityFeeSource::V1Header);
    assert_eq!(a.fee.priority_lamports, 1_000);
    assert_eq!(a.fee.observed_fee_lamports, 6_000);
    assert_eq!(a.fee.model_residual_lamports, 0);

    // The limit is large and the consumption tiny, yet the fee is unaffected: on
    // v1 the priority fee does not scale with the declared limit. Reading 1,000
    // as a per-unit rate would predict 48 lamports instead of 1,000.
    assert_eq!(a.compute.effective_limit, 48_484);
    assert_eq!(a.compute.consumed, Some(287));
    assert_eq!(a.compute.overpaid_lamports, 0);
}

#[test]
fn v1_takes_the_limit_from_the_header_not_an_inert_instruction() {
    let a = analyse("tx-v1-inert-compute-budget");

    // The trap: the message carries a ComputeBudget instruction declaring a
    // limit of 0, while the limit the runtime charged against is in the header.
    assert_eq!(a.directives.instruction_count, 1);
    assert_eq!(a.directives.compute_unit_limit, Some(0));
    assert!(a.directives.inert_on_v1);

    assert_eq!(a.compute.limit_source, ComputeLimitSource::V1Header);
    assert_eq!(a.compute.effective_limit, 327_450);
    assert_eq!(a.compute.consumed, Some(121_199));
    assert_eq!(a.fee.priority_lamports, 5_008);
    assert_eq!(a.fee.model_residual_lamports, 0);
}

#[test]
fn a_failed_transaction_still_paid() {
    let a = analyse("tx-v1-header-priority-fee");

    assert!(!a.succeeded);
    assert!(a.err.is_some());
    assert!(a.fee.observed_fee_lamports > 0);
}

#[test]
fn prices_the_waste_on_an_over_declared_transaction() {
    let a = analyse("tx-v0-runtime-default-limit");

    assert_eq!(a.compute.consumed, Some(40_450));
    assert_eq!(a.compute.wasted, Some(965_550));
    let fraction = a.compute.wasted_fraction.expect("a fraction");
    assert!((fraction - 0.9598).abs() < 0.001, "fraction was {fraction}");
    // ceil(10,100 x 1,006,000 / 1e6) - ceil(10,100 x 40,450 / 1e6).
    assert_eq!(a.compute.overpaid_lamports, 9_752);
}

#[test]
fn finds_nothing_to_recover_on_an_exact_fit() {
    let a = analyse("tx-legacy-explicit-limit");

    assert_eq!(a.compute.effective_limit, 600);
    assert_eq!(a.compute.consumed, Some(600));
    assert_eq!(a.compute.wasted, Some(0));
    assert_eq!(a.compute.overpaid_lamports, 0);
}

#[test]
fn charges_the_base_fee_for_precompile_signatures() {
    let a = analyse("tx-ed25519-precompile");

    // One required signature, one Ed25519 signature verified by the precompile.
    // Counting only the message header would predict 5,000 and be out by 5,000.
    assert_eq!(a.fee.precompile_signatures, 1);
    assert_eq!(a.fee.signatures_charged, 2);
    assert_eq!(a.fee.base_lamports, 10_000);
    assert_eq!(a.fee.priority_lamports, 2_788);
    assert_eq!(a.fee.observed_fee_lamports, 12_788);
    assert_eq!(a.fee.model_residual_lamports, 0);
}

#[test]
fn counts_no_precompile_signatures_on_an_ordinary_transaction() {
    let a = analyse("tx-v0-explicit-limit-with-alts");

    assert_eq!(a.fee.precompile_signatures, 0);
    assert_eq!(a.fee.signatures_charged, 1);
    assert_eq!(a.fee.model_residual_lamports, 0);
}
