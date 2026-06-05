//! Shared skeleton placeholders for Suspense fallbacks.
//!
//! Replaces the old "Loading…" text with a pulsing grey shell that roughly
//! matches the shape of the content about to load.  Fixes the abrupt layout
//! jump when fetched data arrives and gives users a sense that the app is
//! actually working.

use leptos::prelude::*;

const BAR: &str = "animate-pulse bg-stone-200 dark:bg-stone-800 rounded";

/// A generic muted bar.  `width` and `height` accept any Tailwind size class.
#[component]
pub fn SkeletonBar(
    #[prop(default = "w-full")] width: &'static str,
    #[prop(default = "h-4")] height: &'static str,
) -> impl IntoView {
    let class = format!("{BAR} {width} {height}");
    view! { <div class=class /> }
}

/// A row placeholder shaped like a task or list item: a small square on the
/// left (checkbox / icon) and a truncated text bar taking most of the width.
#[component]
pub fn SkeletonListRow() -> impl IntoView {
    view! {
        <div class="flex items-center gap-2 py-2">
            <div class=format!("{BAR} w-5 h-5 flex-shrink-0") />
            <div class=format!("{BAR} h-4 flex-1") style="max-width: 70%;" />
        </div>
    }
}

/// A stack of row placeholders — usable as a Suspense fallback for any
/// list-shaped view (My Day tasks, Inbox tasks, Notes feed, etc.).
#[component]
pub fn SkeletonList(#[prop(default = 5)] rows: usize) -> impl IntoView {
    view! {
        <div class="divide-y divide-stone-100 dark:divide-stone-800/60">
            {(0..rows).map(|_| view! { <SkeletonListRow /> }).collect_view()}
        </div>
    }
}

/// A card-shaped placeholder for the project dashboard.
#[component]
pub fn SkeletonCard() -> impl IntoView {
    view! {
        <div class="rounded-lg border border-stone-200 dark:border-stone-800
                    bg-stone-50/50 dark:bg-stone-800/40 px-4 py-3">
            <div class="flex items-center gap-3 mb-2">
                <div class=format!("{BAR} w-5 h-5 flex-shrink-0") />
                <div class=format!("{BAR} h-4 w-48") />
                <div class=format!("{BAR} h-3 w-16 ml-auto") />
            </div>
            <div class=format!("{BAR} h-1.5 w-28") />
        </div>
    }
}

/// A column of card placeholders for the dashboard.
#[component]
pub fn SkeletonCards(#[prop(default = 3)] cards: usize) -> impl IntoView {
    view! {
        <div class="space-y-4">
            {(0..cards).map(|_| view! { <SkeletonCard /> }).collect_view()}
        </div>
    }
}

/// An article-shaped placeholder: a title bar followed by several text lines
/// of varied widths.  Used as a NodeView fallback.
#[component]
pub fn SkeletonArticle() -> impl IntoView {
    view! {
        <div class="p-6 space-y-4">
            <div class=format!("{BAR} h-6 w-1/2") />
            <div class="space-y-2">
                <div class=format!("{BAR} h-4 w-full") />
                <div class=format!("{BAR} h-4 w-11/12") />
                <div class=format!("{BAR} h-4 w-5/6") />
                <div class=format!("{BAR} h-4 w-3/4") />
            </div>
        </div>
    }
}
