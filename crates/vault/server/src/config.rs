//! Config file model ([`Config`]) plus path/source validation.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use message_ir_format::UNSAFE_ATTACHMENT_PATH_PREFIX;
use serde::Deserialize;

use crate::db::engine::{DbEngine, DbTarget, detect_engine};

/// Complete server configuration, loaded from a TOML file.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Filesystem locations (database, per-account data).
    pub paths: PathsConfig,
    /// HTTP ingest server (`message-vault-server serve`). Required for `serve`.
    #[serde(default)]
    pub server: Option<ServerConfig>,
    /// Database engine and connection URL. When `url` is set (a
    /// `postgres://…` or `sqlite://…` URL), `serve` connects through it
    /// instead of `paths.db`. Required for Postgres.
    #[serde(default)]
    pub database: DatabaseConfig,
}

/// `[database]` section: optional connection URL selecting the engine.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DatabaseConfig {
    /// Connection URL (`postgres://…` or `sqlite://…`). Unset = SQLite at
    /// `paths.db`.
    #[serde(default)]
    pub url: Option<String>,
}

/// `[server]` section: HTTP bind address, CORS, and asset upload limits.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Bind address (default `127.0.0.1:8080`).
    #[serde(default = "default_server_bind")]
    pub bind: String,
    /// Max size of one asset (single PUT or multipart complete), in bytes.
    /// Default 512 MiB.
    #[serde(default = "default_asset_max_bytes")]
    pub asset_max_bytes: u64,
    /// Multipart part size advertised to clients, in bytes. Default 64 MiB
    /// (under Cloudflare Free/Pro ~100 MB). Must be ≤ `asset_max_bytes`.
    #[serde(default = "default_asset_part_size")]
    pub asset_part_size: usize,
    /// Cross-Origin Resource Sharing (CORS) origins allowed to call this API,
    /// on top of the packaged desktop app's own origins, which are always
    /// allowed. CORS is the browser rule that decides which other websites may
    /// call this API. Empty is the right setting for a vault serving its own
    /// website, since that UI is same-origin and needs no header at all.
    /// Use `["*"]` only for local debugging. Example: `["https://app.example.com"]`.
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Serve Swagger UI at `/docs` and the spec at `/openapi.json`. Default false.
    #[serde(default = "default_openapi_ui")]
    pub openapi_ui: bool,
}

/// serde default for `[server] bind`.
fn default_server_bind() -> String {
    "127.0.0.1:8080".to_string()
}

/// serde default for `[server] asset_max_bytes` (512 MiB).
fn default_asset_max_bytes() -> u64 {
    512 * 1024 * 1024
}

/// serde default for `[server] asset_part_size` (64 MiB).
fn default_asset_part_size() -> usize {
    64 * 1024 * 1024
}

/// serde default for `[server] openapi_ui`.
fn default_openapi_ui() -> bool {
    false
}

/// `[paths]` section: database file and per-account data directories.
#[derive(Debug, Clone, Deserialize)]
pub struct PathsConfig {
    /// SQLite database file path.
    pub db: PathBuf,
    /// Root for per-account data (`data/<account_id>/…`).
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// Directory name for originals under each account source (default `assets`).
    #[serde(default = "default_assets_dir_name")]
    pub assets_dir: String,
    /// Directory name for converted media under each account source.
    #[serde(default = "default_assets_converted_dir_name")]
    pub assets_converted_dir: String,
}

/// serde default for `[paths] data_dir`.
fn default_data_dir() -> PathBuf {
    PathBuf::from("data")
}

/// serde default for `[paths] assets_dir`.
fn default_assets_dir_name() -> String {
    "assets".to_string()
}

/// serde default for `[paths] assets_converted_dir`.
fn default_assets_converted_dir_name() -> String {
    "assets_converted".to_string()
}

/// Safe source slug for path segments and `messages.source` values.
///
/// # Errors
///
/// Returns an error when the id is empty, too long, or uses disallowed characters.
pub fn validate_source_id(source: &str) -> Result<()> {
    let s = source.trim();
    if s.is_empty() {
        bail!("source id must not be empty");
    }
    if s.len() > 64 {
        bail!("source id must be at most 64 characters");
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        bail!("source id '{s}' must use only lowercase letters, digits, hyphens, and underscores");
    }
    if s.starts_with('-') || s.starts_with('_') {
        bail!("source id must not start with '-' or '_'");
    }
    Ok(())
}

/// Reject absolute paths and `..` so joins stay under an approved root.
///
/// # Errors
///
/// Returns an error when `name` is empty, absolute, or contains `..`.
pub fn safe_rel_path(name: &str) -> Result<PathBuf> {
    use std::path::{Component, Path};

    let name = name.trim();
    if name.is_empty() {
        bail!("empty attachment path");
    }
    let path = Path::new(name);
    if path.is_absolute() {
        bail!("attachment path must be relative: {name}");
    }
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("{UNSAFE_ATTACHMENT_PATH_PREFIX}: {name}");
            }
        }
    }
    if out.as_os_str().is_empty() {
        bail!("empty attachment path after normalize: {name}");
    }
    Ok(out)
}

/// Join `rel` under `root` after rejecting traversal. Does not follow the final path.
///
/// # Errors
///
/// Returns an error when `rel` is not a safe relative path.
pub fn resolve_under_root(root: &Path, rel: &str) -> Result<PathBuf> {
    Ok(root.join(safe_rel_path(rel)?))
}

impl PathsConfig {
    /// Originals: `data_dir/<account_id>/<source_id>/<assets_dir>`.
    pub fn assets_dir_for_account(&self, account_id: &str, source_id: &str) -> PathBuf {
        self.data_dir
            .join(account_id)
            .join(source_id)
            .join(&self.assets_dir)
    }

    /// Converted media: `data_dir/<account_id>/<source_id>/<assets_converted_dir>`.
    pub fn assets_converted_dir_for_account(&self, account_id: &str, source_id: &str) -> PathBuf {
        self.data_dir
            .join(account_id)
            .join(source_id)
            .join(&self.assets_converted_dir)
    }
}

impl Config {
    /// Read and parse a TOML config file. Relative `paths.db` and
    /// `paths.data_dir` values resolve against the directory above the config
    /// file's folder (the repo root for `config/config.toml`).
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let mut config: Config = toml::from_str(&text)
            .with_context(|| format!("failed to parse config {}", path.display()))?;

        let abs_config = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .context("failed to get current directory")?
                .join(path)
        };
        let config_dir = abs_config
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let repo = config_dir
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(config_dir);

        config.paths.db = resolve_path(repo, &config.paths.db);
        config.paths.data_dir = resolve_path(repo, &config.paths.data_dir);

        Ok(config)
    }

    /// Server settings for `serve`. Fails if `[server]` is missing.
    pub fn require_server(&self) -> Result<&ServerConfig> {
        let server = self
            .server
            .as_ref()
            .context("config missing [server] section (needed for serve)")?;
        if server.asset_part_size == 0 {
            bail!("server.asset_part_size must be > 0");
        }
        if server.asset_max_bytes == 0 {
            bail!("server.asset_max_bytes must be > 0");
        }
        if server.asset_part_size as u64 > server.asset_max_bytes {
            bail!(
                "server.asset_part_size ({}) must be ≤ server.asset_max_bytes ({})",
                server.asset_part_size,
                server.asset_max_bytes
            );
        }
        Ok(server)
    }
}

/// A configured path made absolute against the config file's folder, unless it already is.
fn resolve_path(base: &Path, configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        base.join(configured)
    }
}

impl Config {
    /// Apply the command line's database flags: `--db` replaces `paths.db`
    /// and `--db-url` replaces `[database] url`. After this the config alone
    /// says where the vault's database is; see [`Config::db_target`].
    pub(crate) fn with_db_overrides(mut self, db: Option<PathBuf>, db_url: Option<String>) -> Self {
        if let Some(db) = db {
            self.paths.db = db;
        }
        if let Some(url) = db_url {
            self.database.url = Some(url);
        }
        self
    }

    /// Where the vault's database is: the connection URL when one is set,
    /// otherwise the SQLite file at `paths.db`. The URL always wins because
    /// it can name a Postgres server, which a path never can.
    pub(crate) fn db_target(&self) -> DbTarget<'_> {
        DbTarget::new(self.database.url.as_deref(), &self.paths.db)
    }

    /// The engine [`Config::db_target`] selects: SQLite unless the URL's
    /// scheme says Postgres.
    ///
    /// # Errors
    ///
    /// Returns an error for a URL whose scheme is neither.
    pub(crate) fn db_engine(&self) -> Result<DbEngine> {
        match self.database.url.as_deref() {
            Some(url) => detect_engine(url),
            None => Ok(DbEngine::Sqlite),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_at(db: &str) -> Config {
        Config {
            paths: PathsConfig {
                db: PathBuf::from(db),
                data_dir: PathBuf::from("/vault/data"),
                assets_dir: "assets".into(),
                assets_converted_dir: "assets_converted".into(),
            },
            server: None,
            database: DatabaseConfig::default(),
        }
    }

    #[test]
    fn without_overrides_the_database_is_the_configured_sqlite_file() {
        let cfg = config_at("/vault/vault.db").with_db_overrides(None, None);

        assert_eq!(cfg.paths.db, PathBuf::from("/vault/vault.db"));
        assert_eq!(cfg.database.url, None);
        assert_eq!(cfg.db_target().to_string(), "/vault/vault.db");
        assert_eq!(cfg.db_engine().unwrap(), DbEngine::Sqlite);
    }

    #[test]
    fn db_override_replaces_the_sqlite_path() {
        let cfg = config_at("/vault/vault.db")
            .with_db_overrides(Some(PathBuf::from("/elsewhere/other.db")), None);

        assert_eq!(cfg.db_target().to_string(), "/elsewhere/other.db");
    }

    #[test]
    fn db_url_override_wins_over_the_path_and_names_the_engine() {
        let cfg = config_at("/vault/vault.db").with_db_overrides(
            Some(PathBuf::from("/elsewhere/other.db")),
            Some("postgres://vault:secret@db.example:5432/vault".into()),
        );

        assert_eq!(
            cfg.db_target().to_string(),
            "postgres://db.example:5432/vault"
        );
        assert_eq!(cfg.db_engine().unwrap(), DbEngine::Postgres);
    }

    #[test]
    fn a_configured_url_is_honoured_without_any_override() {
        let mut cfg = config_at("/vault/vault.db");
        cfg.database.url = Some("sqlite:///elsewhere/other.db".into());

        let cfg = cfg.with_db_overrides(None, None);

        assert_eq!(cfg.db_target().to_string(), "sqlite:///elsewhere/other.db");
        assert_eq!(cfg.db_engine().unwrap(), DbEngine::Sqlite);
    }

    #[test]
    fn an_unknown_url_scheme_is_an_error() {
        let mut cfg = config_at("/vault/vault.db");
        cfg.database.url = Some("mysql://db.example/vault".into());

        assert!(cfg.db_engine().is_err());
    }

    #[test]
    fn validate_source_id_accepts_slugs() {
        assert!(validate_source_id("imessage").is_ok());
        assert!(validate_source_id("go-sms-pro").is_ok());
        assert!(validate_source_id("sms_backup_plus").is_ok());
        assert!(validate_source_id("a1").is_ok());
    }

    #[test]
    fn validate_source_id_rejects_bad() {
        assert!(validate_source_id("").is_err());
        assert!(validate_source_id("iMessage").is_err());
        assert!(validate_source_id("../x").is_err());
        assert!(validate_source_id("-bad").is_err());
        assert!(validate_source_id("has space").is_err());
    }

    #[test]
    fn safe_rel_path_rejects_traversal() {
        assert!(safe_rel_path("attachments/a.jpg").is_ok());
        assert!(safe_rel_path("../etc/passwd").is_err());
        assert!(safe_rel_path("/etc/passwd").is_err());
        assert!(safe_rel_path("").is_err());
        assert!(safe_rel_path("a/../../b").is_err());
    }

    #[test]
    fn resolve_under_root_keeps_paths_inside() {
        let root = PathBuf::from("/tmp/export");
        let joined = resolve_under_root(&root, "attachments/a.jpg").unwrap();
        assert_eq!(joined, root.join("attachments/a.jpg"));
        assert!(resolve_under_root(&root, "../outside").is_err());
    }

    #[test]
    fn openapi_ui_defaults_false() {
        let raw = r#"
bind = "127.0.0.1:8080"
"#;
        let cfg: ServerConfig = toml::from_str(raw).unwrap();
        assert!(!cfg.openapi_ui);
    }

    #[test]
    fn openapi_ui_can_enable() {
        let raw = r#"
bind = "127.0.0.1:8080"
openapi_ui = true
"#;
        let cfg: ServerConfig = toml::from_str(raw).unwrap();
        assert!(cfg.openapi_ui);
    }

    const PACKAGED_ORIGINS: &[&str] = &[
        "https://tauri.localhost",
        "http://tauri.localhost",
        "tauri://localhost",
    ];

    /// `scripts/run-vault-dev.sh` only uncomments the `# cors_origins =` line.
    /// That line must be a complete array or first-run / `--reset-demo` configs
    /// are invalid TOML.
    #[test]
    fn example_cors_origins_uncomments_to_a_complete_array() {
        let example = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../config/config.toml.example"
        ));
        let cors_lines: Vec<&str> = example
            .lines()
            .filter(|line| {
                line.starts_with("# cors_origins =") || line.starts_with("cors_origins =")
            })
            .collect();
        assert_eq!(
            cors_lines.len(),
            1,
            "run-vault-dev.sh uncomments one cors_origins line"
        );
        assert!(
            cors_lines[0].contains('[') && cors_lines[0].contains(']'),
            "cors_origins must stay on one line so sed yields a closed array, got {}",
            cors_lines[0]
        );

        let uncommented: String = example
            .lines()
            .map(|line| {
                line.strip_prefix("# cors_origins =")
                    .map(|rest| format!("cors_origins ={rest}"))
                    .unwrap_or_else(|| line.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n");
        let cfg: Config =
            toml::from_str(&uncommented).expect("example after run-vault-dev.sh sed must parse");
        let origins = &cfg
            .server
            .as_ref()
            .expect("[server] in example")
            .cors_origins;
        for origin in [
            "http://localhost:5173",
            "http://127.0.0.1:5173",
            PACKAGED_ORIGINS[0],
            PACKAGED_ORIGINS[1],
            PACKAGED_ORIGINS[2],
        ] {
            assert!(
                origins.iter().any(|item| item == origin),
                "missing {origin} in {origins:?}"
            );
        }
    }

    #[test]
    fn docker_config_includes_packaged_desktop_origins() {
        let docker = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../config/config.docker.toml"
        ));
        let cfg: Config = toml::from_str(docker).expect("config.docker.toml must parse");
        let origins = &cfg
            .server
            .as_ref()
            .expect("[server] in docker config")
            .cors_origins;
        for origin in PACKAGED_ORIGINS {
            assert!(
                origins.iter().any(|item| item == origin),
                "missing {origin} in {origins:?}"
            );
        }
    }
}
