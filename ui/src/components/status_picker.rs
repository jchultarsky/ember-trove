//! Lifecycle-state picker for the node editor (#196).
//!
//! Replaces a bare `<select>` whose three options said nothing about what
//! they do. All three states stay visible as a radio group, and the chosen
//! one's meaning (`NodeStatus::description`) sits right under it, tied to
//! the group with `aria-describedby`. Each option also carries its own
//! description as a tooltip, so the others can be read before choosing.
//! Keyboard: WAI-ARIA APG radio group, as `PriorityPicker` — one tab stop
//! on the checked option, arrow keys move the selection.

use common::node::NodeStatus;
use leptos::prelude::*;

/// An unknown or empty wire value reads as Draft, the server default, so
/// there is always exactly one checked option and one tab stop.
fn parse_status(s: &str) -> NodeStatus {
    NodeStatus::parse(s).unwrap_or(NodeStatus::Draft)
}

#[component]
pub fn StatusPicker(
    /// The status's wire string (`"draft"` / `"published"` / `"archived"`).
    value: RwSignal<String>,
) -> impl IntoView {
    let refs = StoredValue::new(
        NodeStatus::ALL
            .iter()
            .map(|_| NodeRef::<leptos::html::Button>::new())
            .collect::<Vec<_>>(),
    );
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let down = match ev.key().as_str() {
            "ArrowRight" | "ArrowDown" => true,
            "ArrowLeft" | "ArrowUp" => false,
            _ => return,
        };
        ev.prevent_default();
        ev.stop_propagation();
        let current = parse_status(&value.get_untracked());
        let active = NodeStatus::ALL.iter().position(|s| *s == current);
        let Some(i) = common::picker::step(active, NodeStatus::ALL.len(), down) else {
            return;
        };
        if let Some(s) = NodeStatus::ALL.get(i) {
            value.set(s.as_str().to_string());
        }
        // Focus follows the selection (roving tab stop).
        if let Some(btn) = refs.with_value(|r| r.get(i).and_then(|n| n.get_untracked())) {
            let _ = btn.focus();
        }
    };
    view! {
        <div class="flex flex-col gap-0.5 min-w-0">
            <div
                role="radiogroup"
                aria-label="Lifecycle state"
                aria-describedby="status-picker-hint"
                class="inline-flex self-start items-center rounded-lg border border-stone-200 dark:border-stone-700 overflow-hidden"
                data-testid="status-picker"
                on:keydown=on_keydown
            >
                {NodeStatus::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let v = s.as_str();
                        let checked = move || parse_status(&value.get()).as_str() == v;
                        let node_ref = refs.with_value(|r| r.get(i).copied()).unwrap_or_default();
                        view! {
                            <button
                                type="button"
                                role="radio"
                                node_ref=node_ref
                                title=s.description()
                                aria-checked=move || if checked() { "true" } else { "false" }
                                tabindex=move || if checked() { "0" } else { "-1" }
                                class=move || {
                                    let base = "text-sm px-2.5 py-1 transition-colors cursor-pointer \
                                                focus:outline-none focus-visible:ring-1 focus-visible:ring-amber-500";
                                    if checked() {
                                        format!("{base} bg-stone-200 dark:bg-stone-700 \
                                                 text-stone-900 dark:text-stone-100 font-semibold")
                                    } else {
                                        format!("{base} text-stone-500 dark:text-stone-400 \
                                                 hover:bg-stone-100 dark:hover:bg-stone-800")
                                    }
                                }
                                on:click=move |_| value.set(v.to_string())
                            >
                                {s.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            <span
                id="status-picker-hint"
                class="text-xs text-stone-500 dark:text-stone-400"
                data-testid="status-picker-hint"
            >
                {move || parse_status(&value.get()).description()}
            </span>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::parse_status;
    use common::node::NodeStatus;

    #[test]
    fn unknown_values_read_as_draft() {
        assert_eq!(parse_status(""), NodeStatus::Draft);
        assert_eq!(parse_status("bogus"), NodeStatus::Draft);
        assert_eq!(parse_status("archived"), NodeStatus::Archived);
    }
}
