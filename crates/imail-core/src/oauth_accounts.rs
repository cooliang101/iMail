use imail_mail::ProtocolFailure;
use imail_oauth::{CompletedOAuth, OAuthAccountSecret, OAuthProviderKey, OAuthProxyInput};
use imail_protocol::AccountReadModel;
use serde_json::{json, Map, Value};

use crate::{
    accounts::{public_account, AccountSecretCodec},
    AccountRecord, AccountRepository, ApplicationError,
};

pub trait OAuthAccountConnectionValidator {
    fn validate(&self, candidate: &AccountRecord) -> Result<(), ProtocolFailure>;
}

impl<F> OAuthAccountConnectionValidator for F
where
    F: Fn(&AccountRecord) -> Result<(), ProtocolFailure>,
{
    fn validate(&self, candidate: &AccountRecord) -> Result<(), ProtocolFailure> {
        self(candidate)
    }
}

pub struct OAuthAccountService<'a, R: AccountRepository> {
    repository: &'a mut R,
}

impl<'a, R: AccountRepository> OAuthAccountService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn persist_completion<C: AccountSecretCodec, V: OAuthAccountConnectionValidator>(
        &mut self,
        mut completed: CompletedOAuth,
        account_id: &str,
        created_at: &str,
        codec: &C,
        validator: &V,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        if completed.pending.account_id.is_some() {
            return self.persist_reconnection(completed, codec, validator);
        }
        let email = completed.identity.email.to_lowercase();
        completed.secret.proxy_password = completed
            .pending
            .proxy
            .as_ref()
            .and_then(|proxy| proxy.password.clone());
        let display_name = completed
            .pending
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or(completed.identity.name)
            .unwrap_or_else(|| email.clone());
        let mut account = AccountRecord {
            id: account_id.to_string(),
            owner_id: completed.pending.owner_id,
            provider: completed.pending.account_provider,
            email,
            display_name,
            group: completed.pending.group,
            group_icon: "folder".into(),
            color: completed.pending.color,
            settings: provider_settings(completed.secret.oauth_provider),
            proxy: completed.pending.proxy.map(public_proxy),
            encrypted_secret: encode_secret::<R::Error, C>(codec, &completed.secret)?,
            auth_method: Some("oauth2".into()),
            created_at: created_at.to_string(),
            last_sync_at: None,
            status: "syncing".into(),
            last_error: None,
            mailboxes: json!([]),
        };
        if !self
            .repository
            .insert_account_if_email_available(&account)
            .map_err(ApplicationError::Repository)?
        {
            return Err(domain("ACCOUNT_EXISTS", 409, "这个邮箱已经添加"));
        }
        validate_and_update(self.repository, &mut account, validator)?;
        Ok(public_account(&account))
    }

    fn persist_reconnection<C: AccountSecretCodec, V: OAuthAccountConnectionValidator>(
        &mut self,
        mut completed: CompletedOAuth,
        codec: &C,
        validator: &V,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        let account_id = completed.pending.account_id.as_deref().unwrap_or_default();
        let mut current = self
            .repository
            .account(&completed.pending.owner_id, account_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("ACCOUNT_NOT_FOUND", 404, "需要重新授权的邮箱已不存在"))?;
        if current.provider != completed.pending.account_provider {
            return Err(domain(
                "OAUTH_PROVIDER_MISMATCH",
                409,
                "邮箱服务商与重新授权请求不匹配",
            ));
        }
        let expected = completed
            .pending
            .expected_email
            .as_deref()
            .unwrap_or_default();
        if !completed.identity.email.eq_ignore_ascii_case(expected)
            || !completed
                .identity
                .email
                .eq_ignore_ascii_case(&current.email)
        {
            return Err(domain(
                "OAUTH_EMAIL_MISMATCH",
                409,
                "重新授权必须使用原邮箱账号",
            ));
        }
        let previous = codec
            .decrypt(&current.encrypted_secret)
            .map_err(|_| secret_error())?;
        completed.secret.proxy_password = previous
            .get("proxyPassword")
            .and_then(Value::as_str)
            .map(str::to_string);
        current.encrypted_secret = encode_secret::<R::Error, C>(codec, &completed.secret)?;
        current.auth_method = Some("oauth2".into());
        current.status = "syncing".into();
        current.last_error = None;
        self.repository
            .upsert_account(&current)
            .map_err(ApplicationError::Repository)?;
        validate_and_update(self.repository, &mut current, validator)?;
        Ok(public_account(&current))
    }
}

fn validate_and_update<R: AccountRepository, V: OAuthAccountConnectionValidator>(
    repository: &mut R,
    account: &mut AccountRecord,
    validator: &V,
) -> Result<(), ApplicationError<R::Error>> {
    match validator.validate(account) {
        Ok(()) => {
            account.status = "connected".into();
            account.last_error = None;
        }
        Err(failure) => {
            account.status = "error".into();
            account.last_error = Some(failure.message);
        }
    }
    repository
        .upsert_account(account)
        .map_err(ApplicationError::Repository)
}

fn encode_secret<E, C: AccountSecretCodec>(
    codec: &C,
    secret: &OAuthAccountSecret,
) -> Result<String, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    let value = serde_json::to_value(secret).map_err(|_| secret_error())?;
    codec.encrypt(&value).map_err(|_| secret_error())
}

fn public_proxy(proxy: OAuthProxyInput) -> Value {
    let mut result = Map::from_iter([
        ("protocol".into(), Value::String(proxy.protocol)),
        ("host".into(), Value::String(proxy.host)),
        ("port".into(), json!(proxy.port)),
    ]);
    if let Some(username) = proxy.username {
        result.insert("username".into(), Value::String(username));
    }
    Value::Object(result)
}

fn provider_settings(provider: OAuthProviderKey) -> Value {
    match provider {
        OAuthProviderKey::Google => json!({
            "imapHost": "imap.gmail.com", "imapPort": 993, "imapSecure": true,
            "smtpHost": "smtp.gmail.com", "smtpPort": 465, "smtpSecure": true
        }),
        OAuthProviderKey::Microsoft => json!({
            "imapHost": "outlook.office365.com", "imapPort": 993, "imapSecure": true,
            "smtpHost": "smtp.office365.com", "smtpPort": 587, "smtpSecure": false
        }),
        OAuthProviderKey::Yahoo => json!({
            "imapHost": "imap.mail.yahoo.com", "imapPort": 993, "imapSecure": true,
            "smtpHost": "smtp.mail.yahoo.com", "smtpPort": 465, "smtpSecure": true
        }),
    }
}

fn secret_error<E: std::error::Error + Send + Sync + 'static>() -> ApplicationError<E> {
    domain("ACCOUNT_SECRET_UNAVAILABLE", 500, "邮箱账户凭据不可用")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::CredentialCodecError;
    use imail_mail::{ProtocolFailure, ProtocolStage};
    use imail_oauth::{OAuthIdentity, OAuthProviderKey, PendingOAuth};

    #[test]
    fn creates_an_oauth_account_without_exposing_proxy_password() {
        let mut repository = FakeRepository::default();
        let codec = JsonCodec;
        let connected = |_account: &AccountRecord| Ok(());
        let result = OAuthAccountService::new(&mut repository)
            .persist_completion(
                completion(None),
                "account-1",
                "2026-08-10T04:34:56.000Z",
                &codec,
                &connected,
            )
            .unwrap();

        assert_eq!(result.status, "connected");
        assert_eq!(result.settings["imapHost"], "imap.gmail.com");
        assert!(result.proxy.as_ref().unwrap().get("password").is_none());
        let stored = &repository.accounts[0];
        let secret: Value = serde_json::from_str(&stored.encrypted_secret).unwrap();
        assert_eq!(secret["proxyPassword"], "proxy-secret");
    }

    #[test]
    fn reconnect_preserves_proxy_password_and_rejects_email_switching() {
        let codec = JsonCodec;
        let connected = |_account: &AccountRecord| Ok(());
        let mut repository = FakeRepository {
            accounts: vec![existing_account()],
        };
        let result = OAuthAccountService::new(&mut repository)
            .persist_completion(
                completion(Some("account-1")),
                "ignored",
                "ignored",
                &codec,
                &connected,
            )
            .unwrap();
        let secret: Value = serde_json::from_str(&repository.accounts[0].encrypted_secret).unwrap();
        assert_eq!(result.id, "account-1");
        assert_eq!(secret["proxyPassword"], "old-proxy-secret");

        let mut switched = completion(Some("account-1"));
        switched.identity.email = "attacker@example.com".into();
        let error = OAuthAccountService::new(&mut repository)
            .persist_completion(switched, "ignored", "ignored", &codec, &connected)
            .unwrap_err();
        assert_eq!(error.code(), "OAUTH_EMAIL_MISMATCH");
    }

    #[test]
    fn connection_failure_is_persisted_as_a_safe_account_status() {
        let mut repository = FakeRepository::default();
        let failed = |_account: &AccountRecord| {
            Err(ProtocolFailure::from_provider(
                ProtocolStage::Imap,
                Some("NO"),
                "Bearer access-secret authentication failed",
            ))
        };
        let result = OAuthAccountService::new(&mut repository)
            .persist_completion(
                completion(None),
                "account-1",
                "2026-08-10T04:34:56.000Z",
                &JsonCodec,
                &failed,
            )
            .unwrap();

        assert_eq!(result.status, "error");
        assert!(!result.last_error.unwrap().contains("access-secret"));
        assert_eq!(repository.accounts[0].status, "error");
    }

    fn completion(account_id: Option<&str>) -> CompletedOAuth {
        CompletedOAuth {
            pending: PendingOAuth {
                owner_id: "user-1".into(),
                provider_key: OAuthProviderKey::Google,
                account_provider: "gmail".into(),
                code_verifier: "verifier".into(),
                nonce: "nonce".into(),
                created_at: 1,
                display_name: Some("Owner".into()),
                group: "个人".into(),
                color: "#168f78".into(),
                account_id: account_id.map(str::to_string),
                expected_email: account_id.map(|_| "owner@example.com".into()),
                proxy: Some(OAuthProxyInput {
                    protocol: "socks5".into(),
                    host: "127.0.0.1".into(),
                    port: 1080,
                    username: Some("mail".into()),
                    password: Some("proxy-secret".into()),
                }),
            },
            identity: OAuthIdentity {
                email: "owner@example.com".into(),
                name: Some("Owner".into()),
            },
            secret: OAuthAccountSecret {
                auth_type: "oauth2".into(),
                oauth_provider: OAuthProviderKey::Google,
                access_token: "access".into(),
                refresh_token: Some("refresh".into()),
                expires_at: "2026-08-10T05:34:56.000Z".into(),
                scopes: vec!["openid".into()],
                token_type: "Bearer".into(),
                proxy_password: None,
            },
        }
    }

    fn existing_account() -> AccountRecord {
        AccountRecord {
            id: "account-1".into(),
            owner_id: "user-1".into(),
            provider: "gmail".into(),
            email: "owner@example.com".into(),
            display_name: "Owner".into(),
            group: "个人".into(),
            group_icon: "folder".into(),
            color: "#168f78".into(),
            settings: provider_settings(OAuthProviderKey::Google),
            proxy: None,
            encrypted_secret: json!({
                "authType": "oauth2",
                "accessToken": "old",
                "proxyPassword": "old-proxy-secret"
            })
            .to_string(),
            auth_method: Some("oauth2".into()),
            created_at: "2026-08-01T00:00:00.000Z".into(),
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
    #[error("fake repository error")]
    struct FakeError;

    #[derive(Default)]
    struct FakeRepository {
        accounts: Vec<AccountRecord>,
    }

    impl AccountRepository for FakeRepository {
        type Error = FakeError;

        fn account(
            &self,
            user_id: &str,
            account_id: &str,
        ) -> Result<Option<AccountRecord>, Self::Error> {
            Ok(self
                .accounts
                .iter()
                .find(|account| account.owner_id == user_id && account.id == account_id)
                .cloned())
        }

        fn list_accounts(&self, user_id: &str) -> Result<Vec<AccountRecord>, Self::Error> {
            Ok(self
                .accounts
                .iter()
                .filter(|account| account.owner_id == user_id)
                .cloned()
                .collect())
        }

        fn insert_account_if_email_available(
            &mut self,
            account: &AccountRecord,
        ) -> Result<bool, Self::Error> {
            if self.accounts.iter().any(|current| {
                current.owner_id == account.owner_id
                    && current.email.eq_ignore_ascii_case(&account.email)
            }) {
                return Ok(false);
            }
            self.accounts.push(account.clone());
            Ok(true)
        }

        fn upsert_account(&mut self, account: &AccountRecord) -> Result<(), Self::Error> {
            if let Some(current) = self
                .accounts
                .iter_mut()
                .find(|current| current.id == account.id)
            {
                *current = account.clone();
            } else {
                self.accounts.push(account.clone());
            }
            Ok(())
        }

        fn delete_account(
            &mut self,
            _user_id: &str,
            _account_id: &str,
        ) -> Result<bool, Self::Error> {
            Ok(false)
        }

        fn user_metadata(&self, _user_id: &str, _key: &str) -> Result<Option<String>, Self::Error> {
            Ok(None)
        }

        fn set_user_metadata(
            &mut self,
            _user_id: &str,
            _key: &str,
            _value: &str,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }
}
