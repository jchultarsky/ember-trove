//! Focus (#282, proposal B): the controls that put a project in the
//! person's Focus, take it out, and state why it is there.
//!
//! Focus is private to the person looking (`common::project_focus`): the
//! state these controls show is the viewer's own, never the project
//! owner's. The dashboard's Focus section itself is in
//! `project_dashboard.rs`, next to the card it renders.
//!
//! Named `project_focus`: `focus_task` and `focus_note` are about scrolling
//! to a row, and `focus_date` is My Day.

use common::id::NodeId;
use common::project_focus::{FocusInput, MAX_WHY_NOW, normalize_why};
use leptos::prelude::*;

use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::toast::{ToastLevel, push_toast};

/// The glyph of a project that is in the viewer's Focus.
pub const IN_FOCUS_ICON: &str = "center_focus_strong";
/// The glyph of one that is not.
pub const OUT_OF_FOCUS_ICON: &str = "center_focus_weak";

fn focus_glyph(in_focus: bool) -> &'static str {
    if in_focus {
        IN_FOCUS_ICON
    } else {
        OUT_OF_FOCUS_ICON
    }
}

/// What the control does next: its tooltip and its accessible name.
fn focus_action(in_focus: bool) -> &'static str {
    if in_focus {
        "Remove from Focus"
    } else {
        "Add to Focus"
    }
}

/// Put the project in the viewer's Focus, or take it out, and record the
/// new state in `state` once the server has it.
///
/// `state` is `None` while it isn't known. Then nothing happens: an "add"
/// sent for a project that is already there is harmless (the API keeps its
/// why-line), but the control would be showing the wrong word.
pub fn toggle_focus(
    node_id: NodeId,
    state: RwSignal<Option<bool>>,
    busy: RwSignal<bool>,
    on_changed: Option<Callback<bool>>,
) {
    let Some(in_focus) = state.get_untracked() else {
        return;
    };
    if busy.get_untracked() {
        return;
    }
    let next = !in_focus;
    busy.set(true);
    wasm_bindgen_futures::spawn_local(async move {
        let result = if next {
            crate::api::put_focus(node_id, &FocusInput::add())
                .await
                .map(|_| ())
        } else {
            crate::api::remove_focus(node_id).await
        };
        // The control may be gone by now (a menu that closed, a card that
        // re-rendered): `try_` throughout.
        let _ = busy.try_set(false);
        match result {
            Ok(()) => {
                let _ = state.try_set(Some(next));
                push_toast(
                    ToastLevel::Success,
                    if next {
                        "Added to Focus"
                    } else {
                        "Removed from Focus"
                    },
                );
                if let Some(on_changed) = on_changed {
                    on_changed.try_run(next);
                }
            }
            Err(e) => push_toast(ToastLevel::Error, format!("Couldn't change Focus: {e}")),
        }
    });
}

/// The icon button that adds a project to the viewer's Focus or removes it.
/// Renders nothing until `state` is known.
#[component]
pub fn FocusToggle(
    node_id: NodeId,
    /// Whether the project is in the viewer's Focus; `None` while unknown.
    state: RwSignal<Option<bool>>,
    /// Runs after the server took the change, with the new state.
    #[prop(optional, into)]
    on_changed: Option<Callback<bool>>,
    /// The smaller size used on a dashboard card, beside the pin star.
    #[prop(optional)]
    compact: bool,
) -> impl IntoView {
    let busy = RwSignal::new(false);
    move || {
        state.get().map(|in_focus| {
            let tone = if in_focus {
                "text-amber-600 dark:text-amber-400"
            } else {
                "text-stone-400"
            };
            // The node page's header buttons, or the card's smaller ones.
            let shape = if compact {
                "flex-shrink-0 p-1 rounded"
            } else {
                "p-1.5 rounded-lg hover:bg-amber-50 dark:hover:bg-amber-900/30"
            };
            let class = format!(
                "{shape} transition-colors cursor-pointer disabled:opacity-40 \
                 hover:text-amber-600 dark:hover:text-amber-400 {tone}"
            );
            let size = if compact { "font-size:16px;" } else { "" };
            view! {
                <button
                    type="button"
                    class=class
                    data-testid="focus-toggle"
                    aria-pressed=if in_focus { "true" } else { "false" }
                    aria-label=focus_action(in_focus)
                    title=focus_action(in_focus)
                    disabled=move || busy.get()
                    on:click=move |ev| {
                        // On a card, the click must not also open the project.
                        ev.stop_propagation();
                        toggle_focus(node_id, state, busy, on_changed);
                    }
                >
                    <span aria-hidden="true" class="material-symbols-outlined" style=size>
                        {focus_glyph(in_focus)}
                    </span>
                </button>
            }
        })
    }
}

/// The same action as a row of the node page's phone menu. `on_click` runs
/// first (it closes the menu).
#[component]
pub fn FocusMenuItem(
    node_id: NodeId,
    state: RwSignal<Option<bool>>,
    #[prop(into)] on_click: Callback<()>,
) -> impl IntoView {
    let busy = RwSignal::new(false);
    move || {
        state.get().map(|in_focus| {
            view! {
                <button
                    type="button"
                    role="menuitem"
                    class="flex items-center gap-2 w-full text-left px-3 py-2 text-sm
                        text-stone-700 dark:text-stone-200
                        hover:bg-amber-50 dark:hover:bg-amber-900/30
                        hover:text-amber-700 dark:hover:text-amber-400
                        disabled:opacity-40"
                    data-testid="focus-menu-item"
                    disabled=move || busy.get()
                    on:click=move |_| {
                        on_click.run(());
                        toggle_focus(node_id, state, busy, None);
                    }
                >
                    <span aria-hidden="true" class="material-symbols-outlined" style="font-size:18px;">
                        {focus_glyph(in_focus)}
                    </span>
                    {focus_action(in_focus)}
                </button>
            }
        })
    }
}

/// A Focus entry's why-line, with its inline editor: one private line
/// saying why this project, now.
#[component]
pub fn WhyLine(node_id: NodeId, initial: Option<String>) -> impl IntoView {
    let why = RwSignal::new(initial);
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    // Focus the field when the editor opens: tracked in an Effect, not
    // timed (leptos.md, "Overlay close keys").
    let input_ref: NodeRef<leptos::html::Input> = NodeRef::new();
    Effect::new(move |_| {
        if editing.get()
            && let Some(input) = input_ref.get()
        {
            let _ = input.focus();
        }
    });

    let start = move || {
        draft.set(why.get_untracked().unwrap_or_default());
        editing.set(true);
    };
    let save = move || {
        if busy.get_untracked() {
            return;
        }
        // The API's own rule, so a refusal is said here and not after a
        // round trip.
        let text = match normalize_why(Some(&draft.get_untracked())) {
            Ok(text) => text,
            Err(e) => {
                push_toast(ToastLevel::Error, format!("Not saved: {e}."));
                return;
            }
        };
        busy.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::put_focus(node_id, &FocusInput::why(text)).await;
            let _ = busy.try_set(false);
            match result {
                Ok(item) => {
                    let _ = why.try_set(item.why_now);
                    let _ = editing.try_set(false);
                }
                Err(e) => push_toast(
                    ToastLevel::Error,
                    format!("Couldn't save the why-line: {e}"),
                ),
            }
        });
    };

    view! {
        {move || if editing.get() {
            view! {
                <div class="flex items-center gap-1 min-w-0 flex-1">
                    <input
                        node_ref=input_ref
                        type="text"
                        class="min-w-0 flex-1 px-2 py-1 text-sm rounded border
                            border-stone-300 dark:border-stone-600
                            bg-white dark:bg-stone-900
                            text-stone-800 dark:text-stone-100
                            focus:outline-none focus:ring-1 focus:ring-amber-500"
                        data-testid="focus-why-input"
                        aria-label="Why now"
                        placeholder="Why this project, now?"
                        maxlength=MAX_WHY_NOW.to_string()
                        prop:value=move || draft.get()
                        on:input=move |ev| draft.set(event_target_value(&ev))
                        on:keydown=move |ev: web_sys::KeyboardEvent| match ev.key().as_str() {
                            "Enter" => {
                                ev.prevent_default();
                                save();
                            }
                            "Escape" => {
                                // Ours: the window's Escape handler must
                                // not act on it too.
                                ev.stop_propagation();
                                editing.set(false);
                            }
                            _ => {}
                        }
                    />
                    <IconButton
                        icon="check"
                        label="Save why-line"
                        variant=IconButtonVariant::Save
                        disabled=busy
                        on_click=Callback::new(move |()| save())
                    />
                    <IconButton
                        icon="close"
                        label="Cancel"
                        on_click=Callback::new(move |()| editing.set(false))
                    />
                </div>
            }.into_any()
        } else {
            view! {
                <div class="flex items-center gap-1 min-w-0 flex-1">
                    {move || match why.get() {
                        Some(text) => view! {
                            <span class="min-w-0 break-words text-sm text-stone-700 dark:text-stone-200"
                                data-testid="focus-why">
                                {text}
                            </span>
                        }.into_any(),
                        None => view! {
                            <span class="text-sm italic text-stone-400 dark:text-stone-500">
                                "Why now?"
                            </span>
                        }.into_any(),
                    }}
                    <IconButton
                        icon="edit"
                        label="Edit why-line"
                        on_click=Callback::new(move |()| start())
                    />
                </div>
            }.into_any()
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_names_what_a_click_will_do() {
        assert_eq!(focus_action(false), "Add to Focus");
        assert_eq!(focus_action(true), "Remove from Focus");
        assert_ne!(focus_glyph(true), focus_glyph(false));
    }

    /// A glyph missing from the font subset renders as its raw name
    /// (`scripts/refresh-icon-font.sh`, ERRORS.md 2026-11-10).
    #[test]
    fn both_glyphs_are_in_the_font_subset() {
        let names = include_str!("../../icon-names.txt");
        for glyph in [IN_FOCUS_ICON, OUT_OF_FOCUS_ICON] {
            assert!(
                names.lines().any(|n| n == glyph),
                "{glyph} is not in ui/icon-names.txt: run scripts/refresh-icon-font.sh"
            );
        }
    }
}
