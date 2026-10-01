//! What a credential may do. One set for accounts and API tokens, so an
//! account's grant and a token's grant intersect field by field rather than
//! through a translation. A token stores only `import` and `export`: it never
//! carries `delete` (`docs/architecture/http-api.md`, "Credentials and reach").

/// Operations a credential is allowed to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// May call the import endpoints.
    pub import: bool,
    /// May call the export endpoints.
    pub export: bool,
    /// May destroy message data: trash, purge, delete-messages, attachments.
    pub delete: bool,
}

impl Permissions {
    /// Everything allowed. The default for a newly registered account.
    pub const fn all() -> Self {
        Self {
            import: true,
            export: true,
            delete: true,
        }
    }

    /// Nothing allowed.
    pub const fn none() -> Self {
        Self {
            import: false,
            export: false,
            delete: false,
        }
    }

    /// A token's grant: `import` and `export` as chosen, and never `delete`,
    /// because permanent deletion is a person's act.
    pub const fn token(import: bool, export: bool) -> Self {
        Self {
            import,
            export,
            delete: false,
        }
    }

    /// What both sides allow. A token can narrow its owner's grant, never widen it.
    pub const fn intersect(self, other: Self) -> Self {
        Self {
            import: self.import && other.import,
            export: self.export && other.export,
            delete: self.delete && other.delete,
        }
    }

    /// Read from three integer columns as stored by both engines.
    pub fn from_ints(import: i64, export: i64, delete: i64) -> Self {
        Self {
            import: import != 0,
            export: export != 0,
            delete: delete != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_keeps_only_what_both_allow() {
        let account = Permissions {
            import: true,
            export: true,
            delete: false,
        };
        let token = Permissions {
            import: true,
            export: false,
            delete: true,
        };
        let effective = account.intersect(token);
        assert!(effective.import);
        assert!(!effective.export, "token withheld export");
        assert!(!effective.delete, "account withheld delete");
    }
}
