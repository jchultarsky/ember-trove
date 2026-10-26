//! Node-page sections (#131): one primitive for every collapsible section
//! under a node's body, plus the header's section outline.
//!
//! Eleven sections hand-rolled the same header (chevron + icon + heading +
//! count) in three different styles: uppercase `h3`s for Tasks and Notes,
//! title-case `h2`s for most, and boxed buttons for Versions and Activity.
//! Only Tasks and Notes could be jumped to. `SectionShell` gives every
//! section one look, a labelled region and a jump target, and publishes its
//! count for the outline.

use std::collections::HashMap;

use leptos::prelude::*;

/// A section of the node page, in page order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NodeSection {
    Tasks,
    Notes,
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
    pub const ALL: [NodeSection; 10] = [
        NodeSection::Tasks,
        NodeSection::Notes,
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

    pub fn icon(self) -> &'static str {
        match self {
            NodeSection::Tasks => "check_box",
            NodeSection::Notes => "sticky_note_2",
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

/// Provided by `NodeView`: the pending "jump to" request, and each
/// section's current count (for the outline).
#[derive(Clone, Copy)]
pub struct NodeSections {
    pub jump: RwSignal<Option<NodeSection>>,
    pub counts: RwSignal<HashMap<NodeSection, usize>>,
}

impl NodeSections {
    pub fn provide() -> Self {
        let sections = NodeSections {
            jump: RwSignal::new(None),
            counts: RwSignal::new(HashMap::new()),
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
                        {NodeSection::ALL.iter().map(|&s| view! {
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
    use super::NodeSection;

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
}
