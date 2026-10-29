use chrono::{DateTime, Utc};
use common::{
    id::{NodeId, TemplateId},
    node::{CreateNodeRequest, Node, NodeStatus, NodeType, UpdateNodeRequest},
    save_watch::{
        Conflict, DraftBase, DraftRecovery, EditDraft, GIVE_UP_AFTER_MS, STALL_AFTER_MS, SaveWatch,
        Settled, classify_conflict, fingerprint,
    },
    template::{
        CreateBodySource, NodeTemplate, default_for_type, may_replace_body, picker_order,
        resolve_create_body,
    },
};
use gloo_timers::callback::Timeout;
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;

use crate::app::TemplatePrefill;
use crate::components::toast::{ToastLevel, ToastState, push_toast};
use crate::markdown::render_markdown;
use crate::templates::template_for_type;
use leptos_router::hooks::use_navigate;

fn node_type_label(nt: &NodeType) -> &'static str {
    crate::components::node_meta::type_label(nt.as_str())
}

/// Idle time after the last edit before an autosave PATCH (edit mode) or a
/// localStorage draft write (create mode) fires. Long enough not to interrupt
/// a wiki-link autocomplete interaction mid-typing; short enough to keep the
/// potential loss window tiny.
const AUTOSAVE_DEBOUNCE_MS: u32 = 2_000;

/// The editor's persisted snapshot: (title, body, status). `node_type` is
/// excluded in edit mode because `UpdateNodeRequest` cannot change it.
type Snapshot = (String, String, String);

/// Save lifecycle, surfaced in the header indicator.
#[derive(Clone, Copy, PartialEq)]
enum SaveState {
    /// Pristine — nothing to persist.
    Idle,
    /// Local changes not yet persisted (autosave pending).
    Dirty,
    /// A save is in flight.
    Saving,
    /// A save has been in flight longer than `STALL_AFTER_MS` (#161).
    Stalled,
    /// Persisted — server-side in edit mode, localStorage draft in create mode.
    Saved,
    /// Last save attempt failed; the edits are still held locally.
    Failed,
}

impl SaveState {
    /// Indicator label. Create-mode wording must not imply the node exists
    /// on the server — only a local draft does. `saved_at` (local time of the
    /// last server save) turns "Saved" into "Saved · 4:02 PM", so a glance
    /// answers "did my last keystrokes land?" (#98 item 6).
    fn label(self, edit_mode: bool, saved_at: Option<&str>) -> String {
        match (self, saved_at) {
            (SaveState::Idle, _) => String::new(),
            (SaveState::Dirty, _) => "Unsaved changes\u{2026}".into(),
            (SaveState::Saving, _) => "Saving\u{2026}".into(),
            (SaveState::Stalled, _) => "Still saving\u{2026}".into(),
            (SaveState::Saved, Some(at)) if edit_mode => format!("Saved \u{b7} {at}"),
            (SaveState::Saved, None) if edit_mode => "Saved".into(),
            (SaveState::Saved, _) => "Draft kept locally".into(),
            (SaveState::Failed, _) => "Couldn\u{2019}t save \u{2014} edits kept here".into(),
        }
    }

    /// Pending states read a shade stronger than the settled "Saved".
    fn is_pending(self) -> bool {
        matches!(
            self,
            SaveState::Dirty | SaveState::Saving | SaveState::Stalled
        )
    }
}

/// Fingerprint of an editor snapshot (`common::save_watch::fingerprint`).
fn fp(snap: &Snapshot) -> u64 {
    fingerprint(&snap.0, &snap.1, &snap.2)
}

/// The edit-mode save request for `snap`, conditional on `expected`.
fn update_req(snap: &Snapshot, expected: Option<DateTime<Utc>>) -> UpdateNodeRequest {
    UpdateNodeRequest {
        title: Some(snap.0.clone()),
        body: Some(snap.1.clone()),
        metadata: None,
        status: Some(NodeStatus::parse(&snap.2).unwrap_or(NodeStatus::Draft)),
        project_status: None,
        expected_updated_at: expected,
    }
}

/// Why an edit-mode save failed.
enum SaveError {
    /// The node's text changed somewhere else since this editor loaded it;
    /// carries the server's current version, which an explicit Retry may
    /// overwrite.
    ChangedElsewhere(DateTime<Utc>),
    Other(crate::error::UiError),
}

/// Save `snap` only if the node is still at `expected` (#161 follow-up).
///
/// A save the editor gave up on can still land, and must never overwrite a
/// newer one: the API refuses a stale version with 412. Then the server's
/// text decides. If it is text this editor sent (`own`: the baseline and
/// every sent snapshot), our own late save landed (or only the version
/// moved, e.g. a pin elsewhere), so `snap` is re-sent once on the new
/// version. Anything else was written elsewhere and is not overwritten
/// unasked.
async fn put_unless_changed(
    id: NodeId,
    snap: &Snapshot,
    expected: Option<DateTime<Utc>>,
    own: &[u64],
    signal: Option<&web_sys::AbortSignal>,
) -> Result<Node, SaveError> {
    use crate::error::UiError;
    let stale = |e: &UiError| matches!(e, UiError::Api { status: 412, .. });
    match crate::api::update_node_abortable(id, &update_req(snap, expected), signal).await {
        Err(e) if stale(&e) => {
            let server = crate::api::fetch_node(id).await.map_err(SaveError::Other)?;
            let server_fp = fingerprint(
                &server.title,
                server.body.as_deref().unwrap_or_default(),
                server.status.as_str(),
            );
            match classify_conflict(server_fp, own) {
                Conflict::OwnWrite => crate::api::update_node_abortable(
                    id,
                    &update_req(snap, Some(server.updated_at)),
                    signal,
                )
                .await
                .map_err(SaveError::Other),
                Conflict::Foreign => Err(SaveError::ChangedElsewhere(server.updated_at)),
            }
        }
        other => other.map_err(SaveError::Other),
    }
}

/// Shown when a save was refused because the node changed elsewhere.
const CHANGED_ELSEWHERE_MSG: &str = "This node was changed somewhere else since you opened \
     it. Your edits are kept on this device. Retry saves them over that change (it stays \
     in History).";

/// The editor snapshot as a storable edit-mode draft (#161), with the
/// fingerprints of the server text it started from (`own_fps`), so a later
/// visit can tell whether the node changed since.
fn draft_of(snap: &Snapshot, base_fps: Vec<u64>) -> EditDraft {
    EditDraft {
        title: snap.0.clone(),
        body: snap.1.clone(),
        status: snap.2.clone(),
        base_fps,
        saved_at: None, // stamped by crate::draft on write
    }
}

/// The draft-recovery banner text for a draft whose base is `base`.
fn recovery_message(base: DraftBase) -> &'static str {
    match base {
        DraftBase::Unchanged => {
            "This device kept edits to this node that never reached the server."
        }
        DraftBase::Changed => {
            "This device kept edits to this node that never reached the server, and the \
             node has changed since. Restoring replaces that change (it stays in History)."
        }
        DraftBase::Unknown => {
            "This device kept edits to this node that never reached the server. The node \
             may have changed since; restoring replaces it (it stays in History)."
        }
    }
}

/// Convert a UTF-16 code-unit offset (as reported by `selectionStart` in the
/// DOM) to a UTF-8 byte offset safe for slicing a Rust `&str`.
///
/// JavaScript string APIs count positions in UTF-16 code units, but Rust
/// strings are UTF-8. Slicing by a UTF-16 offset can land mid-char and
/// trigger `core::str::slice_error_fail` — e.g. right after an emoji (👉),
/// em-dash (—), curly quote (’), or `π`. The resulting WASM panic poisons
/// the Leptos event-dispatch `RefCell`, so every subsequent keystroke is
/// silently dropped and the user's edit reverts on save. Always route cursor
/// values from `selection_start()` through this helper before using them to
/// index `text`.
fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0usize;
    for (byte_idx, ch) in text.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }
    text.len()
}

/// Inverse of [`utf16_to_utf8_offset`]: convert a UTF-8 byte offset back to a
/// UTF-16 code-unit offset for passing to DOM APIs like `setSelectionRange`.
fn utf8_to_utf16_offset(text: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(text.len());
    text[..clamped].encode_utf16().count()
}

/// Return the partial wiki-link query being typed at the cursor, if any.
///
/// Looks backwards from `cursor` for an unclosed `[[`. Returns the text
/// typed after `[[` up to the cursor, or `None` if the cursor is not inside
/// an open wiki-link context. `cursor` is a UTF-16 code-unit offset (as
/// delivered by `selectionStart`); the helper converts it to a UTF-8 byte
/// offset before slicing so non-ASCII text cannot trigger a char-boundary
/// panic.
fn wikilink_query_at(text: &str, cursor: usize) -> Option<String> {
    let byte_cut = utf16_to_utf8_offset(text, cursor);
    let before = &text[..byte_cut];
    // Find the last `[[` that has not been closed.
    let open = before.rfind("[[")?;
    let after_open = &before[open + 2..];
    // If there's already a closing `]]` or a newline between `[[` and cursor,
    // we are not in a wiki-link context.
    if after_open.contains("]]") || after_open.contains('\n') {
        return None;
    }
    Some(after_open.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offset_ascii_matches_byte_offset() {
        assert_eq!(utf16_to_utf8_offset("hello", 3), 3);
        assert_eq!(utf16_to_utf8_offset("hello", 99), 5);
    }

    #[test]
    fn utf16_offset_handles_emoji_surrogate_pair() {
        // "a👉b" — UTF-16: a(1) + 👉(2) + b(1) = 4 units.
        //          UTF-8:  a(1) + 👉(4) + b(1) = 6 bytes.
        let text = "a👉b";
        assert_eq!(utf16_to_utf8_offset(text, 0), 0);
        assert_eq!(utf16_to_utf8_offset(text, 1), 1);
        assert_eq!(utf16_to_utf8_offset(text, 3), 5);
        assert_eq!(utf16_to_utf8_offset(text, 4), 6);
    }

    #[test]
    fn utf16_offset_handles_multibyte_bmp_chars() {
        // Em-dash: 3 UTF-8 bytes, 1 UTF-16 unit. π: 2 UTF-8 bytes, 1 UTF-16 unit.
        let text = "a—πb";
        assert_eq!(utf16_to_utf8_offset(text, 1), 1);
        assert_eq!(utf16_to_utf8_offset(text, 2), 4);
        assert_eq!(utf16_to_utf8_offset(text, 3), 6);
        assert_eq!(utf16_to_utf8_offset(text, 4), 7);
    }

    #[test]
    fn utf8_to_utf16_is_inverse_at_char_boundaries() {
        let text = "a👉b—πc";
        for (byte_idx, _) in text.char_indices() {
            let u16 = utf8_to_utf16_offset(text, byte_idx);
            assert_eq!(utf16_to_utf8_offset(text, u16), byte_idx);
        }
    }

    #[test]
    fn wikilink_query_does_not_panic_past_emoji() {
        // Regression: before the UTF-16 conversion, this panicked in WASM,
        // poisoning the Leptos event-dispatch RefCell so every keystroke
        // after the first was silently dropped and the edit reverted on save.
        let text = "> 👉 **TIP**: note";
        let cursor_u16 = text.encode_utf16().count();
        assert_eq!(wikilink_query_at(text, cursor_u16), None);
    }

    #[test]
    fn wikilink_query_returns_partial_after_open_bracket() {
        let text = "See [[foo";
        let cursor_u16 = text.encode_utf16().count();
        assert_eq!(wikilink_query_at(text, cursor_u16), Some("foo".to_string()));
    }

    #[test]
    fn save_state_saved_label_distinguishes_modes() {
        // Create mode persists only a local draft — the label must not claim
        // the node was saved to the server.
        assert_eq!(SaveState::Saved.label(true, None), "Saved");
        assert_eq!(SaveState::Saved.label(false, None), "Draft kept locally");
        assert_eq!(SaveState::Idle.label(true, None), "");
    }

    #[test]
    fn a_stalled_save_says_so_and_still_counts_as_pending() {
        // #161: "saving", "saved" and "stuck" must read differently.
        assert_eq!(SaveState::Stalled.label(true, None), "Still saving\u{2026}");
        assert_ne!(
            SaveState::Stalled.label(true, None),
            SaveState::Saving.label(true, None)
        );
        assert!(SaveState::Stalled.is_pending());
        assert!(!SaveState::Failed.is_pending());
    }

    #[test]
    fn save_state_saved_label_carries_the_save_time() {
        assert_eq!(
            SaveState::Saved.label(true, Some("4:02 PM")),
            "Saved \u{b7} 4:02 PM"
        );
        // A local draft never claims a server save time.
        assert_eq!(
            SaveState::Saved.label(false, Some("4:02 PM")),
            "Draft kept locally"
        );
        assert_eq!(
            SaveState::Dirty.label(true, Some("4:02 PM")),
            "Unsaved changes\u{2026}"
        );
    }
}

/// Returns `true` if the browser viewport is ≥ 768 px wide (≈ tablet or larger).
/// Defaults to `true` (preview visible) if `window` is unavailable.
fn is_wide_viewport() -> bool {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .map(|w| w >= 768.0)
        .unwrap_or(true)
}

#[component]
pub fn NodeEditor(node: Option<NodeId>) -> impl IntoView {
    let navigate = use_navigate();
    let nav_back1 = navigate.clone(); // clone for back button in header
    // For the save paths (Done, Revert, Create), which navigate after their
    // save lands; stored so `on_save` stays Copy for two buttons
    // (NavigateFn is Clone, not Copy: leptos.md).
    let nav_store = StoredValue::new(navigate.clone());
    let refresh = expect_context::<RwSignal<u32>>();

    // In create mode, pre-select the type from the active node_type_filter so
    // that opening the editor from e.g. the Projects list defaults to Project.
    // In edit mode the spawn_local block below will override this immediately.
    let node_type_filter = use_context::<RwSignal<Option<String>>>();
    let prefill_signal = use_context::<RwSignal<Option<TemplatePrefill>>>();
    // `/nodes/new?type=project` (sidebar "+", palette "New Project") presets
    // the type explicitly; it wins over the list filter. Unknown values are
    // ignored.
    let query_type: Option<String> = leptos_router::hooks::use_query_map()
        .with_untracked(|q| q.get("type"))
        .filter(|t| NodeType::parse(t).is_some());

    // The signed-in user: both draft kinds are keyed by it (#161). None (never
    // expected inside AuthGate) turns drafts off rather than sharing them.
    let subject: Option<String> =
        use_context::<crate::auth::AuthState>().and_then(|a| match a.get_untracked() {
            crate::auth::AuthStatus::Authenticated(u) => Some(u.sub),
            _ => None,
        });
    let create_subject = StoredValue::new(node.is_none().then(|| subject.clone()).flatten());

    // Create-mode initial state follows `common::template::resolve_create_body`
    // (#111): a TemplatePrefill context (consumed/cleared immediately), a
    // locally persisted draft (work rescued from an abandoned /nodes/new — see
    // crate::draft), the type's default template, or the built-in scaffold.
    // Templates load asynchronously, so the default is applied by an Effect
    // below once they arrive — while the body is still the scaffold.
    let (default_type, initial_body, initial_template_id, initial_title, initial_status) =
        if node.is_none() {
            if let Some(sig) = prefill_signal
                && let Some(p) = sig.get_untracked()
            {
                sig.set(None);
                (
                    p.node_type,
                    p.body,
                    Some(p.template_id),
                    String::new(),
                    "draft".to_string(),
                )
            } else if let Some(d) = subject
                .as_deref()
                .and_then(crate::draft::read_draft)
                .filter(|d| d.is_meaningful(template_for_type(&d.node_type)))
            {
                (d.node_type, d.body, None, d.title, d.status)
            } else {
                let nt = query_type
                    .clone()
                    .or_else(|| node_type_filter.and_then(|f| f.get_untracked()))
                    .unwrap_or_else(|| "article".to_string());
                let body = template_for_type(&nt).to_string();
                (nt, body, None, String::new(), "draft".to_string())
            }
        } else {
            (
                "article".to_string(),
                String::new(),
                None,
                String::new(),
                "draft".to_string(),
            )
        };

    // The body the system last put in the editor (scaffold, default template,
    // prefill, or a picked template). The system may replace the body only
    // while it still equals this (`may_replace_body`); `None` for a rescued
    // draft or edit mode — never overwritten.
    let rescued_draft = node.is_none()
        && initial_template_id.is_none()
        && initial_body != template_for_type(&default_type);
    let last_system_body: RwSignal<Option<String>> =
        RwSignal::new((node.is_none() && !rescued_draft).then(|| initial_body.clone()));
    let title = RwSignal::new(initial_title);
    let node_type = RwSignal::new(default_type);
    // In create mode, pre-populate the body from template or static scaffold.
    // In edit mode spawn_local below will overwrite this with the real body.
    let body = RwSignal::new(initial_body);
    // Template ID used when creating a node from a template (for activity log).
    let template_id_for_create = RwSignal::new(initial_template_id);
    // Selected template value string (drives the <select> prop:value binding).
    let selected_template_value = RwSignal::new(
        initial_template_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
    );

    // Fetch templates for the create-mode picker (no-op overhead in edit mode
    // since the picker is hidden; the resource is lazily evaluated).
    let templates_resource = LocalResource::new(crate::api::list_templates);
    let available_templates: RwSignal<Vec<NodeTemplate>> = RwSignal::new(vec![]);
    Effect::new(move |_| {
        if let Some(Ok(ts)) = templates_resource.get() {
            available_templates.set(ts);
        }
    });
    // Apply the type's default template once templates arrive — the same
    // rule as the create modal — but only on top of the untouched built-in
    // scaffold (never over a prefill, a draft, or anything typed).
    Effect::new(move |_| {
        let ts = available_templates.get();
        if node.is_some() || template_id_for_create.get_untracked().is_some() {
            return;
        }
        let nt = node_type.get_untracked();
        let Some(last) = last_system_body.get_untracked() else {
            return;
        };
        if last != template_for_type(&nt) || !may_replace_body(&body.get_untracked(), &last) {
            return;
        }
        if let Some(d) = NodeType::parse(&nt).and_then(|t| default_for_type(&ts, &t)) {
            body.set(d.body.clone());
            last_system_body.set(Some(d.body.clone()));
            template_id_for_create.set(Some(d.id));
            selected_template_value.set(d.id.to_string());
        }
    });
    // A type change in create mode (the type <select>, or a new `?type=` while
    // the editor stays mounted): re-resolve the body for the new type — its
    // default template, else the built-in scaffold — but only while the body
    // is untouched (`may_replace_body`).
    let apply_type_change = move |new_type: String| {
        if node.is_none()
            && last_system_body
                .get_untracked()
                .is_some_and(|last| may_replace_body(&body.get_untracked(), &last))
        {
            let ts = available_templates.get_untracked();
            let default = NodeType::parse(&new_type).and_then(|t| default_for_type(&ts, &t));
            let (new_body, source) =
                resolve_create_body(None, None, default, template_for_type(&new_type));
            body.set(new_body.clone());
            last_system_body.set(Some(new_body));
            match source {
                CreateBodySource::Default(id) => {
                    template_id_for_create.set(Some(id));
                    selected_template_value.set(id.to_string());
                }
                _ => {
                    template_id_for_create.set(None);
                    selected_template_value.set(String::new());
                }
            }
        }
        node_type.set(new_type);
    };
    // `/nodes/new?type=a` → `?type=b` reuses the mounted editor (two sidebar
    // "+" clicks in a row): follow the query like a type change.
    if node.is_none() {
        let query = leptos_router::hooks::use_query_map();
        Effect::new(move |prev: Option<Option<String>>| {
            let t = query
                .with(|q| q.get("type"))
                .filter(|t| NodeType::parse(t).is_some());
            if let (Some(prev), Some(new)) = (prev, t.clone())
                && prev.as_ref() != Some(&new)
            {
                apply_type_change(new);
            }
            t
        });
    }
    let status = RwSignal::new(initial_status);
    let saving = RwSignal::new(false);
    // True while the initial node fetch is in-flight (edit mode only).
    // The Save button is disabled until this clears to prevent saving stale
    // signal values if the user clicks Save before fetch_node completes.
    let fetching = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);

    // ── Autosave state ──────────────────────────────────────────────────────
    // Last server-persisted snapshot; None until the edit-mode fetch lands
    // (autosave stays disabled while None) and always None in create mode,
    // where changes go to a localStorage draft instead.
    let baseline = RwSignal::new(Option::<Snapshot>::None);
    let save_state = RwSignal::new(SaveState::Idle);
    // Local time of the last successful server save (edit mode).
    let saved_at: RwSignal<Option<String>> = RwSignal::new(None);
    // Debounce version counter (see .claude/patterns/reactive-effect-debounce.rs).
    let autosave_v = RwSignal::new(0u32);
    // Submit trigger (see .claude/patterns/submit-trigger.rs): the debounce
    // timeout and the post-save recheck both set it; one effect does the PATCH.
    let autosave_now = RwSignal::new(false);
    // Captured directly (not via push_toast) so the unmount flush can report
    // its outcome after this component's reactive owner is gone.
    let toast_state = use_context::<ToastState>();
    // ── Save watchdog & edit drafts (#161) ─────────────────────────────────
    // Numbers saves so timers and responses act only on their own save.
    let watch = StoredValue::new(SaveWatch::default());
    // The node version the editor's text is based on (`updated_at` of the
    // last load or confirmed save); every edit-mode save is conditional on it.
    let base_version: RwSignal<Option<DateTime<Utc>>> = RwSignal::new(None);
    // Fingerprints of the baseline and every snapshot sent since: a 412
    // whose server text is one of these is our own late save (see
    // `put_unless_changed`).
    let own_fps: StoredValue<Vec<u64>> = StoredValue::new(Vec::new());
    // Set when a save was refused because the node changed elsewhere: the
    // server's version, adopted only by an explicit Retry.
    let conflict_version: RwSignal<Option<DateTime<Utc>>> = RwSignal::new(None);
    // Done or Revert asked to leave the editor once its save lands.
    let nav_after_save = RwSignal::new(false);
    // The node as the editor opened it (edit mode): Revert's target (#197).
    let opened = RwSignal::new(Option::<Snapshot>::None);
    // A local draft found on load that the server doesn't have, and whether
    // the node changed since it was taken.
    let recovered = RwSignal::new(Option::<(EditDraft, DraftBase)>::None);
    // Drafts are keyed by user and node; with no signed-in user (or in
    // create mode, which has its own draft) they are off.
    let draft_slot = StoredValue::new(node.and_then(|id| Some((subject.clone()?, id.to_string()))));
    let keep_draft = move |snap: &Snapshot| {
        if let Some(Some((sub, id))) = draft_slot.try_get_value() {
            let base = own_fps.try_get_value().unwrap_or_default();
            crate::draft::write_edit_draft(&sub, &id, &draft_of(snap, base));
        }
    };
    let drop_draft = move || {
        if let Some(Some((sub, id))) = draft_slot.try_get_value() {
            crate::draft::clear_edit_draft(&sub, &id);
        }
    };
    // Arm the watchdog for a save that just started: "Still saving…" after
    // STALL_AFTER_MS; at GIVE_UP_AFTER_MS the save is abandoned. Giving up
    // releases the lock and shows the failure by itself, without waiting
    // for the request to fail: a save that is sent to the login page never
    // settles (`api::send_authed` parks it), so the editor used to sit on
    // "Still saving…" forever. The timers may outlive the editor, hence the
    // `try_` accessors.
    let arm_watchdog = move |abort: Option<web_sys::AbortController>| -> u32 {
        let g = watch.try_update_value(|w| w.begin()).unwrap_or_default();
        Timeout::new(STALL_AFTER_MS, move || {
            if watch.try_with_value(|w| w.is_current(g)) == Some(true)
                && save_state.try_get_untracked() == Some(SaveState::Saving)
            {
                save_state.try_set(SaveState::Stalled);
            }
        })
        .forget();
        Timeout::new(GIVE_UP_AFTER_MS, move || {
            if watch.try_update_value(|w| w.give_up(g)) != Some(true) {
                return;
            }
            if let Some(a) = &abort {
                a.abort();
            }
            nav_after_save.try_set(false);
            saving.try_set(false);
            save_state.try_set(SaveState::Failed);
            let retry = if node.is_some() {
                "Retry to send them again"
            } else {
                "Create again to retry"
            };
            error_msg.try_set(Some(format!(
                "The save took too long and was stopped. Your edits are kept on this \
                 device; {retry}."
            )));
            if let (Some(Some((sub, id))), Some(t), Some(b), Some(st)) = (
                draft_slot.try_get_value(),
                title.try_get_untracked(),
                body.try_get_untracked(),
                status.try_get_untracked(),
            ) {
                let base = own_fps.try_get_value().unwrap_or_default();
                crate::draft::write_edit_draft(&sub, &id, &draft_of(&(t, b, st), base));
            }
        })
        .forget();
        g
    };

    // Current editor snapshot, untracked — shared by autosave, the unmount
    // flush, and the beforeunload guard.
    let snapshot_now = move || {
        (
            title.get_untracked(),
            body.get_untracked(),
            status.get_untracked(),
        )
    };

    // Preview visibility — starts visible on wide viewports, hidden on narrow.
    let show_preview = RwSignal::new(is_wide_viewport());

    // Image drag-and-drop / paste upload state.
    let img_drag_over: RwSignal<bool> = RwSignal::new(false);
    let img_uploading: RwSignal<bool> = RwSignal::new(false);
    // Monotonic counter to generate unique placeholder strings for concurrent uploads.
    let upload_counter: RwSignal<u32> = RwSignal::new(0);

    // Wiki-link autocomplete state.
    let wikilink_query = RwSignal::new(Option::<String>::None);
    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();
    // Create mode lands on the title: "one click, then typing" (#111). Tracking
    // the NodeRef in an Effect (not requestAnimationFrame) per leptos.md.
    let title_ref = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if node.is_none()
            && let Some(el) = title_ref.get()
        {
            let _ = el.focus();
        }
    });

    // Helper: upload one image File and insert Markdown at the current cursor position.
    // `node` is captured by value (Copy); all signals are Copy too.
    let upload_image_file = move |file: web_sys::File| {
        let Some(node_id) = node else {
            push_toast(
                ToastLevel::Error,
                "Save the node first before uploading images.",
            );
            return;
        };
        // Claim a unique placeholder ID before entering the async block.
        let uid = upload_counter.get_untracked() + 1;
        upload_counter.set(uid);
        let placeholder = format!("![uploading-{uid}\u{2026}]()");

        // Insert placeholder at cursor (or end of text if cursor unavailable).
        // NodeRef<Textarea>.get() deref-chains to web_sys::HtmlElement; use
        // dyn_ref to reach HtmlTextAreaElement and call selection_start.
        // `selection_start` returns a UTF-16 code-unit offset — convert to
        // a UTF-8 byte offset before slicing (see `utf16_to_utf8_offset`).
        let cursor_u16 = textarea_ref
            .get()
            .and_then(|el| {
                use std::ops::Deref as _;
                use wasm_bindgen::JsCast as _;
                el.deref()
                    .dyn_ref::<web_sys::HtmlTextAreaElement>()
                    .and_then(|ta| ta.selection_start().ok().flatten())
            })
            .unwrap_or(0) as usize;
        let current = body.get_untracked();
        let cursor = utf16_to_utf8_offset(&current, cursor_u16);
        let new_val = format!(
            "{}{}{}",
            &current[..cursor],
            placeholder,
            &current[cursor..]
        );
        body.set(new_val.clone());
        if let Some(el) = textarea_ref.get() {
            el.set_value(&new_val);
            // set_selection_start expects a UTF-16 code-unit offset.
            let placeholder_u16_len: usize = placeholder.chars().map(char::len_utf16).sum();
            let pos = (cursor_u16 + placeholder_u16_len) as u32;
            let _ = el.set_selection_start(Some(pos));
            let _ = el.set_selection_end(Some(pos));
        }

        img_uploading.set(true);
        let filename = file.name();
        // Cast File → Blob for FormData.
        let blob: &web_sys::Blob = file.unchecked_ref();
        let Ok(form_data) = web_sys::FormData::new() else {
            push_toast(ToastLevel::Error, "Failed to create form data.");
            img_uploading.set(false);
            return;
        };
        if form_data
            .append_with_blob_and_filename("file", blob, &filename)
            .is_err()
        {
            push_toast(ToastLevel::Error, "Failed to attach file.");
            img_uploading.set(false);
            return;
        }

        let placeholder_clone = placeholder.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::upload_attachment(node_id, form_data).await {
                Ok(att) => {
                    let url = crate::api::attachment_download_url(att.id);
                    let final_md = format!("![{filename}]({url})");
                    let updated = body
                        .get_untracked()
                        .replacen(&placeholder_clone, &final_md, 1);
                    body.set(updated.clone());
                    if let Some(el) = textarea_ref.get() {
                        el.set_value(&updated);
                    }
                }
                Err(e) => {
                    // Remove the placeholder on failure.
                    let updated = body.get_untracked().replacen(&placeholder_clone, "", 1);
                    body.set(updated.clone());
                    if let Some(el) = textarea_ref.get() {
                        el.set_value(&updated);
                    }
                    push_toast(ToastLevel::Error, format!("Image upload failed: {e}"));
                }
            }
            img_uploading.set(false);
        });
    };

    // Wiki links: the preview resolves just the titles the body links, and
    // `[[` suggestions come from the title lookup — no full title list (#134).
    let wikilink_map = crate::wikilink::use_wikilink_map(body.into());
    let suggest_q = RwSignal::new(String::new());
    Effect::new(move |_| suggest_q.set(wikilink_query.get().unwrap_or_default()));
    let suggestions = crate::components::node_picker::use_title_suggestions(suggest_q);

    // If editing, fetch existing node data.
    if let Some(id) = node {
        fetching.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::fetch_node(id).await {
                Ok(n) => {
                    let snap: Snapshot = (
                        n.title.clone(),
                        n.body.clone().unwrap_or_default(),
                        n.status.as_str().to_string(),
                    );
                    title.set(snap.0.clone());
                    body.set(snap.1.clone());
                    node_type.set(n.node_type.as_str().to_string());
                    status.set(snap.2.clone());
                    // Edits this device kept but the server never got (#161).
                    if let Some(Some((sub, nid))) = draft_slot.try_get_value()
                        && let Some(d) = crate::draft::read_edit_draft(&sub, &nid)
                    {
                        match d.recovery((&snap.0, &snap.1, &snap.2)) {
                            DraftRecovery::Discard => crate::draft::clear_edit_draft(&sub, &nid),
                            DraftRecovery::Offer(base) => recovered.set(Some((d, base))),
                        }
                    }
                    base_version.set(Some(n.updated_at));
                    own_fps.set_value(vec![fp(&snap)]);
                    opened.set(Some(snap.clone()));
                    baseline.set(Some(snap));
                    fetching.set(false);
                }
                Err(e) => {
                    // Deliberately leave `fetching` true: it keeps Save (and
                    // autosave, via the None baseline) disabled, so a failed
                    // load can never be saved back as an empty body over the
                    // real node.
                    error_msg.set(Some(format!("Couldn't load node: {e}")));
                }
            }
        });
    }

    // ── Autosave wiring ─────────────────────────────────────────────────────
    // Debounced change watcher: edit mode schedules an autosave PATCH; create
    // mode persists the draft locally. Re-runs on every keystroke; only the
    // latest scheduled timeout commits (version-counter pattern).
    Effect::new(move |prev: Option<()>| {
        let snap: Snapshot = (title.get(), body.get(), status.get());
        if node.is_some() {
            let Some(base) = baseline.get() else {
                return; // initial fetch not landed (or failed) — autosave off
            };
            if snap == base {
                return;
            }
            save_state.set(SaveState::Dirty);
            let v = autosave_v.get_untracked() + 1;
            autosave_v.set(v);
            // A forgotten timer outlives the editor (Done, Revert or a link
            // within the debounce): `try_`, or it panics on disposed signals.
            // The unmount flush has already saved those edits.
            Timeout::new(AUTOSAVE_DEBOUNCE_MS, move || {
                if autosave_v.try_get_untracked() == Some(v) {
                    autosave_now.try_set(true);
                }
            })
            .forget();
        } else {
            let d = crate::draft::CreateDraft {
                title: snap.0,
                body: snap.1,
                node_type: node_type.get(),
                status: snap.2,
                saved_at: None, // stamped on write
            };
            // Skip the mount-time run: writing here would clobber a stored
            // draft just by opening /nodes/new with a template prefill.
            if prev.is_none() {
                return;
            }
            let v = autosave_v.get_untracked() + 1;
            autosave_v.set(v);
            // `try_` throughout: Cancel or a link within the debounce leaves
            // this timer running after the editor is gone.
            Timeout::new(AUTOSAVE_DEBOUNCE_MS, move || {
                if autosave_v.try_get_untracked() != Some(v) {
                    return;
                }
                // Untouched system body (scaffold OR the applied default
                // template) is not worth rescuing as a draft.
                let pristine_body = last_system_body
                    .try_get_untracked()
                    .flatten()
                    .unwrap_or_else(|| template_for_type(&d.node_type).to_string());
                let Some(Some(sub)) = create_subject.try_get_value() else {
                    return; // no user, no draft (never shared)
                };
                if d.is_meaningful(&pristine_body) {
                    crate::draft::write_draft(&sub, &d);
                    save_state.try_set(SaveState::Saved);
                } else {
                    // Reverted to pristine — leave nothing behind.
                    crate::draft::clear_draft(&sub);
                    save_state.try_set(SaveState::Idle);
                }
            })
            .forget();
        }
    });

    // Autosave executor (edit mode only). Skips when a save is already in
    // flight; the completion recheck below re-triggers if more edits arrived
    // during the round-trip.
    Effect::new(move |_| {
        if !autosave_now.get() {
            return;
        }
        autosave_now.set(false);
        let Some(id) = node else { return };
        if saving.get_untracked() || fetching.get_untracked() {
            return;
        }
        let snap = snapshot_now();
        if baseline.get_untracked().as_ref() == Some(&snap) {
            return;
        }
        saving.set(true);
        save_state.set(SaveState::Saving);
        // Until the server confirms, these edits live on the device too.
        keep_draft(&snap);
        own_fps.update_value(|v| v.push(fp(&snap)));
        let own = own_fps.get_value();
        let expected = base_version.get_untracked();
        let abort = web_sys::AbortController::new().ok();
        let signal = abort.as_ref().map(web_sys::AbortController::signal);
        let g = arm_watchdog(abort);
        wasm_bindgen_futures::spawn_local(async move {
            let result = put_unless_changed(id, &snap, expected, &own, signal.as_ref()).await;
            // The editor may be gone by now: every access below is `try_`.
            let Some(settled) = watch.try_update_value(|w| w.settle(g)) else {
                return;
            };
            let current = settled == Settled::Current;
            match result {
                // A late success while a newer save is in flight is left to
                // that save: its precondition fails, sees our own text and
                // re-sends on top (`put_unless_changed`).
                Ok(saved) if current || watch.try_with_value(|w| !w.in_flight()) == Some(true) => {
                    if snapshot_now() == snap {
                        drop_draft();
                    }
                    own_fps.set_value(vec![fp(&snap)]);
                    base_version.try_set(Some(saved.updated_at));
                    baseline.try_set(Some(snap));
                    saved_at.try_set(
                        Some(crate::components::format_helpers::local_time_now())
                            .filter(|t| !t.is_empty()),
                    );
                    save_state.try_set(SaveState::Saved);
                    // A late success also clears the give-up failure.
                    error_msg.try_set(None);
                    if current && nav_after_save.try_get_untracked() == Some(true) {
                        nav_after_save.try_set(false);
                        refresh.try_update(|n| *n += 1);
                        if let Some(nav) = nav_store.try_get_value() {
                            nav(&format!("/nodes/{}", saved.id), Default::default());
                        }
                        return;
                    }
                }
                Ok(_) => {}
                Err(e) if current => {
                    // Keep the edits (here and in the local draft) and don't
                    // loop on retries (an offline session would hammer the
                    // API). Retry, the next keystroke or coming back online
                    // starts a fresh attempt; the unmount flush is a last resort.
                    keep_draft(&snapshot_now());
                    save_state.try_set(SaveState::Failed);
                    let manual = nav_after_save.try_get_untracked() == Some(true);
                    nav_after_save.try_set(false);
                    match e {
                        SaveError::ChangedElsewhere(v) => {
                            conflict_version.try_set(Some(v));
                            error_msg.try_set(Some(CHANGED_ELSEWHERE_MSG.to_string()));
                        }
                        SaveError::Other(e) if manual => {
                            error_msg.try_set(Some(format!("{e}")));
                        }
                        SaveError::Other(_) => {}
                    }
                }
                // A failure of a save already given up on changes nothing.
                Err(_) => {}
            }
            if current {
                saving.try_set(false);
            }
            if watch.try_with_value(|w| !w.in_flight()) == Some(true)
                && save_state.try_get_untracked() != Some(SaveState::Failed)
                && baseline.try_get_untracked().flatten() != Some(snapshot_now())
            {
                autosave_now.try_set(true);
            }
        });
    });

    // Retry a failed save (the Retry button, or the browser coming back
    // online). After a "changed elsewhere" refusal only the button may
    // adopt the other version and overwrite it: that takes the user's say.
    let retry_save = move |explicit: bool| {
        if save_state.try_get_untracked() != Some(SaveState::Failed) {
            return;
        }
        if let Some(v) = conflict_version.try_get_untracked().flatten() {
            if !explicit {
                return;
            }
            base_version.try_set(Some(v));
            conflict_version.try_set(None);
        }
        error_msg.try_set(None);
        autosave_now.try_set(true);
    };

    // Warn before the tab closes/refreshes with unpersisted edits. Edit mode
    // only: create mode is already covered by the localStorage draft.
    if node.is_some() {
        let unload_handle =
            window_event_listener(ev::beforeunload, move |ev: web_sys::BeforeUnloadEvent| {
                let dirty = baseline
                    .get_untracked()
                    .is_some_and(|base| base != snapshot_now());
                if dirty || saving.get_untracked() {
                    // A reload (the update banner), the login redirect after
                    // a refused session refresh, or a closed tab must not
                    // drop the edits (#161).
                    keep_draft(&snapshot_now());
                    ev.prevent_default();
                    // Some browsers require a return value for the prompt.
                    ev.set_return_value("Unsaved changes");
                }
            });
        on_cleanup(move || unload_handle.remove());
        // Back online after a failed save: try once more by itself.
        let online_handle = window_event_listener(ev::online, move |_| retry_save(false));
        on_cleanup(move || online_handle.remove());
    }

    // Last-chance flush: navigating away (Escape, sidebar link, browser back)
    // unmounts the editor; persist any edits younger than the debounce window.
    // The values are read synchronously here, before the signals are disposed.
    on_cleanup(move || {
        let Some(id) = node else { return };
        let Some(base) = baseline.get_untracked() else {
            return;
        };
        let snap = snapshot_now();
        if snap == base {
            return;
        }
        // Read the draft slot and versions now: they go with the owner.
        let slot = draft_slot.try_get_value().flatten();
        let mut own = own_fps.try_get_value().unwrap_or_default();
        if let Some((sub, nid)) = &slot {
            crate::draft::write_edit_draft(sub, nid, &draft_of(&snap, own.clone()));
        }
        let expected = base_version.try_get_untracked().flatten();
        own.push(fp(&snap));
        wasm_bindgen_futures::spawn_local(async move {
            // A save still in flight may land first; the precondition then
            // sees our own text and re-sends this one on top.
            match put_unless_changed(id, &snap, expected, &own, None).await {
                Ok(_) => {
                    if let Some((sub, nid)) = &slot {
                        crate::draft::clear_edit_draft(sub, nid);
                    }
                    refresh.try_update(|n| *n += 1);
                    if let Some(ts) = toast_state {
                        ts.push(ToastLevel::Success, "Unsaved edits saved.");
                    }
                }
                Err(e) => {
                    let why = match e {
                        SaveError::ChangedElsewhere(_) => {
                            "the node was changed somewhere else".to_string()
                        }
                        SaveError::Other(e) => e.to_string(),
                    };
                    if let Some(ts) = toast_state {
                        ts.push(
                            ToastLevel::Error,
                            format!(
                                "Couldn't save your last edits: {why}. They're kept on this \
                                 device and offered back when you reopen the node."
                            ),
                        );
                    }
                }
            }
        });
    });

    // Image drag events on the textarea.
    let on_img_dragover = move |ev: web_sys::DragEvent| {
        ev.prevent_default();
        img_drag_over.set(true);
    };
    let on_img_dragleave = move |_: web_sys::DragEvent| {
        img_drag_over.set(false);
    };
    let on_img_drop = move |ev: web_sys::DragEvent| {
        ev.prevent_default();
        img_drag_over.set(false);
        let Some(dt) = ev.data_transfer() else { return };
        let Some(fl) = dt.files() else { return };
        for i in 0..fl.length() {
            let Some(file) = fl.get(i) else { continue };
            if !file.type_().starts_with("image/") {
                continue;
            }
            upload_image_file(file);
        }
    };
    // Paste from clipboard (e.g. screenshot paste via Ctrl+V).
    let on_img_paste = move |ev: web_sys::ClipboardEvent| {
        let Some(cd) = ev.clipboard_data() else {
            return;
        };
        let items = cd.items();
        let mut found_image = false;
        for i in 0..items.length() {
            let Some(item) = items.get(i) else { continue };
            if item.kind() != "file" || !item.type_().starts_with("image/") {
                continue;
            }
            let Ok(Some(file)) = item.get_as_file() else {
                continue;
            };
            if !found_image {
                // Only prevent default once we know we have an image.
                ev.prevent_default();
                found_image = true;
            }
            upload_image_file(file);
        }
    };

    // Edit mode autosaves, so leaving never discards (#197): Done keeps the
    // edits, Revert first puts back the node as it was opened. Either way the
    // last state goes through the save executor, then the editor leaves. The
    // text it replaces stays in History (each body save records a version).
    let on_save = move |revert: bool| {
        error_msg.set(None);
        if revert && let Some(o) = opened.get_untracked() {
            title.set(o.0);
            body.set(o.1);
            status.set(o.2);
        }
        let snap = snapshot_now();
        if let Some(id) = node {
            // Edit mode goes through the autosave executor, so a manual Save
            // gets the same precondition, watchdog and late-response rules.
            if baseline.get_untracked().as_ref() == Some(&snap) {
                nav_store.get_value()(&format!("/nodes/{id}"), Default::default());
                return;
            }
            nav_after_save.set(true);
            // Save is as explicit as Retry: after a "changed elsewhere"
            // refusal it saves over that change (kept in History).
            if let Some(v) = conflict_version.get_untracked() {
                base_version.set(Some(v));
                conflict_version.set(None);
            }
            autosave_now.set(true);
            return;
        }
        saving.set(true);
        save_state.set(SaveState::Saving);
        let nt_str = node_type.get_untracked();
        let nav = nav_store.get_value();
        let g = arm_watchdog(None);
        wasm_bindgen_futures::spawn_local(async move {
            let nt = match nt_str.as_str() {
                "project" => NodeType::Project,
                "area" => NodeType::Area,
                "resource" => NodeType::Resource,
                "reference" => NodeType::Reference,
                _ => NodeType::Article,
            };
            let req = CreateNodeRequest {
                title: snap.0.clone(),
                node_type: nt,
                body: Some(snap.1.clone()),
                metadata: serde_json::Value::Object(serde_json::Map::new()),
                status: Some(NodeStatus::parse(&snap.2).unwrap_or(NodeStatus::Draft)),
                template_id: template_id_for_create.get_untracked(),
                project_status: None,
            };
            let result = crate::api::create_node(&req).await;
            let current = watch.try_update_value(|w| w.settle(g)) == Some(Settled::Current);
            match result {
                // Even a late success: the node exists now, so follow it and
                // drop the local draft rather than invite a second create.
                Ok(saved_node) => {
                    // Invalidate any pending draft-write timeout before
                    // clearing, or it would resurrect the draft for a node
                    // that now exists.
                    autosave_v.try_update(|v| *v += 1);
                    if let Some(Some(sub)) = create_subject.try_get_value() {
                        crate::draft::clear_draft(&sub);
                    }
                    save_state.try_set(SaveState::Saved);
                    refresh.try_update(|n| *n += 1);
                    nav(&format!("/nodes/{}", saved_node.id), Default::default());
                }
                Err(e) if current => {
                    save_state.try_set(SaveState::Failed);
                    error_msg.try_set(Some(format!("{e}")));
                }
                // The watchdog already reported this one.
                Err(_) => {}
            }
            if current {
                saving.try_set(false);
            }
        });
    };

    // Detect [[query at cursor on every keystroke.
    let on_body_input = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        body.set(val.clone());

        let query = textarea_ref
            .get()
            .and_then(|el| el.selection_start().ok().flatten())
            .and_then(|cursor| wikilink_query_at(&val, cursor as usize));
        wikilink_query.set(query);
    };

    // Insert the selected title at the cursor, replacing the open [[query.
    let on_select_title = move |selected: String| {
        wikilink_query.set(None);
        let current = body.get_untracked();
        // `selection_start` is a UTF-16 code-unit offset — convert to a
        // UTF-8 byte offset before slicing (see `utf16_to_utf8_offset`).
        let cursor_u16 = textarea_ref
            .get()
            .and_then(|el| el.selection_start().ok().flatten())
            .unwrap_or(0) as usize;
        let cursor = utf16_to_utf8_offset(&current, cursor_u16);
        let before = &current[..cursor];
        if let Some(open_pos) = before.rfind("[[") {
            // open_pos comes from rfind → already a valid UTF-8 boundary.
            let new_val = format!("[[{}]]{}", selected, &current[cursor..],);
            let prefix = &current[..open_pos];
            let new_val = format!("{prefix}{new_val}");
            let new_cursor_bytes = open_pos + 2 + selected.len() + 2;
            // set_selection_start expects UTF-16; convert back.
            let new_cursor_u16 = utf8_to_utf16_offset(&new_val, new_cursor_bytes);
            body.set(new_val.clone());
            // Defer cursor placement until after Leptos re-renders the textarea.
            if let Some(el) = textarea_ref.get() {
                el.set_value(&new_val);
                let _ = el.set_selection_start(Some(new_cursor_u16 as u32));
                let _ = el.set_selection_end(Some(new_cursor_u16 as u32));
                let _ = el.focus();
            }
        }
    };

    let preview_html = move || {
        let title_map = wikilink_map.get();
        render_markdown(&body.get(), &title_map)
    };

    view! {
        <div class="flex flex-col h-full">
            // Header — stacks vertically below md: so the metadata selects +
            // Save/Cancel buttons remain reachable without horizontal scroll
            // on mobile.
            <div class="flex flex-col md:flex-row md:items-center md:justify-between
                        gap-2 px-4 md:px-6 py-3 md:py-4
                        border-b border-stone-200 dark:border-stone-800">
                <div class="flex items-center gap-3 flex-1 min-w-0">
                    <button
                        title="Back to nodes"
                        aria-label="Back to nodes"
                        class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-300 flex-shrink-0"
                        on:click=move |_| nav_back1("/nodes", Default::default())
                    >
                        <span aria-hidden="true" class="material-symbols-outlined">"arrow_back"</span>
                    </button>
                    <input
                        type="text"
                        node_ref=title_ref
                        class="flex-1 min-w-0 text-lg font-semibold bg-transparent
                               text-stone-900 dark:text-stone-100
                               focus:outline-none placeholder-stone-400"
                        placeholder="Node title\u{2026}"
                        prop:value=move || title.get()
                        on:input=move |ev| title.set(event_target_value(&ev))
                    />
                </div>
                <div class="flex items-center gap-2 flex-wrap md:flex-nowrap md:justify-end">
                    <select
                        class="text-sm bg-stone-100 dark:bg-stone-800 text-stone-700 dark:text-stone-300
                            rounded-lg px-2 py-1.5 focus:outline-none"
                        prop:value=move || node_type.get()
                        on:change=move |ev| apply_type_change(event_target_value(&ev))
                    >
                        <option value="article">"Article"</option>
                        <option value="project">"Project"</option>
                        <option value="area">"Area"</option>
                        <option value="resource">"Resource"</option>
                        <option value="reference">"Reference"</option>
                    </select>
                    // Template picker — only visible in create mode.
                    {move || node.is_none().then(|| view! {
                        <select
                            class="text-sm bg-stone-100 dark:bg-stone-800 text-stone-700 dark:text-stone-300
                                rounded-lg px-2 py-1.5 focus:outline-none max-w-[160px]"
                            title="Use a template"
                            prop:value=move || selected_template_value.get()
                            on:change=move |ev| {
                                let val = event_target_value(&ev);
                                selected_template_value.set(val.clone());
                                if val.is_empty() {
                                    template_id_for_create.set(None);
                                } else if let Ok(tid) = val.parse::<TemplateId>() {
                                    let templates = available_templates.get_untracked();
                                    if let Some(t) = templates.into_iter().find(|t| t.id == tid) {
                                        body.set(t.body.clone());
                                        last_system_body.set(Some(t.body.clone()));
                                        node_type.set(t.node_type.as_str().to_string());
                                        template_id_for_create.set(Some(tid));
                                    }
                                }
                            }
                        >
                            <option value="">"— Template —"</option>
                            // This type's templates first (default starred), then
                            // the rest (#111).
                            {move || {
                                let ts = available_templates.get();
                                let nt = NodeType::parse(&node_type.get()).unwrap_or(NodeType::Article);
                                let (own, other) = picker_order(&ts, &nt);
                                let opt = |t: &NodeTemplate| {
                                    let label = if t.is_default {
                                        format!("\u{2605} {}", t.name)
                                    } else {
                                        t.name.clone()
                                    };
                                    view! { <option value=t.id.to_string()>{label}</option> }
                                };
                                let own_label = format!("{} templates", node_type_label(&nt));
                                view! {
                                    {(!own.is_empty()).then(|| view! {
                                        <optgroup label=own_label>
                                            {own.iter().map(|t| opt(t)).collect_view()}
                                        </optgroup>
                                    })}
                                    {(!other.is_empty()).then(|| view! {
                                        <optgroup label="Other types">
                                            {other.iter().map(|t| opt(t)).collect_view()}
                                        </optgroup>
                                    })}
                                }
                            }}
                        </select>
                    })}
                    // Set the default at the moment of intent: a picked template
                    // that isn't its type's default yet (#111).
                    {move || {
                        if node.is_some() {
                            return None;
                        }
                        let id = template_id_for_create.get()?;
                        let t = available_templates.get().into_iter().find(|t| t.id == id)?;
                        (!t.is_default).then(|| {
                            let label = format!("Use as default for {}", node_type_label(&t.node_type));
                            view! {
                                <button
                                    type="button"
                                    class="text-xs px-2 py-1 rounded-lg text-stone-600 dark:text-stone-300
                                        bg-stone-100 dark:bg-stone-800 hover:text-amber-600
                                        dark:hover:text-amber-400 transition-colors whitespace-nowrap"
                                    data-testid="use-as-default"
                                    title="Apply this template automatically to new nodes of this type"
                                    on:click=move |_| {
                                        wasm_bindgen_futures::spawn_local(async move {
                                            match crate::api::set_template_default(id.0).await {
                                                Ok(updated) => {
                                                    // Mirror the server: one default per type.
                                                    available_templates.update(|ts| {
                                                        for t in ts.iter_mut() {
                                                            if t.node_type == updated.node_type {
                                                                t.is_default = t.id == updated.id && updated.is_default;
                                                            }
                                                        }
                                                    });
                                                    crate::components::toast::push_toast(
                                                        crate::components::toast::ToastLevel::Success,
                                                        format!("Default template for {} set", node_type_label(&updated.node_type)),
                                                    );
                                                }
                                                Err(e) => crate::components::toast::push_toast(
                                                    crate::components::toast::ToastLevel::Error,
                                                    format!("Couldn't set default: {e}"),
                                                ),
                                            }
                                        });
                                    }
                                >
                                    {format!("\u{2605} {label}")}
                                </button>
                            }
                        })
                    }}
                    <select
                        class="text-sm bg-stone-100 dark:bg-stone-800 text-stone-700 dark:text-stone-300
                            rounded-lg px-2 py-1.5 focus:outline-none"
                        prop:value=move || status.get()
                        on:change=move |ev| status.set(event_target_value(&ev))
                    >
                        <option value="draft">"Draft"</option>
                        <option value="published">"Published"</option>
                        <option value="archived">"Archived"</option>
                    </select>
                    // Save-state indicator (autosave / local draft).
                    <span
                        class=move || {
                            let base = "text-xs whitespace-nowrap";
                            if save_state.get() == SaveState::Failed {
                                format!("{base} text-red-600 dark:text-red-400")
                            } else if save_state.get() == SaveState::Stalled {
                                format!("{base} text-amber-600 dark:text-amber-400")
                            } else if save_state.get().is_pending() {
                                format!("{base} text-stone-500 dark:text-stone-400")
                            } else {
                                format!("{base} text-stone-400 dark:text-stone-500")
                            }
                        }
                        aria-live="polite"
                    >
                        {move || save_state.get().label(node.is_some(), saved_at.get().as_deref())}
                    </span>
                    // Retry after a failed save (#161). Edit mode only: create
                    // mode retries with its own Create button.
                    {move || (node.is_some() && save_state.get() == SaveState::Failed).then(|| view! {
                        <button
                            type="button"
                            data-testid="save-retry"
                            class="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-xs font-medium
                                text-red-700 dark:text-red-300 bg-red-50 dark:bg-red-900/20
                                hover:bg-red-100 dark:hover:bg-red-900/40"
                            on:click=move |_| retry_save(true)
                        >
                            <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"refresh"</span>
                            "Retry"
                        </button>
                    })}
                    // Preview toggle button
                    <button
                        class=move || {
                            let base = "p-1.5 rounded-lg transition-colors";
                            if show_preview.get() {
                                format!("{base} text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/20 hover:bg-amber-100 dark:hover:bg-amber-900/30")
                            } else {
                                format!("{base} text-stone-400 hover:text-stone-600 dark:hover:text-stone-300 hover:bg-stone-100 dark:hover:bg-stone-800")
                            }
                        }
                        title=move || if show_preview.get() { "Hide preview" } else { "Show preview" }
                        on:click=move |_| show_preview.update(|v| *v = !*v)
                    >
                        <span aria-hidden="true" class="material-symbols-outlined">
                            {move || if show_preview.get() { "visibility" } else { "visibility_off" }}
                        </span>
                    </button>
                    // Edit mode: no Cancel, because autosave has already kept
                    // the edits (#197). Revert shows once something changed.
                    {move || {
                        let changed = node.is_some()
                            && opened.get().is_some_and(|o| o != (title.get(), body.get(), status.get()));
                        changed.then(|| view! {
                            <button
                                type="button"
                                class="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-xs font-medium
                                    text-stone-600 dark:text-stone-300 bg-stone-100 dark:bg-stone-800
                                    hover:text-stone-900 dark:hover:text-stone-100 transition-colors"
                                title="Back to the node as you opened it. The edited text stays in History."
                                on:click=move |_| on_save(true)
                                disabled=move || saving.get() || fetching.get()
                            >
                                <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"history"</span>
                                "Revert"
                            </button>
                        })
                    }}
                    // The commit button names its action (leptos.md: icon + word).
                    <button
                        type="button"
                        class="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium
                            bg-ember hover:bg-ember-strong text-white transition-colors
                            disabled:opacity-60"
                        on:click=move |_| on_save(false)
                        disabled=move || saving.get() || fetching.get()
                        title=move || {
                            if saving.get() {
                                "Saving\u{2026}"
                            } else if fetching.get() {
                                "Loading\u{2026}"
                            } else if node.is_some() {
                                "Changes save automatically. Done returns to the node."
                            } else {
                                "Create the node"
                            }
                        }
                    >
                        <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">
                            {move || if saving.get() { "hourglass_empty" } else if fetching.get() { "hourglass_top" } else if node.is_some() { "check" } else { "add" }}
                        </span>
                        {if node.is_some() { "Done" } else { "Create" }}
                    </button>
                    // Create mode: nothing is on the server until Create, so
                    // Cancel really leaves it out (the local draft is offered
                    // back on the next /nodes/new).
                    {node.is_none().then(|| view! {
                        <button
                            type="button"
                            class="px-2 py-1 rounded-lg text-xs font-medium text-stone-500 dark:text-stone-400
                                hover:text-stone-700 dark:hover:text-stone-200 transition-colors"
                            on:click=move |_| navigate("/nodes", Default::default())
                        >
                            "Cancel"
                        </button>
                    })}
                </div>
            </div>
            // Error banner
            {move || error_msg.get().map(|msg| view! {
                <div class="px-6 py-2 bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-sm">
                    {msg}
                </div>
            })}
            // Unsaved edits this device kept from an earlier visit (#161).
            {move || recovered.get().map(|(d, base)| {
                let d = StoredValue::new(d);
                view! {
                    <div
                        data-testid="draft-recovery"
                        role="status"
                        class="flex flex-wrap items-center gap-3 px-6 py-2 text-sm
                            bg-amber-50 dark:bg-amber-900/20 text-amber-900 dark:text-amber-200"
                    >
                        <span>{recovery_message(base)}</span>
                        <button
                            type="button"
                            class="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-xs font-medium
                                bg-ember hover:bg-ember-strong text-white"
                            on:click=move |_| {
                                let d = d.get_value();
                                title.set(d.title);
                                body.set(d.body);
                                status.set(d.status);
                                recovered.set(None);
                            }
                        >
                            <span class="material-symbols-outlined" style="font-size: 14px;" aria-hidden="true">"refresh"</span>
                            "Restore"
                        </button>
                        <button
                            type="button"
                            class="text-xs underline"
                            on:click=move |_| {
                                drop_draft();
                                recovered.set(None);
                            }
                        >
                            "Discard"
                        </button>
                    </div>
                }
            })}
            // Split editor + preview
            <div class="flex flex-1 divide-x divide-stone-200 dark:divide-stone-700 min-h-0">
                // Editor pane (relative so the autocomplete dropdown can be positioned)
                <div class="flex-1 flex flex-col relative">
                    <textarea
                        node_ref=textarea_ref
                        class=move || {
                            let base = "flex-1 p-4 font-mono text-sm resize-none bg-transparent \
                                text-stone-900 dark:text-stone-100 focus:outline-none \
                                transition-[box-shadow] duration-150";
                            if img_drag_over.get() {
                                format!("{base} ring-2 ring-amber-400 ring-inset")
                            } else {
                                base.to_string()
                            }
                        }
                        placeholder="Write in Markdown… use [[Node Title]] to link nodes, or drag & drop images"
                        prop:value=move || body.get()
                        on:input=on_body_input
                        spellcheck="true"
                        on:dragover=on_img_dragover
                        on:dragleave=on_img_dragleave
                        on:drop=on_img_drop
                        on:paste=on_img_paste
                        // Close dropdown on Escape
                        on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                            if ev.key() == "Escape" {
                                wikilink_query.set(None);
                            }
                        }
                    />
                    // Image uploading indicator
                    {move || img_uploading.get().then(|| view! {
                        <div class="absolute top-2 right-2 z-40 flex items-center gap-1.5
                            bg-stone-800/80 text-stone-100 text-xs rounded-lg px-2.5 py-1
                            backdrop-blur-sm pointer-events-none">
                            <span class="material-symbols-outlined text-sm animate-spin">"progress_activity"</span>
                            "Uploading image\u{2026}"
                        </div>
                    })}
                    // Wiki-link autocomplete dropdown
                    {move || {
                        let query = wikilink_query.get()?;
                        let matches: Vec<String> = suggestions.with(|hits| {
                            hits.iter().take(8).map(|h| h.title.clone()).collect()
                        });
                        if matches.is_empty() {
                            return None;
                        }
                        Some(view! {
                            <div class="absolute bottom-4 left-4 z-50 w-72
                                bg-white dark:bg-stone-900
                                border border-stone-200 dark:border-stone-700
                                rounded-lg shadow-xl overflow-hidden">
                                <div class="px-3 py-1.5 text-xs text-stone-400 border-b border-stone-100 dark:border-stone-800">
                                    "Link to node — " {query.clone()}
                                </div>
                                {matches.into_iter().map(|t| {
                                    let t_clone = t.clone();
                                    let select = on_select_title;
                                    view! {
                                        <button
                                            type="button"
                                            class="w-full text-left px-3 py-2 text-sm
                                                text-stone-800 dark:text-stone-200
                                                hover:bg-amber-50 dark:hover:bg-amber-900/30
                                                hover:text-amber-700 dark:hover:text-amber-300
                                                transition-colors"
                                            on:click=move |ev| {
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                                select(t_clone.clone());
                                            }
                                        >
                                            <span class="material-symbols-outlined text-xs mr-1 align-middle" aria-hidden="true">"link"</span>
                                            {t.clone()}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        })
                    }}
                </div>
                // Preview pane — conditionally rendered based on show_preview signal.
                {move || show_preview.get().then(|| view! {
                    <div class="flex-1 overflow-auto p-6">
                        <div class="prose max-w-none dark:prose-invert" inner_html=preview_html />
                    </div>
                })}
            </div>
        </div>
    }
}
