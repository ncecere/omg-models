//! Application state: the validated catalog and the pre-built JSON files
//! ([`AppState`], one immutable snapshot), and the swappable holder the
//! server shares between requests and the data refresher ([`LiveState`]).

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{Arc, Mutex, PoisonError, RwLock},
};

use omg_models_catalog::{Catalog, export};
use serde::Serialize;
use sha2::{Digest, Sha256};
use topcoat::context::{Cx, app_context, memoize};

/// One static JSON file served under `/api.json` or `/api/v1/...`.
#[derive(Clone, Debug)]
pub struct ApiFile {
    pub bytes: Vec<u8>,
    /// Strong validator: `"sha256-<first 32 hex>"`.
    pub etag: String,
}

/// Where the data being served came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DataSource {
    /// The snapshot baked into the binary's data directory at startup.
    #[default]
    Embedded,
    /// A published snapshot downloaded and verified at runtime.
    Remote,
}

/// Identity of the data snapshot being served.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DataInfo {
    pub source: DataSource,
    /// Git commit the snapshot was built from, when known.
    pub commit: Option<String>,
    /// When the snapshot was built (`YYYY-MM-DDTHH:MM:SSZ`), when known.
    pub built_at: Option<String>,
    /// SHA-256 over the snapshot's data files (see `docs/operations.md`).
    pub data_sha256: Option<String>,
}

/// Everything a request needs: one immutable, validated snapshot.
#[derive(Debug)]
pub struct AppState {
    pub catalog: Catalog,
    /// URL path (e.g. `/api/v1/models.json`) -> file.
    pub files: BTreeMap<String, ApiFile>,
    pub last_updated: Option<String>,
    pub data: DataInfo,
}

impl AppState {
    /// Builds the state from a validated catalog. The JSON files are the
    /// exact bytes `omg-models build` writes to `dist/`.
    pub fn new(catalog: Catalog) -> Self {
        Self::with_data(catalog, DataInfo::default())
    }

    /// [`AppState::new`] with the snapshot's identity.
    pub fn with_data(catalog: Catalog, data: DataInfo) -> Self {
        let files = export::build(&catalog)
            .into_iter()
            .map(|(path, bytes)| {
                let digest = Sha256::digest(&bytes);
                let hex = digest.iter().take(16).fold(String::new(), |mut hex, b| {
                    let _ = write!(hex, "{b:02x}");
                    hex
                });
                (
                    format!("/{path}"),
                    ApiFile {
                        bytes,
                        etag: format!("\"sha256-{hex}\""),
                    },
                )
            })
            .collect();
        let last_updated = export::last_updated(&catalog);
        Self {
            catalog,
            files,
            last_updated,
            data,
        }
    }
}

/// The outcome of the last refresh attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshResult {
    /// A new snapshot was verified, validated and is now served.
    Updated,
    /// The published snapshot is the one being served (or 304).
    Unchanged,
    /// The attempt failed; the previous snapshot is still served.
    Failed,
}

/// Live-refresh bookkeeping, reported by `/api/status`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RefreshStatus {
    /// Whether a data URL is configured.
    pub enabled: bool,
    pub url: Option<String>,
    pub interval_seconds: Option<u64>,
    pub last_attempt_at: Option<String>,
    pub last_result: Option<RefreshResult>,
    /// The last failure's message (cleared by the next success).
    pub last_error: Option<String>,
    pub last_success_at: Option<String>,
    /// When the served snapshot was last replaced.
    pub last_update_at: Option<String>,
    pub consecutive_failures: u32,
}

/// The served snapshot and the refresh status, shared by every request and
/// the refresher. Swapping replaces the whole snapshot at once; a request
/// sees exactly one snapshot from start to finish ([`current`]).
#[derive(Debug)]
pub struct LiveState {
    current: RwLock<Arc<AppState>>,
    refresh: Mutex<RefreshStatus>,
}

impl LiveState {
    pub fn new(state: AppState) -> Self {
        Self {
            current: RwLock::new(Arc::new(state)),
            refresh: Mutex::new(RefreshStatus::default()),
        }
    }

    /// The snapshot being served now.
    pub fn snapshot(&self) -> Arc<AppState> {
        Arc::clone(&self.current.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Atomically replaces the served snapshot. In-flight requests finish
    /// with the snapshot they started with.
    pub fn swap(&self, state: AppState) {
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(state);
    }

    pub fn refresh_status(&self) -> RefreshStatus {
        self.refresh
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Updates the refresh status in place.
    pub fn update_refresh(&self, f: impl FnOnce(&mut RefreshStatus)) {
        f(&mut self.refresh.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

impl From<AppState> for LiveState {
    fn from(state: AppState) -> Self {
        Self::new(state)
    }
}

/// The router's app context: the shared live state.
#[derive(Clone, Debug)]
pub struct Live(pub Arc<LiveState>);

/// The snapshot for this request, taken once and reused by the layout and
/// the page, so a concurrent swap never mixes two snapshots in one response.
#[memoize]
fn request_snapshot(cx: &Cx) -> Arc<AppState> {
    app_context::<Live>(cx).0.snapshot()
}

/// The snapshot serving this request.
pub fn current(cx: &Cx) -> &AppState {
    request_snapshot(cx)
}

/// The shared live state (for status reporting).
pub fn live(cx: &Cx) -> &LiveState {
    &app_context::<Live>(cx).0
}
