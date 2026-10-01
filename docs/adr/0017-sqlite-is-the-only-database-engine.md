# SQLite is the only database engine

The server ran on SQLite and on Postgres. Postgres support is removed (#1039),
and the engine-neutral layer of sqlx (`sqlx::Any`) follows it (#1040). The
server runs on SQLite alone, and its SQL is written for SQLite.

## Why

Postgres had one purpose: a hosted Message Crate that many people share
(#149). That service is not close, and until it is, Postgres support costs
something on every change and returns nothing.

- Every query had to be valid on both engines. What differed went through
  `db::dialect`, and the schema was kept twice: the SQLite files in
  `schema/sql/`, and a Postgres form derived from them by `db/pg_ddl.rs` plus
  a hand-written full-text search file.
- The Postgres run of the server suite was switched off in `ci.yml`. Code no
  test runs does not stay working: three tests already failed there (#1032).
- About 65 files in `crates/server/server/` carried an engine branch, a
  Postgres-only test, or a comment explaining a difference between the two.

## What stays

sqlx stays. The pool, the queries and the row mapping are all sqlx, and sqlx
has a Postgres driver, so adding Postgres again does not mean changing the
database library.

The removed implementation is the reference design for adding it again. The
last commit that has it is `9224fb102c205c4016caa8b667d2f46acedff571`. The
parts worth reading there:

- `crates/server/server/src/db/pg_ddl.rs`: the four rules that turn the
  SQLite table definitions into Postgres ones.
- `schema/sql/fts_postgres.sql`: full-text search as a `tsvector` column, a
  GIN index and sync triggers.
- `crates/server/server/src/db/dialect.rs` and `db/sql.rs`: where the two
  engines' SQL differed, and the placeholder renumbering.
- `crates/server/server/src/db/engine.rs`: the test pool that gave each test
  a Postgres schema of its own.
- `crates/server/server/tests/search_parity.rs`: the test that both engines
  return the same search results, with the one documented exception.

That reference will be out of date by the time it is needed. It shows the
approach, not code to paste back.

## The rule

Write SQL for SQLite. A query is never made more awkward to keep it portable,
and no engine abstraction or dialect layer is added. A SQLite-only feature is
used when the work at hand needs it; a working query is not rewritten only to
use one. Why the second half: each SQLite-only query is one more thing to
port when Postgres is added again, so each one should buy something.

## Considered and rejected

**Keeping Postgres until the hosted service is built.** Rejected because the
code was already untested, and adding Postgres later is a port either way.

**Removing Postgres and keeping `sqlx::Any` with portable SQL.** It would keep
the appearance of portability. Rejected because SQL that never runs on
Postgres does not stay valid there, and `sqlx::Any` costs the placeholder
renumbering, a narrow set of column types, and no access to SQLite's own
connection options.

## Consequences

The `[database]` config section and the `--db-url` flag are gone. `[paths] db`
and `--db` name the database file, and a config that still has `[database]`
is refused by name.

The Schema Fingerprint covers the embedded SQL files, and two of them were
deleted, so the fingerprint changed with this decision. A database made by an
earlier server is rebuilt empty on first start, as after any schema change.
