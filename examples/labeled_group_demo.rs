//! Demo showcasing the `labeled_group!` macro for HTTP request metrics.
//!
//! This example demonstrates how to use labeled counters to track metrics
//! with different label values (e.g., HTTP requests by method).
//!
//! Run with:
//! ```bash
//! cargo run --example labeled_group_demo --features full
//! ```

use contatori::counters::unsigned::Unsigned;
use contatori::counters::Observable;
use contatori::labeled_group;
use contatori::observers::json::JsonObserver;
use contatori::observers::prometheus::{MetricType, PrometheusObserver};
use contatori::observers::table::TableObserver;
use std::sync::Arc;
use std::thread;

// Define a labeled group for HTTP requests by method
labeled_group!(
    HttpRequests,
    "http_requests_total",
    "method",
    total: Unsigned,              // no label (aggregate)
    get: "GET": Unsigned,         // method="GET"
    post: "POST": Unsigned,       // method="POST"
    put: "PUT": Unsigned,         // method="PUT"
    delete: "DELETE": Unsigned,   // method="DELETE"
    patch: "PATCH": Unsigned,     // method="PATCH"
);

// Define a labeled group for HTTP response status codes
labeled_group!(
    HttpResponses,
    "http_responses_total",
    "status",
    ok: "200": Unsigned,              // status="200"
    created: "201": Unsigned,         // status="201"
    bad_request: "400": Unsigned,     // status="400"
    not_found: "404": Unsigned,       // status="404"
    server_error: "500": Unsigned,    // status="500"
);

// Static instances - can be used from anywhere in the application
static HTTP_REQUESTS: HttpRequests = HttpRequests::new();
static HTTP_RESPONSES: HttpResponses = HttpResponses::new();

fn simulate_traffic() {
    // Simulate some HTTP traffic
    
    // GET requests (most common)
    for _ in 0..150 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.get.add(1);
        HTTP_RESPONSES.ok.add(1);
    }
    
    // POST requests
    for _ in 0..50 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.post.add(1);
        HTTP_RESPONSES.created.add(1);
    }
    
    // PUT requests
    for _ in 0..30 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.put.add(1);
        HTTP_RESPONSES.ok.add(1);
    }
    
    // DELETE requests
    for _ in 0..10 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.delete.add(1);
        HTTP_RESPONSES.ok.add(1);
    }
    
    // PATCH requests
    for _ in 0..5 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.patch.add(1);
        HTTP_RESPONSES.ok.add(1);
    }
    
    // Some errors
    for _ in 0..15 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.get.add(1);
        HTTP_RESPONSES.not_found.add(1);
    }
    
    for _ in 0..5 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.post.add(1);
        HTTP_RESPONSES.bad_request.add(1);
    }
    
    for _ in 0..3 {
        HTTP_REQUESTS.total.add(1);
        HTTP_REQUESTS.get.add(1);
        HTTP_RESPONSES.server_error.add(1);
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
                // Simulate different request patterns per thread
                let method_choice = (thread_id + i) % 5;
                
                req.total.add(1);
                
                match method_choice {
                    0 => {
                        req.get.add(1);
                        resp.ok.add(1);
                    }
                    1 => {
                        req.post.add(1);
                        resp.created.add(1);
                    }
                    2 => {
                        req.put.add(1);
                        resp.ok.add(1);
                    }
                    3 => {
                        req.delete.add(1);
                        resp.ok.add(1);
                    }
                    _ => {
                        req.patch.add(1);
                        resp.ok.add(1);
                    }
                }
                
                // Simulate some errors
                if i % 100 == 0 {
                    req.total.add(1);
                    req.get.add(1);
                    resp.not_found.add(1);
                }
            }
        });
        
        handles.push(handle);
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    println!("\n=== Concurrent Traffic Results ({} threads × {} iterations) ===\n", 
             num_threads, iterations);
    
    // Show results
    let counters: Vec<&dyn Observable> = vec![&*requests, &*responses];
    
    println!("--- Table Format ---\n");
    let table = TableObserver::new().with_header(true);
    println!("{}", table.render(counters.iter().copied()));
    
    println!("\n--- Prometheus Format ---\n");
    let prometheus = PrometheusObserver::new()
        .with_namespace("myapp")
        .with_type("http_requests_total", MetricType::Counter)
        .with_type("http_responses_total", MetricType::Counter)
        .with_help("http_requests_total", "Total HTTP requests by method")
        .with_help("http_responses_total", "Total HTTP responses by status code");
    
    match prometheus.render(counters.iter().copied()) {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
}

fn main() {
    println!("=== Labeled Group Demo ===\n");
    println!("This demo shows how to use labeled_group! for HTTP metrics.\n");
    
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
    match json.to_json(counters.iter().copied()) {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
    
    // === Prometheus Output ===
    println!("\n--- Prometheus Format ---\n");
    let prometheus = PrometheusObserver::new()
        .with_namespace("myapp")
        .with_type("http_requests_total", MetricType::Counter)
        .with_type("http_responses_total", MetricType::Counter)
        .with_help("http_requests_total", "Total HTTP requests by method")
        .with_help("http_responses_total", "Total HTTP responses by status code");
    
    match prometheus.render(counters.iter().copied()) {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
    
    // === Direct Access Demo ===
    println!("\n--- Direct Field Access ---\n");
    println!("HTTP_REQUESTS.total = {}", HTTP_REQUESTS.total.value());
    println!("HTTP_REQUESTS.get   = {}", HTTP_REQUESTS.get.value());
    println!("HTTP_REQUESTS.post  = {}", HTTP_REQUESTS.post.value());
    println!("HTTP_REQUESTS.put   = {}", HTTP_REQUESTS.put.value());
    println!("HTTP_REQUESTS.delete = {}", HTTP_REQUESTS.delete.value());
    println!("HTTP_REQUESTS.patch = {}", HTTP_REQUESTS.patch.value());
    
    // === expand() Demo ===
    println!("\n--- Using expand() ---\n");
    for entry in HTTP_REQUESTS.expand() {
        match entry.label {
            Some((key, value)) => {
                println!("  {}{{{}=\"{}\"}} = {}", entry.name, key, value, entry.value);
            }
            None => {
                println!("  {} = {} (total)", entry.name, entry.value);
            }
        }
    }
    
    // === Concurrent Demo ===
    simulate_concurrent_traffic(4, 10_000);
}