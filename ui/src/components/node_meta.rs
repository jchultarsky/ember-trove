// Shared helpers for rendering node-type and status metadata consistently
// across NodeList, NodeView, GraphView, and SearchView.

// ── Node type ─────────────────────────────────────────────────────────────────

pub fn type_icon(node_type: &str) -> &'static str {
    match node_type {
        "project" => "rocket_launch",
        "area" => "category",
        "resource" => "bookmarks",
        "reference" => "menu_book",
        _ => "description", // article (default)
    }
}

pub fn type_label(node_type: &str) -> &'static str {
    match node_type {
        "project" => "Project",
        "area" => "Area",
        "resource" => "Resource",
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
