//! The address book routes: `POST /v1/contacts` loads Message Crate's own
//! CSV into the account, and `POST /v1/contacts/address-book` writes it. The
//! file's rules and both directions are in `db::address_book`; this module
//! checks the request and hands it over.

use std::collections::HashSet;

use axum::extract::{Request, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::db::address_book::{self, LoadCounts, LoadError, LoadMode};
use crate::db::sql::bind_args;
use crate::extract::{Json, Query};
use crate::server::{ApiError, AppState, FullAccess, content_type_base, read_body_limited};

/// Largest address book the load route accepts, in bytes.
///
/// A row is under a hundred bytes, so a few megabytes is already tens of
/// thousands of identities, and the whole file is read into memory before
/// parsing. The route reads the body itself against this cap, as the asset
/// routes do, and not through `crate::extract::Json`, whose cap is
/// [`crate::server::MAX_JSON_BODY_BYTES`].
pub(crate) const MAX_ADDRESS_BOOK_BYTES: usize = 8 * 1024 * 1024;

/// The file name Export gives the address book.
const EXPORT_FILE_NAME: &str = "address-book.csv";

/// The query of `POST /v1/contacts`.
#[derive(Debug, Deserialize)]
pub(crate) struct LoadQuery {
    #[serde(default)]
    mode: LoadMode,
}

impl From<LoadError> for ApiError {
    fn from(error: LoadError) -> Self {
        match error {
            LoadError::Refused(reasons) => Self::ValidationFailed(reasons),
            LoadError::Failed(cause) => Self::Internal(cause.context("load address book")),
        }
    }
}

/// Load an address book into this account. The body is the file itself:
/// Message Crate's own CSV, one row per identity, with the columns
/// `contact_id`, `display_name`, `groups`, `service`, `identity_type` and
/// `identity`, as `POST /v1/contacts/address-book` writes it.
///
/// Rows that share a `contact_id` are one contact. An id the account holds
/// names that contact, a blank id makes a new contact for that row, and any
/// other text groups its rows into one new contact. `append` creates and
/// renames contacts and adds the identities and Contact Group memberships
/// the rows list. `edit` does the same and then takes off each contact in
/// the file every identity and membership its rows do not list. A contact
/// the file does not mention is left alone in both modes.
///
/// A phone number written without `+`, which a spreadsheet can save in place
/// of `+6591234567`, names the identity `+` and its digits when the row's
/// contact holds that key. Otherwise it is keyed as written: ten digits as a
/// US number, any other count as bare digits. `notes` names each row read
/// with its `+` back, and each such number that became a new identity.
///
/// One `'` before a cell that starts with `=`, `+`, `-`, `@`, a tab or a
/// carriage return is taken off, in any column, which undoes the `'` the
/// export writes there.
///
/// The load is one transaction. A file that breaks a rule is refused whole
/// with `422 Unprocessable Entity`, and `errors` holds one sentence for each
/// bad row, starting with its row number.
#[utoipa::path(
    post,
    path = "/v1/contacts",
    tag = "Contacts",
    security(("session" = [])),
    params(
        ("mode" = Option<LoadMode>, Query, description = "How the file is applied: `append` (the default) removes nothing; `edit` makes each contact in the file hold exactly the identities and Contact Group memberships its rows list.")
    ),
    request_body(
        content_type = "text/csv",
        content = String,
        description = "The address book: Message Crate's own CSV, one row per identity."
    ),
    responses(
        (status = 200, body = LoadCounts),
    )
)]
pub(crate) async fn create_contacts(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<LoadQuery>,
    request: Request,
) -> Result<Json<LoadCounts>, ApiError> {
    if !content_type_base(request.headers())
        .is_some_and(|base| base.eq_ignore_ascii_case("text/csv"))
    {
        return Err(ApiError::UnsupportedMediaType(
            "Content-Type must be text/csv".into(),
        ));
    }
    let body = read_body_limited(request.into_body(), MAX_ADDRESS_BOOK_BYTES).await?;
    let content = std::str::from_utf8(&body)
        .map_err(|_| ApiError::MalformedBody("address book is not UTF-8 text".into()))?;
    if content.trim().is_empty() {
        return Err(ApiError::validation("address book is empty"));
    }
    let mut conn = state.db.acquire().await?;
    let counts = address_book::load(&mut conn, auth.account_id, content, query.mode).await?;
    Ok(Json(counts))
}

/// Which contacts `POST /v1/contacts/address-book` writes: the Contacts
/// list's search and its checked rows.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub(crate) struct ExportAddressBookRequest {
    /// A Contacts search, as `GET /v1/contacts` takes in `q`. Absent or
    /// empty matches every contact.
    #[serde(default)]
    q: Option<String>,
    /// Contact ids to keep from what `q` matches. Absent or empty keeps
    /// them all.
    #[serde(default)]
    ids: Vec<i64>,
}

/// Write the address book for the contacts a search matches, the checked
/// ones among them, or every contact when the body names neither.
///
/// The answer is `text/csv`, not JSON: one row per identity, with the
/// columns `contact_id`, `display_name`, `groups`, `service`, `identity_type`
/// and `identity`. A contact's name and its Contact Group names, separated
/// by `;`, repeat on each of its rows. A contact with no name has a blank
/// `display_name`, and a contact with no identity is one row with the last
/// three columns blank. Contacts in the trash are left out.
///
/// A cell that starts with `=`, `+`, `-`, `@`, a tab or a carriage return
/// is written with a `'` in front, in every column, because a spreadsheet
/// runs such a cell as a formula and drops the `+` of a phone number.
/// `POST /v1/contacts` loads the file back and takes that `'` off.
#[utoipa::path(
    post,
    path = "/v1/contacts/address-book",
    tag = "Contacts",
    security(("session" = [])),
    request_body = ExportAddressBookRequest,
    responses(
        (
            status = 200,
            description = "The address book, as an attachment named `address-book.csv`",
            content_type = "text/csv",
            body = String,
            headers(("Content-Disposition" = String, description = "`attachment; filename=\"address-book.csv\"`"))
        ),
        crate::problem::openapi::SearchQueryInvalid
    )
)]
pub(crate) async fn export_address_book(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Json(body): Json<ExportAddressBookRequest>,
) -> Result<Response, ApiError> {
    let mut conn = state.db.acquire().await?;
    let q = body.q.as_deref().unwrap_or("").trim();
    let only: Option<HashSet<i64>> = if q.is_empty() {
        (!body.ids.is_empty()).then(|| body.ids.iter().copied().collect())
    } else {
        // The same compile the Contacts list runs, so the file holds the
        // rows the person was looking at.
        let (zone, today) =
            crate::db::account_profile::account_clock(&mut conn, auth.account_id).await?;
        let filter = crate::search::compile(crate::search::CompileRequest {
            list: crate::search::ListKind::Contacts,
            query: q,
            account_id: auth.account_id,
            today,
            zone,
        })?;
        let sql = format!("SELECT ct.id FROM contacts ct WHERE {}", filter.where_sql());
        let matched: Vec<i64> = sqlx::query_scalar_with(&sql, bind_args(filter.params()))
            .fetch_all(&mut *conn)
            .await?;
        let matched: HashSet<i64> = matched.into_iter().collect();
        Some(if body.ids.is_empty() {
            matched
        } else {
            body.ids
                .iter()
                .copied()
                .filter(|id| matched.contains(id))
                .collect()
        })
    };
    let csv = address_book::export_csv(&mut conn, auth.account_id, only.as_ref()).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{EXPORT_FILE_NAME}\""),
            ),
        ],
        csv,
    )
        .into_response())
}
