//! Named points in the app's start-up (`performance.mark`), read by the
//! performance measurement (`e2e/perf/`, P0 of the performance phase,
//! 2026-11-02) with `performance.getEntriesByType("mark")`. Cheap, and they
//! carry no data: only a name and a time.
//!
//! - `et:mounted`: the Leptos tree is in the DOM (main.rs)
//! - `et:auth`: the sign-in check passed (`AuthGate`)

pub fn mark(name: &str) {
    if let Some(performance) = web_sys::window().and_then(|w| w.performance()) {
        let _ = performance.mark(name);
    }
}
