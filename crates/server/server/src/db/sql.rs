//! Shared SQL query helpers.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use sqlx::AnyConnection;
use sqlx::Arguments;
use sqlx::any::{AnyArguments, AnyRow};

/// One bound parameter in a dynamic query. sqlx's Any driver exposes no
/// user-constructible dynamic value, so heterogeneous binds ride this enum.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlParam {
    Text(String),
    Int(i64),
}

/// Encode `params` into Any-driver arguments, in order.
///
/// `String`/`i64` cannot fail to encode on the Any driver; an encode
/// failure is unreachable and panics like sqlx's own `Query::bind`.
pub fn bind_args<'q>(params: &[SqlParam]) -> AnyArguments<'q> {
    let mut args = AnyArguments::default();
    for p in params {
        match p {
            SqlParam::Text(v) => args.add(v.clone()),
            SqlParam::Int(v) => args.add(*v),
        }
        .expect("error encoding argument");
    }
    args
}

/// Build a query from `sql` with all params bound, in order. Placeholders in
/// the SQL must match this order after [`renumber_placeholders`].
///
/// sqlx 0.8.6 does not re-export `Query` at the crate root (the root
/// `sqlx::Query` re-export is 0.9-only), so the concrete query type is
/// unnameable; this builds the arguments through the public `Arguments` API
/// instead.
pub fn bind_all<'q>(sql: &'q str, params: &[SqlParam]) -> impl sqlx::Execute<'q, sqlx::Any> + 'q {
    sqlx::query_with(sql, bind_args(params))
}

/// Rewrite `?` placeholders to `$1..$N` in order, so a fragment written
/// with `?` joins a statement whose other placeholders are numbered.
/// Valid because no SQL fragment in this crate contains `?` inside a string
/// literal — keep it that way, and unit-test this against the committed
/// fragment set.
pub fn renumber_placeholders(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut n = 0usize;
    for ch in sql.chars() {
        if ch == '?' {
            n += 1;
            out.push('$');
            out.push_str(&n.to_string());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Max ids per `IN (...)` bind list (SQLite's default variable limit is 999).
pub const SQLITE_IN_CHUNK: usize = 400;

/// SQLite default `SQLITE_MAX_VARIABLE_NUMBER`. Multi-row `INSERT` chunks
/// must keep `columns × rows` at or below this.
pub const SQLITE_MAX_VARIABLES: usize = 999;

/// Largest row count whose binds fit in one statement:
/// `columns × rows ≤ 999`.
pub fn max_rows_for_bind_limit(columns: usize) -> usize {
    if columns == 0 {
        return 0;
    }
    SQLITE_MAX_VARIABLES / columns
}

/// Hand-numbered `VALUES` tuples: `($1,$2,$3),($4,$5,$6)` for `row_count` rows
/// of `col_count` columns. sqlx Any does no placeholder rewriting.
pub fn values_tuples(row_count: usize, col_count: usize) -> String {
    (0..row_count)
        .map(|row| {
            let start = row * col_count + 1;
            let inner = (start..start + col_count)
                .map(|i| format!("${i}"))
                .collect::<Vec<_>>()
                .join(",");
            format!("({inner})")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Comma-separated hand-numbered `$N` placeholders for an `IN (...)` list of
/// length `n`, starting at 1-based index `start` (the index of the first
/// placeholder in the full statement). sqlx Any does no placeholder rewriting,
/// so the numbers must be explicit.
pub fn in_placeholders(start: usize, n: usize) -> String {
    (start..start + n)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Load child rows for a set of parent ids, grouped by the parent id in
/// column 0 of each row. `build_sql` receives the `$N` placeholder list for
/// the current chunk of ids (starting at `$1`); `map_row` extracts the parent
/// id and value from each row.
///
/// # Errors
///
/// Returns a database error when a statement fails.
pub async fn group_rows_by_id<T, E>(
    conn: &mut AnyConnection,
    ids: &[i64],
    build_sql: impl Fn(&str) -> String,
    map_row: impl Fn(&AnyRow) -> Result<(i64, T), sqlx::Error>,
) -> Result<HashMap<i64, Vec<T>>, E>
where
    E: From<sqlx::Error>,
{
    let mut map: HashMap<i64, Vec<T>> = HashMap::new();
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        let sql = build_sql(&in_placeholders(1, chunk.len()));
        let mut q = sqlx::query(&sql);
        for id in chunk {
            q = q.bind(*id);
        }
        let rows = q.fetch_all(&mut *conn).await?;
        for row in &rows {
            let (id, value) = map_row(row)?;
            map.entry(id).or_default().push(value);
        }
    }
    Ok(map)
}

/// Run `query_chunk` on successive slices of `ids` and group the results by id.
/// Each chunk keeps binds under SQLite's bind limit; `SQLITE_IN_CHUNK` (400)
/// is the chunk size.
///
/// # Errors
///
/// Returns whatever error `query_chunk` returns.
pub async fn fold_in_id_chunks<T, E>(
    conn: &mut AnyConnection,
    ids: &[i64],
    mut query_chunk: impl for<'a> FnMut(
        &'a mut AnyConnection,
        &'a [i64],
    ) -> Pin<
        Box<dyn Future<Output = Result<Vec<(i64, T)>, E>> + Send + 'a>,
    >,
) -> Result<HashMap<i64, Vec<T>>, E> {
    let mut map = HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    for chunk in ids.chunks(SQLITE_IN_CHUNK) {
        for (id, row) in query_chunk(conn, chunk).await? {
            map.entry(id).or_default().push(row);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_rows_for_bind_limit_respects_sqlite_999() {
        assert_eq!(max_rows_for_bind_limit(18), 55);
        assert_eq!(max_rows_for_bind_limit(10), 99);
        assert_eq!(max_rows_for_bind_limit(6), 166);
        assert_eq!(max_rows_for_bind_limit(0), 0);
    }

    #[test]
    fn renumber_placeholders_numbers_in_order() {
        let sql = "a = ? AND b = ? AND c IN (?, ?) AND d LIKE ?";
        assert_eq!(
            renumber_placeholders(sql),
            "a = $1 AND b = $2 AND c IN ($3, $4) AND d LIKE $5"
        );
        assert_eq!(renumber_placeholders("no placeholders"), "no placeholders");
    }

    #[test]
    fn bind_args_encodes_every_variant_in_order() {
        // Every variant must encode without panicking, in order; the
        // argument count is the only thing the Any driver lets us observe.
        let params = vec![SqlParam::Text("t".into()), SqlParam::Int(7)];
        let args = bind_args(&params);
        assert_eq!(args.len(), params.len());
    }

    #[test]
    fn values_tuples_numbers_placeholders_across_rows() {
        assert_eq!(values_tuples(2, 3), "($1,$2,$3),($4,$5,$6)");
        assert_eq!(values_tuples(1, 2), "($1,$2)");
        assert_eq!(values_tuples(0, 3), "");
    }
}
