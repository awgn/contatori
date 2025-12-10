//! OpenTelemetry observer for exporting counters via OTLP.
//!
//! This module provides [`OtelObserver`], which registers contatori counters
//! with OpenTelemetry's MeterProvider using observable instruments (callbacks).
//!
//! # Feature Flag
//!
//! This module requires the `opentelemetry` feature:
//!
//! ```toml
//! [dependencies]
//! contatori = { version = "0.6", features = ["opentelemetry"] }
//! ```
//!
//! # How It Works
//!
//! Unlike push-based approaches, this observer uses OpenTelemetry's observable
//! instruments which are read via callbacks during metric collection. This
//! integrates naturally with any OpenTelemetry exporter (OTLP, Prometheus, etc.)
//!
//! # Example
//!
//! ```rust,ignore
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::observers::opentelemetry::OtelObserver;
//!
//! static REQUESTS: Unsigned = Unsigned::new().with_name("http_requests_total");
//! static ERRORS: Unsigned = Unsigned::new().with_name("http_errors_total");
//!
//! fn main() -> contatori::observers::Result<()> {
//!     // Setup OpenTelemetry MeterProvider first (see examples)
//!     
//!     let observer = OtelObserver::new("myapp");
//!     observer.register(&[&REQUESTS, &ERRORS])?;
//!
//!     // Counters are now automatically exported by the MeterProvider
//!     REQUESTS.add(1);
//!     
//!     Ok(())
//! }
//! ```

use crate::counters::{MetricKind, Observable, ObservableEntry};
use opentelemetry::{global, metrics::Meter, KeyValue};

use super::{OtelError, Result};

/// Observer that exports counters to OpenTelemetry using observable instruments.
///
/// This observer registers contatori counters with OpenTelemetry's MeterProvider,
/// using callbacks that read counter values during metric collection.
///
/// # Static Counters
///
/// Counters must be `'static` (typically declared as `static` globals) because
/// OpenTelemetry callbacks need to hold references for the lifetime of the program.
///
/// # Example
///
/// ```rust,ignore
/// use contatori::counters::unsigned::Unsigned;
/// use contatori::counters::monotone::Monotone;
/// use contatori::counters::Observable;
/// use contatori::observers::opentelemetry::OtelObserver;
///
/// static REQUESTS: Monotone = Monotone::new().with_name("http_requests_total");
/// static CONNECTIONS: Unsigned = Unsigned::new().with_name("active_connections");
///
/// let observer = OtelObserver::new("myapp")
///     .with_description_prefix("My Application");
///
/// observer.register(&[&REQUESTS, &CONNECTIONS])?;
/// ```
pub struct OtelObserver {
    meter: Meter,
    description_prefix: Option<String>,
}

impl OtelObserver {
    /// Creates a new OpenTelemetry observer with the given meter name.
    ///
    /// The meter name is typically the application or library name.
    /// It will be used to create a meter from the global MeterProvider.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let observer = OtelObserver::new("myapp");
    /// ```
    pub fn new(meter_name: &'static str) -> Self {
        Self {
            meter: global::meter(meter_name),
            description_prefix: None,
        }
    }

    /// Creates an observer with a specific meter instance.
    ///
    /// Use this when you need more control over the meter configuration,
    /// or when you want to use a meter from a specific MeterProvider.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let meter = my_meter_provider.meter("myapp");
    /// let observer = OtelObserver::with_meter(meter);
    /// ```
    pub fn with_meter(meter: Meter) -> Self {
        Self {
            meter,
            description_prefix: None,
        }
    }

    /// Sets a description prefix for all registered metrics.
    ///
    /// The prefix will be prepended to each metric's description.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let observer = OtelObserver::new("myapp")
    ///     .with_description_prefix("My Application");
    /// // Metric "requests" will have description "My Application: requests"
    /// ```
    pub fn with_description_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.description_prefix = Some(prefix.into());
        self
    }

    /// Builds the description string for a metric.
    fn build_description(&self, name: &str) -> String {
        match &self.description_prefix {
            Some(prefix) => format!("{}: {}", prefix, name),
            None => format!("{} metric", name),
        }
    }

    /// Registers all counters with OpenTelemetry.
    ///
    /// Each counter is registered as an observable instrument based on its
    /// [`metric_kind()`](Observable::metric_kind):
    ///
    /// - [`MetricKind::Counter`] → `ObservableCounter` (monotonically increasing)
    /// - [`MetricKind::Gauge`] → `ObservableGauge` (can go up or down)
    /// - [`MetricKind::Histogram`] → `ObservableGauge` (treated as gauge)
    ///
    /// For labeled groups, the labels from [`expand()`](Observable::expand)
    /// are automatically converted to OpenTelemetry attributes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use contatori::counters::unsigned::Unsigned;
    /// use contatori::counters::monotone::Monotone;
    /// use contatori::counters::average::Average;
    /// use contatori::counters::Observable;
    /// use contatori::observers::opentelemetry::OtelObserver;
    ///
    /// static REQUESTS: Monotone = Monotone::new().with_name("requests_total");
    /// static ERRORS: Monotone = Monotone::new().with_name("errors_total");
    /// static LATENCY: Average = Average::new().with_name("latency_ms");
    ///
    /// let observer = OtelObserver::new("myapp");
    /// observer.register(&[&REQUESTS, &ERRORS, &LATENCY])?;
    /// ```
    pub fn register(&self, counters: &[&'static (dyn Observable + Send + Sync)]) -> Result<()> {
        for &counter in counters {
            self.register_one(counter)?;
        }
        Ok(())
    }

    /// Registers a single counter based on its metric kind.
    fn register_one(&self, counter: &'static (dyn Observable + Send + Sync)) -> Result<()> {
        match counter.metric_kind() {
            MetricKind::Counter => self.register_counter(counter),
            MetricKind::Gauge | MetricKind::Histogram => self.register_gauge(counter),
        }
    }

    /// Registers an observable counter (monotonically increasing).
    fn register_counter(&self, counter: &'static (dyn Observable + Send + Sync)) -> Result<()> {
        let name = counter.name();
        if name.is_empty() {
            return Err(OtelError::MetricError("counter must have a name".into()).into());
        }

        let description = self.build_description(name);

        let _ = self
            .meter
            .u64_observable_counter(name)
            .with_description(description)
            .with_callback(move |observer| {
                for entry in counter.expand() {
                    let attributes = entry_to_attributes(&entry);
                    observer.observe(entry.value.as_u64(), &attributes);
                }
            })
            .build();

        Ok(())
    }

    /// Registers an observable gauge (can go up or down).
    fn register_gauge(&self, counter: &'static (dyn Observable + Send + Sync)) -> Result<()> {
        let name = counter.name();
        if name.is_empty() {
            return Err(OtelError::MetricError("counter must have a name".into()).into());
        }

        let description = self.build_description(name);

        // Use f64 gauge to support all value types (unsigned, signed, float)
        let _ = self
            .meter
            .f64_observable_gauge(name)
            .with_description(description)
            .with_callback(move |observer| {
                for entry in counter.expand() {
                    let attributes = entry_to_attributes(&entry);
                    observer.observe(entry.value.as_f64(), &attributes);
                }
            })
            .build();

        Ok(())
    }
}

/// Converts an [`ObservableEntry`]'s label to OpenTelemetry [`KeyValue`] attributes.
fn entry_to_attributes(entry: &ObservableEntry) -> Vec<KeyValue> {
    match &entry.label {
        Some((key, value)) => vec![KeyValue::new(*key, *value)],
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::unsigned::Unsigned;

    #[test]
    fn test_otel_observer_new() {
        let observer = OtelObserver::new("test");
        assert!(observer.description_prefix.is_none());
    }

    #[test]
    fn test_otel_observer_with_description_prefix() {
        let observer = OtelObserver::new("test").with_description_prefix("My App");
        assert_eq!(observer.description_prefix, Some("My App".to_string()));
    }

    #[test]
    fn test_build_description_with_prefix() {
        let observer = OtelObserver::new("test").with_description_prefix("My App");
        assert_eq!(observer.build_description("requests"), "My App: requests");
    }

    #[test]
    fn test_build_description_without_prefix() {
        let observer = OtelObserver::new("test");
        assert_eq!(observer.build_description("requests"), "requests metric");
    }

    #[test]
    fn test_entry_to_attributes_with_label() {
        let entry = ObservableEntry {
            name: "test",
            label: Some(("method", "GET")),
            value: crate::counters::CounterValue::Unsigned(1),
            metric_kind: MetricKind::Counter,
        };
        let attrs = entry_to_attributes(&entry);
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].key.as_str(), "method");
    }

    #[test]
    fn test_entry_to_attributes_without_label() {
        let entry = ObservableEntry {
            name: "test",
            label: None,
            value: crate::counters::CounterValue::Unsigned(1),
            metric_kind: MetricKind::Counter,
        };
        let attrs = entry_to_attributes(&entry);
        assert!(attrs.is_empty());
    }

    #[test]
    fn test_register_unnamed_counter_fails() {
        let observer = OtelObserver::new("test");
        static UNNAMED: Unsigned = Unsigned::new();
        let counters: &[&'static (dyn Observable + Send + Sync)] = &[&UNNAMED];
        let result = observer.register(counters);
        assert!(result.is_err());
    }
}