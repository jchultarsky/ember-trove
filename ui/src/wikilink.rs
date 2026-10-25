use std::collections::HashMap;

use common::id::NodeId;

/// Preprocess markdown content, converting `[[title]]` and `[[title|display]]`
/// (`title_map` keys are lowercased titles: see [`use_wikilink_map`])
/// wiki-links into HTML that the Leptos click handler can intercept.
///
/// - **Resolved** links → `<a class="wikilink" data-node-id="<uuid>">display</a>`
/// - **Unresolved** links → `<span class="wikilink-unresolved">display</span>`
///
/// The returned string is passed to `pulldown_cmark` before HTML rendering.
/// Inline HTML is preserved by pulldown-cmark and then sanitized by ammonia
/// (which must be configured to allow `class` and `data-node-id` on `<a>`).
pub fn preprocess_wikilinks(content: &str, title_map: &HashMap<String, NodeId>) -> String {
    let mut result = String::with_capacity(content.len() + 64);
    let mut remaining = content;

    while let Some(open) = remaining.find("[[") {
        result.push_str(&remaining[..open]);
        remaining = &remaining[open + 2..];

        if let Some(close) = remaining.find("]]") {
            let inner = &remaining[..close];
            let (target, display) = if let Some(pipe) = inner.find('|') {
                (inner[..pipe].trim(), inner[pipe + 1..].trim())
            } else {
                let t = inner.trim();
                (t, t)
            };

            // Keys are lowercased: wiki links match case-insensitively, as
            // the server's edge sync does (#134 — `[[rust]]` used to render
            // unresolved while still creating a Linked Here edge).
            if let Some(node_id) = title_map.get(&target.to_lowercase()) {
                result.push_str(&format!(
                    r#"<a class="wikilink" data-node-id="{}">{}</a>"#,
                    node_id.0,
                    html_escape(display),
                ));
            } else {
                result.push_str(&format!(
                    r#"<span class="wikilink-unresolved">{}</span>"#,
                    html_escape(display),
                ));
            }
            remaining = &remaining[close + 2..];
        } else {
            // Unclosed [[ — emit as-is and stop
            result.push_str("[[");
            result.push_str(remaining);
            return result;
        }
    }

    result.push_str(remaining);
    result
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The lowercased-title → node map for the `[[links]]` in `body`, fetched
/// on demand (#134): only the titles the body actually links, resolved by
/// the server among nodes the caller can see. Debounced (the editor's
/// preview changes per keystroke) and cached, so a title is asked for once.
/// Node pages used to download every visible title just to render links.
pub fn use_wikilink_map(
    body: leptos::prelude::Signal<String>,
) -> leptos::prelude::Signal<HashMap<String, NodeId>> {
    use leptos::prelude::*;
    // title -> Some(id) resolved, None = asked, no visible match.
    let known: RwSignal<HashMap<String, Option<NodeId>>> = RwSignal::new(HashMap::new());
    let version = RwSignal::new(0u32);
    Effect::new(move |_| {
        let titles = common::wikilink::linked_titles_lowered(&body.get());
        let missing: Vec<String> =
            known.with_untracked(|k| titles.into_iter().filter(|t| !k.contains_key(t)).collect());
        if missing.is_empty() {
            return;
        }
        version.update(|v| *v += 1);
        let mine = version.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(300).await;
            if version.try_get_untracked() != Some(mine) {
                return;
            }
            if let Ok(found) = crate::api::resolve_titles(&missing).await {
                known.try_update(|k| {
                    for t in missing {
                        let id = found.get(&t).copied();
                        k.insert(t, id);
                    }
                });
            }
        });
    });
    Signal::derive(move || {
        known.with(|k| {
            k.iter()
                .filter_map(|(t, id)| id.map(|id| (t.clone(), id)))
                .collect()
        })
    })
}

#[cfg(test)]
mod case_tests {
    use super::*;

    #[test]
    fn links_resolve_case_insensitively() {
        let id = NodeId(uuid::Uuid::from_u128(7));
        let map: HashMap<String, NodeId> = [("rust language".to_string(), id)].into();
        let html = preprocess_wikilinks("See [[Rust Language]] and [[rust language|it]].", &map);
        assert_eq!(html.matches("class=\"wikilink\"").count(), 2, "{html}");
        assert!(!html.contains("wikilink-unresolved"));
    }
}
