//! OpenRouter `GET /api/v1/models`: list prices for the `openrouter`
//! provider only (they are OpenRouter's prices, not the upstream labs').
//! Batch prices are the separate `<id>:batch` models.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, bail};
use omg_models_catalog::{Decimal, Meter, model::SourceKind};
use serde_json::Value;

use super::{
    OfferingRef, PriceSource,
    observed::{Document, Lookup, ObservedPrice, insert_rate, tiers_from_map},
};

pub const URL: &str = "https://openrouter.ai/api/v1/models";

pub struct OpenRouter {
    document: Document,
    models: HashMap<String, Value>,
}

/// pricing key -> (meter, per-token?)
fn pricing_key(key: &str, claude_cache: bool) -> Option<(Meter, bool)> {
    Some(match key {
        "prompt" => (Meter::InputTokens, true),
        "completion" => (Meter::OutputTokens, true),
        "input_cache_read" => (Meter::CacheReadTokens, true),
        "input_cache_write" if claude_cache => (Meter::CacheWrite5mTokens, true),
        "input_cache_write" => (Meter::CacheWriteTokens, true),
        "input_cache_write_1h" => (Meter::CacheWrite1hTokens, true),
        "web_search" => (Meter::SearchUnits, false),
        "request" => (Meter::Requests, false),
        _ => return None,
    })
}

impl OpenRouter {
    pub fn parse(document: Document) -> anyhow::Result<Self> {
        let value: Value = serde_json::from_slice(&document.bytes).context("OpenRouter JSON")?;
        let Some(Value::Array(data)) = value.get("data") else {
            bail!("OpenRouter response has no data array");
        };
        let models: HashMap<String, Value> = data
            .iter()
            .filter_map(|m| Some((m.get("id")?.as_str()?.to_owned(), m.clone())))
            .collect();
        if models.is_empty() {
            bail!("OpenRouter returned no models");
        }
        Ok(Self { document, models })
    }

    /// Converts one model's `pricing` into a rate card and tiers.
    fn card(
        pricing: &serde_json::Map<String, Value>,
        claude_cache: bool,
        notes: &mut Vec<String>,
    ) -> (
        BTreeMap<Meter, Decimal>,
        BTreeMap<u64, BTreeMap<Meter, Decimal>>,
    ) {
        let mut base = BTreeMap::new();
        let mut tiers = BTreeMap::new();
        let mut apply =
            |values: &serde_json::Map<String, Value>, tier: u64, notes: &mut Vec<String>| {
                for (key, value) in values {
                    if matches!(key.as_str(), "overrides" | "min_prompt_tokens" | "discount") {
                        continue;
                    }
                    let Some((meter, per_token)) = pricing_key(key, claude_cache) else {
                        if value.as_str().is_some_and(|v| v != "0") {
                            notes.push(format!("not mapped: pricing.{key}"));
                        }
                        continue;
                    };
                    // "-1" marks variable pricing (routers): unknown, not free.
                    if value.as_str() == Some("-1") {
                        notes.push(format!(
                            "pricing.{key} is variable (-1); treated as unknown"
                        ));
                        continue;
                    }
                    match Decimal::from_json_value(value) {
                        Ok(rate) => {
                            let rate = if per_token { rate.shift(6) } else { Some(rate) };
                            if let Some(rate) = rate {
                                insert_rate(&mut base, &mut tiers, tier, meter, rate);
                            }
                        }
                        Err(error) => notes.push(format!("pricing.{key}: {error}")),
                    }
                }
            };
        apply(pricing, 0, notes);
        for item in pricing
            .get("overrides")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(item) = item.as_object() else {
                continue;
            };
            match item.get("min_prompt_tokens").and_then(Value::as_u64) {
                Some(threshold) if item.keys().all(|k| k != "utc_days" && k != "utc_start") => {
                    apply(item, threshold, notes);
                }
                _ => notes.push("time-of-day price overrides exist but are not represented".into()),
            }
        }
        notes.dedup();
        (base, tiers)
    }
}

impl PriceSource for OpenRouter {
    fn kind(&self) -> SourceKind {
        SourceKind::Openrouter
    }

    fn document(&self) -> &Document {
        &self.document
    }

    fn lookup(&self, target: &OfferingRef<'_>) -> Lookup<ObservedPrice> {
        let key = target
            .offering
            .sync
            .openrouter
            .clone()
            .unwrap_or_else(|| target.offering.upstream_id.clone());
        let Some(pricing) = self
            .models
            .get(&key)
            .and_then(|m| m.get("pricing"))
            .and_then(Value::as_object)
        else {
            return Lookup::Missing {
                key,
                reason: "not in OpenRouter's model list".into(),
            };
        };
        let claude_cache = target.claude_cache();
        let mut notes = Vec::new();
        let (standard, tiers) = Self::card(pricing, claude_cache, &mut notes);
        let mut observed = ObservedPrice {
            entry_key: key.clone(),
            standard,
            standard_tiers: tiers_from_map(tiers),
            ..ObservedPrice::default()
        };
        if let Some(batch) = self
            .models
            .get(&format!("{key}:batch"))
            .and_then(|m| m.get("pricing"))
            .and_then(Value::as_object)
        {
            let (batch, batch_tiers) = Self::card(batch, claude_cache, &mut notes);
            observed.batch = (!batch.is_empty()).then_some(batch);
            observed.batch_tiers = tiers_from_map(batch_tiers);
            observed.entry_key = format!("{key} (+ {key}:batch)");
        }
        observed.notes = notes;
        Lookup::Found(observed)
    }
}
