//! Shared page pieces built on the Topcoat UI components.

use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use crate::{
    components::table::{table, table_body, table_cell, table_head, table_header, table_row},
    views::PriceView,
};

/// A `<pre><code>` block that scrolls horizontally on small screens.
#[component]
pub async fn code_block(code: String, #[default] label: Option<String>) -> Result<impl View> {
    Ok(view! {
        <figure class="min-w-0">
            match label {
                Some(caption) => <figcaption class="mb-1.5 text-xs font-medium text-muted-foreground">(caption)</figcaption>,
                None => "",
            }
            <pre class="overflow-x-auto rounded-lg border border-border bg-brand-surface-sunken p-3 text-xs leading-relaxed" tabindex="0"><code>(code)</code></pre>
        </figure>
    })
}

/// A price entry as a table (standard and batch columns, tier groups)
/// followed by its provenance.
#[component]
pub async fn price_table(price: PriceView, caption: String) -> Result<impl View> {
    let has_batch = price.has_batch;
    let source = price.source.clone();
    let carried = source.carried_over.join(", ");
    let not_applicable = price.not_applicable.join(", ");
    Ok(view! {
        <div class="overflow-hidden rounded-lg border border-border">
            table(
                table_header(
                    <tr class="border-b border-border bg-brand-surface-sunken">
                        table_head(attrs: attributes! { scope="col" }, "Meter")
                        table_head(attrs: attributes! { scope="col" class="text-right" }, "Standard")
                        if has_batch {
                            table_head(attrs: attributes! { scope="col" class="text-right" }, "Batch")
                        }
                        table_head(attrs: attributes! { scope="col" }, "Unit")
                    </tr>
                )
                table_body(
                    for group in price.groups {
                        match group.heading {
                            Some(heading) => {
                                <tr class="border-b border-border">
                                    <th
                                        scope="colgroup"
                                        colspan=(if has_batch { "4" } else { "3" })
                                        class="bg-brand-surface-sunken px-3 py-2 text-left text-xs font-semibold text-muted-foreground"
                                    >
                                        (heading)
                                    </th>
                                </tr>
                            },
                            None => "",
                        }
                        for row in group.rows {
                            table_row(
                                table_cell(attrs: attributes! { class="font-medium" }, (row.label))
                                table_cell(attrs: attributes! { class="tabular text-right" }, (row.standard))
                                if has_batch {
                                    table_cell(attrs: attributes! { class="tabular text-right" }, (row.batch))
                                }
                                table_cell(attrs: attributes! { class="text-muted-foreground" }, (row.unit))
                            )
                        }
                    }
                )
            )
        </div>
        <p class="sr-only">(caption)</p>
        <dl class="mt-3 grid gap-x-4 gap-y-1 text-xs text-muted-foreground sm:grid-cols-[max-content_1fr]">
            <dt class="font-medium">"Effective from"</dt>
            <dd>(price.effective_from)</dd>
            <dt class="font-medium">"Source"</dt>
            <dd class="break-words">
                <a href=(source.url.clone()) rel="noopener noreferrer">(source.kind)</a>
                ", fetched " (source.fetched)
                match source.version {
                    Some(version) => <span>", version " <code class="break-all">(version)</code></span>,
                    None => "",
                }
            </dd>
            match source.entry_key {
                Some(key) => {
                    <dt class="font-medium">"Source entry"</dt>
                    <dd><code class="break-all">(key)</code></dd>
                },
                None => "",
            }
            match source.cites {
                Some(cites) => {
                    <dt class="font-medium">"Verify at"</dt>
                    <dd class="break-all"><a href=(cites.clone()) rel="noopener noreferrer">(cites)</a></dd>
                },
                None => "",
            }
            if !carried.is_empty() {
                <dt class="font-medium">"Carried over"</dt>
                <dd>(carried) " (not listed by this source; copied from the previous entry)"</dd>
            }
            if !not_applicable.is_empty() {
                <dt class="font-medium">"Not applicable"</dt>
                <dd>(not_applicable)</dd>
            }
            match price.notes {
                Some(notes) => {
                    <dt class="font-medium">"Notes"</dt>
                    <dd>(notes)</dd>
                },
                None => "",
            }
        </dl>
    })
}

/// A labelled fact for definition grids.
#[component]
pub async fn fact(term: &str, value: String) -> Result<impl View> {
    Ok(view! {
        <div class="rounded-lg border border-border bg-card px-3 py-2">
            <dt class="text-xs font-medium text-muted-foreground">(term)</dt>
            <dd class="mt-0.5 text-sm font-medium tabular break-words">(value)</dd>
        </div>
    })
}
