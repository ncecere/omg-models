//! What a source says about one offering, normalised to catalog meters and
//! units (exact decimals, USD per the meter's unit).

use std::collections::BTreeMap;

use omg_models_catalog::{
    Decimal, Meter,
    model::{RateCard, SourceKind, Tier},
    time::Timestamp,
};

/// A fetched source document.
#[derive(Clone, Debug)]
pub struct Document {
    pub kind: SourceKind,
    /// The URL the bytes were read from (pinned to a commit when possible).
    pub url: String,
    pub fetched_at: Timestamp,
    /// Commit SHA, ETag or publication date.
    pub version: Option<String>,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// A price as reported by a source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObservedPrice {
    pub entry_key: String,
    pub standard: RateCard,
    pub standard_tiers: Vec<Tier>,
    pub batch: Option<RateCard>,
    pub batch_tiers: Vec<Tier>,
    /// A provider page the source cites for this entry.
    pub cites: Option<String>,
    /// Things the parser could not represent (service tiers, unmapped keys).
    pub notes: Vec<String>,
}

/// Adds `rate` for `meter` at `tier` (0 = base) to a card/tier map.
pub fn insert_rate(
    base: &mut RateCard,
    tiers: &mut BTreeMap<u64, RateCard>,
    tier: u64,
    meter: Meter,
    rate: Decimal,
) {
    if tier == 0 {
        base.insert(meter, rate);
    } else {
        tiers.entry(tier).or_default().insert(meter, rate);
    }
}

pub fn tiers_from_map(map: BTreeMap<u64, RateCard>) -> Vec<Tier> {
    map.into_iter()
        .map(|(above_prompt_tokens, rates)| Tier {
            above_prompt_tokens,
            rates,
        })
        .collect()
}

/// Model facts from a metadata source (models.dev).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObservedMetadata {
    pub entry_key: String,
    pub release_date: Option<String>,
    pub knowledge_cutoff: Option<String>,
    pub context: Option<u64>,
    pub output: Option<u64>,
}

/// Result of looking an offering up in a source.
#[derive(Clone, Debug)]
pub enum Lookup<T> {
    Found(T),
    /// The key is not in the source (or not under an accepted provider).
    Missing {
        key: String,
        reason: String,
    },
}
