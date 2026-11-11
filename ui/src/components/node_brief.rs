//! The Brief section on a Project page (#282): where the project stands
//! *now*. An optional one-line headline, a pitch (markdown) and a short
//! list of points, with the age of the last save.
//!
//! - Viewers read it; whoever may edit the node writes it. The API decides
//!   (`require_editor`); `can_edit` only hides controls that would be
//!   refused.
//! - A save carries the version it started from. A 412 means someone else
//!   saved meanwhile: the editor stays open with the typed text, and says
//!   so, and never overwrites.
//! - Freshness is shown, never enforced (ONTOLOGY §12): a Brief older than
//!   `STALE_AFTER_DAYS` carries a marker. Nothing is hidden or blocked.
//! - "Move Status section into Brief" (§6): the body's `## Status` section
//!   fills the form, and leaves the body once the Brief is saved, so the
//!   story has one home. The Brief is saved **first**: if the body can't be
//!   changed (it moved on, 412), the words are in both places, never in
//!   neither. `common::brief::move_status_section` offers it only when the
//!   whole section fits a Brief.
//!
//! [`BriefBody`] is also the dashboard card's Brief (`compact`).

use chrono::{DateTime, Utc};
use common::brief::{
    BriefInput, MAX_HEADLINE, ProjectBrief, STALE_AFTER_DAYS, StatusMove, move_status_section,
    normalize, points_from_text, points_to_text,
};
use common::id::NodeId;
use common::node::UpdateNodeRequest;
use leptos::prelude::*;

use crate::components::format_helpers::format_relative_short;
use crate::components::icon_button::{IconButton, IconButtonVariant};
use crate::components::load_error::LoadError;
use crate::components::node_section::{NodeSection, SectionShell};
use crate::components::toast::{ToastLevel, push_toast};
use crate::error::UiError;
use crate::markdown::{render_markdown_inline, render_markdown_plain};

/// How many points a dashboard card shows before "+N more".
const CARD_POINTS: usize = 5;

const LABEL_CLASS: &str = "block text-xs font-medium text-stone-600 dark:text-stone-400 mb-1";
const INPUT_CLASS: &str = "w-full text-sm bg-stone-100 dark:bg-stone-800 text-stone-800 \
     dark:text-stone-200 rounded-lg px-2.5 py-1.5 focus:outline-none focus:ring-1 \
     focus:ring-amber-500";
const QUIET_BUTTON: &str = "px-3 py-1.5 text-sm rounded-lg text-stone-600 dark:text-stone-400 \
     hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer";
const COMMIT_BUTTON: &str = "px-3 py-1.5 text-sm font-medium rounded-lg bg-ember \
     hover:bg-ember-strong text-white disabled:opacity-50 disabled:cursor-not-allowed \
     transition-colors flex items-center gap-1.5 cursor-pointer";
const DANGER_BUTTON: &str = "px-3 py-1.5 text-sm font-medium rounded-lg bg-heat-max \
     hover:opacity-90 text-white disabled:opacity-50 disabled:cursor-not-allowed \
     transition-colors flex items-center gap-1.5 cursor-pointer";
const LINK_BUTTON: &str = "underline underline-offset-2 text-stone-700 dark:text-stone-200 \
     hover:text-amber-700 dark:hover:text-amber-400 cursor-pointer";

/// What the editor says when the Brief changed while it was open (412).
const OUTDATED: &str = "Someone else changed this brief while you were editing, so it was not \
     saved. Copy your text, then load the latest.";

/// An API refusal as a sentence: `BriefError`'s text is lower-case and has
/// no full stop, since the API puts it after "Validation error:".
fn sentence(message: &str) -> String {
    let mut chars = message.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// A Brief's words and its age. `compact` is the dashboard card: the pitch
/// is clamped and only the first points show.
#[component]
pub fn BriefBody(brief: ProjectBrief, #[prop(optional)] compact: bool) -> impl IntoView {
    let pitch_html = (!brief.pitch.is_empty()).then(|| render_markdown_plain(&brief.pitch));
    let shown = if compact {
        CARD_POINTS
    } else {
        brief.points.len()
    };
    let hidden = brief.points.len().saturating_sub(shown);
    let points: Vec<String> = brief
        .points
        .iter()
        .take(shown)
        .map(|p| render_markdown_inline(p))
        .collect();
    let stale = brief.is_stale(Utc::now(), STALE_AFTER_DAYS);
    let age = format_relative_short(&brief.updated_at);
    let pitch_class = format!(
        "prose prose-sm dark:prose-invert max-w-none text-stone-700 dark:text-stone-300 \
         [&_p]:my-1 [&_ul]:list-disc [&_ul]:pl-4 [&_ol]:list-decimal [&_ol]:pl-4 [&_li]:my-0.5{}",
        if compact { " line-clamp-6" } else { "" }
    );

    view! {
        <div class="space-y-2 min-w-0" data-testid="brief">
            {brief.headline.clone().map(|h| view! {
                <p class="text-sm font-semibold text-stone-900 dark:text-stone-100 break-words"
                    data-testid="brief-headline">
                    {h}
                </p>
            })}
            {pitch_html.map(|html| view! {
                <div class=pitch_class data-testid="brief-pitch" inner_html=html />
            })}
            {(!points.is_empty()).then(|| view! {
                <ul class="list-disc pl-5 space-y-0.5 text-sm text-stone-700 dark:text-stone-300
                    break-words" data-testid="brief-points">
                    {points.into_iter().map(|html| view! { <li inner_html=html /> }).collect_view()}
                </ul>
            })}
            {(hidden > 0).then(|| view! {
                <p class="text-xs italic text-stone-400 dark:text-stone-500">
                    {format!("+{hidden} more")}
                </p>
            })}
            <p class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs
                text-stone-500 dark:text-stone-400" data-testid="brief-age">
                <span>{format!("Updated {age}")}</span>
                {stale.then(|| view! {
                    <span class="inline-flex items-center gap-1 text-amber-700 dark:text-amber-400"
                        data-testid="brief-stale"
                        title=format!("Not updated in {STALE_AFTER_DAYS} days or more")>
                        <span class="material-symbols-outlined" style="font-size: 14px;"
                            aria-hidden="true">"schedule"</span>
                        "May be out of date"
                    </span>
                })}
            </p>
        </div>
    }
}

/// The editor's state. Signals, so the typed text outlives a re-render of
/// the section (a reload of the Brief while the editor is open).
#[derive(Clone, Copy)]
struct BriefForm {
    headline: RwSignal<String>,
    pitch: RwSignal<String>,
    points: RwSignal<String>,
    /// The version being edited; `None` for a first Brief.
    base: RwSignal<Option<DateTime<Utc>>>,
    /// The body without its Status section, while that section is being
    /// moved into this Brief.
    moved_body: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    /// The Brief changed under the editor (a 412).
    outdated: RwSignal<bool>,
    busy: RwSignal<bool>,
    confirm_remove: RwSignal<bool>,
}

impl BriefForm {
    fn new() -> Self {
        Self {
            headline: RwSignal::new(String::new()),
            pitch: RwSignal::new(String::new()),
            points: RwSignal::new(String::new()),
            base: RwSignal::new(None),
            moved_body: RwSignal::new(None),
            error: RwSignal::new(None),
            outdated: RwSignal::new(false),
            busy: RwSignal::new(false),
            confirm_remove: RwSignal::new(false),
        }
    }

    /// Start editing `current` (or a first Brief).
    fn load(&self, current: Option<&ProjectBrief>) {
        self.headline
            .set(current.and_then(|b| b.headline.clone()).unwrap_or_default());
        self.pitch
            .set(current.map(|b| b.pitch.clone()).unwrap_or_default());
        self.points.set(
            current
                .map(|b| points_to_text(&b.points))
                .unwrap_or_default(),
        );
        self.base.set(current.map(|b| b.updated_at));
        self.moved_body.set(None);
        self.error.set(None);
        self.outdated.set(false);
        self.busy.set(false);
        self.confirm_remove.set(false);
    }

    fn input(&self) -> BriefInput {
        BriefInput {
            headline: Some(self.headline.get_untracked()),
            pitch: self.pitch.get_untracked(),
            points: points_from_text(&self.points.get_untracked()),
            expected_updated_at: self.base.get_untracked(),
        }
    }

    /// Put the body's Status section into the form, after anything typed.
    fn take_status(&self, moved: &StatusMove) {
        self.pitch.update(|p| {
            if !moved.fields.pitch.is_empty() {
                if !p.trim().is_empty() {
                    p.push_str("\n\n");
                }
                p.push_str(&moved.fields.pitch);
            }
        });
        self.points.update(|p| {
            if !moved.fields.points.is_empty() {
                if !p.trim().is_empty() && !p.ends_with('\n') {
                    p.push('\n');
                }
                p.push_str(&points_to_text(&moved.fields.points));
            }
        });
        self.moved_body.set(Some(moved.body.clone()));
    }
}

/// How an edit ended, for the section to act on.
#[derive(Clone, Copy, Default)]
struct Done {
    /// The node's body changed too (its Status section moved out).
    body_changed: bool,
}

/// Take the Status section out of the body, now that the Brief holds it.
/// Returns whether the body changed. A body that has moved on since the
/// page loaded is left alone (412): the section then stays in both places.
async fn remove_status_section(node_id: NodeId, body: String, version: DateTime<Utc>) -> bool {
    let req = UpdateNodeRequest {
        title: None,
        body: Some(body),
        metadata: None,
        status: None,
        project_status: None,
        expected_updated_at: Some(version),
    };
    match crate::api::update_node(node_id, &req).await {
        Ok(_) => {
            push_toast(
                ToastLevel::Success,
                "Brief saved. The Status section moved out of the body.",
            );
            true
        }
        Err(e) => {
            push_toast(
                ToastLevel::Error,
                format!("The brief is saved, but the Status section is still in the body: {e}"),
            );
            false
        }
    }
}

#[component]
fn BriefEditor(
    node_id: NodeId,
    form: BriefForm,
    /// The body's Status section, if it has one that fits a Brief.
    status_move: StoredValue<Option<StatusMove>>,
    /// The node's version when the page loaded, for the body change.
    body_version: DateTime<Utc>,
    on_done: Callback<Done>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    let headline_ref: NodeRef<leptos::html::Input> = NodeRef::new();
    Effect::new(move |_| {
        if let Some(el) = headline_ref.get() {
            let _ = el.focus();
        }
    });

    // Everything after an `.await` uses `try_` access: the page may be gone.
    let refused = move |e: UiError, doing: &str| match e {
        UiError::Api { status: 412, .. } => {
            form.outdated.try_set(true);
            form.error.try_set(Some(OUTDATED.to_string()));
        }
        e => {
            form.error
                .try_set(Some(format!("Couldn't {doing} the brief: {e}")));
        }
    };

    let save = move || {
        if form.busy.get_untracked() {
            return;
        }
        let input = form.input();
        if let Err(e) = normalize(&input) {
            form.error.set(Some(sentence(&e.to_string())));
            return;
        }
        form.busy.set(true);
        form.error.set(None);
        let moved_body = form.moved_body.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            let saved = crate::api::save_brief(node_id, &input).await;
            form.busy.try_set(false);
            match saved {
                Ok(_) => {
                    let body_changed = match moved_body {
                        Some(body) => remove_status_section(node_id, body, body_version).await,
                        None => {
                            push_toast(ToastLevel::Success, "Brief saved");
                            false
                        }
                    };
                    on_done.try_run(Done { body_changed });
                }
                Err(e) => refused(e, "save"),
            }
        });
    };

    let remove = move || {
        let Some(version) = form.base.get_untracked() else {
            return;
        };
        if form.busy.get_untracked() {
            return;
        }
        form.busy.set(true);
        form.error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let removed = crate::api::delete_brief(node_id, version).await;
            form.busy.try_set(false);
            form.confirm_remove.try_set(false);
            match removed {
                Ok(()) => {
                    push_toast(ToastLevel::Success, "Brief removed");
                    on_done.try_run(Done::default());
                }
                Err(e) => refused(e, "remove"),
            }
        });
    };

    let has_offer = status_move.with_value(Option::is_some);

    view! {
        <form
            class="space-y-3"
            data-testid="brief-editor"
            on:submit=move |ev| {
                ev.prevent_default();
                save();
            }
            on:keydown=move |ev| {
                if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
                    ev.prevent_default();
                    save();
                }
            }
        >
            {has_offer.then(|| view! {
                <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 rounded-lg
                    border border-stone-200 dark:border-stone-700 px-3 py-2 text-xs
                    text-stone-600 dark:text-stone-300" data-testid="brief-status-offer">
                    {move || if form.moved_body.with(Option::is_some) {
                        view! {
                            <span>"The Status section will leave the body when you save."</span>
                            <button type="button" class=LINK_BUTTON
                                on:click=move |_| form.moved_body.set(None)>
                                "Keep it in the body"
                            </button>
                        }.into_any()
                    } else {
                        view! {
                            <span>"This project's body has a Status section."</span>
                            <button type="button" class=LINK_BUTTON
                                on:click=move |_| status_move.with_value(|m| {
                                    if let Some(m) = m {
                                        form.take_status(m);
                                    }
                                })>
                                "Move it into the brief"
                            </button>
                        }.into_any()
                    }}
                </div>
            })}
            <div>
                <label for="brief-headline" class=LABEL_CLASS>"Headline"</label>
                <input
                    id="brief-headline"
                    node_ref=headline_ref
                    type="text"
                    class=INPUT_CLASS
                    maxlength=MAX_HEADLINE
                    placeholder="Where it stands, in one line (optional)"
                    prop:value=move || form.headline.get()
                    on:input=move |ev| form.headline.set(event_target_value(&ev))
                />
            </div>
            <div>
                <label for="brief-pitch" class=LABEL_CLASS>"Pitch"</label>
                <textarea
                    id="brief-pitch"
                    rows="5"
                    class=INPUT_CLASS
                    placeholder="What this project is, why it matters, where it stands now"
                    prop:value=move || form.pitch.get()
                    on:input=move |ev| form.pitch.set(event_target_value(&ev))
                />
            </div>
            <div>
                <label for="brief-points" class=LABEL_CLASS>"Points"</label>
                <textarea
                    id="brief-points"
                    rows="4"
                    class=INPUT_CLASS
                    placeholder="One per line: blockers, next moves, things to watch"
                    prop:value=move || form.points.get()
                    on:input=move |ev| form.points.set(event_target_value(&ev))
                />
            </div>
            {move || form.error.get().map(|message| view! {
                <p class="text-xs text-heat-max" role="alert" data-testid="brief-error">
                    {message}
                    {form.outdated.get().then(|| view! {
                        " "
                        <button type="button" class=LINK_BUTTON
                            on:click=move |_| { on_done.run(Done::default()); }>
                            "Load the latest"
                        </button>
                    })}
                </p>
            })}
            {move || if form.confirm_remove.get() {
                view! {
                    <div class="flex flex-wrap items-center justify-between gap-2"
                        data-testid="brief-remove-confirm">
                        <span class="text-xs text-stone-600 dark:text-stone-300">
                            "Remove this brief? Its words stay in the project's Activity."
                        </span>
                        <div class="flex gap-2">
                            <button type="button" class=QUIET_BUTTON
                                on:click=move |_| form.confirm_remove.set(false)>
                                "Keep"
                            </button>
                            <button type="button" class=DANGER_BUTTON
                                disabled=move || form.busy.get()
                                on:click=move |_| remove()>
                                <span class="material-symbols-outlined" style="font-size: 16px;"
                                    aria-hidden="true">"delete"</span>
                                "Remove"
                            </button>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="flex items-center justify-between gap-2">
                        <div>
                            {move || form.base.get().is_some().then(|| view! {
                                <IconButton icon="delete" label="Remove brief"
                                    variant=IconButtonVariant::Danger
                                    on_click=Callback::new(move |()| form.confirm_remove.set(true)) />
                            })}
                        </div>
                        <div class="flex gap-2">
                            <button type="button" class=QUIET_BUTTON
                                on:click=move |_| on_cancel.run(())>
                                "Cancel"
                            </button>
                            <button type="submit" class=COMMIT_BUTTON
                                disabled=move || form.busy.get()>
                                <span class="material-symbols-outlined" style="font-size: 16px;"
                                    aria-hidden="true">"check"</span>
                                {move || if form.busy.get() { "Saving\u{2026}" } else { "Save" }}
                            </button>
                        </div>
                    </div>
                }.into_any()
            }}
        </form>
    }
}

/// The Brief section of a Project page.
#[component]
pub fn BriefPanel(
    node_id: NodeId,
    /// Whether to offer the edit controls. The API still decides.
    can_edit: bool,
    /// The node's body and its version when the page loaded, for "Move
    /// Status section into Brief".
    body: Option<String>,
    body_version: DateTime<Utc>,
    /// The body changed (its Status section moved out): reload the node.
    #[prop(into)]
    on_body_changed: Callback<()>,
) -> impl IntoView {
    let open = RwSignal::new(true);
    let refresh = RwSignal::new(0u32);
    let editing = RwSignal::new(false);
    let form = BriefForm::new();
    let status_move = StoredValue::new(body.as_deref().and_then(move_status_section));
    let brief = LocalResource::new(move || {
        refresh.track();
        crate::api::fetch_brief(node_id)
    });
    // `Some(brief)` once loaded; a failed load offers no editing.
    let loaded = move || brief.get().and_then(Result::ok);

    let start_edit = move || {
        if let Some(current) = loaded() {
            form.load(current.as_ref());
            editing.set(true);
        }
    };
    let on_done = Callback::new(move |done: Done| {
        editing.try_set(false);
        refresh.try_update(|n| *n += 1);
        if done.body_changed {
            on_body_changed.try_run(());
        }
    });
    let on_cancel = Callback::new(move |()| editing.set(false));

    let actions: ChildrenFn = std::sync::Arc::new(move || {
        view! {
            {move || {
                let current = loaded()?;
                (can_edit && open.get() && !editing.get()).then(|| {
                    let label = if current.is_some() { "Edit brief" } else { "Write brief" };
                    view! {
                        <IconButton icon="edit" label=label
                            on_click=Callback::new(move |()| start_edit()) />
                    }
                })
            }}
        }
        .into_any()
    });

    view! {
        <SectionShell section=NodeSection::Brief open=open actions=actions.clone()>
            {move || match brief.get() {
                None => view! {
                    <p class="text-xs text-stone-400">"Loading\u{2026}"</p>
                }.into_any(),
                Some(Err(_)) => view! {
                    <LoadError what="the brief"
                        on_retry=Callback::new(move |()| refresh.update(|n| *n += 1)) />
                }.into_any(),
                Some(Ok(_)) if editing.get() => view! {
                    <BriefEditor node_id=node_id form=form status_move=status_move
                        body_version=body_version on_done=on_done on_cancel=on_cancel />
                }.into_any(),
                Some(Ok(Some(current))) => view! { <BriefBody brief=current /> }.into_any(),
                Some(Ok(None)) => view! {
                    <div class="flex flex-wrap items-center gap-3 text-sm
                        text-stone-500 dark:text-stone-400" data-testid="brief-empty">
                        <span>"No brief yet: where this project stands, in a few lines."</span>
                        {can_edit.then(|| view! {
                            <button type="button" class=COMMIT_BUTTON
                                on:click=move |_| start_edit()>
                                <span class="material-symbols-outlined" style="font-size: 16px;"
                                    aria-hidden="true">"edit"</span>
                                "Write brief"
                            </button>
                        })}
                    </div>
                }.into_any(),
            }}
        </SectionShell>
    }
}

#[cfg(test)]
mod tests {
    use super::sentence;

    #[test]
    fn a_refusal_reads_as_a_sentence() {
        assert_eq!(
            sentence("the headline must be a single line"),
            "The headline must be a single line."
        );
        assert_eq!(sentence(""), "");
    }
}
