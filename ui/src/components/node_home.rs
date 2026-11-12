//! A node's one home (#228, ONTOLOGY §3.3, §6.1), shown at the top of its
//! Connections section: the parent as a link, "stands alone", or, for the
//! child's editors only, "A home you can't see" (§14: never named). Editors
//! can choose, change or remove it here, from the child's own page; the
//! picker offers only the kinds that may be its home
//! (`ontology::allowed_homes`). The API checks every rule again.
//!
//! The same home is in the page header as a chip ([`HomeChip`], owner
//! request 2026-11-12): the dashboard groups projects by their Area, and
//! the only place to set one was inside the collapsed Connections section.
//! The chip and the row read one refresh counter, so a change in either
//! shows in both.

use common::edge::{Home, NodeHome};
use common::id::NodeId;
use common::node::NodeType;
use common::ontology::allowed_homes;
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::node_picker::NodePicker;
use crate::components::toast::{ToastLevel, push_toast};

/// What the row says about a home, decided from the API's answer alone.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shown {
    /// The parent, by title, with its kind (for the chip's icon).
    Named {
        id: NodeId,
        title: String,
        kind: NodeType,
    },
    /// A home the caller can't see (only ever sent to the child's editors).
    Hidden,
    /// No home: the node stands alone.
    Alone,
}

fn shown(home: &NodeHome) -> Shown {
    match &home.home {
        Some(Home { node: Some(n), .. }) => Shown::Named {
            id: n.id,
            title: n.title.clone(),
            kind: n.node_type.clone(),
        },
        Some(Home { node: None, .. }) => Shown::Hidden,
        None => Shown::Alone,
    }
}

/// The picker's placeholder: which kinds may be the home.
pub(crate) fn pick_placeholder(kinds: &[NodeType]) -> String {
    let names: Vec<&str> = kinds
        .iter()
        .map(|k| crate::components::node_meta::type_label(k.as_str()))
        .collect();
    format!("Choose a home ({})\u{2026}", names.join(" or "))
}

#[component]
pub fn HomeRow(
    node_id: NodeId,
    node_type: NodeType,
    /// The caller may edit this node (owner or admin; a shared editor is
    /// also allowed by the API but sees no buttons, as elsewhere).
    can_edit: bool,
    /// Only fetch while the Connections section is open.
    open: RwSignal<bool>,
    /// The section's one refresh counter: a home change bumps it so the
    /// link list reloads, and removing the home's link from that list
    /// bumps it so this row does.
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let kinds = allowed_homes(&node_type);
    if kinds.is_empty() {
        // A Reference is never contained (§6.1): nothing to show or pick.
        return view! {
            <p class="mb-3 text-xs text-stone-500 dark:text-stone-400" data-testid="node-home">
                "A Reference has no home: other nodes link to it."
            </p>
        }
        .into_any();
    }
    let home = LocalResource::new(move || {
        let _ = refresh.get();
        let is_open = open.get();
        async move {
            if !is_open {
                return None;
            }
            crate::api::fetch_home(node_id).await.ok()
        }
    });
    let picking = RwSignal::new(false);
    let target = RwSignal::new(None::<(NodeId, String)>);
    let busy = RwSignal::new(false);
    let save = move |parent: Option<NodeId>, done: &'static str| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::set_home(node_id, parent).await {
                Ok(_) => {
                    push_toast(ToastLevel::Success, done.to_string());
                    picking.try_set(false);
                    target.try_set(None);
                    refresh.try_update(|v| *v += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't change the home: {e}")),
            }
            busy.try_set(false);
        });
    };
    let commit = move || {
        if let Some((parent, _)) = target.get_untracked() {
            save(Some(parent), "Home set");
        }
    };
    let placeholder = pick_placeholder(kinds);

    view! {
        <div class="mb-3 flex flex-wrap items-center gap-2 text-sm" data-testid="node-home">
            <span class="text-xs font-medium uppercase tracking-wide text-stone-500 dark:text-stone-400">
                "Home"
            </span>
            {move || {
                if picking.get() {
                    return view! {
                        <div class="flex-1 min-w-[12rem] flex items-start gap-1">
                            <div class="flex-1 min-w-0">
                                <NodePicker
                                    selected=target
                                    placeholder=placeholder.clone()
                                    exclude=node_id
                                    autofocus=true
                                    only_types=kinds
                                    on_commit=Callback::new(move |()| commit())
                                />
                            </div>
                            <IconButton icon="check" label="Set home"
                                variant=IconButtonVariant::Save
                                disabled=Signal::derive(move || busy.get() || target.with(Option::is_none))
                                on_click=Callback::new(move |()| commit()) />
                            <IconButton icon="close" label="Cancel"
                                on_click=Callback::new(move |()| {
                                    picking.set(false);
                                    target.set(None);
                                }) />
                        </div>
                    }
                    .into_any();
                }
                let Some(Some(current)) = home.get() else {
                    return ().into_any();
                };
                let state = shown(&current);
                let has_home = state != Shown::Alone;
                let label = match state {
                    Shown::Named { id, title, .. } => view! {
                        <a href=format!("/nodes/{id}")
                            class="text-stone-800 dark:text-stone-200 hover:text-amber-700 dark:hover:text-amber-400 hover:underline">
                            {title}
                        </a>
                    }
                    .into_any(),
                    Shown::Hidden => view! {
                        <span class="italic text-stone-500 dark:text-stone-400">"A home you can\u{2019}t see"</span>
                    }
                    .into_any(),
                    Shown::Alone => view! {
                        <span class="text-stone-500 dark:text-stone-400">"None: it stands alone"</span>
                    }
                    .into_any(),
                };
                view! {
                    {label}
                    {can_edit.then(|| view! {
                        <IconButton
                            icon=if has_home { "edit" } else { "add" }
                            label=if has_home { "Change home" } else { "Choose a home" }
                            disabled=Signal::derive(move || busy.get())
                            on_click=Callback::new(move |()| picking.set(true)) />
                        {has_home.then(|| view! {
                            <IconButton icon="link_off" label="Remove home"
                                variant=IconButtonVariant::Danger
                                disabled=Signal::derive(move || busy.get())
                                on_click=Callback::new(move |()| save(None, "Home removed")) />
                        })}
                    })}
                }
                .into_any()
            }}
        </div>
    }
    .into_any()
}

const CHIP_CLASS: &str = "inline-flex items-center gap-1 max-w-[14rem] px-2 py-0.5 rounded-full \
     text-xs font-medium bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-300 \
     border border-stone-200 dark:border-stone-700";

/// What the chip says when the node has no home: the kinds it could have.
/// A Project's is "No area or project"; an Article's "No area".
pub(crate) fn no_home_label(kinds: &[NodeType]) -> String {
    let names: Vec<String> = kinds
        .iter()
        .map(|k| crate::components::node_meta::type_label(k.as_str()).to_lowercase())
        .collect();
    format!("No {}", names.join(" or "))
}

/// The node's home as a chip in the page header: its title, linking to it,
/// and for an editor a control to choose, change or remove it. A node
/// with no home says so ("No area or project") to an editor, who can set
/// one there; a viewer sees nothing.
///
/// It reads `GET /nodes/{id}/home` on every page load (the row in
/// Connections reads it only while that section is open). A failed read
/// shows no chip: the row in Connections is still there, and says so.
#[component]
pub fn HomeChip(
    node_id: NodeId,
    node_type: NodeType,
    /// The caller may edit this node. The API decides; this only hides
    /// controls that would be refused.
    can_edit: bool,
    /// The page's home-and-links refresh counter, shared with Connections.
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let kinds = allowed_homes(&node_type);
    if kinds.is_empty() {
        // A Reference is never contained (§6.1).
        return ().into_any();
    }
    let home = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::fetch_home(node_id).await.ok() }
    });
    let open = RwSignal::new(false);
    let target = RwSignal::new(None::<(NodeId, String)>);
    let busy = RwSignal::new(false);
    let save = move |parent: Option<NodeId>, done: &'static str| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::set_home(node_id, parent).await {
                Ok(_) => {
                    push_toast(ToastLevel::Success, done.to_string());
                    open.try_set(false);
                    target.try_set(None);
                    refresh.try_update(|v| *v += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't change the home: {e}")),
            }
            busy.try_set(false);
        });
    };
    let commit = move || {
        if let Some((parent, _)) = target.get_untracked() {
            save(Some(parent), "Home set");
        }
    };
    let placeholder = pick_placeholder(kinds);
    let none = no_home_label(kinds);

    view! {
        <div class="relative flex items-center gap-0.5 min-w-0" data-testid="home-chip">
            {move || {
                let current = home.get().flatten()?;
                let state = shown(&current);
                let has_home = state != Shown::Alone;
                let label = match state {
                    Shown::Named { id, title, kind } => view! {
                        <a href=format!("/nodes/{id}")
                            class=format!("{CHIP_CLASS} hover:border-stone-400 dark:hover:border-stone-500 \
                                hover:text-amber-700 dark:hover:text-amber-400")
                            title=format!("Home: {title}")>
                            <span class="material-symbols-outlined flex-shrink-0" style="font-size: 14px;"
                                aria-hidden="true">
                                {crate::components::node_meta::type_icon(kind.as_str())}
                            </span>
                            <span class="truncate">{title.clone()}</span>
                        </a>
                    }
                    .into_any(),
                    Shown::Hidden => view! {
                        <span class=format!("{CHIP_CLASS} italic")>"A home you can\u{2019}t see"</span>
                    }
                    .into_any(),
                    // Only an editor is told: they can do something about it.
                    Shown::Alone if can_edit => {
                        let none = none.clone();
                        view! {
                            <button
                                type="button"
                                class=format!("{CHIP_CLASS} cursor-pointer hover:border-stone-400 \
                                    dark:hover:border-stone-500")
                                style="border-style:dashed;"
                                title="Choose a home"
                                aria-label=format!("{none}. Choose a home")
                                aria-haspopup="dialog"
                                aria-expanded=move || if open.get() { "true" } else { "false" }
                                on:click=move |_| open.update(|v| *v = !*v)
                            >
                                <span class="truncate">{none.clone()}</span>
                                <span class="material-symbols-outlined" style="font-size: 14px;"
                                    aria-hidden="true">"expand_more"</span>
                            </button>
                        }
                        .into_any()
                    }
                    Shown::Alone => return None,
                };
                Some(view! {
                    {label}
                    {(can_edit && has_home).then(|| view! {
                        <button
                            type="button"
                            class="p-0.5 rounded text-stone-400 hover:text-stone-600
                                dark:hover:text-stone-300 cursor-pointer"
                            title="Change home"
                            aria-label="Change home"
                            aria-haspopup="dialog"
                            aria-expanded=move || if open.get() { "true" } else { "false" }
                            on:click=move |_| open.update(|v| *v = !*v)
                        >
                            <span class="material-symbols-outlined" style="font-size: 16px;"
                                aria-hidden="true">"expand_more"</span>
                        </button>
                    })}
                    {
                        // Cloned out here: moving the page closure's own
                        // copy into the popover's would make it FnOnce.
                        let placeholder = placeholder.clone();
                        move || open.get().then(|| {
                        let placeholder = placeholder.clone();
                        view! {
                            <div class="fixed inset-0 z-20" on:click=move |_| {
                                open.set(false);
                                target.set(None);
                            } />
                            <div class="absolute left-0 top-full mt-1 z-30 w-72 max-w-[calc(100vw-2rem)]
                                rounded-xl border border-stone-200 dark:border-stone-700
                                bg-white dark:bg-stone-900 shadow-lg p-2 space-y-1.5"
                                role="dialog" aria-label="Home" data-testid="home-chip-popover">
                                <div class="flex items-start gap-1">
                                    <div class="flex-1 min-w-0">
                                        <NodePicker
                                            selected=target
                                            placeholder=placeholder
                                            exclude=node_id
                                            autofocus=true
                                            only_types=kinds
                                            on_commit=Callback::new(move |()| commit())
                                        />
                                    </div>
                                    <IconButton icon="check" label="Set home"
                                        variant=IconButtonVariant::Save
                                        disabled=Signal::derive(move || busy.get() || target.with(Option::is_none))
                                        on_click=Callback::new(move |()| commit()) />
                                </div>
                                {has_home.then(|| view! {
                                    <button
                                        type="button"
                                        class="w-full text-left px-2 py-1.5 rounded-lg text-xs text-stone-500
                                            hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer
                                            disabled:opacity-40"
                                        disabled=move || busy.get()
                                        on:click=move |_| save(None, "Home removed")
                                    >
                                        "Remove home"
                                    </button>
                                })}
                            </div>
                        }
                    })
                    }
                })
            }}
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::edge::HomeNode;
    use common::id::EdgeId;
    use uuid::Uuid;

    #[test]
    fn a_home_is_named_hidden_or_absent_as_the_api_says() {
        let edge_id = EdgeId(Uuid::from_u128(1));
        let id = NodeId(Uuid::from_u128(2));
        let named = NodeHome {
            home: Some(Home {
                edge_id,
                node: Some(HomeNode {
                    id,
                    title: "An area".into(),
                    node_type: NodeType::Area,
                }),
            }),
        };
        assert_eq!(
            shown(&named),
            Shown::Named {
                id,
                title: "An area".into(),
                kind: NodeType::Area,
            }
        );
        let hidden = NodeHome {
            home: Some(Home {
                edge_id,
                node: None,
            }),
        };
        assert_eq!(shown(&hidden), Shown::Hidden);
        assert_eq!(shown(&NodeHome { home: None }), Shown::Alone);
    }

    #[test]
    fn a_node_with_no_home_says_which_kinds_it_could_have() {
        assert_eq!(
            no_home_label(allowed_homes(&NodeType::Project)),
            "No area or project"
        );
        assert_eq!(no_home_label(allowed_homes(&NodeType::Article)), "No area");
    }

    #[test]
    fn the_picker_names_the_kinds_that_may_be_home() {
        assert_eq!(
            pick_placeholder(allowed_homes(&NodeType::Project)),
            "Choose a home (Area or Project)\u{2026}"
        );
        assert_eq!(
            pick_placeholder(allowed_homes(&NodeType::Article)),
            "Choose a home (Area)\u{2026}"
        );
    }
}
