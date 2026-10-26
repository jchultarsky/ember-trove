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
    task::{PgTaskRepo, TaskRepo},
};

fn node_req(title: &str) -> CreateNodeRequest {
    CreateNodeRequest {
        title: title.to_string(),
        node_type: NodeType::Article,
        body: Some("pg test body".to_string()),
        metadata: serde_json::json!({}),
        status: None,
        project_status: None,
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
async fn node_title_lookup_matches_titles_ranked_and_stays_scoped(pool: PgPool) {
    let nodes = PgNodeRepo::new(pool.clone());
    // A body mention is not a title match (the picker bug: full-text search
    // listed every node *mentioning* the word, alphabetically).
    nodes
        .create(
            "owner-a",
            CreateNodeRequest {
                body: Some("Talked with Physna today.".to_string()),
                ..node_req("Acme meeting")
            },
        )
        .await
        .expect("create");
    create_node(&pool, "owner-a", "About Physna").await;
    create_node(&pool, "owner-a", "Physna roadmap").await;
    create_node(&pool, "owner-a", "Physna").await;
    create_node(&pool, "owner-a", "100% done_list").await;
    create_node(&pool, "owner-b", "Physna secret").await;
    let shared = create_node(&pool, "owner-b", "Physna shared").await;
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

    let titles = |hits: Vec<common::node::NodeLookupHit>| -> Vec<String> {
        hits.into_iter().map(|h| h.title).collect()
    };
    // Exact first, then prefix, then any other title match; never a body
    // match, never another owner's unshared node.
    let hits = nodes
        .lookup_titles(Some("owner-a"), "physna", 8)
        .await
        .expect("lookup");
    assert_eq!(
        titles(hits),
        ["Physna", "Physna roadmap", "Physna shared", "About Physna"]
    );
    // Partial words match (full-text search returned nothing for these).
    let hits = nodes
        .lookup_titles(Some("owner-a"), " Phy ", 8)
        .await
        .expect("lookup");
    assert_eq!(
        titles(hits),
        ["Physna", "Physna roadmap", "Physna shared", "About Physna"]
    );
    assert_eq!(
        nodes
            .lookup_titles(Some("owner-a"), "physna", 2)
            .await
            .expect("lookup")
            .len(),
        2
    );
    // LIKE metacharacters are literal.
    assert_eq!(
        titles(
            nodes
                .lookup_titles(Some("owner-a"), "%", 8)
                .await
                .expect("lookup")
        ),
        ["100% done_list"]
    );
    assert_eq!(
        titles(
            nodes
                .lookup_titles(Some("owner-a"), "e_l", 8)
                .await
                .expect("lookup")
        ),
        ["100% done_list"]
    );
    assert!(
        nodes
            .lookup_titles(Some("owner-a"), "   ", 8)
            .await
            .expect("lookup")
            .is_empty()
    );
    // Admin (no subject) sees every owner's titles.
    let all = titles(
        nodes
            .lookup_titles(None, "physna", 8)
            .await
            .expect("lookup"),
    );
    assert!(all.contains(&"Physna secret".to_string()));
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
async fn edge_create_rejects_a_duplicate_source_target_type(pool: PgPool) {
    use common::edge::{CreateEdgeRequest, EdgeType};
    let edges = PgEdgeRepo::new(pool.clone());
    let a = create_node(&pool, "owner-a", "A").await;
    let b = create_node(&pool, "owner-a", "B").await;
    let req = |edge_type, label: Option<&str>| CreateEdgeRequest {
        source_id: a.id,
        target_id: b.id,
        edge_type,
        label: label.map(str::to_string),
    };
    edges
        .create(req(EdgeType::References, None))
        .await
        .expect("first");
    // Regression (#129): the same relationship again — even with another
    // label — is a duplicate, not a second row.
    let dup = edges.create(req(EdgeType::References, Some("again"))).await;
    assert!(
        matches!(&dup, Err(EmberTroveError::AlreadyExists(m)) if m == "already linked as references"),
        "{dup:?}"
    );
    // A different relationship between the same nodes is fine, and wiki
    // links (their own type) never collide with explicit edges.
    edges
        .create(req(EdgeType::DependsOn, None))
        .await
        .expect("other type");
    edges
        .sync_wikilinks(a.id, &[b.id])
        .await
        .expect("wiki link");
    edges.sync_wikilinks(a.id, &[b.id]).await.expect("re-sync");
    assert_eq!(edges.list_for_node(a.id).await.expect("list").len(), 3);
}

#[sqlx::test(migrations = "../migrations")]
async fn migration_034_dedupes_edges_keeping_the_oldest_and_its_label(pool: PgPool) {
    let a = create_node(&pool, "owner-a", "A").await;
    let b = create_node(&pool, "owner-a", "B").await;
    // Recreate the pre-034 state: no unique index, duplicates present.
    sqlx::query("DROP INDEX edges_unique_source_target_type")
        .execute(&pool)
        .await
        .expect("drop index");
    let insert = |label: Option<&'static str>, age_hours: i32, edge_type: &'static str| {
        let pool = pool.clone();
        async move {
            let (id,): (uuid::Uuid,) = sqlx::query_as(
                "INSERT INTO edges (source_id, target_id, edge_type, label, created_at)
                 VALUES ($1, $2, $3::edge_type, $4, now() - make_interval(hours => $5))
                 RETURNING id",
            )
            .bind(a.id.0)
            .bind(b.id.0)
            .bind(edge_type)
            .bind(label)
            .bind(age_hours)
            .fetch_one(&pool)
            .await
            .expect("insert");
            id
        }
    };
    let oldest = insert(None, 30, "references").await;
    insert(Some("first label"), 20, "references").await;
    insert(Some("second label"), 10, "references").await;
    let other_type = insert(Some("keep me"), 5, "depends_on").await;

    sqlx::raw_sql(include_str!("../../../migrations/034_unique_edges.sql"))
        .execute(&pool)
        .await
        .expect("re-run migration 034");

    let left: Vec<(uuid::Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT id, edge_type::text, label FROM edges WHERE source_id = $1 ORDER BY edge_type::text",
    )
    .bind(a.id.0)
    .fetch_all(&pool)
    .await
    .expect("edges");
    // The oldest edge survives and inherits the oldest non-empty label; the
    // other relationship type is untouched.
    assert_eq!(
        left,
        vec![
            (
                other_type,
                "depends_on".to_string(),
                Some("keep me".to_string())
            ),
            (
                oldest,
                "references".to_string(),
                Some("first label".to_string())
            ),
        ]
    );
    // Nothing is lost: both removed rows are archived, labels included.
    let mut archived: Vec<(Option<String>,)> =
        sqlx::query_as("SELECT label FROM edges_removed_034 WHERE source_id = $1")
            .bind(a.id.0)
            .fetch_all(&pool)
            .await
            .expect("archive");
    archived.sort();
    assert_eq!(
        archived,
        vec![
            (Some("first label".to_string()),),
            (Some("second label".to_string()),)
        ]
    );
    // And the constraint is back.
    let dup = sqlx::query(
        "INSERT INTO edges (source_id, target_id, edge_type) VALUES ($1, $2, 'references')",
    )
    .bind(a.id.0)
    .bind(b.id.0)
    .execute(&pool)
    .await;
    assert!(dup.is_err(), "unique index restored");
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
async fn search_pagination_totals_and_per_source_snippets(pool: PgPool) {
    let search = PgSearchRepo::new(pool.clone());
    let nodes = PgNodeRepo::new(pool.clone());
    for i in 0..25 {
        nodes
            .create(
                "owner-a",
                CreateNodeRequest {
                    body: Some(format!("the nebulaorbit report number {i}")),
                    ..node_req(&format!("Report {i:02}"))
                },
            )
            .await
            .expect("node");
    }
    let mut q = search_query("nebulaorbit", false);
    q.per_page = Some(10);
    let mut seen = std::collections::HashSet::new();
    for (page, want) in [(1u32, 10usize), (2, 10), (3, 5)] {
        q.page = Some(page);
        let r = search.search(&q, Some("owner-a")).await.expect("page");
        assert_eq!(r.total, 25, "total on page {page}");
        assert_eq!(r.results.len(), want, "rows on page {page}");
        for hit in &r.results {
            assert!(seen.insert(hit.node_id), "duplicate across pages");
            let snip = hit.snippet.as_deref().unwrap_or_default();
            assert!(
                snip.contains("<mark>"),
                "body snippet marks the match: {snip}"
            );
        }
    }
    // Past the end: no rows, but the total still comes back (fallback count).
    q.page = Some(4);
    let r = search.search(&q, Some("owner-a")).await.expect("past end");
    assert!(r.results.is_empty());
    assert_eq!(r.total, 25);

    // Note and task matches get their own snippets after the paging rewrite.
    let host = create_node(&pool, "owner-a", "Snippet host").await;
    PgNoteRepo::new(pool.clone())
        .create(
            Some(host.id),
            "owner-a",
            common::note::CreateNoteRequest {
                body: "a heliotrope remark".to_string(),
                color: "default".to_string(),
                node_id: None,
            },
        )
        .await
        .expect("note");
    PgTaskRepo::new(pool.clone())
        .create(
            Some(host.id),
            "owner-a",
            CreateTaskRequest {
                node_id: Some(host.id),
                ..task_req("buy saxifrage seeds")
            },
        )
        .await
        .expect("task");
    let note_hit = &search
        .search(&search_query("heliotrope", false), Some("owner-a"))
        .await
        .expect("n")
        .results;
    assert_eq!(note_hit.len(), 1);
    assert_eq!(note_hit[0].match_source.as_deref(), Some("note"));
    assert!(
        note_hit[0]
            .snippet
            .as_deref()
            .unwrap_or_default()
            .contains("<mark>heliotrope</mark>")
    );
    assert!(note_hit[0].highlighted_title.is_none());
    let task_hit = &search
        .search(&search_query("saxifrage", false), Some("owner-a"))
        .await
        .expect("t")
        .results;
    assert_eq!(task_hit[0].match_source.as_deref(), Some("task"));
    assert_eq!(task_hit[0].snippet.as_deref(), Some("buy saxifrage seeds"));

    // Fuzzy mode: same window total and fallback.
    let mut fq = search_query("nebulaorbit", true);
    fq.per_page = Some(10);
    fq.page = Some(3);
    let r = search.search(&fq, Some("owner-a")).await.expect("fuzzy");
    assert_eq!((r.total, r.results.len()), (25, 5));
    fq.page = Some(9);
    let r = search
        .search(&fq, Some("owner-a"))
        .await
        .expect("fuzzy past end");
    assert_eq!((r.total, r.results.len()), (25, 0));
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

// ── Project status (#143) ─────────────────────────────────────────────────────

/// Insert a node straight into the table (bypassing the create path, which
/// would itself move a status token) with a fixed old `updated_at`.
async fn insert_raw_node(
    pool: &PgPool,
    owner: &str,
    node_type: &str,
    title: &str,
    body: &str,
) -> uuid::Uuid {
    sqlx::query_scalar(
        "INSERT INTO nodes (owner_id, node_type, title, slug, body, metadata, status, updated_at) \
         VALUES ($1, $2::node_type, $3, $4, $5, '{}', 'draft', '2020-01-01T00:00:00Z') RETURNING id",
    )
    .bind(owner)
    .bind(node_type)
    .bind(title)
    .bind(common::slug::slugify(title))
    .bind(body)
    .fetch_one(pool)
    .await
    .expect("raw node")
}

#[sqlx::test(migrations = "../migrations")]
async fn backfill_035_moves_tokens_keeps_history_and_runs_once(pool: PgPool) {
    let scaffold = insert_raw_node(
        &pool,
        "a",
        "project",
        "Scaffolded",
        "## Status\n\n- Active\n\n## Goals\n\n-\n",
    )
    .await;
    let bold = insert_raw_node(
        &pool,
        "a",
        "project",
        "Bold",
        "## Status\n\n**ON HOLD**\n\nWaiting on legal.\n",
    )
    .await;
    let prose = insert_raw_node(
        &pool,
        "a",
        "project",
        "Prose",
        "## Status\n\nThings are going\nwell.\n",
    )
    .await;
    let article = insert_raw_node(&pool, "a", "article", "Article", "## Status\n\nActive\n").await;

    assert_eq!(
        crate::backfill::project_status_035(&pool)
            .await
            .expect("backfill"),
        2
    );

    let repo = PgNodeRepo::new(pool.clone());
    let get = |id| {
        let repo = &repo;
        async move { repo.get(common::id::NodeId(id)).await.expect("get") }
    };
    let n = get(scaffold).await;
    assert_eq!(n.project_status.as_deref(), Some("Active"));
    assert_eq!(n.body.as_deref(), Some("## Goals\n\n-\n"));
    // A system change, not an edit: updated_at is untouched.
    assert_eq!(n.updated_at.to_rfc3339(), "2020-01-01T00:00:00+00:00");
    let n = get(bold).await;
    assert_eq!(n.project_status.as_deref(), Some("ON HOLD"));
    assert_eq!(n.body.as_deref(), Some("## Status\n\nWaiting on legal.\n"));
    // Not a token, not a project: left exactly as they were.
    let n = get(prose).await;
    assert_eq!(
        (n.project_status, n.body.as_deref()),
        (None, Some("## Status\n\nThings are going\nwell.\n"))
    );
    assert_eq!(get(article).await.project_status, None);

    // The old body is restorable: snapshots are old then new.
    let bodies: Vec<String> =
        sqlx::query_scalar("SELECT body FROM node_versions WHERE node_id = $1 ORDER BY created_at")
            .bind(scaffold)
            .fetch_all(&pool)
            .await
            .expect("versions");
    assert_eq!(
        bodies,
        [
            "## Status\n\n- Active\n\n## Goals\n\n-\n",
            "## Goals\n\n-\n"
        ]
    );

    // Once only: a second start changes nothing, even with a fresh candidate.
    insert_raw_node(&pool, "a", "project", "Later", "## Status\n\nDone\n").await;
    assert_eq!(
        crate::backfill::project_status_035(&pool)
            .await
            .expect("rerun"),
        0
    );
}

#[sqlx::test(migrations = "../migrations")]
async fn project_status_filter_counts_and_three_state_update(pool: PgPool) {
    let repo = PgNodeRepo::new(pool.clone());
    let make =
        |owner: &'static str, title: &'static str, node_type: NodeType, status: &'static str| {
            let repo = &repo;
            async move {
                repo.create(
                    owner,
                    CreateNodeRequest {
                        node_type,
                        project_status: Some(status.to_string()),
                        ..node_req(title)
                    },
                )
                .await
                .expect("create")
            }
        };
    let p1 = make("a", "P1", NodeType::Project, "Active").await;
    make("a", "P2", NodeType::Project, "Active").await;
    make("a", "P3", NodeType::Project, "ACTIVE").await;
    make("a", "P4", NodeType::Project, "On hold").await;
    // Not counted: someone else's project, and a non-project's value.
    make("b", "Theirs", NodeType::Project, "Secret plan").await;
    make("a", "Note", NodeType::Article, "Active").await;

    let counts = repo.project_status_counts("a").await.expect("counts");
    assert_eq!(
        counts,
        vec![
            common::node::ProjectStatusCount {
                status: "Active".into(),
                count: 3
            },
            common::node::ProjectStatusCount {
                status: "On hold".into(),
                count: 1
            },
        ]
    );

    // The list filter is case-insensitive.
    let (hits, total) = repo
        .list(common::node::NodeListParams {
            node_type: Some(NodeType::Project),
            status: None,
            project_status: Some("active".into()),
            tag_id: None,
            owner_id: Some("a".into()),
            subject_id: None,
            page: None,
            per_page: None,
            include_archived: false,
        })
        .await
        .expect("list");
    assert_eq!(total, 3);
    assert!(hits.iter().all(|n| {
        n.project_status
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
            == Some("active")
    }));

    // Update: absent leaves it, a value sets it, null clears it.
    let upd = |ps: Option<Option<String>>| common::node::UpdateNodeRequest {
        title: None,
        body: None,
        metadata: None,
        status: None,
        project_status: ps,
    };
    let n = repo.update(p1.id, upd(None)).await.expect("noop");
    assert_eq!(n.project_status.as_deref(), Some("Active"));
    let n = repo
        .update(p1.id, upd(Some(Some("Done".into()))))
        .await
        .expect("set");
    assert_eq!(n.project_status.as_deref(), Some("Done"));
    let n = repo.update(p1.id, upd(Some(None))).await.expect("clear");
    assert_eq!(n.project_status, None);

    // The database refuses what normalize would (defence in depth).
    let too_long = repo.update(p1.id, upd(Some(Some("x".repeat(41))))).await;
    assert!(too_long.is_err());
}

// ── People (#147) ─────────────────────────────────────────────────────────────

fn person(name: &str, email: Option<&str>) -> common::person::PersonInput {
    common::person::PersonInput {
        display_name: name.into(),
        company: Some("Acme".into()),
        emails: email
            .map(|e| {
                vec![common::person::ContactPoint {
                    label: "work".into(),
                    value: e.into(),
                    preferred: true,
                }]
            })
            .unwrap_or_default(),
        ..Default::default()
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn people_are_owner_scoped_searchable_and_duplicate_checked(pool: PgPool) {
    use super::person::{PersonRepo, PgPersonRepo};
    let repo = PgPersonRepo::new(pool.clone());
    let ada = repo
        .create("a", &person("Ada Lovelace", Some("ada@example.com")))
        .await
        .expect("ada");
    repo.create("a", &person("Charles Babbage", Some("cb@engines.test")))
        .await
        .expect("cb");
    let theirs = repo
        .create("b", &person("Ada Byron", Some("ada@example.com")))
        .await
        .expect("b");

    // The JSON contact list round-trips.
    assert_eq!(ada.emails[0].value, "ada@example.com");
    // Owner-scoped: b's person doesn't exist for a, even by id.
    assert!(matches!(
        repo.get("a", theirs.id).await,
        Err(EmberTroveError::NotFound(_))
    ));
    assert!(matches!(
        repo.delete("a", theirs.id).await,
        Err(EmberTroveError::NotFound(_))
    ));
    let names =
        |v: Vec<common::person::Person>| v.into_iter().map(|p| p.display_name).collect::<Vec<_>>();
    assert_eq!(
        names(repo.list("a", None, 100).await.unwrap()),
        ["Ada Lovelace", "Charles Babbage"]
    );
    // Search covers name, company, email and phone; LIKE wildcards are literal.
    assert_eq!(
        names(repo.list("a", Some("ENGINES"), 100).await.unwrap()),
        ["Charles Babbage"]
    );
    assert_eq!(
        names(repo.list("a", Some("lovel"), 100).await.unwrap()),
        ["Ada Lovelace"]
    );
    assert!(repo.list("a", Some("%"), 100).await.unwrap().is_empty());

    // The duplicate check is per owner and case-insensitive.
    let dup = repo
        .find_by_emails("a", &["ada@example.com".into()])
        .await
        .unwrap();
    assert_eq!(dup.len(), 1);
    assert_eq!(dup[0].id, ada.id);
}

#[sqlx::test(migrations = "../migrations")]
async fn memberships_are_unique_node_scoped_and_cascade(pool: PgPool) {
    use super::person::{PersonRepo, PgPersonRepo};
    let repo = PgPersonRepo::new(pool.clone());
    let project = create_node(&pool, "a", "Project X").await;
    let other = create_node(&pool, "a", "Project Y").await;
    let ada = repo.create("a", &person("Ada", None)).await.unwrap();

    let m = repo
        .add_membership(project.id, ada.id, Some("Sponsor"), Some("kickoff"))
        .await
        .unwrap();
    // One membership per person per node.
    assert!(matches!(
        repo.add_membership(project.id, ada.id, None, None).await,
        Err(EmberTroveError::AlreadyExists(_))
    ));
    repo.add_membership(other.id, ada.id, Some("sponsor"), None)
        .await
        .unwrap();

    let on_project = repo.memberships_for_node(project.id).await.unwrap();
    assert_eq!(on_project.len(), 1);
    assert_eq!(
        (
            on_project[0].role.as_deref(),
            on_project[0].person.display_name.as_str()
        ),
        (Some("Sponsor"), "Ada")
    );

    // "Used in" and the role suggestions (case variants are one role).
    let used = repo.usages("a", ada.id).await.unwrap();
    assert_eq!(
        used.iter()
            .map(|u| u.node_title.as_str())
            .collect::<Vec<_>>(),
        ["Project X", "Project Y"]
    );
    assert!(matches!(
        repo.usages("b", ada.id).await,
        Err(EmberTroveError::NotFound(_))
    ));
    let roles = repo.role_counts("a").await.unwrap();
    assert_eq!((roles.len(), roles[0].count), (1, 2));
    assert!(repo.role_counts("b").await.unwrap().is_empty());

    // Update and remove are scoped to the path's node.
    assert!(matches!(
        repo.update_membership(other.id, m, Some("x"), None).await,
        Err(EmberTroveError::NotFound(_))
    ));
    repo.update_membership(project.id, m, Some("Approver"), None)
        .await
        .unwrap();
    assert!(matches!(
        repo.remove_membership(other.id, m).await,
        Err(EmberTroveError::NotFound(_))
    ));
    repo.remove_membership(project.id, m).await.unwrap();
    // Removing a membership never deletes the person.
    assert!(repo.get("a", ada.id).await.is_ok());

    // Deleting a node or a person removes their memberships.
    PgNodeRepo::new(pool.clone())
        .delete(other.id)
        .await
        .unwrap();
    assert!(repo.usages("a", ada.id).await.unwrap().is_empty());
    repo.add_membership(project.id, ada.id, None, None)
        .await
        .unwrap();
    repo.delete("a", ada.id).await.unwrap();
    assert!(
        repo.memberships_for_node(project.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test(migrations = "../migrations")]
async fn the_database_refuses_an_oversized_person(pool: PgPool) {
    use super::person::{PersonRepo, PgPersonRepo};
    // Defence in depth behind `common::person::normalize`.
    let res = PgPersonRepo::new(pool)
        .create("a", &person(&"x".repeat(201), None))
        .await;
    assert!(res.is_err());
}
