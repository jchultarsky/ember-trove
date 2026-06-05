use leptos::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

#[component]
pub fn DarkModeToggle() -> impl IntoView {
    let theme = expect_context::<RwSignal<Theme>>();

    let icon = move || match theme.get() {
        Theme::Light => "dark_mode",
        Theme::Dark => "light_mode",
    };

    let label = move || match theme.get() {
        Theme::Light => "Switch to dark mode",
        Theme::Dark => "Switch to light mode",
    };

    view! {
        <button
            class="p-1.5 rounded-lg hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
            aria-label=label
            on:click=move |_| {
                theme.update(|t| {
                    *t = match t {
                        Theme::Light => Theme::Dark,
                        Theme::Dark  => Theme::Light,
                    };
                });
            }
        >
            <span class="material-symbols-outlined">{icon}</span>
        </button>
    }
}
