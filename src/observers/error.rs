//! Unified error type for all observers.
//!
//! This module provides a unified [`ObserverError`] type that wraps errors from
//! all observer implementations. This allows client code to switch between
//! observers without changing error handling logic.
//!
//! # Example
//!
//! ```rust,ignore
//! use contatori::observers::{Result, ObserverError};
//!
//! fn export_metrics() -> Result<()> {
//!     // Works with any observer - same error type!
//!     Ok(())
//! }
//! ```

use thiserror::Error;

/// Unified error type for all observer operations.
///
/// This enum wraps errors from all observer implementations, allowing
/// client code to use a single error type regardless of which observer
/// is being used.
#[derive(Debug, Error)]
pub enum ObserverError {
    /// Error from the JSON observer.
    #[cfg(feature = "json")]
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Error from the Prometheus observer.
    #[cfg(feature = "prometheus")]
    #[error("prometheus error: {0}")]
    Prometheus(#[from] PrometheusError),

    /// Error from the OpenTelemetry observer.
    #[cfg(feature = "opentelemetry")]
    #[error("opentelemetry error: {0}")]
    OpenTelemetry(#[from] OtelError),

    /// Error encoding to UTF-8.
    #[error("utf8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// Generic metric error.
    #[error("metric error: {0}")]
    Metric(String),
}

/// Result type for observer operations.
pub type Result<T> = std::result::Result<T, ObserverError>;

/// Error type specific to Prometheus observer operations.
#[cfg(feature = "prometheus")]
#[derive(Debug, Error)]
pub enum PrometheusError {
    /// Error creating or registering a metric.
    #[error("metric error: {0}")]
    MetricError(String),

    /// Error encoding metrics to text format.
    #[error("encode error: {0}")]
    EncodeError(String),

    /// Error converting bytes to UTF-8 string.
    #[error(transparent)]
    Utf8Error(#[from] std::string::FromUtf8Error),
}

#[cfg(feature = "prometheus")]
impl From<prometheus::Error> for PrometheusError {
    fn from(err: prometheus::Error) -> Self {
        PrometheusError::MetricError(err.to_string())
    }
}

#[cfg(feature = "prometheus")]
impl From<prometheus::Error> for ObserverError {
    fn from(err: prometheus::Error) -> Self {
        ObserverError::Prometheus(PrometheusError::from(err))
    }
}

/// Error type specific to OpenTelemetry observer operations.
#[cfg(feature = "opentelemetry")]
#[derive(Debug, Error)]
pub enum OtelError {
    /// Error creating or registering a metric.
    #[error("metric error: {0}")]
    MetricError(String),
}