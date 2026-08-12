use std::collections::HashSet;

use icu_collator::{Collator, CollatorOptions};
use icu_locid::locale;
use imail_protocol::{MessageReadModel, NotificationKind, NotificationView};

use crate::{AccountRecord, ApplicationError, LocalRepository};

pub struct MailOverviewService<'a, R: LocalRepository> {
    repository: &'a R,
}

impl<'a, R: LocalRepository> MailOverviewService<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub fn labels(
        &self,
        user_id: &str,
    ) -> Result<Vec<String>, ApplicationError<<R as crate::AccountRepository>::Error>> {
        let messages = self
            .repository
            .list_messages(user_id)
            .map_err(ApplicationError::Repository)?;
        Ok(list_labels(&messages))
    }

    pub fn notifications(
        &self,
        user_id: &str,
        limit: usize,
        now: &str,
    ) -> Result<Vec<NotificationView>, ApplicationError<<R as crate::AccountRepository>::Error>>
    {
        let accounts = self
            .repository
            .list_accounts(user_id)
            .map_err(ApplicationError::Repository)?;
        let messages = self
            .repository
            .list_messages(user_id)
            .map_err(ApplicationError::Repository)?;
        Ok(build_notifications(&accounts, &messages, limit, now))
    }
}

pub fn list_labels(messages: &[MessageReadModel]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut labels = messages
        .iter()
        .flat_map(|message| message.labels.as_array().into_iter().flatten())
        .filter_map(|label| label.as_str().map(str::to_string))
        .filter(|label| seen.insert(label.clone()))
        .collect::<Vec<_>>();
    let locale = locale!("zh-CN").into();
    if let Ok(collator) = Collator::try_new(&locale, CollatorOptions::new()) {
        labels.sort_by(|left, right| collator.compare(left, right));
    } else {
        labels.sort();
    }
    labels
}

pub fn build_notifications(
    accounts: &[AccountRecord],
    messages: &[MessageReadModel],
    limit: usize,
    now: &str,
) -> Vec<NotificationView> {
    let mut notifications = Vec::new();
    for account in accounts.iter().filter(|account| account.status == "error") {
        notifications.push(NotificationView {
            id: format!("account-{}", account.id),
            kind: NotificationKind::Error,
            title: format!("{} 连接异常", account.display_name),
            detail: account
                .last_error
                .clone()
                .unwrap_or_else(|| account.email.clone()),
            date: account
                .last_sync_at
                .clone()
                .unwrap_or_else(|| account.created_at.clone()),
            message_id: None,
            account_id: account.id.clone(),
        });
    }
    for message in messages.iter().filter(|message| {
        message
            .snoozed_until
            .as_deref()
            .is_some_and(|snoozed| snoozed <= now)
    }) {
        notifications.push(NotificationView {
            id: format!("snooze-{}", message.id),
            kind: NotificationKind::Snooze,
            title: message.subject.clone(),
            detail: "稍后处理的邮件已返回收件箱".into(),
            date: message.snoozed_until.clone().unwrap_or_default(),
            message_id: Some(message.id.clone()),
            account_id: message.account_id.clone(),
        });
    }
    for message in messages.iter().filter(|message| {
        message.mailbox_role == "inbox"
            && message.unread
            && message
                .snoozed_until
                .as_deref()
                .map_or(true, |snoozed| snoozed <= now)
    }) {
        notifications.push(NotificationView {
            id: format!("unread-{}", message.id),
            kind: NotificationKind::Unread,
            title: message.subject.clone(),
            detail: sender_detail(message),
            date: message.date.clone(),
            message_id: Some(message.id.clone()),
            account_id: message.account_id.clone(),
        });
    }
    notifications.sort_by(|left, right| right.date.cmp(&left.date));
    notifications.truncate(limit);
    notifications
}

fn sender_detail(message: &MessageReadModel) -> String {
    let name = message
        .from
        .get("name")
        .and_then(|field| field.as_str())
        .unwrap_or_default();
    if name.is_empty() {
        message
            .from
            .get("address")
            .and_then(|field| field.as_str())
            .unwrap_or_default()
            .to_string()
    } else {
        name.to_string()
    }
}
