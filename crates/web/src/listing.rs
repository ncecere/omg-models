//! The model table on the home page: one row per offering, with filters and
//! sorting done on the server (no client script).

use std::cmp::Ordering;

use omg_models_catalog::{
    Catalog, Decimal, Meter,
    export::offering_limits,
    model::{Modality, PriceEntry},
    time::Date,
};

/// One table row: a model as served by one provider.
#[derive(Clone, Debug)]
pub struct Row {
    pub model_id: String,
    pub model_name: String,
    pub vendor: String,
    pub provider_id: String,
    pub provider_name: String,
    pub label: Option<String>,
    pub upstream_id: String,
    pub input: Option<Decimal>,
    pub output: Option<Decimal>,
    pub cache_read: Option<Decimal>,
    /// The 5-minute write when listed, else the default write.
    pub cache_write: Option<Decimal>,
    pub tiered: Option<u64>,
    pub context: Option<u64>,
    pub max_output: Option<u64>,
    pub input_modalities: Vec<Modality>,
    pub output_modalities: Vec<Modality>,
    pub open_weights: bool,
    pub release: Option<Date>,
}

fn rate(price: Option<&PriceEntry>, meter: Meter) -> Option<Decimal> {
    price.and_then(|p| p.standard.get(&meter).copied())
}

/// All rows, in catalog order (model id, then provider, then upstream id).
pub fn rows(catalog: &Catalog) -> Vec<Row> {
    let mut rows = Vec::new();
    for model in catalog.models.values() {
        let mut offerings: Vec<_> = model.file.offerings.iter().collect();
        offerings.sort_by(|a, b| (&a.provider, &a.upstream_id).cmp(&(&b.provider, &b.upstream_id)));
        for offering in offerings {
            let price = offering.prices.last();
            let limits = offering_limits(model, offering);
            rows.push(Row {
                model_id: model.file.id.clone(),
                model_name: model.file.name.clone(),
                vendor: model.file.vendor.clone(),
                provider_id: offering.provider.clone(),
                provider_name: catalog
                    .providers
                    .get(&offering.provider)
                    .map_or_else(|| offering.provider.clone(), |p| p.file.name.clone()),
                label: offering.label.clone(),
                upstream_id: offering.upstream_id.clone(),
                input: rate(price, Meter::InputTokens),
                output: rate(price, Meter::OutputTokens),
                cache_read: rate(price, Meter::CacheReadTokens),
                cache_write: rate(price, Meter::CacheWrite5mTokens)
                    .or_else(|| rate(price, Meter::CacheWriteTokens)),
                tiered: price.and_then(|p| p.standard_tiers.first().map(|t| t.above_prompt_tokens)),
                context: limits.context,
                max_output: limits.output,
                input_modalities: model.file.modalities.input.clone(),
                output_modalities: model.file.modalities.output.clone(),
                open_weights: model.file.open_weights,
                release: model.file.release_date,
            });
        }
    }
    rows
}

/// Sortable columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Provider,
    Input,
    Output,
    CacheRead,
    CacheWrite,
    Context,
    MaxOutput,
    Release,
}

impl SortKey {
    pub const ALL: [Self; 9] = [
        Self::Name,
        Self::Provider,
        Self::Input,
        Self::Output,
        Self::CacheRead,
        Self::CacheWrite,
        Self::Context,
        Self::MaxOutput,
        Self::Release,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Provider => "provider",
            Self::Input => "input",
            Self::Output => "output",
            Self::CacheRead => "cache_read",
            Self::CacheWrite => "cache_write",
            Self::Context => "context",
            Self::MaxOutput => "max_output",
            Self::Release => "release",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == text)
    }
}

/// Parsed, validated filters from the query string.
#[derive(Clone, Debug, Default)]
pub struct Filters {
    pub q: String,
    pub provider: Option<String>,
    pub modality: Option<Modality>,
    pub max_input: Option<Decimal>,
    pub min_context: Option<u64>,
    pub open_weights: bool,
    pub sort: Option<SortKey>,
    pub descending: bool,
    /// User-facing problems with the submitted filters.
    pub problems: Vec<String>,
}

impl Filters {
    pub fn matches(&self, row: &Row) -> bool {
        let q = self.q.trim().to_lowercase();
        if !q.is_empty() {
            let haystack = format!(
                "{} {} {} {} {} {}",
                row.model_name,
                row.model_id,
                row.vendor,
                row.provider_name,
                row.upstream_id,
                row.label.as_deref().unwrap_or("")
            )
            .to_lowercase();
            if !q.split_whitespace().all(|word| haystack.contains(word)) {
                return false;
            }
        }
        if self
            .provider
            .as_ref()
            .is_some_and(|p| p != &row.provider_id)
        {
            return false;
        }
        if let Some(modality) = self.modality
            && !row.input_modalities.contains(&modality)
            && !row.output_modalities.contains(&modality)
        {
            return false;
        }
        if let Some(max) = self.max_input
            && row.input.is_none_or(|input| input > max)
        {
            return false;
        }
        if let Some(min) = self.min_context
            && row.context.is_none_or(|context| context < min)
        {
            return false;
        }
        !self.open_weights || row.open_weights
    }

    pub fn is_active(&self) -> bool {
        !self.q.trim().is_empty()
            || self.provider.is_some()
            || self.modality.is_some()
            || self.max_input.is_some()
            || self.min_context.is_some()
            || self.open_weights
    }
}

/// `None` sorts last in both directions.
fn cmp_option<T: Ord>(a: Option<&T>, b: Option<&T>, descending: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => {
            if descending {
                b.cmp(a)
            } else {
                a.cmp(b)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn sort(rows: &mut [Row], key: SortKey, descending: bool) {
    rows.sort_by(|a, b| {
        let primary = match key {
            SortKey::Name => cmp_option(
                Some(&a.model_name.to_lowercase()),
                Some(&b.model_name.to_lowercase()),
                descending,
            ),
            SortKey::Provider => {
                cmp_option(Some(&a.provider_name), Some(&b.provider_name), descending)
            }
            SortKey::Input => cmp_option(a.input.as_ref(), b.input.as_ref(), descending),
            SortKey::Output => cmp_option(a.output.as_ref(), b.output.as_ref(), descending),
            SortKey::CacheRead => {
                cmp_option(a.cache_read.as_ref(), b.cache_read.as_ref(), descending)
            }
            SortKey::CacheWrite => {
                cmp_option(a.cache_write.as_ref(), b.cache_write.as_ref(), descending)
            }
            SortKey::Context => cmp_option(a.context.as_ref(), b.context.as_ref(), descending),
            SortKey::MaxOutput => {
                cmp_option(a.max_output.as_ref(), b.max_output.as_ref(), descending)
            }
            SortKey::Release => cmp_option(a.release.as_ref(), b.release.as_ref(), descending),
        };
        primary
            .then_with(|| a.model_name.cmp(&b.model_name))
            .then_with(|| a.provider_name.cmp(&b.provider_name))
            .then_with(|| a.upstream_id.cmp(&b.upstream_id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, input: Option<&str>) -> Row {
        Row {
            model_id: name.to_lowercase(),
            model_name: name.into(),
            vendor: "V".into(),
            provider_id: "p".into(),
            provider_name: "P".into(),
            label: None,
            upstream_id: name.into(),
            input: input.map(|s| Decimal::parse_strict(s).unwrap()),
            output: None,
            cache_read: None,
            cache_write: None,
            tiered: None,
            context: Some(1000),
            max_output: None,
            input_modalities: vec![Modality::Text],
            output_modalities: vec![Modality::Text],
            open_weights: false,
            release: None,
        }
    }

    #[test]
    fn unknown_prices_sort_last_both_ways() {
        let mut rows = vec![row("A", None), row("B", Some("2")), row("C", Some("0.5"))];
        sort(&mut rows, SortKey::Input, false);
        assert_eq!(
            rows.iter()
                .map(|r| r.model_name.as_str())
                .collect::<Vec<_>>(),
            ["C", "B", "A"]
        );
        sort(&mut rows, SortKey::Input, true);
        assert_eq!(
            rows.iter()
                .map(|r| r.model_name.as_str())
                .collect::<Vec<_>>(),
            ["B", "C", "A"]
        );
    }

    #[test]
    fn unknown_price_never_passes_a_price_filter() {
        let filters = Filters {
            max_input: Some(Decimal::parse_strict("1").unwrap()),
            ..Filters::default()
        };
        assert!(!filters.matches(&row("A", None)));
        assert!(filters.matches(&row("C", Some("0.5"))));
        assert!(!filters.matches(&row("B", Some("2"))));
    }
}
