//! Unified error type for all observers.
//!
//! This module provides a unified [`ObserverError`] type that wraps errors from
//! all observer implementations. This allows client code to switch between
//! observers without changing error handling logic.
//!
//! # Example
//!
//! ```rust
//! use contatori::observers::Result;
//!
//! fn export_metrics() -> Result<()> {
//!     // Works with any observer - same error type!
//!     Ok(())
//! }
//! # export_metrics().unwrap();
//! ```

use std::error::Error;
use std::fmt;

/// Unified error type for all observer operations.
///
/// This enum wraps errors from all observer implementations, allowing
/// client code to use a single error type regardless of which observer
/// is being used.
#[derive(Debug)]
pub enum ObserverError {
    /// Error from the JSON observer.
    #[cfg(feature = "json")]
    Json(serde_json::Error),

    /// Error from the Prometheus observer.
    #[cfg(feature = "prometheus")]
    Prometheus(PrometheusError),

    /// Error from the OpenTelemetry observer.
    #[cfg(feature = "opentelemetry")]
    OpenTelemetry(OtelError),

    /// Error encoding to UTF-8.
    Utf8(std::string::FromUtf8Error),

    /// Generic metric error.
    Metric(String),
}

impl fmt::Display for ObserverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "json")]
            ObserverError::Json(err) => write!(f, "json error: {err}"),
            #[cfg(feature = "prometheus")]
            ObserverError::Prometheus(err) => write!(f, "prometheus error: {err}"),
            #[cfg(feature = "opentelemetry")]
            ObserverError::OpenTelemetry(err) => write!(f, "opentelemetry error: {err}"),
            ObserverError::Utf8(err) => write!(f, "utf8 error: {err}"),
            ObserverError::Metric(msg) => write!(f, "metric error: {msg}"),
        }
    }
}

impl Error for ObserverError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            #[cfg(feature = "json")]
            ObserverError::Json(err) => Some(err),
            #[cfg(feature = "prometheus")]
            ObserverError::Prometheus(err) => Some(err),
            #[cfg(feature = "opentelemetry")]
            ObserverError::OpenTelemetry(err) => Some(err),
            ObserverError::Utf8(err) => Some(err),
            ObserverError::Metric(_) => None,
        }
    }
}

#[cfg(feature = "json")]
impl From<serde_json::Error> for ObserverError {
    fn from(err: serde_json::Error) -> Self {
        ObserverError::Json(err)
    }
}

#[cfg(feature = "prometheus")]
impl From<PrometheusError> for ObserverError {
    fn from(err: PrometheusError) -> Self {
        ObserverError::Prometheus(err)
    }
}

#[cfg(feature = "opentelemetry")]
impl From<OtelError> for ObserverError {
    fn from(err: OtelError) -> Self {
        ObserverError::OpenTelemetry(err)
    }
}

impl From<std::string::FromUtf8Error> for ObserverError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        ObserverError::Utf8(err)
    }
}

/// Result type for observer operations.
pub type Result<T> = std::result::Result<T, ObserverError>;

/// Error type specific to Prometheus observer operations.
#[cfg(feature = "prometheus")]
#[derive(Debug)]
pub enum PrometheusError {
    /// Error creating or registering a metric.
    MetricError(String),

    /// Error encoding metrics to text format.
    EncodeError(String),

    /// Error converting bytes to UTF-8 string.
    Utf8Error(std::string::FromUtf8Error),
}

#[cfg(feature = "prometheus")]
impl fmt::Display for PrometheusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrometheusError::MetricError(msg) => write!(f, "metric error: {msg}"),
            PrometheusError::EncodeError(msg) => write!(f, "encode error: {msg}"),
            PrometheusError::Utf8Error(err) => fmt::Display::fmt(err, f),
        }
    }
}

#[cfg(feature = "prometheus")]
impl Error for PrometheusError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PrometheusError::MetricError(_) | PrometheusError::EncodeError(_) => None,
            PrometheusError::Utf8Error(err) => err.source(),
        }
    }
}

#[cfg(feature = "prometheus")]
impl From<std::string::FromUtf8Error> for PrometheusError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        PrometheusError::Utf8Error(err)
    }
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
#[derive(Debug)]
pub enum OtelError {
    /// Error creating or registering a metric.
    MetricError(String),
}

#[cfg(feature = "opentelemetry")]
impl fmt::Display for OtelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OtelError::MetricError(msg) => write!(f, "metric error: {msg}"),
        }
    }
}

#[cfg(feature = "opentelemetry")]
impl Error for OtelError {}
