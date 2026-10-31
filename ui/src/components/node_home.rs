//! A node's one home (#228, ONTOLOGY §3.3, §6.1), shown at the top of its
//! Connections section: the parent as a link, "stands alone", or, for the
//! child's editors only, "A home you can't see" (§14: never named). Editors
//! can choose, change or remove it here, from the child's own page; the
//! picker offers only the kinds that may be its home
//! (`ontology::allowed_homes`). The API checks every rule again.

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
    /// The parent, by title.
    Named { id: NodeId, title: String },
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
        },
        Some(Home { node: None, .. }) => Shown::Hidden,
        None => Shown::Alone,
    }
}

/// The picker's placeholder: which kinds may be the home.
fn pick_placeholder(kinds: &[NodeType]) -> String {
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
    /// Runs after a change, so the section's link list reloads too.
    on_change: Callback<()>,
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
    let version = RwSignal::new(0u32);
    let home = LocalResource::new(move || {
        let _ = version.get();
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
                    version.try_update(|v| *v += 1);
                    on_change.try_run(());
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
                    Shown::Named { id, title } => view! {
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
                title: "An area".into()
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
