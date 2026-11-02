//! The Captures view (`/tasks/captures`; ONTOLOGY §9.1, #234, #240):
//! unclassified input waiting to be processed. Each capture can be
//! reworded, converted into exactly one thing (`ConvertCaptureDialog`), or
//! deliberately deleted, with Undo. Inbox Zero is the goal.
//!
//! It shows captures only (S4c, owner decision on #240). A task with no
//! Project is not a capture: Data health's `OrphanTask` rule lists it,
//! with "Move to a Project".
//!
//! Captures have their own identity (#240): a wall of sticky notes
//! (`StickyPaper`), not task-style rows, each showing how long it has
//! waited (`common::capture::CaptureAge`).
//!
//! ## Keyboard (#240 part B2)
//!
//! A window keydown listener while the view is mounted, like My Day's.
//! It does nothing on a modifier, while a field or button has focus, or
//! while any modal is open, so `d` never acts behind the Convert dialog.
//!
//!   `j` / `→`        next capture (reading order)
//!   `k` / `←`        previous capture
//!   `↓` / `↑`        the card below / above (`common::grid_nav`)
//!   `Enter` / `c`    convert the focused capture
//!   `e`              edit it
//!   `d`              delete it (Undo in the toast)
//!   `Escape`         drop the focus ring

use common::capture::{Capture, CaptureAge};
use common::grid_nav::{self, Cell, Step};
use common::id::CaptureId;
use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

use crate::app::TaskRefresh;
use crate::components::empty_state::EmptyState;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::page_header::PageHeader;
use crate::components::sticky_note::{CAPTURE_ICON, StickyPaper};
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

/// The toast every way of adding a capture shows.
pub const CAPTURED_TOAST: &str = "Added to Captures";

/// What a key asks the focused card to do. The card itself acts, with the
/// same code as its buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardCommand {
    Convert,
    Edit,
    Delete,
}

/// The wall's keyboard cursor, shared by the view and its cards.
#[derive(Clone, Copy)]
struct WallKeys {
    /// The card with the focus ring.
    focused: RwSignal<Option<CaptureId>>,
    /// A command for one card; that card clears it once it has acted.
    command: RwSignal<Option<(CaptureId, CardCommand)>>,
}

#[component]
pub fn CapturesView() -> impl IntoView {
    // The same counter quick capture bumps, so a capture made with `n`
    // while this page is open appears at once.
    let refresh = expect_context::<TaskRefresh>().0;
    take_captured_marker(refresh);

    let toast_state = use_context::<ToastState>();
    let captures = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_captures().await }
    });

    let keys = WallKeys {
        focused: RwSignal::new(None),
        command: RwSignal::new(None),
    };
    // The cards' ids in display order, for the key handler.
    let order = RwSignal::new(Vec::<CaptureId>::new());
    Effect::new(move |_| {
        // Still loading: keep the old order and the focus.
        let Some(Ok(list)) = captures.get() else {
            return;
        };
        let ids: Vec<CaptureId> = list.iter().map(|c| c.id).collect();
        // A converted or deleted card hands the ring to the one now in
        // its place, so `d d d` clears a run of cards.
        if let Some(gone) = keys.focused.get_untracked()
            && !ids.contains(&gone)
        {
            let next = order.with_untracked(|old| old.iter().position(|id| *id == gone));
            let next = next.and_then(|i| ids.get(i.min(ids.len().saturating_sub(1))).copied());
            keys.focused.set(next);
        }
        order.set(ids);
    });

    let key_handle = window_event_listener(ev::keydown, move |ev| {
        if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
            return;
        }
        if crate::keyboard::active_element_is_editable() || crate::keyboard::modal_open() {
            return;
        }
        let ids = order.get_untracked();
        if ids.is_empty() {
            return;
        }
        let key = ev.key();
        let cur = keys
            .focused
            .get_untracked()
            .and_then(|f| ids.iter().position(|id| *id == f));
        let mv = match key.as_str() {
            "j" | "ArrowRight" => Some(Step::Next),
            "k" | "ArrowLeft" => Some(Step::Prev),
            "ArrowDown" => Some(Step::Down),
            "ArrowUp" => Some(Step::Up),
            _ => None,
        };
        if let Some(step) = mv {
            ev.prevent_default();
            let target = grid_nav::step(&card_cells(&ids), cur, step).and_then(|i| ids.get(i));
            if let Some(&id) = target {
                keys.focused.set(Some(id));
                scroll_card_into_view(id);
            }
            return;
        }
        let Some(id) = cur.and_then(|i| ids.get(i)).copied() else {
            return;
        };
        let cmd = match key.as_str() {
            "Enter" | "c" => CardCommand::Convert,
            "e" => CardCommand::Edit,
            "d" => CardCommand::Delete,
            "Escape" => {
                ev.prevent_default();
                keys.focused.set(None);
                return;
            }
            _ => return,
        };
        ev.prevent_default();
        keys.command.set(Some((id, cmd)));
    });
    // Dropping the handle doesn't detach it (leptos.md): a leaked listener
    // reads disposed signals and poisons event dispatch.
    on_cleanup(move || key_handle.remove());

    view! {
        <div class="flex flex-col h-full">
            <PageHeader
                icon=CAPTURE_ICON
                title="Captures"
                subtitle="Turn each into one thing, or delete it".into_any()
            />
            <div class="flex-1 overflow-auto px-4 py-4">
                <Transition fallback=|| ()>
                    {move || captures.get().map(|result| match result {
                        Ok(list) if list.is_empty() => view! {
                            <EmptyState
                                icon=CAPTURE_ICON
                                message="Nothing captured"
                                hint="Press n anywhere to capture a thought."
                            />
                        }.into_any(),
                        Ok(list) => view! {
                            <section data-testid="captures" aria-label="Captures">
                                <p class="mb-3 text-xs tabular-nums text-stone-500 dark:text-stone-400">
                                    {format!("{} waiting", list.len())}
                                </p>
                                // A wall, not a list (#240): scanning a corkboard.
                                // The top padding leaves room for each card's tape.
                                <ul class="grid gap-4 pt-2 grid-cols-1 sm:grid-cols-2 xl:grid-cols-3">
                                    {list.into_iter().map(|c| view! {
                                        <CaptureRow capture=c refresh=refresh toast_state=toast_state keys=keys />
                                    }).collect_view()}
                                </ul>
                            </section>
                        }.into_any(),
                        Err(e) => view! {
                            <p class="text-xs text-red-500">{format!("Couldn\u{2019}t load captures: {e}")}</p>
                        }.into_any(),
                    })}
                </Transition>
            </div>
        </div>
    }
}

/// Each card's top-left corner, in `ids` order. A card that isn't in the
/// DOM (mid-refresh) makes the wall read as one column, which still moves
/// in reading order.
fn card_cells(ids: &[CaptureId]) -> Vec<Cell> {
    let rects: Option<Vec<Cell>> = ids
        .iter()
        .map(|id| {
            let el = card_element(*id)?;
            let r = el.get_bounding_client_rect();
            Some(Cell {
                x: r.left(),
                y: r.top(),
            })
        })
        .collect();
    rects.unwrap_or_else(|| {
        (0..ids.len())
            .map(|i| Cell {
                x: 0.0,
                y: i as f64 * 100.0,
            })
            .collect()
    })
}

fn card_element(id: CaptureId) -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .query_selector(&format!("[data-capture-id=\"{id}\"]"))
        .ok()
        .flatten()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
}

fn scroll_card_into_view(id: CaptureId) {
    if let Some(el) = card_element(id) {
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_behavior(web_sys::ScrollBehavior::Smooth);
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    }
}

/// The share sheet's service worker posts to `/api/inbox/quick` and
/// redirects here with `?captured=1` (older installs to `/tasks/inbox`,
/// which redirects here with its query). Confirm it with a toast, reload
/// the wall, and strip the marker so a reload doesn't toast again. The
/// home-screen "Quick capture" shortcut is `/?capture=1` instead: it opens
/// the composer (`layout.rs`). Runs once per mount.
fn take_captured_marker(refresh: RwSignal<u32>) {
    Effect::new(move |run_count: Option<()>| {
        if run_count.is_some() {
            return;
        }
        let Some(win) = web_sys::window() else {
            return;
        };
        let Ok(href) = win.location().href() else {
            return;
        };
        let Ok(url) = web_sys::Url::new(&href) else {
            return;
        };
        let params = url.search_params();
        if params.get("captured").as_deref() != Some("1") {
            return;
        }
        push_toast(ToastLevel::Success, CAPTURED_TOAST);
        refresh.update(|n| *n += 1);
        params.delete("captured");
        url.set_search(&params.to_string().as_string().unwrap_or_default());
        let new_href = format!(
            "{}{}{}",
            url.pathname(),
            if url.search().is_empty() { "" } else { "?" },
            url.search().trim_start_matches('?')
        );
        if let Ok(history) = win.history() {
            let _ = history.replace_state_with_url(
                &leptos::wasm_bindgen::JsValue::NULL,
                "",
                Some(&new_href),
            );
        }
    });
}

#[component]
fn CaptureRow(
    capture: Capture,
    refresh: RwSignal<u32>,
    toast_state: Option<ToastState>,
    keys: WallKeys,
) -> impl IntoView {
    let id = capture.id;
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(capture.body.clone());
    let busy = RwSignal::new(false);
    let converting = RwSignal::new(false);
    let body = StoredValue::new(capture.body);
    let edit_ref: NodeRef<html::Textarea> = NodeRef::new();

    // A key aimed at this card does what its button would.
    Effect::new(move |_| {
        let Some((target, cmd)) = keys.command.get() else {
            return;
        };
        if target != id {
            return;
        }
        keys.command.set(None);
        if busy.get_untracked() {
            return;
        }
        match cmd {
            CardCommand::Convert => converting.set(true),
            CardCommand::Edit => editing.set(true),
            CardCommand::Delete => delete_with_undo(id, busy, refresh, toast_state),
        }
    });
    // The text box takes the focus when editing starts, by key or click
    // (NodeRef + Effect, leptos.md).
    Effect::new(move |_| {
        if editing.get()
            && let Some(el) = edit_ref.get()
        {
            let _ = el.focus();
        }
    });
    let is_focused = move || keys.focused.get() == Some(id);
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
        <li
            data-testid="capture"
            data-capture-id=id.to_string()
            aria-current=move || is_focused().then_some("true")
            class=move || if is_focused() {
                "list-none rounded-sm ring-2 ring-ember ring-offset-4 \
                 ring-offset-stone-50 dark:ring-offset-stone-950"
            } else {
                "list-none"
            }
            // A click puts the keyboard cursor here too, like My Day.
            on:click=move |_| keys.focused.set(Some(id))
        >
        <StickyPaper class="h-full min-h-[8rem] p-3 pt-4 text-sm flex flex-col gap-2" stale=age.is_stale()>
            {move || if editing.get() {
                view! {
                    <textarea
                        node_ref=edit_ref
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
