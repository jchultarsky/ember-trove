//! Inbox — standalone tasks not yet associated with any node.
//!
//! Shows all tasks where `node_id IS NULL`, lets the user create new ones
//! inline, and provides the standard toggle/edit/delete/My-Day actions.
//!
//! Layout is mobile-first: each task renders as a self-contained card with
//! an always-visible action bar so controls are reachable without hover.

use common::{
    id::NodeId,
    task::{Task, UpdateTaskRequest},
};
use leptos::prelude::*;

use crate::app::TaskRefresh;
use crate::components::due_date_field::DueDateField;
use crate::components::empty_state::EmptyState;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::new_task_form::NewTaskForm;
use crate::components::node_picker::use_node_search;
use crate::components::page_header::PageHeader;
use crate::components::task_actions::{
    PatchOpts, RowSignals, TaskCtx, TaskEditDraft, delete_with_undo, patch_task, toggle_done,
    toggle_my_day,
};
use crate::components::task_common::{is_in_my_day, parse_priority, recurrence_label, status_done};
use crate::components::task_row::TaskEditorHeights;
use crate::components::task_row_scaffold::{
    MyDayButton, RecurrenceOptions, TITLE_CLASS, TaskCheckbox, TaskRowBody, action_btn_class,
    due_badge_view, priority_dot_view,
};
use crate::components::toast::{ToastLevel, ToastState, push_toast};
use crate::focus_task::schedule_focus_task;

// ── InboxView ─────────────────────────────────────────────────────────────────

#[component]
pub fn InboxView() -> impl IntoView {
    let task_refresh = expect_context::<TaskRefresh>();
    let refresh = task_refresh.0;

    // If we got here via the iOS Web Share Target SW handler (which 303s to
    // /tasks/inbox?captured=1) or the home-screen "Quick capture" shortcut,
    // confirm the capture with a toast and strip the marker from the URL so
    // a refresh doesn't re-fire it.  Runs once per mount.
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
        if let Ok(url) = web_sys::Url::new(&href) {
            let params = url.search_params();
            if params.get("captured").as_deref() == Some("1") {
                push_toast(ToastLevel::Success, "Captured to Inbox");
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
            }
        }
    });

    // v2.6.2: when navigation arrived via the Kanban row click on a
    // standalone (Inbox) task, scroll to and briefly highlight the
    // matching row.  See `crate::focus_task`.
    schedule_focus_task();

    let tasks_res = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_inbox().await }
    });

    // Saved inline-editor heights for the rows.
    TaskEditorHeights::provide();

    // Triage ("Process") mode — one task at a time, keyboard-driven.
    let triage = RwSignal::new(false);

    view! {
        <div class="flex flex-col h-full">
            // ── Header ────────────────────────────────────────────────────────
            <PageHeader
                icon="inbox"
                title="Inbox"
                subtitle="Capture tasks — link to a node when ready".into_any()
            >
                {move || (!triage.get()).then(|| view! {
                    <button
                        class="flex items-center gap-1.5 text-sm px-3 py-1.5 rounded-lg
                               border border-stone-200 dark:border-stone-700
                               text-stone-600 dark:text-stone-300
                               hover:border-amber-400 hover:text-amber-600 dark:hover:text-amber-400
                               transition-colors cursor-pointer"
                        title="Process the inbox one task at a time (keyboard-driven)"
                        on:click=move |_| triage.set(true)
                    >
                        <span class="material-symbols-outlined" style="font-size:16px;">"playlist_add_check"</span>
                        "Process"
                    </button>
                })}
            </PageHeader>

            // ── Scrollable content ────────────────────────────────────────────
            <div class="flex-1 overflow-auto px-4 py-4 space-y-4">

                {move || triage.get().then(|| view! {
                    <crate::components::inbox_triage::InboxTriage
                        refresh=refresh
                        on_exit=Callback::new(move |()| triage.set(false))
                    />
                })}

                <div class=move || if triage.get() { "hidden" } else { "contents" }>
                // ── Add-task form — shared with the node-detail TaskPanel ─────
                // node_id omitted ⇒ standalone capture with the optional picker.
                <NewTaskForm refresh=refresh />

                // ── Task list ─────────────────────────────────────────────────
                <Suspense fallback=|| view! {
                    <div class="px-4">
                        <crate::components::skeleton::SkeletonList rows=6 />
                    </div>
                }>
                    {move || {
                        let tasks = tasks_res.get()
                            .and_then(|r| r.ok())
                            .unwrap_or_default();
                        if tasks.is_empty() {
                            return view! {
                                <EmptyState icon="check_circle" message="Inbox zero!" />
                            }.into_any();
                        }
                        let (active, done): (Vec<Task>, Vec<Task>) =
                            tasks.into_iter().partition(|t| !status_done(&t.status));
                        let done_count  = done.len();
                        let show_done   = RwSignal::new(false);
                        let done_stored = StoredValue::new(done);
                        view! {
                            // One bordered container of flat rows (divide-y),
                            // matching the My Day zone-box look.
                            <div class="rounded-lg border border-stone-100 dark:border-stone-800 \
                                        bg-white dark:bg-stone-900 \
                                        divide-y divide-stone-100 dark:divide-stone-800 \
                                        overflow-hidden">
                                {active.into_iter().map(|task| view! {
                                    <InboxTaskRow task=task refresh=refresh />
                                }).collect_view()}

                                // Completed section toggle
                                {(done_count > 0).then(|| view! {
                                    <button
                                        class="w-full flex items-center gap-1.5 px-2 py-2 mt-1
                                            text-xs text-stone-400 hover:text-stone-600
                                            dark:hover:text-stone-300 transition-colors cursor-pointer"
                                        on:click=move |_| show_done.update(|v| *v = !*v)
                                    >
                                        <span class="material-symbols-outlined" style="font-size: 14px;">
                                            {move || if show_done.get() { "expand_more" } else { "chevron_right" }}
                                        </span>
                                        {move || if show_done.get() {
                                            format!("Hide {done_count} completed")
                                        } else {
                                            format!("{done_count} completed")
                                        }}
                                    </button>
                                    {move || show_done.get().then(|| {
                                        done_stored.get_value().into_iter().map(|task| view! {
                                            <InboxTaskRow task=task refresh=refresh />
                                        }).collect_view()
                                    })}
                                })}
                            </div>
                        }.into_any()
                    }}
                </Suspense>
                </div>  // close triage-hidden wrapper
            </div>
        </div>
    }
}

// ── InboxTaskRow ──────────────────────────────────────────────────────────────

#[component]
fn InboxTaskRow(task: Task, refresh: RwSignal<u32>) -> impl IntoView {
    // Captured at setup for the undo closure, which outlives this row.
    let toast_state = use_context::<ToastState>();
    let task_id = task.id;
    let today = crate::components::format_helpers::local_today();

    let status_sig = RwSignal::new(task.status.clone());
    let busy = RwSignal::new(false);
    let focus_date = task.focus_date;
    let priority_val = RwSignal::new(task.priority.as_str().to_string());

    // Inline-edit state
    let editing = RwSignal::new(false);
    let orig_title = RwSignal::new(task.title.clone());
    let draft = TaskEditDraft::from_task(&task);
    let (edit_title, edit_priority, edit_due, edit_recurrence) =
        (draft.title, draft.priority, draft.due, draft.recurrence);

    // My Day toggle — mirrors server carry-forward so tasks set on a
    // previous day but still open still read as "in My Day" today.
    let in_my_day = RwSignal::new(is_in_my_day(&task, today));

    // Node-picker state
    let assigning = RwSignal::new(false);
    let picker_query = RwSignal::new(String::new());
    let picker_results = use_node_search(picker_query);

    let do_assign = move |node_id: NodeId| {
        assigning.set(false);
        picker_query.set(String::new());
        picker_results.set(vec![]);
        patch_task(
            task_id,
            UpdateTaskRequest {
                node_id: Some(Some(node_id)),
                ..Default::default()
            },
            PatchOpts {
                err_prefix: "Couldn't attach to node",
                refresh: Some(refresh),
                ..Default::default()
            },
            |_| {},
        );
    };

    let has_recurrence = task.recurrence.is_some();
    let recurrence_tip = task.recurrence.as_ref().map(|r| recurrence_label(r));
    let overdue = task
        .due_date
        .map(|d| !status_done(&task.status) && d < today)
        .unwrap_or(false);
    let due = task.due_date;

    let do_save = move || {
        let Some(req) = draft.take_request() else {
            return;
        };
        editing.set(false);
        let prev_title = orig_title.get_untracked();
        let prev_priority = priority_val.get_untracked();
        orig_title.set(draft.title.get_untracked().trim().to_string());
        priority_val.set(draft.priority.get_untracked());
        patch_task(
            task_id,
            req,
            PatchOpts {
                err_prefix: "Save failed",
                refresh: Some(refresh),
                ..Default::default()
            },
            // Roll back the optimistic title/priority display.
            move |ok| {
                if !ok {
                    orig_title.set(prev_title);
                    priority_val.set(prev_priority);
                }
            },
        );
    };

    let ctx = TaskCtx {
        today,
        refresh,
        toast_state,
    };

    let on_toggle = move |ev: web_sys::MouseEvent| {
        toggle_done(
            task_id,
            status_sig.get_untracked(),
            focus_date,
            Some(RowSignals {
                status: status_sig,
                busy,
            }),
            ctx,
            Some(&ev),
        );
    };

    let on_delete = move |_| delete_with_undo(task_id, Some(busy), ctx, || {});

    view! {
        <div class="px-3 py-2" data-task-id=task_id.0.to_string()>
            // Row: checkbox | body | actions — geometry per task_row_scaffold.
            <div class="flex items-start gap-2">
                    <TaskCheckbox
                        done=Signal::derive(move || status_done(&status_sig.get()))
                        on_toggle=on_toggle
                    />

                    // Title area
                    <div class="flex-1 min-w-0">
                        {move || if editing.get() {
                            // ── Edit form ──────────────────────────────────────
                            let saved_height = TaskEditorHeights::saved(task_id);
                            view! {
                                <div class="space-y-2">
                                    <crate::components::resizable_editor::ResizableEditor
                                        value=edit_title
                                        placeholder="Task title…"
                                        initial_height=saved_height
                                        on_submit=Callback::new(move |()| do_save())
                                        on_escape=Callback::new(move |()| {
                                            editing.set(false);
                                            draft.reset();
                                        })
                                        on_resize=TaskEditorHeights::on_resize(task_id)
                                        class="w-full text-sm rounded-lg border border-amber-400 \
                                            bg-white dark:bg-stone-800 px-3 py-2 resize-y min-h-[44px] \
                                            text-stone-900 dark:text-stone-100 \
                                            focus:outline-none focus:ring-1 focus:ring-amber-400".to_string()
                                    />
                                    // Edit controls — wrap on mobile
                                    <div class="flex items-center gap-2 flex-wrap">
                                        <select
                                            class="rounded-lg border border-stone-200
                                                dark:border-stone-700 bg-stone-50
                                                dark:bg-stone-800 px-2 py-1.5 text-xs
                                                text-stone-700 dark:text-stone-300
                                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                                            title="Priority"
                                    prop:value=move || edit_priority.get()
                                            on:change=move |ev| edit_priority.set(event_target_value(&ev))
                                        >
                                            <option value="high">"High"</option>
                                            <option value="medium">"Medium"</option>
                                            <option value="low">"Low"</option>
                                        </select>
                                        <DueDateField
                                            value=edit_due
                                            original=due
                                            wrapper_class="flex-1 min-w-0 flex flex-wrap items-center gap-x-2 gap-y-0.5"
                                            input_class="min-w-0 rounded-lg border \
                                                border-stone-200 dark:border-stone-700 \
                                                bg-stone-50 dark:bg-stone-800 \
                                                px-2 py-1.5 text-xs \
                                                text-stone-700 dark:text-stone-300 \
                                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                                        />
                                        <select
                                            class="rounded-lg border border-stone-200
                                                dark:border-stone-700 bg-stone-50
                                                dark:bg-stone-800 px-2 py-1.5 text-xs
                                                text-stone-700 dark:text-stone-300
                                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                                            title="Recurrence"
                                            on:change=move |ev| edit_recurrence.set(event_target_value(&ev))
                                        >
                                            <RecurrenceOptions value=edit_recurrence />
                                        </select>
                                    </div>
                                    // Save / cancel
                                    <div class="flex items-center gap-1">
                                        <IconButton
                                            icon="check"
                                            label="Save"
                                            variant=IconButtonVariant::Save
                                            on_click=Callback::new(move |()| do_save())
                                        />
                                        <IconButton
                                            icon="close"
                                            label="Cancel"
                                            on_click=Callback::new(move |()| {
                                                editing.set(false);
                                                draft.reset();
                                            })
                                        />
                                    </div>
                                </div>
                            }.into_any()
                        } else {
                            // ── Read mode: shared scaffold (title, then meta).
                            // Overdue now colours the due badge, not the title.
                            let title_line = view! {
                                {move || priority_dot_view(&parse_priority(&priority_val.get()))}
                                <span class=TITLE_CLASS
                                      style=move || if status_done(&status_sig.get()) {
                                          "text-decoration:line-through;"
                                      } else { "" }
                                      inner_html=move || crate::markdown::render_markdown_inline(&orig_title.get())>
                                </span>
                            }
                            .into_any();
                            let tip = recurrence_tip.unwrap_or("");
                            let meta_line = view! {
                                {due.map(|d| due_badge_view(d, overdue))}
                                {has_recurrence.then(|| view! {
                                    <span
                                        class="flex items-center gap-0.5 text-stone-400 dark:text-stone-500"
                                        title=format!("Repeats: {tip}")
                                    >
                                        <span class="material-symbols-outlined"
                                            style="font-size: 13px;">"repeat"</span>
                                        <span>{tip}</span>
                                    </span>
                                })}
                            }
                            .into_any();
                            view! { <TaskRowBody title_line=title_line meta_line=meta_line/> }
                                .into_any()
                        }}
                    </div>

                    // Actions — right cluster. Order matches My Day: context
                    // actions, edit, delete. While editing only ☀ stays: it
                    // applies immediately (same as the node panel's row).
                    <div class="flex items-center gap-0.5 flex-shrink-0">
                        {move || (!editing.get()).then(|| view! {
                            <button
                                class=action_btn_class("hover:text-amber-600 dark:hover:text-amber-500")
                                title="Assign to node"
                                on:click=move |_| assigning.set(true)
                            >
                                <span class="material-symbols-outlined" style="font-size:16px;">
                                    "call_merge"
                                </span>
                            </button>
                        })}
                        <MyDayButton
                            active=in_my_day
                            on_toggle=Callback::new(move |()| toggle_my_day(task_id, in_my_day, ctx))
                        />
                        {move || (!editing.get()).then(|| view! {
                            <button
                                class=action_btn_class("hover:text-stone-600 dark:hover:text-stone-300")
                                title="Edit"
                                on:click=move |_| editing.set(true)
                            >
                                <span class="material-symbols-outlined" style="font-size:16px;">
                                    "edit"
                                </span>
                            </button>
                            <button
                                class=action_btn_class("hover:text-red-500 dark:hover:text-red-400")
                                title="Delete"
                                on:click=on_delete
                            >
                                <span class="material-symbols-outlined" style="font-size:16px;">
                                    "delete"
                                </span>
                            </button>
                        })}
                    </div>
            </div>

            // ── Node picker — full-width expansion below the row ──────────────
            {move || assigning.get().then(|| view! {
                <div class="mt-2 rounded-lg px-3 py-3 space-y-2
                    bg-stone-50 dark:bg-stone-800/50">
                    // Search input row
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-stone-400 flex-shrink-0"
                            style="font-size: 16px;">"link"</span>
                        <input
                            type="text"
                            placeholder="Search nodes…"
                            autofocus=true
                            class="flex-1 min-w-0 text-sm rounded-lg border border-amber-400
                                bg-white dark:bg-stone-900 px-3 py-2
                                text-stone-900 dark:text-stone-100
                                focus:outline-none focus:ring-1 focus:ring-amber-400"
                            prop:value=move || picker_query.get()
                            on:input=move |ev| picker_query.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Escape" {
                                    assigning.set(false);
                                    picker_query.set(String::new());
                                    picker_results.set(vec![]);
                                }
                            }
                        />
                        <button
                            class="p-2 rounded-lg text-stone-400 hover:text-stone-600
                                dark:hover:text-stone-300 flex-shrink-0
                                transition-colors cursor-pointer"
                            title="Cancel"
                            on:click=move |_| {
                                assigning.set(false);
                                picker_query.set(String::new());
                                picker_results.set(vec![]);
                            }
                        >
                            <span class="material-symbols-outlined" style="font-size: 18px;">
                                "close"
                            </span>
                        </button>
                    </div>
                    // Results list
                    {move || {
                        let results = picker_results.get();
                        (!results.is_empty()).then(|| view! {
                            <div class="bg-white dark:bg-stone-900
                                border border-stone-200 dark:border-stone-700
                                rounded-xl shadow-md overflow-hidden">
                                {results.into_iter().map(|r| {
                                    let node_id = r.node_id;
                                    let title   = r.title.clone();
                                    let icon    = crate::components::node_meta::type_icon(&r.node_type);
                                    view! {
                                        <button
                                            class="w-full flex items-center gap-2.5 px-3 py-2.5
                                                text-sm text-stone-700 dark:text-stone-300
                                                hover:bg-amber-50 dark:hover:bg-stone-800
                                                active:bg-amber-100 dark:active:bg-stone-700
                                                border-b border-stone-50 dark:border-stone-800
                                                last:border-b-0 transition-colors cursor-pointer"
                                            on:click=move |_| do_assign(node_id)
                                        >
                                            <span class="material-symbols-outlined text-stone-400
                                                flex-shrink-0" style="font-size: 16px;">
                                                {icon}
                                            </span>
                                            <span class="truncate text-left">{title}</span>
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        })
                    }}
                </div>
            })}
        </div>
    }
}
