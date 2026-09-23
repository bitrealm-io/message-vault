//! Every operation in the OpenAPI document, called with every kind of
//! credential, against what the document and the rules say it accepts.
//!
//! Each route's `security(...)` annotation is written by hand, and the guard
//! that actually admits a caller is the extractor the handler takes. Nothing
//! else ties the two together, so a route could say "session only" while its
//! handler takes an API token, or the other way round. This test walks the
//! in-process document, not the committed JSON, so a new route is covered the
//! moment it is registered.
//!
//! The expected answer comes from the operation's declared `security` and from
//! `docs/architecture/http-api.md` ("Credentials and reach") and
//! `docs/adr/0008-the-vault-owner-holds-no-messages.md`. The test asks only
//! whether the credential was accepted or refused as declared, not whether the
//! call succeeded: `401` and `403` are a refusal, anything else is past the
//! guard. A call by another account on this account's rows must answer `404`,
//! except under `/v1/accounts/{id}`, which refuses a stranger with `403`
//! whether or not the row exists.
//!
//! Every call gets accounts, rows and sessions of its own inside one shared
//! vault, so a delete, a password change or a logout cannot change what the
//! next call sees.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::OnceLock;

use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::db::account_profile::{self, OWNER_ACCOUNT_ID};
use crate::db::api_tokens::create_api_token;
use crate::db::permissions::Permissions;
use crate::db::session_tokens::insert_account_session_token;
use crate::test_support::{SeedConversation, SeedMessage, TestServer, TestVault};

/// The one password every account in the fixture has.
const PASSWORD: &str = "matrix-password";

/// The bytes of the stored asset, and of the upload that is under way.
const ASSET_BYTES: &[u8] = b"an attachment already in the vault";
const UPLOAD_BYTES: &[u8] = b"an attachment being uploaded in parts";

/// The credentials every operation is called with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Credential {
    /// Alice's API token with every scope.
    TokenAllScopes,
    /// Alice's API token with no scope.
    TokenNoScopes,
    /// Alice's API token with the import scope only.
    TokenImportOnly,
    /// Alice's API token with the export scope only.
    TokenExportOnly,
    /// The vault owner's session.
    Owner,
    /// Alice's session, every permission on.
    Session,
    /// Alice's session with her `delete` permission off.
    SessionWithoutDelete,
    /// Bob's session, every permission on, calling on Alice's rows.
    OtherAccount,
}

const CREDENTIALS: [Credential; 8] = [
    Credential::TokenAllScopes,
    Credential::TokenNoScopes,
    Credential::TokenImportOnly,
    Credential::TokenExportOnly,
    Credential::Owner,
    Credential::Session,
    Credential::SessionWithoutDelete,
    Credential::OtherAccount,
];

impl Credential {
    /// The security scheme this credential answers to.
    fn scheme(self) -> &'static str {
        match self {
            Self::TokenAllScopes
            | Self::TokenNoScopes
            | Self::TokenImportOnly
            | Self::TokenExportOnly => "api-token",
            Self::Owner | Self::Session | Self::SessionWithoutDelete | Self::OtherAccount => {
                "session"
            }
        }
    }

    /// The scope names this credential carries, as the document spells them.
    fn scopes(self) -> &'static [&'static str] {
        match self {
            Self::TokenAllScopes | Self::Session | Self::OtherAccount => {
                &["import", "export", "delete"]
            }
            Self::TokenNoScopes => &[],
            Self::TokenImportOnly => &["import"],
            Self::TokenExportOnly => &["export"],
            Self::Owner => &["owner"],
            Self::SessionWithoutDelete => &["import", "export"],
        }
    }
}

impl fmt::Display for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::TokenAllScopes => "token, all scopes",
            Self::TokenNoScopes => "token, no scopes",
            Self::TokenImportOnly => "token, import only",
            Self::TokenExportOnly => "token, export only",
            Self::Owner => "owner session",
            Self::Session => "session",
            Self::SessionWithoutDelete => "session, no delete",
            Self::OtherAccount => "other account",
        };
        f.write_str(name)
    }
}

/// What a call should answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// Past the guard: anything but `401` or `403`.
    Accepted,
    /// `401` or `403`.
    Refused,
    /// `404`: another account's row, which the caller must not learn exists.
    NotFound,
}

impl Expected {
    fn matches(self, status: StatusCode) -> bool {
        let refused = status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN;
        match self {
            Self::Accepted => !refused && !status.is_server_error(),
            Self::Refused => refused,
            Self::NotFound => status == StatusCode::NOT_FOUND,
        }
    }
}

impl fmt::Display for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Accepted => "accepted",
            Self::Refused => "401/403",
            Self::NotFound => "404",
        })
    }
}

/// One operation of the document.
#[derive(Debug, Clone)]
struct Operation {
    method: String,
    path: String,
    /// `None` when the operation names no `security`: a public route.
    security: Option<Vec<Value>>,
}

impl Operation {
    fn label(&self) -> String {
        format!("{} {}", self.method.to_uppercase(), self.path)
    }

    /// Whether a requirement of the declared `security` admits `credential`:
    /// its scheme is the credential's, and every scope it names is one the
    /// credential carries. An empty requirement (`{}`) admits a request with
    /// no credential, and every call here sends one.
    fn declares(&self, credential: Credential) -> bool {
        let Some(requirements) = &self.security else {
            return true;
        };
        requirements.iter().any(|requirement| {
            requirement
                .as_object()
                .into_iter()
                .flatten()
                .any(|(scheme, scopes)| {
                    scheme == credential.scheme()
                        && scopes
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .all(|scope| credential.scopes().contains(&scope))
                })
        })
    }

    /// Whether a requirement names the owner's role.
    fn names_owner(&self) -> bool {
        self.security.iter().flatten().any(|requirement| {
            requirement["session"]
                .as_array()
                .is_some_and(|scopes| scopes.iter().any(|s| s == "owner"))
        })
    }

    /// Under `/v1/accounts/{id}`: read and written by the owner or by that
    /// account, and nobody else.
    fn is_under_an_account(&self) -> bool {
        self.path.starts_with("/v1/accounts/{id}")
    }

    /// What `credential` should get from this operation.
    fn expected(&self, credential: Credential) -> Expected {
        if self.security.is_none() {
            return Expected::Accepted;
        }
        if !self.declares(credential) {
            return Expected::Refused;
        }
        match credential {
            // A session with no scope named admits any logged-in person, but
            // the owner's session reaches only what the rules give it: its own
            // session, the routes that name the owner, and the accounts it
            // manages, where it sees no API tokens (ADR-0008).
            Credential::Owner => {
                let reaches = self.names_owner()
                    || self.path == "/v1/session"
                    || (self.is_under_an_account() && !self.path.contains("/api-tokens"));
                if reaches {
                    Expected::Accepted
                } else {
                    Expected::Refused
                }
            }
            Credential::OtherAccount if self.is_under_an_account() => Expected::Refused,
            Credential::OtherAccount if self.names_one_of_alices_rows() => Expected::NotFound,
            _ => Expected::Accepted,
        }
    }

    /// Whether the path names a row of Alice's. Storing an asset and every
    /// upload route work in the caller's own asset store: the hash and the
    /// upload id are looked up there, so Bob naming Alice's upload reaches
    /// an upload of his that does not exist, never hers. [`run`] checks
    /// that her upload survives his call.
    fn names_one_of_alices_rows(&self) -> bool {
        let own_store =
            self.is_an_upload() || (self.method == "put" && self.path == "/v1/assets/{sha256}");
        self.path.contains('{') && !own_store
    }

    fn is_an_upload(&self) -> bool {
        self.path.starts_with("/v1/assets/{sha256}/uploads")
    }
}

/// Every operation in the document the server serves.
fn operations() -> Vec<Operation> {
    let doc: Value = serde_json::from_str(&super::dump_openapi_json()).unwrap();
    let mut operations = Vec::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            if !["get", "put", "post", "delete", "patch", "head"].contains(&method.as_str()) {
                continue;
            }
            operations.push(Operation {
                method: method.clone(),
                path: path.clone(),
                security: op["security"].as_array().cloned(),
            });
        }
    }
    operations
}

/// One vault and one running server for the whole matrix, with its owner.
/// Creating a vault is the slow part, on Postgres above all, so every call
/// gets fresh accounts inside this one instead of a vault of its own.
///
/// An account holds one Session at a time, so every call shares the owner's.
/// The one call that ends it, the owner's `DELETE /v1/session`, runs last.
struct Shared {
    vault: TestVault,
    server: TestServer,
    owner_session: String,
}

impl Shared {
    async fn build() -> Self {
        let vault = crate::test_support::test_vault().await;
        let mut conn = vault.conn().await;
        account_profile::insert_account_at(
            &mut conn,
            OWNER_ACCOUNT_ID,
            "keeper",
            Some(password_hash()),
            None,
        )
        .await
        .unwrap();
        let owner_session = insert_account_session_token(&mut conn, OWNER_ACCOUNT_ID)
            .await
            .unwrap();
        drop(conn);
        let server = crate::test_support::serve(&vault.state).await;
        Self {
            vault,
            server,
            owner_session,
        }
    }
}

/// One call's accounts and rows: Alice, who owns one of every row a path can
/// name, and Bob, a stranger to them. Both are made for this call alone, so a
/// delete, a password change or a logout cannot change what another call
/// sees.
struct World<'a> {
    shared: &'a Shared,
    /// Tells this call's usernames apart from every other call's.
    n: usize,
    alice: i64,
    tokens: Tokens,
    spare_token_id: i64,
    conversation_id: i64,
    message_id: i64,
    contact_id: i64,
    group_id: i64,
    tag_id: i64,
    saved_search_id: i64,
    import_id: i64,
    export_id: i64,
    upload_id: String,
}

struct Tokens {
    owner: String,
    alice: String,
    bob: String,
    all_scopes: String,
    no_scopes: String,
    import_only: String,
    export_only: String,
}

impl Tokens {
    fn for_credential(&self, credential: Credential) -> &str {
        match credential {
            Credential::TokenAllScopes => &self.all_scopes,
            Credential::TokenNoScopes => &self.no_scopes,
            Credential::TokenImportOnly => &self.import_only,
            Credential::TokenExportOnly => &self.export_only,
            Credential::Owner => &self.owner,
            Credential::Session | Credential::SessionWithoutDelete => &self.alice,
            Credential::OtherAccount => &self.bob,
        }
    }
}

/// One Argon2 hash for every account in the matrix: hashing is slow, and
/// what the password is does not matter here.
fn password_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| crate::credentials::hash_password(PASSWORD).unwrap())
}

impl<'a> World<'a> {
    async fn build(shared: &'a Shared, n: usize) -> Self {
        let state = &shared.vault.state;
        let mut conn = shared.vault.conn().await;
        let hash = Some(password_hash());
        // Ids of the call's own, because calls run side by side and the next
        // free id is read and then written. They count down from the top, so
        // the account the owner creates with the next free id never meets one.
        let alice = 10_000_000 - 2 * n as i64;
        let bob = alice + 1;
        for (id, name) in [(alice, "alice"), (bob, "bob")] {
            account_profile::insert_account_at(&mut conn, id, &format!("{name}-{n}"), hash, None)
                .await
                .unwrap();
        }
        let session = async |conn: &mut sqlx::AnyConnection, id| {
            insert_account_session_token(conn, id).await.unwrap()
        };
        let token = async |conn: &mut sqlx::AnyConnection, label, import, export, delete| {
            let permissions = Permissions {
                import,
                export,
                delete,
            };
            create_api_token(conn, alice, label, permissions, None)
                .await
                .unwrap()
        };
        let tokens = Tokens {
            owner: shared.owner_session.clone(),
            alice: session(&mut conn, alice).await,
            bob: session(&mut conn, bob).await,
            all_scopes: token(&mut conn, "all", true, true, true).await.token,
            no_scopes: token(&mut conn, "none", false, false, false).await.token,
            import_only: token(&mut conn, "import", true, false, false).await.token,
            export_only: token(&mut conn, "export", false, true, false).await.token,
        };
        let spare_token_id = token(&mut conn, "spare", false, false, false).await.id;

        let contact_id: i64 = sqlx::query_scalar(
            "INSERT INTO contacts (account_id, preferred_name) VALUES ($1, 'Sam') RETURNING id",
        )
        .bind(alice)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        drop(conn);

        let conversation_id = crate::test_support::seed_conversation(
            state,
            &SeedConversation {
                account_id: alice,
                handle: "+15555550100",
                conversation_type: "individual",
                group_title: None,
                source_file: "seed.jsonl",
                messages: &[SeedMessage {
                    source: "imessage",
                    timestamp: "2020-01-01T00:00:00Z",
                    is_from_me: true,
                    body: "hello",
                }],
            },
        )
        .await;
        let mut conn = shared.vault.conn().await;
        let message_id: i64 =
            sqlx::query_scalar("SELECT id FROM messages WHERE conversation_id = $1")
                .bind(conversation_id)
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        drop(conn);

        let mut world = Self {
            shared,
            n,
            alice,
            tokens,
            spare_token_id,
            conversation_id,
            message_id,
            contact_id,
            group_id: 0,
            tag_id: 0,
            saved_search_id: 0,
            import_id: 0,
            export_id: 0,
            upload_id: String::new(),
        };
        world.group_id = world
            .create("/v1/contact-groups", json!({ "name": "Family" }))
            .await["id"]
            .as_i64()
            .unwrap();
        world.tag_id = world
            .create("/v1/message-tags", json!({ "name": "Holiday" }))
            .await["id"]
            .as_i64()
            .unwrap();
        world.saved_search_id = world
            .create(
                "/v1/saved-searches",
                json!({ "name": "Mine", "query": "from:me" }),
            )
            .await["id"]
            .as_i64()
            .unwrap();
        world.import_id = world
            .create("/v1/imports", json!({ "source": "imessage" }))
            .await["id"]
            .as_i64()
            .unwrap();
        world.export_id = world
            .create("/v1/exports", json!({ "scope": { "kind": "everything" } }))
            .await["id"]
            .as_i64()
            .unwrap();

        let asset = world.url(&format!(
            "/v1/assets/{}?source=imessage",
            crate::assets::sha256_hex(ASSET_BYTES)
        ));
        let response = reqwest::Client::new()
            .put(asset)
            .bearer_auth(&world.tokens.alice)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(ASSET_BYTES)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "store the asset");
        world.upload_id = world
            .create(
                &format!(
                    "/v1/assets/{}/uploads?source=imessage",
                    crate::assets::sha256_hex(UPLOAD_BYTES)
                ),
                json!({ "bytes": UPLOAD_BYTES.len() }),
            )
            .await["upload_id"]
            .as_str()
            .unwrap()
            .to_string();
        world
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.shared.server.base())
    }

    /// POST as Alice and return the body of the `201 Created`.
    async fn create(&self, path: &str, body: Value) -> Value {
        let response = reqwest::Client::new()
            .post(self.url(path))
            .bearer_auth(&self.tokens.alice)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "fixture POST {path}: {text}");
        serde_json::from_str(&text).unwrap()
    }

    /// The operation's path with each parameter filled with Alice's row.
    fn path_for(&self, op: &Operation) -> String {
        let mut path = String::new();
        let mut rest = op.path.as_str();
        while let Some(open) = rest.find('{') {
            let close = rest[open..].find('}').unwrap() + open;
            path.push_str(&rest[..open]);
            path.push_str(&self.parameter(&op.path, &rest[open + 1..close]));
            rest = &rest[close + 1..];
        }
        path.push_str(rest);
        match op.path.as_str() {
            p if p.starts_with("/v1/assets/") => path.push_str("?source=imessage"),
            "/v1/search-fields" => path.push_str("?list=messages"),
            _ => {}
        }
        path
    }

    fn parameter(&self, path: &str, name: &str) -> String {
        let id = |prefix: &str| path.starts_with(prefix);
        match name {
            "id" if id("/v1/accounts/") => self.alice.to_string(),
            "id" if id("/v1/contact-groups/") => self.group_id.to_string(),
            "id" if id("/v1/contacts/") => self.contact_id.to_string(),
            "id" if id("/v1/conversations/") => self.conversation_id.to_string(),
            "id" if id("/v1/exports/") => self.export_id.to_string(),
            "id" if id("/v1/imports/") => self.import_id.to_string(),
            "id" if id("/v1/message-tags/") => self.tag_id.to_string(),
            "id" if id("/v1/messages/") => self.message_id.to_string(),
            "id" if id("/v1/saved-searches/") => self.saved_search_id.to_string(),
            "token_id" => self.spare_token_id.to_string(),
            "import_id" => self.import_id.to_string(),
            "sha256" if path.contains("/uploads") => crate::assets::sha256_hex(UPLOAD_BYTES),
            "sha256" => crate::assets::sha256_hex(ASSET_BYTES),
            "upload_id" => self.upload_id.clone(),
            "part" => "1".to_string(),
            _ => panic!("the credential matrix has no row for {{{name}}} in {path}; add one"),
        }
    }

    /// Call `op` with `credential` and return the status.
    async fn call(&self, op: &Operation, credential: Credential) -> StatusCode {
        let method = reqwest::Method::from_bytes(op.method.to_uppercase().as_bytes()).unwrap();
        let mut request = reqwest::Client::new()
            .request(method, self.url(&self.path_for(op)))
            .bearer_auth(self.tokens.for_credential(credential));
        if let Some((content_type, body)) = body_for(op, self.n) {
            request = request
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let _ = response.bytes().await;
        status
    }

    /// Whether Alice can still send a part of her upload.
    async fn upload_survives(&self) -> bool {
        let path = format!(
            "/v1/assets/{}/uploads/{}/parts/1?source=imessage",
            crate::assets::sha256_hex(UPLOAD_BYTES),
            self.upload_id
        );
        let response = reqwest::Client::new()
            .put(self.url(&path))
            .bearer_auth(&self.tokens.alice)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(UPLOAD_BYTES)
            .send()
            .await
            .unwrap();
        response.status() == StatusCode::OK
    }
}

/// A body that gets the operation past its own validation to the lookup that
/// decides whose row it is, so a stranger's call reaches the `404`. `n` keeps
/// usernames apart between calls.
fn body_for(op: &Operation, n: usize) -> Option<(&'static str, Vec<u8>)> {
    let json = |value: Value| Some(("application/json", serde_json::to_vec(&value).unwrap()));
    match (op.method.as_str(), op.path.as_str()) {
        ("post", "/v1/accounts") => {
            json(json!({ "username": format!("carol-{n}"), "password": PASSWORD }))
        }
        ("delete", "/v1/accounts/{id}") => {
            json(json!({ "confirm": true, "current_password": PASSWORD }))
        }
        ("patch", "/v1/accounts/{id}") => json(json!({})),
        ("post", "/v1/accounts/{id}/api-tokens") => json(json!({ "label": "new" })),
        ("patch", "/v1/accounts/{id}/api-tokens/{token_id}") => json(json!({ "label": "renamed" })),
        ("delete", "/v1/accounts/{id}/messages") => json(json!({ "confirm": true })),
        ("put", "/v1/accounts/{id}/password") => json(json!({
            "password": "another-password",
            "password_confirmation": "another-password",
        })),
        ("put", "/v1/assets/{sha256}") => Some(("application/octet-stream", ASSET_BYTES.to_vec())),
        ("post", "/v1/assets/{sha256}/uploads") => json(json!({ "bytes": UPLOAD_BYTES.len() })),
        ("put", "/v1/assets/{sha256}/uploads/{upload_id}/parts/{part}") => {
            Some(("application/octet-stream", UPLOAD_BYTES.to_vec()))
        }
        ("post", "/v1/contact-groups" | "/v1/message-tags") => json(json!({ "name": "Work" })),
        ("patch", "/v1/contact-groups/{id}" | "/v1/message-tags/{id}") => {
            json(json!({ "name": "Renamed" }))
        }
        ("patch", "/v1/contact-groups/{id}/members" | "/v1/message-tags/{id}/members") => {
            json(json!({ "add": [], "remove": [] }))
        }
        ("post", "/v1/contacts") => Some((
            "text/vcard",
            b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Robin\r\nTEL:+15555550199\r\nEND:VCARD\r\n".to_vec(),
        )),
        ("post", "/v1/contacts/summaries") => json(json!({ "ids": [] })),
        ("post", "/v1/contacts/unmatched-handles") => json(json!({ "identifiers": [] })),
        ("patch", "/v1/contacts/{id}") => json(json!({ "name": "Samantha" })),
        ("post", "/v1/exports") => json(json!({ "scope": { "kind": "everything" } })),
        ("post", "/v1/imports") => json(json!({ "source": "imessage" })),
        ("patch", "/v1/imports/{id}") => json(json!({ "stage": "parse" })),
        ("post", "/v1/imports/{id}/batches") => Some(("application/x-ndjson", Vec::new())),
        ("post", "/v1/imports/{id}/complete") => json(json!({ "ok": true })),
        ("post", "/v1/saved-searches") => json(json!({ "name": "Theirs", "query": "from:me" })),
        ("patch", "/v1/saved-searches/{id}") => {
            json(json!({ "name": "Renamed", "query": "from:me" }))
        }
        ("post", "/v1/session") => {
            json(json!({ "username": format!("alice-{n}"), "password": PASSWORD }))
        }
        ("post", "/v1/vault/claim") => json(json!({ "username": "usurper", "password": PASSWORD })),
        ("patch", "/v1/vault/settings") => json(json!({ "public_registration": true })),
        _ => None,
    }
}

/// Call one operation with one credential, on accounts made for this call.
async fn run(shared: &Shared, n: usize, op: Operation, credential: Credential) -> Option<String> {
    let world = World::build(shared, n).await;
    if credential == Credential::SessionWithoutDelete {
        let mut conn = shared.vault.conn().await;
        sqlx::query("UPDATE accounts SET can_delete = 0 WHERE id = $1")
            .bind(world.alice)
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    let expected = op.expected(credential);
    let status = world.call(&op, credential).await;
    let row = |outcome: String| {
        format!(
            "{:<60} {:<20} {outcome}",
            op.label(),
            credential.to_string()
        )
    };
    if !expected.matches(status) {
        return Some(row(format!(
            "expected {expected:<8} got {}",
            status.as_u16()
        )));
    }
    if credential == Credential::OtherAccount && op.is_an_upload() && !world.upload_survives().await
    {
        return Some(row("reached Alice's upload".to_string()));
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_route_accepts_and_refuses_each_credential_as_the_document_and_rules_say() {
    let operations = operations();
    let calls: Vec<(Operation, Credential)> = operations
        .iter()
        .flat_map(|op| CREDENTIALS.iter().map(move |c| (op.clone(), *c)))
        .collect();
    let total = calls.len();
    let shared = Shared::build().await;
    let ends_the_owners_session = |(_, (op, credential)): &(usize, (Operation, Credential))| {
        *credential == Credential::Owner && op.method == "delete" && op.path == "/v1/session"
    };
    let (last, first): (Vec<_>, Vec<_>) = calls
        .into_iter()
        .enumerate()
        .partition(ends_the_owners_session);

    // SQLite has one writer, and a transaction that starts reading and then
    // writes fails outright when another holds the lock, so calls run one at
    // a time there. Postgres takes them side by side.
    let side_by_side = if crate::test_support::on_postgres() {
        8
    } else {
        1
    };
    let mut mismatches: Vec<String> = futures_util::stream::iter(first)
        .map(|(n, (op, credential))| run(&shared, n, op, credential))
        .buffer_unordered(side_by_side)
        .filter_map(|mismatch| async move { mismatch })
        .collect()
        .await;
    for (n, (op, credential)) in last {
        mismatches.extend(run(&shared, n, op, credential).await);
    }
    mismatches.sort();

    assert!(
        operations.len() >= 80,
        "the matrix walked only {} operations; is the document whole?",
        operations.len()
    );
    assert!(
        mismatches.is_empty(),
        "{} of {total} calls ({} operations x {} credentials) disagree with the declared security:\n{:<60} {:<20} {}\n{}",
        mismatches.len(),
        operations.len(),
        CREDENTIALS.len(),
        "operation",
        "credential",
        "outcome",
        mismatches.join("\n")
    );
}

#[test]
fn the_expected_outcome_follows_the_declared_security_and_the_owner_rule() {
    let op = |method: &str, path: &str, security: Value| Operation {
        method: method.into(),
        path: path.into(),
        security: security.as_array().cloned(),
    };
    let browse = op("get", "/v1/messages", json!([{ "session": [] }]));
    assert_eq!(browse.expected(Credential::Session), Expected::Accepted);
    assert_eq!(
        browse.expected(Credential::TokenAllScopes),
        Expected::Refused
    );
    assert_eq!(browse.expected(Credential::Owner), Expected::Refused);
    assert_eq!(
        browse.expected(Credential::OtherAccount),
        Expected::Accepted
    );

    let one = op("get", "/v1/messages/{id}", json!([{ "session": [] }]));
    assert_eq!(one.expected(Credential::OtherAccount), Expected::NotFound);

    let export = op(
        "get",
        "/v1/exports",
        json!([{ "session": ["export"] }, { "api-token": ["export"] }]),
    );
    assert_eq!(
        export.expected(Credential::TokenExportOnly),
        Expected::Accepted
    );
    assert_eq!(
        export.expected(Credential::TokenImportOnly),
        Expected::Refused
    );
    assert_eq!(export.expected(Credential::Owner), Expected::Refused);

    let account = op("get", "/v1/accounts/{id}", json!([{ "session": [] }]));
    assert_eq!(account.expected(Credential::Owner), Expected::Accepted);
    assert_eq!(
        account.expected(Credential::OtherAccount),
        Expected::Refused
    );
    let tokens = op(
        "get",
        "/v1/accounts/{id}/api-tokens",
        json!([{ "session": [] }]),
    );
    assert_eq!(tokens.expected(Credential::Owner), Expected::Refused);

    let public = op("get", "/v1/vault", Value::Null);
    let outcomes: BTreeSet<String> = CREDENTIALS
        .iter()
        .map(|c| public.expected(*c).to_string())
        .collect();
    assert_eq!(outcomes, BTreeSet::from(["accepted".to_string()]));

    let stranger_or_owner = op(
        "post",
        "/v1/accounts",
        json!([{}, { "session": ["owner"] }]),
    );
    assert_eq!(
        stranger_or_owner.expected(Credential::Owner),
        Expected::Accepted
    );
    assert_eq!(
        stranger_or_owner.expected(Credential::Session),
        Expected::Refused
    );
}
