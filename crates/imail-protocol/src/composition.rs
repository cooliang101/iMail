use serde::{Deserialize, Serialize};

use crate::MailAddressView;

/// Plain text is intentional: consumers escape it before insertion into HTML.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct CompositionPreferences {
    pub signatures: Vec<AccountSignature>,
    pub templates: Vec<ComposeTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountSignature {
    pub account_id: String,
    pub text: String,
    pub new_messages: bool,
    pub replies: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposeTemplate {
    pub id: String,
    pub name: String,
    pub subject: String,
    pub text: String,
}

impl CompositionPreferences {
    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty() && self.templates.is_empty()
    }

    pub fn is_valid(&self) -> bool {
        use std::collections::HashSet;
        let mut accounts = HashSet::new();
        let mut templates = HashSet::new();
        self.signatures.len() <= 100
            && self.templates.len() <= 100
            && self.signatures.iter().all(|value| {
                valid_key(&value.account_id)
                    && accounts.insert(&value.account_id)
                    && value.text.len() <= 16_000
            })
            && self.templates.iter().all(|value| {
                valid_key(&value.id)
                    && templates.insert(&value.id)
                    && !value.name.trim().is_empty()
                    && value.name.len() <= 200
                    && value.subject.len() <= 1_000
                    && !value.subject.contains(['\r', '\n'])
                    && value.text.len() <= 64_000
            })
    }
}

fn valid_key(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
}

/// Reply identifiers are metadata, never raw injectable RFC 822 header lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReplyHeaders {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub in_reply_to: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
}

impl ReplyHeaders {
    pub fn is_valid(&self) -> bool {
        [&self.in_reply_to, &self.references]
            .iter()
            .all(|ids| ids.len() <= 100 && ids.iter().all(|id| normalize_message_id(id).is_some()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MailHeaders {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<MailAddressView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reply_to: Vec<MailAddressView>,
    #[serde(flatten)]
    pub reply: ReplyHeaders,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComposeEnvelope {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bcc: Vec<String>,
    #[serde(flatten)]
    pub reply: ReplyHeaders,
}

impl ComposeEnvelope {
    pub fn is_valid(&self) -> bool {
        self.bcc.len() <= 100
            && self.bcc.iter().all(|address| valid_mail_address(address))
            && self.reply.is_valid()
    }
}

pub fn valid_mail_address(address: &str) -> bool {
    let Some((local, domain)) = address.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && address.len() <= 320
        && !address
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';'))
}

/// Conservative identifier grammar. Malformed IDs never create conversation edges.
pub fn normalize_message_id(value: &str) -> Option<String> {
    let value = value.trim();
    let id = if value.starts_with('<') || value.ends_with('>') {
        value.strip_prefix('<')?.strip_suffix('>')?
    } else {
        value
    };
    let (local, domain) = id.rsplit_once('@')?;
    if local.is_empty()
        || domain.is_empty()
        || id.len() > 996
        || !id
            .bytes()
            .all(|c| c.is_ascii_graphic() && !matches!(c, b'<' | b'>' | b'"' | b'\\'))
    {
        return None;
    }
    Some(format!("<{id}>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_injected_and_ambiguous_reply_headers() {
        assert_eq!(
            normalize_message_id("parent@example.test"),
            Some("<parent@example.test>".into())
        );
        for id in [
            "<parent@example.test>\r\nBcc: hidden@example.test",
            "<a@b> <c@d>",
            "<a@b",
            "",
            "subject",
        ] {
            assert!(normalize_message_id(id).is_none(), "{id:?}");
        }
        assert!(!ComposeEnvelope {
            bcc: vec!["a@b\r\nX: value".into()],
            ..Default::default()
        }
        .is_valid());
    }
}
