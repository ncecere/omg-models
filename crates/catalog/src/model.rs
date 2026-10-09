//! The data files: `providers/<provider>/provider.toml` and
//! `providers/<provider>/models/<model>.toml`. See `docs/data-format.md`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    decimal::Decimal,
    meter::Meter,
    time::{Date, PartialDate, Timestamp},
};

/// `providers/<id>/provider.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderFile {
    /// Provider id; must equal the directory name. `[a-z0-9][a-z0-9-]*`.
    pub id: String,
    /// Display name, e.g. "Amazon Bedrock".
    pub name: String,
    /// What kind of provider this is.
    #[serde(rename = "type")]
    pub kind: ProviderKind,
    /// Public website (https).
    pub website: String,
    /// API base URL (https; http allowed only for self-hosted providers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    /// Which wire protocol the API examples use.
    #[serde(default)]
    pub api_style: ApiStyle,
    /// How clients authenticate (plain text, no secrets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    /// Environment variables conventionally holding credentials.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    /// API documentation (https).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// The provider's official pricing page (https), for verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing_page: Option<String>,
    /// How `omg-models sync` tracks this provider.
    #[serde(default)]
    pub sync: ProviderSync,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// A first-party model API or a cloud platform (OpenAI, Anthropic, Bedrock).
    Cloud,
    /// A router/reseller in front of other providers (OpenRouter).
    Aggregator,
    /// Software you run yourself (vLLM, Ollama); usually no public price.
    SelfHosted,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cloud => "Cloud",
            Self::Aggregator => "Aggregator",
            Self::SelfHosted => "Self-hosted",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ApiStyle {
    /// OpenAI Chat Completions compatible (`POST {api_base}/chat/completions`).
    #[default]
    OpenaiChat,
    /// OpenAI Responses API (`POST {api_base}/responses`).
    OpenaiResponses,
    /// Anthropic Messages API (`POST {api_base}/messages`).
    AnthropicMessages,
    /// Amazon Bedrock Converse API (SigV4).
    BedrockConverse,
}

/// Provider-level sync settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderSync {
    /// The primary price source for this provider's offerings.
    #[serde(default)]
    pub prices: PriceSourceKind,
    /// Accepted LiteLLM `litellm_provider` values (for `prices = "litellm"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub litellm_providers: Vec<String>,
    /// pydantic genai-prices provider id for cross-checks (`anthropic`, `aws`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genai_prices_provider: Option<String>,
    /// models.dev provider id for metadata (`anthropic`, `amazon-bedrock`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_dev_provider: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PriceSourceKind {
    /// Not synced; prices are maintained by hand.
    #[default]
    None,
    /// BerriAI/litellm `model_prices_and_context_window.json`.
    Litellm,
    /// OpenRouter `GET /api/v1/models` (and `:batch` ids).
    Openrouter,
}

/// `providers/<home>/models/<id>.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    /// Catalog model id; must equal the file name. Unique across the catalog.
    pub id: String,
    /// Display name, e.g. "Claude Sonnet 5.5".
    pub name: String,
    /// Who made the model (lab/vendor), e.g. "Anthropic".
    pub vendor: String,
    /// Model family, e.g. "claude-sonnet".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// First public release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<Date>,
    /// Knowledge cutoff, `YYYY-MM` or `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_cutoff: Option<PartialDate>,
    /// Whether the trained weights are publicly downloadable.
    pub open_weights: bool,
    /// Weights licence (SPDX id or name), for open-weights models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Model documentation (https).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<bool>,
    /// Lifecycle status.
    #[serde(default)]
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecation_date: Option<Date>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement_date: Option<Date>,
    pub modalities: Modalities,
    #[serde(default)]
    pub limits: Limits,
    /// Where the model is served. The home provider (the directory) must be one.
    #[serde(default)]
    pub offerings: Vec<Offering>,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    #[default]
    Active,
    Deprecated,
    Retired,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Deprecated => "Deprecated",
            Self::Retired => "Retired",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Modalities {
    pub input: Vec<Modality>,
    pub output: Vec<Modality>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
    Embedding,
}

impl Modality {
    pub const ALL: [Self; 6] = [
        Self::Text,
        Self::Image,
        Self::Audio,
        Self::Video,
        Self::Pdf,
        Self::Embedding,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Pdf => "pdf",
            Self::Embedding => "embedding",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Token limits. Unknown limits are omitted, never zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Context window (input + output) in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Maximum input tokens, when smaller than the context window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    /// Maximum output tokens per request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
}

/// One way to call the model: a provider and the id that provider expects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Offering {
    /// Provider id.
    pub provider: String,
    /// The model id on that provider's wire API (Bedrock profile ids keep
    /// their `global.`/`us.` prefix: the prefix changes the price).
    pub upstream_id: String,
    /// Short qualifier shown next to the provider, e.g. "US cross-Region".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Other ids that reach the same deployment at the same price.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Per-offering limits when they differ from the model's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    /// Source keys for `omg-models sync` when they differ from `upstream_id`.
    #[serde(default, skip_serializing_if = "OfferingSync::is_empty")]
    pub sync: OfferingSync,
    /// Append-only price history, oldest first. Empty means "price unknown".
    #[serde(default)]
    pub prices: Vec<PriceEntry>,
}

/// Overrides for how sync finds this offering in each source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OfferingSync {
    /// Exclude this offering from price sync.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// LiteLLM key (default: `upstream_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub litellm: Option<String>,
    /// genai-prices model id to match (default: `upstream_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genai_prices: Option<String>,
    /// OpenRouter model id (default: `upstream_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter: Option<String>,
    /// models.dev model key within the provider's `models_dev_provider`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_dev: Option<String>,
}

impl OfferingSync {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Rates per meter, exact USD per the meter's unit.
pub type RateCard = BTreeMap<Meter, Decimal>;

/// One price, effective from a date until the next entry. Never edited after
/// publication: a change is a new entry with a later `effective_from`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceEntry {
    pub effective_from: Date,
    pub currency: Currency,
    /// Interactive (standard) rates.
    pub standard: RateCard,
    /// Prompt-size tiers for the standard rates, ascending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standard_tiers: Vec<Tier>,
    /// Batch API rates, when the provider publishes them. Never derived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<RateCard>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub batch_tiers: Vec<Tier>,
    /// Meters that cannot apply to this offering (different from unknown).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_applicable: Vec<Meter>,
    pub source: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Rates that replace the base rates for the whole request when the prompt
/// (all input tokens, including cache reads and writes) is **strictly
/// greater** than `above_prompt_tokens`. The highest exceeded tier wins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub above_prompt_tokens: u64,
    pub rates: RateCard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Currency {
    #[default]
    #[serde(rename = "USD")]
    Usd,
}

/// Where a price came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub kind: SourceKind,
    /// The page or file the values were read from (https).
    pub url: String,
    /// When it was fetched (UTC).
    pub fetched_at: Timestamp,
    /// Source version: git commit, ETag or publication date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// SHA-256 of the fetched document (lowercase hex).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// The entry key inside the source document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_key: Option<String>,
    /// The provider pricing page the source itself cites, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cites: Option<String>,
    /// Meters copied unchanged from the previous entry because this source
    /// does not list them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carried_over: Vec<Meter>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// Entered by a maintainer (explain in `notes`).
    Manual,
    /// The provider's own pricing or model page.
    ProviderPage,
    /// A provider's machine-readable price list (e.g. AWS offer files).
    ProviderPriceList,
    /// BerriAI/litellm `model_prices_and_context_window.json` (MIT).
    Litellm,
    /// pydantic/genai-prices v2 data (MIT).
    GenaiPrices,
    /// OpenRouter's public models API.
    Openrouter,
    /// models.dev `api.json` (MIT).
    #[serde(rename = "models.dev")]
    ModelsDev,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual entry",
            Self::ProviderPage => "Provider pricing page",
            Self::ProviderPriceList => "Provider price list",
            Self::Litellm => "LiteLLM",
            Self::GenaiPrices => "genai-prices",
            Self::Openrouter => "OpenRouter API",
            Self::ModelsDev => "models.dev",
        }
    }

    /// Whether this kind is an automated third-party source (needs entry key).
    pub fn is_dataset(self) -> bool {
        matches!(
            self,
            Self::Litellm | Self::GenaiPrices | Self::Openrouter | Self::ModelsDev
        )
    }
}

impl PriceEntry {
    /// All rate cards with their tier label, for iteration.
    pub fn standard_rate(&self, meter: Meter) -> Option<Decimal> {
        self.standard.get(&meter).copied()
    }
}
