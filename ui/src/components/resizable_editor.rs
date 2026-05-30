//! Shared resizable text editor. A `<textarea>` the user can drag-resize; when
//! the drag ends (mouseup with a changed height) it reports the new pixel
//! height via `on_resize`, so callers can persist it per-item. `initial_height`
//! opens the editor at a previously-saved size. Used for note + task editing so
//! they stay visually and behaviourally consistent.

use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

const DEFAULT_CLASS: &str = "w-full px-3 py-2 rounded-lg border border-stone-200 dark:border-stone-700 \
    bg-stone-50 dark:bg-stone-800 text-sm text-stone-800 dark:text-stone-200 \
    placeholder-stone-400 dark:placeholder-stone-600 resize-y min-h-[64px] \
    focus:outline-none focus:ring-2 focus:ring-amber-500/40";

#[component]
pub fn ResizableEditor(
    /// Editor text, two-way via the signal.
    value: RwSignal<String>,
    #[prop(into)] placeholder: String,
    /// Opens the editor at this pixel height when set (a previously-saved size).
    #[prop(optional_no_strip)] initial_height: Option<i32>,
    /// Invoked with the new pixel height when the user finishes a resize drag.
    #[prop(optional, into)] on_resize: Option<Callback<i32>>,
    /// Invoked on Ctrl/Cmd+Enter (submit shortcut).
    #[prop(optional, into)] on_submit: Option<Callback<()>>,
    /// Invoked on Escape (cancel shortcut).
    #[prop(optional, into)] on_escape: Option<Callback<()>>,
    /// Override the default textarea classes.
    #[prop(optional, into)] class: Option<String>,
) -> impl IntoView {
    // Track the last reported height so a plain click (mouseup without a resize)
    // doesn't fire a redundant save.
    let last_h = RwSignal::new(initial_height.unwrap_or(0));
    let style = initial_height
        .map(|h| format!("height: {h}px;"))
        .unwrap_or_default();
    let cls = class.unwrap_or_else(|| DEFAULT_CLASS.to_string());

    view! {
        <textarea
            class=cls
            style=style
            placeholder=placeholder
            prop:value=move || value.get()
            on:input=move |ev| value.set(event_target_value(&ev))
            on:mouseup=move |ev| {
                let Some(cb) = on_resize else { return; };
                let Some(target) = ev.target() else { return; };
                if let Ok(el) = target.dyn_into::<web_sys::HtmlElement>() {
                    let h = el.offset_height();
                    if h > 0 && h != last_h.get_untracked() {
                        last_h.set(h);
                        cb.run(h);
                    }
                }
            }
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if let Some(cb) = on_submit
                    && ev.key() == "Enter"
                    && (ev.ctrl_key() || ev.meta_key())
                {
                    ev.prevent_default();
                    cb.run(());
                } else if let Some(cb) = on_escape
                    && ev.key() == "Escape"
                {
                    ev.prevent_default();
                    cb.run(());
                }
            }
        ></textarea>
    }
}
