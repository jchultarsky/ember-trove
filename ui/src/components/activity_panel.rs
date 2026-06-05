//! Activity / audit-log panel — shows who did what on a node.
use common::activity::{ActivityAction, ActivityEntry};
use common::id::NodeId;
use leptos::prelude::*;

use crate::api;
use crate::components::format_helpers::format_timestamp;

/// Returns actor name from metadata, falling back to `subject_id`.
fn actor_display(entry: &ActivityEntry) -> String {
    entry
        .metadata
        .get("actor_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            entry
                .metadata
                .get("actor_email")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or(&entry.subject_id)
        .to_string()
}

/// Returns a short detail string for extra context (tag id, filename, role, etc.)
fn action_detail(entry: &ActivityEntry) -> Option<String> {
    match entry.action {
        ActivityAction::TagAdded | ActivityAction::TagRemoved => entry
            .metadata
            .get("tag_id")
            .and_then(|v| v.as_str())
            .map(|s| format!("tag {}", &s[..8.min(s.len())])),
        ActivityAction::AttachmentUploaded => entry
            .metadata
            .get("filename")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        ActivityAction::PermissionGranted => {
            let role = entry
                .metadata
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let who = entry
                .metadata
                .get("invited_email")
                .or_else(|| entry.metadata.get("subject_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            Some(format!("{who} as {role}"))
        }
        ActivityAction::Exported => entry
            .metadata
            .get("format")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => None,
    }
}

/// Tailwind colour class for the action icon bubble.
fn action_colour(action: &ActivityAction) -> &'static str {
    match action {
        ActivityAction::Created => {
            "bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300"
        }
        ActivityAction::Edited => {
            "bg-amber-100 text-amber-700 dark:bg-amber-900 dark:text-amber-300"
        }
        ActivityAction::Deleted => "bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300",
        ActivityAction::TagAdded | ActivityAction::TagRemoved => {
            "bg-violet-100 text-violet-700 dark:bg-violet-900 dark:text-violet-300"
        }
        ActivityAction::AttachmentUploaded => {
            "bg-sky-100 text-sky-700 dark:bg-sky-900 dark:text-sky-300"
        }
        ActivityAction::PermissionGranted | ActivityAction::PermissionRevoked => {
            "bg-orange-100 text-orange-700 dark:bg-orange-900 dark:text-orange-300"
        }
        ActivityAction::Shared => "bg-teal-100 text-teal-700 dark:bg-teal-900 dark:text-teal-300",
        ActivityAction::Exported => {
            "bg-stone-100 text-stone-700 dark:bg-stone-800 dark:text-stone-300"
        }
        ActivityAction::CreatedFromTemplate => {
            "bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300"
        }
    }
}

#[component]
pub fn ActivityPanel(node_id: NodeId) -> impl IntoView {
    let open = RwSignal::new(false);

    let entries = LocalResource::new(move || {
        let is_open = open.get();
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            api::fetch_activity(node_id, Some(50)).await
        }
    });

    view! {
        <div class="mt-4 border-t border-stone-200 dark:border-stone-700 pt-6">
        <div class="border border-stone-200 dark:border-stone-700 rounded-lg overflow-hidden">
            // ── Header / toggle ────────────────────────────────────────────
            <button
                class="w-full flex items-center justify-between px-4 py-3 bg-stone-50 dark:bg-stone-800 hover:bg-stone-100 dark:hover:bg-stone-750 text-sm font-medium text-stone-700 dark:text-stone-300 transition-colors"
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <span class="flex items-center gap-2">
                    <span class="material-symbols-outlined text-base">"history"</span>
                    "Activity"
                </span>
                <span class="material-symbols-outlined text-base">
                    {move || if open.get() { "expand_less" } else { "expand_more" }}
                </span>
            </button>

            // ── Body ───────────────────────────────────────────────────────
            {move || open.get().then(|| view! {
                <div class="px-4 py-3">
                    <Suspense fallback=|| view! {
                        <p class="text-xs text-stone-400 dark:text-stone-500 animate-pulse py-2">"Loading activity…"</p>
                    }>
                        {move || entries.get().map(|result| match result {
                            Err(e) => view! {
                                <p class="text-xs text-red-500">{format!("Error: {e}")}</p>
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
                </div>
            })}
        </div>
        </div>
    }
}
