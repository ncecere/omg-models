//! pydantic/genai-prices v2 data (MIT): the corroboration source. Its
//! values never overwrite the primary source; differences are reported as
//! disagreements and block auto-merge for the affected change.
//!
//! The v1 `prices/data.json` is frozen; v2 lives at
//! `prices/new_data/v2/data.json`. Prices are USD per 1M tokens (`*_mtok`)
//! or per 1,000 units (`*_kcount`). Models are found with the same match
//! rules the genai-prices libraries evaluate, first match wins.

use std::collections::BTreeMap;

use anyhow::{Context, bail};
use omg_models_catalog::{Decimal, Meter, model::SourceKind, time::Date};
use regex::Regex;
use serde_json::Value;

use super::{
    OfferingRef, PriceSource,
    observed::{Document, Lookup, ObservedPrice, insert_rate, tiers_from_map},
};

pub const URL: &str =
    "https://raw.githubusercontent.com/pydantic/genai-prices/main/prices/new_data/v2/data.json";
pub const REPO: &str = "pydantic/genai-prices";
pub const PATH: &str = "prices/new_data/v2/data.json";

pub struct GenaiPrices {
    document: Document,
    providers: Vec<Value>,
    today: Date,
}

/// Evaluates a genai-prices match clause against a model id.
pub fn matches(clause: &Value, id: &str) -> bool {
    let Value::Object(map) = clause else {
        return false;
    };
    map.iter().all(|(op, arg)| match (op.as_str(), arg) {
        ("equals", Value::String(s)) => id == s,
        ("starts_with", Value::String(s)) => id.starts_with(s.as_str()),
        ("ends_with", Value::String(s)) => id.ends_with(s.as_str()),
        ("contains", Value::String(s)) => id.contains(s.as_str()),
        ("regex", Value::String(s)) => Regex::new(s).is_ok_and(|re| re.is_match(id)),
        ("or", Value::Array(items)) => items.iter().any(|c| matches(c, id)),
        ("and", Value::Array(items)) => items.iter().all(|c| matches(c, id)),
        ("not", inner) => !matches(inner, id),
        _ => false,
    })
}

fn price_key(key: &str, claude_cache: bool) -> Option<(Meter, i8)> {
    // (meter, power of ten to apply: 0 = per 1M already, -3 = per 1K units)
    Some(match key {
        "input_mtok" => (Meter::InputTokens, 0),
        "output_mtok" => (Meter::OutputTokens, 0),
        "cache_read_mtok" => (Meter::CacheReadTokens, 0),
        "cache_write_mtok" if claude_cache => (Meter::CacheWrite5mTokens, 0),
        "cache_write_mtok" => (Meter::CacheWriteTokens, 0),
        "cache_write_5m_mtok" => (Meter::CacheWrite5mTokens, 0),
        "cache_write_1h_mtok" => (Meter::CacheWrite1hTokens, 0),
        "web_searches_kcount" => (Meter::SearchUnits, -3),
        "requests_kcount" => (Meter::Requests, -3),
        _ => return None,
    })
}

impl GenaiPrices {
    pub fn parse(document: Document, today: Date) -> anyhow::Result<Self> {
        let value: Value = serde_json::from_slice(&document.bytes).context("genai-prices JSON")?;
        let Value::Array(providers) = value else {
            bail!("genai-prices v2 data is not a list of providers");
        };
        if providers.is_empty() {
            bail!("genai-prices v2 data has no providers");
        }
        Ok(Self {
            document,
            providers,
            today,
        })
    }

    /// Picks the price object in force today from a `prices` value.
    fn current_prices<'a>(&self, prices: &'a Value, notes: &mut Vec<String>) -> Option<&'a Value> {
        match prices {
            Value::Object(_) => Some(prices),
            Value::Array(items) => {
                let mut best: Option<(String, &Value)> = None;
                for item in items {
                    let constraint = item.get("constraint");
                    if constraint.and_then(|c| c.get("start_time")).is_some() {
                        notes.push(
                            "genai-prices has time-of-day prices; compared with the all-day price"
                                .into(),
                        );
                        continue;
                    }
                    let start = constraint
                        .and_then(|c| c.get("start_date"))
                        .and_then(Value::as_str)
                        .unwrap_or("0000-00-00")
                        .to_owned();
                    if start.as_str() > self.today.to_string().as_str() {
                        continue;
                    }
                    if best.as_ref().is_none_or(|(s, _)| start >= *s) {
                        best = Some((start, item.get("prices")?));
                    }
                }
                best.map(|(_, prices)| prices)
            }
            _ => None,
        }
    }

    pub fn convert(&self, entry_key: &str, model: &Value, claude_cache: bool) -> ObservedPrice {
        let mut observed = ObservedPrice {
            entry_key: entry_key.to_owned(),
            ..ObservedPrice::default()
        };
        let mut notes = Vec::new();
        let Some(Value::Object(prices)) = model
            .get("prices")
            .and_then(|p| self.current_prices(p, &mut notes))
        else {
            observed.notes.push("no current prices".into());
            return observed;
        };
        let mut standard = BTreeMap::new();
        let mut tiers = BTreeMap::new();
        for (key, value) in prices {
            let Some((meter, power)) = price_key(key, claude_cache) else {
                notes.push(format!("not compared: `{key}`"));
                continue;
            };
            let scale = |rate: Decimal| {
                if power < 0 {
                    rate.unshift(3)
                } else {
                    Some(rate)
                }
            };
            let (base, tier_values) = match value {
                Value::Object(obj) => (obj.get("base"), obj.get("tiers").and_then(Value::as_array)),
                other => (Some(other), None),
            };
            if let Some(rate) = base
                .and_then(|b| Decimal::from_json_value(b).ok())
                .and_then(scale)
            {
                insert_rate(&mut standard, &mut tiers, 0, meter, rate);
            }
            for tier in tier_values.into_iter().flatten() {
                let start = tier.get("start").and_then(Value::as_u64);
                let rate = tier
                    .get("price")
                    .and_then(|p| Decimal::from_json_value(p).ok())
                    .and_then(scale);
                if let (Some(start), Some(rate)) = (start, rate) {
                    // Cliff tiers: `start` maps to "prompt > start".
                    insert_rate(&mut standard, &mut tiers, start, meter, rate);
                }
            }
        }
        notes.dedup();
        observed.notes = notes;
        observed.standard = standard;
        observed.standard_tiers = tiers_from_map(tiers);
        observed
    }
}

impl PriceSource for GenaiPrices {
    fn kind(&self) -> SourceKind {
        SourceKind::GenaiPrices
    }

    fn document(&self) -> &Document {
        &self.document
    }

    fn lookup(&self, target: &OfferingRef<'_>) -> Lookup<ObservedPrice> {
        let id = target
            .offering
            .sync
            .genai_prices
            .clone()
            .unwrap_or_else(|| target.offering.upstream_id.clone());
        let Some(provider_id) = target.provider.sync.genai_prices_provider.as_deref() else {
            return Lookup::Missing {
                key: id,
                reason: "provider has no genai_prices_provider".into(),
            };
        };
        let Some(provider) = self
            .providers
            .iter()
            .find(|p| p.get("id").and_then(Value::as_str) == Some(provider_id))
        else {
            return Lookup::Missing {
                key: format!("{provider_id}/{id}"),
                reason: format!("genai-prices has no provider {provider_id:?}"),
            };
        };
        let models = provider.get("models").and_then(Value::as_array);
        let found = models.into_iter().flatten().find(|model| {
            model
                .get("match")
                .is_some_and(|clause| matches(clause, &id))
        });
        match found {
            Some(model) => {
                let model_id = model.get("id").and_then(Value::as_str).unwrap_or("?");
                Lookup::Found(self.convert(
                    &format!("{provider_id}/{model_id}"),
                    model,
                    target.claude_cache(),
                ))
            }
            None => Lookup::Missing {
                key: format!("{provider_id}/{id}"),
                reason: "no genai-prices model matches this id".into(),
            },
        }
    }
}
