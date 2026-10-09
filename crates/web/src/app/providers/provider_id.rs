//! `/providers/{provider_id}`: provider facts and its model listings.

use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{error::RouterErrorExt, module_param, page, path_param},
    view::{View, attributes, view},
};

use crate::{
    components::{
        badge::{BadgeVariant, badge},
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    format, listing,
    state::AppState,
    ui::fact,
};

module_param!(provider_id);

#[page]
async fn provider_page(cx: &Cx) -> Result<impl View> {
    let state = app_context::<AppState>(cx);
    let id = path_param::<ProviderId>(cx);
    let provider = &state.catalog.providers.get(id).ok_or_not_found()?.file;
    let name = provider.name.clone();
    let kind = provider.kind.label();
    let dash = || "\u{2014}".to_owned();
    let facts: Vec<(&str, String)> = vec![
        ("API base", provider.api_base.clone().unwrap_or_else(dash)),
        ("Authentication", provider.auth.clone().unwrap_or_else(dash)),
        (
            "Credential variables",
            if provider.env.is_empty() {
                dash()
            } else {
                provider.env.join(", ")
            },
        ),
    ];
    let links: Vec<(&str, String)> = [
        ("Website", Some(provider.website.clone())),
        ("API docs", provider.docs.clone()),
        ("Official pricing", provider.pricing_page.clone()),
    ]
    .into_iter()
    .filter_map(|(label, url)| url.map(|u| (label, u)))
    .collect();
    let sync = match provider.sync.prices {
        omg_models_catalog::model::PriceSourceKind::Litellm => {
            "Prices are checked hourly against LiteLLM's price file and cross-checked with genai-prices."
        }
        omg_models_catalog::model::PriceSourceKind::Openrouter => {
            "Prices are checked hourly against OpenRouter's public models API and cross-checked with genai-prices."
        }
        omg_models_catalog::model::PriceSourceKind::None => "Prices are maintained by hand.",
    };
    let mut rows: Vec<_> = listing::rows(&state.catalog)
        .into_iter()
        .filter(|r| r.provider_id == *id)
        .collect();
    listing::sort(&mut rows, listing::SortKey::Name, false);
    let json_url = format!("/api/v1/providers/{id}.json");

    Ok(view! {
        <nav aria-label="Breadcrumb" class="mb-4 text-sm text-muted-foreground">
            <a href="/providers">"Providers"</a>
            <span aria-hidden="true">" / "</span>
            <span aria-current="page">(name.clone())</span>
        </nav>
        <header class="mb-6">
            <h1 class="text-3xl font-semibold tracking-tight">(name)</h1>
            <div class="mt-2">badge(variant: BadgeVariant::Secondary, (kind))</div>
            <p class="mt-3 max-w-3xl text-sm text-muted-foreground">(sync)</p>
        </header>
        <dl class="mb-4 grid gap-2 sm:grid-cols-3">
            for (term, value) in facts {
                fact(term: term, value: value)
            }
        </dl>
        <ul class="mb-8 flex flex-wrap gap-x-5 gap-y-1 text-sm">
            for (label, url) in links {
                <li><a href=(url) rel="noopener noreferrer">(label)</a></li>
            }
            <li><a href=(json_url)>"JSON"</a></li>
        </ul>

        <h2 class="mb-3 text-xl font-semibold">"Models"</h2>
        <div class="rounded-xl border border-border bg-card shadow-xs">
            table(
                table_header(table_row(
                    table_head(attrs: attributes! { scope="col" }, "Model")
                    table_head(attrs: attributes! { scope="col" }, "Id")
                    table_head(attrs: attributes! { scope="col" class="text-right" }, "Input")
                    table_head(attrs: attributes! { scope="col" class="text-right" }, "Output")
                    table_head(attrs: attributes! { scope="col" class="text-right" }, "Cache read")
                    table_head(attrs: attributes! { scope="col" class="text-right" }, "Context")
                ))
                table_body(
                    for row in rows {
                        let href = format!("/models/{}", row.model_id);
                        let money = |v: Option<omg_models_catalog::Decimal>| v.map_or_else(|| "\u{2014}".to_owned(), format::usd);
                        table_row(
                            table_cell(
                                <a href=(href) class="font-medium">(row.model_name.clone())</a>
                                match row.label.clone() {
                                    Some(q) => <div class="text-xs text-muted-foreground">(q)</div>,
                                    None => "",
                                }
                            )
                            table_cell(<code class="text-xs">(row.upstream_id.clone())</code>)
                            table_cell(attrs: attributes! { class="tabular text-right" }, (money(row.input)))
                            table_cell(attrs: attributes! { class="tabular text-right" }, (money(row.output)))
                            table_cell(attrs: attributes! { class="tabular text-right" }, (money(row.cache_read)))
                            table_cell(attrs: attributes! { class="tabular text-right" }, (row.context.map_or_else(|| "\u{2014}".to_owned(), format::tokens)))
                        )
                    }
                )
            )
        </div>
    })
}
