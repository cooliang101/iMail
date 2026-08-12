use imail_core::authorization_export::{
    AuthorizationExportEncryptionError, AuthorizationExportEncryptor,
};
use imail_protocol::{
    MailAuthorizationExportCipher, MailAuthorizationExportEnvelope, MailAuthorizationExportKdf,
    MailAuthorizationExportPayload, MAIL_AUTHORIZATION_EXPORT_FORMAT,
    MAIL_AUTHORIZATION_EXPORT_VERSION,
};
use imail_security::encrypt_portable_export;
use zeroize::Zeroize;

pub struct PortableAuthorizationExportEncryptor;

impl AuthorizationExportEncryptor for PortableAuthorizationExportEncryptor {
    fn encrypt(
        &self,
        payload: &MailAuthorizationExportPayload,
        password: &str,
    ) -> Result<MailAuthorizationExportEnvelope, AuthorizationExportEncryptionError> {
        let mut plaintext =
            serde_json::to_vec(payload).map_err(|_| AuthorizationExportEncryptionError)?;
        let aad = format!(
            "{}:v{}",
            MAIL_AUTHORIZATION_EXPORT_FORMAT, MAIL_AUTHORIZATION_EXPORT_VERSION
        );
        let encrypted = encrypt_portable_export(&plaintext, password, aad.as_bytes())
            .map_err(|_| AuthorizationExportEncryptionError);
        plaintext.zeroize();
        let encrypted = encrypted?;
        Ok(MailAuthorizationExportEnvelope {
            format: MAIL_AUTHORIZATION_EXPORT_FORMAT.into(),
            format_version: MAIL_AUTHORIZATION_EXPORT_VERSION,
            kdf: MailAuthorizationExportKdf {
                algorithm: "scrypt".into(),
                salt: encrypted.salt,
                cost: 32_768,
                block_size: 8,
                parallelization: 1,
                key_length: 32,
            },
            cipher: MailAuthorizationExportCipher {
                algorithm: "aes-256-gcm".into(),
                iv: encrypted.iv,
                auth_tag: encrypted.auth_tag,
            },
            ciphertext: encrypted.ciphertext,
        })
    }
}
