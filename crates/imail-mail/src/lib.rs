use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use imail_protocol::{
    MailAddressView, MessageMoveDestination, ParsedAttachmentView, ParsedMailView,
    RemoteMessageFlagPatch, RemoteMessageMoveResult, SendMessageInput, SendMessageResult,
};
use mail_parser::{Addr, MessageParser, MimeHeaders};
use regex::Regex;
use std::sync::OnceLock;
use thiserror::Error;

mod sync;
pub use sync::*;

pub const MAX_RFC822_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum MailParseError {
    #[error("邮件原始内容过大")]
    MessageTooLarge,
    #[error("邮件原始内容无法解析")]
    InvalidMessage,
    #[error("附件不存在")]
    AttachmentNotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolStage {
    Imap,
    Smtp,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ProtocolFailure {
    pub stage: ProtocolStage,
    pub status: Option<String>,
    pub message: String,
}

impl ProtocolFailure {
    pub fn from_provider(
        stage: ProtocolStage,
        status: Option<&str>,
        provider_detail: &str,
    ) -> Self {
        let status = status.map(|value| value.trim().chars().take(80).collect::<String>());
        let safe_detail = redact_protocol_detail(provider_detail);
        let stage_name = match stage {
            ProtocolStage::Imap => "IMAP",
            ProtocolStage::Smtp => "SMTP",
        };
        let status_text = status
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| format!(" ({value})"))
            .unwrap_or_default();
        Self {
            stage,
            status,
            message: format!("{stage_name} 验证失败{status_text}：{safe_detail}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailAuthentication {
    Password(String),
    OAuth {
        provider: String,
        access_token: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailProxy {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailConnectionConfig {
    pub email: String,
    pub display_name: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_secure: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_secure: bool,
    pub authentication: MailAuthentication,
    pub proxy: Option<MailProxy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessageLocator {
    pub mailbox: String,
    pub uid: u32,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMailbox {
    pub path: String,
    pub special_use: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteMoveConfirmation {
    pub confirmed: bool,
    pub uid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedAttachment {
    pub content: Vec<u8>,
    pub filename: String,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingAttachment {
    pub filename: String,
    pub content_type: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    pub from: MailAddressView,
    pub to: Vec<String>,
    pub cc: Option<Vec<String>>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub attachments: Vec<OutgoingAttachment>,
}

pub trait ImapPort {
    fn verify(&mut self, config: &MailConnectionConfig) -> Result<(), ProtocolFailure>;
    fn fetch_source(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
    ) -> Result<Vec<u8>, ProtocolFailure>;
    fn update_flags(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
        patch: &RemoteMessageFlagPatch,
    ) -> Result<(), ProtocolFailure>;
    fn list_mailboxes(
        &mut self,
        config: &MailConnectionConfig,
    ) -> Result<Vec<RemoteMailbox>, ProtocolFailure>;
    fn move_message(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
        target_mailbox: &str,
    ) -> Result<RemoteMoveConfirmation, ProtocolFailure>;
}

pub trait SmtpPort {
    fn verify(&mut self, config: &MailConnectionConfig) -> Result<(), ProtocolFailure>;
    fn send(
        &mut self,
        config: &MailConnectionConfig,
        message: &OutgoingMessage,
    ) -> Result<SendMessageResult, ProtocolFailure>;
}

#[derive(Debug, Error)]
pub enum RemoteMailError {
    #[error("附件不存在")]
    AttachmentNotFound,
    #[error("无法从邮箱服务器读取附件")]
    AttachmentUnavailable,
    #[error("服务商没有返回{0}文件夹")]
    DestinationUnavailable(&'static str),
    #[error("服务商未确认邮件移动操作")]
    MoveUnconfirmed,
    #[error("附件数据不是有效的 Base64")]
    InvalidAttachmentData,
    #[error(transparent)]
    Protocol(#[from] ProtocolFailure),
    #[error(transparent)]
    Parse(#[from] MailParseError),
}

pub struct RemoteMailService<'a, I: ImapPort + ?Sized, S: SmtpPort + ?Sized> {
    imap: &'a mut I,
    smtp: &'a mut S,
}

impl<'a, I: ImapPort + ?Sized, S: SmtpPort + ?Sized> RemoteMailService<'a, I, S> {
    pub fn new(imap: &'a mut I, smtp: &'a mut S) -> Self {
        Self { imap, smtp }
    }

    pub fn verify_account(&mut self, config: &MailConnectionConfig) -> Result<(), RemoteMailError> {
        self.imap.verify(config)?;
        self.smtp.verify(config)?;
        Ok(())
    }

    pub fn download_attachment(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
        metadata: &[ParsedAttachmentView],
        index: usize,
    ) -> Result<DownloadedAttachment, RemoteMailError> {
        let fallback = metadata
            .get(index)
            .ok_or(RemoteMailError::AttachmentNotFound)?;
        let source = self.imap.fetch_source(config, locator)?;
        let parsed = parse_rfc822(&source)?;
        let parsed_metadata = parsed
            .attachments
            .get(index)
            .ok_or(RemoteMailError::AttachmentUnavailable)?;
        Ok(DownloadedAttachment {
            content: attachment_content(&source, index)
                .map_err(|_| RemoteMailError::AttachmentUnavailable)?,
            filename: if parsed_metadata.filename.is_empty() {
                fallback.filename.clone()
            } else {
                parsed_metadata.filename.clone()
            },
            content_type: if parsed_metadata.content_type.is_empty() {
                fallback.content_type.clone()
            } else {
                parsed_metadata.content_type.clone()
            },
        })
    }

    pub fn update_flags(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
        patch: &RemoteMessageFlagPatch,
    ) -> Result<(), RemoteMailError> {
        self.imap.update_flags(config, locator, patch)?;
        Ok(())
    }

    pub fn move_message(
        &mut self,
        config: &MailConnectionConfig,
        locator: &RemoteMessageLocator,
        destination: MessageMoveDestination,
    ) -> Result<RemoteMessageMoveResult, RemoteMailError> {
        let mailboxes = self.imap.list_mailboxes(config)?;
        let (uses, label): (&[&str], &str) = match destination {
            MessageMoveDestination::Archive => (&["\\Archive", "\\All"], "归档"),
            MessageMoveDestination::Trash => (&["\\Trash"], "垃圾箱"),
        };
        let target = mailboxes
            .iter()
            .find(|mailbox| {
                mailbox
                    .special_use
                    .as_deref()
                    .is_some_and(|special_use| uses.contains(&special_use))
            })
            .ok_or(RemoteMailError::DestinationUnavailable(label))?;
        let moved = self.imap.move_message(config, locator, &target.path)?;
        if !moved.confirmed {
            return Err(RemoteMailError::MoveUnconfirmed);
        }
        Ok(RemoteMessageMoveResult {
            mailbox: target.path.clone(),
            uid: moved.uid,
        })
    }

    pub fn send_message(
        &mut self,
        config: &MailConnectionConfig,
        input: &SendMessageInput,
    ) -> Result<SendMessageResult, RemoteMailError> {
        let attachments = input
            .attachments
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|attachment| {
                Ok(OutgoingAttachment {
                    filename: attachment.filename.clone(),
                    content_type: attachment.content_type.clone(),
                    content: BASE64
                        .decode(&attachment.data)
                        .map_err(|_| RemoteMailError::InvalidAttachmentData)?,
                })
            })
            .collect::<Result<Vec<_>, RemoteMailError>>()?;
        self.smtp
            .send(
                config,
                &OutgoingMessage {
                    from: MailAddressView {
                        name: config.display_name.clone(),
                        address: config.email.clone(),
                    },
                    to: input.to.clone(),
                    cc: input.cc.clone(),
                    subject: input.subject.clone(),
                    text: input.text.clone(),
                    html: input.html.clone(),
                    attachments,
                },
            )
            .map_err(RemoteMailError::from)
    }
}

pub fn parse_rfc822(input: &[u8]) -> Result<ParsedMailView, MailParseError> {
    if input.len() > MAX_RFC822_BYTES {
        return Err(MailParseError::MessageTooLarge);
    }
    let message = MessageParser::default()
        .parse(input)
        .ok_or(MailParseError::InvalidMessage)?;
    let from = message
        .from()
        .and_then(|addresses| addresses.first())
        .map(address)
        .unwrap_or_else(empty_address);
    let to = message
        .to()
        .map(|addresses| addresses.iter().map(address).collect())
        .unwrap_or_default();
    let text = message
        .body_text(0)
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let html = (message.html_body_count() > 0)
        .then(|| message.body_html(0).map(|value| value.into_owned()))
        .flatten();
    let preview_source = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview = truncate_utf16(&preview_source, 180);
    let date = message.date().and_then(|value| {
        DateTime::<Utc>::from_timestamp(value.to_timestamp(), 0)
            .map(|date| date.to_rfc3339_opts(SecondsFormat::Millis, true))
    });
    let attachments = message
        .attachments()
        .enumerate()
        .map(|(index, part)| {
            let content_type = part
                .content_type()
                .map(|value| {
                    format!(
                        "{}/{}",
                        value.c_type,
                        value.c_subtype.as_deref().unwrap_or("octet-stream")
                    )
                })
                .unwrap_or_else(|| "application/octet-stream".into());
            ParsedAttachmentView {
                filename: part.attachment_name().unwrap_or("attachment").to_string(),
                content_type,
                size: part.len(),
                index,
            }
        })
        .collect();
    Ok(ParsedMailView {
        message_id: message.message_id().map(node_message_id),
        from,
        to,
        subject: message
            .subject()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("（无主题）")
            .to_string(),
        text,
        html,
        date,
        preview,
        attachments,
    })
}

pub fn attachment_content(input: &[u8], index: usize) -> Result<Vec<u8>, MailParseError> {
    if input.len() > MAX_RFC822_BYTES {
        return Err(MailParseError::MessageTooLarge);
    }
    let message = MessageParser::default()
        .parse(input)
        .ok_or(MailParseError::InvalidMessage)?;
    message
        .attachment(index)
        .map(|part| part.contents().to_vec())
        .ok_or(MailParseError::AttachmentNotFound)
}

fn address(value: &Addr<'_>) -> MailAddressView {
    MailAddressView {
        name: value.name.as_deref().unwrap_or_default().to_string(),
        address: value.address.as_deref().unwrap_or_default().to_string(),
    }
}

fn empty_address() -> MailAddressView {
    MailAddressView {
        name: String::new(),
        address: String::new(),
    }
}

fn node_message_id(value: &str) -> String {
    if value.starts_with('<') && value.ends_with('>') {
        value.to_string()
    } else {
        format!("<{value}>")
    }
}

pub fn redact_protocol_detail(value: &str) -> String {
    static URI_CREDENTIALS: OnceLock<Regex> = OnceLock::new();
    static BEARER: OnceLock<Regex> = OnceLock::new();
    static NAMED_SECRET: OnceLock<Regex> = OnceLock::new();
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let without_uri_credentials = URI_CREDENTIALS
        .get_or_init(|| Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s@]+@").unwrap())
        .replace_all(&collapsed, "$1[redacted]@");
    let without_bearer = BEARER
        .get_or_init(|| Regex::new(r"(?i)Bearer\s+[^\s,;]+").unwrap())
        .replace_all(&without_uri_credentials, "Bearer [redacted]");
    let without_named_secrets = NAMED_SECRET
        .get_or_init(|| {
            Regex::new(
                r"(?i)(access[_-]?token|refresh[_-]?token|password|authorization)(\s*[:=]\s*)[^\s,;]+",
            )
            .unwrap()
        })
        .replace_all(&without_bearer, "$1$2[redacted]");
    let safe = without_named_secrets.trim();
    if safe.is_empty() {
        "服务商拒绝了连接请求".to_string()
    } else {
        truncate_utf16(safe, 500)
    }
}

fn truncate_utf16(value: &str, maximum: usize) -> String {
    let mut units = 0;
    value
        .chars()
        .take_while(|character| {
            let next = units + character.len_utf16();
            if next > maximum {
                false
            } else {
                units = next;
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use imail_protocol::{SendAttachmentInput, SendMessageInput};

    const MIME_FIXTURE: &[u8] = include_bytes!("../../../fixtures/r4-mime-v1.eml");

    #[test]
    fn matches_the_shared_node_mime_contract() {
        let expected: ParsedMailView =
            serde_json::from_str(include_str!("../../../fixtures/r4-mime-v1.json")).unwrap();

        assert_eq!(parse_rfc822(MIME_FIXTURE).unwrap(), expected);
        assert_eq!(attachment_content(MIME_FIXTURE, 0).unwrap(), b"hello");
    }

    #[test]
    fn rejects_missing_attachments_and_oversized_messages() {
        assert!(matches!(
            attachment_content(MIME_FIXTURE, 99),
            Err(MailParseError::AttachmentNotFound)
        ));
        let oversized = vec![b'x'; MAX_RFC822_BYTES + 1];
        assert!(matches!(
            parse_rfc822(&oversized),
            Err(MailParseError::MessageTooLarge)
        ));
    }

    #[test]
    fn preview_truncation_matches_javascript_utf16_slicing_at_character_boundaries() {
        let input = format!("{}😀tail", "a".repeat(179));
        assert_eq!(truncate_utf16(&input, 180), "a".repeat(179));
    }

    #[test]
    fn protocol_errors_match_the_existing_safe_message_contract() {
        let failure = ProtocolFailure::from_provider(
            ProtocolStage::Imap,
            Some("NO"),
            "bad\r\nBearer secret-token password=hunter2 socks5://mail:proxy-secret@host:1080",
        );

        assert_eq!(failure.status.as_deref(), Some("NO"));
        assert!(failure
            .message
            .starts_with("IMAP 验证失败 (NO)：bad Bearer [redacted]"));
        assert!(!failure.message.contains("secret-token"));
        assert!(!failure.message.contains("hunter2"));
        assert!(!failure.message.contains("proxy-secret"));
    }

    #[test]
    fn remote_operations_use_protocol_ports_without_http_or_storage() {
        let mut imap = FakeImap {
            source: MIME_FIXTURE.to_vec(),
            mailboxes: vec![
                RemoteMailbox {
                    path: "[Gmail]/All Mail".into(),
                    special_use: Some("\\All".into()),
                },
                RemoteMailbox {
                    path: "Trash".into(),
                    special_use: Some("\\Trash".into()),
                },
            ],
            move_confirmation: RemoteMoveConfirmation {
                confirmed: true,
                uid: Some(84),
            },
            ..Default::default()
        };
        let mut smtp = FakeSmtp::default();
        let config = connection_config();
        let locator = RemoteMessageLocator {
            mailbox: "INBOX".into(),
            uid: 42,
            message_id: Some("<fixture@example.org>".into()),
        };
        let metadata = vec![ParsedAttachmentView {
            filename: "fallback.txt".into(),
            content_type: "application/octet-stream".into(),
            size: 5,
            index: 0,
        }];
        let patch = RemoteMessageFlagPatch {
            unread: Some(false),
            flagged: Some(true),
        };
        let send = SendMessageInput {
            account_id: "account-1".into(),
            to: vec!["recipient@example.com".into()],
            cc: None,
            subject: "Hello".into(),
            text: "Body".into(),
            html: Some("<p>Body</p>".into()),
            attachments: Some(vec![SendAttachmentInput {
                filename: "note.txt".into(),
                content_type: "text/plain".into(),
                data: "aGVsbG8=".into(),
            }]),
        };

        {
            let mut service = RemoteMailService::new(&mut imap, &mut smtp);
            service.verify_account(&config).unwrap();
            let attachment = service
                .download_attachment(&config, &locator, &metadata, 0)
                .unwrap();
            assert_eq!(attachment.content, b"hello");
            assert_eq!(attachment.filename, "报告.txt");
            service.update_flags(&config, &locator, &patch).unwrap();
            assert_eq!(
                service
                    .move_message(&config, &locator, MessageMoveDestination::Archive)
                    .unwrap(),
                RemoteMessageMoveResult {
                    mailbox: "[Gmail]/All Mail".into(),
                    uid: Some(84),
                }
            );
            assert_eq!(
                service.send_message(&config, &send).unwrap().message_id,
                "<sent@example.com>"
            );
        }

        assert_eq!(imap.fetched, vec![locator.clone()]);
        assert_eq!(imap.flag_updates, vec![(locator.clone(), patch)]);
        assert_eq!(imap.moves, vec![(locator, "[Gmail]/All Mail".into())]);
        let outgoing = smtp.sent.as_ref().unwrap();
        assert_eq!(outgoing.from.name, "Owner");
        assert_eq!(outgoing.from.address, "owner@example.com");
        assert_eq!(outgoing.attachments[0].content, b"hello");
    }

    #[test]
    fn remote_operations_preserve_failure_semantics() {
        let mut imap = FakeImap {
            source: MIME_FIXTURE.to_vec(),
            mailboxes: Vec::new(),
            ..Default::default()
        };
        let mut smtp = FakeSmtp::default();
        let config = connection_config();
        let locator = RemoteMessageLocator {
            mailbox: "INBOX".into(),
            uid: 42,
            message_id: None,
        };
        let mut service = RemoteMailService::new(&mut imap, &mut smtp);

        assert!(matches!(
            service.download_attachment(&config, &locator, &[], 0),
            Err(RemoteMailError::AttachmentNotFound)
        ));
        assert!(matches!(
            service.move_message(&config, &locator, MessageMoveDestination::Trash),
            Err(RemoteMailError::DestinationUnavailable("垃圾箱"))
        ));
    }

    fn connection_config() -> MailConnectionConfig {
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

    #[derive(Default)]
    struct FakeImap {
        source: Vec<u8>,
        mailboxes: Vec<RemoteMailbox>,
        move_confirmation: RemoteMoveConfirmation,
        fetched: Vec<RemoteMessageLocator>,
        flag_updates: Vec<(RemoteMessageLocator, RemoteMessageFlagPatch)>,
        moves: Vec<(RemoteMessageLocator, String)>,
    }

    impl ImapPort for FakeImap {
        fn verify(&mut self, _config: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
            Ok(())
        }

        fn fetch_source(
            &mut self,
            _config: &MailConnectionConfig,
            locator: &RemoteMessageLocator,
        ) -> Result<Vec<u8>, ProtocolFailure> {
            self.fetched.push(locator.clone());
            Ok(self.source.clone())
        }

        fn update_flags(
            &mut self,
            _config: &MailConnectionConfig,
            locator: &RemoteMessageLocator,
            patch: &RemoteMessageFlagPatch,
        ) -> Result<(), ProtocolFailure> {
            self.flag_updates.push((locator.clone(), patch.clone()));
            Ok(())
        }

        fn list_mailboxes(
            &mut self,
            _config: &MailConnectionConfig,
        ) -> Result<Vec<RemoteMailbox>, ProtocolFailure> {
            Ok(self.mailboxes.clone())
        }

        fn move_message(
            &mut self,
            _config: &MailConnectionConfig,
            locator: &RemoteMessageLocator,
            target_mailbox: &str,
        ) -> Result<RemoteMoveConfirmation, ProtocolFailure> {
            self.moves
                .push((locator.clone(), target_mailbox.to_string()));
            Ok(self.move_confirmation.clone())
        }
    }

    #[derive(Default)]
    struct FakeSmtp {
        sent: Option<OutgoingMessage>,
    }

    impl SmtpPort for FakeSmtp {
        fn verify(&mut self, _config: &MailConnectionConfig) -> Result<(), ProtocolFailure> {
            Ok(())
        }

        fn send(
            &mut self,
            _config: &MailConnectionConfig,
            message: &OutgoingMessage,
        ) -> Result<SendMessageResult, ProtocolFailure> {
            self.sent = Some(message.clone());
            Ok(SendMessageResult {
                message_id: "<sent@example.com>".into(),
                accepted: message.to.clone(),
            })
        }
    }
}
