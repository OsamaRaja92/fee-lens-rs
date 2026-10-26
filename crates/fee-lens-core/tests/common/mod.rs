//! Fixture loading shared by the integration tests.
//!
//! Each file under `fixtures/` is a verbatim JSON-RPC response recorded from
//! mainnet: `tx-*.json` from `getTransaction`, `block-*.json` from `getBlock`
//! with the fields this tool never reads removed. They go through exactly the
//! same normalisation as a live response, so the tests exercise the real path
//! and need neither a network connection nor an RPC key.

// Each integration test binary compiles this module and uses a subset of it, so
// what one binary does not call looks dead from inside that binary.
#![allow(dead_code)]

use std::path::PathBuf;

use fee_lens_core::wire::{WireBlock, WireTransaction};
use fee_lens_core::{
    AnalyseOptions, NormalisedBlock, NormalisedTransaction, TransactionAnalysis,
    analyse_transaction, normalise_block, normalise_transaction, tip_account_index,
};

/// The recorded block, and the slot it came from.
pub const RECORDED_BLOCK: (&str, u64) = ("block-461358125", 461_358_125);

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(format!("{name}.json"))
}

fn read(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|error| panic!("reading fixture {name}: {error}"))
}

/// Default options: the published registry, heuristic enabled.
pub fn options() -> AnalyseOptions {
    AnalyseOptions {
        tip_accounts: tip_account_index(&[]),
        known_recipients_only: false,
    }
}

pub fn load_transaction(name: &str) -> NormalisedTransaction {
    let wire: WireTransaction = serde_json::from_str(&read(name)).expect("parsing fixture");
    normalise_transaction(&wire, None).expect("normalising fixture")
}

pub fn load_block() -> NormalisedBlock {
    let wire: WireBlock = serde_json::from_str(&read(RECORDED_BLOCK.0)).expect("parsing block");
    normalise_block(&wire, RECORDED_BLOCK.1).expect("normalising block")
}

/// Analyses a recorded transaction with the default options.
pub fn analyse(name: &str) -> TransactionAnalysis {
    analyse_transaction(&load_transaction(name), &options())
}
