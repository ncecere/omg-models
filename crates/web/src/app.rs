//! Route tree (module routing): this module is `/`.

use std::fmt::Write as _;

mod about;
mod api;
mod compare;
mod models;
mod providers;

use omg_models_catalog::{Decimal, model::Modality};
use topcoat::{
    Result,
    asset::{AssetConfig, RouterBuilderAssetExt, asset},
    context::{Cx, app_context},
    router::{
        Router, RouterBuilderDiscoverExt, Slot, StatusCode, error::NotFoundError, layout,
        module_router, not_found, page, query_params, request,
    },
    tailwind,
    view::{View, attributes, class, error_boundary, view},
};

use crate::{
    components::{
        badge::{BadgeVariant, badge},
        button::{ButtonSize, ButtonVariant, button, button_variants},
        input::input,
        label::label,
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    fonts::INTER,
    format,
    listing::{self, Filters, SortKey},
    security,
    state::AppState,
};

not_found!();

/// Builds the router: pages, the JSON API routes, assets and the
/// security-header layer.
pub fn router(state: AppState, assets: impl Into<AssetConfig>) -> Router {
    module_router!()
        .discover()
        .assets(assets)
        .app_context(state)
        .layer(security::layer())
        .build()
}

/// `(href, label)` for the main navigation.
const NAV: [(&str, &str); 5] = [
    ("/", "Models"),
    ("/providers", "Providers"),
    ("/compare", "Compare"),
    ("/api", "API"),
    ("/about", "About"),
];

fn nav_current(href: &str, path: &str) -> bool {
    if href == "/" {
        path == "/" || path.starts_with("/models")
    } else {
        path == href || path.starts_with(&format!("{href}/"))
    }
}

/// The document title for a path (the layout renders `<head>` before the page).
fn page_title(state: &AppState, path: &str) -> String {
    const SITE: &str = "Open Model Catalog";
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let title = match segments.as_slice() {
        [""] => return format!("{SITE}: AI model prices and limits across providers"),
        ["models", id] => state.catalog.models.get(*id).map(|m| m.file.name.clone()),
        ["providers"] => Some("Providers".into()),
        ["providers", id] => state
            .catalog
            .providers
            .get(*id)
            .map(|p| p.file.name.clone()),
        ["compare"] => Some("Compare models".into()),
        ["api"] => Some("JSON API".into()),
        ["about"] => Some("About and attribution".into()),
        _ => None,
    };
    match title {
        Some(title) => format!("{title} · {SITE}"),
        None => format!("Not found · {SITE}"),
    }
}

#[layout]
async fn shell(cx: &Cx, slot: Slot<'_>) -> Result<impl View> {
    let state = app_context::<AppState>(cx);
    let path = request::uri(cx).path().to_owned();
    let title = page_title(state, &path);
    let updated = state
        .last_updated
        .as_deref()
        .map(|t| t.get(..10).unwrap_or(t).to_owned());
    let nav: Vec<(&str, &str, bool)> = NAV
        .iter()
        .map(|(href, text)| (*href, *text, nav_current(href, &path)))
        .collect();

    Ok(view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>(title)</title>
                <meta
                    name="description"
                    content="An open catalog of AI models and their prices across providers, with exact decimal prices, price history, provenance and a free JSON API."
                >
                <meta name="color-scheme" content="light dark">
                <link rel="icon" href=(asset!("assets/brand/favicon.svg")) type="image/svg+xml">
                <link rel="apple-touch-icon" href=(asset!("assets/brand/apple-touch-icon.png"))>
                topcoat::font::link(font: INTER)
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
            </head>
            <body class="flex min-h-screen flex-col">
                <a
                    href="#main"
                    class="sr-only focus:not-sr-only focus:absolute focus:top-3 focus:left-3 focus:z-50 focus:rounded-md focus:bg-card focus:px-3 focus:py-2"
                >
                    "Skip to content"
                </a>
                <header class="border-b border-border bg-card">
                    <div class="mx-auto flex w-full max-w-6xl flex-wrap items-center justify-between gap-x-6 gap-y-2 px-4 py-3">
                        <a
                            href="/"
                            class="inline-flex items-center gap-2.5 rounded-md font-semibold text-foreground no-underline hover:text-foreground"
                        >
                            <img
                                src=(asset!("assets/brand/mark-small.svg"))
                                alt=""
                                width="28"
                                height="28"
                                class="size-7 shrink-0"
                            >
                            <span class="text-[1.05rem] tracking-tight whitespace-nowrap">"Open Model Catalog"</span>
                        </a>
                        <nav aria-label="Main">
                            <ul class="-mx-2 flex flex-wrap items-center gap-0.5 text-sm">
                                for (href, text, current) in nav {
                                    <li>
                                        <a
                                            href=(href)
                                            aria-current=(current.then_some("page"))
                                            class=(class!(
                                                "inline-flex min-h-9 items-center rounded-md px-2.5 font-medium no-underline",
                                                "bg-brand-primary-subtle text-brand-primary-subtle-text" if current,
                                                "text-muted-foreground hover:bg-brand-surface-hover hover:text-foreground" if !current,
                                            ))
                                        >
                                            (text)
                                        </a>
                                    </li>
                                }
                            </ul>
                        </nav>
                    </div>
                </header>
                <main id="main" class="mx-auto w-full max-w-6xl flex-1 px-4 py-8">
                    error_boundary(
                        fallback: |error| {
                            if error.downcast_ref::<NotFoundError>().is_none() {
                                return Err(error);
                            }
                            Ok(view! {
                                (StatusCode::NOT_FOUND)
                                <div class="py-16 text-center">
                                    <p class="text-sm font-medium text-muted-foreground">"404"</p>
                                    <h1 class="mt-2 text-3xl font-semibold tracking-tight">"Page not found"</h1>
                                    <p class="mt-3 text-muted-foreground">
                                        "There is no model, provider or page at this address."
                                    </p>
                                    <p class="mt-6">
                                        <a href="/" class=(button_variants(ButtonVariant::Outline, ButtonSize::Md))>
                                            "Browse all models"
                                        </a>
                                    </p>
                                </div>
                            })
                        },
                        (slot)
                    )
                </main>
                <footer class="border-t border-border bg-card">
                    <div class="mx-auto grid w-full max-w-6xl gap-3 px-4 py-6 text-sm text-muted-foreground sm:flex sm:items-start sm:justify-between">
                        <div class="max-w-xl space-y-1">
                            <p>
                                "Open Model Catalog is part of the "
                                <a href="https://omg.bitop.dev">"Open Model Gateway"</a>
                                " family. Prices are published list prices, not invoices; check the provider before you rely on one."
                            </p>
                            match updated {
                                Some(updated) => <p>"Prices last checked " <time datetime=(updated.clone())>(updated)</time> "."</p>,
                                None => "",
                            }
                        </div>
                        <ul class="flex flex-wrap gap-x-4 gap-y-1">
                            <li><a href="/about">"About and data licence"</a></li>
                            <li><a href="https://docs.omg.bitop.dev">"Gateway docs"</a></li>
                            <li><a href="https://github.com/ncecere/omg-models">"Source"</a></li>
                        </ul>
                    </div>
                </footer>
            </body>
        </html>
    })
}

#[query_params(error = bad_request("invalid filter value"))]
struct HomeQuery {
    q: Option<String>,
    provider: Option<String>,
    modality: Option<String>,
    max_input: Option<String>,
    min_context: Option<String>,
    open_weights: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
}

const CONTEXT_CHOICES: [(u64, &str); 5] = [
    (32_000, "32K or more"),
    (128_000, "128K or more"),
    (200_000, "200K or more"),
    (400_000, "400K or more"),
    (1_000_000, "1M or more"),
];

fn parse_filters(state: &AppState, query: &HomeQuery) -> Filters {
    let mut filters = Filters {
        q: query.q.clone().unwrap_or_default(),
        open_weights: query.open_weights.is_some(),
        descending: query.dir.as_deref() == Some("desc"),
        ..Filters::default()
    };
    if let Some(provider) = &query.provider {
        if state.catalog.providers.contains_key(provider) {
            filters.provider = Some(provider.clone());
        } else {
            filters.problems.push(format!(
                "Unknown provider \u{201c}{provider}\u{201d}; showing all providers."
            ));
        }
    }
    if let Some(modality) = &query.modality {
        match Modality::from_key(modality) {
            Some(m) => filters.modality = Some(m),
            None => filters
                .problems
                .push("Unknown modality; showing all modalities.".into()),
        }
    }
    if let Some(max) = &query.max_input {
        match Decimal::parse_strict(max.trim().trim_start_matches('$')) {
            Ok(value) => filters.max_input = Some(value),
            Err(_) => filters.problems.push(
                "Maximum input price must be a plain number such as 1 or 0.25 (USD per 1M tokens)."
                    .into(),
            ),
        }
    }
    if let Some(min) = &query.min_context {
        match min.parse::<u64>() {
            Ok(value) => filters.min_context = Some(value),
            Err(_) => filters
                .problems
                .push("Unknown context size; showing all.".into()),
        }
    }
    if let Some(sort) = &query.sort {
        filters.sort = SortKey::parse(sort);
    }
    filters
}

/// The query string for the current filters with a different sort.
fn sort_href(query: &HomeQuery, key: SortKey, descending: bool) -> String {
    let mut pairs: Vec<(&str, String)> = Vec::new();
    for (name, value) in [
        ("q", &query.q),
        ("provider", &query.provider),
        ("modality", &query.modality),
        ("max_input", &query.max_input),
        ("min_context", &query.min_context),
        ("open_weights", &query.open_weights),
    ] {
        if let Some(value) = value.as_ref().filter(|v| !v.is_empty()) {
            pairs.push((name, value.clone()));
        }
    }
    pairs.push(("sort", key.key().to_owned()));
    if descending {
        pairs.push(("dir", "desc".into()));
    }
    let encoded: Vec<String> = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", url_encode(&v)))
        .collect();
    format!("/?{}#models", encoded.join("&"))
}

/// Minimal application/x-www-form-urlencoded encoding.
pub(crate) fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

struct Column {
    key: SortKey,
    heading: &'static str,
    numeric: bool,
}

const COLUMNS: [Column; 9] = [
    Column {
        key: SortKey::Name,
        heading: "Model",
        numeric: false,
    },
    Column {
        key: SortKey::Provider,
        heading: "Provider",
        numeric: false,
    },
    Column {
        key: SortKey::Input,
        heading: "Input",
        numeric: true,
    },
    Column {
        key: SortKey::Output,
        heading: "Output",
        numeric: true,
    },
    Column {
        key: SortKey::CacheRead,
        heading: "Cache read",
        numeric: true,
    },
    Column {
        key: SortKey::CacheWrite,
        heading: "Cache write",
        numeric: true,
    },
    Column {
        key: SortKey::Context,
        heading: "Context",
        numeric: true,
    },
    Column {
        key: SortKey::MaxOutput,
        heading: "Max output",
        numeric: true,
    },
    Column {
        key: SortKey::Release,
        heading: "Released",
        numeric: true,
    },
];

fn price_cell(value: Option<Decimal>) -> (String, bool) {
    match value {
        Some(value) => (format::usd(value), true),
        None => ("\u{2014}".into(), false),
    }
}

#[page]
async fn home(cx: &Cx) -> Result<impl View> {
    let state = app_context::<AppState>(cx);
    let query = query_params::<HomeQuery>(cx)?;
    let filters = parse_filters(state, query);
    let mut rows: Vec<_> = listing::rows(&state.catalog)
        .into_iter()
        .filter(|row| filters.matches(row))
        .collect();
    let sort = filters.sort.unwrap_or(SortKey::Name);
    listing::sort(&mut rows, sort, filters.descending);
    let total = listing::rows(&state.catalog).len();
    let shown = rows.len();
    let models = state.catalog.models.len();
    let providers: Vec<(String, String)> = state
        .catalog
        .providers
        .values()
        .map(|p| (p.file.id.clone(), p.file.name.clone()))
        .collect();
    let provider_count = providers.len();

    let headers: Vec<(
        &'static str,
        bool,
        String,
        Option<&'static str>,
        &'static str,
    )> = COLUMNS
        .iter()
        .map(|column| {
            let active = column.key == sort;
            let next_desc = active && !filters.descending;
            let aria = active.then_some(if filters.descending {
                "descending"
            } else {
                "ascending"
            });
            let arrow = match (active, filters.descending) {
                (true, false) => "\u{2191}",
                (true, true) => "\u{2193}",
                _ => "",
            };
            (
                column.heading,
                column.numeric,
                sort_href(query, column.key, next_desc),
                aria,
                arrow,
            )
        })
        .collect();

    let q = filters.q.clone();
    let selected_provider = filters.provider.clone();
    let selected_modality = filters.modality;
    let max_input = query.max_input.clone().unwrap_or_default();
    let min_context = filters.min_context;
    let open_weights = filters.open_weights;
    let problems = filters.problems.clone();
    let active = filters.is_active();
    let sort_key = query.sort.clone();
    let sort_dir = query.dir.clone();

    Ok(view! {
        <section class="mb-8 max-w-3xl">
            <h1 class="text-3xl font-semibold tracking-tight sm:text-4xl">"AI model prices, side by side"</h1>
            <p class="mt-3 text-base text-muted-foreground sm:text-lg">
                "An open catalog of " (models) " models across " (provider_count)
                " providers: exact list prices per 1M tokens, cache and batch rates, limits and price history, each with a link to its source. Everything here is also a free JSON API."
            </p>
        </section>

        <form method="get" action="/" class="mb-6 rounded-xl border border-border bg-card p-4 shadow-xs" aria-label="Filter models">
            <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-6">
                <div class="flex flex-col gap-1.5 lg:col-span-2">
                    label(attrs: attributes! { for="q" }, "Search")
                    input(attrs: attributes! { id="q" name="q" type="search" value=(q.clone()) placeholder="Model, vendor or provider" autocomplete="off" })
                </div>
                <div class="flex flex-col gap-1.5">
                    label(attrs: attributes! { for="provider" }, "Provider")
                    <select id="provider" name="provider" class="h-9 w-full rounded-lg border border-input bg-card px-2 text-sm">
                        <option value="">"All providers"</option>
                        for (id, name) in providers.clone() {
                            <option value=(id.clone()) selected=(selected_provider.as_deref() == Some(id.as_str()))>(name)</option>
                        }
                    </select>
                </div>
                <div class="flex flex-col gap-1.5">
                    label(attrs: attributes! { for="modality" }, "Modality")
                    <select id="modality" name="modality" class="h-9 w-full rounded-lg border border-input bg-card px-2 text-sm">
                        <option value="">"Any"</option>
                        for modality in Modality::ALL {
                            <option value=(modality.key()) selected=(selected_modality == Some(modality))>(modality.key())</option>
                        }
                    </select>
                </div>
                <div class="flex flex-col gap-1.5">
                    label(attrs: attributes! { for="max_input" }, "Max input $/1M")
                    input(attrs: attributes! { id="max_input" name="max_input" inputmode="decimal" value=(max_input.clone()) placeholder="e.g. 1" aria-describedby="max-input-help" })
                    <span id="max-input-help" class="sr-only">"USD per million input tokens; models with an unknown price are hidden."</span>
                </div>
                <div class="flex flex-col gap-1.5">
                    label(attrs: attributes! { for="min_context" }, "Context")
                    <select id="min_context" name="min_context" class="h-9 w-full rounded-lg border border-input bg-card px-2 text-sm">
                        <option value="">"Any size"</option>
                        for (value, text) in CONTEXT_CHOICES {
                            <option value=(value.to_string()) selected=(min_context == Some(value))>(text)</option>
                        }
                    </select>
                </div>
            </div>
            <div class="mt-4 flex flex-wrap items-center gap-x-6 gap-y-3">
                <label class="inline-flex items-center gap-2 text-sm">
                    <input type="checkbox" name="open_weights" value="1" checked=(open_weights) class="size-4 accent-[var(--omg-primary)]">
                    "Open weights only"
                </label>
                match sort_key {
                    Some(sort) => <input type="hidden" name="sort" value=(sort)>,
                    None => "",
                }
                match sort_dir {
                    Some(dir) => <input type="hidden" name="dir" value=(dir)>,
                    None => "",
                }
                <div class="flex gap-2">
                    button(attrs: attributes! { type="submit" }, "Apply filters")
                    if active {
                        <a href="/" class=(button_variants(ButtonVariant::Ghost, ButtonSize::Md))>"Clear"</a>
                    }
                </div>
            </div>
            if !problems.is_empty() {
                <ul class="mt-3 space-y-1 text-sm text-brand-warning-text" role="status">
                    for problem in problems {
                        <li>(problem)</li>
                    }
                </ul>
            }
        </form>

        <div class="mb-2 flex flex-wrap items-baseline justify-between gap-2">
            <h2 id="models" class="text-lg font-semibold">"Models"</h2>
            <p class="text-sm text-muted-foreground" role="status">
                "Showing " (shown) " of " (total) " provider listings. Prices in USD per 1M tokens; \u{2014} means unknown."
            </p>
        </div>
        <div class="rounded-xl border border-border bg-card shadow-xs">
            table(
                attrs: attributes! { aria-describedby="models" },
                table_header(
                    table_row(
                        for (heading, numeric, href, aria, arrow) in headers {
                            table_head(
                                attrs: attributes! { scope="col" aria-sort=(aria) class=(if numeric { "text-right" } else { "" }) },
                                <a href=(href) class="inline-flex items-center gap-1 text-muted-foreground no-underline hover:text-foreground">
                                    (heading)
                                    <span aria-hidden="true">(arrow)</span>
                                </a>
                            )
                        }
                    )
                )
                table_body(
                    if rows.is_empty() {
                        table_row(
                            table_cell(attrs: attributes! { colspan="9" class="py-8 text-center text-muted-foreground" }, "No models match these filters.")
                        )
                    }
                    for row in rows {
                        let (input_text, _) = price_cell(row.input);
                        let (output_text, _) = price_cell(row.output);
                        let (read_text, _) = price_cell(row.cache_read);
                        let (write_text, _) = price_cell(row.cache_write);
                        let model_href = format!("/models/{}", row.model_id);
                        let provider_href = format!("/providers/{}", row.provider_id);
                        let context = row.context.map_or_else(|| "\u{2014}".to_owned(), format::tokens);
                        let context_title = row.context.map(format::thousands);
                        let max_output = row.max_output.map_or_else(|| "\u{2014}".to_owned(), format::tokens);
                        let max_output_title = row.max_output.map(format::thousands);
                        let modalities = format!(
                            "{} \u{2192} {}",
                            row.input_modalities.iter().map(|m| m.key()).collect::<Vec<_>>().join(", "),
                            row.output_modalities.iter().map(|m| m.key()).collect::<Vec<_>>().join(", ")
                        );
                        let release = row.release.map_or_else(|| "\u{2014}".to_owned(), |d| d.to_string());
                        let tier_note = row.tiered.map(|t| format!("Higher rates above {} prompt tokens", format::tokens(t)));
                        table_row(
                            table_cell(
                                <a href=(model_href) class="font-medium">(row.model_name.clone())</a>
                                if row.open_weights {
                                    " "
                                    badge(variant: BadgeVariant::Outline, attrs: attributes! { class="ml-1 align-middle" }, "open weights")
                                }
                                <div class="text-xs text-muted-foreground">(row.vendor.clone()) " \u{00b7} " (modalities)</div>
                            )
                            table_cell(
                                <a href=(provider_href)>(row.provider_name.clone())</a>
                                match row.label.clone() {
                                    Some(qualifier) => <div class="text-xs text-muted-foreground">(qualifier)</div>,
                                    None => "",
                                }
                            )
                            table_cell(attrs: attributes! { class="tabular text-right" },
                                (input_text)
                                match tier_note {
                                    Some(note) => {
                                        <abbr title=(note.clone()) class="ml-0.5 cursor-help text-muted-foreground no-underline" aria-hidden="true">"*"</abbr>
                                        <span class="sr-only">(note)</span>
                                    },
                                    None => "",
                                }
                            )
                            table_cell(attrs: attributes! { class="tabular text-right" }, (output_text))
                            table_cell(attrs: attributes! { class="tabular text-right" }, (read_text))
                            table_cell(attrs: attributes! { class="tabular text-right" }, (write_text))
                            table_cell(attrs: attributes! { class="tabular text-right" title=(context_title) }, (context))
                            table_cell(attrs: attributes! { class="tabular text-right" title=(max_output_title) }, (max_output))
                            table_cell(attrs: attributes! { class="tabular text-right" }, (release))
                        )
                    }
                )
            )
        </div>
        <p class="mt-3 text-sm text-muted-foreground">
            "* Long prompts are billed at higher rates for the whole request; see the model page. Cache write shows the 5-minute rate where a provider prices TTLs separately."
        </p>
    })
}
