//! Pattern: shared submit logic via an `RwSignal<bool>` trigger + `Effect`.
//!
//! Problem: several handlers (button click, Ctrl+Enter, form submit) must run the
//! SAME async submit, but you don't want to duplicate the closure or fight ownership.
//!
//! Solution: one `RwSignal<bool>` "pending" trigger. Every handler just sets it true.
//! A single `Effect` watches it, does the work once, and resets it. The effect owns
//! the async block, so handlers stay trivial and `Copy`.
//!
//! Distilled from ui/src/components/modals/create_node.rs (`submit_pending`).

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
fn CreateForm() -> impl IntoView {
    let loading = RwSignal::new(false);
    // Signal-based submit trigger — set to true from any handler; the Effect does the work.
    let submit_pending = RwSignal::new(false);

    Effect::new(move |_| {
        if !submit_pending.get() {
            return;
        }
        submit_pending.set(false); // reset immediately so re-triggers re-fire the effect
        if loading.get_untracked() {
            return; // guard against double-submit
        }
        loading.set(true);
        spawn_local(async move {
            // ... perform the request ...
            loading.set(false);
        });
    });

    view! {
        // Click handler and keyboard handler share the exact same code path:
        <button on:click=move |_| submit_pending.set(true) disabled=move || loading.get()>
            "Create"
        </button>
        // elsewhere: on Ctrl+Enter → `submit_pending.set(true)`
    }
}
