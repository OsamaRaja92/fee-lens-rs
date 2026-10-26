//! The RPC boundary: the shape of the JSON responses this crate reads, and the
//! conversion into the model's input.
//!
//! Declared here rather than taken from a client library for two reasons. It is
//! the one place untyped wire data becomes typed domain data, so the conversion
//! is visible and testable; and it lets a recorded JSON fixture travel through
//! exactly the same path as a live response, which is what makes the offline
//! tests meaningful.
//!
//! Two things here matter for correctness.
//!
//! Account indices are resolved against the FULL key list: the message's static
//! keys, then the writable addresses loaded from lookup tables, then the
//! readonly ones, in that order. An index past the static keys is an address the
//! runtime loaded from a lookup table, and resolving against the static keys
//! alone simply yields nothing for it. Version 1 transactions inline their
//! accounts and never need this.
//!
//! Instruction data is base58-decoded here, so everything downstream works on
//! bytes and never has to know how the wire encoded them.

use serde::Deserialize;

use crate::types::{
    NormalisedBlock, NormalisedInstruction, NormalisedTransaction, TransactionConfig,
    TransactionVersion,
};

/// Why a wire response could not be turned into model input.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error(
        "unsupported transaction version {0}: this build models legacy, v0 and v1 only, and the \
         fee rules for newer versions may differ"
    )]
    /// A version this build does not model. Failing beats pricing it by the
    /// wrong rules, which is exactly how v1 broke older tooling.
    UnsupportedVersion(u64),
    #[error("transaction has no metadata, so the fee it paid is unknown")]
    /// Without metadata there is no charged fee to compare the model against.
    MissingMeta,
    #[error("transaction carries no signature")]
    /// A transaction with no signature cannot be identified.
    MissingSignature,
    #[error("transaction has no account keys")]
    /// No account keys means no fee payer, so no tip can be attributed.
    MissingAccountKeys,
    #[error("transaction has no slot, and no fallback was supplied")]
    /// Read from a block, a transaction inherits the block's slot; alone it
    /// must carry its own.
    MissingSlot,
    #[error("instruction references account index {index}, but only {resolved} were resolved")]
    /// An index past every resolved key, which usually means lookup-table
    /// addresses were not merged into the key list.
    AccountIndexOutOfRange {
        /// The index the instruction asked for.
        index: usize,
        /// How many keys were actually resolved.
        resolved: usize,
    },
    #[error("instruction data was not valid base58: {0}")]
    /// Instruction data that is not base58, so the directives cannot be read.
    BadBase58(#[from] bs58::decode::Error),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WireVersion {
    Named(String),
    Numbered(u64),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireInstruction {
    program_id_index: usize,
    accounts: Vec<usize>,
    /// Base58-encoded instruction data.
    data: String,
}

#[derive(Debug, Deserialize)]
struct WireInnerGroup {
    instructions: Vec<WireInstruction>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireHeader {
    num_required_signatures: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireTransactionConfig {
    #[serde(default)]
    compute_unit_limit: Option<u32>,
    #[serde(default)]
    priority_fee: Option<u64>,
    #[serde(default)]
    loaded_accounts_data_size_limit: Option<u32>,
    #[serde(default)]
    heap_size: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireMessage {
    account_keys: Vec<String>,
    header: WireHeader,
    instructions: Vec<WireInstruction>,
    #[serde(default)]
    address_table_lookups: Option<Vec<serde_json::Value>>,
    /// Version 1 only.
    #[serde(default)]
    transaction_config: Option<WireTransactionConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct WireLoadedAddresses {
    #[serde(default)]
    writable: Vec<String>,
    #[serde(default)]
    readonly: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireMeta {
    fee: u64,
    #[serde(default)]
    err: Option<serde_json::Value>,
    #[serde(default)]
    compute_units_consumed: Option<u64>,
    #[serde(default)]
    inner_instructions: Option<Vec<WireInnerGroup>>,
    #[serde(default)]
    loaded_addresses: Option<WireLoadedAddresses>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEnvelope {
    message: WireMessage,
    signatures: Vec<String>,
}

/// A `getTransaction` response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireTransaction {
    transaction: WireEnvelope,
    meta: Option<WireMeta>,
    #[serde(default)]
    version: Option<WireVersion>,
    #[serde(default)]
    slot: Option<u64>,
    #[serde(default)]
    block_time: Option<i64>,
}

/// A `getBlock` response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireBlock {
    /// Unix seconds, as the node reported it.
    #[serde(default)]
    pub block_time: Option<i64>,
    /// The block's transactions, in order.
    pub transactions: Vec<WireTransaction>,
}

fn to_version(version: Option<&WireVersion>) -> Result<TransactionVersion, WireError> {
    match version {
        None => Ok(TransactionVersion::Legacy),
        Some(WireVersion::Named(name)) if name == "legacy" => Ok(TransactionVersion::Legacy),
        Some(WireVersion::Named(name)) => Err(WireError::UnsupportedVersion(
            name.parse().unwrap_or(u64::MAX),
        )),
        Some(WireVersion::Numbered(0)) => Ok(TransactionVersion::V0),
        Some(WireVersion::Numbered(1)) => Ok(TransactionVersion::V1),
        Some(WireVersion::Numbered(other)) => Err(WireError::UnsupportedVersion(*other)),
    }
}

/// Static keys, then lookup-table writables, then lookup-table readonlys.
fn resolve_account_keys(wire: &WireTransaction) -> Vec<String> {
    let mut keys = wire.transaction.message.account_keys.clone();
    if let Some(loaded) = wire
        .meta
        .as_ref()
        .and_then(|meta| meta.loaded_addresses.as_ref())
    {
        keys.extend(loaded.writable.iter().cloned());
        keys.extend(loaded.readonly.iter().cloned());
    }
    keys
}

fn normalise_instruction(
    instruction: &WireInstruction,
    keys: &[String],
    stack_height: u32,
) -> Result<NormalisedInstruction, WireError> {
    let program_address = keys
        .get(instruction.program_id_index)
        .ok_or(WireError::AccountIndexOutOfRange {
            index: instruction.program_id_index,
            resolved: keys.len(),
        })?
        .clone();

    // An index with no resolved address is dropped rather than failing the whole
    // transaction: it cannot be a program, and a missing non-program account
    // never changes a fee.
    let account_addresses = instruction
        .accounts
        .iter()
        .filter_map(|index| keys.get(*index).cloned())
        .collect();

    Ok(NormalisedInstruction {
        program_address,
        account_addresses,
        data: bs58::decode(&instruction.data).into_vec()?,
        stack_height,
    })
}

/// Converts a `getTransaction` response into model input.
///
/// `fallback_slot` supplies the slot for a transaction read out of a block,
/// where the per-transaction entry carries none.
///
/// # Errors
///
/// Returns [`WireError`] if the transaction uses a version this build does not
/// model, carries no metadata, signature, account keys or slot, references an
/// account index past every resolved key, or holds instruction data that is not
/// valid base58.
pub fn normalise_transaction(
    wire: &WireTransaction,
    fallback_slot: Option<u64>,
) -> Result<NormalisedTransaction, WireError> {
    let meta = wire.meta.as_ref().ok_or(WireError::MissingMeta)?;
    let keys = resolve_account_keys(wire);

    let signature = wire
        .transaction
        .signatures
        .first()
        .ok_or(WireError::MissingSignature)?
        .clone();
    let fee_payer = keys.first().ok_or(WireError::MissingAccountKeys)?.clone();
    let slot = wire.slot.or(fallback_slot).ok_or(WireError::MissingSlot)?;

    let instructions = wire
        .transaction
        .message
        .instructions
        .iter()
        .map(|instruction| normalise_instruction(instruction, &keys, 1))
        .collect::<Result<Vec<_>, _>>()?;

    let mut inner_instructions = Vec::new();
    for group in meta.inner_instructions.iter().flatten() {
        for instruction in &group.instructions {
            inner_instructions.push(normalise_instruction(instruction, &keys, 2)?);
        }
    }

    let transaction_config = wire
        .transaction
        .message
        .transaction_config
        .as_ref()
        .map(|config| TransactionConfig {
            compute_unit_limit: config.compute_unit_limit,
            priority_fee_lamports: config.priority_fee,
            loaded_accounts_data_size_limit: config.loaded_accounts_data_size_limit,
            heap_size_bytes: config.heap_size,
        });

    Ok(NormalisedTransaction {
        signature,
        fee_payer,
        version: to_version(wire.version.as_ref())?,
        slot,
        num_required_signatures: wire.transaction.message.header.num_required_signatures,
        fee_lamports: meta.fee,
        compute_units_consumed: meta.compute_units_consumed,
        err: meta
            .err
            .as_ref()
            .filter(|value| !value.is_null())
            .map(std::string::ToString::to_string),
        instructions,
        inner_instructions,
        transaction_config,
        address_table_lookup_count: wire
            .transaction
            .message
            .address_table_lookups
            .as_ref()
            .map_or(0, Vec::len),
    })
}

/// Converts a `getBlock` response into model input.
///
/// # Errors
///
/// Returns [`WireError`] if any transaction in the block fails to normalise;
/// see [`normalise_transaction`].
pub fn normalise_block(wire: &WireBlock, slot: u64) -> Result<NormalisedBlock, WireError> {
    Ok(NormalisedBlock {
        slot,
        block_time: wire.block_time,
        transactions: wire
            .transactions
            .iter()
            .map(|transaction| normalise_transaction(transaction, Some(slot)))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

/// The block time of a transaction response, when present.
#[must_use]
pub fn wire_block_time(wire: &WireTransaction) -> Option<i64> {
    wire.block_time
}
