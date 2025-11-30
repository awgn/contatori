//! Prometheus observer for exporting counters using the official `prometheus` crate.
//!
//! This module provides [`PrometheusObserver`], which exports a collection of
//! [`Observable`] counters to a Prometheus [`Registry`](prometheus::Registry) and renders them using
//! the official Prometheus text format.
//!
//! # Feature Flag
//!
//! This module requires the `prometheus` feature:
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.1", features = ["prometheus"] }
//! ```
//!
//! # How It Works
//!
//! Unlike a hand-rolled text formatter, this observer uses the official
//! `prometheus` crate which provides:
//!
//! - Proper metric types (`Counter`, `Gauge`)
//! - A [`Registry`](prometheus::Registry) for managing metrics
//! - [`TextEncoder`](prometheus::TextEncoder) for generating the exposition format
//! - Full compatibility with Prometheus ecosystem
//!
//! # Integration with Prometheus
//!
//! To expose metrics to Prometheus:
//!
//! 1. Create a `PrometheusObserver` and register your counters
//! 2. Call `render()` to get the exposition format string
//! 3. Serve this string on an HTTP `/metrics` endpoint
//! 4. Configure Prometheus to scrape your endpoint
//!
//! # Examples
//!
//! Basic usage:
//!
//! ```rust,ignore
//! use contatori::contatori::unsigned::Unsigned;
//! use contatori::contatori::Observable;
//! use contatori::observers::prometheus::{PrometheusObserver, MetricType};
//!
//! let requests = Unsigned::new().with_name("http_requests_total");
//! requests.add(100);
//!
//! let observer = PrometheusObserver::new();
//! let counters: Vec<&dyn Observable> = vec![&requests];
//!
//! let output = observer.render(counters.into_iter())?;
//! println!("{}", output);
//! # Ok::<(), contatori::observers::prometheus::PrometheusError>(())
//! ```
//!
//! With metric configuration:
//!
//! ```rust,ignore
//! use contatori::observers::prometheus::{PrometheusObserver, MetricConfig, MetricType};
//!
//! let observer = PrometheusObserver::new()
//!     .with_namespace("myapp")
//!     .with_const_label("instance", "localhost:8080")
//!     .with_metric_config("http_requests_total", MetricConfig {
//!         metric_type: MetricType::Counter,
//!         help: Some("Total HTTP requests".into()),
//!         ..Default::default()
//!     });
//! ```
//!
//! With custom registry (for testing or multiple registries):
//!
//! ```rust,ignore
//! use prometheus::Registry;
//! use contatori::observers::prometheus::PrometheusObserver;
//!
//! let registry = Registry::new();
//! let observer = PrometheusObserver::with_registry(registry);
//! ```

use crate::contatori::{CounterValue, Observable};
use prometheus::{Encoder, IntCounter, IntGauge, Registry, TextEncoder};
use std::collections::HashMap;
use std::fmt;

/// Error type for Prometheus observer operations.
#[derive(Debug)]
pub enum PrometheusError {
    /// Error creating or registering a metric.
    MetricError(String),
    /// Error encoding metrics to text format.
    EncodeError(String),
    /// Error converting bytes to UTF-8 string.
    Utf8Error(std::string::FromUtf8Error),
}

impl fmt::Display for PrometheusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrometheusError::MetricError(msg) => write!(f, "metric error: {}", msg),
            PrometheusError::EncodeError(msg) => write!(f, "encode error: {}", msg),
            PrometheusError::Utf8Error(err) => write!(f, "UTF-8 error: {}", err),
        }
    }
}

impl std::error::Error for PrometheusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PrometheusError::Utf8Error(err) => Some(err),
            _ => None,
        }
    }
}

impl From<prometheus::Error> for PrometheusError {
    fn from(err: prometheus::Error) -> Self {
        PrometheusError::MetricError(err.to_string())
    }
}

impl From<std::string::FromUtf8Error> for PrometheusError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        PrometheusError::Utf8Error(err)
    }
}

/// Result type for Prometheus observer operations.
pub type Result<T> = std::result::Result<T, PrometheusError>;

/// Prometheus metric type.
///
/// Determines how the metric is registered and displayed in Prometheus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MetricType {
    /// A counter is a cumulative metric that only ever goes up.
    /// Use for metrics like total requests, errors, bytes sent.
    #[default]
    Counter,
    /// A gauge can go up and down.
    /// Use for metrics like current connections, temperature, queue size.
    Gauge,
}

/// Configuration for a specific metric.
#[derive(Debug, Clone, Default)]
pub struct MetricConfig {
    /// The type of metric (Counter or Gauge).
    pub metric_type: MetricType,
    /// Help text describing the metric.
    pub help: Option<String>,
    /// Additional labels specific to this metric.
    pub labels: HashMap<String, String>,
}

/// Observer that exports counters to Prometheus format using the official crate.
///
/// This observer creates Prometheus metrics from [`Observable`] counters and
/// renders them using the official [`TextEncoder`](prometheus::TextEncoder).
///
/// # Example
///
/// ```rust,ignore
/// use contatori::contatori::unsigned::Unsigned;
/// use contatori::contatori::Observable;
/// use contatori::observers::prometheus::PrometheusObserver;
///
/// let counter = Unsigned::new().with_name("my_counter");
/// counter.add(42);
///
/// let observer = PrometheusObserver::new();
/// let counters: Vec<&dyn Observable> = vec![&counter];
/// let output = observer.render(counters.into_iter())?;
///
/// assert!(output.contains("my_counter 42"));
/// # Ok::<(), contatori::observers::prometheus::PrometheusError>(())
/// ```
pub struct PrometheusObserver {
    /// The Prometheus registry for this observer.
    registry: Registry,
    /// Namespace (prefix) for all metrics.
    namespace: Option<String>,
    /// Subsystem for all metrics.
    subsystem: Option<String>,
    /// Constant labels applied to all metrics.
    const_labels: HashMap<String, String>,
    /// Per-metric configuration.
    metric_configs: HashMap<String, MetricConfig>,
    /// Default metric type when not specified.
    default_type: MetricType,
}

impl Default for PrometheusObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl PrometheusObserver {
    /// Creates a new `PrometheusObserver` with a fresh registry.
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            namespace: None,
            subsystem: None,
            const_labels: HashMap::new(),
            metric_configs: HashMap::new(),
            default_type: MetricType::Counter,
        }
    }

    /// Creates a `PrometheusObserver` with an existing registry.
    ///
    /// Useful when you want to combine metrics from multiple sources
    /// or integrate with an existing Prometheus setup.
    pub fn with_registry(registry: Registry) -> Self {
        Self {
            registry,
            namespace: None,
            subsystem: None,
            const_labels: HashMap::new(),
            metric_configs: HashMap::new(),
            default_type: MetricType::Counter,
        }
    }

    /// Returns a reference to the underlying Prometheus registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Sets the namespace (prefix) for all metrics.
    ///
    /// The namespace is prepended to metric names with an underscore.
    /// For example, namespace "myapp" + metric "requests" = "myapp_requests".
    pub fn with_namespace(mut self, namespace: &str) -> Self {
        self.namespace = Some(namespace.to_string());
        self
    }

    /// Sets the subsystem for all metrics.
    ///
    /// The subsystem appears between namespace and metric name.
    /// For example, namespace "myapp" + subsystem "http" + metric "requests" = "myapp_http_requests".
    pub fn with_subsystem(mut self, subsystem: &str) -> Self {
        self.subsystem = Some(subsystem.to_string());
        self
    }

    /// Adds a constant label to all metrics.
    ///
    /// Constant labels are useful for identifying the source instance,
    /// environment, or other metadata.
    pub fn with_const_label(mut self, name: &str, value: &str) -> Self {
        self.const_labels
            .insert(name.to_string(), value.to_string());
        self
    }

    /// Sets the default metric type for metrics without explicit configuration.
    pub fn with_default_type(mut self, metric_type: MetricType) -> Self {
        self.default_type = metric_type;
        self
    }

    /// Configures a specific metric.
    pub fn with_metric_config(mut self, name: &str, config: MetricConfig) -> Self {
        self.metric_configs.insert(name.to_string(), config);
        self
    }

    /// Sets the metric type for a specific metric.
    pub fn with_type(mut self, name: &str, metric_type: MetricType) -> Self {
        self.metric_configs
            .entry(name.to_string())
            .or_default()
            .metric_type = metric_type;
        self
    }

    /// Sets the help text for a specific metric.
    pub fn with_help(mut self, name: &str, help: &str) -> Self {
        self.metric_configs
            .entry(name.to_string())
            .or_default()
            .help = Some(help.to_string());
        self
    }

    /// Sanitizes a metric name to be Prometheus-compatible.
    ///
    /// Prometheus metric names must match `[a-zA-Z_:][a-zA-Z0-9_:]*`.
    fn sanitize_name(name: &str) -> String {
        let mut result = String::with_capacity(name.len());
        for c in name.chars() {
            if c.is_ascii_alphanumeric() || c == '_' || c == ':' {
                result.push(c);
            } else if c == '-' || c == '.' || c == ' ' {
                result.push('_');
            } else if c.is_alphabetic() {
                result.push('_');
                result.push(c);
            }
        }
        if result.is_empty() {
            result.push_str("unnamed");
        }
        // Ensure name doesn't start with a digit
        if result
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            result.insert(0, '_');
        }
        result
    }

    /// Builds the full metric name with namespace and subsystem.
    fn build_full_name(&self, name: &str) -> String {
        let sanitized = Self::sanitize_name(name);
        match (&self.namespace, &self.subsystem) {
            (Some(ns), Some(ss)) => format!("{}_{}_{}", ns, ss, sanitized),
            (Some(ns), None) => format!("{}_{}", ns, sanitized),
            (None, Some(ss)) => format!("{}_{}", ss, sanitized),
            (None, None) => sanitized,
        }
    }

    /// Renders counters to Prometheus exposition format.
    ///
    /// This method:
    /// 1. Creates Prometheus metrics for each counter
    /// 2. Registers them with the registry
    /// 3. Encodes everything using the TextEncoder
    ///
    /// Note: This creates a fresh registry for each render to avoid
    /// conflicts with previously registered metrics.
    ///
    /// # Errors
    ///
    /// Returns an error if metric creation, registration, or encoding fails.
    pub fn render<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> Result<String> {
        // Create a fresh registry for this render
        let registry = Registry::new();

        for counter in counters {
            let raw_name = if counter.name().is_empty() {
                "unnamed"
            } else {
                counter.name()
            };

            let full_name = self.build_full_name(raw_name);
            let config = self.metric_configs.get(raw_name);
            let metric_type = config.map(|c| c.metric_type).unwrap_or(self.default_type);
            let help = config
                .and_then(|c| c.help.clone())
                .unwrap_or_else(|| format!("{} metric", raw_name));

            // Merge const_labels with metric-specific labels
            let mut labels = self.const_labels.clone();
            if let Some(cfg) = config {
                labels.extend(cfg.labels.clone());
            }

            let value = counter.value();

            match metric_type {
                MetricType::Counter => {
                    self.register_counter(&registry, &full_name, &help, &labels, value)?;
                }
                MetricType::Gauge => {
                    self.register_gauge(&registry, &full_name, &help, &labels, value)?;
                }
            }
        }

        // Encode to text format
        self.encode_registry(&registry)
    }

    /// Renders counters and resets them atomically.
    ///
    /// # Errors
    ///
    /// Returns an error if metric creation, registration, or encoding fails.
    pub fn render_and_reset<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<String> {
        let registry = Registry::new();

        for counter in counters {
            let raw_name = if counter.name().is_empty() {
                "unnamed"
            } else {
                counter.name()
            };

            let full_name = self.build_full_name(raw_name);
            let config = self.metric_configs.get(raw_name);
            let metric_type = config.map(|c| c.metric_type).unwrap_or(self.default_type);
            let help = config
                .and_then(|c| c.help.clone())
                .unwrap_or_else(|| format!("{} metric", raw_name));

            let mut labels = self.const_labels.clone();
            if let Some(cfg) = config {
                labels.extend(cfg.labels.clone());
            }

            // Get value and reset
            let value = counter.value_and_reset();

            match metric_type {
                MetricType::Counter => {
                    self.register_counter(&registry, &full_name, &help, &labels, value)?;
                }
                MetricType::Gauge => {
                    self.register_gauge(&registry, &full_name, &help, &labels, value)?;
                }
            }
        }

        self.encode_registry(&registry)
    }

    /// Renders counters to bytes (useful for HTTP responses).
    ///
    /// # Errors
    ///
    /// Returns an error if metric creation, registration, or encoding fails.
    pub fn render_bytes<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<Vec<u8>> {
        Ok(self.render(counters)?.into_bytes())
    }

    /// Encodes the registry to a string.
    fn encode_registry(&self, registry: &Registry) -> Result<String> {
        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| PrometheusError::EncodeError(e.to_string()))?;
        String::from_utf8(buffer).map_err(PrometheusError::from)
    }

    /// Registers a counter metric with the given value.
    fn register_counter(
        &self,
        registry: &Registry,
        name: &str,
        help: &str,
        labels: &HashMap<String, String>,
        value: CounterValue,
    ) -> Result<()> {
        let val = match value {
            CounterValue::Unsigned(v) => v,
            CounterValue::Signed(v) => v.max(0) as u64, // Counters can't be negative
        };

        if labels.is_empty() {
            let counter = IntCounter::new(name, help)?;
            counter.inc_by(val);
            registry.register(Box::new(counter))?;
        } else {
            let label_names: Vec<&str> = labels.keys().map(|s| s.as_str()).collect();
            let counter =
                prometheus::IntCounterVec::new(prometheus::Opts::new(name, help), &label_names)?;
            let label_values: Vec<&str> = labels.values().map(|s| s.as_str()).collect();
            counter.with_label_values(&label_values).inc_by(val);
            registry.register(Box::new(counter))?;
        }
        Ok(())
    }

    /// Registers a gauge metric with the given value.
    fn register_gauge(
        &self,
        registry: &Registry,
        name: &str,
        help: &str,
        labels: &HashMap<String, String>,
        value: CounterValue,
    ) -> Result<()> {
        let val = match value {
            CounterValue::Unsigned(v) => v as i64,
            CounterValue::Signed(v) => v,
        };

        if labels.is_empty() {
            let gauge = IntGauge::new(name, help)?;
            gauge.set(val);
            registry.register(Box::new(gauge))?;
        } else {
            let label_names: Vec<&str> = labels.keys().map(|s| s.as_str()).collect();
            let gauge =
                prometheus::IntGaugeVec::new(prometheus::Opts::new(name, help), &label_names)?;
            let label_values: Vec<&str> = labels.values().map(|s| s.as_str()).collect();
            gauge.with_label_values(&label_values).set(val);
            registry.register(Box::new(gauge))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contatori::average::Average;
    use crate::contatori::maximum::Maximum;
    use crate::contatori::minimum::Minimum;
    use crate::contatori::signed::Signed;
    use crate::contatori::unsigned::Unsigned;

    #[test]
    fn test_render_empty() {
        let observer = PrometheusObserver::new();
        let counters: Vec<&dyn Observable> = vec![];
        let output = observer.render(counters.into_iter()).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn test_render_single_counter() {
        let counter = Unsigned::new().with_name("test_counter");
        counter.add(42);

        let observer = PrometheusObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("test_counter 42"));
    }

    #[test]
    fn test_render_multiple_counters() {
        let counter1 = Unsigned::new().with_name("counter_one");
        let counter2 = Unsigned::new().with_name("counter_two");
        counter1.add(10);
        counter2.add(20);

        let observer = PrometheusObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter1, &counter2];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("counter_one 10"));
        assert!(output.contains("counter_two 20"));
    }

    #[test]
    fn test_render_with_namespace() {
        let counter = Unsigned::new().with_name("requests");
        counter.add(100);

        let observer = PrometheusObserver::new().with_namespace("myapp");
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("myapp_requests 100"));
    }

    #[test]
    fn test_render_with_namespace_and_subsystem() {
        let counter = Unsigned::new().with_name("requests");
        counter.add(100);

        let observer = PrometheusObserver::new()
            .with_namespace("myapp")
            .with_subsystem("http");
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("myapp_http_requests 100"));
    }

    #[test]
    fn test_render_with_help() {
        let counter = Unsigned::new().with_name("http_requests");
        counter.add(50);

        let observer =
            PrometheusObserver::new().with_help("http_requests", "Total HTTP requests received");
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("# HELP http_requests Total HTTP requests received"));
        assert!(output.contains("# TYPE http_requests counter"));
        assert!(output.contains("http_requests 50"));
    }

    #[test]
    fn test_render_with_type_gauge() {
        let counter = Signed::new().with_name("temperature");
        counter.add(25);

        let observer = PrometheusObserver::new().with_type("temperature", MetricType::Gauge);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("# TYPE temperature gauge"));
        assert!(output.contains("temperature 25"));
    }

    #[test]
    fn test_render_with_const_labels() {
        let counter = Unsigned::new().with_name("requests");
        counter.add(100);

        let observer =
            PrometheusObserver::new().with_const_label("instance", "localhost:8080");
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("requests{instance=\"localhost:8080\"} 100"));
    }

    #[test]
    fn test_render_signed_counter() {
        let counter = Signed::new().with_name("signed_metric");
        counter.sub(50);

        let observer = PrometheusObserver::new().with_type("signed_metric", MetricType::Gauge);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("signed_metric -50"));
    }

    #[test]
    fn test_render_and_reset() {
        let counter = Unsigned::new().with_name("resettable");
        counter.add(100);

        let observer = PrometheusObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];

        let output1 = observer.render_and_reset(counters.into_iter()).unwrap();
        assert!(output1.contains("resettable 100"));

        let counters: Vec<&dyn Observable> = vec![&counter];
        let output2 = observer.render(counters.into_iter()).unwrap();
        assert!(output2.contains("resettable 0"));
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(
            PrometheusObserver::sanitize_name("valid_name"),
            "valid_name"
        );
        assert_eq!(
            PrometheusObserver::sanitize_name("with-dash"),
            "with_dash"
        );
        assert_eq!(PrometheusObserver::sanitize_name("with.dot"), "with_dot");
        assert_eq!(
            PrometheusObserver::sanitize_name("with space"),
            "with_space"
        );
        assert_eq!(PrometheusObserver::sanitize_name(""), "unnamed");
        assert_eq!(
            PrometheusObserver::sanitize_name("123starts"),
            "_123starts"
        );
    }

    #[test]
    fn test_unnamed_counter() {
        let counter = Unsigned::new(); // No name
        counter.add(42);

        let observer = PrometheusObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("unnamed 42"));
    }

    #[test]
    fn test_all_counter_types() {
        let unsigned = Unsigned::new().with_name("unsigned_metric");
        let signed = Signed::new().with_name("signed_metric");
        let minimum = Minimum::new().with_name("min_metric");
        let maximum = Maximum::new().with_name("max_metric");
        let average = Average::new().with_name("avg_metric");

        unsigned.add(100);
        signed.sub(50);
        minimum.observe(25);
        maximum.observe(200);
        average.observe(100);
        average.observe(200);

        let counters: Vec<&dyn Observable> =
            vec![&unsigned, &signed, &minimum, &maximum, &average];

        let observer = PrometheusObserver::new()
            .with_type("unsigned_metric", MetricType::Counter)
            .with_type("signed_metric", MetricType::Gauge)
            .with_type("min_metric", MetricType::Gauge)
            .with_type("max_metric", MetricType::Gauge)
            .with_type("avg_metric", MetricType::Gauge);

        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("unsigned_metric 100"));
        assert!(output.contains("signed_metric -50"));
        assert!(output.contains("min_metric 25"));
        assert!(output.contains("max_metric 200"));
        assert!(output.contains("avg_metric 150"));
    }

    #[test]
    fn test_render_bytes() {
        let counter = Unsigned::new().with_name("bytes_test");
        counter.add(42);

        let observer = PrometheusObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let bytes = observer.render_bytes(counters.into_iter()).unwrap();

        let output = String::from_utf8(bytes).unwrap();
        assert!(output.contains("bytes_test 42"));
    }

    #[test]
    fn test_full_prometheus_format() {
        let requests = Unsigned::new().with_name("http_requests_total");
        let latency = Average::new().with_name("http_request_duration_seconds");

        requests.add(1234);
        latency.observe(100);
        latency.observe(200);

        let observer = PrometheusObserver::new()
            .with_namespace("myapp")
            .with_const_label("instance", "localhost:8080")
            .with_type("http_requests_total", MetricType::Counter)
            .with_help("http_requests_total", "Total HTTP requests")
            .with_type("http_request_duration_seconds", MetricType::Gauge)
            .with_help("http_request_duration_seconds", "HTTP request latency");

        let counters: Vec<&dyn Observable> = vec![&requests, &latency];
        let output = observer.render(counters.into_iter()).unwrap();

        // Check structure
        assert!(output.contains("# HELP myapp_http_requests_total Total HTTP requests"));
        assert!(output.contains("# TYPE myapp_http_requests_total counter"));
        assert!(output.contains("myapp_http_requests_total{instance=\"localhost:8080\"} 1234"));
    }

    #[test]
    fn test_metric_type_default() {
        let default_type: MetricType = Default::default();
        assert_eq!(default_type, MetricType::Counter);
    }

    #[test]
    fn test_with_default_type() {
        let counter = Unsigned::new().with_name("test");
        counter.add(10);

        let observer = PrometheusObserver::new().with_default_type(MetricType::Gauge);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("# TYPE test gauge"));
    }

    #[test]
    fn test_with_custom_registry() {
        let registry = Registry::new();
        let observer = PrometheusObserver::with_registry(registry);

        let counter = Unsigned::new().with_name("custom_registry_test");
        counter.add(42);

        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("custom_registry_test 42"));
    }

    #[test]
    fn test_error_display() {
        let err = PrometheusError::MetricError("test error".to_string());
        assert_eq!(format!("{}", err), "metric error: test error");

        let err = PrometheusError::EncodeError("encode failed".to_string());
        assert_eq!(format!("{}", err), "encode error: encode failed");
    }

    #[test]
    fn test_negative_counter_clamped_to_zero() {
        // When a signed counter with negative value is used as a Counter type,
        // the value should be clamped to 0
        let counter = Signed::new().with_name("negative_counter");
        counter.sub(100); // -100

        let observer = PrometheusObserver::new().with_type("negative_counter", MetricType::Counter);
        let counters: Vec<&dyn Observable> = vec![&counter];
        let output = observer.render(counters.into_iter()).unwrap();

        // Counter can't be negative, so it should be 0
        assert!(output.contains("negative_counter 0"));
    }
}