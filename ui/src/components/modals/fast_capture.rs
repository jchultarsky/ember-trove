//! Fast-capture modal — the *primary* `n`-shortcut surface.
//!
//! One autofocused textarea, no other fields, no decisions.  Cmd/Ctrl+Enter
//! sends the text to `POST /api/inbox/quick`, which lands a Task in the
//! Inbox.  Esc closes.  "More fields…" hands off to the structured
//! `CreateNodeModal` for typed-knowledge captures (article/project/area/etc).
//!
//! Why a separate component vs. extending `CreateNodeModal`:
//!   - The structured modal asks for type + template + body + title — that's
//!     the right surface for "I want to make a Project node," not for "I'm
//!     about to lose this thought."
//!   - Different submit target (Task in Inbox vs. typed Node), different
//!     post-success behaviour (toast + close vs. navigate to new node).
//!   - The `n` shortcut should always be the friction floor.  Extending
//!     CreateNodeModal would mean either gating on a flag (more code paths)
//!     or breaking existing callers.
//!
//! See `common::inbox::QuickCaptureRequest` for the wire contract.

use leptos::html;
use leptos::prelude::*;
use leptos::wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;

use crate::{
    api::quick_capture,
    components::toast::{ToastLevel, push_toast},
};

/// Fast-capture modal.  Show/hide is driven by the `ShowCapture` context
/// signal that's also wired to the `n` keyboard shortcut in `layout.rs`.
///
/// Props:
///   * `show` — visibility signal (typically `ShowCapture(...).0.read_only()`)
///   * `on_close` — fired on Esc, backdrop click, successful save, or
///     when the user clicks "More fields…"
///   * `on_more_fields` — fired when the user clicks "More fields…".  The
///     parent is responsible for opening the structured `CreateNodeModal`.
///     The current draft text is passed so the structured modal can pre-fill
///     its body field (so users don't lose what they already typed).
#[component]
pub fn FastCaptureModal(
    #[prop(into)] show: Signal<bool>,
    on_close: Callback<()>,
    on_more_fields: Callback<String>,
) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error: RwSignal<Option<String>> = RwSignal::new(None);
    let textarea_ref: NodeRef<html::Textarea> = NodeRef::new();

    // Reset every time the modal opens, then focus the textarea on the next
    // animation frame (we cannot focus an element that hasn't yet been
    // attached to the DOM).
    Effect::new(move |_| {
        if show.get() {
            text.set(String::new());
            error.set(None);
            loading.set(false);
            // request_animation_frame is the standard wasm-bindgen approach
            // for "wait one frame" — the textarea exists in the DOM by then.
            if let Some(win) = web_sys::window() {
                let cb = Closure::once_into_js(move || {
                    if let Some(el) = textarea_ref.get_untracked() {
                        let _ = el.focus();
                    }
                });
                let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }
    });

    // Live natural-language parse: "buy milk friday p1" → due + priority.
    // Re-derives on every keystroke; the chips below the textarea preview
    // the interpretation so token misfires are visible before submitting.
    let parsed = Memo::new(move |_| {
        let input = text.get();
        let today = crate::components::format_helpers::local_today();
        common::quickadd::parse_quick_add(&input, today)
    });

    let submit_pending = RwSignal::new(false);
    Effect::new(move |_| {
        if !submit_pending.get() {
            return;
        }
        submit_pending.set(false);
        let p = parsed.get_untracked();
        if p.title.trim().is_empty() {
            error.set(Some("Type something to capture.".to_string()));
            return;
        }
        // Send the de-tokenized text as `body` so the API's coalesce_capture
        // treats it as a single chunk; `title` is left empty and gets derived
        // server-side via truncation.
        let owned = p.title.clone();
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match quick_capture("", Some(&owned), p.due_date, p.priority).await {
                Ok(resp) => {
                    loading.set(false);
                    let msg = if resp.truncated {
                        "Captured to Inbox (text was clipped to 500 chars)".to_string()
                    } else {
                        "Captured to Inbox".to_string()
                    };
                    push_toast(ToastLevel::Success, msg);
                    on_close.run(());
                }
                Err(e) => {
                    loading.set(false);
                    error.set(Some(e.to_string()));
                }
            }
        });
    });

    let handle_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" {
            ev.prevent_default();
            on_close.run(());
        } else if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            submit_pending.set(true);
        }
    };

    let on_more = move |_| {
        let draft = text.get_untracked();
        on_more_fields.run(draft);
    };

    view! {
        <Show when=move || show.get()>
            <div
                class="fixed inset-0 z-40 bg-black/50 backdrop-blur-sm"
                on:click=move |_| on_close.run(())
            />
            <div class="fixed inset-0 z-50 flex items-start justify-center p-4 sm:p-8 pointer-events-none">
                <div
                    class="w-full max-w-xl bg-stone-50 dark:bg-stone-900 rounded-2xl shadow-2xl \
                           border border-stone-200 dark:border-stone-700 pointer-events-auto \
                           mt-8 sm:mt-16"
                    on:keydown=handle_keydown
                >
                    <div class="p-4 flex items-center justify-between border-b border-stone-200 \
                                dark:border-stone-700">
                        <div class="flex items-center gap-2 text-stone-700 dark:text-stone-200">
                            <span class="material-symbols-outlined text-amber-600 dark:text-amber-500">"bolt"</span>
                            <span class="font-medium">"Quick capture"</span>
                        </div>
                        <button
                            type="button"
                            class="text-stone-500 dark:text-stone-400 hover:text-stone-800 \
                                   dark:hover:text-stone-100 text-sm"
                            on:click=move |_| on_close.run(())
                            aria-label="Close"
                        >
                            "Esc"
                        </button>
                    </div>
                    <div class="p-4">
                        <textarea
                            node_ref=textarea_ref
                            class="w-full bg-transparent text-stone-900 dark:text-stone-100 \
                                   placeholder-stone-400 dark:placeholder-stone-500 outline-none \
                                   resize-none text-base leading-relaxed min-h-[6rem]"
                            placeholder="What's on your mind? Try \u{201c}buy milk friday p1\u{201d} (Cmd/Ctrl+Enter to save)"
                            rows="4"
                            prop:value=move || text.get()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                text.set(v);
                            }
                        ></textarea>
                        // Parse preview — what the date/priority tokens will become.
                        {move || {
                            let p = parsed.get();
                            (p.due_date.is_some() || p.priority.is_some()).then(|| view! {
                                <div class="mt-2 flex items-center gap-2 flex-wrap" aria-live="polite">
                                    {p.due_date.map(|d| view! {
                                        <span class="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full
                                                     bg-amber-50 dark:bg-amber-900/30
                                                     text-amber-700 dark:text-amber-400">
                                            <span class="material-symbols-outlined" style="font-size:12px;">"event"</span>
                                            {format!("Due {}", d.format("%a, %b %-d"))}
                                        </span>
                                    })}
                                    {p.priority.map(|pr| {
                                        let label = match pr {
                                            common::task::TaskPriority::High => "High priority",
                                            common::task::TaskPriority::Medium => "Medium priority",
                                            common::task::TaskPriority::Low => "Low priority",
                                        };
                                        view! {
                                            <span class="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full
                                                         bg-stone-100 dark:bg-stone-800
                                                         text-stone-600 dark:text-stone-300">
                                                <span class="material-symbols-outlined" style="font-size:12px;">"flag"</span>
                                                {label}
                                            </span>
                                        }
                                    })}
                                </div>
                            })
                        }}
                        <Show when=move || error.get().is_some()>
                            <div class="mt-2 text-sm text-red-600 dark:text-red-400">
                                {move || error.get().unwrap_or_default()}
                            </div>
                        </Show>
                    </div>
                    <div class="p-3 flex items-center justify-between border-t border-stone-200 \
                                dark:border-stone-700">
                        <button
                            type="button"
                            class="text-sm text-stone-500 dark:text-stone-400 hover:text-stone-800 \
                                   dark:hover:text-stone-100"
                            on:click=on_more
                        >
                            "More fields…"
                        </button>
                        <button
                            type="button"
                            class="px-4 py-1.5 rounded-lg bg-amber-600 text-white text-sm \
                                   hover:bg-amber-700 disabled:opacity-50"
                            prop:disabled=move || loading.get()
                            on:click=move |_| submit_pending.set(true)
                        >
                            {move || if loading.get() { "Saving…" } else { "Capture" }}
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
