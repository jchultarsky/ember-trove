use common::id::NodeId;
use common::tag::{CreateTagRequest, Tag};
use leptos::prelude::*;

use leptos_router::hooks::use_navigate;

/// Tag chips for a node — shows attached tags with remove buttons, plus an
/// inline input for attaching existing or creating new tags.
/// Each chip has a small funnel icon: clicking it sets the global tag_filter
/// and navigates to the NodeList so the user sees all nodes with that tag.
#[component]
pub fn TagBar(node_id: NodeId) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let tag_filter = use_context::<RwSignal<Option<Tag>>>().unwrap_or_else(|| RwSignal::new(None));
    let refresh_tags = RwSignal::new(0u32);
    let input_value = RwSignal::new(String::new());
    let show_input = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);

    // Tags attached to this node
    let node_tags = LocalResource::new(move || {
        let _ = refresh_tags.get();
        let node_id = node_id;
        async move { crate::api::fetch_tags_for_node(node_id).await }
    });

    let on_add_tag = move || {
        let name = input_value.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        input_value.set(String::new());
        show_input.set(false);
        error_msg.set(None);

        wasm_bindgen_futures::spawn_local(async move {
            // Check if a tag with this name already exists among user's tags
            let existing = crate::api::fetch_tags().await.unwrap_or_default();
            let tag = existing.iter().find(|t| t.name.eq_ignore_ascii_case(&name));

            let tag_id = if let Some(t) = tag {
                t.id
            } else {
                // Create new tag
                let req = CreateTagRequest {
                    name,
                    color: "#d97706".to_string(),
                };
                match crate::api::create_tag(&req).await {
                    Ok(t) => t.id,
                    Err(e) => {
                        error_msg.set(Some(format!("{e}")));
                        return;
                    }
                }
            };

            // Attach to node
            if let Err(e) = crate::api::attach_tag(node_id, tag_id).await {
                error_msg.set(Some(format!("{e}")));
            }
            refresh_tags.update(|n| *n += 1);
        });
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" {
            ev.prevent_default();
            on_add_tag();
        } else if ev.key() == "Escape" {
            show_input.set(false);
            input_value.set(String::new());
        }
    };

    view! {
        <div class="flex flex-wrap items-center gap-1.5 px-4 py-2 relative">
            <Suspense fallback=|| ()>
                {move || {
                    node_tags.get().map(|result| {
                        match result {
                            Ok(tags) if tags.is_empty() && !show_input.get_untracked() => {
                                view! {
                                    <span class="text-xs text-stone-400 dark:text-stone-600">"No tags"</span>
                                }.into_any()
                            }
                            Ok(tags) => {
                                view! {
                                    {tags.into_iter().map(|tag| {
                                        let tag_for_filter = tag.clone();
                                        let tag_id = tag.id;
                                        let color = tag.color.clone();
                                        view! {
                                            <span
                                                class="inline-flex items-center gap-0.5 px-2 py-0.5 rounded-full text-xs font-medium text-white"
                                                style:background-color=color
                                            >
                                                // Filter-by-tag button (funnel icon)
                                                <button
                                                    class="hover:opacity-70 mr-0.5"
                                                    title="Filter nodes by this tag"
                                                    on:click=move |_| {
                                                        tag_filter.set(Some(tag_for_filter.clone()));
                                                        navigate.get_value()("/nodes", Default::default());
                                                    }
                                                >
                                                    <span class="material-symbols-outlined" style="font-size:11px;">"filter_list"</span>
                                                </button>
                                                {tag.name.clone()}
                                                // Detach button
                                                <button
                                                    class="hover:opacity-70 ml-0.5"
                                                    title="Remove tag"
                                                    on:click=move |_| {
                                                        let node_id = node_id;
                                                        wasm_bindgen_futures::spawn_local(async move {
                                                            match crate::api::detach_tag(node_id, tag_id).await {
                                                                Ok(_) => refresh_tags.update(|n| *n += 1),
                                                                Err(e) => crate::components::toast::push_toast(
                                                                    crate::components::toast::ToastLevel::Error,
                                                                    format!("Couldn't remove tag: {e}"),
                                                                ),
                                                            }
                                                        });
                                                    }
                                                >
                                                    "\u{00d7}"
                                                </button>
                                            </span>
                                        }
                                    }).collect::<Vec<_>>()}
                                }.into_any()
                            }
                            Err(_) => ().into_any(),
                        }
                    })
                }}
            </Suspense>

            // Add tag button / inline input
            {move || {
                if show_input.get() {
                    view! {
                        <input
                            type="text"
                            class="w-24 px-2 py-0.5 text-xs rounded-full border border-stone-300 dark:border-stone-600
                                bg-transparent text-stone-900 dark:text-stone-100 focus:outline-none
                                focus:ring-1 focus:ring-amber-500"
                            placeholder="Tag name..."
                            prop:value=move || input_value.get()
                            on:input=move |ev| input_value.set(event_target_value(&ev))
                            on:keydown=on_keydown
                            on:blur=move |_| {
                                if input_value.get_untracked().is_empty() {
                                    show_input.set(false);
                                }
                            }
                        />
                    }.into_any()
                } else {
                    view! {
                        <button
                            class="inline-flex items-center px-2 py-0.5 rounded-full text-xs
                                text-stone-500 dark:text-stone-400 border border-dashed
                                border-stone-300 dark:border-stone-600 hover:border-stone-400
                                dark:hover:border-stone-500 transition-colors"
                            on:click=move |_| show_input.set(true)
                        >
                            "+ Tag"
                        </button>
                    }.into_any()
                }
            }}

            // Error message
            {move || error_msg.get().map(|msg| view! {
                <span class="text-xs text-red-500">{msg}</span>
            })}
        </div>
    }
}
