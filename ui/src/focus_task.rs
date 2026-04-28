//! Helper for the v2.6.2 row-click navigation: read `?task=<id>` from
//! the current URL, scroll the matching `[data-task-id="<id>"]` element
//! into view, and briefly highlight it.
//!
//! Used by `NodeView` and `InboxView` (both render task rows that carry
//! `data-task-id` attributes).  The Kanban's `KanbanTaskRow` writes the
//! query param when the user clicks anywhere on the row body.
//!
//! Polls a few times because the destination view's task list is loaded
//! via `LocalResource` and may not be in the DOM at the moment the URL
//! changes.  We try at 0ms, 200ms, 600ms, 1500ms — covers a slow API
//! response without keeping a long-lived listener.  Once an element is
//! found, the param is `replaceState`'d out of the URL so a refresh
//! doesn't re-fire it.

use leptos::wasm_bindgen::{closure::Closure, JsCast};

const RETRY_DELAYS_MS: [i32; 4] = [0, 200, 600, 1500];

/// Schedule the focus-task pass.  Call once on view mount.  No-ops when
/// the URL has no `?task=` param.
pub fn schedule_focus_task() {
    let Some(win) = web_sys::window() else { return; };
    let Ok(href)  = win.location().href() else { return; };
    let Ok(url)   = web_sys::Url::new(&href) else { return; };
    let Some(task_id) = url.search_params().get("task") else { return; };
    if task_id.is_empty() { return; }

    for delay in RETRY_DELAYS_MS {
        let id = task_id.clone();
        let win_ref = win.clone();
        let cb = Closure::once_into_js(move || {
            try_focus_task(&id, &win_ref);
        });
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            delay,
        );
    }
}

fn try_focus_task(task_id: &str, win: &web_sys::Window) {
    let Some(doc) = win.document() else { return; };
    let selector  = format!("[data-task-id=\"{task_id}\"]");
    let Ok(Some(el)) = doc.query_selector(&selector) else { return; };

    // Scroll into view, centred so the user can see surrounding context.
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_behavior(web_sys::ScrollBehavior::Smooth);
    opts.set_block(web_sys::ScrollLogicalPosition::Center);
    el.scroll_into_view_with_scroll_into_view_options(&opts);

    // Apply a temporary amber ring so the user's eye lands on the right
    // row.  The CSS class fades itself out via animation.
    let _ = el.class_list().add_1("focus-task-flash");

    // Strip the `?task=` param so a refresh doesn't re-fire the highlight.
    if let Ok(href) = win.location().href()
        && let Ok(url) = web_sys::Url::new(&href)
    {
        let params = url.search_params();
        params.delete("task");
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

    // Schedule cleanup of the flash class after the animation duration
    // (matches the CSS keyframes — 1800ms).
    let el_for_cleanup = el;
    let cleanup = Closure::once_into_js(move || {
        let _ = el_for_cleanup.class_list().remove_1("focus-task-flash");
    });
    let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
        cleanup.as_ref().unchecked_ref(),
        1800,
    );
}
