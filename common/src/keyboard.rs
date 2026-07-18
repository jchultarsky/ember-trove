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
}
