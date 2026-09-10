use imail_mail::{mailbox_role_for, redact_protocol_detail, SyncTarget};
use imail_protocol::SyncPolicyReadModel;

use crate::AccountRecord;

const RETRY_MINUTES: [i64; 5] = [1, 5, 15, 30, 60];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedSyncFailure {
    pub code: &'static str,
    pub message: String,
    pub auth_required: bool,
}

pub fn reconciliation_interval_minutes(value: f64) -> i64 {
    if value.is_finite() {
        value.round().clamp(5.0, 1_440.0) as i64
    } else {
        30
    }
}

pub fn retry_minutes(consecutive_failures: i64) -> i64 {
    let index = consecutive_failures.saturating_sub(1) as usize;
    RETRY_MINUTES[index.min(RETRY_MINUTES.len() - 1)]
}

pub fn classify_sync_failure(detail: &str) -> ClassifiedSyncFailure {
    let message = redact_protocol_detail(detail);
    let normalized = message.to_lowercase();
    let auth_required = [
        "authenticationfailed",
        "authentication failed",
        "authentication failure",
        "authenticate failed",
        "auth failed",
        "authorization failed",
        "invalid credentials",
        "invalid password",
        "login fail",
        "username and password not accepted",
        "invalid_grant",
        "invalid_token",
        "expired_token",
        "credentials have been revoked",
        "token has been expired or revoked",
        "认证失败",
        "登录失败",
        "授权已过期",
        "凭据不可用",
        "凭据格式无效",
    ]
    .iter()
    .any(|needle| normalized.contains(needle));
    let timeout = normalized.contains("timeout")
        || normalized.contains("timed out")
        || normalized.contains("超时");
    let account_missing =
        normalized.contains("account not found") || normalized.contains("邮箱账户不存在");
    ClassifiedSyncFailure {
        code: if account_missing {
            "ACCOUNT_NOT_FOUND"
        } else if auth_required {
            "AUTH_REQUIRED"
        } else if timeout {
            "TIMEOUT"
        } else {
            "IMAP_UNAVAILABLE"
        },
        message,
        auth_required,
    }
}

pub fn targets_for_policy(
    account: &AccountRecord,
    policy: &SyncPolicyReadModel,
) -> Vec<SyncTarget> {
    let mut targets = vec![SyncTarget {
        mailbox_role: "inbox".into(),
        requested_mailbox: None,
    }];
    if policy.folder_mode == "standard" {
        targets.push(SyncTarget {
            mailbox_role: "sent".into(),
            requested_mailbox: None,
        });
        targets.push(SyncTarget {
            mailbox_role: "archive".into(),
            requested_mailbox: None,
        });
    } else if policy.folder_mode == "selected" {
        let selected = policy
            .selected_mailboxes
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str);
        let folders = account.mailboxes.as_array();
        for requested in selected {
            let Some(folder) = folders.and_then(|items| {
                items.iter().find(|item| {
                    item.get("path").and_then(serde_json::Value::as_str) == Some(requested)
                        && item
                            .get("selectable")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false)
                })
            }) else {
                continue;
            };
            let role = mailbox_role_for(
                requested,
                folder.get("specialUse").and_then(serde_json::Value::as_str),
            );
            let target = SyncTarget {
                requested_mailbox: (role == "custom").then(|| requested.to_string()),
                mailbox_role: role,
            };
            if !targets
                .iter()
                .any(|existing| target_key(existing) == target_key(&target))
            {
                targets.push(target);
            }
        }
    }
    targets
}

pub fn target_key(target: &SyncTarget) -> String {
    target
        .requested_mailbox
        .clone()
        .unwrap_or_else(|| format!("@role:{}", target.mailbox_role))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn account() -> AccountRecord {
        AccountRecord {
            id: "account-1".into(),
            owner_id: "user-1".into(),
            provider: "custom".into(),
            email: "owner@example.com".into(),
            display_name: "Owner".into(),
            group: "personal".into(),
            group_icon: "folder".into(),
            color: "#000".into(),
            settings: json!({}),
            proxy: None,
            encrypted_secret: "cipher".into(),
            auth_method: None,
            created_at: "2026-08-10T00:00:00.000Z".into(),
            last_sync_at: None,
            status: "connected".into(),
            last_error: None,
            mailboxes: json!([
                {"path":"Projects","selectable":true},
                {"path":"Sent","specialUse":"\\Sent","selectable":true},
                {"path":"Missing","selectable":false}
            ]),
        }
    }

    fn policy(mode: &str, selected: serde_json::Value) -> SyncPolicyReadModel {
        SyncPolicyReadModel {
            account_id: "account-1".into(),
            enabled: true,
            folder_mode: mode.into(),
            selected_mailboxes: selected,
            notify_on_error: true,
            updated_at: "2026-08-10T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn bounds_reconciliation_and_retry_schedule() {
        assert_eq!(reconciliation_interval_minutes(1.0), 5);
        assert_eq!(reconciliation_interval_minutes(47.6), 48);
        assert_eq!(reconciliation_interval_minutes(2_000.0), 1_440);
        assert_eq!(reconciliation_interval_minutes(f64::NAN), 30);
        assert_eq!(
            (1..=7).map(retry_minutes).collect::<Vec<_>>(),
            [1, 5, 15, 30, 60, 60, 60]
        );
    }

    #[test]
    fn expands_standard_and_selected_policy_targets() {
        assert_eq!(
            targets_for_policy(&account(), &policy("standard", json!([])))
                .iter()
                .map(target_key)
                .collect::<Vec<_>>(),
            ["@role:inbox", "@role:sent", "@role:archive"]
        );
        assert_eq!(
            targets_for_policy(
                &account(),
                &policy("selected", json!(["Projects", "Sent", "Missing"]))
            )
            .iter()
            .map(target_key)
            .collect::<Vec<_>>(),
            ["@role:inbox", "Projects", "@role:sent"]
        );
    }

    #[test]
    fn classifies_and_redacts_worker_failures() {
        let auth = classify_sync_failure("AUTH failed authorization=Bearer-super-secret");
        assert_eq!(auth.code, "AUTH_REQUIRED");
        assert!(auth.auth_required);
        assert!(!auth.message.contains("super-secret"));
        assert_eq!(classify_sync_failure("socket timeout").code, "TIMEOUT");
        assert_eq!(
            classify_sync_failure("邮箱账户不存在").code,
            "ACCOUNT_NOT_FOUND"
        );
        assert_eq!(
            classify_sync_failure("connection reset").code,
            "IMAP_UNAVAILABLE"
        );
    }

    #[test]
    fn oauth_transport_errors_do_not_require_reauthorization() {
        for detail in [
            "OAuth 网络请求失败：OAuth HTTP 请求失败：connection reset",
            "OAuth Token 交换失败 (503)",
            "OAuth Token 响应不是有效 JSON",
        ] {
            let failure = classify_sync_failure(detail);
            assert!(!failure.auth_required, "{detail}");
            assert_eq!(failure.code, "IMAP_UNAVAILABLE");
        }
        assert_eq!(classify_sync_failure("OAuth HTTP timeout").code, "TIMEOUT");
        for detail in [
            "OAuth 网络请求失败：invalid_grant: grant rejected",
            "IMAP 验证失败：[AUTHENTICATIONFAILED] Invalid credentials",
            "IMAP 验证失败：AUTHENTICATE failed.",
            "IMAP 验证失败：Login fail. Please check your account",
            "OAuth 授权已过期且没有刷新 Token，请重新连接邮箱",
        ] {
            assert!(classify_sync_failure(detail).auth_required, "{detail}");
        }
    }
}
