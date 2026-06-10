#![allow(dead_code)]
use common::auth::UserInfo;
use leptos::prelude::*;

use crate::error::UiError;

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

/// Whether an `/auth/me` failure is transient (API briefly unreachable —
/// e.g. the seconds of downtime while a deploy restarts the container)
/// rather than a genuine "not logged in". Only an authoritative 401/403
/// counts as unauthenticated; anything else deserves a retry before the
/// AuthGate bounces the tab to the IdP login form. Before v2.22.1 every
/// deploy forced all open tabs to re-login because a single failed probe
/// was treated as "no session".
fn is_transient(err: &UiError) -> bool {
    !matches!(
        err,
        UiError::Api {
            status: 401 | 403,
            ..
        }
    )
}

/// On mount, call GET /api/auth/me to check if we have a valid session
/// cookie. Transient failures are retried with backoff (~23s total,
/// comfortably covering a deploy restart) before giving up.
fn init_auth(auth_state: AuthState) {
    wasm_bindgen_futures::spawn_local(async move {
        let delays_ms: [u32; 6] = [0, 1_000, 2_000, 4_000, 8_000, 8_000];
        let attempts = delays_ms.len();
        for (i, delay) in delays_ms.into_iter().enumerate() {
            if delay > 0 {
                gloo_timers::future::TimeoutFuture::new(delay).await;
            }
            match crate::api::fetch_me().await {
                Ok(user_info) => {
                    auth_state.set(AuthStatus::Authenticated(user_info));
                    return;
                }
                Err(e) if is_transient(&e) && i + 1 < attempts => continue,
                Err(_) => break,
            }
        }
        auth_state.set(AuthStatus::Unauthenticated);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_authoritative_auth_failures_are_terminal() {
        // 401/403 mean "the server saw the request and rejected the session".
        assert!(!is_transient(&UiError::api(401, "no")));
        assert!(!is_transient(&UiError::api(403, "no")));
        // Everything else can be a restarting API or a flaky network — retry.
        assert!(is_transient(&UiError::api(500, "boom")));
        assert!(is_transient(&UiError::api(502, "bad gateway")));
        assert!(is_transient(&UiError::Network("refused".to_string())));
        assert!(is_transient(&UiError::Parse("html error page".to_string())));
    }
}
