use super::error::HttpErrorKind;
use super::error::HttpTransportError;

pub fn is_retryable(err: &HttpTransportError) -> bool {
    err.retryable
}

pub fn is_retryable_kind(kind: HttpErrorKind) -> bool {
    matches!(
        kind,
        HttpErrorKind::DnsFailure
            | HttpErrorKind::TcpConnectFailure
            | HttpErrorKind::RequestTimeout
            | HttpErrorKind::ProxyInterrupted
            | HttpErrorKind::ConnectionInterrupted
    )
}
