use common::id::TemplateId;
use common::tag::Tag;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::Router;

use crate::{
    api::fetch_api_version,
    auth::provide_auth_state,
    components::{
        dark_mode_toggle::Theme, layout::Layout, public_share_view::PublicShareView,
        toast::ToastState,
    },
};

// ── localStorage helpers ───────────────────────────────────────────────────

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
}

pub fn storage_get(key: &str) -> Option<String> {
    local_storage().and_then(|s| s.get_item(key).ok()).flatten()
}

pub fn storage_set(key: &str, value: &str) {
    if let Some(s) = local_storage() {
        let _ = s.set_item(key, value);
    }
}

// ── App version ────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct AppVersion(pub RwSignal<String>);

// ── Global task refresh signal ─────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct TaskRefresh(pub RwSignal<u32>);

// ── Favorites refresh signal ────────────────────────────────────────────────

/// Bump `.0` to trigger a re-fetch in `FavoritesSection`.
#[derive(Clone, Copy)]
pub struct FavoritesRefresh(pub RwSignal<u32>);

// ── Quick-capture modal visibility ─────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct ShowCapture(pub RwSignal<bool>);

// ── Template prefill ───────────────────────────────────────────────────────

/// Pre-fill data passed from TemplatesView to NodeEditor when "Use" is clicked.
#[derive(Clone, Debug, PartialEq)]
pub struct TemplatePrefill {
    pub node_type: String,
    pub body: String,
    pub template_id: TemplateId,
}

// ── App root ───────────────────────────────────────────────────────────────

#[component]
pub fn App() -> impl IntoView {
    // Public share links (`/share/<uuid>`) render a standalone read-only view
    // without authentication or the main layout.
    let pathname = web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .unwrap_or_default();
    if let Some(token_str) = pathname.strip_prefix("/share/")
        && let Ok(token) = token_str.parse::<uuid::Uuid>()
    {
        return view! { <PublicShareView token=token /> }.into_any();
    }

    let auth_state = provide_auth_state();

    // Persist theme in localStorage; with no stored preference, follow the
    // OS (`prefers-color-scheme`) — matches the static loading screen, which
    // already honors the media query, so first paint and app agree.
    let initial_theme = storage_get("theme")
        .map(|s| {
            if s == "dark" {
                Theme::Dark
            } else {
                Theme::Light
            }
        })
        .unwrap_or_else(|| {
            let prefers_dark = web_sys::window()
                .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
                .map(|m| m.matches())
                .unwrap_or(false);
            if prefers_dark {
                Theme::Dark
            } else {
                Theme::Light
            }
        });

    let theme = RwSignal::new(initial_theme);
    provide_context(theme);

    // Apply / remove the `dark` class on <html> whenever theme changes
    Effect::new(move |_| {
        let is_dark = theme.get() == Theme::Dark;
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(html) = doc.document_element()
        {
            if is_dark {
                let _ = html.class_list().add_1("dark");
                storage_set("theme", "dark");
            } else {
                let _ = html.class_list().remove_1("dark");
                storage_set("theme", "light");
            }
        }
    });

    // Refresh trigger — bump to re-fetch nodes list.
    let refresh = RwSignal::new(0u32);
    provide_context(refresh);

    // Tag filter — set to Some(tag) to filter NodeList/SearchView by that tag.
    let tag_filter: RwSignal<Option<Tag>> = RwSignal::new(None);
    provide_context(tag_filter);

    // Node-type filter — set by sidebar type links, read by NodeList.
    let node_type_filter: RwSignal<Option<String>> = RwSignal::new(None);
    provide_context(node_type_filter);

    // Shared search query — written by SearchBar, read by SearchView.
    let search_query: RwSignal<String> = RwSignal::new(String::new());
    provide_context(search_query);

    // Global task refresh — shared by TaskPanel and MyDayView.
    let task_refresh = TaskRefresh(RwSignal::new(0u32));
    provide_context(task_refresh);

    // Quick-capture modal — opened via the `n` keyboard shortcut.
    let show_capture = ShowCapture(RwSignal::new(false));
    provide_context(show_capture);

    // Toast notification state.
    let toast_state = ToastState::new();
    provide_context(toast_state);

    // Template prefill — set by TemplatesView "Use", consumed by NodeEditor on create.
    let template_prefill: RwSignal<Option<TemplatePrefill>> = RwSignal::new(None);
    provide_context(template_prefill);

    // API version — fetched once at startup, displayed in the sidebar header.
    let app_version = AppVersion(RwSignal::new(String::new()));
    provide_context(app_version);
    spawn_local(async move {
        let v = fetch_api_version().await;
        app_version.0.set(v);
    });

    // Favorites refresh counter — bump to tell FavoritesSection to re-fetch.
    let favorites_refresh = FavoritesRefresh(RwSignal::new(0u32));
    provide_context(favorites_refresh);

    view! {
        <Router>
            <Layout auth_state=auth_state />
        </Router>
    }
    .into_any()
}
