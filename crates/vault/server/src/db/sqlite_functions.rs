//! SQL functions the vault adds to every SQLite connection.
//!
//! SQLite's built-in `lower()` folds only ASCII letters unless it is built
//! with ICU, and the bundled build is not, so `lower('Élodie')` is
//! `'Élodie'` there while Postgres answers `'élodie'`. The search words
//! compare `lower(column)` with `lower(text)` on both engines (see
//! [`crate::db::dialect::like_ci`]), so SQLite needs a `lower()` that folds
//! the way Postgres does. [`register`] replaces the built-in with one backed
//! by Rust's `str::to_lowercase`, which folds every letter Unicode gives a
//! lower-case mapping.
//!
//! The replacement is registered through `sqlite3_auto_extension`, which
//! SQLite runs for every connection the process opens from then on. That is
//! the one hook that reaches the raw `sqlite3*` of a pool connection: the
//! pool is an sqlx `AnyPool`, whose `after_connect` hands out an
//! `AnyConnection` with no way to the SQLite handle behind it, and whose
//! connect options come from a URL that carries no function or collation
//! list. The vendored sqlx-sqlite source stays unedited (VENDORING.md).
//!
//! No index depends on the function: the schema has no index on a `lower()`
//! expression, so replacing it changes what a query compares, never what a
//! stored index holds.

use std::ffi::{c_char, c_int, c_uchar};
use std::ptr;
use std::slice;
use std::sync::Once;

use libsqlite3_sys as ffi;

/// Register the Unicode `lower()` for every SQLite connection opened after
/// this call. Safe to call any number of times; only the first does work.
pub fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: `sqlite3_auto_extension` takes a plain function pointer
        // with the entry-point signature SQLite documents and keeps it in a
        // process-wide list; it holds no data of ours and frees nothing.
        let status = unsafe { ffi::sqlite3_auto_extension(Some(install_on_connection)) };
        assert_eq!(
            status,
            ffi::SQLITE_OK,
            "sqlite3_auto_extension refused the Unicode lower() (code {status})"
        );
    });
}

/// The auto-extension entry point: SQLite calls it with each new connection.
unsafe extern "C" fn install_on_connection(
    db: *mut ffi::sqlite3,
    _err_msg: *mut *mut c_char,
    _api: *const ffi::sqlite3_api_routines,
) -> c_int {
    // SAFETY: `db` is the connection SQLite is opening, the name is a valid
    // NUL-terminated string, and `unicode_lower` has the scalar-function
    // signature SQLite expects for a one-argument function.
    unsafe {
        ffi::sqlite3_create_function_v2(
            db,
            c"lower".as_ptr(),
            1,
            ffi::SQLITE_UTF8 | ffi::SQLITE_DETERMINISTIC | ffi::SQLITE_INNOCUOUS,
            ptr::null_mut(),
            Some(unicode_lower),
            None,
            None,
            None,
        )
    }
}

/// `lower(X)`: `X` with every letter in lower case, `NULL` for `NULL`. A
/// non-text argument is read as text first, as the built-in does.
unsafe extern "C" fn unicode_lower(
    ctx: *mut ffi::sqlite3_context,
    _n_arg: c_int,
    args: *mut *mut ffi::sqlite3_value,
) {
    // SAFETY: SQLite hands a one-argument function exactly one value, and
    // the value and context pointers stay valid for the duration of the
    // call. `sqlite3_value_bytes` after `sqlite3_value_text` is the UTF-8
    // byte length of the text that call produced.
    unsafe {
        let value = *args;
        if ffi::sqlite3_value_type(value) == ffi::SQLITE_NULL {
            ffi::sqlite3_result_null(ctx);
            return;
        }
        let text = ffi::sqlite3_value_text(value);
        if text.is_null() {
            ffi::sqlite3_result_error_nomem(ctx);
            return;
        }
        let len = usize::try_from(ffi::sqlite3_value_bytes(value)).unwrap_or(0);
        let bytes = slice::from_raw_parts(text, len);
        let folded = String::from_utf8_lossy(bytes).to_lowercase();
        ffi::sqlite3_result_text64(
            ctx,
            folded.as_ptr().cast::<c_char>(),
            folded.len() as u64,
            ffi::SQLITE_TRANSIENT(),
            ffi::SQLITE_UTF8 as c_uchar,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::db::engine::sqlite_test_pool;

    async fn lower_of(pool: &sqlx::AnyPool, text: &str) -> Option<String> {
        sqlx::query_scalar("SELECT lower($1)")
            .bind(text)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// The built-in `lower()` would leave `É` and `Ü` alone; every pool
    /// connection gets the Unicode one.
    #[tokio::test]
    async fn lower_folds_non_ascii_letters_on_every_connection() {
        let (pool, _dir) = sqlite_test_pool().await;
        assert_eq!(
            lower_of(&pool, "Élodie ÜNAL").await.as_deref(),
            Some("élodie ünal")
        );
        assert_eq!(
            lower_of(&pool, "ASCII Only").await.as_deref(),
            Some("ascii only")
        );
        let (other_pool, _dir) = sqlite_test_pool().await;
        assert_eq!(
            lower_of(&other_pool, "ÉQUIPE").await.as_deref(),
            Some("équipe")
        );
    }

    /// `NULL` stays `NULL` and a number is read as text, as the built-in does.
    #[tokio::test]
    async fn lower_keeps_null_and_reads_a_number_as_text() {
        let (pool, _dir) = sqlite_test_pool().await;
        let null: Option<String> = sqlx::query_scalar("SELECT lower(NULL)")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(null, None);
        let number: String = sqlx::query_scalar("SELECT lower(42)")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(number, "42");
    }
}
