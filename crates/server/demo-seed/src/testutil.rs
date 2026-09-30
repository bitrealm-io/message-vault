//! A seed small enough to generate in a test, for this crate's own tests and
//! for the vault server's import test (`testutil` feature).
//!
//! One copy, used from both places: a second copy of the seed in the server
//! would drift from this one the first time a section is added here.

use std::fs;
use std::path::{Path, PathBuf};

use crate::SeedConfig;

/// `demo_seed.toml` shrunk to a dozen contacts. Every section keeps the shape
/// of the checked-in file, so every writer still runs: iMessage-only,
/// Android-only, overlap, WhatsApp, groups, unassigned handles, orphans, and
/// both empty threads. Conversations stay long enough (about a hundred
/// messages) to reach the photo, other-attachment, tapback, and reply strides.
const SMALL_SEED_TOML: &str = r#"
seed = 7
out = "replaced by the test"
reference_time = "2026-08-01T12:00:00Z"

[contacts]
count = 12
no_name = 0.1
first_last = 0.6
first_middle_last = 0.2
first_only = 0.2
us_phones = 0.8
inactive_fraction = 0.1
no_messages_fraction = 0.1
multi_phone_fraction = 0.2

[labels]
names = ["Family", "Work", "College", "Inactive"]
family = 0.3
work = 0.3
college = 0.3

[one_to_one]
typical_min = 40
typical_max = 60
min_per_year = 10
max_per_year = 120
low_tail = 0.1
high_tail = 0.1
span_mean_years = 2.0
span_mean_jitter = 0.5
span_max_years = 4.0
newest_days = 7
one_to_one_fraction = 0.9

[groups]
per_contact_mean = 2.0
per_contact_min = 0
per_contact_max = 4
participants_mean = 3.0
participants_min = 2
participants_max = 6
large_min_count = 1
large_participants_min = 4
large_participants_max = 6
typical_min = 40
typical_max = 80
min_per_year = 10
max_per_year = 200
low_tail = 0.1
high_tail = 0.1
span_mean_years = 1.5
span_max_years = 3.0
phone_only_fraction = 0.25

[messages]
emoji_probability = 0.05
jpg_base_stride = 5
other_base_stride = 7
tapback_stride = 6
reply_stride = 8
apple_fallback_transport_fraction = 0.2

[edge_cases]
unassigned_phones = 2
unassigned_emails = 1
orphaned_messages = 3
empty_individual = true
empty_group = true

[sources]
android_only_fraction = 0.25
overlap_count = 2
overlap_shared_fraction = 0.5
overlap_android_extra_min = 3
overlap_android_extra_max = 6
whatsapp_contact_fraction = 0.5
"#;

/// Write [`SMALL_SEED_TOML`] into `dir` and return its path.
pub fn write_small_seed_toml(dir: &Path) -> PathBuf {
    let path = dir.join("demo_seed.toml");
    fs::write(&path, SMALL_SEED_TOML).expect("write the small seed file");
    path
}

/// Load the small seed file from `dir` with `out` pointed at `dir/demo`.
pub fn small_config(dir: &Path) -> SeedConfig {
    let mut cfg = SeedConfig::load(&write_small_seed_toml(dir)).expect("load the small seed file");
    cfg.out = dir
        .join("demo")
        .to_str()
        .expect("UTF-8 test path")
        .to_string();
    cfg
}
