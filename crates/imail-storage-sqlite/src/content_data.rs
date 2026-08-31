use imail_core::{
    messages::{
        GatewayMessagePage, GatewayMessageQuery, MessagePage, MessageQuery, MessageStats,
        MessageStatsBucket,
    },
    ContentRepository, LogoFetchAttemptRecord, MessageRepository,
};
use imail_protocol::{ContactReadModel, DraftReadModel, MessageReadModel};
use rusqlite::{params, OptionalExtension, Row};

use crate::{AuthStoreError, SqliteAuthStore};

impl ContentRepository for SqliteAuthStore {
    type Error = AuthStoreError;

    fn list_messages(&self, user_id: &str) -> Result<Vec<MessageReadModel>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT m.id, m.account_id, m.mailbox, m.mailbox_role, m.uid, m.message_id,
                    m.from_json, m.to_json, m.subject, m.preview, m.text_body, m.html_body,
                    m.received_at, m.unread, m.flagged, m.has_attachments,
                    m.attachments_json, m.labels_json, m.snoozed_until, m.mail_headers_json
             FROM messages m JOIN accounts a ON a.id=m.account_id
             WHERE a.user_id=?1 ORDER BY m.received_at DESC, m.id",
        )?;
        let rows = statement
            .query_map([user_id], message_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(message_from_raw).collect()
    }

    fn upsert_message(
        &mut self,
        user_id: &str,
        message: &MessageReadModel,
    ) -> Result<(), Self::Error> {
        require_owned_account(&self.connection, user_id, &message.account_id)?;
        reject_foreign_content(
            &self.connection,
            "SELECT a.user_id FROM messages m JOIN accounts a ON a.id=m.account_id WHERE m.id=?1",
            &message.id,
            user_id,
        )?;
        let from = serde_json::to_string(&message.from)?;
        let to = serde_json::to_string(&message.to)?;
        let attachments = serde_json::to_string(&message.attachments)?;
        let labels = serde_json::to_string(&message.labels)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO messages
             (id, account_id, mailbox, mailbox_role, uid, message_id, from_json, to_json,
              subject, preview, text_body, html_body, received_at, unread, flagged,
              has_attachments, attachments_json, labels_json, snoozed_until, mail_headers_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
             ON CONFLICT(id) DO UPDATE SET
               mailbox=excluded.mailbox, mailbox_role=excluded.mailbox_role, uid=excluded.uid,
               message_id=excluded.message_id, from_json=excluded.from_json,
               to_json=excluded.to_json, subject=excluded.subject, preview=excluded.preview,
               text_body=excluded.text_body, html_body=excluded.html_body,
               received_at=excluded.received_at, unread=excluded.unread,
               flagged=excluded.flagged, has_attachments=excluded.has_attachments,
               attachments_json=excluded.attachments_json, labels_json=excluded.labels_json,
               snoozed_until=excluded.snoozed_until, mail_headers_json=excluded.mail_headers_json",
            params![
                message.id,
                message.account_id,
                message.mailbox,
                message.mailbox_role,
                message.uid,
                message.message_id,
                from,
                to,
                message.subject,
                message.preview,
                message.text,
                message.html,
                message.date,
                i64::from(message.unread),
                i64::from(message.flagged),
                i64::from(message.has_attachments),
                attachments,
                labels,
                message.snoozed_until,
                serde_json::to_string(&message.headers)?,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn delete_message(&mut self, user_id: &str, message_id: &str) -> Result<bool, Self::Error> {
        let deleted = self.connection.execute(
            "DELETE FROM messages WHERE id=?1 AND account_id IN
             (SELECT id FROM accounts WHERE user_id=?2)",
            (message_id, user_id),
        )?;
        Ok(deleted == 1)
    }

    fn list_drafts(&self, user_id: &str) -> Result<Vec<DraftReadModel>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT d.id, d.account_id, d.to_json, d.cc_json, d.subject, d.text_body,
                    d.html_body, d.attachments_json, d.created_at, d.updated_at, d.compose_json
             FROM drafts d JOIN accounts a ON a.id=d.account_id
             WHERE a.user_id=?1 ORDER BY d.updated_at DESC, d.id",
        )?;
        let rows = statement
            .query_map([user_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| {
                Ok(DraftReadModel {
                    envelope: serde_json::from_str(&row.10)?,
                    id: row.0,
                    account_id: row.1,
                    to: serde_json::from_str(&row.2)?,
                    cc: serde_json::from_str(&row.3)?,
                    subject: row.4,
                    text: row.5,
                    html: row.6,
                    attachments: serde_json::from_str(&row.7)?,
                    created_at: row.8,
                    updated_at: row.9,
                })
            })
            .collect()
    }

    fn upsert_draft(&mut self, user_id: &str, draft: &DraftReadModel) -> Result<(), Self::Error> {
        require_owned_account(&self.connection, user_id, &draft.account_id)?;
        reject_foreign_content(
            &self.connection,
            "SELECT a.user_id FROM drafts d JOIN accounts a ON a.id=d.account_id WHERE d.id=?1",
            &draft.id,
            user_id,
        )?;
        let to = serde_json::to_string(&draft.to)?;
        let cc = serde_json::to_string(&draft.cc)?;
        let attachments = serde_json::to_string(&draft.attachments)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO drafts
             (id, account_id, to_json, cc_json, subject, text_body, html_body,
              attachments_json, created_at, updated_at, compose_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET account_id=excluded.account_id,
               to_json=excluded.to_json, cc_json=excluded.cc_json,
               subject=excluded.subject, text_body=excluded.text_body,
               html_body=excluded.html_body, attachments_json=excluded.attachments_json,
               updated_at=excluded.updated_at, compose_json=excluded.compose_json",
            params![
                draft.id,
                draft.account_id,
                to,
                cc,
                draft.subject,
                draft.text,
                draft.html,
                attachments,
                draft.created_at,
                draft.updated_at,
                serde_json::to_string(&draft.envelope)?,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn delete_draft(&mut self, user_id: &str, draft_id: &str) -> Result<bool, Self::Error> {
        let deleted = self.connection.execute(
            "DELETE FROM drafts WHERE id=?1 AND account_id IN
             (SELECT id FROM accounts WHERE user_id=?2)",
            (draft_id, user_id),
        )?;
        Ok(deleted == 1)
    }

    fn list_contacts(&self, user_id: &str) -> Result<Vec<ContactReadModel>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT user_id, address, name, message_count, last_contact_at, logo_key,
                    logo_content_type, logo_source_url, logo_fetched_at
             FROM contacts WHERE user_id=?1
             ORDER BY last_contact_at DESC, message_count DESC, address",
        )?;
        let contacts = statement
            .query_map([user_id], |row| {
                Ok(ContactReadModel {
                    owner_id: row.get(0)?,
                    address: row.get(1)?,
                    name: row.get(2)?,
                    message_count: row.get(3)?,
                    last_contact_at: row.get(4)?,
                    logo_key: row.get(5)?,
                    logo_content_type: row.get(6)?,
                    logo_source_url: row.get(7)?,
                    logo_fetched_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(contacts)
    }

    fn upsert_contact(&mut self, contact: &ContactReadModel) -> Result<(), Self::Error> {
        require_user(&self.connection, &contact.owner_id)?;
        validate_contact(contact)?;
        self.connection.execute(
            "INSERT INTO contacts
             (user_id, address, name, message_count, last_contact_at, logo_key,
              logo_content_type, logo_source_url, logo_fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(user_id, address) DO UPDATE SET name=excluded.name,
               message_count=excluded.message_count, last_contact_at=excluded.last_contact_at,
               logo_key=excluded.logo_key, logo_content_type=excluded.logo_content_type,
               logo_source_url=excluded.logo_source_url, logo_fetched_at=excluded.logo_fetched_at",
            params![
                contact.owner_id,
                contact.address.to_lowercase(),
                contact.name,
                contact.message_count,
                contact.last_contact_at,
                contact.logo_key,
                contact.logo_content_type,
                contact.logo_source_url,
                contact.logo_fetched_at,
            ],
        )?;
        Ok(())
    }

    fn replace_contacts(
        &mut self,
        user_id: &str,
        contacts: &[ContactReadModel],
    ) -> Result<(), Self::Error> {
        require_user(&self.connection, user_id)?;
        for contact in contacts {
            if contact.owner_id != user_id {
                return Err(AuthStoreError::ContentOwnershipViolation);
            }
            validate_contact(contact)?;
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM contacts WHERE user_id=?1", [user_id])?;
        for contact in contacts {
            transaction.execute(
                "INSERT INTO contacts
                 (user_id, address, name, message_count, last_contact_at, logo_key,
                  logo_content_type, logo_source_url, logo_fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    contact.owner_id,
                    contact.address.to_lowercase(),
                    contact.name,
                    contact.message_count,
                    contact.last_contact_at,
                    contact.logo_key,
                    contact.logo_content_type,
                    contact.logo_source_url,
                    contact.logo_fetched_at,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn delete_contact(&mut self, user_id: &str, address: &str) -> Result<bool, Self::Error> {
        let deleted = self.connection.execute(
            "DELETE FROM contacts WHERE user_id=?1 AND address=?2 COLLATE NOCASE",
            (user_id, address),
        )?;
        Ok(deleted == 1)
    }

    fn list_logo_fetch_attempts(
        &self,
        user_id: &str,
    ) -> Result<Vec<LogoFetchAttemptRecord>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT user_id, target, domain_key, status, detail, attempted_at
             FROM logo_fetch_attempts WHERE user_id=?1 ORDER BY attempted_at DESC, target",
        )?;
        let attempts = statement
            .query_map([user_id], |row| {
                Ok(LogoFetchAttemptRecord {
                    owner_id: row.get(0)?,
                    target: row.get(1)?,
                    domain_key: row.get(2)?,
                    status: row.get(3)?,
                    detail: row.get(4)?,
                    attempted_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(attempts)
    }

    fn upsert_logo_fetch_attempt(
        &mut self,
        attempt: &LogoFetchAttemptRecord,
    ) -> Result<(), Self::Error> {
        require_user(&self.connection, &attempt.owner_id)?;
        if !matches!(attempt.status.as_str(), "success" | "failed") {
            return Err(AuthStoreError::InvalidContentData);
        }
        self.connection.execute(
            "INSERT INTO logo_fetch_attempts
             (user_id, target, domain_key, status, detail, attempted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(user_id, target) DO UPDATE SET domain_key=excluded.domain_key,
               status=excluded.status, detail=excluded.detail, attempted_at=excluded.attempted_at",
            params![
                attempt.owner_id,
                attempt.target,
                attempt.domain_key,
                attempt.status,
                attempt.detail,
                attempt.attempted_at,
            ],
        )?;
        Ok(())
    }

    fn delete_logo_fetch_attempt(
        &mut self,
        user_id: &str,
        target: &str,
    ) -> Result<bool, Self::Error> {
        let deleted = self.connection.execute(
            "DELETE FROM logo_fetch_attempts WHERE user_id=?1 AND target=?2",
            (user_id, target),
        )?;
        Ok(deleted == 1)
    }
}

impl MessageRepository for SqliteAuthStore {
    type Error = AuthStoreError;

    fn conversation_candidates(&self, user_id: &str) -> Result<Vec<MessageReadModel>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT m.id, m.account_id, m.mailbox, m.mailbox_role, m.uid, m.message_id,
                    m.from_json, m.to_json, m.subject, m.preview, '', NULL,
                    m.received_at, m.unread, m.flagged, m.has_attachments,
                    m.attachments_json, m.labels_json, m.snoozed_until, m.mail_headers_json
             FROM messages m JOIN accounts a ON a.id=m.account_id
             WHERE a.user_id=?1 ORDER BY m.received_at, m.id",
        )?;
        let rows = statement
            .query_map([user_id], message_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(message_from_raw).collect()
    }

    fn query_messages(
        &self,
        user_id: &str,
        query: &MessageQuery,
        now: &str,
    ) -> Result<MessagePage, Self::Error> {
        use rusqlite::types::Value as SqlValue;

        let mut clauses = vec!["a.user_id = ?".to_string()];
        let mut values = vec![SqlValue::Text(user_id.to_string())];
        push_exact(
            &mut clauses,
            &mut values,
            "m.account_id = ?",
            &query.account_id,
        );
        push_exact(&mut clauses, &mut values, "a.group_name = ?", &query.group);
        if query.unread {
            clauses.push("m.unread = 1".into());
        }
        if query.flagged {
            clauses.push("m.flagged = 1".into());
        }
        if query.has_attachments {
            clauses.push("m.has_attachments = 1".into());
        }
        push_exact(
            &mut clauses,
            &mut values,
            "m.mailbox_role = ?",
            &query.mailbox_role,
        );
        push_exact(&mut clauses, &mut values, "m.mailbox = ?", &query.mailbox);
        if let Some(name) = &query.mailbox_name {
            clauses.push("EXISTS (SELECT 1 FROM json_each(a.mailboxes_json) folder WHERE lower(json_extract(folder.value, '$.name')) = lower(?) AND json_extract(folder.value, '$.path') = m.mailbox)".into());
            values.push(SqlValue::Text(name.clone()));
        }
        if query.snoozed {
            clauses.push("m.snoozed_until IS NOT NULL AND m.snoozed_until > ?".into());
            values.push(SqlValue::Text(now.to_string()));
        } else if query.mailbox_role.as_deref() == Some("inbox") {
            clauses.push("(m.snoozed_until IS NULL OR m.snoozed_until <= ?)".into());
            values.push(SqlValue::Text(now.to_string()));
        }
        if let Some(label) = &query.label {
            clauses.push("EXISTS (SELECT 1 FROM json_each(m.labels_json) WHERE value = ?)".into());
            values.push(SqlValue::Text(label.clone()));
        }
        if let Some(text) = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            clauses.push(
                "(m.subject LIKE ? OR m.preview LIKE ? OR m.from_json LIKE ? OR m.to_json LIKE ?)"
                    .into(),
            );
            let pattern = SqlValue::Text(format!("%{text}%"));
            values.extend([pattern.clone(), pattern.clone(), pattern.clone(), pattern]);
        }
        if let Some(sender) = query
            .sender
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            clauses.push(
                "lower(CASE json_type(m.from_json)
                    WHEN 'object' THEN json_extract(m.from_json, '$.address')
                    WHEN 'text' THEN json_extract(m.from_json, '$')
                    ELSE ''
                END) = lower(?)"
                    .into(),
            );
            values.push(SqlValue::Text(sender.to_string()));
        }
        if let Some(recipient) = query
            .recipient
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            clauses.push(
                "EXISTS (
                    SELECT 1 FROM json_each(m.to_json) AS recipient
                    WHERE lower(CASE recipient.type
                        WHEN 'object' THEN json_extract(recipient.value, '$.address')
                        WHEN 'text' THEN recipient.value
                        ELSE ''
                    END) = lower(?)
                )"
                .into(),
            );
            values.push(SqlValue::Text(recipient.to_string()));
        }
        if let Some(filters) = &query.filters {
            crate::search::append_filters(filters, &mut clauses, &mut values, now)?;
        }
        // Total describes the entire query, not only rows after the paging cursor.
        let total: i64 = self.connection.query_row(
            &format!(
                "SELECT count(*) FROM messages m JOIN accounts a ON a.id=m.account_id WHERE {}",
                clauses.join(" AND ")
            ),
            rusqlite::params_from_iter(values.iter()),
            |row| row.get(0),
        )?;
        if let Some(cursor) = &query.cursor {
            clauses.push("(m.received_at < ? OR (m.received_at = ? AND m.id < ?))".into());
            values.extend([
                SqlValue::Text(cursor.date.clone()),
                SqlValue::Text(cursor.date.clone()),
                SqlValue::Text(cursor.id.clone()),
            ]);
        }
        let from = format!(
            "FROM messages m JOIN accounts a ON a.id=m.account_id WHERE {}",
            clauses.join(" AND ")
        );
        values.push(SqlValue::Integer((query.limit + 1) as i64));
        values.push(SqlValue::Integer(query.offset as i64));
        let mut statement = self.connection.prepare(&format!(
            "SELECT m.id, m.account_id, m.mailbox, m.mailbox_role, m.uid, m.message_id,
                    m.from_json, m.to_json, m.subject, m.preview, m.text_body, m.html_body,
                    m.received_at, m.unread, m.flagged, m.has_attachments,
                    m.attachments_json, m.labels_json, m.snoozed_until, m.mail_headers_json
             {from} ORDER BY m.received_at DESC, m.id DESC LIMIT ? OFFSET ?"
        ))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(values.iter()), message_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut messages = rows
            .into_iter()
            .map(message_from_raw)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = messages.len() > query.limit;
        messages.truncate(query.limit);
        Ok(MessagePage {
            messages,
            total: usize::try_from(total).unwrap_or(usize::MAX),
            has_more,
        })
    }

    fn message(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageReadModel>, Self::Error> {
        let mut statement = self.connection.prepare(
            "SELECT m.id, m.account_id, m.mailbox, m.mailbox_role, m.uid, m.message_id,
                    m.from_json, m.to_json, m.subject, m.preview, m.text_body, m.html_body,
                    m.received_at, m.unread, m.flagged, m.has_attachments,
                    m.attachments_json, m.labels_json, m.snoozed_until, m.mail_headers_json
             FROM messages m JOIN accounts a ON a.id=m.account_id
             WHERE m.id=?1 AND a.user_id=?2",
        )?;
        statement
            .query_row((message_id, user_id), message_row)
            .optional()?
            .map(message_from_raw)
            .transpose()
    }

    fn message_source(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        self.connection
            .query_row(
                "SELECT s.source FROM message_sources s
                 JOIN messages m ON m.id=s.message_id
                 JOIN accounts a ON a.id=m.account_id
                 WHERE s.message_id=?1 AND a.user_id=?2",
                (message_id, user_id),
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    fn message_stats(&self, user_id: &str, now: &str) -> Result<MessageStats, Self::Error> {
        let active = "m.mailbox_role='inbox' AND (m.snoozed_until IS NULL OR m.snoozed_until<=?2)";
        let (total, unread): (i64, i64) = self.connection.query_row(
            &format!("SELECT count(*),coalesce(sum(m.unread),0) FROM messages m JOIN accounts a ON a.id=m.account_id WHERE a.user_id=?1 AND {active}"),
            (user_id, now),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut by_account_statement = self.connection.prepare(&format!(
            "SELECT m.account_id,count(*),coalesce(sum(m.unread),0) FROM messages m JOIN accounts a ON a.id=m.account_id WHERE a.user_id=?1 AND {active} GROUP BY m.account_id ORDER BY m.account_id"
        ))?;
        let by_account = by_account_statement
            .query_map((user_id, now), |row| {
                Ok(MessageStatsBucket {
                    account_id: Some(row.get(0)?),
                    group: None,
                    total: count(row, 1)?,
                    unread: count(row, 2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut by_group_statement = self.connection.prepare(&format!(
            "SELECT a.group_name,count(*),coalesce(sum(m.unread),0) FROM messages m JOIN accounts a ON a.id=m.account_id WHERE a.user_id=?1 AND {active} GROUP BY a.group_name ORDER BY a.group_name"
        ))?;
        let by_group = by_group_statement
            .query_map((user_id, now), |row| {
                Ok(MessageStatsBucket {
                    account_id: None,
                    group: Some(row.get(0)?),
                    total: count(row, 1)?,
                    unread: count(row, 2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MessageStats {
            total: usize::try_from(total).unwrap_or(usize::MAX),
            unread: usize::try_from(unread).unwrap_or(usize::MAX),
            by_account,
            by_group,
        })
    }

    fn query_gateway_messages(
        &self,
        user_id: &str,
        query: &GatewayMessageQuery,
    ) -> Result<GatewayMessagePage, Self::Error> {
        use rusqlite::types::Value as SqlValue;

        if query.account_ids.is_empty() {
            return Ok(GatewayMessagePage {
                messages: Vec::new(),
                has_more: false,
            });
        }
        let mut clauses = vec!["a.user_id = ?".to_string()];
        let mut values = vec![SqlValue::Text(user_id.to_string())];
        let placeholders = std::iter::repeat("?")
            .take(query.account_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        clauses.push(format!("m.account_id IN ({placeholders})"));
        values.extend(query.account_ids.iter().cloned().map(SqlValue::Text));
        if let Some(recipient) = query
            .recipient
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            clauses.push(
                "EXISTS (
                    SELECT 1 FROM json_each(m.to_json) AS recipient
                    WHERE lower(CASE recipient.type
                        WHEN 'object' THEN json_extract(recipient.value, '$.address')
                        WHEN 'text' THEN recipient.value
                        ELSE ''
                    END) = lower(?)
                )"
                .into(),
            );
            values.push(SqlValue::Text(recipient.to_string()));
        }
        push_exact(
            &mut clauses,
            &mut values,
            "m.mailbox_role = ?",
            &query.mailbox_role,
        );
        if let Some(unread) = query.unread {
            clauses.push("m.unread = ?".into());
            values.push(SqlValue::Integer(i64::from(unread)));
        }
        if let Some(since) = &query.since {
            clauses.push("m.received_at >= ?".into());
            values.push(SqlValue::Text(since.clone()));
        }
        if let Some(before) = &query.before {
            clauses.push("m.received_at < ?".into());
            values.push(SqlValue::Text(before.clone()));
        }
        if let Some(text) = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            clauses.push("(m.subject LIKE ? OR m.preview LIKE ? OR m.from_json LIKE ?)".into());
            let pattern = SqlValue::Text(format!("%{text}%"));
            values.extend([pattern.clone(), pattern.clone(), pattern]);
        }
        if let Some(cursor) = &query.cursor {
            clauses.push("(m.received_at < ? OR (m.received_at = ? AND m.id < ?))".into());
            values.extend([
                SqlValue::Text(cursor.date.clone()),
                SqlValue::Text(cursor.date.clone()),
                SqlValue::Text(cursor.id.clone()),
            ]);
        }
        if let Some(filters) = &query.filters {
            crate::search::append_filters(
                filters,
                &mut clauses,
                &mut values,
                &chrono::Utc::now().to_rfc3339(),
            )?;
        }
        values.push(SqlValue::Integer((query.limit + 1) as i64));
        let mut statement = self.connection.prepare(&format!(
            "SELECT m.id, m.account_id, m.mailbox, m.mailbox_role, m.uid, m.message_id,
                    m.from_json, m.to_json, m.subject, m.preview, m.text_body, m.html_body,
                    m.received_at, m.unread, m.flagged, m.has_attachments,
                    m.attachments_json, m.labels_json, m.snoozed_until, m.mail_headers_json
             FROM messages m JOIN accounts a ON a.id=m.account_id
             WHERE {} ORDER BY m.received_at DESC, m.id DESC LIMIT ?",
            clauses.join(" AND ")
        ))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(values.iter()), message_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut messages = rows
            .into_iter()
            .map(message_from_raw)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = messages.len() > query.limit;
        messages.truncate(query.limit);
        Ok(GatewayMessagePage { messages, has_more })
    }
}

fn count(row: &Row<'_>, index: usize) -> rusqlite::Result<usize> {
    row.get::<_, i64>(index)
        .map(|value| usize::try_from(value).unwrap_or(usize::MAX))
}

fn push_exact(
    clauses: &mut Vec<String>,
    values: &mut Vec<rusqlite::types::Value>,
    sql: &str,
    value: &Option<String>,
) {
    if let Some(value) = value {
        clauses.push(sql.to_string());
        values.push(rusqlite::types::Value::Text(value.clone()));
    }
}

fn validate_contact(contact: &ContactReadModel) -> Result<(), AuthStoreError> {
    let logo_fields = [
        contact.logo_key.is_some(),
        contact.logo_content_type.is_some(),
        contact.logo_source_url.is_some(),
        contact.logo_fetched_at.is_some(),
    ];
    if logo_fields.iter().any(|present| *present) && !logo_fields.iter().all(|present| *present) {
        return Err(AuthStoreError::InvalidContentData);
    }
    Ok(())
}

type RawMessage = (
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    i64,
    i64,
    i64,
    String,
    String,
    Option<String>,
    String,
);

fn message_row(row: &Row<'_>) -> rusqlite::Result<RawMessage> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
    ))
}

fn message_from_raw(row: RawMessage) -> Result<MessageReadModel, AuthStoreError> {
    Ok(MessageReadModel {
        headers: serde_json::from_str(&row.19)?,
        id: row.0,
        account_id: row.1,
        mailbox: row.2,
        mailbox_role: row.3,
        uid: row.4,
        message_id: row.5,
        from: serde_json::from_str(&row.6)?,
        to: serde_json::from_str(&row.7)?,
        subject: row.8,
        preview: row.9,
        text: row.10,
        html: row.11,
        date: row.12,
        unread: boolean(row.13)?,
        flagged: boolean(row.14)?,
        has_attachments: boolean(row.15)?,
        attachments: serde_json::from_str(&row.16)?,
        labels: serde_json::from_str(&row.17)?,
        snoozed_until: row.18,
    })
}

fn boolean(value: i64) -> Result<bool, AuthStoreError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(AuthStoreError::Sqlite(
            rusqlite::Error::IntegralValueOutOfRange(0, value),
        )),
    }
}

fn require_user(connection: &rusqlite::Connection, user_id: &str) -> Result<(), AuthStoreError> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM app_users WHERE id=?1)",
        [user_id],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(AuthStoreError::UserNotFound)
    }
}

fn require_owned_account(
    connection: &rusqlite::Connection,
    user_id: &str,
    account_id: &str,
) -> Result<(), AuthStoreError> {
    let owned: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND user_id=?2)",
        (account_id, user_id),
        |row| row.get(0),
    )?;
    if owned {
        Ok(())
    } else {
        Err(AuthStoreError::AccountNotOwned)
    }
}

fn reject_foreign_content(
    connection: &rusqlite::Connection,
    owner_sql: &str,
    record_id: &str,
    user_id: &str,
) -> Result<(), AuthStoreError> {
    let owner: Option<String> = connection
        .query_row(owner_sql, [record_id], |row| row.get(0))
        .optional()?;
    if owner.as_deref().is_some_and(|owner| owner != user_id) {
        Err(AuthStoreError::ContentOwnershipViolation)
    } else {
        Ok(())
    }
}
