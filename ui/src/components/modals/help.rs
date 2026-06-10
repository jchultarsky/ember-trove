//! In-app help — keyboard shortcuts + concept reference + workflow tips.
//!
//! Triggered by the `?` keyboard shortcut (anywhere) or the `(?)` icon
//! in the sidebar header.  Three tabs:
//!
//! * **Shortcuts** — full keyboard reference, grouped by surface.
//! * **Concepts**  — what each data type and field actually means.  This
//!   is the highest-ROI tab: the user shouldn't have to reverse-engineer
//!   `focus_date` vs `due_date`, PARA grouping rules, or what "Inbox"
//!   means in this app.
//! * **Workflow** — how to actually use the app day-to-day.
//!
//! Content is intentionally short and structured (headings + bullets, no
//! flowing prose) so updates are surgical.  See the release checklist:
//! when a release changes a user-visible model or workflow, sync the
//! corresponding tab content here.

use leptos::portal::Portal;
use leptos::prelude::*;

// ── Tabs ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum HelpTab {
    Shortcuts,
    Concepts,
    Workflow,
}

impl HelpTab {
    fn label(self) -> &'static str {
        match self {
            HelpTab::Shortcuts => "Shortcuts",
            HelpTab::Concepts => "Concepts",
            HelpTab::Workflow => "Workflow",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            HelpTab::Shortcuts => "keyboard",
            HelpTab::Concepts => "category",
            HelpTab::Workflow => "route",
        }
    }
}

// ── Shortcut table (carried over from the v2.6.x ShortcutsModal) ─────────────

struct Shortcut {
    key: &'static str,
    description: &'static str,
}

struct ShortcutGroup {
    title: &'static str,
    items: &'static [Shortcut],
}

const ANYWHERE: &[Shortcut] = &[
    Shortcut {
        key: "n",
        description: "Quick capture (Inbox)",
    },
    Shortcut {
        key: "g",
        description: "Graph view",
    },
    Shortcut {
        key: "/",
        description: "Open command palette",
    },
    Shortcut {
        key: "⌘K",
        description: "Open command palette (alt)",
    },
    Shortcut {
        key: "?",
        description: "Show this help",
    },
    Shortcut {
        key: "Escape",
        description: "Close modal / back",
    },
];

const NODE_VIEW: &[Shortcut] = &[
    Shortcut {
        key: "d",
        description: "Duplicate current node",
    },
    Shortcut {
        key: "p",
        description: "Pin / unpin current node",
    },
];

const MY_DAY: &[Shortcut] = &[
    Shortcut {
        key: "j / ↓",
        description: "Focus next task",
    },
    Shortcut {
        key: "k / ↑",
        description: "Focus previous task",
    },
    Shortcut {
        key: "Enter",
        description: "Open focused task in its parent",
    },
    Shortcut {
        key: "Space",
        description: "Toggle done on focused task",
    },
    Shortcut {
        key: "t",
        description: "Toggle Today / Backlog for focused task",
    },
    Shortcut {
        key: "e",
        description: "Edit focused task inline",
    },
    Shortcut {
        key: "d",
        description: "Delete focused task (undo via toast)",
    },
];

const INBOX_TRIAGE: &[Shortcut] = &[
    Shortcut {
        key: "t",
        description: "Add to today",
    },
    Shortcut {
        key: "s",
        description: "Schedule a due date",
    },
    Shortcut {
        key: "a",
        description: "Attach to a node",
    },
    Shortcut {
        key: "d",
        description: "Delete (undo via toast)",
    },
    Shortcut {
        key: "j / k",
        description: "Skip / go back",
    },
    Shortcut {
        key: "Escape",
        description: "Exit triage",
    },
];

const SHORTCUT_GROUPS: &[ShortcutGroup] = &[
    ShortcutGroup {
        title: "Anywhere",
        items: ANYWHERE,
    },
    ShortcutGroup {
        title: "My Day Kanban",
        items: MY_DAY,
    },
    ShortcutGroup {
        title: "Inbox triage (Process)",
        items: INBOX_TRIAGE,
    },
    ShortcutGroup {
        title: "Node view",
        items: NODE_VIEW,
    },
];

// ── Concepts content ─────────────────────────────────────────────────────────
//
// Headings + short paragraphs. Keep each section to 1-3 sentences.

struct Concept {
    title: &'static str,
    body: &'static str,
}

const CONCEPTS: &[Concept] = &[
    Concept {
        title: "Nodes",
        body: "The unit of knowledge. Five types — Article (free-form text), Project \
               (an outcome with tasks), Area (long-running responsibility), Resource \
               (reference material), Reference (external citation). Pick the type that \
               matches how you'll come back to it, not the contents.",
    },
    Concept {
        title: "Tasks",
        body: "Discrete actionable work. A task is either standalone (in the Inbox, \
               with no parent node) or attached to a Node — usually a Project. Has \
               status (open/in-progress/done/cancelled), priority, focus_date, due_date, \
               and an optional recurrence rule.",
    },
    Concept {
        title: "Notes",
        body: "Short comments attached to a Node. Notes always have a parent — they \
               can't stand alone. Use them for journal-style annotations on a Node \
               (\"checked in with the team today\", \"deadline pushed to May 8\"). \
               If you want a free-standing thought, that's an Article-typed Node \
               (or, if you'll triage it later, a standalone Task in the Inbox).",
    },
    Concept {
        title: "focus_date vs due_date",
        body: "Two completely separate things. focus_date = \"I'm working on this \
               today\" (drives the My Day Kanban — binary in this model: today or \
               not-today). due_date = an external deadline. A task can have either, \
               both, or neither. The Kanban only manipulates focus_date; the task \
               editor (pencil button) is the place to change due_date.",
    },
    Concept {
        title: "Areas → Projects (PARA)",
        body: "An Area \"contains\" Projects via an edge of type Contains. The \
               Dashboard groups Projects under their parent Area. A Project with \
               no Area parent shows up under \"Ungrouped\" — fine for short-lived \
               projects, but adds friction when you have many. Build the Area first, \
               then attach Projects to it via the Edges panel on the Area's node \
               page.",
    },
    Concept {
        title: "Inbox",
        body: "Tasks where node_id IS NULL — the triage zone. Quick captures \
               (n shortcut, iOS Share Sheet) land here. Visit /tasks/inbox to add \
               quick tasks inline; the My Day Kanban backlog also surfaces every \
               Inbox task with an \"Inbox\" chip so you can promote them to Today \
               without leaving the Kanban.",
    },
    Concept {
        title: "Carryover",
        body: "A task with focus_date in the past that's still open. The Kanban \
               surfaces these in the Backlog with a \"carried from <date>\" badge — \
               nothing falls into limbo, you just see it sitting and decide whether \
               to bring it back to today (☀ tap or `t` shortcut) or leave it in \
               the backlog.",
    },
    Concept {
        title: "Pinning",
        body: "★ on a Project card on the Dashboard. Pinned projects sort to the \
               top of their Area group; within pinned and within unpinned, recency \
               wins. Use sparingly — 2-4 pins, not 20.",
    },
    Concept {
        title: "Tags vs Areas",
        body: "Tags are flat labels for cross-cutting concerns (\"#urgent\", \
               \"#external\"). Areas are hierarchical containers. If a thing \
               belongs to one parent, it's an Area relationship; if it spans many, \
               it's a tag. Don't recreate Areas as tags or vice-versa.",
    },
    Concept {
        title: "Recent + Search",
        body: "Recent (sidebar bottom) is your last 10 visited nodes, kept in \
               localStorage. Search is the Cmd-K palette: top 5 recent on a blank \
               query, then live-debounced search results, then a \"Create node \
               titled '<query>'\" inline action. The full-page /search route still \
               exists for advanced filtering (tag AND/OR, date range, etc).",
    },
];

// ── Workflow content ─────────────────────────────────────────────────────────

struct WorkflowStep {
    heading: &'static str,
    body: &'static str,
}

const WORKFLOW_STEPS: &[WorkflowStep] = &[
    WorkflowStep {
        heading: "Morning — open My Day",
        body: "Visit /tasks/my-day (or just open the app). The Today zone shows \
               what you've already committed to. Carryovers from previous days show \
               up in the Backlog with a \"carried from\" badge — promote them with \
               ☀ tap or `t` shortcut, or leave them.",
    },
    WorkflowStep {
        heading: "Pick today's work from the Backlog",
        body: "Backlog is sorted by deadline first, then priority. Tap ☀ on any \
               row (or drag it into the Today zone, on desktop) to set its \
               focus_date to today. Aim for a realistic pile — the Today zone \
               should fit on one screen.",
    },
    WorkflowStep {
        heading: "During the day — keyboard or mouse",
        body: "Click a task row to open it in its parent project; click the title \
               to navigate, click the ☀/×/✏️/🗑 buttons to act on it. Or use the \
               keyboard: j/k to move between tasks, Space to toggle done, e to \
               edit inline, t to bounce between Today and Backlog. The whole \
               focus loop fits in seven keys.",
    },
    WorkflowStep {
        heading: "Capture quickly when something interrupts",
        body: "Press n anywhere → one textarea → Cmd+Enter to land it in the \
               Inbox. On iPhone, share text or a URL from any app to \"Trove\" via \
               the iOS Share Sheet — the PWA handles it the same way. Triage in \
               the Kanban backlog or /tasks/inbox later.",
    },
    WorkflowStep {
        heading: "Find anything — Cmd-K",
        body: "Press ⌘K (or /) anywhere. Empty query shows your top 5 recent \
               nodes; type to live-search. Enter opens the highlighted result; \
               type a new title and pick the bottom \"Create node titled '<query>'\" \
               action to start a new node from scratch.",
    },
    WorkflowStep {
        heading: "End of day — push unfinished tasks back",
        body: "On any task in Today that you didn't finish, hit `t` (or click ×) \
               to remove it from today. Tomorrow it'll appear in the Backlog with \
               a \"carried from <today>\" badge so you remember it slipped. \
               Nothing is lost; nothing falls into limbo.",
    },
    WorkflowStep {
        heading: "Weekly — review the Dashboard",
        body: "Visit /dashboard. Projects are grouped under their parent Area; \
               pinned projects float to the top. The \"Recent activity\" panel \
               at the top shows what changed in the last 48h — a useful re-entry \
               point if you've been away. Use this view to notice projects that \
               have gone quiet (low activity, lots of open tasks) and either \
               re-prioritise or archive them.",
    },
];

// ── HelpModal ────────────────────────────────────────────────────────────────

#[component]
pub fn HelpModal(#[prop(into)] show: Signal<bool>, on_close: Callback<()>) -> impl IntoView {
    let active = RwSignal::new(HelpTab::Shortcuts);
    let panel_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    super::return_focus_on_close(show);

    // Reset to the Shortcuts tab every time the modal opens — the
    // shortcut reference is the most-frequently-needed surface and a
    // good landing tab.
    Effect::new(move |_| {
        if show.get() {
            active.set(HelpTab::Shortcuts);
        }
    });

    view! {
        <Show when=move || show.get()>
            <Portal>
                // Backdrop
                <div
                    class="fixed inset-0 z-40 bg-black/50 backdrop-blur-sm"
                    on:click=move |_| on_close.run(())
                />
                // Panel
                <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
                    <div
                        node_ref=panel_ref
                        class="bg-white dark:bg-stone-900 rounded-2xl shadow-2xl
                               border border-stone-200 dark:border-stone-700
                               w-full max-w-2xl flex flex-col
                               max-h-[85vh] overflow-hidden"
                        role="dialog"
                        aria-modal="true"
                        aria-label="Help and shortcuts"
                        on:click=|ev| ev.stop_propagation()
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if let Some(panel) = panel_ref.get_untracked() {
                                super::trap_focus(&ev, &panel);
                            }
                        }
                    >
                        // Header
                        <div class="flex items-center justify-between px-6 pt-5 pb-3 \
                                    border-b border-stone-100 dark:border-stone-800">
                            <div class="flex items-center gap-3">
                                <div class="flex-shrink-0 w-9 h-9 rounded-full
                                            bg-amber-100 dark:bg-amber-900/30
                                            flex items-center justify-center">
                                    <span class="material-symbols-outlined text-amber-600 dark:text-amber-400"
                                          style="font-size: 18px;">"help"</span>
                                </div>
                                <h2 class="text-base font-semibold text-stone-900 dark:text-stone-100">
                                    "Help"
                                </h2>
                            </div>
                            <button
                                type="button"
                                class="text-stone-400 hover:text-stone-600 \
                                       dark:hover:text-stone-200 transition-colors"
                                on:click=move |_| on_close.run(())
                            >
                                <span class="material-symbols-outlined">"close"</span>
                            </button>
                        </div>

                        // Tab bar
                        <div class="flex items-center gap-1 px-4 pt-2 border-b \
                                    border-stone-100 dark:border-stone-800">
                            {[HelpTab::Shortcuts, HelpTab::Concepts, HelpTab::Workflow].iter()
                                .map(|&tab| {
                                    let label = tab.label();
                                    let icon  = tab.icon();
                                    view! {
                                        <button
                                            type="button"
                                            class="px-3 py-2 text-sm flex items-center gap-1.5 \
                                                   transition-colors cursor-pointer \
                                                   border-b-2"
                                            style=move || if active.get() == tab {
                                                "color:#b45309;border-color:#f59e0b;font-weight:600;"
                                            } else {
                                                "color:#78716c;border-color:transparent;"
                                            }
                                            on:click=move |_| active.set(tab)
                                        >
                                            <span class="material-symbols-outlined"
                                                  style="font-size:16px;">{icon}</span>
                                            {label}
                                        </button>
                                    }
                                }).collect::<Vec<_>>()}
                        </div>

                        // Tab body — scrolls inside the panel
                        <div class="flex-1 overflow-auto px-6 py-4">
                            {move || match active.get() {
                                HelpTab::Shortcuts => view! { <ShortcutsTab /> }.into_any(),
                                HelpTab::Concepts  => view! { <ConceptsTab  /> }.into_any(),
                                HelpTab::Workflow  => view! { <WorkflowTab  /> }.into_any(),
                            }}
                        </div>

                        // Footer — version stamp.  Auto-fills from CARGO_PKG_VERSION
                        // at compile time; the release checklist is responsible for
                        // syncing the *content* above with whatever shipped.
                        <div class="px-6 py-2 text-[10px] text-stone-400 \
                                    dark:text-stone-500 border-t border-stone-100 \
                                    dark:border-stone-800 flex items-center justify-between">
                            <span>"Shortcuts disabled while typing in inputs."</span>
                            <span class="font-mono">{concat!("Help for v", env!("CARGO_PKG_VERSION"))}</span>
                        </div>
                    </div>
                </div>
            </Portal>
        </Show>
    }
}

// ── Tab renderers ────────────────────────────────────────────────────────────

#[component]
fn ShortcutsTab() -> impl IntoView {
    view! {
        <div class="space-y-5">
            {SHORTCUT_GROUPS.iter().map(|g| view! {
                <section>
                    <h3 class="text-xs font-semibold uppercase tracking-wide \
                               text-amber-700 dark:text-amber-400 mb-2">
                        {g.title}
                    </h3>
                    <table class="w-full text-sm border-collapse">
                        <tbody>
                            {g.items.iter().map(|s| view! {
                                <tr class="border-b border-stone-100 dark:border-stone-800 last:border-0">
                                    <td class="py-1.5 pr-4 w-28">
                                        <kbd class="inline-flex items-center justify-center
                                                    min-w-[2rem] px-2 py-0.5
                                                    rounded border border-stone-300 dark:border-stone-600
                                                    bg-stone-100 dark:bg-stone-800
                                                    font-mono text-xs text-stone-700 dark:text-stone-300
                                                    shadow-sm">
                                            {s.key}
                                        </kbd>
                                    </td>
                                    <td class="py-1.5 text-stone-600 dark:text-stone-400">
                                        {s.description}
                                    </td>
                                </tr>
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                </section>
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn ConceptsTab() -> impl IntoView {
    view! {
        <div class="space-y-4">
            <p class="text-sm text-stone-600 dark:text-stone-400">
                "Definitions of the things this app actually contains. \
                 Read once when you're new; come back when something \
                 surprises you."
            </p>
            {CONCEPTS.iter().map(|c| view! {
                <section>
                    <h3 class="text-sm font-semibold text-amber-700 dark:text-amber-400 mb-1">
                        {c.title}
                    </h3>
                    <p class="text-sm text-stone-600 dark:text-stone-300 leading-relaxed">
                        {c.body}
                    </p>
                </section>
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn WorkflowTab() -> impl IntoView {
    view! {
        <div class="space-y-4">
            <p class="text-sm text-stone-600 dark:text-stone-400">
                "How the pieces fit together day to day. None of this is \
                 enforced — these are the loops the app is shaped around, \
                 not rules."
            </p>
            {WORKFLOW_STEPS.iter().enumerate().map(|(idx, s)| view! {
                <section class="flex gap-3">
                    <span class="flex-shrink-0 w-6 h-6 rounded-full
                                 bg-amber-100 dark:bg-amber-900/30
                                 text-amber-700 dark:text-amber-400
                                 text-xs font-semibold
                                 flex items-center justify-center mt-0.5">
                        {(idx + 1).to_string()}
                    </span>
                    <div class="flex-1 min-w-0">
                        <h3 class="text-sm font-semibold text-stone-800 dark:text-stone-200 mb-0.5">
                            {s.heading}
                        </h3>
                        <p class="text-sm text-stone-600 dark:text-stone-300 leading-relaxed">
                            {s.body}
                        </p>
                    </div>
                </section>
            }).collect::<Vec<_>>()}
        </div>
    }
}
