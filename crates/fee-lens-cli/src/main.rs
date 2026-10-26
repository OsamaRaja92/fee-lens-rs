//! The fee-lens command line interface.
//!
//! Three views over the same model: one transaction, one slot, or one account's
//! recent history. Every command takes `--json` and prints the full analysis, so
//! the tool composes with anything that reads JSON rather than only being
//! readable by a person.
//!
//! This is the Rust twin of the TypeScript implementation at
//! <https://github.com/OsamaRaja92/solana-fee-lens>; both reproduce the same
//! recorded blocks to the lamport.

mod format;
mod render;
mod rpc;
mod table;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use fee_lens_core::{
    AnalyseOptions, analyse_account, analyse_slot, analyse_transaction, tip_account_index,
};

const PUBLIC_RPC: &str = "https://api.mainnet-beta.solana.com";

/// Work out what a Solana transaction actually paid to land: base fee, priority
/// fee and tips, correct across legacy, v0 and v1 transactions.
#[derive(Parser)]
#[command(name = "fee-lens", version, about, long_about = None)]
struct Cli {
    /// JSON-RPC endpoint. Defaults to `$SOLANA_RPC_URL`, else the public endpoint.
    #[arg(long, global = true)]
    rpc: Option<String>,

    /// Print the full analysis as JSON.
    #[arg(long, global = true)]
    json: bool,

    /// Name additional tip accounts: ADDRESS or ADDRESS=LABEL, repeatable.
    #[arg(long = "tip-account", global = true, value_name = "ENTRY")]
    tip_accounts: Vec<String>,

    /// Count only payments to registry recipients, not unlabelled outbound transfers.
    #[arg(long, global = true)]
    known_tips_only: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Break down the cost of one transaction.
    Tx {
        /// Transaction signature, base58.
        signature: String,
    },
    /// What it cost to be in one block.
    Slot {
        /// Slot number. Defaults to the most recent confirmed slot.
        slot: Option<u64>,
        /// Show the n most expensive transactions.
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// What an account has been paying across its recent transactions.
    Account {
        /// Any account address, base58.
        address: String,
        /// How many recent transactions to inspect.
        #[arg(long, default_value_t = 25)]
        limit: usize,
        /// Show the n most expensive transactions.
        #[arg(long, default_value_t = 10)]
        top: usize,
        /// Transactions to fetch at once; lower it if the endpoint rate limits.
        #[arg(long, default_value_t = 2)]
        concurrency: usize,
    },
}

impl Cli {
    fn rpc_url(&self) -> String {
        self.rpc
            .clone()
            .or_else(|| std::env::var("SOLANA_RPC_URL").ok())
            .unwrap_or_else(|| PUBLIC_RPC.to_owned())
    }

    fn analyse_options(&self) -> AnalyseOptions {
        AnalyseOptions {
            tip_accounts: tip_account_index(&self.tip_accounts),
            known_recipients_only: self.known_tips_only,
        }
    }
}

/// Fetches transactions with a bounded number of requests in flight.
///
/// Scoped threads rather than an async runtime: the work is a handful of
/// blocking HTTP calls, and a runtime would be weight without a reason.
fn fetch_concurrently(
    client: &rpc::Rpc,
    signatures: &[String],
    concurrency: usize,
) -> Result<Vec<fee_lens_core::NormalisedTransaction>> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<Result<fee_lens_core::NormalisedTransaction>>>> = (0
        ..signatures.len())
        .map(|_| std::sync::Mutex::new(None))
        .collect();

    std::thread::scope(|scope| {
        for _ in 0..concurrency.max(1).min(signatures.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(signature) = signatures.get(index) else {
                        return;
                    };
                    let outcome = client.transaction(signature);
                    *slots[index]
                        .lock()
                        .expect("a poisoned lock means a panicked worker") = Some(outcome);
                }
            });
        }
    });

    let mut transactions = Vec::with_capacity(signatures.len());
    for slot in slots {
        match slot.into_inner().expect("lock") {
            Some(Ok(transaction)) => transactions.push(transaction),
            Some(Err(error)) => return Err(error),
            None => bail!("a transaction was never fetched"),
        }
    }
    Ok(transactions)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = rpc::Rpc::new(cli.rpc_url());
    let options = cli.analyse_options();

    let rendered = match &cli.command {
        Command::Tx { signature } => {
            let analysis = analyse_transaction(&client.transaction(signature)?, &options);
            if cli.json {
                serde_json::to_string_pretty(&analysis)?
            } else {
                render::transaction(&analysis)
            }
        }
        Command::Slot { slot, top } => {
            let slot = match slot {
                Some(slot) => *slot,
                None => client.slot()?,
            };
            let analysis = analyse_slot(&client.block(slot)?, &options);
            if cli.json {
                serde_json::to_string_pretty(&analysis)?
            } else {
                render::slot(&analysis, *top)
            }
        }
        Command::Account {
            address,
            limit,
            top,
            concurrency,
        } => {
            if *limit == 0 {
                bail!("--limit must be a positive integer");
            }
            let signatures = client.signatures(address, *limit)?;
            let transactions = fetch_concurrently(&client, &signatures, *concurrency)?;
            let analysis = analyse_account(address, signatures.len(), &transactions, &options);
            if cli.json {
                serde_json::to_string_pretty(&analysis)?
            } else {
                render::account(&analysis, *top)
            }
        }
    };

    println!("{rendered}");
    Ok(())
}
