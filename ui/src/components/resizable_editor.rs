//! Shared resizable text editor. A `<textarea>` with an always-visible drag
//! bar along its bottom edge: grab the bar (mouse or touch, via pointer
//! events) and drag to resize. The native bottom-right corner grip stays too
//! (`resize-y`), but the bar is the discoverable affordance — the corner grip
//! is tiny and near-invisible on tinted note cards, which read as "the edit
//! field is fixed".
//!
//! When a drag ends with a changed height the new pixel height is reported
//! via `on_resize`, so callers can persist it per-item (`editor-prefs` API).
//! `initial_height` opens the editor at a previously-saved size. Used for
//! every note surface and the task title editors so they stay visually and
//! behaviourally consistent.

use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

const DEFAULT_CLASS: &str = "w-full px-3 py-2 rounded-lg border border-stone-200 dark:border-stone-700 \
    bg-stone-50 dark:bg-stone-800 text-sm text-stone-800 dark:text-stone-200 \
    placeholder-stone-400 dark:placeholder-stone-600 resize-y min-h-[64px] \
    focus:outline-none focus:ring-2 focus:ring-amber-500/40";

/// Height floor in px — matches the `min-h-[64px]` on the default class.
const MIN_HEIGHT: i32 = 64;

#[component]
pub fn ResizableEditor(
    /// Editor text, two-way via the signal.
    value: RwSignal<String>,
    #[prop(into)] placeholder: String,
    /// Opens the editor at this pixel height when set (a previously-saved size).
    #[prop(optional_no_strip)]
    initial_height: Option<i32>,
    /// Invoked with the new pixel height when the user finishes a resize drag.
    #[prop(optional, into)]
    on_resize: Option<Callback<i32>>,
    /// Invoked on Ctrl/Cmd+Enter (submit shortcut).
    #[prop(optional, into)]
    on_submit: Option<Callback<()>>,
    /// Invoked on Escape (cancel shortcut).
    #[prop(optional, into)]
    on_escape: Option<Callback<()>>,
    /// Override the default textarea classes.
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    // Track the last reported height so a plain click (mouseup without a resize)
    // doesn't fire a redundant save.
    let last_h = RwSignal::new(initial_height.unwrap_or(0));
    let style = initial_height
        .map(|h| format!("height: {h}px;"))
        .unwrap_or_default();
    let cls = class.unwrap_or_else(|| DEFAULT_CLASS.to_string());

    let textarea_ref: NodeRef<leptos::html::Textarea> = NodeRef::new();
    // Drag state for the bottom bar: (pointer start y, textarea start height).
    // None when no drag is in progress.
    let drag: RwSignal<Option<(i32, i32)>> = RwSignal::new(None);

    let report = move |h: i32| {
        if h > 0
            && h != last_h.get_untracked()
            && let Some(cb) = on_resize
        {
            last_h.set(h);
            cb.run(h);
        }
    };

    view! {
        <div class="w-full">
            <textarea
                node_ref=textarea_ref
                class=cls
                style=style
                placeholder=placeholder
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
                // Native corner-grip path: report the height after a drag.
                on:mouseup=move |ev| {
                    if let Some(target) = ev.target()
                        && let Ok(el) = target.dyn_into::<web_sys::HtmlElement>()
                    {
                        report(el.offset_height());
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
                        // The editor owns this Escape. Without the stop it
                        // reached the window dispatcher after `cb` had closed
                        // the editor (focus back on <body>, so not "editing"),
                        // which on a node page means "back to /nodes" — a
                        // stray Escape discarded the edit AND left the node
                        // (#123; .claude/rules/leptos.md, overlay close keys).
                        ev.stop_propagation();
                        cb.run(());
                    }
                }
            ></textarea>
            // Drag bar — full-width grab strip under the editor. Pointer events
            // cover mouse and touch; pointer capture keeps the drag alive when
            // the cursor leaves the strip mid-drag.
            <div
                class="group flex items-center justify-center h-3 -mt-0.5 \
                       cursor-ns-resize touch-none select-none"
                title="Drag to resize"
                on:pointerdown=move |ev: web_sys::PointerEvent| {
                    let Some(ta) = textarea_ref.get_untracked() else { return; };
                    if let Some(target) = ev.target()
                        && let Ok(el) = target.dyn_into::<web_sys::Element>()
                    {
                        let _ = el.set_pointer_capture(ev.pointer_id());
                    }
                    ev.prevent_default();
                    drag.set(Some((ev.client_y(), ta.offset_height())));
                }
                on:pointermove=move |ev: web_sys::PointerEvent| {
                    let Some((start_y, start_h)) = drag.get_untracked() else { return; };
                    let Some(ta) = textarea_ref.get_untracked() else { return; };
                    let h = (start_h + (ev.client_y() - start_y)).max(MIN_HEIGHT);
                    // Fully qualified: leptos's `ElementExt::style` shadows the
                    // web_sys CSSStyleDeclaration accessor on the deref chain.
                    let _ = web_sys::HtmlElement::style(&ta)
                        .set_property("height", &format!("{h}px"));
                }
                on:pointerup=move |_| {
                    if drag.get_untracked().is_none() { return; }
                    drag.set(None);
                    if let Some(ta) = textarea_ref.get_untracked() {
                        report(ta.offset_height());
                    }
                }
                on:pointercancel=move |_| drag.set(None)
            >
                // Always-visible muted grip (Tailwind v4 group-hover is
                // unreliable — see .claude/rules/leptos.md).
                <div class="w-10 h-1 rounded-full bg-stone-300 dark:bg-stone-600"></div>
            </div>
        </div>
    }
}
