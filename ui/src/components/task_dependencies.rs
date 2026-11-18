//! A task's dependencies in the shared task editor (S5d; ONTOLOGY §9.3):
//! the tasks it waits for ("Blocked by", with a picker to add one and a
//! button to remove one) and the tasks that wait for it ("Blocks",
//! read-only).
//!
//! Unlike the editor's fields, a change here **applies at once**: it is
//! not part of the draft, so Save, Cancel and Escape's Undo don't cover
//! it (the `MyDayButton` precedent). The section is set apart and says so.
//!
//! Surfaced, never enforced: nothing here stops a task being worked on or
//! completed. The API cuts both lists to tasks the reader can see, and
//! says whether they may change them (`may_edit`), so a control is shown
//! only where the call would pass.

use common::id::{NodeId, TaskId};
use common::task::TaskStatus;
use common::task_dependency::{TaskDependencies, TaskLink, holds_back};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::components::icon_button::IconButton;
use crate::components::task_row_scaffold::state_badge_view;
use crate::components::toast::{ToastLevel, push_toast};

/// The picker's debounce, the node picker's.
const SEARCH_DEBOUNCE_MS: u32 = 300;

/// Unique DOM ids per picker (`aria-controls`, option ids).
static NEXT_PICKER_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A linked task's title as plain text: it may be someone else's words,
/// and it sits inside a link.
fn plain_title(link: &TaskLink) -> String {
    crate::markdown::markdown_inline_plain_text(&link.title)
}

/// The badge of a linked task's state. Open is the unmarked case; Done is
/// said here (a row's checkbox says it elsewhere), since a done
/// prerequisite no longer holds the task back.
fn link_state_view(status: &TaskStatus) -> Option<AnyView> {
    match status {
        TaskStatus::Done => Some(
            view! {
                <span class="flex-shrink-0 px-1.5 rounded bg-stone-100 dark:bg-stone-800 \
                             text-stone-500 dark:text-stone-400"
                    data-testid="task-state">
                    {status.label()}
                </span>
            }
            .into_any(),
        ),
        other => state_badge_view(other),
    }
}

/// One linked task: its state, its title as a link to it, its Project's
/// name when that is another Project than the edited task's.
#[component]
fn LinkRow(
    link: TaskLink,
    own_node: NodeId,
    /// Remove the link; `None` where the reader may not, and for "Blocks".
    #[prop(optional_no_strip)]
    on_remove: Option<Callback<TaskId>>,
    /// For a task further down the chain (S7b): the edited task's own
    /// prerequisite it comes through.
    #[prop(optional_no_strip)]
    through: Option<String>,
    #[prop(into)] busy: Signal<bool>,
) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let title = plain_title(&link);
    // A finished task sits in its node's folded "completed" list, where
    // `?task=` can't land: link to the node (as Data health does).
    let href = if link.status.is_terminal() {
        format!("/nodes/{}", link.node_id.0)
    } else {
        crate::components::task_common::task_href(link.id.0, link.node_id.0)
    };
    let target = href.clone();
    let id = link.id;
    let elsewhere = (link.node_id != own_node).then(|| link.node_title.clone());
    // A prerequisite that is done is shown, struck through: it is still
    // linked, and no longer in the way.
    let title_class = if holds_back(&link.status) {
        "min-w-0 break-words text-stone-700 dark:text-stone-300 hover:underline"
    } else {
        "min-w-0 break-words text-stone-400 dark:text-stone-500 line-through hover:underline"
    };
    let remove_label = format!("Stop waiting for {title}");
    view! {
        <li class="flex items-center gap-1.5 min-w-0" data-testid="dependency">
            {link_state_view(&link.status)}
            <a
                href=href
                class=title_class
                // The editor keeps clicks to itself (rows navigate on
                // click), so the router never sees this one: navigate here.
                on:click=move |ev: web_sys::MouseEvent| {
                    // A modified click is the browser's (a new tab).
                    if ev.button() != 0
                        || ev.ctrl_key()
                        || ev.meta_key()
                        || ev.shift_key()
                        || ev.alt_key()
                    {
                        return;
                    }
                    ev.prevent_default();
                    // Its row may be on this very page (the same
                    // Project's list): go to it. Navigating to `?task=`
                    // on the page already shown does nothing.
                    if crate::focus_task::focus_task_here(id.0) {
                        return;
                    }
                    navigate.get_value()(&target, Default::default());
                }
            >
                {title}
            </a>
            {elsewhere.map(|project| view! {
                <span class="flex-shrink-0 max-w-[10rem] truncate text-stone-400 dark:text-stone-500"
                    data-testid="dependency-project">
                    {project}
                </span>
            })}
            {through.map(|via| view! {
                <span class="min-w-0 truncate text-stone-400 dark:text-stone-500"
                    data-testid="dependency-through">
                    {format!("through {via}")}
                </span>
            })}
            <span class="flex-1" />
            {on_remove.map(|remove| view! {
                <IconButton
                    icon="link_off"
                    label=remove_label.clone()
                    disabled=busy
                    stop_propagation=true
                    on_click=Callback::new(move |()| remove.run(id))
                />
            })}
        </li>
    }
}

/// The combobox that chooses a task to wait for. It asks the API for what
/// can be added (`…/dependencies/candidates`): tasks still to do, never
/// one that would make a loop. Nothing typed lists the nearest tasks, the
/// same Project's first. Nothing is asked before the field is focused.
///
/// Keys as the node picker's: ↑/↓ move, Enter adds the highlighted task,
/// Escape closes the list, then clears the text, and only then is left to
/// the editor.
#[component]
fn TaskPicker(
    task_id: TaskId,
    own_node: NodeId,
    /// Bumped after a change, so the offer leaves out what was just added.
    reload: RwSignal<u32>,
    #[prop(into)] on_pick: Callback<TaskLink>,
    #[prop(into)] busy: Signal<bool>,
) -> impl IntoView {
    const LABEL: &str = "Wait for a task\u{2026}";
    let base = format!(
        "task-picker-{}",
        NEXT_PICKER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let list_id = format!("{base}-list");
    let opt_id = StoredValue::new(move |i: usize| format!("{base}-opt-{i}"));

    let query = RwSignal::new(String::new());
    let options = RwSignal::new(Vec::<TaskLink>::new());
    // The query the options answer; `None` until an answer is in.
    let answers = RwSignal::new(None::<String>);
    let failed = RwSignal::new(false);
    let armed = RwSignal::new(false);
    let open = RwSignal::new(false);
    let active = RwSignal::new(None::<usize>);
    let version = RwSignal::new(0u32);

    Effect::new(move |_| {
        let q = query.get();
        reload.track();
        if !armed.get() {
            return;
        }
        version.update(|v| *v += 1);
        let mine = version.get_untracked();
        answers.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            if !q.trim().is_empty() {
                gloo_timers::future::TimeoutFuture::new(SEARCH_DEBOUNCE_MS).await;
            }
            // `try_`: the editor may have closed during the wait.
            if version.try_get_untracked() != Some(mine) {
                return;
            }
            let got = crate::api::dependency_candidates(task_id, &q).await;
            if version.try_get_untracked() != Some(mine) {
                return;
            }
            match got {
                Ok(found) => {
                    let titles: Vec<String> = found.iter().map(plain_title).collect();
                    // Typed text highlights its best match; with nothing
                    // typed nothing is highlighted, so Enter adds nothing
                    // the person didn't choose.
                    let best = if q.trim().is_empty() {
                        None
                    } else {
                        common::picker::best_index(&q, &titles)
                    };
                    options.try_set(found);
                    active.try_set(best);
                    failed.try_set(false);
                    answers.try_set(Some(q));
                }
                Err(_) => {
                    options.try_set(Vec::new());
                    active.try_set(None);
                    failed.try_set(true);
                    answers.try_set(Some(q));
                }
            }
        });
    });

    // Asks the first time only: setting `armed` again would run the
    // search again on every click and blank the answer meanwhile.
    let arm_and_open = move || {
        if !armed.get_untracked() {
            armed.set(true);
        }
        open.set(true);
    };

    // Keep the highlighted option in view: the list scrolls.
    Effect::new(move |_| {
        if let Some(i) = active.get()
            && let Some(el) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id(&opt_id.get_value()(i)))
        {
            let opts = web_sys::ScrollIntoViewOptions::new();
            opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
            el.scroll_into_view_with_scroll_into_view_options(&opts);
        }
    });

    let fresh =
        move || answers.with_untracked(|a| a.as_deref() == Some(query.get_untracked().as_str()));
    let commit = move |i: usize| {
        if busy.get_untracked() {
            return;
        }
        let Some(link) = options.with_untracked(|o| o.get(i).cloned()) else {
            return;
        };
        query.set(String::new());
        open.set(false);
        active.set(None);
        on_pick.run(link);
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let len = options.with_untracked(Vec::len);
        match ev.key().as_str() {
            "ArrowDown" | "ArrowUp" => {
                ev.prevent_default();
                open.set(true);
                active.set(common::picker::step(
                    active.get_untracked(),
                    len,
                    ev.key() == "ArrowDown",
                ));
            }
            // Ctrl/⌘+Enter saves the task from any control: the editor's.
            "Enter" if ev.ctrl_key() || ev.meta_key() => {}
            "Enter" => {
                // Never the editor's Save: Enter here is about the list.
                ev.prevent_default();
                ev.stop_propagation();
                if fresh()
                    && open.get_untracked()
                    && let Some(i) = active.get_untracked()
                {
                    commit(i);
                }
            }
            "Escape" => {
                if open.get_untracked() && len > 0 {
                    open.set(false);
                } else if !query.get_untracked().is_empty() {
                    query.set(String::new());
                } else {
                    return; // nothing to do here: the editor has it
                }
                ev.prevent_default();
                ev.stop_propagation();
            }
            "Tab" => open.set(false),
            _ => {}
        }
    };

    let expanded = move || open.get() && !options.with(Vec::is_empty);
    let list_id_attr = list_id.clone();
    view! {
        <div class="relative" data-testid="task-picker">
            <div class="flex items-center gap-1.5">
                <span class="material-symbols-outlined text-stone-400 flex-shrink-0"
                    style="font-size: 14px;" aria-hidden="true">"add_link"</span>
                <input
                    type="text"
                    role="combobox"
                    aria-autocomplete="list"
                    aria-controls=list_id_attr
                    aria-expanded=move || if expanded() { "true" } else { "false" }
                    aria-activedescendant=move || {
                        active.get().filter(|_| expanded()).map(|i| opt_id.get_value()(i))
                    }
                    placeholder=LABEL
                    aria-label=LABEL
                    class="flex-1 min-w-0 text-xs bg-stone-100 dark:bg-stone-700
                        text-stone-700 dark:text-stone-300 rounded-lg px-2 py-1 focus:outline-none
                        focus:ring-1 focus:ring-amber-500"
                    prop:value=move || query.get()
                    on:input=move |ev| {
                        query.set(event_target_value(&ev));
                        open.set(true);
                    }
                    on:focus=move |_| arm_and_open()
                    // After a task is added the field keeps its focus, so
                    // no focus event comes: a click must open the list too.
                    on:click=move |_| arm_and_open()
                    on:blur=move |_| open.set(false)
                    on:keydown=on_keydown
                />
            </div>
            // Said whether or not the list is open: an empty answer and a
            // failed one must not look alike.
            {move || {
                let answered = answers.with(Option::is_some);
                if failed.get() {
                    Some(view! {
                        <p class="mt-1 text-stone-500 dark:text-stone-400" role="alert">
                            "Couldn\u{2019}t search your tasks. Type again to retry."
                        </p>
                    }.into_any())
                } else if open.get() && answered && options.with(Vec::is_empty) {
                    Some(view! {
                        <p class="mt-1 text-stone-400 dark:text-stone-500"
                            data-testid="task-picker-empty">
                            "No task to wait for here."
                        </p>
                    }.into_any())
                } else {
                    None
                }
            }}
            {move || expanded().then(|| {
                let opts = options.get();
                view! {
                    <div
                        id=list_id.clone()
                        role="listbox"
                        aria-label="Tasks"
                        class="absolute z-30 mt-1 w-full bg-white dark:bg-stone-900
                            border border-stone-200 dark:border-stone-700 rounded-xl shadow-md
                            overflow-hidden max-h-56 overflow-y-auto"
                    >
                        {opts.into_iter().enumerate().map(|(i, link)| {
                            let title = plain_title(&link);
                            let elsewhere =
                                (link.node_id != own_node).then(|| link.node_title.clone());
                            view! {
                                <div
                                    id=opt_id.get_value()(i)
                                    role="option"
                                    aria-selected=move || {
                                        if active.get() == Some(i) { "true" } else { "false" }
                                    }
                                    class=move || format!(
                                        "w-full flex items-center gap-2 px-3 py-2 text-xs \
                                         text-stone-700 dark:text-stone-300 border-b \
                                         border-stone-50 dark:border-stone-800 last:border-b-0 \
                                         cursor-pointer {}",
                                        if active.get() == Some(i) {
                                            "bg-amber-50 dark:bg-stone-800"
                                        } else {
                                            ""
                                        }
                                    )
                                    on:mouseenter=move |_| active.set(Some(i))
                                    // mousedown, not click: focus stays in the
                                    // input, so its blur doesn't close the list
                                    // mid-press.
                                    on:mousedown=move |ev: web_sys::MouseEvent| {
                                        ev.prevent_default();
                                        // The main button only: a right-click
                                        // must not write a dependency.
                                        if ev.button() == 0
                                            && common::picker::is_pointer_press(ev.detail())
                                        {
                                            commit(i);
                                        }
                                    }
                                    // An accessibility press is a bare click (#174).
                                    on:click=move |ev: web_sys::MouseEvent| {
                                        if !common::picker::is_pointer_press(ev.detail()) {
                                            commit(i);
                                        }
                                    }
                                >
                                    <span class="min-w-0 break-words text-left">{title}</span>
                                    {state_badge_view(&link.status)}
                                    {elsewhere.map(|project| view! {
                                        <span class="ml-auto flex-shrink-0 max-w-[9rem] truncate \
                                                     text-[10px] text-stone-400">
                                            {project}
                                        </span>
                                    })}
                                </div>
                            }
                        }).collect_view()}
                    </div>
                }
            })}
        </div>
    }
}

/// Whether no element has the focus (it is on the page's body).
fn focus_is_nowhere() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .is_none_or(|el| el.tag_name().eq_ignore_ascii_case("BODY"))
}

/// One change to a task's dependencies.
#[derive(Clone, Copy)]
enum Change {
    Add(TaskId),
    Remove(TaskId),
    RemoveNotShown,
}

/// What the section says of prerequisites its reader isn't shown.
fn not_shown_text(n: u32) -> String {
    let tasks = if n == 1 {
        "1 more task"
    } else {
        &format!("{n} more tasks")
    };
    format!(
        "Also waits for {tasks} not shown here: in the trash, or on a Project you can\u{2019}t see."
    )
}

/// The name of the button that removes them.
fn not_shown_remove_label(n: u32) -> String {
    if n == 1 {
        "Stop waiting for the task not shown".to_string()
    } else {
        format!("Stop waiting for the {n} tasks not shown")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Failed,
    Loaded,
}

/// The dependencies of the task being edited. `blocked_by` is the row's
/// own count: it is set from what is loaded and after each change, so the
/// row's "Blocked" chip is right when the editor closes, without a
/// refetch of the list (which would rebuild the open editor).
#[component]
pub fn TaskDependenciesSection(
    task_id: TaskId,
    /// The edited task's node: a linked task on another one names its own.
    node_id: NodeId,
    blocked_by: RwSignal<u32>,
) -> impl IntoView {
    let deps = RwSignal::new(None::<Result<TaskDependencies, ()>>);
    let load = RwSignal::new(0u32);
    let offer = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let root: NodeRef<leptos::html::Div> = NodeRef::new();

    // The lists hold each task's count as they last loaded it. Once
    // something here changed, they must load again when the editor goes,
    // or a row rebuilt from that snapshot (a reorder, a section folded and
    // opened) would show the old count. After the unmount, not during it.
    let changed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task_refresh = expect_context::<crate::app::TaskRefresh>().0;
    on_cleanup({
        let changed = changed.clone();
        move || {
            if changed.load(std::sync::atomic::Ordering::Relaxed) {
                wasm_bindgen_futures::spawn_local(async move {
                    task_refresh.try_update(|n| *n += 1);
                });
            }
        }
    });

    // What the server says now, into the lists and the row's count.
    let apply = move |fresh: TaskDependencies| {
        blocked_by.try_set(fresh.holding_back());
        deps.try_set(Some(Ok(fresh)));
    };

    Effect::new(move |_| {
        load.track();
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::fetch_task_dependencies(task_id).await {
                Ok(fresh) => apply(fresh),
                Err(_) => {
                    deps.try_set(Some(Err(())));
                }
            }
        });
    });

    // One change at a time; each answers with the fresh lists.
    let change = Callback::new(move |change: Change| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let changed = changed.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let (result, failed) = match change {
                Change::Add(other) => (
                    crate::api::add_task_dependency(task_id, other).await,
                    "Couldn\u{2019}t add the dependency",
                ),
                Change::Remove(other) => (
                    crate::api::remove_task_dependency(task_id, other).await,
                    "Couldn\u{2019}t remove the dependency",
                ),
                Change::RemoveNotShown => (
                    crate::api::remove_task_dependencies_not_shown(task_id).await,
                    "Couldn\u{2019}t remove them",
                ),
            };
            busy.try_set(false);
            match result {
                Ok(fresh) => {
                    changed.store(true, std::sync::atomic::Ordering::Relaxed);
                    apply(fresh);
                    offer.try_update(|n| *n += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("{failed}: {e}")),
            }
            // The button that was pressed may be gone with its row (a
            // remove), leaving the focus on nothing: the next key would
            // then go to the page's own shortcuts, not the editor. Put it
            // back in the section, once the lists have been redrawn.
            gloo_timers::future::TimeoutFuture::new(0).await;
            if focus_is_nowhere()
                && let Some(section) = root.try_get_untracked().flatten()
            {
                let _ = section.focus();
            }
        });
    });
    let on_pick = Callback::new(move |link: TaskLink| change.run(Change::Add(link.id)));
    let on_remove = Callback::new(move |other: TaskId| change.run(Change::Remove(other)));
    let on_remove_not_shown = Callback::new(move |()| change.run(Change::RemoveNotShown));

    // Which of the three the section shows. A memo, so a change to the
    // lists doesn't rebuild the section around the picker.
    let phase = Memo::new(move |_| {
        deps.with(|d| match d {
            None => Phase::Loading,
            Some(Err(())) => Phase::Failed,
            Some(Ok(_)) => Phase::Loaded,
        })
    });
    let may_edit =
        Memo::new(move |_| deps.with(|d| matches!(d, Some(Ok(found)) if found.may_edit)));
    // Prerequisites this reader isn't shown. The API tells only someone
    // who may edit the task; for anyone else it is 0.
    let not_shown = Memo::new(move |_| {
        deps.with(|d| match d {
            Some(Ok(found)) if found.may_edit => found.not_shown,
            _ => 0,
        })
    });
    let lists = Memo::new(move |_| {
        deps.with(|d| match d {
            Some(Ok(found)) => (found.blocked_by.clone(), found.blocks.clone()),
            _ => (Vec::new(), Vec::new()),
        })
    });
    // What the prerequisites wait for in turn (S7b), and whether the
    // chain goes on past what was read.
    let further = Memo::new(move |_| {
        deps.with(|d| match d {
            Some(Ok(found)) => (found.also.clone(), found.also_truncated),
            _ => (Vec::new(), false),
        })
    });

    const HEADING: &str =
        "font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400";
    view! {
        <div
            node_ref=root
            // Focusable by script only: where the focus returns after a
            // remove (see `change`).
            tabindex="-1"
            class="pt-2 border-t border-stone-200 dark:border-stone-700 space-y-1.5 text-xs
                focus:outline-none"
            role="group"
            aria-label="Dependencies"
            data-testid="task-dependencies"
        >
            {move || match phase.get() {
                Phase::Loading => view! {
                    <p class="text-stone-400 dark:text-stone-500">"Loading dependencies\u{2026}"</p>
                }.into_any(),
                Phase::Failed => view! {
                    <crate::components::load_error::LoadError
                        what="this task\u{2019}s dependencies"
                        on_retry=Callback::new(move |()| {
                            deps.set(None);
                            load.update(|n| *n += 1);
                        })
                    />
                }.into_any(),
                // Each part below reads the lists itself. The picker reads
                // only `may_edit`, so a change (which replaces the lists)
                // doesn't rebuild it: it keeps its focus for the next one.
                Phase::Loaded => view! {
                    <div class="flex items-baseline gap-2 flex-wrap">
                        <span class=HEADING>"Blocked by"</span>
                        {move || may_edit.get().then(|| view! {
                            <span class="text-stone-400 dark:text-stone-500">
                                "Changes here apply at once."
                            </span>
                        })}
                    </div>
                    {move || {
                        let links = lists.with(|l| l.0.clone());
                        let may_edit = may_edit.get();
                        if links.is_empty() {
                            return (!may_edit).then(|| view! {
                                <p class="text-stone-400 dark:text-stone-500">
                                    "It waits for no other task."
                                </p>
                            }).into_any();
                        }
                        view! {
                            <ul class="space-y-0.5" aria-label="Blocked by"
                                data-testid="blocked-by-list">
                                {links.into_iter().map(|link| view! {
                                    <LinkRow
                                        link=link
                                        own_node=node_id
                                        on_remove=may_edit.then_some(on_remove)
                                        through=None
                                        busy=busy
                                    />
                                }).collect_view()}
                            </ul>
                        }.into_any()
                    }}
                    // Not named, and not counted as blocking, but still
                    // part of what a loop is: theirs to remove.
                    {move || {
                        let n = not_shown.get();
                        (n > 0).then(|| view! {
                            <div class="flex items-center gap-1.5 text-stone-500 dark:text-stone-400"
                                data-testid="dependencies-not-shown">
                                <span class="min-w-0">{not_shown_text(n)}</span>
                                <span class="flex-1" />
                                <IconButton
                                    icon="link_off"
                                    label=not_shown_remove_label(n)
                                    disabled=busy
                                    stop_propagation=true
                                    on_click=on_remove_not_shown
                                />
                            </div>
                        })
                    }}
                    {move || may_edit.get().then(|| view! {
                        <TaskPicker
                            task_id=task_id
                            own_node=node_id
                            reload=offer
                            on_pick=on_pick
                            busy=busy
                        />
                    })}
                    // Not this task's own links: shown, never removed here.
                    {move || {
                        let (chain, more) = further.get();
                        (!chain.is_empty() || more).then(|| view! {
                            <div class=HEADING>"Further down the chain"</div>
                            <ul class="space-y-0.5" aria-label="Further down the chain"
                                data-testid="chain-list">
                                {chain.into_iter().map(|c| view! {
                                    <LinkRow
                                        link=c.task
                                        own_node=node_id
                                        on_remove=None
                                        // Plain text, as every linked task's
                                        // title is: it may be someone else's.
                                        through=Some(crate::markdown::markdown_inline_plain_text(&c.via_title))
                                        busy=busy
                                    />
                                }).collect_view()}
                            </ul>
                            {more.then(|| view! {
                                <p class="text-stone-400 dark:text-stone-500" data-testid="chain-more">
                                    "The chain goes on: only its first links are shown."
                                </p>
                            })}
                        })
                    }}
                    {move || {
                        let links = lists.with(|l| l.1.clone());
                        (!links.is_empty()).then(|| view! {
                            <div class=HEADING>"Blocks"</div>
                            <ul class="space-y-0.5" aria-label="Blocks" data-testid="blocks-list">
                                {links.into_iter().map(|link| view! {
                                    <LinkRow link=link own_node=node_id on_remove=None through=None busy=busy />
                                }).collect_view()}
                            </ul>
                        })
                    }}
                }.into_any(),
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_about_tasks_not_shown_counts_and_names_none() {
        assert_eq!(
            not_shown_text(1),
            "Also waits for 1 more task not shown here: in the trash, or on a Project you can\u{2019}t see."
        );
        assert!(not_shown_text(3).starts_with("Also waits for 3 more tasks not shown here"));
        assert_eq!(
            not_shown_remove_label(1),
            "Stop waiting for the task not shown"
        );
        assert_eq!(
            not_shown_remove_label(2),
            "Stop waiting for the 2 tasks not shown"
        );
    }
}
