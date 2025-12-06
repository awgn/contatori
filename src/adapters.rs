//! Wrapper types for extending counter functionality.
//!
//! This module provides wrapper types that add additional behavior to
//! counters while maintaining compatibility with the [`Observable`](crate::counters::Observable) trait.
//!
//! # Available Wrappers
//!
//! | Wrapper | Description |
//! |---------|-------------|
//! | [`Resettable`] | Resets counter when `value()` is called - for periodic metrics |
//! | [`Labeled`] | Adds key-value labels/tags to a counter |
//!
//! # Examples
//!
//! ## Resettable Counter
//!
//! ```rust
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::adapters::Resettable;
//!
//! let requests_per_period = Resettable::new(Unsigned::new().with_name("requests_per_period"));
//! requests_per_period.add(100);
//!
//! // value() returns the value AND resets the counter
//! assert_eq!(requests_per_period.value().as_u64(), 100);
//! assert_eq!(requests_per_period.value().as_u64(), 0); // Reset to 0!
//! ```
//!
//! ## Labeled Counter
//!
//! ```rust
//! use contatori::counters::unsigned::Unsigned;
//! use contatori::counters::Observable;
//! use contatori::adapters::Labeled;
//!
//! let requests = Labeled::new(Unsigned::new().with_name("http_requests"))
//!     .with_label("method", "GET")
//!     .with_label("path", "/api/users");
//!
//! // Labels are accessible for observers (e.g., Prometheus)
//! for (key, value) in requests.labels() {
//!     println!("{}: {}", key, value);
//! }
//! ```
//!

mod labeled;
mod resettable;

pub use labeled::Labeled;
pub use resettable::Resettable;