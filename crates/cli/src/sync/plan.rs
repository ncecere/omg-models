//! Compares the catalog with source observations and decides what to append
//! and how to label the change.
//!
//! Policy (see README "Sync policy"):
//! - Prices are appended as a new entry dated today; old entries are never
//!   edited. Meters the primary source does not list are carried over from
//!   the previous entry and named in `source.carried_over`.
//! - `auto-merge` only when every changed rate moved within ±25 %, or the
//!   change only prices a previously unpriced offering or fills missing
//!   model metadata.
//! - `needs-review` for anything else: larger moves, meters added to an
//!   existing price, tier changes, values that are not a whole micro-USD,
//!   a cross-source disagreement on a changed offering, a removal from the
//!   primary source, or an unavailable cross-check source.

use std::collections::{BTreeMap, BTreeSet};

use omg_models_catalog::{
    Catalog, Decimal, Meter,
    model::{Currency, PriceEntry, Provenance, RateCard, SourceKind, Tier},
    time::{Date, PartialDate},
};
use serde::Serialize;

use super::{
    OfferingRef, PriceSource, Sources,
    observed::{Lookup, ObservedPrice},
};

/// Largest relative move (percent) that still counts as a small change.
pub const SMALL_CHANGE_PERCENT: u32 = 25;

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub model: String,
    /// Data-relative path of the model file.
    pub path: String,
    pub provider: String,
    pub upstream_id: String,
    #[serde(skip)]
    pub offering_index: usize,
}

impl Target {
    pub fn label(&self) -> String {
        format!("{} @ {} ({})", self.model, self.provider, self.upstream_id)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MeterDiff {
    /// `standard` or `batch`.
    pub card: &'static str,
    /// Prompt-size tier threshold, 0 for the base rate.
    pub tier: u64,
    pub meter: Meter,
    pub old: Option<Decimal>,
    pub new: Decimal,
    /// Relative change in basis points (None when added or from zero).
    pub change_bp: Option<i128>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceChange {
    pub target: Target,
    /// True when the offering had no price before.
    pub first_price: bool,
    pub source: SourceKind,
    pub diffs: Vec<MeterDiff>,
    /// Why this change needs review (empty = small).
    pub review_reasons: Vec<String>,
    #[serde(skip)]
    pub entry: PriceEntry,
}

#[derive(Clone, Debug, Serialize)]
pub struct MetadataFill {
    pub model: String,
    pub path: String,
    /// `release_date`, `knowledge_cutoff`, `limits.context`, `limits.output`.
    pub field: &'static str,
    pub value: String,
    pub source_key: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Disagreement {
    pub target: Target,
    pub card: &'static str,
    pub tier: u64,
    pub meter: Meter,
    /// The value the catalog has (or will have after this sync).
    pub catalog: Decimal,
    pub catalog_source: SourceKind,
    pub other: Decimal,
    pub other_source: SourceKind,
    pub other_key: String,
    /// True when it concerns an offering changed by this sync.
    pub blocking: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Removal {
    pub target: Target,
    pub source: SourceKind,
    pub key: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Note {
    pub subject: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Label {
    /// Nothing to change.
    None,
    AutoMerge,
    NeedsReview,
}

impl Label {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AutoMerge => "auto-merge",
            Self::NeedsReview => "needs-review",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Plan {
    pub today: String,
    pub price_changes: Vec<PriceChange>,
    pub metadata_fills: Vec<MetadataFill>,
    pub disagreements: Vec<Disagreement>,
    pub removals: Vec<Removal>,
    pub notes: Vec<Note>,
    /// Problems that force review of any change (e.g. cross-check down).
    pub global_review_reasons: Vec<String>,
}

impl Plan {
    pub fn has_changes(&self) -> bool {
        !self.price_changes.is_empty() || !self.metadata_fills.is_empty()
    }

    /// The label and every reason a human must review.
    pub fn label(&self) -> (Label, Vec<String>) {
        if !self.has_changes() {
            return (Label::None, Vec::new());
        }
        let mut reasons: Vec<String> = self.global_review_reasons.clone();
        for change in &self.price_changes {
            for reason in &change.review_reasons {
                reasons.push(format!("{}: {reason}", change.target.label()));
            }
        }
        for disagreement in self.disagreements.iter().filter(|d| d.blocking) {
            reasons.push(format!(
                "{}: {} {} disagrees between {} ({}) and {} ({})",
                disagreement.target.label(),
                disagreement.card,
                disagreement.meter,
                disagreement.catalog_source.label(),
                disagreement.catalog,
                disagreement.other_source.label(),
                disagreement.other,
            ));
        }
        for removal in &self.removals {
            reasons.push(format!(
                "{}: no longer in {} ({})",
                removal.target.label(),
                removal.source.label(),
                removal.reason
            ));
        }
        let label = if reasons.is_empty() {
            Label::AutoMerge
        } else {
            Label::NeedsReview
        };
        (label, reasons)
    }
}

/// `card` and its tiers as one map keyed by tier threshold (0 = base).
fn layered(base: &RateCard, tiers: &[Tier]) -> BTreeMap<u64, RateCard> {
    let mut map = BTreeMap::new();
    map.insert(0, base.clone());
    for tier in tiers {
        map.insert(tier.above_prompt_tokens, tier.rates.clone());
    }
    map
}

fn unlayer(mut map: BTreeMap<u64, RateCard>) -> (RateCard, Vec<Tier>) {
    let base = map.remove(&0).unwrap_or_default();
    let tiers = map
        .into_iter()
        .filter(|(_, rates)| !rates.is_empty())
        .map(|(above_prompt_tokens, rates)| Tier {
            above_prompt_tokens,
            rates,
        })
        .collect();
    (base, tiers)
}

/// Observed values win; meters only the current entry has are carried over.
fn merge(
    current: Option<BTreeMap<u64, RateCard>>,
    observed: BTreeMap<u64, RateCard>,
    carried: &mut BTreeSet<Meter>,
) -> BTreeMap<u64, RateCard> {
    let mut out = observed;
    for (tier, rates) in current.unwrap_or_default() {
        let target = out.entry(tier).or_default();
        for (meter, rate) in rates {
            if let std::collections::btree_map::Entry::Vacant(slot) = target.entry(meter) {
                slot.insert(rate);
                carried.insert(meter);
            }
        }
    }
    out
}

fn diff(
    card: &'static str,
    old: Option<&BTreeMap<u64, RateCard>>,
    new: &BTreeMap<u64, RateCard>,
) -> Vec<MeterDiff> {
    let mut diffs = Vec::new();
    for (tier, rates) in new {
        for (meter, rate) in rates {
            let previous = old
                .and_then(|o| o.get(tier))
                .and_then(|r| r.get(meter))
                .copied();
            if previous != Some(*rate) {
                diffs.push(MeterDiff {
                    card,
                    tier: *tier,
                    meter: *meter,
                    old: previous,
                    new: *rate,
                    change_bp: previous.and_then(|p| rate.change_basis_points(p)),
                });
            }
        }
    }
    diffs
}

fn tier_set(map: Option<&BTreeMap<u64, RateCard>>) -> BTreeSet<u64> {
    map.map(|m| m.keys().copied().filter(|t| *t > 0).collect())
        .unwrap_or_default()
}

/// Builds the new entry for `observed` on top of `current`.
fn proposed_entry(
    current: Option<&PriceEntry>,
    observed: &ObservedPrice,
    source: &dyn PriceSource,
    today: Date,
) -> PriceEntry {
    let mut carried = BTreeSet::new();
    let standard = merge(
        current.map(|c| layered(&c.standard, &c.standard_tiers)),
        layered(&observed.standard, &observed.standard_tiers),
        &mut carried,
    );
    let batch = match (&observed.batch, current.and_then(|c| c.batch.as_ref())) {
        (None, None) => None,
        (observed_batch, _) => Some(merge(
            current.and_then(|c| c.batch.as_ref().map(|b| layered(b, &c.batch_tiers))),
            observed_batch
                .as_ref()
                .map(|b| layered(b, &observed.batch_tiers))
                .unwrap_or_default(),
            &mut carried,
        )),
    };
    let (standard, standard_tiers) = unlayer(standard);
    let (batch, batch_tiers) = match batch.map(unlayer) {
        Some((base, tiers)) if !base.is_empty() => (Some(base), tiers),
        _ => (None, Vec::new()),
    };
    let priced: BTreeSet<Meter> = standard
        .keys()
        .chain(batch.iter().flat_map(|b| b.keys()))
        .copied()
        .collect();
    let not_applicable = current
        .map(|c| {
            c.not_applicable
                .iter()
                .copied()
                .filter(|m| !priced.contains(m))
                .collect()
        })
        .unwrap_or_default();
    let document = source.document();
    let version = document.version.clone().or_else(|| Some("unpinned".into()));
    let mut notes = format!(
        "Synced from {} ({}).",
        source.kind().label(),
        observed.entry_key
    );
    if !carried.is_empty() {
        notes.push_str(" Carried over unchanged because the source does not list them: ");
        notes.push_str(
            &carried
                .iter()
                .map(|m| m.key())
                .collect::<Vec<_>>()
                .join(", "),
        );
        notes.push('.');
    }
    PriceEntry {
        effective_from: today,
        currency: Currency::Usd,
        standard,
        standard_tiers,
        batch,
        batch_tiers,
        not_applicable,
        source: Provenance {
            kind: source.kind(),
            url: document.url.clone(),
            fetched_at: document.fetched_at.clone(),
            version,
            sha256: Some(document.sha256.clone()),
            entry_key: Some(observed.entry_key.clone()),
            cites: observed.cites.clone(),
            carried_over: carried.into_iter().collect(),
        },
        notes: Some(notes),
    }
}

fn review_reasons(change: &PriceChange, current: Option<&PriceEntry>) -> Vec<String> {
    let mut reasons = Vec::new();
    let entry = &change.entry;
    let old_tiers = current.map(|c| layered(&c.standard, &c.standard_tiers));
    let new_tiers = layered(&entry.standard, &entry.standard_tiers);
    let old_batch_tiers =
        current.and_then(|c| c.batch.as_ref().map(|b| layered(b, &c.batch_tiers)));
    let new_batch_tiers = entry.batch.as_ref().map(|b| layered(b, &entry.batch_tiers));
    if current.is_some()
        && (tier_set(old_tiers.as_ref()) != tier_set(Some(&new_tiers))
            || tier_set(old_batch_tiers.as_ref()) != tier_set(new_batch_tiers.as_ref()))
    {
        reasons.push("prompt-size tiers changed".into());
    }
    for diff in &change.diffs {
        let what = if diff.tier == 0 {
            format!("{} {}", diff.card, diff.meter)
        } else {
            format!("{} {} above {} tokens", diff.card, diff.meter, diff.tier)
        };
        if let Some((_, false)) = diff.new.to_micro_ceil() {
            reasons.push(format!(
                "{what} = {} is not a whole micro-USD (float artifact?)",
                diff.new
            ));
        }
        match diff.old {
            None if !change.first_price => {
                reasons.push(format!("{what} is newly listed ({})", diff.new));
            }
            Some(old) if !diff.new.within_percent_of(old, SMALL_CHANGE_PERCENT) => {
                reasons.push(format!(
                    "{what} moved more than {SMALL_CHANGE_PERCENT}% ({old} -> {})",
                    diff.new
                ));
            }
            _ => {}
        }
    }
    reasons
}

/// Builds the sync plan.
pub fn build(catalog: &Catalog, sources: &Sources, today: Date) -> Plan {
    let mut plan = Plan {
        today: today.to_string(),
        ..Plan::default()
    };
    for status in &sources.status {
        if let Some(error) = &status.error {
            plan.notes.push(Note {
                subject: status.name.clone(),
                text: format!("source unavailable: {error}"),
            });
        }
    }
    let cross_check_down = sources.genai.is_none() && sources.wants_cross_check;
    if cross_check_down {
        plan.global_review_reasons.push(
            "the genai-prices cross-check was unavailable, so no change could be corroborated"
                .into(),
        );
    }

    for model in catalog.models.values() {
        for (index, offering) in model.file.offerings.iter().enumerate() {
            let Some(provider) = catalog.providers.get(&offering.provider) else {
                continue;
            };
            let target = Target {
                model: model.file.id.clone(),
                path: model.path.clone(),
                provider: offering.provider.clone(),
                upstream_id: offering.upstream_id.clone(),
                offering_index: index,
            };
            let reference = OfferingRef {
                provider: &provider.file,
                model: &model.file,
                offering,
            };
            if offering.sync.disabled {
                continue;
            }
            let current = offering.prices.last();
            let mut changed_entry: Option<PriceEntry> = None;

            if let Some(source) = sources.primary(provider.file.sync.prices) {
                match source.lookup(&reference) {
                    Lookup::Found(observed) => {
                        for note in &observed.notes {
                            plan.notes.push(Note {
                                subject: target.label(),
                                text: format!("{}: {note}", source.kind().label()),
                            });
                        }
                        if observed.standard.is_empty() {
                            // Nothing usable; never write an empty price.
                        } else if current.is_some_and(|c| c.effective_from > today) {
                            plan.notes.push(Note {
                                subject: target.label(),
                                text: "latest entry is dated in the future; not appending".into(),
                            });
                        } else {
                            let entry = proposed_entry(current, &observed, source, today);
                            let old_std = current.map(|c| layered(&c.standard, &c.standard_tiers));
                            let old_batch = current
                                .and_then(|c| c.batch.as_ref().map(|b| layered(b, &c.batch_tiers)));
                            let new_std = layered(&entry.standard, &entry.standard_tiers);
                            let new_batch =
                                entry.batch.as_ref().map(|b| layered(b, &entry.batch_tiers));
                            let mut diffs = diff("standard", old_std.as_ref(), &new_std);
                            if let Some(new_batch) = &new_batch {
                                diffs.extend(diff("batch", old_batch.as_ref(), new_batch));
                            }
                            let tiers_differ = current.is_some()
                                && (tier_set(old_std.as_ref()) != tier_set(Some(&new_std))
                                    || tier_set(old_batch.as_ref())
                                        != tier_set(new_batch.as_ref()));
                            if !diffs.is_empty() || tiers_differ {
                                let mut change = PriceChange {
                                    target: target.clone(),
                                    first_price: current.is_none(),
                                    source: source.kind(),
                                    diffs,
                                    review_reasons: Vec::new(),
                                    entry,
                                };
                                change.review_reasons = review_reasons(&change, current);
                                changed_entry = Some(change.entry.clone());
                                plan.price_changes.push(change);
                            }
                        }
                    }
                    Lookup::Missing { key, reason } => {
                        if current.is_some() {
                            plan.removals.push(Removal {
                                target: target.clone(),
                                source: source.kind(),
                                key,
                                reason,
                            });
                        } else {
                            plan.notes.push(Note {
                                subject: target.label(),
                                text: format!(
                                    "no price in {} for {key}: {reason}",
                                    source.kind().label()
                                ),
                            });
                        }
                    }
                }
            }

            // Cross-check against genai-prices, comparing what the catalog
            // will hold after this sync.
            let compared = changed_entry.as_ref().or(current);
            if let (Some(genai), Some(compared)) = (&sources.genai, compared)
                && provider.file.sync.genai_prices_provider.is_some()
            {
                match genai.lookup(&reference) {
                    Lookup::Found(other) => {
                        let ours = layered(&compared.standard, &compared.standard_tiers);
                        let theirs = layered(&other.standard, &other.standard_tiers);
                        for (tier, rates) in &theirs {
                            for (meter, value) in rates {
                                if let Some(catalog_value) =
                                    ours.get(tier).and_then(|r| r.get(meter))
                                    && catalog_value != value
                                {
                                    plan.disagreements.push(Disagreement {
                                        target: target.clone(),
                                        card: "standard",
                                        tier: *tier,
                                        meter: *meter,
                                        catalog: *catalog_value,
                                        catalog_source: compared.source.kind,
                                        other: *value,
                                        other_source: SourceKind::GenaiPrices,
                                        other_key: other.entry_key.clone(),
                                        blocking: changed_entry.is_some(),
                                    });
                                }
                            }
                        }
                    }
                    Lookup::Missing { key, reason } => plan.notes.push(Note {
                        subject: target.label(),
                        text: format!("not cross-checked ({key}: {reason})"),
                    }),
                }
            }
        }

        // Metadata from models.dev, through the home provider's offering.
        if let Some(models_dev) = &sources.models_dev {
            let home = model
                .file
                .offerings
                .iter()
                .find(|o| o.provider == model.home_provider);
            if let (Some(offering), Some(provider)) =
                (home, catalog.providers.get(&model.home_provider))
                && let Lookup::Found(meta) = models_dev.lookup(&OfferingRef {
                    provider: &provider.file,
                    model: &model.file,
                    offering,
                })
            {
                let file = &model.file;
                let mut add_fill = |field: &'static str, value: String| {
                    plan.metadata_fills.push(MetadataFill {
                        model: file.id.clone(),
                        path: model.path.clone(),
                        field,
                        value,
                        source_key: meta.entry_key.clone(),
                    });
                };
                let mut differs = Vec::new();
                match (&file.release_date, &meta.release_date) {
                    (None, Some(date)) if Date::parse(date).is_some() => {
                        add_fill("release_date", date.clone());
                    }
                    (Some(ours), Some(theirs)) if ours.to_string() != *theirs => {
                        differs.push(format!("release_date {theirs} (catalog {ours})"));
                    }
                    _ => {}
                }
                match (&file.knowledge_cutoff, &meta.knowledge_cutoff) {
                    (None, Some(cutoff)) if PartialDate::parse(cutoff).is_some() => {
                        add_fill("knowledge_cutoff", cutoff.clone());
                    }
                    (Some(ours), Some(theirs))
                        if !theirs.starts_with(ours.as_str())
                            && !ours.as_str().starts_with(theirs.as_str()) =>
                    {
                        differs.push(format!("knowledge cutoff {theirs} (catalog {ours})"));
                    }
                    _ => {}
                }
                for (field, ours, theirs) in [
                    ("limits.context", file.limits.context, meta.context),
                    ("limits.output", file.limits.output, meta.output),
                ] {
                    match (ours, theirs) {
                        (None, Some(value)) => add_fill(field, value.to_string()),
                        (Some(ours), Some(theirs)) if ours != theirs => {
                            differs.push(format!("{field} {theirs} (catalog {ours})"));
                        }
                        _ => {}
                    }
                }
                if !differs.is_empty() {
                    plan.notes.push(Note {
                        subject: file.id.clone(),
                        text: format!(
                            "models.dev ({}) differs: {}",
                            meta.entry_key,
                            differs.join("; ")
                        ),
                    });
                }
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_threshold_is_inclusive() {
        let d = |s: &str| Decimal::parse_strict(s).unwrap();
        assert!(d("1.25").within_percent_of(d("1"), SMALL_CHANGE_PERCENT));
        assert!(!d("1.2501").within_percent_of(d("1"), SMALL_CHANGE_PERCENT));
    }
}
