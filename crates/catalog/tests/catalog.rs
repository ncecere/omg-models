//! Loader, validator and export tests against the real seed data and small
//! temporary data directories.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use omg_models_catalog::{Severity, export, load_validated};
use serde_json::Value;

fn seed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// A scratch data directory under the target dir, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "omg-models-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("providers")).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, text: &str) -> &Self {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const PROVIDER: &str = r#"
id = "acme"
name = "Acme AI"
type = "cloud"
website = "https://acme.example"
"#;

fn model(prices: &str) -> String {
    format!(
        r#"
id = "acme-1"
name = "Acme 1"
vendor = "Acme"
open_weights = false

[modalities]
input = ["text"]
output = ["text"]

[[offerings]]
provider = "acme"
upstream_id = "acme-1"
{prices}
"#
    )
}

const SOURCE: &str = r#"source = { kind = "provider-page", url = "https://acme.example/pricing", fetched_at = "2026-10-09T12:00:00Z" }"#;

fn errors(dir: &Scratch) -> Vec<String> {
    let (_, issues) = load_validated(&dir.0);
    issues
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.to_string())
        .collect()
}

fn assert_error(dir: &Scratch, needle: &str) {
    let errors = errors(dir);
    assert!(
        errors.iter().any(|e| e.contains(needle)),
        "expected an error containing {needle:?}, got {errors:#?}"
    );
}

#[test]
fn seed_data_is_valid() {
    let (catalog, issues) = load_validated(&seed_dir());
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "seed data has errors: {errors:#?}");
    let catalog = catalog.unwrap();
    assert!(catalog.providers.len() >= 4);
    assert!(catalog.models.len() >= 6);
}

#[test]
fn minimal_catalog_is_valid() {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = {{ input_tokens = \"1.25\" }}\n{SOURCE}\n"
        )),
    );
    assert!(errors(&dir).is_empty(), "{:#?}", errors(&dir));
}

#[test]
fn rejects_floats_and_negative_prices() {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = {{ input_tokens = 1.25 }}\n{SOURCE}\n"
        )),
    );
    assert_error(&dir, "providers/acme/models/acme-1.toml");
    assert_error(&dir, "invalid type");

    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = {{ input_tokens = \"-1\" }}\n{SOURCE}\n"
        )),
    );
    assert_error(&dir, "negative values are not allowed");

    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = {{ input_tokens = \"1e-6\" }}\n{SOURCE}\n"
        )),
    );
    assert_error(&dir, "not an exact decimal");
}

#[test]
fn rejects_unknown_meters_and_fields() {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = {{ prompt = \"1\" }}\n{SOURCE}\n"
        )),
    );
    assert_error(&dir, "unknown variant");
}

#[test]
fn price_history_must_be_append_only() {
    let dir = Scratch::new();
    let entry = |date: &str, at: &str| {
        format!(
            "[[offerings.prices]]\neffective_from = \"{date}\"\ncurrency = \"USD\"\nstandard = {{ input_tokens = \"1\" }}\nsource = {{ kind = \"provider-page\", url = \"https://acme.example/p\", fetched_at = \"{at}\" }}\n"
        )
    };
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "{}{}",
            entry("2026-10-09", "2026-10-09T00:00:00Z"),
            entry("2026-09-01", "2026-10-10T00:00:00Z")
        )),
    );
    assert_error(&dir, "price history must be in date order");
}

#[test]
fn tiers_must_ascend_and_have_base_rates() {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = {{ input_tokens = \"1\" }}\nstandard_tiers = [{{ above_prompt_tokens = 200000, rates = {{ input_tokens = \"2\" }} }}, {{ above_prompt_tokens = 100000, rates = {{ output_tokens = \"3\" }} }}]\n{SOURCE}\n"
        )),
    );
    assert_error(&dir, "strictly ascending");
    assert_error(&dir, "has a tier rate but no base rate");
}

#[test]
fn provenance_and_references_are_checked() {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = { input_tokens = \"1\" }\nsource = { kind = \"litellm\", url = \"http://x\", fetched_at = \"2026-10-09T00:00:00Z\" }\n\n[[offerings]]\nprovider = \"nowhere\"\nupstream_id = \"x\"\n",
        ),
    );
    assert_error(&dir, "source.url must be an https:// URL");
    assert_error(&dir, "requires source.entry_key");
    assert_error(&dir, "requires source.version");
    assert_error(&dir, "unknown provider \"nowhere\"");

    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model("").replace(
            "[[offerings]]\nprovider = \"acme\"\nupstream_id = \"acme-1\"",
            "",
        ),
    );
    assert_error(&dir, "at least one offering");
}

#[test]
fn ids_must_match_files_and_be_unique() {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER)
        .write("providers/acme/models/acme-1.toml", &model(""))
        .write("providers/acme/models/acme-2.toml", &model(""));
    assert_error(&dir, "must equal the file name \"acme-2\"");
}

#[test]
fn inexact_micro_usd_is_a_warning_not_an_error() {
    let dir = Scratch::new();
    dir.write("providers/acme/provider.toml", PROVIDER).write(
        "providers/acme/models/acme-1.toml",
        &model(&format!(
            "[[offerings.prices]]\neffective_from = \"2026-10-09\"\ncurrency = \"USD\"\nstandard = {{ input_tokens = \"0.0000005\" }}\n{SOURCE}\n"
        )),
    );
    let (catalog, issues) = load_validated(&dir.0);
    assert!(
        issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains("rounds it up to 1"))
    );
    let artifacts = export::build(&catalog.unwrap());
    let prices: Value = serde_json::from_slice(&artifacts["api/v1/omg-prices.json"]).unwrap();
    let line = &prices["data"][0]["price_lines"][0];
    assert_eq!(line["microusd_per_batch"], "1");
    assert_eq!(line["rounded_up"], true);
    assert_eq!(prices["data"][0]["rounded_up"], true);
}

fn seed_artifacts() -> export::Artifacts {
    let (catalog, _) = load_validated(&seed_dir());
    export::build(&catalog.unwrap())
}

fn keys_sorted(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            let keys: Vec<_> = map.keys().collect();
            keys.windows(2).all(|w| w[0] < w[1]) && map.values().all(keys_sorted)
        }
        Value::Array(items) => items.iter().all(keys_sorted),
        _ => true,
    }
}

#[test]
fn build_is_deterministic_with_sorted_keys() {
    let first = seed_artifacts();
    let second = seed_artifacts();
    assert_eq!(first, second);
    for (path, bytes) in &first {
        let value: Value = serde_json::from_slice(bytes).unwrap();
        assert!(keys_sorted(&value), "{path} has unsorted keys");
        assert!(bytes.ends_with(b"\n"));
    }
    for path in [
        "api.json",
        "api/v1/index.json",
        "api/v1/providers.json",
        "api/v1/models.json",
        "api/v1/omg-prices.json",
        "api/v1/history.json",
        "api/v1/models/claude-sonnet-5-5.json",
        "api/v1/providers/amazon-bedrock.json",
    ] {
        assert!(first.contains_key(path), "missing {path}");
    }
}

#[test]
fn omg_export_converts_exactly() {
    let artifacts = seed_artifacts();
    let prices: Value = serde_json::from_slice(&artifacts["api/v1/omg-prices.json"]).unwrap();
    let offering = |provider: &str, upstream: &str| {
        prices["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["provider"] == provider && o["upstream_id"] == upstream)
            .unwrap_or_else(|| panic!("{provider}:{upstream} missing"))
            .clone()
    };
    let line = |offering: &Value, key: &str, meter: &str, tier: Option<u64>| {
        offering[key]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["meter"] == meter && l["min_prompt_tokens"].as_u64() == tier)
            .unwrap_or_else(|| panic!("{meter} {tier:?} missing"))
            .clone()
    };

    let haiku = offering("anthropic", "claude-haiku-5-5");
    let input = line(&haiku, "price_lines", "input_tokens", None);
    assert_eq!(input["microusd_per_batch"], "100000");
    assert_eq!(input["batch"], 1_000_000);
    assert_eq!(input["unit_label"], "/M tokens");
    let tier = line(
        &haiku,
        "price_lines",
        "cache_write_5m_tokens",
        Some(100_000),
    );
    assert_eq!(tier["microusd_per_batch"], "625000");
    let search = line(&haiku, "price_lines", "search_units", None);
    assert_eq!(search["microusd_per_batch"], "10000");
    assert_eq!(search["batch"], 1);
    assert_eq!(haiku["rounded_up"], false);
    // Anthropic publishes batch input/output only; OMG needs full parity.
    assert_eq!(haiku["batch_complete"], false);

    let oss = offering("openrouter", "openai/gpt-oss-120b");
    assert_eq!(
        line(&oss, "price_lines", "input_tokens", None)["microusd_per_batch"],
        "37000"
    );
    assert_eq!(
        line(&oss, "batch_price_lines", "input_tokens", None)["microusd_per_batch"],
        "29600"
    );
    assert_eq!(oss["batch_complete"], true);

    let bedrock = offering("amazon-bedrock", "us.anthropic.claude-haiku-5-5");
    assert_eq!(
        line(&bedrock, "price_lines", "cache_write_5m_tokens", None)["microusd_per_batch"],
        "137500"
    );
    assert!(bedrock["batch_price_lines"].is_null());
}

#[test]
fn models_dev_shape() {
    let artifacts = seed_artifacts();
    let api: Value = serde_json::from_slice(&artifacts["api.json"]).unwrap();
    let sonnet = &api["anthropic"]["models"]["claude-sonnet-5-5"];
    assert_eq!(sonnet["cost"]["input"].to_string(), "2");
    assert_eq!(sonnet["cost"]["cache_read"].to_string(), "0.1");
    assert_eq!(sonnet["cost"]["cache_write"].to_string(), "2.5");
    assert_eq!(sonnet["limit"]["context"], 1_000_000);
    assert_eq!(sonnet["catalog_id"], "claude-sonnet-5-5");
    let haiku = &api["amazon-bedrock"]["models"]["global.anthropic.claude-haiku-5-5"];
    assert_eq!(haiku["cost"]["tiers"][0]["tier"]["size"], 100_000);
    assert_eq!(haiku["cost"]["tiers"][0]["input"].to_string(), "0.5");
    assert_eq!(
        api["openrouter"]["models"]["meta-llama/llama-4-maverick"]["limit"]["output"],
        16_384
    );
}

#[test]
fn in_memory_tree_loads_like_the_directory() {
    use omg_models_catalog::{load::read_files, load_validated_files};
    let (disk, disk_issues) = load_validated(&seed_dir());
    let files = read_files(&seed_dir()).expect("read seed data");
    assert!(files.keys().all(|p| p.starts_with("providers/")));
    let (memory, memory_issues) = load_validated_files(&files);
    assert_eq!(disk_issues, memory_issues);
    assert_eq!(
        export::build(&disk.expect("seed validates")),
        export::build(&memory.expect("seed validates in memory"))
    );

    // Errors carry the same paths in memory as on disk.
    let mut broken = files.clone();
    broken.insert(
        "providers/openai/models/notes.txt".into(),
        b"stray".to_vec(),
    );
    broken.insert(
        "providers/openai/models/gpt-6-luna.toml".into(),
        b"id = \"gpt-6-luna\"\nname = 3\n".to_vec(),
    );
    let (catalog, issues) = load_validated_files(&broken);
    assert!(catalog.is_none());
    let paths: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.path.as_str())
        .collect();
    assert!(
        paths.contains(&"providers/openai/models/notes.txt"),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"providers/openai/models/gpt-6-luna.toml"),
        "{paths:?}"
    );
}
