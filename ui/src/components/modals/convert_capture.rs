//! Convert a capture into exactly one thing (ONTOLOGY §9.1, #234): a new
//! node (optionally filed into its home), a note on a node, or a Task in a
//! Project. The server makes it and deletes the capture in one transaction.
//!
//! The text is prefilled and may be reworded. What the form would send is
//! checked with `ConvertCaptureRequest::checked`, the server's own rule, so
//! Convert is only enabled when the request would pass it. A Task's title
//! must fit (owner decision): it is reworded here, never cut.

use chrono::NaiveDate;
use common::{
    capture::{ConvertCaptureRequest, Converted, TITLE_MAX_LEN, split_title},
    id::{CaptureId, NodeId},
    node::NodeType,
    ontology::allowed_homes,
};
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use super::{ModalPlacement, ModalShell};
use crate::components::{
    node_meta::type_label,
    node_picker::NodePicker,
    toast::{ToastAction, ToastLevel, ToastState},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dest {
    Node,
    Note,
    Task,
}

impl Dest {
    const ALL: [Dest; 3] = [Dest::Node, Dest::Note, Dest::Task];
    fn label(self) -> &'static str {
        match self {
            Dest::Node => "Node",
            Dest::Note => "Note",
            Dest::Task => "Task",
        }
    }
}

const KINDS: [NodeType; 4] = [
    NodeType::Article,
    NodeType::Project,
    NodeType::Area,
    NodeType::Reference,
];

/// A Task's title is one line: the capture's lines joined, whole.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[component]
pub fn ConvertCaptureDialog(
    capture_id: CaptureId,
    text: String,
    show: RwSignal<bool>,
    /// Bumped after a conversion, so the Captures wall reloads without it.
    refresh: RwSignal<u32>,
) -> impl IntoView {
    let (first, rest) = split_title(&text);
    let text = StoredValue::new(text);
    let dest = RwSignal::new(Dest::Node);
    let kind = RwSignal::new(NodeType::Article);
    let title = RwSignal::new(first.clone());
    let body = RwSignal::new(rest.clone().unwrap_or_default());
    let home: RwSignal<Option<(NodeId, String)>> = RwSignal::new(None);
    let note_node: RwSignal<Option<(NodeId, String)>> = RwSignal::new(None);
    let note_text = RwSignal::new(text.get_value().trim().to_string());
    let project: RwSignal<Option<(NodeId, String)>> = RwSignal::new(None);
    let task_title = RwSignal::new(one_line(&text.get_value()));
    let due = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);

    // What Convert would send, if every choice is made.
    let request = move || -> Option<ConvertCaptureRequest> {
        Some(match dest.get() {
            Dest::Node => ConvertCaptureRequest::Node {
                node_type: kind.get(),
                title: title.get(),
                body: Some(body.get()),
                home: home.get().map(|(id, _)| id),
            },
            Dest::Note => ConvertCaptureRequest::Note {
                node_id: note_node.get()?.0,
                body: note_text.get(),
            },
            Dest::Task => ConvertCaptureRequest::Task {
                node_id: project.get()?.0,
                title: task_title.get(),
                due_date: NaiveDate::parse_from_str(&due.get(), "%Y-%m-%d").ok(),
                priority: None,
            },
        })
    };
    let ready = Signal::derive(move || request().is_some_and(|r| r.checked().is_ok()));

    // Keys reach ModalShell's Escape, Tab trap and Ctrl+Enter only from
    // inside the panel, so focus must be there: the Title on open (tracking
    // the NodeRef, no rAF; .claude/rules/leptos.md), and after a pick
    // (the picker's input turns into a chip and drops focus) the Convert
    // button, or the form itself while Convert is disabled.
    let form_ref: NodeRef<html::Div> = NodeRef::new();
    let title_ref: NodeRef<html::Input> = NodeRef::new();
    let convert_ref: NodeRef<html::Button> = NodeRef::new();
    Effect::new(move |_| {
        if !show.get() {
            return;
        }
        if let Some(el) = title_ref.get() {
            let _ = el.focus();
        } else if let Some(el) = form_ref.get() {
            let _ = el.focus();
        }
    });
    let after_pick = Callback::new(move |()| {
        let convert = convert_ref.get_untracked();
        match convert {
            Some(b) if ready.get_untracked() && !busy.get_untracked() => {
                // `prop:disabled` catches up on the next tick, and a
                // disabled button can't take focus: enable it now, as the
                // reactive prop is about to.
                b.set_disabled(false);
                let _ = b.focus();
            }
            _ => {
                if let Some(el) = form_ref.get_untracked() {
                    let _ = el.focus();
                }
            }
        }
    });

    let toast_state = use_context::<ToastState>();
    let nav = expect_context::<crate::app::NavRequest>().0;
    let submit_pending = RwSignal::new(false);
    Effect::new(move |_| {
        if !submit_pending.get() {
            return;
        }
        submit_pending.set(false);
        if busy.get_untracked() {
            return;
        }
        let Some(req) = untrack(request) else {
            error.set(Some("Choose where it goes first.".to_string()));
            return;
        };
        let req = match req.checked() {
            Ok(req) => req,
            Err(e) => {
                error.set(Some(e));
                return;
            }
        };
        // Named now: the row (and this dialog) goes once the list reloads.
        let made_in = match dest.get_untracked() {
            Dest::Node => type_label(kind.get_untracked().as_str()).to_string(),
            Dest::Note => note_node.get_untracked().map(|n| n.1).unwrap_or_default(),
            Dest::Task => project.get_untracked().map(|n| n.1).unwrap_or_default(),
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match crate::api::convert_capture(capture_id, &req).await {
                Ok(made) => {
                    let (message, path) = match made {
                        Converted::Node { id } => (format!("Made a new {made_in}"), id),
                        Converted::Note { node_id, .. } => (
                            format!("Added a note to \u{201c}{made_in}\u{201d}"),
                            node_id,
                        ),
                        Converted::Task { node_id, .. } => (
                            format!("Added a task to \u{201c}{made_in}\u{201d}"),
                            node_id,
                        ),
                    };
                    let path = format!("/nodes/{path}");
                    if let Some(ts) = toast_state {
                        ts.push_with_action(
                            ToastLevel::Success,
                            message,
                            ToastAction {
                                label: "Open",
                                on_click: std::sync::Arc::new(move || {
                                    nav.try_set(Some(path.clone()));
                                }),
                            },
                        );
                    }
                    // Stays busy: the capture is gone, so until the list
                    // reloads this row must not convert it again (a 404).
                    show.try_set(false);
                    refresh.try_update(|n| *n += 1);
                }
                Err(e) => {
                    error.try_set(Some(e.to_string()));
                    busy.try_set(false);
                }
            }
        });
    });

    let on_key = Callback::new(move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            submit_pending.set(true);
        }
    });

    let field = "w-full rounded-md border border-stone-200 dark:border-stone-700 bg-transparent \
                 px-2 py-1 text-sm text-stone-900 dark:text-stone-100";
    let counter = move |len: usize| {
        let class = if len > TITLE_MAX_LEN {
            "text-xs tabular-nums text-heat-max"
        } else {
            "text-xs tabular-nums text-stone-400"
        };
        view! {
            <span class=class data-testid="title-counter">
                {format!("{len} / {TITLE_MAX_LEN}")}
            </span>
        }
    };

    view! {
        <ModalShell
            show=show
            on_close=Callback::new(move |()| show.set(false))
            label="Convert capture"
            panel_class="w-full max-w-xl mt-8 sm:mt-16"
            placement=ModalPlacement::Top
            on_key=on_key
        >
            <div class="p-4 border-b border-stone-200 dark:border-stone-700">
                <h2 class="font-medium text-stone-800 dark:text-stone-100">"Convert capture"</h2>
                <p class="mt-1 text-xs text-stone-500 dark:text-stone-400">
                    "It becomes one thing, and the capture goes."
                </p>
                <div class="mt-3 inline-flex rounded-lg border border-stone-200 dark:border-stone-700 overflow-hidden"
                    role="group" aria-label="Convert into">
                    {Dest::ALL.into_iter().map(|d| view! {
                        <button type="button"
                            class=move || if dest.get() == d {
                                "px-3 py-1 text-sm bg-stone-800 text-white dark:bg-stone-200 dark:text-stone-900"
                            } else {
                                "px-3 py-1 text-sm text-stone-600 dark:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800"
                            }
                            aria-pressed=move || (dest.get() == d).to_string()
                            on:click=move |_| { dest.set(d); error.set(None); }
                        >{d.label()}</button>
                    }).collect_view()}
                </div>
            </div>
            <div class="p-4 space-y-3 outline-none" tabindex="-1" node_ref=form_ref>
                {move || match dest.get() {
                    Dest::Node => view! {
                        <label class="block text-xs text-stone-500">"Kind"
                            <select class=field aria-label="Kind"
                                on:change=move |ev| {
                                    let picked = event_target_value(&ev);
                                    if let Some(k) = KINDS.iter().find(|k| k.as_str() == picked) {
                                        kind.set(k.clone());
                                        // Each kind has its own homes (§3.3).
                                        home.set(None);
                                    }
                                }>
                                {KINDS.iter().map(|k| view! {
                                    <option value=k.as_str() selected=move || kind.get() == *k>
                                        {type_label(k.as_str())}
                                    </option>
                                }).collect_view()}
                            </select>
                        </label>
                        <div>
                            <input class=field aria-label="Title" node_ref=title_ref prop:value=move || title.get()
                                on:input=move |ev| title.set(event_target_value(&ev)) />
                            <div class="text-right">{move || counter(title.with(|t| t.trim().chars().count()))}</div>
                        </div>
                        <textarea class=format!("{field} min-h-[5rem]") aria-label="Body"
                            prop:value=move || body.get()
                            on:input=move |ev| body.set(event_target_value(&ev))></textarea>
                        {move || {
                            let kinds = allowed_homes(&kind.get());
                            if kinds.is_empty() {
                                view! {
                                    <p class="text-xs text-stone-500">
                                        "A Reference has no home: other nodes link to it."
                                    </p>
                                }.into_any()
                            } else {
                                view! {
                                    <NodePicker selected=home only_types=kinds on_commit=after_pick
                                        placeholder=crate::components::node_home::pick_placeholder(kinds) />
                                }.into_any()
                            }
                        }}
                    }.into_any(),
                    Dest::Note => view! {
                        <NodePicker selected=note_node on_commit=after_pick
                            placeholder="Note on a node\u{2026}" />
                        <textarea class=format!("{field} min-h-[6rem]") aria-label="Note"
                            prop:value=move || note_text.get()
                            on:input=move |ev| note_text.set(event_target_value(&ev))></textarea>
                    }.into_any(),
                    Dest::Task => view! {
                        <NodePicker selected=project only_type=NodeType::Project on_commit=after_pick
                            placeholder="Task in a Project\u{2026}" />
                        <div>
                            <input class=field aria-label="Task title" prop:value=move || task_title.get()
                                on:input=move |ev| task_title.set(event_target_value(&ev)) />
                            <div class="text-right">{move || counter(task_title.with(|t| t.trim().chars().count()))}</div>
                        </div>
                        <label class="block text-xs text-stone-500">"Due (optional)"
                            <input type="date" class=field aria-label="Due date"
                                prop:value=move || due.get()
                                on:input=move |ev| due.set(event_target_value(&ev)) />
                        </label>
                    }.into_any(),
                }}
                <Show when=move || error.get().is_some()>
                    <p class="text-sm text-red-600 dark:text-red-400" role="alert">
                        {move || error.get().unwrap_or_default()}
                    </p>
                </Show>
            </div>
            <div class="p-3 flex items-center justify-end gap-2 border-t border-stone-200 dark:border-stone-700">
                <button type="button"
                    class="px-3 py-1.5 text-sm text-stone-600 dark:text-stone-300 hover:text-stone-900"
                    on:click=move |_| show.set(false)>"Cancel"</button>
                <button type="button" node_ref=convert_ref
                    class="inline-flex items-center gap-1.5 px-4 py-1.5 rounded-lg bg-ember text-white text-sm \
                           hover:bg-ember-strong disabled:opacity-50"
                    prop:disabled=move || busy.get() || !ready.get()
                    on:click=move |_| submit_pending.set(true)>
                    <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"move_item"</span>
                    {move || if busy.get() { "Converting\u{2026}" } else { "Convert" }}
                </button>
            </div>
        </ModalShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_title_joins_the_capture_into_one_line_whole() {
        assert_eq!(one_line(" call\n the  bank \n"), "call the bank");
        let long = "word ".repeat(200);
        assert_eq!(one_line(&long).chars().count(), 999, "never cut");
    }
}
