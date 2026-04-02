use serde::de::DeserializeOwned;

use super::context::HttpRequestContext;
use super::diagnostics;
use super::error::HttpTransportError;

pub async fn send(
    builder: reqwest::RequestBuilder,
    ctx: HttpRequestContext,
) -> Result<reqwest::Response, HttpTransportError> {
    builder.send().await.map_err(|error| {
        let issue = HttpTransportError::from_reqwest(&error);
        log_transport_error(&ctx, &issue, &error);
        issue
    })
}

pub async fn open_stream(
    builder: reqwest::RequestBuilder,
    ctx: HttpRequestContext,
) -> Result<reqwest::Response, HttpTransportError> {
    send(builder, ctx).await
}

pub fn stream_read_error(error: reqwest::Error, ctx: HttpRequestContext) -> HttpTransportError {
    let issue = HttpTransportError::from_reqwest(&error);
    log_transport_error(&ctx, &issue, &error);
    issue
}

pub async fn read_text(
    response: reqwest::Response,
    ctx: HttpRequestContext,
) -> Result<String, HttpTransportError> {
    response.text().await.map_err(|error| {
        let issue = HttpTransportError::from_reqwest(&error);
        log_transport_error(&ctx, &issue, &error);
        issue
    })
}

pub async fn read_json<T: DeserializeOwned>(
    response: reqwest::Response,
    ctx: HttpRequestContext,
) -> Result<T, HttpTransportError> {
    response.json::<T>().await.map_err(|error| {
        let issue = HttpTransportError::from_reqwest(&error);
        log_transport_error(&ctx, &issue, &error);
        issue
    })
}

fn log_transport_error(
    ctx: &HttpRequestContext,
    issue: &HttpTransportError,
    raw_error: &reqwest::Error,
) {
    diagnostics::log_transport_error(ctx, issue, raw_error);
}
