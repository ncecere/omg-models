//! `/models/{model_id}`: facts, prices per provider with history and
//! provenance, upstream ids and API examples.

use topcoat::{
    Result,
    context::Cx,
    router::{error::RouterErrorExt, module_param, page, path_param},
    view::{View, attributes, view},
};

use crate::{
    components::{
        badge::{BadgeVariant, badge},
        card::{card, card_content, card_header, card_title},
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    format, state,
    ui::{code_block, fact, price_table},
    views::{modalities, offering_view},
};

module_param!(model_id);

#[page]
async fn model_page(cx: &Cx) -> Result<impl View> {
    let state = state::current(cx);
    let id = path_param::<ModelId>(cx);
    let model = state.catalog.models.get(id).ok_or_not_found()?;
    let file = &model.file;

    let name = file.name.clone();
    let vendor = file.vendor.clone();
    let description = file.description.clone();
    let family = file.family.clone();
    let status = file.status;
    let status_label = status.label();
    let open_weights = file.open_weights;
    let license = file.license.clone();
    let docs = file.docs.clone();
    let dash = || "\u{2014}".to_owned();
    let facts: Vec<(&str, String)> = vec![
        (
            "Released",
            file.release_date.map_or_else(dash, |d| d.to_string()),
        ),
        (
            "Knowledge cutoff",
            file.knowledge_cutoff
                .as_ref()
                .map_or_else(dash, ToString::to_string),
        ),
        (
            "Context window",
            file.limits
                .context
                .map_or_else(dash, |v| format!("{} tokens", format::thousands(v))),
        ),
        (
            "Max input",
            file.limits
                .input
                .map_or_else(dash, |v| format!("{} tokens", format::thousands(v))),
        ),
        (
            "Max output",
            file.limits
                .output
                .map_or_else(dash, |v| format!("{} tokens", format::thousands(v))),
        ),
        ("Modalities", modalities(model)),
        (
            "Reasoning",
            file.reasoning
                .map_or_else(dash, |r| if r { "Yes".into() } else { "No".into() }),
        ),
        (
            "Tool calling",
            file.tool_call
                .map_or_else(dash, |r| if r { "Yes".into() } else { "No".into() }),
        ),
    ];
    let lifecycle: Option<String> = match (file.deprecation_date, file.retirement_date) {
        (None, None) => None,
        (deprecated, retired) => Some(format!(
            "{}{}",
            deprecated.map_or(String::new(), |d| format!("Deprecated {d}. ")),
            retired.map_or(String::new(), |d| format!("Retires {d}."))
        )),
    };
    let mut offerings: Vec<_> = file
        .offerings
        .iter()
        .map(|o| offering_view(&state.catalog, model, o))
        .collect();
    offerings.sort_by(|a, b| {
        (&a.provider_name, &a.upstream_id).cmp(&(&b.provider_name, &b.upstream_id))
    });
    let ids: Vec<(String, String, Option<String>, String)> = offerings
        .iter()
        .map(|o| {
            (
                o.provider_name.clone(),
                o.upstream_id.clone(),
                o.label.clone(),
                o.aliases.join(", "),
            )
        })
        .collect();
    let json_href = format!("/api/v1/models/{}.json", file.id);
    let json_curl = format!(
        "curl -s https://models.omg.bitop.dev/api/v1/models/{}.json",
        file.id
    );

    Ok(view! {
        <nav aria-label="Breadcrumb" class="mb-4 text-sm text-muted-foreground">
            <a href="/">"Models"</a>
            <span aria-hidden="true">" / "</span>
            <span aria-current="page">(name.clone())</span>
        </nav>
        <header class="mb-6">
            <h1 class="text-3xl font-semibold tracking-tight">(name.clone())</h1>
            <div class="mt-2 flex flex-wrap items-center gap-2 text-sm">
                badge(variant: BadgeVariant::Secondary, (vendor))
                match family {
                    Some(family) => badge(variant: BadgeVariant::Outline, (family)),
                    None => "",
                }
                if open_weights {
                    badge(variant: BadgeVariant::Outline, "Open weights")
                }
                badge(
                    variant: if status == omg_models_catalog::model::Status::Active { BadgeVariant::Outline } else { BadgeVariant::Destructive },
                    (status_label)
                )
            </div>
            match description {
                Some(description) => <p class="mt-3 max-w-3xl text-muted-foreground">(description)</p>,
                None => "",
            }
            match lifecycle {
                Some(text) => <p class="mt-2 text-sm text-brand-warning-text">(text)</p>,
                None => "",
            }
        </header>

        <dl class="mb-8 grid grid-cols-2 gap-2 sm:grid-cols-4">
            for (term, value) in facts {
                fact(term: term, value: value)
            }
        </dl>
        <p class="-mt-5 mb-8 text-sm text-muted-foreground">
            match license {
                Some(license) => <span>"Licence: " (license) ". "</span>,
                None => "",
            }
            match docs {
                Some(docs) => <a href=(docs) rel="noopener noreferrer">"Model documentation"</a>,
                None => "",
            }
        </p>

        <section aria-labelledby="prices" class="mb-10">
            <h2 id="prices" class="mb-1 text-xl font-semibold">"Prices by provider"</h2>
            <p class="mb-4 text-sm text-muted-foreground">
                "Exact USD list prices. \u{2014} means the price is unknown (not free). History is append-only: a change adds a new entry."
            </p>
            <div class="grid gap-6">
                for offering in offerings {
                    let heading_id = format!("offering-{}-{}", offering.provider_id, offering.upstream_id.replace(['.', '/', ':'], "-"));
                    let provider_href = format!("/providers/{}", offering.provider_id);
                    let caption = format!("Prices for {} on {}", offering.upstream_id, offering.provider_name);
                    let history_len = offering.history.len();
                    card(attrs: attributes! { class="min-w-0" aria-labelledby=(heading_id.clone()) },
                        card_header(
                            card_title(attrs: attributes! { id=(heading_id) },
                                <a href=(provider_href)>(offering.provider_name.clone())</a>
                                match offering.label.clone() {
                                    Some(label) => <span class="font-normal text-muted-foreground">" \u{00b7} " (label)</span>,
                                    None => "",
                                }
                            )
                            <p class="text-sm text-muted-foreground">
                                "Model id: " <code class="break-all text-foreground">(offering.upstream_id.clone())</code>
                            </p>
                        )
                        card_content(
                            match offering.price {
                                Some(price) => price_table(price: price, caption: caption),
                                None => <p class="text-sm text-muted-foreground">"No verified price yet. Unknown is not zero."</p>,
                            }
                            if history_len > 1 {
                                <details class="mt-4">
                                    <summary class="cursor-pointer text-sm font-medium">"Price history (" (history_len) " entries)"</summary>
                                    <div class="mt-2 overflow-x-auto">
                                        table(
                                            table_header(table_row(
                                                table_head(attrs: attributes! { scope="col" }, "Effective from")
                                                table_head(attrs: attributes! { scope="col" class="text-right" }, "Input")
                                                table_head(attrs: attributes! { scope="col" class="text-right" }, "Output")
                                                table_head(attrs: attributes! { scope="col" }, "Source")
                                            ))
                                            table_body(
                                                for row in offering.history {
                                                    table_row(
                                                        table_cell(
                                                            (row.effective_from)
                                                            if row.current { " (current)" }
                                                        )
                                                        table_cell(attrs: attributes! { class="tabular text-right" }, (row.input))
                                                        table_cell(attrs: attributes! { class="tabular text-right" }, (row.output))
                                                        table_cell(<a href=(row.source_url) rel="noopener noreferrer">(row.source_kind)</a>)
                                                    )
                                                }
                                            )
                                        )
                                    </div>
                                </details>
                            } else {
                                <p class="mt-4 text-xs text-muted-foreground">"No earlier prices recorded."</p>
                            }
                            <details class="mt-4">
                                <summary class="cursor-pointer text-sm font-medium">"API example"</summary>
                                <div class="mt-2">code_block(code: offering.example)</div>
                            </details>
                        )
                    )
                }
            </div>
        </section>

        <section aria-labelledby="ids" class="mb-10">
            <h2 id="ids" class="mb-3 text-xl font-semibold">"Model ids"</h2>
            <div class="rounded-xl border border-border bg-card">
                table(
                    table_header(table_row(
                        table_head(attrs: attributes! { scope="col" }, "Provider")
                        table_head(attrs: attributes! { scope="col" }, "Id to send")
                        table_head(attrs: attributes! { scope="col" }, "Also accepted")
                    ))
                    table_body(
                        for (provider, upstream, qualifier, aliases) in ids {
                            table_row(
                                table_cell(
                                    (provider)
                                    match qualifier {
                                        Some(q) => <div class="text-xs text-muted-foreground">(q)</div>,
                                        None => "",
                                    }
                                )
                                table_cell(<code>(upstream)</code>)
                                table_cell(attrs: attributes! { class="text-muted-foreground" }, <code>(aliases)</code>)
                            )
                        }
                    )
                )
            </div>
        </section>

        <section aria-labelledby="json">
            <h2 id="json" class="mb-3 text-xl font-semibold">"This model as JSON"</h2>
            code_block(code: json_curl)
            <p class="mt-2 text-sm"><a href=(json_href)>"Open the JSON"</a></p>
        </section>
    })
}
