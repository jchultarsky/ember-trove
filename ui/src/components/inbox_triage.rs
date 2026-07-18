//! Keyboard-driven inbox triage ("Process" mode).
//!
//! One open inbox task at a time, single-key decisions:
//! `t` add to My Day · `s` schedule a due date · `a` attach to a node ·
//! `d` delete (undoable) · `j` skip · `k` back · `Esc` exit.
//!
//! The component snapshots the open inbox tasks once on mount — actions fire
//! the same task PATCH/DELETE APIs as the row buttons, but the working list
//! never refetches mid-flow, so positions stay stable under the user's
//! fingers. The global task refresh is bumped once on exit.

use chrono::NaiveDate;
use common::id::{NodeId, TaskId};
use common::task::{Task, UpdateTaskRequest};
use leptos::prelude::*;

use crate::components::format_helpers::local_today;
use crate::components::task_common::undo_restore_task;
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

/// Which input the triage card is currently capturing.
#[derive(Clone, Copy, PartialEq)]
enum SubMode {
    /// Single-key actions.
    Keys,
    /// Date input open (`s`).
    Schedule,
    /// Node picker open (`a`).
    Attach,
}

fn blank_update() -> UpdateTaskRequest {
    UpdateTaskRequest {
        title: None,
        status: None,
        priority: None,
        focus_date: None,
        due_date: None,
        recurrence: None,
        node_id: None,
    }
}

#[component]
pub fn InboxTriage(on_exit: Callback<()>, refresh: RwSignal<u32>) -> impl IntoView {
    let tasks: RwSignal<Vec<Task>> = RwSignal::new(Vec::new());
    let loaded = RwSignal::new(false);
    let idx = RwSignal::new(0usize);
    let acted = RwSignal::new(0usize);
    let sub = RwSignal::new(SubMode::Keys);
    let busy = RwSignal::new(false);
    let toast_state = use_context::<ToastState>();

    // Schedule input state.
    let date_input = RwSignal::new(String::new());
    let date_ref = NodeRef::<leptos::html::Input>::new();
    // Attach picker state (debounced search, same pattern as the row picker).
    let picker_query = RwSignal::new(String::new());
    let picker_results: RwSignal<Vec<common::search::SearchResult>> = RwSignal::new(Vec::new());
    let picker_highlight = RwSignal::new(0usize);
    let pick_ver = RwSignal::new(0u32);
    let picker_ref = NodeRef::<leptos::html::Input>::new();

    // Snapshot the open inbox tasks once.
    wasm_bindgen_futures::spawn_local(async move {
        match crate::api::list_inbox().await {
            Ok(all) => {
                tasks.set(
                    all.into_iter()
                        .filter(|t| !crate::components::task_common::status_done(&t.status))
                        .collect(),
                );
                loaded.set(true);
            }
            Err(e) => {
                push_toast(ToastLevel::Error, format!("Couldn't load inbox: {e}"));
                on_exit.run(());
            }
        }
    });

    let total = move || tasks.with(|t| t.len());
    let current = move || tasks.with(|t| t.get(idx.get()).cloned());

    let finish = move || {
        refresh.update(|n| *n += 1);
        let n = acted.get_untracked();
        if n > 0 {
            push_toast(
                ToastLevel::Success,
                format!("Inbox processed — {n} handled"),
            );
        }
        on_exit.run(());
    };

    // Skip: move on without acting. Wraps so skipped tasks come around again;
    // Esc is the way out.
    let advance = move || {
        sub.set(SubMode::Keys);
        let len = total();
        if len <= 1 {
            return;
        }
        idx.update(|i| *i = (*i + 1) % len);
    };

    // A handled task leaves the working set; the next one slides into place.
    // When the set empties, triage is done.
    let remove_current = move || {
        sub.set(SubMode::Keys);
        let i = idx.get_untracked();
        tasks.update(|t| {
            if i < t.len() {
                t.remove(i);
            }
        });
        let len = total();
        if len == 0 {
            finish();
        } else if i >= len {
            idx.set(0);
        }
    };

    // Fire a PATCH for the current task, advance on success.
    let patch_current = move |req: UpdateTaskRequest, ok_msg: &'static str| {
        if busy.get_untracked() {
            return;
        }
        let Some(task) = current() else { return };
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::update_task(task.id, &req).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    acted.update(|n| *n += 1);
                    push_toast(ToastLevel::Success, ok_msg);
                    remove_current();
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Couldn't update: {e}")),
            }
        });
    };

    let do_today = move || {
        let req = UpdateTaskRequest {
            focus_date: Some(Some(local_today())),
            ..blank_update()
        };
        patch_current(req, "Added to today");
    };

    let do_delete = move || {
        if busy.get_untracked() {
            return;
        }
        let Some(task) = current() else { return };
        let task_id: TaskId = task.id;
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::delete_task(task_id).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    acted.update(|n| *n += 1);
                    push_undo_toast(
                        "Task deleted",
                        undo_restore_task(task_id, refresh, toast_state),
                    );
                    remove_current();
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Delete failed: {e}")),
            }
        });
    };

    let do_schedule = move || {
        let Ok(date) = date_input.get_untracked().trim().parse::<NaiveDate>() else {
            push_toast(ToastLevel::Error, "Pick a date first");
            return;
        };
        let req = UpdateTaskRequest {
            due_date: Some(Some(date)),
            ..blank_update()
        };
        patch_current(req, "Due date set");
    };

    let do_attach = move |node_id: NodeId| {
        let req = UpdateTaskRequest {
            node_id: Some(Some(node_id)),
            ..blank_update()
        };
        patch_current(req, "Attached to node");
    };

    // Focus helper for the submode inputs (next frame, post-render — the
    // element isn't in the DOM until then; same approach as fast_capture).
    let focus_after_render = move |which: SubMode| {
        if let Some(win) = web_sys::window() {
            let cb = leptos::wasm_bindgen::closure::Closure::once_into_js(move || match which {
                SubMode::Schedule => {
                    if let Some(el) = date_ref.get_untracked() {
                        let _ = el.focus();
                    }
                }
                SubMode::Attach => {
                    if let Some(el) = picker_ref.get_untracked() {
                        let _ = el.focus();
                    }
                }
                SubMode::Keys => {}
            });
            use leptos::wasm_bindgen::JsCast;
            let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    };

    let enter_schedule = move || {
        let preset = current()
            .and_then(|t| t.due_date)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        date_input.set(preset);
        sub.set(SubMode::Schedule);
        focus_after_render(SubMode::Schedule);
    };

    let enter_attach = move || {
        picker_query.set(String::new());
        picker_results.set(Vec::new());
        picker_highlight.set(0);
        sub.set(SubMode::Attach);
        focus_after_render(SubMode::Attach);
    };

    // Debounced node search for the attach picker.
    Effect::new(move |_| {
        let q = picker_query.get();
        if q.trim().is_empty() {
            picker_results.set(Vec::new());
            return;
        }
        pick_ver.update(|v| *v += 1);
        let ver = pick_ver.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(300).await;
            if pick_ver.get_untracked() != ver {
                return;
            }
            if let Ok(results) = crate::api::node_picker_search(&q).await
                && pick_ver.get_untracked() == ver
            {
                picker_highlight.set(0);
                picker_results.set(results);
            }
        });
    });

    // Single-key actions. Submode inputs handle their own keys; this listener
    // only acts in Keys mode (plus Esc, which always backs out one level).
    let key_handle =
        window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if ev.ctrl_key() || ev.meta_key() || ev.alt_key() {
                return;
            }
            if ev.key() == "Escape" {
                ev.prevent_default();
                ev.stop_propagation();
                if sub.get_untracked() == SubMode::Keys {
                    finish();
                } else {
                    sub.set(SubMode::Keys);
                }
                return;
            }
            if sub.get_untracked() != SubMode::Keys {
                return;
            }
            // Don't steal keys from the capture box or other inputs. Shared
            // guard (crate::keyboard) — reconciled with layout/my_day, which
            // previously also covered <button> and contenteditable; harmless
            // here since nothing is focused in SubMode::Keys.
            if crate::keyboard::active_element_is_editable() {
                return;
            }
            match ev.key().as_str() {
                "t" => {
                    ev.prevent_default();
                    do_today();
                }
                "s" => {
                    ev.prevent_default();
                    enter_schedule();
                }
                "a" => {
                    ev.prevent_default();
                    enter_attach();
                }
                "d" => {
                    ev.prevent_default();
                    do_delete();
                }
                "j" | "ArrowDown" | "ArrowRight" => {
                    ev.prevent_default();
                    advance();
                }
                "k" | "ArrowUp" | "ArrowLeft" => {
                    ev.prevent_default();
                    sub.set(SubMode::Keys);
                    idx.update(|i| *i = i.saturating_sub(1));
                }
                _ => {}
            }
        });
    on_cleanup(move || key_handle.remove());

    let hint_btn = "flex items-center gap-1.5 text-xs px-2.5 py-1.5 rounded-lg \
                    border border-stone-200 dark:border-stone-700 \
                    text-stone-600 dark:text-stone-300 \
                    hover:border-amber-400 hover:text-amber-600 dark:hover:text-amber-400 \
                    disabled:opacity-50 transition-colors cursor-pointer";
    let kbd = "px-1 rounded bg-stone-100 dark:bg-stone-800 font-mono text-[10px] \
               border border-stone-200 dark:border-stone-700";

    view! {
        <div class="max-w-xl mx-auto py-8">
            {move || {
                if !loaded.get() {
                    return view! { <crate::components::skeleton::SkeletonCard /> }.into_any();
                }
                let Some(task) = current() else {
                    // Snapshot was empty.
                    return view! {
                        <div class="text-center py-16 space-y-2">
                            <span class="material-symbols-outlined text-stone-300 dark:text-stone-600"
                                  style="font-size: 48px;">"check_circle"</span>
                            <p class="text-stone-400 dark:text-stone-500 text-sm">"Nothing to process."</p>
                            <button
                                class="text-xs text-amber-600 dark:text-amber-400 hover:underline"
                                on:click=move |_| on_exit.run(())
                            >
                                "Back to inbox"
                            </button>
                        </div>
                    }.into_any();
                };
                let title_html = crate::markdown::render_markdown_inline(&task.title);
                let due_label = task.due_date.map(|d| format!("due {}", d.format("%b %-d")));
                view! {
                    <div
                        data-testid="triage-card"
                        class="bg-white dark:bg-stone-900 rounded-2xl shadow-lg
                               border border-stone-200 dark:border-stone-700 p-6 space-y-5"
                    >
                        // Progress + exit
                        <div class="flex items-center justify-between text-xs
                                    text-stone-400 dark:text-stone-500">
                            <span aria-live="polite">
                                {format!("{} of {}", idx.get() + 1, total())}
                            </span>
                            <button
                                class="hover:text-stone-600 dark:hover:text-stone-300"
                                title="Exit triage (Esc)"
                                on:click=move |_| finish()
                            >
                                "Esc to exit"
                            </button>
                        </div>

                        // The task
                        <div class="space-y-1">
                            <div class="text-lg text-stone-900 dark:text-stone-100"
                                 inner_html=title_html />
                            {due_label.map(|l| view! {
                                <p class="text-xs text-stone-400 dark:text-stone-500">{l}</p>
                            })}
                        </div>

                        // Submode inputs
                        {move || match sub.get() {
                            SubMode::Schedule => view! {
                                <div class="flex items-center gap-2"
                                     on:keydown=move |ev: web_sys::KeyboardEvent| {
                                         if ev.key() == "Enter" {
                                             ev.prevent_default();
                                             do_schedule();
                                         }
                                     }>
                                    <input
                                        type="date"
                                        node_ref=date_ref
                                        class="text-sm rounded-lg border border-amber-400 px-2 py-1.5
                                               bg-white dark:bg-stone-800
                                               text-stone-800 dark:text-stone-200
                                               focus:outline-none focus:ring-1 focus:ring-amber-500"
                                        prop:value=move || date_input.get()
                                        on:input=move |ev| date_input.set(event_target_value(&ev))
                                    />
                                    <button class=hint_btn on:click=move |_| do_schedule()>
                                        "Set due date"
                                    </button>
                                </div>
                            }.into_any(),
                            SubMode::Attach => view! {
                                <div class="space-y-1"
                                     on:keydown=move |ev: web_sys::KeyboardEvent| {
                                         match ev.key().as_str() {
                                             "ArrowDown" => {
                                                 ev.prevent_default();
                                                 picker_highlight.update(|h| {
                                                     *h = (*h + 1).min(picker_results.with(|r| r.len().saturating_sub(1)));
                                                 });
                                             }
                                             "ArrowUp" => {
                                                 ev.prevent_default();
                                                 picker_highlight.update(|h| *h = h.saturating_sub(1));
                                             }
                                             "Enter" => {
                                                 ev.prevent_default();
                                                 let pick = picker_results.with(|r| {
                                                     r.get(picker_highlight.get_untracked()).map(|p| p.node_id)
                                                 });
                                                 if let Some(id) = pick {
                                                     do_attach(id);
                                                 }
                                             }
                                             _ => {}
                                         }
                                     }>
                                    <input
                                        type="text"
                                        node_ref=picker_ref
                                        placeholder="Attach to node…"
                                        class="w-full text-sm rounded-lg border border-amber-400 px-2 py-1.5
                                               bg-white dark:bg-stone-800
                                               text-stone-800 dark:text-stone-200
                                               focus:outline-none focus:ring-1 focus:ring-amber-500"
                                        prop:value=move || picker_query.get()
                                        on:input=move |ev| picker_query.set(event_target_value(&ev))
                                    />
                                    {move || {
                                        let results = picker_results.get();
                                        (!results.is_empty()).then(|| view! {
                                            <div class="rounded-lg border border-stone-200 dark:border-stone-700
                                                        divide-y divide-stone-100 dark:divide-stone-800 overflow-hidden">
                                                {results.into_iter().enumerate().map(|(i, r)| {
                                                    let rid = r.node_id;
                                                    view! {
                                                        <button
                                                            class=move || {
                                                                let base = "w-full text-left px-3 py-1.5 text-sm \
                                                                            text-stone-700 dark:text-stone-300";
                                                                if picker_highlight.get() == i {
                                                                    format!("{base} bg-amber-50 dark:bg-amber-900/30")
                                                                } else {
                                                                    base.to_string()
                                                                }
                                                            }
                                                            on:click=move |_| do_attach(rid)
                                                        >
                                                            {r.title.clone()}
                                                        </button>
                                                    }
                                                }).collect_view()}
                                            </div>
                                        })
                                    }}
                                </div>
                            }.into_any(),
                            SubMode::Keys => ().into_any(),
                        }}

                        // Action hints (clickable)
                        <div class="flex items-center gap-2 flex-wrap">
                            <button class=hint_btn disabled=move || busy.get()
                                    on:click=move |_| do_today()>
                                <span class=kbd>"t"</span> "Today"
                            </button>
                            <button class=hint_btn disabled=move || busy.get()
                                    on:click=move |_| enter_schedule()>
                                <span class=kbd>"s"</span> "Schedule"
                            </button>
                            <button class=hint_btn disabled=move || busy.get()
                                    on:click=move |_| enter_attach()>
                                <span class=kbd>"a"</span> "Attach"
                            </button>
                            <button class=hint_btn disabled=move || busy.get()
                                    on:click=move |_| do_delete()>
                                <span class=kbd>"d"</span> "Delete"
                            </button>
                            <button class=hint_btn on:click=move |_| advance()>
                                <span class=kbd>"j"</span> "Skip"
                            </button>
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
}
