use super::*;
use axum::http::{HeaderMap, HeaderValue, Method, Uri, header};

const SECRET: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn configuration_is_disabled_without_url() {
    assert_eq!(
        StralarConfig::from_values("", SECRET),
        StralarConfig::Disabled
    );
    assert_eq!(
        StralarConfig::from_values("   ", ""),
        StralarConfig::Disabled
    );
}

#[test]
fn configuration_accepts_loopback_or_a_service_name_over_http_only() {
    assert_eq!(
        StralarConfig::from_values("http://127.0.0.1:5184", SECRET),
        StralarConfig::Enabled {
            authority: "127.0.0.1:5184".to_owned(),
            secret: SECRET.to_owned()
        }
    );
    assert_eq!(
        StralarConfig::from_values("http://stralar:5184", SECRET),
        StralarConfig::Enabled {
            authority: "stralar:5184".to_owned(),
            secret: SECRET.to_owned()
        }
    );
    for url in [
        "https://127.0.0.1:5184",
        "http://192.168.1.10:5184",
        "http://10.0.0.5:5184",
        "http://127.0.0.1",
        "http://127.0.0.1:0",
        "http://127.0.0.1:5184/path",
        "http://user@127.0.0.1:5184",
        "http://example.org:5184",
        "http://stralar.local:5184",
        "http://Stralar:5184",
        "http://1stralar:5184",
        "http://stralar-:5184",
        "http://stra_lar:5184",
        "http://[fe80::1]:5184",
    ] {
        assert!(
            matches!(
                StralarConfig::from_values(url, SECRET),
                StralarConfig::Misconfigured(_)
            ),
            "{url}"
        );
    }
}

#[test]
fn configuration_requires_a_long_secret() {
    assert!(matches!(
        StralarConfig::from_values("http://127.0.0.1:5184", "short"),
        StralarConfig::Misconfigured(_)
    ));
}

#[test]
fn internal_authority_keeps_host_and_port() {
    assert_eq!(
        internal_authority("http://localhost:8080/").as_deref(),
        Some("localhost:8080")
    );
    assert_eq!(
        internal_authority("http://[::1]:5184").as_deref(),
        Some("[::1]:5184")
    );
    assert_eq!(
        internal_authority("http://stralar-1:5184").as_deref(),
        Some("stralar-1:5184")
    );
    assert_eq!(internal_authority("http://127.0.0.1:99999"), None);
    assert_eq!(
        internal_authority(&format!("http://{}:5184", "a".repeat(64))),
        None
    );
}

#[test]
fn upstream_uri_strips_the_prefix_and_keeps_the_query() {
    let map = |uri: &str| {
        upstream_uri(&uri.parse::<Uri>().unwrap(), "127.0.0.1:5184").map(|u| u.to_string())
    };
    assert_eq!(map("/stralar/").as_deref(), Some("http://127.0.0.1:5184/"));
    assert_eq!(
        map("/stralar/api/recent/thumbnail?id=a1").as_deref(),
        Some("http://127.0.0.1:5184/api/recent/thumbnail?id=a1")
    );
    assert_eq!(map("/stralarx/api"), None);
    assert_eq!(map("/api/v1/health"), None);
}

#[test]
fn request_headers_lose_credentials_forgeries_and_hop_by_hop() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_static("volund_session=abc"),
    );
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer vlnd_x"),
    );
    headers.insert("x-csrf-token", HeaderValue::from_static("token"));
    headers.insert("x-stralar-user", HeaderValue::from_static("forged"));
    headers.insert("x-stralar-proxy-secret", HeaderValue::from_static("forged"));
    headers.insert(
        header::CONNECTION,
        HeaderValue::from_static("keep-alive, x-private"),
    );
    headers.insert("x-private", HeaderValue::from_static("1"));
    headers.insert(
        header::TRANSFER_ENCODING,
        HeaderValue::from_static("chunked"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    sanitize_request_headers(&mut headers);
    let names: Vec<&str> = headers.keys().map(HeaderName::as_str).collect();
    assert_eq!(names, vec!["content-type"]);
}

#[test]
fn upstream_request_sets_identity_and_host() {
    let request = Request::builder()
        .uri("/stralar/api/model")
        .header(header::HOST, "vault.example")
        .header(header::COOKIE, "volund_session=abc")
        .header("x-stralar-user", "forged")
        .body(Body::empty())
        .unwrap();
    let user = "0f8fad5b-d9cb-469f-a165-70867728950e";
    let Ok(upstream) = upstream_request(request, "127.0.0.1:5184", SECRET, user) else {
        panic!("upstream request could not be built");
    };
    assert_eq!(
        upstream.uri().to_string(),
        "http://127.0.0.1:5184/api/model"
    );
    assert_eq!(upstream.headers()[header::HOST], "127.0.0.1:5184");
    assert_eq!(upstream.headers()["x-stralar-user"], user);
    assert_eq!(upstream.headers()["x-stralar-proxy-secret"], SECRET);
    assert!(!upstream.headers().contains_key(header::COOKIE));
}

#[test]
fn mutations_need_a_present_same_origin() {
    let with = |origin: Option<&str>, host: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        if let Some(origin) = origin {
            headers.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
        }
        headers
    };
    assert!(same_origin(
        &with(Some("https://vault.example"), "vault.example"),
        true
    ));
    assert!(same_origin(
        &with(Some("http://127.0.0.1:8080"), "127.0.0.1:8080"),
        false
    ));
    assert!(!same_origin(&with(None, "vault.example"), true));
    assert!(!same_origin(
        &with(Some("https://evil.example"), "vault.example"),
        true
    ));
    assert!(!same_origin(
        &with(Some("http://vault.example"), "vault.example"),
        true
    ));
    assert!(is_mutation(&Method::POST));
    assert!(is_mutation(&Method::DELETE));
    assert!(!is_mutation(&Method::GET));
    assert!(!is_mutation(&Method::HEAD));
}

#[test]
fn only_a_valid_configuration_is_enabled() {
    assert!(StralarConfig::from_values("http://127.0.0.1:5184", SECRET).is_enabled());
    assert!(!StralarConfig::Disabled.is_enabled());
    assert!(!StralarConfig::from_values("http://10.0.0.1:5184", SECRET).is_enabled());
}
