//! Response headers for every request, including 404/405 misses: a strict
//! Content-Security-Policy (no scripts at all; styles, fonts and images from
//! this origin only), clickjacking and MIME-sniffing protection, and CORS
//! `*` for the read-only JSON API.

use topcoat::{
    context::Cx,
    router::{
        Body, HeaderValue, LayerFn, LayerFuture, Next, header, request,
        response::{IntoResponse, Response},
    },
};

/// The site needs no JavaScript and loads nothing from other origins.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; base-uri 'none'; connect-src 'self'; \
     font-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self'; \
     manifest-src 'self'; style-src 'self'";

/// Whether a request path is part of the JSON API.
pub fn is_api_path(path: &str) -> bool {
    path == "/api.json" || path.starts_with("/api/v1/")
}

fn set(response: &mut Response, name: header::HeaderName, value: &'static str) {
    response
        .headers_mut()
        .insert(name, HeaderValue::from_static(value));
}

/// Adds the security headers to `response` for a request to `path`.
pub fn apply(path: &str, response: &mut Response) {
    set(
        response,
        header::CONTENT_SECURITY_POLICY,
        CONTENT_SECURITY_POLICY,
    );
    set(response, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(response, header::X_FRAME_OPTIONS, "DENY");
    set(
        response,
        header::REFERRER_POLICY,
        "strict-origin-when-cross-origin",
    );
    set(
        response,
        header::HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=(), interest-cohort=()",
    );
    set(
        response,
        header::HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    );
    set(
        response,
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=63072000; includeSubDomains",
    );
    if is_api_path(path) {
        set(response, header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
        set(
            response,
            header::HeaderName::from_static("cross-origin-resource-policy"),
            "cross-origin",
        );
    } else {
        set(
            response,
            header::HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        );
    }
    if !response.headers().contains_key(header::CACHE_CONTROL) {
        // Pages reference content-hashed assets that a new deploy removes,
        // so browsers must revalidate HTML every time.
        set(response, header::CACHE_CONTROL, "no-cache");
    }
}

fn handle<'a>(cx: &'a Cx, body: Body, next: Next<'a>) -> LayerFuture<'a> {
    Box::pin(async move {
        let mut response = match next.run(cx, body).await {
            Ok(response) => response,
            // Misses (404/405) and handler errors: render them here so they
            // carry the same headers.
            Err(error) => error.into_response(cx)?,
        };
        if response.status().is_client_error() || response.status().is_server_error() {
            set(&mut response, header::CACHE_CONTROL, "no-store");
        }
        apply(request::uri(cx).path(), &mut response);
        Ok(response)
    })
}

/// A layer without a path: it wraps every request, misses included.
pub fn layer() -> LayerFn {
    LayerFn::new(None::<&'static str>, handle)
}
