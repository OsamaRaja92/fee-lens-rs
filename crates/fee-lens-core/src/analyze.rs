//! Transaction, slot and account analyses.
//!
//! Pure: each takes normalised input and returns a result. No network, no clock,
//! no configuration beyond the tip registry, which makes every case in `tests/`
//! a plain input-output assertion.

use std::collections::HashMap;

use serde::Serialize;

use crate::aggregate::{CostAggregate, aggregate};
use crate::budget::decode_compute_budget;
use crate::compute::{analyse_compute, resolve_compute_limit};
use crate::fee::{build_fee_breakdown, priority_fee};
use crate::tips::find_tips;
use crate::types::{
    NormalisedBlock, NormalisedTransaction, TransactionAnalysis, TransactionVersion,
};

/// How to analyse: which tip accounts to recognise, and how strictly.
#[derive(Debug, Clone, Default)]
pub struct AnalyseOptions {
    /// Tip accounts to recognise. Empty means the published registry only.
    pub tip_accounts: HashMap<String, String>,
    /// Report only payments to recipients in the registry, suppressing the
    /// unlabelled-outbound heuristic. Off by default, because most relays do not
    /// publish their tip accounts and suppressing them under-reports what was paid.
    pub known_recipients_only: bool,
}

/// Analyses one transaction.
#[must_use]
pub fn analyse_transaction(
    transaction: &NormalisedTransaction,
    options: &AnalyseOptions,
) -> TransactionAnalysis {
    let directives = decode_compute_budget(
        &transaction.instructions,
        transaction.version == TransactionVersion::V1,
    );
    let limit = resolve_compute_limit(transaction, &directives);
    let compute = analyse_compute(transaction, &directives, limit);
    let tips = find_tips(
        transaction,
        &options.tip_accounts,
        options.known_recipients_only,
    );
    let fee = build_fee_breakdown(
        transaction,
        priority_fee(transaction, &directives, limit),
        &tips,
    );

    TransactionAnalysis {
        signature: transaction.signature.clone(),
        version: transaction.version,
        slot: transaction.slot,
        succeeded: transaction.err.is_none(),
        err: transaction.err.clone(),
        fee,
        compute,
        directives,
        tips,
        address_table_lookup_count: transaction.address_table_lookup_count,
    }
}

/// Slot-level view: what it cost to be in this block.
///
/// A caveat worth stating before the numbers are used for anything. This
/// measures the transactions that LANDED. It cannot see the ones that were
/// dropped, so the distribution says what was sufficient, not what was
/// necessary. Reading a percentile as "the bid required to land" treats a sample
/// of winners as if it described the whole field.
#[derive(Debug, Clone, Serialize)]
pub struct SlotAnalysis {
    /// The slot analysed.
    pub slot: u64,
    /// Unix seconds, as the node reported it.
    pub block_time: Option<i64>,
    /// Totals over the block.
    #[serde(flatten)]
    pub totals: CostAggregate,
    /// Every transaction, analysed.
    pub transactions: Vec<TransactionAnalysis>,
}

/// Analyses every transaction in a block.
#[must_use]
pub fn analyse_slot(block: &NormalisedBlock, options: &AnalyseOptions) -> SlotAnalysis {
    let transactions: Vec<_> = block
        .transactions
        .iter()
        .map(|transaction| analyse_transaction(transaction, options))
        .collect();

    SlotAnalysis {
        slot: block.slot,
        block_time: block.block_time,
        totals: aggregate(&transactions),
        transactions,
    }
}

/// Account-level view: what one address has been paying to get its transactions in.
#[derive(Debug, Clone, Serialize)]
pub struct AccountAnalysis {
    /// The account these transactions belong to.
    pub address: String,
    /// Signatures inspected, newest first.
    pub signatures_inspected: usize,
    /// Totals over the inspected transactions.
    #[serde(flatten)]
    pub totals: CostAggregate,
    /// Each inspected transaction, analysed.
    pub transactions: Vec<TransactionAnalysis>,
}

/// Analyses a set of transactions belonging to one account.
#[must_use]
pub fn analyse_account(
    address: &str,
    signatures_inspected: usize,
    transactions: &[NormalisedTransaction],
    options: &AnalyseOptions,
) -> AccountAnalysis {
    let analysed: Vec<_> = transactions
        .iter()
        .map(|transaction| analyse_transaction(transaction, options))
        .collect();

    AccountAnalysis {
        address: address.to_owned(),
        signatures_inspected,
        totals: aggregate(&analysed),
        transactions: analysed,
    }
}
