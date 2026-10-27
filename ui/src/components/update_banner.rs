//! "A new version is available — Reload" (#129).
//!
//! A tab can keep running an old bundle after a deploy, and then a shipped
//! fix looks like it "doesn't work" (exactly what #129 recorded). The bundle
//! knows its own version (`crate::version`); this checks the server's at
//! startup, every 10 minutes, and whenever the window regains focus, and
//! offers a reload when they differ. Navigations are network-first in the
//! service worker, so the reload really loads the new release.
//!
//! The reload must not cost an open dialog its input (#170): the person
//! editor keeps a draft and reopens with it, and while such a dialog is
//! open the banner says so, so following its prompt is safe.

use leptos::prelude::*;

const RECHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[component]
pub fn UpdateBanner() -> impl IntoView {
    let server = RwSignal::new(String::new());
    let check = move || {
        wasm_bindgen_futures::spawn_local(async move {
            let v = crate::api::fetch_api_version().await;
            // A failed check ("") must not hide a mismatch already found.
            if !v.is_empty() {
                server.try_set(v);
            }
        });
    };
    check();

    if let Ok(handle) = set_interval_with_handle(check, RECHECK_EVERY) {
        on_cleanup(move || handle.clear());
    }
    // Removed on cleanup: a leaked window listener panics on disposed
    // signals (leptos.md).
    let focus = window_event_listener(leptos::ev::focus, move |_| check());
    on_cleanup(move || focus.remove());

    let bundle = crate::version::bundle();
    let available = move || common::version::update_available(bundle, &server.get());
    let open_forms = use_context::<crate::draft::OpenForms>();
    let form_open = move || open_forms.is_some_and(|f| f.0.get() > 0);

    view! {
        {move || available().then(|| view! {
            <div
                data-testid="update-banner"
                role="status"
                aria-live="polite"
                // Floating, so it never shifts the layout.
                class="fixed bottom-4 left-1/2 -translate-x-1/2 z-50 max-w-[calc(100vw-2rem)]
                    flex items-center gap-3 px-4 py-2 text-sm rounded-xl shadow-lg
                    bg-amber-50 dark:bg-stone-900 text-amber-900 dark:text-amber-200
                    border border-amber-200 dark:border-amber-800"
            >
                // `info`: already in the subsetted icon font (refresh-icon-font.sh).
                <span class="material-symbols-outlined" style="font-size: 18px;" aria-hidden="true">
                    "info"
                </span>
                <span>
                    {move || format!("A new version of Ember Trove (v{}) is available.", server.get())}
                    {move || form_open().then_some(" The open form is kept and comes back after the reload.")}
                </span>
                <button
                    type="button"
                    class="inline-flex items-center justify-center gap-1.5 px-3 py-1 rounded-lg bg-ember hover:bg-ember-strong text-white text-xs font-medium"
                    on:click=move |_| {
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().reload();
                        }
                    }
                >
                    <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"refresh"</span>
                    "Reload"
                </button>
            </div>
        })}
    }
}
