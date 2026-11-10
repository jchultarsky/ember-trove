//! Confirmation dialog for destructive deletes.
//!
//! Deleting a node also deletes its tasks, notes and files, other
//! accounts' included. The dialog counts them first and keeps Delete off
//! until it has (owner decision, 2026-11-09, #281): pass `details` from
//! [`node_delete_details`].
use super::ModalShell;
use common::{id::NodeId, node::NodeDeletionPreview};
use leptos::html;
use leptos::prelude::*;

/// What a delete takes with it, as the dialog shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteDetails {
    /// Still counting. Delete stays off.
    Counting,
    /// Counted: `losses` in words (`NodeDeletionPreview::losses`), and how
    /// many of the chosen nodes are not the caller's to delete.
    Counted { losses: Vec<String>, kept: u64 },
    /// The count failed. Delete stays off: the numbers come first.
    Failed,
}

impl DeleteDetails {
    pub fn from_preview(preview: &NodeDeletionPreview, asked: usize) -> Self {
        Self::Counted {
            losses: preview.losses(),
            kept: preview.kept(asked),
        }
    }

    /// Delete is offered only once the counts are on screen.
    pub fn allows_delete(&self) -> bool {
        matches!(self, Self::Counted { .. })
    }

    /// The sentence about nodes that will stay, when there are any.
    pub fn kept_line(&self) -> Option<String> {
        match self {
            Self::Counted { kept: 0, .. } | Self::Counting | Self::Failed => None,
            Self::Counted { kept: 1, .. } => {
                Some("1 of the selected nodes is not yours to delete and will be kept.".to_string())
            }
            Self::Counted { kept, .. } => Some(format!(
                "{kept} of the selected nodes are not yours to delete and will be kept."
            )),
        }
    }
}

/// The counts for deleting `ids`, loaded each time the dialog opens.
/// Closing it clears them, so a reopened dialog counts again and never
/// shows the last selection's numbers.
pub fn node_delete_details(show: Signal<bool>, ids: Signal<Vec<NodeId>>) -> Signal<DeleteDetails> {
    let preview = LocalResource::new(move || {
        let open = show.get();
        let ids = if open {
            ids.get_untracked()
        } else {
            Vec::new()
        };
        async move {
            if open {
                Some(
                    crate::api::node_deletion_preview(&ids)
                        .await
                        .map(|p| DeleteDetails::from_preview(&p, ids.len())),
                )
            } else {
                None
            }
        }
    });
    Signal::derive(move || match preview.get().flatten() {
        None => DeleteDetails::Counting,
        Some(Ok(details)) => details,
        Some(Err(_)) => DeleteDetails::Failed,
    })
}

#[component]
pub fn DeleteConfirmModal(
    #[prop(into)] show: Signal<bool>,
    #[prop(into)] item_name: Signal<String>,
    on_confirm: Callback<()>,
    on_cancel: Callback<()>,
    /// What the delete takes with it. When given, Delete is off until the
    /// counts have loaded.
    #[prop(optional)]
    details: Option<Signal<DeleteDetails>>,
) -> impl IntoView {
    let cancel_ref: NodeRef<html::Button> = NodeRef::new();
    let blocked = move || details.is_some_and(|d| !d.get().allows_delete());

    // Autofocus the safe action (Cancel) when the dialog opens. Tracking the
    // NodeRef re-runs this once the button mounts (no rAF; leptos.md).
    Effect::new(move |_| {
        if show.get()
            && let Some(el) = cancel_ref.get()
        {
            let _ = el.focus();
        }
    });

    view! {
        <ModalShell
            show=show
            on_close=on_cancel
            label="Confirm delete"
            panel_class="w-full max-w-sm p-6 flex flex-col gap-4"
            alert=true
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

            // What goes with it (#281)
            {move || details.map(|d| {
                let d = d.get();
                let kept = d.kept_line();
                match d {
                    DeleteDetails::Counting => view! {
                        <p class="text-sm text-stone-500 dark:text-stone-400"
                           role="status" data-testid="delete-details">
                            "Counting what this deletes\u{2026}"
                        </p>
                    }.into_any(),
                    DeleteDetails::Failed => view! {
                        <p class="text-sm text-red-600 dark:text-red-400"
                           role="alert" data-testid="delete-details">
                            "Couldn\u{2019}t count what this deletes. Close this and try again."
                        </p>
                    }.into_any(),
                    DeleteDetails::Counted { losses, .. } => view! {
                        <div class="text-sm text-stone-600 dark:text-stone-400 flex flex-col gap-1"
                             data-testid="delete-details">
                            {(!losses.is_empty()).then(|| view! {
                                <p>"This also deletes:"</p>
                                <ul class="list-disc pl-5">
                                    {losses.into_iter()
                                        .map(|line| view! { <li>{line}</li> })
                                        .collect_view()}
                                </ul>
                            })}
                            {kept.map(|line| view! { <p>{line}</p> })}
                        </div>
                    }.into_any(),
                }
            })}

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
                    class="inline-flex items-center justify-center gap-1.5 px-4 py-2 text-sm font-medium rounded-lg
                           bg-red-600 hover:bg-red-700
                           text-white transition-colors
                           disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-red-600"
                    disabled=blocked
                    on:click=move |_| {
                        if !blocked() {
                            on_confirm.run(());
                        }
                    }
                >
                    <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"delete"</span>
                    "Delete"
                </button>
            </div>
        </ModalShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_is_offered_only_once_the_counts_are_in() {
        assert!(!DeleteDetails::Counting.allows_delete());
        assert!(!DeleteDetails::Failed.allows_delete());
        let nothing = DeleteDetails::from_preview(&NodeDeletionPreview::default(), 0);
        assert!(nothing.allows_delete());
    }

    #[test]
    fn the_details_carry_the_losses_and_the_nodes_that_stay() {
        let preview = NodeDeletionPreview {
            nodes: 2,
            tasks: 4,
            others_tasks: 1,
            ..Default::default()
        };
        let details = DeleteDetails::from_preview(&preview, 3);
        assert_eq!(
            details,
            DeleteDetails::Counted {
                losses: vec!["4 tasks (1 from other people)".to_string()],
                kept: 1,
            }
        );
        assert_eq!(
            details.kept_line().as_deref(),
            Some("1 of the selected nodes is not yours to delete and will be kept.")
        );
        let all = DeleteDetails::from_preview(&preview, 2);
        assert_eq!(all.kept_line(), None);
        let many = DeleteDetails::from_preview(&preview, 5);
        assert_eq!(
            many.kept_line().as_deref(),
            Some("3 of the selected nodes are not yours to delete and will be kept.")
        );
    }
}
