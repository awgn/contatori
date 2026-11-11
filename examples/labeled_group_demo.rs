//! Demo showcasing the `labeled_group!` macro for single-label and multi-label HTTP metrics.
//!
//! Run with:
//! ```bash
//! cargo run --example labeled_group_demo --features full
//! ```

use contatori::observers::json::JsonObserver;
use contatori::observers::prometheus::PrometheusObserver;
use contatori::observers::table::TableObserver;
use contatori::prelude::*;
use std::sync::Arc;
use std::thread;

// Define a single-label group for HTTP requests by method.
// Because sub-counters use `Monotone`, observers automatically export this as a Counter!
labeled_group!(
    HttpRequests,
    "http_requests_total",
    "method",
    get: "GET": Monotone,
    post: "POST": Monotone,
    put: "PUT": Monotone,
    delete: "DELETE": Monotone,
    patch: "PATCH": Monotone,
);

// Define a multi-label group for HTTP responses by method and status code.
labeled_group!(
    HttpResponses,
    "http_responses_total",
    ["method", "status"],
    get_ok: ["GET", "200"]: Monotone,
    post_created: ["POST", "201"]: Monotone,
    put_ok: ["PUT", "200"]: Monotone,
    delete_ok: ["DELETE", "200"]: Monotone,
    patch_ok: ["PATCH", "200"]: Monotone,
    post_bad_request: ["POST", "400"]: Monotone,
    get_not_found: ["GET", "404"]: Monotone,
    get_server_error: ["GET", "500"]: Monotone,
);

// Static instances - can be used from anywhere in the application
static HTTP_REQUESTS: HttpRequests = HttpRequests::new();
static HTTP_RESPONSES: HttpResponses = HttpResponses::new();

fn simulate_traffic() {
    // GET requests (most common)
    for _ in 0..150 {
        HTTP_REQUESTS.get.incr();
        HTTP_RESPONSES.get_ok.incr();
    }

    // POST requests
    for _ in 0..50 {
        HTTP_REQUESTS.post.incr();
        HTTP_RESPONSES.post_created.incr();
    }

    // PUT requests
    for _ in 0..30 {
        HTTP_REQUESTS.put.incr();
        HTTP_RESPONSES.put_ok.incr();
    }

    // DELETE requests
    for _ in 0..10 {
        HTTP_REQUESTS.delete.incr();
        HTTP_RESPONSES.delete_ok.incr();
    }

    // PATCH requests
    for _ in 0..5 {
        HTTP_REQUESTS.patch.incr();
        HTTP_RESPONSES.patch_ok.incr();
    }

    // Some errors
    for _ in 0..15 {
        HTTP_REQUESTS.get.incr();
        HTTP_RESPONSES.get_not_found.incr();
    }

    for _ in 0..5 {
        HTTP_REQUESTS.post.incr();
        HTTP_RESPONSES.post_bad_request.incr();
    }

    for _ in 0..3 {
        HTTP_REQUESTS.get.incr();
        HTTP_RESPONSES.get_server_error.incr();
    }
}

fn simulate_concurrent_traffic(num_threads: usize, iterations: usize) {
    let requests = Arc::new(HttpRequests::new());
    let responses = Arc::new(HttpResponses::new());

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let req = Arc::clone(&requests);
        let resp = Arc::clone(&responses);

        let handle = thread::spawn(move || {
            for i in 0..iterations {
                match (thread_id + i) % 5 {
                    0 => {
                        req.get.incr();
                        resp.get_ok.incr();
                    }
                    1 => {
                        req.post.incr();
                        resp.post_created.incr();
                    }
                    2 => {
                        req.put.incr();
                        resp.put_ok.incr();
                    }
                    3 => {
                        req.delete.incr();
                        resp.delete_ok.incr();
                    }
                    _ => {
                        req.patch.incr();
                        resp.patch_ok.incr();
                    }
                }

                // Simulate some 404s
                if i % 100 == 0 {
                    req.get.incr();
                    resp.get_not_found.incr();
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    println!(
        "\n=== Concurrent Traffic Results ({num_threads} threads × {iterations} iterations) ===\n"
    );

    let counters: Vec<&dyn Observable> = vec![&*requests, &*responses];

    println!("--- Table Format ---\n");
    let table = TableObserver::new().with_header(true);
    println!("{}", table.render(counters.iter().copied()));

    println!("\n--- Prometheus Format ---\n");
    let prometheus = PrometheusObserver::new()
        .with_namespace("myapp")
        .with_help("http_requests_total", "Total HTTP requests by method")
        .with_help(
            "http_responses_total",
            "Total HTTP responses by method and status code",
        );

    match prometheus.render(counters.iter().copied()) {
        Ok(output) => println!("{output}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn main() {
    println!("=== Labeled Group Demo ===\n");
    println!("This demo shows single-label and multi-label groups for HTTP metrics.\n");

    // Simulate traffic on static counters
    simulate_traffic();

    // Collect both groups as observables
    let counters: Vec<&dyn Observable> = vec![&HTTP_REQUESTS, &HTTP_RESPONSES];

    // === Table Output ===
    println!("--- Table Format ---\n");
    let table = TableObserver::new().with_header(true);
    println!("{}", table.render(counters.iter().copied()));

    // === JSON Output ===
    println!("\n--- JSON Format ---\n");
    let json = JsonObserver::new().pretty(true);
    match json.render(counters.iter().copied()) {
        Ok(output) => println!("{output}"),
        Err(e) => eprintln!("Error: {e}"),
    }

    // === Prometheus Output ===
    println!("\n--- Prometheus Format ---\n");
    let prometheus = PrometheusObserver::new()
        .with_namespace("myapp")
        .with_help("http_requests_total", "Total HTTP requests by method")
        .with_help(
            "http_responses_total",
            "Total HTTP responses by method and status code",
        );

    match prometheus.render(counters.iter().copied()) {
        Ok(output) => println!("{output}"),
        Err(e) => eprintln!("Error: {e}"),
    }

    // === Direct Access & Dynamic Total Demo ===
    println!("\n--- Direct Field Access & Dynamic Total ---\n");
    println!("HTTP_REQUESTS.total() = {}", HTTP_REQUESTS.total());
    println!("HTTP_REQUESTS.get     = {}", HTTP_REQUESTS.get.value());
    println!("HTTP_REQUESTS.post    = {}", HTTP_REQUESTS.post.value());
    println!("HTTP_REQUESTS.put     = {}", HTTP_REQUESTS.put.value());
    println!("HTTP_REQUESTS.delete  = {}", HTTP_REQUESTS.delete.value());
    println!("HTTP_REQUESTS.patch   = {}", HTTP_REQUESTS.patch.value());

    // === expand() Demo ===
    println!("\n--- Using expand() ---\n");
    for entry in HTTP_RESPONSES.expand() {
        if let Some(value) = entry.value {
            let labels = entry
                .labels
                .iter()
                .map(|(k, v)| format!("{k}=\"{v}\""))
                .collect::<Vec<_>>()
                .join(",");
            println!("  {}{{{labels}}} = {value}", entry.name);
        }
    }

    // === Concurrent Demo ===
    simulate_concurrent_traffic(4, 10_000);
}
