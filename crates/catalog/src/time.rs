//! Calendar dates and UTC timestamps, written as strings in the data files.

use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

fn parse_digits<T: FromStr>(text: &str, len: usize) -> Option<T> {
    (text.len() == len && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// A calendar date, `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let year = parse_digits(parts.next()?, 4)?;
        let month = parse_digits(parts.next()?, 2)?;
        let day = parse_digits(parts.next()?, 2)?;
        if parts.next().is_some() || !(1..=12).contains(&month) {
            return None;
        }
        (day >= 1 && day <= days_in_month(year, month)).then_some(Self { year, month, day })
    }

    /// The civil date `days` days after 1970-01-01 (proleptic Gregorian).
    pub fn from_unix_days(days: i64) -> Self {
        // Howard Hinnant's days_from_civil inverse.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self {
            year: u16::try_from(year).unwrap_or(0),
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid date {text:?}: expected \"YYYY-MM-DD\""))
        })
    }
}

impl JsonSchema for Date {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Date".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "format": "date", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$" })
    }
}

/// A month or a day, `YYYY-MM` or `YYYY-MM-DD` (knowledge cutoffs).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartialDate(String);

impl PartialDate {
    pub fn parse(text: &str) -> Option<Self> {
        let valid = match text.len() {
            7 => Date::parse(&format!("{text}-01")).is_some(),
            10 => Date::parse(text).is_some(),
            _ => false,
        };
        valid.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for PartialDate {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PartialDate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid date {text:?}: expected \"YYYY-MM\" or \"YYYY-MM-DD\""
            ))
        })
    }
}

impl JsonSchema for PartialDate {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PartialDate".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "pattern": "^[0-9]{4}-[0-9]{2}(-[0-9]{2})?$" })
    }
}

/// A UTC timestamp, `YYYY-MM-DDTHH:MM:SSZ`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn parse(text: &str) -> Option<Self> {
        let (date, time) = text.split_once('T')?;
        Date::parse(date)?;
        let time = time.strip_suffix('Z')?;
        let mut parts = time.split(':');
        let hour: u8 = parse_digits(parts.next()?, 2)?;
        let minute: u8 = parse_digits(parts.next()?, 2)?;
        let second: u8 = parse_digits(parts.next()?, 2)?;
        (parts.next().is_none() && hour < 24 && minute < 60 && second < 60)
            .then(|| Self(text.to_owned()))
    }

    /// The current time, truncated to seconds.
    pub fn now() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let secs = i64::try_from(secs).unwrap_or(0);
        let date = Date::from_unix_days(secs.div_euclid(86_400));
        let rem = secs.rem_euclid(86_400);
        Self(format!(
            "{date}T{:02}:{:02}:{:02}Z",
            rem / 3600,
            (rem % 3600) / 60,
            rem % 60
        ))
    }

    pub fn date(&self) -> Date {
        Date::parse(&self.0[..10]).expect("validated timestamp")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid timestamp {text:?}: expected \"YYYY-MM-DDTHH:MM:SSZ\" (UTC)"
            ))
        })
    }
}

impl JsonSchema for Timestamp {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Timestamp".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "format": "date-time", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert!(Date::parse("2026-02-29").is_none());
        assert!(Date::parse("2028-02-29").is_some());
        assert!(Date::parse("2026-13-01").is_none());
        assert!(Date::parse("2026-1-01").is_none());
        assert_eq!(Date::from_unix_days(0).to_string(), "1970-01-01");
        assert_eq!(Date::from_unix_days(20_735).to_string(), "2026-10-09");
        assert!(PartialDate::parse("2026-06").is_some());
        assert!(PartialDate::parse("2026-6").is_none());
        assert!(Timestamp::parse("2026-10-09T19:40:00Z").is_some());
        assert!(Timestamp::parse("2026-10-09T19:40:00+00:00").is_none());
        assert!(Timestamp::parse("2026-10-09 19:40:00Z").is_none());
        assert!(Timestamp::parse(Timestamp::now().as_str()).is_some());
    }
}
