use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use imail_protocol::MessageReadModel;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{parse_rfc822, MailConnectionConfig, MailParseError, ProtocolFailure};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncCursor {
    pub uid_validity: Option<String>,
    pub last_seen_uid: i64,
    pub highest_modseq: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncTarget {
    pub mailbox_role: String,
    pub requested_mailbox: Option<String>,
}

pub fn mailbox_role_for(path: &str, special_use: Option<&str>) -> String {
    match special_use.map(str::to_ascii_lowercase).as_deref() {
        Some("\\sent") => return "sent".into(),
        Some("\\archive") | Some("\\all") => return "archive".into(),
        Some("\\drafts") => return "drafts".into(),
        Some("\\trash") => return "trash".into(),
        Some("\\junk") => return "junk".into(),
        _ => {}
    }
    let leaf = path
        .rsplit(['/', '.', '\\'])
        .next()
        .unwrap_or(path)
        .trim()
        .to_lowercase();
    match leaf.as_str() {
        "inbox" | "收件箱" => "inbox",
        "sent" | "sent items" | "sent messages" | "sent mail" | "已发送" | "已发送邮件"
        | "发件箱" => "sent",
        "archive" | "archives" | "all mail" | "归档" | "归档邮件" | "所有邮件" => {
            "archive"
        }
        "draft" | "drafts" | "草稿" | "草稿箱" => "drafts",
        "trash" | "bin" | "deleted" | "deleted items" | "已删除" | "已删除邮件" | "废纸篓"
        | "回收站" | "垃圾箱" => "trash",
        "junk" | "spam" | "junk mail" | "junk email" | "bulk mail" | "垃圾邮件" => "junk",
        _ => "custom",
    }
    .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSyncRequest {
    pub target: SyncTarget,
    pub cursor: SyncCursor,
    pub cached_uids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncMailboxFolder {
    pub path: String,
    pub name: String,
    pub delimiter: String,
    pub special_use: Option<String>,
    pub selectable: bool,
    pub subscribed: bool,
    pub total: Option<u32>,
    pub unread: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFetchedSyncMessage {
    pub uid: u32,
    pub source: Vec<u8>,
    pub internal_date: Option<String>,
    pub unread: bool,
    pub flagged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFlagUpdate {
    pub uid: u32,
    pub unread: bool,
    pub flagged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSyncBatch {
    pub mailbox: String,
    pub mailbox_role: String,
    pub uid_validity: Option<String>,
    pub highest_modseq: Option<String>,
    pub uid_validity_changed: bool,
    pub last_seen_uid: i64,
    pub incoming: Vec<RemoteFetchedSyncMessage>,
    pub removed_uids: Vec<u32>,
    pub flag_updates: Vec<RemoteFlagUpdate>,
    pub folders: Vec<SyncMailboxFolder>,
}

pub trait ImapSyncPort {
    fn fetch_incremental(
        &mut self,
        config: &MailConnectionConfig,
        request: &RemoteSyncRequest,
    ) -> Result<RemoteSyncBatch, ProtocolFailure>;
}

impl<T> ImapSyncPort for Box<T>
where
    T: ImapSyncPort + ?Sized,
{
    fn fetch_incremental(
        &mut self,
        config: &MailConnectionConfig,
        request: &RemoteSyncRequest,
    ) -> Result<RemoteSyncBatch, ProtocolFailure> {
        (**self).fetch_incremental(config, request)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxWakeReason {
    Changed,
    Reconcile,
}

pub trait ImapWakePort {
    fn wait_for_inbox_change(
        &mut self,
        config: &MailConnectionConfig,
        maximum_wait: Duration,
    ) -> Result<MailboxWakeReason, ProtocolFailure>;
}

impl<T> ImapWakePort for Box<T>
where
    T: ImapWakePort + ?Sized,
{
    fn wait_for_inbox_change(
        &mut self,
        config: &MailConnectionConfig,
        maximum_wait: Duration,
    ) -> Result<MailboxWakeReason, ProtocolFailure> {
        (**self).wait_for_inbox_change(config, maximum_wait)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxMessageChange {
    pub before: Option<MessageReadModel>,
    pub after: Option<MessageReadModel>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MailboxSyncPlan {
    pub account_id: String,
    pub mailbox: String,
    pub mailbox_role: String,
    pub uid_validity: Option<String>,
    pub highest_modseq: Option<String>,
    pub last_seen_uid: i64,
    pub incoming: Vec<MessageReadModel>,
    pub raw_sources: BTreeMap<String, Vec<u8>>,
    pub removed_uids: Vec<u32>,
    pub uid_validity_changed: bool,
    pub flag_updates: Vec<RemoteFlagUpdate>,
    pub folders: Vec<SyncMailboxFolder>,
    pub synced: i64,
    pub updated: i64,
    pub deleted: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MailboxSyncCommitResult {
    pub created_messages: Vec<MessageReadModel>,
    pub message_changes: Vec<MailboxMessageChange>,
}

#[derive(Debug, Error)]
pub enum MailboxSyncError {
    #[error(transparent)]
    Protocol(#[from] ProtocolFailure),
    #[error("UID 超出本地缓存支持范围")]
    InvalidUid,
    #[error("IMAP 返回了重复 UID")]
    DuplicateUid,
    #[error("IMAP 返回了与目标不一致的邮箱")]
    InvalidTarget,
    #[error(transparent)]
    Parse(#[from] MailParseError),
    #[error("邮件地址或附件无法转换为缓存模型")]
    InvalidMessageModel,
}

pub struct MailboxSyncService<'a, P: ImapSyncPort> {
    port: &'a mut P,
}

impl<'a, P: ImapSyncPort> MailboxSyncService<'a, P> {
    pub fn new(port: &'a mut P) -> Self {
        Self { port }
    }

    pub fn plan(
        &mut self,
        config: &MailConnectionConfig,
        account_id: &str,
        target: SyncTarget,
        cursor: SyncCursor,
        cached: &[MessageReadModel],
        fallback_date: &str,
    ) -> Result<MailboxSyncPlan, MailboxSyncError> {
        let cached_for_target = cached
            .iter()
            .filter(|message| {
                message.account_id == account_id
                    && target
                        .requested_mailbox
                        .as_ref()
                        .map_or(message.mailbox_role == target.mailbox_role, |mailbox| {
                            message.mailbox == *mailbox
                        })
            })
            .collect::<Vec<_>>();
        let cached_uids = cached_for_target
            .iter()
            .map(|message| u32::try_from(message.uid).map_err(|_| MailboxSyncError::InvalidUid))
            .collect::<Result<Vec<_>, _>>()?;
        let request = RemoteSyncRequest {
            target: target.clone(),
            cursor,
            cached_uids,
        };
        let remote = self.port.fetch_incremental(config, &request)?;
        if remote.mailbox_role != target.mailbox_role && target.requested_mailbox.is_none() {
            return Err(MailboxSyncError::InvalidTarget);
        }
        if target
            .requested_mailbox
            .as_ref()
            .is_some_and(|mailbox| mailbox != &remote.mailbox)
        {
            return Err(MailboxSyncError::InvalidTarget);
        }
        validate_remote_batch(&remote)?;

        let flags = remote
            .flag_updates
            .iter()
            .map(|update| (update.uid, update))
            .collect::<BTreeMap<_, _>>();
        let updated = cached_for_target
            .iter()
            .filter(|message| {
                u32::try_from(message.uid)
                    .ok()
                    .and_then(|uid| flags.get(&uid))
                    .is_some_and(|update| {
                        message.unread != update.unread || message.flagged != update.flagged
                    })
            })
            .count() as i64;
        let mut incoming = Vec::with_capacity(remote.incoming.len());
        let mut raw_sources = BTreeMap::new();
        for remote_message in &remote.incoming {
            let message = cache_message(
                account_id,
                &remote.mailbox,
                &remote.mailbox_role,
                remote_message,
                fallback_date,
            )?;
            raw_sources.insert(message.id.clone(), remote_message.source.clone());
            incoming.push(message);
        }
        let deleted = if remote.uid_validity_changed {
            cached_for_target.len() as i64
        } else {
            remote.removed_uids.len() as i64
        };
        Ok(MailboxSyncPlan {
            account_id: account_id.into(),
            mailbox: remote.mailbox,
            mailbox_role: remote.mailbox_role,
            uid_validity: remote.uid_validity,
            highest_modseq: remote.highest_modseq,
            last_seen_uid: remote.last_seen_uid,
            synced: incoming.len() as i64,
            incoming,
            raw_sources,
            removed_uids: remote.removed_uids,
            uid_validity_changed: remote.uid_validity_changed,
            flag_updates: remote.flag_updates,
            folders: remote.folders,
            updated,
            deleted,
        })
    }
}

fn validate_remote_batch(batch: &RemoteSyncBatch) -> Result<(), MailboxSyncError> {
    if batch.mailbox.trim().is_empty()
        || batch.mailbox_role.trim().is_empty()
        || batch.last_seen_uid < 0
    {
        return Err(MailboxSyncError::InvalidTarget);
    }
    let mut seen = BTreeSet::new();
    if batch
        .incoming
        .iter()
        .any(|message| !seen.insert(message.uid))
    {
        return Err(MailboxSyncError::DuplicateUid);
    }
    Ok(())
}

fn cache_message(
    account_id: &str,
    mailbox: &str,
    mailbox_role: &str,
    remote: &RemoteFetchedSyncMessage,
    fallback_date: &str,
) -> Result<MessageReadModel, MailboxSyncError> {
    let parsed = parse_rfc822(&remote.source)?;
    let identity = if mailbox_role == "inbox" {
        format!("{account_id}:{}", remote.uid)
    } else if mailbox_role == "custom" {
        format!("{account_id}:{mailbox}:{}", remote.uid)
    } else {
        format!("{account_id}:{mailbox_role}:{}", remote.uid)
    };
    let id = format!("{:x}", Sha256::digest(identity.as_bytes()))[..24].to_string();
    let from = to_value(parsed.from).map_err(|_| MailboxSyncError::InvalidMessageModel)?;
    let to = to_value(parsed.to).map_err(|_| MailboxSyncError::InvalidMessageModel)?;
    let attachments =
        to_value(parsed.attachments).map_err(|_| MailboxSyncError::InvalidMessageModel)?;
    let has_attachments = attachments
        .as_array()
        .is_some_and(|items| !items.is_empty());
    Ok(MessageReadModel {
        id,
        account_id: account_id.into(),
        mailbox: mailbox.into(),
        mailbox_role: mailbox_role.into(),
        uid: i64::from(remote.uid),
        message_id: parsed.message_id,
        from,
        to,
        subject: parsed.subject,
        preview: parsed.preview,
        text: parsed.text,
        html: parsed.html,
        date: parsed
            .date
            .or_else(|| remote.internal_date.clone())
            .unwrap_or_else(|| fallback_date.into()),
        unread: remote.unread,
        flagged: remote.flagged,
        has_attachments,
        attachments,
        labels: json!([]),
        snoozed_until: None,
    })
}

#[cfg(test)]
mod tests {
    use imail_protocol::MessageReadModel;

    use super::*;
    use crate::MailAuthentication;

    struct FakePort(RemoteSyncBatch);

    impl ImapSyncPort for FakePort {
        fn fetch_incremental(
            &mut self,
            _config: &MailConnectionConfig,
            request: &RemoteSyncRequest,
        ) -> Result<RemoteSyncBatch, ProtocolFailure> {
            assert_eq!(request.cached_uids, [7, 8]);
            Ok(self.0.clone())
        }
    }

    fn config() -> MailConnectionConfig {
        MailConnectionConfig {
            email: "owner@example.com".into(),
            display_name: "Owner".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_secure: true,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            smtp_secure: true,
            authentication: MailAuthentication::Password("secret".into()),
            proxy: None,
        }
    }

    fn cached(uid: i64, unread: bool, flagged: bool) -> MessageReadModel {
        MessageReadModel {
            id: format!("cached-{uid}"),
            account_id: "account-1".into(),
            mailbox: "INBOX".into(),
            mailbox_role: "inbox".into(),
            uid,
            message_id: None,
            from: json!({}),
            to: json!([]),
            subject: "cached".into(),
            preview: "".into(),
            text: "".into(),
            html: None,
            date: "2026-08-10T00:00:00.000Z".into(),
            unread,
            flagged,
            has_attachments: false,
            attachments: json!([]),
            labels: json!(["important"]),
            snoozed_until: None,
        }
    }

    #[test]
    fn builds_a_node_compatible_incremental_plan() {
        let source = b"Message-ID: <new@example.com>\r\nFrom: Sender <sender@example.com>\r\nTo: Owner <owner@example.com>\r\nSubject: New\r\nDate: Sun, 10 Aug 2026 08:00:00 +0800\r\n\r\nBody".to_vec();
        let batch = RemoteSyncBatch {
            mailbox: "INBOX".into(),
            mailbox_role: "inbox".into(),
            uid_validity: Some("44".into()),
            highest_modseq: Some("90".into()),
            uid_validity_changed: false,
            last_seen_uid: 9,
            incoming: vec![RemoteFetchedSyncMessage {
                uid: 9,
                source,
                internal_date: None,
                unread: true,
                flagged: false,
            }],
            removed_uids: vec![7],
            flag_updates: vec![RemoteFlagUpdate {
                uid: 8,
                unread: false,
                flagged: true,
            }],
            folders: vec![],
        };
        let mut port = FakePort(batch);
        let plan = MailboxSyncService::new(&mut port)
            .plan(
                &config(),
                "account-1",
                SyncTarget {
                    mailbox_role: "inbox".into(),
                    requested_mailbox: None,
                },
                SyncCursor {
                    uid_validity: Some("44".into()),
                    last_seen_uid: 8,
                    highest_modseq: Some("80".into()),
                },
                &[cached(7, true, false), cached(8, true, false)],
                "2026-08-10T00:00:00.000Z",
            )
            .unwrap();
        assert_eq!(plan.synced, 1);
        assert_eq!(plan.updated, 1);
        assert_eq!(plan.deleted, 1);
        assert_eq!(
            plan.incoming[0].message_id.as_deref(),
            Some("<new@example.com>")
        );
        assert_eq!(plan.incoming[0].id.len(), 24);
    }

    #[test]
    fn uidvalidity_change_counts_the_rebuilt_cache() {
        let mut batch = RemoteSyncBatch {
            mailbox: "INBOX".into(),
            mailbox_role: "inbox".into(),
            uid_validity: Some("45".into()),
            highest_modseq: None,
            uid_validity_changed: true,
            last_seen_uid: 0,
            incoming: vec![],
            removed_uids: vec![],
            flag_updates: vec![],
            folders: vec![],
        };
        let mut port = FakePort(batch.clone());
        let plan = MailboxSyncService::new(&mut port)
            .plan(
                &config(),
                "account-1",
                SyncTarget {
                    mailbox_role: "inbox".into(),
                    requested_mailbox: None,
                },
                SyncCursor::default(),
                &[cached(7, true, false), cached(8, true, false)],
                "2026-08-10T00:00:00.000Z",
            )
            .unwrap();
        assert_eq!(plan.deleted, 2);
        batch.incoming = vec![];
    }
}
