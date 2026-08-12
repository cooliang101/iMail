use std::error::Error;

use imail_mail::{
    DownloadedAttachment, ImapPort, MailAuthentication, MailConnectionConfig, MailProxy,
    RemoteMailError, RemoteMailService, RemoteMessageLocator, SmtpPort,
};
use imail_protocol::{
    MessageMoveDestination, ParsedAttachmentView, RemoteMessageFlagPatch, RemoteMessageMoveResult,
    SendMessageInput, SendMessageResult,
};
use serde_json::Value;

use crate::{
    accounts::AccountSecretCodec, AccountRecord, AccountRepository, ApplicationError,
    LocalRepository,
};

type RepositoryError<R> = <R as AccountRepository>::Error;

#[derive(Debug, thiserror::Error)]
pub enum MailApplicationError<E: Error + Send + Sync + 'static> {
    #[error("{message}")]
    Domain {
        code: &'static str,
        status: u16,
        message: &'static str,
    },
    #[error(transparent)]
    Repository(E),
    #[error(transparent)]
    Remote(#[from] RemoteMailError),
}

impl<E: Error + Send + Sync + 'static> MailApplicationError<E> {
    pub fn code(&self) -> &str {
        match self {
            Self::Domain { code, .. } => code,
            Self::Repository(_) => "STORAGE_ERROR",
            Self::Remote(RemoteMailError::AttachmentNotFound) => "ATTACHMENT_NOT_FOUND",
            Self::Remote(RemoteMailError::DestinationUnavailable(_)) => {
                "MAILBOX_DESTINATION_UNAVAILABLE"
            }
            Self::Remote(RemoteMailError::MoveUnconfirmed) => "MESSAGE_MOVE_UNCONFIRMED",
            Self::Remote(RemoteMailError::InvalidAttachmentData) => "ATTACHMENT_DATA_INVALID",
            Self::Remote(RemoteMailError::AttachmentUnavailable) => "ATTACHMENT_UNAVAILABLE",
            Self::Remote(RemoteMailError::Protocol(_)) => "MAIL_PROTOCOL_ERROR",
            Self::Remote(RemoteMailError::Parse(_)) => "MAIL_PARSE_ERROR",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Domain { status, .. } => *status,
            Self::Repository(_) => 500,
            Self::Remote(RemoteMailError::AttachmentNotFound) => 404,
            Self::Remote(RemoteMailError::InvalidAttachmentData) => 400,
            Self::Remote(RemoteMailError::DestinationUnavailable(_))
            | Self::Remote(RemoteMailError::MoveUnconfirmed)
            | Self::Remote(RemoteMailError::AttachmentUnavailable)
            | Self::Remote(RemoteMailError::Protocol(_))
            | Self::Remote(RemoteMailError::Parse(_)) => 502,
        }
    }
}

pub struct MailApplicationService<'a, R, C, I, S>
where
    R: LocalRepository,
    C: AccountSecretCodec,
    I: ImapPort + ?Sized,
    S: SmtpPort + ?Sized,
{
    repository: &'a R,
    codec: &'a C,
    imap: &'a mut I,
    smtp: &'a mut S,
}

impl<'a, R, C, I, S> MailApplicationService<'a, R, C, I, S>
where
    R: LocalRepository,
    C: AccountSecretCodec,
    I: ImapPort + ?Sized,
    S: SmtpPort + ?Sized,
{
    pub fn new(repository: &'a R, codec: &'a C, imap: &'a mut I, smtp: &'a mut S) -> Self {
        Self {
            repository,
            codec,
            imap,
            smtp,
        }
    }

    pub fn verify_account(
        &mut self,
        user_id: &str,
        account_id: &str,
    ) -> Result<(), MailApplicationError<RepositoryError<R>>> {
        let config = self.account_config(user_id, account_id)?;
        RemoteMailService::new(self.imap, self.smtp)
            .verify_account(&config)
            .map_err(MailApplicationError::Remote)
    }

    pub fn download_attachment(
        &mut self,
        user_id: &str,
        message_id: &str,
        index: usize,
    ) -> Result<DownloadedAttachment, MailApplicationError<RepositoryError<R>>> {
        let message = self.message(user_id, message_id)?;
        let config = self.account_config(user_id, &message.account_id)?;
        let metadata = serde_json::from_value::<Vec<ParsedAttachmentView>>(message.attachments)
            .map_err(|_| domain("MESSAGE_ATTACHMENTS_INVALID", 500, "邮件附件元数据不可用"))?;
        let locator = locator(message.mailbox, message.uid, message.message_id)?;
        RemoteMailService::new(self.imap, self.smtp)
            .download_attachment(&config, &locator, &metadata, index)
            .map_err(MailApplicationError::Remote)
    }

    pub fn update_flags(
        &mut self,
        user_id: &str,
        message_id: &str,
        patch: &RemoteMessageFlagPatch,
    ) -> Result<(), MailApplicationError<RepositoryError<R>>> {
        let message = self.message(user_id, message_id)?;
        let config = self.account_config(user_id, &message.account_id)?;
        let locator = locator(message.mailbox, message.uid, message.message_id)?;
        RemoteMailService::new(self.imap, self.smtp)
            .update_flags(&config, &locator, patch)
            .map_err(MailApplicationError::Remote)
    }

    pub fn move_message(
        &mut self,
        user_id: &str,
        message_id: &str,
        destination: MessageMoveDestination,
    ) -> Result<RemoteMessageMoveResult, MailApplicationError<RepositoryError<R>>> {
        let message = self.message(user_id, message_id)?;
        let config = self.account_config(user_id, &message.account_id)?;
        let locator = locator(message.mailbox, message.uid, message.message_id)?;
        RemoteMailService::new(self.imap, self.smtp)
            .move_message(&config, &locator, destination)
            .map_err(MailApplicationError::Remote)
    }

    pub fn send_message(
        &mut self,
        user_id: &str,
        input: &SendMessageInput,
    ) -> Result<SendMessageResult, MailApplicationError<RepositoryError<R>>> {
        let config = self.account_config(user_id, &input.account_id)?;
        RemoteMailService::new(self.imap, self.smtp)
            .send_message(&config, input)
            .map_err(MailApplicationError::Remote)
    }

    fn account_config(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<MailConnectionConfig, MailApplicationError<RepositoryError<R>>> {
        let account = self
            .repository
            .account(user_id, account_id)
            .map_err(MailApplicationError::Repository)?
            .ok_or_else(|| domain("ACCOUNT_NOT_FOUND", 404, "邮箱账户不存在"))?;
        connection_config::<RepositoryError<R>, C>(&account, self.codec)
    }

    fn message(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<imail_protocol::MessageReadModel, MailApplicationError<RepositoryError<R>>> {
        self.repository
            .list_messages(user_id)
            .map_err(MailApplicationError::Repository)?
            .into_iter()
            .find(|message| message.id == message_id)
            .ok_or_else(|| domain("MESSAGE_NOT_FOUND", 404, "邮件不存在"))
    }
}

pub fn connection_config<E, C: AccountSecretCodec>(
    account: &AccountRecord,
    codec: &C,
) -> Result<MailConnectionConfig, MailApplicationError<E>>
where
    E: Error + Send + Sync + 'static,
{
    let secret = codec
        .decrypt(&account.encrypted_secret)
        .map_err(|_| domain("ACCOUNT_SECRET_UNAVAILABLE", 500, "邮箱账户凭据不可用"))?;
    let settings = account
        .settings
        .as_object()
        .ok_or_else(|| domain("ACCOUNT_SETTINGS_INVALID", 500, "邮箱服务器配置不可用"))?;
    let proxy_password = secret.get("proxyPassword").and_then(Value::as_str);
    let proxy = account
        .proxy
        .as_ref()
        .map(|proxy| parse_proxy(proxy, proxy_password))
        .transpose()?;
    let authentication =
        if let Some(access_token) = secret.get("accessToken").and_then(Value::as_str) {
            MailAuthentication::OAuth {
                provider: secret
                    .get("oauthProvider")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| oauth_provider_for(&account.provider))
                    .to_string(),
                access_token: access_token.to_string(),
            }
        } else if let Some(password) = secret.get("password").and_then(Value::as_str) {
            MailAuthentication::Password(password.to_string())
        } else {
            return Err(domain("ACCOUNT_SECRET_INVALID", 500, "邮箱账户凭据不可用"));
        };
    Ok(MailConnectionConfig {
        email: account.email.clone(),
        display_name: account.display_name.clone(),
        imap_host: required_string(settings, "imapHost")?,
        imap_port: required_port(settings, "imapPort")?,
        imap_secure: required_bool(settings, "imapSecure")?,
        smtp_host: required_string(settings, "smtpHost")?,
        smtp_port: required_port(settings, "smtpPort")?,
        smtp_secure: required_bool(settings, "smtpSecure")?,
        authentication,
        proxy,
    })
}

fn parse_proxy<E: Error + Send + Sync + 'static>(
    value: &Value,
    password: Option<&str>,
) -> Result<MailProxy, MailApplicationError<E>> {
    let proxy = value
        .as_object()
        .ok_or_else(|| domain("PROXY_INVALID", 500, "代理配置不可用"))?;
    Ok(MailProxy {
        protocol: required_string(proxy, "protocol")?,
        host: required_string(proxy, "host")?,
        port: required_port(proxy, "port")?,
        username: proxy
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string),
        password: password.map(str::to_string),
    })
}

fn required_string<E: Error + Send + Sync + 'static>(
    value: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, MailApplicationError<E>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| domain("ACCOUNT_SETTINGS_INVALID", 500, "邮箱服务器配置不可用"))
}

fn required_port<E: Error + Send + Sync + 'static>(
    value: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<u16, MailApplicationError<E>> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .ok_or_else(|| domain("ACCOUNT_SETTINGS_INVALID", 500, "邮箱服务器配置不可用"))
}

fn required_bool<E: Error + Send + Sync + 'static>(
    value: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<bool, MailApplicationError<E>> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| domain("ACCOUNT_SETTINGS_INVALID", 500, "邮箱服务器配置不可用"))
}

fn locator<E: Error + Send + Sync + 'static>(
    mailbox: String,
    uid: i64,
    message_id: Option<String>,
) -> Result<RemoteMessageLocator, MailApplicationError<E>> {
    Ok(RemoteMessageLocator {
        mailbox,
        uid: u32::try_from(uid)
            .ok()
            .filter(|uid| *uid > 0)
            .ok_or_else(|| domain("MESSAGE_UID_INVALID", 500, "邮件远程标识不可用"))?,
        message_id,
    })
}

fn oauth_provider_for(provider: &str) -> &str {
    match provider {
        "gmail" => "google",
        "outlook" | "hotmail" => "microsoft",
        "yahoo" => "yahoo",
        _ => provider,
    }
}

fn domain<E: Error + Send + Sync + 'static>(
    code: &'static str,
    status: u16,
    message: &'static str,
) -> MailApplicationError<E> {
    MailApplicationError::Domain {
        code,
        status,
        message,
    }
}

impl<E: Error + Send + Sync + 'static> From<ApplicationError<E>> for MailApplicationError<E> {
    fn from(value: ApplicationError<E>) -> Self {
        match value {
            ApplicationError::Domain {
                code,
                status,
                message,
            } => Self::Domain {
                code,
                status,
                message,
            },
            ApplicationError::Repository(error) => Self::Repository(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::CredentialCodecError;
    use serde_json::json;

    #[test]
    fn resolves_node_compatible_oauth_and_proxy_configuration() {
        let account = account(json!({
            "authType": "oauth2",
            "accessToken": "access-secret",
            "refreshToken": "refresh-secret",
            "proxyPassword": "proxy-secret"
        }));
        let config = connection_config::<FakeError, _>(&account, &JsonCodec).unwrap();

        assert_eq!(config.imap_host, "imap.gmail.com");
        assert!(matches!(
            config.authentication,
            MailAuthentication::OAuth { ref provider, ref access_token }
                if provider == "google" && access_token == "access-secret"
        ));
        let proxy = config.proxy.unwrap();
        assert_eq!(proxy.protocol, "socks5");
        assert_eq!(proxy.password.as_deref(), Some("proxy-secret"));
    }

    #[test]
    fn rejects_malformed_settings_without_rendering_secrets() {
        let mut account = account(json!({ "password": "mail-secret" }));
        account.settings["imapPort"] = json!(0);
        let error = connection_config::<FakeError, _>(&account, &JsonCodec).unwrap_err();

        assert_eq!(error.code(), "ACCOUNT_SETTINGS_INVALID");
        assert!(!error.to_string().contains("mail-secret"));
    }

    fn account(secret: Value) -> AccountRecord {
        AccountRecord {
            id: "account-1".into(),
            owner_id: "user-1".into(),
            provider: "gmail".into(),
            email: "owner@example.com".into(),
            display_name: "Owner".into(),
            group: "个人".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: json!({
                "imapHost": "imap.gmail.com", "imapPort": 993, "imapSecure": true,
                "smtpHost": "smtp.gmail.com", "smtpPort": 465, "smtpSecure": true
            }),
            proxy: Some(json!({
                "protocol": "socks5", "host": "127.0.0.1", "port": 1080,
                "username": "mail"
            })),
            encrypted_secret: secret.to_string(),
            auth_method: Some("oauth2".into()),
            created_at: "2026-08-10T04:34:56.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([]),
        }
    }

    struct JsonCodec;

    impl AccountSecretCodec for JsonCodec {
        fn decrypt(&self, payload: &str) -> Result<Value, CredentialCodecError> {
            serde_json::from_str(payload).map_err(|_| CredentialCodecError)
        }

        fn encrypt(&self, value: &Value) -> Result<String, CredentialCodecError> {
            Ok(value.to_string())
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("fake error")]
    struct FakeError;
}
