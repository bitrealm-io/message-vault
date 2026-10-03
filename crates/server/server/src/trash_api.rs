//! Empty Trash (`DELETE /v1/trash`).
//!
//! The database work lives in [`crate::db::trash`], and removing the files
//! it reported as unreferenced lives in [`crate::asset_store`], which
//! `DELETE /v1/conversations/{id}` shares.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;

use crate::asset_store;
use crate::db::audit_trail::AuditActor;
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
pub(crate) async fn delete_trash(
    State(state): State<AppState>,
    FullDeleteAccess(auth): FullDeleteAccess,
) -> Result<StatusCode, ApiError> {
    let unreferenced = {
        let mut conn = state.db.acquire().await?;
        let emptied = trash::empty_trash(&mut conn, auth.account_id, AuditActor::Holder).await?;
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
