//! HTTP tests through the real router (no network).
//!
//! Pages render bundled assets, so these tests need the Topcoat asset bundle
//! for this build: run `topcoat asset bundle --bin omg-models` first (CI
//! does), or point `OMG_MODELS_ASSETS` at a bundle directory. Without a
//! bundle the tests are skipped unless `OMG_MODELS_REQUIRE_ASSETS=1`.

use std::path::{Path, PathBuf};

use omg_models_catalog::{export, load_validated};
use omg_models_web::{
    AppState, AssetBundle, router,
    topcoat::router::{Body, Router, to_bytes},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn app() -> Option<Router> {
    let assets = std::env::var_os("OMG_MODELS_ASSETS")
        .map_or_else(|| root().join("target/debug/assets"), PathBuf::from);
    let bundle = match AssetBundle::load_dir(&assets) {
        Ok(bundle) => bundle,
        Err(error) => {
            assert!(
                std::env::var_os("OMG_MODELS_REQUIRE_ASSETS").is_none(),
                "no asset bundle at {}: {error}; run `topcoat asset bundle --bin omg-models`",
                assets.display()
            );
            eprintln!(
                "skipping: no asset bundle at {} ({error})",
                assets.display()
            );
            return None;
        }
    };
    let (catalog, _) = load_validated(&root().join("data"));
    Some(router(
        AppState::new(catalog.expect("seed data validates")),
        bundle,
    ))
}

struct Reply {
    status: u16,
    headers: http::HeaderMap,
    body: String,
}

async fn send(app: &Router, method: &str, uri: &str, headers: &[(&str, &str)]) -> Reply {
    let mut request = http::Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app.handle(request.body(Body::empty()).unwrap()).await;
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        headers: parts.headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

async fn get(app: &Router, uri: &str) -> Reply {
    send(app, "GET", uri, &[]).await
}

fn header<'a>(reply: &'a Reply, name: &str) -> &'a str {
    reply
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

fn assert_security_headers(reply: &Reply) {
    let csp = header(reply, "content-security-policy");
    assert!(csp.contains("default-src 'none'"), "CSP: {csp}");
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(!csp.contains("unsafe-inline"));
    assert_eq!(header(reply, "x-content-type-options"), "nosniff");
    assert_eq!(header(reply, "x-frame-options"), "DENY");
    assert!(!header(reply, "referrer-policy").is_empty());
    assert!(reply.headers.get("set-cookie").is_none(), "no cookies");
}

#[tokio::test]
async fn healthz() {
    let Some(app) = app() else { return };
    let reply = get(&app, "/healthz").await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "ok\n");
    assert_eq!(header(&reply, "cache-control"), "no-store");
}

#[tokio::test]
async fn pages_render_without_scripts_or_third_party_requests() {
    let Some(app) = app() else { return };
    for uri in [
        "/",
        "/?q=claude&provider=anthropic&max_input=1&sort=input&dir=desc",
        "/models/claude-sonnet-5-5",
        "/models/gpt-oss-120b",
        "/providers",
        "/providers/amazon-bedrock",
        "/compare?m1=claude-sonnet-5-5&m2=gpt-6.1-sol",
        "/api",
        "/about",
    ] {
        let reply = get(&app, uri).await;
        assert_eq!(reply.status, 200, "{uri}");
        assert_security_headers(&reply);
        assert_eq!(header(&reply, "cache-control"), "no-cache", "{uri}");
        assert!(!reply.body.contains("<script"), "{uri} has a script");
        assert!(
            !reply.body.contains(" style=\""),
            "{uri} has an inline style"
        );
        // Every subresource is same-origin.
        for attr in [
            "src=\"",
            "rel=\"stylesheet\" href=\"",
            "rel=\"preload\" href=\"",
        ] {
            for (index, _) in reply.body.match_indices(attr) {
                let rest = &reply.body[index + attr.len()..];
                assert!(
                    rest.starts_with('/'),
                    "{uri}: third-party {attr}{}",
                    &rest[..rest.len().min(60)]
                );
            }
        }
        assert!(reply.body.contains("Open Model Catalog"));
        assert!(reply.body.contains("lang=\"en\""));
    }
}

#[tokio::test]
async fn model_page_shows_exact_prices_and_provenance() {
    let Some(app) = app() else { return };
    let reply = get(&app, "/models/claude-haiku-5-5").await;
    assert_eq!(reply.status, 200);
    for needle in [
        "Claude Haiku 5.5",
        "$0.10",
        "$0.125",
        "Prompts over 100,000 tokens",
        "global.anthropic.claude-haiku-5-5",
        "https://platform.claude.com/docs/en/about-claude/pricing",
        "anthropic-version: 2023-06-01",
        "aws bedrock-runtime converse",
    ] {
        assert!(reply.body.contains(needle), "missing {needle:?}");
    }
}

#[tokio::test]
async fn filters_and_sorting() {
    let Some(app) = app() else { return };
    let reply = get(&app, "/?provider=openrouter").await;
    assert!(reply.body.contains("/providers/openrouter"));
    assert!(!reply.body.contains("href=\"/providers/anthropic\""));
    let reply = get(&app, "/?open_weights=1").await;
    assert!(reply.body.contains("gpt-oss-120b"));
    assert!(!reply.body.contains("Claude Sonnet 5.5"));
    let reply = get(&app, "/?max_input=0.05").await;
    assert!(reply.body.contains("gpt-oss-120b"));
    assert!(!reply.body.contains("Llama 4 Maverick"));
    let reply = get(&app, "/?max_input=abc").await;
    assert_eq!(reply.status, 200);
    assert!(reply.body.contains("must be a plain number"));
    let reply = get(&app, "/?sort=input&dir=desc").await;
    assert!(reply.body.contains("aria-sort=\"descending\""));
}

#[tokio::test]
async fn api_files_match_build_output_with_cache_and_cors_headers() {
    let Some(app) = app() else { return };
    let (catalog, _) = load_validated(&root().join("data"));
    let built = export::build(&catalog.unwrap());
    for (path, bytes) in &built {
        let reply = get(&app, &format!("/{path}")).await;
        assert_eq!(reply.status, 200, "{path}");
        assert_eq!(
            reply.body.as_bytes(),
            bytes.as_slice(),
            "{path} differs from build output"
        );
        assert_eq!(header(&reply, "access-control-allow-origin"), "*");
        assert!(header(&reply, "content-type").starts_with("application/json"));
        assert!(header(&reply, "cache-control").contains("max-age=300"));
        assert_security_headers(&reply);
    }
    let first = get(&app, "/api/v1/models.json").await;
    let etag = header(&first, "etag").to_owned();
    assert!(etag.starts_with("\"sha256-"));
    let again = send(
        &app,
        "GET",
        "/api/v1/models.json",
        &[("if-none-match", &etag)],
    )
    .await;
    assert_eq!(again.status, 304);
    assert!(again.body.is_empty());
    let preflight = send(
        &app,
        "OPTIONS",
        "/api/v1/models.json",
        &[("origin", "https://example.com")],
    )
    .await;
    assert_eq!(preflight.status, 204);
    assert!(header(&preflight, "access-control-allow-headers").contains("If-None-Match"));
}

#[tokio::test]
async fn not_found_responses() {
    let Some(app) = app() else { return };
    let reply = get(&app, "/api/v1/models/nope.json").await;
    assert_eq!(reply.status, 404);
    assert!(reply.body.contains("\"not_found\""));
    assert_eq!(header(&reply, "access-control-allow-origin"), "*");
    for uri in ["/nope", "/models/nope", "/providers/nope"] {
        let reply = get(&app, uri).await;
        assert_eq!(reply.status, 404, "{uri}");
        assert!(reply.body.contains("Page not found"), "{uri}");
        assert_security_headers(&reply);
        assert_eq!(header(&reply, "cache-control"), "no-store");
    }
    let reply = send(&app, "POST", "/", &[]).await;
    assert_eq!(reply.status, 405);
    assert_security_headers(&reply);
    let reply = get(&app, "/models").await;
    assert_eq!(reply.status, 308);
}
