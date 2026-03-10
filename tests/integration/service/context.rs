use anyhow::bail;
use anyhow::Result;
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
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    rt.block_on(async {
        let mut builder = Request::builder().uri(uri);
        if let Some(uid) = user_id {
            builder = builder.header("x-user-id", uid);
        }
        if let Some(tid) = trace_id {
            builder = builder.header("x-request-id", tid);
        }
        let req = builder
            .body(axum::body::Body::empty())
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
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
fn context_extracts_user_and_trace() -> Result<()> {
    let ctx = extract_ctx(Some("user-1"), Some("trace-abc"), "/")
        .map_err(|e| anyhow::anyhow!("status: {e:?}"))?;
    assert_eq!(ctx.user_id, "user-1");
    assert_eq!(ctx.trace_id, "trace-abc");
    Ok(())
}

#[test]
fn context_missing_user_header_returns_401() -> Result<()> {
    let Err(err) = extract_ctx(None, None, "/") else {
        bail!("expected 401 error");
    };
    assert_eq!(err, axum::http::StatusCode::UNAUTHORIZED);
    Ok(())
}

#[test]
fn context_empty_user_header_returns_401() -> Result<()> {
    let Err(err) = extract_ctx(Some(""), None, "/") else {
        bail!("expected 401 error");
    };
    assert_eq!(err, axum::http::StatusCode::UNAUTHORIZED);
    Ok(())
}

#[test]
fn context_trace_id_defaults_to_empty() -> Result<()> {
    let ctx = extract_ctx(Some("u1"), None, "/").map_err(|e| anyhow::anyhow!("status: {e:?}"))?;
    assert_eq!(ctx.trace_id, "");
    Ok(())
}

#[test]
fn context_user_id_from_query_param_fallback() -> Result<()> {
    let ctx = extract_ctx(None, None, "/?x-user-id=query-user")
        .map_err(|e| anyhow::anyhow!("status: {e:?}"))?;
    assert_eq!(ctx.user_id, "query-user");
    Ok(())
}

#[test]
fn context_header_takes_precedence_over_query() -> Result<()> {
    let ctx = extract_ctx(Some("header-user"), None, "/?x-user-id=query-user")
        .map_err(|e| anyhow::anyhow!("status: {e:?}"))?;
    assert_eq!(ctx.user_id, "header-user");
    Ok(())
}
