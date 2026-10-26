//! Tip detection against recorded transactions.

mod common;

use std::collections::HashMap;

use common::{analyse, load_transaction, options};
use fee_lens_core::{
    AnalyseOptions, TipBasis, TipVia, analyse_transaction, find_tips, tip_account_index,
};

#[test]
fn finds_a_top_level_tip_to_a_known_account() {
    let a = analyse("tx-v1-top-level-tip");

    assert_eq!(a.tips.len(), 1);
    let tip = &a.tips[0];
    assert_eq!(tip.lamports, 125_000);
    assert_eq!(tip.via, TipVia::TopLevel);
    assert_eq!(tip.basis, TipBasis::KnownRecipient);
    assert_eq!(tip.service.as_deref(), Some("Jito"));

    // A tip is not part of the protocol fee, so it must not inflate it.
    assert_eq!(a.fee.observed_fee_lamports, 5_000);
    assert_eq!(a.fee.total_cost_lamports, 130_000);
}

#[test]
fn finds_a_tip_paid_through_a_program() {
    let a = analyse("tx-cpi-tip");

    assert_eq!(a.tips.len(), 1);
    assert_eq!(a.tips[0].lamports, 2_635);
    assert_eq!(a.tips[0].via, TipVia::Inner);

    // A top-level-only scan would report this transaction as having tipped nothing.
    let mut transaction = load_transaction("tx-cpi-tip");
    transaction.inner_instructions.clear();
    assert!(find_tips(&transaction, &tip_account_index(&[]), false).is_empty());
}

#[test]
fn reports_payments_to_relays_that_publish_no_tip_account() {
    // A real transaction paying two third parties that are in no registry.
    // Without the unlabelled-outbound rule this reports "no tip paid" and
    // understates the cost of landing it by a factor of 21.
    let a = analyse("tx-v0-explicit-limit-with-alts");

    assert_eq!(a.tips.len(), 2);
    assert!(
        a.tips
            .iter()
            .all(|tip| tip.basis == TipBasis::UnlabelledOutbound)
    );
    assert!(a.tips.iter().all(|tip| tip.service.is_none()));
    assert_eq!(a.fee.tip_lamports, 2_328_594);
    assert_eq!(a.fee.labelled_tip_lamports, 0);

    // The fee was 116,703 lamports; landing it really cost 2,445,297.
    assert_eq!(a.fee.observed_fee_lamports, 116_703);
    assert_eq!(a.fee.total_cost_lamports, 2_445_297);
}

#[test]
fn can_be_restricted_to_recipients_in_the_registry() {
    let strict = AnalyseOptions {
        tip_accounts: tip_account_index(&[]),
        known_recipients_only: true,
    };
    let a = analyse_transaction(&load_transaction("tx-v0-explicit-limit-with-alts"), &strict);

    assert!(a.tips.is_empty());
    assert_eq!(a.fee.total_cost_lamports, 116_703);
}

#[test]
fn labels_a_caller_supplied_account() {
    let known = load_transaction("tx-v1-top-level-tip");
    let recipient = known.instructions.iter().find_map(|instruction| {
        fee_lens_core::decode_system_transfer(instruction).map(|transfer| transfer.destination)
    });
    let recipient = recipient.expect("a transfer");

    let labelled = tip_account_index(&[format!("{recipient}=MyRelay")]);
    let tips = find_tips(&known, &labelled, false);

    assert_eq!(tips[0].service.as_deref(), Some("MyRelay"));
    assert_eq!(tips[0].basis, TipBasis::KnownRecipient);
}

#[test]
fn an_empty_registry_still_finds_unlabelled_payments() {
    let transaction = load_transaction("tx-v0-explicit-limit-with-alts");
    let tips = find_tips(&transaction, &HashMap::new(), false);

    assert_eq!(tips.len(), 2);
    assert!(tips.iter().all(|tip| tip.service.is_none()));
}

#[test]
fn default_options_match_the_published_registry() {
    assert_eq!(
        options().tip_accounts.len(),
        fee_lens_core::KNOWN_TIP_ACCOUNTS.len()
    );
}

#[test]
fn reports_what_a_failed_transaction_instructed_but_not_as_paid() {
    // This transaction's instructions include a 20,356 lamport transfer, but it
    // failed, so the chain rolled that back and took only the fee.
    let a = analyse("tx-v1-inert-compute-budget");

    assert!(!a.succeeded);
    assert_eq!(a.tips.len(), 1);
    assert_eq!(a.fee.tip_lamports_instructed, 20_356);

    // What matters: none of it was paid, so it must not enter the cost to land.
    assert_eq!(a.fee.tip_lamports, 0);
    assert_eq!(a.fee.labelled_tip_lamports, 0);
    assert_eq!(a.fee.total_cost_lamports, a.fee.observed_fee_lamports);
    assert_eq!(a.fee.total_cost_lamports, 10_008);
}

#[test]
fn counts_instructed_and_paid_alike_when_the_transaction_succeeded() {
    let a = analyse("tx-v1-top-level-tip");

    assert!(a.succeeded);
    assert_eq!(a.fee.tip_lamports, 125_000);
    assert_eq!(a.fee.tip_lamports_instructed, 125_000);
}
