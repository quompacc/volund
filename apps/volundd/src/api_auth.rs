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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutePolicy {
    Public,
    Authenticated,
    CatalogWrite,
    UserAdmin,
    SettingsAdmin,
    LibraryAdmin,
    MetadataAdmin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteRule {
    pub method: &'static str,
    pub path: &'static str,
    pub policy: RoutePolicy,
}

pub const ROUTE_POLICIES: &[RouteRule] = &[
    rule("GET", "/api/v1/health", RoutePolicy::Public),
    rule("GET", "/api/v1/setup", RoutePolicy::Public),
    rule("POST", "/api/v1/setup/owner", RoutePolicy::Public),
    rule("POST", "/api/v1/invitations/accept", RoutePolicy::Public),
    rule("POST", "/api/v1/sessions", RoutePolicy::Public),
    rule("GET", "/api/v1/slicer-download/{}", RoutePolicy::Public),
    rule("GET", "/api/v1/sessions", RoutePolicy::Authenticated),
    rule("DELETE", "/api/v1/sessions/{}", RoutePolicy::Authenticated),
    rule(
        "PUT",
        "/api/v1/account/password",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/session", RoutePolicy::Authenticated),
    rule("DELETE", "/api/v1/session", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/users", RoutePolicy::UserAdmin),
    rule("POST", "/api/v1/users", RoutePolicy::UserAdmin),
    rule("POST", "/api/v1/users/invitations", RoutePolicy::UserAdmin),
    rule("PATCH", "/api/v1/users/{}", RoutePolicy::UserAdmin),
    rule("POST", "/api/v1/users/{}/password", RoutePolicy::UserAdmin),
    rule("GET", "/api/v1/settings", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/preferences", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/preferences", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/operations/health",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "POST",
        "/api/v1/operations/support-bundle",
        RoutePolicy::LibraryAdmin,
    ),
    rule("GET", "/api/v1/jobs", RoutePolicy::LibraryAdmin),
    rule(
        "GET",
        "/api/v1/conversion-profiles",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "POST",
        "/api/v1/conversion-profiles",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "PUT",
        "/api/v1/conversion-profiles/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "DELETE",
        "/api/v1/conversion-profiles/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule("GET", "/api/v1/scan-schedules", RoutePolicy::LibraryAdmin),
    rule("POST", "/api/v1/scan-schedules", RoutePolicy::LibraryAdmin),
    rule(
        "PUT",
        "/api/v1/scan-schedules/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "DELETE",
        "/api/v1/scan-schedules/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "GET",
        "/api/v1/retention/preview",
        RoutePolicy::LibraryAdmin,
    ),
    rule("POST", "/api/v1/retention/runs", RoutePolicy::LibraryAdmin),
    rule("GET", "/api/v1/jobs/{}/{}", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/jobs/{}/{}/cancel",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "POST",
        "/api/v1/jobs/{}/{}/retry",
        RoutePolicy::LibraryAdmin,
    ),
    rule("PUT", "/api/v1/settings/{}", RoutePolicy::SettingsAdmin),
    rule("GET", "/api/v1/models", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/models/{}", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/models/{}/files", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/slicer-targets", RoutePolicy::Authenticated),
    rule(
        "PATCH",
        "/api/v1/models/{}/files/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/models/{}/files/{}/slicer-handoff",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/models/{}/history",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/history", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/models/{}/problems",
        RoutePolicy::Authenticated,
    ),
    rule(
        "PUT",
        "/api/v1/models/{}/problems",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/models/{}/thumbnail-candidates",
        RoutePolicy::Authenticated,
    ),
    rule(
        "PUT",
        "/api/v1/models/{}/thumbnail",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/models/{}/thumbnail/regenerate",
        RoutePolicy::CatalogWrite,
    ),
    rule("POST", "/api/v1/models", RoutePolicy::CatalogWrite),
    rule("PATCH", "/api/v1/models/{}", RoutePolicy::CatalogWrite),
    rule(
        "PUT",
        "/api/v1/models/{}/primary",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/lifecycle/preview",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/quarantines", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/lifecycle/plans/{}/apply",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/artifacts/{}/content",
        RoutePolicy::Authenticated,
    ),
    rule(
        "GET",
        "/api/v1/models/{}/components",
        RoutePolicy::Authenticated,
    ),
    rule(
        "POST",
        "/api/v1/models/{}/components",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "DELETE",
        "/api/v1/models/{}/components/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/collections", RoutePolicy::Authenticated),
    rule("POST", "/api/v1/collections", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/collections/{}", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/collections/{}", RoutePolicy::MetadataAdmin),
    rule(
        "DELETE",
        "/api/v1/collections/{}",
        RoutePolicy::MetadataAdmin,
    ),
    rule(
        "PUT",
        "/api/v1/collections/{}/models/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "DELETE",
        "/api/v1/collections/{}/models/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/tags", RoutePolicy::Authenticated),
    rule("POST", "/api/v1/tags", RoutePolicy::MetadataAdmin),
    rule("GET", "/api/v1/tags/{}", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/tags/{}", RoutePolicy::MetadataAdmin),
    rule("DELETE", "/api/v1/tags/{}", RoutePolicy::MetadataAdmin),
    rule("POST", "/api/v1/tags/{}/merge", RoutePolicy::MetadataAdmin),
    rule("GET", "/api/v1/authors", RoutePolicy::Authenticated),
    rule("POST", "/api/v1/authors", RoutePolicy::MetadataAdmin),
    rule("GET", "/api/v1/authors/{}", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/authors/{}", RoutePolicy::MetadataAdmin),
    rule(
        "POST",
        "/api/v1/authors/{}/merge",
        RoutePolicy::MetadataAdmin,
    ),
    rule("POST", "/api/v1/imports/preview", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/imports", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/imports/storage", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/imports/{}", RoutePolicy::CatalogWrite),
    rule(
        "GET",
        "/api/v1/imports/{}/manifest",
        RoutePolicy::CatalogWrite,
    ),
    rule("PATCH", "/api/v1/imports/{}", RoutePolicy::CatalogWrite),
    rule(
        "POST",
        "/api/v1/imports/{}/cancel",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/imports/latest-uploaded",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/metadata",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/review",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/commit",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/items/{}/content",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/items/{}/resolution",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/roots", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/roots/{}/files", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/roots/{}/folders",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/roots/{}/scans", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/libraries", RoutePolicy::LibraryAdmin),
    rule("POST", "/api/v1/libraries", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/libraries/validate",
        RoutePolicy::LibraryAdmin,
    ),
    rule("PATCH", "/api/v1/libraries/{}", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/libraries/{}/scans",
        RoutePolicy::LibraryAdmin,
    ),
    rule("GET", "/api/v1/content/{}", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/files/{}/previews",
        RoutePolicy::Authenticated,
    ),
    rule(
        "POST",
        "/api/v1/files/{}/previews",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/files/{}/content",
        RoutePolicy::Authenticated,
    ),
    rule("POST", "/api/v1/files/{}/move", RoutePolicy::CatalogWrite),
    rule(
        "GET",
        "/api/v1/previews/{}/artifacts/{}",
        RoutePolicy::Authenticated,
    ),
];

const fn rule(method: &'static str, path: &'static str, policy: RoutePolicy) -> RouteRule {
    RouteRule {
        method,
        path,
        policy,
    }
}

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
