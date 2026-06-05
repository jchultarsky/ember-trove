use leptos::prelude::*;

use crate::api::change_password;

#[component]
pub fn ChangePasswordModal(on_close: Callback<()>) -> impl IntoView {
    let current = RwSignal::new(String::new());
    let proposed = RwSignal::new(String::new());
    let confirmed = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let success = RwSignal::new(false);

    let do_submit = move || {
        let cur = current.get_untracked();
        let new = proposed.get_untracked();
        let con = confirmed.get_untracked();

        if cur.is_empty() || new.is_empty() {
            error.set(Some("All fields are required.".to_string()));
            return;
        }
        if new != con {
            error.set(Some("New passwords do not match.".to_string()));
            return;
        }
        if new.len() < 8 {
            error.set(Some(
                "New password must be at least 8 characters.".to_string(),
            ));
            return;
        }

        saving.set(true);
        error.set(None);

        leptos::task::spawn_local(async move {
            match change_password(&cur, &new).await {
                Ok(()) => {
                    saving.set(false);
                    success.set(true);
                }
                Err(e) => {
                    saving.set(false);
                    let msg = e.to_string();
                    // Surface a friendly message for wrong-current-password.
                    let friendly = if msg.to_lowercase().contains("incorrect")
                        || msg.to_lowercase().contains("notauthorized")
                        || msg.to_lowercase().contains("unauthorized")
                        || msg.contains("401")
                    {
                        "Current password is incorrect.".to_string()
                    } else {
                        msg
                    };
                    error.set(Some(friendly));
                }
            }
        });
    };

    view! {
        // Backdrop
        <div
            class="fixed inset-0 z-50 flex items-center justify-center
                   bg-black/40 dark:bg-black/60 backdrop-blur-sm"
            on:click=move |_| on_close.run(())
        >
            // Modal card — stop propagation so clicking inside doesn't close
            <div
                class="relative w-full max-w-sm mx-4 bg-white dark:bg-stone-900
                       rounded-xl shadow-2xl border border-stone-200 dark:border-stone-700
                       p-6 space-y-4"
                on:click=|ev| ev.stop_propagation()
            >
                // Header
                <div class="flex items-center justify-between">
                    <h2 class="text-base font-semibold text-stone-900 dark:text-stone-100 flex items-center gap-2">
                        <span class="material-symbols-outlined text-amber-500" style="font-size: 20px;">
                            "lock_reset"
                        </span>
                        "Change Password"
                    </h2>
                    <button
                        class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                               cursor-pointer p-1 rounded"
                        on:click=move |_| on_close.run(())
                    >
                        <span class="material-symbols-outlined" style="font-size: 18px;">"close"</span>
                    </button>
                </div>

                {move || success.get().then(|| view! {
                    <div class="flex flex-col items-center gap-3 py-4 text-center">
                        <span class="material-symbols-outlined text-green-500" style="font-size: 40px;">
                            "check_circle"
                        </span>
                        <p class="text-sm text-stone-700 dark:text-stone-300">
                            "Password changed successfully."
                        </p>
                        <button
                            class="mt-1 text-xs bg-amber-500 hover:bg-amber-600 text-white
                                   rounded px-4 py-1.5 transition-colors"
                            on:click=move |_| on_close.run(())
                        >
                            "Close"
                        </button>
                    </div>
                })}

                {move || (!success.get()).then(|| view! {
                    <div class="space-y-3">
                        // Current password
                        <div class="space-y-1">
                            <label class="text-xs font-medium text-stone-600 dark:text-stone-400">
                                "Current password"
                            </label>
                            <input
                                type="password"
                                autocomplete="current-password"
                                class="w-full rounded-lg border border-stone-200 dark:border-stone-700
                                       bg-stone-50 dark:bg-stone-800 px-3 py-2 text-sm
                                       text-stone-900 dark:text-stone-100 focus:outline-none
                                       focus:ring-2 focus:ring-amber-400"
                                prop:value=move || current.get()
                                on:input=move |ev| current.set(event_target_value(&ev))
                            />
                        </div>
                        // New password
                        <div class="space-y-1">
                            <label class="text-xs font-medium text-stone-600 dark:text-stone-400">
                                "New password"
                            </label>
                            <input
                                type="password"
                                autocomplete="new-password"
                                class="w-full rounded-lg border border-stone-200 dark:border-stone-700
                                       bg-stone-50 dark:bg-stone-800 px-3 py-2 text-sm
                                       text-stone-900 dark:text-stone-100 focus:outline-none
                                       focus:ring-2 focus:ring-amber-400"
                                prop:value=move || proposed.get()
                                on:input=move |ev| proposed.set(event_target_value(&ev))
                            />
                        </div>
                        // Confirm new password
                        <div class="space-y-1">
                            <label class="text-xs font-medium text-stone-600 dark:text-stone-400">
                                "Confirm new password"
                            </label>
                            <input
                                type="password"
                                autocomplete="new-password"
                                class="w-full rounded-lg border border-stone-200 dark:border-stone-700
                                       bg-stone-50 dark:bg-stone-800 px-3 py-2 text-sm
                                       text-stone-900 dark:text-stone-100 focus:outline-none
                                       focus:ring-2 focus:ring-amber-400"
                                prop:value=move || confirmed.get()
                                on:input=move |ev| confirmed.set(event_target_value(&ev))
                                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                    if ev.key() == "Enter" { do_submit(); }
                                }
                            />
                        </div>

                        {move || error.get().map(|msg| view! {
                            <p class="text-xs text-red-500 dark:text-red-400">{msg}</p>
                        })}

                        // Actions
                        <div class="flex justify-end gap-2 pt-1">
                            <button
                                class="text-xs text-stone-500 hover:text-stone-700
                                       dark:hover:text-stone-300 px-3 py-1.5 cursor-pointer"
                                on:click=move |_| on_close.run(())
                            >
                                "Cancel"
                            </button>
                            <button
                                class="text-xs bg-amber-500 hover:bg-amber-600 text-white
                                       rounded px-4 py-1.5 transition-colors
                                       disabled:opacity-50 disabled:cursor-not-allowed"
                                disabled=move || saving.get()
                                on:click=move |_| do_submit()
                            >
                                {move || if saving.get() { "Saving…" } else { "Change password" }}
                            </button>
                        </div>
                    </div>
                })}
            </div>
        </div>
    }
}
