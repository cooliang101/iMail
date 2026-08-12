use imail_protocol::{
    MailAuthorizationAccountExport, MailAuthorizationExportArtifact,
    MailAuthorizationExportEnvelope, MailAuthorizationExportPayload, MailAuthorizationSecretExport,
    MailProxyExport, MailSettingsExport, MAIL_AUTHORIZATION_EXPORT_FORMAT,
    MAIL_AUTHORIZATION_EXPORT_VERSION,
};
use serde_json::{Map, Value};

use crate::{accounts::AccountSecretCodec, AccountRecord, AccountRepository, ApplicationError};

#[derive(Debug, thiserror::Error)]
#[error("授权导出加密失败")]
pub struct AuthorizationExportEncryptionError;

pub trait AuthorizationExportEncryptor {
    fn encrypt(
        &self,
        payload: &MailAuthorizationExportPayload,
        password: &str,
    ) -> Result<MailAuthorizationExportEnvelope, AuthorizationExportEncryptionError>;
}

pub struct AuthorizationExportService<'a, R: AccountRepository> {
    repository: &'a R,
}

impl<'a, R: AccountRepository> AuthorizationExportService<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub fn prepare<C: AccountSecretCodec, E: AuthorizationExportEncryptor>(
        &self,
        user_id: &str,
        exported_at: &str,
        password: &str,
        codec: &C,
        encryptor: &E,
    ) -> Result<MailAuthorizationExportArtifact, ApplicationError<R::Error>> {
        if !(12..=256).contains(&password.encode_utf16().count()) {
            return Err(domain(
                "AUTHORIZATION_EXPORT_PASSWORD_INVALID",
                400,
                "导出文件密码长度必须为 12–256 个字符",
            ));
        }
        let date = exported_at
            .get(..10)
            .filter(|date| {
                date.len() == 10
                    && date.bytes().enumerate().all(|(index, byte)| {
                        matches!(index, 4 | 7) && byte == b'-'
                            || !matches!(index, 4 | 7) && byte.is_ascii_digit()
                    })
            })
            .ok_or_else(|| domain("AUTHORIZATION_EXPORT_TIME_INVALID", 500, "授权导出时间无效"))?;
        let accounts = self
            .repository
            .list_accounts(user_id)
            .map_err(ApplicationError::Repository)?;
        let exported_accounts = accounts
            .iter()
            .map(|account| export_account::<R::Error, C>(account, codec))
            .collect::<Result<Vec<_>, _>>()?;
        let payload = MailAuthorizationExportPayload {
            format: MAIL_AUTHORIZATION_EXPORT_FORMAT.into(),
            format_version: MAIL_AUTHORIZATION_EXPORT_VERSION,
            exported_at: exported_at.to_string(),
            accounts: exported_accounts,
        };
        let envelope = encryptor.encrypt(&payload, password).map_err(|_| {
            domain(
                "AUTHORIZATION_EXPORT_ENCRYPTION_FAILED",
                500,
                "授权导出加密失败",
            )
        })?;
        Ok(MailAuthorizationExportArtifact {
            filename: format!("imail-mail-authorizations-{date}.imailauth"),
            account_count: accounts.len(),
            envelope,
        })
    }
}

fn export_account<E, C>(
    account: &AccountRecord,
    codec: &C,
) -> Result<MailAuthorizationAccountExport, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
    C: AccountSecretCodec,
{
    let secret = codec
        .decrypt(&account.encrypted_secret)
        .map_err(|_| domain("ACCOUNT_SECRET_UNAVAILABLE", 500, "邮箱账户凭据不可用"))?;
    let secret = secret.as_object().ok_or_else(invalid_data)?;
    let authorization = explicit_secret(secret)?;
    let auth_method = account
        .auth_method
        .clone()
        .or_else(|| authorization.auth_type.clone())
        .unwrap_or_else(|| {
            if authorization.access_token.is_some() || authorization.refresh_token.is_some() {
                "oauth2".into()
            } else {
                "app-password".into()
            }
        });
    Ok(MailAuthorizationAccountExport {
        provider: account.provider.clone(),
        email: account.email.clone(),
        display_name: account.display_name.clone(),
        group: account.group.clone(),
        group_icon: account.group_icon.clone(),
        color: account.color.clone(),
        auth_method,
        settings: export_settings::<E>(&account.settings)?,
        proxy: account.proxy.as_ref().map(export_proxy::<E>).transpose()?,
        authorization,
    })
}

fn export_settings<E>(value: &Value) -> Result<MailSettingsExport, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    Ok(MailSettingsExport {
        imap_host: required_string(value, "imapHost")?,
        imap_port: required_port(value, "imapPort")?,
        imap_secure: required_bool(value, "imapSecure")?,
        smtp_host: required_string(value, "smtpHost")?,
        smtp_port: required_port(value, "smtpPort")?,
        smtp_secure: required_bool(value, "smtpSecure")?,
    })
}

fn export_proxy<E>(value: &Value) -> Result<MailProxyExport, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    Ok(MailProxyExport {
        protocol: required_string(value, "protocol")?,
        host: required_string(value, "host")?,
        port: required_port(value, "port")?,
        username: optional_string(value, "username")?,
    })
}

fn explicit_secret<E>(
    value: &Map<String, Value>,
) -> Result<MailAuthorizationSecretExport, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    let scopes = match value.get("scopes") {
        None => None,
        Some(Value::Array(values)) => Some(
            values
                .iter()
                .map(|scope| scope.as_str().map(str::to_string).ok_or_else(invalid_data))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => return Err(invalid_data()),
    };
    Ok(MailAuthorizationSecretExport {
        auth_type: optional_map_string(value, "authType")?,
        password: optional_map_string(value, "password")?,
        access_token: optional_map_string(value, "accessToken")?,
        refresh_token: optional_map_string(value, "refreshToken")?,
        expires_at: optional_map_string(value, "expiresAt")?,
        oauth_provider: optional_map_string(value, "oauthProvider")?,
        scopes,
        token_type: optional_map_string(value, "tokenType")?,
        proxy_password: optional_map_string(value, "proxyPassword")?,
    })
}

fn required_string<E>(value: &Value, key: &str) -> Result<String, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(invalid_data)
}

fn optional_string<E>(value: &Value, key: &str) -> Result<Option<String>, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match value.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid_data()),
    }
}

fn optional_map_string<E>(
    value: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match value.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid_data()),
    }
}

fn required_port<E>(value: &Value, key: &str) -> Result<u16, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .ok_or_else(invalid_data)
}

fn required_bool<E>(value: &Value, key: &str) -> Result<bool, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(invalid_data)
}

fn invalid_data<E: std::error::Error + Send + Sync + 'static>() -> ApplicationError<E> {
    domain(
        "AUTHORIZATION_EXPORT_DATA_INVALID",
        500,
        "邮箱授权数据不完整，无法导出",
    )
}

fn domain<E: std::error::Error + Send + Sync + 'static>(
    code: &'static str,
    status: u16,
    message: &'static str,
) -> ApplicationError<E> {
    ApplicationError::Domain {
        code,
        status,
        message,
    }
}
