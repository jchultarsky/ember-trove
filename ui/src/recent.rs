//! Recent-node tracking via `localStorage`.
//!
//! Stores up to [`MAX_ENTRIES`] `RecentEntry` records under the key
//! `"ember_trove_recent"`.  All operations are infallible — a missing
//! `localStorage` (e.g. during SSR or private-browsing restrictions) is
//! silently treated as an empty list.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const LS_KEY: &str = "ember_trove_recent";
const MAX_ENTRIES: usize = 10;

/// A single entry in the recent-nodes list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentEntry {
    pub id: Uuid,
    pub title: String,
    /// Material Symbols icon name matching the node type.
    pub icon: String,
}

/// Read the current recent-nodes list from `localStorage`.
/// Returns an empty `Vec` on any error.
pub fn read_recent() -> Vec<RecentEntry> {
    let Some(window) = web_sys::window() else {
        return vec![];
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return vec![];
    };
    let Ok(Some(raw)) = storage.get_item(LS_KEY) else {
        return vec![];
    };
    let entries: Vec<RecentEntry> = serde_json::from_str(&raw).unwrap_or_default();
    entries.into_iter().map(RecentEntry::current).collect()
}

/// The retired Resource kind's glyph (#222).
const RESOURCE_ICON: &str = "bookmarks";

impl RecentEntry {
    /// An entry stored before 3.0 may carry the retired Resource glyph; the
    /// node is a Reference now, so it gets that icon.
    fn current(self) -> Self {
        if self.icon == RESOURCE_ICON {
            let icon = crate::components::node_meta::type_icon("reference").to_string();
            Self { icon, ..self }
        } else {
            self
        }
    }
}

/// Prepend a node to the recent list, dedup by id, cap at [`MAX_ENTRIES`].
/// Silently no-ops on any storage error.
pub fn push_recent(id: Uuid, title: String, node_type: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };

    let icon = crate::components::node_meta::type_icon(node_type).to_string();
    let entry = RecentEntry { id, title, icon };

    let mut entries = read_recent();
    entries.retain(|e| e.id != entry.id);
    entries.insert(0, entry);
    entries.truncate(MAX_ENTRIES);

    if let Ok(json) = serde_json::to_string(&entries) {
        let _ = storage.set_item(LS_KEY, &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pre_3_0_resource_entry_shows_the_reference_icon() {
        let e = |icon: &str| RecentEntry {
            id: Uuid::nil(),
            title: "t".into(),
            icon: icon.into(),
        };
        assert_eq!(e("bookmarks").current().icon, "menu_book");
        assert_eq!(e("rocket_launch").current(), e("rocket_launch"));
    }
}
