use leptos::prelude::*;

use common::{id::NodeId, note::CreateNoteRequest};
use leptos_router::hooks::use_navigate;

use crate::markdown::render_markdown_plain;

/// Mirror of note_panel::PALETTE — full class strings so Tailwind's scanner picks them up.
const PALETTE: &[(&str, &str)] = &[
    ("default", "bg-stone-50 dark:bg-stone-900/50 border-stone-200 dark:border-stone-700"),
    ("amber",   "bg-amber-100 dark:bg-amber-950/60 border-amber-300 dark:border-amber-800"),
    ("rose",    "bg-rose-100 dark:bg-rose-950/60 border-rose-300 dark:border-rose-800"),
    ("lime",    "bg-lime-100 dark:bg-lime-950/60 border-lime-300 dark:border-lime-800"),
    ("sky",     "bg-sky-100 dark:bg-sky-950/60 border-sky-300 dark:border-sky-800"),
    ("violet",  "bg-violet-100 dark:bg-violet-950/60 border-violet-300 dark:border-violet-800"),
];

fn palette_card_class(color: &str) -> &'static str {
    PALETTE.iter()
        .find(|(k, _)| *k == color)
        .map(|(_, cls)| *cls)
        .unwrap_or(PALETTE[0].1)
}

#[component]
pub fn NotesView() -> impl IntoView {
    let navigate = use_navigate();

    // Feed reload counter — bumped after posting a new note (the project rule:
    // re-fetch a LocalResource via a counter signal, never inside a closure).
    let reload = RwSignal::new(0u32);
    let feed_resource = LocalResource::new(move || {
        let _ = reload.get();
        async move { crate::api::fetch_notes_feed().await }
    });

    // Node list for the optional compose-box node picker.
    let node_titles = LocalResource::new(move || async move {
        crate::api::fetch_node_titles().await.unwrap_or_default()
    });

    // ── Compose box state ──────────────────────────────────────────────────
    let body = RwSignal::new(String::new());
    let selected_node = RwSignal::<Option<NodeId>>::new(None);
    let posting = RwSignal::new(false);
    let error = RwSignal::<Option<String>>::new(None);

    let do_post = move || {
        let text = body.get_untracked().trim().to_string();
        if text.is_empty() || posting.get_untracked() {
            return;
        }
        posting.set(true);
        error.set(None);
        let node_id = selected_node.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateNoteRequest {
                body: text,
                color: "default".to_string(),
                node_id,
            };
            match crate::api::create_note_global(&req).await {
                Ok(_) => {
                    body.set(String::new());
                    selected_node.set(None);
                    reload.update(|n| *n += 1);
                }
                Err(e) => error.set(Some(e.to_string())),
            }
            posting.set(false);
        });
    };

    view! {
        <div class="flex flex-col h-full">
            // Header
            <div class="flex items-center gap-3 px-6 py-4 border-b border-stone-200 dark:border-stone-800">
                <span class="material-symbols-outlined text-amber-500" style="font-size: 22px;">
                    {"sticky_note_2"}
                </span>
                <h1 class="text-lg font-semibold text-stone-900 dark:text-stone-100">
                    "Notes"
                </h1>
            </div>

            // ── Compose box ──────────────────────────────────────────────
            <div class="px-6 py-4 border-b border-stone-200 dark:border-stone-800 space-y-2">
                <textarea
                    class="w-full px-3 py-2 rounded-lg border border-stone-200 dark:border-stone-700
                        bg-stone-50 dark:bg-stone-800 text-sm text-stone-800 dark:text-stone-200
                        placeholder-stone-400 dark:placeholder-stone-600 resize-y min-h-[64px]
                        focus:outline-none focus:ring-2 focus:ring-amber-500/40"
                    placeholder="Write a note…  (Ctrl+Enter to post)"
                    prop:value=move || body.get()
                    on:input=move |ev| body.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
                            ev.prevent_default();
                            do_post();
                        }
                    }
                />
                <div class="flex items-center gap-2">
                    <select
                        class="px-2 py-1.5 rounded-lg border border-stone-200 dark:border-stone-700
                            bg-stone-50 dark:bg-stone-800 text-sm text-stone-700 dark:text-stone-300
                            focus:outline-none focus:ring-2 focus:ring-amber-500/40 max-w-[16rem]"
                        prop:value=move || selected_node.get().map(|n| n.0.to_string()).unwrap_or_default()
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            selected_node.set(
                                uuid::Uuid::parse_str(&v).ok().map(NodeId)
                            );
                        }
                    >
                        <option value="">"No node (inbox)"</option>
                        {move || node_titles.get().map(|list| {
                            list.into_iter().map(|e| view! {
                                <option value=e.id.0.to_string()>{e.title}</option>
                            }).collect_view()
                        })}
                    </select>
                    <button
                        class="ml-auto px-3 py-1.5 rounded-lg bg-amber-600 text-white text-sm font-medium
                            hover:bg-amber-700 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                        disabled=move || posting.get() || body.get().trim().is_empty()
                        on:click=move |_| do_post()
                    >
                        {move || if posting.get() { "Posting…" } else { "Post" }}
                    </button>
                </div>
                {move || error.get().map(|e| view! {
                    <p class="text-red-500 text-xs">{format!("Error: {e}")}</p>
                })}
            </div>

            // Feed
            <div class="flex-1 overflow-auto p-6 flex flex-col">
                <Suspense fallback=move || view! {
                    <crate::components::skeleton::SkeletonList rows=6 />
                }>
                    {move || {
                        let notes = feed_resource.get()
                            .and_then(|r| r.ok())
                            .unwrap_or_default();

                        if notes.is_empty() {
                            return view! {
                                <div class="flex-1 flex flex-col items-center justify-center gap-3">
                                    <span class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                        style="font-size: 48px;">{"sticky_note_2"}</span>
                                    <p class="text-stone-400 dark:text-stone-500 text-sm text-center">
                                        "No notes yet."
                                    </p>
                                    <p class="text-stone-400 dark:text-stone-500 text-sm text-center">
                                        "Write a note above, or add one from a node."
                                    </p>
                                </div>
                            }.into_any();
                        }

                        view! {
                            <div class="space-y-4 w-full">
                                {notes.into_iter().map(|feed_note| {
                                    let node_id = feed_note.note.node_id;
                                    let node_title = feed_note.node_title.clone();
                                    let body_html = render_markdown_plain(&feed_note.note.body);
                                    let card_class = palette_card_class(&feed_note.note.color).to_string();
                                    let ts = feed_note.note.created_at
                                        .format("%b %-d, %Y %H:%M")
                                        .to_string();

                                    // Header: a node link for node-attached notes, an
                                    // "Inbox" pill for standalone notes.
                                    let header = match node_id {
                                        Some(nid) => {
                                            let nav = navigate.clone();
                                            let title = node_title.unwrap_or_default();
                                            view! {
                                                <button
                                                    class="flex items-center gap-1.5 mb-2 text-xs font-semibold
                                                        text-stone-400 dark:text-stone-500 uppercase tracking-wider
                                                        hover:text-amber-600 dark:hover:text-amber-400 transition-colors"
                                                    on:click=move |_| nav(&format!("/nodes/{nid}"), Default::default())
                                                >
                                                    <span class="material-symbols-outlined" style="font-size: 13px;">
                                                        {"description"}
                                                    </span>
                                                    {title}
                                                    <span class="material-symbols-outlined" style="font-size: 12px;">
                                                        {"open_in_new"}
                                                    </span>
                                                </button>
                                            }.into_any()
                                        }
                                        None => view! {
                                            <span class="inline-flex items-center gap-1.5 mb-2 text-xs font-semibold
                                                text-stone-400 dark:text-stone-500 uppercase tracking-wider">
                                                <span class="material-symbols-outlined" style="font-size: 13px;">
                                                    {"inbox"}
                                                </span>
                                                "Inbox"
                                            </span>
                                        }.into_any(),
                                    };

                                    view! {
                                        <div class=format!("rounded-lg border px-4 py-3 {card_class}")>
                                            {header}
                                            <div
                                                class="prose prose-sm max-w-none dark:prose-invert
                                                    prose-p:my-0.5 prose-ul:my-0.5 prose-ol:my-0.5
                                                    prose-li:my-0 prose-headings:mt-1 prose-headings:mb-0.5"
                                                inner_html=body_html
                                            />
                                            <p class="text-xs text-stone-400 dark:text-stone-600 mt-2">
                                                {ts}
                                            </p>
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_any()
                    }}
                </Suspense>
            </div>
        </div>
    }
}
