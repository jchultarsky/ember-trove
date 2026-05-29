use common::tag::{CreateTagRequest, Tag, UpdateTagRequest};
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::modals::delete_confirm::DeleteConfirmModal;
use leptos_router::hooks::use_navigate;
use crate::components::toast::{ToastLevel, push_toast};

// ── Palette ───────────────────────────────────────────────────────────────────

/// Curated palette of tag colours — balanced, dark-mode-friendly hues.
const PALETTE: &[(&str, &str)] = &[
    // Ambers / oranges
    ("#d97706", "Amber"),
    ("#ea580c", "Orange"),
    ("#dc2626", "Red"),
    ("#e11d48", "Rose"),
    // Purples / pinks
    ("#9333ea", "Purple"),
    ("#7c3aed", "Violet"),
    ("#db2777", "Pink"),
    ("#be185d", "Deep pink"),
    // Blues / cyans
    ("#2563eb", "Blue"),
    ("#0284c7", "Sky"),
    ("#0891b2", "Cyan"),
    ("#0d9488", "Teal"),
    // Greens
    ("#16a34a", "Green"),
    ("#65a30d", "Lime"),
    // Neutrals
    ("#64748b", "Slate"),
    ("#78716c", "Stone"),
];

// ── Colour picker sub-component ───────────────────────────────────────────────

/// Renders a row of palette swatches above the native `<input type="color">`.
/// Clicking a swatch sets the colour immediately; the native picker is the
/// escape hatch for custom hues.
#[component]
fn ColorPicker(
    /// The currently selected hex colour (e.g. `"#d97706"`).
    value: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-1.5">
            // Swatch grid
            <div class="flex flex-wrap gap-1.5">
                {PALETTE.iter().map(|&(hex, label)| {
                    view! {
                        <button
                            type="button"
                            title=label
                            class=move || {
                                let selected = value.get() == hex;
                                let base = "w-5 h-5 rounded-full transition-transform \
                                            hover:scale-110 focus:outline-none focus:ring-2 \
                                            focus:ring-offset-1 focus:ring-white/60";
                                if selected {
                                    // Slightly larger + ring when active
                                    format!("{base} ring-2 ring-white/80 scale-110")
                                } else {
                                    base.to_string()
                                }
                            }
                            style=format!("background-color: {hex};")
                            on:click=move |_| value.set(hex.to_string())
                        />
                    }
                }).collect::<Vec<_>>()}
            </div>
            // Native colour picker as escape hatch for custom hues
            <div class="flex items-center gap-2">
                <input
                    type="color"
                    class="w-7 h-7 rounded cursor-pointer border border-stone-300
                           dark:border-stone-600 bg-transparent p-0.5"
                    prop:value=move || value.get()
                    on:input=move |ev| value.set(event_target_value(&ev))
                    title="Pick a custom colour"
                />
                <span class="text-xs text-stone-400 dark:text-stone-500 font-mono">
                    {move || value.get()}
                </span>
            </div>
        </div>
    }
}

// ── TagManager ────────────────────────────────────────────────────────────────

/// Tag browser + manager.
///
/// Primary purpose: list all tags, filterable by a search box. Clicking a tag
/// navigates to the node list filtered by that tag (browse mode).
///
/// Secondary: create new tags, edit name/colour, delete — all as hover actions
/// so the browse experience stays front-and-centre.
#[component]
pub fn TagManager() -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let tag_filter_ctx =
        use_context::<RwSignal<Option<Tag>>>().expect("tag_filter signal must be provided");

    let refresh = RwSignal::new(0u32);
    let search_q = RwSignal::new(String::new());

    // Create form
    let new_name = RwSignal::new(String::new());
    let new_color = RwSignal::new("#d97706".to_string());
    let show_create = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);

    // Edit state
    let editing_id = RwSignal::new(Option::<common::id::TagId>::None);
    let edit_name = RwSignal::new(String::new());
    let edit_color = RwSignal::new(String::new());

    // Delete confirmation
    let delete_confirm_id: RwSignal<Option<common::id::TagId>> = RwSignal::new(None);
    let delete_confirm_name = RwSignal::new(String::new());

    let tags = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::fetch_tags().await }
    });

    let do_create = move || {
        let name = new_name.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        let color = new_color.get_untracked();
        error_msg.set(None);

        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateTagRequest { name: name.clone(), color };
            match crate::api::create_tag(&req).await {
                Ok(_) => {
                    new_name.set(String::new());
                    new_color.set("#d97706".to_string());
                    show_create.set(false);
                    refresh.update(|n| *n += 1);
                    push_toast(ToastLevel::Success, format!("Tag \"{name}\" created."));
                }
                Err(e) => error_msg.set(Some(format!("{e}"))),
            }
        });
    };

    let on_save_edit = move || {
        let Some(id) = editing_id.get_untracked() else {
            return;
        };
        let name = edit_name.get_untracked().trim().to_string();
        let color = edit_color.get_untracked();
        error_msg.set(None);

        wasm_bindgen_futures::spawn_local(async move {
            let req = UpdateTagRequest {
                name: if name.is_empty() { None } else { Some(name) },
                color: Some(color),
            };
            match crate::api::update_tag(id, &req).await {
                Ok(_) => {
                    editing_id.set(None);
                    refresh.update(|n| *n += 1);
                    push_toast(ToastLevel::Success, "Tag updated.");
                }
                Err(e) => error_msg.set(Some(format!("{e}"))),
            }
        });
    };

    view! {
        <div class="flex flex-col h-full">

            // ── Header ─────────────────────────────────────────────────────────
            <div class="flex items-center justify-between px-6 py-4
                        border-b border-stone-200 dark:border-stone-800">
                <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">"Tags"</h1>
                <button
                    class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                        hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                    title=move || if show_create.get() { "Cancel" } else { "New Tag" }
                    on:click=move |_| show_create.update(|v| *v = !*v)
                >
                    <span class="material-symbols-outlined" style="font-size: 18px;">
                        {move || if show_create.get() { "close" } else { "add" }}
                    </span>
                </button>
            </div>

            // ── Create form (collapsible) ───────────────────────────────────────
            {move || show_create.get().then(|| view! {
                <div class="px-6 py-4 border-b border-stone-200 dark:border-stone-800
                            bg-stone-50 dark:bg-stone-900/50 space-y-3">
                    // Name row
                    <div class="flex items-center gap-2">
                        // Preview dot — reflects selected colour instantly
                        <span
                            class="w-3.5 h-3.5 rounded-full flex-shrink-0 transition-colors"
                            style=move || format!("background-color: {};", new_color.get())
                        />
                        <input
                            type="text"
                            autofocus
                            class="flex-1 px-3 py-1.5 text-sm rounded-lg
                                   border border-stone-300 dark:border-stone-600
                                   bg-white dark:bg-stone-800
                                   text-stone-900 dark:text-stone-100
                                   focus:outline-none focus:ring-2 focus:ring-amber-500
                                   placeholder-stone-400 dark:placeholder-stone-500"
                            placeholder="Tag name…"
                            prop:value=move || new_name.get()
                            on:input=move |ev| new_name.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Enter" { ev.prevent_default(); do_create(); }
                            }
                        />
                        <button
                            class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                                   hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer"
                            title="Create tag"
                            on:click=move |_| do_create()
                        >
                            <span class="material-symbols-outlined">"check"</span>
                        </button>
                    </div>
                    // Colour picker (palette + native escape hatch)
                    <ColorPicker value=new_color />
                    {move || error_msg.get().map(|msg| view! {
                        <p class="text-xs text-red-500">{msg}</p>
                    })}
                </div>
            })}

            // ── Search bar ─────────────────────────────────────────────────────
            <div class="px-6 py-3 border-b border-stone-200 dark:border-stone-800">
                <div class="relative">
                    <span
                        class="absolute left-2.5 top-1/2 -translate-y-1/2
                               material-symbols-outlined text-stone-400 dark:text-stone-500
                               pointer-events-none"
                        style="font-size: 16px;"
                    >
                        "search"
                    </span>
                    <input
                        type="text"
                        class="w-full pl-8 pr-3 py-1.5 text-sm rounded-lg
                               border border-stone-200 dark:border-stone-700
                               bg-stone-50 dark:bg-stone-800
                               text-stone-900 dark:text-stone-100
                               focus:outline-none focus:ring-2 focus:ring-amber-500
                               placeholder-stone-400 dark:placeholder-stone-500
                               transition-colors"
                        placeholder="Filter tags…"
                        prop:value=move || search_q.get()
                        on:input=move |ev| search_q.set(event_target_value(&ev))
                    />
                </div>
            </div>

            // ── Tag list ───────────────────────────────────────────────────────
            <div class="flex-1 overflow-auto">
                <Suspense fallback=|| view! {
                    <div class="divide-y divide-stone-100 dark:divide-stone-800">
                        {(0..5).map(|_| view! {
                            <div class="flex items-center gap-3 px-6 py-3">
                                <div class="w-3.5 h-3.5 rounded-full bg-stone-200
                                            dark:bg-stone-700 animate-pulse flex-shrink-0" />
                                <div class="h-3.5 rounded bg-stone-200 dark:bg-stone-700
                                            animate-pulse w-28" />
                            </div>
                        }).collect::<Vec<_>>()}
                    </div>
                }>
                    {move || {
                        tags.get().map(|result| {
                            match result {
                                Ok(tag_list) if tag_list.is_empty() => view! {
                                    <div class="flex flex-col items-center justify-center
                                                gap-3 py-16">
                                        <span
                                            class="material-symbols-outlined
                                                   text-stone-300 dark:text-stone-700"
                                            style="font-size: 48px;"
                                        >
                                            "label_off"
                                        </span>
                                        <p class="text-sm text-stone-400 dark:text-stone-600">
                                            "No tags yet. Create one above."
                                        </p>
                                    </div>
                                }.into_any(),

                                Ok(tag_list) => {
                                    let q = search_q.get().to_lowercase();
                                    let filtered: Vec<Tag> = if q.is_empty() {
                                        tag_list
                                    } else {
                                        tag_list.into_iter()
                                            .filter(|t| t.name.to_lowercase().contains(&q))
                                            .collect()
                                    };

                                    if filtered.is_empty() {
                                        return view! {
                                            <div class="flex flex-col items-center
                                                        justify-center gap-2 py-12">
                                                <span
                                                    class="material-symbols-outlined
                                                           text-stone-300 dark:text-stone-700"
                                                    style="font-size: 36px;"
                                                >
                                                    "search_off"
                                                </span>
                                                <p class="text-sm text-stone-400
                                                          dark:text-stone-600">
                                                    "No tags match."
                                                </p>
                                            </div>
                                        }.into_any();
                                    }

                                    view! {
                                        <div class="divide-y divide-stone-100 dark:divide-stone-800">
                                            {filtered.into_iter().map(|tag| {
                                                let tag_id    = tag.id;
                                                let name      = tag.name.clone();
                                                let color     = tag.color.clone();
                                                let tag_clone = tag.clone();

                                                view! {
                                                    <div class="px-6 py-3
                                                                hover:bg-stone-50
                                                                dark:hover:bg-stone-900/50 group">
                                                        {move || {
                                                            if editing_id.get() == Some(tag_id) {
                                                                // ── Edit mode ──────────────────
                                                                let row_color = RwSignal::new(edit_color.get_untracked());
                                                                // Keep shared edit_color in sync
                                                                // when the local swatch changes.
                                                                Effect::new(move |_| {
                                                                    edit_color.set(row_color.get());
                                                                });
                                                                view! {
                                                                    <div class="space-y-2">
                                                                        // Name + save/cancel row
                                                                        <div class="flex items-center gap-2">
                                                                            <span
                                                                                class="w-3.5 h-3.5 rounded-full flex-shrink-0 transition-colors"
                                                                                style=move || format!("background-color: {};", row_color.get())
                                                                            />
                                                                            <input
                                                                                type="text"
                                                                                class="flex-1 px-2 py-1 text-sm rounded
                                                                                       border border-stone-300
                                                                                       dark:border-stone-600
                                                                                       bg-transparent
                                                                                       text-stone-900 dark:text-stone-100
                                                                                       focus:outline-none focus:ring-1
                                                                                       focus:ring-amber-500"
                                                                                prop:value=move || edit_name.get()
                                                                                on:input=move |ev| edit_name.set(event_target_value(&ev))
                                                                            />
                                                                            <IconButton
                                                                                icon="check"
                                                                                label="Save"
                                                                                variant=IconButtonVariant::Save
                                                                                on_click=Callback::new(move |()| on_save_edit())
                                                                            />
                                                                            <IconButton
                                                                                icon="close"
                                                                                label="Cancel"
                                                                                on_click=Callback::new(move |()| editing_id.set(None))
                                                                            />
                                                                        </div>
                                                                        // Colour picker
                                                                        <ColorPicker value=row_color />
                                                                    </div>
                                                                }.into_any()
                                                            } else {
                                                                // ── Browse / display mode ───────
                                                                let display_name  = name.clone();
                                                                let display_color = color.clone();
                                                                let edit_name_val = name.clone();
                                                                let edit_color_val= color.clone();
                                                                let del_name      = name.clone();
                                                                let browse_tag    = tag_clone.clone();
                                                                view! {
                                                                    <div class="flex items-center justify-between">
                                                                        // Click whole row → browse
                                                                        <button
                                                                            class="flex items-center gap-3 flex-1 text-left min-w-0"
                                                                            on:click=move |_| {
                                                                                tag_filter_ctx.set(Some(browse_tag.clone()));
                                                                                navigate.get_value()("/nodes", Default::default());
                                                                            }
                                                                            title="Browse nodes with this tag"
                                                                        >
                                                                            <span
                                                                                class="w-3.5 h-3.5 rounded-full flex-shrink-0"
                                                                                style:background-color=display_color
                                                                            />
                                                                            <span class="text-sm text-stone-900
                                                                                        dark:text-stone-100 truncate">
                                                                                {display_name}
                                                                            </span>
                                                                            <span
                                                                                class="material-symbols-outlined
                                                                                       text-stone-300 dark:text-stone-600
                                                                                       opacity-0 group-hover:opacity-100
                                                                                       transition-opacity ml-auto flex-shrink-0"
                                                                                style="font-size: 15px;"
                                                                            >
                                                                                "arrow_forward"
                                                                            </span>
                                                                        </button>
                                                                        // Edit / delete — hover only
                                                                        <div class="flex items-center gap-1 ml-2
                                                                                    opacity-0 group-hover:opacity-100
                                                                                    transition-opacity flex-shrink-0">
                                                                            <button
                                                                                class="p-1 rounded text-stone-400
                                                                                       hover:text-stone-600
                                                                                       dark:hover:text-stone-300
                                                                                       hover:bg-stone-100
                                                                                       dark:hover:bg-stone-800
                                                                                       transition-colors"
                                                                                title="Edit tag"
                                                                                on:click=move |ev| {
                                                                                    ev.stop_propagation();
                                                                                    editing_id.set(Some(tag_id));
                                                                                    edit_name.set(edit_name_val.clone());
                                                                                    edit_color.set(edit_color_val.clone());
                                                                                }
                                                                            >
                                                                                <span class="material-symbols-outlined"
                                                                                      style="font-size: 15px;">"edit"</span>
                                                                            </button>
                                                                            <button
                                                                                class="p-1 rounded text-stone-400
                                                                                       hover:text-red-500
                                                                                       hover:bg-red-50
                                                                                       dark:hover:bg-red-900/20
                                                                                       transition-colors"
                                                                                title="Delete tag"
                                                                                on:click=move |ev| {
                                                                                    ev.stop_propagation();
                                                                                    delete_confirm_id.set(Some(tag_id));
                                                                                    delete_confirm_name.set(del_name.clone());
                                                                                }
                                                                            >
                                                                                <span class="material-symbols-outlined"
                                                                                      style="font-size: 15px;">"delete"</span>
                                                                            </button>
                                                                        </div>
                                                                    </div>
                                                                }.into_any()
                                                            }
                                                        }}
                                                    </div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                }

                                Err(e) => view! {
                                    <div class="p-6 text-red-500 text-sm">
                                        {format!("Error loading tags: {e}")}
                                    </div>
                                }.into_any(),
                            }
                        })
                    }}
                </Suspense>
            </div>
        </div>

        // Delete confirmation modal
        <DeleteConfirmModal
            show=Signal::derive(move || delete_confirm_id.get().is_some())
            item_name=Signal::derive(move || delete_confirm_name.get())
            on_confirm=Callback::new(move |_| {
                let Some(id) = delete_confirm_id.get_untracked() else { return; };
                let name = delete_confirm_name.get_untracked();
                delete_confirm_id.set(None);
                wasm_bindgen_futures::spawn_local(async move {
                    match crate::api::delete_tag(id).await {
                        Ok(_) => {
                            push_toast(ToastLevel::Success, format!("Tag \"{name}\" deleted."));
                            refresh.update(|n| *n += 1);
                        }
                        Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
                    }
                });
            })
            on_cancel=Callback::new(move |_| delete_confirm_id.set(None))
        />
    }
}
