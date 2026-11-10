//! Accounts whose admin role was taken away: their older tokens stop
//! carrying it at once.
//!
//! An ID token is checked locally and its roles come from the token
//! (`cognito:groups`), so one issued while the account was an admin stays
//! an admin's token until it expires. Removing the role in Cognito only
//! changes the *next* token. This map holds when each account was demoted;
//! `require_auth` drops the admin role from a token issued at or before
//! that time. It mirrors `admin_demotions`, is loaded at startup, is
//! updated by the admin route and is re-read on the suspended list's timer.
//!
//! An entry never needs removing early. A token issued after the demotion
//! has the account's current groups, so if the role is given back, the next
//! token carries it and is newer than the entry. The daily sweep drops rows
//! older than the longest token life.

use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

/// The role this module takes away.
pub const ADMIN_ROLE: &str = "admin";

/// Subject → when its admin role was removed (Unix seconds).
#[derive(Clone, Default)]
pub struct DemotedSet(Arc<RwLock<HashMap<String, i64>>>);

impl DemotedSet {
    #[must_use]
    pub fn new(demotions: HashMap<String, i64>) -> Self {
        Self(Arc::new(RwLock::new(demotions)))
    }

    /// Record a demotion. Call after the row is written. A later time
    /// replaces an earlier one, never the other way round.
    pub async fn insert(&self, subject_id: &str, demoted_at: i64) {
        let mut map = self.0.write().await;
        let at = map.entry(subject_id.to_string()).or_insert(demoted_at);
        *at = (*at).max(demoted_at);
    }

    /// Take in a fresh read of the table. Entries are merged, keeping the
    /// later time, so a demotion recorded here while the table was being
    /// read is never lost; a row the sweep dropped just stays until restart,
    /// which only affects tokens that have long expired.
    pub async fn merge(&self, demotions: HashMap<String, i64>) {
        let mut map = self.0.write().await;
        for (subject, demoted_at) in demotions {
            let at = map.entry(subject).or_insert(demoted_at);
            *at = (*at).max(demoted_at);
        }
    }

    /// The roles a token may use: its own, without `admin` when the token
    /// was issued at or before its subject's demotion.
    pub async fn roles_for(
        &self,
        subject_id: &str,
        issued_at: Option<i64>,
        roles: Vec<String>,
    ) -> Vec<String> {
        let demoted_at = self.0.read().await.get(subject_id).copied();
        roles_after_demotion(roles, demoted_at, issued_at)
    }
}

/// Drop `admin` from `roles` when the token is not newer than the demotion.
///
/// SECURITY: fails closed. A token with no `iat` can't be shown to be newer,
/// so it loses the role; so does one issued in the same second.
#[must_use]
pub fn roles_after_demotion(
    mut roles: Vec<String>,
    demoted_at: Option<i64>,
    issued_at: Option<i64>,
) -> Vec<String> {
    let Some(demoted_at) = demoted_at else {
        return roles;
    };
    if issued_at.is_none_or(|iat| iat <= demoted_at) {
        roles.retain(|r| r != ADMIN_ROLE);
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(names: &[&str]) -> Vec<String> {
        names.iter().map(|r| (*r).to_string()).collect()
    }

    #[test]
    fn an_account_never_demoted_keeps_its_roles() {
        let r = roles(&["admin", "user"]);
        assert_eq!(roles_after_demotion(r.clone(), None, Some(100)), r);
        assert_eq!(roles_after_demotion(r.clone(), None, None), r);
    }

    #[test]
    fn a_token_from_before_the_demotion_loses_admin_only() {
        let r = roles(&["admin", "user"]);
        assert_eq!(
            roles_after_demotion(r.clone(), Some(200), Some(100)),
            roles(&["user"])
        );
        // The same second counts as before; so does a token with no time.
        assert_eq!(
            roles_after_demotion(r.clone(), Some(200), Some(200)),
            roles(&["user"])
        );
        assert_eq!(roles_after_demotion(r, Some(200), None), roles(&["user"]));
    }

    #[test]
    fn a_token_issued_after_the_demotion_is_believed() {
        // Given the role back later: the new token carries it.
        let r = roles(&["admin", "user"]);
        assert_eq!(roles_after_demotion(r.clone(), Some(200), Some(201)), r);
    }

    #[tokio::test]
    async fn the_set_keeps_the_latest_demotion_through_inserts_and_merges() {
        let set = DemotedSet::default();
        let admin = roles(&["admin"]);
        assert_eq!(set.roles_for("a", Some(100), admin.clone()).await, admin);

        set.insert("a", 200).await;
        assert!(
            set.roles_for("a", Some(150), admin.clone())
                .await
                .is_empty()
        );
        assert_eq!(set.roles_for("a", Some(250), admin.clone()).await, admin);
        // Someone else is untouched.
        assert_eq!(set.roles_for("b", Some(150), admin.clone()).await, admin);

        // A stale read of the table can't move the time back.
        set.merge(HashMap::from([
            ("a".to_string(), 120),
            ("b".to_string(), 300),
        ]))
        .await;
        assert!(
            set.roles_for("a", Some(150), admin.clone())
                .await
                .is_empty()
        );
        assert!(
            set.roles_for("b", Some(150), admin.clone())
                .await
                .is_empty()
        );
        set.insert("a", 100).await;
        assert!(set.roles_for("a", Some(150), admin).await.is_empty());
    }
}
