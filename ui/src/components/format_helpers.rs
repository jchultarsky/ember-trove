//! Shared formatting helpers used by multiple UI components.
//!
//! Centralises locale-aware date/time formatting so every panel speaks
//! the same language without duplicating JS interop boilerplate.

/// Return today's date in the browser's local timezone.
///
/// Uses `js_sys::Date` to read the browser clock, which respects the OS
/// timezone setting.  Falls back to UTC if the conversion fails.
pub fn local_today() -> chrono::NaiveDate {
    let d = js_sys::Date::new_0();
    chrono::NaiveDate::from_ymd_opt(
        d.get_full_year() as i32,
        d.get_month() + 1, // JS months are 0-based
        d.get_date(),
    )
    .unwrap_or_else(|| chrono::Utc::now().date_naive())
}

/// The current local time as a short clock label ("4:02 PM" / "16:02",
/// per the browser locale) via `Intl.DateTimeFormat`.
///
/// Uses `Date.prototype.toLocaleTimeString` through `js_sys` — no `eval`, so
/// it works under a CSP without `'unsafe-eval'` (the e2e/local nginx CSP;
/// an `eval` here silently returned nothing and rendered "Saved ·").
pub fn local_time_now() -> String {
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"hour".into(), &"numeric".into());
    let _ = js_sys::Reflect::set(&opts, &"minute".into(), &"2-digit".into());
    js_sys::Date::new_0()
        .to_locale_time_string_with_options("default", &opts)
        .into()
}

/// `Intl` options object from `(key, value)` pairs, for the locale formatters.
fn intl_options(pairs: &[(&str, &str)]) -> js_sys::Object {
    let opts = js_sys::Object::new();
    for (k, v) in pairs {
        let _ = js_sys::Reflect::set(&opts, &(*k).into(), &(*v).into());
    }
    opts
}

fn js_date(ts: &chrono::DateTime<chrono::Utc>) -> js_sys::Date {
    js_sys::Date::new(&(ts.timestamp_millis() as f64).into())
}

/// Format a UTC timestamp as a concise local date-time string via
/// `Date.prototype.toLocaleString` — no `eval` (CSP-safe) and no
/// `chrono-tz` / `time` WASM dependency.
///
/// Example output: `"Mar 25, 2026, 14:32"`
pub fn format_timestamp(ts: &chrono::DateTime<chrono::Utc>) -> String {
    let opts = intl_options(&[
        ("year", "numeric"),
        ("month", "short"),
        ("day", "numeric"),
        ("hour", "2-digit"),
        ("minute", "2-digit"),
    ]);
    let s: String = js_date(ts).to_locale_string("default", &opts).into();
    if s.is_empty() {
        ts.to_rfc3339()[..16].replace('T', " ")
    } else {
        s
    }
}

/// Short relative-time label for scannable lists (e.g. "just now", "2h ago",
/// "3d ago", or a short absolute date for anything older than a week).
/// Uses the browser clock via `js_sys::Date::now()` so the result reflects
/// the user's local time.
pub fn format_relative_short(ts: &chrono::DateTime<chrono::Utc>) -> String {
    let now_ms = js_sys::Date::now();
    let ts_ms = ts.timestamp_millis() as f64;
    let diff_secs = ((now_ms - ts_ms) / 1000.0) as i64;

    if diff_secs < 0 {
        return "just now".to_string();
    }
    if diff_secs < 45 {
        return "just now".to_string();
    }
    let diff_mins = diff_secs / 60;
    if diff_mins < 60 {
        return format!("{diff_mins}m ago");
    }
    let diff_hrs = diff_mins / 60;
    if diff_hrs < 24 {
        return format!("{diff_hrs}h ago");
    }
    let diff_days = diff_hrs / 24;
    if diff_days < 7 {
        return format!("{diff_days}d ago");
    }
    // Older than a week — show a short absolute date ("Apr 18") to remain scannable.
    let opts = intl_options(&[("month", "short"), ("day", "numeric")]);
    let s: String = js_date(ts).to_locale_date_string("default", &opts).into();
    if s.is_empty() {
        ts.to_rfc3339()[..10].to_string()
    } else {
        s
    }
}

/// Copy `text` to the clipboard via `navigator.clipboard.writeText`, reached
/// through `Reflect` — no `eval` (the old helpers built a JS string by hand,
/// which broke under a CSP without 'unsafe-eval') and no extra web-sys
/// features. Best-effort: the browser may refuse without a user gesture.
pub fn copy_to_clipboard(text: &str) {
    use wasm_bindgen::JsCast as _;
    let Some(win) = web_sys::window() else {
        return;
    };
    let get = |obj: &wasm_bindgen::JsValue, key: &str| js_sys::Reflect::get(obj, &key.into()).ok();
    let Some(clipboard) = get(&win.into(), "navigator").and_then(|n| get(&n, "clipboard")) else {
        return;
    };
    if let Some(write) =
        get(&clipboard, "writeText").and_then(|f| f.dyn_into::<js_sys::Function>().ok())
    {
        let _ = write.call1(&clipboard, &text.into());
    }
}
