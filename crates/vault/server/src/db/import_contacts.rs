//! What one import run did to each contact: the `vault_import_contacts`
//! table.
//!
//! An import creates contacts, names them, and adds handles to them, and
//! the person wants to know which happened to whom without surprises. The
//! run writes a reason for each contact it touches as it stages, in the same
//! transaction, so the record says what the import decided rather than what
//! timestamps suggest afterwards. The run's Contact Group, its new and
//! changed counts, and `GET /v1/imports/{id}/contacts` all read this table.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;

/// Why a contact is on an import run's record.
///
/// Ordered from most to least consequential. When one run does more than
/// one of these to a contact, the record keeps the earlier variant: a
/// contact the run created was also named and given a handle by it, and
/// "created" is the fact the person needs.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ContactReason {
    /// The run made this contact in place of one the person had trashed
    /// (ADR-0013). Called out because it is the one case where a contact
    /// comes back after being set aside.
    ReplacedTrashed,
    /// The run made this contact for a participant nothing else owned.
    Created,
    /// The contact existed without a name and the backup supplied one
    /// (ADR-0006).
    Named,
    /// The run linked a handle to a contact that already existed: the same
    /// number on another service.
    HandleAdded,
}

impl ContactReason {
    /// Storage id, the same word the API serializes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReplacedTrashed => "replaced_trashed",
            Self::Created => "created",
            Self::Named => "named",
            Self::HandleAdded => "handle_added",
        }
    }

    /// The variant stored as `word`, if any.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "replaced_trashed" => Some(Self::ReplacedTrashed),
            "created" => Some(Self::Created),
            "named" => Some(Self::Named),
            "handle_added" => Some(Self::HandleAdded),
            _ => None,
        }
    }

    /// True when the run brought this contact into being.
    pub fn is_new(self) -> bool {
        matches!(self, Self::ReplacedTrashed | Self::Created)
    }
}

/// Record that `import_id` did `reason` to `contact_id`. A reason already
/// recorded for the pair stays unless the new one is more consequential
/// (see [`ContactReason`]). Does nothing when `import_id` is `None`, which
/// is an import running without a run record, as some tests do.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn record(
    conn: &mut AnyConnection,
    import_id: Option<i64>,
    contact_id: i64,
    reason: ContactReason,
) -> Result<()> {
    let Some(import_id) = import_id else {
        return Ok(());
    };
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT reason FROM vault_import_contacts WHERE import_id = $1 AND contact_id = $2",
    )
    .bind(import_id)
    .bind(contact_id)
    .fetch_optional(&mut *conn)
    .await?;
    match existing.as_deref().and_then(ContactReason::parse) {
        Some(kept) if kept <= reason => Ok(()),
        Some(_) => {
            sqlx::query(
                "UPDATE vault_import_contacts SET reason = $3
                 WHERE import_id = $1 AND contact_id = $2",
            )
            .bind(import_id)
            .bind(contact_id)
            .bind(reason.as_str())
            .execute(&mut *conn)
            .await?;
            Ok(())
        }
        None => {
            sqlx::query(
                "INSERT INTO vault_import_contacts (import_id, contact_id, reason)
                 VALUES ($1, $2, $3)
                 ON CONFLICT DO NOTHING",
            )
            .bind(import_id)
            .bind(contact_id)
            .bind(reason.as_str())
            .execute(&mut *conn)
            .await?;
            Ok(())
        }
    }
}

/// One contact on a run's record, with its current name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportContact {
    /// Contact id.
    pub id: i64,
    /// Preferred name as it is now; empty when the contact has none.
    pub name: String,
    /// What the run did to it.
    pub reason: ContactReason,
}

/// How many contacts a run created and how many it only changed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ContactCounts {
    /// Contacts the run brought into being.
    pub new_count: u64,
    /// Contacts that existed and the run named or gave a handle.
    pub changed_count: u64,
}

/// The run's tally. The caller has already established that `import_id`
/// is the account's.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn counts(conn: &mut AnyConnection, import_id: i64) -> Result<ContactCounts> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT reason, COUNT(*) FROM vault_import_contacts
         WHERE import_id = $1 GROUP BY reason",
    )
    .bind(import_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut tally = ContactCounts::default();
    for (reason, count) in rows {
        let count = count.max(0) as u64;
        match ContactReason::parse(&reason) {
            Some(reason) if reason.is_new() => tally.new_count += count,
            Some(_) => tally.changed_count += count,
            None => {}
        }
    }
    Ok(tally)
}

/// One page of the run's contacts, most consequential reason first and
/// newest contact first within a reason, with the run's total. The caller
/// has already established that `import_id` is the account's.
///
/// # Errors
///
/// Returns an error when a query fails.
pub async fn page(
    conn: &mut AnyConnection,
    import_id: i64,
    limit: usize,
    offset: usize,
) -> Result<(Vec<ImportContact>, u64)> {
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM vault_import_contacts WHERE import_id = $1")
            .bind(import_id)
            .fetch_one(&mut *conn)
            .await?;
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT c.id, c.preferred_name, ic.reason
         FROM vault_import_contacts ic
         JOIN contacts c ON c.id = ic.contact_id
         WHERE ic.import_id = $1
         ORDER BY CASE ic.reason
                    WHEN 'replaced_trashed' THEN 0
                    WHEN 'created' THEN 1
                    WHEN 'named' THEN 2
                    ELSE 3
                  END,
                  c.id DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(import_id)
    .bind(limit as i64)
    .bind(offset as i64)
    .fetch_all(&mut *conn)
    .await?;
    let items = rows
        .into_iter()
        .filter_map(|(id, name, reason)| {
            ContactReason::parse(&reason).map(|reason| ImportContact { id, name, reason })
        })
        .collect();
    Ok((items, total.max(0) as u64))
}

/// Every contact on the run's record, in id order. The run's Contact Group
/// is made from this list.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn contact_ids(conn: &mut AnyConnection, import_id: i64) -> Result<Vec<i64>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT contact_id FROM vault_import_contacts WHERE import_id = $1 ORDER BY contact_id",
    )
    .bind(import_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(ids)
}

#[cfg(test)]
mod tests;
