use chrono::{SecondsFormat, Utc};
use rusqlite::OptionalExtension;

use crate::{AuthStoreError, SqliteAuthStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleHmeAddressRecord {
    pub account_id: String,
    pub user_id: String,
    pub anonymous_id: String,
    pub email: String,
    pub label: String,
    pub note: String,
    pub forward_to_email: String,
    pub active: bool,
    pub origin: String,
    pub created_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleHmeAddressSnapshot {
    pub addresses: Vec<AppleHmeAddressRecord>,
    pub last_synced_at: Option<String>,
}

impl SqliteAuthStore {
    pub fn apple_hme_addresses(
        &self,
        user_id: &str,
        account_id: &str,
    ) -> Result<AppleHmeAddressSnapshot, AuthStoreError> {
        require_owned_account(self, user_id, account_id)?;
        let addresses = self
            .connection
            .prepare(
                "SELECT account_id,user_id,anonymous_id,email,label,note,forward_to_email,
                        active,origin,created_at,updated_at
                   FROM apple_hme_addresses
                  WHERE account_id=?1 AND user_id=?2
                  ORDER BY active DESC, created_at DESC, email COLLATE NOCASE",
            )?
            .query_map((account_id, user_id), address_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let last_synced_at = self
            .connection
            .query_row(
                "SELECT last_synced_at FROM apple_hme_sync_state
                  WHERE account_id=?1 AND user_id=?2",
                (account_id, user_id),
                |row| row.get(0),
            )
            .optional()?;
        Ok(AppleHmeAddressSnapshot {
            addresses,
            last_synced_at,
        })
    }

    pub fn replace_apple_hme_addresses(
        &mut self,
        user_id: &str,
        account_id: &str,
        addresses: &[AppleHmeAddressRecord],
    ) -> Result<AppleHmeAddressSnapshot, AuthStoreError> {
        validate_addresses(addresses, user_id, account_id)?;
        require_owned_account(self, user_id, account_id)?;
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM apple_hme_addresses WHERE account_id=?1 AND user_id=?2",
            (account_id, user_id),
        )?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO apple_hme_addresses(
                    account_id,user_id,anonymous_id,email,label,note,forward_to_email,
                    active,origin,created_at,updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            )?;
            for address in addresses {
                statement.execute((
                    account_id,
                    user_id,
                    &address.anonymous_id,
                    &address.email,
                    &address.label,
                    &address.note,
                    &address.forward_to_email,
                    address.active,
                    &address.origin,
                    &address.created_at,
                    &now,
                ))?;
            }
        }
        transaction.execute(
            "INSERT INTO apple_hme_sync_state(account_id,user_id,last_synced_at,address_count)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(account_id) DO UPDATE SET
               user_id=excluded.user_id,last_synced_at=excluded.last_synced_at,
               address_count=excluded.address_count
             WHERE apple_hme_sync_state.user_id=excluded.user_id",
            (account_id, user_id, &now, addresses.len() as i64),
        )?;
        transaction.commit()?;
        self.apple_hme_addresses(user_id, account_id)
    }

    pub fn upsert_apple_hme_address(
        &mut self,
        address: &AppleHmeAddressRecord,
    ) -> Result<(), AuthStoreError> {
        validate_addresses(
            std::slice::from_ref(address),
            &address.user_id,
            &address.account_id,
        )?;
        require_owned_account(self, &address.user_id, &address.account_id)?;
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO apple_hme_addresses(
                account_id,user_id,anonymous_id,email,label,note,forward_to_email,
                active,origin,created_at,updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(account_id,anonymous_id) DO UPDATE SET
               email=excluded.email,label=excluded.label,note=excluded.note,
               forward_to_email=excluded.forward_to_email,active=excluded.active,
               origin=excluded.origin,created_at=excluded.created_at,updated_at=excluded.updated_at
             WHERE apple_hme_addresses.user_id=excluded.user_id",
            (
                &address.account_id,
                &address.user_id,
                &address.anonymous_id,
                &address.email,
                &address.label,
                &address.note,
                &address.forward_to_email,
                address.active,
                &address.origin,
                &address.created_at,
                now,
            ),
        )?;
        transaction.execute(
            "UPDATE apple_hme_sync_state
                SET address_count=(SELECT count(*) FROM apple_hme_addresses
                                    WHERE account_id=?1 AND user_id=?2)
              WHERE account_id=?1 AND user_id=?2",
            (&address.account_id, &address.user_id),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_apple_hme_address_active(
        &mut self,
        user_id: &str,
        account_id: &str,
        anonymous_id: &str,
        active: bool,
    ) -> Result<bool, AuthStoreError> {
        require_owned_account(self, user_id, account_id)?;
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        Ok(self.connection.execute(
            "UPDATE apple_hme_addresses SET active=?1,updated_at=?2
              WHERE account_id=?3 AND user_id=?4 AND anonymous_id=?5",
            (active, now, account_id, user_id, anonymous_id),
        )? == 1)
    }

    pub fn delete_apple_hme_address(
        &mut self,
        user_id: &str,
        account_id: &str,
        anonymous_id: &str,
    ) -> Result<bool, AuthStoreError> {
        require_owned_account(self, user_id, account_id)?;
        let transaction = self.connection.transaction()?;
        let deleted = transaction.execute(
            "DELETE FROM apple_hme_addresses
              WHERE account_id=?1 AND user_id=?2 AND anonymous_id=?3",
            (account_id, user_id, anonymous_id),
        )? == 1;
        transaction.execute(
            "UPDATE apple_hme_sync_state
                SET address_count=(SELECT count(*) FROM apple_hme_addresses
                                    WHERE account_id=?1 AND user_id=?2)
              WHERE account_id=?1 AND user_id=?2",
            (account_id, user_id),
        )?;
        transaction.commit()?;
        Ok(deleted)
    }
}

fn require_owned_account(
    store: &SqliteAuthStore,
    user_id: &str,
    account_id: &str,
) -> Result<(), AuthStoreError> {
    let owned: bool = store.connection.query_row(
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

fn validate_addresses(
    addresses: &[AppleHmeAddressRecord],
    user_id: &str,
    account_id: &str,
) -> Result<(), AuthStoreError> {
    if addresses.len() > 10_000
        || addresses.iter().any(|address| {
            address.user_id != user_id
                || address.account_id != account_id
                || address.anonymous_id.is_empty()
                || address.anonymous_id.len() > 512
                || address.email.is_empty()
                || address.email.len() > 512
                || address.label.len() > 1_000
                || address.note.len() > 4_000
                || address.forward_to_email.len() > 512
                || address.origin.len() > 1_000
        })
    {
        return Err(AuthStoreError::InvalidContentData);
    }
    Ok(())
}

fn address_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppleHmeAddressRecord> {
    Ok(AppleHmeAddressRecord {
        account_id: row.get(0)?,
        user_id: row.get(1)?,
        anonymous_id: row.get(2)?,
        email: row.get(3)?,
        label: row.get(4)?,
        note: row.get(5)?,
        forward_to_email: row.get(6)?,
        active: row.get(7)?,
        origin: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}
