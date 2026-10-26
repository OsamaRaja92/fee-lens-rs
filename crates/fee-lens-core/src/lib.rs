//! # fee-lens-core
//!
//! Works out what a Solana transaction actually paid to land, and gets it right
//! across legacy, v0 and v1 transactions.
//!
//! The fee has two parts: a base fee of 5,000 lamports per required signature,
//! and a priority fee whose expression depends on the version. On legacy and v0
//! a `ComputeBudget` instruction sets a *rate* per compute unit and the fee is
//! that rate times the declared limit. On v1 the message header carries an
//! *absolute lamport total* (SIMD-0385) and the compute unit limit does not
//! enter the fee at all. Tips are neither: they are plain SOL transfers that
//! never appear in the charged fee.
//!
//! Every analysis function is pure. Feed it a [`types::NormalisedTransaction`]
//! and it returns a result with no network access involved, which is how this
//! crate is tested against recorded mainnet blocks.
//!
//! The model checks itself. [`types::FeeBreakdown::model_residual_lamports`] is
//! the charged fee minus the modelled fee: two independently derived numbers
//! whose difference should always be zero.
//!
//! This is the Rust twin of the TypeScript implementation at
//! <https://github.com/OsamaRaja92/solana-fee-lens>. Both reproduce the same
//! recorded blocks to the lamport.

pub mod aggregate;
pub mod analyze;
pub mod budget;
pub mod builtins;
pub mod compute;
pub mod fee;
pub mod precompiles;
pub mod registry;
pub mod tips;
pub mod types;
pub mod wire;

pub use aggregate::{CostAggregate, FeeSourceCounts, PercentileTable, aggregate, percentile};
pub use analyze::{
    AccountAnalysis, AnalyseOptions, SlotAnalysis, analyse_account, analyse_slot,
    analyse_transaction,
};
pub use budget::decode_compute_budget;
pub use compute::{
    ResolvedComputeLimit, analyse_compute, resolve_compute_limit, runtime_default_compute_limit,
};
pub use fee::{PriorityFee, base_fee, build_fee_breakdown, charge_for, priority_fee};
pub use precompiles::{
    ED25519_PROGRAM, SECP256K1_PROGRAM, SECP256R1_PROGRAM, SIGNATURE_PRECOMPILES,
    count_precompile_signatures,
};
pub use registry::{KNOWN_TIP_ACCOUNTS, tip_account_index};
pub use tips::{decode_system_transfer, find_tips};
pub use types::*;
pub use wire::{WireError, normalise_block, normalise_transaction};
