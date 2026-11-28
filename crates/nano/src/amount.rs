//! Amounts. The ledger counts in raw: 1 XNO is 10^30 raw.

use anyhow::{Result, bail, ensure};

/// Raw per XNO.
pub const RAW_PER_XNO: u128 = 1_000_000_000_000_000_000_000_000_000_000;

/// Decimal places of one XNO.
const DECIMALS: usize = 30;

/// Reads an amount in XNO, such as `1`, `0.25` or `1,5`, into raw.
pub fn parse_xno(text: &str) -> Result<u128> {
    let text = text.trim().replace(['\u{a0}', ' ', '_'], "");
    let text = text.strip_suffix("XNO").unwrap_or(&text).trim();
    ensure!(!text.is_empty(), "enter an amount");
    let (whole, fraction) = match text.split_once(['.', ',']) {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || !digits(fraction) || (whole.is_empty() && fraction.is_empty()) {
        bail!("the amount must be a number, such as 0.5");
    }
    ensure!(
        fraction.len() <= DECIMALS,
        "an amount has at most {DECIMALS} decimal places"
    );
    let whole: u128 = if whole.is_empty() { 0 } else { whole.parse()? };
    let padded = format!("{fraction:0<DECIMALS$}");
    let fraction: u128 = padded.parse()?;
    let raw = whole
        .checked_mul(RAW_PER_XNO)
        .and_then(|raw| raw.checked_add(fraction))
        .ok_or_else(|| anyhow::anyhow!("the amount is too large"))?;
    ensure!(raw > 0, "the amount must be more than zero");
    Ok(raw)
}

/// Writes raw as XNO with a dot and no trailing zeros: `1.5`, `0.000001`.
pub fn format_xno(raw: u128) -> String {
    let whole = raw / RAW_PER_XNO;
    let fraction = raw % RAW_PER_XNO;
    if fraction == 0 {
        return whole.to_string();
    }
    let fraction = format!("{fraction:0>DECIMALS$}");
    format!("{whole}.{}", fraction.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_round_trip() {
        assert_eq!(parse_xno("1").unwrap(), RAW_PER_XNO);
        assert_eq!(parse_xno("1,5").unwrap(), RAW_PER_XNO * 3 / 2);
        assert_eq!(parse_xno(" 0.000001 XNO").unwrap(), RAW_PER_XNO / 1_000_000);
        assert_eq!(parse_xno(".5").unwrap(), RAW_PER_XNO / 2);
        assert_eq!(format_xno(RAW_PER_XNO * 3 / 2), "1.5");
        assert_eq!(format_xno(1), "0.000000000000000000000000000001");
        assert_eq!(format_xno(0), "0");
        let supply = 133_248_297 * RAW_PER_XNO;
        assert_eq!(parse_xno(&format_xno(supply)).unwrap(), supply);
    }

    #[test]
    fn nonsense_is_refused() {
        for bad in [
            "",
            "0",
            "-1",
            "1.2.3",
            "abc",
            "1e5",
            "0.0000000000000000000000000000001",
        ] {
            assert!(parse_xno(bad).is_err(), "{bad} was accepted");
        }
        assert!(parse_xno("999999999999").is_err(), "overflow");
    }
}
