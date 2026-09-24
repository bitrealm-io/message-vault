//! Contact list/detail used by `GET /v1/contacts`,
//! `GET /v1/contacts/{id}` and `PATCH /v1/contacts/{id}`, `POST /v1/contacts/summaries`,
//! and `POST /v1/contacts/match`. Loading an address book is in
//! `address_book`, the edits a `PATCH` makes in `edit`, and the queries in
//! `db::contacts`.

use crate::extract::{Json, Path as AxumPath, Query};
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;

use crate::db::contacts::read::{
    CONTACT_SORT_KEYS, ContactSelectionSummary, ContactSummary, DEFAULT_CONTACT_SORT,
    contact_name_and_modified, contact_totals, get_contact_summaries, list_contacts_sorted,
    unknown_contact_identifiers,
};
use crate::db::handles::{self, IdentitiesOf, Identity};
use crate::db::trash::{DeleteOutcome, Trashable, delete_trashed, move_to_trash, restore};
use crate::paging::{ListRequest, MAX_CONTACT_SUMMARY_IDS, Page, PageQuery, whole_page};
use crate::server::{ApiError, AppState, FullAccess, FullDeleteAccess};

pub(crate) mod address_book;
mod edit;

use edit::mutate_contact;

/// A handle value plus optional platform service.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AddContactIdentityRequest {
    /// Handle value to link.
    pub handle: String,
    /// Platform service (`phone`, `email`, or `whatsapp`); inferred when omitted.
    #[serde(default)]
    pub service: Option<String>,
}

/// The previous and new handle values for a link change.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateContactIdentityRequest {
    /// Handle value currently linked.
    pub previous_handle: String,
    /// Replacement handle value.
    pub handle: String,
    /// Platform service for the new handle.
    #[serde(default)]
    pub service: Option<String>,
}

/// The handle to unlink.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RemoveContactIdentityRequest {
    /// Handle value to unlink.
    pub handle: String,
    /// Platform service, when the handle is linked with one.
    #[serde(default)]
    pub service: Option<String>,
}

/// Body for `PATCH /v1/contacts/{id}`. Exactly one mutation field should be set.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateContactRequest {
    /// New display name; `None` leaves it unchanged.
    #[serde(default)]
    pub name: Option<String>,
    /// Handle link to add.
    #[serde(default)]
    pub add_handle: Option<AddContactIdentityRequest>,
    /// Handle link to replace.
    #[serde(default)]
    pub update_handle: Option<UpdateContactIdentityRequest>,
    /// Handle link to remove.
    #[serde(default)]
    pub remove_handle: Option<RemoveContactIdentityRequest>,
}

/// Full contact view: every handle with stats, plus totals across them.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Contact {
    /// Contact id.
    pub id: i64,
    /// The contact's preferred name; empty when it has none.
    pub name: String,
    /// True when the contact is in the Unknown Contact Group: it has no
    /// identity, or it has identities and no preferred name.
    pub unknown: bool,
    /// Every handle linked to the contact, with per-handle stats.
    pub handles: Vec<Identity>,
    /// 1:1 conversations the contact appears in.
    pub direct_conversations: u64,
    /// Group conversations the contact appears in.
    pub group_conversations: u64,
    /// Messages across all of the contact's conversations.
    pub total_messages: u64,
    /// When the contact’s address-book shape last changed (`datetime('now')`).
    pub last_modified: String,
    /// Group names on this contact (A–Z).
    #[serde(default)]
    pub groups: Vec<String>,
}

/// Body for `POST /v1/contacts/summaries`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SummarizeContactsRequest {
    /// Contact ids to summarize; an empty list covers every contact.
    #[serde(default)]
    pub ids: Vec<i64>,
}

/// Full contact view: per-handle service + date range + direct message count,
/// plus conversation and total-message stats across all the contact's handles.
///
/// # Errors
///
/// Returns an internal error when a database statement fails.
pub async fn get_contact_detail(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
) -> Result<Option<Contact>, ApiError> {
    let Some((name, unknown, last_modified)) =
        contact_name_and_modified(conn, account_id, contact_id).await?
    else {
        return Ok(None);
    };
    let handles = handles::identities(
        conn,
        IdentitiesOf::Contact {
            account_id,
            contact_id,
        },
    )
    .await?;
    let totals = contact_totals(conn, account_id, contact_id).await?;
    let contact_groups = crate::db::named_membership::names_for_item(
        crate::db::named_membership::group_spec(),
        conn,
        account_id,
        contact_id,
    )
    .await?;

    Ok(Some(Contact {
        id: contact_id,
        name,
        unknown,
        handles,
        direct_conversations: totals.direct,
        group_conversations: totals.groups,
        total_messages: totals.messages,
        last_modified,
        groups: contact_groups,
    }))
}

/// Most identifiers one request to `POST /v1/contacts/match` may ask about.
///
/// A staged folder can reference thousands of participants; the client
/// batches. The query runs as a single statement with no chunking, so the
/// cap keeps its bind count bounded (501 identifiers would mean 502 binds);
/// raising it needs `SQLITE_IN_CHUNK`-style chunking first, not just a
/// bigger number.
pub(crate) const MAX_MATCH_IDENTIFIERS: usize = 500;

/// Body for `POST /v1/contacts/unmatched-identities`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct FindUnmatchedIdentitiesRequest {
    /// Raw identifiers — phone numbers, emails — as they appear in an export.
    identifiers: Vec<String>,
}

/// Report which identifiers this account has no vault contact for.
#[utoipa::path(
    post,
    path = "/v1/contacts/unmatched-identities",
    tag = "Contacts",
    security(("session" = [])),
    request_body = FindUnmatchedIdentitiesRequest,
    responses(
        (status = 200, body = crate::paging::Page<String>),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn find_unmatched_identities(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(body): Json<FindUnmatchedIdentitiesRequest>,
) -> Result<Json<Page<String>>, ApiError> {
    if body.identifiers.len() > MAX_MATCH_IDENTIFIERS {
        return Err(ApiError::validation(format!(
            "at most {MAX_MATCH_IDENTIFIERS} identifiers"
        )));
    }
    let mut conn = state.db.acquire().await?;
    let unknown =
        unknown_contact_identifiers(&mut conn, auth.account_id, &body.identifiers).await?;
    Ok(Json(whole_page(unknown, MAX_MATCH_IDENTIFIERS)))
}

/// Page through the account's contacts (id, name, handles, groups).
#[utoipa::path(
    get,
    path = "/v1/contacts",
    tag = "Contacts",
    security(("session" = [])),
    params(
        ("q" = Option<String>, Query, description = "Contact search; empty lists all"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, max 50000"),
        ("sort" = Option<String>, Query, description = "Comma-separated keys from `name` and `last_heard`, a leading `-` for descending. `last_heard` is when the vault last heard from the contact; contacts it never heard from sort last either way. Default `name`.")
    ),
    responses(
        (status = 200, body = Page<ContactSummary>),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn list_contacts(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<ContactSummary>>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let list = ListRequest::read(
        &mut conn,
        auth.account_id,
        query,
        &CONTACT_SORT_KEYS,
        &DEFAULT_CONTACT_SORT,
    )
    .await?;
    let result = list_contacts_sorted(
        &mut conn,
        auth.account_id,
        &list.q,
        &list.order,
        list.page.limit,
        list.page.offset,
        list.clock,
    )
    .await?;
    Ok(Json(result))
}

/// First/last message dates and counts for a list of contact ids.
#[utoipa::path(
    post,
    path = "/v1/contacts/summaries",
    tag = "Contacts",
    security(("session" = [])),
    request_body = SummarizeContactsRequest,
    responses(
        (status = 200, body = crate::paging::Page<ContactSelectionSummary>),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem)
    )
)]
pub(crate) async fn summarize_contacts(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(body): Json<SummarizeContactsRequest>,
) -> Result<Json<Page<ContactSelectionSummary>>, ApiError> {
    if body.ids.len() > MAX_CONTACT_SUMMARY_IDS {
        return Err(ApiError::validation(format!(
            "at most {MAX_CONTACT_SUMMARY_IDS} contact ids"
        )));
    }
    let mut conn = state.db.acquire().await?;
    let items = get_contact_summaries(&mut conn, auth.account_id, &body.ids).await?;
    Ok(Json(whole_page(items, MAX_CONTACT_SUMMARY_IDS)))
}

/// Full contact view: per-handle services, message stats, and group
/// memberships.
#[utoipa::path(
    get,
    path = "/v1/contacts/{id}",
    tag = "Contacts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 200, body = Contact),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn get_contact(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<Json<Contact>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let detail = get_contact_detail(&mut conn, auth.account_id, contact_id).await?;
    detail
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("contact not found".into()))
}

/// Rename a contact or change its linked handles.
#[utoipa::path(
    patch,
    path = "/v1/contacts/{id}",
    tag = "Contacts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    request_body = UpdateContactRequest,
    responses(
        (status = 200, body = Contact),
        (status = 400, body = crate::problem::Problem),
        (status = 422, body = crate::problem::Problem),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn update_contact(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
    Json(body): Json<UpdateContactRequest>,
) -> Result<Json<Contact>, ApiError> {
    let mut conn = state.db.acquire().await?;
    match mutate_contact(&mut conn, auth.account_id, contact_id, &body).await {
        Ok(false) => Err(ApiError::NotFound("contact not found".into())),
        Err(e) => Err(e.into()),
        Ok(true) => get_contact_detail(&mut conn, auth.account_id, contact_id)
            .await?
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("contact missing after mutate")))
            .map(Json),
    }
}

/// Put a contact in the trash. Idempotent: trashing an already-trashed
/// contact still answers 204.
#[utoipa::path(
    post,
    path = "/v1/contacts/{id}/trash",
    tag = "Contacts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 204, description = "Trashed"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn trash_contact(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    if move_to_trash(&mut conn, auth.account_id, Trashable::Contact(contact_id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("contact not found".into()))
    }
}

/// Take a contact out of the trash. Idempotent: restoring a contact that
/// was not trashed still answers 204.
#[utoipa::path(
    post,
    path = "/v1/contacts/{id}/restore",
    tag = "Contacts",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 204, description = "Restored"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem)
    )
)]
pub(crate) async fn restore_contact(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    if restore(&mut conn, auth.account_id, Trashable::Contact(contact_id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound("contact not found".into()))
    }
}

/// Delete a trashed contact the way a phone's Delete Contact does: the name
/// and the person's edits go, the contact becomes Unknown again and leaves
/// the trash, and every conversation it was in stays as it is, showing the
/// handle. Conversations are never deleted with a contact. A contact that is
/// not in the trash answers 409.
#[utoipa::path(
    delete,
    path = "/v1/contacts/{id}",
    tag = "Contacts",
    security(("session" = ["delete"])),
    params(("id" = i64, Path, description = "Contact id")),
    responses(
        (status = 204, description = "Deleted: the contact is Unknown again"),
        (status = 401, body = crate::problem::Problem),
        (status = 403, body = crate::problem::Problem),
        (status = 404, body = crate::problem::Problem),
        (status = 409, body = crate::problem::Problem, description = "The contact is not in the trash")
    )
)]
pub(crate) async fn delete_contact(
    State(state): State<AppState>,
    FullDeleteAccess(auth): FullDeleteAccess,
    AxumPath(contact_id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    match delete_trashed(&mut conn, auth.account_id, Trashable::Contact(contact_id)).await? {
        // A contact owns no files, so there is nothing to remove from disk.
        DeleteOutcome::Deleted(_) => Ok(StatusCode::NO_CONTENT),
        DeleteOutcome::NotOwned => Err(ApiError::NotFound("contact not found".into())),
        DeleteOutcome::NotTrashed => Err(ApiError::StateConflict(
            "the contact is not in the trash; move it to the trash first".into(),
        )),
    }
}

#[cfg(test)]
mod tests;
