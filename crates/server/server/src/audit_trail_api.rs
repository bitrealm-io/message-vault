//! The Audit Trail: `GET /v1/audit-trail`, every account's entries for the
//! owner, beside `GET /v1/accounts/{id}/audit-trail` in `accounts_api`, one
//! account's for the owner and for that account. Both answer from
//! [`audit_trail_page`], so the two cannot differ.
//!
//! The trail says who did what and when, and how much: never what a message
//! said or which conversation it was in, so the owner reads all of it
//! (`docs/adr/0008-the-owner-holds-no-messages.md`). Nobody edits or deletes
//! an entry, and an account's entries outlive it
//! (`docs/adr/0020-the-audit-trail-outlives-the-account.md`).

use axum::extract::State;
use serde::Deserialize;

use crate::db::audit_trail::{self, AuditEntry, Scope};
use crate::extract::{Json, Query};
use crate::paging::{DEFAULT_LIST_LIMIT, MAX_LIST_OFFSET, Page, page_params};
use crate::server::{ApiError, AppState, Owner};

/// Query string of the two Audit Trail lists.
#[derive(Debug, Deserialize)]
pub(crate) struct ListAuditTrailQuery {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
}

/// One page of the Audit Trail over `scope`, newest first.
pub(crate) async fn audit_trail_page(
    state: &AppState,
    scope: Scope,
    query: ListAuditTrailQuery,
) -> Result<Json<Page<AuditEntry>>, ApiError> {
    let params = page_params(
        query.limit,
        query.offset,
        DEFAULT_LIST_LIMIT,
        Some(MAX_LIST_OFFSET),
    )?;
    let mut conn = state.db.acquire().await?;
    let (items, total) = audit_trail::page(&mut conn, scope, params.limit, params.offset).await?;
    Ok(Json(Page {
        items,
        total,
        limit: params.limit,
        offset: params.offset,
    }))
}

/// Every account's Audit Trail, newest first: logins, sessions ending,
/// refused logins, Import Runs, Export Runs, and the owner's and holders'
/// changes to accounts, including those of deleted accounts under their old
/// usernames. The owner's alone.
#[utoipa::path(
    get,
    path = "/v1/audit-trail",
    tag = "Audit Trail",
    security(("session" = ["owner"])),
    params(
        ("limit" = Option<usize>, Query, description = "Page size, default 40, at most 500"),
        ("offset" = Option<usize>, Query, description = "Rows to skip, at most 50000")
    ),
    responses(
        (status = 200, body = Page<AuditEntry>),
        crate::problem::openapi::NotTheOwner
    )
)]
pub(crate) async fn list_audit_trail(
    State(state): State<AppState>,
    Owner(_auth): Owner,
    Query(query): Query<ListAuditTrailQuery>,
) -> Result<Json<Page<AuditEntry>>, ApiError> {
    audit_trail_page(&state, Scope::All, query).await
}

#[cfg(test)]
mod tests;
