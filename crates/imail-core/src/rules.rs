//! Deterministic mail rules. Matching always uses the original message snapshot.
use imail_protocol::MessageReadModel;
use serde::{Deserialize, Serialize};

use crate::search::valid_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleMatchMode {
    All,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "field",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RuleCondition {
    Sender(String),
    SenderDomain(String),
    Recipient(String),
    SubjectContains(String),
    BodyContains(String),
    HasAttachments(bool),
    Unread(bool),
    Flagged(bool),
    MailboxRole(String),
    Label(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RuleAction {
    AddLabel(String),
    RemoveLabel(String),
    MarkRead(bool),
    Flag(bool),
    Mute(bool),
    Archive,
}

impl RuleAction {
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::MarkRead(_) | Self::Flag(_) | Self::Archive)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailRuleInput {
    pub name: String,
    pub enabled: bool,
    /// Lower numbers run first; equal priorities use creation order, then ID.
    pub priority: u32,
    pub account_ids: Vec<String>,
    pub match_mode: RuleMatchMode,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    pub stop_processing: bool,
}

impl MailRuleInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.name, 80) || self.priority > 10000 {
            return Err("规则名称需要 1–80 个字符，优先级需要 0–10000");
        }
        if self.account_ids.len() > 100
            || self.account_ids.iter().any(|id| !valid_text(id, 200))
            || !(1..=20).contains(&self.conditions.len())
            || !(1..=20).contains(&self.actions.len())
        {
            return Err("规则需要 1–20 个条件和动作，最多选择 100 个账户");
        }
        for condition in &self.conditions {
            let valid = match condition {
                RuleCondition::Sender(v) | RuleCondition::Recipient(v) => {
                    imail_protocol::valid_mail_address(v)
                }
                RuleCondition::SenderDomain(v) => {
                    valid_text(v, 253)
                        && v.contains('.')
                        && v.split('.').all(|part| {
                            !part.is_empty()
                                && part.len() <= 63
                                && !part.starts_with('-')
                                && !part.ends_with('-')
                                && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                        })
                }
                RuleCondition::SubjectContains(v) | RuleCondition::BodyContains(v) => {
                    valid_text(v, 200)
                }
                RuleCondition::Label(v) => valid_text(v, 80),
                RuleCondition::MailboxRole(v) => matches!(
                    v.as_str(),
                    "inbox" | "sent" | "archive" | "drafts" | "trash" | "junk" | "custom"
                ),
                _ => true,
            };
            if !valid {
                return Err("规则匹配条件无效");
            }
        }
        for (index, action) in self.actions.iter().enumerate() {
            match action {
                RuleAction::AddLabel(v) | RuleAction::RemoveLabel(v) if !valid_text(v, 80) => {
                    return Err("标签需要 1–80 个字符")
                }
                // A move changes the remote locator. Keep it last, once per rule.
                RuleAction::Archive if index + 1 != self.actions.len() => {
                    return Err("归档必须是规则的最后一个动作")
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn matches(&self, message: &MessageReadModel) -> bool {
        if !self.account_ids.is_empty() && !self.account_ids.contains(&message.account_id) {
            return false;
        }
        // An invalid/empty imported rule must never accidentally match everything.
        if self.validate().is_err() {
            return false;
        }
        match self.match_mode {
            RuleMatchMode::All => self.conditions.iter().all(|c| c.matches(message)),
            RuleMatchMode::Any => self.conditions.iter().any(|c| c.matches(message)),
        }
    }
}

fn address(value: &serde_json::Value) -> &str {
    value
        .as_str()
        .or_else(|| value.get("address").and_then(|v| v.as_str()))
        .unwrap_or_default()
}

impl RuleCondition {
    pub fn matches(&self, message: &MessageReadModel) -> bool {
        match self {
            Self::Sender(v) => address(&message.from).eq_ignore_ascii_case(v.trim()),
            Self::SenderDomain(v) => address(&message.from)
                .rsplit_once('@')
                .is_some_and(|(_, domain)| domain.eq_ignore_ascii_case(v)),
            Self::Recipient(v) => {
                message
                    .to
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|a| address(a).eq_ignore_ascii_case(v.trim()))
                    || message
                        .headers
                        .cc
                        .iter()
                        .any(|a| a.address.eq_ignore_ascii_case(v.trim()))
            }
            Self::SubjectContains(v) => message
                .subject
                .to_lowercase()
                .contains(&v.trim().to_lowercase()),
            Self::BodyContains(v) => message
                .text
                .to_lowercase()
                .contains(&v.trim().to_lowercase()),
            Self::HasAttachments(v) => message.has_attachments == *v,
            Self::Unread(v) => message.unread == *v,
            Self::Flagged(v) => message.flagged == *v,
            Self::MailboxRole(v) => message.mailbox_role == *v,
            Self::Label(v) => message
                .labels
                .as_array()
                .into_iter()
                .flatten()
                .any(|label| label.as_str() == Some(v)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailRule {
    pub id: String,
    pub revision: u32,
    #[serde(flatten)]
    pub input: MailRuleInput,
    pub created_at: String,
    pub updated_at: String,
}

/// Enabled rules match against one immutable snapshot, never earlier rule actions.
pub fn matching_rules<'a>(rules: &'a [MailRule], message: &MessageReadModel) -> Vec<&'a MailRule> {
    let mut ordered = rules.iter().filter(|r| r.input.enabled).collect::<Vec<_>>();
    ordered.sort_by(|a, b| {
        (a.input.priority, &a.created_at, &a.id).cmp(&(b.input.priority, &b.created_at, &b.id))
    });
    let mut matched = Vec::new();
    for rule in ordered {
        if rule.input.matches(message) {
            matched.push(rule);
            if rule.input.stop_processing {
                break;
            }
        }
    }
    matched
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRun {
    pub id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub revision: u32,
    pub account_id: String,
    pub message_id: String,
    pub source: String,
    pub status: String,
    pub completed_actions: usize,
    pub total_actions: usize,
    pub error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
