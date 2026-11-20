//! Merging two saved People into one (S8c, #363; ONTOLOGY §7.1).
//!
//! `common::person_merge` is the rule for what the kept person holds
//! afterwards. This module reads what that rule needs and writes its
//! result, in one transaction.
//!
//! SECURITY: both people are the caller's own. Every statement that
//! reads or locks a person binds the owner, with no admin bypass, so an
//! id of another account is never read, never locked and never waited
//! on: it is a missing person, in the same time.
//!
//! **Locks, and who gives way.** First two advisory locks, shared: the
//! account's purge key and the restore key. Then both `people` rows, in
//! id order (`.claude/rules/api.md`: rows first, then the email rule's
//! advisory lock, which the trigger takes when the kept person is
//! written); their `node_people` rows; and their `node_mentions` rows.
//! Nothing here locks a node: the rows that hang on nodes are moved by
//! changing `person_id` alone.
//!
//! No order of these fits every other transaction. A node's delete
//! reaches the same rows through its cascades, in an order that depends
//! on the names of the database's triggers; a save, a copy and a note
//! each take a node or a note first and people after. Three reviews of
//! #364 each found a pair that could hold what the other waited for,
//! and each lock added to fit one of them broke the order of another.
//! So the rule is not an order but **who gives way: the merge does.**
//! Once it holds a row, it waits for any other lock for well under the
//! time the database takes to look for a deadlock ([`YIELDS`]). Met
//! head on, it is the merge that gives up, with nothing written, before
//! the database ends either; the other goes on as if nothing happened.
//! The merge is then tried once more and, if it is still in the way,
//! answers "try again". A merge is rare, short and somebody's own
//! deliberate act; a save or a delete ended with a 500 is not.
//!
//! What is left to the others is only what makes them **decide after a
//! merge and not during it** (`repo/mention.rs`, `repo/person.rs`): a
//! statement of their own that waits for the merge's rows before the
//! statement that decides.
//!
//! **No text is touched** (owner decision, 2026-11-19). The id of the
//! person who goes is recorded in `person_aliases` and stands for the
//! one kept wherever a mention is read or written
//! (`api/src/repo/mention.rs`).

use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{MembershipId, NodeId, PersonId},
    person::{ContactPoint, Person},
    person_merge::{MembershipIn, MergePlan, Planned, fingerprint, plan},
};
use sqlx::{PgConnection, PgPool, types::Json};
use uuid::Uuid;

use super::node::{DEADLOCK, LOCK_NOT_AVAILABLE, sqlstate};

/// How long a merge waits for a row or a lock another transaction holds.
const LOCK_TIMEOUT: &str = "SET LOCAL lock_timeout = '5s'";

/// How long a merge waits for a row, once it holds one: two fifths of
/// the time after which the database looks for a deadlock
/// (`deadlock_timeout`, one second unless the server says otherwise).
/// The one that has waited that long finds the deadlock and is ended.
/// A merge that stops waiting sooner is never found in one, and whoever
/// it met goes on. (Not a promise: a merge that had already made the
/// other wait for most of a second, through several short waits of its
/// own, can still be late. Then the database ends one of the two, as it
/// would have, and a merge so ended is tried once more.)
pub(crate) const YIELDS: &str = "SELECT set_config('lock_timeout', greatest(1, \
     (extract(epoch FROM current_setting('deadlock_timeout')::interval) * 400)::int)::text, true)";

/// For a test that holds a row to stop a merge half way and needs it to
/// be there still when the row is let go.
#[cfg(all(test, feature = "pg-tests"))]
pub(crate) const WAITS: &str = "SET LOCAL lock_timeout = '20s'";

/// The restore's key: a restore takes it alone, a merge shares it.
pub(crate) const RESTORE_LOCK: &str = "SELECT pg_advisory_xact_lock(hashtext('ember.restore'))";

/// What the answer is when either person is not the caller's, or is not
/// there. One sentence for both, naming neither.
const NOT_FOUND: &str = "person not found";

/// What a merge is told when the plan it confirmed is no longer the plan.
pub(crate) const PLAN_CHANGED: &str =
    "these people changed since you were shown what the merge would do: look again";

const BUSY: &str = "another change to these people is in progress: try again";

/// A failure inside the merge: a database error, kept whole so a deadlock
/// can be told from the rest, or an answer for the caller.
enum MergeError {
    Db(sqlx::Error),
    Public(EmberTroveError),
}

impl From<sqlx::Error> for MergeError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl From<EmberTroveError> for MergeError {
    fn from(e: EmberTroveError) -> Self {
        Self::Public(e)
    }
}

impl MergeError {
    fn into_public(self) -> EmberTroveError {
        match self {
            Self::Public(e) => e,
            Self::Db(e) if matches!(sqlstate(&e), Some(LOCK_NOT_AVAILABLE | DEADLOCK)) => {
                EmberTroveError::AlreadyExists(BUSY.to_string())
            }
            // The email rule refused the kept person's new list: someone
            // else took one of the addresses between the plan and now.
            Self::Db(sqlx::Error::Database(db))
                if db.constraint() == Some("people_email_identity") =>
            {
                EmberTroveError::PreconditionFailed(PLAN_CHANGED.to_string())
            }
            // SECURITY: `Display` only, never the error's detail: a detail
            // can quote a row.
            Self::Db(e) => EmberTroveError::Internal(format!("merge people failed: {e}")),
        }
    }
}

#[derive(sqlx::FromRow)]
struct PersonRow {
    id: Uuid,
    owner_id: String,
    display_name: String,
    company: Option<String>,
    position: Option<String>,
    emails: Json<Vec<ContactPoint>>,
    phones: Json<Vec<ContactPoint>>,
    addresses: Json<Vec<ContactPoint>>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PersonRow> for Person {
    fn from(r: PersonRow) -> Self {
        Person {
            id: PersonId(r.id),
            owner_id: r.owner_id,
            display_name: r.display_name,
            company: r.company,
            position: r.position,
            emails: r.emails.0,
            phones: r.phones.0,
            addresses: r.addresses.0,
            notes: r.notes,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const PERSON_COLS: &str = "id, owner_id, display_name, company, position, emails, phones, \
     addresses, notes, created_at, updated_at";

/// Everything the rule reads, as one transaction sees it.
struct Read {
    kept: Person,
    gone: Person,
    /// Addresses of the one who goes that a third person lists too.
    third_party: Vec<String>,
    kept_on: Vec<MembershipIn>,
    gone_on: Vec<MembershipIn>,
}

impl Read {
    fn planned(&self) -> Planned {
        plan(
            &self.kept,
            &self.gone,
            &self.third_party,
            &self.kept_on,
            &self.gone_on,
        )
    }

    /// The token that stands for this plan: a hash of what it was made
    /// from. Not a secret; the write decides from the rows under its own
    /// locks, never from the token.
    fn token(&self) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(
            fingerprint(
                &self.kept,
                &self.gone,
                &self.third_party,
                &self.kept_on,
                &self.gone_on,
            )
            .as_bytes(),
        ))
    }
}

/// Read both people and what hangs on them. With `lock`, the rows are
/// held until the transaction ends.
///
/// SECURITY: the people are read by id **and owner**. Fewer than two
/// rows is "not found", whichever is missing and whoever it belongs to.
async fn read(
    conn: &mut PgConnection,
    owner: &str,
    kept: PersonId,
    gone: PersonId,
    lock: bool,
) -> Result<Read, MergeError> {
    let for_update = if lock { " FOR UPDATE" } else { "" };
    let people = sqlx::query_as::<_, PersonRow>(&format!(
        "SELECT {PERSON_COLS} FROM people \
         WHERE id = ANY($1) AND owner_id = $2 ORDER BY id{for_update}"
    ))
    .bind([kept.0, gone.0])
    .bind(owner)
    .fetch_all(&mut *conn)
    .await?;
    let mut people: Vec<Person> = people.into_iter().map(Person::from).collect();
    let not_found = || MergeError::Public(EmberTroveError::NotFound(NOT_FOUND.to_string()));
    let at = |people: &[Person], id: PersonId| people.iter().position(|p| p.id == id);
    let kept_person = at(&people, kept)
        .map(|i| people.remove(i))
        .ok_or_else(not_found)?;
    let gone_person = at(&people, gone)
        .map(|i| people.remove(i))
        .ok_or_else(not_found)?;

    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        node_id: Uuid,
        person_id: Uuid,
        role: Option<String>,
        note: Option<String>,
        title: String,
    }
    let for_update = if lock { " FOR UPDATE OF m" } else { "" };
    let rows = sqlx::query_as::<_, Row>(&format!(
        "SELECT m.id, m.node_id, m.person_id, m.role, m.note, n.title \
         FROM node_people m JOIN nodes n ON n.id = m.node_id \
         WHERE m.person_id = ANY($1) ORDER BY m.id{for_update}"
    ))
    .bind([kept.0, gone.0])
    .fetch_all(&mut *conn)
    .await?;
    let (mut kept_on, mut gone_on) = (Vec::new(), Vec::new());
    for r in rows {
        let row = MembershipIn {
            id: MembershipId(r.id),
            node_id: NodeId(r.node_id),
            node_title: r.title,
            role: r.role,
            note: r.note,
        };
        if r.person_id == kept.0 {
            kept_on.push(row);
        } else {
            gone_on.push(row);
        }
    }

    // The database's own function, the one its rule decides with: the
    // addresses the kept person would gain that someone else lists. The
    // one who goes is not "someone else": they are about to be gone.
    let third_party: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT c.email \
         FROM person_email_collisions($1, $2::uuid, $3::jsonb, $4::jsonb) c \
         WHERE c.person_id <> $5 ORDER BY 1",
    )
    .bind(owner)
    .bind(kept.0)
    .bind(Json(&kept_person.emails))
    .bind(Json(&gone_person.emails))
    .bind(gone.0)
    .fetch_all(&mut *conn)
    .await?;

    Ok(Read {
        kept: kept_person,
        gone: gone_person,
        third_party,
        kept_on,
        gone_on,
    })
}

/// The plan for merging `gone` into `kept`, both `owner`'s. Nothing is
/// written or locked. `mentioned_in` is left at 0: the route counts it,
/// for its reader.
pub(crate) async fn merge_plan(
    pool: &PgPool,
    owner: &str,
    kept: PersonId,
    gone: PersonId,
) -> Result<MergePlan, EmberTroveError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| EmberTroveError::Internal(format!("acquire connection failed: {e}")))?;
    let read = read(&mut conn, owner, kept, gone, false)
        .await
        .map_err(MergeError::into_public)?;
    let planned = read.planned();
    Ok(MergePlan {
        kept,
        gone,
        kept_name: read.kept.display_name.clone(),
        gone_name: read.gone.display_name.clone(),
        token: read.token(),
        moved: u32::try_from(planned.moved.len()).unwrap_or(u32::MAX),
        result: planned.result,
        report: planned.report,
        notes_line: planned.notes_line,
        joined: planned.joined,
        mentioned_in: 0,
        refusals: planned.refusals,
    })
}

/// One attempt at the merge, in its own transaction.
async fn merge_once(
    pool: &PgPool,
    owner: &str,
    kept: PersonId,
    gone: PersonId,
    confirm: &str,
    patience: &'static str,
) -> Result<Person, MergeError> {
    // READ COMMITTED (the pool's default): the email rule refuses to run
    // under anything else.
    let mut tx = pool.begin().await?;
    sqlx::query(LOCK_TIMEOUT).execute(&mut *tx).await?;
    // Before any row, and with nothing held, so with time to wait: a
    // purge and a restore delete what hangs on nodes before they reach
    // `people`, the other way round from a merge.
    //
    // A purge or a restore still running after that wait is answered
    // "try again" at once. It is not a meeting to give way in and try
    // again: a second go would only wait as long once more.
    let busy = |e: sqlx::Error| {
        if sqlstate(&e) == Some(LOCK_NOT_AVAILABLE) {
            MergeError::Public(EmberTroveError::AlreadyExists(BUSY.to_string()))
        } else {
            MergeError::Db(e)
        }
    };
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtext('ember.purge.' || $1))")
        .bind(owner)
        .execute(&mut *tx)
        .await
        .map_err(busy)?;
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtext('ember.restore'))")
        .execute(&mut *tx)
        .await
        .map_err(busy)?;
    // From here on the merge holds rows, and gives way: module doc.
    sqlx::query(patience).execute(&mut *tx).await?;

    let read = read(&mut tx, owner, kept, gone, true).await?;
    // The body rows of both, held before anything is decided about them.
    // A save that is taking one of them out of a body ends first, and
    // the moves below see what it left; a save that comes later waits
    // for the row it means to delete (`repo::mention::drop_body_rows`)
    // and decides after the merge.
    sqlx::query(
        "SELECT 1 FROM node_mentions WHERE person_id = ANY($1) \
         ORDER BY node_id, person_id FOR UPDATE",
    )
    .bind([kept.0, gone.0])
    .execute(&mut *tx)
    .await?;
    let planned = read.planned();
    // What was confirmed must be what is true now, under the locks.
    if read.token() != confirm {
        return Err(EmberTroveError::PreconditionFailed(PLAN_CHANGED.to_string()).into());
    }
    if let Some(refusal) = planned.refusals.first() {
        return Err(EmberTroveError::Validation(refusal.says()).into());
    }

    // Where both are on a node: the kept person's row takes the joined
    // role and note, and the other's row goes.
    for join in &planned.joined {
        sqlx::query("UPDATE node_people SET role = $2, note = $3 WHERE id = $1")
            .bind(join.kept_row.0)
            .bind(&join.role)
            .bind(&join.note)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM node_people WHERE id = $1")
            .bind(join.gone_row.0)
            .execute(&mut *tx)
            .await?;
    }
    // The rest move as they are, ids and all: only `person_id` changes,
    // so no node row is read or locked for it.
    sqlx::query("UPDATE node_people SET person_id = $1 WHERE person_id = $2")
        .bind(kept.0)
        .bind(gone.0)
        .execute(&mut *tx)
        .await?;
    // Mention rows the same way. Where the kept person is mentioned in
    // the same text already, the other's row is left for the cascade.
    // A body's rows first (they are held already), then a note's.
    sqlx::query(
        "UPDATE node_mentions m SET person_id = $1 WHERE m.person_id = $2 \
         AND NOT EXISTS (SELECT 1 FROM node_mentions k \
                         WHERE k.node_id = m.node_id AND k.person_id = $1)",
    )
    .bind(kept.0)
    .bind(gone.0)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE note_mentions m SET person_id = $1 WHERE m.person_id = $2 \
         AND NOT EXISTS (SELECT 1 FROM note_mentions k \
                         WHERE k.note_id = m.note_id AND k.person_id = $1)",
    )
    .bind(kept.0)
    .bind(gone.0)
    .execute(&mut *tx)
    .await?;
    // Ids that stood for the one who goes stand for the kept person now:
    // the table never holds a chain.
    sqlx::query("UPDATE person_aliases SET person_id = $1 WHERE person_id = $2")
        .bind(kept.0)
        .bind(gone.0)
        .execute(&mut *tx)
        .await?;
    // The one who goes, before the kept person is written: the email rule
    // must find their addresses free.
    sqlx::query("DELETE FROM people WHERE id = $1 AND owner_id = $2")
        .bind(gone.0)
        .bind(owner)
        .execute(&mut *tx)
        .await?;
    let p = &planned.result;
    let row = sqlx::query_as::<_, PersonRow>(&format!(
        "UPDATE people SET \
             display_name = $3, company = $4, position = $5, emails = $6, phones = $7, \
             addresses = $8, notes = $9 \
         WHERE id = $1 AND owner_id = $2 \
         RETURNING {PERSON_COLS}"
    ))
    .bind(kept.0)
    .bind(owner)
    .bind(&p.display_name)
    .bind(&p.company)
    .bind(&p.position)
    .bind(Json(&p.emails))
    .bind(Json(&p.phones))
    .bind(Json(&p.addresses))
    .bind(&p.notes)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO person_aliases (old_id, person_id) VALUES ($1, $2)")
        .bind(gone.0)
        .bind(kept.0)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(row.into())
}

/// Merge `gone` into `kept`, both `owner`'s, if `confirm` is the token of
/// the plan as it is now. Returns the kept person as saved.
///
/// - NotFound: either is not the owner's, or not there.
/// - PreconditionFailed (412): something the plan was made from changed.
/// - Validation (422): the plan has a refusal.
/// - AlreadyExists (409): the rows are held by something else; try again.
pub(crate) async fn merge(
    pool: &PgPool,
    owner: &str,
    kept: PersonId,
    gone: PersonId,
    confirm: &str,
) -> Result<Person, EmberTroveError> {
    merge_with(pool, owner, kept, gone, confirm, YIELDS).await
}

/// [`merge`], waiting for a held row as `patience` says.
pub(crate) async fn merge_with(
    pool: &PgPool,
    owner: &str,
    kept: PersonId,
    gone: PersonId,
    confirm: &str,
    patience: &'static str,
) -> Result<Person, EmberTroveError> {
    match merge_once(pool, owner, kept, gone, confirm, patience).await {
        // The merge gave way to something that held a row it needed, or
        // was ended as one of two waiting on each other. Nothing was
        // written: once more, from the start. What it met has most
        // often finished by then, and if what it did changed the plan,
        // the second go says so.
        Err(MergeError::Db(e)) if matches!(sqlstate(&e), Some(DEADLOCK | LOCK_NOT_AVAILABLE)) => {
            merge_once(pool, owner, kept, gone, confirm, patience)
                .await
                .map_err(MergeError::into_public)
        }
        other => other.map_err(MergeError::into_public),
    }
}
