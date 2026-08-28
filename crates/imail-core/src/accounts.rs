use std::collections::BTreeMap;

use imail_protocol::{AccountMetadataPatch, AccountProxyUpdate, AccountReadModel};
use serde_json::{json, Map, Value};

use crate::{AccountRecord, AccountRepository, ApplicationError, AuthRepository};

#[derive(Debug, thiserror::Error)]
#[error("账户凭据编解码失败")]
pub struct CredentialCodecError;

#[derive(Debug, Default, thiserror::Error)]
#[error("邮箱连接验证失败")]
pub struct CredentialValidationError {
    public_message: Option<&'static str>,
}

impl CredentialValidationError {
    pub fn with_public_message(message: &'static str) -> Self {
        Self {
            public_message: Some(message),
        }
    }

    fn public_message_or(&self, fallback: &'static str) -> &'static str {
        self.public_message.unwrap_or(fallback)
    }
}

pub trait AccountSecretCodec {
    fn decrypt(&self, payload: &str) -> Result<Value, CredentialCodecError>;
    fn encrypt(&self, value: &Value) -> Result<String, CredentialCodecError>;
}

pub trait AccountConnectionValidator {
    fn validate(&self, candidate: &AccountRecord) -> Result<(), CredentialValidationError>;
}

impl<F> AccountConnectionValidator for F
where
    F: Fn(&AccountRecord) -> Result<(), CredentialValidationError>,
{
    fn validate(&self, candidate: &AccountRecord) -> Result<(), CredentialValidationError> {
        self(candidate)
    }
}

pub struct AccountService<'a, R: AccountRepository> {
    repository: &'a mut R,
}

impl<'a, R: AccountRepository> AccountService<'a, R> {
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn list(&self, user_id: &str) -> Result<Vec<AccountReadModel>, ApplicationError<R::Error>> {
        self.repository
            .list_accounts(user_id)
            .map(|accounts| accounts.iter().map(public_account).collect())
            .map_err(ApplicationError::Repository)
    }

    pub fn create<V: AccountConnectionValidator>(
        &mut self,
        mut account: AccountRecord,
        validator: &V,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        account.status = "syncing".into();
        account.last_error = None;
        validator.validate(&account).map_err(|cause| {
            domain(
                "ACCOUNT_CONNECTION_FAILED",
                422,
                cause.public_message_or("邮箱连接验证失败，未保存账户"),
            )
        })?;
        account.status = "connected".into();
        if !self
            .repository
            .insert_account_if_email_available(&account)
            .map_err(ApplicationError::Repository)?
        {
            return Err(domain("ACCOUNT_EXISTS", 409, "这个邮箱已经添加"));
        }
        Ok(public_account(&account))
    }

    pub fn get(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        self.account_record(user_id, account_id)
            .map(|account| public_account(&account))
    }

    pub fn update_metadata(
        &mut self,
        user_id: &str,
        account_id: &str,
        patch: AccountMetadataPatch,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        validate_patch::<R::Error>(&patch)?;
        let mut account = self.account_record(user_id, account_id)?;
        if let Some(value) = patch.display_name {
            account.display_name = value.trim().to_string();
        }
        if let Some(value) = patch.group {
            account.group = value.trim().to_string();
        }
        if let Some(value) = patch.group_icon {
            account.group_icon = value.as_str().to_string();
        }
        if let Some(value) = patch.color {
            account.color = value;
        }
        self.repository
            .upsert_account(&account)
            .map_err(ApplicationError::Repository)?;
        Ok(public_account(&account))
    }

    pub fn remove(
        &mut self,
        user_id: &str,
        account_id: &str,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        let account = self.account_record(user_id, account_id)?;
        if !self
            .repository
            .delete_account(user_id, account_id)
            .map_err(ApplicationError::Repository)?
        {
            return Err(domain("ACCOUNT_NOT_FOUND", 404, "邮箱账户已被移除"));
        }
        Ok(public_account(&account))
    }

    pub fn remove_with_audit(
        &mut self,
        user_id: &str,
        actor: &str,
        account_id: &str,
    ) -> Result<AccountReadModel, ApplicationError<<R as AccountRepository>::Error>>
    where
        R: AuthRepository<Error = <R as AccountRepository>::Error>,
    {
        let account = self.remove(user_id, account_id)?;
        self.repository
            .record_security_event(
                "account.removed",
                actor,
                Some(user_id),
                &BTreeMap::from([("accountId".into(), account_id.to_string())]),
            )
            .map_err(ApplicationError::Repository)?;
        Ok(account)
    }

    pub fn replace_password<C: AccountSecretCodec, V: AccountConnectionValidator>(
        &mut self,
        user_id: &str,
        account_id: &str,
        password: &str,
        codec: &C,
        validator: &V,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        if password.is_empty() || password.encode_utf16().count() > 512 {
            return Err(domain(
                "ACCOUNT_CREDENTIAL_INVALID",
                400,
                "邮箱密码格式无效",
            ));
        }
        let mut account = self.account_record(user_id, account_id)?;
        if account.auth_method.as_deref() == Some("oauth2") {
            return Err(domain(
                "OAUTH_RECONNECT_REQUIRED",
                409,
                "OAuth 邮箱请使用重新授权",
            ));
        }
        let current = decode_secret::<R::Error, C>(codec, &account.encrypted_secret)?;
        let mut next = Map::new();
        next.insert("authType".into(), Value::String("app-password".into()));
        next.insert("password".into(), Value::String(password.to_string()));
        if let Some(proxy_password) = current.get("proxyPassword") {
            next.insert("proxyPassword".into(), proxy_password.clone());
        }
        account.encrypted_secret = encode_secret::<R::Error, C>(codec, &Value::Object(next))?;
        account.auth_method = Some("app-password".into());
        validate_and_persist(self.repository, account, validator)
    }

    pub fn update_proxy<C: AccountSecretCodec, V: AccountConnectionValidator>(
        &mut self,
        user_id: &str,
        account_id: &str,
        input: AccountProxyUpdate,
        codec: &C,
        validator: &V,
    ) -> Result<AccountReadModel, ApplicationError<R::Error>> {
        let mut account = self.account_record(user_id, account_id)?;
        let mut secret = decode_secret::<R::Error, C>(codec, &account.encrypted_secret)?;
        let secret_object = secret.as_object_mut().ok_or_else(secret_error)?;
        match input {
            AccountProxyUpdate::Disabled => {
                account.proxy = None;
                secret_object.remove("proxyPassword");
            }
            AccountProxyUpdate::CopyFrom { source_account_id } => {
                if source_account_id == account_id {
                    return Err(domain(
                        "PROXY_SOURCE_SAME_ACCOUNT",
                        400,
                        "不能从当前邮箱复制代理",
                    ));
                }
                let source = self.account_record(user_id, &source_account_id)?;
                account.proxy = Some(source.proxy.clone().ok_or_else(|| {
                    domain("PROXY_SOURCE_MISSING", 400, "所选邮箱没有可复用的代理配置")
                })?);
                let source_secret = decode_secret::<R::Error, C>(codec, &source.encrypted_secret)?;
                if let Some(value) = source_secret.get("proxyPassword") {
                    secret_object.insert("proxyPassword".into(), value.clone());
                } else {
                    secret_object.remove("proxyPassword");
                }
            }
            AccountProxyUpdate::Explicit {
                protocol,
                host,
                port,
                username,
                password,
            } => {
                let host = host.trim();
                let username = username
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                if !valid_proxy_host(host)
                    || port == 0
                    || username
                        .as_ref()
                        .is_some_and(|value| value.encode_utf16().count() > 256)
                    || password
                        .as_ref()
                        .is_some_and(|value| value.encode_utf16().count() > 512)
                {
                    return Err(domain("PROXY_INVALID", 400, "代理配置格式无效"));
                }
                let mut proxy = Map::from_iter([
                    ("protocol".into(), Value::String(protocol.as_str().into())),
                    ("host".into(), Value::String(host.into())),
                    ("port".into(), json!(port)),
                ]);
                if let Some(username) = username {
                    proxy.insert("username".into(), Value::String(username));
                }
                account.proxy = Some(Value::Object(proxy));
                if let Some(password) = password {
                    if password.is_empty() {
                        secret_object.remove("proxyPassword");
                    } else {
                        secret_object.insert("proxyPassword".into(), Value::String(password));
                    }
                }
            }
        }
        account.encrypted_secret = encode_secret::<R::Error, C>(codec, &secret)?;
        validate_and_persist(self.repository, account, validator)
    }

    fn account_record(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<AccountRecord, ApplicationError<R::Error>> {
        self.repository
            .account(user_id, account_id)
            .map_err(ApplicationError::Repository)?
            .ok_or_else(|| domain("ACCOUNT_NOT_FOUND", 404, "邮箱账户不存在"))
    }
}

fn validate_and_persist<R: AccountRepository, V: AccountConnectionValidator>(
    repository: &mut R,
    mut account: AccountRecord,
    validator: &V,
) -> Result<AccountReadModel, ApplicationError<R::Error>> {
    account.status = "syncing".into();
    account.last_error = None;
    validator.validate(&account).map_err(|cause| {
        domain(
            "ACCOUNT_CONNECTION_FAILED",
            422,
            cause.public_message_or("邮箱连接验证失败，未保存更改"),
        )
    })?;
    account.status = "connected".into();
    repository
        .upsert_account(&account)
        .map_err(ApplicationError::Repository)?;
    Ok(public_account(&account))
}

fn decode_secret<E, C>(codec: &C, payload: &str) -> Result<Value, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
    C: AccountSecretCodec,
{
    codec.decrypt(payload).map_err(|_| secret_error())
}

fn encode_secret<E, C>(codec: &C, value: &Value) -> Result<String, ApplicationError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
    C: AccountSecretCodec,
{
    codec.encrypt(value).map_err(|_| secret_error())
}

fn secret_error<E: std::error::Error + Send + Sync + 'static>() -> ApplicationError<E> {
    domain("ACCOUNT_SECRET_UNAVAILABLE", 500, "邮箱账户凭据不可用")
}

fn valid_proxy_host(value: &str) -> bool {
    !value.is_empty()
        && value.encode_utf16().count() <= 253
        && !value.contains("://")
        && !value
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '/' | '?' | '#'))
}

pub fn public_account(account: &AccountRecord) -> AccountReadModel {
    AccountReadModel {
        id: account.id.clone(),
        owner_id: account.owner_id.clone(),
        provider: account.provider.clone(),
        email: account.email.clone(),
        display_name: account.display_name.clone(),
        group: account.group.clone(),
        group_icon: account.group_icon.clone(),
        color: account.color.clone(),
        settings: account.settings.clone(),
        proxy: account.proxy.clone(),
        auth_method: account.auth_method.clone(),
        created_at: account.created_at.clone(),
        last_sync_at: account.last_sync_at.clone(),
        status: account.status.clone(),
        last_error: account.last_error.clone(),
        mailboxes: account.mailboxes.clone(),
    }
}

fn validate_patch<E: std::error::Error + Send + Sync + 'static>(
    patch: &AccountMetadataPatch,
) -> Result<(), ApplicationError<E>> {
    if patch.display_name.is_none()
        && patch.group.is_none()
        && patch.group_icon.is_none()
        && patch.color.is_none()
    {
        return Err(domain(
            "ACCOUNT_METADATA_EMPTY",
            400,
            "至少提供一个要更新的字段",
        ));
    }
    if patch
        .display_name
        .as_ref()
        .is_some_and(|value| invalid_text(value, 80))
        || patch
            .group
            .as_ref()
            .is_some_and(|value| invalid_text(value, 40))
        || patch
            .color
            .as_ref()
            .is_some_and(|value| !valid_color(value))
    {
        return Err(domain(
            "ACCOUNT_METADATA_INVALID",
            400,
            "邮箱账户元数据格式无效",
        ));
    }
    Ok(())
}

fn invalid_text(value: &str, maximum: usize) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed.encode_utf16().count() > maximum
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
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
