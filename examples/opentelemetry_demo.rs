//! OpenTelemetry observer demo.
//!
//! This example demonstrates how to use the OpenTelemetry observer to export
//! contatori metrics via OpenTelemetry's stdout exporter.
//!
//! # Running the example
//!
//! ```bash
//! cargo run --example opentelemetry_demo --features opentelemetry
//! ```

use contatori::counters::average::Average;
use contatori::counters::monotone::Monotone;
use contatori::counters::signed::Signed;
use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::labeled_group;
use contatori::observers::opentelemetry::OtelObserver;
use contatori::observers::Result;

use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::runtime;
use std::time::Duration;

// Define static counters
static HTTP_REQUESTS: Monotone = Monotone::new().with_name("http_requests_total");
static HTTP_ERRORS: Monotone = Monotone::new().with_name("http_errors_total");
static ACTIVE_CONNECTIONS: Unsigned = Unsigned::new().with_name("active_connections");
static REQUEST_LATENCY: Average = Average::new().with_name("request_latency_ms");
static QUEUE_DEPTH: Signed = Signed::new().with_name("queue_depth");

// Define labeled groups
labeled_group!(
    HttpByMethod,
    "http_requests_by_method",
    "method",
    value: Unsigned,
    get: "GET": Unsigned,
    post: "POST": Unsigned,
    put: "PUT": Unsigned,
    delete: "DELETE": Unsigned,
);

static HTTP_METHODS: HttpByMethod = HttpByMethod::new();

labeled_group!(
    HttpByStatus,
    "http_responses_by_status",
    "status",
    value: Unsigned,
    ok: "2xx": Unsigned,
    redirect: "3xx": Unsigned,
    client_error: "4xx": Unsigned,
    server_error: "5xx": Unsigned,
);

static HTTP_STATUS: HttpByStatus = HttpByStatus::new();

fn setup_opentelemetry() -> SdkMeterProvider {
    // Use the stdout exporter - it prints metrics as JSON
    let exporter = opentelemetry_stdout::MetricExporter::default();

    // PeriodicReader collects metrics and sends them to the exporter
    let reader = PeriodicReader::builder(exporter, runtime::Tokio)
        .with_interval(Duration::from_secs(60)) // Long interval, we'll use force_flush
        .build();

    let provider = SdkMeterProvider::builder().with_reader(reader).build();

    opentelemetry::global::set_meter_provider(provider.clone());
    provider
}

fn register_metrics() -> Result<()> {
    let observer = OtelObserver::new("contatori_demo");

    let counters: &[&'static (dyn Observable + Send + Sync)] = &[
        &HTTP_REQUESTS,
        &HTTP_ERRORS,
        &ACTIVE_CONNECTIONS,
        &REQUEST_LATENCY,
        &QUEUE_DEPTH,
        &HTTP_METHODS,
        &HTTP_STATUS,
    ];

    observer.register(counters)?;
    println!("Registered {} metrics with OpenTelemetry\n", counters.len());
    Ok(())
}

fn simulate_traffic() {
    for i in 0..100 {
        HTTP_REQUESTS.add(1);

        HTTP_METHODS.value.add(1);
        match i % 4 {
            0 => HTTP_METHODS.get.add(1),
            1 => HTTP_METHODS.post.add(1),
            2 => HTTP_METHODS.put.add(1),
            _ => HTTP_METHODS.delete.add(1),
        }

        HTTP_STATUS.value.add(1);
        match i % 10 {
            0..=6 => HTTP_STATUS.ok.add(1),
            7 => HTTP_STATUS.redirect.add(1),
            8 => {
                HTTP_STATUS.client_error.add(1);
                HTTP_ERRORS.add(1);
            }
            _ => {
                HTTP_STATUS.server_error.add(1);
                HTTP_ERRORS.add(1);
            }
        }

        REQUEST_LATENCY.observe(10 + (i % 90));

        if i % 5 == 0 {
            ACTIVE_CONNECTIONS.add(1);
        }
        if i % 7 == 0 {
            QUEUE_DEPTH.add(1);
        }
        if i % 4 == 0 {
            QUEUE_DEPTH.sub(1);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== OpenTelemetry Stdout Demo ===\n");

    let provider = setup_opentelemetry();
    register_metrics()?;

    println!("Simulating traffic...\n");
    simulate_traffic();

    println!("Flushing metrics to stdout (JSON format):\n");
    provider.force_flush().expect("flush failed");

    println!("\nSimulating more traffic...\n");
    simulate_traffic();

    println!("Flushing again:\n");
    provider.force_flush().expect("flush failed");

    provider.shutdown().expect("shutdown failed");
    println!("\nDone!");
    Ok(())
}