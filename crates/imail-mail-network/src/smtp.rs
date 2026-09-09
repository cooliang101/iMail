//! SMTP authentication, MIME construction and envelope delivery.
use crate::{
    error::NetworkError,
    tls::{connect_tls, SecureStream},
    tunnel::connect_tunnel,
    NetworkMailAdapter, OPERATION_TIMEOUT,
};
use imail_mail::{
    MailAuthentication, MailConnectionConfig, OutgoingMessage, ProtocolFailure, ProtocolStage,
    SmtpPort,
};
use imail_protocol::SendMessageResult;
use mail_builder::MessageBuilder;
use mail_send::{smtp::AssertReply, Credentials, SmtpClient};
use tokio_rustls::TlsConnector;
type SmtpSession = SmtpClient<SecureStream>;

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
            let recipients = envelope_recipients(message);
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
    if !message.envelope.is_valid() {
        return Err(NetworkError::Provider("密送或回复关联无效".into()));
    }
    let mut builder = MessageBuilder::new()
        .from((message.from.name.clone(), message.from.address.clone()))
        .to(message.to.clone())
        .subject(message.subject.clone())
        .text_body(message.text.clone());
    if let Some(cc) = &message.cc {
        builder = builder.cc(cc.clone());
    }
    // Bcc is an SMTP envelope recipient only. Never write it into delivered MIME.
    let bare_ids = |ids: &[String]| {
        ids.iter()
            .filter_map(|id| imail_protocol::normalize_message_id(id))
            .map(|id| id[1..id.len() - 1].to_string())
            .collect::<Vec<_>>()
    };
    if !message.envelope.reply.in_reply_to.is_empty() {
        builder = builder.in_reply_to(bare_ids(&message.envelope.reply.in_reply_to));
    }
    if !message.envelope.reply.references.is_empty() {
        builder = builder.references(bare_ids(&message.envelope.reply.references));
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

fn envelope_recipients(message: &OutgoingMessage) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    message
        .to
        .iter()
        .chain(message.cc.iter().flatten())
        .chain(&message.envelope.bcc)
        .filter(|address| seen.insert(address.to_ascii_lowercase()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use imail_mail::OutgoingAttachment;
    use imail_protocol::MailAddressView;
    #[test]
    fn builds_a_safe_multipart_message_with_a_message_id() {
        let message = OutgoingMessage {
            envelope: Default::default(),
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
}
