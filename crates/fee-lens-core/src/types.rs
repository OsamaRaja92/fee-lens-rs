//! Domain types for the fee model.
//!
//! Nothing here depends on an HTTP client. [`NormalisedTransaction`] is the single
//! input the whole model works from, which is what lets every analysis function be
//! pure and every test run offline against a recorded fixture.

use serde::Serialize;

/// Transaction versions currently live on mainnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionVersion {
    /// The original format, predating transaction versioning.
    Legacy,
    /// Versioned transaction, version 0: address lookup tables.
    V0,
    /// Versioned transaction, version 1: SIMD-0385, fee config in the header.
    V1,
}

impl std::fmt::Display for TransactionVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy => f.write_str("legacy"),
            Self::V0 => f.write_str("v0"),
            Self::V1 => f.write_str("v1"),
        }
    }
}

/// An instruction, with account references already resolved to addresses.
#[derive(Debug, Clone)]
pub struct NormalisedInstruction {
    /// The program being invoked.
    pub program_address: String,
    /// Accounts passed to the instruction, in order.
    pub account_addresses: Vec<String>,
    /// Raw instruction data.
    pub data: Vec<u8>,
    /// 1 for a top-level instruction, higher for one reached through CPI.
    pub stack_height: u32,
}

/// The compute-budget configuration carried in a version 1 transaction header.
///
/// Introduced by SIMD-0385. In a v1 transaction these values replace the
/// `ComputeBudget` instructions, and `priority_fee_lamports` is a **total in
/// lamports** rather than the per-compute-unit rate `SetComputeUnitPrice` sets.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct TransactionConfig {
    /// Compute units the transaction may use. `None` means it requested none,
    /// and a v1 transaction that requests none is budgeted zero.
    pub compute_unit_limit: Option<u32>,
    /// The whole priority fee, in LAMPORTS — not a per-compute-unit rate.
    /// This is the difference that breaks tooling written for v0.
    pub priority_fee_lamports: Option<u64>,
    /// Bytes of account data the transaction may load.
    pub loaded_accounts_data_size_limit: Option<u32>,
    /// Heap frame requested, in bytes. `None` leaves the 32 KiB default.
    pub heap_size_bytes: Option<u32>,
}

/// A transaction reduced to the fields the fee model needs.
#[derive(Debug, Clone)]
pub struct NormalisedTransaction {
    /// The first signature, which identifies the transaction.
    pub signature: String,
    /// The account that paid the fee: the first account key.
    pub fee_payer: String,
    /// Which of the three live formats this transaction uses; it decides how
    /// the priority fee is read.
    pub version: TransactionVersion,
    /// The slot it landed in.
    pub slot: u64,
    /// Signatures the message requires; the base fee is charged per required signature.
    pub num_required_signatures: u32,
    /// The fee actually charged, from transaction metadata. The ground truth.
    pub fee_lamports: u64,
    /// Compute units the runtime reported consuming, when the RPC supplies it.
    pub compute_units_consumed: Option<u64>,
    /// Execution error as reported, or `None` when the transaction succeeded.
    pub err: Option<String>,
    /// Top-level instructions.
    pub instructions: Vec<NormalisedInstruction>,
    /// Instructions reached through CPI, flattened. Used for tip detection.
    pub inner_instructions: Vec<NormalisedInstruction>,
    /// Present only on version 1 transactions.
    pub transaction_config: Option<TransactionConfig>,
    /// How many address lookup tables the message referenced. Always 0 for v1.
    /// Lookup tables the message referenced; always 0 on v1.
    pub address_table_lookup_count: usize,
}

/// A block reduced to the transactions the fee model can read.
#[derive(Debug, Clone)]
pub struct NormalisedBlock {
    /// The slot this block occupies.
    pub slot: u64,
    /// Unix seconds, as the node reported it.
    pub block_time: Option<i64>,
    /// Every transaction the block contains.
    pub transactions: Vec<NormalisedTransaction>,
}

/// Where the compute unit limit the runtime charged against came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComputeLimitSource {
    /// A `SetComputeUnitLimit` `ComputeBudget` instruction (legacy and v0).
    ComputeBudgetInstruction,
    /// The v1 transaction header config.
    V1Header,
    /// No limit was declared, so the runtime derived one from the instruction list.
    RuntimeDefault,
}

/// Where the priority fee came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PriorityFeeSource {
    /// `SetComputeUnitPrice` times the effective compute unit limit (legacy and v0).
    ComputeUnitPrice,
    /// An absolute lamport total in the v1 header.
    V1Header,
    /// No priority fee was paid.
    None,
}

/// Directives read out of a transaction's `ComputeBudget` instructions.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ComputeBudgetDirectives {
    /// Micro-lamports per compute unit, from `SetComputeUnitPrice`.
    pub compute_unit_price_micro_lamports: Option<u64>,
    /// Compute units requested, from `SetComputeUnitLimit`.
    pub compute_unit_limit: Option<u32>,
    /// Bytes requested, from `SetLoadedAccountsDataSizeLimit`.
    pub loaded_accounts_data_size_limit: Option<u32>,
    /// Bytes requested, from `RequestHeapFrame`.
    pub heap_frame_bytes: Option<u32>,
    /// How many `ComputeBudget` instructions the transaction carried.
    pub instruction_count: usize,
    /// True when these were found on a v1 transaction, where the runtime ignores
    /// them: they configure nothing and still occupy bytes and compute units.
    pub inert_on_v1: bool,
}

/// How a tip was identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TipBasis {
    /// The recipient is in the registry. Certain.
    KnownRecipient,
    /// Matched by shape rather than identity. A heuristic; see [`crate::tips`].
    UnlabelledOutbound,
}

/// Whether a transfer was a top-level instruction or reached through CPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TipVia {
    /// An instruction in the message itself.
    TopLevel,
    /// An instruction a program issued through CPI.
    Inner,
}

/// A payment made to get the transaction included.
#[derive(Debug, Clone, Serialize)]
pub struct TipTransfer {
    /// The account the lamports went to.
    pub recipient: String,
    /// Amount paid, in lamports.
    pub lamports: u64,
    /// The service, when the recipient is a known tip account; `None` when unlisted.
    pub service: Option<String>,
    /// Whether it was sent directly or through a program.
    pub via: TipVia,
    /// Whether the recipient was recognised or the payment matched by shape.
    pub basis: TipBasis,
}

/// What a transaction paid, and where each part came from.
#[derive(Debug, Clone, Serialize)]
pub struct FeeBreakdown {
    /// 5,000 lamports per charged signature.
    pub base_lamports: u64,
    /// Signatures the base fee was charged for: the message's required
    /// signatures plus any a precompile verified.
    pub signatures_charged: u32,
    /// How many of those came from a signature-verifying precompile.
    pub precompile_signatures: u32,
    /// The priority fee the model derives from the transaction's own directives.
    pub priority_lamports: u64,
    /// How the priority fee was expressed: a rate, an absolute total, or absent.
    pub priority_source: PriorityFeeSource,
    /// Payments for inclusion that actually moved, which is zero on a failed
    /// transaction: the chain rolls its instructions back and only the fee is
    /// taken. Not part of the protocol fee.
    pub tip_lamports: u64,
    /// What the transaction's transfers instructed, whether or not they
    /// executed. Equal to `tip_lamports` when the transaction succeeded; on a
    /// failed one this is what was offered and `tip_lamports` what was paid.
    pub tip_lamports_instructed: u64,
    /// The part of `tip_lamports` paid to a recipient in the registry.
    pub labelled_tip_lamports: u64,
    /// `base_lamports + priority_lamports`, the fee the model predicts.
    pub modelled_fee_lamports: u64,
    /// `fee` from transaction metadata. The ground truth.
    pub observed_fee_lamports: u64,
    /// `observed - modelled`. Zero means the model reproduced the charged fee
    /// exactly. A non-zero value is reported rather than hidden: it means the
    /// model is missing something about this transaction.
    pub model_residual_lamports: i128,
    /// The priority fee implied by the charged fee, independent of the model.
    pub implied_priority_lamports: u64,
    /// Total cost to land: protocol fee plus tips.
    pub total_cost_lamports: u64,
}

/// Compute budgeted against compute used.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ComputeAnalysis {
    /// The limit the runtime charged against.
    pub effective_limit: u32,
    /// Which of the three places the limit came from.
    pub limit_source: ComputeLimitSource,
    /// Compute units the runtime reported using, when it reported any.
    pub consumed: Option<u64>,
    /// Units budgeted but not used.
    pub wasted: Option<u64>,
    /// `wasted / effective_limit`, between 0 and 1.
    pub wasted_fraction: Option<f64>,
    /// Lamports an exactly-fitted compute unit limit would have saved.
    ///
    /// Always 0 for v1, where the priority fee is an absolute total and does not
    /// scale with the declared limit. Over-declaring on v1 costs scheduling
    /// priority rather than lamports.
    pub overpaid_lamports: u64,
}

/// The full analysis of one transaction.
#[derive(Debug, Clone, Serialize)]
pub struct TransactionAnalysis {
    /// The transaction this describes.
    pub signature: String,
    /// The format it used.
    pub version: TransactionVersion,
    /// The slot it landed in.
    pub slot: u64,
    /// False when execution failed — the fee was still charged.
    pub succeeded: bool,
    /// The error payload, rendered as JSON text.
    pub err: Option<String>,
    /// What it paid, and the model's self-check against that.
    pub fee: FeeBreakdown,
    /// What it budgeted against what it used.
    pub compute: ComputeAnalysis,
    /// What its `ComputeBudget` instructions asked for, if any.
    pub directives: ComputeBudgetDirectives,
    /// Payments for inclusion, which are not part of the fee.
    pub tips: Vec<TipTransfer>,
    /// Lookup tables the message referenced; always 0 on v1.
    pub address_table_lookup_count: usize,
}
