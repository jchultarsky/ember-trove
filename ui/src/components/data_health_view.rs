//! `/data-health` (#208): the caller's data that doesn't fit the data model
//! yet (`docs/ONTOLOGY.md`), one card per rule that has items.
//!
//! Each item links to where it lives, and each rule says how to fix it.
//! Most rules also get one-click fixes (#216, #228), which call the same
//! endpoints as the rest of the app, so the same checks apply. An admin
//! also sees each rule's count across every account (counts only, §14).

use common::data_health::{
    DataHealthReport, LinkRef, Ref, RefKind, RuleCount, RuleId, RuleReport, Violation,
};
use common::edge::EdgeType;
use common::id::{MembershipId, NodeId, TaskId};
use common::node::{NodeType, UpdateNodeRequest};
use common::task::UpdateTaskRequest;
use leptos::prelude::*;

use crate::api;
use crate::app::TaskRefresh;
use crate::auth::{AuthStatus, use_auth_state};
use crate::components::empty_state::EmptyState;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::node_picker::NodePicker;
use crate::components::page_header::PageHeader;
use crate::components::task_actions::{delete_tasks_with_undo, paced};
use crate::components::task_common::task_href;
use crate::components::task_common::undo_restore_task;
use crate::components::toast::{ToastLevel, ToastState, push_toast, push_undo_toast};

/// Where an item lives in the app. A done or cancelled task sits in its
/// list's collapsed "completed" section, where `?task=` can't land, so it
/// links to the list itself. A task with no Project lives only here (S4c:
/// the Inbox no longer lists it), so it has no link.
fn ref_href(r: &Ref) -> Option<String> {
    match r.kind {
        RefKind::Node => Some(format!("/nodes/{}", r.id)),
        RefKind::Task => {
            let node = r.node_id?;
            Some(if r.closed {
                format!("/nodes/{node}")
            } else {
                task_href(r.id, Some(node))
            })
        }
        RefKind::Person => Some(format!("/people/{}", r.id)),
    }
}

/// A one-click fix for an item (#216).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fix {
    /// Put the task on a Project (a task on a non-Project, or with none).
    MoveToProject,
    /// Clear a project status off a non-Project.
    ClearStatus,
    /// Take the person off the non-Project.
    RemoveMembership,
    /// Delete the task: the other deliberate end for a task with no proper
    /// home (§9.3), most often a finished one.
    DeleteTask,
    /// Turn a contained Reference's home into a `references` link (#228).
    MakeReference,
    /// Turn a home that isn't allowed, or a dependency on an Area, into a
    /// `related_to` link (#228).
    LinkInstead,
    /// Remove the item's link (#228).
    RemoveLink,
}

/// The fixes an item offers, in button order. Removing a membership needs
/// the node's owner (the API's rule), so it's offered only to them.
fn fixes_for(rule: RuleId, v: &Violation) -> Vec<Fix> {
    match rule {
        RuleId::TaskOnNonProject | RuleId::OrphanTask if v.subject.kind == RefKind::Task => {
            vec![Fix::MoveToProject, Fix::DeleteTask]
        }
        RuleId::StatusOnNonProject => vec![Fix::ClearStatus],
        // Needs the membership, the reader's ownership of the node, and the
        // node itself (the call names it).
        RuleId::MembershipOnNonProject
            if v.membership.as_ref().is_some_and(|m| m.node_owned) && !v.related.is_empty() =>
        {
            vec![Fix::RemoveMembership]
        }
        // One bad link each (#228). Offered by the API's own rules
        // (`LinkRef`), judged by ownership.
        RuleId::ContainedReference | RuleId::ContainmentNotAllowed | RuleId::DependsOnArea => {
            let Some(link) = v.links.first() else {
                return Vec::new();
            };
            let (convert, to) = if rule == RuleId::ContainedReference {
                (Fix::MakeReference, EdgeType::References)
            } else {
                (Fix::LinkInstead, EdgeType::RelatedTo)
            };
            let mut fixes = Vec::new();
            if link.convertible_to(&to) {
                fixes.push(convert);
            }
            if link.removable() {
                fixes.push(Fix::RemoveLink);
            }
            fixes
        }
        _ => Vec::new(),
    }
}

/// What a fix's convert turns its link into.
fn convert_target(fix: Fix) -> Option<EdgeType> {
    match fix {
        Fix::MakeReference => Some(EdgeType::References),
        Fix::LinkInstead => Some(EdgeType::RelatedTo),
        _ => None,
    }
}

/// The links a contained-Reference card's "Convert all" turns into
/// `references` links: every listed one the reader may convert.
fn convertible_references(report: &RuleReport) -> Vec<uuid::Uuid> {
    if report.rule != RuleId::ContainedReference {
        return Vec::new();
    }
    report
        .items
        .iter()
        .filter_map(|v| v.links.first())
        .filter(|l| l.convertible_to(&EdgeType::References))
        .map(|l| l.id)
        .collect()
}

/// A loop's links as "A → B", named from the item's own nodes: the subject
/// and the related ones are exactly the loop's members.
fn hop_label(v: &Violation, link: &LinkRef) -> String {
    let name = |id: uuid::Uuid| {
        std::iter::once(&v.subject)
            .chain(&v.related)
            .find(|r| r.id == id)
            .map_or("\u{2026}", |r| r.label.as_str())
            .to_string()
    };
    format!("{} \u{2192} {}", name(link.source), name(link.target))
}

fn is_loop(rule: RuleId) -> bool {
    matches!(
        rule,
        RuleId::ContainsCycle | RuleId::DependsOnCycle | RuleId::DerivedFromCycle
    )
}

/// The finished tasks (done or cancelled) a task card lists: what its
/// "Delete finished" button deletes.
fn finished_tasks(report: &RuleReport) -> Vec<TaskId> {
    if !matches!(report.rule, RuleId::TaskOnNonProject | RuleId::OrphanTask) {
        return Vec::new();
    }
    report
        .items
        .iter()
        .filter(|v| v.subject.kind == RefKind::Task && v.subject.closed)
        .map(|v| TaskId(v.subject.id))
        .collect()
}

/// "showing 200 of 341" when a rule has more items than it lists.
fn shown_of(report: &RuleReport) -> Option<String> {
    let shown = report.items.len() as u64;
    (report.count > shown).then(|| format!("Showing {shown} of {}", report.count))
}

#[component]
pub fn DataHealthView() -> impl IntoView {
    // Reloads on the app-wide task refresh too, so a task delete (and its
    // Undo, which can outlive this page) shows here.
    let task_refresh = expect_context::<TaskRefresh>().0;
    let report = LocalResource::new(move || {
        task_refresh.track();
        api::fetch_data_health()
    });
    // After a fix, read the report again: the item leaves its card.
    let on_fixed = Callback::new(move |()| report.refetch());
    let auth = use_auth_state();
    // Memoized: auth refreshes don't remount (and refetch) the admin counts.
    let is_admin =
        Memo::new(move |_| matches!(auth.get(), AuthStatus::Authenticated(ref u) if u.is_admin()));

    view! {
        <div class="flex flex-col h-full">
            <PageHeader
                icon="checklist"
                title="Data health"
                subtitle="Data that doesn't fit the data model yet. Fix each item where it lives.".into_any()
            />
            <div class="flex-1 overflow-y-auto px-4 md:px-6 py-4 space-y-4">
                <Transition fallback=|| view! {
                    <p class="text-sm text-stone-400 animate-pulse">"Checking your data\u{2026}"</p>
                }>
                    {move || report.get().map(|result| match result {
                        Err(e) => view! {
                            <p class="text-sm text-red-600 dark:text-red-400">{format!("Couldn't load the report: {e}")}</p>
                        }.into_any(),
                        Ok(r) => report_view(r, on_fixed).into_any(),
                    })}
                </Transition>
                {move || is_admin.get().then(|| view! { <AdminCounts /> })}
            </div>
        </div>
    }
}

fn report_view(report: DataHealthReport, on_fixed: Callback<()>) -> impl IntoView {
    let with_items: Vec<RuleReport> = report.rules.into_iter().filter(|r| r.count > 0).collect();
    if with_items.is_empty() {
        return view! {
            <EmptyState
                icon="task_alt"
                message="Everything fits the data model."
                hint="Nothing to clean up."
            />
        }
        .into_any();
    }
    view! {
        <div class="space-y-4">
            {with_items.into_iter().map(|r| rule_card(r, on_fixed)).collect_view()}
        </div>
    }
    .into_any()
}

fn rule_card(report: RuleReport, on_fixed: Callback<()>) -> impl IntoView {
    let rule = report.rule;
    let shown = shown_of(&report);
    let finished = finished_tasks(&report);
    let convertible = convertible_references(&report);
    // Only what this card lists: past the cap, the label says so.
    let capped = report.count > report.items.len() as u64;
    // One in-flight guard for the whole card: a row's Move and Delete, and
    // the card's bulk delete, never run over each other.
    let busy = RwSignal::new(false);
    view! {
        <section
            class="rounded-lg border border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-900"
            data-testid=format!("data-health-{}", rule.as_str())
            aria-labelledby=format!("dh-{}", rule.as_str())
        >
            <header class="px-4 py-3 border-b border-stone-100 dark:border-stone-800">
                <div class="flex items-baseline justify-between gap-3">
                    <h2 id=format!("dh-{}", rule.as_str())
                        class="text-sm font-semibold text-stone-800 dark:text-stone-200">
                        {rule.title()}
                    </h2>
                    <span class="text-xs font-medium tabular-nums text-stone-500 dark:text-stone-400">
                        {report.count}
                    </span>
                </div>
                {(!finished.is_empty()).then(|| view! {
                    <DeleteFinished tasks=finished capped=capped busy=busy />
                })}
                {(convertible.len() > 1).then(|| view! {
                    <ConvertAll links=convertible capped=capped busy=busy on_fixed=on_fixed />
                })}
                <p class="mt-1 text-xs text-stone-600 dark:text-stone-400">
                    {rule.explanation()}
                    <span class="text-stone-400 dark:text-stone-500">{format!(" ({})", rule.section())}</span>
                </p>
                <p class="mt-1 text-xs text-stone-500 dark:text-stone-400">
                    <span class="font-medium">"To fix: "</span>{rule.fix_hint()}
                </p>
            </header>
            <ul class="divide-y divide-stone-100 dark:divide-stone-800">
                {report.items.into_iter().map(|v| {
                    let more = v.more;
                    let fixes = fixes_for(rule, &v);
                    let hops = is_loop(rule).then(|| {
                        let item = v.clone();
                        view! { <LoopLinks item=item busy=busy on_fixed=on_fixed /> }
                    });
                    let fix = (!fixes.is_empty()).then(|| {
                        let item = v.clone();
                        view! {
                            <span class="ml-auto inline-flex flex-wrap items-center justify-end gap-1">
                                {fixes.into_iter().map(|fix| view! {
                                    <FixControl fix=fix item=item.clone() busy=busy on_fixed=on_fixed />
                                }).collect_view()}
                            </span>
                        }
                    });
                    view! {
                        <li class="px-4 py-2 text-sm flex flex-wrap items-center gap-x-2 gap-y-1"
                            data-testid="data-health-item">
                            <RefLink r=v.subject />
                            {(!v.related.is_empty()).then(|| view! {
                                <span class="text-stone-400 dark:text-stone-500" aria-hidden="true">"\u{2192}"</span>
                            })}
                            {v.related.into_iter().map(|r| view! { <RefLink r=r /> }).collect_view()}
                            {(more > 0).then(|| view! {
                                <span class="text-xs text-stone-400">{format!("+{more} more")}</span>
                            })}
                            {fix}
                            {hops}
                        </li>
                    }
                }).collect_view()}
            </ul>
            {shown.map(|s| view! {
                <p class="px-4 py-2 text-xs text-stone-400 dark:text-stone-500">{s}</p>
            })}
        </section>
    }
}

const FIX_BUTTON: &str = "inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-xs \
    text-stone-600 dark:text-stone-300 border border-stone-200 dark:border-stone-700 \
    hover:bg-stone-100 dark:hover:bg-stone-800 disabled:opacity-40";

/// An inline "are you sure?" for a fix: the question, the commit button,
/// and Cancel. Worded for what the action does, not a generic delete.
#[component]
fn InlineConfirm(
    question: String,
    commit: &'static str,
    on_confirm: Callback<()>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    view! {
        <span class="inline-flex flex-wrap items-center gap-2 text-xs" role="group"
            aria-label=format!("Confirm {}", commit.to_lowercase())>
            <span class="text-stone-600 dark:text-stone-300">{question}</span>
            <button type="button" class=FIX_BUTTON on:click=move |_| on_confirm.run(())>{commit}</button>
            <button type="button" class="text-stone-500 hover:text-stone-700 dark:hover:text-stone-300"
                on:click=move |_| on_cancel.run(())>
                "Cancel"
            </button>
        </span>
    }
}

/// An item's one-click fix. Each calls an existing endpoint (task PATCH or
/// DELETE, node PUT, membership DELETE), so the API's own checks apply.
///
/// `busy` is the card's: on success it stays set, because the report then
/// reloads and the item leaves its card, and until then a second action
/// would only repeat or contradict the first. Everything after an `.await`
/// uses `try_` access: the page may be gone by then (`.claude/rules/leptos.md`).
#[component]
fn FixControl(
    fix: Fix,
    item: Violation,
    busy: RwSignal<bool>,
    on_fixed: Callback<()>,
) -> impl IntoView {
    let finish = move |result: Result<String, String>| match result {
        Ok(done) => {
            push_toast(ToastLevel::Success, done);
            on_fixed.try_run(());
        }
        Err(e) => {
            busy.try_set(false);
            push_toast(ToastLevel::Error, e);
        }
    };
    match fix {
        Fix::MoveToProject => {
            let picking = RwSignal::new(false);
            let target = RwSignal::new(None::<(NodeId, String)>);
            let task = TaskId(item.subject.id);
            let commit = move || {
                if busy.get_untracked() {
                    return;
                }
                let Some((dest, title)) = target.get_untracked() else {
                    return;
                };
                busy.set(true);
                wasm_bindgen_futures::spawn_local(async move {
                    let req = UpdateTaskRequest {
                        node_id: Some(Some(dest)),
                        ..Default::default()
                    };
                    finish(
                        api::update_task(task, &req)
                            .await
                            .map(|_| format!("Moved to {title}"))
                            .map_err(|e| format!("Couldn't move: {e}")),
                    );
                });
            };
            view! {
                {move || if picking.get() {
                    view! {
                        <div class="w-full sm:w-80 flex items-start gap-1">
                            <div class="flex-1 min-w-0">
                                <NodePicker
                                    selected=target
                                    placeholder="Move to a Project\u{2026}"
                                    autofocus=true
                                    only_type=NodeType::Project
                                    on_commit=Callback::new(move |()| commit())
                                />
                            </div>
                            <IconButton
                                icon="close"
                                label="Cancel"
                                disabled=Signal::derive(move || busy.get())
                                on_click=Callback::new(move |()| {
                                    picking.set(false);
                                    target.set(None);
                                })
                            />
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <button type="button" class=FIX_BUTTON disabled=move || busy.get()
                            on:click=move |_| picking.set(true)>
                            <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"arrow_forward"</span>
                            "Move to a Project\u{2026}"
                        </button>
                    }.into_any()
                }}
            }
            .into_any()
        }
        Fix::ClearStatus => {
            let node = NodeId(item.subject.id);
            let clear = move |_| {
                busy.set(true);
                wasm_bindgen_futures::spawn_local(async move {
                    let req = UpdateNodeRequest {
                        title: None,
                        body: None,
                        metadata: None,
                        status: None,
                        project_status: Some(None),
                        expected_updated_at: None,
                    };
                    finish(
                        api::update_node(node, &req)
                            .await
                            .map(|_| "Status cleared".to_string())
                            .map_err(|e| format!("Couldn't clear: {e}")),
                    );
                });
            };
            view! {
                <button type="button" class=FIX_BUTTON disabled=move || busy.get() on:click=clear>
                    <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"close"</span>
                    "Clear status"
                </button>
            }
            .into_any()
        }
        Fix::DeleteTask => {
            let task_refresh = expect_context::<TaskRefresh>().0;
            let toast_state = use_context::<ToastState>();
            let confirming = RwSignal::new(false);
            let task = TaskId(item.subject.id);
            let question = format!("Delete \u{201c}{}\u{201d}?", item.subject.label);
            let delete = Callback::new(move |()| {
                confirming.set(false);
                busy.set(true);
                wasm_bindgen_futures::spawn_local(async move {
                    match api::delete_task(task).await {
                        Ok(()) => {
                            // Soft-deleted. The Undo bumps the app-wide task
                            // refresh, so it works even after leaving the page.
                            push_undo_toast(
                                "Task deleted",
                                undo_restore_task(task, task_refresh, toast_state),
                            );
                            task_refresh.try_update(|n| *n += 1);
                        }
                        Err(e) => {
                            busy.try_set(false);
                            push_toast(ToastLevel::Error, format!("Couldn't delete: {e}"));
                        }
                    }
                });
            });
            view! {
                {move || if confirming.get() {
                    view! {
                        <InlineConfirm question=question.clone() commit="Delete"
                            on_confirm=delete
                            on_cancel=Callback::new(move |()| confirming.set(false)) />
                    }.into_any()
                } else {
                    // A row action: icon only (`.claude/rules/leptos.md`).
                    view! {
                        <IconButton icon="delete" label="Delete task"
                            variant=IconButtonVariant::Danger
                            disabled=Signal::derive(move || busy.get())
                            on_click=Callback::new(move |()| confirming.set(true)) />
                    }.into_any()
                }}
            }
            .into_any()
        }
        Fix::MakeReference | Fix::LinkInstead => {
            let (Some(link), Some(to)) = (item.links.first().cloned(), convert_target(fix)) else {
                return ().into_any();
            };
            let (icon, label, done) = if fix == Fix::MakeReference {
                (
                    "menu_book",
                    "Make it a reference link",
                    "Now a reference link",
                )
            } else {
                ("link", "Link instead", "Now a related link")
            };
            let convert = move |_| {
                busy.set(true);
                let to = to.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    finish(
                        api::convert_edge(link.id, to)
                            .await
                            .map(|()| done.to_string())
                            .map_err(|e| format!("Couldn't change the link: {e}")),
                    );
                });
            };
            view! {
                <button type="button" class=FIX_BUTTON disabled=move || busy.get() on:click=convert>
                    <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">{icon}</span>
                    {label}
                </button>
            }
            .into_any()
        }
        Fix::RemoveLink => {
            let (Some(link), Some(other)) =
                (item.links.first().cloned(), item.related.first().cloned())
            else {
                return ().into_any();
            };
            let confirming = RwSignal::new(false);
            // Worded for the link: a home is left, a dependency dropped.
            let question = if link.edge_type == EdgeType::Contains {
                format!(
                    "Take \u{201c}{}\u{201d} out of \u{201c}{}\u{201d}?",
                    item.subject.label, other.label
                )
            } else {
                format!("Drop the dependency on \u{201c}{}\u{201d}?", other.label)
            };
            let remove = Callback::new(move |()| {
                confirming.set(false);
                busy.set(true);
                wasm_bindgen_futures::spawn_local(async move {
                    finish(
                        api::delete_edge(common::id::EdgeId(link.id))
                            .await
                            .map(|()| "Link removed".to_string())
                            .map_err(|e| format!("Couldn't remove: {e}")),
                    );
                });
            });
            view! {
                {move || if confirming.get() {
                    view! {
                        <InlineConfirm question=question.clone() commit="Remove"
                            on_confirm=remove
                            on_cancel=Callback::new(move |()| confirming.set(false)) />
                    }.into_any()
                } else {
                    view! {
                        <IconButton icon="link_off" label="Remove the link"
                            variant=IconButtonVariant::Danger
                            disabled=Signal::derive(move || busy.get())
                            on_click=Callback::new(move |()| confirming.set(true)) />
                    }.into_any()
                }}
            }
            .into_any()
        }
        Fix::RemoveMembership => {
            // `fixes_for` offers this only with a membership and a node.
            let (Some(membership), Some(node)) =
                (item.membership.clone(), item.related.first().cloned())
            else {
                return ().into_any();
            };
            let confirming = RwSignal::new(false);
            let person = item.subject.label.clone();
            // Worded for what it does: the person stays in the registry;
            // only their place on this node goes.
            let question = format!("Remove {person} from {}?", node.label);
            let remove = Callback::new(move |()| {
                confirming.set(false);
                busy.set(true);
                let person = person.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    finish(
                        api::remove_node_person(NodeId(node.id), MembershipId(membership.id))
                            .await
                            .map(|()| format!("Removed {person}"))
                            .map_err(|e| format!("Couldn't remove: {e}")),
                    );
                });
            });
            view! {
                {move || if confirming.get() {
                    view! {
                        <InlineConfirm question=question.clone() commit="Remove"
                            on_confirm=remove
                            on_cancel=Callback::new(move |()| confirming.set(false)) />
                    }.into_any()
                } else {
                    view! {
                        <button type="button" class=FIX_BUTTON disabled=move || busy.get()
                            on:click=move |_| confirming.set(true)>
                            <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"person_remove"</span>
                            "Remove\u{2026}"
                        </button>
                    }.into_any()
                }}
            }
            .into_any()
        }
    }
}

/// "Delete finished" on a task card (#220): its listed done or cancelled
/// tasks, after one confirmation, with one Undo for all of them. Shares
/// the bulk delete with the Tasks panel (`task_actions::delete_tasks_with_undo`).
#[component]
fn DeleteFinished(tasks: Vec<TaskId>, capped: bool, busy: RwSignal<bool>) -> impl IntoView {
    let task_refresh = expect_context::<TaskRefresh>().0;
    let toast_state = use_context::<ToastState>();
    let n = tasks.len();
    let confirming = RwSignal::new(false);
    let shown = if capped { " shown" } else { "" };
    let label = format!("Delete the {n} finished tasks{shown} (done or cancelled)");
    let question = format!(
        "Delete {n} finished task{} (done or cancelled){shown}? You can undo it.",
        if n == 1 { "" } else { "s" }
    );
    let tasks = StoredValue::new(tasks);
    let delete_all = Callback::new(move |()| {
        confirming.set(false);
        busy.set(true);
        delete_tasks_with_undo(tasks.get_value(), "finished", task_refresh, toast_state);
    });
    view! {
        <div class="mt-2 flex flex-wrap items-center gap-2 text-xs" data-testid="delete-finished">
            {move || if confirming.get() {
                view! {
                    <InlineConfirm question=question.clone() commit="Delete"
                        on_confirm=delete_all
                        on_cancel=Callback::new(move |()| confirming.set(false)) />
                }.into_any()
            } else {
                // A card action: icon only, like the Tasks panel's bulk delete.
                view! {
                    <IconButton icon="delete_sweep" label=label.clone()
                        variant=IconButtonVariant::Danger
                        disabled=Signal::derive(move || busy.get())
                        on_click=Callback::new(move |()| confirming.set(true)) />
                    <span class="text-stone-500 dark:text-stone-400">{format!("{n} finished")}</span>
                }.into_any()
            }}
        </div>
    }
}

/// A loop's links (#228): each hop "A → B", with a remove button on the
/// ones the reader may remove. Taking out any one breaks the loop.
#[component]
fn LoopLinks(item: Violation, busy: RwSignal<bool>, on_fixed: Callback<()>) -> impl IntoView {
    let hops: Vec<(LinkRef, String)> = item
        .links
        .iter()
        .filter(|l| l.removable())
        .map(|l| (l.clone(), hop_label(&item, l)))
        .collect();
    if hops.is_empty() {
        return ().into_any();
    }
    let confirming = RwSignal::new(None::<uuid::Uuid>);
    view! {
        <div class="w-full flex flex-wrap items-center gap-1 text-xs" data-testid="loop-links">
            <span class="text-stone-500 dark:text-stone-400">"Break the loop:"</span>
            {hops.into_iter().map(|(link, label)| {
                let id = link.id;
                let question = format!("Remove the link {label}?");
                let remove = Callback::new(move |()| {
                    confirming.set(None);
                    busy.set(true);
                    wasm_bindgen_futures::spawn_local(async move {
                        match api::delete_edge(common::id::EdgeId(id)).await {
                            Ok(()) => {
                                push_toast(ToastLevel::Success, "Link removed".to_string());
                                on_fixed.try_run(());
                            }
                            Err(e) => {
                                busy.try_set(false);
                                push_toast(ToastLevel::Error, format!("Couldn't remove: {e}"));
                            }
                        }
                    });
                });
                view! {
                    <span class="inline-flex items-center gap-1 rounded-md border border-stone-200 dark:border-stone-700 px-1.5 py-0.5">
                        {move || if confirming.get() == Some(id) {
                            view! {
                                <InlineConfirm question=question.clone() commit="Remove"
                                    on_confirm=remove
                                    on_cancel=Callback::new(move |()| confirming.set(None)) />
                            }.into_any()
                        } else {
                            let label = label.clone();
                            view! {
                                <span class="text-stone-700 dark:text-stone-300">{label.clone()}</span>
                                <IconButton icon="link_off" label=format!("Remove the link {label}")
                                    variant=IconButtonVariant::Danger
                                    disabled=Signal::derive(move || busy.get())
                                    on_click=Callback::new(move |()| confirming.set(Some(id))) />
                            }.into_any()
                        }}
                    </span>
                }
            }).collect_view()}
        </div>
    }
    .into_any()
}

/// "Convert all" on the contained-Reference card (#228): every listed link
/// the reader may convert becomes a `references` link, after one
/// confirmation. The report's other fixes stay per item.
#[component]
fn ConvertAll(
    links: Vec<uuid::Uuid>,
    capped: bool,
    busy: RwSignal<bool>,
    on_fixed: Callback<()>,
) -> impl IntoView {
    let n = links.len();
    let confirming = RwSignal::new(false);
    let shown = if capped { " shown" } else { "" };
    let question = format!("Turn the {n}{shown} contains links into references links?");
    let links = StoredValue::new(links);
    let convert_all = Callback::new(move |()| {
        confirming.set(false);
        busy.set(true);
        let links = links.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let total = links.len();
            // Paced like the bulk task delete, so a long card can't trip
            // the write limiter halfway (429s).
            let (done, failed) =
                paced(links, |id| api::convert_edge(id, EdgeType::References)).await;
            if failed == 0 {
                push_toast(
                    ToastLevel::Success,
                    format!("Converted {} links", done.len()),
                );
            } else {
                push_toast(
                    ToastLevel::Error,
                    format!("Converted {} of {total}; {failed} failed", done.len()),
                );
            }
            // Reload either way: what did convert leaves the card.
            on_fixed.try_run(());
        });
    });
    view! {
        <div class="mt-2 flex flex-wrap items-center gap-2 text-xs" data-testid="convert-all">
            {move || if confirming.get() {
                view! {
                    <InlineConfirm question=question.clone() commit="Convert"
                        on_confirm=convert_all
                        on_cancel=Callback::new(move |()| confirming.set(false)) />
                }.into_any()
            } else {
                view! {
                    <button type="button" class=FIX_BUTTON disabled=move || busy.get()
                        on:click=move |_| confirming.set(true)>
                        <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"link"</span>
                        {format!("Convert all {n}")}
                    </button>
                }.into_any()
            }}
        </div>
    }
}

#[component]
fn RefLink(r: Ref) -> impl IntoView {
    let href = ref_href(&r);
    // A task's `archived` is its node's: the badge belongs on the node.
    let archived = r.archived && r.kind == RefKind::Node;
    let closed = r.closed;
    view! {
        {match href {
            Some(href) => view! {
                <a href=href
                   class="text-stone-800 dark:text-stone-200 hover:text-amber-700 dark:hover:text-amber-400 underline-offset-2 hover:underline">
                    {r.label}
                </a>
            }.into_any(),
            None => view! {
                <span class="text-stone-800 dark:text-stone-200">{r.label}</span>
            }.into_any(),
        }}
        {archived.then(|| view! { <Badge text="archived" /> })}
        {closed.then(|| view! { <Badge text="completed" /> })}
    }
}

#[component]
fn Badge(text: &'static str) -> impl IntoView {
    view! {
        <span class="text-[10px] uppercase tracking-wide rounded px-1 bg-stone-100 dark:bg-stone-800 text-stone-500">
            {text}
        </span>
    }
}

/// Admin only: how much cleanup is left across every account. Counts only.
#[component]
fn AdminCounts() -> impl IntoView {
    let counts = LocalResource::new(api::fetch_admin_data_health);
    view! {
        <section class="rounded-lg border border-stone-200 dark:border-stone-700 px-4 py-3"
                 aria-labelledby="dh-admin">
            <h2 id="dh-admin" class="text-sm font-semibold text-stone-800 dark:text-stone-200">
                "Across all accounts"
            </h2>
            <p class="mt-1 text-xs text-stone-500 dark:text-stone-400">
                "Admin view: counts only. A rule's database check is added once its count reaches zero."
            </p>
            <Suspense fallback=|| ()>
                {move || counts.get().map(|result| match result {
                    Err(e) => view! {
                        <p class="mt-2 text-xs text-red-600 dark:text-red-400">{format!("Couldn't load counts: {e}")}</p>
                    }.into_any(),
                    Ok(list) => counts_table(list).into_any(),
                })}
            </Suspense>
        </section>
    }
}

fn counts_table(list: Vec<RuleCount>) -> impl IntoView {
    view! {
        <table class="mt-2 w-full text-xs">
            <tbody class="divide-y divide-stone-100 dark:divide-stone-800">
                {list.into_iter().map(|c| view! {
                    <tr>
                        <td class="py-1 text-stone-600 dark:text-stone-300">{c.rule.title()}</td>
                        <td class="py-1 text-right tabular-nums text-stone-700 dark:text-stone-200">{c.count}</td>
                    </tr>
                }).collect_view()}
            </tbody>
        </table>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::data_health::{RuleId, Violation};
    use uuid::Uuid;

    #[test]
    fn each_kind_links_to_where_it_lives() {
        let id = Uuid::from_u128(1);
        let node = Uuid::from_u128(2);
        assert_eq!(
            ref_href(&Ref::node(id, "n", false)),
            Some(format!("/nodes/{id}"))
        );
        assert_eq!(
            ref_href(&Ref::task(id, "t", Some(node), false)),
            Some(format!("/nodes/{node}?task={id}"))
        );
        assert_eq!(
            ref_href(&Ref::task(id, "t", None, false)),
            None,
            "a task with no Project lives only on this page"
        );
        assert_eq!(
            ref_href(&Ref::person(id, "p")),
            Some(format!("/people/{id}"))
        );
    }

    #[test]
    fn a_closed_task_links_to_its_list_not_into_the_collapsed_section() {
        let id = Uuid::from_u128(1);
        let node = Uuid::from_u128(2);
        let done = Ref::task(id, "t", Some(node), false).with_closed(true);
        assert_eq!(ref_href(&done), Some(format!("/nodes/{node}")));
        let done_orphan = Ref::task(id, "t", None, false).with_closed(true);
        assert_eq!(ref_href(&done_orphan), None);
    }

    #[test]
    fn each_s2_rule_offers_its_fixes_and_the_rest_offer_none() {
        let id = Uuid::from_u128(1);
        let task = Violation::new(Ref::task(id, "t", None, false), vec![]);
        for rule in [RuleId::TaskOnNonProject, RuleId::OrphanTask] {
            assert_eq!(
                fixes_for(rule, &task),
                vec![Fix::MoveToProject, Fix::DeleteTask],
                "a task can be homed or deliberately deleted"
            );
        }
        let node = Violation::new(Ref::node(id, "n", false), vec![]);
        assert_eq!(
            fixes_for(RuleId::StatusOnNonProject, &node),
            vec![Fix::ClearStatus]
        );
        let person = || Violation::new(Ref::person(id, "p"), vec![Ref::node(id, "n", false)]);
        assert_eq!(
            fixes_for(
                RuleId::MembershipOnNonProject,
                &person().with_membership(id, true)
            ),
            vec![Fix::RemoveMembership]
        );
        assert!(
            fixes_for(
                RuleId::MembershipOnNonProject,
                &person().with_membership(id, false)
            )
            .is_empty(),
            "only the node's owner may remove it"
        );
        assert!(fixes_for(RuleId::MembershipOnNonProject, &person()).is_empty());
        let nodeless = Violation::new(Ref::person(id, "p"), vec![]).with_membership(id, true);
        assert!(
            fixes_for(RuleId::MembershipOnNonProject, &nodeless).is_empty(),
            "no node to name in the call: no button"
        );
        // Loops are broken hop by hop (`LoopLinks`), not by item fixes.
        for rule in [RuleId::ContainsCycle, RuleId::DependsOnCycle] {
            assert!(
                fixes_for(rule, &node).is_empty(),
                "{} has no item fix",
                rule.as_str()
            );
        }
    }

    fn link(edge_type: EdgeType, source_owned: bool, target_owned: bool) -> LinkRef {
        LinkRef {
            id: Uuid::from_u128(9),
            edge_type,
            source: Uuid::from_u128(1),
            target: Uuid::from_u128(2),
            source_owned,
            target_owned,
        }
    }

    fn edge_item(l: LinkRef) -> Violation {
        Violation::new(
            Ref::node(l.target, "child", false),
            vec![Ref::node(l.source, "parent", false)],
        )
        .with_links(vec![l])
    }

    #[test]
    fn an_edge_item_offers_what_the_reader_may_do_to_its_link() {
        let parents = edge_item(link(EdgeType::Contains, true, false));
        assert_eq!(
            fixes_for(RuleId::ContainedReference, &parents),
            vec![Fix::MakeReference, Fix::RemoveLink]
        );
        // The Reference's own owner may free it, but not speak for the parent.
        let childs = edge_item(link(EdgeType::Contains, false, true));
        assert_eq!(
            fixes_for(RuleId::ContainedReference, &childs),
            vec![Fix::RemoveLink]
        );
        assert_eq!(
            fixes_for(RuleId::ContainmentNotAllowed, &childs),
            vec![Fix::LinkInstead, Fix::RemoveLink]
        );
        // A dependency is its source's.
        let dep = edge_item(link(EdgeType::DependsOn, false, true));
        assert!(fixes_for(RuleId::DependsOnArea, &dep).is_empty());
        let no_link = Violation::new(Ref::node(Uuid::nil(), "n", false), vec![]);
        assert!(fixes_for(RuleId::ContainedReference, &no_link).is_empty());
    }

    #[test]
    fn convert_all_takes_only_the_links_the_reader_may_convert() {
        let mut theirs = link(EdgeType::Contains, false, true);
        theirs.id = Uuid::from_u128(8);
        let card = RuleReport::new(
            RuleId::ContainedReference,
            vec![
                edge_item(link(EdgeType::Contains, true, false)),
                edge_item(theirs),
            ],
        );
        assert_eq!(convertible_references(&card), vec![Uuid::from_u128(9)]);
        let other = RuleReport::new(
            RuleId::ContainmentNotAllowed,
            vec![edge_item(link(EdgeType::Contains, true, false))],
        );
        assert!(convertible_references(&other).is_empty(), "only that card");
    }

    #[test]
    fn a_loop_hop_is_named_from_the_items_own_nodes() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let v = Violation::new(
            Ref::node(a, "alpha", false),
            vec![Ref::node(b, "beta", false)],
        );
        let mut l = link(EdgeType::DependsOn, true, false);
        (l.source, l.target) = (b, a);
        assert_eq!(hop_label(&v, &l), "beta \u{2192} alpha");
    }

    #[test]
    fn delete_finished_takes_only_the_finished_tasks_of_a_task_card() {
        let task = |n: u128, closed: bool| {
            Violation::new(
                Ref::task(Uuid::from_u128(n), "t", None, false).with_closed(closed),
                vec![],
            )
        };
        let card = RuleReport::new(
            RuleId::OrphanTask,
            vec![task(1, true), task(2, false), task(3, true)],
        );
        assert_eq!(
            finished_tasks(&card),
            vec![TaskId(Uuid::from_u128(1)), TaskId(Uuid::from_u128(3))]
        );
        let other = RuleReport::new(RuleId::StatusOnNonProject, vec![task(4, true)]);
        assert!(finished_tasks(&other).is_empty(), "only task cards");
    }

    #[test]
    fn a_capped_rule_says_how_many_it_shows() {
        let item = Violation::new(Ref::node(Uuid::nil(), "n", false), vec![]);
        let capped = RuleReport::with_count(RuleId::OrphanTask, 341, vec![item.clone()]);
        assert_eq!(shown_of(&capped).as_deref(), Some("Showing 1 of 341"));
        let whole = RuleReport::new(RuleId::OrphanTask, vec![item]);
        assert_eq!(shown_of(&whole), None);
    }
}
