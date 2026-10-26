//! Terminal views. All numbers come from the analysis; nothing is recomputed here.

use fee_lens_core::{
    AccountAnalysis, ComputeLimitSource, CostAggregate, PercentileTable, PriorityFeeSource,
    SlotAnalysis, TipTransfer, TipVia, TransactionAnalysis,
};

use crate::format::{int, percent, shorten, signed, sol};
use crate::table::{Align, Column, render};

const LABEL_WIDTH: usize = 19;

fn field(label: &str, value: &str, note: Option<&str>) -> String {
    let padded = format!("{label:<LABEL_WIDTH$}");
    match note {
        Some(note) => format!("  {padded}{value}  {note}"),
        None => format!("  {padded}{value}"),
    }
}

fn lamport_line(label: &str, lamports: u64, note: Option<&str>) -> String {
    field(label, &format!("{:>14} lamports", int(lamports)), note)
}

fn limit_source_label(source: ComputeLimitSource) -> &'static str {
    match source {
        ComputeLimitSource::ComputeBudgetInstruction => "from a SetComputeUnitLimit instruction",
        ComputeLimitSource::V1Header => "from the v1 header",
        ComputeLimitSource::RuntimeDefault => "derived by the runtime (none declared)",
    }
}

fn priority_source_label(source: PriorityFeeSource) -> &'static str {
    match source {
        PriorityFeeSource::ComputeUnitPrice => "price per compute unit x limit",
        PriorityFeeSource::V1Header => "absolute total in the v1 header",
        PriorityFeeSource::None => "no priority fee paid",
    }
}

/// Says how many payments were found and how many had an identifiable recipient.
fn describe_tips(tips: &[TipTransfer]) -> String {
    let labelled = tips.iter().filter(|tip| tip.service.is_some()).count();
    let unlabelled = tips.len() - labelled;
    match (labelled, unlabelled) {
        (_, 0) => format!("{labelled} payment(s), recipient identified"),
        (0, _) => format!("{unlabelled} payment(s), recipient not in the registry"),
        _ => format!("{labelled} identified, {unlabelled} to unlisted recipient(s)"),
    }
}

fn percentile_rows(table: &PercentileTable) -> String {
    [
        ("p50", table.p50),
        ("p75", table.p75),
        ("p90", table.p90),
        ("p99", table.p99),
        ("max", table.max),
    ]
    .iter()
    .map(|(name, value)| {
        let rendered = value.map_or_else(|| "-".to_owned(), int);
        format!("  {name:<6}{rendered}")
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn tip_table(tips: &[TipTransfer]) -> String {
    let columns = [
        Column {
            header: "recipient",
            align: Align::Left,
        },
        Column {
            header: "service",
            align: Align::Left,
        },
        Column {
            header: "lamports",
            align: Align::Right,
        },
        Column {
            header: "via",
            align: Align::Left,
        },
    ];
    let rows: Vec<Vec<String>> = tips
        .iter()
        .map(|tip| {
            vec![
                shorten(&tip.recipient, 6),
                tip.service
                    .clone()
                    .unwrap_or_else(|| "not in registry".to_owned()),
                int(tip.lamports),
                match tip.via {
                    TipVia::TopLevel => "top-level".to_owned(),
                    TipVia::Inner => "inner".to_owned(),
                },
            ]
        })
        .collect();
    render(&columns, &rows)
}

/// The fee section of the single-transaction view.
fn fee_section(analysis: &TransactionAnalysis) -> Vec<String> {
    let fee = &analysis.fee;
    let mut out = vec![String::new(), "Fee".to_owned()];

    let signature_note = if fee.precompile_signatures > 0 {
        format!(
            "{} signature(s) x 5,000, {} from a precompile",
            int(u64::from(fee.signatures_charged)),
            int(u64::from(fee.precompile_signatures))
        )
    } else {
        format!(
            "{} signature(s) x 5,000",
            int(u64::from(fee.signatures_charged))
        )
    };
    out.push(lamport_line(
        "base",
        fee.base_lamports,
        Some(&signature_note),
    ));
    out.push(lamport_line(
        "priority",
        fee.priority_lamports,
        Some(priority_source_label(fee.priority_source)),
    ));

    let tip_note = if analysis.tips.is_empty() {
        "no outbound payment found".to_owned()
    } else if analysis.succeeded {
        describe_tips(&analysis.tips)
    } else {
        // Rolled back with the transaction, so none of it was paid.
        format!(
            "{} instructed, rolled back with the transaction",
            int(fee.tip_lamports_instructed)
        )
    };
    out.push(lamport_line("tips", fee.tip_lamports, Some(&tip_note)));
    out.push(lamport_line(
        "charged",
        fee.observed_fee_lamports,
        Some(&format!("{} SOL", sol(fee.observed_fee_lamports, 0))),
    ));
    out.push(lamport_line(
        "total to land",
        fee.total_cost_lamports,
        Some(&format!(
            "{} SOL, fee plus tips",
            sol(fee.total_cost_lamports, 0)
        )),
    ));

    if fee.model_residual_lamports == 0 {
        out.push(field(
            "model check",
            &format!("{:>14}", "exact"),
            Some("the model reproduces the charged fee"),
        ));
    } else {
        out.push(field(
            "model check",
            &format!(
                "{:>14}",
                format!("off by {}", signed(fee.model_residual_lamports))
            ),
            Some("lamports unaccounted for: the fee model is incomplete here"),
        ));
    }

    out
}

/// The compute section, including the warning about inert v1 directives.
fn compute_section(analysis: &TransactionAnalysis) -> Vec<String> {
    let compute = &analysis.compute;
    let mut out = vec![String::new(), "Compute".to_owned()];

    out.push(field(
        "limit",
        &format!("{:>14} units", int(u64::from(compute.effective_limit))),
        Some(limit_source_label(compute.limit_source)),
    ));
    out.push(field(
        "consumed",
        &format!(
            "{:>14} units",
            compute.consumed.map_or_else(|| "-".to_owned(), int)
        ),
        None,
    ));
    let wasted_note = compute
        .wasted_fraction
        .map(|_| format!("{} of the budget", percent(compute.wasted_fraction)));
    out.push(field(
        "unused",
        &format!(
            "{:>14} units",
            compute.wasted.map_or_else(|| "-".to_owned(), int)
        ),
        wasted_note.as_deref(),
    ));
    out.push(lamport_line(
        "overpaid",
        compute.overpaid_lamports,
        Some(
            if analysis.version == fee_lens_core::TransactionVersion::V1 {
                "a v1 priority fee does not scale with the limit"
            } else {
                "what an exactly fitted limit would have saved"
            },
        ),
    ));

    if analysis.directives.inert_on_v1 {
        let declared = analysis
            .directives
            .compute_unit_limit
            .map_or_else(|| "none".to_owned(), |limit| int(u64::from(limit)));
        out.push(String::new());
        out.push("ComputeBudget instructions present but inert".to_owned());
        out.push(format!(
            "  This is a v1 transaction, so the runtime reads its limits from the header and ignores\n  \
             the {} ComputeBudget instruction(s) in the message. Those instructions declare a\n  \
             limit of {}, against the header's {}. A tool that reads the instruction\n  \
             rather than the header reports the wrong figure here.",
            analysis.directives.instruction_count,
            declared,
            int(u64::from(compute.effective_limit)),
        ));
    }

    out
}

/// The single-transaction view.
#[must_use]
pub fn transaction(analysis: &TransactionAnalysis) -> String {
    let mut out = vec![format!("Transaction {}", shorten(&analysis.signature, 8))];
    out.push(field("version", &analysis.version.to_string(), None));
    out.push(field("slot", &analysis.slot.to_string(), None));

    let err_note: Option<String> = analysis
        .err
        .as_deref()
        .map(|text| text.chars().take(120).collect());
    out.push(field(
        "result",
        if analysis.succeeded {
            "success"
        } else {
            "failed"
        },
        err_note.as_deref(),
    ));
    if analysis.address_table_lookup_count > 0 {
        out.push(field(
            "lookup tables",
            &analysis.address_table_lookup_count.to_string(),
            None,
        ));
    }

    out.extend(fee_section(analysis));
    out.extend(compute_section(analysis));

    if !analysis.tips.is_empty() {
        out.push(String::new());
        out.push("Tips".to_owned());
        out.push(tip_table(&analysis.tips));
    }

    out.join("\n")
}

/// Counts and version mix.
fn aggregate_header(totals: &CostAggregate) -> Vec<String> {
    let versions = totals
        .version_counts
        .iter()
        .map(|(version, count)| format!("{version} {count}"))
        .collect::<Vec<_>>()
        .join("   ");

    vec![
        field("transactions", &int(totals.transaction_count as u64), None),
        field(
            "outcome",
            &format!(
                "{} succeeded   {} failed",
                int(totals.succeeded as u64),
                int(totals.failed as u64)
            ),
            None,
        ),
        field("versions", &versions, None),
        field(
            "fee expressed as",
            &format!(
                "v1 header {}   price per unit {}   none {}",
                totals.fee_sources.v1_header,
                totals.fee_sources.compute_unit_price,
                totals.fee_sources.none
            ),
            None,
        ),
    ]
}

/// What was paid, and what compute it was paid against.
fn aggregate_cost(totals: &CostAggregate) -> Vec<String> {
    vec![
        String::new(),
        "Cost".to_owned(),
        field(
            "fees charged",
            &format!("{:>14} SOL", sol(totals.total_fee_lamports, 6)),
            None,
        ),
        field(
            "of that, priority",
            &format!("{:>14} SOL", sol(totals.total_priority_lamports, 6)),
            None,
        ),
        field(
            "tips",
            &format!("{:>14} SOL", sol(totals.total_tip_lamports, 6)),
            Some(&format!(
                "across {} transaction(s)",
                int(totals.tip_paying_transactions as u64)
            )),
        ),
        field(
            "overpaid compute",
            &format!("{:>14} SOL", sol(totals.total_overpaid_lamports, 6)),
            Some("recoverable by fitting the limit to what is used"),
        ),
        String::new(),
        "Compute".to_owned(),
        field(
            "budgeted",
            &format!("{:>14} units", int(totals.total_compute_budgeted)),
            None,
        ),
        field(
            "consumed",
            &format!("{:>14} units", int(totals.total_compute_consumed)),
            None,
        ),
        field(
            "unused",
            &format!("{:>14}", percent(totals.compute_wasted_fraction)),
            Some("of what was budgeted"),
        ),
    ]
}

/// The distributions, and the model's self-check.
fn aggregate_distribution(totals: &CostAggregate) -> Vec<String> {
    let exact = totals.transaction_count - totals.model_mismatches;
    let self_check = if totals.model_mismatches == 0 {
        format!(
            "  {}/{} transactions reproduced exactly from their own directives.",
            int(exact as u64),
            int(totals.transaction_count as u64)
        )
    } else {
        format!(
            "  {}/{} reproduced exactly; {} did not. The fee model is incomplete.",
            int(exact as u64),
            int(totals.transaction_count as u64),
            int(totals.model_mismatches as u64)
        )
    };

    vec![
        String::new(),
        "Priority fee per transaction".to_owned(),
        "  lamports, over transactions that paid one".to_owned(),
        percentile_rows(&totals.priority_lamports_percentiles),
        String::new(),
        "Priority fee per compute unit consumed".to_owned(),
        "  micro-lamports, the figure that is comparable between transactions".to_owned(),
        percentile_rows(&totals.priority_per_compute_unit_percentiles),
        String::new(),
        "Model self-check".to_owned(),
        self_check,
    ]
}

/// Everything shared by the slot and account views.
fn aggregate_lines(totals: &CostAggregate) -> Vec<String> {
    let mut out = aggregate_header(totals);
    out.extend(aggregate_cost(totals));
    out.extend(aggregate_distribution(totals));
    out
}

fn top_table(transactions: &[TransactionAnalysis], top_count: usize) -> Vec<String> {
    if top_count == 0 || transactions.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<&TransactionAnalysis> = transactions.iter().collect();
    sorted.sort_by_key(|tx| std::cmp::Reverse(tx.fee.total_cost_lamports));
    sorted.truncate(top_count);

    let columns = [
        Column {
            header: "signature",
            align: Align::Left,
        },
        Column {
            header: "ver",
            align: Align::Left,
        },
        Column {
            header: "fee",
            align: Align::Right,
        },
        Column {
            header: "tip",
            align: Align::Right,
        },
        Column {
            header: "total",
            align: Align::Right,
        },
        Column {
            header: "cu used",
            align: Align::Right,
        },
        Column {
            header: "ok",
            align: Align::Left,
        },
    ];
    let rows: Vec<Vec<String>> = sorted
        .iter()
        .map(|tx| {
            vec![
                shorten(&tx.signature, 7),
                tx.version.to_string(),
                int(tx.fee.observed_fee_lamports),
                int(tx.fee.tip_lamports),
                int(tx.fee.total_cost_lamports),
                tx.compute.consumed.map_or_else(|| "-".to_owned(), int),
                if tx.succeeded {
                    "y".to_owned()
                } else {
                    "n".to_owned()
                },
            ]
        })
        .collect();

    vec![
        String::new(),
        format!("Highest total cost to land (top {})", sorted.len()),
        render(&columns, &rows),
    ]
}

const LANDED_ONLY_NOTE: &str = "  Note: these are the transactions that landed. The ones that were dropped are not\n  \
visible here, so a percentile shows what was sufficient, not what was required.";

/// The slot view.
#[must_use]
pub fn slot(analysis: &SlotAnalysis, top_count: usize) -> String {
    let mut out = vec![format!("Slot {}", analysis.slot)];
    out.extend(aggregate_lines(&analysis.totals));
    out.extend(top_table(&analysis.transactions, top_count));
    out.push(String::new());
    out.push(LANDED_ONLY_NOTE.to_owned());
    out.join("\n")
}

/// The account view.
#[must_use]
pub fn account(analysis: &AccountAnalysis, top_count: usize) -> String {
    let mut out = vec![format!("Account {}", analysis.address)];
    out.push(field(
        "signatures",
        &int(analysis.signatures_inspected as u64),
        Some("newest first"),
    ));
    out.extend(aggregate_lines(&analysis.totals));
    out.extend(top_table(&analysis.transactions, top_count));
    out.push(String::new());
    out.push(LANDED_ONLY_NOTE.to_owned());
    out.join("\n")
}
