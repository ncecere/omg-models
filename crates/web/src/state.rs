//! Application state: the validated catalog and the pre-built JSON files.

use std::{collections::BTreeMap, fmt::Write as _};

use omg_models_catalog::{Catalog, export};
use sha2::{Digest, Sha256};

/// One static JSON file served under `/api.json` or `/api/v1/...`.
#[derive(Clone, Debug)]
pub struct ApiFile {
    pub bytes: Vec<u8>,
    /// Strong validator: `"sha256-<first 32 hex>"`.
    pub etag: String,
}

/// Everything a request needs, shared through Topcoat's app context.
#[derive(Debug)]
pub struct AppState {
    pub catalog: Catalog,
    /// URL path (e.g. `/api/v1/models.json`) -> file.
    pub files: BTreeMap<String, ApiFile>,
    pub last_updated: Option<String>,
}

impl AppState {
    /// Builds the state from a validated catalog. The JSON files are the
    /// exact bytes `omg-models build` writes to `dist/`.
    pub fn new(catalog: Catalog) -> Self {
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
        }
    }
}
