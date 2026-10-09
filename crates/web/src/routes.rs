//! Explicit-path routes: the static JSON API, `/healthz`, `/favicon.ico` and
//! `/robots.txt`. Registered by `.discover()`.

use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{Body, HeaderValue, StatusCode, header, request, response::Response, route},
};

use crate::state::AppState;

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
    let state = app_context::<AppState>(cx);
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

/// Liveness: the process is up and the catalog loaded (it is validated
/// before the server starts).
#[route(GET "/healthz")]
async fn healthz(cx: &Cx) -> Result<Response> {
    let state = app_context::<AppState>(cx);
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
