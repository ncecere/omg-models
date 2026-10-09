//! Semantic validation on top of the schema (serde already rejected unknown
//! fields, bad types, malformed decimals and dates).

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Issue,
    load::Catalog,
    meter::Meter,
    model::{
        Limits, ModelFile, Offering, PriceEntry, ProviderFile, ProviderKind, RateCard, SourceKind,
        Status, Tier,
    },
};

fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'_')
        })
}

fn https(url: &str) -> bool {
    url.strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty() && !rest.contains(char::is_whitespace))
}

/// Validates a loaded catalog. Returns every problem found; errors fail
/// `omg-models validate`, warnings are printed.
pub fn validate(catalog: &Catalog) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (dir, provider) in &catalog.providers {
        validate_provider(dir, &provider.file, &provider.path, &mut issues);
    }

    // (provider, upstream id or alias) -> model path, for global uniqueness.
    let mut wire_ids: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut served: BTreeSet<&str> = BTreeSet::new();
    for model in catalog.models.values() {
        validate_model(
            catalog,
            &model.file,
            &model.path,
            &model.home_provider,
            &mut issues,
        );
        for (index, offering) in model.file.offerings.iter().enumerate() {
            served.insert(offering.provider.as_str());
            for id in std::iter::once(&offering.upstream_id).chain(&offering.aliases) {
                let key = (offering.provider.clone(), id.clone());
                if let Some(other) = wire_ids.get(&key) {
                    issues.push(Issue::error_at(
                        &model.path,
                        format!("offerings[{index}]"),
                        format!(
                            "{}:{id} is already used by an offering in {other}; one upstream id maps to one catalog model",
                            offering.provider
                        ),
                    ));
                } else {
                    wire_ids.insert(key, model.path.clone());
                }
            }
        }
    }
    for (id, provider) in &catalog.providers {
        if !served.contains(id.as_str()) {
            issues.push(Issue::warning(
                &provider.path,
                "provider has no model offerings yet",
            ));
        }
    }
    issues
}

fn validate_provider(dir: &str, file: &ProviderFile, path: &str, issues: &mut Vec<Issue>) {
    if file.id != dir {
        issues.push(Issue::error(
            path,
            format!("id {:?} must equal the directory name {dir:?}", file.id),
        ));
    }
    if !valid_id(&file.id) || file.id.contains(['.', '_']) {
        issues.push(Issue::error(
            path,
            format!("id {:?} must match [a-z0-9][a-z0-9-]*", file.id),
        ));
    }
    if file.name.trim().is_empty() {
        issues.push(Issue::error(path, "name must not be empty"));
    }
    if !https(&file.website) {
        issues.push(Issue::error(path, "website must be an https:// URL"));
    }
    for (field, value) in [("docs", &file.docs), ("pricing_page", &file.pricing_page)] {
        if let Some(url) = value
            && !https(url)
        {
            issues.push(Issue::error(
                path,
                format!("{field} must be an https:// URL"),
            ));
        }
    }
    if let Some(base) = &file.api_base {
        let http_ok = file.kind == ProviderKind::SelfHosted && base.starts_with("http://");
        if !https(base) && !http_ok {
            issues.push(Issue::error(
                path,
                "api_base must be https:// (http:// is allowed only for self-hosted providers)",
            ));
        }
    }
    for var in &file.env {
        if var.is_empty()
            || !var
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            issues.push(Issue::error(
                path,
                format!("env {var:?} must be UPPER_SNAKE_CASE"),
            ));
        }
    }
}

fn validate_limits(limits: &Limits, path: &str, at: &str, issues: &mut Vec<Issue>) {
    for (name, value) in [
        ("context", limits.context),
        ("input", limits.input),
        ("output", limits.output),
    ] {
        if value == Some(0) {
            issues.push(Issue::error_at(
                path,
                at,
                format!("limits.{name} must be positive (omit unknown limits)"),
            ));
        }
    }
    if let Some(context) = limits.context {
        for (name, value) in [("input", limits.input), ("output", limits.output)] {
            if value.is_some_and(|v| v > context) {
                issues.push(Issue::error_at(
                    path,
                    at,
                    format!("limits.{name} exceeds limits.context"),
                ));
            }
        }
    }
}

fn validate_model(
    catalog: &Catalog,
    file: &ModelFile,
    path: &str,
    home: &str,
    issues: &mut Vec<Issue>,
) {
    if !valid_id(&file.id) {
        issues.push(Issue::error(
            path,
            format!("id {:?} must match [a-z0-9][a-z0-9._-]* (max 128)", file.id),
        ));
    }
    if file.name.trim().is_empty() || file.vendor.trim().is_empty() {
        issues.push(Issue::error(path, "name and vendor must not be empty"));
    }
    if let Some(url) = &file.docs
        && !https(url)
    {
        issues.push(Issue::error(path, "docs must be an https:// URL"));
    }
    if file.modalities.input.is_empty() || file.modalities.output.is_empty() {
        issues.push(Issue::error(
            path,
            "modalities.input and modalities.output must each list at least one modality",
        ));
    }
    for (name, list) in [
        ("input", &file.modalities.input),
        ("output", &file.modalities.output),
    ] {
        let unique: BTreeSet<_> = list.iter().collect();
        if unique.len() != list.len() {
            issues.push(Issue::error(
                path,
                format!("modalities.{name} has duplicates"),
            ));
        }
    }
    validate_limits(&file.limits, path, "limits", issues);

    match file.status {
        Status::Deprecated if file.deprecation_date.is_none() => issues.push(Issue::error(
            path,
            "status \"deprecated\" requires deprecation_date",
        )),
        Status::Retired if file.retirement_date.is_none() => issues.push(Issue::error(
            path,
            "status \"retired\" requires retirement_date",
        )),
        _ => {}
    }
    if let (Some(deprecated), Some(retired)) = (file.deprecation_date, file.retirement_date)
        && deprecated > retired
    {
        issues.push(Issue::error(
            path,
            "deprecation_date must not be after retirement_date",
        ));
    }
    if let (Some(release), Some(deprecated)) = (file.release_date, file.deprecation_date)
        && release > deprecated
    {
        issues.push(Issue::error(
            path,
            "release_date must not be after deprecation_date",
        ));
    }

    if file.offerings.is_empty() {
        issues.push(Issue::error(
            path,
            "a model needs at least one offering (provider + upstream_id)",
        ));
    }
    if !file.offerings.iter().any(|o| o.provider == home) {
        issues.push(Issue::error(
            path,
            format!("the model lives under providers/{home}/ but has no offering with provider = {home:?}"),
        ));
    }
    let mut seen = BTreeSet::new();
    for (index, offering) in file.offerings.iter().enumerate() {
        let at = format!(
            "offerings[{index}] ({}:{})",
            offering.provider, offering.upstream_id
        );
        if !catalog.providers.contains_key(&offering.provider) {
            issues.push(Issue::error_at(
                path,
                &at,
                format!(
                    "unknown provider {:?} (no providers/{}/provider.toml)",
                    offering.provider, offering.provider
                ),
            ));
        }
        if !seen.insert((&offering.provider, &offering.upstream_id)) {
            issues.push(Issue::error_at(path, &at, "duplicate offering"));
        }
        validate_offering(offering, path, &at, issues);
    }
}

fn validate_offering(offering: &Offering, path: &str, at: &str, issues: &mut Vec<Issue>) {
    if offering.upstream_id.trim().is_empty() || offering.upstream_id.contains(char::is_whitespace)
    {
        issues.push(Issue::error_at(
            path,
            at,
            "upstream_id must be non-empty without whitespace",
        ));
    }
    if let Some(limits) = &offering.limits {
        validate_limits(limits, path, &format!("{at}.limits"), issues);
    }
    for (index, entry) in offering.prices.iter().enumerate() {
        let entry_at = format!(
            "{at}.prices[{index}] (effective_from {})",
            entry.effective_from
        );
        validate_price(entry, path, &entry_at, issues);
        if index > 0 {
            let previous = &offering.prices[index - 1];
            if entry.effective_from < previous.effective_from {
                issues.push(Issue::error_at(
                    path,
                    &entry_at,
                    format!(
                        "price history must be in date order (append-only): {} comes after {}",
                        entry.effective_from, previous.effective_from
                    ),
                ));
            } else if entry.effective_from == previous.effective_from
                && entry.source.fetched_at <= previous.source.fetched_at
            {
                issues.push(Issue::error_at(
                    path,
                    &entry_at,
                    "two entries on the same effective_from must have increasing source.fetched_at",
                ));
            }
        }
    }
}

fn validate_card(
    card: &RateCard,
    tiers: &[Tier],
    name: &str,
    path: &str,
    at: &str,
    issues: &mut Vec<Issue>,
) {
    for (meter, rate) in card {
        check_micro(*meter, *rate, &format!("{name}.{meter}"), path, at, issues);
    }
    let mut last = 0;
    for (index, tier) in tiers.iter().enumerate() {
        let tier_at = format!("{name}_tiers[{index}]");
        if tier.above_prompt_tokens == 0 || tier.above_prompt_tokens <= last {
            issues.push(Issue::error_at(
                path,
                at,
                format!("{tier_at}: above_prompt_tokens must be positive and strictly ascending"),
            ));
        }
        last = tier.above_prompt_tokens;
        if tier.rates.is_empty() {
            issues.push(Issue::error_at(
                path,
                at,
                format!("{tier_at}: rates must not be empty"),
            ));
        }
        for (meter, rate) in &tier.rates {
            if !card.contains_key(meter) {
                issues.push(Issue::error_at(
                    path,
                    at,
                    format!("{tier_at}: {meter} has a tier rate but no base rate in {name}"),
                ));
            }
            if !meter.is_token() {
                issues.push(Issue::error_at(
                    path,
                    at,
                    format!("{tier_at}: prompt-size tiers apply to token meters only, not {meter}"),
                ));
            }
            check_micro(
                *meter,
                *rate,
                &format!("{tier_at}.{meter}"),
                path,
                at,
                issues,
            );
        }
    }
}

fn check_micro(
    meter: Meter,
    rate: crate::Decimal,
    field: &str,
    path: &str,
    at: &str,
    issues: &mut Vec<Issue>,
) {
    match rate.to_micro_ceil() {
        None => issues.push(Issue::error_at(path, at, format!("{field}: {rate} is too large"))),
        Some((micro, false)) => issues.push(Issue::warning_at(
            path,
            at,
            format!(
                "{field}: {rate} USD {} is not a whole micro-USD per OMG batch; the OMG export rounds it up to {micro} and flags it",
                meter.unit()
            ),
        )),
        Some((_, true)) => {}
    }
}

fn validate_price(entry: &PriceEntry, path: &str, at: &str, issues: &mut Vec<Issue>) {
    if entry.standard.is_empty() && entry.not_applicable.is_empty() {
        issues.push(Issue::error_at(
            path,
            at,
            "standard must list at least one meter (omit the entry when no price is known)",
        ));
    }
    validate_card(
        &entry.standard,
        &entry.standard_tiers,
        "standard",
        path,
        at,
        issues,
    );
    match &entry.batch {
        Some(batch) => {
            if batch.is_empty() {
                issues.push(Issue::error_at(
                    path,
                    at,
                    "batch must not be empty when present",
                ));
            }
            validate_card(batch, &entry.batch_tiers, "batch", path, at, issues);
        }
        None if !entry.batch_tiers.is_empty() => {
            issues.push(Issue::error_at(path, at, "batch_tiers requires batch"));
        }
        None => {}
    }
    let mut na = BTreeSet::new();
    for meter in &entry.not_applicable {
        if !na.insert(meter) {
            issues.push(Issue::error_at(
                path,
                at,
                format!("not_applicable lists {meter} twice"),
            ));
        }
        let priced = entry.standard.contains_key(meter)
            || entry.batch.as_ref().is_some_and(|b| b.contains_key(meter));
        if priced {
            issues.push(Issue::error_at(
                path,
                at,
                format!("{meter} is both priced and not_applicable"),
            ));
        }
    }

    let source = &entry.source;
    if !https(&source.url) {
        issues.push(Issue::error_at(
            path,
            at,
            "source.url must be an https:// URL",
        ));
    }
    if let Some(cites) = &source.cites
        && !https(cites)
    {
        issues.push(Issue::error_at(
            path,
            at,
            "source.cites must be an https:// URL",
        ));
    }
    if source.kind.is_dataset() && source.entry_key.as_deref().is_none_or(str::is_empty) {
        issues.push(Issue::error_at(
            path,
            at,
            format!(
                "source.kind {:?} requires source.entry_key",
                source.kind.label()
            ),
        ));
    }
    if matches!(source.kind, SourceKind::Litellm | SourceKind::GenaiPrices)
        && (source.version.is_none() || source.sha256.is_none())
    {
        issues.push(Issue::error_at(
            path,
            at,
            "LiteLLM and genai-prices provenance requires source.version (commit) and source.sha256",
        ));
    }
    if let Some(sha) = &source.sha256
        && (sha.len() != 64
            || !sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    {
        issues.push(Issue::error_at(
            path,
            at,
            "source.sha256 must be 64 lowercase hex digits",
        ));
    }
    if source.kind == SourceKind::Manual && entry.notes.as_deref().is_none_or(str::is_empty) {
        issues.push(Issue::error_at(
            path,
            at,
            "manual prices need notes explaining where they came from",
        ));
    }
    for meter in &source.carried_over {
        if !entry.standard.contains_key(meter)
            && !entry.batch.as_ref().is_some_and(|b| b.contains_key(meter))
        {
            issues.push(Issue::error_at(
                path,
                at,
                format!("source.carried_over lists {meter}, which this entry does not price"),
            ));
        }
    }
}
