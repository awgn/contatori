//! Prometheus observer and native [`Collector`] implementation.
//!
//! Requires the `prometheus` feature.
//!
//! Two complementary workflows are supported:
//!
//! 1. **Register-once [`ContatoriCollector`]** (recommended for production HTTP servers):
//!    call [`PrometheusObserver::register()`] (or [`PrometheusObserver::collector()`] to
//!    register into an existing [`Registry`]). Metrics are read dynamically on every
//!    `registry.gather()` / [`PrometheusObserver::render_registered()`] call without
//!    re-registering descriptors.
//! 2. **Ad-hoc [`PrometheusObserver::render()`]**: pass any iterator of `&dyn Observable`
//!    (including non-`'static` local counters) to render them alongside any metrics
//!    already registered in `self.registry`.

use crate::counters::{CounterValue, MetricKind, Observable, ObservableEntry};
use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{
    Counter, CounterVec, Encoder, Gauge, GaugeVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::{PrometheusError, Result};

/// Prometheus metric type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MetricType {
    /// A cumulative metric that only ever increases.
    #[default]
    Counter,
    /// A metric that can go up and down.
    Gauge,
}

/// Per-metric configuration for [`PrometheusObserver`].
#[derive(Debug, Clone, Default)]
pub struct MetricConfig {
    /// Explicit metric type override (`None` = auto-detect from [`Observable::metric_kind()`]).
    pub metric_type: Option<MetricType>,
    /// `# HELP` description text.
    pub help: Option<String>,
    /// Additional constant labels specific to this metric.
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct RenderOptions {
    namespace: Option<String>,
    subsystem: Option<String>,
    const_labels: HashMap<String, String>,
    metric_configs: HashMap<String, MetricConfig>,
}

impl RenderOptions {
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
        if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            result.insert(0, '_');
        }
        result
    }

    fn build_full_name(&self, name: &str) -> String {
        let sanitized = Self::sanitize_name(name);
        match (&self.namespace, &self.subsystem) {
            (Some(ns), Some(ss)) => format!("{ns}_{ss}_{sanitized}"),
            (Some(ns), None) => format!("{ns}_{sanitized}"),
            (None, Some(ss)) => format!("{ss}_{sanitized}"),
            (None, None) => sanitized,
        }
    }

    fn resolve_group_meta(
        &self,
        raw_name: &str,
        entries: &[ObservableEntry],
    ) -> (String, String, MetricType, Vec<String>) {
        let full_name = self.build_full_name(raw_name);
        let config = self.metric_configs.get(raw_name);

        let first_kind = entries
            .first()
            .map(|e| e.metric_kind)
            .unwrap_or(MetricKind::Gauge);

        let metric_type = config
            .and_then(|c| c.metric_type)
            .unwrap_or(match first_kind {
                MetricKind::Counter => MetricType::Counter,
                MetricKind::Gauge | MetricKind::Histogram => MetricType::Gauge,
            });

        let help = config
            .and_then(|c| c.help.clone())
            .unwrap_or_else(|| format!("{raw_name} metric"));

        let mut all_label_keys: Vec<String> = Vec::new();
        for key in self.const_labels.keys() {
            if !all_label_keys.contains(key) {
                all_label_keys.push(key.clone());
            }
        }
        if let Some(cfg) = config {
            for key in cfg.labels.keys() {
                if !all_label_keys.contains(key) {
                    all_label_keys.push(key.clone());
                }
            }
        }
        for entry in entries {
            for &(k, _) in entry.labels {
                if !all_label_keys.iter().any(|existing| existing == k) {
                    all_label_keys.push(k.to_string());
                }
            }
        }
        all_label_keys.sort();

        (full_name, help, metric_type, all_label_keys)
    }

    fn group_entries(entries: Vec<ObservableEntry>) -> BTreeMap<String, Vec<ObservableEntry>> {
        let mut grouped: BTreeMap<String, Vec<ObservableEntry>> = BTreeMap::new();
        for entry in entries {
            let raw_name = if entry.name.is_empty() {
                "unnamed".to_string()
            } else {
                entry.name.to_string()
            };
            grouped.entry(raw_name).or_default().push(entry);
        }
        grouped
    }

    fn build_descs(&self, entries: Vec<ObservableEntry>) -> Result<Vec<Desc>> {
        let grouped = Self::group_entries(entries);
        let mut descs = Vec::with_capacity(grouped.len());

        for (raw_name, group_entries) in grouped {
            let (full_name, help, _, all_label_keys) =
                self.resolve_group_meta(&raw_name, &group_entries);
            let desc = Desc::new(full_name, help, all_label_keys, HashMap::new())
                .map_err(PrometheusError::from)?;
            descs.push(desc);
        }

        Ok(descs)
    }

    fn build_metric_families(&self, entries: Vec<ObservableEntry>) -> Result<Vec<MetricFamily>> {
        let grouped = Self::group_entries(entries);
        let mut families = Vec::with_capacity(grouped.len());

        for (raw_name, group_entries) in grouped {
            let (full_name, help, metric_type, all_label_keys) =
                self.resolve_group_meta(&raw_name, &group_entries);
            let config = self.metric_configs.get(&raw_name);

            // Filter to entries that actually have an observed value
            let observed: Vec<(&ObservableEntry, CounterValue)> = group_entries
                .iter()
                .filter_map(|e| e.value.map(|v| (e, v)))
                .collect();

            if observed.is_empty() {
                continue;
            }

            let has_float = observed
                .iter()
                .any(|(_, v)| matches!(v, CounterValue::Float(_)));

            if all_label_keys.is_empty() {
                let value = observed[0].1;
                match metric_type {
                    MetricType::Counter => {
                        if has_float {
                            let c = Counter::new(&full_name, &help)?;
                            c.inc_by(value.as_f64().max(0.0));
                            families.extend(c.collect());
                        } else {
                            let val = match value {
                                CounterValue::Unsigned(v) => v,
                                CounterValue::Signed(v) => v.max(0) as u64,
                                CounterValue::Float(v) => v.max(0.0) as u64,
                            };
                            let c = IntCounter::new(&full_name, &help)?;
                            c.inc_by(val);
                            families.extend(c.collect());
                        }
                    }
                    MetricType::Gauge => {
                        if has_float {
                            let g = Gauge::new(&full_name, &help)?;
                            g.set(value.as_f64());
                            families.extend(g.collect());
                        } else {
                            let g = IntGauge::new(&full_name, &help)?;
                            g.set(value.as_i64());
                            families.extend(g.collect());
                        }
                    }
                }
            } else {
                let label_names: Vec<&str> = all_label_keys.iter().map(String::as_str).collect();
                let opts = Opts::new(&full_name, &help);

                match metric_type {
                    MetricType::Counter => {
                        if has_float {
                            let vec = CounterVec::new(opts, &label_names)?;
                            for (entry, value) in &observed {
                                let values =
                                    self.build_label_values(&all_label_keys, config, entry.labels);
                                let refs: Vec<&str> = values.iter().map(String::as_str).collect();
                                vec.with_label_values(&refs)
                                    .inc_by(value.as_f64().max(0.0));
                            }
                            families.extend(vec.collect());
                        } else {
                            let vec = IntCounterVec::new(opts, &label_names)?;
                            for (entry, value) in &observed {
                                let values =
                                    self.build_label_values(&all_label_keys, config, entry.labels);
                                let refs: Vec<&str> = values.iter().map(String::as_str).collect();
                                let val = match *value {
                                    CounterValue::Unsigned(v) => v,
                                    CounterValue::Signed(v) => v.max(0) as u64,
                                    CounterValue::Float(v) => v.max(0.0) as u64,
                                };
                                vec.with_label_values(&refs).inc_by(val);
                            }
                            families.extend(vec.collect());
                        }
                    }
                    MetricType::Gauge => {
                        if has_float {
                            let vec = GaugeVec::new(opts, &label_names)?;
                            for (entry, value) in &observed {
                                let values =
                                    self.build_label_values(&all_label_keys, config, entry.labels);
                                let refs: Vec<&str> = values.iter().map(String::as_str).collect();
                                vec.with_label_values(&refs).set(value.as_f64());
                            }
                            families.extend(vec.collect());
                        } else {
                            let vec = IntGaugeVec::new(opts, &label_names)?;
                            for (entry, value) in &observed {
                                let values =
                                    self.build_label_values(&all_label_keys, config, entry.labels);
                                let refs: Vec<&str> = values.iter().map(String::as_str).collect();
                                vec.with_label_values(&refs).set(value.as_i64());
                            }
                            families.extend(vec.collect());
                        }
                    }
                }
            }
        }

        Ok(families)
    }

    fn build_label_values(
        &self,
        sorted_keys: &[String],
        config: Option<&MetricConfig>,
        entry_labels: &[(&str, &str)],
    ) -> Vec<String> {
        sorted_keys
            .iter()
            .map(|key| {
                if let Some(&(_, v)) = entry_labels.iter().find(|&&(k, _)| k == key) {
                    return v.to_string();
                }
                if let Some(cfg) = config
                    && let Some(v) = cfg.labels.get(key)
                {
                    return v.clone();
                }
                if let Some(v) = self.const_labels.get(key) {
                    return v.clone();
                }
                String::new()
            })
            .collect()
    }
}

/// A native [`prometheus::core::Collector`] that reads `'static` `contatori` counters
/// on demand whenever `Registry::gather()` is invoked.
pub struct ContatoriCollector {
    counters: Vec<&'static dyn Observable>,
    options: Arc<RenderOptions>,
    descs: Vec<Desc>,
}

impl ContatoriCollector {
    fn new(
        counters: &[&'static dyn Observable],
        options: Arc<RenderOptions>,
    ) -> Result<Self> {
        let mut entries = Vec::with_capacity(counters.len());
        for &counter in counters {
            counter.expand_into(&mut entries);
        }
        let descs = options.build_descs(entries)?;
        Ok(Self {
            counters: counters.to_vec(),
            options,
            descs,
        })
    }
}

impl Collector for ContatoriCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.descs.iter().collect()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        let mut entries = Vec::with_capacity(self.counters.len());
        for &counter in &self.counters {
            counter.expand_into(&mut entries);
        }
        self.options
            .build_metric_families(entries)
            .unwrap_or_default()
    }
}

/// Observer that exports `contatori` counters in Prometheus exposition format.
pub struct PrometheusObserver {
    registry: Registry,
    options: RenderOptions,
}

impl Default for PrometheusObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl PrometheusObserver {
    /// Creates a new `PrometheusObserver` backed by a fresh [`Registry`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            options: RenderOptions::default(),
        }
    }

    /// Creates a `PrometheusObserver` backed by an existing [`Registry`].
    ///
    /// Metrics already registered in `registry` will be included alongside
    /// `contatori` metrics when calling [`render()`](Self::render) or
    /// [`render_registered()`](Self::render_registered), and [`register()`](Self::register)
    /// will install collectors directly into `registry`.
    #[must_use]
    pub fn with_registry(registry: Registry) -> Self {
        Self {
            registry,
            options: RenderOptions::default(),
        }
    }

    /// Returns a reference to the underlying Prometheus [`Registry`].
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Sets the namespace (prefix) prepended to all `contatori` metric names.
    #[must_use]
    pub fn with_namespace(mut self, namespace: &str) -> Self {
        self.options.namespace = Some(namespace.to_string());
        self
    }

    /// Sets the subsystem placed between `namespace` and the metric name.
    #[must_use]
    pub fn with_subsystem(mut self, subsystem: &str) -> Self {
        self.options.subsystem = Some(subsystem.to_string());
        self
    }

    /// Adds a constant `(name, value)` label applied to all `contatori` metrics.
    #[must_use]
    pub fn with_const_label(mut self, name: &str, value: &str) -> Self {
        self.options
            .const_labels
            .insert(name.to_string(), value.to_string());
        self
    }

    /// Configures a specific metric by raw name.
    #[must_use]
    pub fn with_metric_config(mut self, name: &str, config: MetricConfig) -> Self {
        self.options.metric_configs.insert(name.to_string(), config);
        self
    }

    /// Overrides the Prometheus [`MetricType`] for a specific metric.
    #[must_use]
    pub fn with_type(mut self, name: &str, metric_type: MetricType) -> Self {
        self.options
            .metric_configs
            .entry(name.to_string())
            .or_default()
            .metric_type = Some(metric_type);
        self
    }

    /// Sets the `# HELP` description for a specific metric.
    #[must_use]
    pub fn with_help(mut self, name: &str, help: &str) -> Self {
        self.options
            .metric_configs
            .entry(name.to_string())
            .or_default()
            .help = Some(help.to_string());
        self
    }

    /// Builds a [`ContatoriCollector`] for the given `'static` counters using this
    /// observer's current namespace, subsystem, labels, and per-metric configuration.
    ///
    /// You can register the returned collector into any [`prometheus::Registry`].
    pub fn collector(&self, counters: &[&'static dyn Observable]) -> Result<ContatoriCollector> {
        ContatoriCollector::new(counters, Arc::new(self.options.clone()))
    }

    /// Registers `'static` counters once into `self.registry` via a [`ContatoriCollector`].
    ///
    /// Subsequent scrapes via [`render_registered()`](Self::render_registered) or
    /// `self.registry().gather()` will dynamically read the sharded counters without
    /// re-creating descriptors.
    pub fn register(&self, counters: &[&'static dyn Observable]) -> Result<()> {
        if counters.is_empty() {
            return Ok(());
        }
        let collector = self.collector(counters)?;
        self.registry.register(Box::new(collector))?;
        Ok(())
    }

    /// Encodes all metrics registered in `self.registry` into Prometheus text exposition format.
    pub fn render_registered(&self) -> Result<String> {
        self.render(std::iter::empty())
    }

    /// Renders both `self.registry` metrics and any ad-hoc `counters` to Prometheus text format.
    pub fn render<'a>(&self, counters: impl Iterator<Item = &'a dyn Observable>) -> Result<String> {
        let bytes = self.render_bytes(counters)?;
        Ok(String::from_utf8(bytes)?)
    }

    /// Renders both `self.registry` metrics and any ad-hoc `counters` to UTF-8 bytes.
    pub fn render_bytes<'a>(
        &self,
        counters: impl Iterator<Item = &'a dyn Observable>,
    ) -> Result<Vec<u8>> {
        let mut families = self.registry.gather();

        let mut ad_hoc_entries = Vec::new();
        for counter in counters {
            counter.expand_into(&mut ad_hoc_entries);
        }
        if !ad_hoc_entries.is_empty() {
            let mut extra = self.options.build_metric_families(ad_hoc_entries)?;
            families.append(&mut extra);
            families.sort_by(|a, b| a.name().cmp(b.name()));
        }

        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder
            .encode(&families, &mut buffer)
            .map_err(|e| PrometheusError::EncodeError(e.to_string()))?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::average::Average;
    use crate::counters::minimum::Minimum;
    use crate::counters::monotone::Monotone;
    use crate::counters::rate::Rate;
    use crate::counters::signed::Signed;
    use crate::counters::unsigned::Unsigned;
    use crate::labeled_group;

    #[test]
    fn test_collector_register_once_and_scrape_multiple_times() {
        static REQS: Monotone = Monotone::new().with_name("collector_reqs_total");
        static MIN_LAT: Minimum = Minimum::new().with_name("collector_min_latency");

        let observer = PrometheusObserver::new()
            .with_namespace("svc")
            .with_help("collector_reqs_total", "Total requests");

        observer.register(&[&REQS, &MIN_LAT]).unwrap();

        // First scrape: MIN_LAT is unobserved, so it must be omitted!
        REQS.add(10);
        let out1 = observer.render_registered().unwrap();
        assert!(out1.contains("# TYPE svc_collector_reqs_total counter"));
        assert!(out1.contains("svc_collector_reqs_total 10"));
        assert!(!out1.contains("collector_min_latency"));

        // Second scrape: REQS increases and MIN_LAT now has an observation
        REQS.add(5);
        MIN_LAT.observe(42);
        let out2 = observer.render_registered().unwrap();
        assert!(out2.contains("svc_collector_reqs_total 15"));
        assert!(out2.contains("svc_collector_min_latency 42"));
    }

    #[test]
    fn test_with_registry_combines_existing_prometheus_metrics() {
        let app_registry = Registry::new();
        let native_counter = IntCounter::new("native_jobs_total", "Native counter").unwrap();
        native_counter.inc_by(7);
        app_registry.register(Box::new(native_counter)).unwrap();

        let contatori_gauge = Signed::new().with_name("active_workers");
        contatori_gauge.add(4);

        let observer = PrometheusObserver::with_registry(app_registry);
        let counters: Vec<&dyn Observable> = vec![&contatori_gauge];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("native_jobs_total 7"));
        assert!(output.contains("active_workers 4"));
    }

    #[test]
    fn test_labeled_group_no_double_counting_and_multi_label() {
        labeled_group!(
            HttpResponses,
            "http_responses_total",
            ["method", "status"],
            get_200: ["GET", "200"]: Monotone,
            post_201: ["POST", "201"]: Monotone,
        );

        let resp = HttpResponses::new();
        resp.get_200.add(100);
        resp.post_201.add(50);

        let observer = PrometheusObserver::new().with_const_label("env", "prod");
        let counters: Vec<&dyn Observable> = vec![&resp];
        let output = observer.render(counters.into_iter()).unwrap();

        assert!(output.contains("# TYPE http_responses_total counter"));
        assert!(output.contains(
            "http_responses_total{env=\"prod\",method=\"GET\",status=\"200\"} 100"
        ));
        assert!(output.contains(
            "http_responses_total{env=\"prod\",method=\"POST\",status=\"201\"} 50"
        ));
        // Ensure no empty-label series exists
        assert!(!output.contains("method=\"\""));
    }

    #[test]
    fn test_float_average_and_rate() {
        let avg = Average::new().with_name("latency_ms");
        avg.observe(1);
        avg.observe(2);

        let rate = Rate::new().with_name("rps");
        let _ = rate.rate();

        let counters: Vec<&dyn Observable> = vec![&avg, &rate];
        let out = PrometheusObserver::new()
            .render(counters.into_iter())
            .unwrap();
        assert!(out.contains("latency_ms 1.5"));
        assert!(out.contains("rps 0"));
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(RenderOptions::sanitize_name("with-dash.and space"), "with_dash_and_space");
        assert_eq!(RenderOptions::sanitize_name(""), "unnamed");
        assert_eq!(RenderOptions::sanitize_name("9lives"), "_9lives");
    }

    #[test]
    fn test_ad_hoc_and_unnamed() {
        let unnamed = Unsigned::new();
        unnamed.add(9);
        let counters: Vec<&dyn Observable> = vec![&unnamed];
        let out = PrometheusObserver::new()
            .render(counters.into_iter())
            .unwrap();
        assert!(out.contains("unnamed 9"));
    }
}
