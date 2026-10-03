//! The desktop window's Content Security Policy runs only the app's own
//! scripts.
//!
//! Every command is open to any script in the window, so the policy in
//! `tauri.conf.json` is what keeps a script the app did not ship from
//! running there at all (issue #1127).

use serde_json::Value;

fn security() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json");
    let text = std::fs::read_to_string(path).expect("read tauri.conf.json");
    let config: Value = serde_json::from_str(&text).expect("parse tauri.conf.json");
    config["app"]["security"].clone()
}

#[test]
fn the_window_runs_only_the_apps_own_scripts() {
    let security = security();
    let csp = &security["csp"];
    assert!(csp.is_object(), "tauri.conf.json sets no csp: {csp}");

    let script_src = csp["script-src"]
        .as_str()
        .expect("the csp names script-src");
    assert_eq!(script_src, "'self'");

    // Tauri adds the hash of each inline script it shipped to script-src;
    // turning that off would block the theme script in index.html, and is
    // the setting someone reaches for when a script is refused.
    let disabled = &security["dangerousDisableAssetCspModification"];
    let script_src_kept = match disabled {
        Value::Null | Value::Bool(false) => true,
        Value::Array(directives) => !directives.iter().any(|d| d == "script-src"),
        _ => false,
    };
    assert!(
        script_src_kept,
        "Tauri must still add the hashes of the app's own inline scripts: {disabled}"
    );
}
