//! Finding what a transaction paid for inclusion.
//!
//! A tip is not a protocol fee. It reaches a block builder or relay as an
//! ordinary System Program transfer, so it never appears in the charged fee and
//! has to be found in the instructions.
//!
//! Two things are detected, and the distinction is kept in the output:
//!
//! - **`KnownRecipient`** — a transfer to an account in the registry, top-level
//!   or through CPI. Certain, and labelled with the service.
//! - **`UnlabelledOutbound`** — a top-level transfer from the fee payer to an
//!   account that no other instruction in the transaction touches, in a
//!   transaction that also does something other than move SOL. Reported without
//!   a label.
//!
//! The second rule exists because only a handful of relays publish their tip
//! accounts. In the blocks recorded under `fixtures/`, payments matching it
//! outnumber the ones to published Jito accounts, and every one would otherwise
//! be reported as "no tip paid" — a positive claim the tool has no grounds to
//! make. A heuristic that is shown and explained beats silence that reads as
//! fact.
//!
//! The two conditions are what keep it from firing on ordinary activity.
//! Requiring the recipient to be untouched elsewhere excludes SOL moved as part
//! of the trade, such as a payment into a pool. Requiring non-SOL work excludes
//! a plain wallet-to-wallet transfer, where the recipient is the point rather
//! than a toll.

use std::collections::HashMap;

use crate::builtins::{COMPUTE_BUDGET_PROGRAM, SYSTEM_PROGRAM};
use crate::types::{NormalisedInstruction, NormalisedTransaction, TipBasis, TipTransfer, TipVia};

/// Discriminant of `SystemInstruction::Transfer`, as a little-endian u32.
const SYSTEM_TRANSFER_DISCRIMINANT: u32 = 2;

/// 4-byte discriminant followed by a u64 lamport amount.
const TRANSFER_DATA_LENGTH: usize = 12;

/// A decoded System Program transfer.
#[derive(Debug, Clone)]
pub struct SystemTransfer {
    /// The funding account.
    pub source: String,
    /// The recipient.
    pub destination: String,
    /// Amount moved, in lamports.
    pub lamports: u64,
}

/// Decodes a System Program transfer, or `None` if the instruction is not one.
///
/// `TransferWithSeed` has a different layout and is not decoded: it is not used
/// to pay tips in practice, and guessing at it would be worse than saying so.
#[must_use]
pub fn decode_system_transfer(instruction: &NormalisedInstruction) -> Option<SystemTransfer> {
    if instruction.program_address != SYSTEM_PROGRAM {
        return None;
    }
    if instruction.data.len() < TRANSFER_DATA_LENGTH {
        return None;
    }

    let discriminant = u32::from_le_bytes(instruction.data[0..4].try_into().ok()?);
    if discriminant != SYSTEM_TRANSFER_DISCRIMINANT {
        return None;
    }

    // Transfer takes [funding account, recipient account].
    Some(SystemTransfer {
        source: instruction.account_addresses.first()?.clone(),
        destination: instruction.account_addresses.get(1)?.clone(),
        lamports: u64::from_le_bytes(instruction.data[4..12].try_into().ok()?),
    })
}

/// True when the transaction does something besides move SOL around.
fn has_non_transfer_work(transaction: &NormalisedTransaction) -> bool {
    transaction.instructions.iter().any(|instruction| {
        instruction.program_address != SYSTEM_PROGRAM
            && instruction.program_address != COMPUTE_BUDGET_PROGRAM
    })
}

/// True when any instruction other than `exclude` references `address`.
fn referenced_elsewhere(
    transaction: &NormalisedTransaction,
    address: &str,
    exclude: usize,
) -> bool {
    transaction
        .instructions
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != exclude)
        .any(|(_, instruction)| {
            instruction
                .account_addresses
                .iter()
                .any(|candidate| candidate == address)
        })
}

/// Classifies one transfer, or returns `None` if it is not a payment for inclusion.
fn classify(
    transaction: &NormalisedTransaction,
    transfer: &SystemTransfer,
    position: usize,
    via: TipVia,
    tip_accounts: &HashMap<String, String>,
    allow_unlabelled: bool,
) -> Option<TipTransfer> {
    if let Some(service) = tip_accounts.get(&transfer.destination) {
        return Some(TipTransfer {
            recipient: transfer.destination.clone(),
            lamports: transfer.lamports,
            service: Some(service.clone()),
            via,
            basis: TipBasis::KnownRecipient,
        });
    }

    // Unlisted recipients are only considered at the top level: a transfer made
    // through CPI is the program's own logic, such as paying into a pool.
    if !allow_unlabelled || via != TipVia::TopLevel {
        return None;
    }
    if transfer.source != transaction.fee_payer {
        return None;
    }
    if referenced_elsewhere(transaction, &transfer.destination, position) {
        return None;
    }

    Some(TipTransfer {
        recipient: transfer.destination.clone(),
        lamports: transfer.lamports,
        service: None,
        via,
        basis: TipBasis::UnlabelledOutbound,
    })
}

/// Scans one instruction list.
fn scan(
    transaction: &NormalisedTransaction,
    instructions: &[NormalisedInstruction],
    via: TipVia,
    tip_accounts: &HashMap<String, String>,
    allow_unlabelled: bool,
) -> Vec<TipTransfer> {
    instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            let transfer = decode_system_transfer(instruction)?;
            if transfer.lamports == 0 {
                return None;
            }
            classify(
                transaction,
                &transfer,
                index,
                via,
                tip_accounts,
                allow_unlabelled,
            )
        })
        .collect()
}

/// Every payment for inclusion this transaction made.
#[must_use]
// The registry is always built by `tip_account_index`, so generalising this
// signature over hashers would add a parameter no caller would ever vary.
#[allow(clippy::implicit_hasher)]
pub fn find_tips(
    transaction: &NormalisedTransaction,
    tip_accounts: &HashMap<String, String>,
    known_recipients_only: bool,
) -> Vec<TipTransfer> {
    let allow_unlabelled = !known_recipients_only && has_non_transfer_work(transaction);

    let mut tips = scan(
        transaction,
        &transaction.instructions,
        TipVia::TopLevel,
        tip_accounts,
        allow_unlabelled,
    );
    tips.extend(scan(
        transaction,
        &transaction.inner_instructions,
        TipVia::Inner,
        tip_accounts,
        allow_unlabelled,
    ));
    tips
}
