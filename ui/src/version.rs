//! This bundle's release version (#129).
//!
//! `index.html` carries it in the loading screen (`#etl-version`, stamped at
//! build time by `ui/scripts/stamp-splash-version.sh`, #120). Read once in
//! `main` before the splash is removed, so the app can notice when the
//! server has moved on to another release.

use std::sync::OnceLock;

static BUNDLE: OnceLock<String> = OnceLock::new();

/// Remember the stamped version. Call before the loading screen is removed.
pub fn capture() {
    let version = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("etl-version"))
        .and_then(|el| el.get_attribute("data-version"))
        // An unstamped template (`trunk serve` without the hook) is unknown.
        .filter(|v| !v.contains("%%"))
        .unwrap_or_default();
    let _ = BUNDLE.set(version);
}

/// The stamped version, or `""` when unknown.
pub fn bundle() -> &'static str {
    BUNDLE.get().map(String::as_str).unwrap_or("")
}
