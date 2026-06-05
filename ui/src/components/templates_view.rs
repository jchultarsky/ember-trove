use common::{
    node::NodeType,
    template::{CreateTemplateRequest, NodeTemplate, UpdateTemplateRequest},
};
use leptos::prelude::*;

use crate::{
    app::TemplatePrefill,
    components::icon_button::{IconButton, IconButtonVariant},
    components::toast::{ToastLevel, push_toast},
    markdown::render_markdown_plain,
};
use leptos_router::hooks::use_navigate;

fn node_type_label(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Article => "Article",
        NodeType::Project => "Project",
        NodeType::Area => "Area",
        NodeType::Resource => "Resource",
        NodeType::Reference => "Reference",
    }
}

fn node_type_str(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Article => "article",
        NodeType::Project => "project",
        NodeType::Area => "area",
        NodeType::Resource => "resource",
        NodeType::Reference => "reference",
    }
}

fn node_type_icon(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Article => "article",
        NodeType::Project => "rocket_launch",
        NodeType::Area => "category",
        NodeType::Resource => "inventory_2",
        NodeType::Reference => "link",
    }
}

fn node_type_color(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Article => "text-amber-600 dark:text-amber-400",
        NodeType::Project => "text-blue-600 dark:text-blue-400",
        NodeType::Area => "text-green-600 dark:text-green-400",
        NodeType::Resource => "text-purple-600 dark:text-purple-400",
        NodeType::Reference => "text-red-600 dark:text-red-400",
    }
}

fn parse_node_type(s: &str) -> NodeType {
    match s {
        "project" => NodeType::Project,
        "area" => NodeType::Area,
        "resource" => NodeType::Resource,
        "reference" => NodeType::Reference,
        _ => NodeType::Article,
    }
}

/// Truncate the body to a short preview (~120 bytes, break at word boundary).
///
/// Slicing a `&str` at a raw byte offset panics if the cut falls inside a
/// multi-byte char (em-dash, curly quote, emoji, accented letter). Walk
/// `char_indices` to find the last char boundary at or before byte 120 so
/// the slice is always UTF-8-safe.
fn body_preview(body: &str) -> String {
    const MAX_BYTES: usize = 120;
    if body.len() <= MAX_BYTES {
        return body.to_string();
    }
    let cut = body
        .char_indices()
        .take_while(|(i, _)| *i <= MAX_BYTES)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let truncated = &body[..cut];
    if let Some(pos) = truncated.rfind(char::is_whitespace) {
        format!("{}…", &truncated[..pos])
    } else {
        format!("{truncated}…")
    }
}

const ALL_TYPES: [NodeType; 5] = [
    NodeType::Article,
    NodeType::Project,
    NodeType::Area,
    NodeType::Resource,
    NodeType::Reference,
];

#[component]
pub fn TemplatesView() -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let prefill = expect_context::<RwSignal<Option<TemplatePrefill>>>();

    let refresh = RwSignal::new(0u32);

    let templates = LocalResource::new(move || {
        let _ = refresh.get();
        async move { crate::api::list_templates().await.unwrap_or_default() }
    });

    // Filter: None = all types, Some(nt) = only that type
    let type_filter: RwSignal<Option<NodeType>> = RwSignal::new(None);

    // editing_id: None = gallery view; "new" = create form; UUID string = edit form
    let editing_id: RwSignal<Option<String>> = RwSignal::new(None);

    let form_name = RwSignal::new(String::new());
    let form_desc = RwSignal::new(String::new());
    let form_type = RwSignal::new("article".to_string());
    let form_body = RwSignal::new(String::new());

    let start_create = move |_| {
        form_name.set(String::new());
        form_desc.set(String::new());
        form_type.set("article".to_string());
        form_body.set(String::new());
        editing_id.set(Some("new".to_string()));
    };

    let start_edit = move |t: NodeTemplate| {
        form_name.set(t.name.clone());
        form_desc.set(t.description.clone().unwrap_or_default());
        form_type.set(node_type_str(&t.node_type).to_string());
        form_body.set(t.body.clone());
        editing_id.set(Some(t.id.0.to_string()));
    };

    let cancel_edit = move || {
        editing_id.set(None);
    };

    let on_save = move || {
        let name = form_name.get_untracked();
        if name.is_empty() {
            push_toast(ToastLevel::Error, "Template name is required.");
            return;
        }
        let desc = {
            let d = form_desc.get_untracked();
            if d.is_empty() { None } else { Some(d) }
        };
        let nt = parse_node_type(&form_type.get_untracked());
        let body = form_body.get_untracked();
        let id_str = editing_id.get_untracked().unwrap_or_default();

        leptos::task::spawn_local(async move {
            let result = if id_str == "new" {
                let req = CreateTemplateRequest {
                    name,
                    description: desc,
                    node_type: nt,
                    body,
                };
                crate::api::create_template(&req).await.map(|_| ())
            } else if let Ok(id) = id_str.parse::<uuid::Uuid>() {
                let req = UpdateTemplateRequest {
                    name,
                    description: desc,
                    node_type: nt,
                    body,
                };
                crate::api::update_template(id, &req).await.map(|_| ())
            } else {
                Err(crate::error::UiError::Parse(
                    "invalid template ID".to_string(),
                ))
            };

            match result {
                Ok(()) => {
                    push_toast(ToastLevel::Success, "Template saved.");
                    editing_id.set(None);
                    refresh.update(|n| *n += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Error: {e}")),
            }
        });
    };

    let on_delete = move |t: NodeTemplate| {
        let id = t.id.0;
        leptos::task::spawn_local(async move {
            match crate::api::delete_template(id).await {
                Ok(()) => {
                    push_toast(ToastLevel::Success, "Template deleted.");
                    refresh.update(|n| *n += 1);
                }
                Err(e) => push_toast(ToastLevel::Error, format!("Error: {e}")),
            }
        });
    };

    let on_use = move |t: NodeTemplate| {
        let type_str = node_type_str(&t.node_type).to_string();
        let body = t.body.clone();
        let tid = t.id;
        prefill.set(Some(TemplatePrefill {
            node_type: type_str,
            body,
            template_id: tid,
        }));
        navigate.get_value()("/nodes/new", Default::default());
    };

    view! {
        <div class="flex-1 flex flex-col min-h-0 p-4 md:p-6">
            // ── Header ────────────────────────────────────────────────────────
            <div class="flex items-center justify-between mb-4">
                <h1 class="text-xl font-semibold text-stone-900 dark:text-stone-100">
                    "Templates"
                </h1>
                <button
                    class="p-1.5 rounded-lg text-stone-400 hover:text-stone-600 dark:hover:text-stone-300
                           hover:bg-stone-100 dark:hover:bg-stone-800 transition-colors cursor-pointer"
                    title="New Template"
                    on:click=start_create
                >
                    <span class="material-symbols-outlined" style="font-size: 18px;">"add"</span>
                </button>
            </div>

            // ── Type filter tabs ──────────────────────────────────────────────
            <div class="flex items-center gap-1 mb-5 flex-wrap">
                <button
                    class=move || {
                        let base = "px-3 py-1.5 rounded-lg text-xs font-medium transition-colors cursor-pointer";
                        if type_filter.get().is_none() {
                            format!("{base} bg-stone-900 dark:bg-stone-100 text-white dark:text-stone-900")
                        } else {
                            format!("{base} bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-400 hover:bg-stone-200 dark:hover:bg-stone-700")
                        }
                    }
                    on:click=move |_| type_filter.set(None)
                >
                    "All"
                </button>
                {ALL_TYPES.iter().map(|nt| {
                    let nt = nt.clone();
                    let label = node_type_label(&nt);
                    let icon = node_type_icon(&nt);
                    let nt_click = nt.clone();
                    let nt_cmp = nt.clone();
                    view! {
                        <button
                            class=move || {
                                let base = "px-3 py-1.5 rounded-lg text-xs font-medium transition-colors cursor-pointer flex items-center gap-1";
                                if type_filter.get().as_ref() == Some(&nt_cmp) {
                                    format!("{base} bg-stone-900 dark:bg-stone-100 text-white dark:text-stone-900")
                                } else {
                                    format!("{base} bg-stone-100 dark:bg-stone-800 text-stone-600 dark:text-stone-400 hover:bg-stone-200 dark:hover:bg-stone-700")
                                }
                            }
                            on:click=move |_| type_filter.set(Some(nt_click.clone()))
                        >
                            <span class="material-symbols-outlined" style="font-size: 14px;">{icon}</span>
                            {label}
                        </button>
                    }
                }).collect_view()}
            </div>

            // ── Inline editor ─────────────────────────────────────────────────
            {move || editing_id.get().map(|eid| {
                let is_new = eid == "new";
                view! {
                    <div class="mb-6 p-4 rounded-xl border border-stone-200 dark:border-stone-700
                                bg-white dark:bg-stone-900 space-y-3">
                        <h2 class="font-medium text-stone-800 dark:text-stone-200">
                            {if is_new { "New Template" } else { "Edit Template" }}
                        </h2>
                        <div>
                            <label class="block text-xs font-medium text-stone-600 dark:text-stone-400 mb-1">
                                "Name"
                            </label>
                            <input
                                type="text"
                                class="w-full px-3 py-2 rounded-lg border border-stone-200
                                       dark:border-stone-600 bg-white dark:bg-stone-800
                                       text-sm text-stone-900 dark:text-stone-100
                                       focus:outline-none focus:ring-2 focus:ring-amber-400"
                                placeholder="Template name"
                                prop:value=move || form_name.get()
                                on:input=move |ev| form_name.set(event_target_value(&ev))
                            />
                        </div>
                        <div>
                            <label class="block text-xs font-medium text-stone-600 dark:text-stone-400 mb-1">
                                "Description (optional)"
                            </label>
                            <input
                                type="text"
                                class="w-full px-3 py-2 rounded-lg border border-stone-200
                                       dark:border-stone-600 bg-white dark:bg-stone-800
                                       text-sm text-stone-900 dark:text-stone-100
                                       focus:outline-none focus:ring-2 focus:ring-amber-400"
                                placeholder="Short description"
                                prop:value=move || form_desc.get()
                                on:input=move |ev| form_desc.set(event_target_value(&ev))
                            />
                        </div>
                        <div>
                            <label class="block text-xs font-medium text-stone-600 dark:text-stone-400 mb-1">
                                "Node Type"
                            </label>
                            <select
                                class="w-full px-3 py-2 rounded-lg border border-stone-200
                                       dark:border-stone-600 bg-white dark:bg-stone-800
                                       text-sm text-stone-900 dark:text-stone-100
                                       focus:outline-none focus:ring-2 focus:ring-amber-400"
                                on:change=move |ev| form_type.set(event_target_value(&ev))
                            >
                                {["article", "project", "area", "resource", "reference"].iter().map(|v| {
                                    let v = *v;
                                    let label = match v {
                                        "project" => "Project",
                                        "area" => "Area",
                                        "resource" => "Resource",
                                        "reference" => "Reference",
                                        _ => "Article",
                                    };
                                    view! {
                                        <option value=v selected=move || form_type.get() == v>
                                            {label}
                                        </option>
                                    }
                                }).collect_view()}
                            </select>
                        </div>
                        <div>
                            <label class="block text-xs font-medium text-stone-600 dark:text-stone-400 mb-1">
                                "Body (Markdown)"
                            </label>
                            <textarea
                                class="w-full px-3 py-2 rounded-lg border border-stone-200
                                       dark:border-stone-600 bg-white dark:bg-stone-800
                                       text-sm text-stone-900 dark:text-stone-100 font-mono
                                       focus:outline-none focus:ring-2 focus:ring-amber-400
                                       min-h-[200px] resize-y"
                                placeholder="Template body in Markdown"
                                prop:value=move || form_body.get()
                                on:input=move |ev| form_body.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="flex items-center gap-1">
                            <IconButton
                                icon="check"
                                label="Save"
                                variant=IconButtonVariant::Save
                                on_click=Callback::new(move |()| on_save())
                            />
                            <IconButton
                                icon="close"
                                label="Cancel"
                                on_click=Callback::new(move |()| cancel_edit())
                            />
                        </div>
                    </div>
                }
            })}

            // ── Template gallery grid ─────────────────────────────────────────
            <Transition fallback=move || view! {
                <div class="text-sm text-stone-400 dark:text-stone-500">"Loading..."</div>
            }>
                {move || templates.get().map(|list| {
                    let filter = type_filter.get();
                    let filtered: Vec<_> = list.iter()
                        .filter(|t| filter.as_ref().is_none_or(|f| t.node_type == *f))
                        .cloned()
                        .collect();

                    if filtered.is_empty() {
                        return view! {
                            <div class="text-sm text-stone-400 dark:text-stone-500">
                                {if list.is_empty() {
                                    "No templates yet. Create one to get started."
                                } else {
                                    "No templates match this filter."
                                }}
                            </div>
                        }.into_any();
                    }
                    view! {
                        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                            {filtered.iter().map(|t| {
                                let t_edit      = t.clone();
                                let t_del       = t.clone();
                                let t_use       = t.clone();
                                let is_default  = t.is_default;
                                let tid         = t.id.0;
                                let type_label  = node_type_label(&t.node_type);
                                let icon        = node_type_icon(&t.node_type);
                                let icon_color  = node_type_color(&t.node_type);
                                let name        = t.name.clone();
                                let desc        = t.description.clone();
                                let preview_html = render_markdown_plain(&body_preview(&t.body));

                                let star_class = if is_default {
                                    "p-1 rounded text-amber-500 \
                                     hover:bg-stone-100 dark:hover:bg-stone-700 \
                                     transition-colors cursor-pointer"
                                } else {
                                    "p-1 rounded text-stone-300 dark:text-stone-600 \
                                     hover:text-amber-500 \
                                     hover:bg-stone-100 dark:hover:bg-stone-700 \
                                     transition-colors cursor-pointer"
                                };
                                let star_icon   = if is_default { "star" } else { "star_border" };
                                let star_title  = if is_default {
                                    "Remove as default for this type"
                                } else {
                                    "Set as default for this type"
                                };

                                view! {
                                    <div class="rounded-xl border border-stone-200 dark:border-stone-700
                                                bg-white dark:bg-stone-900
                                                flex flex-col overflow-hidden
                                                hover:border-stone-300 dark:hover:border-stone-600
                                                transition-colors">
                                        // ── Card header ─────────────────────────────
                                        <div class="px-4 pt-4 pb-2 flex items-start justify-between gap-2">
                                            <div class="flex items-center gap-2 min-w-0">
                                                <span class=format!("material-symbols-outlined {icon_color}")
                                                      style="font-size: 20px;">
                                                    {icon}
                                                </span>
                                                <div class="min-w-0">
                                                    <div class="flex items-center gap-2">
                                                        <span class="font-medium text-stone-900 dark:text-stone-100 truncate text-sm">
                                                            {name}
                                                        </span>
                                                        {is_default.then_some(view! {
                                                            <span class="flex-shrink-0 px-1.5 py-0.5 rounded text-[10px]
                                                                         font-semibold bg-amber-100 dark:bg-amber-900/30
                                                                         text-amber-700 dark:text-amber-400">
                                                                "DEFAULT"
                                                            </span>
                                                        })}
                                                    </div>
                                                    <span class="text-[11px] text-stone-400 dark:text-stone-500">
                                                        {type_label}
                                                    </span>
                                                </div>
                                            </div>
                                            // ── Action icons ────────────────────────
                                            <div class="flex items-center gap-0.5 flex-shrink-0">
                                                <button
                                                    class=star_class
                                                    title=star_title
                                                    on:click=move |_| {
                                                        leptos::task::spawn_local(async move {
                                                            match crate::api::set_template_default(tid).await {
                                                                Ok(updated) => {
                                                                    let msg = if updated.is_default {
                                                                        "Set as default."
                                                                    } else {
                                                                        "Default removed."
                                                                    };
                                                                    push_toast(ToastLevel::Success, msg);
                                                                    refresh.update(|n| *n += 1);
                                                                }
                                                                Err(e) => {
                                                                    push_toast(ToastLevel::Error, format!("Error: {e}"));
                                                                }
                                                            }
                                                        });
                                                    }
                                                >
                                                    <span class="material-symbols-outlined"
                                                          style="font-size: 16px;">{star_icon}</span>
                                                </button>
                                                <button
                                                    class="p-1 rounded text-stone-400
                                                           hover:text-stone-700 dark:hover:text-stone-200
                                                           hover:bg-stone-100 dark:hover:bg-stone-700
                                                           transition-colors cursor-pointer"
                                                    title="Edit"
                                                    on:click=move |_| start_edit(t_edit.clone())
                                                >
                                                    <span class="material-symbols-outlined"
                                                          style="font-size: 16px;">"edit"</span>
                                                </button>
                                                <button
                                                    class="p-1 rounded text-stone-400
                                                           hover:text-red-600 dark:hover:text-red-400
                                                           hover:bg-stone-100 dark:hover:bg-stone-700
                                                           transition-colors cursor-pointer"
                                                    title="Delete"
                                                    on:click=move |_| on_delete(t_del.clone())
                                                >
                                                    <span class="material-symbols-outlined"
                                                          style="font-size: 16px;">"delete"</span>
                                                </button>
                                            </div>
                                        </div>

                                        // ── Description ─────────────────────────────
                                        {desc.map(|d| view! {
                                            <p class="px-4 text-xs text-stone-500 dark:text-stone-400 line-clamp-1">
                                                {d}
                                            </p>
                                        })}

                                        // ── Body preview (rendered markdown) ────────
                                        <div class="px-4 py-3 flex-1 min-h-0">
                                            <div class="text-xs text-stone-600 dark:text-stone-400 leading-relaxed
                                                        line-clamp-4 prose prose-stone dark:prose-invert prose-xs
                                                        max-w-none"
                                                 inner_html=preview_html
                                            />
                                        </div>

                                        // ── Footer: Use button ──────────────────────
                                        <div class="px-4 pb-4 pt-1">
                                            <button
                                                class="w-full py-2 rounded-lg text-xs font-medium
                                                       bg-amber-50 dark:bg-amber-900/20
                                                       text-amber-700 dark:text-amber-400
                                                       hover:bg-amber-100 dark:hover:bg-amber-900/40
                                                       transition-colors cursor-pointer
                                                       border border-amber-200 dark:border-amber-800/40"
                                                on:click=move |_| on_use(t_use.clone())
                                            >
                                                "Use Template"
                                            </button>
                                        </div>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any()
                })}
            </Transition>
        </div>
    }
}
