//! Mentions of People in text (S9, #360; ONTOLOGY §7.3, §14).
//!
//! `common::mention` reads the syntax. This module holds the two rules
//! that make a mention safe, each in one place:
//!
//! 1. **Who may make one** ([`sync_body_on`], [`sync_note_on`]). A row
//!    is written only for a mention **the save itself adds**, of a person
//!    who belongs to the account saving. Without a row nobody is named.
//!    A person's id is not a secret (a sharee sees it in a shared
//!    Project's People list, and in the raw text), so both halves are
//!    needed: a token typed with someone else's id gets no row from its
//!    typist, and none from that person's owner saving the text later,
//!    because the owner did not put it there.
//! 2. **To whom a person is named** ([`named_to`]). To their owner; and
//!    to another reader of the text only while the person's owner can
//!    still open the node. Without the second half a co-editor could
//!    mention their own contact in someone's node, lose access, and the
//!    node's owner would keep reading that contact's current name
//!    through every rename.
//!
//! There is no lookup of a name by id. Names are read only with the
//! text that holds the mention, by a route that has already checked its
//! reader may read that text, and always for that reader in person:
//! never for "everyone", as an admin's lists are.
//!
//! **Aliases (S8c, #363).** A merge of two people deletes one and leaves
//! their id in `person_aliases`, standing for the one kept. No text is
//! changed, so a token can hold either id. The rows are always of the
//! living person; every statement here that compares a row with the ids
//! a text holds reads those ids through the alias table:
//! - a save that adds a token with the old id makes a row for the kept
//!   person, **if they are the saver's own** (rule 1 is unchanged: the
//!   alias is read inside the same statement that checks the owner);
//! - a row stays while the text holds the person's id or an alias of it;
//! - a read answers a person **only under the ids that text holds**:
//!   their own, an alias, or both. Told of every id of a person named on
//!   one node, a reader would learn that two ids are one person, and
//!   could then read the name behind a token on another node where rule
//!   2 withholds it. A text that holds only the old id never tells
//!   anyone but the person's owner what their id is now.

use std::collections::HashMap;

use async_trait::async_trait;
use common::EmberTroveError;
use common::id::{NodeId, NoteId, PersonId};
use common::mention::{BackupMention, MentionName, PersonMention};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::{owned_or_granted, visible_to};

fn db(what: &'static str) -> impl Fn(sqlx::Error) -> EmberTroveError {
    move |e| EmberTroveError::Internal(format!("{what} failed: {e}"))
}

/// SQL: the person `p` may be named to the reader bound at `$reader`,
/// for a mention on the node `n`. Rule 2 of the module.
///
/// SECURITY: the reader is always a bound subject. `owned_or_granted`
/// has no admin branch, and is given a column, not a bind: nothing here
/// can come to mean "everyone".
fn named_to(reader: u8) -> String {
    format!(
        "(p.owner_id = ${reader}::text OR {})",
        owned_or_granted("n", "p.owner_id")
    )
}

/// SQL: the person `p` is one the ids bound at `$ids` stand for: by
/// their own id, or by the id of someone merged into them (S8c).
fn stands_for(ids: u8) -> String {
    format!(
        "(p.id = ANY(${ids}) OR p.id IN \
           (SELECT a.person_id FROM person_aliases a WHERE a.old_id = ANY(${ids})))"
    )
}

/// SQL: the row `m` is of a person the ids bound at `$ids` stand for.
fn still_mentioned(ids: u8) -> String {
    format!(
        "(m.person_id = ANY(${ids}) OR EXISTS \
           (SELECT 1 FROM person_aliases a \
            WHERE a.person_id = m.person_id AND a.old_id = ANY(${ids})))"
    )
}

/// SQL: the id `held.id`, one a text holds, is one of the person `p`'s:
/// their own, or the id of someone merged into them.
const HELD_IS_THEIRS: &str = "(held.id = p.id OR held.id IN \
       (SELECT a.old_id FROM person_aliases a WHERE a.person_id = p.id))";

/// The one statement that makes rows for a node's body: for the people
/// in `make` who belong to `saver`, and nobody else.
///
/// One statement, so an id of another account's person and an id of
/// nobody leave the same trace: none. `FOR KEY SHARE` holds the people
/// read until this transaction ends; without it a person deleted between
/// the read and the insert would fail the foreign key and take the
/// body's save down with it.
///
/// `ORDER BY p.id`: the people are locked in id order, as a merge locks
/// its two (`repo/person_merge.rs`), so the two do not meet head on
/// over them in the everyday case. (Where they still do, the merge
/// gives way: the second pass below can take the kept person after
/// someone with a higher id.)
async fn make_body_rows(
    conn: &mut PgConnection,
    node: NodeId,
    make: &[PersonId],
    saver: &str,
) -> Result<(), EmberTroveError> {
    if make.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = make.iter().map(|p| p.0).collect();
    let insert = format!(
        "INSERT INTO node_mentions (node_id, person_id) \
         SELECT $1, p.id FROM people p \
         WHERE {theirs} AND p.owner_id = $3 \
         ORDER BY p.id FOR KEY SHARE OF p \
         ON CONFLICT DO NOTHING",
        theirs = stands_for(2),
    );
    // Twice at most. A merge that was under way holds the person's row:
    // this statement waits for it, then finds the person gone, and its
    // own view of the alias table is from before the merge. Asked again,
    // it sees the alias and makes the row for the person kept. (Fewer
    // rows than ids is also the everyday case of an id that is not the
    // saver's, or a row that was there: the second pass then finds the
    // same nothing.)
    for _ in 0..2 {
        let made = sqlx::query(&insert)
            .bind(node.0)
            .bind(&ids)
            .bind(saver)
            .execute(&mut *conn)
            .await
            .map_err(db("sync mentions"))?
            .rows_affected();
        if made >= ids.len() as u64 {
            break;
        }
    }
    Ok(())
}

/// Delete a node's rows for people its body no longer mentions.
///
/// The node's rows are held first, by a statement of its own. A merge
/// under way holds the body rows of its two people
/// (`repo/person_merge.rs`): this waits for it, and the delete that
/// follows then sees the alias it left. In one statement, a save that
/// takes the kept person's token out and leaves the other's deleted the
/// kept person's row (the other's id did not stand for them yet) while
/// the merge dropped the other's row with the person: a mention its
/// owner made was left with no row at all.
///
/// Rows of this node only, and no person: a save holds nobody it does
/// not itself add a mention of, and nobody of another account. (It did,
/// for one commit: the account purge then had to take its people in the
/// same order, and that put the purge out of step with a restore and
/// with a note's save.)
///
/// A note needs none of this: every save of a note makes all its rows
/// again, and that one statement holds every person the note's text
/// names, in id order.
async fn drop_body_rows(
    conn: &mut PgConnection,
    node: NodeId,
    mentioned: &[PersonId],
) -> Result<(), EmberTroveError> {
    let ids: Vec<Uuid> = mentioned.iter().map(|p| p.0).collect();
    sqlx::query(
        "SELECT 1 FROM node_mentions m WHERE m.node_id = $1 ORDER BY m.person_id FOR UPDATE",
    )
    .bind(node.0)
    .execute(&mut *conn)
    .await
    .map_err(db("sync mentions"))?;
    sqlx::query(&format!(
        "DELETE FROM node_mentions m WHERE m.node_id = $1 AND NOT {kept}",
        kept = still_mentioned(2),
    ))
    .bind(node.0)
    .bind(&ids)
    .execute(&mut *conn)
    .await
    .map_err(db("sync mentions"))?;
    Ok(())
}

/// Bring a node's mention rows in line with its body, as saved by
/// `saver`. Runs on the connection of the transaction that wrote the
/// body, so the text and its rows can't part (a row left behind by a
/// failed sync would name a person their owner had taken out).
///
/// - `mentioned`: everyone the saved body mentions
///   (`common::mention::mentioned_ids`). A row for anyone else is
///   deleted. A row for someone still mentioned stays, whoever made it:
///   a co-editor's save keeps the owner's mentions.
/// - `make`: the people this save **added** a mention of
///   (`common::mention::added_mentions`). A row is made for those of
///   them who are `saver`'s own, and for no one else.
///
/// SECURITY: `make` must be what this save added, never "everyone the
/// body mentions". A token already in the text was put there by an
/// earlier save, perhaps a co-editor's with the id of this saver's
/// person: saving around it must not make it real.
///
/// The insert comes first: it locks the people it reads, and the delete
/// then locks rows of this table. The other order could deadlock with
/// an account purge, which deletes people and then, by cascade, rows
/// here.
pub(crate) async fn sync_body_on(
    conn: &mut PgConnection,
    node: NodeId,
    mentioned: &[PersonId],
    make: &[PersonId],
    saver: &str,
) -> Result<(), EmberTroveError> {
    make_body_rows(conn, node, make, saver).await?;
    drop_body_rows(conn, node, mentioned).await
}

/// The rows of a copy (`to`) of the node `from`: for the people the body
/// mentions who have a row on `from` **and** belong to the one copying.
/// So the copier's own real mentions are real in the copy; a co-editor's
/// people, and tokens that were never real, are not.
pub(crate) async fn copy_body_rows_on(
    conn: &mut PgConnection,
    from: NodeId,
    to: NodeId,
    mentioned: &[PersonId],
    copier: &str,
) -> Result<(), EmberTroveError> {
    if mentioned.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = mentioned.iter().map(|p| p.0).collect();
    // The copier's people on the source are held first, by a statement
    // of its own: a copy made while one of them is being merged away
    // waits here, and the insert then sees the row and the alias as the
    // merge left them. In one statement it waited, found the person
    // gone, and made no row for a mention the original still has.
    sqlx::query(
        "SELECT 1 FROM people p \
         WHERE p.id IN (SELECT m.person_id FROM node_mentions m WHERE m.node_id = $1) \
           AND p.owner_id = $2 \
         ORDER BY p.id FOR KEY SHARE OF p",
    )
    .bind(from.0)
    .bind(copier)
    .execute(&mut *conn)
    .await
    .map_err(db("copy mentions"))?;
    sqlx::query(&format!(
        "INSERT INTO node_mentions (node_id, person_id) \
         SELECT $2, p.id FROM node_mentions m \
         JOIN people p ON p.id = m.person_id \
         WHERE m.node_id = $1 AND {held} AND p.owner_id = $4 \
         ORDER BY p.id FOR KEY SHARE OF p \
         ON CONFLICT DO NOTHING",
        held = still_mentioned(3),
    ))
    .bind(from.0)
    .bind(to.0)
    .bind(&ids)
    .bind(copier)
    .execute(&mut *conn)
    .await
    .map_err(db("copy mentions"))?;
    Ok(())
}

/// The same for a note, whose only writer is its `author`: every person
/// of theirs the note mentions has a row, and no one else. (Nobody can
/// plant a token in someone else's note, so there is nothing to tell
/// "added" from "there already".)
pub(crate) async fn sync_note_on(
    conn: &mut PgConnection,
    note: NoteId,
    mentioned: &[PersonId],
    author: &str,
) -> Result<(), EmberTroveError> {
    let ids: Vec<Uuid> = mentioned.iter().map(|p| p.0).collect();
    if !ids.is_empty() {
        // Once: every save of a note makes all its rows again, so one
        // that lost a row to a merge under way gets it with the next.
        sqlx::query(&format!(
            "INSERT INTO note_mentions (note_id, person_id) \
             SELECT $1, p.id FROM people p \
             WHERE {theirs} AND p.owner_id = $3 \
             ORDER BY p.id FOR KEY SHARE OF p \
             ON CONFLICT DO NOTHING",
            theirs = stands_for(2),
        ))
        .bind(note.0)
        .bind(&ids)
        .bind(author)
        .execute(&mut *conn)
        .await
        .map_err(db("sync mentions"))?;
    }
    sqlx::query(&format!(
        "DELETE FROM note_mentions m WHERE m.note_id = $1 AND NOT {kept}",
        kept = still_mentioned(2),
    ))
    .bind(note.0)
    .bind(&ids)
    .execute(&mut *conn)
    .await
    .map_err(db("sync mentions"))?;
    Ok(())
}

/// Every alias, all accounts: the backup's read, on its one snapshot.
pub(crate) async fn aliases_all_on(
    conn: &mut PgConnection,
) -> Result<Vec<common::mention::BackupAlias>, EmberTroveError> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, chrono::DateTime<chrono::Utc>)>(
        "SELECT old_id, person_id, merged_at FROM person_aliases ORDER BY merged_at, old_id",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db("list_all aliases"))?;
    Ok(rows
        .into_iter()
        .map(|(old, person, merged_at)| common::mention::BackupAlias {
            old_id: PersonId(old),
            person_id: PersonId(person),
            merged_at,
        })
        .collect())
}

/// Every stored mention, all accounts: the backup's read, on its one
/// snapshot (`backup::read_snapshot`). A note in the trash is not backed
/// up, so its rows aren't either.
pub(crate) async fn list_all_on(
    conn: &mut PgConnection,
) -> Result<Vec<BackupMention>, EmberTroveError> {
    let rows = sqlx::query_as::<_, (Option<Uuid>, Option<Uuid>, Uuid)>(
        "SELECT node_id, NULL::uuid, person_id FROM node_mentions \
         UNION ALL \
         SELECT NULL::uuid, m.note_id, m.person_id FROM note_mentions m \
         JOIN node_notes nn ON nn.id = m.note_id AND nn.deleted_at IS NULL \
         ORDER BY 1, 2, 3",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db("list_all mentions"))?;
    Ok(rows
        .into_iter()
        .map(|(node, note, person)| BackupMention {
            node_id: node.map(NodeId),
            note_id: note.map(NoteId),
            person_id: PersonId(person),
        })
        .collect())
}

/// Reads of mentions. Each caller has already checked that `reader` may
/// read the text in question; these decide only whom that reader is
/// told the name of.
#[async_trait]
pub trait MentionRepo: Send + Sync {
    /// The people the body of `node` mentions, as `reader` may know
    /// them. Someone not named is simply absent: a forged token, a
    /// deleted person and a person whose owner can no longer open the
    /// node are one answer.
    ///
    /// `in_text`: the ids the body's tokens hold
    /// (`common::mention::mentioned_ids`). A person is answered under
    /// each of those that is theirs: their own id, or the id of someone
    /// merged into them. With none of them in the text, not at all.
    async fn for_node(
        &self,
        node: NodeId,
        reader: &str,
        in_text: &[PersonId],
    ) -> Result<Vec<MentionName>, EmberTroveError>;

    /// The same for each of `notes`, each with the ids its own text
    /// holds. A note in the trash names nobody.
    async fn for_notes(
        &self,
        notes: &[(NoteId, Vec<PersonId>)],
        reader: &str,
    ) -> Result<HashMap<NoteId, Vec<MentionName>>, EmberTroveError>;

    /// "Mentioned in": the nodes that mention `person` in their body or
    /// in a note not in the trash, among those `reader` can open
    /// (`None`: every node, for an admin, who may read them all). No
    /// count of what is left out. The caller has checked `person` is
    /// the caller's own.
    async fn mentioned_in(
        &self,
        person: PersonId,
        reader: Option<&str>,
    ) -> Result<Vec<PersonMention>, EmberTroveError>;
}

/// One answered name: under the id the text's token holds, and for the
/// person's owner alone, where that token's link leads when the id is
/// an alias.
///
/// SECURITY: `links_to` is the person's real id. A reader who is not
/// their owner never gets it: there is no way into another account's
/// People (ONTOLOGY §14).
fn named_as((token, person, display_name, mine): (Uuid, Uuid, String, bool)) -> MentionName {
    MentionName {
        person_id: PersonId(token),
        display_name,
        mine,
        links_to: (mine && token != person).then_some(PersonId(person)),
    }
}

pub struct PgMentionRepo {
    pool: PgPool,
}

impl PgMentionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MentionRepo for PgMentionRepo {
    async fn for_node(
        &self,
        node: NodeId,
        reader: &str,
        in_text: &[PersonId],
    ) -> Result<Vec<MentionName>, EmberTroveError> {
        let held: Vec<Uuid> = in_text.iter().map(|p| p.0).collect();
        let rows = sqlx::query_as::<_, (Uuid, Uuid, String, bool)>(&format!(
            "SELECT t.token_id, p.id, p.display_name, p.owner_id = $2::text \
             FROM node_mentions m \
             JOIN nodes n ON n.id = m.node_id \
             JOIN people p ON p.id = m.person_id \
             JOIN LATERAL ( \
                 SELECT DISTINCT held.id AS token_id FROM unnest($3::uuid[]) AS held(id) \
                 WHERE {theirs} \
             ) t ON true \
             WHERE m.node_id = $1 AND {named} \
             ORDER BY lower(p.display_name), p.id, t.token_id",
            theirs = HELD_IS_THEIRS,
            named = named_to(2),
        ))
        .bind(node.0)
        .bind(reader)
        .bind(&held)
        .fetch_all(&self.pool)
        .await
        .map_err(db("read mentions"))?;
        Ok(rows.into_iter().map(named_as).collect())
    }

    async fn for_notes(
        &self,
        notes: &[(NoteId, Vec<PersonId>)],
        reader: &str,
    ) -> Result<HashMap<NoteId, Vec<MentionName>>, EmberTroveError> {
        let mut named: HashMap<NoteId, Vec<MentionName>> = HashMap::new();
        if notes.is_empty() {
            return Ok(named);
        }
        let ids: Vec<Uuid> = notes.iter().map(|(n, _)| n.0).collect();
        // Which id each note's own text holds, as two arrays side by side.
        let (of_note, held): (Vec<Uuid>, Vec<Uuid>) = notes
            .iter()
            .flat_map(|(note, ids)| ids.iter().map(move |id| (note.0, id.0)))
            .unzip();
        let rows = sqlx::query_as::<_, (Uuid, Uuid, Uuid, String, bool)>(&format!(
            "SELECT m.note_id, t.token_id, p.id, p.display_name, p.owner_id = $2::text \
             FROM note_mentions m \
             JOIN node_notes nn ON nn.id = m.note_id AND nn.deleted_at IS NULL \
             JOIN nodes n ON n.id = nn.node_id \
             JOIN people p ON p.id = m.person_id \
             JOIN LATERAL ( \
                 SELECT DISTINCT held.id AS token_id \
                 FROM unnest($3::uuid[], $4::uuid[]) AS held(note_id, id) \
                 WHERE held.note_id = m.note_id AND {theirs} \
             ) t ON true \
             WHERE m.note_id = ANY($1) AND {named} \
             ORDER BY m.note_id, lower(p.display_name), p.id, t.token_id",
            theirs = HELD_IS_THEIRS,
            named = named_to(2),
        ))
        .bind(&ids)
        .bind(reader)
        .bind(&of_note)
        .bind(&held)
        .fetch_all(&self.pool)
        .await
        .map_err(db("read mentions"))?;
        for (note, token, id, display_name, mine) in rows {
            named
                .entry(NoteId(note))
                .or_default()
                .push(named_as((token, id, display_name, mine)));
        }
        Ok(named)
    }

    async fn mentioned_in(
        &self,
        person: PersonId,
        reader: Option<&str>,
    ) -> Result<Vec<PersonMention>, EmberTroveError> {
        let rows = sqlx::query_as::<_, (Uuid, String, String, bool, i64)>(&format!(
            "WITH hits AS ( \
                 SELECT m.node_id, true AS in_body, 0 AS notes \
                 FROM node_mentions m WHERE m.person_id = $1 \
                 UNION ALL \
                 SELECT nn.node_id, false, 1 \
                 FROM note_mentions m \
                 JOIN node_notes nn ON nn.id = m.note_id AND nn.deleted_at IS NULL \
                 WHERE m.person_id = $1 \
             ) \
             SELECT n.id, n.title, n.node_type::text, bool_or(h.in_body), sum(h.notes) \
             FROM hits h JOIN nodes n ON n.id = h.node_id \
             WHERE {visible} \
             GROUP BY n.id, n.title, n.node_type \
             ORDER BY lower(n.title), n.id",
            visible = visible_to("n", 2),
        ))
        .bind(person.0)
        .bind(reader)
        .fetch_all(&self.pool)
        .await
        .map_err(db("read mentioned-in"))?;
        rows.into_iter()
            .map(|(id, node_title, kind, in_body, notes)| {
                Ok(PersonMention {
                    node_id: NodeId(id),
                    node_title,
                    node_type: super::parse_wire(
                        &kind,
                        "node_type",
                        common::node::NodeType::parse,
                    )?,
                    in_body,
                    notes: u32::try_from(notes).unwrap_or(u32::MAX),
                })
            })
            .collect()
    }
}
