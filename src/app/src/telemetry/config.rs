//! Telemetry configuration.
//!
//! Simplified: set `endpoint` to enable OTel export. Use `capture_content`
//! to include message and tool content in spans.

use serde::Deserialize;
use serde::Serialize;

/// Configuration for OTel telemetry export.
///
/// Telemetry is enabled when `endpoint` is set. No separate enable flag needed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// OTLP endpoint (e.g. "https://cloud.langfuse.com/api/public/otel/v1/traces").
    /// Setting this enables telemetry export.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Whether to capture content (messages, tool args, tool results).
    /// Default: false (privacy).
    #[serde(default)]
    pub capture_content: bool,
}

impl TelemetryConfig {
    /// Returns true if telemetry export is enabled (endpoint is configured).
    pub fn is_enabled(&self) -> bool {
        self.endpoint.is_some()
    }
}
