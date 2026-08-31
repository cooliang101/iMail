//! Portable, owner-scoped saved searches. Conditions are data, never SQL or FTS syntax.
use chrono::DateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchFilters {
    pub account_ids: Vec<String>,
    pub group: Option<String>,
    pub q: Option<String>,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub since: Option<String>,
    pub before: Option<String>,
    pub unread: Option<bool>,
    pub flagged: Option<bool>,
    pub has_attachments: Option<bool>,
    pub labels: Vec<String>,
    pub mailbox_role: Option<String>,
    pub mailbox: Option<String>,
    pub mailbox_name: Option<String>,
    pub snoozed: Option<bool>,
}

impl SearchFilters {
    pub fn validate(&self) -> Result<(), &'static str> {
        for (value, max) in [
            (&self.group, 200),
            (&self.q, 200),
            (&self.subject, 200),
            (&self.body, 200),
            (&self.sender, 320),
            (&self.recipient, 320),
            (&self.mailbox, 500),
            (&self.mailbox_name, 500),
        ] {
            if value.as_ref().is_some_and(|v| !valid_text(v, max)) {
                return Err("搜索条件为空、过长或包含控制字符");
            }
        }
        if self.account_ids.len() > 100
            || self.account_ids.iter().any(|v| !valid_text(v, 200))
            || self.labels.len() > 30
            || self.labels.iter().any(|v| !valid_text(v, 80))
        {
            return Err("搜索账户或标签条件无效");
        }
        for address in [&self.sender, &self.recipient].into_iter().flatten() {
            if !address.contains('@') || address.chars().any(char::is_whitespace) {
                return Err("发件人和收件人条件需要完整邮箱地址");
            }
        }
        if self.mailbox_role.as_deref().is_some_and(|role| {
            !matches!(
                role,
                "inbox" | "sent" | "archive" | "drafts" | "trash" | "junk" | "custom"
            )
        }) {
            return Err("搜索文件夹类型无效");
        }
        let parse = |value: &Option<String>| {
            value
                .as_ref()
                .map(|v| DateTime::parse_from_rfc3339(v))
                .transpose()
        };
        let since = parse(&self.since).map_err(|_| "开始时间需要 RFC 3339 时间戳")?;
        let before = parse(&self.before).map_err(|_| "结束时间需要 RFC 3339 时间戳")?;
        if since.zip(before).is_some_and(|(start, end)| start >= end) {
            return Err("结束时间必须晚于开始时间");
        }
        Ok(())
    }
}

pub fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.encode_utf16().count() <= max
        && !value.chars().any(char::is_control)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmartFolder {
    pub id: String,
    pub name: String,
    pub filters: SearchFilters,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmartFolderInput {
    pub name: String,
    pub filters: SearchFilters,
}

impl SmartFolderInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_text(&self.name, 80) {
            return Err("智能文件夹名称需要 1–80 个字符");
        }
        self.filters.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_filters_instead_of_silently_broadening_search() {
        assert!(serde_json::from_str::<SearchFilters>(r#"{"unkown":true}"#).is_err());
        assert!(SearchFilters {
            body: Some(" ".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(SearchFilters {
            sender: Some("not-an-email".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(SearchFilters {
            since: Some("2026-02-30T00:00:00Z".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn dates_compare_instants_and_false_is_not_an_omitted_condition() {
        let filters = SearchFilters {
            unread: Some(false),
            since: Some("2026-08-31T08:00:00+08:00".into()),
            before: Some("2026-08-31T01:00:00Z".into()),
            ..Default::default()
        };
        assert!(filters.validate().is_ok());
        assert_eq!(serde_json::to_value(&filters).unwrap()["unread"], false);
        assert!(SearchFilters {
            before: Some("2026-08-31T00:00:00Z".into()),
            ..filters
        }
        .validate()
        .is_err());
    }
}
