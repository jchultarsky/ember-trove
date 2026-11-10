use leptos::prelude::*;

/// What a view shows when its data failed to load (#281, 1.7).
///
/// Never an empty state: "No notes yet" after a failed fetch reads as lost
/// data. A view that maps `Err` to its empty list is a defect; render this
/// and offer the retry. `what` completes "Couldn't load …", for example
/// "your notes".
#[component]
pub fn LoadError(
    /// Completes "Couldn't load …".
    what: &'static str,
    /// Runs the fetch again (usually bumps the view's refresh signal).
    #[prop(optional, into)]
    on_retry: Option<Callback<()>>,
) -> impl IntoView {
    view! {
        <div
            class="flex flex-col items-center justify-center gap-2 py-8 text-center"
            role="alert"
            data-testid="load-error"
        >
            <span
                class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                style="font-size:32px;"
                aria-hidden="true"
            >
                "warning"
            </span>
            <p class="text-sm text-stone-600 dark:text-stone-400">
                {load_error_message(what)}
            </p>
            {on_retry
                .map(|retry| {
                    view! {
                        <button
                            class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg
                                   border border-stone-300 dark:border-stone-700
                                   text-stone-700 dark:text-stone-300
                                   hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
                            on:click=move |_| retry.run(())
                        >
                            <span
                                class="material-symbols-outlined"
                                style="font-size:14px;"
                                aria-hidden="true"
                            >
                                "refresh"
                            </span>
                            "Try again"
                        </button>
                    }
                })}
        </div>
    }
}

/// The sentence [`LoadError`] shows. It says the data is safe: the worry
/// after an empty-looking page is that something was lost.
pub fn load_error_message(what: &str) -> String {
    format!("Couldn\u{2019}t load {what}. Nothing was changed; it may be the connection.")
}

/// The toast after deleting several things one by one: success only when
/// every delete succeeded (#281, 1.7; it always said "Success", even for
/// "Deleted 0").
pub fn bulk_delete_outcome(deleted: usize, asked: usize, noun: &str) -> (bool, String) {
    let plural = |n: usize| {
        if n == 1 {
            noun.to_string()
        } else {
            format!("{noun}s")
        }
    };
    if deleted == asked {
        (true, format!("Deleted {deleted} {}.", plural(deleted)))
    } else if deleted == 0 {
        (
            false,
            format!("Couldn\u{2019}t delete the {}.", plural(asked)),
        )
    } else {
        (
            false,
            format!(
                "Deleted {deleted} of {asked} {}; {} couldn\u{2019}t be deleted.",
                plural(asked),
                asked - deleted
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_load_says_what_and_that_nothing_was_lost() {
        let msg = load_error_message("your notes");
        assert!(msg.starts_with("Couldn\u{2019}t load your notes."));
        assert!(msg.contains("Nothing was changed"));
    }

    #[test]
    fn a_bulk_delete_is_a_success_only_when_all_went() {
        assert_eq!(
            bulk_delete_outcome(3, 3, "node"),
            (true, "Deleted 3 nodes.".to_string())
        );
        assert_eq!(
            bulk_delete_outcome(1, 1, "node"),
            (true, "Deleted 1 node.".to_string())
        );
        assert_eq!(
            bulk_delete_outcome(0, 2, "node"),
            (false, "Couldn\u{2019}t delete the nodes.".to_string())
        );
        assert_eq!(
            bulk_delete_outcome(2, 5, "node"),
            (
                false,
                "Deleted 2 of 5 nodes; 3 couldn\u{2019}t be deleted.".to_string()
            )
        );
    }
}
