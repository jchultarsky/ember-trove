//! Node-page sections (#131): one primitive for every collapsible section
//! under a node's body, plus the header's section outline.
//!
//! Eleven sections hand-rolled the same header (chevron + icon + heading +
//! count) in three different styles: uppercase `h3`s for Tasks and Notes,
//! title-case `h2`s for most, and boxed buttons for Versions and Activity.
//! Only Tasks and Notes could be jumped to. `SectionShell` gives every
//! section one look, a labelled region and a jump target, and publishes its
//! count for the outline.

use std::collections::{HashMap, HashSet};

use common::ontology::Capability;
use leptos::prelude::*;

/// A section of the node page, in page order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NodeSection {
    Tasks,
    Notes,
    People,
    Links,
    Connections,
    LocalGraph,
    LinkedHere,
    Attachments,
    Sharing,
    Versions,
    Activity,
}

impl NodeSection {
    /// Page order: the outline lists them this way.
    pub const ALL: [NodeSection; 11] = [
        NodeSection::Tasks,
        NodeSection::Notes,
        NodeSection::People,
        NodeSection::Links,
        NodeSection::Connections,
        NodeSection::LocalGraph,
        NodeSection::LinkedHere,
        NodeSection::Attachments,
        NodeSection::Sharing,
        NodeSection::Versions,
        NodeSection::Activity,
    ];

    pub fn title(self) -> &'static str {
        match self {
            NodeSection::Tasks => "Tasks",
            NodeSection::Notes => "Notes",
            NodeSection::People => "People",
            NodeSection::Links => "External Links",
            NodeSection::Connections => "Connections",
            NodeSection::LocalGraph => "Local Graph",
            NodeSection::LinkedHere => "Linked Here",
            NodeSection::Attachments => "Attachments",
            NodeSection::Sharing => "Sharing",
            NodeSection::Versions => "Version History",
            NodeSection::Activity => "Activity",
        }
    }

    /// Tasks, Notes and People share their titles with the sidebar's global
    /// entries, so their icons must differ from the sidebar's (#173).
    pub fn icon(self) -> &'static str {
        match self {
            NodeSection::Tasks => "check_box",
            NodeSection::Notes => "notes",
            NodeSection::People => "person",
            NodeSection::Links => "open_in_new",
            NodeSection::Connections => "hub",
            NodeSection::LocalGraph => "hub",
            NodeSection::LinkedHere => "link",
            NodeSection::Attachments => "attach_file",
            NodeSection::Sharing => "group",
            NodeSection::Versions => "history_edu",
            NodeSection::Activity => "history",
        }
    }

    /// DOM / test id fragment: `node-<slug>` (`node-tasks`, `node-notes`
    /// are kept from #123).
    pub fn slug(self) -> &'static str {
        match self {
            NodeSection::Tasks => "tasks",
            NodeSection::Notes => "notes",
            NodeSection::People => "people",
            NodeSection::Links => "links",
            NodeSection::Connections => "connections",
            NodeSection::LocalGraph => "local-graph",
            NodeSection::LinkedHere => "linked-here",
            NodeSection::Attachments => "attachments",
            NodeSection::Sharing => "sharing",
            NodeSection::Versions => "versions",
            NodeSection::Activity => "activity",
        }
    }
}

/// Whether a section shows on a node page (#216). One its kind may carry
/// always shows; one it may not (Tasks or People on an Article) shows only
/// while it still holds items from before the rule, so they stay visible
/// and fixable. `total` counts every item, done ones too; `None` (still
/// loading) hides it, so it never flashes up and away.
#[must_use]
pub fn section_shown(allowed: bool, total: Option<usize>) -> bool {
    allowed || total.is_some_and(|n| n > 0)
}

/// The note inside a Tasks or People section on a node whose kind may not
/// carry them (#216): the rule, in the API's own words, and, for the node's
/// owner, where to fix the older items. Data health lists only the reader's
/// own data, so a viewer of someone else's node isn't sent there.
#[component]
pub fn NotOnThisKind(capability: Capability, can_fix: bool) -> impl IntoView {
    let refusal = capability.refusal();
    let sentence = format!("O{}.", refusal.get(1..).unwrap_or_default());
    view! {
        <p class="mb-3 text-xs text-stone-500 dark:text-stone-400" data-testid="not-on-this-kind">
            {sentence}
            {can_fix.then(|| view! {
                " Move these to a Project from "
                <a href="/data-health" class="underline underline-offset-2 hover:text-amber-700 dark:hover:text-amber-400">
                    "Data health"
                </a>
                "."
            })}
        </p>
    }
}

/// Provided by `NodeView`: the pending "jump to" request, and each
/// section's current count (for the outline).
#[derive(Clone, Copy)]
pub struct NodeSections {
    pub jump: RwSignal<Option<NodeSection>>,
    pub counts: RwSignal<HashMap<NodeSection, usize>>,
    /// The sections on the page right now (#216): a section a node's kind
    /// may not carry is left out, so the outline lists only what's there.
    pub present: RwSignal<HashSet<NodeSection>>,
}

impl NodeSections {
    pub fn provide() -> Self {
        let sections = NodeSections {
            jump: RwSignal::new(None),
            counts: RwSignal::new(HashMap::new()),
            present: RwSignal::new(HashSet::new()),
        };
        provide_context(sections);
        sections
    }
}

/// One node-page section: a labelled, focusable region with a toggle
/// heading (`aria-expanded`), an optional count badge and header actions,
/// and a body rendered only while open. Answers "jump to" requests for its
/// `section` by opening, focusing and scrolling to itself.
#[component]
pub fn SectionShell(
    section: NodeSection,
    /// Open state, owned by the section (some load data only while open).
    open: RwSignal<bool>,
    /// The section's item count; `None` hides the badge.
    #[prop(optional, into)]
    count: Option<Signal<Option<usize>>>,
    /// Header buttons on the right (e.g. "Add task").
    #[prop(optional)]
    actions: Option<ChildrenFn>,
    children: ChildrenFn,
) -> impl IntoView {
    let root: NodeRef<leptos::html::Section> = NodeRef::new();
    let heading_id = move || format!("node-{}-heading", section.slug());
    let body_id = move || format!("node-{}-body", section.slug());

    if let Some(sections) = use_context::<NodeSections>() {
        // Register for the outline while mounted (#216); `try_` because the
        // page's context may be gone first when the whole page unmounts.
        sections.present.update(|p| {
            p.insert(section);
        });
        on_cleanup(move || {
            sections.present.try_update(|p| {
                p.remove(&section);
            });
            sections.counts.try_update(|m| {
                m.remove(&section);
            });
        });
        // Publish the count for the outline.
        if let Some(count) = count {
            Effect::new(move |_| {
                let n = count.get();
                sections.counts.update(|m| match n {
                    Some(n) => {
                        m.insert(section, n);
                    }
                    None => {
                        m.remove(&section);
                    }
                });
            });
        }
        // Answer jumps: open, then focus (keyboard and screen-reader users
        // land in the region) and align to the top.
        Effect::new(move |_| {
            if sections.jump.get() != Some(section) {
                return;
            }
            open.set(true);
            sections.jump.set(None);
            if let Some(el) = root.get_untracked() {
                let _ = el.focus();
                el.scroll_into_view_with_bool(true);
            }
        });
    }

    let children = StoredValue::new(children);
    let actions = StoredValue::new(actions);
    let lower = section.title().to_lowercase();
    let toggle_title = move || {
        if open.get() {
            format!("Collapse {lower}")
        } else {
            format!("Expand {lower}")
        }
    };

    view! {
        <section
            node_ref=root
            class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6 focus:outline-none"
            aria-labelledby=heading_id()
            tabindex="-1"
            data-testid=format!("node-{}", section.slug())
        >
            <div class="flex items-center justify-between gap-2">
                <button
                    type="button"
                    class="flex items-center gap-1 text-left cursor-pointer"
                    aria-expanded=move || if open.get() { "true" } else { "false" }
                    aria-controls=body_id()
                    title=toggle_title
                    on:click=move |_| open.update(|v| *v = !*v)
                >
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 16px;"
                        aria-hidden="true"
                    >
                        {move || if open.get() { "expand_more" } else { "chevron_right" }}
                    </span>
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 15px;"
                        aria-hidden="true"
                    >
                        {section.icon()}
                    </span>
                    <h2 id=heading_id() class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                        {section.title()}
                    </h2>
                    {move || {
                        count.and_then(|c| c.get()).filter(|n| *n > 0).map(|n| view! {
                            <span class="ml-1 text-xs bg-stone-200 dark:bg-stone-700
                                        text-stone-600 dark:text-stone-300
                                        rounded-full px-1.5 py-0.5">
                                {n}
                            </span>
                        })
                    }}
                </button>
                {move || actions.with_value(|a| a.as_ref().map(|a| a()))}
            </div>
            {move || open.get().then(|| view! {
                <div id=body_id() class="mt-4">
                    {children.with_value(|c| c())}
                </div>
            })}
        </section>
    }
}

/// The header's "Sections" outline: every section, its count, one click to
/// jump there (#131; generalizes #123's Jump to tasks / notes).
#[component]
pub fn SectionOutline() -> impl IntoView {
    let sections = expect_context::<NodeSections>();
    let open = RwSignal::new(false);
    view! {
        <div class="relative">
            <button
                type="button"
                class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                    dark:hover:text-stone-300 hover:bg-stone-100
                    dark:hover:bg-stone-800 transition-colors"
                title="Sections"
                aria-label="Sections"
                aria-haspopup="menu"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <span class="material-symbols-outlined" aria-hidden="true">"list"</span>
            </button>
            {move || open.get().then(|| view! {
                <>
                    <div class="fixed inset-0 z-20" on:click=move |_| open.set(false) />
                    <div
                        class="absolute right-0 top-full mt-1 w-56 rounded-xl border border-stone-200
                            dark:border-stone-700 bg-white dark:bg-stone-900 shadow-lg z-30 py-1"
                        role="menu"
                        aria-label="Sections"
                    >
                        {NodeSection::ALL
                            .iter()
                            .filter(|s| sections.present.with(|p| p.contains(s)))
                            .map(|&s| view! {
                            <button
                                type="button"
                                role="menuitem"
                                class="flex items-center gap-2 w-full text-left px-3 py-1.5 text-sm
                                    text-stone-700 dark:text-stone-200
                                    hover:bg-stone-100 dark:hover:bg-stone-800"
                                on:click=move |_| {
                                    open.set(false);
                                    sections.jump.set(Some(s));
                                }
                            >
                                <span class="material-symbols-outlined text-stone-400"
                                    style="font-size: 16px;" aria-hidden="true">{s.icon()}</span>
                                <span class="flex-1">{s.title()}</span>
                                {move || sections.counts.with(|m| m.get(&s).copied())
                                    .filter(|n| *n > 0)
                                    .map(|n| view! {
                                        <span class="text-xs text-stone-400">{n}</span>
                                    })}
                            </button>
                        }).collect_view()}
                    </div>
                </>
            })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{NodeSection, section_shown};

    #[test]
    fn a_section_shows_when_allowed_or_while_it_holds_items() {
        assert!(section_shown(true, None), "allowed: no flash while loading");
        assert!(section_shown(true, Some(0)));
        assert!(
            !section_shown(false, None),
            "not allowed: hidden while loading"
        );
        assert!(
            !section_shown(false, Some(0)),
            "not allowed and empty: hidden"
        );
        assert!(section_shown(false, Some(1)), "older items stay visible");
    }

    #[test]
    fn every_section_has_a_unique_slug_and_title() {
        let mut slugs: Vec<_> = NodeSection::ALL.iter().map(|s| s.slug()).collect();
        let mut titles: Vec<_> = NodeSection::ALL.iter().map(|s| s.title()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(slugs.len(), NodeSection::ALL.len());
        assert_eq!(titles.len(), NodeSection::ALL.len());
    }

    /// #173: the same glyph next to the same word reads as the same control,
    /// so a node's own Tasks / Notes / People must not look like the
    /// sidebar's global entries.
    #[test]
    fn sections_named_like_global_entries_use_their_own_icons() {
        use crate::components::sidebar::{NOTES_ICON, PEOPLE_ICON, TASKS_ICON};
        for (section, global_icon) in [
            (NodeSection::Tasks, TASKS_ICON),
            (NodeSection::Notes, NOTES_ICON),
            (NodeSection::People, PEOPLE_ICON),
        ] {
            assert_ne!(section.icon(), global_icon, "{section:?}");
        }
    }
}
