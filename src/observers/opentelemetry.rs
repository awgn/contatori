//! OpenTelemetry observer for exporting counters via observable callbacks.
//!
//! Requires the `opentelemetry` feature.

use crate::counters::{MetricKind, Observable, ObservableEntry};
use opentelemetry::{KeyValue, global, metrics::Meter};

use super::{OtelError, Result};

/// Observer that registers `contatori` counters as OpenTelemetry observable instruments.
///
/// Supports single counters as well as single-label and multi-label
/// [`labeled_group!`](crate::labeled_group) structs, automatically skipping
/// uninitialized statistical trackers (`try_value() == None`).
pub struct OtelObserver {
    meter: Meter,
    description_prefix: Option<String>,
}

impl OtelObserver {
    /// Creates a new OpenTelemetry observer obtaining a `Meter` from the global `MeterProvider`.
    #[must_use]
    pub fn new(meter_name: &'static str) -> Self {
        Self {
            meter: global::meter(meter_name),
            description_prefix: None,
        }
    }

    /// Creates an OpenTelemetry observer using an explicit [`Meter`].
    #[must_use]
    pub fn with_meter(meter: Meter) -> Self {
        Self {
            meter,
            description_prefix: None,
        }
    }

    /// Prepends `prefix` to all generated metric descriptions.
    #[must_use]
    pub fn with_description_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.description_prefix = Some(prefix.into());
        self
    }

    fn build_description(&self, name: &str) -> String {
        match &self.description_prefix {
            Some(prefix) => format!("{prefix}: {name}"),
            None => format!("{name} metric"),
        }
    }

    /// Registers a slice of `'static` observables with OpenTelemetry.
    ///
    /// - [`MetricKind::Counter`] → `u64_observable_counter`
    /// - [`MetricKind::Gauge`] / [`MetricKind::Histogram`] → `f64_observable_gauge`
    pub fn register(&self, counters: &[&'static dyn Observable]) -> Result<()> {
        for &counter in counters {
            self.register_one(counter)?;
        }
        Ok(())
    }

    fn register_one(&self, counter: &'static dyn Observable) -> Result<()> {
        match counter.metric_kind() {
            MetricKind::Counter => self.register_counter(counter),
            MetricKind::Gauge | MetricKind::Histogram => self.register_gauge(counter),
        }
    }

    fn register_counter(&self, counter: &'static dyn Observable) -> Result<()> {
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
                let mut entries = Vec::new();
                counter.expand_into(&mut entries);
                for entry in entries {
                    if let Some(val) = entry.value {
                        let attributes = entry_to_attributes(&entry);
                        observer.observe(val.as_u64(), &attributes);
                    }
                }
            })
            .build();

        Ok(())
    }

    fn register_gauge(&self, counter: &'static dyn Observable) -> Result<()> {
        let name = counter.name();
        if name.is_empty() {
            return Err(OtelError::MetricError("counter must have a name".into()).into());
        }

        let description = self.build_description(name);

        let _ = self
            .meter
            .f64_observable_gauge(name)
            .with_description(description)
            .with_callback(move |observer| {
                let mut entries = Vec::new();
                counter.expand_into(&mut entries);
                for entry in entries {
                    if let Some(val) = entry.value {
                        let attributes = entry_to_attributes(&entry);
                        observer.observe(val.as_f64(), &attributes);
                    }
                }
            })
            .build();

        Ok(())
    }
}

fn entry_to_attributes(entry: &ObservableEntry) -> Vec<KeyValue> {
    entry
        .labels
        .iter()
        .map(|&(key, value)| KeyValue::new(key, value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::CounterValue;
    use crate::counters::monotone::Monotone;
    use crate::counters::unsigned::Unsigned;
    use crate::labeled_group;

    #[test]
    fn test_entry_to_attributes_multi_label() {
        static LABELS: [(&str, &str); 2] = [("method", "GET"), ("status", "200")];
        let entry = ObservableEntry {
            name: "http_responses_total",
            labels: &LABELS,
            value: Some(CounterValue::Unsigned(1)),
            metric_kind: MetricKind::Counter,
        };
        let attrs = entry_to_attributes(&entry);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].key.as_str(), "method");
        assert_eq!(attrs[1].key.as_str(), "status");
    }

    #[test]
    fn test_register_unnamed_counter_fails() {
        let observer = OtelObserver::new("test");
        static UNNAMED: Unsigned = Unsigned::new();
        assert!(observer.register(&[&UNNAMED]).is_err());
    }

    #[test]
    fn test_labeled_group_monotone_registers_as_counter() {
        labeled_group!(
            MonotoneGroup,
            "monotone_group_total",
            "method",
            get: "GET": Monotone,
        );
        static GROUP: MonotoneGroup = MonotoneGroup::new();
        assert_eq!(GROUP.metric_kind(), MetricKind::Counter);

        let observer = OtelObserver::new("test").with_description_prefix("Svc");
        assert!(observer.register(&[&GROUP]).is_ok());
    }
}
