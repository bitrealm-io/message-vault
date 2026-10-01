//! Writes contact cards and the small config files used by `reset-demo`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::personas::{OWNER_EMAIL, OWNER_PHONE, Roster};

/// Write `contacts.vcf`, one vCard (a contact card) per roster contact.
///
/// Cards with no name use the primary phone as the display name so the file
/// stays valid.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_vcf(config_dir: &Path, roster: &Roster) -> Result<()> {
    let contacts_path = config_dir.join("contacts.vcf");
    let mut out = String::new();
    for c in &roster.contacts {
        let display_name = c.display_hint();
        writeln!(out, "BEGIN:VCARD")?;
        writeln!(out, "VERSION:3.0")?;
        if display_name.is_empty() {
            // A card with no name still needs FN. Use the primary phone so the
            // vCard stays valid.
            writeln!(out, "FN:{}", escape_vcf(c.primary_phone()))?;
            writeln!(out, "N:;;;;")?;
        } else {
            writeln!(out, "FN:{}", escape_vcf(&display_name))?;
            writeln!(
                out,
                "N:{};{};{};;",
                escape_vcf(&c.last_name),
                escape_vcf(&c.first_name),
                escape_vcf(&c.middle_name)
            )?;
        }
        for phone in &c.phones {
            writeln!(out, "TEL:{}", escape_vcf(phone))?;
        }
        if !c.groups.is_empty() {
            let mut categories = Vec::with_capacity(c.groups.len());
            for group in &c.groups {
                categories.push(escape_vcf(group));
            }
            writeln!(out, "CATEGORIES:{}", categories.join(","))?;
        }
        writeln!(out, "END:VCARD")?;
    }
    fs::write(&contacts_path, out).with_context(|| format!("write {}", contacts_path.display()))?;
    Ok(())
}

/// Escape characters that would break a vCard field (`\`, newlines, `;`, `,`).
fn escape_vcf(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

/// Write `config.toml` with the database and asset folder paths used after a demo reset.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_config_toml(config_dir: &Path) -> Result<()> {
    let path = config_dir.join("config.toml");
    let body = r#"# Instance config restored by `reset-demo`.
# Demo account identity lives in crates/server/demo-seed/config/seed.toml.

[paths]
db = "data/messagecrate.db"
data_dir = "data"
assets_dir = "assets"
assets_converted_dir = "assets_converted"

# Optional HTTP import API for local development.
# [server]
# bind = "127.0.0.1:8080"
"#;
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Write `seed.toml` with the demo account name, phone, and username.
///
/// This file is only used by `reset-demo`. It is not copied into the running
/// server config.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_seed_toml(config_dir: &Path) -> Result<()> {
    let path = config_dir.join("seed.toml");
    let body = format!(
        r#"# Demo account identity used only by `reset-demo`.
# Not copied into the runtime config.toml.

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
