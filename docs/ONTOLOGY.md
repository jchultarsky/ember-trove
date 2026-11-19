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
| Brief (§10) | ✓ | — | — | — |
| A place in someone's Focus (§10) | ✓ | — | — | — |

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
  `depends_on`. It does **not** hide the node from search: archiving takes
  finished or cancelled work out of view to declutter lists and
  prioritising, and is never a way to make information unfindable. A list
  can show archived nodes on request.

## 4. Anatomy of a Project

A Project assembles nine elements:

1. **Body** — free Markdown documentation; the catch-all, as with every
   kind.
2. **Status** — a short structured status token (`project_status`). Its
   values are **open**: the possible values cannot be known ahead of time,
   so the picker suggests values already in use rather than enforcing a
   closed enum. Narrative status stays in the body or notes. Status is
   distinct from the Draft/Published/Archived lifecycle. Beside the open
   label sits a small **closed state** — *active*, *parked*, *complete*, or
   *cancelled* — which is the machine-readable declaration of §4.1.
   *Complete* and *cancelled* are the **terminal states**; *parked* is not
   terminal.
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

- The closed state is *active*, *parked*, *complete*, or *cancelled*.
  *Complete* and *cancelled* are terminal. A cancelled Project is terminal
  and declared, parallel to a Task's Cancelled state (§9.3).
- **Parked is declared too, and is not terminal.** A parked Project is set
  aside on purpose: it should not occupy its person's immediate attention.
  One person works on many Projects and has to switch between them; parking
  says which ones are deliberately not in motion, so they stop asking for
  attention without being closed, archived or lost. A parked Project is
  made *active* again by declaring it so.
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
- **A Project closed before the cascade existed, or that gained something
  open since, is caught up only when asked.** Declaring the state it
  already has changes nothing. The Project's page and Data health say
  what is still open under it and offer to close it, with the same
  confirmation.
- **Under a closed master a sub-Project is not re-opened**: it cannot be
  made active, and a closed one cannot be parked either (parked is not
  closed). Changing it between complete and cancelled is allowed. An open
  Project may be *given* a closed Project as its home; Data health lists
  it, and such a Project, while still active, may be parked. The rule is
  not applied when the actor cannot view the master: they are not told
  that a hidden Project is closed. An admin reads every Project, so the
  rule applies to them.
- **A sub-Project the actor cannot edit that is already closed** is not
  listed as staying open, and the cascade does not go through it: what is
  under it is left to the people who can edit it.
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

**No edge of any type joins a node to itself.** The database refuses a
self-loop for every type, not only for the types whose longer loops are
checked (§6.1, §6.4, §6.5).

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
- It may be **removed** with edit rights on **either** end, so a child's
  editor can always free its one home, even when the home is a node they
  can't edit (§14).
- `contains` governs structure, navigation, and filing only. Deleting a
  container does **not** cascade-delete its children. (Project completion
  and cancellation *do* cascade state downward — §4.1 — which is a state
  rule, not a deletion rule.)
- What a node **holds** is deleted with it, for good: its Tasks, its notes
  and its files, including Tasks and notes written by other accounts. So
  the delete confirmation first counts what would be lost, the deleter's
  own and other people's, and offers Delete only once the counts are
  shown. It gives counts only, never titles or names (§14).

### 6.2 `related_to` — bidirectional traversal, nothing more

The one symmetric type. Its entire meaning is that the graph can be
traversed in either direction between the two things. There is no other
meaning to it.

- Stored **once, unordered**: creating B→A when A→B exists is the same edge,
  not a second one.
- Because it has no direction, a `related_to` edge may be deleted by anyone
  with edit rights on either end (§14).
- Creating one takes view rights on the target, so someone who may only
  **view** a node can relate a private node of theirs to it. The node's
  owner is not shown that link; they are told one more link exists and
  may remove it (§14). Nothing hidden is named.

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
- An **active** node, for that warning, is one still in use: not
  archived, and not a Project whose state is complete or cancelled. A
  parked Project and a draft count: set aside or unfinished is not done
  with. The warning is a count of links, never a title, and it counts
  only links from nodes its reader can see (§14).

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

  The table's Task row is §9.3's: a Task waits for a Task. There is no
  link from a node to a Task.
- **How the table is read.** A Project is judged by its state alone:
  complete and archived is met; active and archived is not. An archived
  Article is not Published (the lifecycle is one value: draft, published
  or archived), so it is not met. An old link to an Area is never met,
  and is listed in Data health.
- **A node is blocked** while it still waits and one of its own
  dependencies is not met, among those its reader can see. A node still
  waits while it is in use (not archived, and not a complete or
  cancelled Project: the meaning "active" has in §6.3) and is not itself
  what the table above asks for: an active or parked Project, a draft
  Article, an Area. A complete Project, a Published Article and a
  Reference are finished or available, so nothing of theirs waits to
  proceed, whatever they still link to; a node out of use will not
  proceed. Because only dependencies the reader can see count, a node
  can read as blocked for one person and not for another (§14). That
  includes a dependency the node's own owner cannot see (an editor's
  link to a node of theirs): until it is removed it is a real link, and
  those who see both ends read the node as blocked. The owner is told
  the node has a link they are not shown, and may remove it (§14).
- **The chain.** What a node's unmet dependencies wait for in turn is
  shown with them ("also waits for …, through …"). The chain goes on
  only through a dependency that itself still waits: not through one
  that is met (what a finished thing waited for no longer matters), nor
  a cancelled or archived one (it will not proceed, so its own
  prerequisites change nothing), nor an Area (not a valid target at
  all). Such a dependency
  never lets its dependents go: its row says what it is, and its link is
  the person's to remove.

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
- **An Area is never an origin.** An Area is a standing domain, not a
  piece of work or a text that something is adapted, copied or split
  from; a node that belongs to an Area has it as its home (§6.1). An
  Area may itself be derived from something.
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
  - A **new** Person who collides can be added to the Person who has
    the address: that Person's editor opens with what was typed added,
    for review, and nothing is saved until it is saved there. Theirs is
    kept, blanks are filled, lists grow; what was typed and differs is
    said, not used.
  - On a node, the Person who has the address is put on the node
    instead, with the Role and Note typed. The other details typed are
    not added to them.
  - Merging two **saved** People comes with the merge tool.
- **The same address** means the same text once the spaces around it are
  off and ASCII letters are compared without case. Nothing more: no mail
  provider's own rules (dots, `+tags`), and letters outside ASCII are
  compared as typed. An address is not accepted with a wrapper around it
  (`mailto:`, `<…>`) or with the usual characters that take no space on
  screen (a zero-width space, a direction mark): the common ways one
  mailbox gets written twice. Look-alike letters of another script are
  different addresses, here as to a mail server.
- **One address, once.** A Person does not list the same address twice:
  a save of such a list is refused.
- **People who shared an address before the rule stay as they are**, and
  stay editable: each keeps the address it has. What is refused is
  *adding* an address another Person lists. So one of a pair who lets
  the address go cannot take it back while the other lists it. Such
  pairs are listed in the data-health report until they are edited or
  merged; a restore brings back the ones its archive holds.
- **Per account.** The rule is about one person's registry. Two accounts
  may hold the same address, and neither is told (§14).
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

Only the Project's **owner** adds, changes or removes its memberships. An
editor of a shared Project can't, and neither can the admin: a membership
points into the owner's own People registry, which no one else can see
(§14).

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
- **In the text a mention is written `@[name](person:ID)`.** The ID is the
  identity. The name beside it is what the author saw when they picked
  the Person: it keeps the raw text readable, and it is what shows when
  the mention cannot be resolved. It is never used to find anyone, and it
  can be older than the Person's current name; the editor offers to bring
  it up to date and never does so unasked.
- **A mention is real only when its author made it.** A save makes a
  mention only for a mention it **adds** to the text, and only of the
  saver's own People. A token typed or pasted with another account's
  Person's ID is plain text that looks like a mention: it shows the name
  as typed, unresolved, and stays so when that Person's owner later
  saves, restores or copies the text, because the owner did not put it
  there. If a co-editor cuts someone's mention out of a shared body and
  puts it back, it is unresolved until that Person's owner mentions them
  again. An old version put back makes no mention; a copy of a node
  carries only its maker's own mentions that were real in the original.
- Plain words in task titles, Briefs, captures and templates are not
  places for mentions: a token there reads "@name" and links to nothing.

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
  "Edited" means the note's **words** changed: a colour, or a trip
  through the trash and back, is not an edit. Only a note's author edits
  it, while they may still edit its node; they, the node's owner or an
  admin may remove it (§14).
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
  - A Task may wait for **any Task its person can see**, in any Project.
    Adding the link takes edit rights on the Task that waits and view
    rights on the other, as a node-level `depends_on` does. Removing it
    takes edit rights on the Task that waits.
  - A prerequisite holds its dependents back until it is **Done**. A
    Cancelled one never lets them go: the dependent says what it waits
    for, and its person removes the link.
  - *Surfaced, never enforced:* a blocked Task says so, and nothing else
    changes. It keeps its place in My Day and in every list, and can be
    worked on and completed.
  - A prerequisite in the trash neither shows nor counts; restored, it
    does again. One on a Project a reader cannot see is not shown to
    that reader and not counted for them (§14), so the same Task can read
    as blocked for one person and not for another.
  - A prerequisite that is not shown (in the trash, or on a Project the
    reader cannot see) still counts for the loop rule. So whoever may
    edit a Task is told **how many** of its prerequisites they are not
    shown, never which, and may remove them: the rule a home its reader
    cannot see has (§6.1). Someone who may only view the Task is told
    nothing of them. "Not shown" here means shown **neither to the
    reader nor to the owner of the Task's Project**: a prerequisite on
    one of the owner's own private Projects is the owner's business, in
    the trash or out of it, and an editor of the shared Project is not
    told of it and cannot remove it this way (§14). The rule follows the
    Task: once its Project's owner has moved it to another person's
    Project, that person is its Project's owner. Only the owner can make
    that move (§14).
  - Moving a Task takes edit rights on both Projects, and between two
    accounts the owner of the Project it leaves (§14).
  - Moving a Task keeps its dependencies. Closing a Project cancels its
    Tasks (§4.1), so what waited for them stays blocked, and says by
    what.
  - A Task shows what its prerequisites wait for in turn, as a node does
    (§6.4): through Tasks its reader is shown that are still to do. A
    Done prerequisite passes nothing on; nor does a Cancelled one, which
    holds its dependents back for good but will not proceed. The
    "Blocked" mark and its count stay about the Task's own
    prerequisites, and the count of ones not shown stays about its own
    links.
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
In the app the two have different names: **History** is this chronology,
and saved text versions are **Versions**.

History is also distinct from the node's *audit trail* (who created,
edited, tagged, shared or exported it), which the app shows apart, as
Activity. What is in History: notes; a change of the status label; a
change of the declared state; a cascade's record; a replaced Brief; and,
on every kind, a move between draft, published and archived.

- **Brief** (#282) — where a Project stands *now*: an optional one-line
  headline, a pitch (the elevator speech) and a few points. A Project has
  at most one current Brief, shared with the node (viewers read it,
  editors write it). It sits beside the status label: the label is the
  flag, the Brief the narrative. The body keeps the durable material.
  A save by the same person within the edit window (15 minutes from when
  the Brief was stated) corrects it in place; any later save, or one by
  someone else, **supersedes** it, and the old Brief becomes a History
  entry. Removing a Brief records it in History too. Its age is shown
  wherever it renders; past a threshold it is marked stale, never hidden.
- **Focus** (#282) — the Projects that hold a person's attention: a
  declared, ordered set, the Project analog of My Day, shown first on the
  dashboard. An entry may carry a one-line "why now".
  - Focus belongs to the **person**, not to the Project. Each account has
    its own, on any Project it can view. It is private, with no admin
    bypass (§14): nobody learns that a Project is in someone else's
    Focus, or reads their why-line. It has no reverse index and writes
    nothing to History.
  - It is persistent. Nothing expires it; the card shows how long the
    Project has been there. The software sets no cap: past five entries
    it says that Focus works best small (§12, surfacing).
  - An archived Project does not show in Focus, nor does one its reader
    can no longer view. That is a filter on the read: the entry stays, so
    the Project shows again when it is un-archived or shared again. An
    entry is deleted only by its own person, with its node, or with its
    account.
  - Only an *active* Project shows in Focus (§4.1).
    - A **parked** Project does not show: parking is the statement that
      it should not hold attention now. That is the same filter on the
      read as archiving: the entry stays, so the Project is back in its
      place, with its why-line, when it is made active again.
    - A **complete or cancelled** Project leaves the Focus of the person
      who declared it, and re-opening it does not put it back: the
      declaration removes that person's own entry. It can remove no one
      else's (Focus is private, and an entry is deleted only by its own
      person), so anyone else's entry is hidden while the Project is
      closed and shows again if it is re-opened.

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
- **Notes and Tasks follow their node's rights.** Writing one takes edit
  rights on the node, and so does changing it later: its author changes it
  only while they may still edit the node. When a share is revoked, what
  that person wrote stays on the node, leaves their own lists, and is no
  longer theirs to change. A node's owner may delete any note on it.
  **A Task leaves for another person's Project only by the hand of the
  owner of the Project it is on.** Edit rights on both Projects move it
  between Projects of one account, and no further: otherwise someone a
  Project is shared with could take its Tasks with them.
- **Edges need rights on both ends.** Creating an edge requires edit rights
  on the source and at least view rights on the target. `contains` requires
  edit rights on **both** ends, because it claims the child's one home
  (§6.1); it may be removed with edit rights on either end. An unordered
  `related_to` may be deleted with edit rights on either end (§6.2). Errors
  from the structural rules (one home, the containment table, cycles) never
  name a node the actor cannot see.
- **Reverse indexes and warnings show only what the viewer can read.**
  "Used in", "Mentioned in", Descendants, an Article's bibliography,
  transitive blockers, and deletion or archive warnings list only nodes the
  viewer may read. They never show a count of hidden items: a warning may
  understate rather than reveal that a hidden node exists. There are two
  stated exceptions, both told **only to someone who may edit the thing**
  and both a bare number: a Task's prerequisites not shown (§9.3), and a
  node's links not shown (next).
- **A node's links that neither its reader nor its owner can see.**
  Creating an edge takes view rights on the target only, so someone a
  node is shared with as an editor can link it to a node of their own
  that the node's owner cannot see; and a link can become hidden later,
  when a share on its other end is revoked. Such a link is in no list of
  the owner's, and it is still a real link: it decides which new links
  are refused as cycles, and whoever can see both ends reads the node as
  blocked by it (§6.4). So whoever may **edit** the node is told **how
  many** such links it has, never which, to what, or who made them, and
  may remove them together: the rule a Task's prerequisites not shown
  have (§9.3).
  - A link counts only when its other end is hidden from the reader
    **and from the node's owner**. A link to one of the owner's own
    private nodes is the owner's business: an editor of the node is not
    told of it and cannot remove it this way.
  - **The node's owner** is the account the node belongs to, seeing the
    other end because they own it or because it is shared with them.
    Someone else who holds an owner's rights on the node by a grant is,
    for this rule, an editor: their link to a private node of their own
    is told to the node's owner. What an admin can open only by the
    admin role does not count as seen either.
  - An editor who is not the owner may still be refused a link as a
    cycle because of a link the owner can see and they cannot. They are
    told nothing of it: it is the owner's link, and the owner's to
    remove.
  - A hidden home (§6.1) is an older and wider rule: the child's editors
    are told it exists and may free the child, whether or not the
    child's owner can see that home.
  - With two editors there is one thing the number can say: when one of
    them links the node to a third person's node, what the other is told
    turns on whether the node's owner can see it. Refusing the link when
    it is made would tell the first of them the same.
  - The links concerned are the ones an editor of the node could remove
    if they were shown them: every link made by hand **from** the node,
    and a `related_to` link either way (§6.2). Not a home the node sits
    in (the home row already says so and frees it, §6.1), not another
    node's link **into** this one (it is that node's), and never a wiki
    link: it mirrors the text (§6.6), and counting it would let someone
    write `[[a guess]]` and watch the number.
  - Someone who may only view the node is told nothing. An admin is
    shown every link, so for them there is nothing to tell.
  - Nothing about it goes into History: that would tell the node's
    viewers.
- **A chain of blockers stops at a node, or a Task, its reader cannot
  see**, and shows nothing beyond it, even something they could read:
  "stops here" and "nothing further" are one answer. Every link in a
  chain is one its reader could open on that node's own page. Where a
  chain is cut for length, "more" speaks only of links they can see.
- **Mentions** (§7.3). The `@` picker lists only the author's own People.
  Anyone other than the Person's owner sees a mention as the name at most —
  never contact details, and never a way into the owner's registry. A
  mention of a deleted Person renders as unresolved.
  - Another reader is given the Person's **current name, as plain text**,
    and only **while the Person's owner can still open the node**. When
    that ends (a share is revoked), the mention shows the name as typed,
    unresolved: otherwise the node's owner would go on reading the name
    of someone else's contact through every rename.
  - A name is given only together with text the reader may read. There is
    no way to ask whose an ID is: a Person's ID is not a secret (it is in
    the raw text, and in a shared Project's People list), so nothing may
    answer to it.
  - "Mentioned in" is the Person's owner's alone, and lists only nodes
    they can open.
  - Deleting a Person leaves the name as typed in the text, in old
    versions and in exports: it is the author's text. Only the link goes.
- **Memberships** (§7.2). Accounts a Project is shared with see a member's
  name and Role only, never contact details. Only the Project's owner
  writes memberships: not its editors, and not the admin.
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
| 2026-10-31 | Resource merges into Reference in 3.0.0 (migration 040); backups made before 3.0 are refused rather than converted | Owner, S2c planning (#222) |
| 2026-10-31 | A `contains` edge may be removed with edit rights on either end, so one home can't trap a child | Developer review of S3a (#224); flagged to the owner |
| 2026-10-31 | `related_to` merged to one edge per pair (migration 041: the oldest kept, a blank label filled from the other, copies archived) | S3b (#226) |
| 2026-10-31 | One home per node in the database (migration 042: each node keeps its oldest home, the rest archived) | Owner, S3c planning (#228) |
| 2026-10-31 | A home the reader can't see is shown to the child's editors as "a home you can't see" with Remove, never named; viewers aren't told | Owner, S3c planning (#228) |
| 2026-10-31 | Captures get their own table; notes without a node become captures, and a note's node is required (migration 043) | Owner, S4a planning (#234) |
| 2026-10-31 | Captures are owner-only, with no admin bypass; in backups, never in search, export or webhooks | Owner, S4a planning (#234) |
| 2026-10-31 | The calendar's day composer makes the chosen Project's task due that day, or else a capture led by the date | Owner, S4a planning (#234) |
| 2026-10-31 | Converting a capture offers a node (with an optional home), a note on a node, or a Task in a Project; Event and Person join with their slices. A Task's title must fit (500 characters): the text is reworded, never cut | Owner, S4b planning (#234) |
| 2026-11-01 | The captures' place is named Captures (the Inbox until then) and holds captures only; a Task without a Project is listed by Data health, not by Captures. Captures are never searched: a glance at the wall is the point | Owner, on #240 (S4c) |
| 2026-11-09 | Archived nodes stay searchable: archiving declutters lists and navigation, and never makes information unfindable | Owner, on the full review (#281) |
| 2026-11-09 | Deleting a node keeps the hard delete of its Tasks, notes and files, other accounts' included, behind a confirmation that counts them first (not the trash, and not unguarded) | Owner, on the full review (#281) |
| 2026-11-09 | Written down from the code, no behaviour change: no self-loop for any edge type (§6); memberships are written by the Project's owner only (§7.2, §14) | Full review (#281), findings 2.3 and 2.4 |
| 2026-11-10 | Revoking a share ends all of that person's access, including to the notes and Tasks they wrote on the node; those stay with the node (§14) | Owner, on the security audit (#281) |
| 2026-11-09 | Project Brief: headline (optional) + pitch + points, Project-only; a save by the same author within the edit-coalesce window updates it in place, any other save supersedes it into History; Brief first, Focus in a later release | Owner, on #282 |
| 2026-11-09 | Focus is persistent, with the time in Focus shown and no "still focused?" nudge; the optional why-line is kept and is private to its writer | Owner, on #282 |
| 2026-11-09 | Focus is per person and private, with no admin bypass, like captures and people | Owner, on #282 |
| 2026-11-11 | What leaves Focus by its own state is decided on the read, never by deleting the entry: today an archived Project, or one its reader can no longer view. Complete and cancelled join when the terminal state is built | Developer, building Focus (#282); to be confirmed by the owner in the PR |
| 2026-11-13 | A Task's completion date is stamped when it becomes Done and cleared when it leaves Done; a Task completed before the date existed has none (never made up). The database checks only that a date means Done: "Done means a date" would lock every older Task against further edits | Developer, building S5a (#314); the owner approved the plan |
| 2026-11-12 | *Parked* is a real, declared Project state beside *active*, *complete* and *cancelled*: the Project is set aside on purpose and should not occupy its person's immediate attention. It is not terminal | Owner, in the developer session ("I am only one man and have to constantly switch context between projects") |
| 2026-11-13 | Existing status labels were read once, by migration 051: exactly *Done / Complete / Completed* → complete; *Cancelled / Canceled / Abandoned* → cancelled; everything else → active. No label parks a Project: parking is declared by a person, and a migration that parked Projects would take them out of their person's view on the day of the deploy. A label that only suggests a state is listed in Data health | Owner ruling 2026-10-30 for the mapping; developer for "no label parks", approved in the S6a plan (#320) |
| 2026-11-13 | A Project's state carries the date it was last declared. A state nobody has declared (read from the label by the migration, or a new Project's *active*) has none: never made up | Developer, building S6a (#320); the owner approved the plan |
| 2026-11-13 | Tasks of a Project that is not *active* are out of My Day's Today and Overdue; they stay in the Backlog and on the Project. The dashboard's Area groups show active Projects, with one collapsed *Parked* group; complete and cancelled Projects leave it. Un-parking returns a Project to Focus as it was. Parking a master does not park its sub-Projects: the downward cascade is for complete and cancelled only | Owner, in the developer session (S6a planning) |
| 2026-11-13 | Closing a Project removes only the declaring person's own Focus entry; anyone else's is hidden while it is closed and shows again if it is re-opened (§10 amended: it read as if re-opening never put a Project back for anyone) | Developer, building S6a (#320); the owner approved the plan |
| 2026-11-13 | Until the cascade exists (S6b, S6c), declaring *complete* or *cancelled* changes the Project alone: its Tasks and sub-Projects stay as they are. S6b must offer the cascade for Projects closed before it existed | Developer, building S6a (#320); the owner approved the plan |
| 2026-11-14 | The cascade's catch-up is **offered, never automatic**: a closed Project with something still open under it says so on its page and in Data health, with a "Close them" action that shows the same confirmation. Under a closed master a sub-Project may be made **neither active nor parked**. Giving an open Project a closed Project as its home is **allowed** and listed in Data health | Owner, in the developer session (S6b planning, #326) |
| 2026-11-14 | Re-declaring the state a Project already has stays a no-op (the catch-up is that request sent with a confirmation). A sub-Project under a master the actor cannot view may be re-opened (an admin reads every Project, so this never applies to one). An admin with no grant of their own on a Project closes it, and under it only what they do hold a grant on. History stores a cascade's other nodes by id and names each only to a reader who may view it; hidden ones are left out, uncounted, and an entry left with nothing for its reader is not shown to them | Developer, building S6b (#326); the owner approved the plan |
| 2026-11-14 | "Neither active nor parked" under a closed master is read as: no change to active, and none from a closed state to parked. An active sub-Project that was moved under a closed master may still be parked (it re-opens nothing, and refusing would leave it only "close it" or "move it") | Developer's reading of the owner's decision above, building S6b (#326); for the owner to confirm |
| 2026-11-14 | The cascade does not go through a sub-Project the actor cannot edit, open or closed. A closed one is not listed as "staying open" | Developer, after the independent review of #326; for the owner to confirm |
| 2026-11-14 | A cascade's webhooks follow the existing rule for each event: `node.updated` to the hooks of each closed Project's owner, `task.updated` (one per Project) to the hooks of the cancelled tasks' owners. An open sub-Project that its master's owner cannot view is in nobody's Data health report | Developer, building S6b (#326) |
| 2026-11-14 | History in the app (S6d): the Notes section **becomes History** on every kind, notes and changes in one chronology, newest first. Saved text versions are called **Versions**. **Activity** shows only the audit trail; status, state, cascade and Brief changes show in History. Existing "edited" marks on notes are **kept**. A move between draft, published and archived is **recorded in History** from v3.23.0 on, on every kind | Owner, in the developer session (S6d planning, #329) |
| 2026-11-14 | A note is "edited" when its words change, and by nothing else (a colour, the same text saved again, a delete and restore are not edits); no "edited by", since only its author can edit it. Edit and Delete are offered to a reader only where the write would pass. A change in History can be opened to show exactly what it holds. Note colours stay as decoration. Writing, editing or deleting a note is logged nowhere else: the note is the record | Developer, building S6d (#329); the owner approved the plan |
| 2026-11-15 | Recurring Tasks removed in the app (S5b, ruling 4): the rule and the link between a task and the one that scheduled it are dropped from the data, and nothing is kept aside; no task is removed. An older backup restores a task that repeated as a one-time task, without a warning. A request that still names a repeat rule (a tab left open across the release) is accepted and the rule ignored | Developer, building S5b (#332); in the plan the owner approved |
| 2026-11-16 | A Task without a Project is no longer possible (S5c; supersedes the line of 2026-11-01 that listed such a Task in Data health). The API refuses to make one, or to take a Task off its Project; moving a Task to another Project stays. The ones that were left **became captures** (§9.1: "the role of the orphan is played by a capture"), at the upgrade and whenever an older backup is restored, with the Task's id, owner, dates and trash state. The capture's text is the title; a Task still to do with a due date has the date in front, and a finished one is marked *Done* or *Cancelled*. Priority, My Day, list order and the completion date are not carried. The restore preview says how many Tasks come back as captures. The database requires a node, not yet a Project: a Task on another kind of node is still refused by the API and listed in Data health | Developer, building S5c (#335); in the plan the owner approved |
| 2026-11-16 | Task dependencies are built now, and lean (S5d): stored, listed and changed in the task editor ("Blocked by", "Blocks"), with a "Blocked" mark on task rows. Blockers shown through a chain, and node-level blockers, wait for the blockers slice. A Task may depend on **any Task its person can see**, in any Project | Owner, in the developer session (S5d planning, #338) |
| 2026-11-16 | A blocked Task stays where it is in every list and can be completed (unlike Waiting, which leaves Today and Overdue). A prerequisite blocks until it is Done, a Cancelled one for good. One in the trash, or on a Project the reader can't see, neither shows nor counts as blocking. The loop check looks at every link, hidden and trashed ones included, so a refusal can tell someone that a chain exists, never through what. Because such a link still decides those refusals, whoever may edit a Task is told how many of its prerequisites they are not shown, unnamed, and may remove them (the hidden-home rule of 2026-10-31, applied after the security review of this slice; the plan had said such a link could not be removed). Adding takes edit rights on the Task that waits and view rights on the other; removing takes edit rights on the Task that waits. A Task waits for at most 50 Tasks, every link counted. The picker offers Tasks still to do on Projects the person owns or was given, never widened for an admin, without archived Projects. A backup holds the links (format 13); a restore leaves out a link to a Task it doesn't bring back, and any loop | Developer, building S5d (#338); in the plan the owner approved |
| 2026-11-16 | A Task's webhook events (`task.created`, `task.updated`, `task.deleted`) go to the webhooks of the Task's author only while that person can still see the Task's node: they own it or hold a grant on it. They used to go there whatever their rights now. An admin's webhooks follow the same rule, without the admin role: a webhook is sent for someone who is not making a request. Supersedes, for who hears of it, the line of 2026-11-14 on a cascade's `task.updated` | Owner, in the developer session (after the security review of S5d, #338) |
| 2026-11-16 | Reverse indexes (S7a). The old hand-made `derived_from` links that point at an Area were filing, not provenance: they are fixed in Data health, one by one or all at once, and nothing changes until a fix is chosen. "Linked Here" folds into the node's Connections as "Mentioned in"; the tool that finds unlinked mentions stays a section of its own | Owner, in the developer session (S7a planning, #342) |
| 2026-11-17 | Blockers (S7b). A blocked Project says so **on its page and on its dashboard card** (and its Focus card), not in the node list. The card also says **how many of the Project's open Tasks are blocked** by other Tasks. A link whose target the source's owner cannot see (a shared editor's link to a node of their own) is never shown to that owner and still decides loop refusals: it gets **its own release right after this slice** (a count and a remove for the node's editors, as Tasks have); this slice stays display-only | Owner, in the developer session (S7b planning, #346) |
| 2026-11-17 | Until that release, such a link does one thing more than the owner was told when deciding: whoever **can** see both ends (the editor who made it, an admin, anyone both nodes are shared with) reads the owner's node as blocked, on its page, on its Focus card and in the chain of what depends on it, while the owner sees nothing and has no list to remove it from. Nothing hidden is shown to anyone. No node is shared in production today | Developer, from the independent review of #347; for the owner to weigh with the release that follows |
| 2026-11-17 | When a node is blocked and what a chain shows (§6.4, §9.3, §14): blocked only while it still waits (in use, and not itself finished or available: a Published Article or a Reference with an unmet dependency does not say "Blocked"), by its own unmet dependencies that the reader can see; the chain goes on only through a dependency that itself still waits, and stops at one the reader cannot see. A Project is judged by its state alone; an archived Article is not Published; an old link to an Area is never met. "Depends on" lists what is not met first and says "Met" or the target's state on every row. A chain is cut at 100 links, the node's own unmet dependencies among them, and says when there is more. Nothing is stored: no migration, nothing new in a backup | Developer, building S7b (#346); in the plan the owner approved. For the owner to confirm: the archived Article, and the cut at 100 |
| 2026-11-17 | A node's editors are told how many of its links they are not shown, and may remove them together, as Tasks have (§14). Until it is removed, such a link stays a real link: it still decides loop refusals, and those who see both ends read the node as blocked (§6.4) | Owner, in the developer session (planning #349) |
| 2026-11-17 | What "not shown" means, for a node's links and for a Task's prerequisites (§14, §9.3): hidden from the reader **and from the owner** of the node (the Task's Project). Hidden from the reader alone would tell an editor of a shared node how many private nodes its owner links it to, and let them delete those links; the Task rule shipped in 3.26.0 had that fault and is narrowed. Which links: those the node's editor could remove if shown (made by hand from the node, and `related_to` either way); not a hidden home, not another node's link into this one, never a wiki link. One number, never which; nothing in History. **Considered and left out:** refusing such a link when it is made. A share ending hides a link after the fact, and a shared Project depending on an editor's private one is a fair use. (The plan also said a refusal would tell an editor whether a third person's node is shared with the owner. The count does not avoid that either: see the next row) | Developer, building #349; in the plan the owner approved |
| 2026-11-17 | From the independent review of #349, decided or left open. **"The node's owner"** in the rule is the account the node belongs to, seeing by ownership or by a share: not someone else holding an owner's grant, and not what an admin opens by role (the database does not record roles). **A prerequisite in the trash** on the owner's private Project is the owner's business like a live one. **Accepted:** two editors of one node can learn together whether the owner sees a third person's node that one of them linked it to. **Open, for the owner:** (1) someone who may edit a Task can move it to a Project of their own, and as that Project's owner is then told how many of its prerequisites they are not shown, and may remove them (they could already delete the Task); it belongs with the open question of whether an editor may move a Task out of a shared Project at all. (2) The hidden-home rule (§6.1) has no owner half: an editor of a shared child is told of, and may free it from, a home that is the owner's own private node | Developer, from the independent review of #349 |
| 2026-11-18 | **Only a Project's owner moves a Task to another person's Project** (§14, §9.3); an editor may still move it between Projects of one account. It closes the two gaps of the row below: keeping a Task after a share ends, and being told as its new Project's owner how many of its prerequisites one cannot see. "The owner" is the account the Project belongs to, as in the rule about links not shown: someone else holding an owner's grant does not pass; an admin does. **The hidden-home rule stays as it is** (#228) | Owner, in the developer session (#351) |
| 2026-11-18 | **People who already share an email address stay editable**, each keeping the address it has; nobody can add an address another Person has; Data health lists them until they are edited or merged (§7.1) | Owner, in the developer session (planning S8a, #353) |
| 2026-11-18 | How §7.1 is built (S8a). The rule is in the database for every writer, as a trigger under a lock per account, **not a unique index and not a `person_emails` table** as the programme had it: a unique key can't say "old pairs may stay, new ones may not". A `person_emails` table with a unique key can replace it once no address is shared and no Person lists one twice. "The same address": spaces off, ASCII case folded, nothing else, the same on every installation. The same address twice on one Person is refused by the API (the database's rule is about two People; a restore brings back a Person who has one twice), and so is an address with a wrapper or one of the usual invisible characters. `?confirm=true` no longer saves anyway. A restore brings back its archive's pairs and its preview counts them. No backup format change. Shared mailboxes are refused from now on; Companies, where §7.1 puts them, come in S10a | Developer, building S8a; in the plan the owner approved |
| 2026-11-18 | **How a mention is written and shown** (§7.3, §14; S9). The raw text holds the name and the ID, `@[name](person:ID)`: the ID is the identity, the page shows the current name, and the name in the text may go stale after a rename. Another reader (a sharee, the admin) sees the current name as plain text, with no link. No report of leftover mentions yet: it comes with the merge tool (S8c) | Owner, in the developer session (planning S9, #360) |
| 2026-11-18 | How S9 is built. A mention is real only if it has a row, made in the transaction that saves the text, for a mention that save adds, of the saver's own People; a token with no row names nobody, and a token someone else put in the text is never made real by a later save, a restored version or a copy (from the security review of #361: a co-editor could otherwise plant a token with the ID of the owner's Person and have the owner's next save name them). A Person is named to another reader only while the Person's owner can still open the node. Names travel only with the text that holds the mention; there is no lookup by ID. The picker is docked at the editor and driven by the keyboard, and `@` followed by Enter is a new line, never a pick. A mention is shown only where Markdown reads a link and the app's own scanner reads a token, as words on the page, with the sanitizer run last on everything: so a name never lands in a writer's own attribute or address (two earlier designs did not hold, per the reviews of #362); the picker offers People by name or the start of an email address, and Enter picks only when a name matches. A stale name in the text is offered for update in the editor (of a body and of a note alike), with "Keep as typed", and is never rewritten unasked. Search finds a mention by the name as typed. Backup format 14 carries the rows. Not in S9: wiki links in notes; mentions in task titles, Briefs, captures and templates; People in the graph; creating a Person from the picker | Developer, building S9; in the plan the owner approved |
| 2026-11-18 | **What a collision offers** (§7.1, S8b). A new Person who collides: one button adds what was typed to the Person who has the address, for review in that Person's editor; nothing is saved until Save. On a node: one button puts that Person on the node with the Role and Note typed; the other details typed are not added to them, and the notice says so. Editing a saved Person and adding another saved Person's address: opening them is all that is offered, and merging two saved People is the merge tool's (S8c) | Owner, in the developer session (planning S8b, #357) |
| 2026-11-18 | How S8b is built. In the browser, as an ordinary edit of the existing Person: no new route, and the database's rule still decides on Save. The merge is made when their editor opens, against the Person as just read (or an unsaved edit of them the device had kept, which the review then says), never at the click. A typed name, company or position that differs is said and not used. Over a limit the entry is added and the save is refused in words until something is removed. A phone is the same number by its digits; notes are compared by whole lines and theirs stay to the letter; an address that belongs to a third Person is left out and said. Nothing is carried over that the device cannot keep, so backing out always has the typed input to go back to. **For S8c:** a Person save should carry the version it was based on, as a node save does | Developer, building S8b; in the plan the owner approved |
| 2026-11-16 | An Area is never a `derived_from` origin (§6.5): new such links are refused, and Data health lists the old ones. Their fixes: the Area becomes the node's home where the kinds allow and it has none; a Reference, never contained, is linked as a source of the Area (the link converted and turned round); otherwise the link is removed. "Active", in the delete and archive warning of §6.3, means not archived and not a complete or cancelled Project. The delete warning is a count of links from such nodes that the reader can see, apart from what the delete destroys. A node's links are shown in named groups, and a remove control only where the remove would pass | Developer, building S7a (#342); in the plan the owner approved |
