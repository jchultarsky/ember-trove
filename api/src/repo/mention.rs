//! Mentions of People in text (S9, #360; ONTOLOGY §7.3, §14).
//!
//! `common::mention` reads the syntax. This module holds the two rules
//! that make a mention safe, each in one place:
//!
//! 1. **Who may make one** ([`sync_body_on`], [`sync_note_on`]). A row
//!    is written only for a person who belongs to the account saving the
//!    text. A token typed or pasted with someone else's id gets no row,
//!    ever, and without a row nobody is named. A person's id is not a
//!    secret (a sharee sees it in a shared Project's People list, and in
//!    the raw text), so this is what keeps an id from being a way to ask
//!    for a name.
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

use std::collections::HashMap;

use async_trait::async_trait;
use common::EmberTroveError;
use common::id::{NodeId, NoteId, PersonId};
use common::mention::{BackupMention, MentionName, PersonMention, mentioned_ids};
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

/// Bring a node's mention rows in line with its `body`, as saved by
/// `saver`. Runs on the connection of the transaction that wrote the
/// body, so the text and its rows can't part (a row left behind by a
/// failed sync would name a person their owner had taken out).
///
/// - A row whose token left the text is deleted.
/// - A row whose token is still there stays, whoever made it: a
///   co-editor's save keeps the owner's mentions.
/// - A row is **made** only for a person of `saver`'s own.
pub(crate) async fn sync_body_on(
    conn: &mut PgConnection,
    node: NodeId,
    body: &str,
    saver: &str,
) -> Result<(), EmberTroveError> {
    let ids: Vec<Uuid> = mentioned_ids(body).into_iter().map(|p| p.0).collect();
    sqlx::query("DELETE FROM node_mentions WHERE node_id = $1 AND NOT (person_id = ANY($2))")
        .bind(node.0)
        .bind(&ids)
        .execute(&mut *conn)
        .await
        .map_err(db("sync mentions"))?;
    if ids.is_empty() {
        return Ok(());
    }
    // One statement, so an id of another account's person and an id of
    // nobody leave the same trace: none. `FOR KEY SHARE` holds the
    // people read until this transaction ends; without it a person
    // deleted between the read and the insert would fail the foreign
    // key and take the body's save down with it.
    sqlx::query(
        "INSERT INTO node_mentions (node_id, person_id) \
         SELECT $1, p.id FROM people p \
         WHERE p.id = ANY($2) AND p.owner_id = $3 \
         FOR KEY SHARE OF p \
         ON CONFLICT DO NOTHING",
    )
    .bind(node.0)
    .bind(&ids)
    .bind(saver)
    .execute(&mut *conn)
    .await
    .map_err(db("sync mentions"))?;
    Ok(())
}

/// The same for a note, whose only writer is its `author`.
pub(crate) async fn sync_note_on(
    conn: &mut PgConnection,
    note: NoteId,
    body: &str,
    author: &str,
) -> Result<(), EmberTroveError> {
    let ids: Vec<Uuid> = mentioned_ids(body).into_iter().map(|p| p.0).collect();
    sqlx::query("DELETE FROM note_mentions WHERE note_id = $1 AND NOT (person_id = ANY($2))")
        .bind(note.0)
        .bind(&ids)
        .execute(&mut *conn)
        .await
        .map_err(db("sync mentions"))?;
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO note_mentions (note_id, person_id) \
         SELECT $1, p.id FROM people p \
         WHERE p.id = ANY($2) AND p.owner_id = $3 \
         FOR KEY SHARE OF p \
         ON CONFLICT DO NOTHING",
    )
    .bind(note.0)
    .bind(&ids)
    .bind(author)
    .execute(&mut *conn)
    .await
    .map_err(db("sync mentions"))?;
    Ok(())
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
    async fn for_node(
        &self,
        node: NodeId,
        reader: &str,
    ) -> Result<Vec<MentionName>, EmberTroveError>;

    /// The same for each of `notes`. A note in the trash names nobody.
    async fn for_notes(
        &self,
        notes: &[NoteId],
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
    ) -> Result<Vec<MentionName>, EmberTroveError> {
        let rows = sqlx::query_as::<_, (Uuid, String, bool)>(&format!(
            "SELECT p.id, p.display_name, p.owner_id = $2::text \
             FROM node_mentions m \
             JOIN nodes n ON n.id = m.node_id \
             JOIN people p ON p.id = m.person_id \
             WHERE m.node_id = $1 AND {named} \
             ORDER BY lower(p.display_name), p.id",
            named = named_to(2),
        ))
        .bind(node.0)
        .bind(reader)
        .fetch_all(&self.pool)
        .await
        .map_err(db("read mentions"))?;
        Ok(rows
            .into_iter()
            .map(|(id, display_name, mine)| MentionName {
                person_id: PersonId(id),
                display_name,
                mine,
            })
            .collect())
    }

    async fn for_notes(
        &self,
        notes: &[NoteId],
        reader: &str,
    ) -> Result<HashMap<NoteId, Vec<MentionName>>, EmberTroveError> {
        let mut named: HashMap<NoteId, Vec<MentionName>> = HashMap::new();
        if notes.is_empty() {
            return Ok(named);
        }
        let ids: Vec<Uuid> = notes.iter().map(|n| n.0).collect();
        let rows = sqlx::query_as::<_, (Uuid, Uuid, String, bool)>(&format!(
            "SELECT m.note_id, p.id, p.display_name, p.owner_id = $2::text \
             FROM note_mentions m \
             JOIN node_notes nn ON nn.id = m.note_id AND nn.deleted_at IS NULL \
             JOIN nodes n ON n.id = nn.node_id \
             JOIN people p ON p.id = m.person_id \
             WHERE m.note_id = ANY($1) AND {named} \
             ORDER BY m.note_id, lower(p.display_name), p.id",
            named = named_to(2),
        ))
        .bind(&ids)
        .bind(reader)
        .fetch_all(&self.pool)
        .await
        .map_err(db("read mentions"))?;
        for (note, id, display_name, mine) in rows {
            named.entry(NoteId(note)).or_default().push(MentionName {
                person_id: PersonId(id),
                display_name,
                mine,
            });
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
