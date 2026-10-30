# Ember Trove — Data Model Ontology

This document defines what the things in Ember Trove *are*: the kinds of
objects, what each kind may carry, how they relate, and what the relations
mean. It is the durable record of the ontology working session (October 2026).

The ontology is normative for the data model. Where the current
implementation differs, the implementation is what changes — follow-on work
is filed as ordinary issues derived from this document. Existing data that
violates the ontology is a cleanup case, not counter-evidence against the
model: **the ontology judges the data, not the reverse.** A re-typing /
cleanup pass over existing content follows the ontology; it does not precede
or shape it.

## 1. Philosophy

- **One pane of glass for one life.** Trove does not divide its owner's life
  into separate work and personal worlds. It is one person's view of the
  whole world they have to live in and navigate — one person, continually
  switching attention.
- **The purpose is reduced cognitive load.** Information is stored in a
  familiar way and must be ergonomic and intuitive to retrieve at the moment
  it is needed, with enough context preserved to switch attention quickly
  between many concurrent concerns.
- **Structure must remain revisable.** The model cannot be too rigid:
  knowledge is incomplete and uncertain, and understanding evolves. Kinds are
  real and useful without becoming traps.
- **Flexibility lives in the body.** Every document-like object carries a
  free-Markdown body at its core — the catch-all for whatever the model
  cannot predict. Rigidity is avoided by the body's openness, not by
  weakening the kinds.
- **One fact, one home.** Derived things (a birthday, a rendered due date)
  are computed and displayed, never stored a second time somewhere else.

Vocabulary provenance: several kind names are borrowed from GTD (David
Allen) and PARA (Tiago Forte). Neither source keeps "Resource" and
"Reference" as separate kinds — Trove inherited both words, which is why
their boundary was blurry; §3 resolves that by merging them.

## 2. The kinds of things, at a glance

Trove holds four families of objects:

| Family | Members | Nature |
|---|---|---|
| **Documents** (nodes) | Project, Area, Article, Reference | Markdown-bodied, titled, linked in the graph |
| **Registry entities** | Person, Company | Records about the world, not documents; fields + free Markdown |
| **Actions** | Task | A single completable action, owned by a Project |
| **Occurrences** | Event | Something that happens; known about, not completable |

Documents, People, Companies, and Tasks all appear as vertices in one mixed
graph (§6). Person and Company are *entities*, not node kinds — a document
is something you write; an entity is something you keep a record of.

## 3. Node kinds

A node's type is a **real kind**, not a lens or a template. Each kind has a
defined set of sections and features, and the application gates features by
kind.

### 3.1 The four kinds

**Project** — the distinct kind. A Project tracks goal-directed work that
needs more than a single task, aimed at a specific outcome (not only
software work: a house renovation is a Project). It is the most-used kind
for recording current activity, and the only kind with **tasks, people, and
status**. A Project assembles documentation, sources, people, events, a
plan, links, status, and history so that its full context is available at a
glance. Its anatomy is defined in §4.

**Area** — the largest scope: a long-lived life domain (a family
responsibility, a career, a customer engagement). An Area is a standard to
maintain, not an outcome to complete; it has no tasks, people, or status of
its own. Areas may nest — a broad Area can hold sub-Areas as well as
Projects, Articles, and References.

**Article** — a long-form authored document. No people, no tasks. An Article
cites the resources used to write it (§6, `references`); once complete it
stands alone as reading material — an accumulation of knowledge. An Article
may be the result of a Project, but it does not belong to one.

**Reference** — material that is not meant to stand alone: context and
information kept for use by a Project or an Article. (This kind merges the
former Resource and Reference types; the surviving name is Reference.) A
Reference may hold material directly or be a *pointer* to an outside source
— kept-vs-pointer is not a kind boundary.

### 3.2 Feature gates

| Capability | Project | Area | Article | Reference |
|---|---|---|---|---|
| Free-Markdown body | ✓ | ✓ | ✓ | ✓ |
| External Links (§5) | ✓ | ✓ | ✓ | ✓ |
| History (§10) | ✓ | ✓ | ✓ | ✓ |
| Tasks | ✓ | — | — | — |
| People (membership) | ✓ | — | — | — |
| Status (`project_status`) | ✓ | — | — | — |
| Project Plan (§10) | ✓ | — | — | — |

### 3.3 Rules that apply to all kinds

- **References are shared, not owned.** One Reference can be referred to by
  many Projects and Articles. The link to a Reference is never containment
  or ownership (§6).
- **Type permanence.** A node's type, once assigned, stays. There is no
  in-place mutation (a Reference does not become a Project). Flexibility
  comes from the free-Markdown body, not from reclassifying kinds.
- **Single home.** A node has at most one `contains` parent (§6). A Project
  in particular has exactly one Area home, never shared across Areas; an
  Area holds many Projects.
- **Lifecycle.** Draft vs Published is a label only (finished vs not — most
  meaningful for Articles). Archived is the one lifecycle state with an
  effect: an archived node is hidden from lists and navigation.

## 4. Anatomy of a Project

A Project assembles nine elements:

1. **Body** — free Markdown documentation; the catch-all, as with every
   kind.
2. **Status** — a short structured status token (`project_status`). Its
   values are **open**: the possible values cannot be known ahead of time,
   so the picker suggests values already in use rather than enforcing a
   closed enum. Narrative status stays in the body or notes. Status is
   distinct from the Draft/Published/Archived lifecycle.
3. **Tasks** — owned by the Project (§9).
4. **People** — memberships carrying a Role and an optional context note
   (§7). Text may additionally *mention* People (§7.3).
5. **History** — the Project's instance of the general History facet (§10):
   Project notes, occurred events, and status changes, in one evolving
   chronology.
6. **Project Plan** — the forward-looking plan (§10). Project-only.
7. **Sources** — two halves: outgoing `references` edges to material inside
   Trove, and the Project's own External Links to material outside Trove
   (§5).
8. **Position and relations** — exactly one Area home via `contains`;
   `derived_from`, `depends_on`, `related_to`, and wiki-links to other
   nodes (§6).
9. **The goal** — the outcome the Project exists to achieve, stated in the
   body. There is deliberately no separate goal field.

### 4.1 Completion is declared, not derived

A Project is complete when its status **declares** it — never computed from
the state of its Tasks or sub-Projects. A master Project may be suddenly
cancelled (for example, a customer withdraws funding); that decision does
not depend on the state of anything beneath it.

- A Project can itself be **Cancelled** — terminal and declared, parallel
  to a Task's Cancelled state (§9).
- The declaration is made independently of the children (never derived
  *upward*), and once declared it **cascades *downward***: sub-Projects take
  the same terminal state (Complete or Cancelled as the master), and their
  open or Waiting Tasks close correspondingly (Done under completion,
  Cancelled under cancellation), transitively down the `contains` tree.
- A `depends_on` edge targeting a Project is satisfied exactly when that
  Project's status declares completion. A cancelled target can never
  satisfy a dependency.

### 4.2 Worked rule: hubs are Areas

A long-lived engagement hub — the classic case is a customer relationship
spanning many initiatives over years — is an **Area**, with a **Company**
entity behind it (§8), not a Project. The test: if the thing does not
complete when any one initiative completes, if its "goal" is a standard to
maintain ("keep the relationship healthy") rather than an outcome, and if
it accumulates sibling initiatives, it is an Area. Initiatives under it are
Projects, homed in the Area via `contains`; material they share is held in
References. A genuine Project→Project `contains` exists only for
decomposition — a parent Project that completes when its sub-Projects do.

## 5. External Links

Every node carries **External Links**: a first-class list of named URLs
(name + URL) attached to the node, pointing outside Trove.

- **The label is essential, not decoration.** A URL alone is not clear
  enough about what it represents; the name says what the link *is*. Both
  name and URL are required.
- External Links are distinct from links pasted into body prose. A bare URL
  in the body is just a prose link; an External Link is a structured,
  findable part of the node.
- Together with `references` edges, External Links form a node's Sources
  (§4): edges reach material inside Trove, External Links reach material
  outside it.
- A Reference that points at an outside source (a pointer-Reference, §3.1)
  can carry that URL in its own External Links rather than in its body.
- The Company entity's key-links list (§8) is the entity analogue of this
  same pattern.

## 6. Edges and the graph

The graph is **mixed**: nodes, People, Companies, and Tasks are vertices,
connected by typed, directed edges (people→projects, people→companies,
tasks→projects, and node→node). Direction is real for most types; membership
(§7) remains the stored person↔project relation and is *projected* into the
graph, as is a Project's ownership of its Tasks.

Edge types acquire semantics **one at a time, by explicit decision** — a
type means exactly what this section says, no more and no less; behavior may
branch on edge type only where a meaning is settled here.

### 6.1 `contains` — "has its home in"

Directional structural belonging: the child is part of the parent's scope,
and the parent is where the child is found.

- At most **one** `contains` parent per node — a home is singular. Other
  pointers to the same node use other edge types.
- **No cycles**: the structure is a forest.
- A **Reference is never a `contains` child** — it is shared, and reached by
  links.
- `contains` governs structure, navigation, and filing only. Deleting a
  container does **not** cascade-delete its children. (Project completion
  and cancellation *do* cascade state downward — §4.1 — which is a state
  rule, not a deletion rule.)

### 6.2 `related_to` — bidirectional traversal, nothing more

The one symmetric type. Its entire meaning is that the graph can be
traversed in either direction between the two things. There is no other
meaning to it.

- Stored **once, unordered**: creating B→A when A→B exists is the same edge,
  not a second one.

### 6.3 `references` — "A draws on B as source material"

Directional: a Project uses a Reference; an Article cites the sources it
was written from. Direction runs from the consumer to the thing consumed.

- The canonical target is a Reference — this edge is what makes References
  shared rather than contained — but the target kind is not hard-restricted:
  any node can be referenced when genuinely used as source material.
- Ongoing *use*, not origin (that is `derived_from`); informational, not
  blocking (if A cannot proceed without B, that is `depends_on`).
- Behavior, deliberately modest: a reverse **"Used in"** index on the
  target; an Article's outgoing `references` edges are its **bibliography**
  (distinct from incidental `[[title]]` links in its prose); and a
  **warning, not a block**, when deleting or archiving a node that active
  nodes still reference.

### 6.4 `depends_on` — hard prerequisite

Directional and strict: "A cannot proceed or complete until B is done or
available." The strongest claim in the vocabulary; merely-useful connections
are `references` or `related_to`.

- Behavior is **surfacing, never enforcement**: unsatisfied dependencies
  are displayed as blockers (including transitively, display-only). Trove
  never prevents work, reorders anything, or propagates status changes on
  its own.
- **Cycles are rejected**: a dependency cycle is a deadlock — a modeling
  error, not a storable state.
- A dependency on a Project is satisfied when the target's status declares
  completion (§4.1). Task-to-task dependencies mirror this definition (§9).

### 6.5 `derived_from` — provenance

Directional: "A came from B" — adapted, copied, split, or spun off.
Direction runs descendant → origin.

- A **historical fact, not a current state**: once true, it stays true
  whatever happens to either node afterward.
- **No ongoing coupling**: no content sync, no updates flowing from origin
  to descendant, ever. The descendant is independent from birth (contrast
  `references`, continuing use).
- **Not versioning**: versions of the *same* node live in that node's
  version history; `derived_from` is strictly between two different nodes.
- Multiple origins are allowed (merges), and one origin may spawn many
  descendants — no single-parent rule. **Cycles are rejected** (an origin
  must precede its descendant).
- Behavior: **lineage display only** ("Derived from X"; a "Descendants"
  list on the origin).

### 6.6 `wiki_link` — "mentioned in my text"

The one **machine-owned** type: never picked from an edge form, never
created or deleted by hand. Sync keeps it exactly mirroring `[[Title]]`
occurrences in bodies — remove the `[[…]]` and the edge goes away.

- Direction: mentioning node → mentioned node. Title matching is
  case-insensitive; `[[Title|display text]]` is supported.
- Weaker than, and different in kind from, `references`: an in-prose
  mention in context, not a declared source. Both may coexist on a pair.
- **It may dangle**: a `[[Title]]` with no matching node is meaningful —
  prose running ahead of structure — and renders distinctly as unresolved.
  Every other edge type connects two existing things.
- Resolved wiki links render in the body as clickable navigation to the
  target node.

## 7. People

**Person** is a registry entity, not a node: a record (name the only
required field; labeled multi-value contact points; free Markdown), parallel
in shape to Company (§8).

### 7.1 Sameness: email is identity

Names may collide and are **not** unique. **Email is the identity rule:
the same email means the same person.** Duplicates are prevented or healed
at the email. A merge tool is the fallback for the cases the rule cannot
reach (a person with no email, or an email that changed). What happens at
the exact moment of an email collision (block, prompt, auto-merge) is an
implementation detail, not ontology. Shared or role mailboxes are a noted
edge case, not yet decided.

### 7.2 Membership

The stored relation between a Person and a Project is **membership**,
carrying a Role and an optional context note. Membership says "this person
is involved in this Project, in this capacity." Membership is Project-only
(Areas, Articles, and References have no People). In the graph, memberships
are projected as people→project edges, and a Person vertex is labeled
**name + email** — the email is the discriminator, consistent with §7.1.

### 7.3 Mentions

Text can *refer* to a Person without any membership. Mentions are authored
Slack-style: typing `@` in a body or a note opens a picker listing people
as name + email; choosing one inserts a mention that **stores the Person's
ID, not their name**.

- Disambiguation happens at authoring time, by the author — never by
  algorithmic guessing. **Plain names in text are never auto-resolved.**
- A mention renders as a link to the Person (displaying their current
  name) and feeds a **"Mentioned in"** reverse list on the Person's page.
- A mention is **not membership**: it says "this text refers to this
  person," nothing more.
- The addressing symmetry: nodes are addressed by title (`[[Title]]` — a
  title acts as an address); people are addressed by identity (@ picker →
  Person ID — a name is not an address).

## 8. Companies

**Company** is a registry entity like Person — not a node, and not free
text on a Person record. A Company is a graph vertex (people→companies
edges exist), carries structured fields, and also has free Markdown.

### 8.1 Fields

Deliberately parallel to Person, and deliberately *not* a CRM record:

- **Identity:** name (the only required field); legal name and alternate
  names, optional; website; industry as a single open label; logo; short
  description.
- **Domain is the duplicate-warning key — warning-only, not an identity
  rule.** Domains are weaker than a person's email: subsidiaries share
  them, and one company has several.
- **Contact/location:** structured, labeled, multi-value address; labeled
  phone and email on the Person pattern (generic mailboxes — individuals
  are People); optional time zone.
- **Relationship to me:** a multi-value open label — *employer*, *former
  employer*, *customer*, *prospect*, *vendor*, *partner*, … — with optional
  start/end dates carried on the relationship, not the company.
- **People are links, never a field.** Person records link to the Company;
  the Company shows its People as the reverse view.
- **Parent / sub-company** is a Company→Company link (corporate structure —
  distinct from Area nesting, which is attention scope, not an org chart).
- **Personal operations:** a labeled **key-links** list (payroll portal,
  expense system, benefits, help desk, travel booking — the entity analogue
  of a node's External Links, §5) plus labeled service account numbers.
  Account numbers and contact details are owner-only-sensitive, as with
  Person contact details.
- **The body carries the narrative:** org chart, policies and procedures,
  practicalities (sites, travel patterns), history — anything one-off or
  not yet understood.

Explicitly excluded, as sales machinery with no place in a personal record:
revenue and funding data, lifecycle/lead stage, deals and pipelines, ticker
and registry identifiers, account ownership fields.

### 8.2 Placement: framed, never contained

A Company is registry-level and is **not contained by any Area**. An Area
may *frame* a Company contextually, and which Area does so follows the
relationship-to-me: an employer is framed by a career Area; a customer is
framed by that customer's own Area; a vendor by whichever Area is relevant,
or by none. A company may have nothing to do with the owner's career at
all — it could be a customer.

This resolves the classic double-hat: an employer *feels* like an Area
because of the operational content kept for it (portals, policies, travel
practicalities). Under the ontology that content lives on the Company
entity (key-links + body); the Area remains pure attention scope.

## 9. Captures, notes, and Tasks

"Note" names two different concepts, and the ontology keeps them apart.

### 9.1 Captures (the Inbox)

A **capture** is transient, unclassified input — a quick grab during a
call, a passing idea — held in a Notes Inbox. Captures exist *before*
classification (compare §9.3: a Task is a classified thing; the role of the
orphan is played by a capture).

- Process captures as soon as possible; **Inbox Zero is the goal**.
- Processing converts a capture into **exactly one** destination, by
  copying or rewording its text: a new node (Project, Article, Reference,
  …), a Project note, a Task, an Event, etc.
- Or the capture is **deliberately deleted as useless**, resulting in
  nothing. Deletion must always be a deliberate act, never an accidental
  drop — the point of capture is that a good idea is not lost.
- Once converted, **the original capture goes away and no provenance is
  kept**: a result is never traced back to its capture, as long as the
  content indeed landed in the destination.
- Converting a capture into a Task requires naming the owning Project
  (§9.3).

### 9.2 Project notes

A **Project note** is a timestamped, project-bound record of events and
developments — meeting notes, a microblog-like log entry.

- The **timestamp is semantically important**: the note places an event in
  the project's chronology — who said what, when.
- Project notes are one element of the Project's History (§10), alongside
  occurred events and status changes.
- When a Project note gets promoted to something bigger is a matter of
  judgment in practice; the ontology deliberately does not define a
  promotion trigger.

### 9.3 Tasks

A **Task** is a single, completable action — an entity with fields, not a
document. If something needs multiple steps, it is not a Task; it is a
Project (or a sub-Project) — the mirror image of the Project definition.

- **Ownership: exactly one Project.** No orphan Tasks, no sharing. If a
  similar task is needed for another Project, a separate Task object is
  created for it. *A Task is a classified thing.*
- **Lifecycle: Open / Waiting / Done / Cancelled.**
  - *Waiting* is a first-class stored state — the task is on someone or
    something else. What or who it waits on is written in the Task's free
    text (below); there is deliberately no tighter structure for now.
  - *Done* means completed successfully, and carries the completion date.
  - *Cancelled* is terminal with a specific meaning: something **prevented**
    the task from being completed, permanently. It carries no completion
    date.
- **Dates:** a Task may carry a **due date** and a **completion date**, and
  the two may differ — lateness is the derived comparison. A Task's due
  date is *rendered* as an event in Calendar and Project Plan views; the
  stored fact is the due date itself (§11).
- **Free Markdown:** a Task carries enough information to allow its owner
  to complete it. A Task is self-contained context, not a bare title.
- **Dependencies:** task-to-task dependencies mirror the node-level
  `depends_on` definition (§6.4) — hard prerequisites, surfaced as
  blockers, never enforced, cycles rejected.
- **No recurring Tasks.** A Task is a one-time action; recurrence implies
  scheduling beyond the task itself. (This may be revisited in the future;
  any shipped repeat behavior is outside this ontology.)

## 10. Project Plan and History

Two distinct concepts that are easily confused:

- **Project Plan** — the forward-looking structure: what *will* happen.
  Gantt-like; a project-management tool. It holds the Project's planned
  Events, some of which are Milestones. It only makes sense in the context
  of a Project, so it is a **Project-only** element.
- **History** — the record of what *actually occurred*. History is
  **general**: it applies to a Project, an Area, a Reference — any
  document-based node we may want a history of what occurred about it. For
  a Project, History is its notes plus occurred events plus status changes
  (§9.2). A milestone is planned in the Project Plan; its actual date and
  outcome land in History.

History is distinct from a node's body *version history* — version
snapshots are a text mechanism, not the semantic record of what occurred.

## 11. Events

An **Event** is an occurrence, not a commitment: it happens whether or not
the owner acts — often not because of them at all. The owner needs to
*know* about it, and sometimes to do something about it (the associated
doing lives in Tasks and Projects). An Event is not completable.

- **Events are Trove-local, native entities — deliberately minimal:** a
  title, a date/time, an open type label, optional free text, and an
  optional attachment to a context (a Project, an Area, a Person).
- **Event types are open labels**, not a fixed enum — *appointment*,
  *birthday*, *milestone*, and whatever practice adds.
- **No scheduling logistics.** No invites, attendees, rooms, reminders, or
  recurrence engine. That layer belongs to external calendars and stays
  there. Events are for **awareness and context**; calendars are for
  scheduling logistics.
- **No sync, no mirroring — ever.** External calendars remain external and
  authoritative for what lives on them. Where an occurrence exists in both
  worlds, the Trove Event may carry an External Link to the calendar
  entry — a link, never a sync. Native Events exist precisely because some
  occurrences must not sit on a work calendar, and because Project Plan
  milestones and History entries need event objects inside Trove.
- **Derived occurrences stay derived.** People's birthdays are computed
  from the Person record and shown yearly — never stored as Events. A
  Task's due date is rendered as an event in Calendar and Project Plan
  views — the stored fact remains the due date (§9.3). Past events belong
  in History; future events belong in awareness (the Calendar view, which
  is a view over Events and due dates).

## 12. Cross-cutting rules

- **Declared, not derived** — completion, cancellation, and identity are
  stated by the owner (a status, an email), not computed from other
  objects' states. The one derived direction is *downward*: a declared
  Project completion/cancellation cascades state to sub-Projects and their
  Tasks (§4.1).
- **Surfacing, never enforcement** — the model's semantics inform display
  (blockers, indexes, lineage, warnings). They do not prevent work, reorder
  queues, or mutate other objects' state, beyond the single cascade rule
  of §4.1.
- **Labels over enums** — where practice cannot enumerate values in advance
  (project status, event type, industry, relationship-to-me), the model
  uses open labels with suggestions from actual usage.
- **The graph shows everything once** — memberships and task ownership are
  stored relations projected into the graph; mentions and wiki-links are
  text-derived relations mirrored by sync. No relation is stored twice in
  two forms.

## 13. Deliberately undecided

Recorded as open, so future work does not mistake silence for a decision:

- Shared or role mailboxes under the email-identity rule (§7.1).
- Project-note editing: edit-in-place vs append-only; note color semantics,
  if any (§9.2).
- What exactly triggers promoting a Project note to something bigger
  (§9.2) — currently judgment, deliberately.
- Milestone: a label on an Event vs its own subtype (§10).
- A create-node shortcut from an unresolved wiki link (§6.6) — an
  implementation option, not ontology.
- The exact UI behavior at the moment of an email collision (§7.1) —
  implementation detail for the follow-on work.
