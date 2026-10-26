//! Known tip accounts.
//!
//! A tip is not a protocol fee. It is a plain SOL transfer to an account a block
//! builder or relay controls, so it never appears in the charged fee and has to
//! be found by looking at the instructions.
//!
//! Only accounts that can be checked against a published source belong here. The
//! Jito set below is what its `getTipAccounts` endpoint returns; all eight were
//! observed receiving tips in the blocks recorded under `fixtures/`.
//!
//! Other relays run tip accounts that are not published in any one place, which
//! is why [`crate::tips`] also reports payments by shape rather than identity.

use std::collections::HashMap;

/// Jito's published tip accounts.
pub const KNOWN_TIP_ACCOUNTS: &[(&str, &str)] = &[
    ("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5", "Jito"),
    ("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe", "Jito"),
    ("Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY", "Jito"),
    ("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49", "Jito"),
    ("DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh", "Jito"),
    ("ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt", "Jito"),
    ("DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL", "Jito"),
    ("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT", "Jito"),
];

/// Builds a lookup table, optionally extended with caller-supplied accounts.
///
/// An entry may be a bare address or `address=label`. The label matters when you
/// run against several relays: a name you recognise in the output is worth more
/// than "custom" when comparing what each one costs you.
#[must_use]
// The registry is always built by `tip_account_index`, so generalising this
// signature over hashers would add a parameter no caller would ever vary.
#[allow(clippy::implicit_hasher)]
pub fn tip_account_index(extra: &[String]) -> HashMap<String, String> {
    let mut index: HashMap<String, String> = KNOWN_TIP_ACCOUNTS
        .iter()
        .map(|(address, service)| ((*address).to_owned(), (*service).to_owned()))
        .collect();

    for entry in extra {
        let (address, label) = match entry.split_once('=') {
            Some((address, label)) if !label.is_empty() => (address, label),
            Some((address, _)) => (address, "custom"),
            None => (entry.as_str(), "custom"),
        };
        if !address.is_empty() {
            index.insert(address.to_owned(), label.to_owned());
        }
    }

    index
}
