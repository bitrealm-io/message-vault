//! OpenAPI document for message-vault-server HTTP routes.

use std::io::Write;
use std::path::Path;

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::server::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Message Vault HTTP API",
        description = "HTTP API for a local Message Vault. Bearer session tokens come from login. API tokens come from Settings → Account.",
        license(
            name = "Fair Core License 1.0 (ALv2 future)",
            url = "https://github.com/bitrealm-io/message-vault/blob/main/LICENSE.md"
        ),
        version = env!("CARGO_PKG_VERSION")
    ),
    modifiers(&BearerAddon),
    components(schemas(crate::search::ListKind)),
    tags(
        (name = "Health", description = "Process liveness"),
        (name = "Auth", description = "Sign-in, session, and token check"),
        (name = "Account", description = "Profile, storage, and API tokens"),
        (name = "Import", description = "JSONL import sessions and ingest"),
        (name = "Export", description = "Read-only messages and counts"),
        (name = "Assets", description = "Attachment bytes"),
        (name = "Contacts", description = "Address book and contact groups"),
        (name = "Conversations", description = "Conversation list and sources"),
        (name = "Trash", description = "Empty the trash; the one door to permanent deletion, with DELETE on a trashed conversation or contact"),
        (name = "Message tags", description = "Tags on conversations"),
        (name = "Search", description = "The words the search language accepts"),
        (name = "Owner", description = "Account management for the vault owner"),
        (name = "Vault", description = "The vault's own state: claiming it, and what a signed-out visitor may do")
    )
)]
/// OpenAPI document definition assembled from the utoipa-annotated handlers.
pub struct ApiDoc;

struct BearerAddon;

impl Modify for BearerAddon {
    /// Register the `bearer` security scheme on the generated document.
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_default();
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
        );
    }
}

/// Unauthenticated auth JSON (register and login).
pub fn auth_public_openapi() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(crate::auth::register_handler))
        .routes(routes!(crate::auth::login_handler))
        .routes(routes!(crate::vault_api::vault_state_handler))
        .routes(routes!(crate::vault_api::claim_vault_handler))
}

/// Health, session-backed auth, account settings, and browse routes.
pub fn api_openapi() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(crate::server::health))
        .routes(routes!(crate::auth::auth_check))
        .routes(routes!(crate::auth::logout_handler))
        .routes(routes!(crate::profile::change_password_handler))
        .routes(routes!(crate::profile::delete_account_handler))
        .routes(routes!(crate::profile::account_profile_handler))
        .routes(routes!(crate::profile::account_profile_update_handler))
        .routes(routes!(crate::profile::delete_messages_handler))
        .routes(routes!(crate::profile::account_storage_handler))
        .routes(routes!(crate::api_tokens_api::list_api_tokens_handler))
        .routes(routes!(crate::api_tokens_api::create_api_token_handler))
        .routes(routes!(crate::api_tokens_api::delete_api_token_handler))
        .routes(routes!(crate::api_tokens_api::rename_api_token_handler))
        .routes(routes!(crate::export_api::export_messages_handler))
        .routes(routes!(crate::export_api::export_messages_count_handler))
        .routes(routes!(crate::contacts_api::contacts_list_handler))
        .routes(routes!(crate::contacts_api::contact_summaries_handler))
        .routes(routes!(crate::contacts_api::contact_detail_handler))
        .routes(routes!(crate::contacts_api::contact_mutate_handler))
        .routes(routes!(crate::contacts_api::contact_trash_handler))
        .routes(routes!(crate::contacts_api::contact_restore_handler))
        .routes(routes!(crate::contacts_api::contact_delete_handler))
        .routes(routes!(crate::contacts_api::unmatched_handles_handler))
        .routes(routes!(crate::contacts_api::contacts_create_handler))
        .routes(routes!(crate::named_set_api::contact_groups_list))
        .routes(routes!(crate::named_set_api::contact_groups_create))
        .routes(routes!(crate::named_set_api::contact_groups_update))
        .routes(routes!(crate::named_set_api::contact_groups_delete))
        .routes(routes!(crate::named_set_api::contact_group_members_list))
        .routes(routes!(crate::named_set_api::contact_group_members_update))
        .routes(routes!(crate::named_set_api::message_tags_list))
        .routes(routes!(crate::named_set_api::message_tags_create))
        .routes(routes!(crate::named_set_api::message_tags_update))
        .routes(routes!(crate::named_set_api::message_tags_delete))
        .routes(routes!(crate::named_set_api::message_tag_members_list))
        .routes(routes!(crate::named_set_api::message_tag_members_update))
        .routes(routes!(
            crate::saved_searches_api::saved_searches_list_handler
        ))
        .routes(routes!(
            crate::saved_searches_api::saved_searches_create_handler
        ))
        .routes(routes!(
            crate::saved_searches_api::saved_searches_update_handler
        ))
        .routes(routes!(
            crate::saved_searches_api::saved_searches_delete_handler
        ))
        .routes(routes!(crate::search_api::search_fields_list))
        .routes(routes!(
            crate::conversations_api::conversations_list_handler
        ))
        .routes(routes!(
            crate::conversations_api::conversation_detail_handler
        ))
        .routes(routes!(
            crate::conversations_api::conversation_sources_handler
        ))
        .routes(routes!(
            crate::conversations_api::conversation_messages_handler
        ))
        .routes(routes!(crate::messages_api::messages_list_handler))
        .routes(routes!(
            crate::conversations_api::conversation_trash_handler
        ))
        .routes(routes!(
            crate::conversations_api::conversation_restore_handler
        ))
        .routes(routes!(
            crate::conversations_api::conversation_delete_handler
        ))
        .routes(routes!(crate::trash_api::empty_trash_handler))
        .routes(routes!(crate::import::imports_list_handler))
        .routes(routes!(crate::import::imports_create_handler))
        .routes(routes!(crate::import::imports_get_handler))
        .routes(routes!(crate::import::import_contacts_handler))
        .routes(routes!(crate::import::imports_complete_handler))
        .routes(routes!(crate::import::imports_stage_handler))
        .routes(routes!(crate::import::imports_discard_handler))
        .routes(routes!(crate::import::import_batch_handler))
        .routes(routes!(crate::assets::asset_head_handler))
        .routes(routes!(crate::assets::asset_get_handler))
        .routes(routes!(crate::assets::asset_put_handler))
        .routes(routes!(crate::assets::asset_upload_start_handler))
        .routes(routes!(crate::assets::asset_upload_part_handler))
        .routes(routes!(crate::assets::asset_upload_complete_handler))
        .routes(routes!(crate::assets::asset_upload_abort_handler))
        .routes(routes!(crate::owner_api::list_accounts_handler))
        .routes(routes!(crate::owner_api::create_account_handler))
        .routes(routes!(crate::owner_api::patch_account_handler))
        .routes(routes!(crate::owner_api::set_account_password_handler))
        .routes(routes!(crate::owner_api::delete_account_messages_handler))
        .routes(routes!(crate::owner_api::delete_account_handler))
        .routes(routes!(crate::vault_api::vault_settings_handler))
        .routes(routes!(crate::vault_api::patch_vault_settings_handler))
}

/// Pretty OpenAPI JSON. Same string the CLI writes and the stale-spec test compares.
pub fn dump_openapi_json() -> String {
    let (_a, mut spec) = auth_public_openapi().split_for_parts();
    let (_b, rest) = api_openapi().split_for_parts();
    spec.merge(rest);
    serde_json::to_string_pretty(&spec).expect("OpenAPI document serializes to JSON")
}

/// Write the dump to `path`, or stdout when `path` is `None`.
pub fn write_openapi(path: Option<&Path>) -> anyhow::Result<()> {
    let json = dump_openapi_json();
    match path {
        None => {
            let mut out = std::io::stdout().lock();
            out.write_all(json.as_bytes())?;
            if !json.ends_with('\n') {
                out.write_all(b"\n")?;
            }
        }
        Some(p) => std::fs::write(p, json.as_bytes())
            .map_err(|e| anyhow::anyhow!("write {}: {e}", p.display()))?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::dump_openapi_json;

    #[test]
    fn dump_is_openapi_3_with_crate_version() {
        let v: serde_json::Value = serde_json::from_str(&dump_openapi_json()).unwrap();
        let openapi = v["openapi"].as_str().expect("openapi field");
        assert!(
            openapi.starts_with("3."),
            "expected OpenAPI 3.x, got {openapi}"
        );
        assert_eq!(v["info"]["title"], "Message Vault HTTP API");
        assert_eq!(v["info"]["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn dump_pretty_print_is_stable() {
        let a = dump_openapi_json();
        let b = dump_openapi_json();
        assert_eq!(a, b);
        assert!(a.contains('\n'), "expected pretty JSON");
    }

    #[test]
    fn dump_includes_health() {
        let v: serde_json::Value = serde_json::from_str(&dump_openapi_json()).unwrap();
        assert!(
            v["paths"]["/health"]["get"].is_object(),
            "expected GET /health in dump"
        );
    }

    #[test]
    fn dump_includes_auth_and_account_paths() {
        let v: serde_json::Value = serde_json::from_str(&dump_openapi_json()).unwrap();
        let paths = v["paths"].as_object().unwrap();
        for p in [
            "/v1/auth/register",
            "/v1/auth/login",
            "/v1/auth/check",
            "/v1/auth/logout",
            "/v1/account/password",
            "/v1/account",
            "/v1/account/profile",
            "/v1/account/messages",
            "/v1/account/storage",
            "/v1/account/api-tokens",
            "/v1/account/api-tokens/{id}",
        ] {
            assert!(paths.contains_key(p), "missing {p}");
        }
        assert!(
            !operation_has_bearer(&paths["/v1/auth/register"]["post"]),
            "register is public"
        );
        assert!(
            operation_has_bearer(&paths["/v1/auth/check"]["get"]),
            "GET /v1/auth/check must require bearer"
        );
    }

    fn operation_has_bearer(op: &serde_json::Value) -> bool {
        op["security"]
            .as_array()
            .is_some_and(|schemes| schemes.iter().any(|s| s.get("bearer").is_some()))
    }

    #[test]
    fn dump_includes_browse_paths() {
        let v: serde_json::Value = serde_json::from_str(&dump_openapi_json()).unwrap();
        let paths = v["paths"].as_object().unwrap();
        for p in [
            "/v1/export/messages",
            "/v1/export/messages/count",
            "/v1/contacts",
            "/v1/contacts/summaries",
            "/v1/contacts/{id}",
            "/v1/contacts/{id}/trash",
            "/v1/contacts/{id}/restore",
            "/v1/contacts/unmatched-handles",
            "/v1/contact-groups",
            "/v1/contact-groups/{id}",
            "/v1/contact-groups/{id}/members",
            "/v1/message-tags",
            "/v1/message-tags/{id}",
            "/v1/message-tags/{id}/members",
            "/v1/saved-searches",
            "/v1/saved-searches/{id}",
            "/v1/search-fields",
            "/v1/conversations",
            "/v1/messages",
            "/v1/conversations/{id}",
            "/v1/conversations/{id}/sources",
            "/v1/conversations/{id}/messages",
            "/v1/conversations/{id}/trash",
            "/v1/conversations/{id}/restore",
            "/v1/trash",
        ] {
            assert!(paths.contains_key(p), "missing {p}");
        }
        // Trash is the only door to permanent deletion: DELETE exists on a
        // conversation, on a contact, and on the trash as a whole.
        assert!(paths["/v1/conversations/{id}"]["delete"].is_object());
        assert!(paths["/v1/contacts/{id}"]["delete"].is_object());
        assert!(paths["/v1/trash"]["delete"].is_object());
    }

    #[test]
    fn dump_includes_import_and_asset_paths() {
        let v: serde_json::Value = serde_json::from_str(&dump_openapi_json()).unwrap();
        let paths = v["paths"].as_object().unwrap();
        for p in [
            "/v1/imports",
            "/v1/imports/{id}",
            "/v1/imports/{id}/complete",
            "/v1/imports/{id}/batches",
            "/v1/assets/{sha256}",
            "/v1/assets/{sha256}/uploads",
            "/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}",
            "/v1/assets/{sha256}/uploads/{upload_id}/complete",
            "/v1/assets/{sha256}/uploads/{upload_id}",
        ] {
            assert!(paths.contains_key(p), "missing {p}");
        }
    }

    #[test]
    fn dump_includes_owner_paths() {
        let v: serde_json::Value = serde_json::from_str(&dump_openapi_json()).unwrap();
        let paths = v["paths"].as_object().unwrap();
        for p in [
            "/v1/owner/accounts",
            "/v1/owner/accounts/{id}",
            "/v1/owner/accounts/{id}/password",
            "/v1/owner/accounts/{id}/messages",
        ] {
            assert!(paths.contains_key(p), "missing {p}");
        }
        assert!(paths["/v1/owner/accounts"]["get"].is_object());
        assert!(paths["/v1/owner/accounts"]["post"].is_object());
        assert!(paths["/v1/owner/accounts/{id}"]["patch"].is_object());
        assert!(paths["/v1/owner/accounts/{id}"]["delete"].is_object());
        assert!(paths["/v1/owner/accounts/{id}/password"]["put"].is_object());
        assert!(paths["/v1/owner/accounts/{id}/messages"]["delete"].is_object());
        let import = &paths["/v1/imports/{id}/batches"]["post"]["requestBody"]["content"];
        for ct in ["application/x-ndjson", "application/jsonl"] {
            assert!(
                import.get(ct).is_some(),
                "POST /v1/imports/{{id}}/batches must document {ct}"
            );
        }
        assert!(
            import.get("multipart/form-data").is_none(),
            "POST /v1/imports/{{id}}/batches no longer accepts multipart (#337)"
        );
        let put = &paths["/v1/assets/{sha256}"]["put"]["requestBody"]["content"];
        assert!(
            put.get("application/octet-stream").is_some(),
            "PUT asset must be raw bytes"
        );
    }

    #[test]
    fn committed_openapi_matches_dump() {
        let dumped = dump_openapi_json();
        let committed = include_str!("../../../../docs/src/assets/openapi.json");
        assert_eq!(
            dumped.trim_end(),
            committed.trim_end(),
            "run: cargo run -p message-vault-server -- dump-openapi --output docs/src/assets/openapi.json"
        );
    }
}
