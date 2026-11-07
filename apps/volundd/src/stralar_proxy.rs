//! Reverse proxy to the Stralar slicer service under `/stralar/` (ADR 0042).
//!
//! Stralar runs on the same host (loopback only) and trusts the identity VÖLUND hands it. This
//! module is the boundary: it authenticates the browser session, refuses bearer tokens (a
//! browser-only area, ADR 0037), requires a same-origin `Origin` on every mutation (Stralar's
//! browser code does not carry VÖLUND's CSRF token), removes VÖLUND's own credentials and any
//! client-supplied `x-stralar-*` header, and adds the account's public id plus a shared secret.
//! Bodies stream in both directions (uploads, live status, camera).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::api_session::authenticate_headers;
use crate::session::{Capability, SessionError};

const URL_ENV: &str = "VOLUND_STRALAR_URL";
const SECRET_FILE_ENV: &str = "VOLUND_STRALAR_SECRET_FILE";
const PREFIX: &str = "/stralar";
const USER_HEADER: &str = "x-stralar-user";
const SECRET_HEADER: &str = "x-stralar-proxy-secret";
const MIN_SECRET_LEN: usize = 32;

/// Operator configuration of the Stralar integration, read once at startup.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum StralarConfig {
    /// `VOLUND_STRALAR_URL` is not set: `/stralar/` does not exist (404).
    #[default]
    Disabled,
    /// Set but unusable: `/stralar/` answers 503 and the daemon logs the reason.
    Misconfigured(String),
    /// Loopback upstream (`host:port`) and the shared secret.
    Enabled { authority: String, secret: String },
}

impl StralarConfig {
    /// Read `VOLUND_STRALAR_URL` and the secret file named by `VOLUND_STRALAR_SECRET_FILE`.
    #[must_use]
    pub fn from_environment() -> Self {
        let url = std::env::var(URL_ENV).unwrap_or_default();
        let secret = std::env::var(SECRET_FILE_ENV)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        Self::from_values(&url, &secret)
    }

    /// Validate raw values: the upstream must be plain HTTP on a loopback address (Stralar never
    /// leaves the host) and the secret at least 32 characters long.
    #[must_use]
    pub fn from_values(url: &str, secret: &str) -> Self {
        let url = url.trim();
        if url.is_empty() {
            return Self::Disabled;
        }
        let Some(authority) = loopback_authority(url) else {
            return Self::Misconfigured(format!(
                "{URL_ENV} must be http://127.0.0.1:PORT, http://[::1]:PORT or http://localhost:PORT"
            ));
        };
        let secret = secret.trim();
        if secret.len() < MIN_SECRET_LEN {
            return Self::Misconfigured(format!(
                "{SECRET_FILE_ENV} must name a readable file with a secret of at least {MIN_SECRET_LEN} characters"
            ));
        }
        Self::Enabled {
            authority,
            secret: secret.to_owned(),
        }
    }
}

impl StralarConfig {
    /// True when `/stralar/` is usable (valid upstream and secret).
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled { .. })
    }
}

/// Response of `GET /api/v1/stralar`.
#[derive(Debug, serde::Serialize)]
pub(crate) struct StralarAvailability {
    available: bool,
}

/// `GET /api/v1/stralar`: whether the browser application may offer Stralar (navigation
/// entry, "In Stralar öffnen"). A misconfigured integration reports `false`.
pub(crate) async fn availability(State(state): State<ApiState>) -> Json<StralarAvailability> {
    Json(StralarAvailability {
        available: state.stralar.is_enabled(),
    })
}

/// `host:port` of a loopback `http://` URL without path, user info or query; else `None`.
pub(crate) fn loopback_authority(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?.trim_end_matches('/');
    if rest.contains(['/', '@', '?', '#']) {
        return None;
    }
    let (host, port) = rest.rsplit_once(':')?;
    if !matches!(host, "127.0.0.1" | "[::1]" | "localhost") {
        return None;
    }
    let port: u16 = port.parse().ok().filter(|port| *port != 0)?;
    Some(format!("{host}:{port}"))
}

#[derive(Clone)]
struct ProxyState {
    api: ApiState,
    proxy: Arc<Proxy>,
}

struct Proxy {
    config: StralarConfig,
    client: Client<HttpConnector, Body>,
}

/// Build the default API router with an explicit Stralar configuration (integration tests;
/// the daemon reads it from the environment in `api::router_with_security`).
pub fn router_with_config(pool: sqlx::PgPool, stralar: StralarConfig) -> Router {
    let state = (
        "/srv/volund/derived".into(),
        "/srv/volund/support".into(),
        crate::identity::IdentityConfig::default(),
        crate::session::SessionConfig::default(),
        crate::slicer_handoff::SlicerConfig::default(),
        stralar,
    );
    crate::api::router_with_state(pool, "/usr/share/volund/web".into(), state)
}

/// Routes `/stralar` and `/stralar/*` with the given configuration.
pub(crate) fn router_with(api: ApiState, config: StralarConfig) -> Router {
    if let StralarConfig::Misconfigured(reason) = &config {
        eprintln!("stralar integration disabled: {reason}");
    }
    let mut connector = HttpConnector::new();
    connector.set_connect_timeout(Some(Duration::from_secs(5)));
    let client = Client::builder(TokioExecutor::new()).build(connector);
    let state = ProxyState {
        api,
        proxy: Arc::new(Proxy { config, client }),
    };
    Router::new()
        .route(PREFIX, get(redirect_to_slash))
        .route("/stralar/", any(forward))
        .route("/stralar/{*rest}", any(forward))
        .with_state(state)
}

/// `/stralar` → `/stralar/`, so the Studio's relative URLs resolve below the prefix.
async fn redirect_to_slash(State(state): State<ProxyState>) -> Response {
    match state.proxy.config {
        StralarConfig::Disabled => not_found().into_response(),
        _ => Redirect::permanent("/stralar/").into_response(),
    }
}

async fn forward(State(state): State<ProxyState>, request: Request) -> Response {
    match forward_checked(&state, request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn forward_checked(state: &ProxyState, request: Request) -> Result<Response, ApiError> {
    let (authority, secret) = match &state.proxy.config {
        StralarConfig::Disabled => return Err(not_found()),
        StralarConfig::Misconfigured(_) => {
            return Err(ApiError::Unavailable(
                "stralar_misconfigured",
                "the Stralar integration is not fully set up on this server".to_owned(),
            ));
        }
        StralarConfig::Enabled { authority, secret } => (authority.clone(), secret.clone()),
    };
    // Browser-only area: a bearer token is never enough to reach Stralar (ADR 0037).
    if request.headers().contains_key(header::AUTHORIZATION) {
        return Err(ApiError::Forbidden);
    }
    let actor = authenticate_headers(&state.api, request.headers()).await?;
    if !actor.has_capability(Capability::CatalogRead) || actor.must_change_password {
        return Err(ApiError::Forbidden);
    }
    let secure = state.api.session.secure_cookies();
    if is_mutation(request.method()) && !same_origin(request.headers(), secure) {
        crate::security_audit::denied(
            &state.api.pool,
            &actor,
            "stralar.request",
            "http-route",
            "origin_mismatch",
        )
        .await;
        return Err(ApiError::Session(SessionError::InvalidCsrf));
    }
    let started = Instant::now();
    let request_id = request
        .extensions()
        .get::<crate::operational_log::RequestId>()
        .map(|value| value.0.clone());
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let upstream = upstream_request(request, &authority, &secret, &actor.user_id)?;
    let response = state
        .proxy
        .client
        .request(upstream)
        .await
        .map_err(|error| {
            eprintln!("stralar upstream request failed: {error}");
            ApiError::Unavailable(
                "stralar_unavailable",
                "the Stralar service is not reachable".to_owned(),
            )
        })?;
    let (mut parts, body) = response.into_parts();
    remove_hop_by_hop(&mut parts.headers);
    crate::operational_log::record_request(
        &state.api.pool,
        &actor,
        request_id.as_deref(),
        &method,
        &path,
        parts.status.as_u16(),
        started,
    )
    .await;
    Ok(Response::from_parts(parts, Body::new(body)))
}

/// The request as Stralar receives it: prefix removed, VÖLUND credentials and client-supplied
/// `x-stralar-*` headers dropped, identity and secret added.
pub(crate) fn upstream_request(
    request: Request,
    authority: &str,
    secret: &str,
    user_id: &str,
) -> Result<Request, ApiError> {
    let (mut parts, body) = request.into_parts();
    parts.uri = upstream_uri(&parts.uri, authority).ok_or_else(not_found)?;
    sanitize_request_headers(&mut parts.headers);
    let value = |text: &str| {
        HeaderValue::from_str(text).map_err(|_| ApiError::BadRequest("invalid header".to_owned()))
    };
    parts.headers.insert(header::HOST, value(authority)?);
    parts
        .headers
        .insert(HeaderName::from_static(USER_HEADER), value(user_id)?);
    parts
        .headers
        .insert(HeaderName::from_static(SECRET_HEADER), value(secret)?);
    Ok(Request::from_parts(parts, body))
}

/// `http://<authority>/<rest>?<query>` for `/stralar/<rest>?<query>`; `None` outside the prefix.
pub(crate) fn upstream_uri(uri: &Uri, authority: &str) -> Option<Uri> {
    let rest = uri.path().strip_prefix(PREFIX)?;
    if !rest.starts_with('/') {
        return None;
    }
    let path_and_query = match uri.query() {
        Some(query) => format!("{rest}?{query}"),
        None => rest.to_owned(),
    };
    format!("http://{authority}{path_and_query}").parse().ok()
}

/// Remove VÖLUND credentials, identity headers a client could forge, and hop-by-hop headers.
pub(crate) fn sanitize_request_headers(headers: &mut HeaderMap) {
    remove_hop_by_hop(headers);
    for name in [header::COOKIE, header::AUTHORIZATION] {
        headers.remove(name);
    }
    let forged: Vec<HeaderName> = headers
        .keys()
        .filter(|name| name.as_str().starts_with("x-stralar-") || name.as_str() == "x-csrf-token")
        .cloned()
        .collect();
    for name in forged {
        headers.remove(name);
    }
}

/// Drop RFC 9110 hop-by-hop headers, including any listed in `Connection`.
pub(crate) fn remove_hop_by_hop(headers: &mut HeaderMap) {
    let listed: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    for name in listed {
        headers.remove(name.as_str());
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

/// Strict same-origin check for mutations: `Origin` must be present and equal the request's
/// own scheme and host (VÖLUND's API also accepts a missing `Origin`; here it is required
/// because Stralar's requests carry no CSRF token).
pub(crate) fn same_origin(headers: &HeaderMap, secure: bool) -> bool {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    let scheme = if secure { "https" } else { "http" };
    matches!((origin, host), (Some(origin), Some(host)) if origin == format!("{scheme}://{host}"))
}

pub(crate) fn is_mutation(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn not_found() -> ApiError {
    ApiError::NotFound("not found".to_owned())
}

#[cfg(test)]
#[path = "stralar_proxy_tests.rs"]
mod tests;
