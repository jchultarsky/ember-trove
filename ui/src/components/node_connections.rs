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

use common::blockers::{self, NodeBlockers};
use common::connections::{Connection, Group, grouped};
use common::edge::{CreateEdgeRequest, EdgeType, EdgeWithTitles};
use common::id::NodeId;
use common::node::{NodeStatus, NodeType};
use common::ontology::allowed_homes;
use common::permission::holds_editor;
use common::project_state::ProjectState;
use leptos::prelude::*;

use crate::auth::{AuthStatus, use_auth_state};
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
/// see; listing it again would show it twice. It stays in the list in
/// two cases:
/// - a kind that can't have a home (a Reference) has no such row, so a
///   home it was given before the rule shows here, where it can be
///   removed;
/// - the row's buttons are the owner's (`can_edit`). Someone else who
///   may remove the home's link (a shared editor of the child: a home
///   never traps a child) is offered that here, as before S7a.
fn listed(
    kind: &NodeType,
    can_edit: bool,
    groups: Vec<(Group, Vec<Connection>)>,
) -> Vec<(Group, Vec<Connection>)> {
    let home_row_shows_it = !allowed_homes(kind).is_empty();
    groups
        .into_iter()
        .filter(|(group, rows)| {
            if *group != Group::Home || !home_row_shows_it {
                return true;
            }
            !can_edit && rows.iter().any(|row| row.may_remove)
        })
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

/// How many of the node's own dependencies are not met (S7b): from the
/// links once the page has read them (so it is right after one is added
/// or removed here), and until then the count the node's own answer
/// carried. The two are one rule (`blockers::direct`; the server's count
/// is held equal to it), so the chip can't say more than the list.
fn unmet_dependencies(node: NodeId, served: u32, links: Option<&LinksResult>) -> u32 {
    match links {
        Some(Ok(edges)) => blockers::direct(node, edges),
        // Not read yet, or the read failed: what the node's answer said.
        _ => served,
    }
}

/// "Blocked" in the page header (S7b, ONTOLOGY §6.4): the node still
/// waits itself and one of its own dependencies is not met, among those
/// this reader can see. It opens Connections, where "Depends on" says
/// what. Surfaced, never enforced: nothing else about the node changes.
#[component]
pub fn NodeBlockedChip(
    node_id: NodeId,
    node_type: NodeType,
    status: NodeStatus,
    /// The page's own state signal: the chip follows a state declared
    /// here (a Project made complete no longer says "Blocked").
    project_state: RwSignal<Option<ProjectState>>,
    /// `ViewedNode::blocked_by`: the count the node's answer carried.
    served: u32,
    links: NodeLinks,
) -> impl IntoView {
    let sections = use_context::<crate::components::node_section::NodeSections>();
    // Derived signals, not memos: a `Memo` that reads a `LocalResource`
    // doesn't run again when the read lands (the chip kept saying "2"
    // after a dependency was removed and the list already showed one).
    let unmet = Signal::derive(move || unmet_dependencies(node_id, served, links.get().as_ref()));
    let shown = Signal::derive(move || {
        blockers::is_blocked(&node_type, &status, project_state.get(), unmet.get())
    });
    move || {
        shown.get().then(|| {
            let label = blockers::blocked_label(unmet.get());
            view! {
                <button
                    type="button"
                    class=format!(
                        "{} text-xs py-0.5 hover:text-stone-900 dark:hover:text-stone-100",
                        crate::components::task_row_scaffold::BLOCKED_CHIP
                    )
                    title=label.clone()
                    aria-label=format!("Blocked. {label}. Show what it waits for")
                    data-testid="node-blocked"
                    on:click=move |_| {
                        if let Some(sections) = sections {
                            sections.jump.set(Some(NodeSection::Connections));
                        }
                    }
                >
                    <span class="material-symbols-outlined" style="font-size:12px;"
                        aria-hidden="true">"block"</span>
                    "Blocked"
                </button>
            }
        })
    }
}

/// A node's links in their groups, with its home first (#228) and a form
/// to add one.
#[component]
pub fn ConnectionsPanel(
    node_id: NodeId,
    node_type: NodeType,
    can_edit: bool,
    /// The node's own standing (S7b): only a node that still waits has a
    /// chain behind its dependencies.
    status: NodeStatus,
    project_state: RwSignal<Option<ProjectState>>,
    links: NodeLinks,
) -> impl IntoView {
    let open = RwSignal::new(false);
    links.wanted_while(open);
    // What the unmet dependencies wait for in turn (S7b). Read only while
    // the section is open, the node still waits, and its links say one
    // of its dependencies is not met; read again when the links are.
    let chain_kind = node_type.clone();
    let chain = LocalResource::new(move || {
        let waits = blockers::still_waits(&chain_kind, &status, project_state.get());
        let wanted = open.get()
            && waits
            && links
                .get()
                .and_then(Result::ok)
                .is_some_and(|edges| blockers::direct(node_id, &edges) > 0);
        async move {
            if wanted {
                Some(crate::api::fetch_blockers(node_id).await)
            } else {
                None
            }
        }
    });
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

    // Who may add a link here: someone who may write on this node (the
    // rule of `POST /edges` for the link's source). The owner and an
    // admin are known from the page (`can_edit`). Anyone else is asked
    // for their own grant, once the section is opened: a shared editor
    // is offered the form, a viewer is not (they used to be, and got a
    // refusal). While that isn't known, or if it can't be read, nothing
    // is offered: the API decides in any case.
    let reader = StoredValue::new(match use_auth_state().get_untracked() {
        AuthStatus::Authenticated(user) => Some(user.sub),
        _ => None,
    });
    let own_grants = LocalResource::new(move || {
        let wanted = !can_edit && links.wanted.get();
        async move {
            if wanted {
                Some(crate::api::list_permissions(node_id).await)
            } else {
                None
            }
        }
    });
    let may_add = Signal::derive(move || {
        if can_edit {
            return true;
        }
        let Some(Ok(grants)) = own_grants.get().flatten() else {
            return false;
        };
        reader.with_value(|sub| sub.as_deref().is_some_and(|sub| holds_editor(sub, &grants)))
    });

    let section_count = Signal::derive(move || links.get()?.ok().map(|edges| edges.len()));
    let section_actions: ChildrenFn = std::sync::Arc::new(move || {
        view! {
            {move || (open.get() && may_add.get()).then(|| view! {
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
            {move || (show_add.get() && may_add.get()).then(|| view! {
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
                        <div role="status" class="text-xs text-amber-700 dark:text-amber-400" data-testid="edge-already-linked">
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
                        let groups = listed(&kind, can_edit, grouped(node_id, edges));
                        view! {
                            <div class="space-y-4" data-testid="connections-groups">
                                {groups.into_iter().map(|(group, rows)| view! {
                                    {group_view(group, group.heading(&kind), rows, unfolded, links)}
                                    // Under "Depends on": what those wait for in turn.
                                    {(group == Group::DependsOn).then(|| chain_view(chain))}
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

/// "Further down the chain" (S7b, §6.4): what the node's unmet
/// dependencies wait for in turn, each with the dependency of this node
/// it comes through. Not this node's links: no remove control. Its own
/// boundary, so the list above doesn't blink while the chain loads.
fn chain_view(chain: LocalResource<Option<Result<NodeBlockers, UiError>>>) -> impl IntoView {
    view! {
        <Suspense fallback=|| ()>
            {move || chain.get().flatten().map(|read| match read {
                Ok(found) if found.also.is_empty() && !found.truncated => ().into_any(),
                Ok(found) => view! {
                    <div role="group" aria-labelledby="connections-chain-heading"
                         data-testid="connections-chain">
                        <h3 id="connections-chain-heading"
                            class="mb-1 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide
                                   text-stone-500 dark:text-stone-400">
                            "Further down the chain"
                            <span class="font-normal text-stone-400 dark:text-stone-500">
                                {found.also.len()}
                            </span>
                        </h3>
                        <ul class="space-y-0.5">
                            {found.also.into_iter().map(|b| {
                                let kind = b.node.node_type.as_str();
                                view! {
                                    <li class="rounded px-2 hover:bg-stone-50 dark:hover:bg-stone-800/50"
                                        data-testid="chain-row">
                                        <a href=format!("/nodes/{}", b.id)
                                            class="flex min-w-0 items-center gap-2 py-1.5 text-xs
                                                   text-stone-600 dark:text-stone-400
                                                   hover:text-amber-700 dark:hover:text-amber-400">
                                            <span class="material-symbols-outlined shrink-0 text-stone-400 dark:text-stone-500"
                                                  style="font-size: 14px;" aria-hidden="true"
                                                  title=type_label(kind)>
                                                {type_icon(kind)}
                                            </span>
                                            <span class="sr-only">{format!("{}: ", type_label(kind))}</span>
                                            <span class="font-medium truncate">{b.title.clone()}</span>
                                            <span class="shrink-0 rounded bg-stone-100 px-1.5 py-0.5 text-[10px] font-medium
                                                         text-stone-500 dark:bg-stone-800 dark:text-stone-400"
                                                  data-testid="connection-standing">
                                                {blockers::standing(&b.node)}
                                            </span>
                                            <span class="min-w-0 truncate text-stone-400 dark:text-stone-500"
                                                  data-testid="chain-through">
                                                {format!("through {}", b.via_title)}
                                            </span>
                                        </a>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                        {found.truncated.then(|| view! {
                            <p class="mt-1 px-2 text-xs text-stone-400 dark:text-stone-500"
                               data-testid="chain-more">
                                "The chain goes on: only its first links are shown."
                            </p>
                        })}
                    </div>
                }.into_any(),
                // The list above is whole without it: say so, quietly.
                Err(_) => view! {
                    <p class="px-2 text-xs text-stone-400 dark:text-stone-500" role="alert"
                       data-testid="chain-error">
                        "Couldn\u{2019}t load what these wait for in turn."
                    </p>
                }.into_any(),
            })}
        </Suspense>
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
                {head.into_iter().map(|row| row_view(row, group, links)).collect_view()}
                {move || whole().then(|| {
                    tail.get_value().into_iter().map(|row| row_view(row, group, links)).collect_view()
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
fn row_view(row: Connection, group: Group, links: NodeLinks) -> impl IntoView {
    let edge_id = row.edge_id;
    // In "Depends on" every row says whether it is met (S7b).
    let standing = row.standing_in(group);
    // There, a row is quiet once it is met; elsewhere, once out of use.
    let live = if group == Group::DependsOn {
        row.blocks_in(group) || row.other.is_none()
    } else {
        row.is_live()
    };
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
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mention {
    id: NodeId,
    title: String,
}

/// What the search for mentions found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mentions {
    hits: Vec<Mention>,
    /// The search has more matches than were looked through. An empty
    /// list then says so: "none" would be a claim nobody checked.
    more_unchecked: bool,
}

/// At most this many mentions are listed.
const MENTIONS_SHOWN: usize = 10;
/// The search is read a page at a time, up to this many pages. Every
/// node that links here names the title too, so on a well-linked node
/// the first page can be all links and no plain mention.
const SEARCH_PAGE: u32 = 25;
const SEARCH_PAGES: u32 = 4;

/// The hits of one search page that are unlinked mentions: not this node,
/// not a node that already links here, and a match in a node's own text
/// (a note's or a task's can't be rewritten into a wikilink).
fn unlinked(
    node: NodeId,
    linked: &HashSet<NodeId>,
    results: Vec<common::search::SearchResult>,
) -> Vec<Mention> {
    results
        .into_iter()
        .filter(|r| {
            r.node_id != node
                && !linked.contains(&r.node_id)
                && r.match_source.as_deref() == Some("node")
        })
        .map(|r| Mention {
            id: r.node_id,
            title: r.title,
        })
        .collect()
}

/// Whether the search stopped with matches it never looked at, and room
/// left in the list: only then might a mention be missing from it.
fn stopped_short(shown: usize, checked: u64, total: u64) -> bool {
    shown < MENTIONS_SHOWN && checked < total
}

/// Reads the search until the list is full, the matches run out, or
/// `SEARCH_PAGES` pages have been read.
async fn find_mentions(
    node: NodeId,
    title: &str,
    linked: &HashSet<NodeId>,
) -> Result<Mentions, UiError> {
    let mut hits: Vec<Mention> = Vec::new();
    let mut checked = 0u64;
    let mut total = 0u64;
    for page in 1..=SEARCH_PAGES {
        let resp = crate::api::search_nodes(
            title,
            false,
            None,
            None,
            &[],
            "or",
            Some("relevance"),
            None,
            None,
            page,
            SEARCH_PAGE,
        )
        .await?;
        total = resp.total;
        let got = resp.results.len() as u64;
        checked += got;
        hits.extend(unlinked(node, linked, resp.results));
        if hits.len() >= MENTIONS_SHOWN || got == 0 || checked >= total {
            break;
        }
    }
    hits.truncate(MENTIONS_SHOWN);
    Ok(Mentions {
        more_unchecked: stopped_short(hits.len(), checked, total),
        hits,
    })
}

/// Nodes whose text names this node's title without linking to it, each
/// with a one-click "turn the mention into a `[[link]]`". The nodes that
/// do link here are in Connections ("Mentioned in" and the other groups).
#[component]
pub fn UnlinkedMentionsPanel(node_id: NodeId, title: String, links: NodeLinks) -> impl IntoView {
    let open = RwSignal::new(false);
    links.wanted_while(open);
    let title_sv = StoredValue::new(title);

    // `Ok(None)` while the links are still on their way: which nodes
    // already link here isn't known yet, so no list is shown that might
    // name one of them.
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
                return Ok(Some(Mentions {
                    hits: Vec::new(),
                    more_unchecked: false,
                }));
            }
            find_mentions(node_id, &title, &linked).await.map(Some)
        }
    });

    // The mentions linked from this page since the list was read. The
    // list is read again only once the links have been (one round trip
    // later); until then a linked row would still stand there with a live
    // button, and a second click would link a second mention.
    let just_linked = RwSignal::new(HashSet::<NodeId>::new());
    let shown = move |found: Mentions| -> Mentions {
        let gone = just_linked.get();
        Mentions {
            hits: found
                .hits
                .into_iter()
                .filter(|m| !gone.contains(&m.id))
                .collect(),
            more_unchecked: found.more_unchecked,
        }
    };

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
            match result {
                Ok(Some(_)) => {
                    let _ = just_linked.try_update(|done| {
                        done.insert(mention_id);
                    });
                    push_toast(ToastLevel::Success, "Mention linked.");
                    links.reload();
                }
                Ok(None) => push_toast(
                    ToastLevel::Info,
                    "No literal mention found \u{2014} the match may be a different word form.",
                ),
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn\u{2019}t link: {e}")),
            }
            let _ = linking.try_set(false);
        });
    };

    let section_count = Signal::derive(move || {
        let found = mentions.get()?.ok()??;
        Some(shown(found).hits.len())
    });
    view! {
        <SectionShell section=NodeSection::UnlinkedMentions open=open count=section_count>
            <Suspense fallback=|| view! {
                <div class="text-xs text-stone-400" data-testid="mentions-loading">"Looking for mentions\u{2026}"</div>
            }>
                {move || mentions.get().map(|result| match result.map(|found| found.map(shown)) {
                    Ok(None) => view! {
                        <div class="text-xs text-stone-400" data-testid="mentions-loading">"Looking for mentions\u{2026}"</div>
                    }.into_any(),
                    Ok(Some(found)) if found.hits.is_empty() => view! {
                        <div class="flex flex-col items-center gap-2 py-6" data-testid="mentions-empty">
                            <span
                                class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                style="font-size: 32px;"
                                aria-hidden="true"
                            >
                                "alternate_email"
                            </span>
                            <p class="text-xs text-stone-400 dark:text-stone-600">
                                {if found.more_unchecked {
                                    // Not "none": only the best matches were read.
                                    "None among the closest matches for this title. There are more matches than were checked."
                                } else {
                                    "No other node names this one without linking to it."
                                }}
                            </p>
                        </div>
                    }.into_any(),
                    Ok(Some(found)) => view! {
                        <ul class="space-y-0.5">
                            {found.hits.into_iter().map(|hit| {
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
    fn the_chip_counts_from_the_links_once_they_are_read_and_from_the_node_until_then() {
        let me = 1;
        let unmet = |title: u128| {
            let mut e = edge(title, me, title, EdgeType::DependsOn);
            e.target_node = Some(common::edge::EdgeEnd {
                node_type: NodeType::Project,
                status: NodeStatus::Draft,
                project_state: Some(ProjectState::Active),
            });
            e
        };
        // Not read yet: what the node's own answer carried.
        assert_eq!(unmet_dependencies(id(me), 2, None), 2);
        // Read: the links decide, so a link added or removed on the page
        // shows at once, whatever the node's answer said when it loaded.
        let one: LinksResult = Ok(vec![unmet(2)]);
        assert_eq!(unmet_dependencies(id(me), 2, Some(&one)), 1);
        let none: LinksResult = Ok(Vec::new());
        assert_eq!(unmet_dependencies(id(me), 2, Some(&none)), 0);
        let three: LinksResult = Ok(vec![unmet(2), unmet(3), unmet(4)]);
        assert_eq!(unmet_dependencies(id(me), 0, Some(&three)), 3);
        // A failed read of the links says nothing new: the node's count
        // stands (the section shows the error itself).
        let failed: LinksResult = Err(UiError::Network("offline".to_string()));
        assert_eq!(unmet_dependencies(id(me), 2, Some(&failed)), 2);
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
        let groups_of = |kind: &NodeType, can_edit: bool, edges: &[EdgeWithTitles]| -> Vec<Group> {
            listed(kind, can_edit, grouped(id(me), edges.to_vec()))
                .into_iter()
                .map(|(g, _)| g)
                .collect()
        };
        let groups = |kind: &NodeType| groups_of(kind, true, &edges);
        // A Project has a home row above the list.
        assert_eq!(groups(&NodeType::Project), vec![Group::UsedIn]);
        // A viewer has the row too, without buttons, and no remove here.
        assert_eq!(
            groups_of(&NodeType::Project, false, &edges),
            vec![Group::UsedIn]
        );
        // A Reference has none (it is never contained), so a home from
        // before the rule still shows, where it can be removed.
        assert!(allowed_homes(&NodeType::Reference).is_empty());
        assert_eq!(
            groups(&NodeType::Reference),
            vec![Group::Home, Group::UsedIn]
        );
        // Someone who may remove the home's link but gets no buttons on
        // the row (a shared editor of the child) is offered it here.
        let mut removable = edges.clone();
        if let Some(home) = removable.first_mut() {
            home.may_remove = true;
        }
        assert_eq!(
            groups_of(&NodeType::Project, false, &removable),
            vec![Group::Home, Group::UsedIn]
        );
        // The owner has the row's own "Remove home": not twice.
        assert_eq!(
            groups_of(&NodeType::Project, true, &removable),
            vec![Group::UsedIn]
        );
    }

    fn hit(n: u128, source: &str) -> common::search::SearchResult {
        common::search::SearchResult {
            node_id: id(n),
            title: format!("node {n}"),
            slug: String::new(),
            snippet: None,
            rank: 0.0,
            node_type: "article".to_string(),
            status: "draft".to_string(),
            match_source: Some(source.to_string()),
            highlighted_title: None,
            updated_at: None,
        }
    }

    #[test]
    fn an_unlinked_mention_is_another_nodes_own_text_that_does_not_link_here() {
        let linked: HashSet<NodeId> = [id(2)].into_iter().collect();
        let found = unlinked(
            id(1),
            &linked,
            vec![
                hit(1, "node"), // this node itself
                hit(2, "node"), // links here already
                hit(3, "node"),
                hit(4, "note"), // a note's text can't be rewritten
                hit(5, "task"),
            ],
        );
        assert_eq!(
            found,
            vec![Mention {
                id: id(3),
                title: "node 3".to_string()
            }]
        );
    }

    #[test]
    fn an_empty_list_is_none_only_when_every_match_was_looked_at() {
        // Everything was read: "none" is true.
        assert!(!stopped_short(0, 25, 25));
        assert!(!stopped_short(0, 0, 0));
        // More matches than were read, and room in the list: one may be
        // missing, so the page says so.
        assert!(stopped_short(0, 100, 240));
        assert!(stopped_short(3, 100, 240));
        // A full list is not a claim about the rest.
        assert!(!stopped_short(MENTIONS_SHOWN, 25, 240));
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
