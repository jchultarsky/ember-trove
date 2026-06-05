use leptos::prelude::*;

/// Hover-color treatment for a compact icon-only button.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum IconButtonVariant {
    /// Stone-grey hover — neutral actions (cancel, edit, expand, generic toolbar).
    #[default]
    Neutral,
    /// Green hover — confirm / save actions.
    Save,
    /// Red hover — destructive actions (delete, remove).
    Danger,
    /// Amber hover — accent actions (add, navigate, download).
    /// Part of the deliberate IconButton palette (v2.11.2); no current call
    /// site, kept so accent actions can adopt it without re-deriving the class.
    #[allow(dead_code)]
    Accent,
}

impl IconButtonVariant {
    fn hover_classes(self) -> &'static str {
        match self {
            IconButtonVariant::Neutral => {
                "hover:text-stone-600 dark:hover:text-stone-300 \
                 hover:bg-stone-100 dark:hover:bg-stone-800"
            }
            IconButtonVariant::Save => {
                "hover:text-green-600 dark:hover:text-green-400 \
                 hover:bg-green-50 dark:hover:bg-green-900/30"
            }
            IconButtonVariant::Danger => {
                "hover:text-red-600 dark:hover:text-red-400 \
                 hover:bg-red-50 dark:hover:bg-red-900/30"
            }
            IconButtonVariant::Accent => {
                "hover:text-amber-600 dark:hover:text-amber-400 \
                 hover:bg-amber-50 dark:hover:bg-amber-900/30"
            }
        }
    }
}

/// A compact, icon-only button with a tooltip and accessible label.
///
/// Canonical treatment for inline-edit confirm/cancel and row/toolbar actions —
/// see `.claude/patterns/` and CLAUDE.md "UI language" notes. Modal-footer and
/// full-form CTAs intentionally keep visible text labels and do NOT use this.
#[component]
pub fn IconButton(
    /// Material Symbols glyph name, e.g. `"check"`, `"close"`, `"delete"`.
    #[prop(into)]
    icon: String,
    /// Tooltip text + accessible label (`title` + `aria-label`).
    #[prop(into)]
    label: String,
    /// Click handler.
    #[prop(into)]
    on_click: Callback<()>,
    /// Hover-color treatment. Defaults to `Neutral`.
    #[prop(optional)]
    variant: IconButtonVariant,
    /// When `true`, renders disabled and non-interactive.
    #[prop(optional, into)]
    disabled: Signal<bool>,
    /// When `true`, stops click-event propagation — needed when the button sits
    /// inside a clickable row whose handler must not also fire.
    #[prop(optional)]
    stop_propagation: bool,
) -> impl IntoView {
    let class = format!(
        "p-1.5 rounded-lg text-stone-400 transition-colors cursor-pointer \
         disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-transparent {}",
        variant.hover_classes()
    );
    view! {
        <button
            type="button"
            class=class
            title=label.clone()
            aria-label=label
            disabled=move || disabled.get()
            on:click=move |ev| {
                if stop_propagation {
                    ev.stop_propagation();
                }
                on_click.run(());
            }
        >
            <span class="material-symbols-outlined">{icon}</span>
        </button>
    }
}
