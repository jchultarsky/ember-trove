use common::{
    id::{NodeId, NodeLinkId},
    node_link::{CreateNodeLinkRequest, UpdateNodeLinkRequest},
};
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant};

/// Collapsible panel showing external links (named URLs) attached to a node.
/// Editors and Owners can add, edit, and delete links.
/// Viewers see links but have no mutation controls.
#[component]
pub fn LinksPanel(node_id: NodeId, is_editor: bool) -> impl IntoView {
    let open    = RwSignal::new(false);
    let refresh = RwSignal::new(0u32);

    // ── Add-form state ────────────────────────────────────────────────────────
    let show_add  = RwSignal::new(false);
    let new_name  = RwSignal::new(String::new());
    let new_url   = RwSignal::new(String::new());
    let add_error = RwSignal::new(Option::<String>::None);

    // ── Edit-form state (per link) ────────────────────────────────────────────
    let editing_id  = RwSignal::new(Option::<NodeLinkId>::None);
    let edit_name   = RwSignal::new(String::new());
    let edit_url    = RwSignal::new(String::new());
    let edit_error  = RwSignal::new(Option::<String>::None);

    let links_res = LocalResource::new(move || {
        let _ = refresh.get();
        let is_open = open.get();
        async move {
            if !is_open { return Ok(vec![]); }
            crate::api::fetch_node_links(node_id).await
        }
    });

    // ── Add handler ───────────────────────────────────────────────────────────
    let do_add = move || {
        let name = new_name.get_untracked().trim().to_string();
        let url  = new_url.get_untracked().trim().to_string();
        if name.is_empty() || url.is_empty() {
            add_error.set(Some("Name and URL are required.".to_string()));
            return;
        }
        // Prefix with https:// if the user omitted a scheme, so the browser
        // opens an absolute URL rather than a relative path.
        let url = if !url.starts_with("http://") && !url.starts_with("https://") {
            format!("https://{url}")
        } else {
            url
        };
        let req = CreateNodeLinkRequest { name, url };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::create_node_link(node_id, &req).await {
                Ok(_) => {
                    new_name.set(String::new());
                    new_url.set(String::new());
                    add_error.set(None);
                    show_add.set(false);
                    refresh.update(|n| *n += 1);
                }
                Err(e) => add_error.set(Some(e.to_string())),
            }
        });
    };

    // ── Edit save handler ─────────────────────────────────────────────────────
    let do_save_edit = move || {
        let Some(link_id) = editing_id.get_untracked() else { return };
        let name = edit_name.get_untracked().trim().to_string();
        let url  = edit_url.get_untracked().trim().to_string();
        if name.is_empty() || url.is_empty() {
            edit_error.set(Some("Name and URL are required.".to_string()));
            return;
        }
        let url = if !url.starts_with("http://") && !url.starts_with("https://") {
            format!("https://{url}")
        } else {
            url
        };
        let req = UpdateNodeLinkRequest {
            name: Some(name),
            url:  Some(url),
        };
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_node_link(node_id, link_id, &req).await {
                Ok(_) => {
                    editing_id.set(None);
                    edit_error.set(None);
                    refresh.update(|n| *n += 1);
                }
                Err(e) => edit_error.set(Some(e.to_string())),
            }
        });
    };

    view! {
        <div class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6">
            // ── Section header ────────────────────────────────────────────────
            <div class="flex items-center justify-between">
                <button
                    class="flex items-center gap-1 text-left cursor-pointer"
                    on:click=move |_| open.update(|o| *o = !*o)
                >
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 16px;"
                    >
                        {move || if open.get() { "expand_more" } else { "chevron_right" }}
                    </span>
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 15px;"
                    >
                        "open_in_new"
                    </span>
                    <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                        "External Links"
                    </h2>
                    <Suspense fallback=|| ()>
                    {move || {
                        links_res.with(|r| r.as_ref().and_then(|res| match res {
                            Ok(v) if !v.is_empty() => Some(view! {
                                <span class="ml-1 text-xs bg-stone-200 dark:bg-stone-700
                                            text-stone-600 dark:text-stone-300
                                            rounded-full px-1.5 py-0.5">
                                    {v.len()}
                                </span>
                            }),
                            _ => None,
                        }))
                    }}
                    </Suspense>
                </button>
                {move || (open.get() && is_editor).then(|| view! {
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                            dark:hover:text-stone-300 hover:bg-stone-100
                            dark:hover:bg-stone-800 transition-colors"
                        on:click=move |_| {
                            show_add.update(|v| *v = !*v);
                            editing_id.set(None);
                        }
                        title=move || if show_add.get() { "Cancel" } else { "Add link" }
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;">
                            {move || if show_add.get() { "close" } else { "add_link" }}
                        </span>
                    </button>
                })}
            </div>

            // ── Collapsible body ──────────────────────────────────────────────
            {move || open.get().then(|| view! {
                <div class="mt-4">
                    <Suspense fallback=move || view! {
                        <p class="text-xs text-stone-400 py-2">"Loading…"</p>
                    }>
                        {move || links_res.with(|r| r.as_ref().map(|res| {
                            match res {
                                Err(e) => view! {
                                    <p class="text-xs text-red-500 py-2">{e.to_string()}</p>
                                }.into_any(),
                                Ok(links) if links.is_empty() && !is_editor => view! {
                                    <p class="text-xs text-stone-400 py-2 italic">"No external links yet."</p>
                                }.into_any(),
                                Ok(links) => {
                                    let links = links.clone();
                                    view! {
                                        <ul class="space-y-1 mb-3">
                                            {links.into_iter().map(|link| {
                                                let link_id = link.id;
                                                let name_clone = link.name.clone();
                                                let url_clone  = link.url.clone();

                                                view! {
                                                    <li class="group flex items-center gap-2 py-1">
                                                        {move || {
                                                            if editing_id.get() == Some(link_id) {
                                                                // ── Inline edit form ─────────────────────
                                                                view! {
                                                                    <div class="flex flex-col gap-1.5 w-full">
                                                                        <input
                                                                            type="text"
                                                                            class="text-sm border border-stone-300 dark:border-stone-600
                                                                                   rounded px-2 py-1 w-full
                                                                                   bg-white dark:bg-stone-800
                                                                                   text-stone-900 dark:text-stone-100
                                                                                   focus:outline-none focus:ring-1 focus:ring-amber-500"
                                                                            placeholder="Link name"
                                                                            prop:value=move || edit_name.get()
                                                                            on:input=move |ev| edit_name.set(event_target_value(&ev))
                                                                        />
                                                                        <input
                                                                            type="url"
                                                                            class="text-sm border border-stone-300 dark:border-stone-600
                                                                                   rounded px-2 py-1 w-full
                                                                                   bg-white dark:bg-stone-800
                                                                                   text-stone-900 dark:text-stone-100
                                                                                   focus:outline-none focus:ring-1 focus:ring-amber-500"
                                                                            placeholder="https://example.com"
                                                                            prop:value=move || edit_url.get()
                                                                            on:input=move |ev| edit_url.set(event_target_value(&ev))
                                                                        />
                                                                        {move || edit_error.get().map(|e| view! {
                                                                            <p class="text-xs text-red-500">{e}</p>
                                                                        })}
                                                                        <div class="flex gap-1">
                                                                            <IconButton
                                                                                icon="check"
                                                                                label="Save"
                                                                                variant=IconButtonVariant::Save
                                                                                on_click=Callback::new(move |()| do_save_edit())
                                                                            />
                                                                            <IconButton
                                                                                icon="close"
                                                                                label="Cancel"
                                                                                on_click=Callback::new(move |()| {
                                                                                    editing_id.set(None);
                                                                                    edit_error.set(None);
                                                                                })
                                                                            />
                                                                        </div>
                                                                    </div>
                                                                }.into_any()
                                                            } else {
                                                                // ── Read-only row ─────────────────────────
                                                                let name_display = name_clone.clone();
                                                                let url_display  = url_clone.clone();
                                                                let name_for_edit = name_clone.clone();
                                                                let url_for_edit  = url_clone.clone();
                                                                view! {
                                                                    <span class="material-symbols-outlined text-amber-500"
                                                                          style="font-size:16px; flex-shrink:0;">
                                                                        "open_in_new"
                                                                    </span>
                                                                    <a
                                                                        href=url_display.clone()
                                                                        target="_blank"
                                                                        rel="noopener noreferrer"
                                                                        class="flex-1 text-sm text-amber-600 dark:text-amber-400
                                                                               hover:underline truncate"
                                                                        title=url_display
                                                                    >
                                                                        {name_display}
                                                                    </a>
                                                                    {is_editor.then(|| view! {
                                                                        <div class="flex items-center gap-1
                                                                                    opacity-0 group-hover:opacity-100
                                                                                    transition-opacity">
                                                                            <button
                                                                                class="p-1 rounded text-stone-400
                                                                                       hover:text-stone-600 dark:hover:text-stone-200
                                                                                       hover:bg-stone-100 dark:hover:bg-stone-800
                                                                                       cursor-pointer transition-colors"
                                                                                title="Edit link"
                                                                                on:click=move |_| {
                                                                                    edit_name.set(name_for_edit.clone());
                                                                                    edit_url.set(url_for_edit.clone());
                                                                                    edit_error.set(None);
                                                                                    editing_id.set(Some(link_id));
                                                                                    show_add.set(false);
                                                                                }
                                                                            >
                                                                                <span class="material-symbols-outlined"
                                                                                      style="font-size:14px;">"edit"</span>
                                                                            </button>
                                                                            <button
                                                                                class="p-1 rounded text-stone-400
                                                                                       hover:text-red-600 dark:hover:text-red-400
                                                                                       hover:bg-red-50 dark:hover:bg-red-900/30
                                                                                       cursor-pointer transition-colors"
                                                                                title="Delete link"
                                                                                on:click=move |_| {
                                                                                    wasm_bindgen_futures::spawn_local(async move {
                                                                                        let _ = crate::api::delete_node_link(node_id, link_id).await;
                                                                                        refresh.update(|n| *n += 1);
                                                                                    });
                                                                                }
                                                                            >
                                                                                <span class="material-symbols-outlined"
                                                                                      style="font-size:14px;">"delete"</span>
                                                                            </button>
                                                                        </div>
                                                                    })}
                                                                }.into_any()
                                                            }
                                                        }}
                                                    </li>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </ul>

                                        // ── Add-link form (editor only, toggled from header) ──
                                        {is_editor.then(|| view! {
                                            {move || show_add.get().then(|| view! {
                                                <div class="flex flex-col gap-1.5 mt-1">
                                                    <input
                                                        type="text"
                                                        class="text-sm border border-stone-300 dark:border-stone-600
                                                               rounded px-2 py-1 w-full
                                                               bg-white dark:bg-stone-800
                                                               text-stone-900 dark:text-stone-100
                                                               focus:outline-none focus:ring-1 focus:ring-amber-500"
                                                        placeholder="Link name"
                                                        prop:value=move || new_name.get()
                                                        on:input=move |ev| new_name.set(event_target_value(&ev))
                                                    />
                                                    <input
                                                        type="url"
                                                        class="text-sm border border-stone-300 dark:border-stone-600
                                                               rounded px-2 py-1 w-full
                                                               bg-white dark:bg-stone-800
                                                               text-stone-900 dark:text-stone-100
                                                               focus:outline-none focus:ring-1 focus:ring-amber-500"
                                                        placeholder="https://example.com"
                                                        prop:value=move || new_url.get()
                                                        on:input=move |ev| new_url.set(event_target_value(&ev))
                                                    />
                                                    {move || add_error.get().map(|e| view! {
                                                        <p class="text-xs text-red-500">{e}</p>
                                                    })}
                                                    <div class="flex gap-1">
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                                                                   hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer"
                                                            title="Add link"
                                                            on:click=move |_| do_add()
                                                        >
                                                            <span class="material-symbols-outlined">"check"</span>
                                                        </button>
                                                        <button
                                                            class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                                                                   hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                                                            title="Cancel"
                                                            on:click=move |_| {
                                                                show_add.set(false);
                                                                new_name.set(String::new());
                                                                new_url.set(String::new());
                                                                add_error.set(None);
                                                            }
                                                        >
                                                            <span class="material-symbols-outlined">"close"</span>
                                                        </button>
                                                    </div>
                                                </div>
                                            })}
                                        })}
                                    }.into_any()
                                }
                            }
                        }))}
                    </Suspense>
                </div>
            })}
        </div>
    }
}
