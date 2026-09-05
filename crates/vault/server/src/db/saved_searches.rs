//! Per-account saved searches: named queries a user runs again from the sidebar.
//!
//! A saved search collects nothing. It stores a query string verbatim and is
//! never validated: each list accepts its own subset of the search language,
//! so a query legal for one list can be a 400 on another (see `search`).
//!
//! Rows are addressed by `id` rather than by name, unlike contact groups and
//! message tags: an edit changes the name and the query together, so a
//! name-addressed update would use the changing field as its key.

use serde::Serialize;
use sqlx::any::AnyRow;
use sqlx::{AnyConnection, Row};

use crate::db::dialect::{engine_of, name_eq_ci, order_by_name_ci};
use crate::named_membership::MAX_NAME_LEN;

/// How a saved search was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedSearchKind {
    /// A person wrote it.
    Manual,
    /// The server created it at the end of an import run.
    Import,
}

impl SavedSearchKind {
    /// Stored spelling of this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Import => "import",
        }
    }
}

/// One row of `saved_searches`.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SavedSearch {
    /// Saved search id, unique across the vault.
    pub id: i64,
    /// Display name, unique per account.
    pub name: String,
    /// Query string, run against the conversation list.
    pub query: String,
    /// `manual` or `import`.
    pub kind: String,
}

/// Create / update / delete failures for a saved search.
#[derive(Debug)]
pub enum SavedSearchError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl From<sqlx::Error> for SavedSearchError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<SavedSearchError> for crate::server::ApiError {
    fn from(e: SavedSearchError) -> Self {
        match e {
            SavedSearchError::BadRequest(m) => Self::BadRequest(m),
            SavedSearchError::NotFound(m) => Self::NotFound(m),
            SavedSearchError::Conflict(m) => Self::Conflict(m),
            SavedSearchError::Internal(m) => Self::Internal(m),
        }
    }
}

type Result<T> = std::result::Result<T, SavedSearchError>;

/// Map one `saved_searches` row by column name.
fn row_to_saved_search(row: &AnyRow) -> Result<SavedSearch> {
    Ok(SavedSearch {
        id: row
            .try_get::<i64, _>("id")
            .map_err(|e| SavedSearchError::Internal(e.to_string()))?,
        name: row
            .try_get::<String, _>("name")
            .map_err(|e| SavedSearchError::Internal(e.to_string()))?,
        query: row
            .try_get::<String, _>("query")
            .map_err(|e| SavedSearchError::Internal(e.to_string()))?,
        kind: row
            .try_get::<String, _>("kind")
            .map_err(|e| SavedSearchError::Internal(e.to_string()))?,
    })
}

/// Trim and length-check a name. Empty names and names over
/// [`MAX_NAME_LEN`] characters are rejected, matching the neighbouring
/// collections.
fn normalize_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(SavedSearchError::BadRequest("name required".into()));
    }
    if trimmed.chars().count() > MAX_NAME_LEN {
        return Err(SavedSearchError::BadRequest(format!(
            "name must be {MAX_NAME_LEN} characters or fewer"
        )));
    }
    Ok(trimmed.to_string())
}

/// Trim a query. Empty queries are rejected; the contents are never inspected.
fn normalize_query(query: &str) -> Result<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(SavedSearchError::BadRequest("query required".into()));
    }
    Ok(trimmed.to_string())
}

/// Id of an account's saved search with this name, case-insensitively.
async fn find_id_by_name(
    conn: &mut AnyConnection,
    account_id: &str,
    name: &str,
) -> Result<Option<i64>> {
    let sql = format!(
        "SELECT id FROM saved_searches WHERE account_id = $1 AND {}",
        name_eq_ci(engine_of(conn), "name", "$2")
    );
    let id = sqlx::query_scalar::<_, i64>(&sql)
        .bind(account_id)
        .bind(name)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(id)
}

/// One account's saved searches, A–Z.
pub async fn list(conn: &mut AnyConnection, account_id: &str) -> Result<Vec<SavedSearch>> {
    let sql = format!(
        "SELECT id, name, query, kind FROM saved_searches WHERE account_id = $1 {}",
        order_by_name_ci(engine_of(conn), "name")
    );
    let rows = sqlx::query(&sql)
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(row_to_saved_search).collect()
}

/// One saved search by id, scoped to the account that owns it.
pub async fn get(
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
) -> Result<Option<SavedSearch>> {
    let row = sqlx::query(
        "SELECT id, name, query, kind FROM saved_searches WHERE account_id = $1 AND id = $2",
    )
    .bind(account_id)
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(row_to_saved_search).transpose()
}

/// Create a saved search. The name must be free within the account.
pub async fn create(
    conn: &mut AnyConnection,
    account_id: &str,
    name: &str,
    query: &str,
    kind: SavedSearchKind,
) -> Result<SavedSearch> {
    let name = normalize_name(name)?;
    let query = normalize_query(query)?;
    if find_id_by_name(conn, account_id, &name).await?.is_some() {
        return Err(SavedSearchError::Conflict(
            "saved search already exists".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO saved_searches (account_id, name, query, kind) VALUES ($1, $2, $3, $4)",
    )
    .bind(account_id)
    .bind(&name)
    .bind(&query)
    .bind(kind.as_str())
    .execute(&mut *conn)
    .await?;
    let Some(id) = find_id_by_name(conn, account_id, &name).await? else {
        return Err(SavedSearchError::Internal(
            "saved search vanished after insert".into(),
        ));
    };
    Ok(SavedSearch {
        id,
        name,
        query,
        kind: kind.as_str().to_string(),
    })
}

/// Replace a saved search's name and query. `kind` is not editable: it records
/// how the row was born.
pub async fn update(
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
    name: &str,
    query: &str,
) -> Result<SavedSearch> {
    let name = normalize_name(name)?;
    let query = normalize_query(query)?;
    let Some(existing) = get(conn, account_id, id).await? else {
        return Err(SavedSearchError::NotFound("saved search not found".into()));
    };
    // A name already used by a *different* row is a conflict; keeping or
    // recasing this row's own name is not.
    if let Some(other) = find_id_by_name(conn, account_id, &name).await?
        && other != id
    {
        return Err(SavedSearchError::Conflict(
            "saved search already exists".into(),
        ));
    }
    sqlx::query(
        "UPDATE saved_searches SET name = $1, query = $2 WHERE account_id = $3 AND id = $4",
    )
    .bind(&name)
    .bind(&query)
    .bind(account_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(SavedSearch {
        id,
        name,
        query,
        kind: existing.kind,
    })
}

/// Delete a saved search.
///
/// This never touches `vault_imports`: an import-created saved search is a
/// shortcut to a run's messages, and the run's own record is permanent.
pub async fn delete(conn: &mut AnyConnection, account_id: &str, id: i64) -> Result<()> {
    let result = sqlx::query("DELETE FROM saved_searches WHERE account_id = $1 AND id = $2")
        .bind(account_id)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if result.rows_affected() == 0 {
        return Err(SavedSearchError::NotFound("saved search not found".into()));
    }
    Ok(())
}

/// Name for an import's saved search, adding " 2", " 3", … when the account
/// already used the plain name on the same day.
async fn unique_import_name(
    conn: &mut AnyConnection,
    account_id: &str,
    source: &str,
    date_ymd: &str,
) -> Result<String> {
    let base = format!("Import {source} {date_ymd}");
    if find_id_by_name(conn, account_id, &base).await?.is_none() {
        return Ok(base);
    }
    for n in 2..1000 {
        let candidate = format!("{base} {n}");
        if find_id_by_name(conn, account_id, &candidate)
            .await?
            .is_none()
        {
            return Ok(candidate);
        }
    }
    Err(SavedSearchError::Conflict(
        "too many imports named alike on one day".into(),
    ))
}

/// Create the saved search that points at one import run's messages.
///
/// Called when a run finishes having inserted at least one message. A run
/// that failed, was cancelled, or stored nothing gets no saved search — it is
/// still recorded in `vault_imports` either way.
pub async fn create_for_import(
    conn: &mut AnyConnection,
    account_id: &str,
    import_id: i64,
    source: &str,
    date_ymd: &str,
) -> Result<SavedSearch> {
    let name = unique_import_name(conn, account_id, source, date_ymd).await?;
    create(
        conn,
        account_id,
        &name,
        &format!("import:{import_id}"),
        SavedSearchKind::Import,
    )
    .await
}

#[cfg(test)]
mod tests;
