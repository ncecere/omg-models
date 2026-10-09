//! Published data snapshots: `catalog.tar.gz` (the data files and the JSON
//! API built from them) and `catalog.manifest.json` (commit, build time and
//! the SHA-256 of every file).
//!
//! `omg-models package` writes both ([`package`]); the server's live refresh
//! checks a downloaded pair ([`parse_manifest`], [`unpack`]) before it
//! validates and serves the data. Archive paths are `data/<path>` (exactly
//! the files of the data directory) and `dist/<path>` (exactly what
//! `omg-models build` writes).
//!
//! Digests (`data_sha256`, `total_sha256`) are SHA-256 over `sha256sum`-style
//! lines, `"<hex sha256>  <path>\n"`, for the covered files sorted by path
//! (bytewise). They depend on file contents and paths only, never on tar or
//! gzip metadata, so the publisher can tell whether anything changed.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, bail, ensure};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use omg_models_catalog::{
    DataFiles, Severity, export, load::read_files, load_validated_files, time::Timestamp,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The manifest format version this binary writes and accepts.
pub const SCHEMA_VERSION: u32 = 1;
/// `kind` of a snapshot manifest.
pub const KIND: &str = "omg-models-catalog-snapshot";
pub const ARCHIVE_NAME: &str = "catalog.tar.gz";
pub const MANIFEST_NAME: &str = "catalog.manifest.json";

/// Upper bound for a manifest document.
pub const MANIFEST_LIMIT: u64 = 1024 * 1024;
/// Upper bound for the number of files in a snapshot.
pub const MAX_FILES: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveInfo {
    /// File name of the archive, next to the manifest.
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub kind: String,
    /// Git commit (40 hex) the snapshot was built from.
    pub commit: String,
    /// `YYYY-MM-DDTHH:MM:SSZ`.
    pub built_at: String,
    /// The `omg-models` version that built it.
    pub generator: String,
    pub archive: ArchiveInfo,
    /// Digest over the `data/` files.
    pub data_sha256: String,
    /// Digest over every file.
    pub total_sha256: String,
    /// Every file in the archive, sorted by path.
    pub files: Vec<FileEntry>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The digest of `(path, sha256)` pairs (see the module docs).
pub fn digest<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut entries: Vec<(&str, &str)> = entries.into_iter().collect();
    entries.sort_unstable();
    let mut hasher = Sha256::new();
    for (path, sha) in entries {
        hasher.update(sha.as_bytes());
        hasher.update(b"  ");
        hasher.update(path.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().iter().fold(String::new(), |mut hex, b| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// The `data_sha256` of a data directory's files (as loaded into memory).
pub fn data_digest(files: &DataFiles) -> String {
    let hashed: Vec<(String, String)> = files
        .iter()
        .map(|(path, bytes)| (format!("data/{path}"), sha256_hex(bytes)))
        .collect();
    digest(hashed.iter().map(|(p, s)| (p.as_str(), s.as_str())))
}

/// A relative archive path: `data/...` or `dist/...`, `/`-separated, no
/// empty, `.` or `..` components, printable ASCII without backslashes.
fn check_path(path: &str) -> anyhow::Result<()> {
    ensure!(
        path.len() <= 512
            && path
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b'\\' && b != b':'),
        "invalid path {path:?}"
    );
    let mut parts = path.split('/');
    let top = parts.next().unwrap_or_default();
    ensure!(
        top == "data" || top == "dist",
        "path {path:?} is outside data/ and dist/"
    );
    let rest: Vec<&str> = parts.collect();
    ensure!(
        !rest.is_empty()
            && rest
                .iter()
                .all(|c| !c.is_empty() && *c != "." && *c != ".."),
        "invalid path {path:?}"
    );
    Ok(())
}

/// The snapshot files for a data tree: `data/<path>` and `dist/<path>`.
/// Fails unless the data passes the full validator.
pub fn collect(data: DataFiles) -> anyhow::Result<BTreeMap<String, Vec<u8>>> {
    let (catalog, issues) = load_validated_files(&data);
    let Some(catalog) = catalog else {
        let errors: Vec<String> = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(ToString::to_string)
            .collect();
        bail!("the data does not validate:\n{}", errors.join("\n"));
    };
    let mut files = BTreeMap::new();
    for (path, bytes) in data {
        files.insert(format!("data/{path}"), bytes);
    }
    for (path, bytes) in export::build(&catalog) {
        files.insert(format!("dist/{path}"), bytes);
    }
    for path in files.keys() {
        check_path(path)?;
    }
    Ok(files)
}

/// A reproducible `.tar.gz` of `files` (sorted, mtime 0, uid/gid 0, 0644).
pub fn archive(files: &BTreeMap<String, Vec<u8>>) -> anyhow::Result<Vec<u8>> {
    let gz = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::best());
    let mut builder = tar::Builder::new(gz);
    builder.mode(tar::HeaderMode::Deterministic);
    for (path, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder
            .append_data(&mut header, path, bytes.as_slice())
            .with_context(|| format!("archiving {path}"))?;
    }
    let gz = builder.into_inner().context("finishing the tar stream")?;
    gz.finish().context("finishing the gzip stream")
}

/// Builds the archive and its manifest for `data_dir` at `commit`.
pub fn package(
    data_dir: &Path,
    commit: &str,
    built_at: &Timestamp,
) -> anyhow::Result<(Vec<u8>, Manifest)> {
    ensure!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "--commit must be a full 40-character commit SHA"
    );
    let data = read_files(data_dir).with_context(|| format!("reading {}", data_dir.display()))?;
    seal(&collect(data)?, commit, built_at)
}

/// Archives `files` (already collected; not validated here) and describes
/// them in a manifest.
pub fn seal(
    files: &BTreeMap<String, Vec<u8>>,
    commit: &str,
    built_at: &Timestamp,
) -> anyhow::Result<(Vec<u8>, Manifest)> {
    let bytes = archive(files)?;
    let entries: Vec<FileEntry> = files
        .iter()
        .map(|(path, bytes)| FileEntry {
            path: path.clone(),
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
        })
        .collect();
    let all = || entries.iter().map(|e| (e.path.as_str(), e.sha256.as_str()));
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        kind: KIND.to_owned(),
        commit: commit.to_ascii_lowercase(),
        built_at: built_at.to_string(),
        generator: format!("omg-models {}", env!("CARGO_PKG_VERSION")),
        archive: ArchiveInfo {
            name: ARCHIVE_NAME.to_owned(),
            sha256: sha256_hex(&bytes),
            size: bytes.len() as u64,
        },
        data_sha256: digest(all().filter(|(p, _)| p.starts_with("data/"))),
        total_sha256: digest(all()),
        files: entries,
    };
    Ok((bytes, manifest))
}

/// The manifest as published (pretty JSON, trailing newline).
pub fn manifest_json(manifest: &Manifest) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(manifest).expect("manifest serializes");
    bytes.push(b'\n');
    bytes
}

/// Parses a manifest and checks it is internally consistent: known format,
/// well-formed fields, safe unique paths, and both digests recomputed from
/// the file list. Says nothing about the archive yet ([`unpack`] does).
pub fn parse_manifest(bytes: &[u8]) -> anyhow::Result<Manifest> {
    let value: serde_json::Value = serde_json::from_slice(bytes).context("manifest is not JSON")?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64);
    ensure!(
        version == Some(u64::from(SCHEMA_VERSION)),
        "unsupported manifest schema_version {} (this server reads {SCHEMA_VERSION})",
        value
            .get("schema_version")
            .map_or_else(|| "(missing)".to_owned(), ToString::to_string)
    );
    let manifest: Manifest = serde_json::from_value(value).context("malformed manifest")?;
    ensure!(
        manifest.kind == KIND,
        "manifest kind is {:?}",
        manifest.kind
    );
    ensure!(
        manifest.commit.len() == 40
            && manifest
                .commit
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "manifest commit {:?} is not a 40-character SHA",
        manifest.commit
    );
    ensure!(
        Timestamp::parse(&manifest.built_at).is_some(),
        "manifest built_at {:?} is not YYYY-MM-DDTHH:MM:SSZ",
        manifest.built_at
    );
    let name = &manifest.archive.name;
    ensure!(
        !name.is_empty()
            && name.len() <= 128
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')),
        "archive name {name:?} is not a plain file name"
    );
    ensure!(
        is_sha256(&manifest.archive.sha256)
            && is_sha256(&manifest.data_sha256)
            && is_sha256(&manifest.total_sha256),
        "manifest digests must be lowercase SHA-256 hex"
    );
    ensure!(
        !manifest.files.is_empty() && manifest.files.len() <= MAX_FILES,
        "manifest lists {} files",
        manifest.files.len()
    );
    let mut previous: Option<&str> = None;
    for entry in &manifest.files {
        check_path(&entry.path)?;
        ensure!(is_sha256(&entry.sha256), "bad sha256 for {}", entry.path);
        if let Some(previous) = previous {
            ensure!(
                previous < entry.path.as_str(),
                "manifest files are not sorted and unique at {}",
                entry.path
            );
        }
        previous = Some(&entry.path);
    }
    ensure!(
        manifest.files.iter().any(|e| e.path.starts_with("data/")),
        "manifest has no data/ files"
    );
    let all = || {
        manifest
            .files
            .iter()
            .map(|e| (e.path.as_str(), e.sha256.as_str()))
    };
    ensure!(
        digest(all()) == manifest.total_sha256,
        "total_sha256 does not match the file list"
    );
    ensure!(
        digest(all().filter(|(p, _)| p.starts_with("data/"))) == manifest.data_sha256,
        "data_sha256 does not match the file list"
    );
    Ok(manifest)
}

/// A verified snapshot, split into the data tree and the published API files.
#[derive(Debug)]
pub struct Unpacked {
    /// Relative to the data directory (`providers/...`).
    pub data: DataFiles,
    /// Relative to `dist/` (`api.json`, `api/v1/...`).
    pub dist: BTreeMap<String, Vec<u8>>,
}

/// Checks `archive` against `manifest` and unpacks it in memory: the
/// archive's size and SHA-256, at most `max_unpacked` bytes after
/// decompression, regular files only, safe paths, no duplicates, and exactly
/// the manifest's files with matching sizes and SHA-256s.
pub fn unpack(manifest: &Manifest, archive: &[u8], max_unpacked: u64) -> anyhow::Result<Unpacked> {
    ensure!(
        archive.len() as u64 == manifest.archive.size,
        "archive is {} bytes, manifest says {}",
        archive.len(),
        manifest.archive.size
    );
    let actual = sha256_hex(archive);
    ensure!(
        actual == manifest.archive.sha256,
        "archive SHA-256 {actual} does not match the manifest ({})",
        manifest.archive.sha256
    );
    let expected: BTreeMap<&str, &FileEntry> = manifest
        .files
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let total: u64 = manifest.files.iter().map(|e| e.size).sum();
    ensure!(
        total <= max_unpacked,
        "snapshot is {total} bytes unpacked (limit {max_unpacked})"
    );

    // The decoder can read at most the tar stream for the listed files plus
    // headers; cap it so a gzip bomb stops early.
    let cap = max_unpacked
        .saturating_add(1024 * (manifest.files.len() as u64 + 16))
        .saturating_add(1024 * 1024);
    let mut tar = tar::Archive::new(GzDecoder::new(archive).take(cap));
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry.context("reading the archive")?;
        let kind = entry.header().entry_type();
        let path = String::from_utf8(entry.path_bytes().into_owned())
            .context("archive path is not UTF-8")?;
        let path = path.trim_start_matches("./").to_owned();
        if kind.is_dir() {
            continue;
        }
        ensure!(
            kind.is_file(),
            "archive entry {path:?} is not a regular file ({kind:?})"
        );
        check_path(&path)?;
        let Some(want) = expected.get(path.as_str()) else {
            bail!("archive has {path:?}, which the manifest does not list");
        };
        ensure!(
            entry.size() == want.size,
            "{path}: {} bytes, manifest says {}",
            entry.size(),
            want.size
        );
        ensure!(
            !files.contains_key(&path),
            "{path} appears twice in the archive"
        );
        let mut bytes = Vec::with_capacity(usize::try_from(want.size).unwrap_or(0));
        entry
            .by_ref()
            .take(want.size + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("reading {path}"))?;
        ensure!(bytes.len() as u64 == want.size, "{path}: truncated");
        let sha = sha256_hex(&bytes);
        ensure!(
            sha == want.sha256,
            "{path}: SHA-256 {sha} does not match the manifest ({})",
            want.sha256
        );
        files.insert(path, bytes);
    }
    for path in expected.keys() {
        ensure!(files.contains_key(*path), "archive is missing {path}");
    }
    let mut out = Unpacked {
        data: DataFiles::new(),
        dist: BTreeMap::new(),
    };
    for (path, bytes) in files {
        if let Some(rest) = path.strip_prefix("data/") {
            out.data.insert(rest.to_owned(), bytes);
        } else if let Some(rest) = path.strip_prefix("dist/") {
            out.dist.insert(rest.to_owned(), bytes);
        }
    }
    Ok(out)
}

/// `omg-models package`: writes the archive and manifest into `out`.
pub fn write_package(
    data_dir: &Path,
    out: &Path,
    commit: &str,
    built_at: &Timestamp,
) -> anyhow::Result<Manifest> {
    let (bytes, manifest) = package(data_dir, commit, built_at)?;
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let archive_path = out.join(ARCHIVE_NAME);
    std::fs::File::create(&archive_path)
        .and_then(|mut f| f.write_all(&bytes))
        .with_context(|| format!("writing {}", archive_path.display()))?;
    let manifest_path = out.join(MANIFEST_NAME);
    std::fs::write(&manifest_path, manifest_json(&manifest))
        .with_context(|| format!("writing {}", manifest_path.display()))?;
    Ok(manifest)
}
