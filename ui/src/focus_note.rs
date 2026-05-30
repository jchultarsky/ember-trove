//! Sibling of `focus_task` for notes: read `?note=<id>` from the current URL,
//! scroll the matching `[data-note-id="<id>"]` element into view, and briefly
//! highlight it. Used by `NodeView` so that clicking a node-attached note in
//! the central Notes feed jumps to the node and lands on that note.
//!
//! Polls a few times because the node's `NotePanel` loads its notes via
//! `LocalResource` and may not be in the DOM when the URL changes. Reuses the
//! `focus-task-flash` CSS animation. Strips the `?note=` param after focusing
//! so a refresh doesn't re-fire.

use leptos::wasm_bindgen::{closure::Closure, JsCast};

const RETRY_DELAYS_MS: [i32; 4] = [0, 200, 600, 1500];

/// Schedule the focus-note pass. Call once on view mount. No-ops when the URL
/// has no `?note=` param.
pub fn schedule_focus_note() {
    let Some(win) = web_sys::window() else { return; };
    let Ok(href) = win.location().href() else { return; };
    let Ok(url) = web_sys::Url::new(&href) else { return; };
    let Some(note_id) = url.search_params().get("note") else { return; };
    if note_id.is_empty() { return; }

    for delay in RETRY_DELAYS_MS {
        let id = note_id.clone();
        let win_ref = win.clone();
        let cb = Closure::once_into_js(move || {
            try_focus_note(&id, &win_ref);
        });
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            delay,
        );
    }
}

/// Read the `?note=<id>` param if present (so the NotePanel can auto-expand to
/// reveal a focused note that would otherwise be hidden behind "Show N more").
#[must_use]
pub fn pending_focus_note() -> Option<String> {
    let win = web_sys::window()?;
    let href = win.location().href().ok()?;
    let url = web_sys::Url::new(&href).ok()?;
    url.search_params().get("note").filter(|s| !s.is_empty())
}

fn try_focus_note(note_id: &str, win: &web_sys::Window) {
    let Some(doc) = win.document() else { return; };
    let selector = format!("[data-note-id=\"{note_id}\"]");
    let Ok(Some(el)) = doc.query_selector(&selector) else { return; };

    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_behavior(web_sys::ScrollBehavior::Smooth);
    opts.set_block(web_sys::ScrollLogicalPosition::Center);
    el.scroll_into_view_with_scroll_into_view_options(&opts);

    let _ = el.class_list().add_1("focus-task-flash");

    if let Ok(href) = win.location().href()
        && let Ok(url) = web_sys::Url::new(&href)
    {
        let params = url.search_params();
        params.delete("note");
        let qs = params.to_string().as_string().unwrap_or_default();
        let new_href = if qs.is_empty() {
            url.pathname()
        } else {
            format!("{}?{}", url.pathname(), qs)
        };
        if let Ok(history) = win.history() {
            let _ = history.replace_state_with_url(
                &leptos::wasm_bindgen::JsValue::NULL,
                "",
                Some(&new_href),
            );
        }
    }

    let el_for_cleanup = el;
    let cleanup = Closure::once_into_js(move || {
        let _ = el_for_cleanup.class_list().remove_1("focus-task-flash");
    });
    let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
        cleanup.as_ref().unchecked_ref(),
        1800,
    );
}
