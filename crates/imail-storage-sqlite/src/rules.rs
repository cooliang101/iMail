//! Owner-scoped rule storage and transactional local actions / durable remote outbox.
use chrono::{Duration, Utc};
use imail_core::{
    rules::{MailRule, MailRuleInput, RuleAction, RuleRun},
    AccountRepository, ContentRepository,
};
use imail_protocol::MessageReadModel;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{AuthStoreError, SqliteAuthStore};

pub fn message_identity(message: &MessageReadModel) -> String {
    // Account scope is stored separately. RFC Message-ID survives IMAP moves and UID resets.
    match message
        .message_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        Some(id) => format!("rfc:{id}"),
        None => format!("local:{}", message.id),
    }
}

pub(crate) fn load_rules(
    connection: &Connection,
    owner: &str,
) -> Result<Vec<MailRule>, AuthStoreError> {
    let mut query = connection.prepare(
        "SELECT id,revision,input_json,created_at,updated_at FROM mail_rules WHERE user_id=?1",
    )?;
    let rows = query
        .query_map([owner], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut rules = rows
        .into_iter()
        .map(|(id, revision, input, created_at, updated_at)| {
            Ok(MailRule {
                id,
                revision,
                input: serde_json::from_str(&input)?,
                created_at,
                updated_at,
            })
        })
        .collect::<Result<Vec<_>, AuthStoreError>>()?;
    rules.sort_by(|a, b| {
        (a.input.priority, &a.created_at, &a.id).cmp(&(b.input.priority, &b.created_at, &b.id))
    });
    Ok(rules)
}

/// Called inside the sync transaction, so an imported message cannot lose its rule work.
pub(crate) fn enqueue_matches(
    connection: &Connection,
    owner: &str,
    rules: &[MailRule],
    message: &mut MessageReadModel,
    source: &str,
    now: &str,
) -> Result<usize, AuthStoreError> {
    let matched = imail_core::rules::matching_rules(rules, message);
    let identity = message_identity(message);
    let mut count = 0;
    for rule in matched {
        let remote = rule
            .input
            .actions
            .iter()
            .filter(|a| a.is_remote())
            .cloned()
            .collect::<Vec<_>>();
        let local_count = rule.input.actions.len() - remote.len();
        let inserted = connection.execute("INSERT OR IGNORE INTO mail_rule_runs(id,user_id,rule_id,rule_name,revision,account_id,message_id,identity,source,status,actions_json,local_actions,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)", params![uuid::Uuid::new_v4().to_string(),owner,rule.id,rule.input.name,rule.revision,message.account_id,message.id,identity,source,if remote.is_empty() {"succeeded"} else {"pending"},serde_json::to_string(&remote)?,local_count,now])?;
        if inserted == 0 {
            continue;
        }
        count += 1;
        let mut labels = message.labels.as_array().cloned().unwrap_or_default();
        for action in &rule.input.actions {
            match action {
                RuleAction::AddLabel(label) => {
                    if !labels.iter().any(|v| v.as_str() == Some(label)) {
                        labels.push(json!(label));
                    }
                }
                RuleAction::RemoveLabel(label) => labels.retain(|v| v.as_str() != Some(label)),
                RuleAction::Mute(muted) => {
                    connection.execute("INSERT INTO mail_rule_message_state(account_id,identity,muted) VALUES(?1,?2,?3) ON CONFLICT(account_id,identity) DO UPDATE SET muted=excluded.muted", params![message.account_id,identity,muted])?;
                }
                _ => {}
            }
        }
        message.labels = json!(labels);
    }
    Ok(count)
}

pub(crate) fn is_muted(
    connection: &Connection,
    message: &MessageReadModel,
) -> Result<bool, rusqlite::Error> {
    Ok(connection
        .query_row(
            "SELECT muted FROM mail_rule_message_state WHERE account_id=?1 AND identity=?2",
            params![message.account_id, message_identity(message)],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulePreview {
    pub token: Option<String>,
    pub total: usize,
    pub eligible: usize,
    pub messages: Vec<Value>,
    pub expires_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct PreviewMessage {
    id: String,
    fingerprint: String,
}

fn fingerprint(message: &MessageReadModel) -> Result<String, AuthStoreError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(message)?)
    ))
}

impl SqliteAuthStore {
    pub fn list_mail_rules(&self, owner: &str) -> Result<Vec<MailRule>, AuthStoreError> {
        load_rules(&self.connection, owner)
    }

    pub fn save_mail_rule(
        &mut self,
        owner: &str,
        id: Option<&str>,
        input: &MailRuleInput,
    ) -> Result<Option<MailRule>, AuthStoreError> {
        input
            .validate()
            .map_err(|_| AuthStoreError::InvalidContentData)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for account in &input.account_ids {
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND user_id=?2)",
                params![account, owner],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(AuthStoreError::AccountNotOwned);
            }
        }
        let existing = load_rules(&tx, owner)?;
        let previous = id.and_then(|id| existing.iter().find(|r| r.id == id));
        if id.is_some() && previous.is_none() {
            return Ok(None);
        }
        if previous.is_none() && existing.len() >= 100 {
            return Err(AuthStoreError::InvalidContentData);
        }
        let now = Utc::now().to_rfc3339();
        let rule = MailRule {
            id: id
                .map(str::to_owned)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            revision: previous.map_or(1, |r| r.revision + 1),
            input: input.clone(),
            created_at: previous.map_or_else(|| now.clone(), |r| r.created_at.clone()),
            updated_at: now.clone(),
        };
        tx.execute("INSERT INTO mail_rules(id,user_id,revision,input_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,input_json=excluded.input_json,updated_at=excluded.updated_at", params![rule.id,owner,rule.revision,serde_json::to_string(input)?,rule.created_at,now])?;
        // Editing/disabling cancels not-yet-started old work. Running remote commands may finish.
        tx.execute("UPDATE mail_rule_runs SET status='cancelled',updated_at=?1 WHERE user_id=?2 AND rule_id=?3 AND status IN ('pending','failed')", params![now,owner,rule.id])?;
        tx.execute(
            "DELETE FROM mail_rule_previews WHERE user_id=?1 AND rule_id=?2",
            params![owner, rule.id],
        )?;
        tx.commit()?;
        Ok(Some(rule))
    }

    pub fn delete_mail_rule(&mut self, owner: &str, id: &str) -> Result<bool, AuthStoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute(
            "DELETE FROM mail_rules WHERE user_id=?1 AND id=?2",
            params![owner, id],
        )? > 0;
        tx.execute("UPDATE mail_rule_runs SET status='cancelled',updated_at=?1 WHERE user_id=?2 AND rule_id=?3 AND status IN ('pending','failed')", params![Utc::now().to_rfc3339(),owner,id])?;
        tx.execute(
            "DELETE FROM mail_rule_previews WHERE user_id=?1 AND rule_id=?2",
            params![owner, id],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn preview_mail_rule(
        &mut self,
        owner: &str,
        input: &MailRuleInput,
        saved_id: Option<&str>,
    ) -> Result<RulePreview, AuthStoreError> {
        input
            .validate()
            .map_err(|_| AuthStoreError::InvalidContentData)?;
        for account in &input.account_ids {
            if self.account(owner, account)?.is_none() {
                return Err(AuthStoreError::AccountNotOwned);
            }
        }
        let saved = saved_id
            .map(|id| {
                self.list_mail_rules(owner)
                    .map(|rules| rules.into_iter().find(|r| r.id == id))
            })
            .transpose()?
            .flatten();
        if saved_id.is_some() && saved.as_ref().map_or(true, |r| r.input != *input) {
            return Err(AuthStoreError::InvalidContentData);
        }
        let matches = self
            .list_messages(owner)?
            .into_iter()
            .filter(|m| input.matches(m))
            .collect::<Vec<_>>();
        let total = matches.len();
        let mut eligible = Vec::new();
        for m in matches {
            let already = if let Some(rule) = &saved {
                self.connection.query_row("SELECT EXISTS(SELECT 1 FROM mail_rule_runs WHERE user_id=?1 AND rule_id=?2 AND revision=?3 AND account_id=?4 AND identity=?5)",params![owner,rule.id,rule.revision,m.account_id,message_identity(&m)],|r|r.get::<_,bool>(0))?
            } else {
                false
            };
            if !already {
                eligible.push(m);
            }
        }
        let mut token = None;
        let mut expires_at = None;
        if let Some(saved) = saved {
            let id = uuid::Uuid::new_v4().to_string();
            let expiry = (Utc::now() + Duration::minutes(10)).to_rfc3339();
            let snapshot = eligible
                .iter()
                .map(|m| {
                    Ok(PreviewMessage {
                        id: m.id.clone(),
                        fingerprint: fingerprint(m)?,
                    })
                })
                .collect::<Result<Vec<_>, AuthStoreError>>()?;
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute(
                "DELETE FROM mail_rule_previews WHERE expires_at<?1 OR user_id=?2",
                params![Utc::now().to_rfc3339(), owner],
            )?;
            tx.execute("INSERT INTO mail_rule_previews(id,user_id,rule_id,revision,messages_json,expires_at) VALUES(?1,?2,?3,?4,?5,?6)",params![id,owner,saved.id,saved.revision,serde_json::to_string(&snapshot)?,expiry])?;
            tx.commit()?;
            token = Some(id);
            expires_at = Some(expiry);
        }
        Ok(RulePreview { token,total,eligible:eligible.len(),messages:eligible.iter().take(50).map(|m|json!({"id":m.id,"accountId":m.account_id,"subject":m.subject,"from":m.from,"date":m.date})).collect(),expires_at })
    }

    /// Apply exactly the confirmed preview, rejecting changes instead of broadening its scope.
    pub fn apply_mail_rule_preview(
        &mut self,
        owner: &str,
        token: &str,
    ) -> Result<usize, AuthStoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let preview: Option<(String,u32,String)> = tx.query_row("SELECT rule_id,revision,messages_json FROM mail_rule_previews WHERE id=?1 AND user_id=?2 AND expires_at>?3",params![token,owner,Utc::now().to_rfc3339()],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (id, revision, raw) = preview.ok_or(AuthStoreError::InvalidContentData)?;
        let mut rule = load_rules(&tx, owner)?
            .into_iter()
            .find(|r| r.id == id && r.revision == revision)
            .ok_or(AuthStoreError::InvalidContentData)?;
        // Disabled rules can be explicitly run once, without enabling automatic processing.
        rule.input.enabled = true;
        let snapshot: Vec<PreviewMessage> = serde_json::from_str(&raw)?;
        let now = Utc::now().to_rfc3339();
        let mut count = 0;
        for expected in snapshot {
            let mut message = crate::content_data::rule_message(&tx, owner, &expected.id)?
                .ok_or(AuthStoreError::InvalidContentData)?;
            if fingerprint(&message)? != expected.fingerprint {
                return Err(AuthStoreError::InvalidContentData);
            }
            count += enqueue_matches(
                &tx,
                owner,
                std::slice::from_ref(&rule),
                &mut message,
                "manual",
                &now,
            )?;
            tx.execute(
                "UPDATE messages SET labels_json=?1 WHERE id=?2",
                params![serde_json::to_string(&message.labels)?, message.id],
            )?;
            // Local-only rules must not contact the mailbox server.
            if rule.input.actions.iter().any(RuleAction::is_remote) {
                tx.execute("INSERT OR IGNORE INTO sync_jobs(id,account_id,mailbox,mailbox_role,reason,status,priority,not_before,attempts,created_at) VALUES(?1,?2,NULL,'inbox','manual','queued',10,?3,0,?3)",params![uuid::Uuid::new_v4().to_string(),message.account_id,now])?;
                tx.execute(
                    "UPDATE sync_jobs SET rerun_requested=1 WHERE account_id=?1 AND status='running'",
                    [&message.account_id],
                )?;
            }
        }
        tx.execute("DELETE FROM mail_rule_previews WHERE id=?1", [token])?;
        tx.commit()?;
        Ok(count)
    }

    pub fn mail_rule_runs(&self, owner: &str) -> Result<Vec<RuleRun>, AuthStoreError> {
        let mut query = self.connection.prepare("SELECT id,rule_id,rule_name,revision,account_id,message_id,source,status,completed_actions,local_actions,json_array_length(actions_json),error_code,created_at,updated_at FROM mail_rule_runs WHERE user_id=?1 ORDER BY sequence DESC LIMIT 200")?;
        let rows = query
            .query_map([owner], |r| {
                Ok(RuleRun {
                    id: r.get(0)?,
                    rule_id: r.get(1)?,
                    rule_name: r.get(2)?,
                    revision: r.get(3)?,
                    account_id: r.get(4)?,
                    message_id: r.get(5)?,
                    source: r.get(6)?,
                    status: r.get(7)?,
                    completed_actions: r.get::<_, usize>(8)? + r.get::<_, usize>(9)?,
                    total_actions: r.get::<_, usize>(9)? + r.get::<_, usize>(10)?,
                    error_code: r.get(11)?,
                    created_at: r.get(12)?,
                    updated_at: r.get(13)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn rule_message_muted(
        &self,
        owner: &str,
        message: &MessageReadModel,
    ) -> Result<bool, AuthStoreError> {
        if self.account(owner, &message.account_id)?.is_none() {
            return Err(AuthStoreError::AccountNotOwned);
        }
        Ok(is_muted(&self.connection, message)?)
    }
}

#[derive(Debug)]
pub struct RuleWork {
    pub id: String,
    pub message: MessageReadModel,
    pub action: RuleAction,
    pub index: usize,
    lease: String,
}

impl SqliteAuthStore {
    /// One remote action per lease. Earlier unfinished runs block conflicting work
    /// on the same message, while unrelated messages continue.
    pub fn has_pending_rule_actions(
        &self,
        owner: &str,
        account: &str,
    ) -> Result<bool, AuthStoreError> {
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM mail_rule_runs WHERE user_id=?1 AND account_id=?2 AND status IN ('pending','running'))",params![owner,account],|r|r.get(0))?)
    }

    pub fn claim_rule_action(
        &mut self,
        owner: &str,
        account: &str,
    ) -> Result<Option<RuleWork>, AuthStoreError> {
        if self.account(owner, account)?.is_none() {
            return Err(AuthStoreError::AccountNotOwned);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now().to_rfc3339();
        tx.execute("UPDATE mail_rule_runs SET status=CASE WHEN json_extract(actions_json,'$[' || completed_actions || '].type')='archive' THEN 'needsReview' ELSE 'pending' END,error_code='INTERRUPTED',updated_at=?1 WHERE user_id=?2 AND account_id=?3 AND status='running' AND lease_until<?1",params![now,owner,account])?;
        // A crashed command can outlive edits/deletion of its rule. Never revive
        // the old revision when recovering its lease; uncertain moves stay review-only.
        tx.execute("UPDATE mail_rule_runs SET status='cancelled',lease_until=NULL,updated_at=?1 WHERE user_id=?2 AND account_id=?3 AND status='pending' AND NOT EXISTS(SELECT 1 FROM mail_rules rule WHERE rule.id=mail_rule_runs.rule_id AND rule.user_id=mail_rule_runs.user_id AND rule.revision=mail_rule_runs.revision)",params![now,owner,account])?;
        let row: Option<(String,String,String,usize)> = tx.query_row("SELECT r.id,r.message_id,r.actions_json,r.completed_actions FROM mail_rule_runs r WHERE r.user_id=?1 AND r.account_id=?2 AND r.status='pending' AND (r.lease_until IS NULL OR r.lease_until<=?3) AND NOT EXISTS(SELECT 1 FROM mail_rule_runs earlier WHERE earlier.account_id=r.account_id AND earlier.identity=r.identity AND earlier.sequence<r.sequence AND earlier.status IN ('pending','running','failed','needsReview')) ORDER BY r.sequence LIMIT 1",params![owner,account,now], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((id, message_id, raw, index)) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let actions: Vec<RuleAction> = serde_json::from_str(&raw)?;
        let action = actions
            .get(index)
            .cloned()
            .ok_or(AuthStoreError::InvalidContentData)?;
        let Some(message) = crate::content_data::rule_message(&tx, owner, &message_id)? else {
            tx.execute("UPDATE mail_rule_runs SET status='failed',error_code='MESSAGE_NOT_FOUND',updated_at=?1 WHERE id=?2",params![now,id])?;
            tx.commit()?;
            return Ok(None);
        };
        let lease = (Utc::now() + Duration::minutes(10)).to_rfc3339();
        tx.execute("UPDATE mail_rule_runs SET status='running',lease_until=?1,attempts=attempts+1,updated_at=?2 WHERE id=?3",params![lease,now,id])?;
        tx.commit()?;
        Ok(Some(RuleWork {
            id,
            message,
            action,
            index,
            lease,
        }))
    }

    /// Persist only the action's changed columns, never overwrite concurrent labels/body edits.
    pub fn finish_rule_action(
        &mut self,
        owner: &str,
        work: &RuleWork,
        result: Result<Option<imail_protocol::RemoteMessageMoveResult>, &'static str>,
    ) -> Result<(), AuthStoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let valid = tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_rule_runs WHERE id=?1 AND user_id=?2 AND status='running' AND completed_actions=?3 AND lease_until=?4)",params![work.id,owner,work.index,work.lease],|r|r.get::<_,bool>(0))?;
        if !valid {
            return Err(AuthStoreError::InvalidContentData);
        }
        let now = Utc::now().to_rfc3339();
        match result {
            Ok(moved) => {
                match &work.action {
                    RuleAction::MarkRead(read) => {
                        tx.execute(
                            "UPDATE messages SET unread=?1 WHERE id=?2 AND account_id=?3",
                            params![!read, work.message.id, work.message.account_id],
                        )?;
                    }
                    RuleAction::Flag(flag) => {
                        tx.execute(
                            "UPDATE messages SET flagged=?1 WHERE id=?2 AND account_id=?3",
                            params![flag, work.message.id, work.message.account_id],
                        )?;
                    }
                    RuleAction::Archive => {
                        let moved = moved.ok_or(AuthStoreError::InvalidContentData)?;
                        // UID-less move confirmations cannot safely address later commands.
                        let uid = moved.uid.ok_or(AuthStoreError::InvalidContentData)?;
                        tx.execute("UPDATE messages SET mailbox=?1,mailbox_role='archive',uid=?2,snoozed_until=NULL WHERE id=?3 AND account_id=?4",params![moved.mailbox,uid,work.message.id,work.message.account_id])?;
                    }
                    _ => return Err(AuthStoreError::InvalidContentData),
                }
                tx.execute("UPDATE mail_rule_runs SET completed_actions=completed_actions+1,status=CASE WHEN completed_actions+1=json_array_length(actions_json) THEN 'succeeded' WHEN NOT EXISTS(SELECT 1 FROM mail_rules rule WHERE rule.id=mail_rule_runs.rule_id AND rule.user_id=mail_rule_runs.user_id AND rule.revision=mail_rule_runs.revision) THEN 'cancelled' ELSE 'pending' END,attempts=0,error_code=NULL,lease_until=NULL,updated_at=?1 WHERE id=?2",params![now,work.id])?;
            }
            Err(code) => {
                // Never persist provider responses, credentials, or arbitrary exception strings.
                let code = match code {
                    "MESSAGE_NOT_FOUND" | "MOVE_UNCONFIRMED" | "CANCELLED" => code,
                    _ => "REMOTE_ACTION_FAILED",
                };
                let uncertain = matches!(work.action, RuleAction::Archive);
                tx.execute("UPDATE mail_rule_runs SET status=CASE WHEN ?1 THEN 'needsReview' WHEN NOT EXISTS(SELECT 1 FROM mail_rules rule WHERE rule.id=mail_rule_runs.rule_id AND rule.user_id=mail_rule_runs.user_id AND rule.revision=mail_rule_runs.revision) THEN 'cancelled' WHEN attempts>=3 THEN 'failed' ELSE 'pending' END,error_code=?2,lease_until=?3,updated_at=?4 WHERE id=?5",params![uncertain,code,(Utc::now()+Duration::minutes(1)).to_rfc3339(),now,work.id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn retry_mail_rule_run(&mut self, owner: &str, id: &str) -> Result<bool, AuthStoreError> {
        // Uncertain archive results require mailbox inspection, not blind replay.
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account: Option<String> = tx.query_row("SELECT r.account_id FROM mail_rule_runs r JOIN mail_rules rule ON rule.id=r.rule_id AND rule.user_id=r.user_id AND rule.revision=r.revision WHERE r.id=?1 AND r.user_id=?2 AND r.status='failed'",params![id,owner],|r|r.get(0)).optional()?;
        let Some(account) = account else {
            return Ok(false);
        };
        let now = Utc::now().to_rfc3339();
        tx.execute("UPDATE mail_rule_runs SET status='pending',attempts=0,error_code=NULL,lease_until=NULL,updated_at=?1 WHERE id=?2",params![now,id])?;
        tx.execute("INSERT OR IGNORE INTO sync_jobs(id,account_id,mailbox,mailbox_role,reason,status,priority,not_before,attempts,created_at) VALUES(?1,?2,NULL,'inbox','manual','queued',10,?3,0,?3)",params![uuid::Uuid::new_v4().to_string(),account,now])?;
        tx.execute(
            "UPDATE sync_jobs SET rerun_requested=1 WHERE account_id=?1 AND status='running'",
            [&account],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
