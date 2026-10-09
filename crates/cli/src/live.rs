//! Live data refresh for `omg-models serve --data-url`.
//!
//! The server starts with the snapshot in its data directory (baked into the
//! image) and then, on an interval, asks for the published manifest with
//! `If-None-Match`. When the manifest names different data it downloads the
//! archive, checks every SHA-256 against the manifest, unpacks it in memory,
//! runs the full validator and atomically swaps the served snapshot (catalog,
//! JSON API and pages together). Any failure keeps the current snapshot and
//! is logged and reported by `/api/status`.
//!
//! Fetch policy: HTTPS only (plain HTTP only to a loopback address, and only
//! when explicitly allowed for local testing), redirects followed by hand and
//! only to the configured host or GitHub's release hosts, no credentials, a
//! timeout per request, and size caps on every body. The browser never talks
//! to these hosts; the site's CSP is unchanged.

use std::{collections::BTreeSet, net::IpAddr, sync::Arc, time::Duration};

use anyhow::{Context, bail, ensure};
use omg_models_catalog::{Severity, export, load_validated_files, time::Timestamp};
use omg_models_web::{
    AppState,
    state::{DataInfo, DataSource, LiveState, RefreshResult},
};
use ureq::http::Uri;

use crate::snapshot::{self, MANIFEST_LIMIT, Manifest};

pub const USER_AGENT: &str = concat!(
    "omg-models/",
    env!("CARGO_PKG_VERSION"),
    " (live data refresh; +https://github.com/ncecere/omg-models)"
);

/// Hosts a GitHub release download may redirect through.
pub const GITHUB_RELEASE_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// Default cap on the downloaded archive (compressed).
pub const DEFAULT_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Unpacked data may be at most this many times the archive cap.
const UNPACKED_FACTOR: u64 = 8;
const MAX_REDIRECTS: usize = 5;

/// How a refresher fetches.
#[derive(Clone, Debug)]
pub struct Config {
    /// URL of `catalog.manifest.json`; the archive is fetched from the same
    /// directory under the name the manifest gives.
    pub manifest_url: String,
    /// Cap on the archive download (bytes).
    pub max_bytes: u64,
    /// Timeout for each request (connect to last body byte).
    pub timeout: Duration,
    /// Accept `http://` for loopback addresses (local testing only).
    pub allow_http_loopback: bool,
}

impl Config {
    pub fn new(manifest_url: impl Into<String>) -> Self {
        Self {
            manifest_url: manifest_url.into(),
            max_bytes: DEFAULT_MAX_BYTES,
            timeout: Duration::from_secs(60),
            allow_http_loopback: false,
        }
    }
}

/// The result of one refresh attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Swapped in the snapshot from this commit.
    Updated { commit: String },
    /// 304, or the published data is what is being served.
    Unchanged,
}

/// A URL that passed the fetch policy.
#[derive(Clone, Debug)]
struct Target {
    uri: Uri,
}

impl Target {
    fn origin(&self) -> String {
        format!(
            "{}://{}",
            self.uri.scheme_str().unwrap_or("https"),
            self.uri.authority().map_or("", |a| a.as_str())
        )
    }
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Periodically refreshes a [`LiveState`] from a published snapshot.
pub struct Refresher {
    live: Arc<LiveState>,
    config: Config,
    agent: ureq::Agent,
    /// Hosts a request (or redirect) may go to.
    allowed_hosts: BTreeSet<String>,
    manifest: Target,
    /// ETag of the manifest behind the served snapshot (only kept after a
    /// successful check, so a failed attempt is retried in full).
    etag: Option<String>,
}

impl Refresher {
    /// Checks the configuration; fails on a URL the policy rejects.
    pub fn new(live: Arc<LiveState>, config: Config) -> anyhow::Result<Self> {
        let uri: Uri = config
            .manifest_url
            .parse()
            .with_context(|| format!("invalid data URL {:?}", config.manifest_url))?;
        let host = uri
            .host()
            .context("the data URL has no host")?
            .to_ascii_lowercase();
        let mut allowed_hosts: BTreeSet<String> = GITHUB_RELEASE_HOSTS
            .iter()
            .map(|h| (*h).to_owned())
            .collect();
        allowed_hosts.insert(host);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(config.timeout))
            .max_redirects(0)
            .http_status_as_error(false)
            .https_only(false) // enforced per URL by `check`
            .user_agent(USER_AGENT)
            .build()
            .into();
        let mut refresher = Self {
            live,
            config,
            agent,
            allowed_hosts,
            manifest: Target {
                uri: Uri::from_static("https://invalid/"),
            },
            etag: None,
        };
        refresher.manifest = refresher.check(uri)?;
        let path = refresher.manifest.uri.path();
        ensure!(
            !path.ends_with('/') && path.len() > 1,
            "the data URL must point at the manifest file ({})",
            snapshot::MANIFEST_NAME
        );
        Ok(refresher)
    }

    /// Applies the fetch policy to a URL.
    fn check(&self, uri: Uri) -> anyhow::Result<Target> {
        let host = uri
            .host()
            .with_context(|| format!("{uri}: no host"))?
            .to_ascii_lowercase();
        match uri.scheme_str() {
            Some("https") => {}
            Some("http") if self.config.allow_http_loopback && is_loopback(&host) => {}
            _ => bail!("{uri}: only https:// URLs are allowed"),
        }
        ensure!(
            self.allowed_hosts.contains(&host),
            "{uri}: host {host} is not allowed (allowed: {})",
            self.allowed_hosts
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
        ensure!(
            uri.authority().is_some_and(|a| !a.as_str().contains('@')),
            "{uri}: credentials in URLs are not allowed"
        );
        Ok(Target { uri })
    }

    /// GET with manual, policy-checked redirects. Returns (status, etag,
    /// body); the body is capped at `limit` bytes.
    fn get(
        &self,
        target: &Target,
        etag: Option<&str>,
        limit: u64,
    ) -> anyhow::Result<(u16, Option<String>, Vec<u8>)> {
        let mut current = target.clone();
        for _ in 0..=MAX_REDIRECTS {
            let mut request = self.agent.get(current.uri.clone());
            if let Some(etag) = etag {
                request = request.header("If-None-Match", etag);
            }
            let mut response = request
                .call()
                .with_context(|| format!("GET {}", current.uri))?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .with_context(|| format!("GET {}: redirect without Location", current.uri))?;
                let next = if location.starts_with('/') && !location.starts_with("//") {
                    format!("{}{location}", current.origin())
                } else {
                    location.to_owned()
                };
                let uri: Uri = next
                    .parse()
                    .with_context(|| format!("GET {}: bad redirect {location:?}", current.uri))?;
                current = self
                    .check(uri)
                    .with_context(|| format!("GET {}: redirect refused", target.uri))?;
                continue;
            }
            if let Some(length) = response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
            {
                ensure!(
                    length <= limit,
                    "GET {}: {length} bytes exceeds the limit of {limit}",
                    current.uri
                );
            }
            let etag = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let body = if status == 304 {
                Vec::new()
            } else {
                response
                    .body_mut()
                    .with_config()
                    .limit(limit)
                    .read_to_vec()
                    .with_context(|| {
                        format!(
                            "GET {}: reading the body (limit {limit} bytes)",
                            current.uri
                        )
                    })?
            };
            return Ok((status, etag, body));
        }
        bail!("GET {}: more than {MAX_REDIRECTS} redirects", target.uri)
    }

    /// The archive URL: the manifest URL's directory plus the archive name.
    fn archive_target(&self, manifest: &Manifest) -> anyhow::Result<Target> {
        let path = self.manifest.uri.path();
        let dir = &path[..=path.rfind('/').unwrap_or(0)];
        let url = format!("{}{dir}{}", self.manifest.origin(), manifest.archive.name);
        self.check(url.parse().context("archive URL")?)
    }

    /// One refresh attempt, without status bookkeeping.
    pub fn try_refresh(&mut self) -> anyhow::Result<Outcome> {
        let (status, etag, body) =
            self.get(&self.manifest, self.etag.as_deref(), MANIFEST_LIMIT)?;
        match status {
            304 if self.etag.is_some() => return Ok(Outcome::Unchanged),
            200 => {}
            other => bail!("GET {}: HTTP {other}", self.manifest.uri),
        }
        let manifest = snapshot::parse_manifest(&body).context("checking the manifest")?;
        let current = self.live.snapshot();
        if current.data.data_sha256.as_deref() == Some(manifest.data_sha256.as_str()) {
            // Same data. Adopt the published identity (commit, build time)
            // so the footer and status describe the published snapshot.
            if current.data.source == DataSource::Embedded
                || current.data.commit.as_deref() != Some(manifest.commit.as_str())
            {
                let data = DataInfo {
                    source: DataSource::Remote,
                    commit: Some(manifest.commit.clone()),
                    built_at: Some(manifest.built_at.clone()),
                    data_sha256: Some(manifest.data_sha256.clone()),
                };
                self.live
                    .swap(AppState::with_data(current.catalog.clone(), data));
            }
            self.etag = etag;
            return Ok(Outcome::Unchanged);
        }

        ensure!(
            manifest.archive.size <= self.config.max_bytes,
            "the archive is {} bytes, over the limit of {}",
            manifest.archive.size,
            self.config.max_bytes
        );
        let archive_target = self.archive_target(&manifest)?;
        let (status, _, archive) = self.get(&archive_target, None, self.config.max_bytes)?;
        ensure!(status == 200, "GET {}: HTTP {status}", archive_target.uri);
        let unpacked = snapshot::unpack(
            &manifest,
            &archive,
            self.config.max_bytes.saturating_mul(UNPACKED_FACTOR),
        )
        .context("verifying the archive")?;

        let (catalog, issues) = load_validated_files(&unpacked.data);
        let Some(catalog) = catalog else {
            let errors: Vec<String> = issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .map(ToString::to_string)
                .collect();
            bail!(
                "the published data does not validate ({} error(s)): {}",
                errors.len(),
                errors
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        };
        if export::build(&catalog) != unpacked.dist {
            // A different `omg-models` version built the snapshot. Serve this
            // server's own build so pages and API agree.
            eprintln!(
                "data refresh: the published dist/ differs from this server's build of the same data ({}); serving this server's build",
                manifest.generator
            );
        }
        let data = DataInfo {
            source: DataSource::Remote,
            commit: Some(manifest.commit.clone()),
            built_at: Some(manifest.built_at.clone()),
            data_sha256: Some(manifest.data_sha256.clone()),
        };
        self.live.swap(AppState::with_data(catalog, data));
        self.etag = etag;
        Ok(Outcome::Updated {
            commit: manifest.commit,
        })
    }

    /// One refresh attempt with logging and status bookkeeping. Never
    /// panics or changes the served data on failure.
    pub fn refresh(&mut self) -> anyhow::Result<Outcome> {
        let started = Timestamp::now().to_string();
        let result = self.try_refresh();
        self.live.update_refresh(|status| {
            status.last_attempt_at = Some(started.clone());
            match &result {
                Ok(outcome) => {
                    status.last_result = Some(match outcome {
                        Outcome::Updated { .. } => RefreshResult::Updated,
                        Outcome::Unchanged => RefreshResult::Unchanged,
                    });
                    status.last_error = None;
                    status.last_success_at = Some(started.clone());
                    status.consecutive_failures = 0;
                    if matches!(outcome, Outcome::Updated { .. }) {
                        status.last_update_at = Some(started.clone());
                    }
                }
                Err(error) => {
                    status.last_result = Some(RefreshResult::Failed);
                    status.last_error = Some(format!("{error:#}"));
                    status.consecutive_failures = status.consecutive_failures.saturating_add(1);
                }
            }
        });
        match &result {
            Ok(Outcome::Updated { commit }) => {
                eprintln!("data refresh: now serving the snapshot from commit {commit}");
            }
            Ok(Outcome::Unchanged) => {}
            Err(error) => {
                eprintln!("data refresh failed (still serving the current data): {error:#}");
            }
        }
        result
    }

    /// Runs forever on a background thread: one attempt now, then one per
    /// `interval`.
    pub fn spawn(mut self, interval: Duration) -> std::io::Result<std::thread::JoinHandle<()>> {
        let url = self.config.manifest_url.clone();
        self.live.update_refresh(|status| {
            status.enabled = true;
            status.url = Some(url);
            status.interval_seconds = Some(interval.as_secs());
        });
        std::thread::Builder::new()
            .name("data-refresh".into())
            .spawn(move || {
                loop {
                    let _ = self.refresh();
                    std::thread::sleep(interval);
                }
            })
    }
}

/// Parses `15m`, `1h`, `30s`, `1h30m` or plain seconds (`900`).
pub fn parse_interval(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    if let Ok(seconds) = text.parse::<u64>() {
        return Ok(Duration::from_secs(seconds));
    }
    let mut total: u64 = 0;
    let mut number = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            _ => return Err(format!("invalid duration {text:?} (use e.g. 15m, 1h, 900)")),
        };
        let value: u64 = number
            .parse()
            .map_err(|_| format!("invalid duration {text:?} (use e.g. 15m, 1h, 900)"))?;
        total = total.saturating_add(value.saturating_mul(unit));
        number.clear();
    }
    if !number.is_empty() || text.is_empty() {
        return Err(format!("invalid duration {text:?} (use e.g. 15m, 1h, 900)"));
    }
    Ok(Duration::from_secs(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals() {
        assert_eq!(parse_interval("15m"), Ok(Duration::from_secs(900)));
        assert_eq!(parse_interval("1h30m"), Ok(Duration::from_secs(5400)));
        assert_eq!(parse_interval("45"), Ok(Duration::from_secs(45)));
        assert!(parse_interval("15x").is_err());
        assert!(parse_interval("m").is_err());
        assert!(parse_interval("10m5").is_err());
    }

    #[test]
    fn loopback() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("[::1]"));
        assert!(is_loopback("localhost"));
        assert!(!is_loopback("example.com"));
        assert!(!is_loopback("10.0.0.1"));
    }
}
