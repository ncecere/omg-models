//! `omg-models`: validate, build, sync and serve the Open Model Catalog.

mod healthcheck;
mod serve;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::Context;
use clap::{Parser, Subcommand};
use omg_models_catalog::{Severity, export, load_validated, schema};
use omg_models_cli::sync;

#[derive(Parser)]
#[command(
    name = "omg-models",
    version,
    about = "Open Model Catalog: an open catalog of AI models and prices"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check the data directory: schema, ids, dates, decimals, tiers, provenance.
    Validate {
        #[arg(long, env = "OMG_MODELS_DATA", default_value = "data")]
        data: PathBuf,
        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,
    },
    /// Write the JSON API (api.json, api/v1/...) to the output directory.
    Build {
        #[arg(long, env = "OMG_MODELS_DATA", default_value = "data")]
        data: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
    },
    /// Print the JSON Schema of provider.toml and model files.
    Schema {
        /// Write to a file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Compare the catalog with upstream price sources and append changes.
    Sync(sync::SyncArgs),
    /// Serve the website and the JSON API.
    Serve(serve::ServeArgs),
    /// Probe a running server's /healthz (for container health checks).
    Healthcheck(healthcheck::HealthcheckArgs),
}

fn print_issues(issues: &[omg_models_catalog::Issue]) -> (usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    for issue in issues {
        eprintln!("{issue}");
        match issue.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
        }
    }
    (errors, warnings)
}

fn validate(data: &Path, strict: bool) -> ExitCode {
    let (catalog, issues) = load_validated(data);
    let (errors, warnings) = print_issues(&issues);
    let Some(catalog) = catalog else {
        eprintln!("{errors} error(s), {warnings} warning(s)");
        return ExitCode::FAILURE;
    };
    let offerings: usize = catalog
        .models
        .values()
        .map(|m| m.file.offerings.len())
        .sum();
    let prices: usize = catalog
        .models
        .values()
        .flat_map(|m| &m.file.offerings)
        .map(|o| o.prices.len())
        .sum();
    println!(
        "ok: {} providers, {} models, {offerings} offerings, {prices} price entries ({warnings} warning(s))",
        catalog.providers.len(),
        catalog.models.len()
    );
    if strict && warnings > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn build(data: &Path, out: &Path) -> anyhow::Result<ExitCode> {
    let (catalog, issues) = load_validated(data);
    let (errors, _) = print_issues(&issues);
    let Some(catalog) = catalog else {
        eprintln!("not building: {errors} validation error(s)");
        return Ok(ExitCode::FAILURE);
    };
    let artifacts = export::build(&catalog);
    // Replace the API tree wholesale so removed models leave no stale files.
    for stale in [out.join("api"), out.join("api.json")] {
        if stale.is_dir() {
            fs::remove_dir_all(&stale).with_context(|| format!("removing {}", stale.display()))?;
        } else if stale.is_file() {
            fs::remove_file(&stale).with_context(|| format!("removing {}", stale.display()))?;
        }
    }
    for (path, bytes) in &artifacts {
        let target = out.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&target, bytes).with_context(|| format!("writing {}", target.display()))?;
    }
    println!("wrote {} files to {}", artifacts.len(), out.display());
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Validate { data, strict } => Ok(validate(&data, strict)),
        Command::Build { data, out } => build(&data, &out),
        Command::Schema { out } => {
            let text = schema::schemas_json();
            if let Some(path) = out {
                fs::write(&path, text)
                    .with_context(|| format!("writing {}", path.display()))
                    .map(|()| ExitCode::SUCCESS)
            } else {
                print!("{text}");
                Ok(ExitCode::SUCCESS)
            }
        }
        Command::Sync(args) => sync::run(&args),
        Command::Serve(args) => serve::run(&args),
        Command::Healthcheck(args) => Ok(healthcheck::run(&args)),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
