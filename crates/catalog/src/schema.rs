//! JSON Schema for the data files (`omg-models schema`). Editors such as
//! Taplo/Even Better TOML can use it for completion and checking.

use crate::model::{ModelFile, ProviderFile};

/// `{ "provider": <schema>, "model": <schema> }`, pretty-printed.
pub fn schemas_json() -> String {
    let mut provider = schemars::schema_for!(ProviderFile);
    provider.insert(
        "$id".into(),
        "https://models.omg.bitop.dev/schema/provider.json".into(),
    );
    let mut model = schemars::schema_for!(ModelFile);
    model.insert(
        "$id".into(),
        "https://models.omg.bitop.dev/schema/model.json".into(),
    );
    let value = serde_json::json!({ "provider": provider, "model": model });
    let mut text = serde_json::to_string_pretty(&value).expect("schema serializes");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    #[test]
    fn schema_mentions_meters() {
        let text = super::schemas_json();
        assert!(text.contains("cache_write_1h_tokens"));
        assert!(text.contains("above_prompt_tokens"));
        assert!(text.contains("\"Decimal\""));
    }
}
