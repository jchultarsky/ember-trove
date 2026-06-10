pub mod add_favorite;
pub mod command_palette;
pub mod create_node;
pub mod delete_confirm;
pub mod fast_capture;
pub mod help;

use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

/// Selector for the elements a modal focus trap cycles through.
const FOCUSABLE: &str = "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])";

/// Keep `Tab` / `Shift+Tab` cycling inside `container` (WCAG modal focus
/// trap). Attach from the modal panel's `on:keydown` with the panel element.
pub fn trap_focus(ev: &web_sys::KeyboardEvent, container: &web_sys::HtmlElement) {
    if ev.key() != "Tab" {
        return;
    }
    let Ok(nodes) = container.query_selector_all(FOCUSABLE) else {
        return;
    };
    if nodes.length() == 0 {
        return;
    }
    let first = nodes.get(0);
    let last = nodes.get(nodes.length() - 1);
    let active: Option<web_sys::Node> = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .map(|e| e.unchecked_into());

    let is_same = |a: &Option<web_sys::Node>, b: &Option<web_sys::Node>| match (a, b) {
        (Some(a), Some(b)) => a.is_same_node(Some(b)),
        _ => false,
    };

    if ev.shift_key() {
        if is_same(&active, &first) {
            ev.prevent_default();
            if let Some(el) = last.and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
                let _ = el.focus();
            }
        }
    } else if is_same(&active, &last) {
        ev.prevent_default();
        if let Some(el) = first.and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    }
}

/// Restore focus to whatever had it before the modal opened.
///
/// Call once during component setup with the modal's `show` signal: the
/// element focused at open time is remembered, and focused again when the
/// modal closes (WCAG focus-return).
pub fn return_focus_on_close(show: Signal<bool>) {
    let prev_focus: StoredValue<Option<web_sys::HtmlElement>> = StoredValue::new(None);
    Effect::new(move |was_open: Option<bool>| {
        let open = show.get();
        if open && was_open != Some(true) {
            prev_focus.set_value(
                web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.active_element())
                    .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()),
            );
        } else if !open
            && was_open == Some(true)
            && let Some(el) = prev_focus.get_value()
        {
            let _ = el.focus();
        }
        open
    });
}
