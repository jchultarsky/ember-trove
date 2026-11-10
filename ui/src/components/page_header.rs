use leptos::prelude::*;

/// Canonical page-header bar for top-level views (design phase 2).
///
/// Every list-style view renders its header through this one shape so the
/// title scale, icon size, padding, and border can never drift again
/// (they had: Inbox was `text-xl`/26px while its siblings were
/// `text-lg`/22px, and padding varied `px-4`/`px-6` by view):
///
/// - bar: `px-4 md:px-6 py-4` with a bottom border
/// - icon: optional Material Symbols name, 22 px, amber-500
/// - title: `text-lg font-semibold` (pass `Signal::derive(..)` for a
///   reactive title — see `node_list.rs`)
/// - subtitle: optional `text-xs` view under the title
/// - children: optional right-aligned cluster (counters, actions, nav)
///
/// Not used by `node_view.rs` (bespoke: back button + editable title +
/// action toolbar) or `search_view.rs` (its header bar is a wrapping
/// filter toolbar, a different pattern).
#[component]
pub fn PageHeader(
    /// Material Symbols icon name (22 px, amber-500). Omit for no icon.
    #[prop(optional)]
    icon: Option<&'static str>,
    /// Title text; `#[prop(into)]` accepts `&'static str` or a `Signal`.
    #[prop(into)]
    title: Signal<&'static str>,
    /// Optional subtitle view rendered under the title.
    #[prop(optional)]
    subtitle: Option<AnyView>,
    /// Optional right-aligned cluster.
    #[prop(optional)]
    children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="flex-shrink-0 px-4 md:px-6 py-4 border-b border-stone-200 dark:border-stone-800">
            // Wraps (#262): a wide right-hand cluster (the calendar's month
            // navigation) drops under the title at phone width, where it
            // used to squeeze the title to a few characters a line.
            <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
                {icon
                    .map(|name| {
                        view! {
                            <span
                                class="material-symbols-outlined text-amber-500"
                                style="font-size:22px;"
                            >
                                {name}
                            </span>
                        }
                    })}
                <div class="flex-1 min-w-[9rem]">
                    <h1 class="font-display text-lg font-semibold text-stone-900 dark:text-stone-100">
                        {move || title.get()}
                    </h1>
                    {subtitle
                        .map(|s| {
                            view! { <p class="text-xs text-stone-500 dark:text-stone-400">{s}</p> }
                        })}
                </div>
                {children.map(|c| c())}
            </div>
        </div>
    }
}
