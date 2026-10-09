//! Human-readable change summary (the PR body) and a machine-readable
//! summary for the workflow.

use std::fmt::Write as _;

use serde_json::json;

use super::{
    SourceStatus,
    plan::{Label, Plan},
};

fn pct(bp: Option<i128>) -> String {
    match bp {
        Some(bp) => {
            let sign = if bp >= 0 { "+" } else { "-" };
            let abs = bp.unsigned_abs();
            format!("{sign}{}.{:02}%", abs / 100, abs % 100)
        }
        None => "new".into(),
    }
}

pub fn markdown(plan: &Plan, sources: &[SourceStatus]) -> String {
    let (label, reasons) = plan.label();
    let mut out = String::new();
    let _ = writeln!(out, "## Price sync {}\n", plan.today);
    let _ = writeln!(
        out,
        "**Label:** `{}` · {} price change(s), {} metadata fill(s), {} disagreement(s) on changed offerings, {} removal(s)\n",
        label.as_str(),
        plan.price_changes.len(),
        plan.metadata_fills.len(),
        plan.disagreements.iter().filter(|d| d.blocking).count(),
        plan.removals.len(),
    );
    if label == Label::NeedsReview {
        out.push_str("### Why this needs review\n\n");
        for reason in &reasons {
            let _ = writeln!(out, "- {reason}");
        }
        out.push('\n');
    }
    if !plan.price_changes.is_empty() {
        out.push_str("### Price changes\n\nRates are USD per the meter's unit (per 1M tokens for token meters). Each change is a new entry dated today; earlier entries are untouched.\n\n");
        for change in &plan.price_changes {
            let kind = if change.first_price {
                "first known price"
            } else {
                "update"
            };
            let _ = writeln!(
                out,
                "#### {} ({kind}, from {})\n",
                change.target.label(),
                change.source.label()
            );
            out.push_str(
                "| Card | Tier | Meter | Old | New | Change |\n|---|---|---|---:|---:|---:|\n",
            );
            for diff in &change.diffs {
                let tier = if diff.tier == 0 {
                    "base".to_owned()
                } else {
                    format!("> {}", diff.tier)
                };
                let _ = writeln!(
                    out,
                    "| {} | {tier} | `{}` | {} | {} | {} |",
                    diff.card,
                    diff.meter,
                    diff.old.map_or_else(|| "—".to_owned(), |d| d.to_string()),
                    diff.new,
                    pct(diff.change_bp),
                );
            }
            if !change.entry.source.carried_over.is_empty() {
                let carried: Vec<&str> = change
                    .entry
                    .source
                    .carried_over
                    .iter()
                    .map(|m| m.key())
                    .collect();
                let _ = writeln!(
                    out,
                    "\nCarried over (not listed by the source): {}",
                    carried.join(", ")
                );
            }
            let _ = writeln!(
                out,
                "\nSource: <{}> · entry `{}` · version `{}`\n",
                change.entry.source.url,
                change.entry.source.entry_key.as_deref().unwrap_or("?"),
                change.entry.source.version.as_deref().unwrap_or("?"),
            );
        }
    }
    if !plan.metadata_fills.is_empty() {
        out.push_str("### Metadata filled from models.dev\n\n| Model | Field | Value | models.dev key |\n|---|---|---|---|\n");
        for fill in &plan.metadata_fills {
            let _ = writeln!(
                out,
                "| {} | `{}` | {} | `{}` |",
                fill.model, fill.field, fill.value, fill.source_key
            );
        }
        out.push('\n');
    }
    if !plan.removals.is_empty() {
        out.push_str("### Missing from the primary source (no file change)\n\n");
        for removal in &plan.removals {
            let _ = writeln!(
                out,
                "- {}: `{}` not found in {} ({})",
                removal.target.label(),
                removal.key,
                removal.source.label(),
                removal.reason
            );
        }
        out.push('\n');
    }
    if !plan.disagreements.is_empty() {
        out.push_str("### Cross-source disagreements\n\nThe catalog keeps the primary source's value; review against the provider's pricing page.\n\n| Offering | Meter | Tier | Catalog | genai-prices | Blocks auto-merge |\n|---|---|---|---:|---:|---|\n");
        for d in &plan.disagreements {
            let tier = if d.tier == 0 {
                "base".to_owned()
            } else {
                format!("> {}", d.tier)
            };
            let _ = writeln!(
                out,
                "| {} | `{}` | {tier} | {} ({}) | {} (`{}`) | {} |",
                d.target.label(),
                d.meter,
                d.catalog,
                d.catalog_source.label(),
                d.other,
                d.other_key,
                if d.blocking {
                    "yes"
                } else {
                    "no (unchanged offering)"
                },
            );
        }
        out.push('\n');
    }
    out.push_str("### Sources\n\n| Source | Version | SHA-256 | Status |\n|---|---|---|---|\n");
    for source in sources {
        let _ = writeln!(
            out,
            "| [{}]({}) | `{}` | `{}` | {} |",
            source.name,
            source.url,
            source.version.as_deref().unwrap_or("-"),
            source
                .sha256
                .as_deref()
                .map_or("-", |s| &s[..s.len().min(16)]),
            source
                .error
                .as_deref()
                .map_or_else(|| "ok".to_owned(), |e| format!("unavailable: {e}")),
        );
    }
    if !plan.notes.is_empty() {
        out.push_str("\n<details><summary>Notes (");
        let _ = write!(out, "{}", plan.notes.len());
        out.push_str(")</summary>\n\n");
        for note in &plan.notes {
            let _ = writeln!(out, "- {}: {}", note.subject, note.text);
        }
        out.push_str("\n</details>\n");
    }
    out
}

pub fn summary_json(plan: &Plan, sources: &[SourceStatus]) -> serde_json::Value {
    let (label, reasons) = plan.label();
    json!({
        "today": plan.today,
        "changed": plan.has_changes(),
        "label": label.as_str(),
        "small": label == Label::AutoMerge,
        "review_reasons": reasons,
        "price_changes": plan.price_changes,
        "metadata_fills": plan.metadata_fills,
        "disagreements": plan.disagreements,
        "removals": plan.removals,
        "notes": plan.notes,
        "sources": sources,
    })
}
