use std::fmt;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorOrigin {
    Client,
    Server,
    Network,
}

impl ErrorOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
            Self::Network => "network",
        }
    }

    pub fn from_status_code(code: u16) -> Self {
        match code {
            400..=499 => Self::Client,
            500..=599 => Self::Server,
            _ => Self::Network,
        }
    }
}

impl fmt::Display for ErrorOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpErrorKind {
    DnsFailure,
    TcpConnectFailure,
    TlsHandshakeFailure,
    RequestTimeout,
    ProxyInterrupted,
    ConnectionInterrupted,
    InvalidRequest,
    InvalidResponse,
    Unknown,
}

impl HttpErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DnsFailure => "dns_failure",
            Self::TcpConnectFailure => "tcp_connect_failure",
            Self::TlsHandshakeFailure => "tls_handshake_failure",
            Self::RequestTimeout => "request_timeout",
            Self::ProxyInterrupted => "proxy_interrupted",
            Self::ConnectionInterrupted => "connection_interrupted",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidResponse => "invalid_response",
            Self::Unknown => "unknown",
        }
    }

    pub fn origin(self) -> ErrorOrigin {
        match self {
            Self::InvalidRequest => ErrorOrigin::Client,
            Self::DnsFailure
            | Self::TcpConnectFailure
            | Self::TlsHandshakeFailure
            | Self::ProxyInterrupted
            | Self::RequestTimeout => ErrorOrigin::Network,
            Self::ConnectionInterrupted | Self::InvalidResponse => ErrorOrigin::Server,
            Self::Unknown => ErrorOrigin::Network,
        }
    }
}

impl fmt::Display for HttpErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpErrorPhase {
    BuildRequest,
    Connect,
    Send,
    ReadBody,
    DecodeBody,
    Unknown,
}

impl HttpErrorPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuildRequest => "build_request",
            Self::Connect => "connect",
            Self::Send => "send",
            Self::ReadBody => "read_body",
            Self::DecodeBody => "decode_body",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for HttpErrorPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpTransportError {
    pub kind: HttpErrorKind,
    pub phase: HttpErrorPhase,
    pub retryable: bool,
    pub host: Option<String>,
    pub detail: String,
    pub source_chain: String,
}

impl HttpTransportError {
    pub fn from_reqwest(error: &reqwest::Error) -> Self {
        let phase = classify_phase(error);
        let source_chain = source_chain(error);
        let lower = source_chain.to_lowercase();
        let kind = classify_kind(error, phase, &lower);
        let host = error
            .url()
            .and_then(|url| url.host_str())
            .map(str::to_string);
        let detail = source_chain
            .rsplit(" <- ")
            .find(|part| !part.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());

        Self {
            kind,
            phase,
            retryable: super::retry::is_retryable_kind(kind),
            host,
            detail,
            source_chain,
        }
    }

    pub fn summary(&self) -> String {
        match self.host.as_deref() {
            Some(host) if !host.is_empty() => {
                format!(
                    "{} during {} to {}: {}",
                    self.kind, self.phase, host, self.detail
                )
            }
            _ => format!("{} during {}: {}", self.kind, self.phase, self.detail),
        }
    }
}

impl fmt::Display for HttpTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.summary())
    }
}

fn classify_phase(error: &reqwest::Error) -> HttpErrorPhase {
    if error.is_connect() {
        HttpErrorPhase::Connect
    } else if error.is_body() {
        HttpErrorPhase::ReadBody
    } else if error.is_decode() {
        HttpErrorPhase::DecodeBody
    } else if error.is_request() {
        HttpErrorPhase::Send
    } else {
        HttpErrorPhase::Unknown
    }
}

fn classify_kind(error: &reqwest::Error, phase: HttpErrorPhase, lower: &str) -> HttpErrorKind {
    if error.is_timeout() || contains_any(lower, &["timed out", "deadline has elapsed"]) {
        return HttpErrorKind::RequestTimeout;
    }
    if contains_any(lower, &[
        "builder error",
        "relative URL without a base",
        "url error",
        "builder:",
    ]) {
        return HttpErrorKind::InvalidRequest;
    }
    if contains_any(lower, &[
        "dns error",
        "failed to lookup address information",
        "temporary failure in name resolution",
        "name or service not known",
        "no such host",
        "failed to resolve host",
        "dns lookup failed",
        "nodename nor servname provided",
        "no address associated with hostname",
    ]) {
        return HttpErrorKind::DnsFailure;
    }
    if contains_any(lower, &[
        "tls",
        "rustls",
        "certificate",
        "invalid peer certificate",
        "unknown issuer",
        "handshake failure",
        "tls handshake",
        "peer sent no certificates",
        "cert verify failed",
    ]) {
        return HttpErrorKind::TlsHandshakeFailure;
    }
    if contains_any(lower, &[
        "proxy",
        "tunnel",
        "http connect",
        "unsuccessful tunnel",
        "proxy connect",
    ]) {
        return HttpErrorKind::ProxyInterrupted;
    }
    if phase == HttpErrorPhase::ReadBody
        && contains_any(lower, &[
            "connection reset by peer",
            "broken pipe",
            "unexpected eof",
            "unexpected end of file",
            "peer closed connection",
            "connection closed before message completed",
            "connection terminated",
            "incomplete message",
        ])
    {
        return HttpErrorKind::ConnectionInterrupted;
    }
    if error.is_connect()
        || contains_any(lower, &[
            "tcp connect error",
            "connect error",
            "connection refused",
            "network is unreachable",
            "no route to host",
            "connection aborted",
            "host is down",
        ])
    {
        return HttpErrorKind::TcpConnectFailure;
    }
    if phase == HttpErrorPhase::DecodeBody {
        return HttpErrorKind::InvalidResponse;
    }
    HttpErrorKind::Unknown
}

fn source_chain(error: &reqwest::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut current: &dyn std::error::Error = error;
    while let Some(source) = current.source() {
        let text = source.to_string();
        if parts.last() != Some(&text) {
            parts.push(text);
        }
        current = source;
    }
    parts.join(" <- ")
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| text.contains(pattern))
}
