//! Suspended accounts (#239): the API refuses their sessions at once.
//!
//! An ID token is checked locally, so one issued before a suspension stays
//! valid until it expires (about an hour). Cognito's disable and global
//! sign-out stop new sign-ins and renewals; this set stops the token that
//! is already out. It mirrors `suspended_accounts`, is loaded at startup,
//! is updated by the admin routes and is re-read on a timer.

use std::{collections::HashSet, sync::Arc};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use common::auth::AuthClaims;
use tokio::sync::RwLock;

use crate::{error::ApiError, state::AppState};

/// How often the set is re-read from the table. It bounds how long another
/// API instance, or a change made outside the admin routes, takes to apply.
pub const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Default)]
struct Inner {
    subjects: HashSet<String>,
    /// Bumped by every change from the admin routes. A refresh whose read
    /// raced one is dropped (see `refresh`).
    generation: u64,
}

/// The suspended subjects, shared by every request.
#[derive(Clone, Default)]
pub struct SuspendedSet(Arc<RwLock<Inner>>);

impl SuspendedSet {
    #[must_use]
    pub fn new(subjects: HashSet<String>) -> Self {
        Self(Arc::new(RwLock::new(Inner {
            subjects,
            generation: 0,
        })))
    }

    pub async fn contains(&self, subject_id: &str) -> bool {
        self.0.read().await.subjects.contains(subject_id)
    }

    /// Call after the row is written, so a concurrent refresh can't undo it.
    pub async fn insert(&self, subject_id: &str) {
        let mut inner = self.0.write().await;
        inner.subjects.insert(subject_id.to_string());
        inner.generation += 1;
    }

    /// Call after the row is deleted.
    pub async fn remove(&self, subject_id: &str) {
        let mut inner = self.0.write().await;
        inner.subjects.remove(subject_id);
        inner.generation += 1;
    }

    /// Replace the set with a fresh read of the table, unless a change
    /// landed while it was being read.
    ///
    /// The routes write the row first and the set second. A read that
    /// started before a row change may miss it; if that change has already
    /// reached the set, `generation` moved and the read is dropped (the next
    /// tick catches up). If it hasn't, the change reaches the set after
    /// this replace. Either way the set is never left without a suspension
    /// that is in the table.
    pub async fn refresh<F, Fut, E>(&self, read: F) -> Result<bool, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<HashSet<String>, E>>,
    {
        let before = self.0.read().await.generation;
        let subjects = read().await?;
        let mut inner = self.0.write().await;
        if inner.generation != before {
            return Ok(false);
        }
        inner.subjects = subjects;
        Ok(true)
    }
}

/// Refuse a suspended subject's request with the same 401 as a bad token,
/// so nothing tells the client why. Runs inside `require_auth`, which has
/// put the claims in place (the e2e bypass included).
pub async fn reject_suspended(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if let Some(claims) = request.extensions().get::<AuthClaims>()
        && state.suspended.contains(&claims.sub).await
    {
        return Err(ApiError::Unauthorized("invalid token".to_string()));
    }
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_of(subjects: &[&str]) -> HashSet<String> {
        subjects.iter().map(|s| (*s).to_string()).collect()
    }

    #[tokio::test]
    async fn refresh_replaces_the_set_when_nothing_changed() {
        let set = SuspendedSet::new(set_of(&["a"]));
        let applied = set
            .refresh(|| async { Ok::<_, ()>(set_of(&["b"])) })
            .await
            .unwrap();
        assert!(applied);
        assert!(!set.contains("a").await);
        assert!(set.contains("b").await);
    }

    /// A suspension that reaches the set while the table is being read
    /// must survive the refresh, even though the read missed it.
    #[tokio::test]
    async fn a_suspension_during_the_read_survives_the_refresh() {
        let set = SuspendedSet::default();
        let racing = set.clone();
        let applied = set
            .refresh(|| async move {
                racing.insert("new").await;
                Ok::<_, ()>(HashSet::new())
            })
            .await
            .unwrap();
        assert!(!applied);
        assert!(set.contains("new").await);
    }

    #[tokio::test]
    async fn a_reactivation_during_the_read_survives_the_refresh() {
        let set = SuspendedSet::new(set_of(&["old"]));
        let racing = set.clone();
        set.refresh(|| async move {
            racing.remove("old").await;
            Ok::<_, ()>(set_of(&["old"]))
        })
        .await
        .unwrap();
        assert!(!set.contains("old").await);
    }

    #[tokio::test]
    async fn a_failed_read_keeps_the_set() {
        let set = SuspendedSet::new(set_of(&["a"]));
        let result = set
            .refresh(|| async { Err::<HashSet<String>, _>("db down") })
            .await;
        assert!(result.is_err());
        assert!(set.contains("a").await);
    }
}
