//! One shape for every paged list on the HTTP interface (ADR-0005).
//!
//! A list takes `?offset=&limit=` and answers `{items, total, limit, offset}`.
//! A `limit` above the cap or a zero `limit` is a 400, never a silent clamp,
//! so a caller learns the rule the first time it breaks it.

use serde::{Deserialize, Serialize};

use crate::server::ApiError;

/// Default page size for the Contacts and Conversations lists.
pub const DEFAULT_LIST_LIMIT: usize = 40;
/// Default page size for `GET /v1/export/messages`.
pub const DEFAULT_EXPORT_LIMIT: usize = 100;
/// The largest page any list route returns. One number, one meaning.
pub const MAX_LIST_LIMIT: usize = 500;
/// Cap on `OFFSET` skips for the Contacts and Conversations lists. Export has
/// no cap: it walks the whole set.
pub const MAX_LIST_OFFSET: usize = 50_000;
/// Most contact ids one `POST /v1/contacts/summaries` body may carry, so the
/// `IN` list stays under SQLite's variable cap.
pub const MAX_CONTACT_SUMMARY_IDS: usize = 500;

/// One page of a list.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Page<T> {
    /// The rows on this page.
    pub items: Vec<T>,
    /// Rows matching the query across every page.
    pub total: u64,
    /// Page size used.
    pub limit: usize,
    /// Page offset used.
    pub offset: usize,
}

/// The `q`/`limit`/`offset` query string of a plain list route; lists with
/// extra parameters declare their own struct and call `page_params` directly.
#[derive(Debug, Deserialize)]
pub struct PageQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

/// A validated `limit` and `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageParams {
    pub limit: usize,
    pub offset: usize,
}

/// Turn the raw `limit` and `offset` into a page, or a 400 that says which
/// one is wrong. `max_offset` is `None` for a route that may walk the whole set.
pub fn page_params(
    limit: Option<usize>,
    offset: Option<usize>,
    default_limit: usize,
    max_offset: Option<usize>,
) -> Result<PageParams, ApiError> {
    let limit = limit.unwrap_or(default_limit);
    if limit == 0 {
        return Err(ApiError::validation("limit must be at least 1"));
    }
    if limit > MAX_LIST_LIMIT {
        return Err(ApiError::validation(format!(
            "limit exceeds maximum of {MAX_LIST_LIMIT}"
        )));
    }
    let offset = offset.unwrap_or(0);
    if let Some(max) = max_offset
        && offset > max
    {
        return Err(ApiError::validation(format!(
            "offset exceeds maximum of {max}"
        )));
    }
    Ok(PageParams { limit, offset })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fill_in_when_nothing_is_sent() {
        let p = page_params(None, None, DEFAULT_LIST_LIMIT, Some(MAX_LIST_OFFSET)).unwrap();
        assert_eq!(
            p,
            PageParams {
                limit: 40,
                offset: 0
            }
        );
    }

    #[test]
    fn a_limit_above_the_cap_is_refused_not_clamped() {
        let err = page_params(Some(501), None, 40, None).unwrap_err();
        assert!(
            matches!(err, ApiError::ValidationFailed(m) if m == ["limit exceeds maximum of 500"])
        );
        let p = page_params(Some(500), None, 40, None).unwrap();
        assert_eq!(p.limit, 500);
    }

    #[test]
    fn a_zero_limit_is_refused() {
        let err = page_params(Some(0), None, 40, None).unwrap_err();
        assert!(matches!(err, ApiError::ValidationFailed(m) if m == ["limit must be at least 1"]));
    }

    #[test]
    fn an_offset_past_the_cap_is_refused_only_when_a_cap_is_given() {
        let err = page_params(None, Some(50_001), 40, Some(MAX_LIST_OFFSET)).unwrap_err();
        assert!(
            matches!(err, ApiError::ValidationFailed(m) if m == ["offset exceeds maximum of 50000"])
        );
        let p = page_params(None, Some(50_001), 40, None).unwrap();
        assert_eq!(p.offset, 50_001);
    }

    #[test]
    fn a_page_serializes_with_the_four_agreed_keys() {
        let page = Page {
            items: vec![1, 2],
            total: 9,
            limit: 2,
            offset: 4,
        };
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"items": [1, 2], "total": 9, "limit": 2, "offset": 4})
        );
    }
}
