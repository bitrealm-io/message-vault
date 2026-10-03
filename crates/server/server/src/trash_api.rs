//! Empty Trash (`DELETE /v1/trash`).
//!
//! The database work lives in [`crate::db::trash`], and removing the files
//! it reported as unreferenced lives in [`crate::asset_store`], which
//! `DELETE /v1/conversations/{id}` shares.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;

use crate::asset_store;
use crate::db::audit_trail::{self, AuditAction, AuditActor, Details};
use crate::db::trash;
use crate::server::{ApiError, AppState, FullDeleteAccess};

/// Empty the trash: every trashed conversation is deleted for good, with
/// its messages and any attachment file no other message uses, and every
/// trashed contact loses its name and details and becomes Unknown, its
/// conversations untouched. Trash is the only door to permanent deletion;
/// this is the door for everything in it at once.
#[utoipa::path(
    delete,
    path = "/v1/trash",
    tag = "Trash",
    security(("session" = ["delete"])),
    responses(
        (status = 204, description = "Trash emptied"),
    )
)]
pub(crate) async fn empty_trash(
    State(state): State<AppState>,
    FullDeleteAccess(auth): FullDeleteAccess,
) -> Result<StatusCode, ApiError> {
    let unreferenced = {
        let mut conn = state.db.acquire().await?;
        let emptied = trash::empty_trash(&mut conn, auth.account_id).await?;
        let count = |n: usize| Some(i64::try_from(n).unwrap_or(i64::MAX));
        let details = Details {
            conversations: count(emptied.conversations),
            contacts: count(emptied.contacts),
            ..Details::default()
        };
        audit_trail::record_about(
            &mut conn,
            AuditAction::TrashEmptied,
            AuditActor::Holder,
            auth.account_id,
            details,
        )
        .await?;
        emptied.orphaned
    };
    asset_store::remove_unreferenced(
        &state.db,
        Arc::clone(&state.cfg),
        auth.account_id,
        unreferenced,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
