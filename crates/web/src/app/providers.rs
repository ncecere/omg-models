//! `/providers`: every provider with its type and model count.

mod provider_id;

use topcoat::{
    Result,
    context::Cx,
    router::page,
    view::{View, attributes, view},
};

use crate::{
    components::{
        badge::{BadgeVariant, badge},
        card::{card, card_content, card_description, card_header, card_title},
    },
    state,
};

#[page]
async fn providers_page(cx: &Cx) -> Result<impl View> {
    let state = state::current(cx);
    let providers: Vec<(String, String, &'static str, String, usize)> = state
        .catalog
        .providers
        .values()
        .map(|p| {
            let count = state
                .catalog
                .models
                .values()
                .flat_map(|m| &m.file.offerings)
                .filter(|o| o.provider == p.file.id)
                .count();
            (
                p.file.id.clone(),
                p.file.name.clone(),
                p.file.kind.label(),
                p.file.website.clone(),
                count,
            )
        })
        .collect();
    Ok(view! {
        <h1 class="mb-2 text-3xl font-semibold tracking-tight">"Providers"</h1>
        <p class="mb-6 max-w-3xl text-muted-foreground">
            "Where the catalog's models can be called: first-party APIs, cloud platforms and aggregators. Prices differ by provider and, on cloud platforms, by endpoint type."
        </p>
        <ul class="grid gap-4 sm:grid-cols-2">
            for (id, name, kind, website, count) in providers {
                let href = format!("/providers/{id}");
                <li>
                    card(attrs: attributes! { class="h-full" },
                        card_header(
                            card_title(<a href=(href)>(name)</a>)
                            card_description(
                                badge(variant: BadgeVariant::Secondary, (kind))
                                " " (count) if count == 1 { " model listing" } else { " model listings" }
                            )
                        )
                        card_content(
                            <p class="text-sm text-muted-foreground break-all">(website)</p>
                        )
                    )
                </li>
            }
        </ul>
    })
}
