//! Cmd-K command palette (v2.8.0).
//!
//! Floating overlay over the current view, opened with `⌘K` / `Ctrl-K`
//! (or `/` — repurposed from the v2.6.x full-page navigation to
//! `SearchView`).  Closes on Esc, click outside, or after picking
//! a result.
//!
//! Sections, in display order:
//!
//! 1. **Recent** — top 5 entries from `crate::recent::read_recent()`
//!    (localStorage-backed).  Shown only when the query is blank, as
//!    a "no-typing-needed" fast path.
//! 2. **Search results** — live, 300ms-debounced, up to 8 results from
//!    `node_picker_search`.  Shown when the query is non-empty.
//! 3. **Create node** — "Create node titled '<query>'" inline action.
//!    Always present as the last item when the query is non-empty,
//!    even if the search returned a match (sometimes you want to
//!    create *another* node with the same title — e.g. the canonical
//!    target was archived).  Selecting it opens the structured
//!    `CreateNodeModal` pre-filled with the query as the title.
//!
//! Keyboard model inside the palette:
//!   * `↑` / `↓`  — move highlight up / down across all visible items
//!   * `Enter`    — pick the highlighted item
//!   * `Esc`      — close
//!   * Typing     — updates query; auto-resets highlight to the first
//!     item so Enter always lands somewhere sensible
//!
//! Keeps debounce + stale-response guard from
//! `.claude/patterns/reactive-effect-debounce.rs`.

use common::search::SearchResult;
use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::wasm_bindgen::{closure::Closure, JsCast};
use leptos_router::hooks::use_navigate;
use wasm_bindgen_futures::spawn_local;

use crate::recent::{read_recent, RecentEntry};

// ── PaletteAction ─────────────────────────────────────────────────────────────
//
// One displayed item in the palette.  Keeps typed-discriminated payloads
// rather than smuggling everything through strings, so the dispatch
// branch in `pick()` is exhaustive.

#[derive(Clone, PartialEq, Eq)]
enum PaletteAction {
    /// Navigate to an existing node (from Recent or Search results).
    OpenNode { id: uuid::Uuid, title: String, icon: String },
    /// Open the structured `CreateNodeModal` with `title` pre-filled.
    CreateNode { title: String },
}

impl PaletteAction {
    fn icon(&self) -> &str {
        match self {
            PaletteAction::OpenNode  { icon, .. } => icon,
            PaletteAction::CreateNode { .. }      => "add",
        }
    }
    fn primary(&self) -> &str {
        match self {
            PaletteAction::OpenNode   { title, .. } => title,
            PaletteAction::CreateNode { title }     => title,
        }
    }
    fn secondary(&self) -> &'static str {
        match self {
            PaletteAction::OpenNode   { .. } => "Open",
            PaletteAction::CreateNode { .. } => "Create new node",
        }
    }
}

// ── Component ────────────────────────────────────────────────────────────────

/// `CommandPalette` is rendered once at the layout root and toggled
/// open/closed via the `show` signal.  When opened, the input
/// autofocuses on the next animation frame.
///
/// Props:
/// * `show`     — visibility signal driven by the global hotkey handler.
/// * `on_close` — fired on Esc / backdrop click / successful pick.
/// * `on_create` — fired when the user picks the "Create node titled
///   '<query>'" action.  The parent is responsible for opening the
///   structured modal pre-filled with the title.  Receives the title.
#[component]
pub fn CommandPalette(
    #[prop(into)] show: Signal<bool>,
    on_close: Callback<()>,
    on_create: Callback<String>,
) -> impl IntoView {
    let query: RwSignal<String> = RwSignal::new(String::new());
    let results: RwSignal<Vec<SearchResult>> = RwSignal::new(Vec::new());
    let recent: RwSignal<Vec<RecentEntry>> = RwSignal::new(Vec::new());
    let highlight: RwSignal<usize> = RwSignal::new(0);
    let version: RwSignal<u32> = RwSignal::new(0);
    let input_ref: NodeRef<html::Input> = NodeRef::new();

    let navigate = StoredValue::new(use_navigate());

    // Reset state every time the palette opens, populate Recent from
    // localStorage, and focus the input on the next animation frame
    // (the input doesn't exist in the DOM yet at the moment `show`
    // flips to true).
    Effect::new(move |_| {
        if show.get() {
            query.set(String::new());
            results.set(Vec::new());
            recent.set(read_recent());
            highlight.set(0);
            if let Some(win) = web_sys::window() {
                let cb = Closure::once_into_js(move || {
                    if let Some(el) = input_ref.get_untracked() {
                        let _ = el.focus();
                    }
                });
                let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }
    });

    // Debounced search effect.  Mirrors the canonical pattern at
    // `.claude/patterns/reactive-effect-debounce.rs`: bump a monotonic
    // version counter, sleep 300ms, drop stale responses on the way in
    // and on the way out.
    Effect::new(move |_| {
        let q = query.get();
        version.update(|v| *v += 1);
        let my_v = version.get_untracked();
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(300).await;
            if version.get_untracked() != my_v { return; }
            if q.trim().is_empty() {
                results.set(Vec::new());
                return;
            }
            if let Ok(rs) = crate::api::node_picker_search(&q).await
                && version.get_untracked() == my_v
            {
                results.set(rs);
            }
        });
    });

    // Build the action list reactively.  The order here is the order
    // shown in the palette and the order Enter/arrow-keys traverse.
    let actions = Memo::new(move |_| {
        let q = query.get();
        let trimmed = q.trim();
        let mut out: Vec<PaletteAction> = Vec::new();

        if trimmed.is_empty() {
            // Recent only — the "no-typing-needed" path.
            for r in recent.get().into_iter().take(5) {
                out.push(PaletteAction::OpenNode {
                    id: r.id, title: r.title, icon: r.icon,
                });
            }
        } else {
            for sr in results.get() {
                out.push(PaletteAction::OpenNode {
                    id: sr.node_id.0,
                    title: sr.title,
                    icon: type_to_icon(&sr.node_type),
                });
            }
            // Always offer Create as the bottom action when typing.
            out.push(PaletteAction::CreateNode { title: trimmed.to_string() });
        }
        out
    });

    // Clamp the highlight whenever the action list shrinks (e.g. typing
    // narrows the result set).  Without this, Enter could fire on a
    // missing index.
    Effect::new(move |_| {
        let len = actions.get().len();
        if highlight.get_untracked() > len.saturating_sub(1) {
            highlight.set(0);
        }
    });

    // Pick by index — runs the action and closes the palette.
    let pick = move |idx: usize| {
        let acts = actions.get_untracked();
        let Some(action) = acts.get(idx).cloned() else { return; };
        match action {
            PaletteAction::OpenNode { id, .. } => {
                navigate.get_value()(&format!("/nodes/{id}"), Default::default());
                on_close.run(());
            }
            PaletteAction::CreateNode { title } => {
                on_close.run(());
                on_create.run(title);
            }
        }
    };

    // Keyboard handler on the palette container.  ↑/↓ move the
    // highlight, Enter picks, Esc closes.  Plain typing falls through
    // to the input element so the query updates normally.
    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        match ev.key().as_str() {
            "Escape" => {
                ev.prevent_default();
                on_close.run(());
            }
            "ArrowDown" => {
                ev.prevent_default();
                let len = actions.get_untracked().len();
                if len == 0 { return; }
                highlight.update(|h| *h = (*h + 1).min(len - 1));
            }
            "ArrowUp" => {
                ev.prevent_default();
                highlight.update(|h| *h = h.saturating_sub(1));
            }
            "Enter" => {
                ev.prevent_default();
                pick(highlight.get_untracked());
            }
            _ => {}
        }
    };

    view! {
        <Show when=move || show.get()>
            <Portal>
                // Backdrop — click closes
                <div
                    class="fixed inset-0 z-40 bg-black/40 backdrop-blur-sm"
                    on:click=move |_| on_close.run(())
                />
                // Panel
                <div
                    class="fixed inset-x-0 top-16 z-50 mx-auto w-full max-w-xl px-4 \
                           pointer-events-none"
                >
                    <div
                        class="pointer-events-auto bg-white dark:bg-stone-900 \
                               rounded-2xl shadow-2xl border border-stone-200 \
                               dark:border-stone-700 overflow-hidden"
                        on:keydown=on_keydown
                    >
                        // Search input
                        <div class="flex items-center gap-2 px-4 py-3 border-b \
                                    border-stone-200 dark:border-stone-700">
                            <span class="material-symbols-outlined text-stone-400" style="font-size:18px;">
                                "search"
                            </span>
                            <input
                                node_ref=input_ref
                                type="text"
                                placeholder="Search nodes, or type a new title…"
                                class="flex-1 bg-transparent text-sm text-stone-900 \
                                       dark:text-stone-100 placeholder-stone-400 \
                                       outline-none"
                                prop:value=move || query.get()
                                on:input=move |ev| {
                                    query.set(event_target_value(&ev));
                                    highlight.set(0);
                                }
                            />
                            <kbd class="text-[10px] font-mono px-1.5 py-0.5 rounded \
                                        bg-stone-100 dark:bg-stone-800 \
                                        text-stone-500 dark:text-stone-400">
                                "Esc"
                            </kbd>
                        </div>

                        // Section header (Recent vs Results)
                        {move || {
                            let q = query.get();
                            let acts = actions.get();
                            if acts.is_empty() {
                                return view! {
                                    <p class="px-4 py-6 text-sm text-stone-400 dark:text-stone-500 text-center">
                                        {if q.trim().is_empty() {
                                            "No recent nodes — start typing to search.".to_string()
                                        } else {
                                            "No matches.".to_string()
                                        }}
                                    </p>
                                }.into_any();
                            }
                            let header = if q.trim().is_empty() { "Recent" } else { "Matches" };
                            view! {
                                <div class="px-3 pt-2 pb-1 text-[10px] font-semibold uppercase \
                                            tracking-wider text-amber-700 dark:text-amber-400">
                                    {header}
                                </div>
                                <ul class="max-h-[60vh] overflow-auto">
                                    {acts.into_iter().enumerate().map(|(idx, action)| {
                                        let icon = action.icon().to_string();
                                        let primary = action.primary().to_string();
                                        let secondary = action.secondary();
                                        view! {
                                            <li>
                                                <button
                                                    type="button"
                                                    class="w-full flex items-center gap-3 px-4 py-2 \
                                                           text-left transition-colors cursor-pointer"
                                                    style=move || if highlight.get() == idx {
                                                        "background-color:rgba(245,158,11,0.12);"
                                                    } else { "" }
                                                    on:mouseenter=move |_| highlight.set(idx)
                                                    on:click=move |_| pick(idx)
                                                >
                                                    <span class="material-symbols-outlined \
                                                                 text-amber-600 dark:text-amber-500 \
                                                                 flex-shrink-0"
                                                          style="font-size:18px;">
                                                        {icon}
                                                    </span>
                                                    <span class="flex-1 min-w-0 text-sm \
                                                                 text-stone-800 dark:text-stone-200 \
                                                                 truncate">
                                                        {primary}
                                                    </span>
                                                    <span class="text-xs text-stone-400 \
                                                                 dark:text-stone-500 flex-shrink-0">
                                                        {secondary}
                                                    </span>
                                                </button>
                                            </li>
                                        }
                                    }).collect_view()}
                                </ul>
                                <div class="px-3 py-2 text-[10px] text-stone-400 \
                                            dark:text-stone-500 border-t border-stone-100 \
                                            dark:border-stone-800 flex items-center gap-3">
                                    <span><kbd class="font-mono">"↑↓"</kbd>" navigate"</span>
                                    <span><kbd class="font-mono">"Enter"</kbd>" pick"</span>
                                    <span><kbd class="font-mono">"Esc"</kbd>" close"</span>
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>
            </Portal>
        </Show>
    }
}

/// Map a node type string to a Material Symbols icon name.  Keeps the
/// palette and the recent-list visually consistent (the recent helper
/// stores the icon name directly so we don't have to do this mapping
/// twice for that section).
fn type_to_icon(node_type: &str) -> String {
    match node_type {
        "article"   => "description",
        "project"   => "rocket_launch",
        "area"      => "category",
        "resource"  => "bookmarks",
        "reference" => "menu_book",
        _           => "note",
    }.to_string()
}
