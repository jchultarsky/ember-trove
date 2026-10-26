use leptos::prelude::*;

use crate::api::change_password;
use crate::components::modals::ModalShell;

#[component]
pub fn ChangePasswordModal(
    #[prop(into)] show: Signal<bool>,
    on_close: Callback<()>,
) -> impl IntoView {
    let current = RwSignal::new(String::new());
    let proposed = RwSignal::new(String::new());
    let confirmed = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let success = RwSignal::new(false);
    let current_ref: NodeRef<leptos::html::Input> = NodeRef::new();

    // Start clean on every open, and drop typed passwords on close rather
    // than keeping them in signals until the next open.
    Effect::new(move |_| {
        let _ = show.get();
        current.set(String::new());
        proposed.set(String::new());
        confirmed.set(String::new());
        error.set(None);
        success.set(false);
        saving.set(false);
    });
    // Focus "Current password" once mounted (the shell traps Tab inside).
    Effect::new(move |_| {
        if show.get()
            && let Some(el) = current_ref.get()
        {
            let _ = el.focus();
        }
    });

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
    <ModalShell
                show=show
                on_close=on_close
                label="Change password"
                panel_class="relative w-full max-w-sm mx-4 p-6 space-y-4"
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
                        title="Close"
                        aria-label="Close"
                        class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                               cursor-pointer p-1 rounded"
                        on:click=move |_| on_close.run(())
                    >
                        <span aria-hidden="true" class="material-symbols-outlined" style="font-size: 18px;">"close"</span>
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
                            class="mt-1 text-xs bg-ember hover:bg-ember-strong text-white
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
                                node_ref=current_ref
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
                                class="inline-flex items-center justify-center gap-1.5 text-xs bg-ember hover:bg-ember-strong text-white
                                   rounded px-4 py-1.5 transition-colors
                                   disabled:opacity-50 disabled:cursor-not-allowed"
                                disabled=move || saving.get()
                                on:click=move |_| do_submit()
                            >
                                <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"key"</span>
                                {move || if saving.get() { "Saving…" } else { "Change password" }}
                            </button>
                        </div>
                    </div>
                })}
            </ModalShell>
        }
}
