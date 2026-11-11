//! JSON observer for serializing counters.
//!
//! Requires the `json` feature.

use crate::counters::Observable;
use crate::observers::Result;

pub use crate::snapshot::{CounterSnapshot, MetricsSnapshot};

/// Configuration for [`JsonObserver`].
#[derive(Debug, Clone, Default)]
pub struct JsonConfig {
    /// Whether to pretty-print the JSON output.
    pub pretty: bool,
    /// Whether to include a Unix millisecond timestamp (requires `wrap_in_snapshot`).
    pub include_timestamp: bool,
    /// Whether to wrap the counter array inside a [`MetricsSnapshot`] object.
    pub wrap_in_snapshot: bool,
}

/// An observer that serializes [`Observable`] counters to JSON.
///
/// # Examples
///
/// ```rust
/// use contatori::{Observable, Unsigned};
/// use contatori::observers::json::JsonObserver;
///
/// let requests = Unsigned::new().with_name("http_requests");
/// requests.add(1000);
///
/// let counters: Vec<&dyn Observable> = vec![&requests];
/// let json = JsonObserver::new().render(counters.into_iter()).unwrap();
/// assert!(json.contains("\"http_requests\""));
/// ```
#[derive(Debug, Clone, Default)]
pub struct JsonObserver {
    config: JsonConfig,
}

impl JsonObserver {
    /// Creates a new JSON observer with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new JSON observer from an explicit [`JsonConfig`].
    #[must_use]
    pub fn with_config(config: JsonConfig) -> Self {
        Self { config }
    }

    /// Enables or disables pretty-printed JSON output.
    #[must_use]
    pub fn pretty(mut self, enabled: bool) -> Self {
        self.config.pretty = enabled;
        self
    }

    /// Enables or disables timestamp inclusion (effective when `wrap_in_snapshot` is `true`).
    #[must_use]
    pub fn include_timestamp(mut self, enabled: bool) -> Self {
        self.config.include_timestamp = enabled;
        self
    }

    /// Enables or disables wrapping the output in a [`MetricsSnapshot`] object.
    #[must_use]
    pub fn wrap_in_snapshot(mut self, enabled: bool) -> Self {
        self.config.wrap_in_snapshot = enabled;
        self
    }

    /// Collects counters into [`CounterSnapshot`] entries (omitting unobserved trackers).
    #[must_use]
    pub fn collect<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Vec<CounterSnapshot> {
        let mut entries = Vec::new();
        for counter in counters {
            counter.expand_into(&mut entries);
        }
        entries
            .into_iter()
            .filter_map(CounterSnapshot::from_entry)
            .collect()
    }

    /// Renders counters to a JSON string using the unified [`observers::Result`](Result).
    pub fn render<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> Result<String> {
        let snapshots = self.collect(counters);

        let out = if self.config.wrap_in_snapshot {
            let snapshot = if self.config.include_timestamp {
                MetricsSnapshot::with_timestamp(snapshots, current_timestamp_ms())
            } else {
                MetricsSnapshot::new(snapshots)
            };

            if self.config.pretty {
                serde_json::to_string_pretty(&snapshot)?
            } else {
                serde_json::to_string(&snapshot)?
            }
        } else if self.config.pretty {
            serde_json::to_string_pretty(&snapshots)?
        } else {
            serde_json::to_string(&snapshots)?
        };

        Ok(out)
    }

    /// Renders counters to a JSON byte vector using the unified [`observers::Result`](Result).
    pub fn render_bytes<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<Vec<u8>> {
        let snapshots = self.collect(counters);

        let bytes = if self.config.wrap_in_snapshot {
            let snapshot = if self.config.include_timestamp {
                MetricsSnapshot::with_timestamp(snapshots, current_timestamp_ms())
            } else {
                MetricsSnapshot::new(snapshots)
            };
            if self.config.pretty {
                serde_json::to_vec_pretty(&snapshot)?
            } else {
                serde_json::to_vec(&snapshot)?
            }
        } else if self.config.pretty {
            serde_json::to_vec_pretty(&snapshots)?
        } else {
            serde_json::to_vec(&snapshots)?
        };

        Ok(bytes)
    }

    /// Alias for [`render()`](Self::render).
    pub fn to_json<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<String> {
        self.render(counters)
    }

    /// Alias for [`render_bytes()`](Self::render_bytes).
    pub fn to_json_bytes<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<Vec<u8>> {
        self.render_bytes(counters)
    }
}

fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::average::Average;
    use crate::counters::maximum::Maximum;
    use crate::counters::minimum::Minimum;
    use crate::counters::signed::Signed;
    use crate::counters::unsigned::Unsigned;
    use crate::counters::CounterValue;

    #[test]
    fn test_render_empty_and_unobserved() {
        let observer = JsonObserver::new();
        let empty_min = Minimum::new().with_name("unobserved_min");
        let counters: Vec<&dyn Observable> = vec![&empty_min];
        assert_eq!(observer.render(counters.into_iter()).unwrap(), "[]");
    }

    #[test]
    fn test_render_all_counter_types() {
        let unsigned = Unsigned::new().with_name("unsigned");
        let signed = Signed::new().with_name("signed");
        let minimum = Minimum::new().with_name("minimum");
        let maximum = Maximum::new().with_name("maximum");
        let average = Average::new().with_name("average");

        unsigned.add(100);
        signed.sub(50);
        minimum.observe(25);
        maximum.observe(200);
        average.observe(1);
        average.observe(2);

        let counters: Vec<&dyn Observable> = vec![&unsigned, &signed, &minimum, &maximum, &average];
        let json = JsonObserver::new()
            .pretty(true)
            .wrap_in_snapshot(true)
            .include_timestamp(true)
            .render(counters.into_iter())
            .unwrap();

        assert!(json.contains("timestamp_ms"));
        assert!(json.contains("1.5"));
        assert!(json.contains("-50"));
    }

    #[test]
    fn test_render_bytes_and_aliases() {
        let counter = Unsigned::new().with_name("bytes_test");
        counter.add(123);

        let observer = JsonObserver::new();
        let counters: Vec<&dyn Observable> = vec![&counter];
        let bytes = observer.render_bytes(counters.iter().copied()).unwrap();
        let s = observer.to_json(counters.into_iter()).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), s);
        assert_eq!(
            serde_json::from_str::<Vec<CounterSnapshot>>(&s).unwrap()[0].value,
            CounterValue::Unsigned(123)
        );
    }
}
