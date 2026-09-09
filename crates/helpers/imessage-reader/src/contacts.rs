//! Look up contact names from macOS Address Book or iOS Contacts databases.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use imessage_database::{
    error::table::TableError, tables::table::get_connection, util::dirs::home,
};
use rusqlite::{Connection, Result};

/// Default contacts database path inside an iOS backup.
pub(crate) const DEFAULT_PATH_IOS: &str = "31/31bb7ba8914766d4ba40d6dfb6113c8b614be442";

/// Minimum digits required to index a phone number.
const MIN_PHONE_DIGITS: usize = 7;

// MARK: Name
#[derive(Clone, Debug, PartialEq, Eq)]
/// Contact name and the handle IDs it resolved from.
pub(crate) struct Name {
    /// First name.
    pub first: String,
    /// Last name.
    pub last: String,
    /// Full name.
    pub full: String,
    /// Combined handle details from the Messages database.
    pub details: String,
    /// Original handle IDs that map to this name.
    pub handle_ids: HashSet<i32>,
}

impl Name {
    /// Build a name from optional first and last name fields.
    fn from_opt(first: Option<String>, last: Option<String>) -> Option<Self> {
        // Return None if both are None
        if first.is_none() && last.is_none() {
            return None;
        }

        // Build full name
        let full = format!(
            "{}{}{}",
            first.as_deref().unwrap_or(""),
            if first.is_some() && last.is_some() {
                " "
            } else {
                ""
            },
            last.as_deref().unwrap_or(""),
        );

        Some(Name {
            first: first.unwrap_or_default(),
            last: last.unwrap_or_default(),
            full,
            details: String::new(),
            handle_ids: HashSet::new(),
        })
    }

    /// Score name completeness: one point each for first and last name.
    fn score(&self) -> u8 {
        u8::from(!self.first.is_empty()) + u8::from(!self.last.is_empty())
    }

    /// Return the full name, falling back to handle details.
    pub fn get_display_name(&self) -> &str {
        if self.full.is_empty() {
            &self.details
        } else {
            &self.full
        }
    }

    /// Build a name that only carries the details string.
    pub fn from_details<D: Into<String>>(details: D) -> Self {
        Name {
            first: String::new(),
            last: String::new(),
            full: String::new(),
            details: details.into(),
            handle_ids: HashSet::new(),
        }
    }
}

// MARK: Index
#[derive(Debug, Default)]
/// Contacts index keyed by normalized phone number or email address.
pub(crate) struct ContactsIndex {
    /// Names keyed by normalized identifier.
    index: HashMap<String, Name>,
}

impl ContactsIndex {
    /// Build a contacts index from one database path or all local macOS sources.
    ///
    /// When `path` is `Some`, only that database is read. When `path` is `None`,
    /// scans macOS Contacts sources under
    /// `~/Library/Application Support/AddressBook/Sources/*/AddressBook-v22.abcddb`.
    ///
    /// Accepts macOS (`AddressBook-v22.abcddb`) and iOS (`AddressBook.sqlitedb`)
    /// databases.
    ///
    /// # Errors
    ///
    /// Returns an error when a database cannot be opened or queried.
    pub fn build(path: Option<&Path>) -> Result<Self, TableError> {
        if let Some(path) = path {
            let conn = get_connection(path)?;
            if table_exists(&conn, "ABPersonFullTextSearch_content") {
                return Ok(Self::build_from_ios(&conn)?);
            }
            return Ok(Self::build_from_macos(&conn)?);
        }

        let mut idx: HashMap<String, Name> = HashMap::new();

        for db_path in find_macos_addressbook_db_paths() {
            if let Ok(local_conn) = get_connection(&db_path) {
                let sub = Self::build_from_macos(&local_conn)?;

                for (k, v) in sub.index {
                    upsert_best(&mut idx, k, &v);
                }
            }
        }

        Ok(Self { index: idx })
    }

    // MARK: macOS
    /// Build a contacts index from a macOS Contacts database.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    fn build_from_macos(conn: &Connection) -> Result<Self> {
        let mut index = HashMap::new();

        let mut stmt = conn.prepare(
            "SELECT r.ZFIRSTNAME, r.ZLASTNAME, p.ZFULLNUMBER, e.ZADDRESSNORMALIZED
             FROM ZABCDRECORD AS r
             LEFT JOIN ZABCDPHONENUMBER AS p ON r.Z_PK = p.ZOWNER
             LEFT JOIN ZABCDEMAILADDRESS AS e ON r.Z_PK = e.ZOWNER",
        )?;

        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name = Name::from_opt(
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            );

            if let Some(name) = name {
                if let Some(email_raw) = row.get::<_, Option<String>>(3)? {
                    // Some macOS rows are like "<addr@dom>"
                    for email in parse_email_list(&email_raw) {
                        upsert_best(&mut index, email, &name);
                    }
                }

                if let Some(phone_raw) = row.get::<_, Option<String>>(2)? {
                    for key in phone_keys(&phone_raw) {
                        upsert_best(&mut index, key, &name);
                    }
                }
            }
        }

        Ok(Self { index })
    }

    // MARK: iOS
    /// Build a contacts index from an iOS backup Contacts database.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    fn build_from_ios(conn: &Connection) -> Result<Self> {
        // iOS backup contacts: ABPersonFullTextSearch_content with columns:
        // c0First (TEXT), c1Last (TEXT), c16Phone (TEXT: space-separated variants), c17Email (TEXT: space-separated)
        let mut index = HashMap::new();

        let mut stmt = conn.prepare(
            "SELECT c0First, c1Last, c16Phone, c17Email
             FROM ABPersonFullTextSearch_content",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name = Name::from_opt(
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            );

            if let Some(name) = name {
                if let Some(phones_blob) = row.get::<_, Option<String>>(2)? {
                    for token in phones_blob.split_whitespace() {
                        for key in phone_keys(token) {
                            upsert_best(&mut index, key, &name);
                        }
                    }
                }

                if let Some(emails_blob) = row.get::<_, Option<String>>(3)? {
                    for email in emails_blob.split_whitespace() {
                        if let Some(norm) = normalize_email(email) {
                            upsert_best(&mut index, norm, &name);
                        }
                    }
                }
            }
        }

        Ok(Self { index })
    }

    /// Look up a contact name by handle string.
    pub fn lookup(&self, id: &str) -> Option<Name> {
        // Look up each space-separated token
        for id_part in id.split_whitespace() {
            if looks_like_email(id_part) {
                if let Some(name) =
                    normalize_email(id_part).and_then(|k| self.index.get(&k).cloned())
                {
                    return Some(name);
                }
                continue;
            }

            for k in phone_keys(id_part) {
                if let Some(n) = self.index.get(&k) {
                    return Some(n.clone());
                }
            }
        }
        None
    }

    /// Build names keyed by deduplicated participant ID.
    ///
    /// - `participants`: map of handle ID to handle details
    /// - `deduped_handles`: map of handle ID to deduplicated handle ID
    /// - Returns: map of deduplicated handle ID to Name
    pub fn build_participants_map(
        &self,
        participants: &HashMap<i32, String>,
        deduped_handles: &HashMap<i32, i32>,
    ) -> HashMap<i32, Name> {
        let mut result: HashMap<i32, Name> = HashMap::new();

        for (&handle_id, details) in participants {
            let Some(&deduped_id) = deduped_handles.get(&handle_id) else {
                continue;
            };

            result
                .entry(deduped_id)
                .and_modify(|name| {
                    name.handle_ids.insert(handle_id);
                })
                .or_insert_with(|| {
                    let mut name = self
                        .lookup(details)
                        .unwrap_or_else(|| Name::from_details(details.clone()));

                    // Keep the original details string for display/fallback
                    name.details.clone_from(details);
                    name.handle_ids.insert(handle_id);
                    name
                });
        }

        result
    }
}

/// Return whether a table or view exists in the database.
fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = ?1 LIMIT 1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

/// Insert a name when it is more complete than the existing value.
fn upsert_best(map: &mut HashMap<String, Name>, key: String, incoming: &Name) {
    match map.get_mut(&key) {
        Some(existing) => {
            if incoming.score() > existing.score() {
                *existing = incoming.clone();
            }
        }
        None => {
            map.insert(key, incoming.clone());
        }
    }
}

// MARK: Email
/// `true` when the identifier looks like an email address.
fn looks_like_email(s: &str) -> bool {
    s.contains('@')
}

/// Normalize an email by trimming, lowercasing, and removing angle brackets.
fn normalize_email(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.trim_start_matches('<').trim_end_matches('>');
    if s.is_empty() {
        return None;
    }
    Some(s.to_lowercase())
}

/// Parse a space-separated email list.
fn parse_email_list(raw: &str) -> Vec<String> {
    if raw.contains(' ') {
        raw.split_whitespace().filter_map(normalize_email).collect()
    } else {
        normalize_email(raw).into_iter().collect()
    }
}

// MARK: Phone
/// Build lookup keys from a raw phone number.
///
/// - If the number contains "urn:", returns an empty vector
/// - If the number has fewer than [`MIN_PHONE_DIGITS`] digits, returns an empty vector
/// - Returns keys with and without '+' prefix
/// - For US numbers starting with +1 and 11 digits, also adds variants without the `+1` country code
fn phone_keys(raw: &str) -> Vec<String> {
    // Skip iMessage business accounts
    if raw.contains("urn:") {
        return vec![];
    }

    // The digits include the country code portion of the number
    let digits = to_phone_digits(raw);

    // Skip numbers that are too short to be valid phone numbers
    // This prevents matching on country codes alone (e.g., "+1" -> "1")
    if digits.len() < MIN_PHONE_DIGITS {
        return vec![];
    }

    let mut keys = vec![digits.clone(), format!("+{digits}")];

    // US numbers may appear with or without the country code.
    if digits.len() == 11 && raw.starts_with("+1") {
        let last_10 = &digits[digits.len() - 10..];
        keys.push(last_10.to_string());
        keys.push(format!("+{last_10}"));
    }

    keys.dedup();
    keys
}

/// Extract digits from a raw phone number string.
/// Keep only ASCII digits from a phone-like string.
fn to_phone_digits(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            out.push(ch);
        }
    }
    out
}

// MARK: macOS Dirs
/// Scans the macOS Contacts Sources directory (`~/Library/Application Support/AddressBook/Sources`)
/// for AddressBook-v22.abcddb database files.
fn find_macos_addressbook_db_paths() -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(macos_sources_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let db_path = path.join("AddressBook-v22.abcddb");
                if db_path.is_file() {
                    results.push(db_path);
                }
            }
        }
    }
    results
}

/// Resolve the standard macOS Contacts Sources directory: `~/Library/Application Support/AddressBook/Sources`
fn macos_sources_dir() -> PathBuf {
    PathBuf::from(&home())
        .join("Library")
        .join("Application Support")
        .join("AddressBook")
        .join("Sources")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handle is looked up by these keys, so a key the builder does not
    /// produce is a contact the reader never matches — the message arrives
    /// with a bare number instead of a name.
    ///
    /// None of these functions had a test. Each was reported as reachable but
    /// unguarded: `phone_keys` could return an empty vector, `to_phone_digits`
    /// an empty string, `normalize_email` `None`, and every message in an
    /// iMessage import would come through unnamed with the suite green.
    #[test]
    fn a_phone_number_yields_the_keys_a_lookup_might_use() {
        // A US number is stored both ways, because Apple records it either
        // way depending on how the message was addressed.
        let us = phone_keys("+1 (555) 123-4567");
        assert!(us.contains(&"15551234567".to_string()), "{us:?}");
        assert!(us.contains(&"+15551234567".to_string()), "{us:?}");
        assert!(us.contains(&"5551234567".to_string()), "{us:?}");
        assert!(us.contains(&"+5551234567".to_string()), "{us:?}");

        // A non-US number keeps its country code and gains no ten-digit form,
        // because the last ten digits of a UK number are not the number.
        let uk = phone_keys("+44 20 7183 8750");
        assert!(uk.contains(&"442071838750".to_string()), "{uk:?}");
        assert!(uk.contains(&"+442071838750".to_string()), "{uk:?}");
        assert_eq!(uk.len(), 2, "no ten-digit variant for a UK number: {uk:?}");
    }

    #[test]
    fn a_handle_that_is_not_a_phone_number_yields_no_keys() {
        // An iMessage business account is not a person.
        assert!(phone_keys("urn:biz:apple").is_empty());
        // Too few digits to be a number: a country code on its own must not
        // become a key that matches every number in that country.
        assert!(phone_keys("+1").is_empty());
        assert!(phone_keys("123").is_empty());
        assert!(phone_keys("").is_empty());
    }

    #[test]
    fn phone_digits_are_the_digits_and_nothing_else() {
        assert_eq!(to_phone_digits("+1 (555) 123-4567"), "15551234567");
        assert_eq!(to_phone_digits("555.123.4567"), "5551234567");
        assert_eq!(to_phone_digits("no digits here"), "");
        assert_eq!(to_phone_digits(""), "");
    }

    #[test]
    fn an_email_is_lower_cased_and_stripped_of_its_brackets() {
        assert_eq!(
            normalize_email("  <Alice@Example.COM>  "),
            Some("alice@example.com".to_string())
        );
        assert_eq!(
            normalize_email("alice@example.com"),
            Some("alice@example.com".to_string())
        );
        // Nothing but whitespace or brackets is not an address.
        assert_eq!(normalize_email("   "), None);
        assert_eq!(normalize_email("<>"), None);
        assert_eq!(normalize_email(""), None);
    }

    #[test]
    fn an_email_list_splits_on_spaces_and_a_single_address_does_not() {
        assert_eq!(
            parse_email_list("Alice@Example.com  bob@example.com"),
            vec![
                "alice@example.com".to_string(),
                "bob@example.com".to_string()
            ]
        );
        assert_eq!(
            parse_email_list("alice@example.com"),
            vec!["alice@example.com".to_string()]
        );
        assert!(parse_email_list("   ").is_empty());
    }

    #[test]
    fn an_identifier_is_an_email_when_it_has_an_at_sign() {
        assert!(looks_like_email("alice@example.com"));
        assert!(!looks_like_email("+15551234567"));
        assert!(!looks_like_email(""));
    }

    /// When two address-book rows name the same handle, the fuller name wins.
    /// `upsert_best` is what decides, and replacing it with "keep the first"
    /// or "keep the last" means a contact is named "Sam" or "" when the book
    /// has "Sam Example".
    #[test]
    fn the_fuller_name_wins_when_two_rows_name_one_handle() {
        let first_only = Name::from_opt(Some("Sam".into()), None).expect("a first name");
        let full = Name::from_opt(Some("Sam".into()), Some("Example".into())).expect("a full name");

        // Fuller arriving second replaces the sparser one.
        let mut map = HashMap::new();
        upsert_best(&mut map, "key".into(), &first_only);
        upsert_best(&mut map, "key".into(), &full);
        assert_eq!(map["key"].last, "Example");

        // Fuller arriving first is not replaced by the sparser one.
        let mut map = HashMap::new();
        upsert_best(&mut map, "key".into(), &full);
        upsert_best(&mut map, "key".into(), &first_only);
        assert_eq!(
            map["key"].last, "Example",
            "a sparser row must not overwrite a fuller one"
        );
    }
}
