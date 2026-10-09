//! JSON exports (`omg-models build`). Pure and deterministic: object keys are
//! sorted, lists have a stable order, and nothing depends on the clock.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{
    Catalog, Decimal, Meter, Model,
    model::{Offering, PriceEntry, RateCard, Tier},
};

/// Public base URL of the deployed catalog.
pub const SITE_URL: &str = "https://models.omg.bitop.dev";

/// Relative output path -> file bytes.
pub type Artifacts = BTreeMap<String, Vec<u8>>;

/// Builds every artifact.
pub fn build(catalog: &Catalog) -> Artifacts {
    let mut out = Artifacts::new();
    put(&mut out, "api.json", &models_dev_api(catalog));
    put(&mut out, "api/v1/index.json", &index(catalog));
    put(&mut out, "api/v1/providers.json", &providers(catalog));
    put(&mut out, "api/v1/models.json", &models(catalog));
    put(&mut out, "api/v1/omg-prices.json", &omg_prices(catalog));
    put(&mut out, "api/v1/history.json", &history(catalog));
    out.insert(
        "api/v1/schema.json".to_owned(),
        crate::schema::schemas_json().into_bytes(),
    );
    for id in catalog.providers.keys() {
        put(
            &mut out,
            &format!("api/v1/providers/{id}.json"),
            &provider_detail(catalog, id),
        );
    }
    for model in catalog.models.values() {
        put(
            &mut out,
            &format!("api/v1/models/{}.json", model.file.id),
            &model_detail(catalog, model),
        );
    }
    out
}

fn put(out: &mut Artifacts, path: &str, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(&sorted(value)).expect("JSON serializes");
    bytes.push(b'\n');
    out.insert(path.to_owned(), bytes);
}

/// Rebuilds objects with keys in sorted order, whatever map type serde_json
/// was compiled with.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for key in keys {
                out.insert(key.clone(), sorted(&map[key]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// An exact decimal as a JSON number token (text preserved, no float).
fn number(value: Decimal) -> Value {
    Value::Number(serde_json::from_str(&value.to_string()).expect("decimal is a JSON number"))
}

fn rates_json(card: &RateCard) -> Value {
    card.iter()
        .map(|(meter, rate)| (meter.key().to_owned(), Value::String(rate.to_string())))
        .collect::<Map<_, _>>()
        .into()
}

fn tiers_json(tiers: &[Tier]) -> Value {
    tiers
        .iter()
        .map(|tier| json!({ "above_prompt_tokens": tier.above_prompt_tokens, "rates": rates_json(&tier.rates) }))
        .collect()
}

fn units_json(entry: &PriceEntry) -> Value {
    let mut meters: Vec<Meter> = entry.standard.keys().copied().collect();
    if let Some(batch) = &entry.batch {
        meters.extend(batch.keys().copied());
    }
    meters.sort();
    meters.dedup();
    meters
        .into_iter()
        .map(|m| (m.key().to_owned(), Value::String(m.unit().to_owned())))
        .collect::<Map<_, _>>()
        .into()
}

/// A price entry in the catalog's own API (exact decimal strings).
pub fn price_json(entry: &PriceEntry) -> Value {
    let source = &entry.source;
    let mut value = json!({
        "effective_from": entry.effective_from.to_string(),
        "currency": "USD",
        "standard": rates_json(&entry.standard),
        "standard_tiers": tiers_json(&entry.standard_tiers),
        "batch": entry.batch.as_ref().map(rates_json),
        "batch_tiers": tiers_json(&entry.batch_tiers),
        "not_applicable": entry.not_applicable.iter().map(|m| m.key()).collect::<Vec<_>>(),
        "units": units_json(entry),
        "source": {
            "kind": serde_json::to_value(source.kind).expect("kind"),
            "url": source.url,
            "fetched_at": source.fetched_at.as_str(),
            "version": source.version,
            "sha256": source.sha256,
            "entry_key": source.entry_key,
            "cites": source.cites,
            "carried_over": source.carried_over.iter().map(|m| m.key()).collect::<Vec<_>>(),
        },
    });
    if let Some(notes) = &entry.notes {
        value["notes"] = Value::String(notes.clone());
    }
    value
}

fn limits_json(limits: &crate::model::Limits) -> Value {
    json!({ "context": limits.context, "input": limits.input, "output": limits.output })
}

/// Effective limits of an offering (its own, else the model's).
pub fn offering_limits(model: &Model, offering: &Offering) -> crate::model::Limits {
    let base = model.file.limits;
    let own = offering.limits.unwrap_or_default();
    crate::model::Limits {
        context: own.context.or(base.context),
        input: own.input.or(base.input),
        output: own.output.or(base.output),
    }
}

fn offering_json(catalog: &Catalog, model: &Model, offering: &Offering, history: bool) -> Value {
    let provider_name = catalog
        .providers
        .get(&offering.provider)
        .map(|p| p.file.name.clone());
    let mut value = json!({
        "provider": offering.provider,
        "provider_name": provider_name,
        "upstream_id": offering.upstream_id,
        "label": offering.label,
        "aliases": offering.aliases,
        "limits": limits_json(&offering_limits(model, offering)),
        "current_price": offering.prices.last().map(price_json),
    });
    if history {
        value["price_history"] = offering.prices.iter().map(price_json).collect();
    }
    value
}

fn model_summary(model: &Model) -> Value {
    let file = &model.file;
    json!({
        "id": file.id,
        "name": file.name,
        "vendor": file.vendor,
        "family": file.family,
        "description": file.description,
        "home_provider": model.home_provider,
        "release_date": file.release_date.map(|d| d.to_string()),
        "knowledge_cutoff": file.knowledge_cutoff.as_ref().map(|d| d.as_str().to_owned()),
        "open_weights": file.open_weights,
        "license": file.license,
        "reasoning": file.reasoning,
        "tool_call": file.tool_call,
        "status": serde_json::to_value(file.status).expect("status"),
        "deprecation_date": file.deprecation_date.map(|d| d.to_string()),
        "retirement_date": file.retirement_date.map(|d| d.to_string()),
        "docs": file.docs,
        "modalities": {
            "input": file.modalities.input.iter().map(|m| m.key()).collect::<Vec<_>>(),
            "output": file.modalities.output.iter().map(|m| m.key()).collect::<Vec<_>>(),
        },
        "limits": limits_json(&file.limits),
        "url": format!("{SITE_URL}/models/{}", file.id),
        "api_url": format!("{SITE_URL}/api/v1/models/{}.json", file.id),
    })
}

fn sorted_offerings(model: &Model) -> Vec<&Offering> {
    let mut offerings: Vec<&Offering> = model.file.offerings.iter().collect();
    offerings.sort_by(|a, b| (&a.provider, &a.upstream_id).cmp(&(&b.provider, &b.upstream_id)));
    offerings
}

fn models(catalog: &Catalog) -> Value {
    let items: Vec<Value> = catalog
        .models
        .values()
        .map(|model| {
            let mut value = model_summary(model);
            value["offerings"] = sorted_offerings(model)
                .into_iter()
                .map(|o| offering_json(catalog, model, o, false))
                .collect();
            value
        })
        .collect();
    json!({ "object": "list", "data": items })
}

fn model_detail(catalog: &Catalog, model: &Model) -> Value {
    let mut value = model_summary(model);
    value["offerings"] = sorted_offerings(model)
        .into_iter()
        .map(|o| offering_json(catalog, model, o, true))
        .collect();
    value
}

fn provider_json(catalog: &Catalog, id: &str) -> Value {
    let provider = &catalog.providers[id].file;
    let offerings = catalog
        .models
        .values()
        .flat_map(|m| m.file.offerings.iter())
        .filter(|o| o.provider == id)
        .count();
    json!({
        "id": provider.id,
        "name": provider.name,
        "type": serde_json::to_value(provider.kind).expect("kind"),
        "website": provider.website,
        "api_base": provider.api_base,
        "api_style": serde_json::to_value(provider.api_style).expect("style"),
        "auth": provider.auth,
        "env": provider.env,
        "docs": provider.docs,
        "pricing_page": provider.pricing_page,
        "offering_count": offerings,
        "url": format!("{SITE_URL}/providers/{id}"),
        "api_url": format!("{SITE_URL}/api/v1/providers/{id}.json"),
    })
}

fn providers(catalog: &Catalog) -> Value {
    let items: Vec<Value> = catalog
        .providers
        .keys()
        .map(|id| provider_json(catalog, id))
        .collect();
    json!({ "object": "list", "data": items })
}

fn provider_detail(catalog: &Catalog, id: &str) -> Value {
    let mut value = provider_json(catalog, id);
    let mut rows = Vec::new();
    for model in catalog.models.values() {
        for offering in sorted_offerings(model) {
            if offering.provider == id {
                let mut row = offering_json(catalog, model, offering, false);
                row["model"] = Value::String(model.file.id.clone());
                row["model_name"] = Value::String(model.file.name.clone());
                rows.push(row);
            }
        }
    }
    value["offerings"] = Value::Array(rows);
    value
}

fn history(catalog: &Catalog) -> Value {
    let mut events: Vec<(String, String, String, String, Value)> = Vec::new();
    for model in catalog.models.values() {
        for offering in &model.file.offerings {
            for (index, entry) in offering.prices.iter().enumerate() {
                let mut value = price_json(entry);
                value["model"] = Value::String(model.file.id.clone());
                value["provider"] = Value::String(offering.provider.clone());
                value["upstream_id"] = Value::String(offering.upstream_id.clone());
                value["sequence"] = json!(index);
                value["superseded"] = json!(index + 1 < offering.prices.len());
                events.push((
                    format!("{}|{}", entry.effective_from, entry.source.fetched_at),
                    model.file.id.clone(),
                    offering.provider.clone(),
                    offering.upstream_id.clone(),
                    value,
                ));
            }
        }
    }
    // Newest first, then a stable tiebreak.
    events.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| (&a.1, &a.2, &a.3).cmp(&(&b.1, &b.2, &b.3)))
    });
    json!({ "object": "list", "data": events.into_iter().map(|e| e.4).collect::<Vec<_>>() })
}

/// Latest `fetched_at` across all prices: the data's "last updated" time.
pub fn last_updated(catalog: &Catalog) -> Option<String> {
    catalog
        .models
        .values()
        .flat_map(|m| m.file.offerings.iter())
        .flat_map(|o| o.prices.iter())
        .map(|p| p.source.fetched_at.as_str().to_owned())
        .max()
}

fn index(catalog: &Catalog) -> Value {
    let offerings: usize = catalog
        .models
        .values()
        .map(|m| m.file.offerings.len())
        .sum();
    json!({
        "name": "Open Model Catalog",
        "homepage": SITE_URL,
        "last_updated": last_updated(catalog),
        "counts": { "providers": catalog.providers.len(), "models": catalog.models.len(), "offerings": offerings },
        "license": { "code": "MIT", "data": "MIT", "attribution": format!("{SITE_URL}/about") },
        "endpoints": [
            format!("{SITE_URL}/api.json"),
            format!("{SITE_URL}/api/v1/index.json"),
            format!("{SITE_URL}/api/v1/providers.json"),
            format!("{SITE_URL}/api/v1/providers/{{provider}}.json"),
            format!("{SITE_URL}/api/v1/models.json"),
            format!("{SITE_URL}/api/v1/models/{{model}}.json"),
            format!("{SITE_URL}/api/v1/omg-prices.json"),
            format!("{SITE_URL}/api/v1/history.json"),
        ],
    })
}

// ---------------------------------------------------------------------------
// models.dev-compatible api.json
// ---------------------------------------------------------------------------

fn models_dev_cost(card: &RateCard) -> Map<String, Value> {
    let mut cost = Map::new();
    let mut set = |key: &str, meter: Meter| {
        if let Some(rate) = card.get(&meter) {
            cost.insert(key.to_owned(), number(*rate));
        }
    };
    set("input", Meter::InputTokens);
    set("output", Meter::OutputTokens);
    set("cache_read", Meter::CacheReadTokens);
    // models.dev has one cache_write: the 5-minute write for Claude-style
    // caches, otherwise the default write.
    if card.contains_key(&Meter::CacheWrite5mTokens) {
        set("cache_write", Meter::CacheWrite5mTokens);
    } else {
        set("cache_write", Meter::CacheWriteTokens);
    }
    cost
}

fn models_dev_api(catalog: &Catalog) -> Value {
    let mut providers = Map::new();
    for (id, provider) in &catalog.providers {
        let mut models = Map::new();
        for model in catalog.models.values() {
            for offering in model.file.offerings.iter().filter(|o| &o.provider == id) {
                let file = &model.file;
                let limits = offering_limits(model, offering);
                let mut entry = json!({
                    "id": offering.upstream_id,
                    "name": match &offering.label {
                        Some(label) => format!("{} ({label})", file.name),
                        None => file.name.clone(),
                    },
                    "catalog_id": file.id,
                    "family": file.family,
                    "open_weights": file.open_weights,
                    "modalities": {
                        "input": file.modalities.input.iter().map(|m| m.key()).collect::<Vec<_>>(),
                        "output": file.modalities.output.iter().map(|m| m.key()).collect::<Vec<_>>(),
                    },
                });
                let object = entry.as_object_mut().expect("object");
                let mut limit = Map::new();
                if let Some(context) = limits.context {
                    limit.insert("context".into(), json!(context));
                }
                if let Some(input) = limits.input {
                    limit.insert("input".into(), json!(input));
                }
                if let Some(output) = limits.output {
                    limit.insert("output".into(), json!(output));
                }
                object.insert("limit".into(), Value::Object(limit));
                for (key, value) in [
                    ("release_date", file.release_date.map(|d| d.to_string())),
                    (
                        "knowledge",
                        file.knowledge_cutoff
                            .as_ref()
                            .map(|d| d.as_str().to_owned()),
                    ),
                ] {
                    if let Some(value) = value {
                        object.insert(key.into(), Value::String(value));
                    }
                }
                if let Some(reasoning) = file.reasoning {
                    object.insert("reasoning".into(), Value::Bool(reasoning));
                }
                if let Some(tool_call) = file.tool_call {
                    object.insert("tool_call".into(), Value::Bool(tool_call));
                }
                if file.status != crate::model::Status::Active {
                    object.insert(
                        "status".into(),
                        serde_json::to_value(file.status).expect("status"),
                    );
                }
                if let Some(price) = offering.prices.last() {
                    let mut cost = models_dev_cost(&price.standard);
                    if !price.standard_tiers.is_empty() {
                        let tiers: Vec<Value> = price
                            .standard_tiers
                            .iter()
                            .map(|tier| {
                                let mut tier_cost = models_dev_cost(&tier.rates);
                                tier_cost.insert(
                                    "tier".into(),
                                    json!({ "type": "context", "size": tier.above_prompt_tokens }),
                                );
                                Value::Object(tier_cost)
                            })
                            .collect();
                        cost.insert("tiers".into(), Value::Array(tiers));
                    }
                    if !cost.is_empty() {
                        object.insert("cost".into(), Value::Object(cost));
                    }
                    object.insert(
                        "last_updated".into(),
                        Value::String(price.effective_from.to_string()),
                    );
                }
                models.insert(offering.upstream_id.clone(), entry);
            }
        }
        let file = &provider.file;
        let mut value = json!({ "id": id, "name": file.name, "env": file.env, "models": models });
        let object = value.as_object_mut().expect("object");
        if let Some(api) = &file.api_base {
            object.insert("api".into(), Value::String(api.clone()));
        }
        if let Some(doc) = &file.docs {
            object.insert("doc".into(), Value::String(doc.clone()));
        }
        providers.insert(id.clone(), value);
    }
    Value::Object(providers)
}

// ---------------------------------------------------------------------------
// OMG Pricing v3 export
// ---------------------------------------------------------------------------

/// OMG v3 price lines for one rate card plus its tiers. Returns the lines
/// and whether any value was rounded up to a whole micro-USD.
pub fn omg_lines(card: &RateCard, tiers: &[Tier], not_applicable: &[Meter]) -> (Vec<Value>, bool) {
    let mut lines = Vec::new();
    let mut any_rounded = false;
    let mut line = |meter: Meter, rate: Decimal, min_prompt_tokens: Option<u64>| {
        let (micro, exact) = rate.to_micro_ceil().expect("validated rates fit");
        any_rounded |= !exact;
        let mut value = json!({
            "meter": meter.omg_meter(),
            "microusd_per_batch": micro.to_string(),
            "batch": meter.omg_batch(),
            "unit_label": meter.omg_unit_label(),
            "sku_label": meter.label(),
            "source_usd_per_unit": rate.to_string(),
        });
        if let Some(tokens) = min_prompt_tokens {
            value["min_prompt_tokens"] = json!(tokens);
        }
        if !exact {
            value["rounded_up"] = Value::Bool(true);
        }
        lines.push(value);
    };
    for (meter, rate) in card {
        line(*meter, *rate, None);
    }
    for tier in tiers {
        for (meter, rate) in &tier.rates {
            line(*meter, *rate, Some(tier.above_prompt_tokens));
        }
    }
    for meter in not_applicable {
        lines.push(json!({ "meter": meter.omg_meter(), "not_applicable": true }));
    }
    (lines, any_rounded)
}

/// The set of (meter, tier) keys a card covers, for batch/standard parity.
fn coverage(card: &RateCard, tiers: &[Tier]) -> Vec<(Meter, u64)> {
    let mut keys: Vec<(Meter, u64)> = card.keys().map(|m| (*m, 0)).collect();
    for tier in tiers {
        keys.extend(tier.rates.keys().map(|m| (*m, tier.above_prompt_tokens)));
    }
    keys.sort();
    keys
}

fn omg_prices(catalog: &Catalog) -> Value {
    let mut offerings = Vec::new();
    for model in catalog.models.values() {
        for offering in sorted_offerings(model) {
            let Some(price) = offering.prices.last() else {
                continue;
            };
            let (lines, mut rounded) = omg_lines(
                &price.standard,
                &price.standard_tiers,
                &price.not_applicable,
            );
            let batch = price.batch.as_ref().map(|batch| {
                let (lines, batch_rounded) =
                    omg_lines(batch, &price.batch_tiers, &price.not_applicable);
                rounded |= batch_rounded;
                lines
            });
            // OMG requires batch lines to cover exactly the same meters/tiers.
            let batch_complete = price.batch.as_ref().map(|batch| {
                coverage(batch, &price.batch_tiers)
                    == coverage(&price.standard, &price.standard_tiers)
            });
            let limits = offering_limits(model, offering);
            offerings.push(json!({
                "model": model.file.id,
                "model_name": model.file.name,
                "provider": offering.provider,
                "upstream_id": offering.upstream_id,
                "aliases": offering.aliases,
                "label": offering.label,
                "effective_from": price.effective_from.to_string(),
                "price_lines": lines,
                "batch_price_lines": batch,
                "batch_complete": batch_complete,
                "rounded_up": rounded,
                "limits": limits_json(&limits),
                "provenance": price_json(price)["source"].clone(),
            }));
        }
    }
    json!({
        "format": "omg-prices",
        "format_version": 1,
        "pricing_version": 3,
        "currency": "USD",
        "note": "Integer micro-USD per OMG batch, converted exactly from the catalog's decimal USD rates; values that are not a whole micro-USD are rounded up and flagged rounded_up. Prices are list-price estimates, not invoices. A missing meter is unknown, never free.",
        "last_updated": last_updated(catalog),
        "data": offerings,
    })
}
