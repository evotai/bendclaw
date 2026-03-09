use axum::http::Request;
use bendclaw::service::context::RequestContext;

fn extract_ctx(
    user_id: Option<&str>,
    trace_id: Option<&str>,
    uri: &str,
) -> Result<RequestContext, axum::http::StatusCode> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut builder = Request::builder().uri(uri);
        if let Some(uid) = user_id {
            builder = builder.header("x-user-id", uid);
        }
        if let Some(tid) = trace_id {
            builder = builder.header("x-request-id", tid);
        }
        let req = builder.body(axum::body::Body::empty()).unwrap();
        let (mut parts, _body) = req.into_parts();
        use axum::extract::FromRequestParts;
        RequestContext::from_request_parts(&mut parts, &())
            .await
            .map_err(|rejection| {
                use axum::response::IntoResponse;
                rejection.into_response().status()
            })
    })
}

#[test]
fn context_extracts_user_and_trace() {
    let ctx = extract_ctx(Some("user-1"), Some("trace-abc"), "/").unwrap();
    assert_eq!(ctx.user_id, "user-1");
    assert_eq!(ctx.trace_id, "trace-abc");
}

#[test]
fn context_missing_user_header_returns_401() {
    let err = extract_ctx(None, None, "/").unwrap_err();
    assert_eq!(err, axum::http::StatusCode::UNAUTHORIZED);
}

#[test]
fn context_empty_user_header_returns_401() {
    let err = extract_ctx(Some(""), None, "/").unwrap_err();
    assert_eq!(err, axum::http::StatusCode::UNAUTHORIZED);
}

#[test]
fn context_trace_id_defaults_to_empty() {
    let ctx = extract_ctx(Some("u1"), None, "/").unwrap();
    assert_eq!(ctx.trace_id, "");
}

#[test]
fn context_user_id_from_query_param_fallback() {
    let ctx = extract_ctx(None, None, "/?x-user-id=query-user").unwrap();
    assert_eq!(ctx.user_id, "query-user");
}

#[test]
fn context_header_takes_precedence_over_query() {
    let ctx = extract_ctx(Some("header-user"), None, "/?x-user-id=query-user").unwrap();
    assert_eq!(ctx.user_id, "header-user");
}
