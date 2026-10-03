//! The address book: Message Crate's own CSV of contacts and their
//! identities, one row per identity.
//!
//! [`export_csv`] writes it and [`load`] reads it back. The rules are in
//! `docs/architecture/contacts-identities-and-messages.md`, the five starting
//! at "The address book is a file for editing contacts, not a source of
//! them": a load is Append or Edit, touches only the contacts in the file, is
//! strict, refuses whole, and moves an identity only from a contact the load
//! may change.
//!
//! A load reads the account's contacts, identities and Contact Groups once,
//! checks every row against that picture, and writes only when no row was
//! refused. Everything runs in one transaction.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result};
use message_ir::{HandleService, HandleType};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::db::contacts::{self, Origin};
use crate::db::named_membership;
use crate::db::{WriteTx, begin_write};

/// The columns of the file, in the order Export writes them.
pub const COLUMNS: [&str; 6] = [
    "contact_id",
    "display_name",
    "groups",
    "service",
    "identity_type",
    "identity",
];

/// What separates Contact Group names in the `groups` column. A Contact
/// Group's name may not hold it (`named_membership::group_spec`), so the cell
/// needs no escaping and an export loads back as the groups it was written from.
pub(crate) const GROUP_SEPARATOR: char = ';';

/// How a load applies the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoadMode {
    /// Create the contacts the file names, rename the ones it holds, and add
    /// the identities and Contact Group memberships it lists. Nothing is
    /// removed.
    #[default]
    Append,
    /// Append, and then make each contact in the file hold exactly the
    /// identities and memberships its rows list.
    Edit,
}

/// What a load changed.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct LoadCounts {
    /// Contacts the load created.
    pub contacts_created: u64,
    /// Contacts the load renamed, or whose identities or Contact Group
    /// memberships it changed.
    pub contacts_updated: u64,
    /// Contacts the load deleted: the ones it left with neither a name nor
    /// an identity.
    pub contacts_deleted: u64,
    /// Identities linked to a contact that no contact held before.
    pub identities_added: u64,
    /// Identities taken from one contact and given to another.
    pub identities_moved: u64,
    /// Identities taken off a contact, which only Edit does.
    pub identities_removed: u64,
    /// Contact Groups the load created.
    pub groups_created: u64,
    /// One sentence for each phone number the file wrote without `+` that
    /// the load matched to the `+` key its contact holds, or that became a
    /// new identity. Each starts with its row number. A spreadsheet can drop
    /// the `+` from a number without showing it, so the load says how it
    /// read the number.
    pub notes: Vec<String>,
}

/// Why a load did not happen.
#[derive(Debug)]
pub enum LoadError {
    /// The file broke a rule. Each sentence names a row and its reason, and
    /// nothing was written.
    Refused(Vec<String>),
    /// Something failed that changing the file would not help.
    Failed(anyhow::Error),
}

impl From<sqlx::Error> for LoadError {
    fn from(error: sqlx::Error) -> Self {
        Self::Failed(error.into())
    }
}

impl From<anyhow::Error> for LoadError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(reasons) => {
                write!(f, "the address book was refused: {}", reasons.join("; "))
            }
            Self::Failed(cause) => write!(f, "{cause:#}"),
        }
    }
}

/// The key of one identity: the three columns `handles` is unique on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IdentityKey {
    service: &'static str,
    handle_type: &'static str,
    normalized: String,
}

/// One data row of the file, fields trimmed.
#[derive(Debug)]
struct FileRow {
    /// The row as a spreadsheet numbers it: the header is row 1.
    number: usize,
    contact_id: String,
    display_name: String,
    groups: String,
    service: String,
    identity_type: String,
    identity: String,
}

/// Which contact a group of rows speaks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Target {
    /// A contact the account holds, by its id.
    Known(i64),
    /// A contact the load creates.
    New,
}

/// One identity a file contact lists.
#[derive(Debug)]
struct FileIdentity {
    row: usize,
    key: IdentityKey,
    /// The identity as the file wrote it, kept as the `raw` of a new row.
    written: String,
    /// What the load says about how it read this row, for
    /// [`LoadCounts::notes`].
    note: Option<String>,
}

/// The rows of one contact, gathered.
#[derive(Debug)]
struct FileContact {
    target: Target,
    /// The `contact_id` text, for a refusal to quote.
    id_text: String,
    first_row: usize,
    /// The name the rows agree on, and the row that first gave it.
    name: Option<(String, usize)>,
    /// The Contact Group names the rows agree on, and the row that first
    /// gave them. `None` when every row left the column blank.
    groups: Option<(Vec<String>, usize)>,
    identities: Vec<FileIdentity>,
}

impl FileContact {
    /// How a refusal names this contact.
    fn describe(&self) -> String {
        let name = match &self.name {
            Some((name, _)) => format!("\"{name}\""),
            None => "the contact with no name".to_string(),
        };
        match self.target {
            Target::Known(id) => format!("{name} (contact {id})"),
            Target::New if self.id_text.is_empty() => {
                format!("{name} (a new contact, row {})", self.first_row)
            }
            Target::New => format!("{name} (a new contact, contact_id {})", self.id_text),
        }
    }
}

/// What the account holds when the load starts.
#[derive(Debug, Default)]
struct Snapshot {
    /// Live contacts: id to trimmed name.
    contacts: HashMap<i64, String>,
    /// Trashed contacts: id to trimmed name. A trashed contact is not in
    /// Contacts, so the file cannot speak for it, but it can hold an identity.
    trashed: HashMap<i64, String>,
    /// Every identity: its row id and the contact holding it, if any.
    handles: HashMap<IdentityKey, (i64, Option<i64>)>,
    /// Contact Groups by lower-cased name.
    groups: HashMap<String, i64>,
    /// Which groups each contact is a member of.
    memberships: HashMap<i64, HashSet<i64>>,
}

impl Snapshot {
    /// Read the account's contacts, identities and Contact Groups.
    async fn read(conn: &mut SqliteConnection, account_id: i64) -> Result<Self> {
        let mut snapshot = Self::default();
        let rows: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT ct.id, trim(ct.preferred_name),
                    EXISTS (SELECT 1 FROM trashed_contacts t
                            WHERE t.account_id = ct.account_id AND t.contact_id = ct.id)
             FROM contacts ct WHERE ct.account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;
        for (id, name, trashed) in rows {
            if trashed != 0 {
                snapshot.trashed.insert(id, name);
            } else {
                snapshot.contacts.insert(id, name);
            }
        }

        let rows: Vec<(i64, String, String, String, Option<i64>)> = sqlx::query_as(
            "SELECT h.id, h.service, h.handle_type, h.normalized, ch.contact_id
             FROM handles h
             LEFT JOIN contact_handles ch
               ON ch.account_id = h.account_id AND ch.handle_id = h.id
             WHERE h.account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;
        for (id, service, handle_type, normalized, holder) in rows {
            let (Some(service), Some(handle_type)) =
                (parse_service(&service), parse_handle_type(&handle_type))
            else {
                continue;
            };
            snapshot.handles.insert(
                IdentityKey {
                    service: service.as_str(),
                    handle_type: handle_type.as_str(),
                    normalized,
                },
                (id, holder),
            );
        }

        let rows: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, name FROM contact_groups WHERE account_id = $1")
                .bind(account_id)
                .fetch_all(&mut *conn)
                .await?;
        for (id, name) in rows {
            snapshot.groups.insert(name.to_lowercase(), id);
        }

        let rows: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT m.contact_id, m.group_id
             FROM contact_group_members m
             JOIN contact_groups g ON g.id = m.group_id
             WHERE g.account_id = $1",
        )
        .bind(account_id)
        .fetch_all(&mut *conn)
        .await?;
        for (contact_id, group_id) in rows {
            snapshot
                .memberships
                .entry(contact_id)
                .or_default()
                .insert(group_id);
        }
        Ok(snapshot)
    }

    /// The name of a contact, live or trashed.
    fn name_of(&self, contact_id: i64) -> &str {
        self.contacts
            .get(&contact_id)
            .or_else(|| self.trashed.get(&contact_id))
            .map_or("", String::as_str)
    }

    /// How a sentence names a contact the account holds.
    fn describe(&self, contact_id: i64) -> String {
        match self.name_of(contact_id) {
            "" => format!("the contact with no name (contact {contact_id})"),
            name => format!("\"{name}\" (contact {contact_id})"),
        }
    }

    /// Whether `contact_id` holds the identity `key`.
    fn holds(&self, contact_id: i64, key: &IdentityKey) -> bool {
        self.handles
            .get(key)
            .is_some_and(|&(_, holder)| holder == Some(contact_id))
    }
}

/// The `service` value the `handles` table stores, or `None` for any other
/// text. Unlike [`HandleService::parse`], which reads every unknown word as
/// the phone platform, an unknown word here is an error.
fn parse_service(text: &str) -> Option<HandleService> {
    [HandleService::Phone, HandleService::Whatsapp]
        .into_iter()
        .find(|s| s.as_str().eq_ignore_ascii_case(text))
}

/// The `handle_type` value the `handles` table stores, or `None` for any
/// other text.
fn parse_handle_type(text: &str) -> Option<HandleType> {
    [
        HandleType::Phone,
        HandleType::Email,
        HandleType::Username,
        HandleType::Other,
    ]
    .into_iter()
    .find(|t| t.as_str().eq_ignore_ascii_case(text))
}

/// What a spreadsheet reads as the start of a formula when a cell begins
/// with it.
const FORMULA_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// The `'` a spreadsheet reads as "this cell is text".
const TEXT_MARK: char = '\'';

/// A cell as Export writes it: with a `'` in front when a spreadsheet would
/// otherwise run it as a formula, or, for a phone number, drop its `+`. A
/// cell that already starts with `'`s and then one of those characters gets
/// one more `'`, so the one [`read_cell`] takes off leaves the cell as it
/// was.
fn written_cell(cell: &str) -> std::borrow::Cow<'_, str> {
    if cell
        .trim_start_matches(TEXT_MARK)
        .starts_with(FORMULA_STARTS)
    {
        format!("{TEXT_MARK}{cell}").into()
    } else {
        cell.into()
    }
}

/// A cell as the load reads it: one `'` taken off when one of the characters
/// a spreadsheet reads as a formula follows it, which undoes
/// [`written_cell`]. A spreadsheet can keep that `'` when it saves or drop
/// it, and both read the same.
fn read_cell(cell: &str) -> &str {
    match cell.strip_prefix(TEXT_MARK) {
        Some(rest)
            if rest
                .trim_start_matches(TEXT_MARK)
                .starts_with(FORMULA_STARTS) =>
        {
            rest
        }
        _ => cell,
    }
}

/// Read the CSV into rows. A row whose every field is blank is skipped, the
/// way a spreadsheet's trailing empty rows are. A row with fewer fields than
/// the header reads its missing trailing cells as blank, since a spreadsheet
/// can drop empty cells at the end of a row and no column moves. A row with
/// more fields than the header is an error: its cells cannot be matched to
/// the columns, most often because a cell holding a comma lacks quotes.
fn read_rows(csv_text: &str) -> Result<Vec<FileRow>, Vec<String>> {
    let text = csv_text.strip_prefix('\u{feff}').unwrap_or(csv_text);
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| vec![format!("the file is not CSV: {e}")])?
        .clone();
    let mut index = [0usize; 6];
    let mut missing = Vec::new();
    for (slot, column) in COLUMNS.iter().enumerate() {
        match headers.iter().position(|h| h.eq_ignore_ascii_case(column)) {
            Some(at) => index[slot] = at,
            None => missing.push(*column),
        }
    }
    if !missing.is_empty() {
        return Err(vec![format!(
            "row 1: the header is missing {}; the columns are {}",
            missing.join(", "),
            COLUMNS.join(", ")
        )]);
    }

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (at, record) in reader.records().enumerate() {
        let number = at + 2;
        let record = match record {
            Ok(record) => record,
            Err(e) => {
                errors.push(format!("row {number}: not CSV: {e}"));
                continue;
            }
        };
        if record.len() > headers.len() && !record.iter().all(str::is_empty) {
            errors.push(format!(
                "row {number}: has {} fields where the header has {}; \
                 put a cell that holds a comma in double quotes",
                record.len(),
                headers.len()
            ));
            continue;
        }
        let field =
            |slot: usize| read_cell(record.get(index[slot]).unwrap_or("").trim()).to_string();
        let row = FileRow {
            number,
            contact_id: field(0),
            display_name: field(1),
            groups: field(2),
            service: field(3),
            identity_type: field(4),
            identity: field(5),
        };
        let blank = row.contact_id.is_empty()
            && row.display_name.is_empty()
            && row.groups.is_empty()
            && row.service.is_empty()
            && row.identity_type.is_empty()
            && row.identity.is_empty();
        if !blank {
            rows.push(row);
        }
    }
    if errors.is_empty() {
        Ok(rows)
    } else {
        Err(errors)
    }
}

/// The identity a row lists, keyed the way the `handles` table keys it.
/// `Ok(None)` for a row that lists no identity: one that leaves `service`,
/// `identity_type` and `identity` all blank, which is how a contact with no
/// identity is written.
///
/// `contact` is the contact the row names, when the account holds it. A
/// phone number written without `+` that no key matches as written is read
/// as `+` and its digits when that contact holds that key, because a
/// spreadsheet that opens the file can save `+6591234567` as the number
/// `6591234567`. Other contacts' identities are not looked at, so a dropped
/// `+` never attaches another person's number to this one.
fn row_identity(
    row: &FileRow,
    contact: Option<i64>,
    snapshot: &Snapshot,
) -> Result<Option<FileIdentity>, String> {
    let n = row.number;
    if row.service.is_empty() && row.identity_type.is_empty() && row.identity.is_empty() {
        return Ok(None);
    }
    let Some(service) = parse_service(&row.service) else {
        return Err(format!(
            "row {n}: service \"{}\" is not one Message Crate stores; use phone or whatsapp",
            row.service
        ));
    };
    let Some(handle_type) = parse_handle_type(&row.identity_type) else {
        return Err(format!(
            "row {n}: identity_type \"{}\" is not one Message Crate stores; use phone, email, username or other",
            row.identity_type
        ));
    };
    if row.identity.is_empty() {
        return Err(format!("row {n}: identity is blank"));
    }
    let key = |normalized: String| IdentityKey {
        service: service.as_str(),
        handle_type: handle_type.as_str(),
        normalized,
    };
    // An identity written exactly as the database keys it is one Export
    // wrote, so it is accepted as it stands: a file loaded straight back must
    // never be refused over a key an import stored.
    let verbatim = key(row.identity.clone());
    let mut note = None;
    let normalized = if snapshot.handles.contains_key(&verbatim) {
        row.identity.clone()
    } else {
        match handle_type {
            HandleType::Phone => {
                if phone::sanitize_phone_shaped(&row.identity).is_none() {
                    return Err(format!(
                        "row {n}: \"{}\" is not a phone number Message Crate can key: \
                         it needs 4 to 15 digits and nothing but digits, spaces and + - ( ) .",
                        row.identity
                    ));
                }
                let keyed = phone::normalize_typed_handle(&row.identity, HandleType::Phone).0;
                if row.identity.contains('+') {
                    keyed
                } else {
                    let digits: String =
                        row.identity.chars().filter(char::is_ascii_digit).collect();
                    let with_plus = format!("+{digits}");
                    let holder = contact.filter(|&id| {
                        with_plus != keyed && snapshot.holds(id, &key(with_plus.clone()))
                    });
                    if let Some(id) = holder {
                        if snapshot.holds(id, &key(keyed.clone())) {
                            return Err(format!(
                                "row {n}: {} has no +, and {} holds both {with_plus} and {keyed}; \
                                 write the number with its + to say which",
                                row.identity,
                                snapshot.describe(id)
                            ));
                        }
                        note = Some(format!(
                            "row {n}: {} has no +, so it was read as {with_plus}, which {} holds",
                            row.identity,
                            snapshot.describe(id)
                        ));
                        with_plus
                    } else {
                        if !snapshot.handles.contains_key(&key(keyed.clone())) {
                            note = Some(format!(
                                "row {n}: {} has no +, so it became the new identity {keyed}",
                                row.identity
                            ));
                        }
                        keyed
                    }
                }
            }
            HandleType::Email => {
                let lowered = row.identity.to_lowercase();
                let mut parts = lowered.split('@');
                let well_formed = matches!(
                    (parts.next(), parts.next(), parts.next()),
                    (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty()
                ) && !lowered.chars().any(char::is_whitespace);
                if !well_formed {
                    return Err(format!(
                        "row {n}: \"{}\" is not an email address: it needs one @ with text on both sides",
                        row.identity
                    ));
                }
                lowered
            }
            HandleType::Username | HandleType::Other => row.identity.clone(),
        }
    };
    Ok(Some(FileIdentity {
        row: n,
        key: key(normalized),
        written: row.identity.clone(),
        note,
    }))
}

/// The Contact Group names one `groups` cell lists, in the order written,
/// each once. `Err` names the first one the product would not let a person
/// create.
fn row_groups(row: &FileRow) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for name in row.groups.split(GROUP_SEPARATOR).map(str::trim) {
        if name.is_empty() {
            continue;
        }
        let name = named_membership::check_name(named_membership::group_spec(), name)
            .map_err(|reason| format!("row {}: Contact Group \"{name}\": {reason}", row.number))?;
        if !names
            .iter()
            .any(|n| n.to_lowercase() == name.to_lowercase())
        {
            names.push(name);
        }
    }
    Ok(names)
}

/// The same set of group names, whatever their order or case.
fn same_groups(a: &[String], b: &[String]) -> bool {
    let fold =
        |names: &[String]| -> BTreeSet<String> { names.iter().map(|n| n.to_lowercase()).collect() };
    fold(a) == fold(b)
}

/// How rows are gathered into contacts before they are numbered.
#[derive(Debug, PartialEq, Eq, Hash)]
enum GroupingKey {
    Known(i64),
    Text(String),
    /// A blank `contact_id`: every such row is a contact of its own.
    Blank(usize),
}

/// Check every row and gather the rows into contacts. Every broken rule is
/// collected, so one refusal names them all.
fn plan(rows: &[FileRow], snapshot: &Snapshot) -> Result<Vec<FileContact>, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    let mut file: Vec<FileContact> = Vec::new();
    let mut by_key: HashMap<GroupingKey, usize> = HashMap::new();
    // Where each identity was first listed: the contact and the row.
    let mut listed: HashMap<IdentityKey, (usize, usize)> = HashMap::new();

    for row in rows {
        let n = row.number;
        let known = row
            .contact_id
            .parse::<i64>()
            .ok()
            .filter(|id| snapshot.contacts.contains_key(id));
        let grouping = match known {
            Some(id) => GroupingKey::Known(id),
            None if row.contact_id.is_empty() => GroupingKey::Blank(n),
            None => GroupingKey::Text(row.contact_id.clone()),
        };
        let at = *by_key.entry(grouping).or_insert_with(|| {
            file.push(FileContact {
                target: known.map_or(Target::New, Target::Known),
                id_text: row.contact_id.clone(),
                first_row: n,
                name: None,
                groups: None,
                identities: Vec::new(),
            });
            file.len() - 1
        });
        let contact = &mut file[at];

        if !row.display_name.is_empty() {
            match &contact.name {
                None => contact.name = Some((row.display_name.clone(), n)),
                Some((name, first)) if *name != row.display_name => errors.push(format!(
                    "row {n}: display_name \"{}\" disagrees with \"{name}\" on row {first}; \
                     the rows of contact_id {} must agree or be blank",
                    row.display_name, contact.id_text
                )),
                Some(_) => {}
            }
        }

        if !row.groups.is_empty() {
            match row_groups(row) {
                Err(reason) => errors.push(reason),
                Ok(names) => match &contact.groups {
                    None => contact.groups = Some((names, n)),
                    Some((first_names, first)) if !same_groups(first_names, &names) => {
                        errors.push(format!(
                            "row {n}: groups \"{}\" disagrees with \"{}\" on row {first}; \
                             the rows of contact_id {} must agree or be blank",
                            names.join("; "),
                            first_names.join("; "),
                            contact.id_text
                        ));
                    }
                    Some(_) => {}
                },
            }
        }

        match row_identity(row, known, snapshot) {
            Err(reason) => errors.push(reason),
            Ok(None) => {}
            Ok(Some(identity)) => match listed.get(&identity.key) {
                Some(&(other, first)) if other != at => errors.push(format!(
                    "row {n}: {} is also on row {first} under another contact_id; \
                     an identity belongs to one contact",
                    identity.key.normalized
                )),
                // The same identity twice under one contact says it once.
                Some(_) => {}
                None => {
                    listed.insert(identity.key.clone(), (at, n));
                    contact.identities.push(identity);
                }
            },
        }
    }

    let in_file: HashSet<i64> = file
        .iter()
        .filter_map(|c| match c.target {
            Target::Known(id) => Some(id),
            Target::New => None,
        })
        .collect();
    for contact in &file {
        if contact.target == Target::New && contact.name.is_none() && contact.identities.is_empty()
        {
            errors.push(format!(
                "row {}: a new contact needs a display_name or an identity",
                contact.first_row
            ));
        }
        for identity in &contact.identities {
            let Some(&(_, Some(holder))) = snapshot.handles.get(&identity.key) else {
                continue;
            };
            if contact.target == Target::Known(holder) {
                continue;
            }
            let holder_name = snapshot.name_of(holder);
            if holder_name.is_empty() || in_file.contains(&holder) {
                continue;
            }
            // A trashed holder cannot be added to the file, because its id
            // reads as unknown text, so the way through is the Trash.
            let (where_it_is, way_through) = if snapshot.trashed.contains_key(&holder) {
                (
                    "is in the Trash",
                    format!(
                        "restore \"{holder_name}\" and add it to the file, \
                         or delete \"{holder_name}\" for good,"
                    ),
                )
            } else {
                (
                    "is not in the file",
                    format!("add \"{holder_name}\" to the file"),
                )
            };
            errors.push(format!(
                "row {}: {} belongs to \"{holder_name}\" (contact {holder}), which {where_it_is}, \
                 so it cannot move to {}; {way_through} to move it",
                identity.row,
                identity.key.normalized,
                contact.describe()
            ));
        }
    }

    if errors.is_empty() {
        Ok(file)
    } else {
        Err(errors)
    }
}

/// Load an address book into the account.
///
/// The whole load is one transaction. A file that breaks a rule is refused
/// whole, with one sentence for each bad row, and nothing is written.
///
/// # Errors
///
/// [`LoadError::Refused`] when the file breaks a rule; [`LoadError::Failed`]
/// when a statement fails.
pub async fn load(
    conn: &mut SqliteConnection,
    account_id: i64,
    csv_text: &str,
    mode: LoadMode,
) -> Result<LoadCounts, LoadError> {
    let rows = read_rows(csv_text).map_err(LoadError::Refused)?;
    // A write transaction from the first read: the plan is checked against
    // the contacts the writes then change, and an import that commits
    // meanwhile waits instead of failing the load.
    let mut tx = begin_write(conn).await?;
    let snapshot = Snapshot::read(&mut tx, account_id).await?;
    let file = plan(&rows, &snapshot).map_err(LoadError::Refused)?;
    let counts = apply(&mut tx, account_id, &snapshot, &file, mode).await?;
    tx.commit().await?;
    Ok(counts)
}

/// Write a checked file. Nothing here refuses: [`plan`] already has.
async fn apply(
    tx: &mut WriteTx<'_>,
    account_id: i64,
    snapshot: &Snapshot,
    file: &[FileContact],
    mode: LoadMode,
) -> Result<LoadCounts> {
    let conn: &mut SqliteConnection = tx;
    let mut counts = LoadCounts::default();
    let mut groups = snapshot.groups.clone();
    // Who holds each identity as the load goes: it changes as rows move them.
    let mut holder_of: HashMap<i64, i64> = snapshot
        .handles
        .values()
        .filter_map(|&(handle_id, holder)| holder.map(|h| (handle_id, h)))
        .collect();
    // Contacts the account already held that this load changed, and the ones
    // it took an identity from.
    let mut changed: HashSet<i64> = HashSet::new();
    let mut lost_identity: HashSet<i64> = HashSet::new();
    // Each file contact's id and the identity rows it lists.
    let mut placed: Vec<(i64, HashSet<i64>)> = Vec::with_capacity(file.len());

    // First every contact takes what its rows list. Removal waits until all
    // of them have, so an identity that goes from one file contact to another
    // is one move, whichever of the two comes first in the file.
    for contact in file {
        let contact_id = match contact.target {
            Target::Known(id) => {
                // The file is the person typing, so its name replaces
                // whatever the contact carried; `propose_name` holds the rule.
                if let Some((name, _)) = &contact.name
                    && contacts::propose_name(conn, account_id, id, name, Origin::AddressBook)
                        .await?
                {
                    changed.insert(id);
                }
                id
            }
            Target::New => {
                let name = contact.name.as_ref().map_or("", |(name, _)| name.as_str());
                counts.contacts_created += 1;
                contacts::create_contact(conn, account_id, name, Origin::AddressBook).await?
            }
        };

        let mut listed: HashSet<i64> = HashSet::new();
        for identity in &contact.identities {
            let handle_id = match snapshot.handles.get(&identity.key) {
                Some(&(handle_id, _)) => handle_id,
                None => {
                    sqlx::query_scalar(
                        "INSERT INTO handles (account_id, raw, normalized, handle_type, service, origin)
                         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
                    )
                    .bind(account_id)
                    .bind(&identity.written)
                    .bind(&identity.key.normalized)
                    .bind(identity.key.handle_type)
                    .bind(identity.key.service)
                    .bind(Origin::AddressBook.as_str())
                    .fetch_one(&mut *conn)
                    .await?
                }
            };
            listed.insert(handle_id);
            match holder_of.insert(handle_id, contact_id) {
                Some(holder) if holder == contact_id => {}
                Some(holder) => {
                    let goes = contacts::IdentityGoes::To(contact_id, Origin::AddressBook);
                    contacts::move_identity(conn, account_id, handle_id, goes).await?;
                    counts.identities_moved += 1;
                    lost_identity.insert(holder);
                    changed.insert(holder);
                    changed.insert(contact_id);
                }
                None => {
                    contacts::link_handle_to_contact(
                        conn,
                        account_id,
                        handle_id,
                        contact_id,
                        Origin::AddressBook,
                    )
                    .await?;
                    counts.identities_added += 1;
                    changed.insert(contact_id);
                }
            }
        }

        let mut wanted: HashSet<i64> = HashSet::new();
        for name in contact.groups.iter().flat_map(|(names, _)| names) {
            let group_id = match groups.get(&name.to_lowercase()) {
                Some(&id) => id,
                None => {
                    let id: i64 = sqlx::query_scalar(
                        "INSERT INTO contact_groups (account_id, name) VALUES ($1, $2) RETURNING id",
                    )
                    .bind(account_id)
                    .bind(name)
                    .fetch_one(&mut *conn)
                    .await?;
                    groups.insert(name.to_lowercase(), id);
                    counts.groups_created += 1;
                    id
                }
            };
            wanted.insert(group_id);
        }
        let held = snapshot.memberships.get(&contact_id);
        for &group_id in &wanted {
            if held.is_some_and(|h| h.contains(&group_id)) {
                continue;
            }
            sqlx::query(
                "INSERT INTO contact_group_members (contact_id, group_id) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            )
            .bind(contact_id)
            .bind(group_id)
            .execute(&mut *conn)
            .await?;
            changed.insert(contact_id);
        }
        if mode == LoadMode::Edit {
            for &group_id in held.into_iter().flatten() {
                if wanted.contains(&group_id) {
                    continue;
                }
                sqlx::query(
                    "DELETE FROM contact_group_members WHERE contact_id = $1 AND group_id = $2",
                )
                .bind(contact_id)
                .bind(group_id)
                .execute(&mut *conn)
                .await?;
                changed.insert(contact_id);
            }
        }
        placed.push((contact_id, listed));
    }

    // Edit: an identity a file contact still holds and no row of it lists
    // comes off the contact, the one way an identity leaves a contact: one in
    // a conversation goes to a new contact with no name, so the person is
    // Unknown for it again, and one nothing uses is deleted.
    if mode == LoadMode::Edit {
        for (contact_id, listed) in &placed {
            let mut unlisted: Vec<i64> = holder_of
                .iter()
                .filter(|&(handle_id, holder)| holder == contact_id && !listed.contains(handle_id))
                .map(|(&handle_id, _)| handle_id)
                .collect();
            unlisted.sort_unstable();
            contacts::take_identities_off(conn, account_id, &unlisted).await?;
            for handle_id in unlisted {
                holder_of.remove(&handle_id);
                counts.identities_removed += 1;
                lost_identity.insert(*contact_id);
                changed.insert(*contact_id);
            }
        }
    }

    // A contact this load left with neither a name nor an identity is one
    // nothing could ever reach, so it goes.
    for contact_id in lost_identity {
        if contacts::delete_if_empty(conn, account_id, contact_id).await? {
            changed.remove(&contact_id);
            counts.contacts_deleted += 1;
        }
    }

    let in_file: HashSet<i64> = file
        .iter()
        .filter_map(|c| match c.target {
            Target::Known(id) => Some(id),
            Target::New => None,
        })
        .collect();
    for contact_id in changed {
        contacts::touch_contact(conn, account_id, contact_id).await?;
        if in_file.contains(&contact_id) {
            counts.contacts_updated += 1;
        }
    }

    // The notes in the order of the file's rows, which a contact's rows need
    // not be.
    let mut notes: Vec<(usize, &String)> = file
        .iter()
        .flat_map(|contact| &contact.identities)
        .filter_map(|identity| identity.note.as_ref().map(|note| (identity.row, note)))
        .collect();
    notes.sort_by_key(|&(row, _)| row);
    counts.notes = notes.into_iter().map(|(_, note)| note.clone()).collect();
    Ok(counts)
}

/// One row of [`export_csv`]'s query: the contact's id and name, and one of
/// its identities as service, handle type and key, absent for a contact
/// with no identity.
type ExportRow = (i64, String, Option<String>, Option<String>, Option<String>);

/// Write the address book for the account's contacts, or for the ones in
/// `only` when it is given. One row per identity; a contact with no identity
/// is one row with the last three columns blank. Contacts in the trash are
/// left out, as they are from Contacts.
///
/// Rows are ordered by name, a contact with no name first, so the Unknowns
/// a person exports to name sit together at the top.
///
/// # Errors
///
/// Returns an error when a query fails.
pub async fn export_csv(
    conn: &mut SqliteConnection,
    account_id: i64,
    only: Option<&HashSet<i64>>,
) -> Result<String> {
    let rows: Vec<ExportRow> = sqlx::query_as(
        "SELECT ct.id, trim(ct.preferred_name), h.service, h.handle_type, h.normalized
         FROM contacts ct
         LEFT JOIN contact_handles ch
           ON ch.account_id = ct.account_id AND ch.contact_id = ct.id
         LEFT JOIN handles h ON h.id = ch.handle_id
         WHERE ct.account_id = $1
           AND NOT EXISTS (SELECT 1 FROM trashed_contacts t
                           WHERE t.account_id = ct.account_id AND t.contact_id = ct.id)
         ORDER BY lower(trim(ct.preferred_name)), ct.id,
                  h.handle_type, h.service, h.normalized",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    let memberships: Vec<(i64, String)> = sqlx::query_as(
        "SELECT m.contact_id, g.name
         FROM contact_group_members m
         JOIN contact_groups g ON g.id = m.group_id
         WHERE g.account_id = $1
         ORDER BY lower(g.name)",
    )
    .bind(account_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut groups: HashMap<i64, Vec<String>> = HashMap::new();
    for (contact_id, name) in memberships {
        groups.entry(contact_id).or_default().push(name);
    }

    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(COLUMNS)?;
    for (id, name, service, handle_type, normalized) in rows {
        if only.is_some_and(|only| !only.contains(&id)) {
            continue;
        }
        let group_names = groups
            .get(&id)
            .map(|names| names.join(&GROUP_SEPARATOR.to_string()))
            .unwrap_or_default();
        let id = id.to_string();
        let cells = [
            id.as_str(),
            name.as_str(),
            group_names.as_str(),
            service.as_deref().unwrap_or(""),
            handle_type.as_deref().unwrap_or(""),
            normalized.as_deref().unwrap_or(""),
        ]
        .map(written_cell);
        writer.write_record(cells.iter().map(|cell| cell.as_bytes()))?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|e| anyhow::anyhow!("finish the address book: {e}"))?;
    String::from_utf8(bytes).context("the address book is not UTF-8")
}

#[cfg(test)]
mod tests;
