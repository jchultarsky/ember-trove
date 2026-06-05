//! Read-only public node view rendered at `/share/<token>`.
//! No authentication required — the token is the credential.
use leptos::prelude::*;
use uuid::Uuid;

use crate::api;
use crate::markdown::render_markdown_plain;

#[component]
pub fn PublicShareView(token: Uuid) -> impl IntoView {
    let node = LocalResource::new(move || async move { api::fetch_shared_node(token).await });

    view! {
        <div class="min-h-screen bg-stone-50 dark:bg-stone-950">
            // Simple header — no sidebar or auth controls.
            <header class="border-b border-stone-200 dark:border-stone-800 bg-white dark:bg-stone-900 px-6 py-3 flex items-center gap-3">
                <span class="material-symbols-outlined text-amber-500" style="font-size: 22px;">
                    "local_fire_department"
                </span>
                <span class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                    "Ember Trove"
                </span>
                <span class="ml-auto text-xs text-stone-400 dark:text-stone-500 flex items-center gap-1">
                    <span class="material-symbols-outlined" style="font-size: 14px;">"lock"</span>
                    "Read-only shared view"
                </span>
            </header>

            <main class="max-w-3xl mx-auto px-6 py-10">
                <Suspense fallback=|| view! {
                    <div class="text-stone-400 text-sm animate-pulse">"Loading\u{2026}"</div>
                }>
                    {move || node.get().map(|result| match result {
                        Ok(n) => {
                            let body_html = render_markdown_plain(n.body.as_deref().unwrap_or(""));
                            let node_type = format!("{:?}", n.node_type).to_lowercase();
                            view! {
                                <article>
                                    <h1 class="text-2xl font-bold text-stone-900 dark:text-stone-100 mb-2">
                                        {n.title.clone()}
                                    </h1>
                                    <div class="flex items-center gap-3 mb-6 text-xs text-stone-400 dark:text-stone-500">
                                        <span class="capitalize">{node_type}</span>
                                        <span>"·"</span>
                                        <span>{n.updated_at.format("%B %-d, %Y").to_string()}</span>
                                        {(!n.tags.is_empty()).then(|| view! {
                                            <span>"·"</span>
                                            <span>
                                                {n.tags.iter().map(|t| t.name.clone()).collect::<Vec<_>>().join(", ")}
                                            </span>
                                        })}
                                    </div>
                                    <div
                                        class="prose max-w-none dark:prose-invert"
                                        inner_html=body_html
                                    />
                                </article>
                            }.into_any()
                        }
                        Err(e) => view! {
                            <div class="text-center py-20">
                                <span class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                    style="font-size: 48px; display: block; margin-bottom: 12px;">
                                    "link_off"
                                </span>
                                <p class="text-stone-500 dark:text-stone-400 text-sm">
                                    "This link is invalid or has expired."
                                </p>
                                <p class="text-stone-400 dark:text-stone-500 text-xs mt-1">
                                    {format!("{e}")}
                                </p>
                            </div>
                        }.into_any(),
                    })}
                </Suspense>
            </main>
        </div>
    }
}
