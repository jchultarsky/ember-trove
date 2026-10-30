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

The owner's rulings on the review of this document (issue #204, 2026-10-30)
are folded into the text below; §15 logs what changed and why. Nothing is
left open: §13 records how each formerly undecided question was settled.

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
Projects and Articles. It reaches References by links, never by
containment (§3.3, §6.3).

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
| History (§10), including notes (§9.2) | ✓ | ✓ | ✓ | ✓ |
| Tasks | ✓ | — | — | — |
| People (membership) | ✓ | — | — | — |
| Status label (`project_status`) and terminal state (§4.1) | ✓ | — | — | — |
| Project Plan (§10) | ✓ | — | — | — |

### 3.3 Rules that apply to all kinds

- **References are shared, not owned.** One Reference can be referred to by
  many Projects and Articles. The link to a Reference is never containment
  or ownership (§6).
- **Type permanence.** A node's type, once assigned, stays. There is no
  in-place mutation (a Reference does not become a Project). Flexibility
  comes from the free-Markdown body, not from reclassifying kinds.
- **Single home.** A node has at most one `contains` parent (§6.1), never
  shared. Which kinds may be a home is fixed:

  | Home (parent) | May contain |
  |---|---|
  | Area | Areas, Projects, Articles |
  | Project | sub-Projects only (decomposition, §4.2) |
  | Article | nothing |
  | Reference | nothing |

  A Reference is never contained. A Project's home is an Area or a parent
  Project; a Project may also **stand alone**, with no home at all. An
  Area holds many Projects.
- **Lifecycle.** Draft vs Published is a label (finished vs not — most
  meaningful for Articles) with one effect: a `depends_on` on an Article is
  satisfied once the Article is Published (§6.4). Archived hides a node from
  lists and navigation, and an archived Reference no longer satisfies a
  `depends_on`.

## 4. Anatomy of a Project

A Project assembles nine elements:

1. **Body** — free Markdown documentation; the catch-all, as with every
   kind.
2. **Status** — a short structured status token (`project_status`). Its
   values are **open**: the possible values cannot be known ahead of time,
   so the picker suggests values already in use rather than enforcing a
   closed enum. Narrative status stays in the body or notes. Status is
   distinct from the Draft/Published/Archived lifecycle. Beside the open
   label sits a small **closed terminal state** — *active*, *complete*, or
   *cancelled* — which is the machine-readable declaration of §4.1.
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
8. **Position and relations** — at most one home via `contains` (an Area
   or a parent Project), or none;
   `derived_from`, `depends_on`, `related_to`, and wiki-links to other
   nodes (§6).
9. **The goal** — the outcome the Project exists to achieve, stated in the
   body. There is deliberately no separate goal field.

### 4.1 Completion is declared, not derived

A Project is complete when its **terminal state** (§4, element 2)
**declares** it — never computed from the state of its Tasks or
sub-Projects, and never inferred from the wording of its open status label.
A master Project may be suddenly cancelled (for example, a customer
withdraws funding); that decision does not depend on the state of anything
beneath it.

- The terminal state is *active*, *complete*, or *cancelled*. A cancelled
  Project is terminal and declared, parallel to a Task's Cancelled state
  (§9.3).
- The declaration is made independently of the children (never derived
  *upward*), and once declared it **cascades *downward***, transitively
  down the `contains` tree:
  - sub-Projects take the master's terminal state (complete or cancelled);
  - their open or Waiting Tasks become **Cancelled** — under completion as
    well as cancellation. A Task closed by a cascade was not done, so it
    never receives a completion date it did not earn.
- **The cascade changes state only.** It never deletes, archives, or hides
  a sub-Project; closing a master closes its sub-Projects, it does not
  remove them.
- **Cascade safeguards:**
  - *Confirmation first.* Before a cascade runs, the owner is shown the
    sub-Projects and Tasks it will close — and any child it cannot touch
    (below) — and confirms.
  - *Recorded in History.* A cascade writes a History entry (§10): who
    declared it, when, and exactly which sub-Projects and Tasks it closed.
  - *Master-only reopening.* Setting a closed Project back to *active*
    affects that Project alone. Children closed by the cascade stay closed,
    and a sub-Project that was closed on its own before the cascade is never
    silently reopened. Once the master is active again, any child can be
    reopened deliberately, one by one.
  - *No active child under a closed master.* A sub-Project cannot be made
    active while its master is closed.
- **The cascade stays inside the actor's rights** (§14). It closes only
  sub-Projects and Tasks the declaring person may edit through their own
  ownership or sharing grants; the admin role does not extend it. A child
  the actor cannot edit is skipped, listed in the confirmation, and shown
  afterwards as open under a closed master — the "no active child" rule
  above applies only to children the actor can edit.
- A `depends_on` edge targeting a Project is satisfied exactly when that
  Project's terminal state is *complete*. A cancelled target can never
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
- Only the pairs in the containment table of §3.3 are allowed (Area →
  Area / Project / Article; Project → sub-Project).
- **No cycles**: the structure is a forest.
- A **Reference is never a `contains` child** — it is shared, and reached by
  links.
- Because it claims the child's single home, creating a `contains` edge
  requires edit rights on **both** ends (§14).
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
- Because it has no direction, a `related_to` edge may be deleted by anyone
  with edit rights on either end (§14).

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
- When a dependency is satisfied depends on the target's kind:

  | Target | Satisfied when |
  |---|---|
  | Project | its terminal state is *complete* (§4.1); never when cancelled |
  | Article | it is Published (§3.3) |
  | Reference | it exists and is not archived — the material is available |
  | Task | it is Done (§9.3); never when Cancelled |

  An Area is a standard to maintain and never completes (§3.1), so it is
  not a valid `depends_on` target. Task-to-task dependencies mirror this definition (§9.3).

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
reach (a person with no email, or an email that changed).

- **A collision blocks the save.** Saving a Person with an email that
  already belongs to another Person is refused, with a way to open the
  existing record or merge into it. Records are never merged
  automatically.
- **A Person's email belongs to that individual.** Shared and role
  mailboxes (support@, a team or family inbox) are not People: they are
  contact points on a Company (§8.1). This keeps the identity rule strict.

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
  …), a note on a Project or another node (§9.2), a Task, an Event, etc.
- Or the capture is **deliberately deleted as useless**, resulting in
  nothing. Deletion must always be a deliberate act, never an accidental
  drop — the point of capture is that a good idea is not lost.
- Once converted, **the original capture goes away and no provenance is
  kept**: a result is never traced back to its capture, as long as the
  content indeed landed in the destination.
- Converting a capture into a Task requires naming the owning Project
  (§9.3).

### 9.2 Project notes (and notes on other kinds)

A **Project note** is a timestamped, project-bound record of events and
developments — meeting notes, a microblog-like log entry.

- The **timestamp is semantically important**: the note places an event in
  the project's chronology — who said what, when.
- Project notes are one element of the Project's History (§10), alongside
  occurred events and status changes.
- **Every document kind may carry notes**, as entries in its History
  (§3.2, §10). A "Project note" is simply a note on a Project.
- **Notes are editable; their timestamp is not.** Editing a note keeps its
  original timestamp — its place in the chronology — and the note shows
  that it was edited. Note colours carry no meaning in the model.
- When a Project note gets promoted to something bigger is a matter of
  the owner's judgment, **by design**: the ontology defines no promotion
  trigger.

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
    date. Closing a Task's Project closes it this way (§4.1): once the
    Project is closed, the Task can no longer be done within it.
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
  scheduling beyond the task itself. The repeat behaviour shipped before
  this ontology is removed, and existing repeating Tasks become one-time
  Tasks. Recurrence may come back only as a new, explicit decision.

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
  (§9.2), plus the record of any completion cascade (§4.1). A milestone is
  planned in the Project Plan; its actual date and outcome land in History.
- **A Milestone is an Event** (§11) labelled `milestone` that belongs to a
  Project Plan — not a separate subtype.

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
  stated by the owner (a terminal state, an email), not computed from other
  objects' states. The one derived direction is *downward*: a declared
  Project completion/cancellation cascades state to sub-Projects and their
  Tasks (§4.1).
- **Surfacing, never enforcement** — the model's semantics inform display
  (blockers, indexes, lineage, warnings). They do not prevent work, reorder
  queues, or mutate other objects' state. There are exactly two exceptions,
  both in §4.1: the downward cascade, and that a sub-Project cannot be made
  active while its master is closed (for children the actor can edit).
  Structural rules — one home, the containment table, no cycles, email
  identity — are integrity rules of the data, not enforcement on work.
- **Labels over enums** — where practice cannot enumerate values in advance
  (project status, event type, industry, relationship-to-me), the model
  uses open labels with suggestions from actual usage. Where a value must
  drive behaviour, a small closed set sits beside the label (a Project's
  terminal state, §4.1).
- **The graph shows everything once** — memberships and task ownership are
  stored relations projected into the graph; mentions and wiki-links are
  text-derived relations mirrored by sync. No relation is stored twice in
  two forms.

## 13. Formerly undecided — now settled

The first draft recorded these as open. Each is now settled (2026-10-30),
so no question in this document is open:

| Question | Settled as |
|---|---|
| Shared or role mailboxes under the email-identity rule | Company contact points, never People (§7.1) |
| Project-note editing; note colour semantics | Editable, timestamp fixed, shown as edited; colours carry no meaning (§9.2) |
| What triggers promoting a Project note | The owner's judgment, by design — no trigger is defined (§9.2) |
| Milestone: label on an Event or its own subtype | An Event labelled `milestone` in a Project Plan (§10) |
| A create-node shortcut from an unresolved wiki link | An implementation option outside the ontology; kept in the product backlog |
| UI behaviour at an email collision | The save is blocked, with open-existing or merge; never auto-merge (§7.1) |

## 14. Ownership and visibility

Trove is one person's view of their life (§1), but an installation serves
more than one account: every object has an owner, a node can be shared with
another account as a viewer or an editor, and an admin role exists. The
model's semantics never widen what anyone can see or change.

- **Owner scope.** A node is visible to its owner and to the accounts it is
  shared with; the admin role may manage nodes across owners. **Person and
  Company records are owner-only:** never shared, and the admin role does
  not reach them.
- **Edges need rights on both ends.** Creating an edge requires edit rights
  on the source and at least view rights on the target. `contains`
  requires edit rights on **both** ends, because it claims the child's one
  home (§6.1). An unordered `related_to` may be deleted with edit rights on
  either end (§6.2). Errors from the structural rules (one home, the
  containment table, cycles) never name a node the actor cannot see.
- **Reverse indexes and warnings show only what the viewer can read.**
  "Used in", "Mentioned in", Descendants, an Article's bibliography,
  transitive blockers, and deletion or archive warnings list only nodes the
  viewer may read. They never show a count of hidden items: a warning may
  understate rather than reveal that a hidden node exists.
- **Mentions** (§7.3). The `@` picker lists only the author's own People.
  Anyone other than the Person's owner sees a mention as the name at most —
  never contact details, and never a way into the owner's registry. A
  mention of a deleted Person renders as unresolved.
- **Memberships** (§7.2). Accounts a Project is shared with see a member's
  name and Role only, never contact details.
- **Company personal-operations data** (§8.1). Account numbers, key links,
  and contact details are shown only to the owner. They are never logged,
  never indexed for search, never sent in webhooks or exports, and are
  encrypted at rest.
- **The cascade** (§4.1) acts only through the declaring person's own
  ownership or sharing grants; the admin role does not extend it. Children
  it may not touch are skipped, listed in the confirmation, and surfaced.
- **Wiki links** (§6.6) resolve only among titles visible to the
  mentioning node's owner; titles are unique per owner, not globally.
- **Data-health reports** (the cleanup pass in the preamble) are per
  account. An admin sees counts per rule across the installation — never
  titles, ids, or content.

## 15. Decision log

| Date | Decision | Source |
|---|---|---|
| 2026-10 | The ontology as first written (§1–§13) | Owner's ontology working session |
| 2026-10-30 | A closed terminal state (*active / complete / cancelled*) beside the open status label drives the cascade and `depends_on` | Owner ruling 1 on the #204 review |
| 2026-10-30 | A Project has at most one home — an Area or a parent Project — and may stand alone | Owner ruling 2 |
| 2026-10-30 | `depends_on` on an Article is satisfied when Published; on a Reference when it exists and is not archived | Owner ruling 3 |
| 2026-10-30 | Recurring Tasks are removed; existing ones become one-time Tasks | Owner ruling 4 |
| 2026-10-30 | §14 Ownership and visibility adopted | Owner ruling 5 |
| 2026-10-30 | Cascade safeguards: confirmation, History record, master-only reopening; the cascade changes state only, never deletes; no active child under a closed master | Owner rulings on concern B and the cascade clarification |
| 2026-10-30 | A cascade closes open Tasks as **Cancelled**, under completion too — no unearned completion dates (supersedes "Done under completion") | Owner, developer session |
| 2026-10-30 | The cascade skips children the actor may not edit and surfaces them; the "no active child" rule applies to editable children only; the admin role does not extend the cascade | Owner, developer session |
| 2026-10-30 | Containment table: Area → Area / Project / Article; Project → sub-Project; Article and Reference contain nothing | Owner, developer session |
| 2026-10-30 | Notes stay on every document kind, as History entries | Owner, developer session |
| 2026-10-30 | §13 questions settled (mailboxes, note editing, promotion, Milestone, wiki-link shortcut, email collision) | Owner, developer session |
| 2026-10-30 | `contains` needs edit rights on both ends; `related_to` deletable from either end; an Area is not a `depends_on` target | Developer review; confirmed by the owner |
