//! Live data refresh against a local HTTP test server (no network): manifest
//! and archive download, ETag revalidation, checksum mismatches, invalid
//! data, size caps, the HTTPS-only rule and the redirect allowlist. Every
//! failure must leave the served snapshot untouched.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use omg_models_catalog::{DataFiles, load::read_files, load_validated_files, time::Timestamp};
use omg_models_cli::{
    live::{Config, Outcome, Refresher},
    snapshot::{self, Manifest},
};
use omg_models_web::{
    AppState,
    state::{DataInfo, DataSource, LiveState, RefreshResult},
};

const COMMIT: &str = "1111111111111111111111111111111111111111";
const COMMIT2: &str = "2222222222222222222222222222222222222222";

fn seed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn seed_files() -> DataFiles {
    read_files(&seed_dir()).unwrap()
}

fn built_at() -> Timestamp {
    Timestamp::parse("2026-10-09T12:20:00Z").unwrap()
}

/// The live state a server starts with: the seed data as the embedded
/// snapshot.
fn embedded() -> Arc<LiveState> {
    let files = seed_files();
    let (catalog, _) = load_validated_files(&files);
    Arc::new(LiveState::new(AppState::with_data(
        catalog.unwrap(),
        DataInfo {
            source: DataSource::Embedded,
            commit: None,
            built_at: None,
            data_sha256: Some(snapshot::data_digest(&files)),
        },
    )))
}

/// The seed data without one model: a valid, different snapshot.
fn changed_files() -> DataFiles {
    let mut files = seed_files();
    files.remove("providers/openrouter/models/gpt-oss-120b.toml");
    files
}

fn sealed(data: DataFiles, commit: &str) -> (Vec<u8>, Manifest) {
    snapshot::seal(&snapshot::collect(data).unwrap(), commit, &built_at()).unwrap()
}

/// Requests seen by the test server: (path, `If-None-Match`).
type RequestLog = Arc<Mutex<Vec<(String, Option<String>)>>>;

#[derive(Clone)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body,
        }
    }

    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            headers: vec![("Location".into(), location.into())],
            body: Vec::new(),
        }
    }
}

/// A minimal HTTP/1.1 server: one reply per path, `If-None-Match` honoured
/// for replies with an `ETag`, every request recorded.
struct TestServer {
    addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    /// (path, if-none-match)
    log: RequestLog,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let routes: Arc<Mutex<HashMap<String, Reply>>> = Arc::default();
        let log: RequestLog = Arc::default();
        let (r, l) = (Arc::clone(&routes), Arc::clone(&log));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (r, l) = (Arc::clone(&r), Arc::clone(&l));
                thread::spawn(move || handle(stream, &r, &l));
            }
        });
        Self { addr, routes, log }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn set(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.into(), reply);
    }

    fn publish(&self, archive: Vec<u8>, manifest: &Manifest, etag: &str) {
        self.set("/rel/catalog.tar.gz", Reply::ok(archive));
        self.set(
            "/rel/catalog.manifest.json",
            Reply::ok(snapshot::manifest_json(manifest)).header("ETag", etag),
        );
    }

    fn requests(&self) -> Vec<(String, Option<String>)> {
        self.log.lock().unwrap().clone()
    }
}

fn handle(
    mut stream: TcpStream,
    routes: &Mutex<HashMap<String, Reply>>,
    log: &Mutex<Vec<(String, Option<String>)>>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let mut if_none_match = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("if-none-match")
        {
            if_none_match = Some(value.trim().to_owned());
        }
    }
    log.lock()
        .unwrap()
        .push((path.clone(), if_none_match.clone()));
    let reply = routes.lock().unwrap().get(&path).cloned().unwrap_or(Reply {
        status: 404,
        headers: Vec::new(),
        body: b"not found".to_vec(),
    });
    let etag = reply
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("etag"))
        .map(|(_, v)| v.clone());
    let (status, body) = if reply.status == 200 && etag.is_some() && etag == if_none_match {
        (304, Vec::new())
    } else {
        (reply.status, reply.body)
    };
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in &reply.headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
    let _ = stream.read(&mut [0u8; 1]);
}

fn config(server: &TestServer) -> Config {
    let mut config = Config::new(server.url("/rel/catalog.manifest.json"));
    config.allow_http_loopback = true;
    config
}

fn refresher(live: &Arc<LiveState>, server: &TestServer) -> Refresher {
    Refresher::new(Arc::clone(live), config(server)).unwrap()
}

fn has_model(live: &LiveState, id: &str) -> bool {
    live.snapshot().catalog.models.contains_key(id)
}

#[test]
fn update_then_revalidate_with_etag() {
    let server = TestServer::start();
    let (archive, manifest) = sealed(changed_files(), COMMIT);
    server.publish(archive, &manifest, "\"v1\"");
    let live = embedded();
    let mut refresher = refresher(&live, &server);
    assert!(has_model(&live, "gpt-oss-120b"));

    let outcome = refresher.refresh().unwrap();
    assert_eq!(
        outcome,
        Outcome::Updated {
            commit: COMMIT.into()
        }
    );
    let snapshot = live.snapshot();
    assert!(!has_model(&live, "gpt-oss-120b"));
    assert_eq!(snapshot.data.source, DataSource::Remote);
    assert_eq!(snapshot.data.commit.as_deref(), Some(COMMIT));
    assert_eq!(
        snapshot.data.built_at.as_deref(),
        Some("2026-10-09T12:20:00Z")
    );
    assert!(
        !snapshot
            .files
            .contains_key("/api/v1/models/gpt-oss-120b.json")
    );
    let status = live.refresh_status();
    assert_eq!(status.last_result, Some(RefreshResult::Updated));
    assert!(status.last_update_at.is_some());

    // Second check: conditional request, 304, nothing downloaded.
    let before = server.requests().len();
    assert_eq!(refresher.refresh().unwrap(), Outcome::Unchanged);
    let requests = server.requests();
    assert_eq!(requests.len(), before + 1);
    assert_eq!(
        requests.last().unwrap(),
        &(
            "/rel/catalog.manifest.json".to_owned(),
            Some("\"v1\"".to_owned())
        )
    );
    assert!(Arc::ptr_eq(&snapshot, &live.snapshot()), "no swap on 304");
    assert_eq!(
        live.refresh_status().last_result,
        Some(RefreshResult::Unchanged)
    );
}

#[test]
fn same_data_is_not_downloaded() {
    let server = TestServer::start();
    let (archive, manifest) = sealed(seed_files(), COMMIT);
    server.publish(archive, &manifest, "\"same\"");
    let live = embedded();
    let mut refresher = refresher(&live, &server);
    assert_eq!(refresher.refresh().unwrap(), Outcome::Unchanged);
    assert!(
        !server
            .requests()
            .iter()
            .any(|(p, _)| p.ends_with(".tar.gz")),
        "the archive must not be fetched for identical data"
    );
    // The footer/status now name the published commit.
    let snapshot = live.snapshot();
    assert_eq!(snapshot.data.commit.as_deref(), Some(COMMIT));
    assert_eq!(snapshot.data.source, DataSource::Remote);
}

/// Asserts the refresh fails with `needle` and the embedded data is kept.
fn assert_rejected(live: &Arc<LiveState>, refresher: &mut Refresher, needle: &str) {
    let before = live.snapshot();
    let error = format!("{:#}", refresher.refresh().unwrap_err());
    assert!(error.contains(needle), "expected {needle:?} in: {error}");
    assert!(Arc::ptr_eq(&before, &live.snapshot()), "snapshot replaced");
    assert!(has_model(live, "gpt-oss-120b"));
    let status = live.refresh_status();
    assert_eq!(status.last_result, Some(RefreshResult::Failed));
    assert!(status.last_error.unwrap().contains(needle));
}

#[test]
fn archive_checksum_mismatch_is_rejected_then_recovers() {
    let server = TestServer::start();
    let (good_archive, manifest) = sealed(changed_files(), COMMIT);
    // Same length, different bytes.
    let mut tampered = good_archive.clone();
    let middle = tampered.len() / 2;
    tampered[middle] ^= 0xff;
    server.publish(tampered, &manifest, "\"v1\"");
    let live = embedded();
    let mut refresher = refresher(&live, &server);
    assert_rejected(&live, &mut refresher, "does not match the manifest");
    assert_rejected(&live, &mut refresher, "does not match the manifest");
    assert_eq!(live.refresh_status().consecutive_failures, 2);
    // A failed attempt does not keep the ETag: the next check is unconditional.
    assert!(server.requests().iter().all(|(_, inm)| inm.is_none()));

    // The publisher fixes the archive: the next attempt succeeds.
    server.publish(good_archive, &manifest, "\"v1\"");
    assert!(matches!(
        refresher.refresh().unwrap(),
        Outcome::Updated { .. }
    ));
    assert_eq!(live.refresh_status().consecutive_failures, 0);
    assert!(live.refresh_status().last_error.is_none());
}

#[test]
fn file_checksum_mismatch_is_rejected() {
    let server = TestServer::start();
    let (_, manifest) = sealed(changed_files(), COMMIT);
    // An archive whose own checksum the manifest vouches for, but with a
    // modified data file inside.
    let mut files = snapshot::collect(changed_files()).unwrap();
    let path = "data/providers/openai/provider.toml";
    let mut bytes = files[path].clone();
    bytes.extend_from_slice(b"\n# tampered\n");
    files.insert(path.into(), bytes);
    let tampered = snapshot::archive(&files).unwrap();
    let mut lying = manifest.clone();
    lying.archive.sha256 = snapshot::sha256_hex(&tampered);
    lying.archive.size = tampered.len() as u64;
    server.publish(tampered, &lying, "\"v1\"");
    let live = embedded();
    let mut refresher = refresher(&live, &server);
    assert_rejected(&live, &mut refresher, "data/providers/openai/provider.toml");
}

#[test]
fn inconsistent_or_unsupported_manifests_are_rejected() {
    let server = TestServer::start();
    let (archive, manifest) = sealed(changed_files(), COMMIT);
    let live = embedded();
    let mut refresher = refresher(&live, &server);

    let mut bad = manifest.clone();
    bad.total_sha256 = "0".repeat(64);
    server.publish(archive.clone(), &bad, "\"a\"");
    assert_rejected(&live, &mut refresher, "total_sha256 does not match");

    let mut bad = manifest.clone();
    bad.files[0].path = "data/../../etc/passwd".into();
    server.publish(archive.clone(), &bad, "\"b\"");
    assert_rejected(&live, &mut refresher, "invalid path");

    let mut value: serde_json::Value =
        serde_json::from_slice(&snapshot::manifest_json(&manifest)).unwrap();
    value["schema_version"] = 2.into();
    server.set(
        "/rel/catalog.manifest.json",
        Reply::ok(serde_json::to_vec(&value).unwrap()),
    );
    assert_rejected(&live, &mut refresher, "unsupported manifest schema_version");

    server.set("/rel/catalog.manifest.json", Reply::ok(b"<html>".to_vec()));
    assert_rejected(&live, &mut refresher, "manifest is not JSON");

    server.set(
        "/rel/catalog.manifest.json",
        Reply {
            status: 500,
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    assert_rejected(&live, &mut refresher, "HTTP 500");
}

#[test]
fn invalid_data_is_rejected_by_the_validator() {
    let server = TestServer::start();
    let mut data = changed_files();
    // Well-formed TOML that fails the checks: the id must equal the file name.
    let path = "providers/openai/models/gpt-6-luna.toml";
    let text = String::from_utf8(data[path].clone()).unwrap();
    data.insert(
        path.into(),
        text.replacen("id = \"gpt-6-luna\"", "id = \"not-luna\"", 1)
            .into_bytes(),
    );
    let mut files: BTreeMap<String, Vec<u8>> = data
        .into_iter()
        .map(|(p, b)| (format!("data/{p}"), b))
        .collect();
    files.insert("dist/api.json".into(), b"{}\n".to_vec());
    let (archive, manifest) = snapshot::seal(&files, COMMIT, &built_at()).unwrap();
    server.publish(archive, &manifest, "\"bad\"");
    let live = embedded();
    let mut refresher = refresher(&live, &server);
    assert_rejected(&live, &mut refresher, "does not validate");
}

#[test]
fn oversize_archives_are_rejected() {
    let server = TestServer::start();
    let (archive, manifest) = sealed(changed_files(), COMMIT);
    let live = embedded();

    // The manifest announces more than the cap: nothing is downloaded.
    server.publish(archive.clone(), &manifest, "\"v1\"");
    let mut small = config(&server);
    small.max_bytes = 512;
    let mut refresher = Refresher::new(Arc::clone(&live), small.clone()).unwrap();
    assert_rejected(&live, &mut refresher, "over the limit");
    assert!(
        !server
            .requests()
            .iter()
            .any(|(p, _)| p.ends_with(".tar.gz"))
    );

    // The manifest lies about the size: the body cap stops the download.
    let mut lying = manifest.clone();
    lying.archive.size = 100;
    server.publish(archive, &lying, "\"v2\"");
    let mut refresher = Refresher::new(Arc::clone(&live), small).unwrap();
    assert_rejected(&live, &mut refresher, "exceeds the limit");
}

#[test]
fn redirects_only_to_allowed_hosts() {
    let server = TestServer::start();
    let (archive, manifest) = sealed(changed_files(), COMMIT);
    server.publish(archive, &manifest, "\"v1\"");
    let live = embedded();

    // Same host: followed. The archive is fetched next to the configured
    // URL (GitHub redirects to signed, per-asset URLs).
    server.set(
        "/rel/latest.json",
        Reply::redirect("/rel/catalog.manifest.json"),
    );
    let mut config = config(&server);
    config.manifest_url = server.url("/rel/latest.json");
    let mut ok = Refresher::new(Arc::clone(&live), config.clone()).unwrap();

    // Another host (same machine, different name): refused.
    let port = server.addr.port();
    server.set(
        "/elsewhere.json",
        Reply::redirect(&format!(
            "http://localhost:{port}/rel/catalog.manifest.json"
        )),
    );
    config.manifest_url = server.url("/elsewhere.json");
    let mut refused = Refresher::new(Arc::clone(&live), config.clone()).unwrap();
    assert_rejected(&live, &mut refused, "host localhost is not allowed");

    // A non-GitHub public host: refused before any request is made to it.
    server.set(
        "/evil.json",
        Reply::redirect("https://evil.example/catalog.manifest.json"),
    );
    config.manifest_url = server.url("/evil.json");
    let mut evil = Refresher::new(Arc::clone(&live), config).unwrap();
    assert_rejected(&live, &mut evil, "host evil.example is not allowed");

    assert!(matches!(ok.refresh().unwrap(), Outcome::Updated { .. }));
}

#[test]
fn https_only() {
    let live = embedded();
    let error = Refresher::new(
        Arc::clone(&live),
        Config::new("http://127.0.0.1:9/catalog.manifest.json"),
    )
    .err()
    .unwrap();
    assert!(format!("{error:#}").contains("only https://"), "{error:#}");

    let mut config = Config::new("http://example.com/catalog.manifest.json");
    config.allow_http_loopback = true;
    let error = Refresher::new(Arc::clone(&live), config).err().unwrap();
    assert!(format!("{error:#}").contains("only https://"), "{error:#}");

    assert!(
        Refresher::new(
            live,
            Config::new("https://github.com/ncecere/omg-models/releases/download/data-latest/catalog.manifest.json"),
        )
        .is_ok()
    );
}

#[test]
fn packaging_is_reproducible_and_round_trips() {
    let (a, manifest) = snapshot::package(&seed_dir(), COMMIT, &built_at()).unwrap();
    let (b, again) = snapshot::package(&seed_dir(), COMMIT, &built_at()).unwrap();
    assert_eq!(a, b, "archive bytes are reproducible");
    assert_eq!(manifest, again);
    let parsed = snapshot::parse_manifest(&snapshot::manifest_json(&manifest)).unwrap();
    assert_eq!(parsed, manifest);
    let unpacked = snapshot::unpack(&parsed, &a, 64 * 1024 * 1024).unwrap();
    assert_eq!(unpacked.data, seed_files());
    assert!(unpacked.dist.contains_key("api/v1/omg-prices.json"));
    assert_eq!(manifest.data_sha256, snapshot::data_digest(&seed_files()));
    // A different commit changes the manifest but not the content digests.
    let (_, other) = snapshot::package(&seed_dir(), COMMIT2, &built_at()).unwrap();
    assert_eq!(other.total_sha256, manifest.total_sha256);
    // The unpacked size cap applies.
    let error = snapshot::unpack(&parsed, &a, 10).unwrap_err();
    assert!(format!("{error:#}").contains("unpacked"));
}
