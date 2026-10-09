//! Exact, non-negative decimal numbers.
//!
//! Prices are never floats. A [`Decimal`] is an integer mantissa and a decimal
//! scale (`value = mantissa / 10^scale`), parsed from text and normalised so
//! that equal values compare and print identically (`"2.50"` == `"2.5"`).

use std::{cmp::Ordering, fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The largest scale accepted (digits after the decimal point).
pub const MAX_SCALE: u32 = 30;

/// A non-negative exact decimal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Decimal {
    mantissa: u128,
    scale: u32,
}

/// Why a decimal string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecimalError {
    #[error("empty number")]
    Empty,
    #[error("negative values are not allowed: {0:?}")]
    Negative(String),
    #[error("not an exact decimal (digits with an optional fraction, e.g. \"2.50\"): {0:?}")]
    Syntax(String),
    #[error("too many digits after the decimal point (max {MAX_SCALE}): {0:?}")]
    TooPrecise(String),
    #[error("number too large: {0:?}")]
    Overflow(String),
}

impl Decimal {
    pub const ZERO: Self = Self {
        mantissa: 0,
        scale: 0,
    };

    /// Builds `mantissa / 10^scale`, normalised.
    pub fn new(mantissa: u128, scale: u32) -> Self {
        Self { mantissa, scale }.normalized()
    }

    /// A whole number.
    pub fn from_u64(value: u64) -> Self {
        Self::new(u128::from(value), 0)
    }

    fn normalized(mut self) -> Self {
        if self.mantissa == 0 {
            return Self::ZERO;
        }
        while self.scale > 0 && self.mantissa.is_multiple_of(10) {
            self.mantissa /= 10;
            self.scale -= 1;
        }
        self
    }

    pub fn is_zero(self) -> bool {
        self.mantissa == 0
    }

    /// Parses the catalog's strict format: ASCII digits with an optional
    /// fractional part (`"0"`, `"2"`, `"0.125"`). No sign, exponent,
    /// whitespace, separators or leading `.`.
    pub fn parse_strict(text: &str) -> Result<Self, DecimalError> {
        if text.is_empty() {
            return Err(DecimalError::Empty);
        }
        if text.starts_with('-') {
            return Err(DecimalError::Negative(text.to_owned()));
        }
        let (int, frac) = match text.split_once('.') {
            Some((int, frac)) => (int, Some(frac)),
            None => (text, None),
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !digits(int) || frac.is_some_and(|f| !digits(f)) {
            return Err(DecimalError::Syntax(text.to_owned()));
        }
        if int.len() > 1 && int.starts_with('0') {
            return Err(DecimalError::Syntax(text.to_owned()));
        }
        Self::from_parts(text, int, frac.unwrap_or(""), 0)
    }

    /// Parses a JSON number token exactly as written (`3e-06`, `1.5E-5`,
    /// `0.000015000020000000002`, `10`). Used for upstream sources; the text
    /// must come from the raw JSON (never from an `f64`).
    pub fn parse_json_number(text: &str) -> Result<Self, DecimalError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(DecimalError::Empty);
        }
        if let Some(rest) = text.strip_prefix('-') {
            // "-0" is still zero; everything else is a negative price.
            if Self::parse_json_number(rest).is_ok_and(Self::is_zero) {
                return Ok(Self::ZERO);
            }
            return Err(DecimalError::Negative(text.to_owned()));
        }
        let (number, exponent) = match text.find(['e', 'E']) {
            Some(index) => {
                let exp = &text[index + 1..];
                let exp: i64 = exp
                    .strip_prefix('+')
                    .unwrap_or(exp)
                    .parse()
                    .map_err(|_| DecimalError::Syntax(text.to_owned()))?;
                (&text[..index], exp)
            }
            None => (text, 0),
        };
        let (int, frac) = match number.split_once('.') {
            Some((int, frac)) => (int, frac),
            None => (number, ""),
        };
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if int.is_empty()
            || !digits(int)
            || !digits(frac)
            || (number.contains('.') && frac.is_empty())
        {
            return Err(DecimalError::Syntax(text.to_owned()));
        }
        Self::from_parts(text, int, frac, exponent)
    }

    /// Accepts a JSON value that is either a number or a numeric string.
    pub fn from_json_value(value: &serde_json::Value) -> Result<Self, DecimalError> {
        match value {
            serde_json::Value::Number(number) => Self::parse_json_number(&number.to_string()),
            serde_json::Value::String(text) => Self::parse_json_number(text),
            other => Err(DecimalError::Syntax(other.to_string())),
        }
    }

    fn from_parts(
        original: &str,
        int: &str,
        frac: &str,
        exponent: i64,
    ) -> Result<Self, DecimalError> {
        let overflow = || DecimalError::Overflow(original.to_owned());
        let mut mantissa: u128 = 0;
        for byte in int.bytes().chain(frac.bytes()) {
            mantissa = mantissa
                .checked_mul(10)
                .and_then(|m| m.checked_add(u128::from(byte - b'0')))
                .ok_or_else(overflow)?;
        }
        let mut scale = i64::try_from(frac.len()).map_err(|_| overflow())? - exponent;
        while scale < 0 {
            mantissa = mantissa.checked_mul(10).ok_or_else(overflow)?;
            scale += 1;
        }
        // Trailing zeros beyond MAX_SCALE are harmless; strip them first.
        while scale > i64::from(MAX_SCALE) && mantissa.is_multiple_of(10) && mantissa != 0 {
            mantissa /= 10;
            scale -= 1;
        }
        if mantissa == 0 {
            return Ok(Self::ZERO);
        }
        if scale > i64::from(MAX_SCALE) {
            return Err(DecimalError::TooPrecise(original.to_owned()));
        }
        Ok(Self::new(
            mantissa,
            u32::try_from(scale).map_err(|_| overflow())?,
        ))
    }

    /// Multiplies by `10^power` exactly (for example per-token to per-1M-tokens).
    pub fn shift(self, power: u32) -> Option<Self> {
        if self.scale >= power {
            return Some(Self::new(self.mantissa, self.scale - power));
        }
        let factor = 10u128.checked_pow(power - self.scale)?;
        Some(Self::new(self.mantissa.checked_mul(factor)?, 0))
    }

    /// Divides by `10^power` exactly (for example per-1K to per-unit).
    pub fn unshift(self, power: u32) -> Option<Self> {
        let scale = self.scale.checked_add(power)?;
        let value = Self::new(self.mantissa, scale);
        (value.scale <= MAX_SCALE).then_some(value)
    }

    /// `self × 10^6` as integer micro-units (micro-USD), rounded **up**, and
    /// whether the conversion was exact.
    pub fn to_micro_ceil(self) -> Option<(u128, bool)> {
        if self.scale <= 6 {
            let factor = 10u128.pow(6 - self.scale);
            return Some((self.mantissa.checked_mul(factor)?, true));
        }
        let divisor = 10u128.checked_pow(self.scale - 6)?;
        let whole = self.mantissa / divisor;
        let exact = self.mantissa.is_multiple_of(divisor);
        Some((if exact { whole } else { whole + 1 }, exact))
    }

    /// Mantissa and scale aligned to a common scale.
    fn aligned(self, other: Self) -> Option<(u128, u128)> {
        let scale = self.scale.max(other.scale);
        let a = self
            .mantissa
            .checked_mul(10u128.checked_pow(scale - self.scale)?)?;
        let b = other
            .mantissa
            .checked_mul(10u128.checked_pow(scale - other.scale)?)?;
        Some((a, b))
    }

    /// Whether `self` is within ±`percent` % of `reference` (inclusive).
    /// A zero reference only accepts zero.
    pub fn within_percent_of(self, reference: Self, percent: u32) -> bool {
        let Some((value, base)) = self.aligned(reference) else {
            return false;
        };
        let diff = value.abs_diff(base);
        match (diff.checked_mul(100), base.checked_mul(u128::from(percent))) {
            (Some(lhs), Some(rhs)) => lhs <= rhs,
            _ => false,
        }
    }

    /// Relative change from `reference` in basis points (1/100 %), signed.
    /// `None` when the reference is zero.
    pub fn change_basis_points(self, reference: Self) -> Option<i128> {
        if reference.is_zero() {
            return None;
        }
        let (value, base) = self.aligned(reference)?;
        let value = i128::try_from(value).ok()?;
        let base = i128::try_from(base).ok()?;
        (value - base).checked_mul(10_000).map(|n| n / base)
    }

    /// Fixed-point display with at least `min_places` decimals (trailing
    /// zeros beyond that are dropped): `2` -> `"2.00"`, `0.125` -> `"0.125"`.
    pub fn display_min_places(self, min_places: u32) -> String {
        let places = self.scale.max(min_places);
        let mantissa = self.mantissa * 10u128.pow(places - self.scale);
        let divisor = 10u128.pow(places);
        if places == 0 {
            return mantissa.to_string();
        }
        format!(
            "{}.{:0width$}",
            mantissa / divisor,
            mantissa % divisor,
            width = places as usize
        )
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_min_places(0))
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_strict(s)
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.aligned(*other) {
            Some((a, b)) => a.cmp(&b),
            // Alignment overflows only for wildly different magnitudes;
            // fall back to comparing integer parts.
            None => (self.mantissa / 10u128.pow(self.scale))
                .cmp(&(other.mantissa / 10u128.pow(other.scale))),
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse_strict(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Decimal {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Decimal".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^(0|[1-9][0-9]*)(\\.[0-9]+)?$",
            "description": "Exact non-negative decimal in USD per unit, written as a string (never a float)."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse_strict(s).unwrap()
    }

    #[test]
    fn strict_parsing() {
        assert_eq!(d("2.50"), d("2.5"));
        assert_eq!(d("2.50").to_string(), "2.5");
        assert_eq!(d("0").to_string(), "0");
        assert_eq!(d("0.000").to_string(), "0");
        assert_eq!(d("0.0125").to_string(), "0.0125");
        for bad in [
            "", "-1", "1e-6", ".5", "5.", "01", "1,000", " 1", "+1", "NaN", "1.2.3",
        ] {
            assert!(
                Decimal::parse_strict(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn json_number_parsing_is_exact() {
        let n = |s: &str| Decimal::parse_json_number(s).unwrap();
        assert_eq!(n("3e-06"), d("0.000003"));
        assert_eq!(n("1.5E-5"), d("0.000015"));
        assert_eq!(n("1.25e+2"), d("125"));
        assert_eq!(n("0.000015000020000000002"), d("0.000015000020000000002"));
        assert_eq!(n("10"), d("10"));
        assert_eq!(n("-0"), Decimal::ZERO);
        assert!(Decimal::parse_json_number("-1e-6").is_err());
        assert!(Decimal::parse_json_number("1.").is_err());
        assert!(Decimal::parse_json_number("e5").is_err());
    }

    #[test]
    fn per_token_to_per_million() {
        let per_token = Decimal::parse_json_number("3.75e-06").unwrap();
        assert_eq!(per_token.shift(6).unwrap(), d("3.75"));
        let per_token = Decimal::parse_json_number("0.0000000296").unwrap();
        assert_eq!(per_token.shift(6).unwrap(), d("0.0296"));
        assert_eq!(d("10").unshift(3).unwrap(), d("0.01"));
    }

    #[test]
    fn micro_conversion_rounds_up_and_flags() {
        assert_eq!(d("2").to_micro_ceil(), Some((2_000_000, true)));
        assert_eq!(d("0.0125").to_micro_ceil(), Some((12_500, true)));
        assert_eq!(d("0.000001").to_micro_ceil(), Some((1, true)));
        assert_eq!(d("0.0000001").to_micro_ceil(), Some((1, false)));
        assert_eq!(d("0.0296").to_micro_ceil(), Some((29_600, true)));
        // Float artifact from a source: rounds up, flagged inexact.
        assert_eq!(
            Decimal::parse_json_number("15.000020000000002")
                .unwrap()
                .to_micro_ceil(),
            Some((15_000_021, false))
        );
        assert_eq!(Decimal::ZERO.to_micro_ceil(), Some((0, true)));
    }

    #[test]
    fn ordering_and_percent() {
        assert!(d("0.1") < d("0.125"));
        assert!(d("10") > d("9.99"));
        assert!(d("1.25").within_percent_of(d("1"), 25));
        assert!(!d("1.26").within_percent_of(d("1"), 25));
        assert!(d("0.75").within_percent_of(d("1"), 25));
        assert!(!d("0.74").within_percent_of(d("1"), 25));
        assert!(!d("0.01").within_percent_of(Decimal::ZERO, 25));
        assert_eq!(d("1.5").change_basis_points(d("1")), Some(5000));
        assert_eq!(d("0.5").change_basis_points(d("1")), Some(-5000));
    }

    #[test]
    fn display_places() {
        assert_eq!(d("2").display_min_places(2), "2.00");
        assert_eq!(d("0.1").display_min_places(2), "0.10");
        assert_eq!(d("0.125").display_min_places(2), "0.125");
        assert_eq!(d("15").display_min_places(0), "15");
    }
}
