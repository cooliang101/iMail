//! IMAP sessions, message retrieval and remote mail operations.
use crate::{
    error::NetworkError,
    tls::{connect_tls, SecureStream},
    tunnel::connect_tunnel,
    NetworkMailAdapter,
};
use async_imap::{Authenticator, Client, Session};
use futures_util::TryStreamExt;
use imail_mail::{
    ImapPort, MailAuthentication, MailConnectionConfig, ProtocolFailure, ProtocolStage,
    RemoteMailbox, RemoteMessageLocator, RemoteMoveConfirmation,
};
use imail_protocol::RemoteMessageFlagPatch;
use mail_parser::MessageParser;
use std::fmt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsConnector;
type ImapSession = Session<SecureStream>;
const MESSAGE_ID_SCAN_UID_BATCH: usize = 500;

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

pub(super) async fn connect_imap(
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

pub(super) fn special_use(attributes: &[async_imap::types::NameAttribute<'_>]) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
