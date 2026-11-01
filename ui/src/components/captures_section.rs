//! The Inbox's captures (ONTOLOGY §9.1, #234): unclassified input waiting
//! to be processed. Each one can be reworded, converted into exactly one
//! thing (`ConvertCaptureDialog`), or deliberately deleted, with Undo. S4c
//! folds this into a redesigned Inbox.

use common::capture::Capture;
use common::id::CaptureId;
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

#[component]
pub fn CapturesSection(
    /// Bumped by anything that adds a capture, so the list reloads.
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let toast_state = use_context::<ToastState>();
    let captures = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_captures().await }
    });
    view! {
        <Transition fallback=|| ()>
            {move || captures.get().map(|result| match result {
                Ok(list) if list.is_empty() => ().into_any(),
                Ok(list) => view! {
                    <section class="rounded-lg border border-stone-200 dark:border-stone-700
                                    bg-white dark:bg-stone-900"
                        data-testid="captures" aria-labelledby="captures-heading">
                        <header class="px-4 py-2 border-b border-stone-100 dark:border-stone-800
                                       flex items-baseline justify-between">
                            <h2 id="captures-heading"
                                class="text-sm font-semibold text-stone-800 dark:text-stone-200">
                                "Captures"
                            </h2>
                            <span class="text-xs tabular-nums text-stone-500">{list.len()}</span>
                        </header>
                        <ul class="divide-y divide-stone-100 dark:divide-stone-800">
                            {list.into_iter().map(|c| view! {
                                <CaptureRow capture=c refresh=refresh toast_state=toast_state />
                            }).collect_view()}
                        </ul>
                    </section>
                }.into_any(),
                Err(e) => view! {
                    <p class="text-xs text-red-500">{format!("Couldn\u{2019}t load captures: {e}")}</p>
                }.into_any(),
            })}
        </Transition>
    }
}

#[component]
fn CaptureRow(
    capture: Capture,
    refresh: RwSignal<u32>,
    toast_state: Option<ToastState>,
) -> impl IntoView {
    let id = capture.id;
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(capture.body.clone());
    let busy = RwSignal::new(false);
    let converting = RwSignal::new(false);
    let body = StoredValue::new(capture.body);
    let when = crate::components::format_helpers::format_timestamp(&capture.created_at);

    let save = move || {
        let text = draft.get_untracked().trim().to_string();
        if text.is_empty() || busy.get_untracked() {
            return;
        }
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::update_capture(id, &text).await {
                Ok(_) => {
                    editing.try_set(false);
                    refresh.try_update(|n| *n += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn\u{2019}t save: {e}")),
            }
            busy.try_set(false);
        });
    };

    view! {
        <li class="px-4 py-2 text-sm flex items-start gap-2" data-testid="capture">
            {move || if editing.get() {
                view! {
                    <textarea
                        class="flex-1 min-h-[3rem] rounded-md border border-stone-200 dark:border-stone-700
                               bg-transparent px-2 py-1 text-stone-900 dark:text-stone-100"
                        aria-label="Capture text"
                        prop:value=move || draft.get()
                        on:input=move |ev| draft.set(event_target_value(&ev))
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
                                ev.prevent_default();
                                save();
                            } else if ev.key() == "Escape" {
                                // Handled here; the window dispatcher must not
                                // also act on it (.claude/rules/leptos.md).
                                // Escape cancels, like ✕: the draft goes too.
                                ev.stop_propagation();
                                draft.set(body.get_value());
                                editing.set(false);
                            }
                        }
                    ></textarea>
                    <IconButton icon="check" label="Save capture" variant=IconButtonVariant::Save
                        disabled=Signal::derive(move || busy.get() || draft.get().trim().is_empty())
                        on_click=Callback::new(move |()| save()) />
                    <IconButton icon="close" label="Cancel"
                        on_click=Callback::new(move |()| {
                            draft.set(body.get_value());
                            editing.set(false);
                        }) />
                }.into_any()
            } else {
                view! {
                    <div class="flex-1 min-w-0">
                        <p class="whitespace-pre-wrap break-words text-stone-800 dark:text-stone-200">
                            {body.get_value()}
                        </p>
                        <p class="mt-0.5 text-xs text-stone-400">{when.clone()}</p>
                    </div>
                    <IconButton icon="move_item" label="Convert capture"
                        disabled=Signal::derive(move || busy.get())
                        on_click=Callback::new(move |()| converting.set(true)) />
                    <IconButton icon="edit" label="Edit capture"
                        disabled=Signal::derive(move || busy.get())
                        on_click=Callback::new(move |()| editing.set(true)) />
                    <IconButton icon="delete" label="Delete capture"
                        variant=IconButtonVariant::Danger
                        disabled=Signal::derive(move || busy.get())
                        on_click=Callback::new(move |()| delete_with_undo(id, busy, refresh, toast_state)) />
                }.into_any()
            }}
            <crate::components::modals::convert_capture::ConvertCaptureDialog
                capture_id=id text=body.get_value() show=converting refresh=refresh />
        </li>
    }
}

/// A deliberate delete (§9.1) that can be undone: the capture is
/// tombstoned, and the toast's Undo restores it. Like `note_actions`, the
/// Undo closure outlives the row, so it only touches signals (a no-op once
/// disposed) and the toast state captured up front.
fn delete_with_undo(
    id: CaptureId,
    busy: RwSignal<bool>,
    refresh: RwSignal<u32>,
    toast_state: Option<ToastState>,
) {
    if busy.get_untracked() {
        return;
    }
    busy.set(true);
    wasm_bindgen_futures::spawn_local(async move {
        let result = crate::api::delete_capture(id).await;
        busy.try_set(false);
        match result {
            Ok(()) => {
                refresh.try_update(|n| *n += 1);
                push_undo_toast(
                    "Capture deleted",
                    std::sync::Arc::new(move || {
                        wasm_bindgen_futures::spawn_local(async move {
                            match crate::api::restore_capture(id).await {
                                Ok(_) => {
                                    refresh.try_update(|n| *n += 1);
                                }
                                Err(e) => {
                                    if let Some(ts) = toast_state {
                                        ts.push(ToastLevel::Error, format!("Undo failed: {e}"));
                                    }
                                }
                            }
                        });
                    }),
                );
            }
            Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
        }
    });
}
