//! Keyboard shortcut reference overlay — shown when the user presses `?`.
use leptos::portal::Portal;
use leptos::prelude::*;

struct Shortcut {
    key: &'static str,
    description: &'static str,
}

struct ShortcutGroup {
    title: &'static str,
    items: &'static [Shortcut],
}

const ANYWHERE: &[Shortcut] = &[
    Shortcut { key: "n",      description: "Quick capture (Inbox)" },
    Shortcut { key: "g",      description: "Graph view" },
    Shortcut { key: "/",      description: "Open command palette" },
    Shortcut { key: "⌘K",     description: "Open command palette (alt)" },
    Shortcut { key: "?",      description: "Show this help" },
    Shortcut { key: "Escape", description: "Close modal / back" },
];

const NODE_VIEW: &[Shortcut] = &[
    Shortcut { key: "d",      description: "Duplicate current node" },
    Shortcut { key: "p",      description: "Pin / unpin current node" },
];

// v2.7.0 — Kanban keyboard triage.  Mirrors the shortcut set in
// `my_day_view.rs`'s window keydown handler.
const MY_DAY: &[Shortcut] = &[
    Shortcut { key: "j / ↓",  description: "Focus next task" },
    Shortcut { key: "k / ↑",  description: "Focus previous task" },
    Shortcut { key: "Enter",  description: "Open focused task in its parent" },
    Shortcut { key: "Space",  description: "Toggle done on focused task" },
    Shortcut { key: "t",      description: "Toggle Today / Backlog for focused task" },
    Shortcut { key: "e",      description: "Edit focused task inline" },
    Shortcut { key: "d",      description: "Delete focused task" },
];

const GROUPS: &[ShortcutGroup] = &[
    ShortcutGroup { title: "Anywhere",        items: ANYWHERE },
    ShortcutGroup { title: "My Day Kanban",   items: MY_DAY },
    ShortcutGroup { title: "Node view",       items: NODE_VIEW },
];

#[component]
pub fn ShortcutsModal(
    #[prop(into)] show: Signal<bool>,
    on_close: Callback<()>,
) -> impl IntoView {
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
                        class="bg-white dark:bg-stone-900 rounded-2xl shadow-2xl
                               border border-stone-200 dark:border-stone-700
                               w-full max-w-md p-6 flex flex-col gap-5
                               max-h-[85vh] overflow-auto"
                        on:click=|ev| ev.stop_propagation()
                    >
                        // Header
                        <div class="flex items-center justify-between">
                            <div class="flex items-center gap-3">
                                <div class="flex-shrink-0 w-10 h-10 rounded-full
                                            bg-amber-100 dark:bg-amber-900/30
                                            flex items-center justify-center">
                                    <span class="material-symbols-outlined text-amber-600 dark:text-amber-400"
                                          style="font-size: 20px;">"keyboard"</span>
                                </div>
                                <h2 class="text-base font-semibold text-stone-900 dark:text-stone-100">
                                    "Keyboard Shortcuts"
                                </h2>
                            </div>
                            <button
                                class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-200 transition-colors"
                                on:click=move |_| on_close.run(())
                            >
                                <span class="material-symbols-outlined">"close"</span>
                            </button>
                        </div>

                        // Grouped shortcut tables
                        {GROUPS.iter().map(|g| view! {
                            <section class="space-y-2">
                                <h3 class="text-xs font-semibold uppercase tracking-wide
                                           text-amber-700 dark:text-amber-400">
                                    {g.title}
                                </h3>
                                <table class="w-full text-sm border-collapse">
                                    <tbody>
                                        {g.items.iter().map(|s| view! {
                                            <tr class="border-b border-stone-100 dark:border-stone-800 last:border-0">
                                                <td class="py-1.5 pr-4 w-24">
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

                        // Footer hint
                        <p class="text-xs text-stone-400 dark:text-stone-500">
                            "Shortcuts are disabled when focus is inside an input field."
                        </p>
                    </div>
                </div>
            </Portal>
        </Show>
    }
}
