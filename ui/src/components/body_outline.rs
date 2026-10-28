//! In-page outline of a node body's `##` / `###` headings (#185).
//!
//! The entries come from `markdown::render_body`, the same parse that renders
//! the body, so an entry can't point at a heading that isn't there. Shown
//! only from `OUTLINE_MIN_HEADINGS` entries: short bodies stay unchanged.

use leptos::prelude::*;

use crate::markdown::OutlineEntry;

/// Scroll the rendered heading with this id to the top of its scroller.
fn scroll_to_heading(id: &str) {
    let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
    else {
        return;
    };
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_behavior(web_sys::ScrollBehavior::Smooth);
    opts.set_block(web_sys::ScrollLogicalPosition::Start);
    el.scroll_into_view_with_scroll_into_view_options(&opts);
}

/// A compact contents list above the body; `entries` empty renders nothing.
#[component]
pub fn BodyOutline(#[prop(into)] entries: Signal<Vec<OutlineEntry>>) -> impl IntoView {
    move || {
        let entries = entries.get();
        (!entries.is_empty()).then(|| {
            view! {
                <nav
                    aria-label="Contents"
                    data-testid="body-outline"
                    class="mb-6 rounded-lg border border-stone-200 dark:border-stone-800
                        bg-stone-50 dark:bg-stone-900/40 px-4 py-3 max-w-md"
                >
                    <p class="text-xs font-semibold uppercase tracking-wide
                        text-stone-500 dark:text-stone-400 mb-1">"Contents"</p>
                    <ol class="text-sm space-y-0.5">
                        {entries.into_iter().map(|e| {
                            let indent = if e.level == 3 { "pl-4" } else { "" };
                            let id = e.id;
                            view! {
                                <li class=indent>
                                    <button
                                        type="button"
                                        class="text-left text-stone-600 dark:text-stone-300
                                            hover:text-ember hover:underline"
                                        on:click=move |_| scroll_to_heading(&id)
                                    >
                                        {e.text}
                                    </button>
                                </li>
                            }
                        }).collect_view()}
                    </ol>
                </nav>
            }
        })
    }
}
