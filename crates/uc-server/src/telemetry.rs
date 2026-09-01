//! OpenTelemetry wiring.
//!
//! Traces are exported over OTLP to a collector. The whole stack is inert
//! unless `OTEL_EXPORTER_OTLP_ENDPOINT` is set: with no endpoint there is no
//! exporter, no batch processor and no span layer, so a deployment that wants
//! no telemetry pays nothing rather than paying for failing exports.
//!
//! Configured through the standard `OTEL_*` environment variables rather than
//! bespoke flags, so it behaves the way anyone who has deployed a collector
//! expects.

use anyhow::Context;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace::SdkTracerProvider, Resource};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Held so spans can be flushed at shutdown. Dropping it without calling
/// `shutdown` loses whatever the batch processor has not yet exported — which
/// is the last few seconds of traces, exactly the ones describing why the
/// process is going away.
pub(crate) struct Telemetry {
    provider: Option<SdkTracerProvider>,
}

impl Telemetry {
    /// Flush pending spans. Called on the shutdown path; safe to call when
    /// tracing was never enabled.
    pub(crate) fn shutdown(self) {
        if let Some(provider) = self.provider {
            if let Err(e) = provider.shutdown() {
                // Nothing useful to do but say so: the process is exiting.
                eprintln!("otel: failed to flush traces on shutdown: {e}");
            }
        }
    }
}

/// Install the tracing subscriber, with an OTLP layer when an endpoint is set.
pub(crate) fn init(log_level: &str) -> anyhow::Result<Telemetry> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!("uc_server={log_level},uc_api={log_level},uc_db={log_level}").into()
    });
    let stdout = tracing_subscriber::fmt::layer().with_ansi(false);

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").unwrap_or_default();
    if endpoint.is_empty() {
        tracing_subscriber::registry().with(filter).with(stdout).init();
        return Ok(Telemetry { provider: None });
    }

    let service_name =
        std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "uc-server".to_string());

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .context("building the OTLP span exporter")?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_attributes([KeyValue::new("service.name", service_name.clone())])
                .build(),
        )
        .build();

    let tracer = provider.tracer("uc-server");
    opentelemetry::global::set_tracer_provider(provider.clone());

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout)
        // The OTLP layer takes spans only; log events still go to stdout, which
        // is what the collector scrapes for logs.
        .with(tracing_opentelemetry::layer().with_tracer(tracer).with_filter(
            tracing_subscriber::filter::LevelFilter::INFO,
        ))
        .init();

    tracing::info!("OTLP traces -> {endpoint} (service.name={service_name})");
    Ok(Telemetry {
        provider: Some(provider),
    })
}
