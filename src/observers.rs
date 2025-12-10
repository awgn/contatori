//! Observer implementations for collecting and exporting counter metrics.
//!
//! This module provides various ways to observe and export counter values:
//!
//! - [`table`] - Pretty-print counters as tables using the `tabled` crate
//! - [`json`] - Serialize counters to JSON format
//! - [`prometheus`] - Export counters in Prometheus exposition format
//! - [`opentelemetry`] - Export counters via OpenTelemetry
//!
//! # Unified Error Handling
//!
//! All observers use a unified [`ObserverError`] type, allowing you to switch
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
//! ```rust,ignore
//! use contatori::counters::Observable;
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::observers::{Result, ObserverError};
//!
//! static REQUESTS: Unsigned = Unsigned::new().with_name("requests");
//! static ERRORS: Unsigned = Unsigned::new().with_name("errors");
//!
//! fn export_metrics() -> Result<()> {
//!     let counters: &[&'static dyn Observable] = &[&REQUESTS, &ERRORS];
//!
//!     #[cfg(feature = "prometheus")]
//!     {
//!         use contatori::observers::prometheus::PrometheusObserver;
//!         let observer = PrometheusObserver::new();
//!         let output = observer.render(counters.iter().copied())?;
//!         println!("{}", output);
//!     }
//!
//!     #[cfg(feature = "opentelemetry")]
//!     {
//!         use contatori::observers::opentelemetry::OtelObserver;
//!         let observer = OtelObserver::new("myapp");
//!         observer.register(counters)?;
//!     }
//!
//!     Ok(())
//! }
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