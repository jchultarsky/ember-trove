// Shared helpers for rendering node-type and status metadata consistently
// across NodeList, NodeView, GraphView, and SearchView.

// ── Node type ─────────────────────────────────────────────────────────────────

pub fn type_icon(node_type: &str) -> &'static str {
    match node_type {
        "project" => "rocket_launch",
        "area" => "category",
        "reference" => "menu_book",
        _ => "description", // article (default)
    }
}

pub fn type_label(node_type: &str) -> &'static str {
    match node_type {
        "project" => "Project",
        "area" => "Area",
        "reference" => "Reference",
        _ => "Article",
    }
}

// ── Status ────────────────────────────────────────────────────────────────────

pub fn status_icon(status: &str) -> &'static str {
    match status {
        "published" => "check_circle",
        "archived" => "inventory_2",
        _ => "edit_note", // draft
    }
}

pub fn status_label(status: &str) -> &'static str {
    match status {
        "published" => "Published",
        "archived" => "Archived",
        _ => "Draft",
    }
}

/// Returns the hex colour code for the given status (no CSS property wrapper).
///
/// Use this for SVG `fill:` and similar raw-colour contexts.
pub fn status_color_hex(status: &str) -> &'static str {
    match status {
        "published" => "#16a34a", // green-600
        "archived" => "#d97706",  // amber-600
        _ => "#a8a29e",           // stone-400 (draft = neutral)
    }
}

/// Returns an inline CSS `color:` declaration for the given status string.
///
/// Suitable for use in HTML `style` attributes.
pub fn status_color(status: &str) -> &'static str {
    match status {
        "published" => "color: #16a34a;",
        "archived" => "color: #d97706;",
        _ => "color: #a8a29e;",
    }
}

/// The `<option>`s of a connection-type `<select>`: the user-selectable
/// types with one label each (`EdgeType::USER_SELECTABLE`,
/// `option_label`). The Connections form and the graph's New Edge dialog
/// had hand-written lists that already disagreed (#133). Each option binds
/// its own `selected`, like `RecurrenceOptions`.
#[leptos::component]
pub fn EdgeTypeOptions(value: leptos::prelude::RwSignal<String>) -> impl leptos::IntoView {
    use leptos::prelude::*;
    common::edge::EdgeType::USER_SELECTABLE
        .iter()
        .map(|t| {
            let v = t.as_str();
            view! {
                <option value=v selected=move || value.get() == v>{t.option_label()}</option>
            }
        })
        .collect_view()
}

#[cfg(test)]
mod icon_font_tests {
    use super::{status_icon, type_icon};

    /// The names bundled in the self-hosted icon font, written by
    /// `scripts/refresh-icon-font.sh`.
    const BUNDLED: &str = include_str!("../../icon-names.txt");

    #[test]
    fn every_type_and_status_icon_is_in_the_font_subset() {
        // The font holds only the glyphs the script finds in the sources. A
        // name it misses renders as raw text: the Draft icon showed
        // "edit_note" on every list (found 2026-11-10), because this module
        // has no markup for the scan to notice. Run the script after adding
        // an icon; this test fails until you do.
        let bundled: Vec<&str> = BUNDLED.lines().collect();
        let used = ["article", "project", "area", "reference"]
            .map(type_icon)
            .into_iter()
            .chain(["draft", "published", "archived"].map(status_icon));
        for name in used {
            assert!(
                bundled.contains(&name),
                "icon `{name}` is not in ui/icon-names.txt: run scripts/refresh-icon-font.sh"
            );
        }
    }
}
