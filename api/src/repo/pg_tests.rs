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
    task::{CreateTaskRequest, TaskStatus, UpdateTaskRequest},
};
use sqlx::PgPool;

use super::{
    edge::{EdgeRepo, PgEdgeRepo},
    editor_pref::{EditorPrefRepo, PgEditorPrefRepo},
    graph::{GraphRepo, PgGraphRepo},
    node::{NodeRepo, PgNodeRepo},
    note::{NoteRepo, PgNoteRepo},
    permission::{PermissionRepo, PgPermissionRepo},
    search::{PgSearchRepo, SearchRepo},
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

// ── Cross-owner isolation (hotfix: wiki links, edges, permissions, graph) ────

#[sqlx::test(migrations = "../migrations")]
async fn wikilink_titles_resolve_only_to_nodes_the_owner_can_see(pool: PgPool) {
    let nodes = PgNodeRepo::new(pool.clone());
    let mine = create_node(&pool, "owner-a", "Plan").await;
    let theirs = create_node(&pool, "owner-b", "Journal").await;
    // Same title under another owner must never win either.
    create_node(&pool, "owner-b", "Plan").await;

    let shared = create_node(&pool, "owner-b", "Shared Spec").await;
    PgPermissionRepo::new(pool.clone())
        .grant(
            shared.id,
            "owner-b",
            common::permission::GrantPermissionRequest {
                subject_id: "owner-a".to_string(),
                role: common::permission::PermissionRole::Viewer,
            },
        )
        .await
        .expect("share");

    let titles = ["plan", "journal", "shared spec"].map(String::from);
    let found = nodes
        .find_ids_by_titles("owner-a", &titles)
        .await
        .expect("resolve");
    // Own node wins over another owner's same-titled node.
    assert_eq!(found.get("plan"), Some(&mine.id));
    // Regression: `[[Journal]]` used to resolve to another owner's PRIVATE node.
    assert!(!found.contains_key("journal"), "resolved {:?}", theirs.id);
    // No regression: links into nodes deliberately shared with A still resolve.
    assert_eq!(found.get("shared spec"), Some(&shared.id));
}

#[sqlx::test(migrations = "../migrations")]
async fn neighbors_backlinks_and_edges_hide_nodes_the_caller_cannot_see(pool: PgPool) {
    let nodes = PgNodeRepo::new(pool.clone());
    let edges = PgEdgeRepo::new(pool.clone());
    let a = create_node(&pool, "owner-a", "A source").await;
    let a2 = create_node(&pool, "owner-a", "A own target").await;
    let b = create_node(&pool, "owner-b", "B private").await;
    // Simulate a pre-hotfix cross-owner link plus a legitimate own link.
    edges
        .sync_wikilinks(a.id, &[a2.id, b.id])
        .await
        .expect("edges");
    edges
        .sync_wikilinks(b.id, &[a.id])
        .await
        .expect("back edge");

    let ids = |v: Vec<common::node::Node>| v.into_iter().map(|n| n.id).collect::<Vec<_>>();
    let n = ids(nodes
        .neighbors(a.id, Some("owner-a"))
        .await
        .expect("neighbors"));
    assert_eq!(n, vec![a2.id], "B's node (and body) must not leak to A");
    let bl = ids(nodes
        .backlinks(a.id, Some("owner-a"))
        .await
        .expect("backlinks"));
    assert!(bl.is_empty(), "B's linking node must not leak to A");
    let e = edges
        .list_for_node_with_titles(a.id, Some("owner-a"))
        .await
        .expect("edges with titles");
    assert!(e.iter().all(|e| e.source_id != b.id && e.target_id != b.id));
    assert_eq!(e.len(), 1);

    // Sharing B with owner-a makes it visible; admins (None) see everything.
    PgPermissionRepo::new(pool.clone())
        .grant(
            b.id,
            "owner-b",
            common::permission::GrantPermissionRequest {
                subject_id: "owner-a".to_string(),
                role: common::permission::PermissionRole::Viewer,
            },
        )
        .await
        .expect("share");
    assert_eq!(
        nodes
            .neighbors(a.id, Some("owner-a"))
            .await
            .expect("n")
            .len(),
        2
    );
    assert_eq!(nodes.neighbors(a.id, None).await.expect("admin").len(), 2);
}

#[sqlx::test(migrations = "../migrations")]
async fn migration_033_drops_only_wikilinks_into_unshared_nodes(pool: PgPool) {
    let edges = PgEdgeRepo::new(pool.clone());
    let a = create_node(&pool, "owner-a", "A").await;
    let a2 = create_node(&pool, "owner-a", "A2").await;
    let b = create_node(&pool, "owner-b", "B").await;
    let shared = create_node(&pool, "owner-b", "B shared").await;
    PgPermissionRepo::new(pool.clone())
        .grant(
            shared.id,
            "owner-b",
            common::permission::GrantPermissionRequest {
                subject_id: "owner-a".to_string(),
                role: common::permission::PermissionRole::Editor,
            },
        )
        .await
        .expect("share");
    edges
        .sync_wikilinks(a.id, &[a2.id, b.id, shared.id])
        .await
        .expect("wiki edges");
    // An explicit (user-created) cross-owner edge is left to the read filter.
    edges
        .create(common::edge::CreateEdgeRequest {
            source_id: a.id,
            target_id: b.id,
            edge_type: common::edge::EdgeType::References,
            label: None,
        })
        .await
        .expect("explicit edge");

    sqlx::raw_sql(include_str!(
        "../../../migrations/033_drop_cross_owner_wikilinks.sql"
    ))
    .execute(&pool)
    .await
    .expect("re-run cleanup");

    let mut left: Vec<(uuid::Uuid, String)> = sqlx::query_as(
        "SELECT target_id, edge_type::text FROM edges WHERE source_id = $1 ORDER BY edge_type",
    )
    .bind(a.id.0)
    .fetch_all(&pool)
    .await
    .expect("edges");
    // Kept: the explicit edge, the own-node link, and the link into a node
    // shared with A (user data). Dropped: only the link into B's private node.
    let mut expected = vec![
        (b.id.0, "references".to_string()),
        (a2.id.0, "wiki_link".to_string()),
        (shared.id.0, "wiki_link".to_string()),
    ];
    left.sort();
    expected.sort();
    assert_eq!(left, expected);

    // Reversible: exactly the removed row is archived, and the documented
    // undo statement restores it.
    let archived: Vec<(uuid::Uuid,)> =
        sqlx::query_as("SELECT target_id FROM edges_removed_033 WHERE source_id = $1")
            .bind(a.id.0)
            .fetch_all(&pool)
            .await
            .expect("archive");
    assert_eq!(archived, vec![(b.id.0,)]);
    sqlx::query(
        "INSERT INTO edges (id, source_id, target_id, edge_type, label, created_at)
         SELECT id, source_id, target_id, edge_type, label, created_at
         FROM edges_removed_033 ON CONFLICT (id) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("undo");
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM edges WHERE source_id = $1 AND edge_type = 'wiki_link'",
    )
    .bind(a.id.0)
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(n, 3, "undo restores the archived wiki link");
}

#[sqlx::test(migrations = "../migrations")]
async fn permission_revoke_is_scoped_to_its_node(pool: PgPool) {
    let perms = PgPermissionRepo::new(pool.clone());
    let x = create_node(&pool, "attacker", "X").await;
    let y = create_node(&pool, "victim", "Y").await;
    let victim_perm = perms
        .grant(
            y.id,
            "victim",
            common::permission::GrantPermissionRequest {
                subject_id: "friend".to_string(),
                role: common::permission::PermissionRole::Editor,
            },
        )
        .await
        .expect("grant on Y");

    // Regression (IDOR): revoking Y's permission through node X's route.
    assert!(matches!(
        perms.revoke(victim_perm.id, x.id).await,
        Err(EmberTroveError::NotFound(_))
    ));
    assert_eq!(perms.list(y.id).await.expect("list").len(), 1);
    perms
        .revoke(victim_perm.id, y.id)
        .await
        .expect("revoke on Y");
    assert!(perms.list(y.id).await.expect("list").is_empty());
}

#[sqlx::test(migrations = "../migrations")]
async fn graph_position_writes_stay_owner_scoped(pool: PgPool) {
    let graph = PgGraphRepo::new(pool.clone());
    let b = create_node(&pool, "owner-b", "B node").await;
    // A viewer/stranger writing B's position is dropped; the owner's lands.
    graph
        .save_positions(&[(b.id.0, 1.0, 2.0)], Some("owner-a"))
        .await
        .expect("foreign write");
    assert!(graph.list_positions(None).await.expect("list").is_empty());
    graph
        .save_positions(&[(b.id.0, 3.0, 4.0)], Some("owner-b"))
        .await
        .expect("owner write");
    assert_eq!(graph.list_positions(None).await.expect("list").len(), 1);
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

// ── Search result titles (#107) ──────────────────────────────────────────────

async fn search_one(pool: &PgPool, owner: &str, q: &str) -> Vec<common::search::SearchResult> {
    let query = common::search::SearchQuery {
        q: q.to_string(),
        fuzzy: None,
        node_type: None,
        status: None,
        tag_ids: None,
        tag_op: None,
        page: None,
        per_page: None,
        sort: None,
        updated_after: None,
        updated_before: None,
    };
    PgSearchRepo::new(pool.clone())
        .search(&query, Some(owner))
        .await
        .expect("search")
        .results
}

fn search_query(q: &str, fuzzy: bool) -> common::search::SearchQuery {
    common::search::SearchQuery {
        q: q.to_string(),
        fuzzy: Some(fuzzy),
        node_type: None,
        status: None,
        tag_ids: None,
        tag_op: None,
        page: None,
        per_page: None,
        sort: None,
        updated_after: None,
        updated_before: None,
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn search_ignores_soft_deleted_notes_and_tasks(pool: PgPool) {
    let search = PgSearchRepo::new(pool.clone());
    let notes = PgNoteRepo::new(pool.clone());
    let tasks = PgTaskRepo::new(pool.clone());
    let host = create_node(&pool, "owner-a", "Host node").await;
    let note = notes
        .create(
            Some(host.id),
            "owner-a",
            common::note::CreateNoteRequest {
                body: "zebrafinch notebook".to_string(),
                color: "default".to_string(),
                node_id: None,
            },
        )
        .await
        .expect("note");
    let task = tasks
        .create(
            Some(host.id),
            "owner-a",
            CreateTaskRequest {
                node_id: Some(host.id),
                ..task_req("quokkaparade errand")
            },
        )
        .await
        .expect("task");

    for fuzzy in [false, true] {
        for q in ["zebrafinch", "quokkaparade"] {
            let hits = search
                .search(&search_query(q, fuzzy), Some("owner-a"))
                .await
                .expect("search");
            assert_eq!(hits.results.len(), 1, "{q} fuzzy={fuzzy} before delete");
        }
    }
    notes.delete(note.id, "owner-a").await.expect("trash note");
    tasks.delete(task.id).await.expect("trash task");
    // Regression: trashed notes/tasks still matched (and showed snippets of
    // the deleted body) for the 30-day tombstone window.
    for fuzzy in [false, true] {
        for q in ["zebrafinch", "quokkaparade"] {
            let hits = search
                .search(&search_query(q, fuzzy), Some("owner-a"))
                .await
                .expect("search");
            assert!(hits.results.is_empty(), "{q} fuzzy={fuzzy} after delete");
            assert_eq!(hits.total, 0);
        }
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn fuzzy_search_treats_like_wildcards_literally(pool: PgPool) {
    let search = PgSearchRepo::new(pool.clone());
    create_node(&pool, "owner-a", "Plain one").await;
    create_node(&pool, "owner-a", "Plain two").await;
    // Regression: `%` used to become ILIKE '%%%' and match every body.
    let hits = search
        .search(&search_query("%", true), Some("owner-a"))
        .await
        .expect("search");
    assert!(
        hits.results.is_empty(),
        "{:?}",
        hits.results.iter().map(|r| &r.title).collect::<Vec<_>>()
    );
    // An absurd page number is an empty page, not an overflow.
    let mut q = search_query("plain", true);
    q.page = Some(u32::MAX);
    let hits = search.search(&q, Some("owner-a")).await.expect("huge page");
    assert!(hits.results.is_empty());
}

#[sqlx::test(migrations = "../migrations")]
async fn search_title_highlight_is_the_full_title(pool: PgPool) {
    let nodes = PgNodeRepo::new(pool.clone());
    for (title, body) in [
        ("Buy a gift for Sam", "wrapping paper and a card"),
        ("Master Bath Remodel", "pick the tile and the grout colour"),
    ] {
        nodes
            .create(
                "owner-a",
                CreateNodeRequest {
                    body: Some(body.to_string()),
                    ..node_req(title)
                },
            )
            .await
            .expect("create node");
    }

    // Title match: the full title comes back, with the match marked.
    let hits = search_one(&pool, "owner-a", "Sam").await;
    let r = hits
        .iter()
        .find(|r| r.title == "Buy a gift for Sam")
        .expect("title hit");
    let hl = r.highlighted_title.as_deref().expect("highlighted title");
    assert!(
        common::search::title_highlight_is_full(hl, &r.title),
        "{hl}"
    );
    assert!(common::search::has_mark(hl));
    assert!(r.updated_at.is_some());

    // Body-only match: still the full title (previously "Master"), unmarked.
    let hits = search_one(&pool, "owner-a", "tile").await;
    let r = hits
        .iter()
        .find(|r| r.title == "Master Bath Remodel")
        .expect("body hit");
    let hl = r.highlighted_title.as_deref().expect("highlighted title");
    assert!(
        common::search::title_highlight_is_full(hl, &r.title),
        "{hl}"
    );
    assert!(!common::search::has_mark(hl));

    // Owner-scoped as before.
    assert!(search_one(&pool, "owner-b", "Sam").await.is_empty());
}

// ── Recurring-task occurrence link ───────────────────────────────────────────

fn status_req(status: TaskStatus) -> UpdateTaskRequest {
    UpdateTaskRequest {
        status: Some(status),
        ..Default::default()
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn retract_occurrence_only_takes_the_owners_open_occurrence(pool: PgPool) {
    let tasks = PgTaskRepo::new(pool.clone());
    let parent = tasks
        .create(None, "owner-a", task_req("weekly review"))
        .await
        .expect("create parent");
    let next = tasks
        .create_occurrence(parent.id, "owner-a", task_req("weekly review"))
        .await
        .expect("create occurrence");
    // An unrelated task without a link is never touched.
    let other = tasks
        .create(None, "owner-a", task_req("unrelated"))
        .await
        .expect("create other");

    // Owner-scoped: a mismatched owner retracts nothing.
    assert_eq!(
        tasks
            .retract_occurrence(parent.id, "owner-b")
            .await
            .expect("retract (wrong owner)"),
        0
    );

    assert_eq!(
        tasks
            .retract_occurrence(parent.id, "owner-a")
            .await
            .expect("retract"),
        1
    );
    assert!(matches!(
        tasks.get(next.id).await,
        Err(EmberTroveError::NotFound(_))
    ));
    // Soft delete, so it's restorable; and idempotent.
    tasks.get_deleted(next.id).await.expect("tombstoned");
    assert_eq!(
        tasks
            .retract_occurrence(parent.id, "owner-a")
            .await
            .expect("retract again"),
        0
    );
    tasks.get(other.id).await.expect("unrelated task untouched");
}

#[sqlx::test(migrations = "../migrations")]
async fn retract_occurrence_keeps_a_started_occurrence(pool: PgPool) {
    let tasks = PgTaskRepo::new(pool.clone());
    let parent = tasks
        .create(None, "owner-a", task_req("daily standup"))
        .await
        .expect("create parent");
    let next = tasks
        .create_occurrence(parent.id, "owner-a", task_req("daily standup"))
        .await
        .expect("create occurrence");
    tasks
        .update(next.id, status_req(TaskStatus::InProgress))
        .await
        .expect("start occurrence");

    assert_eq!(
        tasks
            .retract_occurrence(parent.id, "owner-a")
            .await
            .expect("retract"),
        0,
        "an occurrence the user already started is their work — keep it"
    );
    tasks.get(next.id).await.expect("still live");
}

// ── My Day membership ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../migrations")]
async fn my_day_keeps_done_tasks_only_when_focused_today(pool: PgPool) {
    let tasks = PgTaskRepo::new(pool.clone());
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 12).expect("date");
    let earlier = today - Duration::days(2);
    let mk = |title: &str, focus| CreateTaskRequest {
        focus_date: Some(focus),
        ..task_req(title)
    };
    let done_today = tasks
        .create(None, "owner-a", mk("done today", today))
        .await
        .expect("create");
    let carried_open = tasks
        .create(None, "owner-a", mk("carried open", earlier))
        .await
        .expect("create");
    let carried_done = tasks
        .create(None, "owner-a", mk("carried done", earlier))
        .await
        .expect("create");
    for t in [&done_today, &carried_done] {
        tasks
            .update(t.id, status_req(TaskStatus::Done))
            .await
            .expect("complete");
    }

    let ids: Vec<_> = tasks
        .list_my_day("owner-a", today)
        .await
        .expect("my day")
        .into_iter()
        .map(|t| t.task.id)
        .collect();
    assert!(
        ids.contains(&done_today.id),
        "done today stays (counts as done)"
    );
    assert!(ids.contains(&carried_open.id), "open carry-over is sticky");
    // The query contract the UI's "done today" stamp exists for: a task
    // completed while still carrying an earlier focus_date drops out.
    assert!(!ids.contains(&carried_done.id));
}

// ── Editor prefs: per-field upsert + note collapse ────────────────────────────

/// (height, collapsed) for one owner's note pref, or None if no row exists.
async fn note_pref(
    repo: &PgEditorPrefRepo,
    owner: &str,
    id: uuid::Uuid,
) -> Option<(Option<i32>, bool)> {
    repo.list_for_owner(owner)
        .await
        .expect("list prefs")
        .into_iter()
        .find(|p| p.entity_kind == "note" && p.entity_id == id)
        .map(|p| (p.height, p.collapsed))
}

#[sqlx::test(migrations = "../migrations")]
async fn editor_pref_set_only_changes_the_fields_sent(pool: PgPool) {
    let repo = PgEditorPrefRepo::new(pool);
    let id = uuid::Uuid::new_v4();

    repo.set("alice", "note", id, Some(300), None)
        .await
        .expect("height");
    assert_eq!(
        note_pref(&repo, "alice", id).await,
        Some((Some(300), false))
    );

    // Collapsing must not reset the saved height…
    repo.set("alice", "note", id, None, Some(true))
        .await
        .expect("collapse");
    assert_eq!(note_pref(&repo, "alice", id).await, Some((Some(300), true)));

    // …and resizing must not reset the collapse state.
    repo.set("alice", "note", id, Some(420), None)
        .await
        .expect("resize");
    assert_eq!(note_pref(&repo, "alice", id).await, Some((Some(420), true)));

    // A collapse-only pref needs no height (column is nullable since 031).
    let fresh = uuid::Uuid::new_v4();
    repo.set("alice", "note", fresh, None, Some(true))
        .await
        .expect("collapse new");
    assert_eq!(note_pref(&repo, "alice", fresh).await, Some((None, true)));
}

#[sqlx::test(migrations = "../migrations")]
async fn editor_pref_bulk_collapse_and_expand(pool: PgPool) {
    let repo = PgEditorPrefRepo::new(pool);
    let (a, b, untouched) = (
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    repo.set("alice", "note", a, Some(250), None)
        .await
        .expect("seed a");
    // Bob's pref for the same note id must be unaffected by Alice's batches.
    repo.set("bob", "note", a, None, Some(true))
        .await
        .expect("seed bob");

    // Collapse all: upserts, keeps a's height, tolerates a duplicate id.
    repo.set_collapsed_many("alice", "note", &[a, b, a], true)
        .await
        .expect("collapse many");
    assert_eq!(note_pref(&repo, "alice", a).await, Some((Some(250), true)));
    assert_eq!(note_pref(&repo, "alice", b).await, Some((None, true)));

    // Expand all: updates existing rows only — no row appears for an id that
    // was never collapsed (absent already means expanded).
    repo.set_collapsed_many("alice", "note", &[a, b, untouched], false)
        .await
        .expect("expand many");
    assert_eq!(note_pref(&repo, "alice", a).await, Some((Some(250), false)));
    assert_eq!(note_pref(&repo, "alice", b).await, Some((None, false)));
    assert_eq!(note_pref(&repo, "alice", untouched).await, None);

    // Owner scoping: Bob's row is untouched by Alice's expand.
    assert_eq!(note_pref(&repo, "bob", a).await, Some((None, true)));
}
