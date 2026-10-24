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
    serde_json::from_str(&raw).unwrap_or_default()
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
