use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::Response;
use std::time::Instant;

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::api_session::authenticate_headers;
use crate::session::{AuthenticatedSession, Capability, SessionError};

pub use crate::route_policy::{ROUTE_POLICIES, RoutePolicy, RouteRule};

/// Enforce the same browser-origin boundary before public mutations.
pub(crate) async fn require_public_origin(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if is_mutation(request.method()) {
        verify_browser_origin(request.headers(), state.session.secure_cookies())?;
    }
    Ok(next.run(request).await)
}

/// Authenticate every request in the protected router and verify mutation CSRF.
pub(crate) async fn require_session(
    State(state): State<ApiState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let actor = authenticate_headers(&state, request.headers()).await?;
    let started = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let request_id = request
        .extensions()
        .get::<crate::operational_log::RequestId>()
        .map(|value| value.0.clone());
    if !actor.has_capability(Capability::CatalogRead) {
        return Err(ApiError::Forbidden);
    }
    if actor.must_change_password
        && !allows_required_password_change(request.method(), request.uri().path())
    {
        return Err(ApiError::Forbidden);
    }
    if is_mutation(request.method()) {
        if let Err(error) = verify_browser_origin(request.headers(), state.session.secure_cookies())
        {
            crate::security_audit::denied(
                &state.pool,
                &actor,
                "request.mutation",
                "http-route",
                "origin_mismatch",
            )
            .await;
            return Err(error);
        }
        let csrf = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !actor.csrf_matches(csrf) {
            crate::security_audit::denied(
                &state.pool,
                &actor,
                "request.mutation",
                "http-route",
                "csrf_mismatch",
            )
            .await;
            return Err(ApiError::Session(SessionError::InvalidCsrf));
        }
    }
    request.extensions_mut().insert(actor.clone());
    let response = next.run(request).await;
    crate::operational_log::record_request(
        &state.pool,
        &actor,
        request_id.as_deref(),
        &method,
        &path,
        response.status().as_u16(),
        started,
    )
    .await;
    Ok(response)
}

fn verify_browser_origin(headers: &HeaderMap, secure: bool) -> Result<(), ApiError> {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let origin = origin
        .to_str()
        .map_err(|_| ApiError::Session(SessionError::InvalidCsrf))?;
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Session(SessionError::InvalidCsrf))?;
    let scheme = if secure { "https" } else { "http" };
    if origin == format!("{scheme}://{host}") {
        Ok(())
    } else {
        Err(ApiError::Session(SessionError::InvalidCsrf))
    }
}

pub(crate) fn require_catalog_write(actor: &AuthenticatedSession) -> Result<(), ApiError> {
    require_capability(actor, Capability::CatalogWrite)
}

pub(crate) fn require_user_admin(actor: &AuthenticatedSession) -> Result<(), ApiError> {
    require_capability(actor, Capability::UserAdmin)
}

pub(crate) fn require_settings_admin(actor: &AuthenticatedSession) -> Result<(), ApiError> {
    require_capability(actor, Capability::SettingsAdmin)
}

pub(crate) fn require_library_admin(actor: &AuthenticatedSession) -> Result<(), ApiError> {
    require_capability(actor, Capability::LibraryAdmin)
}

pub(crate) fn require_metadata_admin(actor: &AuthenticatedSession) -> Result<(), ApiError> {
    require_capability(actor, Capability::MetadataAdmin)
}

fn require_capability(
    actor: &AuthenticatedSession,
    capability: Capability,
) -> Result<(), ApiError> {
    if actor.has_capability(capability) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

fn is_mutation(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn allows_required_password_change(method: &Method, path: &str) -> bool {
    matches!(
        (method, path),
        (
            &Method::GET | &Method::DELETE,
            "/session" | "/api/v1/session"
        ) | (
            &Method::PUT,
            "/account/password" | "/api/v1/account/password"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ROUTE_POLICIES, RoutePolicy, allows_required_password_change, is_mutation,
        verify_browser_origin,
    };
    use axum::http::{HeaderMap, HeaderValue, Method, header};

    #[test]
    fn only_safe_methods_skip_csrf_verification() {
        assert!(!is_mutation(&Method::GET));
        assert!(!is_mutation(&Method::HEAD));
        assert!(!is_mutation(&Method::OPTIONS));
        assert!(is_mutation(&Method::POST));
        assert!(is_mutation(&Method::DELETE));
        assert!(is_mutation(&Method::PATCH));
    }

    #[test]
    fn phase_three_routes_follow_the_catalog_role_matrix() {
        let policy = |method, path| {
            ROUTE_POLICIES
                .iter()
                .find(|rule| rule.method == method && rule.path == path)
                .map(|rule| rule.policy)
        };
        assert_eq!(
            policy("GET", "/api/v1/models/{}/history"),
            Some(RoutePolicy::Authenticated)
        );
        assert_eq!(
            policy("GET", "/api/v1/models/{}/export"),
            Some(RoutePolicy::Authenticated)
        );
        assert_eq!(
            policy("POST", "/api/v1/exports/metadata"),
            Some(RoutePolicy::LibraryAdmin)
        );
        assert_eq!(
            policy("PUT", "/api/v1/models/{}/thumbnail"),
            Some(RoutePolicy::CatalogWrite)
        );
        assert_eq!(
            policy("PUT", "/api/v1/collections/{}/models/{}"),
            Some(RoutePolicy::CatalogWrite)
        );
        assert_eq!(
            policy("POST", "/api/v1/tags/{}/merge"),
            Some(RoutePolicy::MetadataAdmin)
        );
        assert_eq!(
            policy("POST", "/api/v1/authors/{}/merge"),
            Some(RoutePolicy::MetadataAdmin)
        );
    }

    #[test]
    fn browser_origin_must_match_the_effective_host_and_cookie_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("vault.example:8080"));
        assert!(verify_browser_origin(&headers, false).is_ok());

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://vault.example:8080"),
        );
        assert!(verify_browser_origin(&headers, false).is_ok());
        assert!(verify_browser_origin(&headers, true).is_err());

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://attacker.example"),
        );
        assert!(verify_browser_origin(&headers, false).is_err());
    }

    #[test]
    fn temporary_credentials_only_allow_session_and_password_routes() {
        assert!(allows_required_password_change(&Method::GET, "/session"));
        assert!(allows_required_password_change(
            &Method::DELETE,
            "/api/v1/session"
        ));
        assert!(allows_required_password_change(
            &Method::PUT,
            "/account/password"
        ));
        assert!(!allows_required_password_change(
            &Method::GET,
            "/api/v1/models"
        ));
        assert!(!allows_required_password_change(
            &Method::GET,
            "/api/v1/settings"
        ));
    }
}
