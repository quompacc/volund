use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::api::ApiState;
use crate::api_error::ApiError;
use crate::api_models::{CurrentSessionResponse, LoginRequest};
use crate::session::AuthenticatedSession;
use crate::session::{self, LoginInput, SessionError};

const INSECURE_SESSION_COOKIE: &str = "volund_session";
const SECURE_SESSION_COOKIE: &str = "__Host-volund_session";
const INSECURE_CSRF_COOKIE: &str = "volund_csrf";
const SECURE_CSRF_COOKIE: &str = "__Host-volund_csrf";

pub(crate) async fn login(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let logged_in = session::login(
        &state.pool,
        LoginInput {
            email: request.email,
            password: request.password,
            client_address: None,
            user_agent: headers
                .get(header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        },
        crate::settings::session_policy(&state.pool)
            .await
            .map_err(ApiError::Settings)?,
    )
    .await
    .map_err(ApiError::Session)?;
    let secure = state.session.secure_cookies();
    let mut response = Json(&logged_in.session).into_response();
    append_cookie(
        response.headers_mut(),
        cookie(
            session_cookie_name(secure),
            &logged_in.session_token,
            true,
            secure,
            false,
        )?,
    );
    append_cookie(
        response.headers_mut(),
        cookie(
            csrf_cookie_name(secure),
            &logged_in.csrf_token,
            false,
            secure,
            false,
        )?,
    );
    Ok(response)
}

pub(crate) async fn current(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let actor = authenticate_headers(&state, &headers).await?;
    Ok(Json(current_response(actor)))
}

pub(crate) async fn list(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
) -> Result<impl IntoResponse, ApiError> {
    session::list_own(&state.pool, &actor)
        .await
        .map(Json)
        .map_err(ApiError::Session)
}

pub(crate) async fn revoke(
    State(state): State<ApiState>,
    Extension(actor): Extension<AuthenticatedSession>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    session::revoke_own(&state.pool, &actor, &session_id)
        .await
        .map_err(ApiError::Session)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn logout(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let actor = authenticate_headers(&state, &headers).await?;
    let csrf = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !actor.csrf_matches(csrf) {
        return Err(ApiError::Session(SessionError::InvalidCsrf));
    }
    session::revoke_current(&state.pool, &actor)
        .await
        .map_err(ApiError::Session)?;
    let secure = state.session.secure_cookies();
    let mut response = StatusCode::NO_CONTENT.into_response();
    append_cookie(
        response.headers_mut(),
        cookie(session_cookie_name(secure), "", true, secure, true)?,
    );
    append_cookie(
        response.headers_mut(),
        cookie(csrf_cookie_name(secure), "", false, secure, true)?,
    );
    Ok(response)
}

pub(crate) async fn authenticate_headers(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<session::AuthenticatedSession, ApiError> {
    let token = request_cookie(headers, session_cookie_name(state.session.secure_cookies()))
        .ok_or(ApiError::Session(SessionError::InvalidSession))?;
    let policy = crate::settings::session_policy(&state.pool)
        .await
        .map_err(ApiError::Settings)?;
    session::authenticate(&state.pool, token, policy)
        .await
        .map_err(ApiError::Session)
}

fn current_response(actor: session::AuthenticatedSession) -> CurrentSessionResponse {
    CurrentSessionResponse {
        session_id: actor.session_id,
        user_id: actor.user_id,
        email: actor.email,
        display_name: actor.display_name,
        role: actor.role,
        must_change_password: actor.must_change_password,
    }
}

fn request_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .filter_map(|part| part.split_once('='))
        .find_map(|(candidate, value)| (candidate == name).then_some(value))
}

fn cookie(
    name: &str,
    value: &str,
    http_only: bool,
    secure: bool,
    remove: bool,
) -> Result<HeaderValue, ApiError> {
    let mut value = format!("{name}={value}; Path=/; SameSite=Lax");
    if http_only {
        value.push_str("; HttpOnly");
    }
    if secure {
        value.push_str("; Secure");
    }
    if remove {
        value.push_str("; Max-Age=0");
    }
    HeaderValue::from_str(&value).map_err(|error| {
        ApiError::Session(SessionError::Unavailable(format!(
            "cannot build session cookie: {error}"
        )))
    })
}

fn append_cookie(headers: &mut HeaderMap, value: HeaderValue) {
    headers.append(header::SET_COOKIE, value);
}

fn session_cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_SESSION_COOKIE
    } else {
        INSECURE_SESSION_COOKIE
    }
}

fn csrf_cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_CSRF_COOKIE
    } else {
        INSECURE_CSRF_COOKIE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_attributes_and_deletion_preserve_the_security_contract() {
        for secure in [false, true] {
            for (name, http_only) in [
                (session_cookie_name(secure), true),
                (csrf_cookie_name(secure), false),
            ] {
                assert_eq!(name.starts_with("__Host-"), secure);
                for remove in [false, true] {
                    let value = cookie(
                        name,
                        if remove { "" } else { "test-token" },
                        http_only,
                        secure,
                        remove,
                    )
                    .unwrap_or_else(|_| panic!("valid cookie rejected"));
                    let value = value.to_str().unwrap();
                    assert!(value.contains("; Path=/; SameSite=Lax"));
                    assert!(!value.contains("Domain="));
                    assert_eq!(value.contains("; Secure"), secure);
                    assert_eq!(value.contains("; HttpOnly"), http_only);
                    assert_eq!(value.contains("; Max-Age=0"), remove);
                }
            }
        }
    }

    #[test]
    fn cookie_parser_uses_exact_names_and_keeps_secure_names_separate() {
        let mut headers = HeaderMap::new();
        assert_eq!(request_cookie(&headers, SECURE_SESSION_COOKIE), None);
        headers.insert(header::COOKIE, HeaderValue::from_static("other=x; volund_session_suffix=wrong; volund_session=plain; __Host-volund_session=secure"));
        assert_eq!(
            request_cookie(&headers, SECURE_SESSION_COOKIE),
            Some("secure")
        );
        assert_eq!(
            request_cookie(&headers, INSECURE_SESSION_COOKIE),
            Some("plain")
        );
        assert_eq!(request_cookie(&headers, SECURE_CSRF_COOKIE), None);
    }
}
