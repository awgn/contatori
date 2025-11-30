//! Observer implementations for collecting and exporting counter metrics.
//!
//! This module provides various ways to observe and export counter values:
//!
//! - [`table`] - Pretty-print counters as tables using the `tabled` crate
//! - [`json`] - Serialize counters to JSON format
//! - [`prometheus`] - Export counters in Prometheus exposition format
//!
//! # Feature Flags
//!
//! Each observer is gated behind a feature flag to minimize dependencies:
//!
//! - `table` - Enables the [`table`] module
//! - `json` - Enables the [`json`] module
//! - `prometheus` - Enables the [`prometheus`] module
//! - `full` - Enables all observer modules
//!
//! # Example
//!
//! ```rust,ignore
//! use contatori::counters::Observable;
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::observers::table::TableObserver;
//!
//! let counters: Vec<Box<dyn Observable>> = vec![
//!     Box::new(Unsigned::new().with_name("requests")),
//!     Box::new(Unsigned::new().with_name("errors")),
//! ];
//!
//! let observer = TableObserver::new();
//! println!("{}", observer.render(counters.iter().map(|c| c.as_ref())));
//! ```

#[cfg(feature = "table")]
pub mod table;

#[cfg(feature = "serde_json")]
pub mod json;

#[cfg(feature = "prometheus")]
pub mod prometheus;
