//! Node version history panel — shows previous body snapshots and lets editors
//! restore to any of them.
use common::id::NodeId;
use leptos::prelude::*;

use crate::api;
use crate::components::format_helpers::format_timestamp;
use crate::components::toast::{ToastLevel, push_toast};

#[component]
pub fn VersionPanel(
    node_id: NodeId,
    /// Only editors/owners may restore; viewers see the list read-only.
    is_editor: bool,
    /// Called after a successful restore so the parent can refresh the node body.
    on_restore: Callback<()>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let restoring: RwSignal<Option<uuid::Uuid>> = RwSignal::new(None);
    let refresh = RwSignal::new(0u32);

    let versions = LocalResource::new(move || {
        let _ = refresh.get(); // re-fetch after restore
        let is_open = open.get();
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            api::fetch_versions(node_id, Some(20)).await
        }
    });

    let do_restore = move |version_id: uuid::Uuid| {
        restoring.set(Some(version_id));
        let on_restore = on_restore;
        leptos::task::spawn_local(async move {
            match api::restore_version(node_id, version_id).await {
                Ok(_) => {
                    push_toast(ToastLevel::Success, "Body restored to selected version.");
                    refresh.update(|n| *n += 1);
                    on_restore.run(());
                }
                Err(e) => {
                    push_toast(ToastLevel::Error, format!("Restore failed: {e}"));
                }
            }
            restoring.set(None);
        });
    };

    view! {
        <crate::components::node_section::SectionShell section=crate::components::node_section::NodeSection::Versions open=open>

                    <Suspense fallback=|| view! {
                        <p class="text-xs text-stone-400 dark:text-stone-500 animate-pulse py-2">"Loading versions…"</p>
                    }>
                        {move || versions.get().map(|result| match result {
                            Err(e) => view! {
                                <p class="text-xs text-red-500">{format!("Error: {e}")}</p>
                            }.into_any(),
                            Ok(list) if list.is_empty() => view! {
                                <p class="text-xs text-stone-400 dark:text-stone-500 py-2">
                                    "No saved versions yet. Versions are created automatically each time you save."
                                </p>
                            }.into_any(),
                            Ok(list) => view! {
                                <ol class="relative border-l border-stone-200 dark:border-stone-700 ml-2 space-y-4 py-1">
                                    {list.into_iter().enumerate().map(|(i, ver)| {
                                        let vid = ver.id.0;
                                        let ts = format_timestamp(&ver.created_at);
                                        let is_latest = i == 0;
                                        let busy = move || restoring.get() == Some(vid);
                                        view! {
                                            <li class="ml-4">
                                                // Timeline dot
                                                <span class=move || format!(
                                                    "absolute -left-2.5 h-5 w-5 rounded-full border-2 border-white dark:border-stone-900 {}",
                                                    if is_latest { "bg-amber-500" } else { "bg-stone-400 dark:bg-stone-600" }
                                                )></span>
                                                <div class="flex items-center justify-between gap-2">
                                                    <div class="text-xs leading-relaxed">
                                                        <span class=move || format!(
                                                            "font-medium {}",
                                                            if is_latest { "text-amber-600 dark:text-amber-400" } else { "text-stone-700 dark:text-stone-300" }
                                                        )>
                                                            {if is_latest { "Current version" } else { "Previous version" }}
                                                        </span>
                                                        <br />
                                                        <span class="text-stone-400 dark:text-stone-500 text-[11px]">{ts}</span>
                                                    </div>
                                                    {(!is_latest && is_editor).then(move || view! {
                                                        <button
                                                            class="inline-flex items-center justify-center gap-1.5 text-xs px-2 py-1 rounded border border-stone-300 dark:border-stone-600 text-stone-600 dark:text-stone-400 hover:border-amber-500 hover:text-amber-600 dark:hover:text-amber-400 transition-colors disabled:opacity-40"
                                                            disabled=busy
                                                            on:click=move |_| do_restore(vid)
                                                        >
                                                            <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"history"</span>
                                                            {move || if busy() { "Restoring…" } else { "Restore" }}
                                                        </button>
                                                    })}
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
