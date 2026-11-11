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

use contatori::observers::opentelemetry::OtelObserver;
use contatori::observers::Result;
use contatori::prelude::*;

use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::runtime;
use std::time::Duration;

// Define static counters
static HTTP_REQUESTS: Monotone = Monotone::new().with_name("http_requests_total");
static HTTP_ERRORS: Monotone = Monotone::new().with_name("http_errors_total");
static ACTIVE_CONNECTIONS: Unsigned = Unsigned::new().with_name("active_connections");
static REQUEST_LATENCY: Average = Average::new().with_name("request_latency_ms");
static QUEUE_DEPTH: Signed = Signed::new().with_name("queue_depth");

// Define single-label and multi-label groups
labeled_group!(
    HttpByMethod,
    "http_requests_by_method",
    "method",
    get: "GET": Monotone,
    post: "POST": Monotone,
    put: "PUT": Monotone,
    delete: "DELETE": Monotone,
);

static HTTP_METHODS: HttpByMethod = HttpByMethod::new();

labeled_group!(
    HttpByRouteStatus,
    "http_responses_by_route",
    ["route", "status"],
    users_ok: ["/api/users", "2xx"]: Monotone,
    users_client_error: ["/api/users", "4xx"]: Monotone,
    orders_ok: ["/api/orders", "2xx"]: Monotone,
    orders_server_error: ["/api/orders", "5xx"]: Monotone,
);

static HTTP_ROUTES: HttpByRouteStatus = HttpByRouteStatus::new();

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

    let counters: &[&'static dyn Observable] = &[
        &HTTP_REQUESTS,
        &HTTP_ERRORS,
        &ACTIVE_CONNECTIONS,
        &REQUEST_LATENCY,
        &QUEUE_DEPTH,
        &HTTP_METHODS,
        &HTTP_ROUTES,
    ];

    observer.register(counters)?;
    println!("Registered {} metrics with OpenTelemetry\n", counters.len());
    Ok(())
}

fn simulate_traffic() {
    for i in 0..100_u64 {
        HTTP_REQUESTS.incr();

        match i % 4 {
            0 => HTTP_METHODS.get.incr(),
            1 => HTTP_METHODS.post.incr(),
            2 => HTTP_METHODS.put.incr(),
            _ => HTTP_METHODS.delete.incr(),
        }

        match i % 10 {
            0..=4 => HTTP_ROUTES.users_ok.incr(),
            5..=7 => HTTP_ROUTES.orders_ok.incr(),
            8 => {
                HTTP_ROUTES.users_client_error.incr();
                HTTP_ERRORS.incr();
            }
            _ => {
                HTTP_ROUTES.orders_server_error.incr();
                HTTP_ERRORS.incr();
            }
        }

        REQUEST_LATENCY.observe(10 + (i % 90));

        if i % 5 == 0 {
            ACTIVE_CONNECTIONS.incr();
        }
        if i % 7 == 0 {
            QUEUE_DEPTH.incr();
        }
        if i % 4 == 0 {
            QUEUE_DEPTH.decr();
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
