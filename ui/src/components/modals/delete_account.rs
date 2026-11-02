//! Deleting an account and everything it owns (#239).
//!
//! The dialog loads what would go (counts only, never titles), says what
//! happens to shared nodes and to tasks in other people's Projects, warns
//! that earlier backups still hold the data, and stays disabled until the
//! admin types the account's email.

use common::admin::PurgeCounts;
use leptos::html;
use leptos::prelude::*;

use super::ModalShell;
use crate::api;
use crate::components::toast::{ToastLevel, push_toast};

/// "1 node", "3 nodes".
fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What deleting removes, in words: the non-zero kinds, joined with commas
/// and a final "and". "nothing" when there is nothing to remove.
pub fn removal_list(c: &PurgeCounts) -> String {
    let parts: Vec<String> = [
        (c.nodes, "node", "nodes"),
        (c.notes, "note", "notes"),
        (c.tasks, "task", "tasks"),
        (c.captures, "capture", "captures"),
        (c.people, "person", "people"),
        (c.files, "file", "files"),
        (c.settings, "setting", "settings"),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, one, many)| plural(n, one, many))
    .collect();
    match parts.as_slice() {
        [] => "nothing".to_string(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// Whether the typed confirmation matches the account's email (case and
/// surrounding spaces ignored, as email addresses are).
pub fn confirms(typed: &str, email: &str) -> bool {
    let email = email.trim();
    !email.is_empty() && typed.trim().eq_ignore_ascii_case(email)
}

#[component]
pub fn DeleteAccountDialog(
    show: RwSignal<bool>,
    /// The account id (canonical UUID).
    user_id: String,
    /// What the admin must type to confirm: the account's email.
    email: String,
    /// Shown in the text.
    name: String,
    /// The account changed on the server: deleted, or (after a failure)
    /// suspended and maybe partly purged. The row reloads the list.
    on_changed: Callback<()>,
) -> impl IntoView {
    let typed = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    // Bumped after a failed delete, so the counts are read again.
    let recount = RwSignal::new(0u32);
    // A delete failed: the account is suspended now, and may be partly
    // purged, so the list must reload when the dialog closes.
    let failed = RwSignal::new(false);
    let cancel_ref: NodeRef<html::Button> = NodeRef::new();
    let user_id = StoredValue::new(user_id);
    let email = StoredValue::new(email);
    let name = StoredValue::new(name);

    // Load the counts each time the dialog opens.
    let preview = LocalResource::new(move || {
        let open = show.get();
        let _ = recount.get();
        let id = user_id.get_value();
        async move {
            if open {
                Some(api::deletion_preview(&id).await)
            } else {
                None
            }
        }
    });

    // Reset and focus the safe action on open (NodeRef + Effect; leptos.md).
    Effect::new(move |_| {
        if show.get() {
            typed.set(String::new());
            error.set(None);
            if let Some(el) = cancel_ref.get() {
                let _ = el.focus();
            }
        }
    });

    let counts = move || preview.get().flatten().and_then(Result::ok);
    let ready = move || {
        counts().is_some() && !busy.get() && email.with_value(|e| confirms(&typed.get(), e))
    };
    let close = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        show.set(false);
        if failed.get_untracked() {
            failed.set(false);
            on_changed.run(());
        }
    });
    let delete = move |_| {
        if !ready() {
            return;
        }
        busy.set(true);
        error.set(None);
        let id = user_id.get_value();
        let who = name.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            // The page may be gone by the time this returns (the admin
            // navigated away): `try_` everything, and toast from here.
            match api::delete_admin_user(&id).await {
                Ok(removed) => {
                    push_toast(
                        ToastLevel::Success,
                        format!("Deleted {who}'s account and {}", removal_list(&removed)),
                    );
                    let _ = show.try_set(false);
                    on_changed.try_run(());
                }
                Err(e) => {
                    let _ = error.try_set(Some(format!(
                        "Delete failed: {e}. The account is suspended; try again."
                    )));
                    let _ = failed.try_set(true);
                    let _ = recount.try_update(|n| *n += 1);
                }
            }
            let _ = busy.try_set(false);
        });
    };

    view! {
        <ModalShell
            show=show
            on_close=close
            label="Delete account"
            panel_class="w-full max-w-md p-6 flex flex-col gap-4"
            alert=true
        >
            <div class="flex items-center gap-3">
                <div class="flex-shrink-0 w-10 h-10 rounded-full bg-red-100 dark:bg-red-900/30
                            flex items-center justify-center">
                    <span class="material-symbols-outlined text-red-600 dark:text-red-400"
                          style="font-size: 20px;" aria-hidden="true">"person_remove"</span>
                </div>
                <h2 class="text-base font-semibold text-stone-900 dark:text-stone-100">
                    "Delete account and data"
                </h2>
            </div>

            <div class="text-sm text-stone-600 dark:text-stone-400 leading-relaxed space-y-2"
                 data-testid="deletion-preview" aria-live="polite">
                {move || match preview.get().flatten() {
                    None => view! { <p>"Counting what this account owns…"</p> }.into_any(),
                    Some(Err(e)) => view! {
                        <p class="text-red-600 dark:text-red-400" role="alert">
                            {format!("Couldn't count what this account owns: {e}")}
                        </p>
                    }.into_any(),
                    Some(Ok(c)) => view! {
                        <p>
                            "Deletes "
                            <strong class="text-stone-800 dark:text-stone-200">{name.get_value()}</strong>
                            "'s account and everything they own: "
                            {removal_list(&c)}
                            "."
                        </p>
                        {(c.shared_nodes > 0).then(|| view! {
                            <p>
                                {format!(
                                    "{} shared with other people, who lose access.",
                                    if c.shared_nodes == 1 {
                                        "1 of those nodes is".to_string()
                                    } else {
                                        format!("{} of those nodes are", c.shared_nodes)
                                    }
                                )}
                            </p>
                        })}
                        {(c.tasks_handed_over > 0).then(|| view! {
                            <p>
                                {format!(
                                    "{} they added to other people's Projects {} to those Projects' owners.",
                                    plural(c.tasks_handed_over, "task", "tasks"),
                                    if c.tasks_handed_over == 1 { "passes" } else { "pass" }
                                )}
                            </p>
                        })}
                    }.into_any(),
                }}
                <p class="text-amber-700 dark:text-amber-400">
                    "This can't be undone. Backups taken before now still hold this data until you delete them."
                </p>
            </div>

            <label class="flex flex-col gap-1 text-sm text-stone-700 dark:text-stone-300">
                <span>
                    "Type "
                    <strong class="font-mono">{email.get_value()}</strong>
                    " to confirm"
                </span>
                <input
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    aria-label="Type the account's email to confirm"
                    class="px-3 py-2 rounded-lg border border-stone-300 dark:border-stone-700
                           bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100"
                    prop:value=move || typed.get()
                    on:input=move |ev| typed.set(event_target_value(&ev))
                />
            </label>

            {move || error.get().map(|msg| view! {
                <p class="text-sm text-red-600 dark:text-red-400" role="alert">{msg}</p>
            })}

            <div class="flex justify-end gap-2 pt-1">
                <button
                    node_ref=cancel_ref
                    class="px-4 py-2 text-sm rounded-lg text-stone-600 dark:text-stone-400
                           hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors
                           disabled:opacity-40 disabled:cursor-not-allowed"
                    disabled=move || busy.get()
                    on:click=move |_| close.run(())
                >
                    "Cancel"
                </button>
                <button
                    class="inline-flex items-center justify-center gap-1.5 px-4 py-2 text-sm font-medium rounded-lg
                           bg-red-600 hover:bg-red-700 text-white transition-colors
                           disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-red-600"
                    disabled=move || !ready()
                    on:click=delete
                >
                    <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"delete_forever"</span>
                    {move || if busy.get() { "Deleting…" } else { "Delete account and data" }}
                </button>
            </div>
        </ModalShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_removal_list_names_only_what_there_is() {
        assert_eq!(removal_list(&PurgeCounts::default()), "nothing");
        let one = PurgeCounts {
            nodes: 1,
            ..PurgeCounts::default()
        };
        assert_eq!(removal_list(&one), "1 node");
        let some = PurgeCounts {
            nodes: 12,
            notes: 1,
            people: 3,
            files: 2,
            shared_nodes: 4,
            tasks_handed_over: 5,
            ..PurgeCounts::default()
        };
        assert_eq!(
            removal_list(&some),
            "12 nodes, 1 note, 3 people and 2 files"
        );
    }

    #[test]
    fn the_confirmation_must_be_the_email() {
        assert!(confirms("ann@example.com", "ann@example.com"));
        assert!(confirms("  Ann@Example.com ", "ann@example.com"));
        assert!(!confirms("ann@example", "ann@example.com"));
        assert!(!confirms("", "ann@example.com"));
        assert!(!confirms("", ""), "no email means no way to confirm");
    }
}
