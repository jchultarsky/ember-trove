//! A project's status as a structured value (#143): the read-only badge
//! (node list, dashboard), the editable chip in a project's header, and
//! the picker behind it.
//!
//! Values are open: the picker suggests the statuses you already use, and
//! "active" offers your existing "Active" rather than a second spelling.
//! The matching rules live in `common::project_status` (unit-tested).

use common::id::NodeId;
use common::node::UpdateNodeRequest;
use common::project_status::{PickerOption, picker_options};
use leptos::prelude::*;

use crate::components::toast::{ToastLevel, push_toast};

/// Pill classes shared by the badge and the chip. Neutral stone: a status
/// is information, and warm colour is reserved for urgency.
const PILL_CLASS: &str = "inline-flex items-center gap-1 max-w-[12rem] px-2 py-0.5 rounded-full \
     text-xs font-medium bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-300 \
     border border-stone-200 dark:border-stone-700";

/// The status as a read-only pill (node list rows, dashboard cards).
#[component]
pub fn ProjectStatusBadge(status: String) -> impl IntoView {
    let title = format!("Status: {status}");
    view! {
        <span class=PILL_CLASS title=title data-testid="project-status-badge">
            <span class="truncate">{status}</span>
        </span>
    }
}

/// The project header's status chip: shows the status and, for editors,
/// opens [`StatusPicker`] on click. Saves straight away, and goes back to
/// the old value if the save fails.
#[component]
pub fn ProjectStatusChip(
    node_id: NodeId,
    initial: Option<String>,
    editable: bool,
) -> impl IntoView {
    let status = RwSignal::new(initial);
    let open = RwSignal::new(false);

    let save = move |next: Option<String>| {
        open.set(false);
        let prev = status.get_untracked();
        if next == prev {
            return;
        }
        status.set(next.clone());
        let req = UpdateNodeRequest {
            title: None,
            body: None,
            metadata: None,
            status: None,
            project_status: Some(next),
            expected_updated_at: None,
        };
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = crate::api::update_node(node_id, &req).await {
                status.try_set(prev);
                push_toast(ToastLevel::Error, format!("Couldn't save the status: {e}"));
            }
        });
    };

    view! {
        <div class="relative" data-testid="project-status">
            {move || {
                let label = status.get();
                if !editable {
                    return label.map(|s| view! { <ProjectStatusBadge status=s /> }.into_any());
                }
                let text = label.clone().unwrap_or_else(|| "Set status".to_string());
                let name = match &label {
                    Some(s) => format!("Status: {s}. Change status"),
                    None => "Set status".to_string(),
                };
                Some(view! {
                    <button
                        type="button"
                        class=format!("{PILL_CLASS} cursor-pointer hover:border-stone-400 dark:hover:border-stone-500")
                        style=if label.is_none() { "border-style:dashed;" } else { "" }
                        title=name.clone()
                        aria-label=name
                        aria-haspopup="listbox"
                        aria-expanded=move || if open.get() { "true" } else { "false" }
                        on:click=move |_| open.update(|v| *v = !*v)
                    >
                        <span class="truncate">{text}</span>
                        <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">
                            "expand_more"
                        </span>
                    </button>
                }.into_any())
            }}
            {move || open.get().then(|| view! {
                <div class="fixed inset-0 z-20" on:click=move |_| open.set(false) />
                <div class="absolute left-0 top-full mt-1 z-30 w-64 rounded-xl border border-stone-200
                    dark:border-stone-700 bg-white dark:bg-stone-900 shadow-lg p-2">
                    <StatusPicker
                        current=status.get_untracked()
                        on_commit=Callback::new(save)
                        on_close=Callback::new(move |()| open.set(false))
                    />
                </div>
            })}
        </div>
    }
}

/// Unique DOM ids per picker instance.
static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Status picker: an APG combobox like `NodePicker`, over the statuses you
/// already use (`GET /api/nodes/project-statuses`) plus "Use "…"" for a new
/// one. ↑/↓ move, Enter commits the active option, and Escape closes the
/// popover (stopped, so the page's own Escape doesn't also run). "Clear"
/// removes the status.
#[component]
pub fn StatusPicker(
    current: Option<String>,
    on_commit: Callback<Option<String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let base = format!(
        "status-picker-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let list_id = format!("{base}-list");
    let opt_id = StoredValue::new(move |i: usize| format!("{base}-opt-{i}"));

    let query = RwSignal::new(String::new());
    let suggestions = LocalResource::new(crate::api::fetch_project_statuses);
    let options = Memo::new(move |_| {
        let known: Vec<String> = suggestions
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.status)
            .collect();
        picker_options(&query.get(), &known)
    });
    let active = RwSignal::new(None::<usize>);
    // Typing re-highlights the best option (the first).
    Effect::new(move |_| {
        let empty = options.with(Vec::is_empty);
        active.set((!empty).then_some(0));
    });

    let input_ref: NodeRef<leptos::html::Input> = NodeRef::new();
    Effect::new(move |_| {
        if let Some(el) = input_ref.get() {
            let _ = el.focus();
        }
    });

    let commit = move |i: usize| {
        if let Some(opt) = options.with_untracked(|o| o.get(i).cloned()) {
            on_commit.run(Some(opt.value().to_string()));
        }
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "ArrowDown" | "ArrowUp" => {
            ev.prevent_default();
            let len = options.with_untracked(Vec::len);
            active.set(common::picker::step(
                active.get_untracked(),
                len,
                ev.key() == "ArrowDown",
            ));
        }
        "Enter" => {
            ev.prevent_default();
            ev.stop_propagation();
            if let Some(i) = active.get_untracked() {
                commit(i);
            }
        }
        "Escape" => {
            ev.prevent_default();
            ev.stop_propagation();
            on_close.run(());
        }
        _ => {}
    };

    let has_current = current.is_some();
    let list_id_attr = list_id.clone();
    view! {
        <div class="space-y-1.5" data-testid="status-picker">
            <input
                node_ref=input_ref
                type="text"
                role="combobox"
                aria-label="Status"
                aria-autocomplete="list"
                aria-controls=list_id_attr
                aria-expanded=move || if options.with(Vec::is_empty) { "false" } else { "true" }
                aria-activedescendant=move || active.get().map(|i| opt_id.get_value()(i))
                placeholder="Type a status\u{2026}"
                maxlength=common::project_status::MAX_LEN.to_string()
                class="w-full text-xs bg-stone-100 dark:bg-stone-800 text-stone-700 dark:text-stone-200
                    rounded-lg px-2 py-1.5 focus:outline-none focus:ring-1 focus:ring-amber-500"
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
                on:keydown=on_keydown
            />
            <div id=list_id role="listbox" aria-label="Statuses" class="max-h-48 overflow-y-auto">
                {move || options.get().into_iter().enumerate().map(|(i, opt)| {
                    let (label, is_new) = match &opt {
                        PickerOption::Existing(s) => (s.clone(), false),
                        PickerOption::New(s) => (format!("Use \u{201c}{s}\u{201d}"), true),
                    };
                    view! {
                        <div
                            id=opt_id.get_value()(i)
                            role="option"
                            aria-selected=move || if active.get() == Some(i) { "true" } else { "false" }
                            class=move || format!(
                                "flex items-center gap-2 px-2 py-1.5 rounded-lg text-xs cursor-pointer \
                                 text-stone-700 dark:text-stone-300 {}",
                                if active.get() == Some(i) { "bg-amber-50 dark:bg-stone-800" } else { "" }
                            )
                            on:mouseenter=move |_| active.set(Some(i))
                            // mousedown keeps focus in the input.
                            on:mousedown=move |ev: web_sys::MouseEvent| {
                                ev.prevent_default();
                                if common::picker::is_pointer_press(ev.detail()) {
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
                            <span class="material-symbols-outlined text-stone-400" style="font-size: 14px;"
                                aria-hidden="true">{if is_new { "add" } else { "label" }}</span>
                            <span class="truncate">{label}</span>
                        </div>
                    }
                }).collect_view()}
            </div>
            {has_current.then(|| view! {
                <button
                    type="button"
                    class="w-full text-left px-2 py-1.5 rounded-lg text-xs text-stone-500
                        hover:bg-stone-100 dark:hover:bg-stone-800 cursor-pointer"
                    on:click=move |_| on_commit.run(None)
                >
                    "Clear status"
                </button>
            })}
        </div>
    }
}
