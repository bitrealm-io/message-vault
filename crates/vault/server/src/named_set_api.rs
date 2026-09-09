//! One HTTP surface for Contact Groups and Message Tags.
//!
//! Both are a named set the account owns plus a membership of contact or
//! conversation ids. The request and response types and the six operations
//! live here once, over [`MembershipSpec`]; the `named_set_routes!` macro
//! stamps out both collections' twelve route handlers from them, one
//! `#[utoipa::path]` function per route, because utoipa needs a concrete
//! function with literal strings to describe each route and cannot see
//! through a generic or a `concat!`. The two invocations below name every
//! path, so both collections' routes stay greppable here.

use crate::extract::{Json, Query};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::named_membership::{self, MembershipSpec};
use crate::paging::{DEFAULT_LIST_LIMIT, Page, PageQuery, page_of, page_params};
use crate::server::{ApiError, AppState, Created, FullAccess};

/// One Contact Group or Message Tag: its id and name.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct NamedSet {
    pub(crate) id: i64,
    pub(crate) name: String,
}

/// A name to create, or the new name for an existing set.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct NamedSetBody {
    pub(crate) name: String,
}

/// Members to put in and take out of one set, in one request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub(crate) struct MembersPatch {
    #[serde(default)]
    pub(crate) add: Vec<i64>,
    #[serde(default)]
    pub(crate) remove: Vec<i64>,
}

/// How many memberships a patch created and how many it removed.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct MembersChanged {
    pub(crate) added: u64,
    pub(crate) removed: u64,
}

/// The account's sets of this kind, A–Z. Never fails on its own; errors here
/// come from the database connection (500).
pub(crate) async fn list(
    spec: &'static MembershipSpec,
    state: &AppState,
    account_id: i64,
    query: PageQuery,
) -> Result<Json<Page<NamedSet>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let mut conn = state.db.acquire().await?;
    let rows: Vec<NamedSet> = named_membership::list_sets(spec, &mut conn, account_id)
        .await?
        .into_iter()
        .map(|(id, name)| NamedSet { id, name })
        .collect();
    Ok(Json(page_of(rows, params)))
}

/// Create a set and answer `201 Created` with its id and trimmed name, and a
/// `Location` of `{root_path}/{id}`. A blank or over-long name, or a reserved
/// name, answers 400; a name already taken (ignoring case) answers 409.
pub(crate) async fn create(
    spec: &'static MembershipSpec,
    root_path: &str,
    state: &AppState,
    account_id: i64,
    body: NamedSetBody,
) -> Result<Created<NamedSet>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let (id, name) = named_membership::create_set(spec, &mut conn, account_id, &body.name).await?;
    Ok(Created {
        location: format!("{root_path}/{id}"),
        body: NamedSet { id, name },
    })
}

/// Rename a set by id, answering its id and the new name. An unknown or
/// another account's id answers 404; a blank, over-long, or reserved name
/// answers 400; another set's name (ignoring case) answers 409.
pub(crate) async fn update(
    spec: &'static MembershipSpec,
    state: &AppState,
    account_id: i64,
    id: i64,
    body: NamedSetBody,
) -> Result<Json<NamedSet>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let name = named_membership::rename_set(spec, &mut conn, account_id, id, &body.name).await?;
    Ok(Json(NamedSet { id, name }))
}

/// Delete a set by id and its memberships, answering 204. An unknown or
/// another account's id answers 404.
pub(crate) async fn delete(
    spec: &'static MembershipSpec,
    state: &AppState,
    account_id: i64,
    id: i64,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.db.acquire().await?;
    named_membership::delete_set(spec, &mut conn, account_id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Member ids of one set, ascending. An unknown or another account's id
/// answers 404.
pub(crate) async fn members_list(
    spec: &'static MembershipSpec,
    state: &AppState,
    account_id: i64,
    id: i64,
    query: PageQuery,
) -> Result<Json<Page<i64>>, ApiError> {
    let params = page_params(query.limit, query.offset, DEFAULT_LIST_LIMIT, None)?;
    let mut conn = state.db.acquire().await?;
    let rows = named_membership::list_member_ids_of(spec, &mut conn, account_id, id).await?;
    Ok(Json(page_of(rows, params)))
}

/// Add and remove members of one set in one call, answering how many
/// changed. An unknown or another account's set id, or an unknown member id
/// in `add` or `remove`, answers 404; an empty patch answers 400.
pub(crate) async fn members_update(
    spec: &'static MembershipSpec,
    state: &AppState,
    account_id: i64,
    id: i64,
    body: MembersPatch,
) -> Result<Json<MembersChanged>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let (added, removed) =
        named_membership::patch_members(spec, &mut conn, account_id, id, &body.add, &body.remove)
            .await?;
    Ok(Json(MembersChanged { added, removed }))
}

/// One collection's six HTTP handlers.
///
/// Contact Groups and Message Tags are the same six operations over
/// [`MembershipSpec`]; what differs is the paths, the tag, the noun in the
/// prose, and which spec function to call. utoipa needs a concrete function
/// per route with literal strings in its attribute — it cannot describe a
/// generic, and it cannot see through `concat!` — so the handlers are stamped
/// out here rather than written twice.
///
/// The function names and doc comments are load-bearing: `operationId` comes
/// from the name and `summary` from the first line of the doc, so both appear
/// in `docs/src/assets/openapi.json`.
///
/// Adding a third collection is this macro invoked a third time, plus a
/// `MembershipSpec` for it in `named_membership.rs` and six
/// `.routes(routes!(..))` lines in `openapi.rs` — `utoipa_axum` needs each
/// route named there and that cannot be folded in here.
///
/// The formatting is pinned because rustfmt reindents an attribute body
/// inside a macro arm to three times the depth of the code around it.
#[rustfmt::skip]
macro_rules! named_set_routes {
    (
        spec: $spec:path,
        tag: $tag:literal,
        id_description: $id_description:literal,
        root_path: $root_path:literal,
        id_path: $id_path:literal,
        members_path: $members_path:literal,
        list: $list_fn:ident, $list_doc:literal,
        create: $create_fn:ident, $create_doc:literal,
        update: $update_fn:ident, $update_doc:literal,
        delete: $delete_fn:ident, $delete_doc:literal,
        members_list: $members_list_fn:ident, $members_list_doc:literal,
        members_update: $members_update_fn:ident, $members_update_doc:literal,
    ) => {
        #[doc = $list_doc]
        #[utoipa::path(
            get,
            path = $root_path,
            tag = $tag,
            security(("session" = [])),
            params(
                ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
                ("offset" = Option<usize>, Query, description = "Page offset")
            ),
            responses(
                (status = 200, body = crate::paging::Page<NamedSet>),
                (status = 401, body = crate::problem::Problem),
                (status = 403, body = crate::problem::Problem)
            )
        )]
        pub(crate) async fn $list_fn(
            axum::extract::State(state): axum::extract::State<AppState>,
            FullAccess(auth): FullAccess,
            Query(query): Query<PageQuery>,
        ) -> Result<Json<Page<NamedSet>>, ApiError> {
            list($spec(), &state, auth.account_id, query).await
        }

        #[doc = $create_doc]
        #[utoipa::path(
            post,
            path = $root_path,
            tag = $tag,
            security(("session" = [])),
            request_body = NamedSetBody,
            responses(
                (
                    status = 201,
                    body = NamedSet,
                    headers(("Location" = String, description = "Path of the new set"))
                ),
                (status = 400, body = crate::problem::Problem),
                (status = 401, body = crate::problem::Problem),
                (status = 403, body = crate::problem::Problem),
                (status = 409, body = crate::problem::Problem)
            )
        )]
        pub(crate) async fn $create_fn(
            axum::extract::State(state): axum::extract::State<AppState>,
            FullAccess(auth): FullAccess,
            Json(body): Json<NamedSetBody>,
        ) -> Result<Created<NamedSet>, ApiError> {
            create($spec(), $root_path, &state, auth.account_id, body).await
        }

        #[doc = $update_doc]
        #[utoipa::path(
            patch,
            path = $id_path,
            tag = $tag,
            security(("session" = [])),
            params(("id" = i64, Path, description = $id_description)),
            request_body = NamedSetBody,
            responses(
                (status = 200, body = NamedSet),
                (status = 400, body = crate::problem::Problem),
                (status = 422, body = crate::problem::Problem),
                (status = 401, body = crate::problem::Problem),
                (status = 403, body = crate::problem::Problem),
                (status = 404, body = crate::problem::Problem),
                (status = 409, body = crate::problem::Problem)
            )
        )]
        pub(crate) async fn $update_fn(
            axum::extract::State(state): axum::extract::State<AppState>,
            FullAccess(auth): FullAccess,
            crate::extract::Path(id): crate::extract::Path<i64>,
            Json(body): Json<NamedSetBody>,
        ) -> Result<Json<NamedSet>, ApiError> {
            update($spec(), &state, auth.account_id, id, body).await
        }

        #[doc = $delete_doc]
        #[utoipa::path(
            delete,
            path = $id_path,
            tag = $tag,
            security(("session" = [])),
            params(("id" = i64, Path, description = $id_description)),
            responses(
                (status = 204),
                (status = 401, body = crate::problem::Problem),
                (status = 403, body = crate::problem::Problem),
                (status = 404, body = crate::problem::Problem)
            )
        )]
        pub(crate) async fn $delete_fn(
            axum::extract::State(state): axum::extract::State<AppState>,
            FullAccess(auth): FullAccess,
            crate::extract::Path(id): crate::extract::Path<i64>,
        ) -> Result<StatusCode, ApiError> {
            delete($spec(), &state, auth.account_id, id).await
        }

        #[doc = $members_list_doc]
        #[utoipa::path(
            get,
            path = $members_path,
            tag = $tag,
            security(("session" = [])),
            params(
                ("id" = i64, Path, description = $id_description),
                ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
                ("offset" = Option<usize>, Query, description = "Page offset")
            ),
            responses(
                (status = 200, body = crate::paging::Page<i64>),
                (status = 401, body = crate::problem::Problem),
                (status = 403, body = crate::problem::Problem),
                (status = 404, body = crate::problem::Problem)
            )
        )]
        pub(crate) async fn $members_list_fn(
            axum::extract::State(state): axum::extract::State<AppState>,
            FullAccess(auth): FullAccess,
            crate::extract::Path(id): crate::extract::Path<i64>,
            Query(query): Query<PageQuery>,
        ) -> Result<Json<Page<i64>>, ApiError> {
            members_list($spec(), &state, auth.account_id, id, query).await
        }

        #[doc = $members_update_doc]
        #[utoipa::path(
            patch,
            path = $members_path,
            tag = $tag,
            security(("session" = [])),
            params(("id" = i64, Path, description = $id_description)),
            request_body = MembersPatch,
            responses(
                (status = 200, body = MembersChanged),
                (status = 400, body = crate::problem::Problem),
                (status = 422, body = crate::problem::Problem),
                (status = 401, body = crate::problem::Problem),
                (status = 403, body = crate::problem::Problem),
                (status = 404, body = crate::problem::Problem)
            )
        )]
        pub(crate) async fn $members_update_fn(
            axum::extract::State(state): axum::extract::State<AppState>,
            FullAccess(auth): FullAccess,
            crate::extract::Path(id): crate::extract::Path<i64>,
            Json(body): Json<MembersPatch>,
        ) -> Result<Json<MembersChanged>, ApiError> {
            members_update($spec(), &state, auth.account_id, id, body).await
        }
    };
}

named_set_routes! {
    spec: crate::named_membership::group_spec,
    tag: "Contacts",
    id_description: "Contact Group id",
    root_path: "/v1/contact-groups",
    id_path: "/v1/contact-groups/{id}",
    members_path: "/v1/contact-groups/{id}/members",
    list: contact_groups_list, "The account's Contact Groups, A–Z.",
    create: contact_groups_create, "Create a Contact Group.",
    update: contact_groups_update, "Rename a Contact Group.",
    delete: contact_groups_delete, "Delete a Contact Group and its memberships.",
    members_list: contact_group_members_list, "Contact ids in one Contact Group.",
    members_update: contact_group_members_update,
        "Put contacts in and take contacts out of one Contact Group.",
}

named_set_routes! {
    spec: crate::named_membership::tag_spec,
    tag: "Message tags",
    id_description: "Message Tag id",
    root_path: "/v1/message-tags",
    id_path: "/v1/message-tags/{id}",
    members_path: "/v1/message-tags/{id}/members",
    list: message_tags_list, "The account's Message Tags, A–Z.",
    create: message_tags_create, "Create a Message Tag.",
    update: message_tags_update, "Rename a Message Tag.",
    delete: message_tags_delete, "Delete a Message Tag and its memberships.",
    members_list: message_tag_members_list, "Conversation ids in one Message Tag.",
    members_update: message_tag_members_update,
        "Put conversations in and take conversations out of one Message Tag.",
}

#[cfg(test)]
mod tests;
