use imail_protocol::MessageReadModel;
use serde::Serialize;

use crate::{ApplicationError, MessageRepository};

mod conversation;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageQuery {
    pub account_id: Option<String>,
    pub group: Option<String>,
    pub text: Option<String>,
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub unread: bool,
    pub flagged: bool,
    pub has_attachments: bool,
    pub mailbox_role: Option<String>,
    pub mailbox: Option<String>,
    pub mailbox_name: Option<String>,
    pub snoozed: bool,
    pub label: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub cursor: Option<GatewayMessageCursor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessagePage {
    pub messages: Vec<MessageReadModel>,
    pub total: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayMessageCursor {
    pub date: String,
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayMessageQuery {
    pub account_ids: Vec<String>,
    pub recipient: Option<String>,
    pub mailbox_role: Option<String>,
    pub unread: Option<bool>,
    pub since: Option<String>,
    pub before: Option<String>,
    pub text: Option<String>,
    pub cursor: Option<GatewayMessageCursor>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayMessagePage {
    pub messages: Vec<MessageReadModel>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageStatsBucket {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub total: usize,
    pub unread: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageStats {
    pub total: usize,
    pub unread: usize,
    pub by_account: Vec<MessageStatsBucket>,
    pub by_group: Vec<MessageStatsBucket>,
}

pub struct MessageQueryService<'a, R: MessageRepository> {
    repository: &'a R,
}

impl<'a, R: MessageRepository> MessageQueryService<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub fn conversation(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Vec<MessageReadModel>, ApplicationError<R::Error>> {
        let candidates = self
            .repository
            .conversation_candidates(user_id)
            .map_err(ApplicationError::Repository)?;
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for candidate in &candidates {
            if let Some(id) = candidate
                .message_id
                .as_deref()
                .and_then(imail_protocol::normalize_message_id)
            {
                *counts.entry(id).or_default() += 1;
            }
        }
        // Only duplicates need cached body comparison. Retain hashes, never all bodies.
        let mut fingerprints = std::collections::HashMap::new();
        for candidate in &candidates {
            if candidate
                .message_id
                .as_deref()
                .and_then(imail_protocol::normalize_message_id)
                .is_some_and(|id| counts.get(&id).copied().unwrap_or_default() > 1)
            {
                let full = self.get(user_id, &candidate.id)?;
                fingerprints.insert(candidate.id.clone(), conversation::fingerprint(&full));
            }
        }
        conversation::related(candidates, message_id, &fingerprints).ok_or(
            ApplicationError::Domain {
                code: "MESSAGE_NOT_FOUND",
                status: 404,
                message: "邮件不存在",
            },
        )
    }

    pub fn query(
        &self,
        user_id: &str,
        query: &MessageQuery,
        now: &str,
    ) -> Result<MessagePage, ApplicationError<R::Error>> {
        self.repository
            .query_messages(user_id, query, now)
            .map_err(ApplicationError::Repository)
    }

    pub fn get(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<MessageReadModel, ApplicationError<R::Error>> {
        self.repository
            .message(user_id, message_id)
            .map_err(ApplicationError::Repository)?
            .ok_or(ApplicationError::Domain {
                code: "MESSAGE_NOT_FOUND",
                status: 404,
                message: "邮件不存在",
            })
    }

    pub fn source(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Option<Vec<u8>>, ApplicationError<R::Error>> {
        self.get(user_id, message_id)?;
        self.repository
            .message_source(user_id, message_id)
            .map_err(ApplicationError::Repository)
    }

    pub fn stats(
        &self,
        user_id: &str,
        now: &str,
    ) -> Result<MessageStats, ApplicationError<R::Error>> {
        self.repository
            .message_stats(user_id, now)
            .map_err(ApplicationError::Repository)
    }
}
