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

use common::id::NodeId;
use common::search::SearchResult;
use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::wasm_bindgen::{JsCast, closure::Closure};
use leptos_router::hooks::{use_location, use_navigate};
use wasm_bindgen_futures::spawn_local;

use crate::app::ShowCapture;
use crate::components::dark_mode_toggle::Theme;
use crate::components::layout::ShowHelp;
use crate::components::toast::{ToastLevel, push_toast};
use crate::recent::{RecentEntry, read_recent};

// ── PaletteAction ─────────────────────────────────────────────────────────────
//
// One displayed item in the palette.  Keeps typed-discriminated payloads
// rather than smuggling everything through strings, so the dispatch
// branch in `pick()` is exhaustive.

#[derive(Clone, PartialEq, Eq)]
enum PaletteAction {
    /// Navigate to an existing node (from Recent or Search results).
    OpenNode {
        id: uuid::Uuid,
        title: String,
        icon: String,
    },
    /// Open the structured `CreateNodeModal` with `title` pre-filled.
    CreateNode { title: String },
    /// An app command (navigation, capture, theme, …).
    Command(&'static CommandSpec),
}

impl PaletteAction {
    fn icon(&self) -> &str {
        match self {
            PaletteAction::OpenNode { icon, .. } => icon,
            PaletteAction::CreateNode { .. } => "add",
            PaletteAction::Command(spec) => spec.icon,
        }
    }
    fn primary(&self) -> &str {
        match self {
            PaletteAction::OpenNode { title, .. } => title,
            PaletteAction::CreateNode { title } => title,
            PaletteAction::Command(spec) => spec.label,
        }
    }
    fn secondary(&self) -> &'static str {
        match self {
            PaletteAction::OpenNode { .. } => "Open",
            PaletteAction::CreateNode { .. } => "Create new node",
            // The global shortcut, shown so the palette teaches the faster path.
            PaletteAction::Command(spec) => spec.hint,
        }
    }
}

// ── Commands ─────────────────────────────────────────────────────────────────
//
// Every command the palette can run. `keywords` are extra search aliases so
// users can find a command in their own vocabulary ("dark", "theme", …);
// `hint` is the existing global shortcut (empty when there is none) — shown
// in the row so the palette doubles as shortcut documentation.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    GoMyDay,
    GoInbox,
    GoCalendar,
    GoDashboard,
    GoGraph,
    GoNotes,
    GoNodes,
    GoTags,
    GoTemplates,
    GoSearch,
    GoWebhooks,
    NewTask,
    NewNode,
    ToggleDark,
    OpenHelp,
    EditCurrentNode,
    DuplicateCurrentNode,
}

#[derive(PartialEq, Eq)]
struct CommandSpec {
    cmd: Command,
    label: &'static str,
    icon: &'static str,
    keywords: &'static str,
    hint: &'static str,
}

/// Commands always available, in palette display order.
const GLOBAL_COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        cmd: Command::NewTask,
        label: "New task (quick capture)",
        icon: "bolt",
        keywords: "add create todo capture inbox",
        hint: "n",
    },
    CommandSpec {
        cmd: Command::NewNode,
        label: "New node…",
        icon: "add_circle",
        keywords: "add create article project area note",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoMyDay,
        label: "Go to My Day",
        icon: "wb_sunny",
        keywords: "today tasks kanban",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoInbox,
        label: "Go to Inbox",
        icon: "inbox",
        keywords: "tasks triage process",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoCalendar,
        label: "Go to Calendar",
        icon: "calendar_month",
        keywords: "due dates schedule",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoDashboard,
        label: "Go to Dashboard",
        icon: "space_dashboard",
        keywords: "projects overview recap",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoGraph,
        label: "Go to Graph",
        icon: "hub",
        keywords: "knowledge map network",
        hint: "g",
    },
    CommandSpec {
        cmd: Command::GoNotes,
        label: "Go to Notes",
        icon: "sticky_note_2",
        keywords: "feed journal",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoNodes,
        label: "Go to All Nodes",
        icon: "list",
        keywords: "browse articles projects",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoTags,
        label: "Go to Tags",
        icon: "sell",
        keywords: "labels manage",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoTemplates,
        label: "Go to Templates",
        icon: "stacks",
        keywords: "scaffold prefill",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoSearch,
        label: "Go to Search",
        icon: "search",
        keywords: "find query full-text advanced presets filters",
        hint: "",
    },
    CommandSpec {
        cmd: Command::GoWebhooks,
        label: "Go to Webhooks",
        icon: "webhook",
        keywords: "hooks integrations notify endpoints",
        hint: "",
    },
    CommandSpec {
        cmd: Command::ToggleDark,
        label: "Toggle dark mode",
        icon: "dark_mode",
        keywords: "theme light appearance",
        hint: "",
    },
    CommandSpec {
        cmd: Command::OpenHelp,
        label: "Help & shortcuts",
        icon: "help",
        keywords: "keyboard reference docs",
        hint: "?",
    },
];

/// Commands offered only while viewing a node (`/nodes/<uuid>`).
const NODE_COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        cmd: Command::EditCurrentNode,
        label: "Edit current node",
        icon: "edit",
        keywords: "modify body",
        hint: "",
    },
    CommandSpec {
        cmd: Command::DuplicateCurrentNode,
        label: "Duplicate current node",
        icon: "content_copy",
        keywords: "copy clone",
        hint: "d",
    },
];

/// Parse `/nodes/<uuid>` (view route only — not /new, not /edit).
fn node_route_id(path: &str) -> Option<NodeId> {
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    match segs.as_slice() {
        ["nodes", id] => id.parse::<NodeId>().ok(),
        _ => None,
    }
}

fn command_matches(spec: &CommandSpec, q: &str) -> bool {
    let q = q.to_lowercase();
    spec.label.to_lowercase().contains(&q) || spec.keywords.to_lowercase().contains(&q)
}

/// Order the non-empty-query action list:
///
/// 1. nodes whose **title** contains the query — the quick-switcher core,
/// 2. matching commands,
/// 3. nodes that matched only via body/notes/tasks text,
/// 4. the Create action (always last).
///
/// Commands outrank body-only node hits so a command-intent query ("theme",
/// "dark") isn't hijacked by prose that merely mentions the word — found
/// live-testing v2.21.3 (ROADMAP, 2026-06-10). Typing a node's actual title
/// still puts that node first.
fn ranked_actions(
    results: &[SearchResult],
    cmds: Vec<&'static CommandSpec>,
    trimmed: &str,
) -> Vec<PaletteAction> {
    let q = trimmed.to_lowercase();
    let open = |r: &SearchResult| PaletteAction::OpenNode {
        id: r.node_id.0,
        title: r.title.clone(),
        icon: type_to_icon(&r.node_type),
    };
    let (title_hits, body_hits): (Vec<&SearchResult>, Vec<&SearchResult>) = results
        .iter()
        .partition(|r| r.title.to_lowercase().contains(&q));

    let mut out: Vec<PaletteAction> = title_hits.into_iter().map(open).collect();
    out.extend(cmds.into_iter().map(PaletteAction::Command));
    out.extend(body_hits.into_iter().map(open));
    out.push(PaletteAction::CreateNode {
        title: trimmed.to_string(),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::id::NodeId;

    fn result(title: &str) -> SearchResult {
        SearchResult {
            node_id: NodeId(uuid::Uuid::new_v4()),
            title: title.to_string(),
            slug: String::new(),
            snippet: None,
            rank: 1.0,
            node_type: "article".to_string(),
            status: "draft".to_string(),
            match_source: Some("node".to_string()),
            highlighted_title: None,
        }
    }

    fn labels(actions: &[PaletteAction]) -> Vec<String> {
        actions.iter().map(|a| a.primary().to_string()).collect()
    }

    #[test]
    fn commands_outrank_body_only_node_matches() {
        // "theme" matched two nodes via body text only — the command must
        // come first (the v2.21.3 hijack).
        let results = vec![result("Dartmouth Seminar"), result("Product Definition")];
        let cmds = vec![&GLOBAL_COMMANDS[11]]; // Toggle dark mode
        let out = ranked_actions(&results, cmds, "theme");
        assert_eq!(
            labels(&out),
            vec![
                "Toggle dark mode",
                "Dartmouth Seminar",
                "Product Definition",
                "theme", // Create action carries the query as its title
            ]
        );
    }

    #[test]
    fn title_matched_nodes_stay_above_commands() {
        // Quick-switcher behavior: typing a node's title puts it first even
        // when a command also matches.
        let results = vec![result("Dark Patterns Research"), result("Unrelated")];
        let cmds = vec![&GLOBAL_COMMANDS[11]];
        let out = ranked_actions(&results, cmds, "dark");
        assert_eq!(
            labels(&out),
            vec![
                "Dark Patterns Research",
                "Toggle dark mode",
                "Unrelated",
                "dark",
            ]
        );
    }

    #[test]
    fn title_match_is_case_insensitive_and_create_is_last() {
        let results = vec![result("my PROJECT plan")];
        let out = ranked_actions(&results, vec![], "project");
        assert_eq!(labels(&out), vec!["my PROJECT plan", "project"]);
        assert!(matches!(out.last(), Some(PaletteAction::CreateNode { .. })));
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
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    super::return_focus_on_close(show);

    let navigate = StoredValue::new(use_navigate());
    let location = use_location();
    // Contexts the commands act on (all provided at app/layout root).
    let show_capture = use_context::<ShowCapture>();
    let show_help = use_context::<ShowHelp>();
    let theme = use_context::<RwSignal<Theme>>();
    let refresh = use_context::<RwSignal<u32>>();

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
            if version.get_untracked() != my_v {
                return;
            }
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
        let on_node_page = node_route_id(&location.pathname.get()).is_some();
        let mut out: Vec<PaletteAction> = Vec::new();

        if trimmed.is_empty() {
            // Recent + a few core commands — the "no-typing-needed" path.
            for r in recent.get().into_iter().take(5) {
                out.push(PaletteAction::OpenNode {
                    id: r.id,
                    title: r.title,
                    icon: r.icon,
                });
            }
            for spec in GLOBAL_COMMANDS.iter().take(2) {
                out.push(PaletteAction::Command(spec));
            }
            if on_node_page {
                for spec in NODE_COMMANDS {
                    out.push(PaletteAction::Command(spec));
                }
            }
        } else {
            // Matching commands (node-context ones first when applicable).
            let mut cmds: Vec<&'static CommandSpec> = Vec::new();
            if on_node_page {
                cmds.extend(NODE_COMMANDS.iter().filter(|s| command_matches(s, trimmed)));
            }
            cmds.extend(
                GLOBAL_COMMANDS
                    .iter()
                    .filter(|s| command_matches(s, trimmed)),
            );
            cmds.truncate(5);
            out.extend(ranked_actions(&results.get(), cmds, trimmed));
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
        let Some(action) = acts.get(idx).cloned() else {
            return;
        };
        match action {
            PaletteAction::OpenNode { id, .. } => {
                navigate.get_value()(&format!("/nodes/{id}"), Default::default());
                on_close.run(());
            }
            PaletteAction::CreateNode { title } => {
                on_close.run(());
                on_create.run(title);
            }
            PaletteAction::Command(spec) => {
                on_close.run(());
                let nav = navigate.get_value();
                match spec.cmd {
                    Command::GoMyDay => nav("/tasks/my-day", Default::default()),
                    Command::GoInbox => nav("/tasks/inbox", Default::default()),
                    Command::GoCalendar => nav("/tasks/calendar", Default::default()),
                    Command::GoDashboard => nav("/dashboard", Default::default()),
                    Command::GoGraph => nav("/graph", Default::default()),
                    Command::GoNotes => nav("/notes", Default::default()),
                    Command::GoNodes => nav("/nodes", Default::default()),
                    Command::GoTags => nav("/tags", Default::default()),
                    Command::GoTemplates => nav("/templates", Default::default()),
                    Command::GoSearch => nav("/search", Default::default()),
                    Command::GoWebhooks => nav("/webhooks", Default::default()),
                    Command::NewTask => {
                        if let Some(c) = show_capture {
                            c.0.set(true);
                        }
                    }
                    Command::NewNode => on_create.run(String::new()),
                    Command::ToggleDark => {
                        if let Some(t) = theme {
                            t.update(|v| {
                                *v = if *v == Theme::Dark {
                                    Theme::Light
                                } else {
                                    Theme::Dark
                                };
                            });
                        }
                    }
                    Command::OpenHelp => {
                        if let Some(h) = show_help {
                            h.0.set(true);
                        }
                    }
                    Command::EditCurrentNode => {
                        if let Some(id) = node_route_id(&location.pathname.get_untracked()) {
                            nav(&format!("/nodes/{id}/edit"), Default::default());
                        }
                    }
                    Command::DuplicateCurrentNode => {
                        if let Some(id) = node_route_id(&location.pathname.get_untracked()) {
                            let nav2 = navigate.get_value();
                            spawn_local(async move {
                                match crate::api::duplicate_node(id).await {
                                    Ok(dup) => {
                                        push_toast(ToastLevel::Success, "Node duplicated.");
                                        if let Some(r) = refresh {
                                            r.update(|n| *n += 1);
                                        }
                                        nav2(&format!("/nodes/{}", dup.id), Default::default());
                                    }
                                    Err(e) => push_toast(
                                        ToastLevel::Error,
                                        format!("Duplicate failed: {e}"),
                                    ),
                                }
                            });
                        }
                    }
                }
            }
        }
    };

    // Keyboard handler on the palette container.  ↑/↓ move the
    // highlight, Enter picks, Esc closes.  Plain typing falls through
    // to the input element so the query updates normally.
    let on_keydown = move |ev: web_sys::KeyboardEvent| match ev.key().as_str() {
        "Escape" => {
            ev.prevent_default();
            on_close.run(());
        }
        "ArrowDown" => {
            ev.prevent_default();
            let len = actions.get_untracked().len();
            if len == 0 {
                return;
            }
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
        _ => {
            if let Some(panel) = panel_ref.get_untracked() {
                super::trap_focus(&ev, &panel);
            }
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
                        node_ref=panel_ref
                        class="pointer-events-auto bg-white dark:bg-stone-900 \
                               rounded-2xl shadow-2xl border border-stone-200 \
                               dark:border-stone-700 overflow-hidden"
                        role="dialog"
                        aria-modal="true"
                        aria-label="Command palette"
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
        "article" => "description",
        "project" => "rocket_launch",
        "area" => "category",
        "resource" => "bookmarks",
        "reference" => "menu_book",
        _ => "note",
    }
    .to_string()
}
