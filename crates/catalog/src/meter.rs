//! Price meters. Each meter has one canonical unit; catalog rates are exact
//! USD per that unit. The OMG v3 export maps every meter to a gateway meter,
//! batch size and unit label (see `docs/data-format.md`).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a price line charges for.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum Meter {
    /// Uncached input tokens, USD per 1M tokens.
    #[serde(rename = "input_tokens")]
    InputTokens,
    /// Output tokens (including reasoning tokens), USD per 1M tokens.
    #[serde(rename = "output_tokens")]
    OutputTokens,
    /// Cache read (hit/refresh) input tokens, USD per 1M tokens.
    #[serde(rename = "cache_read_tokens")]
    CacheReadTokens,
    /// Cache write tokens with the provider's default TTL, USD per 1M tokens.
    #[serde(rename = "cache_write_tokens")]
    CacheWriteTokens,
    /// Cache write tokens with a 5-minute TTL, USD per 1M tokens.
    #[serde(rename = "cache_write_5m_tokens")]
    CacheWrite5mTokens,
    /// Cache write tokens with a 1-hour TTL, USD per 1M tokens.
    #[serde(rename = "cache_write_1h_tokens")]
    CacheWrite1hTokens,
    /// Input characters (for example text to speech), USD per 1M characters.
    #[serde(rename = "input_characters")]
    InputCharacters,
    /// Generated images, USD per image.
    #[serde(rename = "output_images")]
    OutputImages,
    /// Input audio duration, USD per second.
    #[serde(rename = "input_audio_seconds")]
    InputAudioSeconds,
    /// Output audio duration, USD per second.
    #[serde(rename = "output_audio_seconds")]
    OutputAudioSeconds,
    /// Search/tool units (for example web searches), USD per unit.
    #[serde(rename = "search_units")]
    SearchUnits,
    /// Flat per-request charge, USD per request.
    #[serde(rename = "requests")]
    Requests,
}

impl Meter {
    pub const ALL: [Self; 12] = [
        Self::InputTokens,
        Self::OutputTokens,
        Self::CacheReadTokens,
        Self::CacheWriteTokens,
        Self::CacheWrite5mTokens,
        Self::CacheWrite1hTokens,
        Self::InputCharacters,
        Self::OutputImages,
        Self::InputAudioSeconds,
        Self::OutputAudioSeconds,
        Self::SearchUnits,
        Self::Requests,
    ];

    /// The catalog key (`input_tokens`, ...).
    pub fn key(self) -> &'static str {
        match self {
            Self::InputTokens => "input_tokens",
            Self::OutputTokens => "output_tokens",
            Self::CacheReadTokens => "cache_read_tokens",
            Self::CacheWriteTokens => "cache_write_tokens",
            Self::CacheWrite5mTokens => "cache_write_5m_tokens",
            Self::CacheWrite1hTokens => "cache_write_1h_tokens",
            Self::InputCharacters => "input_characters",
            Self::OutputImages => "output_images",
            Self::InputAudioSeconds => "input_audio_seconds",
            Self::OutputAudioSeconds => "output_audio_seconds",
            Self::SearchUnits => "search_units",
            Self::Requests => "requests",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|meter| meter.key() == key)
    }

    /// Whether the meter counts tokens (rates per 1M tokens).
    pub fn is_token(self) -> bool {
        matches!(
            self,
            Self::InputTokens
                | Self::OutputTokens
                | Self::CacheReadTokens
                | Self::CacheWriteTokens
                | Self::CacheWrite5mTokens
                | Self::CacheWrite1hTokens
        )
    }

    /// Human label, e.g. "Cache write (1h)".
    pub fn label(self) -> &'static str {
        match self {
            Self::InputTokens => "Input",
            Self::OutputTokens => "Output",
            Self::CacheReadTokens => "Cache read",
            Self::CacheWriteTokens => "Cache write",
            Self::CacheWrite5mTokens => "Cache write (5m)",
            Self::CacheWrite1hTokens => "Cache write (1h)",
            Self::InputCharacters => "Input characters",
            Self::OutputImages => "Output images",
            Self::InputAudioSeconds => "Input audio",
            Self::OutputAudioSeconds => "Output audio",
            Self::SearchUnits => "Search",
            Self::Requests => "Requests",
        }
    }

    /// The catalog unit, e.g. "per 1M tokens".
    pub fn unit(self) -> &'static str {
        match self {
            Self::InputTokens
            | Self::OutputTokens
            | Self::CacheReadTokens
            | Self::CacheWriteTokens
            | Self::CacheWrite5mTokens
            | Self::CacheWrite1hTokens => "per 1M tokens",
            Self::InputCharacters => "per 1M characters",
            Self::OutputImages => "per image",
            Self::InputAudioSeconds | Self::OutputAudioSeconds => "per second",
            Self::SearchUnits => "per search",
            Self::Requests => "per request",
        }
    }

    /// OMG Pricing v3 meter name.
    pub fn omg_meter(self) -> &'static str {
        match self {
            Self::InputAudioSeconds => "input_audio_seconds_ms",
            Self::OutputAudioSeconds => "output_audio_seconds_ms",
            other => other.key(),
        }
    }

    /// OMG Pricing v3 batch size for one catalog unit. In every case the
    /// micro-USD per OMG batch equals the catalog's USD-per-unit rate × 10^6.
    pub fn omg_batch(self) -> u64 {
        match self {
            Self::InputTokens
            | Self::OutputTokens
            | Self::CacheReadTokens
            | Self::CacheWriteTokens
            | Self::CacheWrite5mTokens
            | Self::CacheWrite1hTokens
            | Self::InputCharacters => 1_000_000,
            Self::InputAudioSeconds | Self::OutputAudioSeconds => 1_000,
            Self::OutputImages | Self::SearchUnits | Self::Requests => 1,
        }
    }

    /// OMG Pricing v3 canonical unit label.
    pub fn omg_unit_label(self) -> &'static str {
        match self {
            Self::InputTokens
            | Self::OutputTokens
            | Self::CacheReadTokens
            | Self::CacheWriteTokens
            | Self::CacheWrite5mTokens
            | Self::CacheWrite1hTokens => "/M tokens",
            Self::InputCharacters => "/M characters",
            Self::OutputImages => "/image",
            Self::InputAudioSeconds | Self::OutputAudioSeconds => "/second",
            Self::SearchUnits => "/search",
            Self::Requests => "/request",
        }
    }
}

impl fmt::Display for Meter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}
