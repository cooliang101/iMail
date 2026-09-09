//! IMAP incremental reconciliation and mailbox wakeups.
use crate::{
    error::NetworkError,
    imap::{connect_imap, special_use},
    NetworkMailAdapter,
};
use futures_util::TryStreamExt;
use imail_mail::{
    decode_modified_utf7, mailbox_role_for, ImapSyncPort, ImapWakePort, MailConnectionConfig,
    MailboxWakeReason, ProtocolFailure, ProtocolStage, RemoteFetchedSyncMessage, RemoteFlagUpdate,
    RemoteSyncBatch, RemoteSyncRequest, SyncMailboxFolder,
};
use std::time::Duration;
use tokio_rustls::TlsConnector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncrementalCapabilityMode {
    Condstore,
    Basic,
}

fn incremental_capability_mode(qresync: bool, condstore: bool) -> IncrementalCapabilityMode {
    if qresync || condstore {
        IncrementalCapabilityMode::Condstore
    } else {
        IncrementalCapabilityMode::Basic
    }
}

impl ImapSyncPort for NetworkMailAdapter {
    fn fetch_incremental(
        &mut self,
        config: &MailConnectionConfig,
        request: &RemoteSyncRequest,
    ) -> Result<RemoteSyncBatch, ProtocolFailure> {
        self.run(
            ProtocolStage::Imap,
            fetch_incremental(config, request, &self.tls_connector),
        )
    }
}

impl ImapWakePort for NetworkMailAdapter {
    fn wait_for_inbox_change(
        &mut self,
        config: &MailConnectionConfig,
        maximum_wait: Duration,
    ) -> Result<MailboxWakeReason, ProtocolFailure> {
        let maximum_wait = maximum_wait.clamp(Duration::from_secs(15), Duration::from_secs(60));
        self.run_with_timeout(
            ProtocolStage::Imap,
            maximum_wait + Duration::from_secs(30),
            wait_for_inbox_change(config, maximum_wait, &self.tls_connector),
        )
    }
}

async fn wait_for_inbox_change(
    config: &MailConnectionConfig,
    maximum_wait: Duration,
    tls_connector: &TlsConnector,
) -> Result<MailboxWakeReason, NetworkError> {
    let mut session = connect_imap(config, tls_connector).await?;
    let capabilities = session.capabilities().await.map_err(NetworkError::imap)?;
    if capabilities.has_str("IDLE") {
        session.examine("INBOX").await.map_err(NetworkError::imap)?;
        let mut idle = session.idle();
        idle.init().await.map_err(NetworkError::imap)?;
        let (waiting, _interrupt) = idle.wait_with_timeout(maximum_wait);
        let response = waiting.await.map_err(NetworkError::imap)?;
        let mut session = idle.done().await.map_err(NetworkError::imap)?;
        let _ = session.logout().await;
        return Ok(match response {
            async_imap::extensions::idle::IdleResponse::NewData(_) => MailboxWakeReason::Changed,
            async_imap::extensions::idle::IdleResponse::Timeout
            | async_imap::extensions::idle::IdleResponse::ManualInterrupt => {
                MailboxWakeReason::Reconcile
            }
        });
    }

    let before = session
        .status("INBOX", "(MESSAGES UNSEEN UIDNEXT UIDVALIDITY)")
        .await
        .map_err(NetworkError::imap)?;
    tokio::time::sleep(maximum_wait).await;
    let after = session
        .status("INBOX", "(MESSAGES UNSEEN UIDNEXT UIDVALIDITY)")
        .await
        .map_err(NetworkError::imap)?;
    let _ = session.logout().await;
    if before.exists != after.exists
        || before.unseen != after.unseen
        || before.uid_next != after.uid_next
        || before.uid_validity != after.uid_validity
    {
        Ok(MailboxWakeReason::Changed)
    } else {
        Ok(MailboxWakeReason::Reconcile)
    }
}

async fn fetch_incremental(
    config: &MailConnectionConfig,
    request: &RemoteSyncRequest,
    tls_connector: &TlsConnector,
) -> Result<RemoteSyncBatch, NetworkError> {
    let mut session = connect_imap(config, tls_connector).await?;
    let capabilities = session.capabilities().await.map_err(NetworkError::imap)?;
    let incremental_mode = incremental_capability_mode(
        capabilities.has_str("QRESYNC"),
        capabilities.has_str("CONDSTORE"),
    );
    let gmail_labels = capabilities.has_str("X-GM-EXT-1");
    let mut listed = session
        .list(None, Some("*"))
        .await
        .map_err(NetworkError::imap)?;
    let mut folders = Vec::new();
    while let Some(name) = listed.try_next().await.map_err(NetworkError::imap)? {
        let delimiter = name.delimiter().unwrap_or("/").to_string();
        let path = name.name().to_string();
        let raw_name = path.rsplit(&delimiter).next().unwrap_or(path.as_str());
        let display_name = decode_modified_utf7(raw_name).unwrap_or_else(|| raw_name.to_string());
        let selectable = !name
            .attributes()
            .contains(&async_imap::types::NameAttribute::NoSelect);
        folders.push(SyncMailboxFolder {
            name: display_name,
            path,
            delimiter,
            special_use: special_use(name.attributes()),
            selectable,
            subscribed: true,
            total: None,
            unread: None,
        });
    }
    drop(listed);
    for folder in folders.iter_mut().filter(|folder| folder.selectable) {
        let status = session
            .status(&folder.path, "(MESSAGES UNSEEN)")
            .await
            .map_err(NetworkError::imap)?;
        folder.total = Some(status.exists);
        folder.unread = status.unseen;
    }
    let target = resolve_sync_mailbox(&folders, request)?;
    let mailbox_role = mailbox_role_for(&target.path, target.special_use.as_deref());
    let all_mail_archive =
        mailbox_role == "archive" && target.special_use.as_deref() == Some("\\All") && gmail_labels;
    let selected = if incremental_mode == IncrementalCapabilityMode::Condstore {
        match session.select_condstore(&target.path).await {
            Ok(mailbox) => mailbox,
            Err(_) => session
                .examine(&target.path)
                .await
                .map_err(NetworkError::imap)?,
        }
    } else {
        session
            .examine(&target.path)
            .await
            .map_err(NetworkError::imap)?
    };
    let uid_validity = selected.uid_validity.map(|value| value.to_string());
    let highest_modseq = selected.highest_modseq.map(|value| value.to_string());
    let uid_validity_changed = request.cursor.uid_validity.is_some()
        && uid_validity.is_some()
        && request.cursor.uid_validity != uid_validity;
    let max_cached_uid = request.cached_uids.iter().copied().max().unwrap_or(0);
    let cursor_uid = u32::try_from(request.cursor.last_seen_uid.max(0))
        .map_err(|_| NetworkError::Provider("同步游标超出 IMAP UID 范围".into()))?;
    let incremental_uid = if uid_validity_changed {
        0
    } else {
        max_cached_uid.max(cursor_uid)
    };
    let mut incoming = Vec::new();
    let mut last_seen_uid = i64::from(incremental_uid);
    let source_query = if all_mail_archive {
        "(UID FLAGS X-GM-LABELS BODY.PEEK[] INTERNALDATE)"
    } else {
        "(UID FLAGS BODY.PEEK[] INTERNALDATE)"
    };
    if selected.exists > 0 && incremental_uid == 0 {
        let start = selected.exists.saturating_sub(79).max(1);
        let mut fetches = session
            .fetch(format!("{start}:*"), source_query)
            .await
            .map_err(NetworkError::imap)?;
        while let Some(fetch) = fetches.try_next().await.map_err(NetworkError::imap)? {
            collect_incoming(&fetch, all_mail_archive, &mut last_seen_uid, &mut incoming)?;
        }
        drop(fetches);
    } else if incremental_uid > 0
        && selected
            .uid_next
            .is_some_and(|uid_next| uid_next > incremental_uid.saturating_add(1))
    {
        let mut fetches = session
            .uid_fetch(
                format!("{}:*", incremental_uid.saturating_add(1)),
                source_query,
            )
            .await
            .map_err(NetworkError::imap)?;
        while let Some(fetch) = fetches.try_next().await.map_err(NetworkError::imap)? {
            collect_incoming(&fetch, all_mail_archive, &mut last_seen_uid, &mut incoming)?;
        }
        drop(fetches);
    }

    let cached_uids = if uid_validity_changed {
        Vec::new()
    } else {
        request.cached_uids.clone()
    };
    let changed_since = request
        .cursor
        .highest_modseq
        .as_deref()
        .filter(|_| {
            highest_modseq.is_some() && incremental_mode == IncrementalCapabilityMode::Condstore
        })
        .and_then(|value| value.parse::<u64>().ok());
    let mut checked = std::collections::BTreeSet::new();
    let mut flag_updates = Vec::new();
    for batch in cached_uids.chunks(500) {
        let set = batch
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let query = if changed_since.is_some() {
            "UID"
        } else {
            "(UID FLAGS)"
        };
        let mut fetches = session
            .uid_fetch(set, query)
            .await
            .map_err(NetworkError::imap)?;
        while let Some(fetch) = fetches.try_next().await.map_err(NetworkError::imap)? {
            let Some(uid) = fetch.uid else { continue };
            checked.insert(uid);
            if changed_since.is_none() {
                flag_updates.push(flags_from_fetch(uid, &fetch));
            }
        }
        drop(fetches);
    }
    if let (Some(changed_since), Some(minimum_uid)) =
        (changed_since, cached_uids.iter().copied().min())
    {
        let mut fetches = session
            .uid_fetch(
                format!("{minimum_uid}:*"),
                format!("(UID FLAGS) (CHANGEDSINCE {changed_since})"),
            )
            .await
            .map_err(NetworkError::imap)?;
        while let Some(fetch) = fetches.try_next().await.map_err(NetworkError::imap)? {
            if let Some(uid) = fetch.uid {
                flag_updates.push(flags_from_fetch(uid, &fetch));
            }
        }
        drop(fetches);
    }
    let removed_uids = cached_uids
        .into_iter()
        .filter(|uid| !checked.contains(uid))
        .collect();
    let _ = session.logout().await;
    Ok(RemoteSyncBatch {
        mailbox: target.path,
        mailbox_role,
        uid_validity,
        highest_modseq,
        uid_validity_changed,
        last_seen_uid,
        incoming,
        removed_uids,
        flag_updates,
        folders,
    })
}

fn resolve_sync_mailbox(
    folders: &[SyncMailboxFolder],
    request: &RemoteSyncRequest,
) -> Result<SyncMailboxFolder, NetworkError> {
    if let Some(requested) = request.target.requested_mailbox.as_deref() {
        return folders
            .iter()
            .find(|folder| folder.path == requested && folder.selectable)
            .cloned()
            .ok_or_else(|| NetworkError::Provider("邮箱文件夹不存在或不可选择".into()));
    }
    if request.target.mailbox_role == "inbox" {
        return folders
            .iter()
            .find(|folder| folder.path.eq_ignore_ascii_case("INBOX") && folder.selectable)
            .cloned()
            .or_else(|| {
                Some(SyncMailboxFolder {
                    path: "INBOX".into(),
                    name: "INBOX".into(),
                    delimiter: "/".into(),
                    special_use: Some("\\Inbox".into()),
                    selectable: true,
                    subscribed: true,
                    total: None,
                    unread: None,
                })
            })
            .ok_or_else(|| NetworkError::Provider("服务商没有返回收件箱".into()));
    }
    folders
        .iter()
        .find(|folder| {
            folder.selectable
                && mailbox_role_for(&folder.path, folder.special_use.as_deref())
                    == request.target.mailbox_role
        })
        .cloned()
        .ok_or_else(|| {
            NetworkError::Provider(format!(
                "服务商没有返回{}文件夹",
                request.target.mailbox_role
            ))
        })
}

fn collect_incoming(
    fetch: &async_imap::types::Fetch,
    all_mail_archive: bool,
    last_seen_uid: &mut i64,
    incoming: &mut Vec<RemoteFetchedSyncMessage>,
) -> Result<(), NetworkError> {
    let Some(uid) = fetch.uid else { return Ok(()) };
    *last_seen_uid = (*last_seen_uid).max(i64::from(uid));
    if all_mail_archive
        && fetch.gmail_labels().is_some_and(|labels| {
            labels.iter().any(|label| {
                matches!(
                    label.as_ref().to_ascii_lowercase().as_str(),
                    "\\inbox" | "\\sent" | "\\drafts" | "\\trash"
                )
            })
        })
    {
        return Ok(());
    }
    let source = fetch
        .body()
        .ok_or_else(|| NetworkError::Provider("邮件服务器没有返回原始内容".into()))?
        .to_vec();
    let flags = flags_from_fetch(uid, fetch);
    incoming.push(RemoteFetchedSyncMessage {
        uid,
        source,
        internal_date: fetch.internal_date().map(|date| date.to_rfc3339()),
        unread: flags.unread,
        flagged: flags.flagged,
    });
    Ok(())
}

fn flags_from_fetch(uid: u32, fetch: &async_imap::types::Fetch) -> RemoteFlagUpdate {
    let flags = fetch.flags().collect::<Vec<_>>();
    RemoteFlagUpdate {
        uid,
        unread: !flags.contains(&async_imap::types::Flag::Seen),
        flagged: flags.contains(&async_imap::types::Flag::Flagged),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_sync_roles_and_selected_mailboxes_like_the_node_runtime() {
        let folders = vec![
            SyncMailboxFolder {
                path: "INBOX".into(),
                name: "INBOX".into(),
                delimiter: "/".into(),
                special_use: None,
                selectable: true,
                subscribed: true,
                total: None,
                unread: None,
            },
            SyncMailboxFolder {
                path: "[Gmail]/All Mail".into(),
                name: "All Mail".into(),
                delimiter: "/".into(),
                special_use: Some("\\All".into()),
                selectable: true,
                subscribed: true,
                total: None,
                unread: None,
            },
            SyncMailboxFolder {
                path: "Projects".into(),
                name: "Projects".into(),
                delimiter: "/".into(),
                special_use: None,
                selectable: true,
                subscribed: true,
                total: None,
                unread: None,
            },
        ];
        let archive = resolve_sync_mailbox(
            &folders,
            &RemoteSyncRequest {
                target: imail_mail::SyncTarget {
                    mailbox_role: "archive".into(),
                    requested_mailbox: None,
                },
                cursor: Default::default(),
                cached_uids: vec![],
            },
        )
        .unwrap();
        assert_eq!(archive.path, "[Gmail]/All Mail");
        assert_eq!(
            mailbox_role_for(&archive.path, archive.special_use.as_deref()),
            "archive"
        );
        let custom = resolve_sync_mailbox(
            &folders,
            &RemoteSyncRequest {
                target: imail_mail::SyncTarget {
                    mailbox_role: "custom".into(),
                    requested_mailbox: Some("Projects".into()),
                },
                cursor: Default::default(),
                cached_uids: vec![],
            },
        )
        .unwrap();
        assert_eq!(custom.path, "Projects");
        assert_eq!(mailbox_role_for("已发送邮件", None), "sent");
    }

    #[test]
    fn chooses_safe_incremental_mode_for_qresync_and_condstore_servers() {
        assert_eq!(
            incremental_capability_mode(true, false),
            IncrementalCapabilityMode::Condstore
        );
        assert_eq!(
            incremental_capability_mode(false, true),
            IncrementalCapabilityMode::Condstore
        );
        assert_eq!(
            incremental_capability_mode(false, false),
            IncrementalCapabilityMode::Basic
        );
    }

    #[test]
    fn explicit_sync_target_never_falls_back_to_another_mailbox() {
        let folder = SyncMailboxFolder {
            path: "Projects".into(),
            name: "Projects".into(),
            delimiter: "/".into(),
            special_use: None,
            selectable: false,
            subscribed: true,
            total: None,
            unread: None,
        };
        for requested in ["Projects", "Missing"] {
            let request = RemoteSyncRequest {
                target: imail_mail::SyncTarget {
                    mailbox_role: "inbox".into(),
                    requested_mailbox: Some(requested.into()),
                },
                cursor: Default::default(),
                cached_uids: vec![],
            };
            assert!(resolve_sync_mailbox(std::slice::from_ref(&folder), &request).is_err());
        }
        let request = |role: &str| RemoteSyncRequest {
            target: imail_mail::SyncTarget {
                mailbox_role: role.into(),
                requested_mailbox: None,
            },
            cursor: Default::default(),
            cached_uids: vec![],
        };
        assert!(resolve_sync_mailbox(&[], &request("sent")).is_err());
        assert_eq!(
            resolve_sync_mailbox(&[], &request("inbox")).unwrap().path,
            "INBOX"
        );
    }
}
