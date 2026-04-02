use super::context::HttpRequestContext;
use super::error::HttpTransportError;

pub(crate) fn log_transport_error(
    ctx: &HttpRequestContext,
    issue: &HttpTransportError,
    raw_error: &reqwest::Error,
) {
    crate::observability::log::slog!(
        error,
        "http",
        "transport_error",
        service = %ctx.service,
        operation = %ctx.operation,
        endpoint = %ctx.endpoint,
        model = %ctx.model.as_deref().unwrap_or(""),
        warehouse = %ctx.warehouse.as_deref().unwrap_or(""),
        url = %ctx.url,
        error_origin = %issue.kind.origin(),
        transport_error_kind = %issue.kind,
        transport_error_phase = %issue.phase,
        transport_retryable = issue.retryable,
        transport_error_host = %issue.host.as_deref().unwrap_or(""),
        transport_error_detail = %issue.detail,
        transport_error_chain = %issue.source_chain,
        raw_error = %raw_error,
    );
}
