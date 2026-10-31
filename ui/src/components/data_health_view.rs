//! `/data-health` (#208): the caller's data that doesn't fit the data model
//! yet (`docs/ONTOLOGY.md`), one card per rule that has items.
//!
//! Read-only: each item links to where it lives, and each rule says how to
//! fix it by hand until its own release adds a fix button. An admin also
//! sees each rule's count across every account (counts only, §14).

use common::data_health::{DataHealthReport, Ref, RefKind, RuleCount, RuleReport};
use leptos::prelude::*;

use crate::api;
use crate::auth::{AuthStatus, use_auth_state};
use crate::components::empty_state::EmptyState;
use crate::components::page_header::PageHeader;
use crate::components::task_common::task_href;

/// Where an item lives in the app. A done or cancelled task sits in its
/// list's collapsed "completed" section, where `?task=` can't land, so it
/// links to the list itself.
fn ref_href(r: &Ref) -> String {
    match r.kind {
        RefKind::Node => format!("/nodes/{}", r.id),
        RefKind::Task if r.closed => match r.node_id {
            Some(node) => format!("/nodes/{node}"),
            None => "/tasks/inbox".to_string(),
        },
        RefKind::Task => task_href(r.id, r.node_id),
        RefKind::Person => format!("/people/{}", r.id),
    }
}

/// "showing 200 of 341" when a rule has more items than it lists.
fn shown_of(report: &RuleReport) -> Option<String> {
    let shown = report.items.len() as u64;
    (report.count > shown).then(|| format!("Showing {shown} of {}", report.count))
}

#[component]
pub fn DataHealthView() -> impl IntoView {
    let report = LocalResource::new(api::fetch_data_health);
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
                        Ok(r) => report_view(r).into_any(),
                    })}
                </Transition>
                {move || is_admin.get().then(|| view! { <AdminCounts /> })}
            </div>
        </div>
    }
}

fn report_view(report: DataHealthReport) -> impl IntoView {
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
            {with_items.into_iter().map(rule_card).collect_view()}
        </div>
    }
    .into_any()
}

fn rule_card(report: RuleReport) -> impl IntoView {
    let rule = report.rule;
    let shown = shown_of(&report);
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

#[component]
fn RefLink(r: Ref) -> impl IntoView {
    let href = ref_href(&r);
    // A task's `archived` is its node's: the badge belongs on the node.
    let archived = r.archived && r.kind == RefKind::Node;
    let closed = r.closed;
    view! {
        <a href=href
           class="text-stone-800 dark:text-stone-200 hover:text-amber-700 dark:hover:text-amber-400 underline-offset-2 hover:underline">
            {r.label}
        </a>
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
        assert_eq!(ref_href(&Ref::node(id, "n", false)), format!("/nodes/{id}"));
        assert_eq!(
            ref_href(&Ref::task(id, "t", Some(node), false)),
            format!("/nodes/{node}?task={id}")
        );
        assert_eq!(
            ref_href(&Ref::task(id, "t", None, false)),
            format!("/tasks/inbox?task={id}"),
            "an orphan task opens in the Inbox"
        );
        assert_eq!(ref_href(&Ref::person(id, "p")), format!("/people/{id}"));
    }

    #[test]
    fn a_closed_task_links_to_its_list_not_into_the_collapsed_section() {
        let id = Uuid::from_u128(1);
        let node = Uuid::from_u128(2);
        let done = Ref::task(id, "t", Some(node), false).with_closed(true);
        assert_eq!(ref_href(&done), format!("/nodes/{node}"));
        let done_orphan = Ref::task(id, "t", None, false).with_closed(true);
        assert_eq!(ref_href(&done_orphan), "/tasks/inbox");
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
