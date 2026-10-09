//! Writes a plan to the data files. Edits are textual appends so existing
//! entries, comments and formatting stay byte-for-byte unchanged; every
//! edit is re-parsed and checked before the file is written.

use std::{fmt::Write as _, fs, path::Path};

use anyhow::{Context, bail, ensure};
use omg_models_catalog::model::{ModelFile, PriceEntry, RateCard, Tier};

use super::plan::{MetadataFill, Plan, PriceChange};

/// A TOML basic string.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn card(rates: &RateCard) -> String {
    let fields: Vec<String> = rates
        .iter()
        .map(|(meter, rate)| format!("{} = {}", meter.key(), quote(&rate.to_string())))
        .collect();
    format!("{{ {} }}", fields.join(", "))
}

fn tiers(name: &str, tiers: &[Tier]) -> String {
    let mut out = format!("{name} = [\n");
    for tier in tiers {
        let _ = writeln!(
            out,
            "  {{ above_prompt_tokens = {}, rates = {} }},",
            tier.above_prompt_tokens,
            card(&tier.rates)
        );
    }
    out.push_str("]\n");
    out
}

/// Renders a price entry as a `[[offerings.prices]]` block.
pub fn render_entry(entry: &PriceEntry) -> String {
    let mut out = String::from("[[offerings.prices]]\n");
    let _ = writeln!(
        out,
        "effective_from = {}",
        quote(&entry.effective_from.to_string())
    );
    out.push_str("currency = \"USD\"\n");
    let _ = writeln!(out, "standard = {}", card(&entry.standard));
    if !entry.standard_tiers.is_empty() {
        out.push_str(&tiers("standard_tiers", &entry.standard_tiers));
    }
    if let Some(batch) = &entry.batch {
        let _ = writeln!(out, "batch = {}", card(batch));
        if !entry.batch_tiers.is_empty() {
            out.push_str(&tiers("batch_tiers", &entry.batch_tiers));
        }
    }
    if !entry.not_applicable.is_empty() {
        let list: Vec<String> = entry
            .not_applicable
            .iter()
            .map(|m| quote(m.key()))
            .collect();
        let _ = writeln!(out, "not_applicable = [{}]", list.join(", "));
    }
    let source = &entry.source;
    let kind = serde_json::to_value(source.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut fields = vec![
        format!("kind = {}", quote(&kind)),
        format!("url = {}", quote(&source.url)),
        format!("fetched_at = {}", quote(source.fetched_at.as_str())),
    ];
    for (key, value) in [
        ("version", &source.version),
        ("sha256", &source.sha256),
        ("entry_key", &source.entry_key),
        ("cites", &source.cites),
    ] {
        if let Some(value) = value {
            fields.push(format!("{key} = {}", quote(value)));
        }
    }
    if !source.carried_over.is_empty() {
        let list: Vec<String> = source.carried_over.iter().map(|m| quote(m.key())).collect();
        fields.push(format!("carried_over = [{}]", list.join(", ")));
    }
    let _ = writeln!(out, "source = {{ {} }}", fields.join(", "));
    if let Some(notes) = &entry.notes {
        let _ = writeln!(out, "notes = {}", quote(notes));
    }
    out
}

/// Line indexes of `[[offerings]]` headers.
fn offering_headers(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.trim() == "[[offerings]]")
        .map(|(index, _)| index)
        .collect()
}

/// Appends the change's entry to the end of its offering's section.
pub fn append_price(text: &str, change: &PriceChange) -> anyhow::Result<String> {
    let before: ModelFile = toml::from_str(text).context("parsing before edit")?;
    let headers = offering_headers(text);
    let index = change.target.offering_index;
    ensure!(
        headers.len() == before.offerings.len(),
        "offerings must be written as [[offerings]] tables to be synced"
    );
    let offering = &before.offerings[index];
    ensure!(
        offering.provider == change.target.provider
            && offering.upstream_id == change.target.upstream_id,
        "offering {index} is not {}",
        change.target.label()
    );
    let lines: Vec<&str> = text.lines().collect();
    let mut end = headers.get(index + 1).copied().unwrap_or(lines.len());
    while end > headers[index] + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut out = String::with_capacity(text.len() + 512);
    for line in &lines[..end] {
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&render_entry(&change.entry));
    if end < lines.len() {
        out.push('\n');
        for line in &lines[end..] {
            out.push_str(line);
            out.push('\n');
        }
    }

    let after: ModelFile = toml::from_str(&out).context("parsing after edit")?;
    let old = &before.offerings[index].prices;
    let new = &after.offerings[index].prices;
    ensure!(
        new.len() == old.len() + 1,
        "edit did not add exactly one price entry"
    );
    ensure!(
        new[..old.len()] == old[..],
        "edit changed existing price entries"
    );
    ensure!(
        new.last() == Some(&change.entry),
        "written entry does not round-trip"
    );
    for (i, (a, b)) in before.offerings.iter().zip(&after.offerings).enumerate() {
        if i != index {
            ensure!(a == b, "edit changed another offering");
        }
    }
    Ok(out)
}

/// Inserts a missing metadata field.
pub fn fill_metadata(text: &str, fill: &MetadataFill) -> anyhow::Result<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    match fill.field {
        "release_date" | "knowledge_cutoff" => {
            let at = lines
                .iter()
                .position(|l| l.trim_start().starts_with('['))
                .unwrap_or(lines.len());
            let mut at = at;
            while at > 0 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            out.insert(at, format!("{} = {}", fill.field, quote(&fill.value)));
        }
        "limits.context" | "limits.output" => {
            let key = fill.field.trim_start_matches("limits.");
            let value: u64 = fill.value.parse().context("limit value")?;
            if let Some(header) = lines.iter().position(|l| l.trim() == "[limits]") {
                out.insert(header + 1, format!("{key} = {value}"));
            } else {
                let at = lines
                    .iter()
                    .position(|l| l.trim() == "[[offerings]]")
                    .unwrap_or(lines.len());
                out.insert(at, String::new());
                out.insert(at, format!("{key} = {value}"));
                out.insert(at, "[limits]".to_owned());
            }
        }
        other => bail!("cannot fill unknown field {other}"),
    }
    let mut text_out = out.join("\n");
    text_out.push('\n');
    let parsed: ModelFile = toml::from_str(&text_out).context("parsing after metadata edit")?;
    let applied = match fill.field {
        "release_date" => parsed.release_date.map(|d| d.to_string()),
        "knowledge_cutoff" => parsed.knowledge_cutoff.map(|d| d.as_str().to_owned()),
        "limits.context" => parsed.limits.context.map(|v| v.to_string()),
        _ => parsed.limits.output.map(|v| v.to_string()),
    };
    ensure!(
        applied.as_deref() == Some(fill.value.as_str()),
        "metadata edit did not round-trip"
    );
    Ok(text_out)
}

/// Applies every change in the plan to files under `data_dir`.
pub fn apply(data_dir: &Path, plan: &Plan) -> anyhow::Result<usize> {
    let mut written = 0;
    for change in &plan.price_changes {
        let path = data_dir.join(&change.target.path);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let updated =
            append_price(&text, change).with_context(|| format!("editing {}", path.display()))?;
        fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
        written += 1;
    }
    for fill in &plan.metadata_fills {
        let path = data_dir.join(&fill.path);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let updated =
            fill_metadata(&text, fill).with_context(|| format!("editing {}", path.display()))?;
        fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
        written += 1;
    }
    Ok(written)
}
