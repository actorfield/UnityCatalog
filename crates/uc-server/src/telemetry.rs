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
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider, Resource,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Held so spans can be flushed at shutdown. Dropping it without calling
/// `shutdown` loses whatever the batch processor has not yet exported — which
/// is the last few seconds of traces, exactly the ones describing why the
/// process is going away.
pub(crate) struct Telemetry {
    traces: Option<SdkTracerProvider>,
    logs: Option<SdkLoggerProvider>,
    metrics: Option<SdkMeterProvider>,
}

impl Telemetry {
    /// Flush pending spans. Called on the shutdown path; safe to call when
    /// tracing was never enabled.
    pub(crate) fn shutdown(self) {
        // Each is flushed independently: a failure in one signal must not skip
        // the others, and by this point stderr is the only place left to report.
        if let Some(p) = self.traces {
            if let Err(e) = p.shutdown() {
                eprintln!("otel: failed to flush traces on shutdown: {e}");
            }
        }
        if let Some(p) = self.logs {
            if let Err(e) = p.shutdown() {
                eprintln!("otel: failed to flush logs on shutdown: {e}");
            }
        }
        if let Some(p) = self.metrics {
            if let Err(e) = p.shutdown() {
                eprintln!("otel: failed to flush metrics on shutdown: {e}");
            }
        }
    }
}

/// Install the tracing subscriber, with an OTLP layer when an endpoint is set.
pub(crate) fn init(log_level: &str) -> anyhow::Result<Telemetry> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // tower_http included on purpose: the per-request response event is
        // emitted under that target, and it is the only log that carries a
        // trace id. Leaving it out silently drops request logging.
        format!(
            "uc_server={log_level},uc_api={log_level},uc_db={log_level},\
             tower_http=info"
        )
        .into()
    });
    let stdout = tracing_subscriber::fmt::layer().with_ansi(false);

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").unwrap_or_default();
    if endpoint.is_empty() {
        tracing_subscriber::registry()
            .with(filter)
            .with(stdout)
            .init();
        return Ok(Telemetry {
            traces: None,
            logs: None,
            metrics: None,
        });
    }

    let service_name =
        std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "uc-server".to_string());
    let resource = Resource::builder()
        .with_attributes([KeyValue::new("service.name", service_name.clone())])
        .build();

    // ── traces ────────────────────────────────────────────────────────────────
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .context("building the OTLP span exporter")?;
    let traces = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();
    let tracer = traces.tracer("uc-server");
    opentelemetry::global::set_tracer_provider(traces.clone());

    // ── logs ──────────────────────────────────────────────────────────────────
    // Exported over OTLP rather than left for the collector to scrape off
    // stdout, because the appender attaches the active trace and span ids. That
    // is the whole point of emitting both signals: a log line you cannot pivot
    // to its trace is not much better than a log line on its own.
    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .context("building the OTLP log exporter")?;
    let logs = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource.clone())
        .build();

    // ── metrics ───────────────────────────────────────────────────────────────
    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .context("building the OTLP metric exporter")?;
    let metrics = SdkMeterProvider::builder()
        .with_periodic_exporter(metric_exporter)
        .with_resource(resource)
        .build();
    opentelemetry::global::set_meter_provider(metrics.clone());

    tracing_subscriber::registry()
        .with(filter)
        // stdout stays: it is what you read when something is wrong with the
        // collector itself.
        .with(stdout)
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(tracer)
                .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
        )
        .with(
            opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logs)
                .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
        )
        .init();

    tracing::info!("OTLP traces, logs and metrics -> {endpoint} (service.name={service_name})");
    Ok(Telemetry {
        traces: Some(traces),
        logs: Some(logs),
        metrics: Some(metrics),
    })
}

/// Register the gauges that describe resident state.
///
/// Only state goes here. Everything event-shaped — commit rate, latency,
/// contention — is already on the spans (`store.commit` carries `uc.attempts`
/// and `uc.version`), and the collector's spanmetrics connector derives RED
/// metrics from those. Emitting both would be double instrumentation that can
/// disagree with itself.
///
/// A no-op when telemetry is disabled: the global meter provider is then the
/// SDK's default, which drops everything.
pub(crate) fn register_store_metrics(pool: uc_db::AnyPool) {
    let meter = opentelemetry::global::meter_provider().meter("uc-server");

    let entities_pool = pool.clone();
    meter
        .u64_observable_gauge("uc.store.entities")
        .with_description(
            "Rows resident in the in-memory snapshot. The whole catalog is in \
             memory, so this is what decides whether the process fits its limit.",
        )
        .with_callback(move |observer| {
            // Skips this interval rather than blocking a writer; see
            // Store::try_snapshot.
            if let Some(snap) = entities_pool.try_snapshot() {
                for (kind, count) in snap.counts() {
                    observer.observe(
                        count as u64,
                        &[KeyValue::new("uc.kind", format!("{kind:?}"))],
                    );
                }
            }
        })
        .build();

    meter
        .u64_observable_gauge("uc.store.version")
        .with_description("Log version the in-memory snapshot has replayed to.")
        .with_callback(move |observer| {
            if let Some(snap) = pool.try_snapshot() {
                observer.observe(snap.version, &[]);
            }
        })
        .build();
}
