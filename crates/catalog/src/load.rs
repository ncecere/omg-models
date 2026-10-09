//! Reading the data tree, from a directory ([`load`]) or from files already
//! in memory ([`load_files`], used for live data refreshes). Both walk the
//! same layout with the same rules, so a snapshot loads identically either
//! way. Reading the data directory is the only I/O in this crate.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
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

/// Data files in memory: path relative to the data directory
/// (`providers/openai/provider.toml`, `/`-separated) -> bytes.
pub type DataFiles = BTreeMap<String, Vec<u8>>;

/// One directory entry: its name and whether it is a directory.
#[derive(Debug)]
struct Entry {
    name: String,
    is_dir: bool,
}

/// A read-only view of a data tree. Paths are relative and `/`-separated.
trait Tree {
    /// The entries of `dir`, sorted by name, hidden (`.`) entries removed.
    fn list(&self, dir: &str) -> io::Result<Vec<Entry>>;
    fn read(&self, path: &str) -> io::Result<String>;
    /// Whether `dir` exists (as anything).
    fn exists(&self, dir: &str) -> bool;
}

struct Disk<'a>(&'a Path);

impl Disk<'_> {
    fn path(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.0.to_path_buf(), |p, s| p.join(s))
    }
}

impl Tree for Disk<'_> {
    fn list(&self, dir: &str) -> io::Result<Vec<Entry>> {
        let mut entries = fs::read_dir(self.path(dir))?
            .map(|entry| {
                entry.map(|e| {
                    let path = e.path();
                    Entry {
                        name: file_name(&path),
                        is_dir: path.is_dir(),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        entries.retain(|e| !e.name.starts_with('.'));
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn read(&self, path: &str) -> io::Result<String> {
        fs::read_to_string(self.path(path))
    }

    fn exists(&self, dir: &str) -> bool {
        self.path(dir).exists()
    }
}

struct Memory<'a>(&'a DataFiles);

impl Tree for Memory<'_> {
    fn list(&self, dir: &str) -> io::Result<Vec<Entry>> {
        if self.0.contains_key(dir) {
            return Err(io::Error::other("not a directory"));
        }
        let prefix = format!("{dir}/");
        let mut names: BTreeMap<String, bool> = BTreeMap::new();
        for path in self.0.keys() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((name, _)) => names.insert(name.to_owned(), true),
                None => names.insert(rest.to_owned(), false),
            };
        }
        if names.is_empty() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(names
            .into_iter()
            .filter(|(name, _)| !name.starts_with('.') && !name.is_empty())
            .map(|(name, is_dir)| Entry { name, is_dir })
            .collect())
    }

    fn read(&self, path: &str) -> io::Result<String> {
        let bytes = self
            .0
            .get(path)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        String::from_utf8(bytes.clone()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )
        })
    }

    fn exists(&self, dir: &str) -> bool {
        let prefix = format!("{dir}/");
        self.0.contains_key(dir) || self.0.keys().any(|p| p.starts_with(&prefix))
    }
}

/// Loads `<data>/providers/*/provider.toml` and
/// `<data>/providers/*/models/*.toml`. Parse errors are returned as issues
/// (with file paths) rather than aborting, so `validate` reports them all.
pub fn load(data_dir: &Path) -> (Catalog, Vec<Issue>) {
    load_tree(&Disk(data_dir))
}

/// [`load`] for a data tree held in memory (paths relative to the data
/// directory). Entries outside `providers/` are ignored, as on disk.
pub fn load_files(files: &DataFiles) -> (Catalog, Vec<Issue>) {
    load_tree(&Memory(files))
}

/// Every non-hidden regular file under `data_dir` (recursively), keyed by its
/// `/`-separated relative path. This is what a published snapshot carries.
pub fn read_files(data_dir: &Path) -> io::Result<DataFiles> {
    fn walk(base: &Path, dir: &Path, out: &mut DataFiles) -> io::Result<()> {
        let mut paths = fs::read_dir(dir)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        for path in paths {
            if file_name(&path).starts_with('.') {
                continue;
            }
            let meta = fs::metadata(&path)?;
            if meta.is_dir() {
                walk(base, &path, out)?;
            } else if meta.is_file() {
                out.insert(rel(base, &path), fs::read(&path)?);
            }
        }
        Ok(())
    }
    let mut out = DataFiles::new();
    walk(data_dir, data_dir, &mut out)?;
    Ok(out)
}

fn load_tree(tree: &dyn Tree) -> (Catalog, Vec<Issue>) {
    let mut catalog = Catalog::default();
    let mut issues = Vec::new();
    let entries = match tree.list("providers") {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(Issue::error(
                "providers",
                format!("cannot read providers directory: {error}"),
            ));
            return (catalog, issues);
        }
    };

    let mut seen = BTreeSet::new();
    for dir in entries.into_iter().filter(|e| e.is_dir) {
        let dir_name = dir.name;
        seen.insert(dir_name.clone());
        let provider_rel = format!("providers/{dir_name}/provider.toml");
        match read_toml::<ProviderFile>(tree, &provider_rel) {
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

        let models_dir = format!("providers/{dir_name}/models");
        if !tree.exists(&models_dir) {
            continue;
        }
        let model_entries = match tree.list(&models_dir) {
            Ok(entries) => entries,
            Err(error) => {
                issues.push(Issue::error(
                    models_dir,
                    format!("cannot read models directory: {error}"),
                ));
                continue;
            }
        };
        for entry in model_entries {
            let model_rel = format!("{models_dir}/{}", entry.name);
            let stem = match entry.name.rsplit_once('.') {
                Some((stem, "toml")) if !stem.is_empty() && !entry.is_dir => stem.to_owned(),
                _ => {
                    issues.push(Issue::error(
                        model_rel,
                        "unexpected entry in models/ (only <model-id>.toml files are allowed)",
                    ));
                    continue;
                }
            };
            let file = match read_toml::<ModelFile>(tree, &model_rel) {
                Ok(file) => file,
                Err(message) => {
                    issues.push(Issue::error(model_rel, message));
                    continue;
                }
            };
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

fn read_toml<T: serde::de::DeserializeOwned>(tree: &dyn Tree, path: &str) -> Result<T, String> {
    let text = tree
        .read(path)
        .map_err(|error| format!("cannot read file: {error}"))?;
    toml::from_str(&text).map_err(|error| error.to_string().trim_end().to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_tree_lists_like_a_directory() {
        let mut files = DataFiles::new();
        files.insert("providers/a/provider.toml".into(), b"x".to_vec());
        files.insert("providers/a/models/m.toml".into(), b"y".to_vec());
        files.insert("providers/.hidden/provider.toml".into(), b"z".to_vec());
        files.insert("providers/README.md".into(), b"r".to_vec());
        let tree = Memory(&files);
        let names: Vec<(String, bool)> = tree
            .list("providers")
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.is_dir))
            .collect();
        assert_eq!(names, vec![("README.md".into(), false), ("a".into(), true)]);
        assert!(tree.exists("providers/a/models"));
        assert!(!tree.exists("providers/b/models"));
        assert!(tree.list("providers/a/provider.toml").is_err());
        assert_eq!(
            tree.list("missing").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
