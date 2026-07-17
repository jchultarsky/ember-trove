//! Repo-layer tests against a real PostgreSQL (v2.23.0 "trust the suite").
//!
//! The stub-backed router tests in `crate::tests` prove handler logic; nothing
//! there executes the actual SQL. Each test here runs against its own
//! freshly-created, fully-migrated database via `#[sqlx::test]`.
//!
//! Gated behind the `pg-tests` cargo feature so the default
//! `cargo test --workspace --exclude ui` (and the pre-push hook) needs no
//! database. Run locally with a throwaway Postgres:
//!
//! ```text
//! docker run -d --name pg-repo-tests -e POSTGRES_USER=ember_trove \
//!   -e POSTGRES_PASSWORD=ember_trove_dev -e POSTGRES_DB=ember_trove \
//!   -p 55432:5432 postgres:16-alpine
//! DATABASE_URL=postgres://ember_trove:ember_trove_dev@localhost:55432/ember_trove \
//!   cargo test -p api --features pg-tests
//! ```
//!
//! CI runs these in the `repo tests (postgres)` job.

use chrono::{Duration, Utc};
use common::{
    EmberTroveError,
    node::{CreateNodeRequest, Node, NodeType},
    share_token::CreateShareTokenRequest,
    task::CreateTaskRequest,
};
use sqlx::PgPool;

use super::{
    node::{NodeRepo, PgNodeRepo},
    share_token::{PgShareTokenRepo, ShareTokenRepo},
    task::{PgTaskRepo, TaskRepo},
};

fn node_req(title: &str) -> CreateNodeRequest {
    CreateNodeRequest {
        title: title.to_string(),
        node_type: NodeType::Article,
        body: Some("pg test body".to_string()),
        metadata: serde_json::json!({}),
        status: None,
        template_id: None,
    }
}

fn task_req(title: &str) -> CreateTaskRequest {
    CreateTaskRequest {
        title: title.to_string(),
        node_id: None,
        status: None,
        priority: None,
        focus_date: None,
        due_date: None,
        recurrence: None,
    }
}

async fn create_node(pool: &PgPool, owner: &str, title: &str) -> Node {
    PgNodeRepo::new(pool.clone())
        .create(owner, node_req(title))
        .await
        .expect("node create")
}

// ── Node owner-scoping ────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../migrations")]
async fn node_list_all_for_owner_scopes_to_owner(pool: PgPool) {
    let repo = PgNodeRepo::new(pool.clone());
    let mine = create_node(&pool, "owner-a", "A's node").await;
    let _theirs = create_node(&pool, "owner-b", "B's node").await;

    let scoped = repo
        .list_all_for_owner("owner-a")
        .await
        .expect("scoped list");
    assert_eq!(
        scoped.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![mine.id],
        "owner-scoped list must contain exactly the owner's node"
    );

    let all = repo.list_all().await.expect("admin list");
    assert_eq!(all.len(), 2, "unscoped list must see both owners' nodes");
}

// ── Share tokens: lifecycle, expiry, node-scoped revocation ───────────────────

#[sqlx::test(migrations = "../migrations")]
async fn share_token_expiry_filters_lookup(pool: PgPool) {
    let node = create_node(&pool, "owner-a", "shared").await;
    let tokens = PgShareTokenRepo::new(pool.clone());

    let live = tokens
        .create(
            node.id,
            "owner-a",
            &CreateShareTokenRequest { expires_at: None },
        )
        .await
        .expect("create live token");
    assert!(
        tokens
            .find_by_token(live.token)
            .await
            .expect("lookup")
            .is_some(),
        "unexpired token must resolve"
    );

    let expired = tokens
        .create(
            node.id,
            "owner-a",
            &CreateShareTokenRequest {
                expires_at: Some(Utc::now() - Duration::hours(1)),
            },
        )
        .await
        .expect("create expired token");
    assert!(
        tokens
            .find_by_token(expired.token)
            .await
            .expect("lookup")
            .is_none(),
        "expired token must NOT resolve"
    );
}

#[sqlx::test(migrations = "../migrations")]
async fn share_token_revoke_is_scoped_to_node(pool: PgPool) {
    // SQL-level regression for the v2.22.4 fix: revocation matches BOTH the
    // token id and the node id, so proving ownership of one node cannot
    // revoke another node's token.
    let node_a = create_node(&pool, "owner-a", "node a").await;
    let node_b = create_node(&pool, "owner-b", "node b").await;
    let tokens = PgShareTokenRepo::new(pool.clone());
    let token = tokens
        .create(
            node_a.id,
            "owner-a",
            &CreateShareTokenRequest { expires_at: None },
        )
        .await
        .expect("create token");

    // Wrong node: NotFound, and the token must survive.
    let wrong = tokens.revoke(token.id, node_b.id).await;
    assert!(
        matches!(wrong, Err(EmberTroveError::NotFound(_))),
        "cross-node revoke must be NotFound, got {wrong:?}"
    );
    assert!(
        tokens
            .find_by_token(token.token)
            .await
            .expect("lookup")
            .is_some(),
        "token must still exist after a cross-node revoke attempt"
    );

    // Owning node: revoked for real.
    tokens
        .revoke(token.id, node_a.id)
        .await
        .expect("legit revoke");
    assert!(
        tokens
            .find_by_token(token.token)
            .await
            .expect("lookup")
            .is_none(),
        "token must be gone after a legitimate revoke"
    );
}

// ── Task soft-delete lifecycle ────────────────────────────────────────────────

#[sqlx::test(migrations = "../migrations")]
async fn task_soft_delete_restore_and_purge(pool: PgPool) {
    let tasks = PgTaskRepo::new(pool.clone());
    let task = tasks
        .create(None, "owner-a", task_req("inbox task"))
        .await
        .expect("create task");

    // Visible in the inbox (standalone task), then tombstoned by delete.
    assert_eq!(tasks.list_inbox("owner-a").await.expect("inbox").len(), 1);
    tasks.delete(task.id).await.expect("soft delete");
    assert!(
        tasks.list_inbox("owner-a").await.expect("inbox").is_empty(),
        "soft-deleted task must not appear in the inbox"
    );

    // Deleting an already-deleted task is NotFound (tombstone is idempotent-safe).
    assert!(matches!(
        tasks.delete(task.id).await,
        Err(EmberTroveError::NotFound(_))
    ));

    // Restore brings it back.
    let restored = tasks.restore(task.id).await.expect("restore");
    assert_eq!(restored.id, task.id);
    assert_eq!(tasks.list_inbox("owner-a").await.expect("inbox").len(), 1);

    // Delete again, purge everything tombstoned up to now — the row is gone
    // for good and restore must fail.
    tasks.delete(task.id).await.expect("second soft delete");
    let purged = tasks
        .purge_deleted_before(Utc::now() + Duration::seconds(1))
        .await
        .expect("purge");
    assert_eq!(purged, 1, "exactly one tombstoned task must be purged");
    assert!(matches!(
        tasks.restore(task.id).await,
        Err(EmberTroveError::NotFound(_))
    ));
}
