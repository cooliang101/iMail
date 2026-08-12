use std::{
    fmt,
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use async_imap::{Authenticator, Client, Session};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::TryStreamExt;
use imail_mail::{
    mailbox_role_for, ImapPort, ImapSyncPort, ImapWakePort, MailAuthentication,
    MailConnectionConfig, MailProxy, MailboxWakeReason, OutgoingMessage, ProtocolFailure,
    ProtocolStage, RemoteFetchedSyncMessage, RemoteFlagUpdate, RemoteMailbox, RemoteMessageLocator,
    RemoteMoveConfirmation, RemoteSyncBatch, RemoteSyncRequest, SmtpPort, SyncMailboxFolder,
};
use imail_protocol::{RemoteMessageFlagPatch, SendMessageResult};
use mail_builder::MessageBuilder;
use mail_parser::MessageParser;
use mail_send::{smtp::AssertReply, Credentials, SmtpClient};
use rustls_pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::TcpStream,
    runtime::{Builder as RuntimeBuilder, Runtime},
    time::timeout,
};
use tokio_rustls::{client::TlsStream, TlsConnector};
use tokio_socks::tcp::Socks5Stream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PROXY_RESPONSE_BYTES: usize = 16 * 1024;
const MESSAGE_ID_SCAN_UID_BATCH: usize = 500;

pub trait NetworkCancellation: Send + Sync + 'static {
    fn is_cancelled(&self) -> bool;
}

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

type SecureStream = TlsStream<TunnelStream>;
type ImapSession = Session<SecureStream>;
type SmtpSession = SmtpClient<SecureStream>;

pub struct NetworkMailAdapter {
    runtime: Runtime,
    cancellation: Option<Arc<dyn NetworkCancellation>>,
    tls_connector: TlsConnector,
}

impl NetworkMailAdapter {
    pub fn new() -> io::Result<Self> {
        Self::with_tls_connector(tls_connector())
    }

    pub fn with_tls_connector(tls_connector: TlsConnector) -> io::Result<Self> {
        Ok(Self {
            runtime: RuntimeBuilder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
            cancellation: None,
            tls_connector,
        })
    }

    pub fn with_cancellation(cancellation: Arc<dyn NetworkCancellation>) -> io::Result<Self> {
        let mut adapter = Self::new()?;
        adapter.cancellation = Some(cancellation);
        Ok(adapter)
    }

    pub fn with_cancellation_and_tls_connector(
        cancellation: Arc<dyn NetworkCancellation>,
        tls_connector: TlsConnector,
    ) -> io::Result<Self> {
        let mut adapter = Self::with_tls_connector(tls_connector)?;
        adapter.cancellation = Some(cancellation);
        Ok(adapter)
    }

    fn run<T>(
        &self,
        stage: ProtocolStage,
        operation: impl Future<Output = Result<T, NetworkError>>,
    ) -> Result<T, ProtocolFailure> {
        self.run_with_timeout(stage, OPERATION_TIMEOUT, operation)
    }

    fn run_with_timeout<T>(
        &self,
        stage: ProtocolStage,
        maximum_wait: Duration,
        operation: impl Future<Output = Result<T, NetworkError>>,
    ) -> Result<T, ProtocolFailure> {
        let cancellation = self.cancellation.clone();
        let has_cancellation = cancellation.is_some();
        self.runtime.block_on(async move {
            tokio::select! {
                result = timeout(maximum_wait, operation) => result
                    .map_err(|_| ProtocolFailure::from_provider(stage, Some("TIMEOUT"), "网络操作超时"))?
                    .map_err(|error| error.into_protocol_failure(stage)),
                () = wait_for_cancellation(cancellation), if has_cancellation => {
                    Err(ProtocolFailure::from_provider(stage, Some("CANCELLED"), "网络操作已取消"))
                }
            }
        })
    }
}

async fn fetch_source_by_uid(
    session: &mut ImapSession,
    uid: u32,
) -> Result<Option<Vec<u8>>, NetworkError> {
    for query in [
        "(UID BODY.PEEK[])",
        "(UID RFC822)",
        "(UID BODY[])",
        "BODY.PEEK[]",
        "RFC822",
    ] {
        let mut fetches = session
            .uid_fetch(uid.to_string(), query)
            .await
            .map_err(NetworkError::imap)?;
        let mut source = None;
        while let Some(fetch) = fetches.try_next().await.map_err(NetworkError::imap)? {
            if let Some(body) = fetch.body().filter(|body| !body.is_empty()) {
                source = Some(body.to_vec());
                break;
            }
        }
        drop(fetches);
        if source.is_some() {
            return Ok(source);
        }
    }
    Ok(None)
}

fn imap_search_message_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 998 || value.contains(['\r', '\n', '\0']) {
        return None;
    }
    Some(value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn normalized_message_id(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(value)
        .trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn header_message_id(header: &[u8]) -> Option<String> {
    MessageParser::default()
        .parse_headers(header)
        .and_then(|message| message.message_id().and_then(normalized_message_id))
}

fn source_matches_message_id(source: &[u8], expected: Option<&str>) -> bool {
    let Some(expected) = expected.and_then(normalized_message_id) else {
        return true;
    };
    header_message_id(source).as_deref() == Some(expected.as_str())
}

async fn find_uid_by_message_id_headers(
    session: &mut ImapSession,
    expected: &str,
) -> Result<Option<u32>, NetworkError> {
    let Some(expected) = normalized_message_id(expected) else {
        return Ok(None);
    };
    let mut uids = session
        .uid_search("ALL")
        .await
        .map_err(NetworkError::imap)?
        .into_iter()
        .collect::<Vec<_>>();
    uids.sort_unstable_by(|left, right| right.cmp(left));
    for batch in uids.chunks(MESSAGE_ID_SCAN_UID_BATCH) {
        let set = batch
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut fetches = session
            .uid_fetch(set, "(UID BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])")
            .await
            .map_err(NetworkError::imap)?;
        let mut found = None;
        while let Some(fetch) = fetches.try_next().await.map_err(NetworkError::imap)? {
            if fetch.header().and_then(header_message_id).as_deref() == Some(expected.as_str()) {
                found = fetch.uid;
                break;
            }
        }
        drop(fetches);
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

async fn fetch_source_by_message_id(
    session: &mut ImapSession,
    expected: &str,
) -> Result<Option<Vec<u8>>, NetworkError> {
    let Some(search_value) = imap_search_message_id(expected) else {
        return Ok(None);
    };
    let query = format!("HEADER Message-ID \"{search_value}\"");
    let mut matches = session
        .uid_search(query)
        .await
        .map_err(NetworkError::imap)?
        .into_iter()
        .collect::<Vec<_>>();
    matches.sort_unstable_by(|left, right| right.cmp(left));
    for uid in matches {
        if let Some(found) = fetch_source_by_uid(session, uid).await? {
            if source_matches_message_id(&found, Some(expected)) {
                return Ok(Some(found));
            }
        }
    }
    if let Some(uid) = find_uid_by_message_id_headers(session, expected).await? {
        return Ok(fetch_source_by_uid(session, uid)
            .await?
            .filter(|found| source_matches_message_id(found, Some(expected))));
    }
    Ok(None)
}

async fn selectable_mailboxes(
    session: &mut ImapSession,
    excluded: &str,
) -> Result<Vec<(String, Option<String>)>, NetworkError> {
    let mut names = session
        .list(None, Some("*"))
        .await
        .map_err(NetworkError::imap)?;
    let mut result = Vec::new();
    while let Some(name) = names.try_next().await.map_err(NetworkError::imap)? {
        if name.name() == excluded
            || name
                .attributes()
                .iter()
                .any(|attribute| matches!(attribute, async_imap::types::NameAttribute::NoSelect))
        {
            continue;
        }
        result.push((name.name().to_string(), special_use(name.attributes())));
    }
    drop(names);
    result.sort_by_key(|(path, special_use)| {
        let priority = match special_use.as_deref() {
            Some("\\Archive") | Some("\\All") => 0,
            Some("\\Sent") => 1,
            Some("\\Trash") | Some("\\Junk") | Some("\\Drafts") => 3,
            _ => 2,
        };
        (priority, path.to_ascii_lowercase())
    });
    Ok(result)
}

async fn wait_for_cancellation(cancellation: Option<Arc<dyn NetworkCancellation>>) {
    let Some(cancellation) = cancellation else {
        std::future::pending::<()>().await;
        return;
    };
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

impl ImapPort for NetworkMailAdapter {
    fn verify(&mut self, config: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
        self.run(ProtocolStage::Imap, async {
            let mut session = connect_imap(config, &self.tls_connector).await?;
            session.examine("INBOX").await.map_err(NetworkError::imap)?;
            let _ = session.logout().await;
            Ok(())
        })
    }

    fn fetch_source(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
    ) -> Result<Vec<u8>, ProtocolFailure> {
        self.run(ProtocolStage::Imap, async {
            let mut session = connect_imap(config, &self.tls_connector).await?;
            session
                .examine(&locator.mailbox)
                .await
                .map_err(NetworkError::imap)?;
            let mut source = fetch_source_by_uid(&mut session, locator.uid).await?;
            if source.as_deref().is_some_and(|source| {
                !source_matches_message_id(source, locator.message_id.as_deref())
            }) {
                source = None;
            }
            if source.is_none() {
                if let Some(message_id) = locator.message_id.as_deref() {
                    source = fetch_source_by_message_id(&mut session, message_id).await?;
                    if source.is_none() {
                        let mailboxes =
                            selectable_mailboxes(&mut session, &locator.mailbox).await?;
                        for (mailbox, _) in mailboxes {
                            if session.examine(&mailbox).await.is_err() {
                                continue;
                            }
                            source = fetch_source_by_message_id(&mut session, message_id).await?;
                            if source.is_some() {
                                break;
                            }
                        }
                    }
                }
            }
            let _ = session.logout().await;
            source
                .ok_or_else(|| NetworkError::Provider("邮件服务器没有返回可读取的原始内容".into()))
        })
    }

    fn update_flags(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
        patch: &RemoteMessageFlagPatch,
    ) -> Result<(), ProtocolFailure> {
        self.run(ProtocolStage::Imap, async {
            let mut session = connect_imap(config, &self.tls_connector).await?;
            session
                .select(&locator.mailbox)
                .await
                .map_err(NetworkError::imap)?;
            let uid = locator.uid.to_string();
            if let Some(unread) = patch.unread {
                let operation = if unread {
                    "-FLAGS.SILENT (\\Seen)"
                } else {
                    "+FLAGS.SILENT (\\Seen)"
                };
                consume_store(&mut session, &uid, operation).await?;
            }
            if let Some(flagged) = patch.flagged {
                let operation = if flagged {
                    "+FLAGS.SILENT (\\Flagged)"
                } else {
                    "-FLAGS.SILENT (\\Flagged)"
                };
                consume_store(&mut session, &uid, operation).await?;
            }
            let _ = session.logout().await;
            Ok(())
        })
    }

    fn list_mailboxes(
        &mut self,
        config: &MailConnectionConfig,
    ) -> Result<Vec<RemoteMailbox>, ProtocolFailure> {
        self.run(ProtocolStage::Imap, async {
            let mut session = connect_imap(config, &self.tls_connector).await?;
            let mut names = session
                .list(None, Some("*"))
                .await
                .map_err(NetworkError::imap)?;
            let mut result = Vec::new();
            while let Some(name) = names.try_next().await.map_err(NetworkError::imap)? {
                result.push(RemoteMailbox {
                    path: name.name().to_string(),
                    special_use: special_use(name.attributes()),
                });
            }
            drop(names);
            let _ = session.logout().await;
            Ok(result)
        })
    }

    fn move_message(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
        target_mailbox: &str,
    ) -> Result<RemoteMoveConfirmation, ProtocolFailure> {
        self.run(ProtocolStage::Imap, async {
            let mut session = connect_imap(config, &self.tls_connector).await?;
            session
                .select(&locator.mailbox)
                .await
                .map_err(NetworkError::imap)?;
            session
                .uid_mv(locator.uid.to_string(), target_mailbox)
                .await
                .map_err(NetworkError::imap)?;
            let _ = session.logout().await;
            Ok(RemoteMoveConfirmation {
                confirmed: true,
                uid: None,
            })
        })
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
        let selectable = !name
            .attributes()
            .contains(&async_imap::types::NameAttribute::NoSelect);
        folders.push(SyncMailboxFolder {
            name: path
                .rsplit(&delimiter)
                .next()
                .unwrap_or(path.as_str())
                .to_string(),
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

impl SmtpPort for NetworkMailAdapter {
    fn verify(&mut self, config: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
        self.run(ProtocolStage::Smtp, async {
            let session = connect_smtp(config, &self.tls_connector).await?;
            let _ = session.quit().await;
            Ok(())
        })
    }

    fn send(
        &mut self,
        config: &MailConnectionConfig,
        message: &OutgoingMessage,
    ) -> Result<SendMessageResult, ProtocolFailure> {
        self.run(ProtocolStage::Smtp, async {
            let (source, message_id) = build_message_source(message)?;
            let recipients = message
                .to
                .iter()
                .chain(message.cc.as_deref().unwrap_or_default())
                .cloned()
                .collect::<Vec<_>>();
            let envelope = mail_send::smtp::message::Message::new(
                config.email.clone(),
                recipients.clone(),
                source,
            );
            let mut session = connect_smtp(config, &self.tls_connector).await?;
            session.send(envelope).await.map_err(NetworkError::smtp)?;
            let _ = session.quit().await;
            Ok(SendMessageResult {
                message_id,
                accepted: recipients,
            })
        })
    }
}

async fn connect_imap(
    config: &MailConnectionConfig,
    tls_connector: &TlsConnector,
) -> Result<ImapSession, NetworkError> {
    let tunnel = connect_tunnel(
        config.proxy.as_ref(),
        &config.imap_host,
        config.imap_port,
        tls_connector,
    )
    .await?;
    let mut client = if config.imap_secure {
        Client::new(connect_tls(tunnel, &config.imap_host, tls_connector).await?)
    } else {
        let mut plain = Client::new(tunnel);
        read_imap_greeting(&mut plain).await?;
        plain
            .run_command_and_check_ok("STARTTLS", None)
            .await
            .map_err(NetworkError::imap)?;
        Client::new(connect_tls(plain.into_inner(), &config.imap_host, tls_connector).await?)
    };
    if config.imap_secure {
        read_imap_greeting(&mut client).await?;
    }
    match &config.authentication {
        MailAuthentication::Password(password) => client
            .login(&config.email, password)
            .await
            .map_err(|(error, _)| NetworkError::imap(error)),
        MailAuthentication::OAuth { access_token, .. } => client
            .authenticate(
                "XOAUTH2",
                XOAuth2 {
                    username: &config.email,
                    access_token,
                },
            )
            .await
            .map_err(|(error, _)| NetworkError::imap(error)),
    }
}

async fn read_imap_greeting<T>(client: &mut Client<T>) -> Result<(), NetworkError>
where
    T: AsyncRead + AsyncWrite + Unpin + fmt::Debug + Send,
{
    client
        .read_response()
        .await
        .map_err(NetworkError::io)?
        .ok_or_else(|| NetworkError::Provider("IMAP 服务商未返回欢迎消息".into()))?;
    Ok(())
}

async fn consume_store(
    session: &mut ImapSession,
    uid: &str,
    operation: &str,
) -> Result<(), NetworkError> {
    let mut updates = session
        .uid_store(uid, operation)
        .await
        .map_err(NetworkError::imap)?;
    while updates
        .try_next()
        .await
        .map_err(NetworkError::imap)?
        .is_some()
    {}
    Ok(())
}

async fn connect_smtp(
    config: &MailConnectionConfig,
    tls_connector: &TlsConnector,
) -> Result<SmtpSession, NetworkError> {
    let tunnel = connect_tunnel(
        config.proxy.as_ref(),
        &config.smtp_host,
        config.smtp_port,
        tls_connector,
    )
    .await?;
    let mut client = if config.smtp_secure {
        let stream = connect_tls(tunnel, &config.smtp_host, tls_connector).await?;
        let mut client = SmtpClient {
            stream,
            timeout: OPERATION_TIMEOUT,
        };
        client
            .read()
            .await
            .map_err(NetworkError::smtp)?
            .assert_positive_completion()
            .map_err(NetworkError::smtp)?;
        client
    } else {
        let mut plain = SmtpClient {
            stream: tunnel,
            timeout: OPERATION_TIMEOUT,
        };
        plain
            .read()
            .await
            .map_err(NetworkError::smtp)?
            .assert_positive_completion()
            .map_err(NetworkError::smtp)?;
        plain.ehlo("localhost").await.map_err(NetworkError::smtp)?;
        plain
            .cmd(b"STARTTLS\r\n")
            .await
            .map_err(NetworkError::smtp)?
            .assert_positive_completion()
            .map_err(NetworkError::smtp)?;
        SmtpClient {
            stream: connect_tls(plain.stream, &config.smtp_host, tls_connector).await?,
            timeout: OPERATION_TIMEOUT,
        }
    };
    let capabilities = client.ehlo("localhost").await.map_err(NetworkError::smtp)?;
    let credentials = smtp_credentials(config);
    client
        .authenticate(&credentials, &capabilities)
        .await
        .map_err(NetworkError::smtp)?;
    Ok(client)
}

fn smtp_credentials(config: &MailConnectionConfig) -> Credentials<String> {
    match &config.authentication {
        MailAuthentication::Password(password) => {
            Credentials::new(config.email.clone(), password.clone())
        }
        MailAuthentication::OAuth {
            provider,
            access_token,
        } if provider == "yahoo" => Credentials::new_oauth(format!(
            "n,a={},\x01host={}\x01port={}\x01auth=Bearer {}\x01\x01",
            config.email, config.smtp_host, config.smtp_port, access_token
        )),
        MailAuthentication::OAuth { access_token, .. } => {
            Credentials::new_xoauth2(config.email.clone(), access_token.clone())
        }
    }
}

fn build_message_source(message: &OutgoingMessage) -> Result<(Vec<u8>, String), NetworkError> {
    let mut builder = MessageBuilder::new()
        .from((message.from.name.clone(), message.from.address.clone()))
        .to(message.to.clone())
        .subject(message.subject.clone())
        .text_body(message.text.clone());
    if let Some(cc) = &message.cc {
        builder = builder.cc(cc.clone());
    }
    if let Some(html) = &message.html {
        builder = builder.html_body(html.clone());
    }
    for attachment in &message.attachments {
        builder = builder.attachment(
            attachment.content_type.clone(),
            attachment.filename.clone(),
            attachment.content.clone(),
        );
    }
    let source = builder.write_to_vec().map_err(NetworkError::io)?;
    let message_id = imail_mail::parse_rfc822(&source)
        .map_err(|error| NetworkError::Provider(error.to_string()))?
        .message_id
        .ok_or_else(|| NetworkError::Provider("发件内容缺少 Message-ID".into()))?;
    Ok((source, message_id))
}

async fn connect_tunnel(
    proxy: Option<&MailProxy>,
    target_host: &str,
    target_port: u16,
    tls_connector: &TlsConnector,
) -> Result<TunnelStream, NetworkError> {
    timeout(CONNECT_TIMEOUT, async {
        match proxy {
            None => TcpStream::connect((target_host, target_port))
                .await
                .map(TunnelStream::Direct)
                .map_err(NetworkError::io),
            Some(proxy) if proxy.protocol == "socks5" => {
                let proxy_address = (proxy.host.as_str(), proxy.port);
                let target = (target_host, target_port);
                let stream = if let Some(username) = proxy.username.as_deref() {
                    Socks5Stream::connect_with_password(
                        proxy_address,
                        target,
                        username,
                        proxy.password.as_deref().unwrap_or_default(),
                    )
                    .await
                } else {
                    Socks5Stream::connect(proxy_address, target).await
                }
                .map_err(|error| NetworkError::Provider(format!("SOCKS5 代理连接失败：{error}")))?;
                Ok(TunnelStream::Socks(stream))
            }
            Some(proxy) if proxy.protocol == "http" || proxy.protocol == "https" => {
                let stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
                    .await
                    .map_err(NetworkError::io)?;
                let mut stream = if proxy.protocol == "https" {
                    ProxyConnection::Tls(Box::new(
                        connect_tls_tcp(stream, &proxy.host, tls_connector).await?,
                    ))
                } else {
                    ProxyConnection::Plain(stream)
                };
                establish_http_connect(&mut stream, proxy, target_host, target_port).await?;
                Ok(match stream {
                    ProxyConnection::Plain(stream) => TunnelStream::Http(stream),
                    ProxyConnection::Tls(stream) => TunnelStream::Https(stream),
                })
            }
            Some(_) => Err(NetworkError::Provider("不支持的代理协议".into())),
        }
    })
    .await
    .map_err(|_| NetworkError::Provider("代理或服务器连接超时".into()))?
}

async fn establish_http_connect<T: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut T,
    proxy: &MailProxy,
    target_host: &str,
    target_port: u16,
) -> Result<(), NetworkError> {
    let authority = if target_host.contains(':') {
        format!("[{target_host}]:{target_port}")
    } else {
        format!("{target_host}:{target_port}")
    };
    let authorization = proxy.username.as_deref().map(|username| {
        let credentials = format!(
            "{}:{}",
            username,
            proxy.password.as_deref().unwrap_or_default()
        );
        format!(
            "Proxy-Authorization: Basic {}\r\n",
            BASE64.encode(credentials)
        )
    });
    let request = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: Keep-Alive\r\n{}\r\n",
        authorization.as_deref().unwrap_or_default()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(NetworkError::io)?;
    stream.flush().await.map_err(NetworkError::io)?;
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while response.len() < MAX_PROXY_RESPONSE_BYTES {
        stream
            .read_exact(&mut byte)
            .await
            .map_err(NetworkError::io)?;
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    if !response.ends_with(b"\r\n\r\n") {
        return Err(NetworkError::Provider("HTTP 代理响应过大或不完整".into()));
    }
    let status_line = String::from_utf8_lossy(&response)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let accepted = status_line
        .split_whitespace()
        .nth(1)
        .is_some_and(|status| status == "200");
    if !accepted {
        return Err(NetworkError::Provider(format!(
            "HTTP 代理拒绝 CONNECT：{status_line}"
        )));
    }
    Ok(())
}

async fn connect_tls(
    stream: TunnelStream,
    host: &str,
    tls_connector: &TlsConnector,
) -> Result<SecureStream, NetworkError> {
    tls_connector
        .connect(server_name(host)?, stream)
        .await
        .map_err(|error| NetworkError::Provider(format!("TLS 握手失败：{error}")))
}

async fn connect_tls_tcp(
    stream: TcpStream,
    host: &str,
    tls_connector: &TlsConnector,
) -> Result<TlsStream<TcpStream>, NetworkError> {
    tls_connector
        .connect(server_name(host)?, stream)
        .await
        .map_err(|error| NetworkError::Provider(format!("代理 TLS 握手失败：{error}")))
}

fn tls_connector() -> TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Ok(native) = rustls_native_certs::load_native_certs() {
        for certificate in native {
            let _ = roots.add(certificate);
        }
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

fn server_name(host: &str) -> Result<ServerName<'static>, NetworkError> {
    ServerName::try_from(host.to_string())
        .map_err(|_| NetworkError::Provider("TLS 服务器名称无效".into()))
}

fn special_use(attributes: &[async_imap::types::NameAttribute<'_>]) -> Option<String> {
    use async_imap::types::NameAttribute;
    attributes.iter().find_map(|attribute| match attribute {
        NameAttribute::Archive => Some("\\Archive".into()),
        NameAttribute::All => Some("\\All".into()),
        NameAttribute::Trash => Some("\\Trash".into()),
        NameAttribute::Sent => Some("\\Sent".into()),
        NameAttribute::Drafts => Some("\\Drafts".into()),
        NameAttribute::Junk => Some("\\Junk".into()),
        _ => None,
    })
}

struct XOAuth2<'a> {
    username: &'a str,
    access_token: &'a str,
}

impl Authenticator for XOAuth2<'_> {
    type Response = Vec<u8>;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.username, self.access_token
        )
        .into_bytes()
    }
}

enum ProxyConnection {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for ProxyConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_read(context, buffer),
            Self::Tls(stream) => Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for ProxyConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_write(context, buffer),
            Self::Tls(stream) => Pin::new(stream).poll_write(context, buffer),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_flush(context),
            Self::Tls(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Tls(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

pub enum TunnelStream {
    Direct(TcpStream),
    Socks(Socks5Stream<TcpStream>),
    Http(TcpStream),
    Https(Box<TlsStream<TcpStream>>),
}

impl fmt::Debug for TunnelStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Direct(_) => "TunnelStream::Direct",
            Self::Socks(_) => "TunnelStream::Socks",
            Self::Http(_) => "TunnelStream::Http",
            Self::Https(_) => "TunnelStream::Https",
        })
    }
}

impl AsyncRead for TunnelStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => {
                Pin::new(stream).poll_read(context, buffer)
            }
            Self::Socks(stream) => Pin::new(stream).poll_read(context, buffer),
            Self::Https(stream) => Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for TunnelStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => {
                Pin::new(stream).poll_write(context, buffer)
            }
            Self::Socks(stream) => Pin::new(stream).poll_write(context, buffer),
            Self::Https(stream) => Pin::new(stream).poll_write(context, buffer),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => Pin::new(stream).poll_flush(context),
            Self::Socks(stream) => Pin::new(stream).poll_flush(context),
            Self::Https(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Self::Direct(stream) | Self::Http(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Socks(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Https(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

#[derive(Debug)]
enum NetworkError {
    Provider(String),
}

impl NetworkError {
    fn io(error: io::Error) -> Self {
        Self::Provider(error.to_string())
    }

    fn imap(error: async_imap::error::Error) -> Self {
        Self::Provider(error.to_string())
    }

    fn smtp(error: mail_send::Error) -> Self {
        Self::Provider(error.to_string())
    }

    fn into_protocol_failure(self, stage: ProtocolStage) -> ProtocolFailure {
        let Self::Provider(detail) = self;
        ProtocolFailure::from_provider(stage, None, &detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imail_mail::{OutgoingAttachment, OutgoingMessage};
    use imail_protocol::MailAddressView;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::{
        net::TcpListener as StdTcpListener,
        thread::{self, JoinHandle},
    };
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::{TcpListener, TcpStream},
    };
    use tokio_rustls::{server::TlsStream as ServerTlsStream, TlsAcceptor};

    fn fixture_tls() -> (TlsConnector, Arc<rustls::ServerConfig>) {
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let certificate_der = CertificateDer::from(certificate.serialize_der().unwrap());
        let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certificate.serialize_private_key_der(),
        ));
        let server = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate_der.clone()], private_key)
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate_der).unwrap();
        let client = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        (TlsConnector::from(Arc::new(client)), Arc::new(server))
    }

    fn fixture_listener() -> (StdTcpListener, u16) {
        let listener = StdTcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        (listener, port)
    }

    fn spawn_imap_fixture(
        listener: StdTcpListener,
        tls: Arc<rustls::ServerConfig>,
        connections: usize,
        source: Vec<u8>,
    ) -> JoinHandle<Vec<String>> {
        thread::spawn(move || {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let listener = TcpListener::from_std(listener).unwrap();
                    let acceptor = TlsAcceptor::from(tls);
                    let mut transcript = Vec::new();
                    for _ in 0..connections {
                        let (stream, _) = listener.accept().await.unwrap();
                        let stream = acceptor.accept(stream).await.unwrap();
                        transcript.extend(serve_imap_connection(stream, &source).await);
                    }
                    transcript
                })
        })
    }

    async fn serve_imap_connection(
        mut stream: ServerTlsStream<TcpStream>,
        source: &[u8],
    ) -> Vec<String> {
        stream
            .write_all(b"* OK iMail fixture ready\r\n")
            .await
            .unwrap();
        let (read, mut write) = tokio::io::split(stream);
        let mut lines = BufReader::new(read).lines();
        let mut transcript = Vec::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            transcript.push(line.clone());
            let tag = line.split_whitespace().next().unwrap_or("A0");
            let command = line.to_ascii_uppercase();
            let response = if command.contains(" LOGIN ") {
                format!("{tag} OK LOGIN completed\r\n").into_bytes()
            } else if command.contains(" CAPABILITY") {
                format!("* CAPABILITY IMAP4rev1 SPECIAL-USE\r\n{tag} OK CAPABILITY completed\r\n")
                    .into_bytes()
            } else if command.contains(" LIST ") {
                format!("* LIST (\\Inbox) \"/\" \"INBOX\"\r\n{tag} OK LIST completed\r\n")
                    .into_bytes()
            } else if command.contains(" STATUS ") {
                format!(
                    "* STATUS \"INBOX\" (MESSAGES 1 UNSEEN 1 UIDNEXT 2 UIDVALIDITY 7)\r\n{tag} OK STATUS completed\r\n"
                )
                .into_bytes()
            } else if command.contains(" EXAMINE ") || command.contains(" SELECT ") {
                format!(
                    "* FLAGS (\\Seen \\Flagged)\r\n* 1 EXISTS\r\n* OK [UIDVALIDITY 7] UIDs valid\r\n* OK [UIDNEXT 2] next UID\r\n{tag} OK [READ-ONLY] mailbox selected\r\n"
                )
                .into_bytes()
            } else if command.contains(" FETCH ") {
                let mut response = format!(
                    "* 1 FETCH (UID 1 FLAGS () INTERNALDATE \"10-Aug-2026 12:00:00 +0000\" BODY[] {{{}}}\r\n",
                    source.len()
                )
                .into_bytes();
                response.extend_from_slice(source);
                response.extend_from_slice(format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes());
                response
            } else if command.contains(" LOGOUT") {
                let response =
                    format!("* BYE fixture closing\r\n{tag} OK LOGOUT completed\r\n").into_bytes();
                write.write_all(&response).await.unwrap();
                break;
            } else {
                format!("{tag} BAD unsupported fixture command\r\n").into_bytes()
            };
            write.write_all(&response).await.unwrap();
        }
        transcript
    }

    fn spawn_icloud_fallback_fixture(
        listener: StdTcpListener,
        tls: Arc<rustls::ServerConfig>,
        source: Vec<u8>,
    ) -> JoinHandle<Vec<String>> {
        thread::spawn(move || {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let listener = TcpListener::from_std(listener).unwrap();
                    let acceptor = TlsAcceptor::from(tls);
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut stream = acceptor.accept(stream).await.unwrap();
                    stream.write_all(b"* OK iCloud fixture ready\r\n").await.unwrap();
                    let (read, mut write) = tokio::io::split(stream);
                    let mut lines = BufReader::new(read).lines();
                    let mut transcript = Vec::new();
                    while let Some(line) = lines.next_line().await.unwrap() {
                        transcript.push(line.clone());
                        let tag = line.split_whitespace().next().unwrap_or("A0");
                        let command = line.to_ascii_uppercase();
                        let response = if command.contains(" LOGIN ") {
                            format!("{tag} OK LOGIN completed\r\n").into_bytes()
                        } else if command.contains(" CAPABILITY") {
                            format!("* CAPABILITY IMAP4rev1\r\n{tag} OK CAPABILITY completed\r\n")
                                .into_bytes()
                        } else if command.contains(" EXAMINE ") {
                            format!("* 1 EXISTS\r\n* OK [UIDVALIDITY 8] UIDs valid\r\n* OK [UIDNEXT 2] next UID\r\n{tag} OK [READ-ONLY] selected\r\n").into_bytes()
                        } else if command.contains("UID SEARCH HEADER MESSAGE-ID") {
                            format!("* SEARCH\r\n{tag} OK SEARCH completed\r\n").into_bytes()
                        } else if command.contains("UID SEARCH ALL") {
                            format!("* SEARCH 1\r\n{tag} OK SEARCH completed\r\n").into_bytes()
                        } else if command.contains("UID FETCH 1 ")
                            && command.contains("HEADER.FIELDS")
                        {
                            let header = b"Message-ID: <icloud-fallback@example.test>\r\n\r\n";
                            let mut response = format!(
                                "* 1 FETCH (UID 1 BODY[HEADER.FIELDS (MESSAGE-ID)] {{{}}}\r\n",
                                header.len()
                            )
                            .into_bytes();
                            response.extend_from_slice(header);
                            response.extend_from_slice(
                                format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes(),
                            );
                            response
                        } else if command.contains("UID FETCH 1 ")
                            && command.contains("(UID BODY.PEEK[])")
                        {
                            let mut response = format!(
                                "* 1 FETCH (UID 1 BODY[] {{{}}}\r\n",
                                source.len()
                            )
                            .into_bytes();
                            response.extend_from_slice(&source);
                            response.extend_from_slice(
                                format!(")\r\n{tag} OK FETCH completed\r\n").as_bytes(),
                            );
                            response
                        } else if command.contains("UID FETCH 1 ")
                            || command.contains("UID FETCH 99 ")
                        {
                            format!("{tag} OK FETCH completed\r\n").into_bytes()
                        } else if command.contains(" LOGOUT") {
                            let response = format!(
                                "* BYE fixture closing\r\n{tag} OK LOGOUT completed\r\n"
                            )
                            .into_bytes();
                            write.write_all(&response).await.unwrap();
                            break;
                        } else {
                            format!("{tag} BAD unsupported fixture command\r\n").into_bytes()
                        };
                        write.write_all(&response).await.unwrap();
                    }
                    transcript
                })
        })
    }

    fn spawn_smtp_fixture(
        listener: StdTcpListener,
        tls: Arc<rustls::ServerConfig>,
        connections: usize,
    ) -> JoinHandle<Vec<Vec<u8>>> {
        thread::spawn(move || {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let listener = TcpListener::from_std(listener).unwrap();
                    let acceptor = TlsAcceptor::from(tls);
                    let mut messages = Vec::new();
                    for _ in 0..connections {
                        let (stream, _) = listener.accept().await.unwrap();
                        let stream = acceptor.accept(stream).await.unwrap();
                        if let Some(message) = serve_smtp_connection(stream).await {
                            messages.push(message);
                        }
                    }
                    messages
                })
        })
    }

    async fn serve_smtp_connection(mut stream: ServerTlsStream<TcpStream>) -> Option<Vec<u8>> {
        stream
            .write_all(b"220 localhost iMail fixture\r\n")
            .await
            .unwrap();
        let (read, mut write) = tokio::io::split(stream);
        let mut lines = BufReader::new(read).lines();
        let mut login_step = 0_u8;
        let mut data: Option<Vec<u8>> = None;
        let mut collecting_data = false;
        while let Some(line) = lines.next_line().await.unwrap() {
            let upper = line.to_ascii_uppercase();
            if collecting_data {
                if line == "." {
                    collecting_data = false;
                    write.write_all(b"250 2.0.0 queued\r\n").await.unwrap();
                } else {
                    let payload = data.as_mut().unwrap();
                    payload.extend_from_slice(line.as_bytes());
                    payload.extend_from_slice(b"\r\n");
                    continue;
                }
            } else if login_step == 1 {
                login_step = 2;
                write.write_all(b"334 UGFzc3dvcmQ6\r\n").await.unwrap();
            } else if login_step == 2 {
                login_step = 0;
                write
                    .write_all(b"235 2.7.0 authentication successful\r\n")
                    .await
                    .unwrap();
            } else if upper.starts_with("EHLO ") {
                write
                    .write_all(b"250-localhost\r\n250-AUTH PLAIN LOGIN XOAUTH2\r\n250 8BITMIME\r\n")
                    .await
                    .unwrap();
            } else if upper == "AUTH LOGIN" {
                login_step = 1;
                write.write_all(b"334 VXNlcm5hbWU6\r\n").await.unwrap();
            } else if upper.starts_with("AUTH PLAIN ") || upper.starts_with("AUTH XOAUTH2 ") {
                write
                    .write_all(b"235 2.7.0 authentication successful\r\n")
                    .await
                    .unwrap();
            } else if upper.starts_with("MAIL FROM:") || upper.starts_with("RCPT TO:") {
                write.write_all(b"250 2.1.0 accepted\r\n").await.unwrap();
            } else if upper == "DATA" {
                data = Some(Vec::new());
                collecting_data = true;
                write
                    .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                    .await
                    .unwrap();
            } else if upper == "QUIT" {
                write.write_all(b"221 2.0.0 bye\r\n").await.unwrap();
                break;
            } else {
                write.write_all(b"500 unsupported\r\n").await.unwrap();
            }
        }
        data
    }

    fn fixture_config(imap_port: u16, smtp_port: u16) -> MailConnectionConfig {
        MailConnectionConfig {
            email: "fixture@example.test".into(),
            display_name: "Fixture Owner".into(),
            imap_host: "localhost".into(),
            imap_port,
            imap_secure: true,
            smtp_host: "localhost".into(),
            smtp_port,
            smtp_secure: true,
            authentication: MailAuthentication::Password("fixture-password".into()),
            proxy: None,
        }
    }

    #[test]
    fn real_tls_imap_and_smtp_fixture_covers_verify_sync_mime_and_send() {
        let (tls_connector, tls_server) = fixture_tls();
        let (imap_listener, imap_port) = fixture_listener();
        let (smtp_listener, smtp_port) = fixture_listener();
        let source = concat!(
            "From: Sender <sender@example.test>\r\n",
            "To: Fixture <fixture@example.test>\r\n",
            "Subject: fixture inbound\r\n",
            "Message-ID: <fixture-inbound@example.test>\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "fixture body\r\n"
        )
        .as_bytes()
        .to_vec();
        let imap = spawn_imap_fixture(imap_listener, Arc::clone(&tls_server), 2, source);
        let smtp = spawn_smtp_fixture(smtp_listener, tls_server, 2);
        let config = fixture_config(imap_port, smtp_port);
        let mut adapter = NetworkMailAdapter::with_tls_connector(tls_connector).unwrap();

        ImapPort::verify(&mut adapter, &config).unwrap();
        let batch = ImapSyncPort::fetch_incremental(
            &mut adapter,
            &config,
            &RemoteSyncRequest {
                target: imail_mail::SyncTarget {
                    mailbox_role: "inbox".into(),
                    requested_mailbox: None,
                },
                cursor: Default::default(),
                cached_uids: vec![],
            },
        )
        .unwrap();
        assert_eq!(batch.uid_validity.as_deref(), Some("7"));
        assert_eq!(batch.last_seen_uid, 1);
        assert_eq!(batch.incoming.len(), 1);
        let parsed = imail_mail::parse_rfc822(&batch.incoming[0].source).unwrap();
        assert_eq!(parsed.subject, "fixture inbound");
        assert_eq!(parsed.text, "fixture body");

        SmtpPort::verify(&mut adapter, &config).unwrap();
        let sent = SmtpPort::send(
            &mut adapter,
            &config,
            &OutgoingMessage {
                from: MailAddressView {
                    name: "Fixture Owner".into(),
                    address: "fixture@example.test".into(),
                },
                to: vec!["recipient@example.test".into()],
                cc: None,
                subject: "fixture outbound".into(),
                text: "fixture send body".into(),
                html: None,
                attachments: vec![],
            },
        )
        .unwrap();
        assert_eq!(sent.accepted, vec!["recipient@example.test"]);

        let imap_transcript = imap.join().unwrap();
        assert!(imap_transcript
            .iter()
            .any(|line| line.to_ascii_uppercase().contains(" LOGIN ")));
        assert!(imap_transcript
            .iter()
            .any(|line| line.to_ascii_uppercase().contains(" FETCH ")));
        let smtp_messages = smtp.join().unwrap();
        assert_eq!(smtp_messages.len(), 1);
        let outbound = imail_mail::parse_rfc822(&smtp_messages[0]).unwrap();
        assert_eq!(outbound.subject, "fixture outbound");
        assert_eq!(outbound.text, "fixture send body");
    }

    #[test]
    fn icloud_fallback_matches_headers_locally_when_uid_and_server_search_are_stale() {
        let (tls_connector, tls_server) = fixture_tls();
        let (imap_listener, imap_port) = fixture_listener();
        let source = concat!(
            "From: Sender <sender@example.test>\r\n",
            "To: Fixture <fixture@example.test>\r\n",
            "Subject: iCloud attachment\r\n",
            "Message-ID: <icloud-fallback@example.test>\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "attachment source\r\n"
        )
        .as_bytes()
        .to_vec();
        let fixture = spawn_icloud_fallback_fixture(imap_listener, tls_server, source.clone());
        let mut adapter = NetworkMailAdapter::with_tls_connector(tls_connector).unwrap();
        let fetched = ImapPort::fetch_source(
            &mut adapter,
            &fixture_config(imap_port, 465),
            &RemoteMessageLocator {
                mailbox: "INBOX".into(),
                uid: 99,
                message_id: Some("<icloud-fallback@example.test>".into()),
            },
        )
        .unwrap();
        assert_eq!(fetched, source);
        let transcript = fixture.join().unwrap().join("\n").to_ascii_uppercase();
        assert!(transcript.contains("UID SEARCH HEADER MESSAGE-ID"));
        assert!(transcript.contains("UID SEARCH ALL"));
        assert!(transcript.contains("HEADER.FIELDS (MESSAGE-ID)"));
    }

    #[test]
    fn builds_a_safe_multipart_message_with_a_message_id() {
        let message = OutgoingMessage {
            from: MailAddressView {
                name: "发件人".into(),
                address: "sender@example.com".into(),
            },
            to: vec!["recipient@example.com".into()],
            cc: Some(vec!["copy@example.com".into()]),
            subject: "你好".into(),
            text: "正文".into(),
            html: Some("<p>正文</p>".into()),
            attachments: vec![OutgoingAttachment {
                filename: "报告.txt".into(),
                content_type: "text/plain".into(),
                content: b"hello".to_vec(),
            }],
        };

        let (source, message_id) = build_message_source(&message).unwrap();
        let parsed = imail_mail::parse_rfc822(&source).unwrap();
        assert_eq!(parsed.subject, "你好");
        assert_eq!(parsed.attachments[0].filename, "报告.txt");
        assert_eq!(message_id, parsed.message_id.unwrap());
        assert!(!source.windows(7).any(|window| window == b"file://"));
    }

    #[test]
    fn yahoo_uses_the_existing_oauthbearer_frame() {
        let config = MailConnectionConfig {
            email: "owner@example.com".into(),
            display_name: "Owner".into(),
            imap_host: "imap.mail.yahoo.com".into(),
            imap_port: 993,
            imap_secure: true,
            smtp_host: "smtp.mail.yahoo.com".into(),
            smtp_port: 465,
            smtp_secure: true,
            authentication: MailAuthentication::OAuth {
                provider: "yahoo".into(),
                access_token: "secret-token".into(),
            },
            proxy: None,
        };
        assert!(matches!(
            smtp_credentials(&config),
            Credentials::OAuthBearer { .. }
        ));
    }

    #[test]
    fn message_id_search_values_reject_injection_and_escape_quotes() {
        assert_eq!(
            imap_search_message_id("<mail@example.org>"),
            Some("<mail@example.org>".into())
        );
        assert_eq!(
            imap_search_message_id("<mail\\\"tag@example.org>"),
            Some("<mail\\\\\\\"tag@example.org>".into())
        );
        assert_eq!(imap_search_message_id("bad\r\nUID SEARCH ALL"), None);
        assert_eq!(imap_search_message_id(""), None);
    }

    #[test]
    fn extracts_message_id_from_the_minimal_header_used_for_icloud_fallback() {
        assert_eq!(
            header_message_id(b"Message-ID: <Cloud-Part-42@icloud.example>\r\n\r\n"),
            Some("cloud-part-42@icloud.example".into())
        );
        assert_eq!(
            normalized_message_id("  <Cloud-Part-42@icloud.example>  "),
            Some("cloud-part-42@icloud.example".into())
        );
        assert_eq!(header_message_id(b"Subject: no identity\r\n\r\n"), None);
        assert!(source_matches_message_id(
            b"Message-ID: <same@example.test>\r\n\r\nbody",
            Some("<same@example.test>")
        ));
        assert!(!source_matches_message_id(
            b"Message-ID: <reused-uid@example.test>\r\n\r\nbody",
            Some("<same@example.test>")
        ));
    }

    #[test]
    fn http_connect_uses_basic_proxy_auth_without_exposing_it_in_errors() {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let proxy = MailProxy {
                protocol: "http".into(),
                host: "proxy.example.com".into(),
                port: 3128,
                username: Some("mail user".into()),
                password: Some("proxy-secret".into()),
            };
            let (mut client, mut server) = tokio::io::duplex(4_096);
            let server_task = tokio::spawn(async move {
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    server.read_exact(&mut byte).await.unwrap();
                    request.push(byte[0]);
                }
                server
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
                String::from_utf8(request).unwrap()
            });

            establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                .await
                .unwrap();
            let request = server_task.await.unwrap();
            assert!(request.starts_with("CONNECT imap.example.com:993 HTTP/1.1\r\n"));
            assert!(request.contains("Proxy-Authorization: Basic bWFpbCB1c2VyOnByb3h5LXNlY3JldA=="));

            let (mut client, mut server) = tokio::io::duplex(4_096);
            let rejection = tokio::spawn(async move {
                let mut request = [0_u8; 256];
                let _ = server.read(&mut request).await.unwrap();
                server
                    .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                    .await
                    .unwrap();
            });
            let error = establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                .await
                .unwrap_err();
            rejection.await.unwrap();
            let NetworkError::Provider(detail) = error;
            assert!(detail.contains("407"));
            assert!(!detail.contains("proxy-secret"));
            assert!(!detail.contains("bWFpbCB1c2Vy"));
        });
    }

    #[test]
    fn maps_standard_imap_special_use_attributes() {
        use async_imap::types::NameAttribute;
        assert_eq!(
            special_use(&[NameAttribute::Archive]).as_deref(),
            Some("\\Archive")
        );
        assert_eq!(special_use(&[NameAttribute::All]).as_deref(), Some("\\All"));
        assert_eq!(
            special_use(&[NameAttribute::Trash]).as_deref(),
            Some("\\Trash")
        );
        assert_eq!(special_use(&[NameAttribute::NoSelect]), None);
    }

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
    fn adapter_timeout_is_stable_and_does_not_wait_for_the_real_network_limit() {
        let adapter = NetworkMailAdapter::new().unwrap();
        let failure = adapter
            .run_with_timeout::<()>(
                ProtocolStage::Imap,
                Duration::from_millis(1),
                std::future::pending(),
            )
            .unwrap_err();
        assert_eq!(failure.status.as_deref(), Some("TIMEOUT"));
        assert_eq!(failure.message, "IMAP 验证失败 (TIMEOUT)：网络操作超时");
    }

    struct Cancelled;

    impl NetworkCancellation for Cancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    #[test]
    fn injected_cancellation_interrupts_an_in_flight_network_future() {
        let adapter = NetworkMailAdapter::with_cancellation(Arc::new(Cancelled)).unwrap();
        let started = std::time::Instant::now();
        let failure = adapter
            .run_with_timeout::<()>(
                ProtocolStage::Imap,
                Duration::from_secs(120),
                std::future::pending(),
            )
            .unwrap_err();
        assert_eq!(failure.status.as_deref(), Some("CANCELLED"));
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn rejects_disconnected_and_oversized_http_proxy_responses() {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let proxy = MailProxy {
                protocol: "http".into(),
                host: "proxy.example.com".into(),
                port: 3128,
                username: None,
                password: None,
            };
            let (mut client, server) = tokio::io::duplex(128);
            drop(server);
            assert!(
                establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                    .await
                    .is_err()
            );

            let (mut client, mut server) = tokio::io::duplex(256);
            let server_task = tokio::spawn(async move {
                let mut request = [0_u8; 256];
                let _ = server.read(&mut request).await.unwrap();
                server
                    .write_all(&vec![b'x'; MAX_PROXY_RESPONSE_BYTES + 1])
                    .await
                    .unwrap();
            });
            let error = establish_http_connect(&mut client, &proxy, "imap.example.com", 993)
                .await
                .unwrap_err();
            server_task.await.unwrap();
            let NetworkError::Provider(detail) = error;
            assert_eq!(detail, "HTTP 代理响应过大或不完整");
        });
    }
}
