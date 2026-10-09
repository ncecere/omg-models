//! `/compare?m1=..&m2=..&m3=..&m4=..`: two to four models side by side.

use omg_models_catalog::{Decimal, Meter, Model};
use topcoat::{
    Result,
    context::Cx,
    router::{page, query_params},
    view::{View, attributes, view},
};

use crate::{
    components::{
        button::button,
        label::label,
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    format,
    state::{self, AppState},
    views::modalities,
};

#[query_params(error = bad_request("invalid compare selection"))]
struct CompareQuery {
    m1: Option<String>,
    m2: Option<String>,
    m3: Option<String>,
    m4: Option<String>,
}

/// The cheapest current standard rate for `meter` across a model's
/// offerings, with the provider that charges it.
fn lowest(state: &AppState, model: &Model, meter: Meter) -> String {
    let mut best: Option<(Decimal, String)> = None;
    for offering in &model.file.offerings {
        if let Some(rate) = offering.prices.last().and_then(|p| p.standard.get(&meter)) {
            let provider = state
                .catalog
                .providers
                .get(&offering.provider)
                .map_or_else(|| offering.provider.clone(), |p| p.file.name.clone());
            let provider = match &offering.label {
                Some(qualifier) => format!("{provider}, {qualifier}"),
                None => provider,
            };
            if best.as_ref().is_none_or(|(b, _)| rate < b) {
                best = Some((*rate, provider));
            }
        }
    }
    best.map_or_else(
        || "\u{2014}".to_owned(),
        |(rate, provider)| format!("{} ({provider})", format::usd(rate)),
    )
}

#[page]
async fn compare_page(cx: &Cx) -> Result<impl View> {
    let state = state::current(cx);
    let query = query_params::<CompareQuery>(cx)?;
    let requested: Vec<String> = [&query.m1, &query.m2, &query.m3, &query.m4]
        .into_iter()
        .flatten()
        .filter(|id| !id.is_empty())
        .cloned()
        .collect();
    let mut unknown = Vec::new();
    let mut selected: Vec<&Model> = Vec::new();
    for id in &requested {
        match state.catalog.models.get(id) {
            Some(model) if !selected.iter().any(|m| m.file.id == *id) => selected.push(model),
            Some(_) => {}
            None => unknown.push(id.clone()),
        }
    }
    let options: Vec<(String, String)> = state
        .catalog
        .models
        .values()
        .map(|m| {
            (
                m.file.id.clone(),
                format!("{} ({})", m.file.name, m.file.vendor),
            )
        })
        .collect();
    let slots: Vec<(String, String, Option<String>)> = (0..4)
        .map(|i| {
            (
                format!("m{}", i + 1),
                format!("Model {}", i + 1),
                selected.get(i).map(|m| m.file.id.clone()),
            )
        })
        .collect();

    let dash = || "\u{2014}".to_owned();
    let names: Vec<(String, String)> = selected
        .iter()
        .map(|m| (m.file.name.clone(), format!("/models/{}", m.file.id)))
        .collect();
    let rows: Vec<(&str, Vec<String>)> = vec![
        (
            "Vendor",
            selected.iter().map(|m| m.file.vendor.clone()).collect(),
        ),
        (
            "Released",
            selected
                .iter()
                .map(|m| m.file.release_date.map_or_else(dash, |d| d.to_string()))
                .collect(),
        ),
        (
            "Knowledge cutoff",
            selected
                .iter()
                .map(|m| {
                    m.file
                        .knowledge_cutoff
                        .as_ref()
                        .map_or_else(dash, ToString::to_string)
                })
                .collect(),
        ),
        (
            "Context window",
            selected
                .iter()
                .map(|m| m.file.limits.context.map_or_else(dash, format::thousands))
                .collect(),
        ),
        (
            "Max output",
            selected
                .iter()
                .map(|m| m.file.limits.output.map_or_else(dash, format::thousands))
                .collect(),
        ),
        (
            "Modalities",
            selected.iter().map(|m| modalities(m)).collect(),
        ),
        (
            "Open weights",
            selected
                .iter()
                .map(|m| {
                    if m.file.open_weights {
                        "Yes".into()
                    } else {
                        "No".into()
                    }
                })
                .collect(),
        ),
        (
            "Lowest input / 1M",
            selected
                .iter()
                .map(|m| lowest(state, m, Meter::InputTokens))
                .collect(),
        ),
        (
            "Lowest output / 1M",
            selected
                .iter()
                .map(|m| lowest(state, m, Meter::OutputTokens))
                .collect(),
        ),
        (
            "Lowest cache read / 1M",
            selected
                .iter()
                .map(|m| lowest(state, m, Meter::CacheReadTokens))
                .collect(),
        ),
        (
            "Providers",
            selected
                .iter()
                .map(|m| m.file.offerings.len().to_string())
                .collect(),
        ),
    ];
    let enough = selected.len() >= 2;
    let one = selected.len() == 1;

    Ok(view! {
        <h1 class="mb-2 text-3xl font-semibold tracking-tight">"Compare models"</h1>
        <p class="mb-6 max-w-3xl text-muted-foreground">
            "Pick two to four models. Prices are the lowest current standard rate across each model's providers; open a model for every provider's prices."
        </p>
        <form method="get" action="/compare" class="mb-6 rounded-xl border border-border bg-card p-4 shadow-xs" aria-label="Choose models to compare">
            <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                for (name, text, current) in slots {
                    <div class="flex flex-col gap-1.5">
                        label(attrs: attributes! { for=(name.clone()) }, (text))
                        <select id=(name.clone()) name=(name) class="h-9 w-full rounded-lg border border-input bg-card px-2 text-sm">
                            <option value="">"None"</option>
                            for (id, title) in options.clone() {
                                <option value=(id.clone()) selected=(current.as_deref() == Some(id.as_str()))>(title)</option>
                            }
                        </select>
                    </div>
                }
            </div>
            <div class="mt-4">button(attrs: attributes! { type="submit" }, "Compare")</div>
            if !unknown.is_empty() {
                <p class="mt-3 text-sm text-brand-warning-text" role="status">"Unknown model id: " (unknown.join(", "))</p>
            }
        </form>
        if enough {
            <div class="rounded-xl border border-border bg-card shadow-xs">
                table(
                    table_header(table_row(
                        table_head(attrs: attributes! { scope="col" }, <span class="sr-only">"Attribute"</span>)
                        for (name, href) in names {
                            table_head(attrs: attributes! { scope="col" }, <a href=(href) class="font-semibold">(name)</a>)
                        }
                    ))
                    table_body(
                        for (attribute, values) in rows {
                            table_row(
                                <th scope="row" class="p-3 text-left align-middle font-medium whitespace-nowrap text-muted-foreground">(attribute)</th>
                                for value in values {
                                    table_cell(attrs: attributes! { class="tabular" }, (value))
                                }
                            )
                        }
                    )
                )
            </div>
        } else {
            <p class="text-sm text-muted-foreground" role="status">
                if one { "Choose at least one more model." } else { "Choose at least two models." }
            </p>
        }
    })
}
