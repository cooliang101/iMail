use crate::{AuthStoreError, SqliteAuthStore};
use imail_core::search::{SearchFilters, SmartFolder, SmartFolderInput};
use rusqlite::{params, types::Value, OptionalExtension, TransactionBehavior};

pub(crate) fn append_filters(
    filters: &SearchFilters,
    clauses: &mut Vec<String>,
    values: &mut Vec<Value>,
    now: &str,
) -> Result<(), AuthStoreError> {
    filters
        .validate()
        .map_err(|_| AuthStoreError::InvalidContentData)?;
    if !filters.account_ids.is_empty() {
        clauses.push(format!(
            "m.account_id IN ({})",
            vec!["?"; filters.account_ids.len()].join(",")
        ));
        values.extend(filters.account_ids.iter().cloned().map(Value::Text));
    }
    for (column, value) in [
        ("a.group_name", &filters.group),
        ("m.mailbox_role", &filters.mailbox_role),
        ("m.mailbox", &filters.mailbox),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{column} = ?"));
            values.push(Value::Text(value.clone()));
        }
    }
    if let Some(name) = &filters.mailbox_name {
        clauses.push("EXISTS (SELECT 1 FROM json_each(a.mailboxes_json) f WHERE lower(json_extract(f.value,'$.name'))=lower(?) AND json_extract(f.value,'$.path')=m.mailbox)".into());
        values.push(Value::Text(name.clone()));
    }
    for (column, value) in [
        ("m.unread", filters.unread),
        ("m.flagged", filters.flagged),
        ("m.has_attachments", filters.has_attachments),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{column} = ?"));
            values.push(Value::Integer(i64::from(value)));
        }
    }
    for (operator, value) in [(">=", &filters.since), ("<", &filters.before)] {
        if let Some(value) = value {
            clauses.push(format!("julianday(m.received_at) {operator} julianday(?)"));
            values.push(Value::Text(value.clone()));
        }
    }
    if let Some(value) = filters.snoozed {
        clauses.push(
            if value {
                "m.snoozed_until IS NOT NULL AND m.snoozed_until > ?"
            } else {
                "(m.snoozed_until IS NULL OR m.snoozed_until <= ?)"
            }
            .into(),
        );
        values.push(Value::Text(now.into()));
    }
    for label in &filters.labels {
        clauses.push("EXISTS (SELECT 1 FROM json_each(m.labels_json) WHERE value=?)".into());
        values.push(Value::Text(label.clone()));
    }
    if let Some(sender) = &filters.sender {
        clauses.push("lower(CASE json_type(m.from_json) WHEN 'object' THEN json_extract(m.from_json,'$.address') WHEN 'text' THEN json_extract(m.from_json,'$') ELSE '' END)=lower(?)".into());
        values.push(Value::Text(sender.trim().into()));
    }
    if let Some(recipient) = &filters.recipient {
        clauses.push("EXISTS (SELECT 1 FROM (SELECT value,type FROM json_each(m.to_json) UNION ALL SELECT value,type FROM json_each(m.mail_headers_json,'$.cc')) r WHERE lower(CASE r.type WHEN 'object' THEN json_extract(r.value,'$.address') WHEN 'text' THEN r.value ELSE '' END)=lower(?))".into());
        values.push(Value::Text(recipient.trim().into()));
    }
    if let Some(q) = &filters.q {
        clauses.push("(instr(lower(m.subject),lower(?))>0 OR instr(lower(m.preview),lower(?))>0 OR instr(lower(m.from_json),lower(?))>0 OR instr(lower(m.to_json),lower(?))>0 OR instr(lower(json_extract(m.mail_headers_json,'$.cc')),lower(?))>0)".into());
        values.extend(std::iter::repeat(Value::Text(q.trim().into())).take(5));
    }
    if let Some(subject) = &filters.subject {
        clauses.push("instr(lower(m.subject),lower(?))>0".into());
        values.push(Value::Text(subject.trim().into()));
    }
    if let Some(body) = &filters.body {
        let body = body.trim();
        // Quoted FTS phrase treats operators and quotes as literal text. The final
        // substring predicate gives identical semantics for 1/2-character queries.
        if body.chars().count() >= 3 {
            clauses.push(
                "m.rowid IN (SELECT rowid FROM message_body_fts WHERE message_body_fts MATCH ?)"
                    .into(),
            );
            values.push(Value::Text(format!("\"{}\"", body.replace('"', "\"\""))));
        }
        clauses.push("instr(lower(m.text_body),lower(?))>0".into());
        values.push(Value::Text(body.into()));
    }
    Ok(())
}

impl SqliteAuthStore {
    pub fn list_smart_folders(&self, owner: &str) -> Result<Vec<SmartFolder>, AuthStoreError> {
        let mut statement = self.connection.prepare("SELECT id,name,filters_json,created_at,updated_at FROM smart_folders WHERE user_id=?1 ORDER BY created_at,id")?;
        let rows = statement
            .query_map([owner], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, name, filters, created_at, updated_at)| {
                Ok(SmartFolder {
                    id,
                    name,
                    filters: serde_json::from_str(&filters)?,
                    created_at,
                    updated_at,
                })
            })
            .collect()
    }

    /// Create or replace one folder atomically; never replace another device's list.
    pub fn save_smart_folder(
        &mut self,
        owner: &str,
        id: Option<&str>,
        input: &SmartFolderInput,
    ) -> Result<Option<SmartFolder>, AuthStoreError> {
        input
            .validate()
            .map_err(|_| AuthStoreError::InvalidContentData)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let user_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_users WHERE id=?1)",
            [owner],
            |r| r.get(0),
        )?;
        if !user_exists {
            return Err(AuthStoreError::UserNotFound);
        }
        for account in &input.filters.account_ids {
            let owned: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND user_id=?2)",
                (account, owner),
                |r| r.get(0),
            )?;
            if !owned {
                return Err(AuthStoreError::AccountNotOwned);
            }
        }
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let (id, created_at) = if let Some(id) = id {
            let created: Option<String> = transaction
                .query_row(
                    "SELECT created_at FROM smart_folders WHERE user_id=?1 AND id=?2",
                    (owner, id),
                    |r| r.get(0),
                )
                .optional()?;
            let Some(created) = created else {
                return Ok(None);
            };
            (id.to_string(), created)
        } else {
            let count: i64 = transaction.query_row(
                "SELECT count(*) FROM smart_folders WHERE user_id=?1",
                [owner],
                |r| r.get(0),
            )?;
            if count >= 100 {
                return Err(AuthStoreError::InvalidContentData);
            }
            (uuid::Uuid::new_v4().to_string(), now.clone())
        };
        transaction.execute("INSERT INTO smart_folders(user_id,id,name,filters_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(user_id,id) DO UPDATE SET name=excluded.name,filters_json=excluded.filters_json,updated_at=excluded.updated_at", params![owner,id,input.name.trim(),serde_json::to_string(&input.filters)?,created_at,now])?;
        transaction.commit()?;
        Ok(Some(SmartFolder {
            id,
            name: input.name.trim().into(),
            filters: input.filters.clone(),
            created_at,
            updated_at: now,
        }))
    }

    pub fn delete_smart_folder(&mut self, owner: &str, id: &str) -> Result<bool, AuthStoreError> {
        Ok(self.connection.execute(
            "DELETE FROM smart_folders WHERE user_id=?1 AND id=?2",
            (owner, id),
        )? == 1)
    }

    /// Maintenance operation: callers must authorize instance-wide administration.
    pub fn rebuild_body_search_index(&mut self) -> Result<(), AuthStoreError> {
        self.connection.execute(
            "INSERT INTO message_body_fts(message_body_fts) VALUES('rebuild')",
            [],
        )?;
        Ok(())
    }
}
