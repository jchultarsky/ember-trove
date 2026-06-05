#![allow(dead_code)]
use common::auth::UserInfo;
use leptos::prelude::*;

/// Tri-state auth status: still loading, authenticated, or unauthenticated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthStatus {
    Loading,
    Authenticated(UserInfo),
    Unauthenticated,
}

/// Global auth signal.
pub type AuthState = RwSignal<AuthStatus>;

/// Initialise the auth state signal, provide it via context, and kick off
/// the `/api/auth/me` probe.
pub fn provide_auth_state() -> AuthState {
    let auth_state: AuthState = RwSignal::new(AuthStatus::Loading);
    provide_context(auth_state);
    init_auth(auth_state);
    auth_state
}

/// Read the auth state from context.
#[must_use]
pub fn use_auth_state() -> AuthState {
    expect_context::<AuthState>()
}

/// On mount, call GET /api/auth/me to check if we have a valid session cookie.
fn init_auth(auth_state: AuthState) {
    wasm_bindgen_futures::spawn_local(async move {
        match crate::api::fetch_me().await {
            Ok(user_info) => auth_state.set(AuthStatus::Authenticated(user_info)),
            Err(_) => auth_state.set(AuthStatus::Unauthenticated),
        }
    });
}
