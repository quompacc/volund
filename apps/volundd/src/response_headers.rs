//! Default security headers of every VÖLUND response. Each is set only when the handler (or a
//! proxied service such as Stralar, ADR 0042) did not provide its own.

use axum::Router;
use axum::http::{HeaderName, HeaderValue};
use tower_http::set_header::SetResponseHeaderLayer;

/// Content-Security-Policy of the VÖLUND browser application.
pub(crate) const DEFAULT_CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'";

/// Add `nosniff`, frame denial and the default CSP to responses that lack them.
pub(crate) fn with_defaults(router: Router) -> Router {
    router
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(DEFAULT_CSP),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, header};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use tower::ServiceExt;

    async fn headers_of(router: Router, path: &str) -> axum::http::HeaderMap {
        let response = with_defaults(router)
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        response.headers().clone()
    }

    #[tokio::test]
    async fn adds_defaults_to_plain_responses() {
        let headers = headers_of(Router::new().route("/", get(|| async { "ok" })), "/").await;
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers["x-frame-options"], "DENY");
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], DEFAULT_CSP);
    }

    #[tokio::test]
    async fn keeps_a_policy_the_handler_set() {
        let own = || async {
            (
                [(header::CONTENT_SECURITY_POLICY, "default-src 'none'")],
                "x",
            )
                .into_response()
        };
        let headers = headers_of(Router::new().route("/own", get(own)), "/own").await;
        assert_eq!(
            headers[header::CONTENT_SECURITY_POLICY],
            "default-src 'none'"
        );
        assert_eq!(headers["x-frame-options"], "DENY");
    }
}
