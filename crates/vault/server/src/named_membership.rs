//! Shared storage for named sets (Message Tags and Contact Groups).
//!
//! Both domains store a named set (rows in a names table) whose members are
//! conversation or contact ids. The HTTP layer addresses a set by id through
//! `list_sets`, `get_set`, `create_set`, `rename_set`, `delete_set`,
//! `list_member_ids_of`, and `patch_members`. The import path still fills a
//! group by name through `set_membership`, which creates the name on demand.
//! The operations are identical apart from table and column names, reserved
//! names, and one post-change hook, so this module implements them once
//! behind [`MembershipSpec`].

use std::future::Future;
use std::pin::Pin;

use anyhow::Result as AnyResult;
use sqlx::AnyConnection;

use crate::db::dialect::{engine_of, name_eq_ci, order_by_name_ci};

/// Longest allowed name for either kind of set (characters).
pub const MAX_NAME_LEN: usize = 80;

/// Create / rename / delete / membership failures for a named set.
#[derive(Debug)]
pub enum MembershipError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl From<sqlx::Error> for MembershipError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<MembershipError> for crate::server::ApiError {
    fn from(e: MembershipError) -> Self {
        match e {
            MembershipError::BadRequest(m) => Self::BadRequest(m),
            MembershipError::NotFound(m) => Self::NotFound(m),
            MembershipError::Conflict(m) => Self::Conflict(m),
            MembershipError::Internal(m) => Self::Internal(m),
        }
    }
}

/// Extra work after a membership change, async over the connection borrow.
type ChangeHook = for<'a> fn(
    &'a mut AnyConnection,
    &'a str,
    i64,
) -> Pin<Box<dyn Future<Output = AnyResult<()>> + Send + 'a>>;

/// Table names, labels, reserved names, and messages for one named set.
///
/// `name_column` and `member_column` live on the membership table;
/// `member_table` is the table members must exist in. All values are compile
/// time constants, so the SQL built from them is fixed at build time.
pub struct MembershipSpec {
    /// Names table (`message_tags` / `contact_groups`).
    pub table: &'static str,
    /// Membership table (`message_tag_members` / `contact_group_members`).
    pub members_table: &'static str,
    /// Column on the membership table that references the names table.
    pub name_column: &'static str,
    /// Column on the membership table that holds the member id.
    pub member_column: &'static str,
    /// Table members must exist in (`conversations` / `contacts`).
    pub member_table: &'static str,
    /// Singular label used in error messages (`"tag"` / `"group"`).
    pub label: &'static str,
    /// Member label used in error messages (`"conversation"` / `"contact"`).
    pub member_label: &'static str,
    /// Longest allowed name (characters).
    pub max_name_len: usize,
    /// Names that must not be created.
    pub reserved: &'static [&'static str],
    /// Reserved names with dedicated error messages (lowercase name, message).
    pub special_reserved: &'static [(&'static str, &'static str)],
    /// Extra work after a membership change (groups touch the contact row).
    pub on_change: Option<ChangeHook>,
}

/// Message tags on conversations.
pub fn tag_spec() -> &'static MembershipSpec {
    static SPEC: MembershipSpec = MembershipSpec {
        table: "message_tags",
        members_table: "message_tag_members",
        name_column: "tag_id",
        member_column: "conversation_id",
        member_table: "conversations",
        label: "tag",
        member_label: "conversation",
        max_name_len: MAX_NAME_LEN,
        reserved: &[
            "home",
            "contacts",
            "threads",
            "thread",
            "all",
            "excluded",
            "unassigned",
            "trash",
            "tags",
            "tag",
            "no-tag",
            "no tag",
            "groups",
            "group",
            "labels",
            "label",
        ],
        special_reserved: &[],
        on_change: None,
    };
    &SPEC
}

/// Contact groups on contacts.
pub fn group_spec() -> &'static MembershipSpec {
    static SPEC: MembershipSpec = MembershipSpec {
        table: "contact_groups",
        members_table: "contact_group_members",
        name_column: "group_id",
        member_column: "contact_id",
        member_table: "contacts",
        label: "group",
        member_label: "contact",
        max_name_len: MAX_NAME_LEN,
        reserved: &[
            "home",
            "contacts",
            "all",
            "excluded",
            "no-messages",
            "no messages",
            "unassigned",
            "trash",
            "groups",
            "group",
            "group-chats",
            "group chats",
            "group-chats-2",
            "group chats 2",
            "group-messages",
            "group messages",
            "group-messages-2",
            "group messages 2",
            "no-label",
            "no-group",
            "no group",
            "labels",
            "label",
            "no label",
        ],
        special_reserved: &[
            ("contacts", "Contacts is a reserved group"),
            ("all", "All is a reserved group"),
            ("excluded", "Excluded is a reserved group"),
            ("unassigned", "Unassigned is a reserved group"),
            ("trash", "Trash is a reserved group"),
            ("no messages", "No messages is a reserved group"),
            ("no-messages", "No messages is a reserved group"),
            ("groups", "Group Messages is a reserved name"),
            ("group", "Group Messages is a reserved name"),
            ("group chats", "Group Messages is a reserved name"),
            ("group-chats", "Group Messages is a reserved name"),
            ("group chats 2", "Group Messages is a reserved name"),
            ("group-chats-2", "Group Messages is a reserved name"),
            ("group messages", "Group Messages is a reserved name"),
            ("group-messages", "Group Messages is a reserved name"),
            ("group messages 2", "Group Messages is a reserved name"),
            ("group-messages-2", "Group Messages is a reserved name"),
        ],
        on_change: Some(touch_member_owner),
    };
    &SPEC
}

/// Bump the member contact's updated-at, boxed so the spec table can hold it as a plain function pointer.
fn touch_member_owner<'a>(
    conn: &'a mut AnyConnection,
    account_id: &'a str,
    member_id: i64,
) -> Pin<Box<dyn Future<Output = AnyResult<()>> + Send + 'a>> {
    Box::pin(crate::db::contacts::touch_contact(
        conn, account_id, member_id,
    ))
}

/// Id of the named set called `name`, if it exists.
async fn find_id(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    name: &str,
) -> Result<Option<i64>, MembershipError> {
    let sql = format!(
        "SELECT id FROM {table} WHERE account_id = $1 AND {name_eq}",
        table = spec.table,
        name_eq = name_eq_ci(engine_of(conn), "name", "$2"),
    );
    let id = sqlx::query_scalar::<_, i64>(&sql)
        .bind(account_id)
        .bind(name)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(id)
}

/// Id of the named set called `name`, creating it if needed.
async fn ensure_id(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    name: &str,
) -> Result<i64, MembershipError> {
    let name = normalize_name(spec, name)?;
    let sql = format!(
        "INSERT INTO {table} (account_id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        table = spec.table
    );
    sqlx::query(&sql)
        .bind(account_id)
        .bind(&name)
        .execute(&mut *conn)
        .await?;
    find_id(spec, conn, account_id, &name)
        .await?
        .ok_or_else(|| MembershipError::Internal(format!("failed to ensure {} {name}", spec.label)))
}

/// True when `name` is reserved and must not be created.
pub fn is_reserved(spec: &MembershipSpec, name: &str) -> bool {
    let key = name.trim().to_ascii_lowercase();
    spec.reserved.contains(&key.as_str())
}

/// The message for a reserved name: the spec's specific one, or the generic one.
fn reserved_error(spec: &MembershipSpec, name: &str) -> String {
    let key = name.trim().to_ascii_lowercase();
    for (reserved, message) in spec.special_reserved {
        if key == *reserved {
            return (*message).to_string();
        }
    }
    format!("\"{}\" is a reserved {}", name.trim(), spec.label)
}

/// Trim and validate a set name against the spec's length and reserved-name rules.
fn normalize_name(spec: &MembershipSpec, name: &str) -> Result<String, MembershipError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(MembershipError::BadRequest("name required".into()));
    }
    if trimmed.chars().count() > spec.max_name_len {
        return Err(MembershipError::BadRequest(format!(
            "name must be at most {} characters",
            spec.max_name_len
        )));
    }
    if is_reserved(spec, trimmed) {
        return Err(MembershipError::BadRequest(reserved_error(spec, trimmed)));
    }
    Ok(trimmed.to_string())
}

/// True when the member row belongs to this account.
async fn member_exists(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    member_id: i64,
) -> Result<bool, MembershipError> {
    let sql = format!(
        "SELECT id FROM {mt} WHERE id = $1 AND account_id = $2",
        mt = spec.member_table
    );
    let found: Option<i64> = sqlx::query_scalar::<_, i64>(&sql)
        .bind(member_id)
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(found.is_some())
}

/// Add or remove one name for many members. Creates the name when enabling.
pub async fn set_membership(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    member_ids: &[i64],
    name: &str,
    enable: bool,
) -> Result<u64, MembershipError> {
    let ids = clean_ids(member_ids);
    if ids.is_empty() {
        return Err(MembershipError::BadRequest(format!(
            "{} ids required",
            spec.member_label
        )));
    }
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        return Err(MembershipError::BadRequest(format!(
            "{} name required",
            spec.label
        )));
    }
    if is_reserved(spec, name_trimmed) {
        return Err(MembershipError::BadRequest(reserved_error(
            spec,
            name_trimmed,
        )));
    }

    for id in &ids {
        if !member_exists(spec, conn, account_id, *id).await? {
            return Err(MembershipError::NotFound(format!(
                "{} {id} not found",
                spec.member_label
            )));
        }
    }

    let name_row_id = if enable {
        ensure_id(spec, conn, account_id, name_trimmed).await?
    } else {
        match find_id(spec, conn, account_id, name_trimmed).await? {
            Some(id) => id,
            None => return Ok(0),
        }
    };

    let mut changed = 0u64;
    for id in ids {
        let n = if enable {
            let sql = insert_member_sql(spec);
            sqlx::query(&sql)
                .bind(name_row_id)
                .bind(id)
                .bind(account_id)
                .execute(&mut *conn)
                .await?
                .rows_affected()
        } else {
            let sql = delete_member_sql(spec);
            sqlx::query(&sql)
                .bind(id)
                .bind(name_row_id)
                .bind(account_id)
                .execute(&mut *conn)
                .await?
                .rows_affected()
        };
        if n > 0 {
            changed += 1;
            if let Some(hook) = spec.on_change {
                hook(conn, account_id, id)
                    .await
                    .map_err(|e| MembershipError::Internal(e.to_string()))?;
            }
        }
    }
    Ok(changed)
}

/// Drop non-positive ids, sort, and dedupe a caller's member id list.
fn clean_ids(ids: &[i64]) -> Vec<i64> {
    let mut out: Vec<i64> = ids.iter().copied().filter(|id| *id > 0).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Sets for this account with their ids, A–Z, excluding reserved leftovers.
pub async fn list_sets(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
) -> Result<Vec<(i64, String)>, MembershipError> {
    let order = order_by_name_ci(engine_of(conn), "name");
    let sql = format!(
        "SELECT id, name FROM {table} WHERE account_id = $1 {order}",
        table = spec.table
    );
    let rows = sqlx::query_as::<_, (i64, String)>(&sql)
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .into_iter()
        .filter(|(_, name)| !is_reserved(spec, name))
        .collect())
}

/// One set by id, or `NotFound` when it is not this account's.
pub async fn get_set(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
) -> Result<(i64, String), MembershipError> {
    let sql = format!(
        "SELECT id, name FROM {table} WHERE id = $1 AND account_id = $2",
        table = spec.table
    );
    let row = sqlx::query_as::<_, (i64, String)>(&sql)
        .bind(id)
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| MembershipError::NotFound(format!("{} not found", spec.label)))?;
    // A reserved-name row can only be a leftover (create_set and rename_set
    // both refuse reserved names): list_sets never shows it, so its id must
    // not work either.
    if is_reserved(spec, &row.1) {
        return Err(MembershipError::NotFound(format!(
            "{} not found",
            spec.label
        )));
    }
    Ok(row)
}

/// Create a set and answer its id and trimmed name. Fails when the name is
/// taken (ignoring case) or reserved.
pub async fn create_set(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    name: &str,
) -> Result<(i64, String), MembershipError> {
    let name = normalize_name(spec, name)?;
    if find_id(spec, conn, account_id, &name).await?.is_some() {
        return Err(MembershipError::Conflict(format!(
            "{} already exists",
            spec.label
        )));
    }
    let sql = format!(
        "INSERT INTO {table} (account_id, name) VALUES ($1, $2) RETURNING id",
        table = spec.table
    );
    let id: i64 = sqlx::query_scalar(&sql)
        .bind(account_id)
        .bind(&name)
        .fetch_one(&mut *conn)
        .await?;
    Ok((id, name))
}

/// Rename a set by id. A case-only change of its own name is allowed; another
/// set's name (ignoring case) is a conflict.
pub async fn rename_set(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
    name: &str,
) -> Result<String, MembershipError> {
    let (_, old_name) = get_set(spec, conn, account_id, id).await?;
    let new_name = normalize_name(spec, name)?;
    if old_name == new_name {
        return Ok(new_name);
    }
    if let Some(other) = find_id(spec, conn, account_id, &new_name).await?
        && other != id
    {
        return Err(MembershipError::Conflict(format!(
            "{} already exists",
            spec.label
        )));
    }
    let sql = format!(
        "UPDATE {table} SET name = $1 WHERE id = $2 AND account_id = $3",
        table = spec.table
    );
    sqlx::query(&sql)
        .bind(&new_name)
        .bind(id)
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(new_name)
}

/// Delete a set by id, and its memberships.
pub async fn delete_set(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
) -> Result<(), MembershipError> {
    get_set(spec, conn, account_id, id).await?;
    let members_sql = format!(
        "DELETE FROM {mt} WHERE {nc} = $1",
        mt = spec.members_table,
        nc = spec.name_column
    );
    sqlx::query(&members_sql)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    let sql = format!(
        "DELETE FROM {table} WHERE id = $1 AND account_id = $2",
        table = spec.table
    );
    sqlx::query(&sql)
        .bind(id)
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Member ids of one set, ascending.
pub async fn list_member_ids_of(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
) -> Result<Vec<i64>, MembershipError> {
    get_set(spec, conn, account_id, id).await?;
    let sql = format!(
        "SELECT {mc} FROM {mt} WHERE {nc} = $1 ORDER BY {mc}",
        mc = spec.member_column,
        mt = spec.members_table,
        nc = spec.name_column,
    );
    let rows = sqlx::query_scalar::<_, i64>(&sql)
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows)
}

/// Add and remove members of one set in one call, answering
/// `(added, removed)`. Every id is checked before anything is written, so a
/// foreign or unknown member id leaves the set as it was. An id present in
/// both `add` and `remove` nets to "removed": it is dropped from `add` so it
/// is deleted, not inserted then deleted, and the `on_change` hook fires
/// once for it rather than twice.
pub async fn patch_members(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    id: i64,
    add: &[i64],
    remove: &[i64],
) -> Result<(u64, u64), MembershipError> {
    get_set(spec, conn, account_id, id).await?;
    let remove = clean_ids(remove);
    let add: Vec<i64> = clean_ids(add)
        .into_iter()
        .filter(|id| !remove.contains(id))
        .collect();
    if add.is_empty() && remove.is_empty() {
        return Err(MembershipError::BadRequest(format!(
            "{} ids required",
            spec.member_label
        )));
    }
    for member in add.iter().chain(remove.iter()) {
        if !member_exists(spec, conn, account_id, *member).await? {
            return Err(MembershipError::NotFound(format!(
                "{} {member} not found",
                spec.member_label
            )));
        }
    }

    let insert_sql = insert_member_sql(spec);
    let delete_sql = delete_member_sql(spec);

    let mut added = 0u64;
    for member in add {
        let n = sqlx::query(&insert_sql)
            .bind(id)
            .bind(member)
            .bind(account_id)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        if n > 0 {
            added += 1;
            if let Some(hook) = spec.on_change {
                hook(conn, account_id, member)
                    .await
                    .map_err(|e| MembershipError::Internal(e.to_string()))?;
            }
        }
    }
    let mut removed = 0u64;
    for member in remove {
        let n = sqlx::query(&delete_sql)
            .bind(member)
            .bind(id)
            .bind(account_id)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        if n > 0 {
            removed += 1;
            if let Some(hook) = spec.on_change {
                hook(conn, account_id, member)
                    .await
                    .map_err(|e| MembershipError::Internal(e.to_string()))?;
            }
        }
    }
    Ok((added, removed))
}

/// Names attached to one member, A–Z.
pub async fn names_for_item(
    spec: &MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    item_id: i64,
) -> AnyResult<Vec<String>> {
    let order = order_by_name_ci(engine_of(conn), "n.name");
    let sql = format!(
        "SELECT n.name
         FROM {table} n
         JOIN {members} m ON m.{name_col} = n.id
         WHERE n.account_id = $1 AND m.{member_col} = $2
         {order}",
        table = spec.table,
        members = spec.members_table,
        name_col = spec.name_column,
        member_col = spec.member_column,
    );
    let rows = sqlx::query_scalar::<_, String>(&sql)
        .bind(account_id)
        .bind(item_id)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows)
}

/// Names attached to each member id, A–Z within each list.
pub async fn names_for_items(
    spec: &'static MembershipSpec,
    conn: &mut AnyConnection,
    account_id: &str,
    item_ids: &[i64],
) -> AnyResult<std::collections::HashMap<i64, Vec<String>>> {
    use crate::db::sql::{fold_in_id_chunks, in_placeholders};
    let account_id = account_id.to_string();
    fold_in_id_chunks(conn, item_ids, |conn, chunk| {
        let account_id = account_id.clone();
        Box::pin(async move {
            let placeholders = in_placeholders(2, chunk.len());
            let order = order_by_name_ci(engine_of(conn), "n.name");
            let sql = format!(
                "SELECT m.{member_col}, n.name
                 FROM {members} m
                 JOIN {table} n ON n.id = m.{name_col}
                 WHERE n.account_id = $1 AND m.{member_col} IN ({placeholders})
                 {order}",
                table = spec.table,
                members = spec.members_table,
                name_col = spec.name_column,
                member_col = spec.member_column,
            );
            let mut q = sqlx::query_as::<_, (i64, String)>(&sql).bind(&account_id);
            for id in chunk {
                q = q.bind(*id);
            }
            let rows = q.fetch_all(&mut *conn).await?;
            Ok(rows)
        })
    })
    .await
}

/// Link one member to one name row, doing nothing when the link exists or
/// the member is not the account's. Binds: name row id, member id, account.
fn insert_member_sql(spec: &MembershipSpec) -> String {
    format!(
        "INSERT INTO {mt} ({mc}, {nc})
         SELECT id, $1 FROM {member_table} WHERE id = $2 AND account_id = $3
         ON CONFLICT DO NOTHING",
        mt = spec.members_table,
        mc = spec.member_column,
        nc = spec.name_column,
        member_table = spec.member_table,
    )
}

/// Unlink one member from one name row when the member is the account's.
/// Binds: member id, name row id, account.
fn delete_member_sql(spec: &MembershipSpec) -> String {
    format!(
        "DELETE FROM {mt}
         WHERE {mc} = $1 AND {nc} = $2
           AND EXISTS (
             SELECT 1 FROM {member_table}
             WHERE {member_table}.id = {mt}.{mc}
               AND {member_table}.account_id = $3
           )",
        mt = spec.members_table,
        mc = spec.member_column,
        nc = spec.name_column,
        member_table = spec.member_table,
    )
}

#[cfg(test)]
mod tests;
