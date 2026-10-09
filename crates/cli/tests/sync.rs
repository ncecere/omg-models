//! `omg-models sync` on recorded fixtures (no network).

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use omg_models_catalog::{
    Decimal, Meter, load_validated,
    model::SourceKind,
    time::{Date, Timestamp},
};
use omg_models_cli::sync::{
    Sources, apply,
    fetch::{from_file, sha256_hex},
    genai::{self, GenaiPrices},
    litellm::LiteLlm,
    models_dev::ModelsDev,
    observed::Document,
    openrouter::OpenRouter,
    plan::{self, Label},
};
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn seed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn d(text: &str) -> Decimal {
    Decimal::parse_strict(text).unwrap()
}

fn today() -> Date {
    Date::parse("2026-10-10").unwrap()
}

fn doc(kind: SourceKind, bytes: Vec<u8>) -> Document {
    Document {
        kind,
        url: "https://example.test/source.json".into(),
        fetched_at: Timestamp::parse("2026-10-10T00:00:00Z").unwrap(),
        version: Some("0123456789abcdef0123456789abcdef01234567".into()),
        sha256: sha256_hex(&bytes),
        bytes,
    }
}

fn fixture(name: &str, kind: SourceKind) -> Document {
    from_file(
        &fixtures().join(name),
        kind,
        "https://example.test/x.json",
        "recorded-fixture",
    )
    .unwrap()
}

fn litellm_entry(key: &str) -> serde_json::Map<String, Value> {
    let value: Value =
        serde_json::from_slice(&fs::read(fixtures().join("litellm.json")).unwrap()).unwrap();
    value[key].as_object().unwrap().clone()
}

// --- Parsers ----------------------------------------------------------------

#[test]
fn litellm_maps_claude_cache_batch_and_tiers_exactly() {
    let sonnet = LiteLlm::convert(
        "claude-sonnet-5-5",
        &litellm_entry("claude-sonnet-5-5"),
        true,
    );
    assert_eq!(sonnet.standard[&Meter::InputTokens], d("2"));
    assert_eq!(sonnet.standard[&Meter::OutputTokens], d("10"));
    assert_eq!(sonnet.standard[&Meter::CacheReadTokens], d("0.1"));
    assert_eq!(sonnet.standard[&Meter::CacheWrite5mTokens], d("2.5"));
    assert_eq!(sonnet.standard[&Meter::CacheWrite1hTokens], d("4"));
    assert!(!sonnet.standard.contains_key(&Meter::CacheWriteTokens));
    let batch = sonnet.batch.as_ref().unwrap();
    assert_eq!(batch[&Meter::InputTokens], d("1"));
    assert_eq!(batch[&Meter::CacheReadTokens], d("0.05"));
    assert_eq!(
        sonnet.cites.as_deref(),
        Some("https://platform.claude.com/docs/en/about-claude/pricing")
    );
    assert!(
        sonnet
            .notes
            .iter()
            .any(|n| n.contains("search_context_cost_per_query"))
    );

    let haiku = LiteLlm::convert("claude-haiku-5-5", &litellm_entry("claude-haiku-5-5"), true);
    assert_eq!(haiku.standard_tiers.len(), 1);
    let tier = &haiku.standard_tiers[0];
    assert_eq!(tier.above_prompt_tokens, 100_000);
    assert_eq!(tier.rates[&Meter::InputTokens], d("0.5"));
    assert_eq!(tier.rates[&Meter::CacheWrite1hTokens], d("1"));
    assert_eq!(
        haiku.batch_tiers[0].rates[&Meter::CacheWrite5mTokens],
        d("0.3125")
    );
}

#[test]
fn litellm_non_claude_cache_writes_and_service_tiers() {
    let sol = LiteLlm::convert("gpt-6.1-sol", &litellm_entry("gpt-6.1-sol"), false);
    assert_eq!(sol.standard[&Meter::CacheWriteTokens], d("2.5"));
    assert_eq!(sol.standard_tiers[0].above_prompt_tokens, 272_000);
    assert_eq!(sol.standard_tiers[0].rates[&Meter::OutputTokens], d("15"));
    assert_eq!(sol.batch_tiers[0].rates[&Meter::OutputTokens], d("7.5"));
    assert!(sol.notes.iter().any(|n| n.contains("service-tier")));
}

#[test]
fn litellm_xai_thresholds_are_inclusive() {
    let key = "xai/grok-4.20-multi-agent-beta-0309";
    let grok = LiteLlm::convert(key, &litellm_entry(key), false);
    assert_eq!(grok.standard_tiers[0].above_prompt_tokens, 199_999);
    assert!(
        grok.notes
            .iter()
            .any(|n| n.contains("input_cost_per_image_token"))
    );
}

#[test]
fn litellm_float_artifacts_stay_exact_and_are_flagged_inexact() {
    let key = "azure/gpt-realtime-whisper";
    let whisper = LiteLlm::convert(key, &litellm_entry(key), false);
    let rate = whisper.standard[&Meter::InputAudioSeconds];
    assert_eq!(rate.to_string(), "0.0002833333333333333");
    assert_eq!(rate.to_micro_ceil(), Some((284, false)));
}

#[test]
fn genai_match_rules() {
    let value: Value = serde_json::json!({"or": [{"equals": "anthropic.claude-sonnet-5-5"}, {"equals": "us.anthropic.claude-sonnet-5-5"}, {"regex": "^claude-sonnet-5-5-\\d{8}$"}]});
    assert!(genai::matches(&value, "us.anthropic.claude-sonnet-5-5"));
    assert!(genai::matches(&value, "claude-sonnet-5-5-20261001"));
    assert!(!genai::matches(
        &value,
        "global.anthropic.claude-sonnet-5-5"
    ));
    assert!(genai::matches(
        &serde_json::json!({"and": [{"starts_with": "a"}, {"not": {"contains": "z"}}]}),
        "abc"
    ));
}

// --- Plans --------------------------------------------------------------------

fn fixture_sources() -> Sources {
    Sources {
        litellm: Some(LiteLlm::parse(fixture("litellm.json", SourceKind::Litellm)).unwrap()),
        genai: Some(
            GenaiPrices::parse(
                fixture("genai-prices.json", SourceKind::GenaiPrices),
                today(),
            )
            .unwrap(),
        ),
        openrouter: Some(
            OpenRouter::parse(fixture("openrouter.json", SourceKind::Openrouter)).unwrap(),
        ),
        models_dev: Some(
            ModelsDev::parse(fixture("models-dev.json", SourceKind::ModelsDev)).unwrap(),
        ),
        wants_cross_check: true,
        status: Vec::new(),
    }
}

#[test]
fn seed_data_against_recorded_sources() {
    let (catalog, _) = load_validated(&seed());
    let catalog = catalog.unwrap();
    let plan = plan::build(&catalog, &fixture_sources(), today());

    // OpenRouter matches the seed (the seed was read from the same response).
    assert!(
        !plan
            .price_changes
            .iter()
            .any(|c| c.target.provider == "openrouter"),
        "{:#?}",
        plan.price_changes
            .iter()
            .filter(|c| c.target.provider == "openrouter")
            .map(|c| (&c.target, &c.diffs))
            .collect::<Vec<_>>()
    );
    // LiteLLM lists Anthropic batch cache rates the seed (pricing page) lacks:
    // added meters on an existing price need review.
    let sonnet = plan
        .price_changes
        .iter()
        .find(|c| c.target.provider == "anthropic" && c.target.model == "claude-sonnet-5-5")
        .expect("sonnet change");
    assert!(
        sonnet
            .diffs
            .iter()
            .all(|d| d.card == "batch" && d.old.is_none())
    );
    assert!(
        sonnet
            .review_reasons
            .iter()
            .any(|r| r.contains("newly listed"))
    );
    // search_units is not in LiteLLM's mapped keys: carried over, not dropped.
    assert!(
        sonnet
            .entry
            .source
            .carried_over
            .contains(&Meter::SearchUnits)
    );
    assert_eq!(sonnet.entry.standard[&Meter::SearchUnits], d("0.01"));
    assert_eq!(sonnet.entry.source.kind, SourceKind::Litellm);
    assert_eq!(
        sonnet.entry.source.entry_key.as_deref(),
        Some("claude-sonnet-5-5")
    );

    // genai-prices says $0.20 cache read for Sonnet 5.5; catalog keeps $0.10.
    let disagreement = plan
        .disagreements
        .iter()
        .find(|d| d.target.model == "claude-sonnet-5-5" && d.target.provider == "anthropic")
        .expect("known disagreement");
    assert_eq!(disagreement.meter, Meter::CacheReadTokens);
    assert_eq!(disagreement.catalog, d("0.1"));
    assert_eq!(disagreement.other, d("0.2"));
    assert!(disagreement.blocking);

    // Bedrock global/us keys are matched exactly, never prefix-stripped.
    assert!(
        !plan
            .removals
            .iter()
            .any(|r| r.target.provider == "amazon-bedrock"),
        "{:#?}",
        plan.removals
    );
    let (label, reasons) = plan.label();
    assert_eq!(label, Label::NeedsReview);
    assert!(reasons.iter().any(|r| r.contains("disagrees")));

    // models.dev fills metadata only where the catalog has none.
    for fill in &plan.metadata_fills {
        assert!(matches!(
            fill.field,
            "release_date" | "knowledge_cutoff" | "limits.context" | "limits.output"
        ));
    }
}

#[test]
fn applying_appends_and_revalidates() {
    let dir = scratch_copy(&seed());
    let (catalog, _) = load_validated(&dir.0);
    let plan = plan::build(&catalog.unwrap(), &fixture_sources(), today());
    let path = dir
        .0
        .join("providers/anthropic/models/claude-sonnet-5-5.toml");
    let before = fs::read_to_string(&path).unwrap();
    apply::apply(&dir.0, &plan).unwrap();
    let after = fs::read_to_string(&path).unwrap();
    // Existing text is preserved; the change is an append within the offering.
    let first_offering_end = before
        .find("\n[[offerings]]\nprovider = \"amazon-bedrock\"")
        .unwrap();
    assert!(after.starts_with(&before[..first_offering_end]));
    assert!(after.contains("effective_from = \"2026-10-10\""));
    assert!(after.contains("kind = \"litellm\""));
    let (catalog, issues) = load_validated(&dir.0);
    assert!(catalog.is_some(), "{issues:#?}");
    let model = &catalog.unwrap().models["claude-sonnet-5-5"];
    let anthropic = model
        .file
        .offerings
        .iter()
        .find(|o| o.provider == "anthropic")
        .unwrap();
    assert_eq!(anthropic.prices.len(), 2);
    assert_eq!(
        anthropic.prices[1].batch.as_ref().unwrap()[&Meter::CacheReadTokens],
        d("0.05")
    );
    // Re-planning against the same sources is a no-op for that offering.
    let (catalog, _) = load_validated(&dir.0);
    let again = plan::build(&catalog.unwrap(), &fixture_sources(), today());
    assert!(
        !again
            .price_changes
            .iter()
            .any(|c| c.target.model == "claude-sonnet-5-5" && c.target.provider == "anthropic")
    );
}

// --- Policy on a one-model catalog ------------------------------------------

const PROVIDER: &str = r#"id = "acme"
name = "Acme"
type = "cloud"
website = "https://acme.example"

[sync]
prices = "litellm"
litellm_providers = ["acme"]
genai_prices_provider = "acme"
"#;

fn acme_model(prices: &str) -> String {
    format!(
        r#"id = "acme-1"
name = "Acme 1"
vendor = "Acme"
open_weights = false

[modalities]
input = ["text"]
output = ["text"]

[[offerings]]
provider = "acme"
upstream_id = "acme-1"
{prices}"#
    )
}

const PRICED: &str = r#"
[[offerings.prices]]
effective_from = "2026-10-01"
currency = "USD"
standard = { input_tokens = "1", output_tokens = "4" }
source = { kind = "provider-page", url = "https://acme.example/pricing", fetched_at = "2026-10-01T00:00:00Z" }
"#;

fn litellm_with(entries: &str) -> LiteLlm {
    LiteLlm::parse(doc(SourceKind::Litellm, entries.as_bytes().to_vec())).unwrap()
}

fn genai_with(input: &str) -> GenaiPrices {
    let json = format!(
        r#"[{{"id": "acme", "models": [{{"id": "acme-1", "match": {{"equals": "acme-1"}}, "prices": {{"input_mtok": {input}, "output_mtok": 4}}}}]}}]"#
    );
    GenaiPrices::parse(doc(SourceKind::GenaiPrices, json.into_bytes()), today()).unwrap()
}

fn acme_plan(prices: &str, litellm: &str, genai_input: Option<&str>) -> plan::Plan {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER);
    dir.write("providers/acme/models/acme-1.toml", &acme_model(prices));
    let (catalog, issues) = load_validated(&dir.0);
    let catalog = catalog.unwrap_or_else(|| panic!("{issues:#?}"));
    let sources = Sources {
        litellm: Some(litellm_with(litellm)),
        genai: genai_input.map(genai_with),
        wants_cross_check: genai_input.is_some(),
        ..Sources::default()
    };
    plan::build(&catalog, &sources, today())
}

#[test]
fn unchanged_prices_produce_no_change() {
    let plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1e-06, "output_cost_per_token": 4e-06}}"#,
        Some("1"),
    );
    assert!(!plan.has_changes());
    assert_eq!(plan.label().0, Label::None);
}

#[test]
fn small_moves_auto_merge() {
    let plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1.25e-06, "output_cost_per_token": 4e-06}}"#,
        Some("1.25"),
    );
    assert_eq!(plan.price_changes.len(), 1);
    assert_eq!(plan.label(), (Label::AutoMerge, vec![]));
}

#[test]
fn large_moves_need_review() {
    let plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1.5e-06, "output_cost_per_token": 4e-06}}"#,
        Some("1.5"),
    );
    let (label, reasons) = plan.label();
    assert_eq!(label, Label::NeedsReview);
    assert!(reasons[0].contains("moved more than 25%"), "{reasons:?}");
}

#[test]
fn cross_source_disagreement_blocks_auto_merge() {
    let plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1.1e-06, "output_cost_per_token": 4e-06}}"#,
        Some("1"),
    );
    assert_eq!(plan.disagreements.len(), 1);
    assert_eq!(plan.label().0, Label::NeedsReview);
}

#[test]
fn missing_cross_check_blocks_auto_merge() {
    let mut plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1.1e-06, "output_cost_per_token": 4e-06}}"#,
        None,
    );
    assert_eq!(plan.label().0, Label::AutoMerge);
    plan.global_review_reasons
        .push("cross-check unavailable".into());
    assert_eq!(plan.label().0, Label::NeedsReview);
}

#[test]
fn removals_need_review_but_change_nothing() {
    let plan = acme_plan(
        PRICED,
        r#"{"other": {"litellm_provider": "acme", "input_cost_per_token": 1e-06}}"#,
        Some("1"),
    );
    assert_eq!(plan.removals.len(), 1);
    assert!(!plan.has_changes());
}

#[test]
fn first_price_for_an_offering_is_small() {
    let plan = acme_plan(
        "",
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1e-06, "output_cost_per_token": 4e-06}}"#,
        Some("1"),
    );
    assert!(plan.price_changes[0].first_price);
    assert_eq!(plan.label().0, Label::AutoMerge);
}

#[test]
fn wrong_litellm_provider_is_not_matched() {
    let plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "someone-else", "input_cost_per_token": 9e-06}}"#,
        None,
    );
    assert!(plan.removals[0].reason.contains("expected one of"));
}

#[test]
fn inexact_values_need_review_and_export_rounds_up() {
    let plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1.0000000000000002e-06, "output_cost_per_token": 4e-06}}"#,
        None,
    );
    let (label, reasons) = plan.label();
    assert_eq!(label, Label::NeedsReview);
    assert!(reasons.iter().any(|r| r.contains("not a whole micro-USD")));
    let rate = plan.price_changes[0].entry.standard[&Meter::InputTokens];
    assert_eq!(rate.to_string(), "1.0000000000000002");
    assert_eq!(rate.to_micro_ceil(), Some((1_000_001, false)));
}

#[test]
fn rendered_entries_round_trip() {
    let plan = acme_plan(
        PRICED,
        r#"{"acme-1": {"litellm_provider": "acme", "input_cost_per_token": 1.1e-06, "output_cost_per_token": 4e-06, "input_cost_per_token_above_200k_tokens": 2.2e-06, "input_cost_per_token_batches": 5.5e-07, "output_cost_per_token_batches": 2e-06}}"#,
        None,
    );
    let change = &plan.price_changes[0];
    let text = acme_model(PRICED);
    let updated = apply::append_price(&text, change).unwrap();
    assert!(updated.starts_with(&text));
    assert!(updated.contains("standard_tiers = [\n  { above_prompt_tokens = 200000"));
}

// --- Scratch directories -------------------------------------------------------

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "omg-models-sync-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn scratch_copy(from: &Path) -> Scratch {
    let scratch = Scratch::new();
    copy_dir(from, &scratch.0);
    scratch
}
