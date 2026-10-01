//! Builds a list of demo contacts, groups, and unassigned handles.

use std::collections::HashSet;

use anyhow::{Result, bail};
use rand::Rng;
use rand::RngExt;
use rand::seq::SliceRandom;
use rand_distr::{Distribution, Normal, Poisson};

use crate::config::SeedConfig;
use crate::names::NameBank;
use crate::phones;
pub use crate::phones::OWNER_PHONE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageScope {
    OneToOne,
    Group,
    Both,
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub phones: Vec<String>,
    pub first_name: String,
    pub middle_name: String,
    pub last_name: String,
    pub groups: Vec<String>,
    pub has_messages: bool,
    pub message_scope: MessageScope,
    pub msgs_per_year: f64,
    pub span_years: f64,
    /// Also write a WhatsApp conversation for this contact's primary phone.
    pub has_whatsapp: bool,
}

#[derive(Debug, Clone)]
pub struct Unassigned {
    pub handle: String,
    pub name_alias: Option<String>,
    pub email_only: bool,
}

#[derive(Debug, Clone)]
pub struct GroupSpec {
    pub index: usize,
    pub member_idxs: Vec<usize>,
    pub phone_only: bool,
    pub phone_only_handles: Vec<String>,
    pub msgs_per_year: f64,
    pub span_years: f64,
    pub title: Option<String>,
}

/// Contacts, groups, and unassigned handles used to write the demo conversations.
#[derive(Debug)]
pub struct Roster {
    pub contacts: Vec<Contact>,
    pub unassigned: Vec<Unassigned>,
    pub groups: Vec<GroupSpec>,
}

pub const EMPTY_GROUP_HANDLE: &str = "chat0000000001";
pub const EMPTY_THREAD_HANDLE: &str = "+18007438200";
pub const ORPHAN_SENDER: &str = "+447700900999";

const GROUP_TITLES: &[&str] = &[
    "Weekend Trip",
    "Book Club",
    "Soccer Parents",
    "Apartment 4B",
    "Project Atlas",
    "Family Chat",
    "College Reunion",
    "Neighborhood Watch",
    "Hiking Crew",
    "Potluck Planning",
    "Fantasy Draft",
    "Office Lunch",
    "Road Trip West",
    "Baby Shower",
    "Game Night",
    "Volunteer Squad",
];

/// Build contacts, groups, and unassigned handles from the settings and name lists.
///
/// # Errors
///
/// Returns an error if not enough contacts can be placed into the required
/// large groups.
pub fn build_roster(cfg: &SeedConfig, names: &NameBank, rng: &mut impl Rng) -> Result<Roster> {
    let mut used_phones = HashSet::new();
    used_phones.insert(OWNER_PHONE.to_string());

    let mut contacts = Vec::with_capacity(cfg.contacts.count);
    for _ in 0..cfg.contacts.count {
        contacts.push(make_contact(cfg, names, rng, &mut used_phones));
    }

    mark_whatsapp_contacts(&mut contacts, cfg.sources.whatsapp_contact_fraction, rng);

    let groups = build_groups(cfg, &contacts, rng, &mut used_phones)?;
    let unassigned = build_unassigned(cfg, rng, &mut used_phones);

    Ok(Roster {
        contacts,
        unassigned,
        groups,
    })
}

/// Mark a share of one-to-one contacts so they also get a WhatsApp conversation.
fn mark_whatsapp_contacts(contacts: &mut [Contact], fraction: f64, rng: &mut impl Rng) {
    let mut eligible = Vec::new();
    for (index, contact) in contacts.iter().enumerate() {
        if contact.phones.is_empty() {
            continue;
        }
        if !contact.has_one_to_one() {
            continue;
        }
        eligible.push(index);
    }
    eligible.shuffle(rng);
    let count = crate::rounded_fraction(eligible.len(), fraction);
    for &index in eligible.iter().take(count) {
        contacts[index].has_whatsapp = true;
    }
}

/// Build one contact: name shape, phones, groups, and how often they message.
fn make_contact(
    cfg: &SeedConfig,
    names: &NameBank,
    rng: &mut impl Rng,
    used: &mut HashSet<String>,
) -> Contact {
    let nameless = rng.random_bool(cfg.contacts.no_name);
    let (first, middle, last) = if nameless {
        (String::new(), String::new(), String::new())
    } else {
        sample_name_shape(cfg, names, rng)
    };

    let mut phones = vec![phones::generate_phone(rng, cfg.contacts.us_phones, used)];
    if rng.random_bool(cfg.contacts.multi_phone_fraction) {
        phones.push(phones::generate_phone(rng, cfg.contacts.us_phones, used));
    }

    let inactive = rng.random_bool(cfg.contacts.inactive_fraction);
    let no_messages = inactive || rng.random_bool(cfg.contacts.no_messages_fraction);

    let mut groups = Vec::new();
    if inactive {
        groups.push(cfg.labels.names[3].clone());
    } else {
        if rng.random_bool(cfg.labels.family) {
            groups.push(cfg.labels.names[0].clone());
        }
        if rng.random_bool(cfg.labels.work) {
            groups.push(cfg.labels.names[1].clone());
        }
        if rng.random_bool(cfg.labels.college) {
            groups.push(cfg.labels.names[2].clone());
        }
    }

    let message_scope = if no_messages {
        MessageScope::Both
    } else if rng.random_bool(cfg.one_to_one.one_to_one_fraction) {
        if rng.random_bool(0.75) {
            MessageScope::Both
        } else {
            MessageScope::OneToOne
        }
    } else {
        MessageScope::Group
    };

    Contact {
        phones,
        first_name: first,
        middle_name: middle,
        last_name: last,
        groups,
        has_messages: !no_messages,
        message_scope,
        msgs_per_year: sample_msgs_per_year(cfg, rng),
        span_years: sample_span_years(
            cfg.one_to_one.span_mean_years,
            cfg.one_to_one.span_mean_jitter,
            cfg.one_to_one.span_max_years,
            cfg.one_to_one.newest_days,
            rng,
        ),
        has_whatsapp: false,
    }
}

/// Pick first-only, first-middle-last, or first-last from the configured shares.
// The final `else` intentionally mirrors the first-last branch as a defensive
// fallthrough for configs whose shares sum to less than 1.0.
/// Pick a first, last, and display name for a persona, in one of the shapes real address books have.
#[allow(clippy::if_same_then_else)]
fn sample_name_shape(
    cfg: &SeedConfig,
    names: &NameBank,
    rng: &mut impl Rng,
) -> (String, String, String) {
    let first = names.pick_first(rng).to_string();
    let roll: f64 = rng.random();
    if roll < cfg.contacts.first_only {
        (first, String::new(), String::new())
    } else if roll < cfg.contacts.first_only + cfg.contacts.first_middle_last {
        (
            first,
            names.pick_middle(rng).to_string(),
            names.pick_last(rng).to_string(),
        )
    } else if roll
        < cfg.contacts.first_only + cfg.contacts.first_middle_last + cfg.contacts.first_last
    {
        (first, String::new(), names.pick_last(rng).to_string())
    } else {
        (first, String::new(), names.pick_last(rng).to_string())
    }
}

/// Messages per year for a one-to-one conversation, using the one-to-one settings.
fn sample_msgs_per_year(cfg: &SeedConfig, rng: &mut impl Rng) -> f64 {
    sample_skewed_msgs_per_year(
        cfg.one_to_one.min_per_year,
        cfg.one_to_one.typical_min,
        cfg.one_to_one.typical_max,
        cfg.one_to_one.max_per_year,
        cfg.one_to_one.low_tail,
        cfg.one_to_one.high_tail,
        rng,
    )
}

/// Messages per year for a group conversation, using the group settings.
fn sample_group_msgs_per_year(cfg: &SeedConfig, rng: &mut impl Rng) -> f64 {
    sample_skewed_msgs_per_year(
        cfg.groups.min_per_year,
        cfg.groups.typical_min,
        cfg.groups.typical_max,
        cfg.groups.max_per_year,
        cfg.groups.low_tail,
        cfg.groups.high_tail,
        rng,
    )
}

/// Pick a yearly message count: usually typical, sometimes very low or very high.
fn sample_skewed_msgs_per_year(
    min_per_year: u32,
    typical_min: u32,
    typical_max: u32,
    max_per_year: u32,
    low_tail: f64,
    high_tail: f64,
    rng: &mut impl Rng,
) -> f64 {
    let roll: f64 = rng.random();
    if roll < low_tail {
        rng.random_range(min_per_year as f64..(typical_min as f64).max(min_per_year as f64 + 1.0))
    } else if roll < low_tail + high_tail {
        let unit = rng.random::<f64>().clamp(1e-6, 1.0);
        let log_low = (typical_max as f64).max(1.0).ln();
        let log_high = (max_per_year as f64).max(typical_max as f64 + 1.0).ln();
        (log_low + unit * (log_high - log_low)).exp()
    } else {
        rng.random_range(typical_min as f64..=typical_max as f64)
    }
}

/// How many years the conversation history covers, clustered around `mean`.
///
/// A few conversations are much older, and a few started only days ago.
fn sample_span_years(
    mean: f64,
    jitter: f64,
    max_years: f64,
    newest_days: u32,
    rng: &mut impl Rng,
) -> f64 {
    let min_years = (newest_days as f64) / 365.25;
    let std_dev = jitter.max(0.1);
    let normal = match Normal::new(mean, std_dev) {
        Ok(dist) => dist,
        Err(_) => Normal::new(4.0, 1.0).unwrap(),
    };
    let mut years = normal.sample(rng);
    if rng.random_bool(0.06) {
        years = rng.random_range((max_years * 0.7)..=max_years);
    }
    if rng.random_bool(0.04) {
        years = rng.random_range(min_years..=(30.0 / 365.25));
    }
    years.clamp(min_years, max_years)
}

/// How many groups this contact should join, clustered around the configured mean.
fn sample_groups_per_contact(cfg: &SeedConfig, rng: &mut impl Rng) -> usize {
    let g = &cfg.groups;
    let poisson = match Poisson::new(g.per_contact_mean.max(0.1)) {
        Ok(dist) => dist,
        Err(_) => Poisson::new(5.0).unwrap(),
    };
    let n = poisson.sample(rng) as u32;
    n.clamp(g.per_contact_min, g.per_contact_max) as usize
}

/// How many people are in a typical group.
fn sample_group_size(cfg: &SeedConfig, rng: &mut impl Rng) -> usize {
    let g = &cfg.groups;
    let normal = match Normal::new(g.participants_mean, 2.0) {
        Ok(dist) => dist,
        Err(_) => Normal::new(4.0, 2.0).unwrap(),
    };
    let n = normal.sample(rng).round() as i32;
    n.clamp(g.participants_min as i32, g.participants_max as i32) as usize
}

/// How many people are in a large group, within the configured large-group range.
fn sample_large_group_size(cfg: &SeedConfig, rng: &mut impl Rng) -> usize {
    let lo = cfg.groups.large_participants_min as usize;
    let hi = cfg.groups.large_participants_max as usize;
    rng.random_range(lo..=hi)
}

/// Remaining group slots for each contact, in the same order as `contacts`.
fn membership_budgets(cfg: &SeedConfig, contacts: &[Contact], rng: &mut impl Rng) -> Vec<usize> {
    let mut budgets = Vec::with_capacity(contacts.len());
    for contact in contacts {
        budgets.push(group_membership_budget(cfg, contact, rng));
    }
    budgets
}

/// How many groups this contact may still join. Inactive and silent contacts get zero.
fn group_membership_budget(cfg: &SeedConfig, contact: &Contact, rng: &mut impl Rng) -> usize {
    if !contact.has_messages || contact.has_group("Inactive") {
        return 0;
    }
    if matches!(contact.message_scope, MessageScope::OneToOne) {
        // One-to-one contacts rarely join groups.
        if rng.random_bool(0.15) {
            return sample_groups_per_contact(cfg, rng).min(2);
        }
        return 0;
    }
    sample_groups_per_contact(cfg, rng)
}

/// Fill a group from contacts who still have room, then from other active contacts if needed.
///
/// # Errors
///
/// Returns an error if fewer than `min_size` members can be found.
fn pick_group_members(
    target_size: usize,
    min_size: usize,
    remaining: &mut [usize],
    contacts: &[Contact],
    rng: &mut impl Rng,
) -> Result<Vec<usize>> {
    let mut member_idxs = Vec::new();
    let mut candidates = Vec::new();
    for (index, &budget) in remaining.iter().enumerate() {
        if budget > 0 {
            candidates.push(index);
        }
    }
    candidates.shuffle(rng);
    for &idx in candidates.iter().take(target_size) {
        member_idxs.push(idx);
        remaining[idx] = remaining[idx].saturating_sub(1);
    }

    if member_idxs.len() < target_size {
        let mut extras = Vec::new();
        for (index, contact) in contacts.iter().enumerate() {
            if !contact.has_messages || contact.has_group("Inactive") {
                continue;
            }
            if member_idxs.contains(&index) {
                continue;
            }
            extras.push(index);
        }
        extras.shuffle(rng);
        for idx in extras {
            member_idxs.push(idx);
            if member_idxs.len() >= target_size {
                break;
            }
        }
    }

    if member_idxs.len() < min_size {
        bail!(
            "could not assemble a group with at least {min_size} participants (got {}); \
             raise contacts.count or lower groups.large_min_count / large_participants_min",
            member_idxs.len()
        );
    }
    Ok(member_idxs)
}

/// Attach yearly rate, history length, and an optional title to a group's member list.
fn finish_group_spec(
    cfg: &SeedConfig,
    index: usize,
    member_idxs: Vec<usize>,
    phone_only: bool,
    phone_only_handles: Vec<String>,
    rng: &mut impl Rng,
    title_index: usize,
) -> GroupSpec {
    let msgs_per_year = sample_group_msgs_per_year(cfg, rng);
    let span_years = sample_span_years(
        cfg.groups.span_mean_years,
        1.5,
        cfg.groups.span_max_years,
        cfg.one_to_one.newest_days,
        rng,
    );
    let title = if rng.random_bool(0.55) {
        let base = GROUP_TITLES[title_index % GROUP_TITLES.len()];
        let reuse_count = title_index / GROUP_TITLES.len();
        if reuse_count > 0 && rng.random_bool(0.35) {
            Some(format!("{base} {}", reuse_count + 1))
        } else {
            Some(base.to_string())
        }
    } else {
        None
    };

    GroupSpec {
        index,
        member_idxs,
        phone_only,
        phone_only_handles,
        msgs_per_year,
        span_years,
        title,
    }
}

/// Create the required large groups first, then fill remaining membership slots.
///
/// # Errors
///
/// Returns an error if the large groups cannot be filled before the group cap.
fn build_groups(
    cfg: &SeedConfig,
    contacts: &[Contact],
    rng: &mut impl Rng,
    used_phones: &mut HashSet<String>,
) -> Result<Vec<GroupSpec>> {
    let contact_count = contacts.len();
    let mut remaining = membership_budgets(cfg, contacts, rng);

    let mut groups: Vec<GroupSpec> = Vec::new();
    let max_groups = (contact_count * cfg.groups.per_contact_max as usize / 2).max(8);

    // Create the large named groups first so searches for groups with many
    // participants always have conversations to show.
    for _ in 0..cfg.groups.large_min_count {
        if groups.len() >= max_groups {
            bail!(
                "hit max_groups ({max_groups}) before reserving {} large groups",
                cfg.groups.large_min_count
            );
        }
        let target_size = sample_large_group_size(cfg, rng);
        let member_idxs = pick_group_members(
            target_size,
            cfg.groups.large_participants_min as usize,
            &mut remaining,
            contacts,
            rng,
        )?;
        let index = groups.len();
        groups.push(finish_group_spec(
            cfg,
            index,
            member_idxs,
            false,
            Vec::new(),
            rng,
            index,
        ));
    }

    let mut safety = 0usize;
    while remaining.iter().any(|&n| n > 0) && groups.len() < max_groups && safety < max_groups * 4 {
        safety += 1;
        let phone_only = rng.random_bool(cfg.groups.phone_only_fraction);
        let target_size = sample_group_size(cfg, rng).max(2);

        let mut member_idxs = Vec::new();
        let mut phone_only_handles = Vec::new();

        if phone_only {
            for _ in 0..target_size {
                phone_only_handles.push(phones::generate_phone(
                    rng,
                    cfg.contacts.us_phones,
                    used_phones,
                ));
            }
        } else {
            match pick_group_members(target_size, 2, &mut remaining, contacts, rng) {
                Ok(idxs) => member_idxs = idxs,
                Err(_) => continue,
            }
        }

        let index = groups.len();
        groups.push(finish_group_spec(
            cfg,
            index,
            member_idxs,
            phone_only,
            phone_only_handles,
            rng,
            index,
        ));
    }

    Ok(groups)
}

/// Phone numbers and email addresses that have messages but no contact card.
fn build_unassigned(
    cfg: &SeedConfig,
    rng: &mut impl Rng,
    used: &mut HashSet<String>,
) -> Vec<Unassigned> {
    let mut out = Vec::new();
    for i in 0..cfg.edge_cases.unassigned_phones {
        let handle = phones::generate_phone(rng, cfg.contacts.us_phones, used);
        let name_alias = if i % 2 == 0 {
            Some("(Unverified)".into())
        } else {
            None
        };
        out.push(Unassigned {
            handle,
            name_alias,
            email_only: false,
        });
    }
    for i in 0..cfg.edge_cases.unassigned_emails {
        out.push(Unassigned {
            handle: format!("guest{i}@demo.example"),
            name_alias: Some(if i == 0 {
                "Stranger Email".into()
            } else {
                "Contractor".into()
            }),
            email_only: true,
        });
    }
    out
}

impl Contact {
    /// True when this contact has a group that matches `name`, ignoring letter case.
    pub fn has_group(&self, name: &str) -> bool {
        self.groups
            .iter()
            .any(|group| group.eq_ignore_ascii_case(name))
    }

    /// First phone number on the contact. Contacts always have at least one.
    pub fn primary_phone(&self) -> &str {
        &self.phones[0]
    }

    /// Display name from first, middle, and last. Empty when the contact has no name.
    pub fn display_hint(&self) -> String {
        let mut parts = Vec::new();
        if !self.first_name.is_empty() {
            parts.push(self.first_name.as_str());
        }
        if !self.middle_name.is_empty() {
            parts.push(self.middle_name.as_str());
        }
        if !self.last_name.is_empty() {
            parts.push(self.last_name.as_str());
        }
        parts.join(" ")
    }

    /// True when this contact should get a one-to-one conversation.
    pub fn has_one_to_one(&self) -> bool {
        self.has_messages
            && matches!(
                self.message_scope,
                MessageScope::OneToOne | MessageScope::Both
            )
    }

    /// How many messages to write in the one-to-one conversation. Zero if there is none.
    pub fn message_count(&self) -> usize {
        if !self.has_one_to_one() {
            return 0;
        }
        if self.has_group("Inactive") {
            return ((self.msgs_per_year * self.span_years) as usize).clamp(3, 12);
        }
        let n = (self.msgs_per_year * self.span_years).round() as isize;
        n.max(1) as usize
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    use super::*;
    use crate::config::SeedConfig;
    use crate::names::NameBank;

    #[test]
    fn roster_guarantees_large_groups() {
        let cfg = SeedConfig::load(&SeedConfig::default_path()).expect("load demo_seed.toml");
        let names = NameBank::load_default().expect("names");
        let mut rng = ChaCha8Rng::seed_from_u64(cfg.seed);
        let roster = build_roster(&cfg, &names, &mut rng).expect("roster");

        let lo = cfg.groups.large_participants_min as usize;
        let hi = cfg.groups.large_participants_max as usize;
        let mut large = 0;
        for group in &roster.groups {
            if group.phone_only {
                continue;
            }
            if (lo..=hi).contains(&group.member_idxs.len()) {
                large += 1;
            }
        }
        assert!(
            large >= cfg.groups.large_min_count,
            "expected >= {} groups sized {lo}..={hi}, got {large}",
            cfg.groups.large_min_count
        );
    }

    #[test]
    fn sample_name_shape_respects_configured_shares() {
        let mut cfg = SeedConfig::load(&SeedConfig::default_path()).expect("load demo_seed.toml");
        let names = NameBank::load_default().expect("names");
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        cfg.contacts.first_only = 1.0;
        cfg.contacts.first_middle_last = 0.0;
        cfg.contacts.first_last = 0.0;
        for _ in 0..50 {
            let (first, middle, last) = sample_name_shape(&cfg, &names, &mut rng);
            assert!(!first.is_empty());
            assert!(middle.is_empty());
            assert!(last.is_empty());
        }
        cfg.contacts.first_only = 0.0;
        cfg.contacts.first_middle_last = 1.0;
        cfg.contacts.first_last = 0.0;
        for _ in 0..50 {
            let (first, middle, last) = sample_name_shape(&cfg, &names, &mut rng);
            assert!(!first.is_empty());
            assert!(!middle.is_empty());
            assert!(!last.is_empty());
        }
        cfg.contacts.first_middle_last = 0.0;
        cfg.contacts.first_last = 1.0;
        for _ in 0..50 {
            let (first, middle, last) = sample_name_shape(&cfg, &names, &mut rng);
            assert!(!first.is_empty());
            assert!(middle.is_empty());
            assert!(!last.is_empty());
        }
    }

    #[test]
    fn group_labels_come_from_config_names() {
        let cfg = SeedConfig::load(&SeedConfig::default_path()).expect("load demo_seed.toml");
        let names = NameBank::load_default().expect("names");
        let mut rng = ChaCha8Rng::seed_from_u64(cfg.seed);
        let roster = build_roster(&cfg, &names, &mut rng).expect("roster");
        for contact in &roster.contacts {
            for group in &contact.groups {
                assert!(
                    cfg.labels.names.iter().any(|n| n == group),
                    "group label not from config: {group}"
                );
            }
        }
    }
}
