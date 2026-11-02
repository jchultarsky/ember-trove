use leptos::prelude::*;

use common::{
    id::{NodeId, NoteId},
    note::{CreateFeedNoteRequest, CreateNoteRequest, NoteSort},
};
use gloo_timers::callback::Timeout;
use leptos_router::hooks::use_navigate;

use crate::components::empty_state::EmptyState;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::node_picker::NodePicker;
use crate::components::page_header::PageHeader;
use crate::components::resizable_editor::ResizableEditor;
use crate::components::toast::{ToastAction, ToastLevel, ToastState};
use crate::markdown::render_markdown_plain;

/// Mirror of note_panel::PALETTE — full class strings so Tailwind's scanner picks them up.
const PALETTE: &[(&str, &str)] = &[
    (
        "default",
        "bg-stone-50 dark:bg-stone-900/50 border-stone-200 dark:border-stone-700",
    ),
    (
        "amber",
        "bg-amber-100 dark:bg-amber-950/60 border-amber-300 dark:border-amber-800",
    ),
    (
        "rose",
        "bg-rose-100 dark:bg-rose-950/60 border-rose-300 dark:border-rose-800",
    ),
    (
        "lime",
        "bg-lime-100 dark:bg-lime-950/60 border-lime-300 dark:border-lime-800",
    ),
    (
        "sky",
        "bg-sky-100 dark:bg-sky-950/60 border-sky-300 dark:border-sky-800",
    ),
    (
        "violet",
        "bg-violet-100 dark:bg-violet-950/60 border-violet-300 dark:border-violet-800",
    ),
];

fn palette_card_class(color: &str) -> &'static str {
    PALETTE
        .iter()
        .find(|(k, _)| *k == color)
        .map(|(_, cls)| *cls)
        .unwrap_or(PALETTE[0].1)
}

const INPUT_CLASS: &str = "px-2 py-1.5 rounded-lg border border-stone-200 dark:border-stone-700 \
    bg-stone-50 dark:bg-stone-800 text-sm text-stone-700 dark:text-stone-300 \
    focus:outline-none focus:ring-2 focus:ring-amber-500/40";

#[component]
pub fn NotesView() -> impl IntoView {
    let navigate = use_navigate();

    // Feed reload counter — bumped after posting (re-fetch a LocalResource via a
    // counter, never inside the closure).
    let reload = RwSignal::new(0u32);

    // ── Compose box state ──────────────────────────────────────────────────
    let body = RwSignal::new(String::new());
    let selected_node = RwSignal::<Option<(NodeId, String)>>::new(None);
    let posting = RwSignal::new(false);
    let error = RwSignal::<Option<String>>::new(None);
    let nav = expect_context::<crate::app::NavRequest>().0;

    // ── Delete state ────────────────────────────────────────────────────────
    // Instant delete + undo toast (the API soft-deletes, so the toast's Undo
    // can restore). Replaces the old inline are-you-sure confirmation.
    let deleting = RwSignal::new(false);
    // Captured at setup for the undo closure, which outlives the note card.
    let toast_state = use_context::<ToastState>();
    let do_delete = move |note_id: NoteId| {
        crate::note_actions::delete_note_with_undo(note_id, deleting, reload, toast_state);
    };

    // ── Filter / sort state ─────────────────────────────────────────────────
    let sort = RwSignal::new(NoteSort::Newest);
    // Node filter: "" = all, else a node UUID. Chosen as a scope ("all" /
    // "node") plus, for "node", the shared NodePicker — the dropdown used to
    // list every node title (#134).
    let node_filter = RwSignal::new(String::new());
    let filter_mode = RwSignal::new("all");
    let filter_node = RwSignal::new(None::<(NodeId, String)>);
    Effect::new(move |_| {
        let value = match filter_mode.get() {
            "node" => filter_node
                .get()
                .map(|(id, _)| id.0.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        node_filter.set(value);
    });
    let from_date = RwSignal::new(String::new());
    let to_date = RwSignal::new(String::new());
    // Text filter: `text_input` is bound to the box; `text_q` is the debounced
    // value the feed actually queries on (300 ms after typing stops).
    let text_input = RwSignal::new(String::new());
    let text_q = RwSignal::new(String::new());
    let debounce_v = RwSignal::new(0u32);
    Effect::new(move |_| {
        let val = text_input.get();
        let v = debounce_v.get_untracked() + 1;
        debounce_v.set(v);
        Timeout::new(300, move || {
            if debounce_v.get_untracked() == v {
                text_q.set(val.clone());
            }
        })
        .forget();
    });

    let any_filter_active = move || {
        sort.get() != NoteSort::Newest
            || !node_filter.get().is_empty()
            || !from_date.get().is_empty()
            || !to_date.get().is_empty()
            || !text_input.get().is_empty()
    };
    let reset_filters = move || {
        sort.set(NoteSort::Newest);
        filter_mode.set("all");
        filter_node.set(None);
        node_filter.set(String::new());
        from_date.set(String::new());
        to_date.set(String::new());
        text_input.set(String::new());
        text_q.set(String::new());
    };

    // ── Paged feed state ────────────────────────────────────────────────────
    // The feed accumulates pages: a fresh load (filters/reload change) replaces
    // the list; "Load more" appends the next page. `req_v` is a version guard so
    // a stale in-flight fetch (filters changed mid-request) can't clobber a newer
    // one — same pattern as the debounce above.
    let displayed = RwSignal::<Vec<common::note::FeedNote>>::new(Vec::new());
    let page = RwSignal::new(1u32);
    let has_more = RwSignal::new(false);
    let loading = RwSignal::new(true);
    let loading_more = RwSignal::new(false);
    let req_v = RwSignal::new(0u32);

    let parse_node_filter = |nf: &str| uuid::Uuid::parse_str(nf).ok().map(NodeId);

    // Fresh load whenever any filter (or the reload counter) changes.
    Effect::new(move |_| {
        let sort_v = sort.get();
        let nf = node_filter.get();
        let from = from_date.get();
        let to = to_date.get();
        let q = text_q.get();
        let _ = reload.get();

        let v = req_v.get_untracked() + 1;
        req_v.set(v);
        page.set(1);
        loading.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let res = crate::api::fetch_notes_feed(
                parse_node_filter(&nf),
                Some(from.as_str()),
                Some(to.as_str()),
                Some(q.as_str()),
                sort_v,
                1,
            )
            .await
            .unwrap_or_default();
            // Commit only if this is still the latest request.
            if req_v.get_untracked() == v {
                has_more.set(res.len() as u32 == crate::api::FEED_PAGE_SIZE);
                displayed.set(res);
                loading.set(false);
            }
        });
    });

    // Append the next page. Snapshots the current request generation so a
    // filter change mid-fetch drops the now-irrelevant appended results.
    let load_more = move || {
        if loading.get_untracked() || loading_more.get_untracked() || !has_more.get_untracked() {
            return;
        }
        loading_more.set(true);
        let v = req_v.get_untracked();
        let next = page.get_untracked() + 1;
        let sort_v = sort.get_untracked();
        let nf = node_filter.get_untracked();
        let from = from_date.get_untracked();
        let to = to_date.get_untracked();
        let q = text_q.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            let res = crate::api::fetch_notes_feed(
                parse_node_filter(&nf),
                Some(from.as_str()),
                Some(to.as_str()),
                Some(q.as_str()),
                sort_v,
                next,
            )
            .await
            .unwrap_or_default();
            if req_v.get_untracked() == v {
                let got = res.len() as u32;
                has_more.set(got == crate::api::FEED_PAGE_SIZE);
                displayed.update(|d| d.extend(res));
                page.set(next);
            }
            loading_more.set(false);
        });
    };

    let do_post = move || {
        let text = body.get_untracked().trim().to_string();
        if text.is_empty() || posting.get_untracked() {
            return;
        }
        // A note belongs to a node (§9.2): Post waits for one. Checked before
        // `posting` is set, or Ctrl+Enter without a node (the button's
        // `disabled` doesn't cover the keyboard) would leave it stuck.
        let Some((node_id, _)) = selected_node.get_untracked() else {
            error.set(Some(
                "Pick a node for this note, or capture it instead.".to_string(),
            ));
            return;
        };
        posting.set(true);
        error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateFeedNoteRequest {
                note: CreateNoteRequest {
                    body: text,
                    color: "default".to_string(),
                },
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

    // Text that belongs to no node yet is a capture (§9.1): hand it over
    // as typed, then clear the box (Muse's v3.5.0 friction item, #234: "press
    // n" opened the composer empty). On failure the text stays put. A node
    // already picked stays picked: only the text moved.
    let do_capture = move || {
        let text = body.get_untracked().trim().to_string();
        if text.is_empty() || posting.get_untracked() {
            return;
        }
        posting.set(true);
        error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::create_capture(&text).await {
                Ok(_) => {
                    body.try_set(String::new());
                    if let Some(ts) = toast_state {
                        ts.push_with_action(
                            ToastLevel::Success,
                            crate::components::captures_view::CAPTURED_TOAST,
                            ToastAction {
                                label: "Open",
                                on_click: std::sync::Arc::new(move || {
                                    nav.try_set(Some("/tasks/captures".to_string()));
                                }),
                            },
                        );
                    }
                }
                Err(e) => {
                    error.try_set(Some(e.to_string()));
                }
            }
            posting.try_set(false);
        });
    };

    view! {
        <div class="flex flex-col h-full">
            <PageHeader icon="sticky_note_2" title="Notes" />

            // ── Compose box ──────────────────────────────────────────────
            <div class="px-6 py-4 border-b border-stone-200 dark:border-stone-800 space-y-2">
                <ResizableEditor
                    value=body
                    placeholder="Write a note…  (Ctrl+Enter to post)"
                    on_submit=Callback::new(move |()| do_post())
                />
                <div class="flex items-center gap-2">
                    <div class="flex-1 min-w-0 max-w-[20rem]">
                        <NodePicker selected=selected_node placeholder="Note on a node\u{2026}" />
                    </div>
                    // Input that belongs nowhere yet is a capture (§9.1). A
                    // link inside the sentence, so word-only (leptos.md).
                    <span class="text-xs text-stone-400 dark:text-stone-500">
                        "No node yet? "
                        <button
                            type="button"
                            class="underline underline-offset-2 text-stone-600 dark:text-stone-300
                                hover:text-ember disabled:opacity-40 disabled:cursor-not-allowed
                                disabled:no-underline"
                            disabled=move || posting.get() || body.get().trim().is_empty()
                            on:click=move |_| do_capture()
                        >
                            "Capture it"
                        </button>
                    </span>
                    <button
                        class="inline-flex items-center justify-center gap-1.5 ml-auto px-3 py-1.5 rounded-lg bg-ember text-white text-sm font-medium
                            hover:bg-ember-strong transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                        disabled=move || {
                            posting.get() || body.get().trim().is_empty() || selected_node.with(Option::is_none)
                        }
                        on:click=move |_| do_post()
                    >
                        <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"send"</span>
                        {move || if posting.get() { "Posting…" } else { "Post" }}
                    </button>
                </div>
                {move || error.get().map(|e| view! {
                    <p class="text-red-500 text-xs">{format!("Error: {e}")}</p>
                })}
            </div>

            // ── Filter / sort toolbar ────────────────────────────────────
            <div class="px-6 py-2.5 border-b border-stone-200 dark:border-stone-800
                flex flex-wrap items-center gap-2">
                // Sort
                <select
                    class=INPUT_CLASS
                    title="Sort order"
                    prop:value=move || match sort.get() {
                        NoteSort::Newest => "newest",
                        NoteSort::Oldest => "oldest",
                        NoteSort::Updated => "updated",
                    }
                    on:change=move |ev| sort.set(match event_target_value(&ev).as_str() {
                        "oldest" => NoteSort::Oldest,
                        "updated" => NoteSort::Updated,
                        _ => NoteSort::Newest,
                    })
                >
                    <option value="newest">"Newest first"</option>
                    <option value="oldest">"Oldest first"</option>
                    <option value="updated">"Recently updated"</option>
                </select>
                // Node filter: a scope, plus the shared picker for one node.
                <select
                    class=INPUT_CLASS
                    title="Filter by node"
                    on:change=move |ev| {
                        let mode = match event_target_value(&ev).as_str() {
                            "node" => "node",
                            _ => "all",
                        };
                        filter_mode.set(mode);
                    }
                >
                    <option value="all" selected=move || filter_mode.get() == "all">"All notes"</option>
                    <option value="node" selected=move || filter_mode.get() == "node">"A node\u{2026}"</option>
                </select>
                {move || (filter_mode.get() == "node").then(|| view! {
                    <div class="min-w-[12rem] max-w-[16rem]" data-testid="notes-node-filter">
                        <NodePicker
                            selected=filter_node
                            placeholder="Filter by node\u{2026}"
                            autofocus=true
                        />
                    </div>
                })}
                // Date range
                <input
                    type="date" class=INPUT_CLASS title="From date (inclusive)"
                    prop:value=move || from_date.get()
                    on:input=move |ev| from_date.set(event_target_value(&ev))
                />
                <span class="text-stone-400 text-sm">"–"</span>
                <input
                    type="date" class=INPUT_CLASS title="To date (inclusive)"
                    prop:value=move || to_date.get()
                    on:input=move |ev| to_date.set(event_target_value(&ev))
                />
                // Text filter
                <input
                    type="text" class=format!("{INPUT_CLASS} flex-1 min-w-[8rem]")
                    placeholder="Filter text…"
                    prop:value=move || text_input.get()
                    on:input=move |ev| text_input.set(event_target_value(&ev))
                />
                // Reset
                <Show when=any_filter_active>
                    <button
                    aria-label="Clear all filters"
                        class=crate::components::icon_button::icon_button_class(
                        crate::components::icon_button::IconButtonVariant::Neutral,
                    )
                        title="Clear all filters"
                        on:click=move |_| reset_filters()
                    >
                        <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">"filter_alt_off"</span>                    </button>
                </Show>
            </div>

            // Feed
            <div class="flex-1 overflow-auto p-6 flex flex-col">
                    {move || {
                        let notes = displayed.get();

                        if notes.is_empty() {
                            if loading.get() {
                                return view! {
                                    <crate::components::skeleton::SkeletonList rows=6 />
                                }.into_any();
                            }
                            let msg = if any_filter_active() {
                                "No notes match these filters."
                            } else {
                                "No notes yet. Write one above, or add one from a node."
                            };
                            return view! {
                                <div class="flex-1 flex flex-col justify-center">
                                    <EmptyState icon="sticky_note_2" message=msg />
                                </div>
                            }.into_any();
                        }

                        view! {
                            <div class="space-y-4 w-full">
                                {notes.into_iter().map(|feed_note| {
                                    let node_id = feed_note.note.node_id;
                                    let note_id = feed_note.note.id;
                                    let node_title = feed_note.node_title.clone();
                                    let body_html = render_markdown_plain(&feed_note.note.body);
                                    let card_class = palette_card_class(&feed_note.note.color).to_string();
                                    // Local time, not the UTC value printed as-is.
                                    let ts = crate::components::format_helpers::format_timestamp(&feed_note.note.created_at);

                                    // Header: clickable node label that deep-links to the
                                    // note inside its node (`?note=`).
                                    let header = {
                                        let nid = node_id;
                                        {
                                            let nav = navigate.clone();
                                            let title = node_title.unwrap_or_default();
                                            view! {
                                                <button
                                                    class="flex items-center gap-1.5 text-xs font-semibold
                                                        text-stone-400 dark:text-stone-500 uppercase tracking-wider
                                                        hover:text-amber-600 dark:hover:text-amber-400 transition-colors"
                                                    title="Open this note in its node"
                                                    on:click=move |_| nav(
                                                        &format!("/nodes/{nid}?note={note_id}"),
                                                        Default::default(),
                                                    )
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
                                    };

                                    view! {
                                        <div class=format!("rounded-lg border px-4 py-3 {card_class}")>
                                            <div class="flex items-start justify-between gap-2 mb-2">
                                                {header}
                                                // Delete: instant, with an Undo toast (soft delete).
                                                <IconButton
                                                    icon="delete"
                                                    label="Delete note"
                                                    variant=IconButtonVariant::Danger
                                                    on_click=Callback::new(move |()| do_delete(note_id))
                                                />
                                            </div>
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
                    {move || has_more.get().then(|| view! {
                        <div class="flex justify-center pt-4">
                            <button
                                class="inline-flex items-center justify-center gap-1.5 px-4 py-1.5 rounded-lg text-sm font-medium text-amber-700 \
                                    dark:text-amber-400 border border-amber-300 dark:border-amber-800 \
                                    hover:bg-amber-50 dark:hover:bg-amber-950/40 transition-colors \
                                    disabled:opacity-50 disabled:cursor-not-allowed"
                                disabled=move || loading_more.get()
                                on:click=move |_| load_more()
                            >
                                <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"expand_more"</span>
                                {move || if loading_more.get() { "Loading…" } else { "Load more" }}
                            </button>
                        </div>
                    })}
            </div>
        </div>
    }
}
