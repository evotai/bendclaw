//! OTel tracer initialization and shutdown.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;

use super::config::TelemetryConfig;

/// Holds the initialized OTel tracer provider. Drop to flush and shutdown.
pub struct TelemetryExporter {
    provider: SdkTracerProvider,
}

impl TelemetryExporter {
    /// Initialize the OTLP exporter from config. Returns `None` if disabled or
    /// endpoint is not configured.
    pub fn init(config: &TelemetryConfig) -> Option<Self> {
        let endpoint = config.endpoint.as_deref()?;

        let exporter_builder = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint);

        let exporter = match exporter_builder.build() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "failed to build OTel exporter");
                return None;
            }
        };

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(build_resource())
            .build();

        // Set as global provider
        opentelemetry::global::set_tracer_provider(provider.clone());

        Some(Self { provider })
    }

    /// Get a tracer from this provider.
    pub fn tracer(&self) -> opentelemetry_sdk::trace::Tracer {
        self.provider.tracer("evot")
    }

    /// Graceful shutdown — flush pending spans with timeout.
    pub fn shutdown(&self) {
        if let Err(e) = self.provider.shutdown() {
            tracing::debug!(error = %e, "OTel provider shutdown error (non-fatal)");
        }
    }
}

impl Drop for TelemetryExporter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn build_resource() -> opentelemetry_sdk::Resource {
    use opentelemetry::KeyValue;
    opentelemetry_sdk::Resource::builder()
        .with_attributes([
            KeyValue::new("service.name", "evot"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build()
}
