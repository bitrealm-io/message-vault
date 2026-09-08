//! The registry of problem types: what each kind of failure is, the status it
//! answers, the page that describes it (ADR-0010).
//!
//! This is the one place a type is declared. [`crate::server::ApiError`] names
//! one per variant, the OpenAPI document describes the body through
//! [`Problem`], and `dump-error-docs` ([`crate::error_docs`]) writes one page
//! per type under `docs/src/content/docs/vault/developer/reference/errors/`
//! from the same declarations, so nothing has to be kept in step by hand.

use axum::http::StatusCode;
pub use vault_api_types::Problem;

/// Where the type pages are published; each `type` URL is this plus the slug.
pub const ERRORS_URL: &str = "https://bitrealm.io/vault/developer/reference/errors/";

/// The `type` of a `500 Internal Server Error`: no page could say anything a
/// reader could act on.
pub const INTERNAL_TYPE: &str = "about:blank";

/// One registered kind of failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProblemType {
    /// A query parameter, path segment or body field parsed and then broke a rule.
    ValidationFailed,
    /// A required query parameter or body field is absent.
    MissingParameter,
    /// The request cannot be read: not valid JSON or JSONL, or a body that failed to arrive.
    MalformedBody,
    /// `Content-Type` is absent or not one the route accepts.
    UnsupportedMediaType,
    /// The body is over the configured cap.
    PayloadTooLarge,
    /// A username, password, or current-password check failed.
    InvalidCredentials,
    /// The bearer token is missing, malformed, unknown or expired.
    AuthenticationRequired,
    /// The auth rate limiter refused the attempt.
    RateLimited,
    /// Registration or a rename collides with an existing username.
    UsernameTaken,
    /// A Contact Group, Message Tag, or Saved Search name collides.
    NameTaken,
    /// The demo account refuses a destructive operation.
    DemoAccountProtected,
    /// The account is not the vault owner.
    NotTheOwner,
    /// The token is valid but lacks the scope the route needs.
    InsufficientScope,
    /// The account exists but may not sign in or act.
    AccountDisabled,
    /// The search language refused a word.
    SearchQueryInvalid,
    /// The resource is not in a state that allows the operation.
    StateConflict,
    /// A part number, upload id, or completion does not match the upload.
    AssetUploadInvalid,
    /// The addressed resource does not exist for this account.
    NotFound,
    /// The path exists and the method does not.
    MethodNotAllowed,
    /// `Accept` names nothing this route can produce.
    NotAcceptable,
}

impl ProblemType {
    /// Every registered type, in the order the docs index lists them.
    pub const ALL: [Self; 20] = [
        Self::ValidationFailed,
        Self::MissingParameter,
        Self::MalformedBody,
        Self::UnsupportedMediaType,
        Self::PayloadTooLarge,
        Self::InvalidCredentials,
        Self::AuthenticationRequired,
        Self::RateLimited,
        Self::UsernameTaken,
        Self::NameTaken,
        Self::DemoAccountProtected,
        Self::NotTheOwner,
        Self::InsufficientScope,
        Self::AccountDisabled,
        Self::SearchQueryInvalid,
        Self::StateConflict,
        Self::AssetUploadInvalid,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::NotAcceptable,
    ];

    /// The last segment of the `type` URL and the page's file name.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::ValidationFailed => "validation-failed",
            Self::MissingParameter => "missing-parameter",
            Self::MalformedBody => "malformed-body",
            Self::UnsupportedMediaType => "unsupported-media-type",
            Self::PayloadTooLarge => "payload-too-large",
            Self::InvalidCredentials => "invalid-credentials",
            Self::AuthenticationRequired => "authentication-required",
            Self::RateLimited => "rate-limited",
            Self::UsernameTaken => "username-taken",
            Self::NameTaken => "name-taken",
            Self::DemoAccountProtected => "demo-account-protected",
            Self::NotTheOwner => "not-the-owner",
            Self::InsufficientScope => "insufficient-scope",
            Self::AccountDisabled => "account-disabled",
            Self::SearchQueryInvalid => "search-query-invalid",
            Self::StateConflict => "state-conflict",
            Self::AssetUploadInvalid => "asset-upload-invalid",
            Self::NotFound => "not-found",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::NotAcceptable => "not-acceptable",
        }
    }

    /// The status every problem of this type answers.
    #[must_use]
    pub const fn status(self) -> StatusCode {
        match self {
            Self::ValidationFailed => StatusCode::UNPROCESSABLE_ENTITY,
            Self::MissingParameter
            | Self::MalformedBody
            | Self::SearchQueryInvalid
            | Self::AssetUploadInvalid => StatusCode::BAD_REQUEST,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::InvalidCredentials | Self::AuthenticationRequired => StatusCode::UNAUTHORIZED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::UsernameTaken | Self::NameTaken | Self::StateConflict => StatusCode::CONFLICT,
            Self::DemoAccountProtected
            | Self::NotTheOwner
            | Self::InsufficientScope
            | Self::AccountDisabled => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
        }
    }

    /// The fixed, human-readable name every problem of this type carries.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::ValidationFailed => "Validation failed",
            Self::MissingParameter => "Missing parameter",
            Self::MalformedBody => "Malformed body",
            Self::UnsupportedMediaType => "Unsupported media type",
            Self::PayloadTooLarge => "Payload too large",
            Self::InvalidCredentials => "Invalid credentials",
            Self::AuthenticationRequired => "Authentication required",
            Self::RateLimited => "Rate limited",
            Self::UsernameTaken => "Username taken",
            Self::NameTaken => "Name taken",
            Self::DemoAccountProtected => "Demo account protected",
            Self::NotTheOwner => "Not the owner",
            Self::InsufficientScope => "Insufficient scope",
            Self::AccountDisabled => "Account disabled",
            Self::SearchQueryInvalid => "Search query invalid",
            Self::StateConflict => "State conflict",
            Self::AssetUploadInvalid => "Asset upload invalid",
            Self::NotFound => "Not found",
            Self::MethodNotAllowed => "Method not allowed",
            Self::NotAcceptable => "Not acceptable",
        }
    }

    /// The `type` URL: the page's address.
    #[must_use]
    pub fn url(self) -> String {
        format!("{ERRORS_URL}{}", self.slug())
    }

    /// The page's text: when the vault answers this, and what to do about it.
    /// Markdown paragraphs, no heading; the generator adds the frontmatter.
    #[must_use]
    pub fn page(self) -> String {
        match self {
            Self::ValidationFailed => "A query parameter, path segment or body field was read and then broke a rule: a `limit` of zero, an id that is not a number, a name that is blank or too long, an unknown `sort` key or `status` value, a body missing a required field.\n\n\
`errors` lists every rule the request broke, one sentence each, not only the first. Fix each one and send the request again.".to_string(),
            Self::MissingParameter => "A query parameter or body field the route requires was absent. `detail` names it.".to_string(),
            Self::MalformedBody => "The request could not be read at all: the body is not valid JSON, an import line is not the JSON Lines the vault reads, or the body failed to arrive. Nothing was parsed, so nothing is reported field by field; `detail` says where reading stopped.".to_string(),
            Self::UnsupportedMediaType => "The request's `Content-Type` is absent or not one this route accepts. An import body is `application/x-ndjson` or `application/jsonl`; a JSON route takes `application/json`. Send the right header with the same body.".to_string(),
            Self::PayloadTooLarge => "The body is over the vault's configured cap, whether announced by `Content-Length` or discovered while reading. Auth routes cap at 32 KiB; other routes at `[server] max_body_bytes`. Send less, or raise the cap on the vault.".to_string(),
            Self::InvalidCredentials => "The username or password did not match an account, or the current password given to confirm a change was wrong. The vault does not say which half failed. Check both and try again; repeated attempts are rate limited.".to_string(),
            Self::AuthenticationRequired => "The request carried no usable credential: the `Authorization: Bearer <token>` header is missing, malformed, unknown or expired. Sign in again, or issue a new API token, and send the new token.".to_string(),
            Self::RateLimited => format!(
                "The vault refused an authentication attempt because the same username has tried too often: more than {} attempts to sign in, register or claim the vault inside {} seconds. Wait the number of seconds in the `Retry-After` header (repeated as `retry_after` in the body) and try again.",
                crate::auth::AUTH_RATE_MAX,
                crate::auth::AUTH_RATE_WINDOW.as_secs()
            ),
            Self::UsernameTaken => "The username already belongs to an account on this vault. Usernames are compared ignoring case. Pick another.".to_string(),
            Self::NameTaken => "A Contact Group, Message Tag or Saved Search with this name already exists for the account. Names are compared ignoring case. Pick another, or rename the existing one.".to_string(),
            Self::DemoAccountProtected => "The demo account refuses this operation, because it exists to be looked at and reset rather than changed. Sign in as a real account, or run `reset-demo` on the vault to restore the demo data.".to_string(),
            Self::NotTheOwner => "This route belongs to the vault owner: creating accounts, changing vault settings, or anything the owner gates. Ask the owner to do it, or to give you what you need.".to_string(),
            Self::InsufficientScope => "The credential was accepted but may not do this. An API token carries import and export permissions and never a signed-in session's full access; an account may be restricted from import, export or deletion by the owner. Use a session, a token with the right scope, or ask the owner.".to_string(),
            Self::AccountDisabled => "The account exists but the vault owner has disabled it, so it may not sign in or act. Ask the owner to enable it.".to_string(),
            Self::SearchQueryInvalid => "The search language refused the query. `detail` names the word and the list it was used on; `word` carries the word, and `did_you_mean` a word the language does have when one is close. The query language is documented in the search reference.".to_string(),
            Self::StateConflict => "The resource is not in a state that allows the operation: an import that is no longer running or already has a live run, a vault that already has an owner, or a delete on something not yet trashed. `detail` says which. Read the resource's current state and choose the operation it allows.".to_string(),
            Self::AssetUploadInvalid => "Something about the upload does not match what the vault expected: the bytes do not hash to the claimed SHA-256, a part number or upload id is unknown, or a completion names parts that never arrived. `detail` says which. Start the upload again.".to_string(),
            Self::NotFound => "No resource at that address exists for this account. An id that belongs to another account answers this too, so an unknown id and a forbidden one look the same.".to_string(),
            Self::MethodNotAllowed => "The path exists but does not take this method. The OpenAPI document lists each route's methods.".to_string(),
            Self::NotAcceptable => "The request's `Accept` header named nothing this route can produce. Every `/v1` route but the asset download answers `application/json`, and a failure `application/problem+json`; send `Accept: application/json`, `*/*`, or no `Accept` at all.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_type_has_a_distinct_slug_and_a_page() {
        let slugs: HashSet<&str> = ProblemType::ALL.iter().map(|t| t.slug()).collect();
        assert_eq!(slugs.len(), ProblemType::ALL.len());
        for t in ProblemType::ALL {
            assert!(!t.page().trim().is_empty(), "{} has no page text", t.slug());
            assert!(t.url().ends_with(t.slug()));
            assert!(t.status().is_client_error(), "{} is not a 4xx", t.slug());
        }
    }
}
