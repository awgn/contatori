//! Demo application showcasing different counter serialization formats.
//!
//! Run with:
//! ```bash
//! cargo run --example demo --features demo -- --help
//! ```

use clap::{Parser, ValueEnum};
use contatori::contatori::average::Average;
use contatori::contatori::maximum::Maximum;
use contatori::contatori::minimum::Minimum;
use contatori::contatori::signed::Signed;
use contatori::contatori::unsigned::Unsigned;
use contatori::contatori::Observable;
use contatori::observers::json::JsonObserver;
use contatori::observers::prometheus::{MetricType, PrometheusObserver};
use contatori::observers::table::{CompactSeparator, TableObserver, TableStyle};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Output format for counter serialization.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    /// Pretty ASCII table (standard two-column format)
    Table,
    /// Compact table with multiple columns
    Compact,
    /// JSON format
    Json,
    /// Prometheus exposition format
    Prometheus,
}

/// Table style selection.
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum StyleChoice {
    Ascii,
    #[default]
    Rounded,
    Sharp,
    Modern,
    Markdown,
    Dots,
    Blank,
}

impl From<StyleChoice> for TableStyle {
    fn from(choice: StyleChoice) -> Self {
        match choice {
            StyleChoice::Ascii => TableStyle::Ascii,
            StyleChoice::Rounded => TableStyle::Rounded,
            StyleChoice::Sharp => TableStyle::Sharp,
            StyleChoice::Modern => TableStyle::Modern,
            StyleChoice::Markdown => TableStyle::Markdown,
            StyleChoice::Dots => TableStyle::Dots,
            StyleChoice::Blank => TableStyle::Blank,
        }
    }
}

/// Separator style for compact table format.
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum SeparatorChoice {
    #[default]
    Colon,
    Equals,
    Arrow,
    Pipe,
    Space,
}

impl From<SeparatorChoice> for CompactSeparator {
    fn from(choice: SeparatorChoice) -> Self {
        match choice {
            SeparatorChoice::Colon => CompactSeparator::Colon,
            SeparatorChoice::Equals => CompactSeparator::Equals,
            SeparatorChoice::Arrow => CompactSeparator::Arrow,
            SeparatorChoice::Pipe => CompactSeparator::Pipe,
            SeparatorChoice::Space => CompactSeparator::Space,
        }
    }
}

/// Demo application for contatori - high-performance sharded counters.
///
/// This demo creates sample counters, optionally simulates concurrent updates,
/// and serializes them in various formats.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Output format
    #[arg(short, long, value_enum, default_value = "table")]
    format: OutputFormat,

    /// Table style (for table/compact formats)
    #[arg(short, long, value_enum, default_value = "rounded")]
    style: StyleChoice,

    /// Number of columns (for compact format)
    #[arg(short, long, default_value = "3")]
    columns: usize,

    /// Separator style (for compact format)
    #[arg(long, value_enum, default_value = "colon")]
    separator: SeparatorChoice,

    /// Pretty print JSON output
    #[arg(long)]
    pretty: bool,

    /// Include timestamp in JSON output
    #[arg(long)]
    timestamp: bool,

    /// Prometheus metric namespace (prefix)
    #[arg(long, default_value = "demo")]
    namespace: String,

    /// Prometheus instance label
    #[arg(long)]
    instance: Option<String>,

    /// Simulate concurrent updates with N threads
    #[arg(long)]
    simulate: Option<usize>,

    /// Number of iterations per thread in simulation
    #[arg(long, default_value = "10000")]
    iterations: usize,

    /// Reset counters after reading (show delta)
    #[arg(long)]
    reset: bool,

    /// Add a title to the output (table formats)
    #[arg(long)]
    title: Option<String>,

    /// Watch mode: refresh every N milliseconds
    #[arg(short, long)]
    watch: Option<u64>,

    /// Hide header in standard table mode
    #[arg(long)]
    no_header: bool,
}

/// Creates sample counters with initial values.
fn create_counters() -> (
    Unsigned,
    Unsigned,
    Signed,
    Minimum,
    Maximum,
    Average,
) {
    let requests = Unsigned::new().with_name("http_requests_total");
    let errors = Unsigned::new().with_name("http_errors_total");
    let connections = Signed::new().with_name("active_connections");
    let min_latency = Minimum::new().with_name("request_latency_min_ms");
    let max_latency = Maximum::new().with_name("request_latency_max_ms");
    let avg_latency = Average::new().with_name("request_latency_avg_ms");

    // Initialize with some values
    requests.add(1000);
    errors.add(23);
    connections.add(42);
    min_latency.observe(5);
    max_latency.observe(250);
    avg_latency.observe(45);
    avg_latency.observe(67);
    avg_latency.observe(89);

    (requests, errors, connections, min_latency, max_latency, avg_latency)
}

/// Simulates concurrent counter updates.
fn simulate_traffic(
    requests: &Arc<Unsigned>,
    errors: &Arc<Unsigned>,
    connections: &Arc<Signed>,
    min_latency: &Arc<Minimum>,
    max_latency: &Arc<Maximum>,
    avg_latency: &Arc<Average>,
    num_threads: usize,
    iterations: usize,
) {
    let mut handles = vec![];

    for i in 0..num_threads {
        let req = Arc::clone(requests);
        let err = Arc::clone(errors);
        let conn = Arc::clone(connections);
        let min_lat = Arc::clone(min_latency);
        let max_lat = Arc::clone(max_latency);
        let avg_lat = Arc::clone(avg_latency);

        let handle = thread::spawn(move || {
            for j in 0..iterations {
                req.add(1);

                // Simulate ~5% error rate
                if (i * iterations + j) % 20 == 0 {
                    err.add(1);
                }

                // Simulate connection churn
                if j % 10 == 0 {
                    conn.add(1);
                }
                if j % 15 == 0 {
                    conn.sub(1);
                }

                // Simulate latency observations
                let latency = 10 + (j % 200);
                min_lat.observe(latency);
                max_lat.observe(latency);
                avg_lat.observe(latency);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

/// Renders counters in the specified format.
fn render_output(args: &Args, counters: Vec<&dyn Observable>) -> String {
    match args.format {
        OutputFormat::Table => {
            let mut observer = TableObserver::new()
                .with_style(args.style.into())
                .with_header(!args.no_header);

            if let Some(ref title) = args.title {
                observer = observer.with_title(title.clone());
            }

            if args.reset {
                observer.render_and_reset(counters.into_iter())
            } else {
                observer.render(counters.into_iter())
            }
        }

        OutputFormat::Compact => {
            let mut observer = TableObserver::new()
                .compact(true)
                .columns(args.columns)
                .separator(args.separator.into())
                .with_style(args.style.into());

            if let Some(ref title) = args.title {
                observer = observer.with_title(title.clone());
            }

            if args.reset {
                observer.render_and_reset(counters.into_iter())
            } else {
                observer.render(counters.into_iter())
            }
        }

        OutputFormat::Json => {
            let observer = JsonObserver::new()
                .pretty(args.pretty)
                .wrap_in_snapshot(args.timestamp)
                .include_timestamp(args.timestamp);

            if args.reset {
                observer.to_json_and_reset(counters.into_iter())
            } else {
                observer.to_json(counters.into_iter())
            }
            .unwrap_or_else(|e| format!("Error: {}", e))
        }

        OutputFormat::Prometheus => {
            let mut observer = PrometheusObserver::new()
                .with_namespace(&args.namespace)
                .with_type("http_requests_total", MetricType::Counter)
                .with_type("http_errors_total", MetricType::Counter)
                .with_type("active_connections", MetricType::Gauge)
                .with_type("request_latency_min_ms", MetricType::Gauge)
                .with_type("request_latency_max_ms", MetricType::Gauge)
                .with_type("request_latency_avg_ms", MetricType::Gauge)
                .with_help("http_requests_total", "Total number of HTTP requests")
                .with_help("http_errors_total", "Total number of HTTP errors")
                .with_help("active_connections", "Number of active connections")
                .with_help("request_latency_min_ms", "Minimum request latency in milliseconds")
                .with_help("request_latency_max_ms", "Maximum request latency in milliseconds")
                .with_help("request_latency_avg_ms", "Average request latency in milliseconds");

            if let Some(ref instance) = args.instance {
                observer = observer.with_const_label("instance", instance);
            }

            if args.reset {
                observer.render_and_reset(counters.into_iter())
            } else {
                observer.render(counters.into_iter())
            }
            .unwrap_or_else(|e| format!("Error: {}", e))
        }
    }
}

fn main() {
    let args = Args::parse();

    // Create counters
    let (requests, errors, connections, min_latency, max_latency, avg_latency) = create_counters();

    // Wrap in Arc for potential simulation
    let requests = Arc::new(requests);
    let errors = Arc::new(errors);
    let connections = Arc::new(connections);
    let min_latency = Arc::new(min_latency);
    let max_latency = Arc::new(max_latency);
    let avg_latency = Arc::new(avg_latency);

    // Run simulation if requested
    if let Some(num_threads) = args.simulate {
        eprintln!(
            "Simulating {} threads × {} iterations...",
            num_threads, args.iterations
        );
        simulate_traffic(
            &requests,
            &errors,
            &connections,
            &min_latency,
            &max_latency,
            &avg_latency,
            num_threads,
            args.iterations,
        );
        eprintln!("Simulation complete.\n");
    }

    // Watch mode or single output
    if let Some(interval_ms) = args.watch {
        loop {
            // Clear screen (ANSI escape code)
            print!("\x1B[2J\x1B[1;1H");

            let counters: Vec<&dyn Observable> = vec![
                requests.as_ref(),
                errors.as_ref(),
                connections.as_ref(),
                min_latency.as_ref(),
                max_latency.as_ref(),
                avg_latency.as_ref(),
            ];

            println!("{}", render_output(&args, counters));

            thread::sleep(Duration::from_millis(interval_ms));
        }
    } else {
        let counters: Vec<&dyn Observable> = vec![
            requests.as_ref(),
            errors.as_ref(),
            connections.as_ref(),
            min_latency.as_ref(),
            max_latency.as_ref(),
            avg_latency.as_ref(),
        ];

        println!("{}", render_output(&args, counters));
    }
}