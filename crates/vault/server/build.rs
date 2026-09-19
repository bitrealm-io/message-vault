//! Embed this vault's Build, the Product Version plus the commit, as
//! `MESSAGE_VAULT_BUILD`. The rules are in the `build-version` crate.

fn main() {
    build_version::emit();
}
