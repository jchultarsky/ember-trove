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
pub fn local_time_now() -> String {
    js_sys::eval(
        "new Intl.DateTimeFormat(undefined, {hour:'numeric',minute:'2-digit'}).format(new Date())",
    )
    .ok()
    .and_then(|v| v.as_string())
    .unwrap_or_default()
}

/// Format a UTC timestamp as a concise local date-time string using JS's
/// `Intl.DateTimeFormat` (avoids a full `chrono-tz` / `time` WASM dependency).
///
/// Example output: `"Mar 25, 2026, 14:32"`
pub fn format_timestamp(ts: &chrono::DateTime<chrono::Utc>) -> String {
    let iso = ts.to_rfc3339();
    let js = format!(
        "new Intl.DateTimeFormat(undefined, {{year:'numeric',month:'short',day:'numeric',\
         hour:'2-digit',minute:'2-digit'}}).format(new Date('{iso}'))"
    );
    js_sys::eval(&js)
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| iso[..16].replace('T', " "))
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
    let iso = ts.to_rfc3339();
    let js = format!(
        "new Intl.DateTimeFormat(undefined, {{month:'short',day:'numeric'}}).format(new Date('{iso}'))"
    );
    js_sys::eval(&js)
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| iso[..10].to_string())
}
