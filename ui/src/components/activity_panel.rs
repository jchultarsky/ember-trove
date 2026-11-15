//! The Activity section of a node page: its audit trail, who created,
//! edited, tagged, shared or exported it. What *occurred* about the node
//! (a status or state change, a cascade, a replaced Brief, a publish) is
//! its History, shown in the History section (S6d; owner decision,
//! 2026-11-14: nothing is listed in both).
use common::activity::ActivityScope;
use common::id::NodeId;
use leptos::prelude::*;

use crate::api;
use crate::components::activity_lines::{action_colour, action_detail, actor_display};
use crate::components::format_helpers::format_timestamp;

#[component]
pub fn ActivityPanel(node_id: NodeId) -> impl IntoView {
    let open = RwSignal::new(false);
    let retry = RwSignal::new(0u32);

    let entries = LocalResource::new(move || {
        let is_open = open.get();
        retry.track();
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            api::fetch_activity(node_id, Some(50), Some(ActivityScope::Audit))
                .await
                // The API filters; this holds the line if it ever doesn't.
                .map(|list| {
                    list.into_iter()
                        .filter(|entry| !entry.action.is_history())
                        .collect::<Vec<_>>()
                })
        }
    });

    view! {
        <crate::components::node_section::SectionShell section=crate::components::node_section::NodeSection::Activity open=open>

                    <Suspense fallback=|| view! {
                        <p class="text-xs text-stone-400 dark:text-stone-500 animate-pulse py-2">"Loading activity…"</p>
                    }>
                        {move || entries.get().map(|result| match result {
                            // A failed load is not "nothing recorded" (#281, 1.7).
                            Err(_) => view! {
                                <crate::components::load_error::LoadError
                                    what="this node\u{2019}s activity"
                                    on_retry=Callback::new(move |()| retry.update(|n| *n += 1))
                                />
                            }.into_any(),
                            Ok(list) if list.is_empty() => view! {
                                <p class="text-xs text-stone-400 dark:text-stone-500 py-2">"No activity recorded yet."</p>
                            }.into_any(),
                            Ok(list) => view! {
                                <ol class="relative border-l border-stone-200 dark:border-stone-700 ml-2 space-y-4 py-1">
                                    {list.into_iter().map(|entry| {
                                        let icon = entry.action.icon();
                                        let label = entry.action.label();
                                        let colour = action_colour(&entry.action);
                                        let actor = actor_display(&entry);
                                        let detail = action_detail(&entry);
                                        let ts = format_timestamp(&entry.created_at);
                                        view! {
                                            <li class="ml-4">
                                                // Icon bubble on the timeline line
                                                <span class=format!(
                                                    "absolute -left-3 flex h-6 w-6 items-center justify-center rounded-full text-xs {colour}"
                                                )>
                                                    <span class="material-symbols-outlined" style="font-size:14px">{icon}</span>
                                                </span>
                                                <div class="text-xs leading-relaxed">
                                                    <span class="font-medium text-stone-800 dark:text-stone-200">{actor}</span>
                                                    <span class="text-stone-500 dark:text-stone-400">{format!(" {label}")}</span>
                                                    {detail.map(|d| view! {
                                                        <span class="text-stone-400 dark:text-stone-500">{format!(" · {d}")}</span>
                                                    })}
                                                    <br />
                                                    <span class="text-stone-400 dark:text-stone-500 text-[11px]">{ts}</span>
                                                </div>
                                            </li>
                                        }
                                    }).collect::<Vec<_>>()}
                                </ol>
                            }.into_any(),
                        })}
                    </Suspense>
        </crate::components::node_section::SectionShell>
    }
}
