//! Writes the demo's address book and the small config files the server reads
//! when it builds the Demo Account.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::personas::{OWNER_EMAIL, OWNER_PHONE, Roster};

/// The columns of the address book, as the server's Export writes them.
const ADDRESS_BOOK_HEADER: &str = "contact_id,display_name,groups,service,identity_type,identity";

/// Write `contacts.csv`, the demo's address book, in the format the server
/// exports and loads: one row per identity.
///
/// The demo is built the way a person builds theirs. The three backups are
/// imported first and carry no names for these people, so each arrives as an
/// Unknown. The server's Demo Account build then loads this file, which names
/// them: every contact here is a new one under a key of the file's own
/// (`demo-1`, `demo-2`, ...), and its identities move to it from the Unknown
/// the import made. A contact on WhatsApp lists its number once for each service, so the
/// Unknown is left holding nothing and goes.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_address_book(config_dir: &Path, roster: &Roster) -> Result<()> {
    let contacts_path = config_dir.join("contacts.csv");
    let mut out = String::from(ADDRESS_BOOK_HEADER);
    out.push('\n');
    for (index, c) in roster.contacts.iter().enumerate() {
        let lead = format!(
            "demo-{},{},{}",
            index + 1,
            csv_field(&c.display_hint()),
            csv_field(&c.groups.join(";"))
        );
        for phone in &c.phones {
            writeln!(out, "{lead},phone,phone,{}", csv_field(phone))?;
        }
        if c.has_whatsapp {
            writeln!(
                out,
                "{lead},whatsapp,phone,{}",
                csv_field(c.primary_phone())
            )?;
        }
    }
    fs::write(&contacts_path, out).with_context(|| format!("write {}", contacts_path.display()))?;
    Ok(())
}

/// One CSV field: quoted when it holds a comma, a quote, or a line break.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Write `seed.toml` with the demo account name, phone, and username.
///
/// The server reads this file only when it builds the Demo Account:
/// `reset-demo`, `serve` on a new database, and the Owner Home action.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_seed_toml(config_dir: &Path) -> Result<()> {
    let path = config_dir.join("seed.toml");
    let body = format!(
        r#"# Demo account identity, read only when the server builds the Demo Account
# (`reset-demo`, `serve` on a new database, the Owner Home action).

[owner]
display_name = "Demo User"
# (raw handle, handle type) pairs linked into `account_handles` by reset-demo.
handle_specs = [["{OWNER_PHONE}", "phone"]]
# Each email is an identity too: reset-demo writes it to `account_emails` and
# links it into `account_handles`.
emails = ["{OWNER_EMAIL}"]

[account]
username = "demo"
"#
    );
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
