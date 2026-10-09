//! `/api`: the JSON endpoints, formats and an example.

use topcoat::{
    Result,
    context::Cx,
    router::page,
    view::{View, attributes, view},
};

use crate::{
    components::table::{table, table_body, table_cell, table_head, table_header, table_row},
    state::{self, AppState},
    ui::code_block,
};

const ENDPOINTS: [(&str, &str); 8] = [
    (
        "/api.json",
        "Everything in the models.dev shape: providers \u{2192} models (keyed by the provider's model id) \u{2192} cost per 1M tokens, limits, modalities.",
    ),
    (
        "/api/v1/index.json",
        "Counts, last update, licence and the endpoint list.",
    ),
    ("/api/v1/providers.json", "All providers."),
    (
        "/api/v1/providers/{provider}.json",
        "One provider and every model it serves, with current prices.",
    ),
    (
        "/api/v1/models.json",
        "All models, each with its offerings and current prices (exact decimal strings).",
    ),
    (
        "/api/v1/models/{model}.json",
        "One model with every provider's full, append-only price history and provenance.",
    ),
    (
        "/api/v1/omg-prices.json",
        "Open Model Gateway Pricing v3 price lines: integer micro-USD per batch, ready to publish.",
    ),
    (
        "/api/v1/history.json",
        "Every price entry ever recorded, newest first.",
    ),
];

/// The first offering of the OMG export, trimmed for display.
fn omg_example(state: &AppState) -> String {
    let Some(file) = state.files.get("/api/v1/omg-prices.json") else {
        return String::new();
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&file.bytes) else {
        return String::new();
    };
    let Some(first) = value.get_mut("data").and_then(|d| d.get_mut(0)) else {
        return String::new();
    };
    let mut first = first.take();
    for key in ["price_lines", "batch_price_lines"] {
        if let Some(lines) = first.get_mut(key).and_then(|l| l.as_array_mut()) {
            lines.truncate(2);
        }
    }
    if let Some(provenance) = first.get_mut("provenance").and_then(|p| p.as_object_mut()) {
        provenance.retain(|k, _| matches!(k.as_str(), "kind" | "url" | "fetched_at"));
    }
    serde_json::to_string_pretty(&first).unwrap_or_default()
}

#[page]
async fn api_page(cx: &Cx) -> Result<impl View> {
    let state = state::current(cx);
    let example = omg_example(state);
    Ok(view! {
        <h1 class="mb-2 text-3xl font-semibold tracking-tight">"JSON API"</h1>
        <p class="mb-6 max-w-3xl text-muted-foreground">
            "Static JSON files, rebuilt from the catalog whenever its data changes; " <a href="/api/status"><code>"/api/status"</code></a> " names the data snapshot being served. No key, no sign-up, no rate limits beyond fair use. Responses carry "
            <code>"Access-Control-Allow-Origin: *"</code> ", an " <code>"ETag"</code> " and "
            <code>"Cache-Control: public, max-age=300"</code> "; send " <code>"If-None-Match"</code> " to get a 304 when nothing changed."
        </p>

        <h2 class="mb-3 text-xl font-semibold">"Endpoints"</h2>
        <div class="mb-8 rounded-xl border border-border bg-card shadow-xs">
            table(
                table_header(table_row(
                    table_head(attrs: attributes! { scope="col" }, "Path")
                    table_head(attrs: attributes! { scope="col" }, "Contents")
                ))
                table_body(
                    for (path, description) in ENDPOINTS {
                        table_row(
                            table_cell(
                                if path.contains('{') {
                                    <code>(path)</code>
                                } else {
                                    <a href=(path)><code>(path)</code></a>
                                }
                            )
                            table_cell(<div class="min-w-64 whitespace-normal text-muted-foreground">(description)</div>)
                        )
                    }
                )
            )
        </div>

        <h2 class="mb-3 text-xl font-semibold">"Formats"</h2>
        <ul class="mb-8 max-w-3xl list-disc space-y-2 pl-5 text-sm">
            <li>"Prices in " <code>"/api/v1/"</code> " are exact decimal strings in USD per unit (" <code>"\"2.5\""</code> " per 1M tokens), never floats. A missing meter means unknown, not free; " <code>"\"0\""</code> " is free."</li>
            <li><code>"/api.json"</code> " follows models.dev, so its costs are JSON numbers. The numbers are written exactly as stored (no float rounding). " <code>"cache_write"</code> " is the 5-minute write where a provider prices cache TTLs separately; tiers are " <code>"cost.tiers[].tier.size"</code> ", applying above that prompt size. It adds " <code>"catalog_id"</code> " and has no npm or AI SDK fields."</li>
            <li>"Prompt-size tiers apply to the whole request when the prompt (all input tokens, cache reads and writes included) is strictly greater than the threshold."</li>
            <li>"Batch rates appear only when the provider publishes them; they are never derived from standard rates."</li>
            <li><code>"omg-prices.json"</code> " converts each rate to integer micro-USD per Open Model Gateway batch (1M tokens, 1 image, 1,000 ms of audio, ...). A rate that is not a whole micro-USD is rounded up and flagged " <code>"rounded_up"</code> "; " <code>"batch_complete"</code> " is false when batch lines do not cover the same meters as standard lines, which the gateway requires."</li>
            <li>"The data-file schema is at " <a href="/api/v1/schema.json"><code>"/api/v1/schema.json"</code></a> " (JSON Schema for the TOML data files)."</li>
        </ul>

        <h2 class="mb-3 text-xl font-semibold">"Example"</h2>
        <div class="grid gap-4">
            code_block(label: Some("Fetch the gateway price lines".into()), code: "curl -s https://models.omg.bitop.dev/api/v1/omg-prices.json | jq '.data[0]'".into())
            code_block(label: Some("First entry (lines trimmed to two)".into()), code: example)
        </div>
    })
}
