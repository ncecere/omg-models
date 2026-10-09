//! Polite fetching: a descriptive User-Agent, one request per source per
//! run, conditional requests (ETag) against a local cache, no redirects, a
//! size cap and a timeout. GitHub-hosted sources are pinned to the commit
//! that last touched the file, so provenance URLs are reproducible.

use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, bail};
use omg_models_catalog::{model::SourceKind, time::Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::observed::Document;

pub const USER_AGENT: &str = concat!(
    "omg-models-sync/",
    env!("CARGO_PKG_VERSION"),
    " (+https://models.omg.bitop.dev/about; https://github.com/ncecere/omg-models)"
);

/// Upper bound for one source document (models.dev is about 5.4 MB today).
const BODY_LIMIT: u64 = 32 * 1024 * 1024;

pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

#[derive(Default, Serialize, Deserialize)]
struct CacheMeta {
    url: String,
    etag: Option<String>,
    version: Option<String>,
}

pub struct Fetcher {
    agent: ureq::Agent,
    cache_dir: Option<PathBuf>,
    github_token: Option<String>,
}

impl Fetcher {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .build()
            .into();
        Self {
            agent,
            cache_dir,
            github_token: std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()),
        }
    }

    fn cache_paths(&self, name: &str) -> Option<(PathBuf, PathBuf)> {
        let dir = self.cache_dir.as_ref()?;
        Some((
            dir.join(format!("{name}.body")),
            dir.join(format!("{name}.meta.json")),
        ))
    }

    fn read_cache(&self, name: &str) -> Option<(CacheMeta, Vec<u8>)> {
        let (body, meta) = self.cache_paths(name)?;
        let meta: CacheMeta = serde_json::from_slice(&fs::read(meta).ok()?).ok()?;
        Some((meta, fs::read(body).ok()?))
    }

    fn write_cache(&self, name: &str, meta: &CacheMeta, bytes: &[u8]) {
        let Some((body, meta_path)) = self.cache_paths(name) else {
            return;
        };
        if let Some(dir) = body.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(body, bytes);
        if let Ok(json) = serde_json::to_vec_pretty(meta) {
            let _ = fs::write(meta_path, json);
        }
    }

    /// GET with an optional `If-None-Match`. Returns (status, body, etag).
    fn get(
        &self,
        url: &str,
        etag: Option<&str>,
        github_api: bool,
    ) -> anyhow::Result<(u16, Vec<u8>, Option<String>)> {
        let mut request = self.agent.get(url).header("Accept", "application/json");
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }
        if github_api {
            request = request.header("X-GitHub-Api-Version", "2022-11-28");
            if let Some(token) = &self.github_token {
                request = request.header("Authorization", &format!("Bearer {token}"));
            }
        }
        let mut response = request.call().with_context(|| format!("GET {url}"))?;
        let status = response.status().as_u16();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()
            .with_context(|| format!("reading {url}"))?;
        Ok((status, body, etag))
    }

    /// Fetches `url`, revalidating a cached copy with its ETag.
    pub fn fetch(&self, name: &str, kind: SourceKind, url: &str) -> anyhow::Result<Document> {
        let cached = self.read_cache(name).filter(|(meta, _)| meta.url == url);
        let etag = cached.as_ref().and_then(|(meta, _)| meta.etag.clone());
        let (status, body, new_etag) = self.get(url, etag.as_deref(), false)?;
        let (bytes, etag) = match status {
            304 => match cached {
                Some((meta, bytes)) => (bytes, meta.etag),
                None => bail!("GET {url}: 304 without a cached copy"),
            },
            200..=299 => (body, new_etag),
            other => bail!("GET {url}: HTTP {other}"),
        };
        self.write_cache(
            name,
            &CacheMeta {
                url: url.to_owned(),
                etag: etag.clone(),
                version: etag.clone(),
            },
            &bytes,
        );
        Ok(Document {
            kind,
            url: url.to_owned(),
            fetched_at: Timestamp::now(),
            version: etag.map(|e| format!("etag:{}", e.trim_start_matches("W/").trim_matches('"'))),
            sha256: sha256_hex(&bytes),
            bytes,
        })
    }

    /// The latest commit touching `path` in `repo` (one GitHub API call).
    fn latest_commit(&self, repo: &str, path: &str) -> anyhow::Result<String> {
        let url = format!("https://api.github.com/repos/{repo}/commits?path={path}&per_page=1");
        let (status, body, _) = self.get(&url, None, true)?;
        if status != 200 {
            bail!("GET {url}: HTTP {status}");
        }
        let value: serde_json::Value =
            serde_json::from_slice(&body).context("GitHub commits JSON")?;
        let sha = value
            .get(0)
            .and_then(|c| c.get("sha"))
            .and_then(serde_json::Value::as_str)
            .context("no commit sha in GitHub response")?;
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("unexpected commit sha {sha:?}");
        }
        Ok(sha.to_owned())
    }

    /// Fetches a file from a GitHub repository pinned to its latest commit.
    /// Skips the download when the cached copy is from the same commit.
    /// Falls back to the `main` URL (versioned by ETag) when the API fails.
    pub fn fetch_github(
        &self,
        name: &str,
        kind: SourceKind,
        repo: &str,
        path: &str,
        main_url: &str,
    ) -> anyhow::Result<(Document, Option<String>)> {
        let sha = match self.latest_commit(repo, path) {
            Ok(sha) => sha,
            Err(error) => {
                let warning =
                    format!("could not pin {repo} to a commit ({error:#}); used {main_url}");
                return Ok((self.fetch(name, kind, main_url)?, Some(warning)));
            }
        };
        let url = format!("https://raw.githubusercontent.com/{repo}/{sha}/{path}");
        if let Some((meta, bytes)) = self.read_cache(name)
            && meta.version.as_deref() == Some(sha.as_str())
        {
            return Ok((
                Document {
                    kind,
                    url,
                    fetched_at: Timestamp::now(),
                    version: Some(sha),
                    sha256: sha256_hex(&bytes),
                    bytes,
                },
                None,
            ));
        }
        let (status, bytes, etag) = self.get(&url, None, false)?;
        if !(200..300).contains(&status) {
            bail!("GET {url}: HTTP {status}");
        }
        self.write_cache(
            name,
            &CacheMeta {
                url: url.clone(),
                etag,
                version: Some(sha.clone()),
            },
            &bytes,
        );
        Ok((
            Document {
                kind,
                url,
                fetched_at: Timestamp::now(),
                version: Some(sha),
                sha256: sha256_hex(&bytes),
                bytes,
            },
            None,
        ))
    }
}

/// Reads a recorded document from disk (offline runs and tests).
pub fn from_file(
    path: &std::path::Path,
    kind: SourceKind,
    url: &str,
    version: &str,
) -> anyhow::Result<Document> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(Document {
        kind,
        url: url.to_owned(),
        fetched_at: Timestamp::now(),
        version: Some(version.to_owned()),
        sha256: sha256_hex(&bytes),
        bytes,
    })
}
