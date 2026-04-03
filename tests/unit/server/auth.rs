use bendclaw::server::auth::extract_bearer;
use bendclaw::server::auth::extract_query_param;

#[test]
fn extract_bearer_valid() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        "Bearer mykey".parse().unwrap(),
    );
    assert_eq!(extract_bearer(&headers), Some("mykey".to_string()));
}

#[test]
fn extract_bearer_missing() {
    let headers = axum::http::HeaderMap::new();
    assert_eq!(extract_bearer(&headers), None);
}

#[test]
fn extract_bearer_wrong_scheme() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        "Basic abc".parse().unwrap(),
    );
    assert_eq!(extract_bearer(&headers), None);
}

#[test]
fn extract_query_param_present() {
    let uri: axum::http::Uri = "/health?api_key=secret123".parse().unwrap();
    assert_eq!(
        extract_query_param(&uri, "api_key"),
        Some("secret123".to_string())
    );
}

#[test]
fn extract_query_param_missing() {
    let uri: axum::http::Uri = "/health?other=val".parse().unwrap();
    assert_eq!(extract_query_param(&uri, "api_key"), None);
}

#[test]
fn extract_query_param_empty_value() {
    let uri: axum::http::Uri = "/health?api_key=".parse().unwrap();
    assert_eq!(extract_query_param(&uri, "api_key"), None);
}
