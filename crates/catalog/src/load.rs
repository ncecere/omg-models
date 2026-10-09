//! Reading the data directory. The only I/O in this crate.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    Issue,
    model::{ModelFile, ProviderFile},
};

/// A provider definition and the file it came from.
#[derive(Clone, Debug)]
pub struct Provider {
    pub file: ProviderFile,
    /// Path relative to the data directory.
    pub path: String,
}

/// A model definition, its file, and its home provider (the directory).
#[derive(Clone, Debug)]
pub struct Model {
    pub file: ModelFile,
    pub path: String,
    pub home_provider: String,
}

/// The whole catalog as loaded from disk (not yet validated).
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    /// Keyed by provider id (directory name).
    pub providers: BTreeMap<String, Provider>,
    /// Keyed by model id. Duplicate ids are reported by the loader.
    pub models: BTreeMap<String, Model>,
}

/// Loads `<data>/providers/*/provider.toml` and
/// `<data>/providers/*/models/*.toml`. Parse errors are returned as issues
/// (with file paths) rather than aborting, so `validate` reports them all.
pub fn load(data_dir: &Path) -> (Catalog, Vec<Issue>) {
    let mut catalog = Catalog::default();
    let mut issues = Vec::new();
    let providers_dir = data_dir.join("providers");
    let entries = match sorted_entries(&providers_dir) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(Issue::error(
                rel(data_dir, &providers_dir),
                format!("cannot read providers directory: {error}"),
            ));
            return (catalog, issues);
        }
    };

    for dir in entries.into_iter().filter(|path| path.is_dir()) {
        let dir_name = file_name(&dir);
        let provider_path = dir.join("provider.toml");
        let provider_rel = rel(data_dir, &provider_path);
        match read_toml::<ProviderFile>(&provider_path) {
            Ok(file) => {
                catalog.providers.insert(
                    dir_name.clone(),
                    Provider {
                        file,
                        path: provider_rel,
                    },
                );
            }
            Err(message) => issues.push(Issue::error(provider_rel, message)),
        }

        let models_dir = dir.join("models");
        if !models_dir.exists() {
            continue;
        }
        let model_paths = match sorted_entries(&models_dir) {
            Ok(paths) => paths,
            Err(error) => {
                issues.push(Issue::error(
                    rel(data_dir, &models_dir),
                    format!("cannot read models directory: {error}"),
                ));
                continue;
            }
        };
        for path in model_paths {
            let model_rel = rel(data_dir, &path);
            if path.extension().and_then(|e| e.to_str()) != Some("toml") || !path.is_file() {
                issues.push(Issue::error(
                    model_rel,
                    "unexpected entry in models/ (only <model-id>.toml files are allowed)",
                ));
                continue;
            }
            let file = match read_toml::<ModelFile>(&path) {
                Ok(file) => file,
                Err(message) => {
                    issues.push(Issue::error(model_rel, message));
                    continue;
                }
            };
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            if file.id != stem {
                issues.push(Issue::error(
                    model_rel.clone(),
                    format!("id {:?} must equal the file name {stem:?}", file.id),
                ));
            }
            if let Some(existing) = catalog.models.get(&file.id) {
                issues.push(Issue::error(
                    model_rel,
                    format!(
                        "duplicate model id {:?} (already defined in {})",
                        file.id, existing.path
                    ),
                ));
                continue;
            }
            catalog.models.insert(
                file.id.clone(),
                Model {
                    file,
                    path: model_rel,
                    home_provider: dir_name.clone(),
                },
            );
        }
    }
    (catalog, issues)
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("cannot read file: {error}"))?;
    toml::from_str(&text).map_err(|error| error.to_string().trim_end().to_owned())
}

fn sorted_entries(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|p| !file_name(p).starts_with('.'));
    paths.sort();
    Ok(paths)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn rel(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
