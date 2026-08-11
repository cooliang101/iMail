use std::collections::{BTreeMap, BTreeSet, HashMap};

use imail_protocol::DeveloperTokenReadModel;
use serde::{Deserialize, Serialize};

use crate::{AccountRepository, ApplicationError, AuthRepository, DeveloperTokenRepository};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDeveloperTokenInput {
    pub name: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub mailboxes: Vec<String>,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDeveloperToken {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub mailboxes: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IssuedPublicDeveloperToken {
    pub token: String,
    pub detail: PublicDeveloperToken,
}

pub struct DeveloperTokenService<'a, R> {
    repository: &'a mut R,
}

impl<'a, R> DeveloperTokenService<'a, R>
where
    R: AccountRepository
        + AuthRepository<Error = <R as AccountRepository>::Error>
        + DeveloperTokenRepository<Error = <R as AccountRepository>::Error>,
{
    pub fn new(repository: &'a mut R) -> Self {
        Self { repository }
    }

    pub fn list(
        &self,
        user_id: &str,
    ) -> Result<Vec<PublicDeveloperToken>, ApplicationError<<R as AccountRepository>::Error>> {
        let emails = self.account_emails(user_id)?;
        self.repository
            .list_developer_tokens(user_id)
            .map_err(ApplicationError::Repository)
            .map(|tokens| {
                tokens
                    .into_iter()
                    .map(|token| public_token(token, &emails))
                    .collect()
            })
    }

    pub fn create(
        &mut self,
        user_id: &str,
        actor: &str,
        input: CreateDeveloperTokenInput,
    ) -> Result<IssuedPublicDeveloperToken, ApplicationError<<R as AccountRepository>::Error>> {
        validate_input(&input)?;
        let accounts = self
            .repository
            .list_accounts(user_id)
            .map_err(ApplicationError::Repository)?;
        let by_email = accounts
            .iter()
            .map(|account| (account.email.to_ascii_lowercase(), account))
            .collect::<HashMap<_, _>>();
        let requested = input
            .mailboxes
            .iter()
            .map(|email| email.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if requested.iter().any(|email| !by_email.contains_key(email)) {
            return Err(domain(
                "DEVELOPER_TOKEN_MAILBOX_NOT_FOUND",
                "包含不存在的邮箱账户",
            ));
        }
        let mcp = input.scopes.iter().any(|scope| scope == "mcp:full");
        if !mcp && requested.is_empty() {
            return Err(domain(
                "DEVELOPER_TOKEN_MAILBOX_REQUIRED",
                "非 MCP Token 至少需要选择一个邮箱",
            ));
        }
        let scopes = if mcp {
            vec!["mcp:full".to_string()]
        } else {
            input
                .scopes
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        let account_ids = if mcp {
            accounts
                .iter()
                .map(|account| account.id.clone())
                .collect::<Vec<_>>()
        } else {
            requested
                .iter()
                .filter_map(|email| by_email.get(email).map(|account| account.id.clone()))
                .collect::<Vec<_>>()
        };
        let issued = self
            .repository
            .issue_developer_token(
                user_id,
                &input.name,
                &scopes,
                &account_ids,
                input.ttl_seconds,
            )
            .map_err(ApplicationError::Repository)?;
        let emails = accounts
            .into_iter()
            .map(|account| (account.id, account.email))
            .collect::<HashMap<_, _>>();
        self.repository
            .record_security_event(
                "developer-token.created",
                actor,
                Some(user_id),
                &BTreeMap::from([
                    ("tokenId".into(), issued.token.id.clone()),
                    ("scopes".into(), scopes.join(",")),
                    ("mailboxCount".into(), account_ids.len().to_string()),
                ]),
            )
            .map_err(ApplicationError::Repository)?;
        Ok(IssuedPublicDeveloperToken {
            token: issued.raw,
            detail: public_token(issued.token, &emails),
        })
    }

    pub fn revoke(
        &mut self,
        user_id: &str,
        actor: &str,
        token_id: &str,
    ) -> Result<(), ApplicationError<<R as AccountRepository>::Error>> {
        self.repository
            .revoke_developer_token(user_id, token_id)
            .map_err(ApplicationError::Repository)?;
        self.repository
            .record_security_event(
                "developer-token.revoked",
                actor,
                Some(user_id),
                &BTreeMap::from([("tokenId".into(), token_id.to_string())]),
            )
            .map_err(ApplicationError::Repository)
    }

    fn account_emails(
        &self,
        user_id: &str,
    ) -> Result<HashMap<String, String>, ApplicationError<<R as AccountRepository>::Error>> {
        self.repository
            .list_accounts(user_id)
            .map_err(ApplicationError::Repository)
            .map(|accounts| {
                accounts
                    .into_iter()
                    .map(|account| (account.id, account.email))
                    .collect()
            })
    }
}

fn validate_input<E: std::error::Error + Send + Sync + 'static>(
    input: &CreateDeveloperTokenInput,
) -> Result<(), ApplicationError<E>> {
    if input.name.is_empty()
        || input.name.chars().count() > 80
        || input.scopes.is_empty()
        || !(300..=7 * 24 * 3600).contains(&input.ttl_seconds)
        || input.mailboxes.iter().any(|email| !valid_email(email))
        || input.scopes.iter().any(|scope| {
            !matches!(
                scope.as_str(),
                "messages:read" | "messages:send" | "accounts:read" | "mcp:full"
            )
        })
    {
        return Err(domain("DEVELOPER_TOKEN_INPUT_INVALID", "请求参数无效"));
    }
    Ok(())
}

fn public_token(
    token: DeveloperTokenReadModel,
    emails: &HashMap<String, String>,
) -> PublicDeveloperToken {
    PublicDeveloperToken {
        id: token.id,
        name: token.name,
        prefix: token.prefix,
        scopes: token.scopes,
        mailboxes: token
            .account_ids
            .into_iter()
            .filter_map(|id| emails.get(&id).cloned())
            .collect(),
        created_at: token.created_at,
        expires_at: token.expires_at,
        last_used_at: token.last_used_at,
    }
}

fn valid_email(value: &str) -> bool {
    let value = value.trim();
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !value.chars().any(char::is_whitespace)
        && value.chars().count() <= 320
}

fn domain<E: std::error::Error + Send + Sync + 'static>(
    code: &'static str,
    message: &'static str,
) -> ApplicationError<E> {
    ApplicationError::Domain {
        code,
        status: 400,
        message,
    }
}
