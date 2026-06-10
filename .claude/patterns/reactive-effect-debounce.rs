//! Pattern: debounced search input (version counter + 300 ms Timeout).
//!
//! Problem: a text box drives an expensive query; you only want to fire 300 ms after
//! the user STOPS typing, not on every keystroke.
//!
//! Solution: bump a version counter on each change and schedule a Timeout. When the
//! timeout fires it only commits if its captured version is still the latest — so
//! every keystroke cancels the previous pending commit without any timer-handle juggling.
//!
//! Distilled from ui/src/components/notes_view.rs (`debounce_v` / `text_q`).

use gloo_timers::callback::Timeout; // NOT leptos::leptos_dom::helpers — that path has no Timeout
use leptos::prelude::*;

#[component]
fn DebouncedSearch() -> impl IntoView {
    let text_input = RwSignal::new(String::new()); // bound to the <input>
    let text_q = RwSignal::new(String::new());     // the debounced value the feed queries
    let debounce_v = RwSignal::new(0u32);           // monotonically increasing version

    Effect::new(move |_| {
        let val = text_input.get();                 // re-runs on every keystroke
        let v = debounce_v.get_untracked() + 1;
        debounce_v.set(v);
        Timeout::new(300, move || {
            // Only the latest scheduled timeout wins; stale ones see a newer version.
            if debounce_v.get_untracked() == v {
                text_q.set(val.clone());
            }
        })
        .forget();
    });

    // `text_q` (not `text_input`) is what the LocalResource/feed depends on.
    view! { <input on:input=move |e| text_input.set(event_target_value(&e)) /> }
}
