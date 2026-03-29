mod analytics;
pub mod tracker;
/// OpenPanel SDK for tracking events
mod user;
pub use crate::{analytics::Analytics, user::IdentifyUser};

/// Result type for SDK functions
pub type TrackerResult<T> = Result<T, TrackerError>;

/// Errors that can occur when using the SDK
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("Not Authorized")]
    NotAuthorized,
    #[error("Too many requests")]
    TooManyRequests,
    #[error("Internal error")]
    Internal,
    #[error("Request error")]
    Request,
    #[error("Error serializing payload: {0:?}")]
    Serializing(#[from] serde_json::Error),
    #[error("Invalid header name")]
    HeaderName,
    #[error("Invalid header value")]
    HeaderValue,
    #[error("Tracker is disabled")]
    Disabled,
    #[error("Event filtered")]
    Filtered,
}
