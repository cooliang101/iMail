use std::{
    env,
    error::Error,
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use imail_mail::{
    attachment_content, parse_rfc822, ImapPort, ImapSyncPort, ImapWakePort, MailAuthentication,
    MailConnectionConfig, OutgoingAttachment, OutgoingMessage, RemoteMailbox, RemoteMessageLocator,
    RemoteSyncRequest, SmtpPort, SyncCursor, SyncTarget,
};
use imail_mail_network::{NetworkCancellation, NetworkMailAdapter};
use imail_oauth::{
    callback_error, provider_config, BeginOAuthInput, OAuthEnvironment, OAuthProviderKey,
    OAuthService,
};
use imail_oauth_http::{loopback::LoopbackCallbackServer, OAuthHttpAdapter};
use imail_protocol::{MailAddressView, RemoteMessageFlagPatch};
use imail_security::MasterKey;
use serde_json::json;

fn main() -> Result<(), Box<dyn Error>> {
    require_remote_write_guard()?;
    let AcceptanceConfig {
        mail: sender_config,
        authentication,
        provider,
        oauth_authorized,
        oauth_refresh_verified,
    } = load_config()?;
    let recipient = load_recipient_config()?;
    let allowed_recipients = required_allowed_recipients()?;
    validate_closed_recipient_set(
        &allowed_recipients,
        &sender_config.email,
        &recipient.mail.email,
    )?;
    let (run_id, subject, submit_this_run) = acceptance_identity()?;
    let attachment = format!("imail-rust-acceptance-{run_id}").into_bytes();
    let started = Instant::now();

    let mut sender_adapter = NetworkMailAdapter::new()?;
    ImapPort::verify(&mut sender_adapter, &sender_config)?;
    SmtpPort::verify(&mut sender_adapter, &sender_config)?;
    let mut recipient_adapter = NetworkMailAdapter::new()?;
    ImapPort::verify(&mut recipient_adapter, &recipient.mail)?;
    let mailboxes = recipient_adapter.list_mailboxes(&recipient.mail)?;
    let archive = mailboxes
        .iter()
        .find(|mailbox| is_archive_mailbox(&mailbox.path, mailbox.special_use.as_deref()))
        .map(|mailbox| mailbox.path.clone())
        .ok_or("专用验收邮箱没有可识别的归档文件夹")?;

    if submit_this_run {
        let send = sender_adapter.send(
            &sender_config,
            &OutgoingMessage {
                from: MailAddressView {
                    name: sender_config.display_name.clone(),
                    address: sender_config.email.clone(),
                },
                to: vec![recipient.mail.email.clone()],
                cc: None,
                subject: subject.clone(),
                text: format!("iMail Rust acceptance body {run_id}"),
                html: Some(format!(
                    "<p>iMail Rust acceptance body <strong>{run_id}</strong></p>"
                )),
                attachments: vec![OutgoingAttachment {
                    filename: "imail-rust-acceptance.txt".into(),
                    content_type: "text/plain".into(),
                    content: attachment.clone(),
                }],
            },
        )?;
        if !send
            .accepted
            .iter()
            .any(|address| address.eq_ignore_ascii_case(&recipient.mail.email))
        {
            return Err("SMTP 未接受闭集内验收收件人".into());
        }
    }

    let ReceivedAcceptance {
        locator,
        source,
        first_batch,
        delivery_mailbox_role,
    } = wait_for_message(
        &mut recipient_adapter,
        &recipient.mail,
        &mailboxes,
        &subject,
    )?;
    let parsed = parse_rfc822(&source)?;
    if parsed.attachments.len() != 1
        || parsed.attachments[0].filename != "imail-rust-acceptance.txt"
        || attachment_content(&source, 0)? != attachment
    {
        return Err("验收附件往返不一致".into());
    }

    recipient_adapter.update_flags(
        &recipient.mail,
        &locator,
        &RemoteMessageFlagPatch {
            unread: Some(false),
            flagged: Some(true),
        },
    )?;
    wait_for_flags(
        &mut recipient_adapter,
        &recipient.mail,
        &first_batch,
        &locator.mailbox,
        locator.uid,
        false,
        true,
    )?;
    recipient_adapter.update_flags(
        &recipient.mail,
        &locator,
        &RemoteMessageFlagPatch {
            unread: Some(false),
            flagged: Some(false),
        },
    )?;
    wait_for_flags(
        &mut recipient_adapter,
        &recipient.mail,
        &first_batch,
        &locator.mailbox,
        locator.uid,
        false,
        false,
    )?;

    let cancellation_checked = verify_real_cancellation(&recipient.mail)?;
    ImapPort::verify(&mut recipient_adapter, &recipient.mail)?;
    let moved = recipient_adapter.move_message(&recipient.mail, &locator, &archive)?;
    if !moved.confirmed {
        return Err("服务商未确认验收邮件归档".into());
    }

    let report = json!({
        "ok": true,
        "runId": run_id.to_string(),
        "sender": { "provider": provider, "authentication": authentication },
        "recipient": { "provider": recipient.provider, "authentication": recipient.authentication },
        "checks": {
            "oauthAuthorized": oauth_authorized,
            "oauthRefreshVerified": oauth_refresh_verified,
            "imapVerified": true,
            "smtpVerified": true,
            "closedRecipientSetSize": allowed_recipients.len(),
            "crossAccountDelivery": true,
            "smtpSubmittedThisRun": submit_this_run,
            "existingDeliveryRecovery": !submit_this_run,
            "deliveryMailboxRole": delivery_mailbox_role,
            "mimeParsed": true,
            "attachmentRoundTrip": true,
            "readAndFlagRoundTrip": true,
            "networkCancellation": cancellation_checked,
            "reconnectAfterCancellation": true,
            "movedToArchive": true,
            "deleted": false,
        },
        "mailboxCapabilities": {
            "mailboxCount": mailboxes.len(),
            "hasArchive": true,
        },
        "durationMs": started.elapsed().as_millis().to_string(),
    });
    let output = format!("{}\n", serde_json::to_string_pretty(&report)?);
    if let Some(path) = env::var_os("IMAIL_ACCEPTANCE_REPORT") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Err("验收报告目标已存在，拒绝覆盖".into());
        }
        fs::write(path, output.as_bytes())?;
    }
    print!("{output}");
    Ok(())
}

fn acceptance_identity() -> Result<(u128, String, bool), Box<dyn Error>> {
    const PREFIX: &str = "iMail Rust acceptance ";
    if let Ok(subject) = env::var("IMAIL_ACCEPTANCE_EXISTING_SUBJECT") {
        let run_id = subject
            .strip_prefix(PREFIX)
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or("恢复主题格式无效，拒绝跳过 SMTP")?
            .parse::<u128>()?;
        return Ok((run_id, subject, false));
    }
    let run_id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok((run_id, format!("{PREFIX}{run_id}"), true))
}

fn require_remote_write_guard() -> Result<(), Box<dyn Error>> {
    validate_remote_write_guard(
        env::var("IMAIL_ACCEPTANCE_ALLOW_REMOTE_WRITE")
            .ok()
            .as_deref(),
        env::var("IMAIL_ACCEPTANCE_DEDICATED_ACCOUNT")
            .ok()
            .as_deref(),
    )
}

fn validate_remote_write_guard(
    allow_remote_write: Option<&str>,
    dedicated_account: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    if allow_remote_write != Some("true") {
        return Err("必须显式设置 IMAIL_ACCEPTANCE_ALLOW_REMOTE_WRITE=true".into());
    }
    if dedicated_account != Some("true") {
        return Err("必须确认 IMAIL_ACCEPTANCE_DEDICATED_ACCOUNT=true，禁止使用生产邮箱".into());
    }
    Ok(())
}

struct AcceptanceConfig {
    mail: MailConnectionConfig,
    authentication: &'static str,
    provider: String,
    oauth_authorized: bool,
    oauth_refresh_verified: bool,
}

struct ReceivedAcceptance {
    locator: RemoteMessageLocator,
    source: Vec<u8>,
    first_batch: imail_mail::RemoteSyncBatch,
    delivery_mailbox_role: &'static str,
}

fn load_config() -> Result<AcceptanceConfig, Box<dyn Error>> {
    let email = required("IMAIL_ACCEPTANCE_EMAIL")?;
    let password = env::var("IMAIL_ACCEPTANCE_PASSWORD").ok();
    let access_token = env::var("IMAIL_ACCEPTANCE_ACCESS_TOKEN").ok();
    let interactive_oauth = exact_true("IMAIL_ACCEPTANCE_OAUTH_INTERACTIVE");
    let (authentication, authentication_label, provider, oauth_authorized, oauth_refresh_verified) =
        match (password, access_token, interactive_oauth) {
            (Some(password), None, false) => (
                MailAuthentication::Password(password),
                "password",
                env::var("IMAIL_ACCEPTANCE_PROVIDER").unwrap_or_else(|_| "custom".into()),
                false,
                false,
            ),
            (None, Some(access_token), false) => {
                let provider =
                    env::var("IMAIL_ACCEPTANCE_PROVIDER").unwrap_or_else(|_| "custom".into());
                (
                    MailAuthentication::OAuth {
                        provider: provider.clone(),
                        access_token,
                    },
                    "oauth2-token",
                    provider,
                    false,
                    false,
                )
            }
            (None, None, true) => {
                let (authentication, provider) = interactive_oauth_authentication(&email)?;
                (authentication, "oauth2-interactive", provider, true, true)
            }
            _ => return Err("密码、access token 与交互式 OAuth 必须且只能选择一种认证方式".into()),
        };
    Ok(AcceptanceConfig {
        mail: MailConnectionConfig {
            email,
            display_name: env::var("IMAIL_ACCEPTANCE_DISPLAY_NAME")
                .unwrap_or_else(|_| "iMail Rust Acceptance".into()),
            imap_host: required("IMAIL_ACCEPTANCE_IMAP_HOST")?,
            imap_port: optional_u16("IMAIL_ACCEPTANCE_IMAP_PORT", 993)?,
            imap_secure: optional_bool("IMAIL_ACCEPTANCE_IMAP_SECURE", true)?,
            smtp_host: required("IMAIL_ACCEPTANCE_SMTP_HOST")?,
            smtp_port: optional_u16("IMAIL_ACCEPTANCE_SMTP_PORT", 465)?,
            smtp_secure: optional_bool("IMAIL_ACCEPTANCE_SMTP_SECURE", true)?,
            authentication,
            proxy: None,
        },
        authentication: authentication_label,
        provider,
        oauth_authorized,
        oauth_refresh_verified,
    })
}

fn load_recipient_config() -> Result<AcceptanceConfig, Box<dyn Error>> {
    let email = required("IMAIL_ACCEPTANCE_RECIPIENT_EMAIL")?;
    let password = env::var("IMAIL_ACCEPTANCE_RECIPIENT_PASSWORD").ok();
    let access_token = env::var("IMAIL_ACCEPTANCE_RECIPIENT_ACCESS_TOKEN").ok();
    let (authentication, authentication_label, provider) = match (password, access_token) {
        (Some(password), None) => (
            MailAuthentication::Password(password),
            "password",
            env::var("IMAIL_ACCEPTANCE_RECIPIENT_PROVIDER").unwrap_or_else(|_| "custom".into()),
        ),
        (None, Some(access_token)) => {
            let provider =
                env::var("IMAIL_ACCEPTANCE_RECIPIENT_PROVIDER").unwrap_or_else(|_| "custom".into());
            (
                MailAuthentication::OAuth {
                    provider: provider.clone(),
                    access_token,
                },
                "oauth2-token",
                provider,
            )
        }
        _ => return Err("接收方密码与 access token 必须且只能选择一种认证方式".into()),
    };
    Ok(AcceptanceConfig {
        mail: MailConnectionConfig {
            email,
            display_name: env::var("IMAIL_ACCEPTANCE_RECIPIENT_DISPLAY_NAME")
                .unwrap_or_else(|_| "iMail Rust Acceptance Recipient".into()),
            imap_host: required("IMAIL_ACCEPTANCE_RECIPIENT_IMAP_HOST")?,
            imap_port: optional_u16("IMAIL_ACCEPTANCE_RECIPIENT_IMAP_PORT", 993)?,
            imap_secure: optional_bool("IMAIL_ACCEPTANCE_RECIPIENT_IMAP_SECURE", true)?,
            smtp_host: required("IMAIL_ACCEPTANCE_RECIPIENT_SMTP_HOST")?,
            smtp_port: optional_u16("IMAIL_ACCEPTANCE_RECIPIENT_SMTP_PORT", 465)?,
            smtp_secure: optional_bool("IMAIL_ACCEPTANCE_RECIPIENT_SMTP_SECURE", true)?,
            authentication,
            proxy: None,
        },
        authentication: authentication_label,
        provider,
        oauth_authorized: false,
        oauth_refresh_verified: false,
    })
}

fn required_allowed_recipients() -> Result<Vec<String>, Box<dyn Error>> {
    let raw = required("IMAIL_ACCEPTANCE_ALLOWED_RECIPIENTS_JSON")?;
    let values: Vec<String> = serde_json::from_str(&raw)
        .map_err(|_| "IMAIL_ACCEPTANCE_ALLOWED_RECIPIENTS_JSON 必须是字符串数组")?;
    Ok(values)
}

fn validate_closed_recipient_set(
    allowed: &[String],
    sender: &str,
    recipient: &str,
) -> Result<(), Box<dyn Error>> {
    let normalized = allowed
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if allowed.len() != 4 || normalized.len() != 4 || normalized.iter().any(String::is_empty) {
        return Err("验收收件人闭集必须恰好包含 4 个唯一邮箱".into());
    }
    let sender = sender.trim().to_ascii_lowercase();
    let recipient = recipient.trim().to_ascii_lowercase();
    if sender == recipient {
        return Err("闭集验收必须跨账户发送，拒绝自发自收".into());
    }
    if !normalized.contains(&sender) || !normalized.contains(&recipient) {
        return Err("发送方或接收方不在 4 邮箱闭集中，拒绝远程写入".into());
    }
    Ok(())
}

fn is_archive_mailbox(path: &str, special_use: Option<&str>) -> bool {
    if matches!(special_use, Some("\\Archive" | "\\All")) {
        return true;
    }
    let decoded = decode_modified_utf7(path).unwrap_or_else(|| path.to_string());
    let leaf = decoded
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(decoded.as_str())
        .trim()
        .to_ascii_lowercase();
    matches!(
        leaf.as_str(),
        "archive" | "all" | "all mail" | "all messages" | "存档" | "归档" | "所有邮件"
    )
}

fn is_junk_mailbox(path: &str, special_use: Option<&str>) -> bool {
    if matches!(special_use, Some("\\Junk")) {
        return true;
    }
    let decoded = decode_modified_utf7(path).unwrap_or_else(|| path.to_string());
    let leaf = decoded
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(decoded.as_str())
        .trim()
        .to_ascii_lowercase();
    matches!(leaf.as_str(), "junk" | "spam" | "junk mail" | "垃圾邮件")
}

fn decode_modified_utf7(value: &str) -> Option<String> {
    let mut result = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        result.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        let end = rest.find('-')?;
        let encoded = &rest[..end];
        if encoded.is_empty() {
            result.push('&');
        } else {
            let bytes = STANDARD_NO_PAD.decode(encoded.replace(',', "/")).ok()?;
            if bytes.len() % 2 != 0 {
                return None;
            }
            let utf16 = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            result.push_str(&String::from_utf16(&utf16).ok()?);
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    Some(result)
}

fn interactive_oauth_authentication(
    expected_email: &str,
) -> Result<(MailAuthentication, String), Box<dyn Error>> {
    validate_interactive_oauth_guard(exact_true("IMAIL_ACCEPTANCE_ALLOW_INTERACTIVE_OAUTH"))?;
    let provider_name = required("IMAIL_ACCEPTANCE_OAUTH_PROVIDER")?;
    let provider_key = parse_oauth_provider(&provider_name)?;
    let account_provider = env::var("IMAIL_ACCEPTANCE_ACCOUNT_PROVIDER")
        .unwrap_or_else(|_| default_account_provider(provider_key).into());
    let callback = LoopbackCallbackServer::bind(
        &env::var("IMAIL_ACCEPTANCE_OAUTH_REDIRECT_URI")
            .unwrap_or_else(|_| "http://127.0.0.1:0/oauth/callback".into()),
    )?;
    let redirect_uri = callback.redirect_uri().to_string();
    let client_id = required("IMAIL_ACCEPTANCE_OAUTH_CLIENT_ID")?;
    let client_secret = env::var("IMAIL_ACCEPTANCE_OAUTH_CLIENT_SECRET").ok();
    let environment =
        oauth_environment(provider_key, client_id, client_secret, redirect_uri.clone());
    let config = provider_config(&environment, provider_key, Some(&account_provider));
    if !config.configured {
        return Err(config.configuration_hint.into());
    }
    let master_key = MasterKey::from_hex(&MasterKey::generate_hex())?;
    let mut provider = OAuthHttpAdapter::new();
    let now_ms = unix_millis()?;
    let started = OAuthService::new(&master_key, &mut provider).begin(
        &config,
        BeginOAuthInput {
            owner_id: "acceptance-only".into(),
            account_provider,
            display_name: None,
            group: Some("验收".into()),
            color: Some("#168f78".into()),
            account_id: None,
            expected_email: Some(expected_email.into()),
            proxy: None,
        },
        now_ms,
    )?;
    open_authorization_url(&started.authorization_url)?;
    eprintln!("已打开系统浏览器，请完成专用邮箱 OAuth 授权。授权 URL 不会写入报告或终端。");
    let maximum_wait = Duration::from_secs(optional_bounded_u64(
        "IMAIL_ACCEPTANCE_OAUTH_TIMEOUT_SECONDS",
        300,
        30,
        900,
    )?);
    let callback = callback.wait(&started.state, maximum_wait)?;
    if let Some(error) = callback.error.as_deref() {
        return Err(callback_error(error, callback.error_description.as_deref()).into());
    }
    let mut service = OAuthService::new(&master_key, &mut provider);
    let completed = service.complete(
        &config,
        provider_key,
        Some(&callback.state),
        callback.code.as_deref(),
        unix_millis()?,
    )?;
    validate_oauth_identity(expected_email, &completed.identity.email)?;
    if completed.secret.refresh_token.is_none() {
        return Err("OAuth 服务商未返回 refresh token，无法完成刷新门禁".into());
    }
    let refreshed = service.refresh(&config, &completed.secret, unix_millis()?)?;
    if refreshed.refresh_token.is_none() {
        return Err("OAuth 刷新后丢失 refresh token".into());
    }
    Ok((
        MailAuthentication::OAuth {
            provider: provider_key.as_str().into(),
            access_token: refreshed.access_token,
        },
        provider_key.as_str().into(),
    ))
}

fn parse_oauth_provider(value: &str) -> Result<OAuthProviderKey, Box<dyn Error>> {
    match value.trim().to_ascii_lowercase().as_str() {
        "google" => Ok(OAuthProviderKey::Google),
        "microsoft" => Ok(OAuthProviderKey::Microsoft),
        "yahoo" => Ok(OAuthProviderKey::Yahoo),
        _ => Err("IMAIL_ACCEPTANCE_OAUTH_PROVIDER 只能是 google、microsoft 或 yahoo".into()),
    }
}

fn default_account_provider(provider: OAuthProviderKey) -> &'static str {
    match provider {
        OAuthProviderKey::Google => "gmail",
        OAuthProviderKey::Microsoft => "outlook",
        OAuthProviderKey::Yahoo => "yahoo",
    }
}

fn oauth_environment(
    provider: OAuthProviderKey,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
) -> OAuthEnvironment {
    let mut environment = OAuthEnvironment::default();
    match provider {
        OAuthProviderKey::Google => {
            environment.google_client_id = Some(client_id);
            environment.google_client_secret = client_secret;
            environment.google_redirect_uri = Some(redirect_uri);
        }
        OAuthProviderKey::Microsoft => {
            environment.microsoft_client_id = Some(client_id);
            environment.microsoft_client_secret = client_secret;
            environment.microsoft_redirect_uri = Some(redirect_uri);
        }
        OAuthProviderKey::Yahoo => {
            environment.yahoo_client_id = Some(client_id);
            environment.yahoo_client_secret = client_secret;
            environment.yahoo_redirect_uri = Some(redirect_uri);
            environment.yahoo_mail_oauth_approved =
                exact_true("IMAIL_ACCEPTANCE_YAHOO_MAIL_OAUTH_APPROVED");
        }
    }
    environment
}

fn validate_oauth_identity(expected: &str, actual: &str) -> Result<(), Box<dyn Error>> {
    if expected.trim().eq_ignore_ascii_case(actual.trim()) {
        Ok(())
    } else {
        Err("OAuth 授权邮箱与专用验收邮箱不一致，拒绝执行远程写入".into())
    }
}

fn validate_interactive_oauth_guard(allowed: bool) -> Result<(), Box<dyn Error>> {
    if allowed {
        Ok(())
    } else {
        Err("交互式 OAuth 必须额外设置 IMAIL_ACCEPTANCE_ALLOW_INTERACTIVE_OAUTH=true".into())
    }
}

fn open_authorization_url(url: &str) -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    return Err("当前平台不支持自动打开 OAuth 授权浏览器".into());

    command
        .spawn()
        .map(|_| ())
        .map_err(|_| "无法打开系统浏览器完成 OAuth 授权".into())
}

fn unix_millis() -> Result<i64, Box<dyn Error>> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}

fn exact_true(name: &str) -> bool {
    env::var(name).ok().as_deref() == Some("true")
}

fn optional_bounded_u64(
    name: &str,
    fallback: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, Box<dyn Error>> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(fallback);
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{name} 必须在 {minimum}..={maximum} 范围内").into());
    };
    Ok(value)
}

fn wait_for_message(
    adapter: &mut NetworkMailAdapter,
    config: &MailConnectionConfig,
    mailboxes: &[RemoteMailbox],
    subject: &str,
) -> Result<ReceivedAcceptance, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut targets = vec![("inbox", None)];
    targets.extend(
        mailboxes
            .iter()
            .filter(|mailbox| is_junk_mailbox(&mailbox.path, mailbox.special_use.as_deref()))
            .map(|mailbox| ("junk", Some(mailbox.path.clone()))),
    );
    loop {
        for (role, requested_mailbox) in &targets {
            let batch = adapter.fetch_incremental(
                config,
                &RemoteSyncRequest {
                    target: SyncTarget {
                        mailbox_role: (*role).into(),
                        requested_mailbox: requested_mailbox.clone(),
                    },
                    cursor: SyncCursor::default(),
                    cached_uids: vec![],
                },
            )?;
            for message in &batch.incoming {
                if parse_rfc822(&message.source)?.subject == subject {
                    return Ok(ReceivedAcceptance {
                        locator: RemoteMessageLocator {
                            mailbox: batch.mailbox.clone(),
                            uid: message.uid,
                            message_id: parse_rfc822(&message.source)?.message_id,
                        },
                        source: message.source.clone(),
                        first_batch: batch,
                        delivery_mailbox_role: role,
                    });
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("180 秒内未在收件箱或垃圾邮件箱收到唯一主题验收邮件".into());
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn wait_for_flags(
    adapter: &mut NetworkMailAdapter,
    config: &MailConnectionConfig,
    first_batch: &imail_mail::RemoteSyncBatch,
    mailbox: &str,
    uid: u32,
    unread: bool,
    flagged: bool,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let batch = adapter.fetch_incremental(
            config,
            &RemoteSyncRequest {
                target: SyncTarget {
                    mailbox_role: first_batch.mailbox_role.clone(),
                    requested_mailbox: Some(mailbox.into()),
                },
                cursor: SyncCursor {
                    uid_validity: first_batch.uid_validity.clone(),
                    last_seen_uid: first_batch.last_seen_uid,
                    highest_modseq: first_batch.highest_modseq.clone(),
                },
                cached_uids: vec![uid],
            },
        )?;
        if batch
            .flag_updates
            .iter()
            .any(|update| update.uid == uid && update.unread == unread && update.flagged == flagged)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("60 秒内未观察到已读/星标远程状态".into());
        }
        thread::sleep(Duration::from_secs(2));
    }
}

#[derive(Default)]
struct CancellationFlag(AtomicBool);

impl NetworkCancellation for CancellationFlag {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn verify_real_cancellation(config: &MailConnectionConfig) -> Result<bool, Box<dyn Error>> {
    let cancellation = Arc::new(CancellationFlag::default());
    let probe: Arc<dyn NetworkCancellation> = cancellation.clone();
    let mut adapter = NetworkMailAdapter::with_cancellation(probe)?;
    let config = config.clone();
    let handle =
        thread::spawn(move || adapter.wait_for_inbox_change(&config, Duration::from_secs(60)));
    thread::sleep(Duration::from_secs(1));
    cancellation.0.store(true, Ordering::SeqCst);
    let result = handle.join().map_err(|_| "取消验收线程异常退出")?;
    let failure = match result {
        Err(failure) => failure,
        Ok(_) => return Err("IDLE/STATUS 在取消前意外完成".into()),
    };
    if failure.status.as_deref() != Some("CANCELLED") {
        return Err("真实网络取消没有返回 CANCELLED".into());
    }
    Ok(true)
}

fn required(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("缺少环境变量 {name}").into())
}

fn optional_u16(name: &str, fallback: u16) -> Result<u16, Box<dyn Error>> {
    Ok(match env::var(name).ok() {
        Some(value) => value.parse()?,
        None => fallback,
    })
}

fn optional_bool(name: &str, fallback: bool) -> Result<bool, Box<dyn Error>> {
    match env::var(name).ok().as_deref() {
        None => Ok(fallback),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(format!("{name} 只能是 true 或 false").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_remote_side_effects_without_both_explicit_guards() {
        assert!(validate_remote_write_guard(None, None).is_err());
        assert!(validate_remote_write_guard(Some("true"), None).is_err());
        assert!(validate_remote_write_guard(None, Some("true")).is_err());
        assert!(validate_remote_write_guard(Some("TRUE"), Some("true")).is_err());
        validate_remote_write_guard(Some("true"), Some("true")).unwrap();
    }

    #[test]
    fn validates_existing_delivery_subject_before_skipping_smtp() {
        let name = "IMAIL_ACCEPTANCE_EXISTING_SUBJECT";
        std::env::set_var(name, "iMail Rust acceptance 12345");
        assert_eq!(
            acceptance_identity().unwrap(),
            (12345, "iMail Rust acceptance 12345".into(), false)
        );
        std::env::set_var(name, "unexpected subject");
        assert!(acceptance_identity().is_err());
        std::env::remove_var(name);
    }

    #[test]
    fn validates_interactive_oauth_provider_guard_and_identity_before_mail_writes() {
        assert!(validate_interactive_oauth_guard(false).is_err());
        validate_interactive_oauth_guard(true).unwrap();
        assert_eq!(
            parse_oauth_provider("GOOGLE").unwrap(),
            OAuthProviderKey::Google
        );
        assert_eq!(
            parse_oauth_provider("microsoft").unwrap(),
            OAuthProviderKey::Microsoft
        );
        assert_eq!(
            parse_oauth_provider(" yahoo ").unwrap(),
            OAuthProviderKey::Yahoo
        );
        assert!(parse_oauth_provider("custom").is_err());
        validate_oauth_identity("Dedicated@Example.com", "dedicated@example.com").unwrap();
        assert!(validate_oauth_identity("dedicated@example.com", "personal@example.com").is_err());
    }

    #[test]
    fn restricts_cross_account_delivery_to_exact_four_address_closed_set() {
        let allowed = vec![
            "one@example.test".into(),
            "two@example.test".into(),
            "three@example.test".into(),
            "four@example.test".into(),
        ];
        validate_closed_recipient_set(&allowed, "ONE@example.test", "two@example.test").unwrap();
        assert!(
            validate_closed_recipient_set(&allowed, "one@example.test", "one@example.test")
                .is_err()
        );
        assert!(validate_closed_recipient_set(
            &allowed,
            "one@example.test",
            "outside@example.test"
        )
        .is_err());
        assert!(validate_closed_recipient_set(
            &allowed[..3],
            "one@example.test",
            "two@example.test"
        )
        .is_err());
        let duplicates = vec![
            "one@example.test".into(),
            "ONE@example.test".into(),
            "three@example.test".into(),
            "four@example.test".into(),
        ];
        assert!(validate_closed_recipient_set(
            &duplicates,
            "one@example.test",
            "three@example.test"
        )
        .is_err());
    }

    #[test]
    fn recognizes_provider_archive_folders_without_special_use_metadata() {
        assert!(is_archive_mailbox("Archive", None));
        assert!(is_archive_mailbox("其他文件夹/Archive", None));
        assert!(is_archive_mailbox("存档", None));
        assert!(is_archive_mailbox("[Gmail]/所有邮件", None));
        assert!(is_archive_mailbox("anything", Some("\\All")));
        assert!(!is_archive_mailbox("Deleted Messages", Some("\\Trash")));
        assert!(!is_archive_mailbox("INBOX", Some("\\Inbox")));
        assert!(is_junk_mailbox("Junk", None));
        assert!(is_junk_mailbox("[Gmail]/垃圾邮件", None));
        assert!(is_junk_mailbox("anything", Some("\\Junk")));
        assert!(!is_junk_mailbox("Archive", Some("\\Archive")));
        assert_eq!(decode_modified_utf7("A&-B").as_deref(), Some("A&B"));
        assert_eq!(
            decode_modified_utf7("&ZeVnLIqe-").as_deref(),
            Some("日本語")
        );
    }
}
