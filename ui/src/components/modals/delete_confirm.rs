/// Confirmation dialog for destructive deletes.
use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;
use leptos::wasm_bindgen::{JsCast, closure::Closure};

#[component]
pub fn DeleteConfirmModal(
    #[prop(into)] show: Signal<bool>,
    #[prop(into)] item_name: Signal<String>,
    on_confirm: Callback<()>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    let cancel_ref: NodeRef<html::Button> = NodeRef::new();
    super::return_focus_on_close(show);

    // Autofocus the safe action (Cancel) when the dialog opens.
    Effect::new(move |_| {
        if show.get()
            && let Some(win) = web_sys::window()
        {
            let cb = Closure::once_into_js(move || {
                if let Some(el) = cancel_ref.get_untracked() {
                    let _ = el.focus();
                }
            });
            let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    });

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Escape" {
            ev.prevent_default();
            on_cancel.run(());
        } else if let Some(panel) = panel_ref.get_untracked() {
            super::trap_focus(&ev, &panel);
        }
    };

    view! {
        <Show when=move || show.get()>
            <Portal>
                // Backdrop
                <div
                    class="fixed inset-0 z-40 bg-black/50 backdrop-blur-sm"
                    on:click=move |_| on_cancel.run(())
                />
                // Panel
                <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
                    <div
                        node_ref=panel_ref
                        class="bg-white dark:bg-stone-900 rounded-2xl shadow-2xl
                               border border-stone-200 dark:border-stone-700
                               w-full max-w-sm p-6 flex flex-col gap-4"
                        role="alertdialog"
                        aria-modal="true"
                        aria-label="Confirm delete"
                        on:click=|ev| ev.stop_propagation()
                        on:keydown=on_keydown
                    >
                        // Icon + title
                        <div class="flex items-center gap-3">
                            <div class="flex-shrink-0 w-10 h-10 rounded-full bg-red-100 dark:bg-red-900/30
                                        flex items-center justify-center">
                                <span class="material-symbols-outlined text-red-600 dark:text-red-400"
                                      style="font-size: 20px;">"delete_forever"</span>
                            </div>
                            <h2 class="text-base font-semibold text-stone-900 dark:text-stone-100">
                                "Confirm Delete"
                            </h2>
                        </div>

                        // Body
                        <p class="text-sm text-stone-600 dark:text-stone-400 leading-relaxed">
                            "Are you sure you want to delete "
                            <strong class="text-stone-800 dark:text-stone-200">
                                {move || item_name.get()}
                            </strong>
                            "? This action cannot be undone."
                        </p>

                        // Actions
                        <div class="flex justify-end gap-2 pt-1">
                            <button
                                node_ref=cancel_ref
                                class="px-4 py-2 text-sm rounded-lg
                                       text-stone-600 dark:text-stone-400
                                       hover:bg-stone-100 dark:hover:bg-stone-800
                                       transition-colors"
                                on:click=move |_| on_cancel.run(())
                            >
                                "Cancel"
                            </button>
                            <button
                                class="px-4 py-2 text-sm font-medium rounded-lg
                                       bg-red-600 hover:bg-red-700
                                       text-white transition-colors"
                                on:click=move |_| on_confirm.run(())
                            >
                                "Delete"
                            </button>
                        </div>
                    </div>
                </div>
            </Portal>
        </Show>
    }
}
