//! Config file model ([`Config`]) plus path/source validation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::db::engine::{DbEngine, DbTarget, detect_engine};

/// Complete server configuration, loaded from a TOML file. It is read only
/// through [`Config::load`], which refuses a key the server does not use.
#[derive(Debug, Clone)]
pub struct Config {
    /// Filesystem locations (database, per-account data).
    pub paths: PathsConfig,
    /// HTTP ingest server (`message-crate-server serve`). Required for `serve`.
    pub server: Option<ServerConfig>,
    /// Database engine and connection URL. When `url` is set (a
    /// `postgres://…` or `sqlite://…` URL), `serve` connects through it
    /// instead of `paths.db`. Required for Postgres.
    pub database: DatabaseConfig,
}

/// Where the keys the config file takes are listed, for a refusal to point at.
const CONFIG_REFERENCE: &str =
    "https://messagecrate.app/docs/developer/reference/config-and-accounts/";

/// One section as the file has it: the keys the server uses, and every other
/// key the section carries. The second map is what lets [`Config::load`]
/// refuse an unknown key by its own name and its section's.
#[derive(Debug, Deserialize)]
struct Section<T> {
    #[serde(flatten)]
    known: T,
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

/// The config file as it is read, before unknown keys are refused.
#[derive(Debug, Deserialize)]
struct ConfigFile {
    paths: Section<PathsConfig>,
    #[serde(default)]
    server: Option<Section<ServerConfig>>,
    #[serde(default)]
    database: Option<Section<DatabaseConfig>>,
    /// Sections the server does not have, and keys outside any section.
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

/// Unknown keys as a refusal lists them: `` `a`, `b` ``.
fn key_list(unknown: &BTreeMap<String, toml::Value>) -> String {
    unknown
        .keys()
        .map(|key| format!("`{key}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Take a section's known keys, refusing it when it carries any other.
///
/// A key the server does not use is an error, never ignored: a misspelt key
/// would load as its default, and a key that was removed would sit in the
/// file looking as though it still held.
fn known_keys<T>(name: &str, section: Section<T>) -> Result<T> {
    if name == "server" && section.unknown.contains_key("asset_max_bytes") {
        bail!(
            "[server] asset_max_bytes is not a config key. The attachment size limit is a \
             Server Setting: the owner changes it on the Server Settings screen. Remove the line."
        );
    }
    if !section.unknown.is_empty() {
        bail!(
            "[{name}] has a key the server does not use: {}. Remove it, or correct its name; \
             the keys the config file takes are listed at {CONFIG_REFERENCE}",
            key_list(&section.unknown)
        );
    }
    Ok(section.known)
}

impl ConfigFile {
    /// The configuration the file states, or the refusal of what it should not hold.
    fn into_config(self) -> Result<Config> {
        if !self.unknown.is_empty() {
            bail!(
                "{} is not a section or key the server uses. The sections are [paths], \
                 [server] and [database]; their keys are listed at {CONFIG_REFERENCE}",
                key_list(&self.unknown)
            );
        }
        Ok(Config {
            paths: known_keys("paths", self.paths)?,
            server: self
                .server
                .map(|section| known_keys("server", section))
                .transpose()?,
            database: self
                .database
                .map(|section| known_keys("database", section))
                .transpose()?
                .unwrap_or_default(),
        })
    }
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
    /// Multipart part size advertised to clients, in bytes. Default 64 MiB
    /// (under Cloudflare Free/Pro ~100 MB). Must not exceed the attachment
    /// size limit, which is a Server Setting the owner changes in the app and
    /// has no key in this file.
    #[serde(default = "default_asset_part_size")]
    pub asset_part_size: usize,
    /// Cross-Origin Resource Sharing (CORS) origins allowed to call this API,
    /// on top of the packaged desktop app's own origins, which are always
    /// allowed. CORS is the browser rule that decides which other websites may
    /// call this API. Empty is the right setting for a server serving its own
    /// website, since that UI is same-origin and needs no header at all.
    /// Use `["*"]` only for local debugging. Example: `["https://app.example.com"]`.
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Serve Swagger UI at `/docs` and the spec at `/openapi.json`. Default false.
    #[serde(default = "default_openapi_ui")]
    pub openapi_ui: bool,
    /// Folder holding the built website, served at `/`. Default `static`,
    /// relative to the directory the server is started in.
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
}

impl Default for ServerConfig {
    /// The `[server]` section with every key left out.
    fn default() -> Self {
        Self {
            bind: default_server_bind(),
            asset_part_size: default_asset_part_size(),
            cors_origins: Vec::new(),
            openapi_ui: default_openapi_ui(),
            static_dir: default_static_dir(),
        }
    }
}

/// serde default for `[server] static_dir`.
fn default_static_dir() -> PathBuf {
    PathBuf::from("static")
}

/// serde default for `[server] bind`.
fn default_server_bind() -> String {
    "127.0.0.1:8080".to_string()
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

impl PathsConfig {
    /// Originals: `data_dir/<account_id>/<source_id>/<assets_dir>`.
    pub fn assets_dir_for_account(&self, account_id: i64, source_id: &str) -> PathBuf {
        self.data_dir
            .join(account_id.to_string())
            .join(source_id)
            .join(&self.assets_dir)
    }

    /// Converted media: `data_dir/<account_id>/<source_id>/<assets_converted_dir>`.
    pub fn assets_converted_dir_for_account(&self, account_id: i64, source_id: &str) -> PathBuf {
        self.data_dir
            .join(account_id.to_string())
            .join(source_id)
            .join(&self.assets_converted_dir)
    }
}

impl Config {
    /// Read and parse a TOML config file. Relative `paths.db` and
    /// `paths.data_dir` values resolve against the directory above the config
    /// file's folder (the repo root for `config/config.toml`).
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, or carries a
    /// section or key the server does not use; the error names each one.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let mut config =
            Self::parse(&text).with_context(|| format!("config {} was refused", path.display()))?;

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

    /// The config for a Message Crate kept whole in one folder, with no config
    /// file: the database is `messagecrate.db` in `data_dir`, the accounts'
    /// files sit beside it, and every server setting has its default. This is
    /// what `serve --data-dir` runs on, and how the desktop app starts the
    /// server without writing a file a person would have to find.
    pub fn for_data_dir(data_dir: &Path) -> Self {
        Self {
            paths: PathsConfig {
                db: data_dir.join("messagecrate.db"),
                data_dir: data_dir.to_path_buf(),
                assets_dir: default_assets_dir_name(),
                assets_converted_dir: default_assets_converted_dir_name(),
            },
            server: Some(ServerConfig::default()),
            database: DatabaseConfig::default(),
        }
    }

    /// Apply `serve`'s own flags: `--bind` replaces `[server] bind` and
    /// `--static-dir` replaces `[server] static_dir`. A config with no
    /// `[server]` section is left without one, for `require_server` to refuse.
    pub(crate) fn with_serve_overrides(
        mut self,
        bind: Option<String>,
        static_dir: Option<PathBuf>,
    ) -> Self {
        if let Some(server) = self.server.as_mut() {
            if let Some(bind) = bind {
                server.bind = bind;
            }
            if let Some(static_dir) = static_dir {
                server.static_dir = static_dir;
            }
        }
        self
    }

    /// The configuration a config file's text states, with paths as written.
    ///
    /// # Errors
    ///
    /// Returns an error when the text is not the TOML the server expects, or
    /// carries a section or key the server does not use.
    fn parse(text: &str) -> Result<Self> {
        let file: ConfigFile = toml::from_str(text)?;
        file.into_config()
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
    /// says where the database is; see [`Config::db_target`].
    pub(crate) fn with_db_overrides(mut self, db: Option<PathBuf>, db_url: Option<String>) -> Self {
        if let Some(db) = db {
            self.paths.db = db;
        }
        if let Some(url) = db_url {
            self.database.url = Some(url);
        }
        self
    }

    /// Where the database is: the connection URL when one is set,
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
                data_dir: PathBuf::from("/srv/data"),
                assets_dir: "assets".into(),
                assets_converted_dir: "assets_converted".into(),
            },
            server: None,
            database: DatabaseConfig::default(),
        }
    }

    #[test]
    fn without_overrides_the_database_is_the_configured_sqlite_file() {
        let cfg = config_at("/srv/messagecrate.db").with_db_overrides(None, None);

        assert_eq!(cfg.paths.db, PathBuf::from("/srv/messagecrate.db"));
        assert_eq!(cfg.database.url, None);
        assert_eq!(cfg.db_target().to_string(), "/srv/messagecrate.db");
        assert_eq!(cfg.db_engine().unwrap(), DbEngine::Sqlite);
    }

    #[test]
    fn db_override_replaces_the_sqlite_path() {
        let cfg = config_at("/srv/messagecrate.db")
            .with_db_overrides(Some(PathBuf::from("/elsewhere/other.db")), None);

        assert_eq!(cfg.db_target().to_string(), "/elsewhere/other.db");
    }

    #[test]
    fn db_url_override_wins_over_the_path_and_names_the_engine() {
        let cfg = config_at("/srv/messagecrate.db").with_db_overrides(
            Some(PathBuf::from("/elsewhere/other.db")),
            Some("postgres://app:secret@db.example:5432/messagecrate".into()),
        );

        assert_eq!(
            cfg.db_target().to_string(),
            "postgres://db.example:5432/messagecrate"
        );
        assert_eq!(cfg.db_engine().unwrap(), DbEngine::Postgres);
    }

    #[test]
    fn a_configured_url_is_honoured_without_any_override() {
        let mut cfg = config_at("/srv/messagecrate.db");
        cfg.database.url = Some("sqlite:///elsewhere/other.db".into());

        let cfg = cfg.with_db_overrides(None, None);

        assert_eq!(cfg.db_target().to_string(), "sqlite:///elsewhere/other.db");
        assert_eq!(cfg.db_engine().unwrap(), DbEngine::Sqlite);
    }

    #[test]
    fn an_unknown_url_scheme_is_an_error() {
        let mut cfg = config_at("/srv/messagecrate.db");
        cfg.database.url = Some("mysql://db.example/messagecrate".into());

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
    fn relative_paths_resolve_against_the_folder_above_the_config_folder() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let path = config_dir.join("server.toml");
        fs::write(
            &path,
            "[paths]\ndb = \"data/messagecrate.db\"\ndata_dir = \"data\"\n",
        )
        .unwrap();

        let cfg = Config::load(&path).unwrap();

        assert_eq!(cfg.paths.db, dir.path().join("data/messagecrate.db"));
        assert_eq!(cfg.paths.data_dir, dir.path().join("data"));
    }

    /// The defaults `docs/developer/reference/config-and-accounts.md` states.
    #[test]
    fn a_config_with_no_settings_loads_the_documented_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let path = config_dir.join("config.toml");
        fs::write(
            &path,
            "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\n",
        )
        .unwrap();

        let cfg = Config::load(&path).unwrap();

        assert_eq!(cfg.paths.data_dir, dir.path().join("data"));
        assert_eq!(
            cfg.paths.assets_dir_for_account(7, "imessage"),
            dir.path().join("data/7/imessage/assets")
        );
        assert_eq!(
            cfg.paths.assets_converted_dir_for_account(7, "imessage"),
            dir.path().join("data/7/imessage/assets_converted")
        );
        let server = cfg.require_server().unwrap();
        assert_eq!(server.bind, "127.0.0.1:8080");
        assert_eq!(server.asset_part_size, 67_108_864);
        assert!(!server.openapi_ui);
        assert!(server.cors_origins.is_empty());
    }

    /// Write `text` as `config/config.toml` under a fresh folder and load it.
    fn load_text(text: &str) -> Result<Config> {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let path = config_dir.join("config.toml");
        fs::write(&path, text).unwrap();
        Config::load(&path)
    }

    /// The limit left the config file. A line that still sets it is refused,
    /// and the refusal says where the limit is set now, because a line that
    /// loaded and did nothing would leave the operator believing it held.
    #[test]
    fn a_config_that_still_sets_asset_max_bytes_is_refused_and_told_where_the_limit_lives() {
        let err = load_text(
            "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nasset_max_bytes = 536870912\n",
        )
        .unwrap_err();
        let text = format!("{err:#}");

        assert!(text.contains("config.toml"), "{text}");
        assert!(text.contains("[server] asset_max_bytes"), "{text}");
        assert!(text.contains("Server Settings screen"), "{text}");
    }

    /// A key the server does not use is refused wherever it sits, by its name
    /// and its section: a misspelt key otherwise loads as the default.
    #[test]
    fn a_config_with_an_unknown_key_is_refused_naming_the_key_and_its_section() {
        for (section, config) in [
            (
                "[paths]",
                "[paths]\ndb = \"data/messagecrate.db\"\nasset_dir = \"assets\"\n",
            ),
            (
                "[server]",
                "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nasset_dir = 1\n",
            ),
            (
                "[database]",
                "[paths]\ndb = \"data/messagecrate.db\"\n\n[database]\nasset_dir = 1\n",
            ),
        ] {
            let text = format!("{:#}", load_text(config).unwrap_err());
            assert!(text.contains("`asset_dir`"), "{section}: {text}");
            assert!(text.contains(section), "{section}: {text}");
        }
    }

    /// Every unknown key is named, not only the first.
    #[test]
    fn every_unknown_key_in_a_section_is_named() {
        let text = format!(
            "{:#}",
            load_text(
                "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nbnd = \"x\"\nport = 1\n"
            )
            .unwrap_err()
        );
        assert!(text.contains("`bnd`") && text.contains("`port`"), "{text}");
    }

    /// A section the server does not have, or a key outside any section.
    #[test]
    fn a_config_with_an_unknown_section_is_refused_naming_it() {
        let text = format!(
            "{:#}",
            load_text("[paths]\ndb = \"data/messagecrate.db\"\n\n[sever]\nbind = \"x\"\n")
                .unwrap_err()
        );
        assert!(text.contains("`sever`"), "{text}");
        assert!(text.contains("[paths], [server] and [database]"), "{text}");
    }

    /// The config files the repository ships must load under the same rule:
    /// the example a developer copies, the one the Docker image starts from,
    /// and the one `reset-demo` installs.
    #[test]
    fn every_committed_config_file_loads() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        for file in [
            "config/config.toml.example",
            "config/config.docker.toml",
            "crates/server/demo-seed/config/config.toml",
        ] {
            let text = fs::read_to_string(repo.join(file)).unwrap();
            if let Err(err) = load_text(&text) {
                panic!("{file} does not load: {err:#}");
            }
        }
    }

    const PACKAGED_ORIGINS: &[&str] = &[
        "https://tauri.localhost",
        "http://tauri.localhost",
        "tauri://localhost",
    ];

    /// `scripts/run-dev.sh` only uncomments the `# cors_origins =` line.
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
            "run-dev.sh uncomments one cors_origins line"
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
            Config::parse(&uncommented).expect("example after run-dev.sh sed must parse");
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
        let cfg = Config::parse(docker).expect("config.docker.toml must parse");
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
