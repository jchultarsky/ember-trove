pub mod add_favorite;
pub mod command_palette;
pub mod convert_capture;
pub mod create_node;
pub mod delete_account;
pub mod delete_confirm;
pub mod fast_capture;
pub mod help;

use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

/// Where a [`ModalShell`] panel sits.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ModalPlacement {
    /// Vertically centred (dialogs and confirmations).
    #[default]
    Center,
    /// Near the top, like a launcher (palette, quick capture).
    Top,
    /// A full-screen sheet on phones, a centred dialog from `sm` up: for
    /// forms too tall for a phone-sized dialog (the person editor).
    Sheet,
}

/// The chrome every modal shares: portal, blurred backdrop (click closes),
/// a positioned `rounded-2xl` panel with `role` / `aria-modal` / `aria-label`,
/// Escape-to-close, the Tab focus trap, and focus return on close.
///
/// Seven modals hand-rolled this and drifted: one had no backdrop close, one
/// no portal, and the change-password dialog had no role, Escape, or trap.
///
/// `on_key` runs first for the modal's own keys (Ctrl+Enter submit, arrows);
/// it claims a key with `prevent_default()`. Escape is handled in the panel
/// and stopped there, so the window dispatcher never also acts on it
/// (`common::keyboard::escape_target`).
#[component]
pub fn ModalShell(
    #[prop(into)] show: Signal<bool>,
    on_close: Callback<()>,
    /// Accessible name (`aria-label`).
    label: &'static str,
    /// Size, padding and layout of the panel; surface classes are shared.
    panel_class: &'static str,
    #[prop(optional)] placement: ModalPlacement,
    /// `role="alertdialog"`: a confirmation that interrupts.
    #[prop(optional)]
    alert: bool,
    #[prop(optional)] on_key: Option<Callback<web_sys::KeyboardEvent>>,
    children: ChildrenFn,
) -> impl IntoView {
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    return_focus_on_close(show);

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if let Some(cb) = on_key {
            cb.run(ev.clone());
            if ev.default_prevented() {
                return;
            }
        }
        if ev.key() == "Escape" {
            ev.prevent_default();
            ev.stop_propagation();
            on_close.run(());
        } else if let Some(panel) = panel_ref.get_untracked() {
            trap_focus(&ev, &panel);
        }
    };

    // The wrapper ignores the pointer so clicks beside the panel reach the
    // backdrop; the panel takes them back.
    let wrapper = match placement {
        ModalPlacement::Center => {
            "fixed inset-0 z-50 flex items-center justify-center p-4 pointer-events-none"
        }
        ModalPlacement::Top => {
            "fixed inset-0 z-50 flex items-start justify-center p-4 pt-16 sm:pt-24 \
             pointer-events-none"
        }
        ModalPlacement::Sheet => {
            "fixed inset-0 z-50 flex items-stretch justify-center sm:items-center sm:p-4 \
             pointer-events-none"
        }
    };
    // A sheet is edge to edge on phones: no rounding or border until `sm`.
    // Separate base strings, so no two classes compete for one property.
    let surface = match placement {
        ModalPlacement::Sheet => {
            "pointer-events-auto bg-white dark:bg-stone-900 shadow-2xl w-full \
             sm:rounded-2xl sm:border border-stone-200 dark:border-stone-700"
        }
        _ => {
            "pointer-events-auto bg-white dark:bg-stone-900 rounded-2xl shadow-2xl \
             border border-stone-200 dark:border-stone-700"
        }
    };
    let panel = StoredValue::new(format!("{surface} {panel_class}"));
    let children = StoredValue::new(children);

    view! {
        <Show when=move || show.get()>
            <Portal>
                <div
                    class="fixed inset-0 z-40 bg-black/50 backdrop-blur-sm"
                    on:click=move |_| on_close.run(())
                />
                <div class=wrapper>
                    <div
                        node_ref=panel_ref
                        class=panel.get_value()
                        role=if alert { "alertdialog" } else { "dialog" }
                        aria-modal="true"
                        aria-label=label
                        on:keydown=on_keydown
                    >
                        {children.with_value(|c| c())}
                    </div>
                </div>
            </Portal>
        </Show>
    }
}

/// Selector for the elements a modal focus trap cycles through.
const FOCUSABLE: &str = "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])";

/// Keep `Tab` / `Shift+Tab` cycling inside `container` (WCAG modal focus
/// trap). Attach from the modal panel's `on:keydown` with the panel element.
pub fn trap_focus(ev: &web_sys::KeyboardEvent, container: &web_sys::HtmlElement) {
    if ev.key() != "Tab" {
        return;
    }
    let Ok(nodes) = container.query_selector_all(FOCUSABLE) else {
        return;
    };
    if nodes.length() == 0 {
        return;
    }
    let first = nodes.get(0);
    let last = nodes.get(nodes.length() - 1);
    let active: Option<web_sys::Node> = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .map(|e| e.unchecked_into());

    let is_same = |a: &Option<web_sys::Node>, b: &Option<web_sys::Node>| match (a, b) {
        (Some(a), Some(b)) => a.is_same_node(Some(b)),
        _ => false,
    };

    if ev.shift_key() {
        if is_same(&active, &first) {
            ev.prevent_default();
            if let Some(el) = last.and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
                let _ = el.focus();
            }
        }
    } else if is_same(&active, &last) {
        ev.prevent_default();
        if let Some(el) = first.and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    }
}

/// Restore focus to whatever had it before the modal opened.
///
/// Call once during component setup with the modal's `show` signal: the
/// element focused at open time is remembered, and focused again when the
/// modal closes (WCAG focus-return).
pub fn return_focus_on_close(show: Signal<bool>) {
    let prev_focus: StoredValue<Option<web_sys::HtmlElement>> = StoredValue::new(None);
    Effect::new(move |was_open: Option<bool>| {
        let open = show.get();
        if open && was_open != Some(true) {
            prev_focus.set_value(
                web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.active_element())
                    .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()),
            );
        } else if !open
            && was_open == Some(true)
            && let Some(el) = prev_focus.get_value()
        {
            let _ = el.focus();
        }
        open
    });
}
