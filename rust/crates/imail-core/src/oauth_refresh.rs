use std::error::Error;

use imail_mail::MailConnectionConfig;
use imail_oauth::{
    refresh_account_secret, token_needs_refresh, OAuthAccountSecret, OAuthConfigResolver,
    OAuthEnvironment, OAuthError, OAuthProviderPort, RefreshCoordinator,
    StandardOAuthConfigResolver,
};

use crate::{
    accounts::AccountSecretCodec,
    mail_operations::{connection_config, MailApplicationError},
    AccountRepository,
};

#[derive(Debug, thiserror::Error)]
pub enum RefreshingConnectionError<E>
where
    E: Error + Send + Sync + 'static,
{
    #[error("邮箱账户不存在")]
    AccountNotFound,
    #[error("邮箱账户凭据不可用")]
    CredentialUnavailable,
    #[error("OAuth 账户凭据格式无效")]
    InvalidOAuthSecret,
    #[error(transparent)]
    Repository(E),
    #[error(transparent)]
    OAuth(#[from] OAuthError),
    #[error(transparent)]
    Configuration(MailApplicationError<E>),
}

pub struct RefreshingConnectionService<'a, R, C, P>
where
    R: AccountRepository,
    C: AccountSecretCodec,
    P: OAuthProviderPort + ?Sized,
{
    repository: &'a mut R,
    codec: &'a C,
    provider: &'a mut P,
    coordinator: &'a RefreshCoordinator,
    environment: &'a OAuthEnvironment,
    config_resolver: &'a dyn OAuthConfigResolver,
}

impl<'a, R, C, P> RefreshingConnectionService<'a, R, C, P>
where
    R: AccountRepository,
    C: AccountSecretCodec,
    P: OAuthProviderPort + ?Sized,
{
    pub fn new(
        repository: &'a mut R,
        codec: &'a C,
        provider: &'a mut P,
        coordinator: &'a RefreshCoordinator,
        environment: &'a OAuthEnvironment,
    ) -> Self {
        static STANDARD_RESOLVER: StandardOAuthConfigResolver = StandardOAuthConfigResolver;
        Self::new_with_config_resolver(
            repository,
            codec,
            provider,
            coordinator,
            environment,
            &STANDARD_RESOLVER,
        )
    }

    pub fn new_with_config_resolver(
        repository: &'a mut R,
        codec: &'a C,
        provider: &'a mut P,
        coordinator: &'a RefreshCoordinator,
        environment: &'a OAuthEnvironment,
        config_resolver: &'a dyn OAuthConfigResolver,
    ) -> Self {
        Self {
            repository,
            codec,
            provider,
            coordinator,
            environment,
            config_resolver,
        }
    }

    pub fn resolve(
        &mut self,
        user_id: &str,
        account_id: &str,
        now_ms: i64,
    ) -> Result<MailConnectionConfig, RefreshingConnectionError<R::Error>> {
        let mut account = self
            .repository
            .account(user_id, account_id)
            .map_err(RefreshingConnectionError::Repository)?
            .ok_or(RefreshingConnectionError::AccountNotFound)?;
        let value = self
            .codec
            .decrypt(&account.encrypted_secret)
            .map_err(|_| RefreshingConnectionError::CredentialUnavailable)?;
        if value.get("authType").and_then(serde_json::Value::as_str) == Some("oauth2")
            && value.get("expiresAt").is_some()
        {
            let current: OAuthAccountSecret = serde_json::from_value(value)
                .map_err(|_| RefreshingConnectionError::InvalidOAuthSecret)?;
            if token_needs_refresh(&current, now_ms) {
                let config = self.config_resolver.resolve(
                    self.environment,
                    current.oauth_provider,
                    Some(&account.provider),
                );
                if !config.configured {
                    return Err(RefreshingConnectionError::OAuth(OAuthError::Configuration(
                        config.configuration_hint,
                    )));
                }
                let provider = &mut self.provider;
                let refreshed = self.coordinator.run(account_id, || {
                    refresh_account_secret(*provider, &config, &current, now_ms)
                })?;
                let refreshed_value = serde_json::to_value(&refreshed)
                    .map_err(|_| RefreshingConnectionError::InvalidOAuthSecret)?;
                account.encrypted_secret = self
                    .codec
                    .encrypt(&refreshed_value)
                    .map_err(|_| RefreshingConnectionError::CredentialUnavailable)?;
                self.repository
                    .upsert_account(&account)
                    .map_err(RefreshingConnectionError::Repository)?;
            }
        }
        connection_config::<R::Error, C>(&account, self.codec)
            .map_err(RefreshingConnectionError::Configuration)
    }
}
