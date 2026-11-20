//! People and memberships (#147).
//!
//! SECURITY: every person query is scoped to `owner_id = $owner`, with no
//! admin bypass: a person's contact details are visible to their owner only.
//! Membership reads are keyed by node; the route decides who may read the
//! node and redacts the result per viewer (`MembershipView::for_viewer`).
//! Every value is a bind parameter.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{MembershipId, NodeId, PersonId},
    node::NodeType,
    person::{
        ContactPoint, DuplicateMatch, Membership, MembershipRecord, Person, PersonInput,
        PersonUsage, RoleCount,
    },
};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

#[async_trait]
pub trait PersonRepo: Send + Sync {
    /// The owner's people, by name. `q` filters on name, company, email and
    /// phone, case-insensitively.
    async fn list(
        &self,
        owner: &str,
        q: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Person>, EmberTroveError>;
    /// NotFound unless `owner` owns it (another owner's id is indistinguishable
    /// from a missing one).
    async fn get(&self, owner: &str, id: PersonId) -> Result<Person, EmberTroveError>;
    /// `AlreadyExists` when the database refuses the save because it would
    /// give the person an address another of the owner's people lists
    /// (S8a, ONTOLOGY §7.1; the trigger of migration 056). The route asks
    /// [`Self::email_collisions`] first, so this is the answer to a race.
    async fn create(&self, owner: &str, input: &PersonInput) -> Result<Person, EmberTroveError>;
    /// As [`Self::create`]; NotFound unless `owner` owns `id`.
    ///
    /// `expected`: the person's `updated_at` as the sender read it (S8c).
    /// When given, the save is applied only if the person is still at
    /// that version; else `PreconditionFailed` and nothing is written.
    /// Without it the save goes through, **unless the person has been
    /// merged into** (they have an alias): a tab from before the merge,
    /// or from before versions were sent, would undo it.
    async fn update(
        &self,
        owner: &str,
        id: PersonId,
        input: &PersonInput,
        expected: Option<DateTime<Utc>>,
    ) -> Result<Person, EmberTroveError>;
    /// What merging `gone` into `kept` would do (S8c): nothing is
    /// written. NotFound unless `owner` owns both.
    async fn merge_plan(
        &self,
        owner: &str,
        kept: PersonId,
        gone: PersonId,
    ) -> Result<common::person_merge::MergePlan, EmberTroveError>;
    /// Merge `gone` into `kept` in one transaction, if `confirm` is the
    /// token of the plan as it is now (`person_merge::merge`).
    async fn merge(
        &self,
        owner: &str,
        kept: PersonId,
        gone: PersonId,
        confirm: &str,
    ) -> Result<Person, EmberTroveError>;
    async fn delete(&self, owner: &str, id: PersonId) -> Result<(), EmberTroveError>;
    /// The owner's **other** people whom a save of `id` (`None`: a person
    /// not yet made) with `emails` would collide with: those who list an
    /// address that `emails` holds and the stored person does not. An
    /// address the person already has is never a collision, so people who
    /// shared one before the rule stay editable (owner decision,
    /// 2026-11-18). The database decides with the same function
    /// (`person_email_collisions`); this read is for saying who.
    ///
    /// SECURITY: scoped to `owner`, with no admin bypass. An `id` that is
    /// not the owner's finds nothing, so the save that follows answers
    /// 404 as it would for any id.
    async fn email_collisions(
        &self,
        owner: &str,
        id: Option<PersonId>,
        emails: &[ContactPoint],
    ) -> Result<Vec<DuplicateMatch>, EmberTroveError>;
    /// "Used in": the nodes this person appears in, with their role there.
    async fn usages(&self, owner: &str, id: PersonId) -> Result<Vec<PersonUsage>, EmberTroveError>;
    /// The roles the owner already uses, most used first (suggestions).
    async fn role_counts(&self, owner: &str) -> Result<Vec<RoleCount>, EmberTroveError>;
    /// A node's memberships with their people. Unredacted: the route
    /// redacts per viewer.
    async fn memberships_for_node(&self, node: NodeId) -> Result<Vec<Membership>, EmberTroveError>;
    /// AlreadyExists if the person is already on the node.
    async fn add_membership(
        &self,
        node: NodeId,
        person: PersonId,
        role: Option<&str>,
        note: Option<&str>,
    ) -> Result<MembershipId, EmberTroveError>;
    /// NotFound unless the membership is on `node`.
    async fn update_membership(
        &self,
        node: NodeId,
        id: MembershipId,
        role: Option<&str>,
        note: Option<&str>,
    ) -> Result<(), EmberTroveError>;
    /// NotFound unless the membership is on `node`. Never deletes the person.
    async fn remove_membership(
        &self,
        node: NodeId,
        id: MembershipId,
    ) -> Result<(), EmberTroveError>;
    /// Everything, for a full (admin) backup.
    async fn list_all(&self) -> Result<(Vec<Person>, Vec<MembershipRecord>), EmberTroveError>;
}

pub struct PgPersonRepo {
    pool: PgPool,
}

impl PgPersonRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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

fn internal(what: &str) -> impl Fn(sqlx::Error) -> EmberTroveError + '_ {
    move |e| EmberTroveError::Internal(format!("{what} failed: {e}"))
}

/// What a save is told when the database refuses it for the email rule.
/// It names nothing: the route reads who has the address separately.
pub(crate) const EMAIL_TAKEN: &str = "an email address already belongs to another person";

/// A failed person write. The email rule's refusal (migration 056's
/// trigger, raised under the constraint name `people_email_identity`) is
/// `AlreadyExists`; anything else is internal.
///
/// SECURITY: the driver's error is formatted with `Display`, which is the
/// message alone. Never `{e:?}` or the error's detail: a detail can quote
/// a row.
fn save_failed(what: &'static str) -> impl Fn(sqlx::Error) -> EmberTroveError {
    move |e| match &e {
        sqlx::Error::Database(db) if db.constraint() == Some("people_email_identity") => {
            EmberTroveError::AlreadyExists(EMAIL_TAKEN.to_string())
        }
        _ => EmberTroveError::Internal(format!("{what} failed: {e}")),
    }
}

/// What a save is told when the person is no longer at the version the
/// sender read.
pub(crate) const PERSON_CHANGED: &str =
    "this person changed since you opened them: reload to see how they are now";

fn person_not_found(id: PersonId) -> EmberTroveError {
    EmberTroveError::NotFound(format!("person {id} not found"))
}

/// Every person and membership, all owners. A free function so the backup can run it on its one
/// snapshot transaction (`backup::read_snapshot`).
pub(crate) async fn list_all_on(
    conn: &mut sqlx::PgConnection,
) -> Result<(Vec<Person>, Vec<MembershipRecord>), EmberTroveError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        node_id: Uuid,
        person_id: Uuid,
        role: Option<String>,
        note: Option<String>,
        created_at: DateTime<Utc>,
    }
    let people = sqlx::query_as::<_, PersonRow>(&format!(
        "SELECT {PERSON_COLS} FROM people ORDER BY created_at, id"
    ))
    .fetch_all(&mut *conn)
    .await
    .map_err(internal("list all people"))?;
    let members = sqlx::query_as::<_, Row>(
        "SELECT id, node_id, person_id, role, note, created_at FROM node_people \
         ORDER BY created_at, id",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(internal("list all memberships"))?;
    Ok((
        people.into_iter().map(Person::from).collect(),
        members
            .into_iter()
            .map(|r| MembershipRecord {
                id: MembershipId(r.id),
                node_id: NodeId(r.node_id),
                person_id: PersonId(r.person_id),
                role: r.role,
                note: r.note,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

#[async_trait]
impl PersonRepo for PgPersonRepo {
    async fn list(
        &self,
        owner: &str,
        q: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Person>, EmberTroveError> {
        let pattern = q
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| super::escape_like(&q.to_lowercase()));
        let rows = sqlx::query_as::<_, PersonRow>(&format!(
            r#"
            SELECT {PERSON_COLS} FROM people
            WHERE owner_id = $1
              AND ($2::text IS NULL
                   OR lower(display_name) LIKE ('%' || $2 || '%') ESCAPE '\'
                   OR lower(coalesce(company, '')) LIKE ('%' || $2 || '%') ESCAPE '\'
                   OR lower(emails::text) LIKE ('%' || $2 || '%') ESCAPE '\'
                   OR lower(phones::text) LIKE ('%' || $2 || '%') ESCAPE '\')
            ORDER BY lower(display_name), id
            LIMIT $3
            "#
        ))
        .bind(owner)
        .bind(pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(internal("list people"))?;
        Ok(rows.into_iter().map(Person::from).collect())
    }

    async fn get(&self, owner: &str, id: PersonId) -> Result<Person, EmberTroveError> {
        sqlx::query_as::<_, PersonRow>(&format!(
            "SELECT {PERSON_COLS} FROM people WHERE id = $1 AND owner_id = $2"
        ))
        .bind(id.0)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal("get person"))?
        .map(Person::from)
        .ok_or_else(|| person_not_found(id))
    }

    async fn create(&self, owner: &str, p: &PersonInput) -> Result<Person, EmberTroveError> {
        let row = sqlx::query_as::<_, PersonRow>(&format!(
            r#"
            INSERT INTO people (owner_id, display_name, company, position, emails, phones,
                                addresses, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING {PERSON_COLS}
            "#
        ))
        .bind(owner)
        .bind(&p.display_name)
        .bind(&p.company)
        .bind(&p.position)
        .bind(Json(&p.emails))
        .bind(Json(&p.phones))
        .bind(Json(&p.addresses))
        .bind(&p.notes)
        .fetch_one(&self.pool)
        .await
        .map_err(save_failed("create person"))?;
        Ok(row.into())
    }

    async fn update(
        &self,
        owner: &str,
        id: PersonId,
        p: &PersonInput,
        expected: Option<DateTime<Utc>>,
    ) -> Result<Person, EmberTroveError> {
        // The row is held first, by a statement of its own. A save that
        // meets a merge under way waits here, and the write below then
        // reads the alias table as the merge left it. In one statement
        // the wait comes after that table was read: a save with no
        // version would pass, and write the card from before the merge
        // over its result.
        //
        // SECURITY: by id **and owner**, so another account's person is
        // never locked or waited on.
        let mut tx = self.pool.begin().await.map_err(internal("update person"))?;
        sqlx::query("SELECT 1 FROM people WHERE id = $1 AND owner_id = $2 FOR NO KEY UPDATE")
            .bind(id.0)
            .bind(owner)
            .execute(&mut *tx)
            .await
            .map_err(internal("update person"))?;
        // The version is checked in the statement that writes, so a save
        // can't slip in between a check and the write. A save that sends
        // none passes, except over a person someone was merged into.
        let saved = sqlx::query_as::<_, PersonRow>(&format!(
            r#"
            UPDATE people SET
                display_name = $3, company = $4, position = $5, emails = $6, phones = $7,
                addresses = $8, notes = $9
            WHERE id = $1 AND owner_id = $2
              AND CASE WHEN $10::timestamptz IS NULL
                       THEN NOT EXISTS (SELECT 1 FROM person_aliases a WHERE a.person_id = $1)
                       ELSE updated_at = $10 END
            RETURNING {PERSON_COLS}
            "#
        ))
        .bind(id.0)
        .bind(owner)
        .bind(&p.display_name)
        .bind(&p.company)
        .bind(&p.position)
        .bind(Json(&p.emails))
        .bind(Json(&p.phones))
        .bind(Json(&p.addresses))
        .bind(&p.notes)
        .bind(expected)
        .fetch_optional(&mut *tx)
        .await
        .map_err(save_failed("update person"))?;
        tx.commit().await.map_err(internal("update person"))?;
        match saved {
            Some(row) => Ok(row.into()),
            // No row: not the owner's (or not there), or not at that
            // version. Told apart only for the owner.
            None => {
                self.get(owner, id).await?;
                Err(EmberTroveError::PreconditionFailed(
                    PERSON_CHANGED.to_string(),
                ))
            }
        }
    }

    async fn merge_plan(
        &self,
        owner: &str,
        kept: PersonId,
        gone: PersonId,
    ) -> Result<common::person_merge::MergePlan, EmberTroveError> {
        super::person_merge::merge_plan(&self.pool, owner, kept, gone).await
    }

    async fn merge(
        &self,
        owner: &str,
        kept: PersonId,
        gone: PersonId,
        confirm: &str,
    ) -> Result<Person, EmberTroveError> {
        super::person_merge::merge(&self.pool, owner, kept, gone, confirm).await
    }

    async fn delete(&self, owner: &str, id: PersonId) -> Result<(), EmberTroveError> {
        let done = sqlx::query("DELETE FROM people WHERE id = $1 AND owner_id = $2")
            .bind(id.0)
            .bind(owner)
            .execute(&self.pool)
            .await
            .map_err(internal("delete person"))?;
        if done.rows_affected() == 0 {
            return Err(person_not_found(id));
        }
        Ok(())
    }

    async fn email_collisions(
        &self,
        owner: &str,
        id: Option<PersonId>,
        emails: &[ContactPoint],
    ) -> Result<Vec<DuplicateMatch>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            display_name: String,
            email: String,
        }
        if emails.is_empty() {
            return Ok(Vec::new());
        }
        // The stored list is read here, in the statement, and only for a
        // person the owner has: `$2` is NULL for a new one. The last line
        // keeps another account's id from finding anything.
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT c.person_id AS id, p.display_name, c.email
            FROM person_email_collisions(
                     $1, $2::uuid,
                     COALESCE((SELECT emails FROM people WHERE id = $2 AND owner_id = $1),
                              '[]'::jsonb),
                     $3::jsonb) c
            JOIN people p ON p.id = c.person_id
            WHERE $2::uuid IS NULL
               OR EXISTS (SELECT 1 FROM people WHERE id = $2 AND owner_id = $1)
            ORDER BY lower(p.display_name), p.id, c.email
            "#,
        )
        .bind(owner)
        .bind(id.map(|id| id.0))
        .bind(Json(emails))
        .fetch_all(&self.pool)
        .await
        .map_err(internal("read email collisions"))?;
        Ok(rows
            .into_iter()
            .map(|r| DuplicateMatch {
                id: PersonId(r.id),
                display_name: r.display_name,
                email: r.email,
            })
            .collect())
    }

    async fn usages(&self, owner: &str, id: PersonId) -> Result<Vec<PersonUsage>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            node_id: Uuid,
            title: String,
            node_type: String,
            role: Option<String>,
        }
        // Ownership first, so another owner's id is a 404, not an empty list.
        self.get(owner, id).await?;
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT m.id, n.id AS node_id, n.title, n.node_type::text AS node_type, m.role
            FROM node_people m
            JOIN people p ON p.id = m.person_id
            JOIN nodes n ON n.id = m.node_id
            WHERE m.person_id = $1 AND p.owner_id = $2
            ORDER BY lower(n.title), n.id
            "#,
        )
        .bind(id.0)
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .map_err(internal("person usages"))?;
        rows.into_iter()
            .map(|r| {
                Ok(PersonUsage {
                    membership_id: MembershipId(r.id),
                    node_id: NodeId(r.node_id),
                    node_title: r.title,
                    node_type: super::parse_wire(&r.node_type, "node_type", NodeType::parse)?,
                    role: r.role,
                })
            })
            .collect()
    }

    async fn role_counts(&self, owner: &str) -> Result<Vec<RoleCount>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            role: String,
            count: i64,
        }
        // Case variants are one role, spelled the most-used way.
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT (array_agg(role ORDER BY c DESC, role))[1] AS role, SUM(c)::bigint AS count
            FROM (
                SELECT m.role, COUNT(*) AS c
                FROM node_people m JOIN people p ON p.id = m.person_id
                WHERE p.owner_id = $1 AND m.role IS NOT NULL
                GROUP BY m.role
            ) spelled
            GROUP BY lower(role)
            ORDER BY count DESC, role
            LIMIT 50
            "#,
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .map_err(internal("role counts"))?;
        Ok(rows
            .into_iter()
            .map(|r| RoleCount {
                role: r.role,
                count: u32::try_from(r.count).unwrap_or(u32::MAX),
            })
            .collect())
    }

    async fn memberships_for_node(&self, node: NodeId) -> Result<Vec<Membership>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            m_id: Uuid,
            m_node_id: Uuid,
            m_role: Option<String>,
            m_note: Option<String>,
            m_created_at: DateTime<Utc>,
            #[sqlx(flatten)]
            person: PersonRow,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT m.id AS m_id, m.node_id AS m_node_id, m.role AS m_role, m.note AS m_note,
                   m.created_at AS m_created_at,
                   p.id, p.owner_id, p.display_name, p.company, p.position, p.emails,
                   p.phones, p.addresses, p.notes, p.created_at, p.updated_at
            FROM node_people m JOIN people p ON p.id = m.person_id
            WHERE m.node_id = $1
            ORDER BY lower(p.display_name), m.id
            "#,
        )
        .bind(node.0)
        .fetch_all(&self.pool)
        .await
        .map_err(internal("node memberships"))?;
        Ok(rows
            .into_iter()
            .map(|r| Membership {
                id: MembershipId(r.m_id),
                node_id: NodeId(r.m_node_id),
                person: r.person.into(),
                role: r.m_role,
                note: r.m_note,
                created_at: r.m_created_at,
            })
            .collect())
    }

    async fn add_membership(
        &self,
        node: NodeId,
        person: PersonId,
        role: Option<&str>,
        note: Option<&str>,
    ) -> Result<MembershipId, EmberTroveError> {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO node_people (node_id, person_id, role, note) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(node.0)
        .bind(person.0)
        .bind(role)
        .bind(note)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.constraint() == Some("node_people_unique") => {
                EmberTroveError::AlreadyExists("this person is already on this node".into())
            }
            _ => EmberTroveError::Internal(format!("add membership failed: {e}")),
        })?;
        Ok(MembershipId(id))
    }

    async fn update_membership(
        &self,
        node: NodeId,
        id: MembershipId,
        role: Option<&str>,
        note: Option<&str>,
    ) -> Result<(), EmberTroveError> {
        let done = sqlx::query(
            "UPDATE node_people SET role = $3, note = $4 WHERE id = $1 AND node_id = $2",
        )
        .bind(id.0)
        .bind(node.0)
        .bind(role)
        .bind(note)
        .execute(&self.pool)
        .await
        .map_err(internal("update membership"))?;
        if done.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!(
                "membership {id} not found"
            )));
        }
        Ok(())
    }

    async fn remove_membership(
        &self,
        node: NodeId,
        id: MembershipId,
    ) -> Result<(), EmberTroveError> {
        let done = sqlx::query("DELETE FROM node_people WHERE id = $1 AND node_id = $2")
            .bind(id.0)
            .bind(node.0)
            .execute(&self.pool)
            .await
            .map_err(internal("remove membership"))?;
        if done.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!(
                "membership {id} not found"
            )));
        }
        Ok(())
    }

    async fn list_all(&self) -> Result<(Vec<Person>, Vec<MembershipRecord>), EmberTroveError> {
        let mut conn =
            self.pool.acquire().await.map_err(|e| {
                EmberTroveError::Internal(format!("acquire connection failed: {e}"))
            })?;
        list_all_on(&mut conn).await
    }
}
