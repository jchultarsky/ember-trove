use std::collections::HashMap;

use common::{
    id::{NodeId, TemplateId},
    node::{CreateNodeRequest, NodeStatus, NodeTitleEntry, NodeType, UpdateNodeRequest},
    template::NodeTemplate,
};
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;

use crate::app::TemplatePrefill;
use crate::components::toast::{ToastLevel, push_toast};
use crate::markdown::render_markdown;
use crate::templates::template_for_type;
use leptos_router::hooks::use_navigate;

fn build_title_map(entries: &[NodeTitleEntry]) -> HashMap<String, NodeId> {
    entries.iter().map(|e| (e.title.clone(), e.id)).collect()
}

fn parse_status(s: &str) -> NodeStatus {
    match s {
        "published" => NodeStatus::Published,
        "archived" => NodeStatus::Archived,
        _ => NodeStatus::Draft,
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
    let nav_save = navigate.clone(); // clone for on_save spawn_local
    let nav_back1 = navigate.clone(); // clone for back button in header
    let refresh = expect_context::<RwSignal<u32>>();

    let title = RwSignal::new(String::new());
    // In create mode, pre-select the type from the active node_type_filter so
    // that opening the editor from e.g. the Projects list defaults to Project.
    // In edit mode the spawn_local block below will override this immediately.
    let node_type_filter = use_context::<RwSignal<Option<String>>>();
    let prefill_signal = use_context::<RwSignal<Option<TemplatePrefill>>>();

    // If a TemplatePrefill context is set, consume it (clear immediately) and
    // use its values as the create-mode defaults instead of the static templates.
    let (default_type, initial_body, initial_template_id) = if node.is_none() {
        if let Some(sig) = prefill_signal
            && let Some(p) = sig.get_untracked()
        {
            sig.set(None);
            (p.node_type, p.body, Some(p.template_id))
        } else {
            let nt = node_type_filter
                .and_then(|f| f.get_untracked())
                .unwrap_or_else(|| "article".to_string());
            let body = template_for_type(&nt).to_string();
            (nt, body, None)
        }
    } else {
        ("article".to_string(), String::new(), None)
    };

    let node_type = RwSignal::new(default_type.clone());
    // In create mode, pre-populate the body from template or static scaffold.
    // In edit mode spawn_local below will overwrite this with the real body.
    let body = RwSignal::new(initial_body);
    // Template ID used when creating a node from a template (for activity log).
    let template_id_for_create = RwSignal::new(initial_template_id);
    // Selected template value string (drives the <select> prop:value binding).
    let selected_template_value = RwSignal::new(String::new());

    // Fetch templates for the create-mode picker (no-op overhead in edit mode
    // since the picker is hidden; the resource is lazily evaluated).
    let templates_resource = LocalResource::new(crate::api::list_templates);
    let available_templates: RwSignal<Vec<NodeTemplate>> = RwSignal::new(vec![]);
    Effect::new(move |_| {
        if let Some(Ok(ts)) = templates_resource.get() {
            available_templates.set(ts);
        }
    });
    let status = RwSignal::new("draft".to_string());
    let saving = RwSignal::new(false);
    // True while the initial node fetch is in-flight (edit mode only).
    // The Save button is disabled until this clears to prevent saving stale
    // signal values if the user clicks Save before fetch_node completes.
    let fetching = RwSignal::new(false);
    let error_msg = RwSignal::new(Option::<String>::None);

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

    // Fetch all node titles for wiki-link autocomplete and preview.
    let titles_resource =
        LocalResource::new(|| async move { crate::api::fetch_node_titles().await });

    // If editing, fetch existing node data.
    if let Some(id) = node {
        fetching.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(n) = crate::api::fetch_node(id).await {
                title.set(n.title);
                body.set(n.body.unwrap_or_default());
                node_type.set(format!("{:?}", n.node_type).to_lowercase());
                status.set(format!("{:?}", n.status).to_lowercase());
            }
            fetching.set(false);
        });
    }

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

    let on_save = move |_: web_sys::MouseEvent| {
        saving.set(true);
        error_msg.set(None);
        let t = title.get_untracked();
        let b = body.get_untracked();
        let nt_str = node_type.get_untracked();
        let st_str = status.get_untracked();

        let nav = nav_save.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = if let Some(id) = node {
                let req = UpdateNodeRequest {
                    title: Some(t),
                    body: Some(b),
                    metadata: None,
                    status: Some(parse_status(&st_str)),
                };
                crate::api::update_node(id, &req).await
            } else {
                let nt = match nt_str.as_str() {
                    "project" => NodeType::Project,
                    "area" => NodeType::Area,
                    "resource" => NodeType::Resource,
                    "reference" => NodeType::Reference,
                    _ => NodeType::Article,
                };
                let req = CreateNodeRequest {
                    title: t,
                    node_type: nt,
                    body: Some(b),
                    metadata: serde_json::Value::Object(serde_json::Map::new()),
                    status: Some(parse_status(&st_str)),
                    template_id: template_id_for_create.get_untracked(),
                };
                crate::api::create_node(&req).await
            };

            match result {
                Ok(saved_node) => {
                    refresh.update(|n| *n += 1);
                    nav(&format!("/nodes/{}", saved_node.id), Default::default());
                }
                Err(e) => {
                    error_msg.set(Some(format!("{e}")));
                }
            }
            saving.set(false);
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
        let title_map = titles_resource
            .get()
            .and_then(|r| r.ok())
            .map(|entries| build_title_map(&entries))
            .unwrap_or_default();
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
                        class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-300 flex-shrink-0"
                        on:click=move |_| nav_back1("/nodes", Default::default())
                    >
                        <span class="material-symbols-outlined">"arrow_back"</span>
                    </button>
                    <input
                        type="text"
                        class="flex-1 min-w-0 text-lg font-semibold bg-transparent
                               text-stone-900 dark:text-stone-100
                               focus:outline-none placeholder-stone-400"
                        placeholder="Node title..."
                        prop:value=move || title.get()
                        on:input=move |ev| title.set(event_target_value(&ev))
                    />
                </div>
                <div class="flex items-center gap-2 flex-wrap md:flex-nowrap md:justify-end">
                    <select
                        class="text-sm bg-stone-100 dark:bg-stone-800 text-stone-700 dark:text-stone-300
                            rounded-lg px-2 py-1.5 focus:outline-none"
                        prop:value=move || node_type.get()
                        on:change=move |ev| {
                            let new_type = event_target_value(&ev);
                            // In create mode, swap the body template when the type
                            // changes — but only if the user hasn't modified it yet
                            // (body still equals the previous type's template).
                            if node.is_none() {
                                let current_body = body.get_untracked();
                                let old_tmpl = template_for_type(&node_type.get_untracked());
                                if current_body == old_tmpl {
                                    body.set(template_for_type(&new_type).to_string());
                                }
                            }
                            node_type.set(new_type);
                        }
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
                                        let type_str = match t.node_type {
                                            NodeType::Project   => "project",
                                            NodeType::Area      => "area",
                                            NodeType::Resource  => "resource",
                                            NodeType::Reference => "reference",
                                            NodeType::Article   => "article",
                                        };
                                        body.set(t.body.clone());
                                        node_type.set(type_str.to_string());
                                        template_id_for_create.set(Some(tid));
                                    }
                                }
                            }
                        >
                            <option value="">"— Template —"</option>
                            {move || available_templates.get().into_iter().map(|t| {
                                let name = t.name.clone();
                                let id = t.id.to_string();
                                view! { <option value=id>{name}</option> }
                            }).collect_view()}
                        </select>
                    })}
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
                        <span class="material-symbols-outlined">
                            {move || if show_preview.get() { "visibility" } else { "visibility_off" }}
                        </span>
                    </button>
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-green-600 dark:hover:text-green-400
                            hover:bg-green-50 dark:hover:bg-green-900/30 transition-colors"
                        on:click=on_save
                        disabled=move || saving.get() || fetching.get()
                        title=move || if saving.get() { "Saving\u{2026}" } else if fetching.get() { "Loading\u{2026}" } else { "Save" }
                    >
                        <span class="material-symbols-outlined">
                            {move || if saving.get() { "hourglass_empty" } else if fetching.get() { "hourglass_top" } else { "check" }}
                        </span>
                    </button>
                    <button
                        class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                            hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors"
                        on:click=move |_| navigate("/nodes", Default::default())
                        title="Cancel"
                    >
                        <span class="material-symbols-outlined">"close"</span>
                    </button>
                </div>
            </div>
            // Error banner
            {move || error_msg.get().map(|msg| view! {
                <div class="px-6 py-2 bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-sm">
                    {msg}
                </div>
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
                        let entries = titles_resource.get().and_then(|r| r.ok()).unwrap_or_default();
                        let q_lower = query.to_lowercase();
                        let matches: Vec<String> = entries
                            .iter()
                            .filter(|e| e.title.to_lowercase().contains(&q_lower))
                            .take(8)
                            .map(|e| e.title.clone())
                            .collect();
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
                                            <span class="material-symbols-outlined text-xs mr-1 align-middle">"link"</span>
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
