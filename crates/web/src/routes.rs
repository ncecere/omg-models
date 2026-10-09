//! Explicit-path routes: the static JSON API, `/healthz`, `/favicon.ico` and
//! `/robots.txt`. Registered by `.discover()`.

use topcoat::{
    Result,
    context::Cx,
    router::{Body, HeaderValue, StatusCode, header, request, response::Response, route},
};

use crate::state;

/// Browsers may cache API files for five minutes and use a stale copy for an
/// hour while revalidating with the ETag.
const API_CACHE_CONTROL: &str = "public, max-age=300, stale-while-revalidate=3600";

fn response(status: StatusCode, content_type: &'static str, body: impl Into<Body>) -> Response {
    let mut response = Response::new(body.into());
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn if_none_match(cx: &Cx, etag: &str) -> bool {
    request::headers(cx)
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.trim_start_matches("W/") == etag)
}

/// Serves a pre-built JSON file with caching validators; unknown paths get a
/// JSON 404 (never the HTML not-found page).
fn serve_file(cx: &Cx, path: &str) -> Response {
    let state = state::current(cx);
    let Some(file) = state.files.get(path) else {
        let mut response = response(
            StatusCode::NOT_FOUND,
            "application/json; charset=utf-8",
            format!(
                "{{\"error\":\"not_found\",\"message\":\"No API file at this path. See https://models.omg.bitop.dev/api for the endpoint list.\",\"path\":{}}}\n",
                serde_json::Value::String(path.to_owned())
            ),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    };
    let mut response = if if_none_match(cx, &file.etag) {
        response(
            StatusCode::NOT_MODIFIED,
            "application/json; charset=utf-8",
            Body::empty(),
        )
    } else {
        response(
            StatusCode::OK,
            "application/json; charset=utf-8",
            file.bytes.clone(),
        )
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(API_CACHE_CONTROL),
    );
    if let Ok(etag) = HeaderValue::from_str(&file.etag) {
        headers.insert(header::ETAG, etag);
    }
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("ETag"),
    );
    response
}

/// CORS preflight (needed when a browser client sends `If-None-Match`).
fn preflight() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("If-None-Match, If-Modified-Since"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    response
}

#[route(GET "/api.json")]
async fn api_json(cx: &Cx) -> Result<Response> {
    Ok(serve_file(cx, "/api.json"))
}

#[route(OPTIONS "/api.json")]
async fn api_json_preflight() -> Result<Response> {
    Ok(preflight())
}

#[route(GET "/api/v1/{*path}")]
async fn api_v1(cx: &Cx) -> Result<Response> {
    let path = request::uri(cx).path().to_owned();
    Ok(serve_file(cx, &path))
}

#[route(OPTIONS "/api/v1/{*path}")]
async fn api_v1_preflight() -> Result<Response> {
    Ok(preflight())
}

/// Readiness: a validated catalog is being served. The embedded snapshot is
/// validated before the server starts and a refresh only ever swaps in a
/// validated snapshot, so a failed refresh does not make the server unready
/// (it keeps serving the last good data; see `/api/status`).
#[route(GET "/readyz")]
async fn readyz(cx: &Cx) -> Result<Response> {
    let state = state::current(cx);
    let (status, body) = if state.catalog.models.is_empty() {
        (StatusCode::SERVICE_UNAVAILABLE, "empty\n")
    } else {
        (StatusCode::OK, "ready\n")
    };
    let mut response = response(status, "text/plain; charset=utf-8", body);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// The served data snapshot and the live-refresh status.
#[route(GET "/api/status")]
async fn api_status(cx: &Cx) -> Result<Response> {
    let snapshot = state::current(cx);
    let refresh = state::live(cx).refresh_status();
    let body = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "data": {
            "source": snapshot.data.source,
            "commit": snapshot.data.commit,
            "built_at": snapshot.data.built_at,
            "data_sha256": snapshot.data.data_sha256,
            "last_updated": snapshot.last_updated,
            "providers": snapshot.catalog.providers.len(),
            "models": snapshot.catalog.models.len(),
        },
        "refresh": refresh,
    });
    let mut bytes = serde_json::to_vec_pretty(&body).unwrap_or_default();
    bytes.push(b'\n');
    let mut response = response(StatusCode::OK, "application/json; charset=utf-8", bytes);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Liveness: the process is up and serving a catalog (it is validated
/// before the server starts).
#[route(GET "/healthz")]
async fn healthz(cx: &Cx) -> Result<Response> {
    let state = state::current(cx);
    let body = if state.catalog.models.is_empty() {
        "empty\n"
    } else {
        "ok\n"
    };
    let mut response = response(StatusCode::OK, "text/plain; charset=utf-8", body);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[route(GET "/favicon.ico")]
async fn favicon() -> Result<Response> {
    let mut response = response(
        StatusCode::OK,
        "image/x-icon",
        include_bytes!("../assets/brand/favicon.ico").as_slice(),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    Ok(response)
}

#[route(GET "/robots.txt")]
async fn robots() -> Result<Response> {
    Ok(response(
        StatusCode::OK,
        "text/plain; charset=utf-8",
        "User-agent: *\nAllow: /\n",
    ))
}
