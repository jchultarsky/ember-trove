//! Keyboard-dispatch helpers shared by the UI's shortcut handlers.
//!
//! The UI (`ui/src/keyboard.rs`) extracts the focused element's tag and
//! `contenteditable` attribute from the DOM and calls [`target_is_editable`]
//! to decide whether a single-key shortcut should be suppressed. Keeping the
//! decision here — a pure function over primitives — lets it be unit-tested on
//! the host (the WASM `ui` crate is not host-tested; see POLICY §5).

/// True when a focused element should swallow single-key shortcuts: text
/// inputs, `<select>`, `<button>`, or any `contenteditable` region.
///
/// `tag_name_upper` is the element's tag name upper-cased (e.g. `"INPUT"`).
/// `contenteditable` is the raw `contenteditable` attribute value if present
/// (`Some("")`/`Some("true")` = editable, `Some("false")` = not, `None` =
/// attribute absent).
///
/// `<button>` is included so a single-key shortcut doesn't fire while a tap
/// button holds focus (e.g. `Enter` on a focused row button shouldn't also
/// trigger the row's Enter shortcut).
pub fn target_is_editable(tag_name_upper: &str, contenteditable: Option<&str>) -> bool {
    if matches!(tag_name_upper, "INPUT" | "TEXTAREA" | "SELECT" | "BUTTON") {
        return true;
    }
    contenteditable.map(|v| v != "false").unwrap_or(false)
}

// ── Global shortcut registry (single source of truth) ────────────────────────
//
// One table drives BOTH dispatch (via `match_global`, called from the single
// window listener in `ui/src/components/layout.rs`) AND the help modal's
// "Anywhere" table (`ui/src/components/modals/help.rs`), so the documented
// shortcuts and the ones that actually fire cannot drift. Only the six
// genuinely-global shortcuts live here; contextual keys (`d` duplicate, the
// My-Day / triage view keys) join with the Phase 2 `KeyboardScope` model —
// keeping them out avoids mis-documenting a node-only key as "anywhere".

/// A global shortcut's effect. The UI maps each to a closure in `layout.rs`;
/// this crate only names them so the mapping is exhaustive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlobalAction {
    QuickCapture,
    GoGraph,
    OpenPalette,
    TogglePalette,
    ToggleHelp,
    Escape,
}

/// One registry row: the action, the `KeyboardEvent.key` it matches, whether it
/// requires the Ctrl/Cmd modifier, and how it is shown + described in help.
pub struct GlobalShortcut {
    pub action: GlobalAction,
    /// The `KeyboardEvent.key()` value to match (ASCII-case-insensitive when `cmd`).
    pub key: &'static str,
    /// Requires Ctrl or Cmd (and is allowed even while an input is focused).
    pub cmd: bool,
    /// Human-facing key label for the help table (e.g. `"⌘K"`).
    pub display: &'static str,
    pub desc: &'static str,
}

/// The registry. Order is the help-table display order.
pub const GLOBAL: &[GlobalShortcut] = &[
    GlobalShortcut {
        action: GlobalAction::QuickCapture,
        key: "n",
        cmd: false,
        display: "n",
        desc: "Quick capture (Inbox)",
    },
    GlobalShortcut {
        action: GlobalAction::GoGraph,
        key: "g",
        cmd: false,
        display: "g",
        desc: "Graph view",
    },
    GlobalShortcut {
        action: GlobalAction::OpenPalette,
        key: "/",
        cmd: false,
        display: "/",
        desc: "Open command palette",
    },
    GlobalShortcut {
        action: GlobalAction::TogglePalette,
        key: "k",
        cmd: true,
        display: "⌘K",
        desc: "Open command palette (alt)",
    },
    GlobalShortcut {
        action: GlobalAction::ToggleHelp,
        key: "?",
        cmd: false,
        display: "?",
        desc: "Show this help",
    },
    GlobalShortcut {
        action: GlobalAction::Escape,
        key: "Escape",
        cmd: false,
        display: "Escape",
        desc: "Close modal / back",
    },
];

/// Resolve a keydown to a global action, or `None` if no global shortcut
/// applies. Pure so it is host-tested; the UI supplies the modifier flags and
/// the `editable` result from [`target_is_editable`].
///
/// Rules (preserving the pre-registry behavior): a `cmd` shortcut fires only on
/// exactly Ctrl/Cmd (no Shift/Alt) and works even while editing; a plain
/// shortcut fires only with no Ctrl/Cmd/Alt and only when not editing (Shift is
/// allowed, since e.g. `?` is Shift+`/`).
pub fn match_global(
    key: &str,
    ctrl_or_meta: bool,
    shift: bool,
    alt: bool,
    editable: bool,
) -> Option<GlobalAction> {
    for s in GLOBAL {
        if s.cmd {
            if ctrl_or_meta && !shift && !alt && key.eq_ignore_ascii_case(s.key) {
                return Some(s.action);
            }
        } else if !ctrl_or_meta && !alt && !editable && key == s.key {
            return Some(s.action);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::target_is_editable;

    #[test]
    fn editable_form_elements_are_flagged() {
        for tag in ["INPUT", "TEXTAREA", "SELECT", "BUTTON"] {
            assert!(target_is_editable(tag, None), "{tag} should be editable");
        }
    }

    #[test]
    fn non_editable_elements_are_not_flagged() {
        for tag in ["DIV", "SPAN", "A", "MAIN", "G", "CIRCLE"] {
            assert!(
                !target_is_editable(tag, None),
                "{tag} should NOT be editable"
            );
        }
    }

    #[test]
    fn contenteditable_attribute_semantics() {
        // Present-and-empty and "true" mean editable; "false" and absent do not.
        assert!(target_is_editable("DIV", Some("")));
        assert!(target_is_editable("DIV", Some("true")));
        assert!(target_is_editable("DIV", Some("plaintext-only")));
        assert!(!target_is_editable("DIV", Some("false")));
        assert!(!target_is_editable("DIV", None));
    }

    #[test]
    fn tag_wins_even_when_contenteditable_false() {
        // A form element is editable regardless of a stray contenteditable=false.
        assert!(target_is_editable("INPUT", Some("false")));
    }

    // ── Registry / match_global ──────────────────────────────────────────────
    use super::{GLOBAL, GlobalAction, match_global};

    #[test]
    fn plain_shortcuts_resolve_when_not_editing() {
        assert_eq!(
            match_global("n", false, false, false, false),
            Some(GlobalAction::QuickCapture)
        );
        assert_eq!(
            match_global("g", false, false, false, false),
            Some(GlobalAction::GoGraph)
        );
        assert_eq!(
            match_global("/", false, false, false, false),
            Some(GlobalAction::OpenPalette)
        );
        // `?` is Shift+`/`; shift is allowed for plain shortcuts.
        assert_eq!(
            match_global("?", false, true, false, false),
            Some(GlobalAction::ToggleHelp)
        );
        assert_eq!(
            match_global("Escape", false, false, false, false),
            Some(GlobalAction::Escape)
        );
    }

    #[test]
    fn plain_shortcuts_suppressed_while_editing() {
        for key in ["n", "g", "/", "?", "Escape"] {
            assert_eq!(
                match_global(key, false, false, false, true),
                None,
                "{key} must not fire while an input is focused"
            );
        }
    }

    #[test]
    fn cmd_k_fires_even_while_editing_but_not_with_extra_mods() {
        // ⌘K / Ctrl-K works mid-edit (a system-wide affordance)…
        assert_eq!(
            match_global("k", true, false, false, true),
            Some(GlobalAction::TogglePalette)
        );
        assert_eq!(
            match_global("K", true, false, false, false),
            Some(GlobalAction::TogglePalette)
        );
        // …but not with Shift or Alt also held.
        assert_eq!(match_global("k", true, true, false, false), None);
        assert_eq!(match_global("k", true, false, true, false), None);
    }

    #[test]
    fn plain_key_with_modifier_or_unknown_key_is_none() {
        // A plain shortcut key held with Ctrl/Cmd is NOT the plain shortcut.
        assert_eq!(match_global("g", true, false, false, false), None);
        assert_eq!(match_global("n", false, false, true, false), None); // Alt+n
        assert_eq!(match_global("z", false, false, false, false), None); // unmapped
    }

    /// Anti-drift guarantee: every registry row is reachable through
    /// `match_global`, and the help table (which renders the same `GLOBAL`)
    /// therefore cannot document a shortcut that doesn't dispatch.
    #[test]
    fn every_registry_row_round_trips() {
        for s in GLOBAL {
            let got = if s.cmd {
                match_global(s.key, true, false, false, false)
            } else {
                match_global(s.key, false, false, false, false)
            };
            assert_eq!(
                got,
                Some(s.action),
                "registry row {:?} ({}) is not reachable via match_global",
                s.action,
                s.display
            );
            assert!(!s.display.is_empty() && !s.desc.is_empty());
        }
    }
}
