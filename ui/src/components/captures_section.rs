//! The Inbox's captures (ONTOLOGY §9.1, #234): unclassified input waiting
//! to be processed. Each one can be reworded, converted into exactly one
//! thing (`ConvertCaptureDialog`), or deliberately deleted, with Undo. S4c
//! folds this into a redesigned Inbox.
//!
//! #240 gives captures their own identity: a wall of sticky notes
//! (`StickyPaper`), not task-style rows, each showing how long it has
//! waited (`common::capture::CaptureAge`), so Inbox Zero stays in view.

use common::capture::{Capture, CaptureAge};
use common::id::CaptureId;
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::sticky_note::{CAPTURE_ICON, StickyPaper};
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
                    <section data-testid="captures" aria-labelledby="captures-heading">
                        <header class="mb-3 flex items-center gap-2">
                            <span class="material-symbols-outlined text-ember" style="font-size: 18px;"
                                aria-hidden="true">{CAPTURE_ICON}</span>
                            <h2 id="captures-heading"
                                class="text-sm font-semibold text-stone-800 dark:text-stone-200">
                                "Captures"
                            </h2>
                            <span class="text-xs tabular-nums text-stone-500">{list.len()}</span>
                        </header>
                        // A wall, not a list (#240): scanning a corkboard. The
                        // top padding leaves room for each card's tape.
                        <ul class="grid gap-4 pt-2 grid-cols-1 sm:grid-cols-2 xl:grid-cols-3">
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
    let age = CaptureAge::between(capture.created_at, chrono::Utc::now());
    let age_title = format!("{} \u{00b7} {when}", age.describe());

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
        <li class="list-none" data-testid="capture">
        <StickyPaper class="h-full min-h-[8rem] p-3 pt-4 text-sm flex flex-col gap-2" stale=age.is_stale()>
            {move || if editing.get() {
                view! {
                    <textarea
                        class="flex-1 min-h-[5rem] w-full rounded-sm border border-sticky-edge
                               dark:border-sticky-dim-edge bg-white/40 dark:bg-black/20 px-2 py-1
                               text-stone-900 dark:text-stone-100"
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
                    <div class="flex items-center justify-end gap-1">
                        <IconButton icon="check" label="Save capture" variant=IconButtonVariant::Save
                            disabled=Signal::derive(move || busy.get() || draft.get().trim().is_empty())
                            on_click=Callback::new(move |()| save()) />
                        <IconButton icon="close" label="Cancel"
                            on_click=Callback::new(move |()| {
                                draft.set(body.get_value());
                                editing.set(false);
                            }) />
                    </div>
                }.into_any()
            } else {
                let age_title = age_title.clone();
                view! {
                    <p class="flex-1 whitespace-pre-wrap break-words">
                        {body.get_value()}
                    </p>
                    <div class="flex items-center gap-1">
                        // The glyph + "Capture" keep the identity without colour.
                        <span class="material-symbols-outlined text-stone-500 dark:text-stone-400"
                            style="font-size: 14px;" aria-hidden="true">{CAPTURE_ICON}</span>
                        // The tag is for the eye; screen readers get the
                        // words ("captured 3 weeks ago"), which say "capture" too.
                        <span class="sr-only">{age.describe()}</span>
                        <span data-testid="capture-age" aria-hidden="true"
                            class=if age.is_stale() {
                                "text-xs tabular-nums font-medium text-stone-700 dark:text-stone-300"
                            } else {
                                "text-xs tabular-nums text-stone-500 dark:text-stone-400"
                            }
                            title=age_title>
                            {age.tag()}
                        </span>
                        <span class="flex-1"></span>
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
                    </div>
                }.into_any()
            }}
        </StickyPaper>
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
