//! A free-text field with suggestions: an APG combobox of our own, not an
//! `<input list>` (#171), shared by the membership Role and the contact
//! labels (#183).
//!
//! The browser's datalist could commit a suggestion's spelling ("abc") over
//! what was typed ("ABC") and swallowed select-all. Here the typed text is
//! what's saved; a suggestion is committed only when it's chosen, by click
//! or by ↑/↓ then Enter, and it then shows in the field before any save.
//! Every other key is left to the input.

use leptos::prelude::*;

static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[component]
pub fn FreeTextCombo(
    /// The field's text: what's typed, or a suggestion once chosen.
    value: RwSignal<String>,
    /// Suggestions, best first; filtered by what's typed.
    #[prop(into)]
    choices: Signal<Vec<String>>,
    /// The accessible name of the field.
    #[prop(into)]
    label: String,
    /// The accessible name of the suggestion list.
    #[prop(into)]
    list_label: String,
    #[prop(into)] placeholder: String,
    #[prop(into)] class: String,
    /// The id of helper text announced with the field.
    #[prop(optional, into)]
    describedby: Option<String>,
    #[prop(optional)] input_ref: Option<NodeRef<leptos::html::Input>>,
) -> impl IntoView {
    let input_ref = input_ref.unwrap_or_default();
    let base = format!(
        "combo-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let list_id = StoredValue::new(format!("{base}-list"));
    let opt_id = StoredValue::new(move |i: usize| format!("{base}-opt-{i}"));
    // Shown while typing; closed by a choice, Escape or leaving the field.
    let listing = RwSignal::new(false);
    // Only the arrow keys set this, never typing or hover: Enter commits an
    // option only when the user moved onto it.
    let active = RwSignal::new(None::<usize>);
    let options = Memo::new(move |_| {
        choices.with(|c| common::picker::free_text_suggestions(&value.get(), c, 8))
    });
    let expanded = move || listing.get() && options.with(|o| !o.is_empty());
    let close = move || {
        listing.set(false);
        active.set(None);
    };
    let choose = move |i: usize| {
        let picked = options.with_untracked(|o| {
            common::picker::free_text_commit(&value.get_untracked(), Some(i), o).to_string()
        });
        value.set(picked);
        close();
    };
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "ArrowDown" | "ArrowUp" => {
            let len = options.with_untracked(Vec::len);
            if len > 0 {
                ev.prevent_default();
                listing.set(true);
                active.set(common::picker::step(
                    active.get_untracked(),
                    len,
                    ev.key() == "ArrowDown",
                ));
            }
        }
        // Plain Enter on a chosen option picks it; otherwise it falls through
        // to the form (attach / save) with the typed text.
        "Enter" if !(ev.ctrl_key() || ev.meta_key()) => {
            if listing.get_untracked()
                && let Some(i) = active.get_untracked()
            {
                ev.prevent_default();
                ev.stop_propagation();
                choose(i);
            }
        }
        // Closing the list is this Escape's whole job: stopped, so the
        // form, dialog or page doesn't also act on it.
        "Escape" if listing.get_untracked() && options.with_untracked(|o| !o.is_empty()) => {
            ev.prevent_default();
            ev.stop_propagation();
            close();
        }
        _ => {}
    };
    view! {
        <div>
            <input type="text" node_ref=input_ref class=class
                placeholder=placeholder
                role="combobox" aria-label=label aria-autocomplete="list"
                aria-controls=list_id.get_value()
                aria-describedby=describedby
                aria-expanded=move || if expanded() { "true" } else { "false" }
                aria-activedescendant=move || active.get().map(|i| opt_id.get_value()(i))
                autocomplete="off"
                prop:value=move || value.get()
                on:input=move |ev| {
                    value.set(event_target_value(&ev));
                    active.set(None);
                    listing.set(true);
                }
                on:click=move |_| listing.set(true)
                on:blur=move |_| close()
                on:keydown=on_keydown />
            // In the flow, not floating: an open list never covers the
            // fields below it.
            <div id=list_id.get_value() role="listbox" aria-label=list_label
                class="mt-1 max-h-40 overflow-y-auto"
                class:hidden=move || !expanded()>
                {move || expanded().then(|| options.get().into_iter().enumerate().map(|(i, o)| view! {
                    <div
                        id=opt_id.get_value()(i)
                        role="option"
                        aria-selected=move || if active.get() == Some(i) { "true" } else { "false" }
                        class=move || format!(
                            "px-2 py-1 rounded-lg text-xs cursor-pointer text-stone-700 \
                             dark:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800 {}",
                            if active.get() == Some(i) { "bg-amber-50 dark:bg-stone-800" } else { "" }
                        )
                        // mousedown, not click: it lands before the input's blur.
                        on:mousedown=move |ev: web_sys::MouseEvent| {
                            ev.prevent_default();
                            if common::picker::is_pointer_press(ev.detail()) { choose(i); }
                        }
                        // An accessibility press is a bare click (#174).
                        on:click=move |ev: web_sys::MouseEvent| {
                            if !common::picker::is_pointer_press(ev.detail()) { choose(i); }
                        }
                    >
                        {o}
                    </div>
                }).collect_view())}
            </div>
        </div>
    }
}
