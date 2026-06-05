//! Pattern: using `use_navigate()` inside reactive contexts (Leptos 0.8).
//!
//! `NavigateFn` is `Clone`, NOT `Copy`. Moving it into an inner `move ||` closure
//! consumes it, making the closure `FnOnce` — which breaks reactivity (the closure
//! can only run once). Two correct approaches below.
//!
//! Distilled from ui/src/components/tasks_view.rs and layout.rs.

use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

// ── Approach A: StoredValue (preferred for handlers reused across renders) ──────────
#[component]
fn TabButton(path: &'static str) -> impl IntoView {
    // Wrap once; `StoredValue` is `Copy`, so it can be freely captured.
    let navigate = StoredValue::new(use_navigate());

    view! {
        <button on:click=move |_| {
            // Call through `get_value()`. `Default::default()` = default NavigateOptions.
            navigate.get_value()(path, Default::default());
        }>"Go"</button>
    }
}

// ── Approach B: clone before each inner closure (e.g. inside a `map`) ────────────────
#[component]
fn NodeLinks(paths: Vec<String>) -> impl IntoView {
    let navigate = use_navigate();
    view! {
        <ul>
            {paths.into_iter().map(|p| {
                let navigate = navigate.clone();   // clone per iteration
                view! {
                    <li on:click=move |_| navigate(&p, Default::default())>{p.clone()}</li>
                }
            }).collect_view()}
        </ul>
    }
}
