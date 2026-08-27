use imail_core::accounts::{AccountSecretCodec, CredentialCodecError};
use imail_core::translation_settings::{
    TranslationCredentialCodec, TranslationCredentialCodecError,
};
use imail_security::MasterKey;
use serde_json::Value;

pub struct MasterKeyCredentialCodec<'a> {
    key: &'a MasterKey,
}

impl<'a> MasterKeyCredentialCodec<'a> {
    pub fn new(key: &'a MasterKey) -> Self {
        Self { key }
    }
}

impl AccountSecretCodec for MasterKeyCredentialCodec<'_> {
    fn decrypt(&self, payload: &str) -> Result<Value, CredentialCodecError> {
        self.key
            .decrypt_json(payload)
            .map_err(|_| CredentialCodecError)
    }

    fn encrypt(&self, value: &Value) -> Result<String, CredentialCodecError> {
        self.key
            .encrypt_json(value)
            .map_err(|_| CredentialCodecError)
    }
}

impl TranslationCredentialCodec for MasterKeyCredentialCodec<'_> {
    fn decrypt(&self, payload: &str) -> Result<Value, TranslationCredentialCodecError> {
        self.key
            .decrypt_json(payload)
            .map_err(|_| TranslationCredentialCodecError)
    }

    fn encrypt(&self, value: &Value) -> Result<String, TranslationCredentialCodecError> {
        self.key
            .encrypt_json(value)
            .map_err(|_| TranslationCredentialCodecError)
    }
}
