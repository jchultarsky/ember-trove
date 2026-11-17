//! The node page's links (S7a, #342; ONTOLOGY §6, §14).
//!
//! **Connections** shows a node's links in named groups, each with the
//! meaning the ontology gives its type and direction: "Used in",
//! "Bibliography", "Descendants" and so on (`common::connections`).
//! **Unlinked Mentions** finds nodes whose text names this one without
//! linking to it.
//!
//! Both read the node's links, and the page reads them once
//! ([`NodeLinks`]): two sections asking for the same answer was a second
//! round trip.
//!
//! Nothing here decides who sees what. The answer holds only links whose
//! two ends the reader can view, and each row says whether this reader
//! may remove it (`may_remove`, the delete route's own rule), so no
//! control is offered that the server would refuse.

use std::collections::HashSet;

use common::connections::{Connection, Group, grouped};
use common::edge::{CreateEdgeRequest, EdgeType, EdgeWithTitles};
use common::id::NodeId;
use common::node::NodeType;
use common::ontology::allowed_homes;
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant, icon_button_class};
use crate::components::load_error::LoadError;
use crate::components::node_meta::{type_icon, type_label};
use crate::components::node_section::{NodeSection, SectionShell};
use crate::components::toast::{ToastLevel, push_toast};
use crate::error::UiError;

type LinksResult = Result<Vec<EdgeWithTitles>, UiError>;

/// The node's links, read once for the page and shared by the sections
/// that show them. Nothing is read until a section that wants them opens.
#[derive(Clone, Copy)]
pub struct NodeLinks {
    wanted: RwSignal<bool>,
    /// The page's home-and-links counter (`NodeView`'s `home_refresh`):
    /// the header's home chip reads it too.
    refresh: RwSignal<u32>,
    read: LocalResource<Option<LinksResult>>,
}

impl NodeLinks {
    pub fn new(node_id: NodeId, refresh: RwSignal<u32>) -> Self {
        let wanted = RwSignal::new(false);
        let read = LocalResource::new(move || {
            let _ = refresh.get();
            let wanted = wanted.get();
            async move {
                if wanted {
                    Some(crate::api::fetch_edges_for_node(node_id).await)
                } else {
                    None
                }
            }
        });
        Self {
            wanted,
            refresh,
            read,
        }
    }

    /// The links are wanted from the first time `open` is true.
    pub fn wanted_while(self, open: RwSignal<bool>) {
        Effect::new(move |_| {
            // Set once: setting it again would read the links again.
            if open.get() && !self.wanted.get_untracked() {
                self.wanted.set(true);
            }
        });
    }

    /// The answer; `None` while nobody wants it or it is on its way.
    pub fn get(self) -> Option<LinksResult> {
        self.read.get().flatten()
    }

    /// Read again. `try_`: this is called after an `.await`, when the
    /// page may be gone.
    pub fn reload(self) {
        let _ = self.refresh.try_update(|n| *n += 1);
    }
}

/// A group longer than this shows its first rows and a "Show N more…".
/// Rows that are not live sort last, so they are the ones folded away.
const FOLD_AT: usize = 12;

/// How many of a group's `total` rows show before "Show N more…". A
/// group only a little over the limit is shown whole: folding two rows
/// behind a click saves nothing.
fn rows_before_fold(total: usize) -> usize {
    if total > FOLD_AT + 2 { FOLD_AT } else { total }
}

/// The groups the list shows on a node of `kind`. The Home group is the
/// `HomeRow` above the list, which also names a home the reader can't
/// see; listing it again would show it twice. A kind that can't have a
/// home (a Reference) has no such row, so a home it was given before the
/// rule still shows here, where it can be removed.
fn listed(kind: &NodeType, groups: Vec<(Group, Vec<Connection>)>) -> Vec<(Group, Vec<Connection>)> {
    let home_row_shows_it = !allowed_homes(kind).is_empty();
    groups
        .into_iter()
        .filter(|(group, _)| !(home_row_shows_it && *group == Group::Home))
        .collect()
}

/// The nodes that already link to `node`: the source of every link into
/// it, and the other end of a `related_to`, which has no direction. A
/// mention in one of these is not an unlinked one.
fn linked_here(node: NodeId, edges: &[EdgeWithTitles]) -> HashSet<NodeId> {
    edges
        .iter()
        .filter_map(|e| {
            if e.target_id == node {
                Some(e.source_id)
            } else if e.source_id == node && e.edge_type.is_symmetric() {
                Some(e.target_id)
            } else {
                None
            }
        })
        .collect()
}

/// The type `target` is already linked with from `node`, when it is the
/// one being added (#129): the form says so instead of the server's 409.
fn already_linked_as(
    node: NodeId,
    target: NodeId,
    edge_type: &str,
    edges: &[EdgeWithTitles],
) -> Option<&'static str> {
    edges
        .iter()
        .find(|e| {
            let forward = e.source_id == node && e.target_id == target;
            // `related_to` has no direction (#226): the reverse is the
            // same link.
            let reverse =
                e.edge_type.is_symmetric() && e.source_id == target && e.target_id == node;
            (forward || reverse) && e.edge_type.as_str() == edge_type
        })
        .map(|e| e.edge_type.label())
}

/// A node's links in their groups, with its home first (#228) and a form
/// to add one.
#[component]
pub fn ConnectionsPanel(
    node_id: NodeId,
    node_type: NodeType,
    can_edit: bool,
    links: NodeLinks,
) -> impl IntoView {
    let open = RwSignal::new(false);
    links.wanted_while(open);
    let show_add = RwSignal::new(false);
    // Which long groups are shown whole. Kept here, not in the list: the
    // list is rebuilt after every change, and a group opened to remove a
    // link from it must not fold again.
    let unfolded = RwSignal::new(HashSet::<Group>::new());
    // The edge target is a committed node, never typed text (#126): the
    // old text field doubled as the UUID slot, so typing a title and
    // pressing ✓ failed with "Invalid target node UUID".
    let edge_target = RwSignal::new(None::<(NodeId, String)>);
    let confirm_ref = NodeRef::<leptos::html::Button>::new();
    // After a commit, focus ✓ so Enter creates the edge. Deferred: when the
    // picker commits, ✓ is still disabled until this tick renders, and a
    // disabled button refuses focus.
    let focus_confirm = RwSignal::new(false);
    Effect::new(move |_| {
        if focus_confirm.get() && edge_target.with(|t| t.is_some()) {
            focus_confirm.set(false);
            if let Some(el) = confirm_ref.get_untracked() {
                set_timeout(
                    move || {
                        let _ = el.focus();
                    },
                    std::time::Duration::ZERO,
                );
            }
        }
    });
    let edge_type_input = RwSignal::new("references".to_string());
    let label_input = RwSignal::new(String::new());
    let error_msg = RwSignal::new(Option::<String>::None);

    // The committed target is already linked with the selected type (#129):
    // say so and keep ✓ disabled, instead of letting the server answer 409.
    let already_linked = Signal::derive(move || {
        let (target, _) = edge_target.get()?;
        let et = edge_type_input.get();
        let edges = links.get()?.ok()?;
        already_linked_as(node_id, target, &et, &edges)
    });

    let on_add_edge = move |_| {
        // ✓ is disabled until a node is committed; this is the backstop.
        let Some((target_id, _)) = edge_target.get_untracked() else {
            return;
        };
        if already_linked.get_untracked().is_some() {
            return;
        }
        let et = edge_type_input.get_untracked();
        let label = label_input.get_untracked().trim().to_string();
        error_msg.set(None);

        let edge_type = EdgeType::parse(&et).unwrap_or(EdgeType::References);

        let req = CreateEdgeRequest {
            source_id: node_id,
            target_id,
            edge_type,
            label: if label.is_empty() { None } else { Some(label) },
        };

        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::create_edge(&req).await {
                Ok(_) => {
                    let _ = show_add.try_set(false);
                    let _ = edge_target.try_set(None);
                    let _ = label_input.try_set(String::new());
                    links.reload();
                }
                // A 409 ("already linked as …", e.g. another tab got there
                // first) and a 422 (a rule of the ontology) read as the
                // message itself, not "server error 409".
                Err(UiError::Api { message, .. }) => {
                    let mut msg = message;
                    if let Some(first) = msg.get(..1) {
                        msg.replace_range(..1, &first.to_uppercase());
                    }
                    let _ = error_msg.try_set(Some(msg));
                }
                Err(e) => {
                    let _ = error_msg.try_set(Some(format!("{e}")));
                }
            }
        });
    };

    let section_count = Signal::derive(move || links.get()?.ok().map(|edges| edges.len()));
    let section_actions: ChildrenFn = std::sync::Arc::new(move || {
        view! {
            {move || open.get().then(|| view! {
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                            dark:hover:text-stone-300 hover:bg-stone-100
                            dark:hover:bg-stone-800 transition-colors"
                        on:click=move |_| show_add.update(|v| *v = !*v)
                        title=move || if show_add.get() { "Cancel" } else { "Add Edge" }
                    >
                        <span aria-hidden="true" class="material-symbols-outlined" style="font-size: 16px;">
                            {move || if show_add.get() { "close" } else { "add_link" }}
                        </span>
                    </button>
                })}
        }
        .into_any()
    });
    let kind = StoredValue::new(node_type.clone());
    view! {
        <SectionShell section=NodeSection::Connections open=open count=section_count actions=section_actions>
            <crate::components::node_home::HomeRow
                node_id=node_id
                node_type=node_type.clone()
                can_edit=can_edit
                open=open
                refresh=links.refresh
            />

            // Add edge form
            {move || show_add.get().then(|| view! {
                <div class="mb-4 p-3 bg-stone-50 dark:bg-stone-900 rounded-lg space-y-2">
                    <div class="flex gap-2">
                        <div class="flex-1 min-w-0">
                            <crate::components::node_picker::NodePicker
                                selected=edge_target
                                placeholder="Search for a node…"
                                exclude=node_id
                                // Committed: Enter again (or a click) on ✓
                                // creates the edge; type and label can
                                // still be set first.
                                on_commit=Callback::new(move |()| focus_confirm.set(true))
                            />
                        </div>
                        <select
                            class="px-2 py-1 text-xs rounded-lg border border-stone-300 dark:border-stone-600
                                bg-stone-50 dark:bg-stone-800 text-stone-700 dark:text-stone-300
                                focus:outline-none"
                            aria-label="Link type"
                            prop:value=move || edge_type_input.get()
                            on:change=move |ev| edge_type_input.set(event_target_value(&ev))
                        >
                            <crate::components::node_meta::EdgeTypeOptions value=edge_type_input />
                        </select>
                    </div>
                    <div class="flex gap-2">
                        <input
                            type="text"
                            class="flex-1 px-2 py-1 text-xs rounded-lg border border-stone-300 dark:border-stone-600
                                bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none
                                focus:ring-1 focus:ring-amber-500"
                            placeholder="Label (optional)\u{2026}"
                            aria-label="Label (optional)"
                            prop:value=move || label_input.get()
                            on:input=move |ev| label_input.set(event_target_value(&ev))
                        />
                        <button
                            node_ref=confirm_ref
                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                dark:hover:text-stone-300 hover:bg-stone-100
                                dark:hover:bg-stone-800 transition-colors
                                disabled:opacity-40 disabled:cursor-not-allowed"
                            on:click=on_add_edge
                            disabled=move || {
                                edge_target.with(|t| t.is_none()) || already_linked.get().is_some()
                            }
                            title="Add edge"
                            aria-label="Add edge"
                        >
                            <span class="material-symbols-outlined" style="font-size: 16px;">"check"</span>
                        </button>
                    </div>
                    {move || already_linked.get().map(|label| view! {
                        <div class="text-xs text-amber-700 dark:text-amber-400" data-testid="edge-already-linked">
                            {format!("Already linked as {label}")}
                        </div>
                    })}
                    {move || error_msg.get().map(|msg| view! {
                        <div role="alert" class="text-xs text-red-500">{msg}</div>
                    })}
                </div>
            })}

            // The links, in their groups
            <Suspense fallback=|| view! {
                <div class="text-xs text-stone-400">"Loading connections\u{2026}"</div>
            }>
                {move || links.get().map(|result| match result {
                    Ok(edges) if edges.is_empty() => view! {
                        <div class="flex flex-col items-center gap-2 py-6" data-testid="connections-empty">
                            <span
                                class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                style="font-size: 32px;"
                                aria-hidden="true"
                            >
                                "hub"
                            </span>
                            <p class="text-xs text-stone-400 dark:text-stone-600">
                                "No connections yet."
                            </p>
                        </div>
                    }.into_any(),
                    Ok(edges) => {
                        let kind = kind.get_value();
                        let groups = listed(&kind, grouped(node_id, edges));
                        view! {
                            <div class="space-y-4" data-testid="connections-groups">
                                {groups.into_iter().map(|(group, rows)| {
                                    group_view(group, group.heading(&kind), rows, unfolded, links)
                                }).collect_view()}
                            </div>
                        }.into_any()
                    }
                    // Never the empty state (#281, 1.7): "No connections
                    // yet" after a failed read looks like lost links.
                    Err(_) => view! {
                        <LoadError what="this node\u{2019}s connections"
                            on_retry=Callback::new(move |()| links.reload()) />
                    }.into_any(),
                })}
            </Suspense>
        </SectionShell>
    }
}

/// One group: its heading, its rows, and "Show N more…" when it is long.
fn group_view(
    group: Group,
    heading: &'static str,
    rows: Vec<Connection>,
    unfolded: RwSignal<HashSet<Group>>,
    links: NodeLinks,
) -> impl IntoView {
    let total = rows.len();
    let first = rows_before_fold(total);
    let mut head = rows;
    let tail = head.split_off(first);
    let more = tail.len();
    let tail = StoredValue::new(tail);
    let whole = move || unfolded.with(|u| u.contains(&group));
    let heading_id = format!("connections-{}-heading", group.slug());
    let labelled_by = heading_id.clone();
    view! {
        <div role="group" aria-labelledby=labelled_by
             data-testid=format!("connections-{}", group.slug())>
            // Not an `h2`: those are the page's sections (the outline).
            <h3 id=heading_id
                class="mb-1 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide
                       text-stone-500 dark:text-stone-400">
                {heading}
                <span class="font-normal text-stone-400 dark:text-stone-500">{total}</span>
            </h3>
            <ul class="space-y-0.5">
                {head.into_iter().map(|row| row_view(row, links)).collect_view()}
                {move || whole().then(|| {
                    tail.get_value().into_iter().map(|row| row_view(row, links)).collect_view()
                })}
            </ul>
            {(more > 0).then(|| view! {
                // A word, as "Show 3 more…" is elsewhere (leptos.md).
                <button type="button"
                    class="mt-1 px-2 text-xs text-stone-500 dark:text-stone-400 underline underline-offset-2
                           hover:text-amber-700 dark:hover:text-amber-400"
                    aria-expanded=move || if whole() { "true" } else { "false" }
                    on:click=move |_| unfolded.update(|u| {
                        if !u.remove(&group) {
                            u.insert(group);
                        }
                    })>
                    {move || if whole() {
                        "Show fewer".to_string()
                    } else {
                        format!("Show {more} more\u{2026}")
                    }}
                </button>
            })}
        </div>
    }
}

/// One link: the node at its other end (kind, title, standing, the
/// link's label), and a remove control when this reader may remove it.
fn row_view(row: Connection, links: NodeLinks) -> impl IntoView {
    let edge_id = row.edge_id;
    let standing = row.standing();
    let live = row.is_live();
    let kind = row.other.as_ref().map(|o| o.node_type.as_str().to_string());
    // An answer cached before S7a has no kind: a plain link glyph.
    let (icon, kind_label) = match kind.as_deref() {
        Some(k) => (type_icon(k), type_label(k)),
        None => ("link", "Node"),
    };
    let title = row.other_title.clone();
    let title_class = if live {
        "font-medium truncate"
    } else {
        // Out of use: shown, and quieter than the live rows above it.
        "truncate text-stone-400 dark:text-stone-500"
    };
    let busy = RwSignal::new(false);
    let remove = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::delete_edge(edge_id).await {
                Ok(()) => links.reload(),
                Err(e) => push_toast(
                    ToastLevel::Error,
                    format!("Couldn\u{2019}t remove the link: {e}"),
                ),
            }
            let _ = busy.try_set(false);
        });
    });
    view! {
        <li class="flex items-center justify-between gap-2 rounded px-2
                   hover:bg-stone-50 dark:hover:bg-stone-800/50"
            data-testid="connection-row">
            // A real link: the keyboard reaches it and the router follows it.
            <a href=format!("/nodes/{}", row.other_id)
                class="flex min-w-0 flex-1 items-center gap-2 py-1.5 text-xs text-stone-600 dark:text-stone-400
                       hover:text-amber-700 dark:hover:text-amber-400">
                <span class="material-symbols-outlined shrink-0 text-stone-400 dark:text-stone-500"
                      style="font-size: 14px;" aria-hidden="true" title=kind_label>
                    {icon}
                </span>
                <span class="sr-only">{format!("{kind_label}: ")}</span>
                <span class=title_class>{title.clone()}</span>
                {standing.map(|word| view! {
                    <span class="shrink-0 rounded bg-stone-100 px-1.5 py-0.5 text-[10px] font-medium
                                 text-stone-500 dark:bg-stone-800 dark:text-stone-400"
                          data-testid="connection-standing">
                        {word}
                    </span>
                })}
                {row.label.clone().map(|label| view! {
                    <span class="shrink-0 italic text-stone-400 dark:text-stone-500">
                        {format!("({label})")}
                    </span>
                })}
            </a>
            // Offered only where the delete would pass (`may_remove`).
            // Always visible and muted, never hover-only (#281).
            {row.may_remove.then(|| view! {
                <IconButton icon="link_off" label=format!("Remove the link to {title}")
                    variant=IconButtonVariant::Danger
                    disabled=Signal::derive(move || busy.get())
                    on_click=remove />
            })}
        </li>
    }
}

/// A full-text hit on this node's title in another node's text.
#[derive(Clone)]
struct Mention {
    id: NodeId,
    title: String,
}

/// Nodes whose text names this node's title without linking to it, each
/// with a one-click "turn the mention into a `[[link]]`". The nodes that
/// do link here are in Connections ("Mentioned in" and the other groups).
#[component]
pub fn UnlinkedMentionsPanel(node_id: NodeId, title: String, links: NodeLinks) -> impl IntoView {
    let open = RwSignal::new(false);
    links.wanted_while(open);
    let title_sv = StoredValue::new(title);

    // Full-text hits on this node's title, minus the node itself and
    // anything that already links here. Only node-body matches: note and
    // task hits can't be rewritten into wikilinks. `Ok(None)` while the
    // links are still on their way: which nodes already link here isn't
    // known yet, so no list is shown that might name one of them.
    let mentions = LocalResource::new(move || {
        let is_open = open.get();
        let linked = is_open
            .then(|| links.get())
            .flatten()
            .map(|r| r.map(|edges| linked_here(node_id, &edges)));
        let title = title_sv.get_value();
        async move {
            let Some(linked) = linked else {
                return Ok::<_, UiError>(None);
            };
            let linked = linked?;
            if title.trim().is_empty() {
                return Ok(Some(Vec::new()));
            }
            let resp = crate::api::search_nodes(
                &title,
                false,
                None,
                None,
                &[],
                "or",
                Some("relevance"),
                None,
                None,
                1,
                25,
            )
            .await?;
            Ok(Some(
                resp.results
                    .into_iter()
                    .filter(|r| {
                        r.node_id != node_id
                            && !linked.contains(&r.node_id)
                            && r.match_source.as_deref() == Some("node")
                    })
                    .take(10)
                    .map(|r| Mention {
                        id: r.node_id,
                        title: r.title,
                    })
                    .collect::<Vec<_>>(),
            ))
        }
    });

    // Turn the first plain-text mention in `mention_id`'s body into a
    // wikilink to this node. The save runs the server's wikilink sync, so
    // after the reload the node is under "Mentioned in" in Connections.
    let linking = RwSignal::new(false);
    let do_link = move |mention_id: NodeId| {
        if linking.get_untracked() {
            return;
        }
        linking.set(true);
        let title = title_sv.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                let n = crate::api::fetch_node(mention_id).await?;
                let body = n.body.unwrap_or_default();
                match common::markdown::link_first_mention(&body, &title) {
                    Some(new_body) => {
                        let req = common::node::UpdateNodeRequest {
                            title: None,
                            body: Some(new_body),
                            metadata: None,
                            status: None,
                            project_status: None,
                            // The text just read: a save from another
                            // tab in between is not written over.
                            expected_updated_at: Some(n.updated_at),
                        };
                        crate::api::update_node(mention_id, &req).await.map(Some)
                    }
                    None => Ok(None),
                }
            }
            .await;
            let _ = linking.try_set(false);
            match result {
                Ok(Some(_)) => {
                    push_toast(ToastLevel::Success, "Mention linked.");
                    links.reload();
                }
                Ok(None) => push_toast(
                    ToastLevel::Info,
                    "No literal mention found \u{2014} the match may be a different word form.",
                ),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn\u{2019}t link: {e}")),
            }
        });
    };

    let section_count = Signal::derive(move || {
        mentions.with(|r| r.as_ref()?.as_ref().ok()?.as_ref().map(Vec::len))
    });
    view! {
        <SectionShell section=NodeSection::UnlinkedMentions open=open count=section_count>
            <Suspense fallback=|| view! {
                <div class="text-xs text-stone-400">"Looking for mentions\u{2026}"</div>
            }>
                {move || mentions.get().map(|result| match result {
                    Ok(None) => view! {
                        <div class="text-xs text-stone-400">"Looking for mentions\u{2026}"</div>
                    }.into_any(),
                    Ok(Some(hits)) if hits.is_empty() => view! {
                        <div class="flex flex-col items-center gap-2 py-6" data-testid="mentions-empty">
                            <span
                                class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                style="font-size: 32px;"
                                aria-hidden="true"
                            >
                                "alternate_email"
                            </span>
                            <p class="text-xs text-stone-400 dark:text-stone-600">
                                "No other node names this one without linking to it."
                            </p>
                        </div>
                    }.into_any(),
                    Ok(Some(hits)) => view! {
                        <ul class="space-y-0.5">
                            {hits.into_iter().map(|hit| {
                                let hit_id = hit.id;
                                view! {
                                    <li class="flex items-center gap-1" data-testid="unlinked-mention">
                                        <a href=format!("/nodes/{hit_id}")
                                            class="flex min-w-0 flex-1 items-center gap-2 rounded px-2 py-1.5 text-xs
                                                   text-stone-600 dark:text-stone-400
                                                   hover:bg-stone-50 dark:hover:bg-stone-800/50
                                                   hover:text-amber-700 dark:hover:text-amber-400">
                                            <span class="material-symbols-outlined shrink-0 text-stone-400 dark:text-stone-600"
                                                style="font-size: 14px;" aria-hidden="true">"alternate_email"</span>
                                            <span class="font-medium truncate">{hit.title.clone()}</span>
                                        </a>
                                        <button
                                            type="button"
                                            aria-label=format!("Link the mention in {}", hit.title)
                                            class=icon_button_class(IconButtonVariant::Accent)
                                            title="Turn the first plain-text mention into a wikilink"
                                            disabled=move || linking.get()
                                            on:click=move |_| do_link(hit_id)
                                        >
                                            <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">"link"</span>
                                        </button>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                    Err(_) => view! {
                        <LoadError what="the mentions of this node"
                            on_retry=Callback::new(move |()| links.reload()) />
                    }.into_any(),
                })}
            </Suspense>
        </SectionShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::id::EdgeId;
    use uuid::Uuid;

    fn id(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    fn edge(n: u128, source: u128, target: u128, edge_type: EdgeType) -> EdgeWithTitles {
        EdgeWithTitles {
            id: EdgeId(Uuid::from_u128(0xe000 + n)),
            source_id: id(source),
            target_id: id(target),
            source_title: format!("node {source}"),
            target_title: format!("node {target}"),
            edge_type,
            label: None,
            created_at: chrono::DateTime::UNIX_EPOCH,
            source_node: None,
            target_node: None,
            may_remove: false,
        }
    }

    #[test]
    fn a_long_group_folds_and_one_only_just_over_does_not() {
        assert_eq!(rows_before_fold(0), 0);
        assert_eq!(rows_before_fold(FOLD_AT), FOLD_AT);
        // Two over: shown whole, nothing behind a click.
        assert_eq!(rows_before_fold(FOLD_AT + 2), FOLD_AT + 2);
        assert_eq!(rows_before_fold(FOLD_AT + 3), FOLD_AT);
        assert_eq!(rows_before_fold(41), FOLD_AT);
    }

    #[test]
    fn the_home_is_listed_only_where_no_home_row_shows_it() {
        let me = 1;
        let edges = vec![
            edge(1, 9, me, EdgeType::Contains),
            edge(2, 8, me, EdgeType::References),
        ];
        let groups = |kind: &NodeType| -> Vec<Group> {
            listed(kind, grouped(id(me), edges.clone()))
                .into_iter()
                .map(|(g, _)| g)
                .collect()
        };
        // A Project has a home row above the list.
        assert_eq!(groups(&NodeType::Project), vec![Group::UsedIn]);
        // A Reference has none (it is never contained), so a home from
        // before the rule still shows, where it can be removed.
        assert!(allowed_homes(&NodeType::Reference).is_empty());
        assert_eq!(
            groups(&NodeType::Reference),
            vec![Group::Home, Group::UsedIn]
        );
    }

    #[test]
    fn a_node_that_links_here_is_not_an_unlinked_mention() {
        let me = 1;
        let edges = vec![
            edge(1, 2, me, EdgeType::WikiLink),
            edge(2, 3, me, EdgeType::References),
            // Outgoing: 4 doesn't link here, this node links to it.
            edge(3, me, 4, EdgeType::References),
            // `related_to` has no direction: 5 is linked either way.
            edge(4, me, 5, EdgeType::RelatedTo),
            edge(5, 6, me, EdgeType::RelatedTo),
        ];
        let linked = linked_here(id(me), &edges);
        let expect: HashSet<NodeId> = [2, 3, 5, 6].into_iter().map(id).collect();
        assert_eq!(linked, expect);
    }

    #[test]
    fn the_add_form_knows_a_link_that_is_already_there() {
        let me = 1;
        let edges = vec![
            edge(1, me, 2, EdgeType::References),
            edge(2, 3, me, EdgeType::RelatedTo),
            edge(3, 4, me, EdgeType::References),
        ];
        let linked = |target: u128, t: &str| already_linked_as(id(me), id(target), t, &edges);
        assert_eq!(linked(2, "references"), Some(EdgeType::References.label()));
        // Another type between the same two is a different link.
        assert_eq!(linked(2, "related_to"), None);
        // `related_to` stored the other way round is the same link.
        assert_eq!(linked(3, "related_to"), Some(EdgeType::RelatedTo.label()));
        // A directed link the other way round is not.
        assert_eq!(linked(4, "references"), None);
    }
}
