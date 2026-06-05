/// Attachment panel — list, upload (bulk), and inline-preview files attached to a node.
///
/// Upload UX:
///   - Click the drop zone (or the folder icon) to open a multi-file picker.
///   - Drag files onto the drop zone to queue them.
///   - Files are uploaded sequentially; a progress counter shows (n / total).
use common::id::{AttachmentId, NodeId};
use leptos::{html::Input, prelude::*};
use wasm_bindgen::JsCast;

use crate::api;

/// Copy `text` to the clipboard via the JS Clipboard API.
fn copy_to_clipboard(text: &str) {
    let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
    let _ = js_sys::eval(&format!("navigator.clipboard.writeText('{escaped}')"));
}

/// Returns true for MIME types the browser can display inline.
fn is_previewable(content_type: &str) -> bool {
    content_type.starts_with("image/") || content_type == "application/pdf"
}

#[component]
pub fn AttachmentPanel(node_id: NodeId) -> impl IntoView {
    let refresh = RwSignal::new(0u32);
    let error_msg = RwSignal::new(Option::<String>::None);
    let drag_over = RwSignal::new(false);
    let file_input_ref = NodeRef::<Input>::new();

    // Pending files selected by picker or drop.  web_sys::File is Send on wasm32.
    let pending_files: RwSignal<Vec<web_sys::File>> = RwSignal::new(Vec::new());

    // Upload progress: Some((completed, total)) while a batch is running.
    let upload_progress: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);

    // open must be declared before attachments so the resource closure can capture it.
    let open = RwSignal::new(false);

    // Track which attachment's URL was most recently copied (for brief visual feedback).
    let copied_att: RwSignal<Option<AttachmentId>> = RwSignal::new(None);

    let attachments = LocalResource::new(move || {
        let _ = refresh.get();
        let is_open = open.get();
        async move {
            if !is_open {
                return Ok(vec![]);
            }
            api::fetch_attachments(node_id).await
        }
    });

    // ── Helpers ───────────────────────────────────────────────────────────────

    // Collect files from a FileList into the pending queue.
    let queue_file_list = move |fl: web_sys::FileList| {
        let files: Vec<web_sys::File> = (0..fl.length()).filter_map(|i| fl.get(i)).collect();
        if !files.is_empty() {
            pending_files.set(files);
            error_msg.set(None);
        }
    };

    // ── Event handlers ────────────────────────────────────────────────────────

    let on_pick = move |_| {
        if let Some(el) = file_input_ref.get_untracked() {
            el.click();
        }
    };

    let on_file_change = move |_| {
        if let Some(fl) = file_input_ref.get_untracked().and_then(|el| el.files()) {
            queue_file_list(fl);
        }
    };

    let on_drag_over = move |ev: web_sys::DragEvent| {
        ev.prevent_default();
        drag_over.set(true);
    };

    let on_drag_leave = move |_: web_sys::DragEvent| {
        drag_over.set(false);
    };

    let on_drop = move |ev: web_sys::DragEvent| {
        ev.prevent_default();
        drag_over.set(false);
        if let Some(fl) = ev.data_transfer().and_then(|dt| dt.files()) {
            queue_file_list(fl);
        }
    };

    let on_clear = move |_| {
        pending_files.set(vec![]);
        if let Some(el) = file_input_ref.get_untracked() {
            el.set_value("");
        }
    };

    let on_upload = move |_| {
        let files = pending_files.get_untracked();
        if files.is_empty() {
            return;
        }
        let total = files.len();
        upload_progress.set(Some((0, total)));
        error_msg.set(None);

        wasm_bindgen_futures::spawn_local(async move {
            let mut failed = 0usize;
            for (idx, file) in files.into_iter().enumerate() {
                let form_data = match web_sys::FormData::new() {
                    Ok(fd) => fd,
                    Err(_) => {
                        failed += 1;
                        upload_progress.set(Some((idx + 1, total)));
                        continue;
                    }
                };
                let blob: &web_sys::Blob = file.unchecked_ref();
                if form_data
                    .append_with_blob_and_filename("file", blob, &file.name())
                    .is_err()
                {
                    failed += 1;
                    upload_progress.set(Some((idx + 1, total)));
                    continue;
                }
                if api::upload_attachment(node_id, form_data).await.is_err() {
                    failed += 1;
                }
                upload_progress.set(Some((idx + 1, total)));
            }
            upload_progress.set(None);
            pending_files.set(vec![]);
            if let Some(el) = file_input_ref.get_untracked() {
                el.set_value("");
            }
            if failed > 0 {
                error_msg.set(Some(format!("{failed} file(s) failed to upload.")));
            }
            refresh.update(|n| *n += 1);
        });
    };

    view! {
        <div class="mt-8 border-t border-stone-200 dark:border-stone-700 pt-6">
            <div class="flex items-center justify-between">
                <button
                    class="flex items-center gap-1 text-left cursor-pointer"
                    on:click=move |_| open.update(|v| *v = !*v)
                >
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 16px;"
                    >
                        {move || if open.get() { "expand_more" } else { "chevron_right" }}
                    </span>
                    <span
                        class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                        style="font-size: 15px;"
                    >
                        "attach_file"
                    </span>
                    <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">
                        "Attachments"
                    </h2>
                    <Suspense fallback=|| ()>
                    {move || {
                        attachments.with(|r| r.as_ref().and_then(|res| match res {
                            Ok(v) if !v.is_empty() => Some(view! {
                                <span class="ml-1 text-xs bg-stone-200 dark:bg-stone-700
                                            text-stone-600 dark:text-stone-300
                                            rounded-full px-1.5 py-0.5">
                                    {v.len()}
                                </span>
                            }),
                            _ => None,
                        }))
                    }}
                    </Suspense>
                </button>
            </div>

            {move || open.get().then(|| view! {
                <div class="mt-4">

                // ── Hidden multi-file input ───────────────────────────────────
                <input
                    type="file"
                    multiple=true
                    node_ref=file_input_ref
                    on:change=on_file_change
                    class="hidden"
                />

                // ── Drop zone ─────────────────────────────────────────────────
                <div
                    class=move || {
                        let base = "relative mb-3 rounded-xl border-2 border-dashed px-4 py-5 \
                                    text-center cursor-pointer transition-colors select-none";
                        if drag_over.get() {
                            format!("{base} border-amber-400 bg-amber-50 dark:bg-amber-900/10")
                        } else if !pending_files.get().is_empty() {
                            format!("{base} border-amber-300 dark:border-amber-700 \
                                     bg-amber-50/50 dark:bg-amber-900/5")
                        } else {
                            format!("{base} border-stone-300 dark:border-stone-600 \
                                     hover:border-amber-400 dark:hover:border-amber-600 \
                                     bg-stone-50 dark:bg-stone-800/30")
                        }
                    }
                    on:click=on_pick
                    on:dragover=on_drag_over
                    on:dragleave=on_drag_leave
                    on:drop=on_drop
                >
                    {move || {
                        let files = pending_files.get();
                        if files.is_empty() {
                            view! {
                                <div class="flex flex-col items-center gap-1.5 pointer-events-none">
                                    <span class="material-symbols-outlined text-stone-400 dark:text-stone-500"
                                        style="font-size: 28px;">"cloud_upload"</span>
                                    <p class="text-xs font-medium text-stone-600 dark:text-stone-400">
                                        "Drop files here or click to browse"
                                    </p>
                                    <p class="text-xs text-stone-400 dark:text-stone-500">
                                        "Multiple files supported · 50 MB per file"
                                    </p>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="pointer-events-none">
                                    <p class="text-xs font-semibold text-amber-700 dark:text-amber-400 mb-1.5">
                                        {format!("{} file{} selected", files.len(),
                                            if files.len() == 1 { "" } else { "s" })}
                                    </p>
                                    <ul class="space-y-0.5 text-left max-h-28 overflow-y-auto">
                                        {files.iter().map(|f| {
                                            let name = f.name();
                                            let kb   = f.size() as u64 / 1024;
                                            view! {
                                                <li class="flex items-center gap-2 text-xs
                                                    text-stone-600 dark:text-stone-400 truncate">
                                                    <span class="material-symbols-outlined text-stone-400
                                                        dark:text-stone-500 shrink-0"
                                                        style="font-size: 13px;">"attach_file"</span>
                                                    <span class="truncate">{name}</span>
                                                    <span class="shrink-0 text-stone-400 dark:text-stone-500">
                                                        {format!("{kb} KB")}
                                                    </span>
                                                </li>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </ul>
                                </div>
                            }.into_any()
                        }
                    }}
                </div>

                // ── Action row ────────────────────────────────────────────────
                {move || {
                    let files = pending_files.get();
                    let progress = upload_progress.get();

                    if files.is_empty() && progress.is_none() {
                        return None;
                    }

                    Some(view! {
                        <div class="flex items-center gap-2 mb-3">
                            // Progress label or file count
                            <span class="flex-1 text-xs text-stone-500 dark:text-stone-400">
                                {match progress {
                                    Some((done, total)) =>
                                        format!("Uploading… {done} / {total}"),
                                    None =>
                                        format!("{} file{} ready",
                                            files.len(),
                                            if files.len() == 1 { "" } else { "s" }),
                                }}
                            </span>

                            // Clear button
                            {progress.is_none().then(|| view! {
                                <button
                                    class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600
                                        dark:hover:text-stone-300 hover:bg-stone-100
                                        dark:hover:bg-stone-800 transition-colors"
                                    on:click=on_clear
                                    title="Clear selection"
                                >
                                    <span class="material-symbols-outlined" style="font-size: 15px;">
                                        "close"
                                    </span>
                                </button>
                            })}

                            // Upload button
                            <button
                                class="flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs
                                    font-medium bg-amber-500 hover:bg-amber-600 text-white
                                    transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                                disabled=move || upload_progress.get().is_some()
                                on:click=on_upload
                            >
                                <span class="material-symbols-outlined" style="font-size: 14px;">
                                    {move || if upload_progress.get().is_some() {
                                        "hourglass_empty"
                                    } else {
                                        "upload"
                                    }}
                                </span>
                                {move || match upload_progress.get() {
                                    Some((done, total)) => format!("{done}/{total}"),
                                    None => "Upload".to_string(),
                                }}
                            </button>
                        </div>
                    })
                }}

                // ── Error message ─────────────────────────────────────────────
                {move || error_msg.get().map(|msg| view! {
                    <div class="mb-2 text-xs text-red-500">{msg}</div>
                })}

                // ── Attachment list ───────────────────────────────────────────
                <Suspense fallback=|| view! {
                    <div class="text-xs text-stone-400">"Loading attachments..."</div>
                }>
                    {move || {
                        attachments.get().map(|result| {
                            match result {
                                Ok(list) if list.is_empty() => view! {
                                    <div class="flex flex-col items-center gap-2 py-6">
                                        <span
                                            class="material-symbols-outlined text-stone-300 dark:text-stone-700"
                                            style="font-size: 32px;"
                                        >
                                            "attach_file"
                                        </span>
                                        <p class="text-xs text-stone-400 dark:text-stone-600">
                                            "No attachments yet."
                                        </p>
                                    </div>
                                }.into_any(),
                                Ok(list) => view! {
                                    <div class="space-y-2">
                                        {list.into_iter().map(|att| {
                                            let att_id = att.id;
                                            let filename = att.filename.clone();
                                            let content_type = att.content_type.clone();
                                            let size_kb = att.size_bytes / 1024;
                                            let download_url = api::attachment_download_url(att_id);
                                            let previewable = is_previewable(&content_type);
                                            let is_image = content_type.starts_with("image/");
                                            let preview_open = RwSignal::new(false);
                                            let url_for_preview = download_url.clone();
                                            let url_for_copy = download_url.clone();
                                            view! {
                                                <div class="rounded-lg border border-stone-200 dark:border-stone-700
                                                            bg-stone-50 dark:bg-stone-800/30">
                                                    // Header row: icon + name + meta + actions
                                                    <div class="flex items-center gap-2 px-3 py-2 group">
                                                        <span class="material-symbols-outlined text-stone-400
                                                                     dark:text-stone-500 shrink-0"
                                                              style="font-size: 16px;">
                                                            {if is_image { "image" }
                                                             else if content_type == "application/pdf" { "picture_as_pdf" }
                                                             else { "attach_file" }}
                                                        </span>
                                                        <span class="flex-1 text-xs text-stone-700 dark:text-stone-300
                                                                     truncate min-w-0">
                                                            {filename.clone()}
                                                        </span>
                                                        <span class="text-xs text-stone-400 dark:text-stone-500 shrink-0">
                                                            {format!("{size_kb} KB")}
                                                        </span>
                                                        // Preview toggle
                                                        {previewable.then(|| view! {
                                                            <button
                                                                class="p-0.5 text-stone-400 hover:text-amber-500
                                                                       dark:hover:text-amber-400 cursor-pointer
                                                                       transition-colors"
                                                                title=move || if preview_open.get() { "Hide preview" } else { "Show preview" }
                                                                on:click=move |_| preview_open.update(|v| *v = !*v)
                                                            >
                                                                <span class="material-symbols-outlined"
                                                                      style="font-size: 16px;">
                                                                    {move || if preview_open.get() { "visibility_off" } else { "visibility" }}
                                                                </span>
                                                            </button>
                                                        })}
                                                        // Copy URL button
                                                        <button
                                                            class="p-0.5 text-stone-400 hover:text-amber-500
                                                                   dark:hover:text-amber-400 cursor-pointer
                                                                   transition-colors"
                                                            title=move || if copied_att.get() == Some(att_id) {
                                                                "URL copied!"
                                                            } else {
                                                                "Copy attachment URL"
                                                            }
                                                            on:click=move |_| {
                                                                copy_to_clipboard(&url_for_copy);
                                                                copied_att.set(Some(att_id));
                                                                wasm_bindgen_futures::spawn_local(async move {
                                                                    gloo_timers::future::TimeoutFuture::new(2000).await;
                                                                    if copied_att.get_untracked() == Some(att_id) {
                                                                        copied_att.set(None);
                                                                    }
                                                                });
                                                            }
                                                        >
                                                            <span class="material-symbols-outlined"
                                                                  style="font-size: 16px;">
                                                                {move || if copied_att.get() == Some(att_id) {
                                                                    "check"
                                                                } else {
                                                                    "link"
                                                                }}
                                                            </span>
                                                        </button>
                                                        // Download link
                                                        <a
                                                            href=download_url
                                                            target="_blank"
                                                            class="p-0.5 text-stone-400 hover:text-amber-500
                                                                   dark:hover:text-amber-400 transition-colors"
                                                            title="Download"
                                                        >
                                                            <span class="material-symbols-outlined"
                                                                  style="font-size: 16px;">
                                                                "download"
                                                            </span>
                                                        </a>
                                                        // Delete button
                                                        <button
                                                            class="p-0.5 text-stone-400 hover:text-red-500
                                                                   cursor-pointer transition-colors"
                                                            title="Delete attachment"
                                                            on:click=move |_| {
                                                                wasm_bindgen_futures::spawn_local(async move {
                                                                    let _ = api::delete_attachment(att_id).await;
                                                                    refresh.update(|n| *n += 1);
                                                                });
                                                            }
                                                        >
                                                            <span class="material-symbols-outlined"
                                                                  style="font-size: 16px;">
                                                                "delete"
                                                            </span>
                                                        </button>
                                                    </div>

                                                    // Inline preview (toggled)
                                                    {move || {
                                                        if !preview_open.get() { return None; }
                                                        Some(if is_image {
                                                            view! {
                                                                <div class="px-3 pb-3">
                                                                    <img
                                                                        src=url_for_preview.clone()
                                                                        alt=filename.clone()
                                                                        class="max-w-full max-h-96 rounded-lg
                                                                               object-contain border border-stone-200
                                                                               dark:border-stone-700"
                                                                    />
                                                                </div>
                                                            }.into_any()
                                                        } else {
                                                            view! {
                                                                <div class="px-3 pb-3">
                                                                    <iframe
                                                                        src=url_for_preview.clone()
                                                                        class="w-full rounded-lg border border-stone-200
                                                                               dark:border-stone-700"
                                                                        style="height: 500px;"
                                                                        title=filename.clone()
                                                                    />
                                                                </div>
                                                            }.into_any()
                                                        })
                                                    }}
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any(),
                                Err(e) => view! {
                                    <div class="text-xs text-red-500">{format!("Error: {e}")}</div>
                                }.into_any(),
                            }
                        })
                    }}
                </Suspense>
                </div>  // close mt-4
            })}         // close open.then
        </div>
    }
}
