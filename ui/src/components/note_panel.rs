use common::{
    id::NodeId,
    note::{CreateNoteRequest, UpdateNoteRequest},
};
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::markdown::render_markdown_plain;

/// Number of notes to show before collapsing the rest behind "Show N more".
const INITIAL_VISIBLE: usize = 5;

/// Post-it palette — 5 warm colours + neutral default.
/// Each entry: (key, swatch inline-style, full Tailwind card classes).
/// All card class strings are written out in full so Tailwind's scanner picks them up.
const PALETTE: &[(&str, &str, &str)] = &[
    ("default", "background:#e7e5e4",
     "bg-stone-50 dark:bg-stone-900/50 border-stone-200 dark:border-stone-700"),
    ("amber",   "background:#fef3c7",
     "bg-amber-100 dark:bg-amber-950/60 border-amber-300 dark:border-amber-800"),
    ("rose",    "background:#ffe4e6",
     "bg-rose-100 dark:bg-rose-950/60 border-rose-300 dark:border-rose-800"),
    ("lime",    "background:#dcfce7",
     "bg-lime-100 dark:bg-lime-950/60 border-lime-300 dark:border-lime-800"),
    ("sky",     "background:#e0f2fe",
     "bg-sky-100 dark:bg-sky-950/60 border-sky-300 dark:border-sky-800"),
    ("violet",  "background:#ede9fe",
     "bg-violet-100 dark:bg-violet-950/60 border-violet-300 dark:border-violet-800"),
];

fn palette_card_class(color: &str) -> &'static str {
    PALETTE.iter()
        .find(|(k, _, _)| *k == color)
        .map(|(_, _, cls)| *cls)
        .unwrap_or(PALETTE[0].2)
}

// ── ColorPicker ───────────────────────────────────────────────────────────────

#[component]
fn ColorPicker(selected: RwSignal<String>) -> impl IntoView {
    view! {
        <div class="flex items-center gap-1.5">
            {PALETTE.iter().map(|(key, swatch_style, _)| {
                let key_str  = key.to_string();
                let key_cmp  = key.to_string();
                let swatch   = swatch_style.to_string();
                view! {
                    <button
                        type="button"
                        title=key_str.clone()
                        style=format!("{swatch}; width:18px; height:18px; border-radius:50%; \
                            flex-shrink:0; transition:transform 0.1s;")
                        class=move || {
                            if selected.get() == key_cmp {
                                "ring-2 ring-offset-1 ring-amber-500 scale-110"
                            } else {
                                "ring-1 ring-stone-300 dark:ring-stone-600 hover:scale-110"
                            }
                        }
                        on:click={
                            let k = key_str.clone();
                            move |_| selected.set(k.clone())
                        }
                    />
                }
            }).collect_view()}
        </div>
    }
}

/// Pre-defined note structures — (label, Material Symbol icon name, markdown body).
const NOTE_TEMPLATES: &[(&str, &str, &str)] = &[
    ("meeting", "groups", "## Meeting Notes\n\n**Date:** \n**Attendees:** \n\n## Agenda\n\n1. \n\n## Decisions\n\n- \n\n## Action Items\n\n- [ ] "),
    ("decision", "gavel", "## Decision Record\n\n**Status:** Proposed\n**Date:** \n\n## Context\n\n\n\n## Decision\n\n\n\n## Consequences\n\n- "),
    ("status", "trending_up", "## Status Update\n\n**Period:** \n\n## Progress\n\n- \n\n## Blockers\n\n- \n\n## Next Steps\n\n- "),
    ("checklist", "checklist", "## Checklist\n\n- [ ] \n- [ ] \n- [ ] "),
    ("idea", "lightbulb", "## Idea\n\n**Problem:** \n\n**Proposed Solution:** \n\n**Open Questions:** \n\n- "),
];

// ── NotePanel ─────────────────────────────────────────────────────────────────

#[component]
pub fn NotePanel(node_id: NodeId, is_owner: bool) -> impl IntoView {
    let refresh   = RwSignal::new(0u32);
    let show_form = RwSignal::new(false);
    let body      = RwSignal::new(String::new());
    let new_color = RwSignal::new("default".to_string());
    let show_all  = RwSignal::new(false);
    let collapsed = RwSignal::new(false);

    // Arrived via a `/nodes/:id?note=<id>` deep-link from the Notes feed —
    // expand the panel and reveal all notes so the focused one is in the DOM
    // for `focus_note` to scroll to (it may be beyond the first 5).
    if crate::focus_note::pending_focus_note().is_some() {
        show_all.set(true);
        collapsed.set(false);
    }

    let notes_resource = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::fetch_notes(node_id).await }
    });

    let do_add = move || {
        let text = body.get_untracked();
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let req = CreateNoteRequest {
            body: trimmed,
            color: new_color.get_untracked(),
            // Node comes from the URL path on the node-scoped POST; the body
            // node_id is ignored by that route.
            node_id: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            if crate::api::create_note(node_id, &req).await.is_ok() {
                body.set(String::new());
                new_color.set("default".to_string());
                show_form.set(false);
                refresh.update(|n| *n += 1);
            }
        });
    };

    let on_add     = move |_| do_add();
    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            do_add();
        }
    };

    view! {
        <div class="border-t border-stone-200 dark:border-stone-800 pt-6">
            // ── Section header ────────────────────────────────────────────
            <div class="flex items-center justify-between mb-3">
                <button
                    class="flex items-center gap-2 group"
                    title=move || if collapsed.get() { "Expand notes" } else { "Collapse notes" }
                    on:click=move |_| {
                        collapsed.update(|v| *v = !*v);
                        if collapsed.get_untracked() { show_form.set(false); }
                    }
                >
                    <span class="material-symbols-outlined text-stone-400 dark:text-stone-500
                        group-hover:text-amber-500 transition-colors"
                        style="font-size: 18px;">{"sticky_note_2"}</span>
                    <h3 class="text-sm font-semibold text-stone-700 dark:text-stone-300
                        uppercase tracking-wider group-hover:text-stone-900
                        dark:group-hover:text-stone-100 transition-colors">
                        "Notes"
                    </h3>
                    <span class="material-symbols-outlined text-stone-300 dark:text-stone-600
                        group-hover:text-stone-500 transition-colors"
                        style="font-size: 16px;">
                        {move || if collapsed.get() { "expand_more" } else { "expand_less" }}
                    </span>
                </button>

                {move || (is_owner && !collapsed.get()).then(|| view! {
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                            hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                        title=move || if show_form.get() { "Cancel" } else { "Add note" }
                        on:click=move |_| show_form.update(|v| *v = !*v)
                    >
                        <span class="material-symbols-outlined" style="font-size: 18px;">
                            {move || if show_form.get() { "close" } else { "add" }}
                        </span>
                    </button>
                })}
            </div>

            // ── Collapsible body ──────────────────────────────────────────
            {move || (!collapsed.get()).then(|| view! {
                <div>
                    // Add note form (owner only, toggled)
                    {move || (is_owner && show_form.get()).then(|| view! {
                        <div class="mb-4 rounded-lg bg-stone-50 dark:bg-stone-900/50
                            border border-stone-200 dark:border-stone-700 p-3">
                            <textarea
                                class="w-full bg-transparent text-sm text-stone-800 dark:text-stone-200
                                    placeholder:text-stone-400 dark:placeholder:text-stone-600
                                    resize-none focus:outline-none"
                                rows="3"
                                placeholder="Write a note… (Ctrl+Enter to save)"
                                prop:value=move || body.get()
                                on:input=move |ev| body.set(event_target_value(&ev))
                                on:keydown=on_keydown
                            />
                            // ── Template picker ───────────────────
                            <div class="flex flex-wrap items-center gap-1.5 mt-2 mb-2">
                                <span class="text-[10px] uppercase tracking-wider text-stone-400
                                    dark:text-stone-500 mr-0.5">"Template"</span>
                                {NOTE_TEMPLATES.iter().map(|(label, icon, tmpl)| {
                                    let tmpl = *tmpl;
                                    let label = *label;
                                    let icon = *icon;
                                    view! {
                                        <button
                                            type="button"
                                            title=label
                                            class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px]
                                                text-stone-500 dark:text-stone-400 bg-stone-100 dark:bg-stone-800
                                                hover:bg-amber-100 dark:hover:bg-amber-900/40
                                                hover:text-amber-700 dark:hover:text-amber-300
                                                transition-colors cursor-pointer"
                                            on:click=move |_| body.set(tmpl.to_string())
                                        >
                                            <span class="material-symbols-outlined" style="font-size: 14px;">{icon}</span>
                                            {label}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>

                            <div class="flex items-center justify-between">
                                <ColorPicker selected=new_color />
                                <button
                                    class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                                        hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors cursor-pointer
                                        disabled:opacity-40 disabled:cursor-not-allowed"
                                    title="Save note"
                                    disabled=move || body.get().trim().is_empty()
                                    on:click=on_add
                                >
                                    <span class="material-symbols-outlined">"check"</span>
                                </button>
                            </div>
                        </div>
                    })}

                    // Notes list
                    <Suspense fallback=move || view! {
                        <p class="text-xs text-stone-400">"Loading…"</p>
                    }>
                        {move || {
                            let notes = notes_resource.get()
                                .and_then(|r| r.ok())
                                .unwrap_or_default();

                            if notes.is_empty() {
                                return view! {
                                    <p class="text-xs text-stone-400 dark:text-stone-600 italic py-2">
                                        "No notes yet."
                                    </p>
                                }.into_any();
                            }

                            let total   = notes.len();
                            let hidden  = total.saturating_sub(INITIAL_VISIBLE);
                            let expanded = show_all.get();

                            let visible: Vec<_> = if expanded {
                                notes
                            } else {
                                notes.into_iter().take(INITIAL_VISIBLE).collect()
                            };

                            view! {
                                <div>
                                    <div class=move || {
                                        if show_all.get() && total > INITIAL_VISIBLE {
                                            "max-h-[480px] overflow-y-auto pr-1"
                                        } else { "" }
                                    }>
                                        <div class="space-y-3">
                                            {visible.into_iter().map(|note| {
                                                let note_id    = note.id;
                                                let note_color = note.color.clone();

                                                let edited = (note.updated_at - note.created_at).num_seconds() > 2;
                                                let ts = {
                                                    let base = note.created_at
                                                        .format("%b %-d, %Y %H:%M").to_string();
                                                    if edited { format!("{base} · edited") } else { base }
                                                };

                                                let editing    = RwSignal::new(false);
                                                let edit_body  = RwSignal::new(note.body.clone());
                                                let edit_color = RwSignal::new(note.color.clone());
                                                let orig_body  = RwSignal::new(note.body.clone());
                                                let orig_color = RwSignal::new(note.color.clone());
                                                let save_error = RwSignal::new(Option::<String>::None);

                                                // Pre-render Markdown (computed once per note in this iteration)
                                                let body_html  = render_markdown_plain(&note.body);
                                                let ts_display = ts.clone();
                                                let card_class = palette_card_class(&note_color).to_string();

                                                view! {
                                                    <div
                                                        class=format!("rounded-lg border px-3 py-2.5 {card_class}")
                                                        data-note-id=note_id.0.to_string()
                                                    >

                                                        // ── Display mode ──────────────────────
                                                        {move || (!editing.get()).then({
                                                            let bh = body_html.clone();
                                                            let td = ts_display.clone();
                                                            move || view! {
                                                                <div
                                                                    class="prose prose-sm max-w-none dark:prose-invert
                                                                        prose-p:my-0.5 prose-ul:my-0.5 prose-ol:my-0.5
                                                                        prose-li:my-0 prose-headings:mt-1 prose-headings:mb-0.5"
                                                                    inner_html=bh.clone()
                                                                />
                                                                <div class="flex items-center justify-between mt-1.5">
                                                                    <p class="text-xs text-stone-400 dark:text-stone-500">
                                                                        {td.clone()}
                                                                    </p>
                                                                    {is_owner.then(|| view! {
                                                                        <button
                                                                            class="p-0.5 rounded
                                                                                text-stone-300 dark:text-stone-600
                                                                                hover:text-amber-500 dark:hover:text-amber-400
                                                                                transition-colors"
                                                                            title="Edit note"
                                                                            on:click=move |_| editing.set(true)
                                                                        >
                                                                            <span class="material-symbols-outlined"
                                                                                style="font-size: 14px;">{"edit"}</span>
                                                                        </button>
                                                                    })}
                                                                </div>
                                                            }
                                                        })}

                                                        // ── Edit mode ─────────────────────────
                                                        {move || editing.get().then(move || view! {
                                                            <textarea
                                                                class="w-full bg-transparent text-sm
                                                                    text-stone-800 dark:text-stone-200
                                                                    resize-none focus:outline-none"
                                                                rows="4"
                                                                prop:value=move || edit_body.get()
                                                                on:input=move |ev| edit_body.set(event_target_value(&ev))
                                                                on:keydown=move |ev: web_sys::KeyboardEvent| {
                                                                    if ev.key() == "Escape" {
                                                                        editing.set(false);
                                                                        edit_body.set(orig_body.get_untracked());
                                                                        edit_color.set(orig_color.get_untracked());
                                                                    }
                                                                    if ev.key() == "Enter"
                                                                        && (ev.ctrl_key() || ev.meta_key())
                                                                    {
                                                                        let new_body =
                                                                            edit_body.get_untracked().trim().to_string();
                                                                        if new_body.is_empty() { return; }
                                                                        let req = UpdateNoteRequest {
                                                                            body: new_body,
                                                                            color: edit_color.get_untracked(),
                                                                        };
                                                                        wasm_bindgen_futures::spawn_local(async move {
                                                                            match crate::api::update_note(note_id, &req).await {
                                                                                Ok(_) => {
                                                                                    editing.set(false);
                                                                                    save_error.set(None);
                                                                                    refresh.update(|n| *n += 1);
                                                                                }
                                                                                Err(e) => {
                                                                                    save_error.set(Some(format!("{e}")));
                                                                                }
                                                                            }
                                                                        });
                                                                    }
                                                                }
                                                            />
                                                            {move || save_error.get().map(|msg| view! {
                                                                <p class="text-xs text-red-500 mt-1">{msg}</p>
                                                            })}
                                                            <div class="flex items-center justify-between mt-2">
                                                                <ColorPicker selected=edit_color />
                                                                <div class="flex items-center gap-1">
                                                                    <IconButton
                                                                        icon="close"
                                                                        label="Cancel"
                                                                        on_click=Callback::new(move |()| {
                                                                            editing.set(false);
                                                                            edit_body.set(orig_body.get_untracked());
                                                                            edit_color.set(orig_color.get_untracked());
                                                                        })
                                                                    />
                                                                    <IconButton
                                                                        icon="check"
                                                                        label="Save"
                                                                        variant=IconButtonVariant::Save
                                                                        disabled=Signal::derive(move || edit_body.get().trim().is_empty())
                                                                        on_click=Callback::new(move |()| {
                                                                            let new_body =
                                                                                edit_body.get_untracked().trim().to_string();
                                                                            if new_body.is_empty() { return; }
                                                                            let req = UpdateNoteRequest {
                                                                                body: new_body,
                                                                                color: edit_color.get_untracked(),
                                                                            };
                                                                            wasm_bindgen_futures::spawn_local(async move {
                                                                                match crate::api::update_note(
                                                                                    note_id, &req,
                                                                                ).await {
                                                                                    Ok(_) => {
                                                                                        editing.set(false);
                                                                                        save_error.set(None);
                                                                                        refresh.update(|n| *n += 1);
                                                                                    }
                                                                                    Err(e) => {
                                                                                        save_error.set(Some(format!("{e}")));
                                                                                    }
                                                                                }
                                                                            });
                                                                        })
                                                                    />
                                                                </div>
                                                            </div>
                                                        })}
                                                    </div>
                                                }
                                            }).collect_view()}
                                        </div>
                                    </div>

                                    // "Show N more" / "Show less" toggle
                                    {(hidden > 0 || expanded).then(|| view! {
                                        <button
                                            class="mt-3 text-xs text-stone-400 dark:text-stone-500
                                                hover:text-amber-600 dark:hover:text-amber-400
                                                transition-colors cursor-pointer"
                                            on:click=move |_| show_all.update(|v| *v = !*v)
                                        >
                                            {move || {
                                                if show_all.get() {
                                                    "Show less".to_string()
                                                } else {
                                                    format!("Show {} more…", hidden)
                                                }
                                            }}
                                        </button>
                                    })}
                                </div>
                            }.into_any()
                        }}
                    </Suspense>
                </div>
            })}
        </div>
    }
}
