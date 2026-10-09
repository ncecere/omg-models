//! `omg-models serve`: validate the data, build the JSON files in memory
//! (byte-identical to `omg-models build`) and serve site + API. With
//! `--data-url`, newer published snapshots replace the data at runtime
//! (see `live`).

use std::{net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use clap::Args;
use omg_models_catalog::{Severity, load::read_files, load_validated_files};
use omg_models_cli::{live, snapshot};
use omg_models_web::{
    AppState, AssetBundle, router,
    state::{DataInfo, DataSource, LiveState},
};

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
    /// URL of a published `catalog.manifest.json` to refresh the data from,
    /// e.g. https://github.com/ncecere/omg-models/releases/download/data-latest/catalog.manifest.json.
    /// Unset: serve the data directory only.
    #[arg(long, env = "OMG_MODELS_DATA_URL")]
    pub data_url: Option<String>,
    /// How often to check the data URL (`15m`, `1h`, `900`; at least 10s).
    #[arg(long, env = "OMG_MODELS_DATA_REFRESH", default_value = "15m", value_parser = live::parse_interval)]
    pub data_refresh: Duration,
    /// Size cap for the downloaded archive, in bytes (unpacked data may be
    /// up to 8x this).
    #[arg(long, env = "OMG_MODELS_DATA_MAX_BYTES", default_value_t = live::DEFAULT_MAX_BYTES)]
    pub data_max_bytes: u64,
    /// Allow `http://` data URLs on loopback addresses (local testing only).
    #[arg(long, env = "OMG_MODELS_DATA_ALLOW_HTTP_LOOPBACK", hide = true)]
    pub data_allow_http_loopback: bool,
    /// Commit of the data directory, shown in the footer and `/api/status`
    /// until a remote snapshot is loaded (the image sets it at build time).
    #[arg(long, env = "OMG_MODELS_DATA_COMMIT", hide = true)]
    pub data_commit: Option<String>,
}

pub fn run(args: &ServeArgs) -> anyhow::Result<ExitCode> {
    let files = read_files(&args.data)
        .with_context(|| format!("reading the data directory {}", args.data.display()))?;
    let (catalog, issues) = load_validated_files(&files);
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
    let data = DataInfo {
        source: DataSource::Embedded,
        commit: args
            .data_commit
            .clone()
            .filter(|c| c.len() == 40 && c.bytes().all(|b| b.is_ascii_hexdigit())),
        built_at: None,
        data_sha256: Some(snapshot::data_digest(&files)),
    };
    let live_state = Arc::new(LiveState::new(AppState::with_data(catalog, data)));

    if let Some(url) = &args.data_url {
        if args.data_refresh < Duration::from_secs(10) {
            bail!("--data-refresh must be at least 10s");
        }
        let mut config = live::Config::new(url.clone());
        config.max_bytes = args.data_max_bytes;
        config.allow_http_loopback = args.data_allow_http_loopback;
        let refresher = live::Refresher::new(Arc::clone(&live_state), config)
            .context("configuring the data refresh")?;
        refresher
            .spawn(args.data_refresh)
            .context("starting the data refresh thread")?;
        eprintln!(
            "data refresh: checking {url} every {}s",
            args.data_refresh.as_secs()
        );
    }

    let app = router(Arc::clone(&live_state), bundle);

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
