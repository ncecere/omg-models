//! Display formatting. Money is formatted from exact decimals, never floats.

use omg_models_catalog::{Decimal, Meter};

/// `$2.00`, `$0.125`, `$0.0296`.
pub fn usd(value: Decimal) -> String {
    format!("${}", value.display_min_places(2))
}

/// A rate with its unit, e.g. `$2.00 / 1M tokens`.
pub fn rate_with_unit(meter: Meter, value: Decimal) -> String {
    let unit = match meter.unit() {
        "per 1M tokens" => "/ 1M tokens",
        "per 1M characters" => "/ 1M characters",
        "per image" => "/ image",
        "per second" => "/ second",
        "per search" => "/ search",
        _ => "/ request",
    };
    format!("{} {unit}", usd(value))
}

/// Compact token counts: `1M`, `1.05M`, `400K`, `131K`, `16K`.
pub fn tokens(value: u64) -> String {
    if value >= 1_000_000 {
        let whole = value / 1_000_000;
        let hundredths = (value % 1_000_000) / 10_000;
        if hundredths == 0 {
            format!("{whole}M")
        } else if hundredths.is_multiple_of(10) {
            format!("{whole}.{}M", hundredths / 10)
        } else {
            format!("{whole}.{hundredths:02}M")
        }
    } else if value >= 1_000 {
        format!("{}K", value / 1_000)
    } else {
        value.to_string()
    }
}

/// `1,050,000`.
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `100K` style label for a tier threshold, e.g. "over 100K prompt tokens".
pub fn tier_label(above: u64) -> String {
    format!("Prompt > {}", tokens(above))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let d = |s: &str| Decimal::parse_strict(s).unwrap();
        assert_eq!(usd(d("2")), "$2.00");
        assert_eq!(usd(d("0.125")), "$0.125");
        assert_eq!(usd(d("0.0296")), "$0.0296");
        assert_eq!(tokens(1_000_000), "1M");
        assert_eq!(tokens(1_050_000), "1.05M");
        assert_eq!(tokens(1_048_576), "1.04M");
        assert_eq!(tokens(1_500_000), "1.5M");
        assert_eq!(tokens(131_072), "131K");
        assert_eq!(tokens(999), "999");
        assert_eq!(thousands(1_050_000), "1,050,000");
        assert_eq!(thousands(128), "128");
        assert_eq!(
            rate_with_unit(Meter::SearchUnits, d("0.01")),
            "$0.01 / search"
        );
    }
}
