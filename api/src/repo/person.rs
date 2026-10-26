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
    async fn create(&self, owner: &str, input: &PersonInput) -> Result<Person, EmberTroveError>;
    async fn update(
        &self,
        owner: &str,
        id: PersonId,
        input: &PersonInput,
    ) -> Result<Person, EmberTroveError>;
    async fn delete(&self, owner: &str, id: PersonId) -> Result<(), EmberTroveError>;
    /// The owner's people with an email in `lowered` (the duplicate warning).
    async fn find_by_emails(
        &self,
        owner: &str,
        lowered: &[String],
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
    address_label: Option<String>,
    address: Option<String>,
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
            address_label: r.address_label,
            address: r.address,
            notes: r.notes,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const PERSON_COLS: &str = "id, owner_id, display_name, company, position, emails, phones, \
     address_label, address, notes, created_at, updated_at";

fn internal(what: &str) -> impl Fn(sqlx::Error) -> EmberTroveError + '_ {
    move |e| EmberTroveError::Internal(format!("{what} failed: {e}"))
}

fn person_not_found(id: PersonId) -> EmberTroveError {
    EmberTroveError::NotFound(format!("person {id} not found"))
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
                                address_label, address, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING {PERSON_COLS}
            "#
        ))
        .bind(owner)
        .bind(&p.display_name)
        .bind(&p.company)
        .bind(&p.position)
        .bind(Json(&p.emails))
        .bind(Json(&p.phones))
        .bind(&p.address_label)
        .bind(&p.address)
        .bind(&p.notes)
        .fetch_one(&self.pool)
        .await
        .map_err(internal("create person"))?;
        Ok(row.into())
    }

    async fn update(
        &self,
        owner: &str,
        id: PersonId,
        p: &PersonInput,
    ) -> Result<Person, EmberTroveError> {
        sqlx::query_as::<_, PersonRow>(&format!(
            r#"
            UPDATE people SET
                display_name = $3, company = $4, position = $5, emails = $6, phones = $7,
                address_label = $8, address = $9, notes = $10
            WHERE id = $1 AND owner_id = $2
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
        .bind(&p.address_label)
        .bind(&p.address)
        .bind(&p.notes)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal("update person"))?
        .map(Person::from)
        .ok_or_else(|| person_not_found(id))
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

    async fn find_by_emails(
        &self,
        owner: &str,
        lowered: &[String],
    ) -> Result<Vec<DuplicateMatch>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            display_name: String,
            email: String,
        }
        if lowered.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT p.id, p.display_name, e->>'value' AS email
            FROM people p, jsonb_array_elements(p.emails) e
            WHERE p.owner_id = $1 AND lower(e->>'value') = ANY($2)
            ORDER BY lower(p.display_name), p.id
            "#,
        )
        .bind(owner)
        .bind(lowered)
        .fetch_all(&self.pool)
        .await
        .map_err(internal("find people by email"))?;
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
                   p.phones, p.address_label, p.address, p.notes, p.created_at, p.updated_at
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
        .fetch_all(&self.pool)
        .await
        .map_err(internal("list all people"))?;
        let members = sqlx::query_as::<_, Row>(
            "SELECT id, node_id, person_id, role, note, created_at FROM node_people \
             ORDER BY created_at, id",
        )
        .fetch_all(&self.pool)
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
}
