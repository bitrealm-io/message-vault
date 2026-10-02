//! Loads generator settings: the two built-in sizes, or a settings file.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, de};

/// How much Demo Data to generate. The settings for each size are compiled
/// into the program, so the server can seed with no files beside it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum DemoSize {
    /// About 54,000 messages. A new Message Crate starts with this.
    #[default]
    Medium,
    /// About 613,000 messages.
    Large,
}

impl DemoSize {
    /// The size's name as typed on a command line: `medium` or `large`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    /// The settings file for this size, as compiled in.
    fn settings(self) -> &'static str {
        match self {
            Self::Medium => include_str!("../demo_seed_medium.toml"),
            Self::Large => include_str!("../demo_seed_large.toml"),
        }
    }
}

impl std::fmt::Display for DemoSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Generator settings: how many contacts, how conversations are split across
/// backups, and how often messages get photos or replies.
///
/// A key the generator does not use is an error, never ignored
/// (`deny_unknown_fields` here and on every section): a misspelt key would
/// otherwise be dropped and the run would differ from what the file says.
/// The refusal names the key, its line, and the keys that section takes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedConfig {
    /// Random seed. The same seed and settings produce the same backups.
    pub seed: u64,
    /// Folder the generated backups are written under.
    pub out: String,
    /// The "now" that every generated timestamp counts back from.
    #[serde(deserialize_with = "deserialize_reference_time")]
    pub reference_time: DateTime<Utc>,
    /// How many contacts to invent and how their handles are shaped.
    pub contacts: ContactsConfig,
    /// Contact labels and the share of contacts that get each one.
    pub labels: LabelsConfig,
    /// One-to-one conversation sizes and shapes.
    pub one_to_one: OneToOneConfig,
    /// Group conversation counts and sizes.
    pub groups: GroupsConfig,
    /// Message mix: attachments, replies, tapbacks, transports.
    pub messages: MessagesConfig,
    /// Deliberately awkward data: unassigned handles, orphans, empty threads.
    pub edge_cases: EdgeCasesConfig,
    /// How conversations are split across the backup folders.
    pub sources: SourcesConfig,
}

/// Read a timestamp string such as `2026-08-01T12:00:00Z` and convert it to UTC.
fn deserialize_reference_time<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    DateTime::parse_from_rfc3339(&value)
        .map(|date_time| date_time.with_timezone(&Utc))
        .map_err(de::Error::custom)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactsConfig {
    pub count: usize,
    pub no_name: f64,
    pub first_last: f64,
    pub first_middle_last: f64,
    pub first_only: f64,
    pub us_phones: f64,
    pub inactive_fraction: f64,
    pub no_messages_fraction: f64,
    pub multi_phone_fraction: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelsConfig {
    pub names: Vec<String>,
    pub family: f64,
    pub work: f64,
    pub college: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OneToOneConfig {
    pub typical_min: u32,
    pub typical_max: u32,
    pub min_per_year: u32,
    pub max_per_year: u32,
    pub low_tail: f64,
    pub high_tail: f64,
    pub span_mean_years: f64,
    pub span_mean_jitter: f64,
    pub span_max_years: f64,
    pub newest_days: u32,
    pub one_to_one_fraction: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupsConfig {
    pub per_contact_mean: f64,
    pub per_contact_min: u32,
    pub per_contact_max: u32,
    pub participants_mean: f64,
    pub participants_min: u32,
    pub participants_max: u32,
    /// At least this many groups must have a participant count between
    /// `large_participants_min` and `large_participants_max`.
    pub large_min_count: usize,
    pub large_participants_min: u32,
    pub large_participants_max: u32,
    pub typical_min: u32,
    pub typical_max: u32,
    pub min_per_year: u32,
    pub max_per_year: u32,
    pub low_tail: f64,
    pub high_tail: f64,
    pub span_mean_years: f64,
    pub span_max_years: f64,
    pub phone_only_fraction: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagesConfig {
    pub emoji_probability: f64,
    pub jpg_base_stride: usize,
    pub other_base_stride: usize,
    pub tapback_stride: usize,
    pub reply_stride: usize,
    /// Share of messages in the iMessage folder that are marked as SMS or RCS
    /// so the conversation view can show those labels.
    pub apple_fallback_transport_fraction: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeCasesConfig {
    pub unassigned_phones: usize,
    pub unassigned_emails: usize,
    pub orphaned_messages: usize,
    pub empty_individual: bool,
    pub empty_group: bool,
}

/// How demo conversations are split across the iMessage, Android, and WhatsApp folders.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfig {
    /// Share of one-to-one contacts (excluding the ones that appear in both
    /// backups) that only appear in the Android backup.
    pub android_only_fraction: f64,
    /// How many contacts are written into both the iMessage and Android folders.
    pub overlap_count: usize,
    /// Share of messages in those overlapping iMessage threads that also appear
    /// in the Android backup with the same text and time.
    pub overlap_shared_fraction: f64,
    pub overlap_android_extra_min: usize,
    pub overlap_android_extra_max: usize,
    /// Share of contacts that also get a WhatsApp conversation. That conversation
    /// uses the same phone number, marked as WhatsApp rather than iMessage or SMS.
    pub whatsapp_contact_fraction: f64,
}

impl SeedConfig {
    /// Load settings from a TOML file and check them with [`Self::validate`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the TOML is invalid, or
    /// [`Self::validate`] fails.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read demo-seed config {}", path.display()))?;
        Self::from_toml(&text).with_context(|| format!("parse {}", path.display()))
    }

    /// The built-in settings for `size`.
    ///
    /// # Errors
    ///
    /// Returns an error if the compiled-in settings do not parse or fail
    /// [`Self::validate`]; a test loads both sizes, so neither happens in a
    /// released program.
    pub fn for_size(size: DemoSize) -> Result<Self> {
        Self::from_toml(size.settings())
            .with_context(|| format!("parse the built-in {size} demo settings"))
    }

    /// Settings from the text of a settings file, checked by [`Self::validate`].
    fn from_toml(text: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Check that `labels.names` has four entries, that the name-shape shares
    /// sum to at most 1.0, that `one_to_one.newest_days` is at most 30, that
    /// each `typical_min` and `groups.per_contact_min` is no larger than its
    /// maximum, and that the large-group size range sits inside the overall
    /// group size range. The generator draws from each of these ranges, and an
    /// empty or upside-down one makes it panic.
    ///
    /// # Errors
    ///
    /// Returns an error if `labels.names` does not have exactly four entries,
    /// if the name-shape shares sum to more than 1.0, if `newest_days` is over
    /// 30, if a minimum is larger than its maximum, or if the large-group range
    /// sticks out past the overall min or max.
    pub fn validate(&self) -> Result<()> {
        if self.labels.names.len() != 4 {
            anyhow::bail!(
                "labels.names must have exactly 4 entries (family, work, college, inactive), found {}",
                self.labels.names.len()
            );
        }
        if self.contacts.first_only + self.contacts.first_middle_last + self.contacts.first_last
            > 1.0
        {
            anyhow::bail!(
                "contacts name-shape shares must sum to at most 1.0 (first_only {} + first_middle_last {} + first_last {} = {})",
                self.contacts.first_only,
                self.contacts.first_middle_last,
                self.contacts.first_last,
                self.contacts.first_only
                    + self.contacts.first_middle_last
                    + self.contacts.first_last
            );
        }
        let one = &self.one_to_one;
        if one.newest_days > 30 {
            anyhow::bail!(
                "one_to_one.newest_days must be in 0 to 30, found {}",
                one.newest_days
            );
        }
        if one.typical_min > one.typical_max {
            anyhow::bail!(
                "one_to_one.typical_min ({}) > typical_max ({})",
                one.typical_min,
                one.typical_max
            );
        }
        let g = &self.groups;
        if g.typical_min > g.typical_max {
            anyhow::bail!(
                "groups.typical_min ({}) > typical_max ({})",
                g.typical_min,
                g.typical_max
            );
        }
        if g.per_contact_min > g.per_contact_max {
            anyhow::bail!(
                "groups.per_contact_min ({}) > per_contact_max ({})",
                g.per_contact_min,
                g.per_contact_max
            );
        }
        if g.large_min_count == 0 {
            return Ok(());
        }
        if g.large_participants_min > g.large_participants_max {
            anyhow::bail!(
                "groups.large_participants_min ({}) > large_participants_max ({})",
                g.large_participants_min,
                g.large_participants_max
            );
        }
        if g.large_participants_min < g.participants_min {
            anyhow::bail!(
                "groups.large_participants_min ({}) < participants_min ({})",
                g.large_participants_min,
                g.participants_min
            );
        }
        if g.large_participants_max > g.participants_max {
            anyhow::bail!(
                "groups.large_participants_max ({}) > participants_max ({})",
                g.large_participants_max,
                g.participants_max
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_built_in_sizes_load_and_medium_is_the_smaller() {
        let medium = SeedConfig::for_size(DemoSize::Medium).expect("medium settings");
        let large = SeedConfig::for_size(DemoSize::Large).expect("large settings");
        assert!(medium.contacts.count < large.contacts.count);
        assert_eq!(DemoSize::default(), DemoSize::Medium);
    }

    /// The medium settings with `find` replaced by `replace`.
    fn medium_with(find: &str, replace: &str) -> String {
        let settings = DemoSize::Medium.settings();
        assert!(settings.contains(find), "the medium settings hold {find:?}");
        settings.replacen(find, replace, 1)
    }

    /// The refusal for the medium settings with `find` replaced by `replace`.
    fn refusal_of_medium_with(find: &str, replace: &str) -> String {
        let text = medium_with(find, replace);
        format!("{:#}", SeedConfig::from_toml(&text).unwrap_err())
    }

    /// A key the generator does not use is refused wherever it sits, by its
    /// name and its line: a misspelt key otherwise loads as nothing at all.
    #[test]
    fn settings_with_an_unknown_key_are_refused_naming_the_key_and_its_line() {
        for (section, key) in [("[contacts]\n", "cuont"), ("[sources]\n", "overlap")] {
            let settings = medium_with(section, &format!("{section}{key} = 3\n"));
            let line = 1 + settings
                .lines()
                .position(|line| line == format!("{key} = 3"))
                .expect("the added line");
            let text = format!("{:#}", SeedConfig::from_toml(&settings).unwrap_err());
            assert!(text.contains(&format!("`{key}`")), "{text}");
            assert!(text.contains(&format!("line {line},")), "{text}");
        }
    }

    /// A key outside any section, and a section the generator does not have.
    #[test]
    fn settings_with_an_unknown_top_level_key_or_section_are_refused_naming_it() {
        let text = refusal_of_medium_with("seed = ", "sede = 1\nseed = ");
        assert!(text.contains("`sede`"), "{text}");

        let text = refusal_of_medium_with("[contacts]\n", "[contcats]\nx = 1\n\n[contacts]\n");
        assert!(text.contains("`contcats`"), "{text}");
    }

    /// A refusal from a file names the file.
    #[test]
    fn a_settings_file_with_an_unknown_key_is_refused_naming_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("demo_seed.toml");
        let text =
            DemoSize::Medium
                .settings()
                .replacen("[labels]\n", "[labels]\nfriends = 0.1\n", 1);
        fs::write(&path, text).expect("write settings");
        let text = format!("{:#}", SeedConfig::load(&path).unwrap_err());
        assert!(text.contains("demo_seed.toml"), "{text}");
        assert!(text.contains("`friends`"), "{text}");
    }

    /// The span sampler draws some conversations from between `newest_days`
    /// and 30 days old, a range that is empty past 30 and made the generator
    /// panic.
    #[test]
    fn settings_with_newest_days_over_30_are_refused_naming_the_key_and_its_range() {
        let text = refusal_of_medium_with("newest_days = 7", "newest_days = 45");
        assert!(text.contains("one_to_one.newest_days"), "{text}");
        assert!(text.contains("0 to 30"), "{text}");
        assert!(text.contains("45"), "{text}");
    }

    /// Both sections draw a typical rate from `typical_min..=typical_max`,
    /// which panics when the range is upside down.
    #[test]
    fn settings_with_typical_min_over_typical_max_are_refused_naming_the_keys() {
        for (find, replace, section) in [
            ("typical_min = 80", "typical_min = 130", "one_to_one"),
            ("typical_min = 60", "typical_min = 160", "groups"),
        ] {
            let text = refusal_of_medium_with(find, replace);
            assert!(text.contains(&format!("{section}.typical_min")), "{text}");
            assert!(text.contains("typical_max"), "{text}");
        }
    }

    /// The groups-per-contact count is clamped to this range, and `clamp`
    /// panics when the minimum is over the maximum.
    #[test]
    fn settings_with_per_contact_min_over_per_contact_max_are_refused_naming_the_keys() {
        let text = refusal_of_medium_with("per_contact_min = 0", "per_contact_min = 25");
        assert!(text.contains("groups.per_contact_min"), "{text}");
        assert!(text.contains("per_contact_max"), "{text}");
    }

    #[test]
    fn rejects_large_band_outside_participants_max() {
        let mut cfg = SeedConfig::for_size(DemoSize::Large).expect("load");
        cfg.groups.large_participants_max = cfg.groups.participants_max + 1;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_inverted_large_band() {
        let mut cfg = SeedConfig::for_size(DemoSize::Large).expect("load");
        cfg.groups.large_participants_min = 15;
        cfg.groups.large_participants_max = 10;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_labels_names_without_four_entries() {
        let mut cfg = SeedConfig::for_size(DemoSize::Large).expect("load");
        cfg.labels.names = vec!["Family".into(), "Work".into()];
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_name_shape_shares_above_one() {
        let mut cfg = SeedConfig::for_size(DemoSize::Large).expect("load");
        cfg.contacts.first_last = 1.0;
        assert!(cfg.validate().is_err());
    }
}
