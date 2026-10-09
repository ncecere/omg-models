//! `/about`: what the catalog is, data licence, sources and attribution.

use topcoat::{
    Result,
    router::page,
    view::{View, attributes, view},
};

use crate::components::table::{
    table, table_body, table_cell, table_head, table_header, table_row,
};

/// (name, url, licence, how it is used)
const SOURCES: [(&str, &str, &str, &str); 7] = [
    (
        "LiteLLM model_prices_and_context_window.json",
        "https://github.com/BerriAI/litellm",
        "MIT",
        "Primary price source for OpenAI, Anthropic and Amazon Bedrock offerings (standard, batch, cache and prompt-size tiers). Each synced entry records the commit, SHA-256 and entry key.",
    ),
    (
        "pydantic genai-prices (v2 data)",
        "https://github.com/pydantic/genai-prices",
        "MIT",
        "Cross-check only. Disagreements are reported in the sync pull request and never auto-merged; its values are not written to the catalog.",
    ),
    (
        "OpenRouter models API",
        "https://openrouter.ai/api/v1/models",
        "OpenRouter terms of service",
        "Prices for the OpenRouter provider only (its own list prices; batch from the :batch model ids).",
    ),
    (
        "models.dev api.json",
        "https://github.com/anomalyco/models.dev",
        "MIT",
        "Fills missing model metadata (release date, knowledge cutoff, limits). Not used for prices.",
    ),
    (
        "Provider pricing pages",
        "https://platform.claude.com/docs/en/about-claude/pricing",
        "Provider terms",
        "Seed prices were read from the official Anthropic and OpenAI pricing pages; each entry links the page it came from.",
    ),
    (
        "AWS Price List (Amazon Bedrock foundation models)",
        "https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonBedrockFoundationModels/current/us-east-1/index.json",
        "AWS site terms",
        "Seed Bedrock prices and verification links.",
    ),
    (
        "Inter typeface",
        "https://github.com/rsms/inter",
        "SIL Open Font License 1.1",
        "Self-hosted web font.",
    ),
];

#[page]
async fn about_page() -> Result<impl View> {
    Ok(view! {
        <article class="max-w-3xl">
            <h1 class="mb-4 text-3xl font-semibold tracking-tight">"About the Open Model Catalog"</h1>
            <div class="space-y-4 text-base leading-relaxed">
                <p>
                    "The Open Model Catalog is an open list of AI models, where you can call them and what they cost. It is part of the "
                    <a href="https://omg.bitop.dev">"Open Model Gateway"</a>
                    " family: the gateway can import its exact price lines from "
                    <a href="/api/v1/omg-prices.json"><code>"omg-prices.json"</code></a>
                    ". Gateway documentation lives at "
                    <a href="https://docs.omg.bitop.dev">"docs.omg.bitop.dev"</a> "."
                </p>
                <p>
                    "Prices are stored as exact decimals with a source link, the time they were fetched and, for automated sources, the commit and file hash. "
                    "Price history is append-only: a change adds a new entry and never rewrites an old one. An unknown price is left empty; it is never shown as zero."
                </p>
                <p>
                    "These are published list prices, not invoices. Discounts, regional uplifts, priority or flex tiers and taxes can change what you pay. Check the provider's pricing page before relying on a number."
                </p>
            </div>

            <h2 class="mt-10 mb-3 text-xl font-semibold">"Licences"</h2>
            <ul class="list-disc space-y-2 pl-5">
                <li>"Code: MIT."</li>
                <li>"Catalog data (prices, limits, metadata and the JSON API): MIT, with attribution to the sources below, whose data is MIT licensed or factual price information. Keep this notice when you redistribute the data."</li>
                <li>"The Portal logo belongs to the Open Model Gateway project."</li>
            </ul>

            <h2 class="mt-10 mb-3 text-xl font-semibold">"Sources and attribution"</h2>
        </article>
        <div class="max-w-5xl rounded-xl border border-border bg-card shadow-xs">
            table(
                table_header(table_row(
                    table_head(attrs: attributes! { scope="col" }, "Source")
                    table_head(attrs: attributes! { scope="col" }, "Licence")
                    table_head(attrs: attributes! { scope="col" }, "Used for")
                ))
                table_body(
                    for (name, url, licence, usage) in SOURCES {
                        table_row(
                            table_cell(<div class="min-w-40 whitespace-normal"><a href=(url) rel="noopener noreferrer">(name)</a></div>)
                            table_cell(<div class="min-w-28 whitespace-normal">(licence)</div>)
                            table_cell(<div class="min-w-64 whitespace-normal text-muted-foreground">(usage)</div>)
                        )
                    }
                )
            )
        </div>
        <article class="max-w-3xl">
            <h2 class="mt-10 mb-3 text-xl font-semibold">"How prices stay current"</h2>
            <ul class="list-disc space-y-2 pl-5">
                <li>"Every hour a GitHub workflow compares the catalog with the sources above and opens or updates one pull request with a readable summary."</li>
                <li>"It merges automatically only when every changed rate moved by 25% or less, or when it adds a first known price or missing model details, and the cross-check agrees. Anything else, any disagreement between sources and anything that disappeared from a source waits for a person."</li>
                <li>"Model, provider and price changes are plain TOML files in the repository; corrections are welcome as pull requests."</li>
            </ul>

            <h2 class="mt-10 mb-3 text-xl font-semibold">"Privacy"</h2>
            <p>"No cookies, no analytics, no tracking and no requests to other sites: fonts, styles and images are served from this domain, and the site runs without JavaScript."</p>
        </article>
    })
}
