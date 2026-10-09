//! `omg-models serve`: validate the data, build the JSON files in memory
//! (byte-identical to `omg-models build`) and serve site + API.

use std::{net::SocketAddr, path::PathBuf, process::ExitCode};

use anyhow::{Context, bail};
use clap::Args;
use omg_models_catalog::{Severity, load_validated};
use omg_models_web::{AppState, AssetBundle, router};

#[derive(Args)]
pub struct ServeArgs {
    #[arg(long, env = "OMG_MODELS_DATA", default_value = "data")]
    pub data: PathBuf,
    /// Address to listen on.
    #[arg(long, env = "OMG_MODELS_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: SocketAddr,
    /// Topcoat asset bundle directory (default: `assets/` next to the binary).
    #[arg(long, env = "OMG_MODELS_ASSETS")]
    pub assets: Option<PathBuf>,
}

pub fn run(args: &ServeArgs) -> anyhow::Result<ExitCode> {
    let (catalog, issues) = load_validated(&args.data);
    for issue in &issues {
        eprintln!("{issue}");
    }
    let Some(catalog) = catalog else {
        let errors = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count();
        bail!(
            "refusing to serve: {errors} validation error(s) in {}",
            args.data.display()
        );
    };
    let bundle = match &args.assets {
        Some(dir) => AssetBundle::load_dir(dir)
            .with_context(|| format!("loading the asset bundle from {}", dir.display()))?,
        None => AssetBundle::load()
            .context("loading the asset bundle next to the binary (run `topcoat asset bundle` or set OMG_MODELS_ASSETS)")?,
    };
    let models = catalog.models.len();
    let app = router(AppState::new(catalog), bundle);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(args.listen)
            .await
            .with_context(|| format!("binding {}", args.listen))?;
        let local = listener.local_addr()?;
        eprintln!(
            "omg-models {} serving {models} models on http://{local}",
            env!("CARGO_PKG_VERSION")
        );
        omg_models_web::topcoat::serve(listener, app)
            .await
            .context("serving")
    })?;
    Ok(ExitCode::SUCCESS)
}
