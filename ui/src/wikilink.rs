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
    // The one scanner (`common::wikilink::wikilinks`; #281, 3.1). It skips
    // code, so no `<a>` is put inside a code span or a fenced block.
    common::wikilink::replace_wikilinks(content, |link| {
        // Keys are lowercased: wiki links match case-insensitively, as
        // the server's edge sync does (#134 — `[[rust]]` used to render
        // unresolved while still creating a Linked Here edge).
        match title_map.get(&link.target.to_lowercase()) {
            Some(node_id) => format!(
                r#"<a class="wikilink" data-node-id="{}">{}</a>"#,
                node_id.0,
                html_escape(link.display),
            ),
            None => format!(
                r#"<span class="wikilink-unresolved">{}</span>"#,
                html_escape(link.display),
            ),
        }
    })
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

    #[test]
    fn code_is_left_as_typed() {
        // #281, 1.10: an <a> was written into the code span, where Markdown
        // then showed its HTML as text.
        let id = NodeId(uuid::Uuid::from_u128(7));
        let map: HashMap<String, NodeId> = [("rust".to_string(), id)].into();
        let source = "Type `[[Rust]]` to link.\n\n```\n[[Rust]]\n```\n";
        assert_eq!(preprocess_wikilinks(source, &map), source);
        // Outside the code it is still a link.
        let html = preprocess_wikilinks("`[[Rust]]` makes [[Rust]]", &map);
        assert!(
            html.starts_with("`[[Rust]]` makes <a class=\"wikilink\""),
            "{html}"
        );
    }

    #[test]
    fn what_is_not_a_link_stays_text() {
        let map = HashMap::new();
        // No target: nothing to link to, so no empty span either.
        assert_eq!(
            preprocess_wikilinks("a [[]] b [[|x]] c", &map),
            "a [[]] b [[|x]] c"
        );
        // An opener that never closes on its line doesn't swallow the rest.
        let html = preprocess_wikilinks("see [[ this\nand [[That]]", &map);
        assert_eq!(
            html,
            "see [[ this\nand <span class=\"wikilink-unresolved\">That</span>"
        );
        // Nothing after the pipe shows the title, not an empty link.
        assert_eq!(
            preprocess_wikilinks("[[That|]]", &map),
            "<span class=\"wikilink-unresolved\">That</span>"
        );
    }
}
