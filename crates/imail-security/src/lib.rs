use std::{fs, path::Path};

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use scrypt::{scrypt, Params as ScryptParams};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroize;

const PASSWORD_SALT_BYTES: usize = 16;
const PASSWORD_HASH_BYTES: usize = 64;
const GCM_IV_BYTES: usize = 12;
const GCM_TAG_BYTES: usize = 16;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("主密钥必须是 32 字节的十六进制值")]
    InvalidMasterKey,
    #[error("账户凭据已损坏")]
    InvalidEncryptedPayload,
    #[error("账户凭据认证失败")]
    AuthenticationFailed,
    #[error("账户凭据 JSON 无效")]
    InvalidJson,
    #[error("密码哈希格式无效")]
    InvalidPasswordHash,
    #[error("密码派生失败")]
    PasswordDerivation,
    #[error("无法读取主密钥")]
    MasterKeyRead,
    #[error("便携导出加密参数无效")]
    InvalidPortableExport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableEncryptedPayload {
    pub salt: String,
    pub iv: String,
    pub auth_tag: String,
    pub ciphertext: String,
}

#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct MasterKey([u8; 32]);

impl MasterKey {
    pub fn generate_hex() -> String {
        use std::fmt::Write as _;

        let mut key = [0_u8; 32];
        OsRng.fill_bytes(&mut key);
        let mut encoded = String::with_capacity(64);
        for byte in key {
            write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
        }
        key.zeroize();
        encoded
    }

    pub fn from_hex(value: &str) -> Result<Self, SecurityError> {
        let value = value.trim();
        if value.len() != 64 {
            return Err(SecurityError::InvalidMasterKey);
        }
        let mut key = [0_u8; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let text = std::str::from_utf8(pair).map_err(|_| SecurityError::InvalidMasterKey)?;
            key[index] =
                u8::from_str_radix(text, 16).map_err(|_| SecurityError::InvalidMasterKey)?;
        }
        Ok(Self(key))
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, SecurityError> {
        let value = fs::read_to_string(path).map_err(|_| SecurityError::MasterKeyRead)?;
        Self::from_hex(&value)
    }

    pub fn encrypt_json<T: Serialize>(&self, value: &T) -> Result<String, SecurityError> {
        let plaintext = serde_json::to_vec(value).map_err(|_| SecurityError::InvalidJson)?;
        let mut iv = [0_u8; GCM_IV_BYTES];
        OsRng.fill_bytes(&mut iv);
        self.encrypt_bytes_with_iv(&plaintext, iv)
    }

    pub fn encrypt_bytes_with_iv(
        &self,
        plaintext: &[u8],
        iv: [u8; GCM_IV_BYTES],
    ) -> Result<String, SecurityError> {
        let cipher =
            Aes256Gcm::new_from_slice(&self.0).map_err(|_| SecurityError::InvalidMasterKey)?;
        let mut encrypted = cipher
            .encrypt(Nonce::from_slice(&iv), plaintext)
            .map_err(|_| SecurityError::AuthenticationFailed)?;
        if encrypted.len() < GCM_TAG_BYTES {
            return Err(SecurityError::InvalidEncryptedPayload);
        }
        let tag = encrypted.split_off(encrypted.len() - GCM_TAG_BYTES);
        Ok([
            URL_SAFE_NO_PAD.encode(iv),
            URL_SAFE_NO_PAD.encode(tag),
            URL_SAFE_NO_PAD.encode(encrypted),
        ]
        .join("."))
    }

    pub fn decrypt_bytes(&self, payload: &str) -> Result<Vec<u8>, SecurityError> {
        let parts = payload.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(SecurityError::InvalidEncryptedPayload);
        }
        let iv = URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|_| SecurityError::InvalidEncryptedPayload)?;
        let tag = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|_| SecurityError::InvalidEncryptedPayload)?;
        let mut ciphertext = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| SecurityError::InvalidEncryptedPayload)?;
        if iv.len() != GCM_IV_BYTES || tag.len() != GCM_TAG_BYTES || ciphertext.is_empty() {
            return Err(SecurityError::InvalidEncryptedPayload);
        }
        ciphertext.extend_from_slice(&tag);
        let cipher =
            Aes256Gcm::new_from_slice(&self.0).map_err(|_| SecurityError::InvalidMasterKey)?;
        cipher
            .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref())
            .map_err(|_| SecurityError::AuthenticationFailed)
    }

    pub fn decrypt_json<T: DeserializeOwned>(&self, payload: &str) -> Result<T, SecurityError> {
        let mut plaintext = self.decrypt_bytes(payload)?;
        let value = serde_json::from_slice(&plaintext).map_err(|_| SecurityError::InvalidJson);
        plaintext.zeroize();
        value
    }
}

pub fn hash_password(password: &str) -> Result<String, SecurityError> {
    let mut salt = [0_u8; PASSWORD_SALT_BYTES];
    OsRng.fill_bytes(&mut salt);
    hash_password_with_salt(password, salt)
}

pub fn hash_password_with_salt(
    password: &str,
    salt: [u8; PASSWORD_SALT_BYTES],
) -> Result<String, SecurityError> {
    let params = ScryptParams::new(14, 8, 1, PASSWORD_HASH_BYTES)
        .map_err(|_| SecurityError::PasswordDerivation)?;
    let mut derived = [0_u8; PASSWORD_HASH_BYTES];
    scrypt(password.as_bytes(), &salt, &params, &mut derived)
        .map_err(|_| SecurityError::PasswordDerivation)?;
    let encoded = format!(
        "scrypt:{}:{}",
        URL_SAFE_NO_PAD.encode(salt),
        URL_SAFE_NO_PAD.encode(derived)
    );
    derived.zeroize();
    Ok(encoded)
}

pub fn verify_password(password: &str, encoded: &str) -> Result<bool, SecurityError> {
    let parts = encoded.split(':').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0] != "scrypt" {
        return Ok(false);
    }
    let salt = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| SecurityError::InvalidPasswordHash)?;
    let expected = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| SecurityError::InvalidPasswordHash)?;
    if salt.len() != PASSWORD_SALT_BYTES || expected.is_empty() {
        return Ok(false);
    }
    let params = ScryptParams::new(14, 8, 1, expected.len())
        .map_err(|_| SecurityError::PasswordDerivation)?;
    let mut actual = vec![0_u8; expected.len()];
    scrypt(password.as_bytes(), &salt, &params, &mut actual)
        .map_err(|_| SecurityError::PasswordDerivation)?;
    let matches = actual.ct_eq(expected.as_slice()).into();
    actual.zeroize();
    Ok(matches)
}

pub fn sha256_hex(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}

pub fn audit_actor_hmac_hex(salt: &str, actor: &str) -> String {
    let mut hmac = <Hmac<Sha256> as Mac>::new_from_slice(salt.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    hmac.update(actor.as_bytes());
    format!("{:x}", hmac.finalize().into_bytes())
}

pub fn encrypt_portable_export(
    plaintext: &[u8],
    password: &str,
    aad: &[u8],
) -> Result<PortableEncryptedPayload, SecurityError> {
    let mut salt = [0_u8; 16];
    let mut iv = [0_u8; GCM_IV_BYTES];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);
    encrypt_portable_export_with_parameters(plaintext, password, aad, salt, iv)
}

pub fn encrypt_portable_export_with_parameters(
    plaintext: &[u8],
    password: &str,
    aad: &[u8],
    salt: [u8; 16],
    iv: [u8; GCM_IV_BYTES],
) -> Result<PortableEncryptedPayload, SecurityError> {
    let params = ScryptParams::new(15, 8, 1, 32).map_err(|_| SecurityError::PasswordDerivation)?;
    let mut key = [0_u8; 32];
    scrypt(password.as_bytes(), &salt, &params, &mut key)
        .map_err(|_| SecurityError::PasswordDerivation)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| SecurityError::InvalidPortableExport)?;
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| SecurityError::AuthenticationFailed);
    key.zeroize();
    let mut encrypted = encrypted?;
    if encrypted.len() < GCM_TAG_BYTES {
        encrypted.zeroize();
        return Err(SecurityError::InvalidPortableExport);
    }
    let tag = encrypted.split_off(encrypted.len() - GCM_TAG_BYTES);
    let result = PortableEncryptedPayload {
        salt: URL_SAFE_NO_PAD.encode(salt),
        iv: URL_SAFE_NO_PAD.encode(iv),
        auth_tag: URL_SAFE_NO_PAD.encode(tag),
        ciphertext: URL_SAFE_NO_PAD.encode(&encrypted),
    };
    encrypted.zeroize();
    Ok(result)
}

pub fn decrypt_portable_export(
    payload: &PortableEncryptedPayload,
    password: &str,
    aad: &[u8],
) -> Result<Vec<u8>, SecurityError> {
    let salt = URL_SAFE_NO_PAD
        .decode(&payload.salt)
        .map_err(|_| SecurityError::InvalidPortableExport)?;
    let iv = URL_SAFE_NO_PAD
        .decode(&payload.iv)
        .map_err(|_| SecurityError::InvalidPortableExport)?;
    let tag = URL_SAFE_NO_PAD
        .decode(&payload.auth_tag)
        .map_err(|_| SecurityError::InvalidPortableExport)?;
    let mut ciphertext = URL_SAFE_NO_PAD
        .decode(&payload.ciphertext)
        .map_err(|_| SecurityError::InvalidPortableExport)?;
    if salt.len() != 16 || iv.len() != GCM_IV_BYTES || tag.len() != GCM_TAG_BYTES {
        ciphertext.zeroize();
        return Err(SecurityError::InvalidPortableExport);
    }
    let params = ScryptParams::new(15, 8, 1, 32).map_err(|_| SecurityError::PasswordDerivation)?;
    let mut key = [0_u8; 32];
    scrypt(password.as_bytes(), &salt, &params, &mut key)
        .map_err(|_| SecurityError::PasswordDerivation)?;
    ciphertext.extend_from_slice(&tag);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| SecurityError::InvalidPortableExport)?;
    let decrypted = cipher
        .decrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: &ciphertext,
                aad,
            },
        )
        .map_err(|_| SecurityError::AuthenticationFailed);
    key.zeroize();
    ciphertext.zeroize();
    decrypted
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SecurityFixture {
        master_key_hex: String,
        iv_hex: String,
        plaintext: String,
        encrypted_payload: String,
        password: String,
        password_salt_hex: String,
        password_encoded: String,
        raw_token: String,
        token_sha256: String,
        audit_salt: String,
        audit_actor: String,
        audit_actor_hmac_sha256: String,
    }

    fn fixture() -> SecurityFixture {
        serde_json::from_str(include_str!("../../../fixtures/security-v1.json")).unwrap()
    }

    fn fixed<const N: usize>(hex: &str) -> [u8; N] {
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
            .collect::<Vec<_>>();
        bytes.try_into().ok().unwrap()
    }

    #[test]
    fn matches_node_aes_gcm_payload_in_both_directions() {
        let fixture = fixture();
        let key = MasterKey::from_hex(&fixture.master_key_hex).unwrap();
        let iv = fixed::<12>(&fixture.iv_hex);
        assert_eq!(
            key.encrypt_bytes_with_iv(fixture.plaintext.as_bytes(), iv)
                .unwrap(),
            fixture.encrypted_payload
        );
        assert_eq!(
            key.decrypt_bytes(&fixture.encrypted_payload).unwrap(),
            fixture.plaintext.as_bytes()
        );
    }

    #[test]
    fn matches_node_password_token_and_audit_vectors() {
        let fixture = fixture();
        let salt = fixed::<16>(&fixture.password_salt_hex);
        assert_eq!(
            hash_password_with_salt(&fixture.password, salt).unwrap(),
            fixture.password_encoded
        );
        assert!(verify_password(&fixture.password, &fixture.password_encoded).unwrap());
        assert!(!verify_password("wrong", &fixture.password_encoded).unwrap());
        assert_eq!(sha256_hex(&fixture.raw_token), fixture.token_sha256);
        assert_eq!(
            audit_actor_hmac_hex(&fixture.audit_salt, &fixture.audit_actor),
            fixture.audit_actor_hmac_sha256
        );
    }

    #[test]
    fn matches_node_portable_authorization_export_vector() {
        let plaintext = br#"{"format":"imail-mail-authorizations","formatVersion":1,"exportedAt":"2026-08-10T00:00:00.000Z","accounts":[]}"#;
        let aad = b"imail-mail-authorizations:v1";
        let encrypted = encrypt_portable_export_with_parameters(
            plaintext,
            "portable-export-password",
            aad,
            std::array::from_fn(|index| index as u8),
            std::array::from_fn(|index| index as u8 + 16),
        )
        .unwrap();
        assert_eq!(encrypted.salt, "AAECAwQFBgcICQoLDA0ODw");
        assert_eq!(encrypted.iv, "EBESExQVFhcYGRob");
        assert_eq!(encrypted.auth_tag, "7-3v6qVNorI9Lk6eZN2Rew");
        assert_eq!(encrypted.ciphertext, "TghlOB34YxaTpmVK1LnvWZ0h0e5q0yps3MEVfT0iq3ki2heHXy17i3zYNmS9KNT0ViktX10Fu88AAG9Crwq2Q5d9X623JrjBxSgOjDpmGtpV_yaeZw5e_G3w4y1bnirUpfDfM06nouOzPN3Y_Mc");
        assert_eq!(
            decrypt_portable_export(&encrypted, "portable-export-password", aad).unwrap(),
            plaintext
        );
        assert!(decrypt_portable_export(&encrypted, "wrong-password", aad).is_err());
        let mut tampered = encrypted;
        tampered.ciphertext.push('a');
        assert!(decrypt_portable_export(&tampered, "portable-export-password", aad).is_err());
    }

    #[test]
    fn rejects_tampered_or_malformed_payloads() {
        let fixture = fixture();
        let key = MasterKey::from_hex(&fixture.master_key_hex).unwrap();
        let mut tampered = fixture.encrypted_payload;
        tampered.push('a');
        assert!(key.decrypt_bytes(&tampered).is_err());
        assert!(key.decrypt_bytes("invalid").is_err());
        assert!(MasterKey::from_hex("abcd").is_err());
    }
}
