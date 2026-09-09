use imail_mail::{ProtocolFailure, ProtocolStage};
use std::io;

#[derive(Debug)]
pub(super) enum NetworkError {
    Provider(String),
}

impl NetworkError {
    pub(super) fn io(error: io::Error) -> Self {
        Self::Provider(error.to_string())
    }

    pub(super) fn imap(error: async_imap::error::Error) -> Self {
        Self::Provider(error.to_string())
    }

    pub(super) fn smtp(error: mail_send::Error) -> Self {
        Self::Provider(error.to_string())
    }

    pub(super) fn into_protocol_failure(self, stage: ProtocolStage) -> ProtocolFailure {
        let Self::Provider(detail) = self;
        ProtocolFailure::from_provider(stage, None, &detail)
    }
}
