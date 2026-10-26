//! Formatting helpers. Lamport amounts are rendered exactly, never through a float.

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Renders lamports as SOL with no trailing zeroes beyond `minimum_decimals`.
#[must_use]
pub fn sol(lamports: u64, minimum_decimals: usize) -> String {
    let whole = lamports / LAMPORTS_PER_SOL;
    let fraction = lamports % LAMPORTS_PER_SOL;

    let mut digits = format!("{fraction:09}").trim_end_matches('0').to_owned();
    while digits.len() < minimum_decimals {
        digits.push('0');
    }

    if digits.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{digits}")
    }
}

/// Thousands separators, which the standard library does not provide.
#[must_use]
pub fn int(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Thousands separators for a signed value, used for the model residual.
#[must_use]
pub fn signed(value: i128) -> String {
    let magnitude = value.unsigned_abs();
    let body = int(u64::try_from(magnitude).unwrap_or(u64::MAX));
    if value < 0 { format!("-{body}") } else { body }
}

#[must_use]
pub fn percent(fraction: Option<f64>) -> String {
    fraction.map_or_else(|| "-".to_owned(), |value| format!("{:.1}%", value * 100.0))
}

/// Shortens a base58 string to its ends, which is how they are read in practice.
#[must_use]
pub fn shorten(value: &str, edge: usize) -> String {
    if value.len() <= edge * 2 + 1 {
        return value.to_owned();
    }
    format!("{}..{}", &value[..edge], &value[value.len() - edge..])
}
