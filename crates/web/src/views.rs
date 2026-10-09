//! Owned, display-ready data for pages (built before `view!` so templates
//! don't borrow request state).

use omg_models_catalog::{
    Catalog, Meter, Model,
    export::offering_limits,
    model::{ApiStyle, Offering, PriceEntry, ProviderFile},
};

use crate::format;

#[derive(Clone, Debug)]
pub struct PriceRow {
    pub label: &'static str,
    pub unit: &'static str,
    pub standard: String,
    pub batch: String,
}

#[derive(Clone, Debug)]
pub struct PriceGroup {
    /// `None` for base rates, else "Prompt > 100K tokens".
    pub heading: Option<String>,
    pub rows: Vec<PriceRow>,
}

#[derive(Clone, Debug)]
pub struct SourceView {
    pub kind: &'static str,
    pub url: String,
    pub fetched: String,
    pub version: Option<String>,
    pub entry_key: Option<String>,
    pub cites: Option<String>,
    pub carried_over: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub struct PriceView {
    pub effective_from: String,
    pub groups: Vec<PriceGroup>,
    pub has_batch: bool,
    pub not_applicable: Vec<&'static str>,
    pub source: SourceView,
    pub notes: Option<String>,
}

const DASH: &str = "\u{2014}";

fn cell(card: Option<&omg_models_catalog::model::RateCard>, meter: Meter) -> String {
    card.and_then(|c| c.get(&meter))
        .map_or_else(|| DASH.to_owned(), |v| format::usd(*v))
}

pub fn price_view(entry: &PriceEntry) -> PriceView {
    let has_batch = entry.batch.is_some();
    let mut groups = Vec::new();
    let meters = |a: &omg_models_catalog::model::RateCard,
                  b: Option<&omg_models_catalog::model::RateCard>| {
        let mut list: Vec<Meter> = a
            .keys()
            .chain(b.into_iter().flat_map(|b| b.keys()))
            .copied()
            .collect();
        list.sort();
        list.dedup();
        list
    };
    groups.push(PriceGroup {
        heading: None,
        rows: meters(&entry.standard, entry.batch.as_ref())
            .into_iter()
            .map(|meter| PriceRow {
                label: meter.label(),
                unit: meter.unit(),
                standard: cell(Some(&entry.standard), meter),
                batch: cell(entry.batch.as_ref(), meter),
            })
            .collect(),
    });
    let mut thresholds: Vec<u64> = entry
        .standard_tiers
        .iter()
        .chain(&entry.batch_tiers)
        .map(|t| t.above_prompt_tokens)
        .collect();
    thresholds.sort_unstable();
    thresholds.dedup();
    for threshold in thresholds {
        let standard = entry
            .standard_tiers
            .iter()
            .find(|t| t.above_prompt_tokens == threshold)
            .map(|t| &t.rates);
        let batch = entry
            .batch_tiers
            .iter()
            .find(|t| t.above_prompt_tokens == threshold)
            .map(|t| &t.rates);
        let empty = omg_models_catalog::model::RateCard::new();
        groups.push(PriceGroup {
            heading: Some(format!(
                "Prompts over {} tokens (whole request)",
                format::thousands(threshold)
            )),
            rows: meters(standard.unwrap_or(&empty), batch)
                .into_iter()
                .map(|meter| PriceRow {
                    label: meter.label(),
                    unit: meter.unit(),
                    standard: cell(standard, meter),
                    batch: cell(batch, meter),
                })
                .collect(),
        });
    }
    let source = &entry.source;
    PriceView {
        effective_from: entry.effective_from.to_string(),
        groups,
        has_batch,
        not_applicable: entry.not_applicable.iter().map(|m| m.label()).collect(),
        source: SourceView {
            kind: source.kind.label(),
            url: source.url.clone(),
            fetched: source
                .fetched_at
                .as_str()
                .replace('T', " ")
                .replace('Z', " UTC"),
            version: source.version.clone(),
            entry_key: source.entry_key.clone(),
            cites: source.cites.clone(),
            carried_over: source.carried_over.iter().map(|m| m.key()).collect(),
        },
        notes: entry.notes.clone(),
    }
}

#[derive(Clone, Debug)]
pub struct HistoryRow {
    pub effective_from: String,
    pub input: String,
    pub output: String,
    pub source_kind: &'static str,
    pub source_url: String,
    pub current: bool,
}

#[derive(Clone, Debug)]
pub struct OfferingView {
    pub provider_id: String,
    pub provider_name: String,
    pub label: Option<String>,
    pub upstream_id: String,
    pub aliases: Vec<String>,
    pub context: Option<u64>,
    pub max_output: Option<u64>,
    pub price: Option<PriceView>,
    pub history: Vec<HistoryRow>,
    pub example: String,
}

/// A shell example for calling `upstream_id` on this provider.
pub fn api_example(provider: Option<&ProviderFile>, upstream_id: &str) -> String {
    let Some(provider) = provider else {
        return String::new();
    };
    let base = provider.api_base.clone().unwrap_or_default();
    let key = provider
        .env
        .first()
        .cloned()
        .unwrap_or_else(|| "API_KEY".into());
    match provider.api_style {
        ApiStyle::OpenaiChat => format!(
            "curl {base}/chat/completions \\\n  -H \"Authorization: Bearer ${key}\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{{\"model\": \"{upstream_id}\", \"messages\": [{{\"role\": \"user\", \"content\": \"Hello\"}}]}}'"
        ),
        ApiStyle::OpenaiResponses => format!(
            "curl {base}/responses \\\n  -H \"Authorization: Bearer ${key}\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{{\"model\": \"{upstream_id}\", \"input\": \"Hello\"}}'"
        ),
        ApiStyle::AnthropicMessages => format!(
            "curl {base}/messages \\\n  -H \"x-api-key: ${key}\" \\\n  -H \"anthropic-version: 2023-06-01\" \\\n  -H \"content-type: application/json\" \\\n  -d '{{\"model\": \"{upstream_id}\", \"max_tokens\": 1024, \"messages\": [{{\"role\": \"user\", \"content\": \"Hello\"}}]}}'"
        ),
        ApiStyle::BedrockConverse => format!(
            "aws bedrock-runtime converse \\\n  --region us-east-1 \\\n  --model-id {upstream_id} \\\n  --messages '[{{\"role\": \"user\", \"content\": [{{\"text\": \"Hello\"}}]}}]'"
        ),
    }
}

pub fn offering_view(catalog: &Catalog, model: &Model, offering: &Offering) -> OfferingView {
    let provider = catalog.providers.get(&offering.provider).map(|p| &p.file);
    let limits = offering_limits(model, offering);
    let count = offering.prices.len();
    OfferingView {
        provider_id: offering.provider.clone(),
        provider_name: provider.map_or_else(|| offering.provider.clone(), |p| p.name.clone()),
        label: offering.label.clone(),
        upstream_id: offering.upstream_id.clone(),
        aliases: offering.aliases.clone(),
        context: limits.context,
        max_output: limits.output,
        price: offering.prices.last().map(price_view),
        history: offering
            .prices
            .iter()
            .enumerate()
            .rev()
            .map(|(index, entry)| HistoryRow {
                effective_from: entry.effective_from.to_string(),
                input: cell(Some(&entry.standard), Meter::InputTokens),
                output: cell(Some(&entry.standard), Meter::OutputTokens),
                source_kind: entry.source.kind.label(),
                source_url: entry.source.url.clone(),
                current: index + 1 == count,
            })
            .collect(),
        example: api_example(provider, &offering.upstream_id),
    }
}

/// `text, image → text`.
pub fn modalities(model: &Model) -> String {
    let join = |list: &[omg_models_catalog::model::Modality]| {
        list.iter().map(|m| m.key()).collect::<Vec<_>>().join(", ")
    };
    format!(
        "{} \u{2192} {}",
        join(&model.file.modalities.input),
        join(&model.file.modalities.output)
    )
}
