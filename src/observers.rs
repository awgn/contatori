//! Observer implementations for collecting and exporting counter metrics.
//!
//! This module provides various ways to observe and export counter values:
//!
//! - [`table`] - Pretty-print counters as tables using the `tabled` crate
//! - [`json`] - Serialize counters to JSON format
//! - [`prometheus`] - Export counters in Prometheus exposition format or as a `Collector`
//! - [`opentelemetry`] - Export counters via OpenTelemetry
//!
//! # Unified Error Handling
//!
//! Fallible observers share a unified [`ObserverError`] type, allowing you to switch
//! between observers without changing error handling code.
//!
//! # Feature Flags
//!
//! Each observer is gated behind a feature flag to minimize dependencies:
//!
//! - `table` - Enables the [`table`] module
//! - `json` - Enables the [`json`] module
//! - `prometheus` - Enables the [`prometheus`] module
//! - `opentelemetry` - Enables the [`opentelemetry`] module
//! - `full` - Enables all observer modules
//!
//! # Example
//!
//! ```rust
//! use contatori::prelude::*;
//! use contatori::observers::Result;
//!
//! static REQUESTS: Monotone = Monotone::new().with_name("requests_total");
//! static ERRORS: Monotone = Monotone::new().with_name("errors_total");
//!
//! fn export_metrics() -> Result<()> {
//!     let counters: &[&'static dyn Observable] = &[&REQUESTS, &ERRORS];
//!
//!     #[cfg(feature = "json")]
//!     {
//!         use contatori::observers::json::JsonObserver;
//!         let json = JsonObserver::new().render(counters.iter().copied())?;
//!         assert!(!json.is_empty());
//!     }
//!
//!     #[cfg(feature = "prometheus")]
//!     {
//!         use contatori::observers::prometheus::PrometheusObserver;
//!         let output = PrometheusObserver::new().render(counters.iter().copied())?;
//!         assert!(output.contains("requests_total"));
//!     }
//!
//!     #[cfg(feature = "opentelemetry")]
//!     {
//!         use contatori::observers::opentelemetry::OtelObserver;
//!         OtelObserver::new("myapp").register(counters)?;
//!     }
//!
//!     Ok(())
//! }
//! # export_metrics().unwrap();
//! ```

mod error;

pub use error::{ObserverError, Result};

#[cfg(feature = "prometheus")]
pub use error::PrometheusError;

#[cfg(feature = "opentelemetry")]
pub use error::OtelError;

#[cfg(feature = "table")]
pub mod table;

#[cfg(feature = "json")]
pub mod json;

#[cfg(feature = "prometheus")]
pub mod prometheus;

#[cfg(feature = "opentelemetry")]
pub mod opentelemetry;
