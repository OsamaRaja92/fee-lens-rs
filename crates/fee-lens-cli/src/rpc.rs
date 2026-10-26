//! A thin, retrying JSON-RPC client.
//!
//! Every read passes `maxSupportedTransactionVersion: 1`. Omitting it, or
//! passing 0, makes the node refuse the whole response with error -32015 the
//! moment a version it considers newer appears, and about half the transactions
//! in a current mainnet block are v1. Tooling written before v1 landed either
//! errors out or, worse, quietly reports partial coverage.
//!
//! Blocking HTTP on purpose: see the note in `Cargo.toml`.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use fee_lens_core::wire::{WireBlock, WireTransaction};
use fee_lens_core::{
    NormalisedBlock, NormalisedTransaction, normalise_block, normalise_transaction,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// The newest transaction version whose fee rules this build implements.
pub const MAX_SUPPORTED_TRANSACTION_VERSION: u8 = 1;

/// The page size `getSignaturesForAddress` allows.
const SIGNATURE_PAGE_SIZE: usize = 1000;

/// Attempts before giving up. Set with the public endpoint in mind: it rate
/// limits a burst of reads hard, and backing off for a few seconds usually
/// clears it.
const MAX_ATTEMPTS: u32 = 6;

#[derive(Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcErrorBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureEntry {
    signature: String,
}

/// Reads the chain over JSON-RPC.
pub struct Rpc {
    url: String,
    commitment: &'static str,
}

impl Rpc {
    pub fn new(url: String) -> Self {
        Self {
            url,
            commitment: "confirmed",
        }
    }

    /// One call, with retries on rate limiting and transient failure.
    fn call<T: DeserializeOwned>(&self, method: &str, params: &Value) -> Result<Option<T>> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let mut delay = Duration::from_millis(400);

        for attempt in 1..=MAX_ATTEMPTS {
            match self.attempt::<T>(&body) {
                Ok(result) => return Ok(result),
                Err(error) => {
                    // `{:#}` walks the whole chain; `{}` shows only the
                    // outermost context and would hide the status entirely.
                    let text = format!("{error:#}");
                    if let Some(refusal) = describe_version_refusal(&text) {
                        bail!(refusal);
                    }
                    if attempt == MAX_ATTEMPTS {
                        if is_rate_limit(&error) {
                            bail!(
                                "The RPC endpoint rate limited this read and was still rate \
                                 limiting after {MAX_ATTEMPTS} attempts. Point --rpc at an endpoint \
                                 with more headroom, or ask for less at once (a smaller --limit, or \
                                 --concurrency 1)."
                            );
                        }
                        return Err(error);
                    }
                    if !is_retryable(&error) {
                        return Err(error);
                    }
                    std::thread::sleep(delay);
                    delay *= 2;
                }
            }
        }

        unreachable!("the loop either returns or bails")
    }

    fn attempt<T: DeserializeOwned>(&self, body: &Value) -> Result<Option<T>> {
        // Only the host goes into the error. A full RPC URL usually carries an
        // API key in its path or query, and errors get pasted into issues.
        let mut response = ureq::post(&self.url)
            .send_json(body)
            .with_context(|| format!("POST to {}", host_of(&self.url)))?;
        let parsed: RpcResponse<T> = response
            .body_mut()
            .read_json()
            .context("decoding the JSON-RPC response")?;

        if let Some(error) = parsed.error {
            return Err(anyhow!("RPC error {}: {}", error.code, error.message));
        }
        Ok(parsed.result)
    }

    /// Fetches and normalises one transaction.
    pub fn transaction(&self, signature: &str) -> Result<NormalisedTransaction> {
        let params = json!([
            signature,
            {
                "encoding": "json",
                "commitment": self.commitment,
                "maxSupportedTransactionVersion": MAX_SUPPORTED_TRANSACTION_VERSION,
            }
        ]);

        let wire: WireTransaction = self.call("getTransaction", &params)?.ok_or_else(|| {
            anyhow!(
                "No transaction found for signature {signature}. It may have been dropped, or be \
                 outside the node's history."
            )
        })?;

        normalise_transaction(&wire, None).map_err(Into::into)
    }

    /// The most recent slot at the configured commitment.
    pub fn slot(&self) -> Result<u64> {
        self.call("getSlot", &json!([{ "commitment": self.commitment }]))?
            .ok_or_else(|| anyhow!("the node returned no slot"))
    }

    /// Fetches and normalises one block.
    pub fn block(&self, slot: u64) -> Result<NormalisedBlock> {
        let params = json!([
            slot,
            {
                "encoding": "json",
                "commitment": self.commitment,
                "maxSupportedTransactionVersion": MAX_SUPPORTED_TRANSACTION_VERSION,
                "rewards": false,
                "transactionDetails": "full",
            }
        ]);

        let wire: WireBlock = self
            .call("getBlock", &params)?
            .ok_or_else(|| anyhow!("Slot {slot} produced no block. It was probably skipped."))?;

        normalise_block(&wire, slot).map_err(Into::into)
    }

    /// Newest first, paging until `limit` signatures are collected.
    pub fn signatures(&self, address: &str, limit: usize) -> Result<Vec<String>> {
        let mut collected: Vec<String> = Vec::new();
        let mut before: Option<String> = None;

        while collected.len() < limit {
            let page = SIGNATURE_PAGE_SIZE.min(limit - collected.len());
            let mut config = json!({ "commitment": self.commitment, "limit": page });
            if let Some(cursor) = &before {
                config["before"] = json!(cursor);
            }

            let entries: Vec<SignatureEntry> = self
                .call("getSignaturesForAddress", &json!([address, config]))?
                .unwrap_or_default();

            if entries.is_empty() {
                break;
            }
            let short_page = entries.len() < page;
            before = entries.last().map(|entry| entry.signature.clone());
            collected.extend(entries.into_iter().map(|entry| entry.signature));

            if short_page {
                break;
            }
        }

        Ok(collected)
    }
}

/// The HTTP status a failed request carried, if it carried one.
///
/// Read by downcasting rather than by matching the error's text. `anyhow`'s
/// `Display` prints only the outermost context, so a status never appears in
/// `to_string()` and a text match silently never fires — which is what made the
/// retry above dead code until this was fixed.
fn status_code(error: &anyhow::Error) -> Option<u16> {
    error
        .downcast_ref::<ureq::Error>()
        .and_then(|error| match error {
            ureq::Error::StatusCode(code) => Some(*code),
            _ => None,
        })
}

fn is_rate_limit(error: &anyhow::Error) -> bool {
    if status_code(error) == Some(429) {
        return true;
    }
    let text = format!("{error:#}").to_lowercase();
    text.contains("rate limit") || text.contains("too many requests")
}

fn is_retryable(error: &anyhow::Error) -> bool {
    if is_rate_limit(error) || matches!(status_code(error), Some(500 | 502 | 503 | 504)) {
        return true;
    }
    let text = format!("{error:#}").to_lowercase();
    ["timeout", "timed out", "connection", "dns", "io error"]
        .iter()
        .any(|needle| text.contains(needle))
}

/// The host part of a URL, used so credentials never reach an error message.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("the endpoint")
        .to_owned()
}

/// Translates the node's version refusal into something actionable.
fn describe_version_refusal(message: &str) -> Option<String> {
    if !message.contains("-32015") && !message.contains("not supported by the requesting client") {
        return None;
    }
    Some(format!(
        "The node refused the response because it contains a transaction version newer than \
         {MAX_SUPPORTED_TRANSACTION_VERSION}. A new transaction version has probably shipped, and \
         its fee rules are not modelled here. Treat any older reading of this block as incomplete \
         rather than correct."
    ))
}
