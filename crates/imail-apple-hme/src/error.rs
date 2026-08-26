use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppleHmeErrorCode {
    InvalidInput,
    CredentialsInvalid,
    TwoFactorRequired,
    TwoFactorInvalid,
    PendingLoginExpired,
    SessionMissing,
    SessionExpired,
    SubscriptionRequired,
    AddressLimitReached,
    AddressNotFound,
    Unsupported,
    Network,
    BadResponse,
    Protocol,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct AppleHmeError {
    pub code: AppleHmeErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl AppleHmeError {
    pub(crate) fn new(
        code: AppleHmeErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(AppleHmeErrorCode::InvalidInput, message, false)
    }

    pub(crate) fn network(message: impl Into<String>) -> Self {
        Self::new(AppleHmeErrorCode::Network, message, true)
    }

    pub(crate) fn bad_response(message: impl Into<String>) -> Self {
        Self::new(AppleHmeErrorCode::BadResponse, message, true)
    }
}

pub(crate) type Result<T> = std::result::Result<T, AppleHmeError>;
