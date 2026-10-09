//! BerriAI/litellm `model_prices_and_context_window.json` (MIT): the primary
//! price source for first-party and cloud providers.
//!
//! Keys are exact wire ids (Bedrock `global.`/`us.` prefixes included; they
//! change the price, so they are never stripped). Rates are JSON numbers in
//! USD per token or per unit, parsed from their source text (never `f64`).
//! The key parser is an explicit allowlist; anything unknown is reported.

use std::{collections::BTreeMap, sync::LazyLock};

use anyhow::{Context, bail};
use omg_models_catalog::{Decimal, Meter, model::SourceKind};
use regex::Regex;
use serde_json::{Map, Value};

use super::{
    OfferingRef, PriceSource,
    observed::{Document, Lookup, ObservedPrice, insert_rate, tiers_from_map},
};

pub const URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
pub const REPO: &str = "BerriAI/litellm";
pub const PATH: &str = "model_prices_and_context_window.json";

/// Providers whose `_above_Nk_tokens` threshold is inclusive (`>=`) in
/// LiteLLM's cost calculator (`_INCLUSIVE_THRESHOLD_PROVIDERS`).
const INCLUSIVE_THRESHOLD_PROVIDERS: [&str; 1] = ["xai"];

/// Service-tier suffixes the catalog does not represent.
const SERVICE_TIERS: [&str; 5] = ["_priority", "_flex", "_ultrafast", "_balanced", "_fast"];

static TIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*)_above_(\d+)k_tokens$").expect("valid regex"));

/// How a base key maps onto a meter.
#[derive(Clone, Copy)]
enum Base {
    /// Per token -> per 1M tokens (or characters).
    PerToken(Meter),
    /// `cache_creation_input_token_cost`: 5-minute write for Claude-style
    /// caches, the default write otherwise.
    CacheWrite,
    /// Already per unit (image, second, request).
    PerUnit(Meter),
}

fn base_key(key: &str, mode: &str) -> Option<Base> {
    Some(match key {
        "input_cost_per_token" => Base::PerToken(Meter::InputTokens),
        "output_cost_per_token" => Base::PerToken(Meter::OutputTokens),
        "cache_read_input_token_cost" => Base::PerToken(Meter::CacheReadTokens),
        "cache_creation_input_token_cost" => Base::CacheWrite,
        "cache_creation_input_token_cost_above_1hr" => Base::PerToken(Meter::CacheWrite1hTokens),
        "input_cost_per_character" => Base::PerToken(Meter::InputCharacters),
        "output_cost_per_image" => Base::PerUnit(Meter::OutputImages),
        "input_cost_per_request" => Base::PerUnit(Meter::Requests),
        "input_cost_per_second" | "input_cost_per_audio_per_second"
            if mode == "audio_transcription" =>
        {
            Base::PerUnit(Meter::InputAudioSeconds)
        }
        // The same key is used for video seconds; only speech maps to audio.
        "output_cost_per_second" if mode == "audio_speech" => {
            Base::PerUnit(Meter::OutputAudioSeconds)
        }
        _ => return None,
    })
}

/// The parsed file.
pub struct LiteLlm {
    document: Document,
    entries: Map<String, Value>,
}

impl LiteLlm {
    pub fn parse(document: Document) -> anyhow::Result<Self> {
        let value: Value = serde_json::from_slice(&document.bytes).context("LiteLLM JSON")?;
        let Value::Object(mut entries) = value else {
            bail!("LiteLLM JSON is not an object");
        };
        entries.remove("sample_spec");
        if entries.is_empty() {
            bail!("LiteLLM JSON has no entries");
        }
        Ok(Self { document, entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Converts one entry. `claude_cache` selects the 5-minute meter for
    /// `cache_creation_input_token_cost`.
    pub fn convert(key: &str, entry: &Map<String, Value>, claude_cache: bool) -> ObservedPrice {
        let provider = entry
            .get("litellm_provider")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mode = entry.get("mode").and_then(Value::as_str).unwrap_or("");
        let inclusive = INCLUSIVE_THRESHOLD_PROVIDERS.contains(&provider);
        let mut observed = ObservedPrice {
            entry_key: key.to_owned(),
            cites: entry
                .get("source")
                .and_then(Value::as_str)
                .filter(|s| s.starts_with("https://"))
                .map(str::to_owned),
            ..ObservedPrice::default()
        };
        let mut standard = BTreeMap::new();
        let mut standard_tiers = BTreeMap::new();
        let mut batch = BTreeMap::new();
        let mut batch_tiers = BTreeMap::new();
        let mut service_tiers = false;

        for (field, value) in entry {
            if !field.contains("cost") && field != "tiered_pricing" {
                continue;
            }
            if SERVICE_TIERS.iter().any(|suffix| field.contains(suffix)) {
                service_tiers = true;
                continue;
            }
            let (rest, is_batch) = match field.strip_suffix("_batches") {
                Some(rest) => (rest, true),
                None => (field.as_str(), false),
            };
            let (base, tier) = match TIER.captures(rest) {
                Some(caps) => {
                    let thousands: u64 = caps[2].parse().unwrap_or(0);
                    let mut threshold = thousands * 1000;
                    if inclusive && threshold > 0 {
                        threshold -= 1;
                    }
                    (caps.get(1).map_or("", |m| m.as_str()), threshold)
                }
                None => (rest, 0),
            };
            let Some(mapping) = base_key(base, mode) else {
                observed
                    .notes
                    .push(format!("not mapped: `{field}` (review manually)"));
                continue;
            };
            let rate = match Decimal::from_json_value(value) {
                Ok(rate) => rate,
                Err(error) => {
                    observed.notes.push(format!("`{field}`: {error}"));
                    continue;
                }
            };
            let (meter, rate) = match mapping {
                Base::PerToken(meter) => (meter, rate.shift(6)),
                Base::CacheWrite if claude_cache => (Meter::CacheWrite5mTokens, rate.shift(6)),
                Base::CacheWrite => (Meter::CacheWriteTokens, rate.shift(6)),
                Base::PerUnit(meter) => (meter, Some(rate)),
            };
            let Some(rate) = rate else {
                observed
                    .notes
                    .push(format!("`{field}`: value out of range"));
                continue;
            };
            if tier > 0 && !meter.is_token() {
                observed
                    .notes
                    .push(format!("`{field}`: tiers apply to token meters only"));
                continue;
            }
            if is_batch {
                insert_rate(&mut batch, &mut batch_tiers, tier, meter, rate);
            } else {
                insert_rate(&mut standard, &mut standard_tiers, tier, meter, rate);
            }
        }
        if service_tiers {
            observed
                .notes
                .push("priority/flex/fast service-tier rates exist but are not represented".into());
        }
        observed.standard = standard;
        observed.standard_tiers = tiers_from_map(standard_tiers);
        observed.batch = (!batch.is_empty()).then_some(batch);
        observed.batch_tiers = tiers_from_map(batch_tiers);
        observed
    }
}

impl PriceSource for LiteLlm {
    fn kind(&self) -> SourceKind {
        SourceKind::Litellm
    }

    fn document(&self) -> &Document {
        &self.document
    }

    fn lookup(&self, target: &OfferingRef<'_>) -> Lookup<ObservedPrice> {
        let key = target
            .offering
            .sync
            .litellm
            .clone()
            .unwrap_or_else(|| target.offering.upstream_id.clone());
        let Some(Value::Object(entry)) = self.entries.get(&key) else {
            return Lookup::Missing {
                key,
                reason: "no LiteLLM entry with this key".into(),
            };
        };
        let provider = entry
            .get("litellm_provider")
            .and_then(Value::as_str)
            .unwrap_or("");
        let accepted = &target.provider.sync.litellm_providers;
        if !accepted.is_empty() && !accepted.iter().any(|p| p == provider) {
            return Lookup::Missing {
                key,
                reason: format!(
                    "entry has litellm_provider {provider:?}, expected one of {accepted:?}"
                ),
            };
        }
        let mut observed = Self::convert(&key, entry, target.claude_cache());
        if observed.standard.is_empty() {
            observed.notes.push("entry lists no standard rates".into());
        }
        Lookup::Found(observed)
    }
}
