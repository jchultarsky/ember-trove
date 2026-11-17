//! Lifecycle-state picker for the node editor (#196).
//!
//! Replaces a bare `<select>` whose three options said nothing about what
//! they do. All three states stay visible as a radio group, and the chosen
//! one's meaning (`NodeStatus::description`) sits right under it, tied to
//! the group with `aria-describedby`. Each option also carries its own
//! description as a tooltip, so the others can be read before choosing.
//! Keyboard: WAI-ARIA APG radio group, as `PriorityPicker` — one tab stop
//! on the checked option, arrow keys move the selection.

use common::connections::{archive_notice, live_users};
use common::edge::EdgeWithTitles;
use common::id::NodeId;
use common::node::NodeStatus;
use leptos::prelude::*;

/// An unknown or empty wire value reads as Draft, the server default, so
/// there is always exactly one checked option and one tab stop.
fn parse_status(s: &str) -> NodeStatus {
    NodeStatus::parse(s).unwrap_or(NodeStatus::Draft)
}

/// What to say under the picker while Archived is chosen (S7a, ONTOLOGY
/// §6.3: "a warning, not a block"). `links` is the read of the node's
/// links: nodes in use that still draw on it as a source are named by
/// count, and a read that failed says so. It never reads as "none": that
/// would tell someone the node is free to retire when nobody checked.
fn archived_notice<E>(node: NodeId, links: &Result<Vec<EdgeWithTitles>, E>) -> Option<String> {
    match links {
        Ok(edges) => archive_notice(live_users(node, edges)),
        Err(_) => Some(
            "Couldn\u{2019}t check whether nodes in use still draw on this as a source."
                .to_string(),
        ),
    }
}

/// The notice under the picker for a node that exists: read when Archived
/// is chosen, and for a node opened already archived. Archiving is never
/// held back; the editor saves on its own, so this is worded to be true
/// before the save and after it.
#[component]
fn ArchivedNotice(node: NodeId, value: RwSignal<String>) -> impl IntoView {
    let archived = Memo::new(move |_| parse_status(&value.get()) == NodeStatus::Archived);
    let links = LocalResource::new(move || {
        let wanted = archived.get();
        async move {
            if wanted {
                Some(crate::api::fetch_edges_for_node(node).await)
            } else {
                None
            }
        }
    });
    view! {
        // Always rendered, so a screen reader hears the notice arrive.
        <span class="text-xs text-amber-700 dark:text-amber-400" role="status"
              data-testid="archive-notice">
            <Suspense fallback=|| ()>
                {move || {
                    links.get().flatten().and_then(|read| archived_notice(node, &read))
                }}
            </Suspense>
        </span>
    }
}

#[component]
pub fn StatusPicker(
    /// The status's wire string (`"draft"` / `"published"` / `"archived"`).
    value: RwSignal<String>,
    /// The node being edited, when it exists already: archiving it warns
    /// while nodes in use still draw on it (S7a). `None` for a new node.
    node: Option<NodeId>,
) -> impl IntoView {
    let refs = StoredValue::new(
        NodeStatus::ALL
            .iter()
            .map(|_| NodeRef::<leptos::html::Button>::new())
            .collect::<Vec<_>>(),
    );
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let down = match ev.key().as_str() {
            "ArrowRight" | "ArrowDown" => true,
            "ArrowLeft" | "ArrowUp" => false,
            _ => return,
        };
        ev.prevent_default();
        ev.stop_propagation();
        let current = parse_status(&value.get_untracked());
        let active = NodeStatus::ALL.iter().position(|s| *s == current);
        let Some(i) = common::picker::step(active, NodeStatus::ALL.len(), down) else {
            return;
        };
        if let Some(s) = NodeStatus::ALL.get(i) {
            value.set(s.as_str().to_string());
        }
        // Focus follows the selection (roving tab stop).
        if let Some(btn) = refs.with_value(|r| r.get(i).and_then(|n| n.get_untracked())) {
            let _ = btn.focus();
        }
    };
    view! {
        <div class="flex flex-col gap-0.5 min-w-0">
            <div
                role="radiogroup"
                aria-label="Lifecycle state"
                aria-describedby="status-picker-hint"
                class="inline-flex self-start items-center rounded-lg border border-stone-200 dark:border-stone-700 overflow-hidden"
                data-testid="status-picker"
                on:keydown=on_keydown
            >
                {NodeStatus::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let v = s.as_str();
                        let checked = move || parse_status(&value.get()).as_str() == v;
                        let node_ref = refs.with_value(|r| r.get(i).copied()).unwrap_or_default();
                        view! {
                            <button
                                type="button"
                                role="radio"
                                node_ref=node_ref
                                title=s.description()
                                aria-checked=move || if checked() { "true" } else { "false" }
                                tabindex=move || if checked() { "0" } else { "-1" }
                                class=move || {
                                    let base = "text-sm px-2.5 py-1 transition-colors cursor-pointer \
                                                focus:outline-none focus-visible:ring-1 focus-visible:ring-amber-500";
                                    if checked() {
                                        format!("{base} bg-stone-200 dark:bg-stone-700 \
                                                 text-stone-900 dark:text-stone-100 font-semibold")
                                    } else {
                                        format!("{base} text-stone-500 dark:text-stone-400 \
                                                 hover:bg-stone-100 dark:hover:bg-stone-800")
                                    }
                                }
                                on:click=move |_| value.set(v.to_string())
                            >
                                {s.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            <span
                id="status-picker-hint"
                class="text-xs text-stone-500 dark:text-stone-400"
                data-testid="status-picker-hint"
            >
                {move || parse_status(&value.get()).description()}
            </span>
            {node.map(|node| view! { <ArchivedNotice node=node value=value /> })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{archived_notice, parse_status};
    use common::edge::{EdgeEnd, EdgeType, EdgeWithTitles};
    use common::id::{EdgeId, NodeId};
    use common::node::{NodeStatus, NodeType};
    use uuid::Uuid;

    fn id(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    /// A `references` link from `source` to node 1, the node being archived.
    fn used_by(source: u128, status: NodeStatus) -> EdgeWithTitles {
        EdgeWithTitles {
            id: EdgeId(Uuid::from_u128(0xe000 + source)),
            source_id: id(source),
            target_id: id(1),
            source_title: format!("node {source}"),
            target_title: "the source".to_string(),
            edge_type: EdgeType::References,
            label: None,
            created_at: chrono::DateTime::UNIX_EPOCH,
            source_node: Some(EdgeEnd {
                node_type: NodeType::Article,
                status,
                project_state: None,
            }),
            target_node: None,
            may_remove: false,
        }
    }

    #[test]
    fn archiving_says_how_many_nodes_in_use_still_draw_on_it_or_that_it_cant_tell() {
        let read = |edges: Vec<EdgeWithTitles>| archived_notice::<()>(id(1), &Ok(edges));
        // None: nothing is said.
        assert_eq!(read(vec![]), None);
        // One that is itself archived is not in use.
        assert_eq!(read(vec![used_by(2, NodeStatus::Archived)]), None);
        let one = read(vec![
            used_by(2, NodeStatus::Published),
            used_by(3, NodeStatus::Archived),
        ]);
        assert_eq!(one, common::connections::archive_notice(1));
        assert!(one.is_some_and(|s| s.starts_with("1 node in use")));
        let two = read(vec![
            used_by(2, NodeStatus::Published),
            used_by(3, NodeStatus::Draft),
        ]);
        assert!(two.is_some_and(|s| s.starts_with("2 nodes in use")));
        // A failed read never reads as "none".
        let unknown = archived_notice(id(1), &Err(()));
        assert!(unknown.is_some_and(|s| s.starts_with("Couldn\u{2019}t check")));
    }

    #[test]
    fn unknown_values_read_as_draft() {
        assert_eq!(parse_status(""), NodeStatus::Draft);
        assert_eq!(parse_status("bogus"), NodeStatus::Draft);
        assert_eq!(parse_status("archived"), NodeStatus::Archived);
    }
}
