//! UI-side keyboard helpers — the thin `web_sys` glue over the pure decision
//! logic in `common::keyboard`.
//!
//! Phase 0 of the unified-keyboard-model work: a single shared
//! `active_element_is_editable()` replaces three copy-pasted inline guards
//! (`layout.rs`, `my_day_view.rs`, `inbox_triage.rs`) that had drifted — the
//! triage copy omitted `<button>` and `contenteditable`. Later phases add a
//! central dispatcher and shortcut registry on top of this.

use common::keyboard::target_is_editable;

/// True when the document's currently-focused element should swallow
/// single-key shortcuts (a text input, `<select>`, `<button>`, or
/// `contenteditable` region). Global single-key handlers return early when
/// this is true so typing never triggers a shortcut.
pub fn active_element_is_editable() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .map(|el| {
            let tag = el.tag_name().to_uppercase();
            target_is_editable(&tag, el.get_attribute("contenteditable").as_deref())
        })
        .unwrap_or(false)
}
