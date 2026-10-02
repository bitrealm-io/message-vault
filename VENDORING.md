# Vendored sqlx-sqlite fork

`vendor/sqlx-sqlite/` is a byte-identical copy of the `sqlx-sqlite` 0.8.6
source from crates.io, with one change: `libsqlite3-sys` is bumped from
`0.30.1` to `0.38.0` so the workspace unifies on a single native SQLite
bindings version (rusqlite 0.40 / crabapple 0.4.7 use 0.38). Cargo's
`links` rule permits only one libsqlite3-sys per dependency graph; without
this bump, sqlx 0.8 (0.30) and rusqlite 0.40 (0.38) cannot coexist.
Released sqlx 0.9 does not help either (its range caps below 0.38).

## Re-vendoring on sqlx upgrades

1. Note the new `sqlx-sqlite` version from the lockfile after the sqlx
   bump, then download that version's `.crate` tarball from crates.io.
2. Unpack it over `vendor/sqlx-sqlite/`, then re-apply the manifest
   changes: materialize the workspace-inherited keys (version, license,
   edition), set `libsqlite3-sys` to the rusqlite-matched release
   (`0.38.0` unless rusqlite moved), pin `sqlx-core` to the matching
   version, and drop the `[lints] workspace = true` block.
3. Run the full verification suite — the server's SQLite tests are the
   gate for this combination (upstream tests sqlx-sqlite only against its own
   libsqlite3-sys pin).
4. Never edit fork source beyond the manifest. Upstream license:
   MIT OR Apache-2.0 (both license files stay in the vendor dir).
