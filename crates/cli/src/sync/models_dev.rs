//! models.dev `api.json` (MIT): model metadata only (release date,
//! knowledge cutoff, limits). Its prices are not used: it has no batch
//! rates and no 1-hour cache writes.

use anyhow::{Context, bail};
use serde_json::Value;

use super::{
    OfferingRef,
    observed::{Document, Lookup, ObservedMetadata},
};

pub const URL: &str = "https://models.dev/api.json";

pub struct ModelsDev {
    document: Document,
    root: serde_json::Map<String, Value>,
}

impl ModelsDev {
    pub fn parse(document: Document) -> anyhow::Result<Self> {
        let value: Value = serde_json::from_slice(&document.bytes).context("models.dev JSON")?;
        let Value::Object(root) = value else {
            bail!("models.dev api.json is not an object");
        };
        Ok(Self { document, root })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn lookup(&self, target: &OfferingRef<'_>) -> Lookup<ObservedMetadata> {
        let key = target
            .offering
            .sync
            .models_dev
            .clone()
            .unwrap_or_else(|| target.offering.upstream_id.clone());
        let Some(provider) = target.provider.sync.models_dev_provider.as_deref() else {
            return Lookup::Missing {
                key,
                reason: "provider has no models_dev_provider".into(),
            };
        };
        let Some(model) = self
            .root
            .get(provider)
            .and_then(|p| p.get("models"))
            .and_then(|m| m.get(&key))
        else {
            return Lookup::Missing {
                key: format!("{provider}/{key}"),
                reason: "not in models.dev".into(),
            };
        };
        let text = |field: &str| model.get(field).and_then(Value::as_str).map(str::to_owned);
        let limit = |field: &str| {
            model
                .get("limit")
                .and_then(|l| l.get(field))
                .and_then(Value::as_u64)
                .filter(|v| *v > 0)
        };
        Lookup::Found(ObservedMetadata {
            entry_key: format!("{provider}/{key}"),
            release_date: text("release_date").filter(|d| d.len() == 10),
            knowledge_cutoff: text("knowledge"),
            context: limit("context"),
            output: limit("output"),
        })
    }
}
