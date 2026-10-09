//! `omg-models sync`: compare the catalog with upstream price sources,
//! report the differences, and append changes as new price entries.
//!
//! Sources (chosen in `.local/.../price-sources-research.md`):
//! - **LiteLLM** `model_prices_and_context_window.json` (MIT): primary for
//!   providers with `sync.prices = "litellm"`.
//! - **OpenRouter** `/api/v1/models`: primary for `sync.prices = "openrouter"`.
//! - **pydantic genai-prices v2** (MIT): cross-check only, never written.
//! - **models.dev** `api.json` (MIT): fills missing model metadata only.
//!
//! Each price source implements [`PriceSource`]; adding one means a parser
//! module, a [`PriceSourceKind`] value and a line in [`Sources::load`].

pub mod apply;
pub mod fetch;
pub mod genai;
pub mod litellm;
pub mod models_dev;
pub mod observed;
pub mod openrouter;
pub mod plan;
pub mod report;

use std::{fs, path::PathBuf, process::ExitCode};

use anyhow::{Context, bail};
use clap::{Args, ValueEnum};
use omg_models_catalog::{
    Severity, load_validated,
    model::{ModelFile, Offering, PriceSourceKind, ProviderFile, SourceKind},
    time::{Date, Timestamp},
};
use serde::Serialize;

use self::{
    fetch::Fetcher,
    genai::GenaiPrices,
    litellm::LiteLlm,
    models_dev::ModelsDev,
    observed::{Document, Lookup, ObservedPrice},
    openrouter::OpenRouter,
};

/// The offering being looked up, with its provider and model.
pub struct OfferingRef<'a> {
    pub provider: &'a ProviderFile,
    pub model: &'a ModelFile,
    pub offering: &'a Offering,
}

impl OfferingRef<'_> {
    /// Claude-style prompt caches price 5-minute and 1-hour writes apart;
    /// a source's single "cache write" rate is then the 5-minute rate.
    pub fn claude_cache(&self) -> bool {
        self.model.vendor.eq_ignore_ascii_case("anthropic")
            || self.offering.upstream_id.contains("claude")
    }
}

/// A source of prices for offerings.
pub trait PriceSource {
    fn kind(&self) -> SourceKind;
    fn document(&self) -> &Document;
    fn lookup(&self, target: &OfferingRef<'_>) -> Lookup<ObservedPrice>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SourceName {
    Litellm,
    GenaiPrices,
    Openrouter,
    ModelsDev,
}

impl SourceName {
    pub const ALL: [Self; 4] = [
        Self::Litellm,
        Self::GenaiPrices,
        Self::Openrouter,
        Self::ModelsDev,
    ];

    /// File name used by `--from-dir` and the cache.
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::Litellm => "litellm",
            Self::GenaiPrices => "genai-prices",
            Self::Openrouter => "openrouter",
            Self::ModelsDev => "models-dev",
        }
    }
}

/// Fetch outcome per source, for the report.
#[derive(Clone, Debug, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub url: String,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub fetched_at: Option<String>,
    pub error: Option<String>,
    pub warning: Option<String>,
}

/// Every loaded source.
#[derive(Default)]
pub struct Sources {
    pub litellm: Option<LiteLlm>,
    pub genai: Option<GenaiPrices>,
    pub openrouter: Option<OpenRouter>,
    pub models_dev: Option<ModelsDev>,
    /// Whether the cross-check was requested (so its absence matters).
    pub wants_cross_check: bool,
    pub status: Vec<SourceStatus>,
}

impl Sources {
    pub fn primary(&self, kind: PriceSourceKind) -> Option<&dyn PriceSource> {
        match kind {
            PriceSourceKind::Litellm => self.litellm.as_ref().map(|s| s as &dyn PriceSource),
            PriceSourceKind::Openrouter => self.openrouter.as_ref().map(|s| s as &dyn PriceSource),
            PriceSourceKind::None => None,
        }
    }

    fn record(
        &mut self,
        name: SourceName,
        url: &str,
        result: &anyhow::Result<Document>,
        warning: Option<String>,
    ) {
        self.status.push(SourceStatus {
            name: name.file_stem().to_owned(),
            url: result
                .as_ref()
                .map_or_else(|_| url.to_owned(), |d| d.url.clone()),
            version: result.as_ref().ok().and_then(|d| d.version.clone()),
            sha256: result.as_ref().ok().map(|d| d.sha256.clone()),
            fetched_at: result.as_ref().ok().map(|d| d.fetched_at.to_string()),
            error: result.as_ref().err().map(|e| format!("{e:#}")),
            warning,
        });
    }

    /// Loads the selected sources from the network or `from_dir`.
    pub fn load(
        selected: &[SourceName],
        from_dir: Option<&PathBuf>,
        cache_dir: Option<PathBuf>,
        today: Date,
    ) -> Self {
        let fetcher = Fetcher::new(cache_dir);
        let mut sources = Self {
            wants_cross_check: selected.contains(&SourceName::GenaiPrices),
            ..Self::default()
        };
        for name in selected {
            let (kind, url, repo) = match name {
                SourceName::Litellm => (
                    SourceKind::Litellm,
                    litellm::URL,
                    Some((litellm::REPO, litellm::PATH)),
                ),
                SourceName::GenaiPrices => (
                    SourceKind::GenaiPrices,
                    genai::URL,
                    Some((genai::REPO, genai::PATH)),
                ),
                SourceName::Openrouter => (SourceKind::Openrouter, openrouter::URL, None),
                SourceName::ModelsDev => (SourceKind::ModelsDev, models_dev::URL, None),
            };
            let (document, warning) = match from_dir {
                Some(dir) => (
                    fetch::from_file(
                        &dir.join(format!("{}.json", name.file_stem())),
                        kind,
                        url,
                        "recorded-fixture",
                    ),
                    None,
                ),
                None => match repo {
                    Some((repo, path)) => {
                        match fetcher.fetch_github(name.file_stem(), kind, repo, path, url) {
                            Ok((doc, warning)) => (Ok(doc), warning),
                            Err(error) => (Err(error), None),
                        }
                    }
                    None => (fetcher.fetch(name.file_stem(), kind, url), None),
                },
            };
            let parsed = document.and_then(|doc| {
                let status_doc = doc.clone();
                let result = match name {
                    SourceName::Litellm => LiteLlm::parse(doc).map(|s| sources.litellm = Some(s)),
                    SourceName::GenaiPrices => {
                        GenaiPrices::parse(doc, today).map(|s| sources.genai = Some(s))
                    }
                    SourceName::Openrouter => {
                        OpenRouter::parse(doc).map(|s| sources.openrouter = Some(s))
                    }
                    SourceName::ModelsDev => {
                        ModelsDev::parse(doc).map(|s| sources.models_dev = Some(s))
                    }
                };
                result.map(|()| status_doc)
            });
            sources.record(*name, url, &parsed, warning);
        }
        sources
    }
}

#[derive(Args)]
pub struct SyncArgs {
    #[arg(long, env = "OMG_MODELS_DATA", default_value = "data")]
    pub data: PathBuf,
    /// Report what would change without writing files.
    #[arg(long)]
    pub dry_run: bool,
    /// Sources to use (default: all).
    #[arg(long, value_enum, value_delimiter = ',')]
    pub sources: Vec<SourceName>,
    /// Read recorded source files (`litellm.json`, `genai-prices.json`,
    /// `openrouter.json`, `models-dev.json`) instead of fetching.
    #[arg(long)]
    pub from_dir: Option<PathBuf>,
    /// Cache for conditional requests (ETag) between runs.
    #[arg(long, default_value = ".sync/cache")]
    pub cache_dir: PathBuf,
    /// Write the Markdown change summary here (also printed to stdout).
    #[arg(long)]
    pub report: Option<PathBuf>,
    /// Write the JSON summary (label, reasons, changes) here.
    #[arg(long)]
    pub summary: Option<PathBuf>,
    /// Override today's date (YYYY-MM-DD), for reproducible runs.
    #[arg(long)]
    pub today: Option<String>,
}

pub fn run(args: &SyncArgs) -> anyhow::Result<ExitCode> {
    let today = match &args.today {
        Some(text) => Date::parse(text).context("--today must be YYYY-MM-DD")?,
        None => Timestamp::now().date(),
    };
    let (catalog, issues) = load_validated(&args.data);
    let Some(catalog) = catalog else {
        for issue in issues.iter().filter(|i| i.severity == Severity::Error) {
            eprintln!("{issue}");
        }
        bail!("the catalog must validate before syncing");
    };
    let selected = if args.sources.is_empty() {
        SourceName::ALL.to_vec()
    } else {
        args.sources.clone()
    };
    let sources = Sources::load(
        &selected,
        args.from_dir.as_ref(),
        Some(args.cache_dir.clone()),
        today,
    );
    for status in &sources.status {
        match (&status.error, &status.warning) {
            (Some(error), _) => eprintln!("source {}: unavailable: {error}", status.name),
            (None, Some(warning)) => eprintln!("source {}: {warning}", status.name),
            (None, None) => eprintln!(
                "source {}: {} ({})",
                status.name,
                status.version.as_deref().unwrap_or("unversioned"),
                status.url
            ),
        }
    }
    let primary_down = sources.status.iter().any(|s| {
        s.error.is_some()
            && (s.name == SourceName::Litellm.file_stem()
                || s.name == SourceName::Openrouter.file_stem())
    });
    let plan = plan::build(&catalog, &sources, today);
    let markdown = report::markdown(&plan, &sources.status);
    let summary = report::summary_json(&plan, &sources.status);
    print!("{markdown}");
    if let Some(path) = &args.report {
        fs::write(path, &markdown).with_context(|| format!("writing {}", path.display()))?;
    }
    if let Some(path) = &args.summary {
        let mut text = serde_json::to_string_pretty(&summary)?;
        text.push('\n');
        fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    if args.dry_run || !plan.has_changes() {
        eprintln!(
            "{}: {} price change(s), {} metadata fill(s); label {}",
            if args.dry_run {
                "dry run"
            } else {
                "no changes"
            },
            plan.price_changes.len(),
            plan.metadata_fills.len(),
            plan.label().0.as_str()
        );
    } else {
        let written = apply::apply(&args.data, &plan)?;
        let (after, issues) = load_validated(&args.data);
        if after.is_none() {
            for issue in issues.iter().filter(|i| i.severity == Severity::Error) {
                eprintln!("{issue}");
            }
            bail!(
                "the synced catalog does not validate; not keeping a broken state (revert with git)"
            );
        }
        eprintln!(
            "applied {written} edit(s); label {}",
            plan.label().0.as_str()
        );
    }
    Ok(if primary_down {
        // Surface a failed primary source to the scheduler.
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}
