//! `POST /v1/contacts`: load a vCard or vCard CSV address book into the
//! account. The file is read by `db::contacts::read_address_book` and written
//! by `db::contacts::replace_address_book`; this module checks the upload and
//! hands it over.

use axum::extract::{Request, State};
use serde::Serialize;

use crate::db::contacts;
use crate::extract::Json;
use crate::server::{ApiError, AppState, FullAccess, content_type_base, read_body_limited};

/// Largest address book the load route accepts, in bytes.
///
/// A phone's contacts export is measured in tens of kilobytes; a few megabytes
/// is already far past any real address book, and the whole file is read into
/// memory before parsing. The route reads the body itself against this cap,
/// as the asset routes do: Axum's `Bytes` extractor would stop at its own
/// 2 MiB default first, and a cap that never answers is no cap.
pub(crate) const MAX_ADDRESS_BOOK_BYTES: usize = 8 * 1024 * 1024;

/// What loading an address book changed.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct CreateContactsResponse {
    /// Contacts written from the file.
    pub contacts: u64,
    /// Phone identities linked to those contacts.
    pub phones: u64,
    /// Identities written with a review note (an ambiguous number).
    pub phones_needing_review: u64,
}

/// Load a VCF or vCard CSV address book into this account. The body is the
/// file itself, and `Content-Type` says which: `text/vcard` or `text/csv`.
///
/// This is a standalone act against the vault, never part of an Import Run:
/// contacts are vault state, and a person may load them before or after
/// bringing messages in. Only the rows the address book owns are replaced, so
/// Contact Groups, names the person typed, and identities an import discovered
/// all survive. How the file is read is the open question in #270; this route
/// is where that answer lands.
#[utoipa::path(
    post,
    path = "/v1/contacts",
    tag = "Contacts",
    security(("session" = [])),
    request_body(
        content(
            ("text/vcard"),
            ("text/csv")
        ),
        description = "The address book file: a vCard file as text/vcard, or a vCard CSV export as text/csv."
    ),
    responses(
        (status = 200, body = CreateContactsResponse),
    )
)]
pub(crate) async fn create_contacts(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    request: Request,
) -> Result<Json<CreateContactsResponse>, ApiError> {
    let Some(name) = address_book_file_name(content_type_base(request.headers())) else {
        return Err(ApiError::UnsupportedMediaType(
            "Content-Type must be text/vcard or text/csv".into(),
        ));
    };
    let body = read_body_limited(request.into_body(), MAX_ADDRESS_BOOK_BYTES).await?;
    let content = std::str::from_utf8(&body)
        .map_err(|_| ApiError::MalformedBody("address book is not UTF-8 text".into()))?;
    if content.trim().is_empty() {
        return Err(ApiError::validation("address book is empty"));
    }
    // The loader detects VCF versus vCard CSV from the path, so the upload is
    // written to a temp file under the name its media type earns.
    let dir = tempfile::tempdir()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("create temp dir: {e}")))?;
    let path = dir.path().join(name);
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("write address book: {e}")))?;

    // A file that does not parse is a body that broke a rule, `422`; only a
    // failed write after it parsed is the vault's own fault. The reader names
    // the file it read, which here is the vault's temp copy: the caller is
    // told about "the upload" instead.
    let book = contacts::read_address_book(&path).map_err(|e| {
        let reason = format!("{e:#}").replace(&path.display().to_string(), "the upload");
        ApiError::validation(format!("the address book could not be read: {reason}"))
    })?;
    let mut conn = state.db.acquire().await?;
    let stats = contacts::replace_address_book(&mut conn, auth.account_id, book)
        .await
        .map_err(|e| ApiError::Internal(e.context("load address book")))?;
    Ok(Json(CreateContactsResponse {
        contacts: stats.contacts,
        phones: stats.phones,
        phones_needing_review: stats.phones_needing_review,
    }))
}

/// The temp file name a media type earns, so the loader's extension check
/// reads the format the client declared. `text/x-vcard` is the older
/// spelling some exporters still write.
pub(super) fn address_book_file_name(content_type: Option<&str>) -> Option<&'static str> {
    match content_type.map(str::to_ascii_lowercase).as_deref() {
        Some("text/vcard" | "text/x-vcard") => Some("address-book.vcf"),
        Some("text/csv") => Some("address-book.csv"),
        _ => None,
    }
}
