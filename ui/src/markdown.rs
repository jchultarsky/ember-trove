//! Shared Markdown rendering utilities.
//!
//! All Markdown-to-HTML rendering passes through `ammonia` for sanitisation.
//! The builder allows a curated set of HTML elements and attributes so that
//! users can write rich content (coloured text, highlights, etc.) without
//! exposing the application to XSS.
//!
//! ## Inline styling
//!
//! Users may use raw HTML within Markdown to apply inline styles:
//!
//! ```markdown
//! <span style="color: #e85d04;">Important!</span>
//! Normal text with <span style="background-color: #fef9c3;">highlight</span>.
//! ```
//!
//! The `style` attribute is permitted on a range of block and inline elements,
//! but its VALUE is filtered (see [`sanitize_style`]) down to a safe CSS
//! property allowlist (colour / weight / decoration). Layout properties
//! (`position`, `z-index`, offsets, sizes) and `url()` are stripped so that a
//! crafted note can't build a full-viewport clickjacking/phishing overlay or a
//! tracking-pixel — relevant now that one owner's content may be rendered in
//! another user's (or an admin's) session.

use crate::wikilink::preprocess_wikilinks;
use common::id::NodeId;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd, html as cmark_html};
use std::borrow::Cow;
use std::collections::HashMap;

/// Markdown extensions enabled for all renderers.
const MD_OPTIONS: Options = Options::ENABLE_STRIKETHROUGH
    .union(Options::ENABLE_TABLES)
    .union(Options::ENABLE_TASKLISTS);

/// Elements on which the `style` attribute is permitted.
const STYLED_ELEMENTS: &[&str] = &[
    "span",
    "div",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "strong",
    "em",
    "s",
    "u",
    "code",
    "pre",
    "blockquote",
    "ul",
    "ol",
    "li",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
];

/// CSS properties allowed to survive in a sanitised `style` attribute. Chosen
/// to support inline colour / highlight / emphasis while excluding anything
/// that affects layout/positioning (the overlay-clickjacking vector).
const SAFE_STYLE_PROPS: &[&str] = &[
    "color",
    "background-color",
    "font-weight",
    "font-style",
    "font-size",
    "text-decoration",
    "text-align",
];

/// Filter a `style` attribute value to the [`SAFE_STYLE_PROPS`] allowlist,
/// dropping unknown properties and any value containing `url(`, `expression`,
/// `javascript`, or a comment. Returns `None` when nothing safe remains (the
/// attribute is then removed entirely).
fn sanitize_style(value: &str) -> Option<String> {
    let mut kept: Vec<String> = Vec::new();
    for decl in value.split(';') {
        let Some((prop, val)) = decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let val = val.trim();
        if !SAFE_STYLE_PROPS.contains(&prop.as_str()) || val.is_empty() {
            continue;
        }
        let lv = val.to_ascii_lowercase();
        if lv.contains("url(")
            || lv.contains("expression")
            || lv.contains("javascript")
            || lv.contains("/*")
        {
            continue;
        }
        kept.push(format!("{prop}: {val}"));
    }
    if kept.is_empty() {
        None
    } else {
        Some(kept.join("; "))
    }
}

/// Build a pre-configured ammonia sanitiser that:
/// - Preserves the default-allowed tag set (headings, lists, links, etc.)
/// - Adds `<span>`, `<div>`, `<input>` (for task-list checkboxes)
/// - Permits `style` on block/inline elements, with its VALUE filtered to a
///   safe CSS-property allowlist (see [`sanitize_style`])
/// - Permits `class` and `data-node-id` on `<a>` (WikiLink integration)
/// - Permits `class` on `<span>` (WikiLink unresolved spans)
fn sanitizer() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::new();
    b.add_tags(&["span", "div", "input"]);
    b.add_tag_attributes("a", &["class", "data-node-id"]);
    b.add_tag_attributes("span", &["class"]);
    b.add_tag_attributes("input", &["type", "checked", "disabled"]);
    b.add_tag_attributes("img", &["src", "alt", "width", "height", "style"]);
    for &tag in STYLED_ELEMENTS {
        b.add_tag_attributes(tag, &["style"]);
    }
    b.add_tag_attributes("h2", &["id"]);
    b.add_tag_attributes("h3", &["id"]);
    // SECURITY: sanitise the value of every surviving `style` attribute so it
    // can carry colour/emphasis but not layout/positioning (overlay) CSS.
    // Heading ids survive only in the generated `ember-h-N` form (#185), so a
    // body can't name an id the app looks up or clobber a `window` global.
    b.attribute_filter(|_element, attribute, value| match attribute {
        "style" => sanitize_style(value).map(Cow::Owned),
        "id" => is_outline_id(value).then_some(Cow::Borrowed(value)),
        _ => Some(Cow::Borrowed(value)),
    });
    b
}

/// Render Markdown with WikiLink resolution.
///
/// `[[title]]` and `[[title|display]]` are first expanded by
/// [`preprocess_wikilinks`], then rendered with pulldown-cmark, and finally
/// sanitised by ammonia.
pub fn render_markdown(source: &str, title_map: &HashMap<String, NodeId>) -> String {
    let preprocessed = preprocess_wikilinks(source, title_map);
    let parser = Parser::new_ext(&preprocessed, MD_OPTIONS);
    let mut html_out = String::new();
    cmark_html::push_html(&mut html_out, parser);
    sanitizer().clean(&html_out).to_string()
}

// ── Body outline (#185) ─────────────────────────────────────────────────────

/// Prefix of the ids [`render_body`] puts on h2/h3 headings.
const OUTLINE_ID_PREFIX: &str = "ember-h-";

/// The outline shows only from this many entries; below it, it is noise.
pub const OUTLINE_MIN_HEADINGS: usize = 3;

/// `ember-h-<digits>`: the only id form the sanitiser keeps.
fn is_outline_id(value: &str) -> bool {
    value
        .strip_prefix(OUTLINE_ID_PREFIX)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// One `##` / `###` heading of a node body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineEntry {
    /// 2 or 3.
    pub level: u8,
    /// The heading's plain text (markup and link targets dropped).
    pub text: String,
    /// The `id` of the heading in the rendered body.
    pub id: String,
}

/// A node body rendered for the node page: the HTML and the outline of its
/// headings, taken from the same parse so they can't disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedBody {
    pub html: String,
    pub outline: Vec<OutlineEntry>,
}

impl RenderedBody {
    /// The outline to show: `None` below [`OUTLINE_MIN_HEADINGS`] entries.
    pub fn visible_outline(&self) -> Option<&[OutlineEntry]> {
        (self.outline.len() >= OUTLINE_MIN_HEADINGS).then_some(self.outline.as_slice())
    }
}

/// [`render_markdown`] for the node page (#185): the same HTML, except that
/// every h2/h3 carries an `ember-h-N` id, plus the outline of those headings.
/// Headings with no text get an id but no outline entry.
pub fn render_body(source: &str, title_map: &HashMap<String, NodeId>) -> RenderedBody {
    let preprocessed = preprocess_wikilinks(source, title_map);
    let mut outline = Vec::new();
    let mut next_id = 0usize;
    // The open h2/h3: (level, id, text so far).
    let mut current: Option<(u8, String, String)> = None;
    let events = Parser::new_ext(&preprocessed, MD_OPTIONS).map(|ev| match ev {
        Event::Start(Tag::Heading {
            level: level @ (HeadingLevel::H2 | HeadingLevel::H3),
            classes,
            attrs,
            ..
        }) => {
            let id = format!("{OUTLINE_ID_PREFIX}{next_id}");
            next_id += 1;
            let n = if level == HeadingLevel::H2 { 2 } else { 3 };
            current = Some((n, id.clone(), String::new()));
            Event::Start(Tag::Heading {
                level,
                id: Some(id.into()),
                classes,
                attrs,
            })
        }
        Event::End(TagEnd::Heading(_)) => {
            if let Some((level, id, text)) = current.take() {
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() {
                    outline.push(OutlineEntry { level, text, id });
                }
            }
            ev
        }
        Event::Text(ref t) | Event::Code(ref t) => {
            if let Some((_, _, text)) = current.as_mut() {
                text.push_str(t);
            }
            ev
        }
        Event::SoftBreak | Event::HardBreak => {
            if let Some((_, _, text)) = current.as_mut() {
                text.push(' ');
            }
            ev
        }
        _ => ev,
    });
    let mut html_out = String::new();
    cmark_html::push_html(&mut html_out, events);
    RenderedBody {
        html: sanitizer().clean(&html_out).to_string(),
        outline,
    }
}

/// Render Markdown without WikiLink resolution (notes, public share view).
pub fn render_markdown_plain(source: &str) -> String {
    let parser = Parser::new_ext(source, MD_OPTIONS);
    let mut html_out = String::new();
    cmark_html::push_html(&mut html_out, parser);
    sanitizer().clean(&html_out).to_string()
}

/// Render Markdown **inline only** — emphasis, strong, strikethrough, inline
/// code, and links — flattening every block wrapper (paragraphs, headings,
/// lists, blockquotes, tables) and line breaks into spaces so the result stays
/// on a single line. Used for task titles, which display as one-line truncated
/// labels in rows; full block rendering would break that layout.
pub fn render_markdown_inline(source: &str) -> String {
    let events = Parser::new_ext(source, MD_OPTIONS).filter_map(|ev| match ev {
        Event::Start(tag) => Some(match tag {
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } => {
                Event::Start(tag)
            }
            // Any block wrapper (or image) → a space at the boundary.
            _ => Event::Text(" ".into()),
        }),
        Event::End(end) => Some(match end {
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                Event::End(end)
            }
            _ => Event::Text(" ".into()),
        }),
        // Keep inline text/code/html as-is.
        e @ (Event::Text(_) | Event::Code(_) | Event::InlineHtml(_)) => Some(e),
        // Collapse breaks/rules to a single space (stay one line).
        Event::SoftBreak | Event::HardBreak | Event::Rule => Some(Event::Text(" ".into())),
        // Drop block HTML, footnote refs, task-list checkboxes, etc.
        _ => None,
    });
    let mut html_out = String::new();
    cmark_html::push_html(&mut html_out, events);
    sanitizer().clean(html_out.trim()).to_string()
}

// ── List-card excerpts (#175) ──────────────────────────────────────────────────

/// Visible-text budget for a card excerpt. Rendering stops at the first block
/// boundary past it; the card's CSS line clamp makes the visual cut, so this
/// only keeps the DOM small for long bodies.
const EXCERPT_BUDGET: usize = 400;

/// The one class an excerpt may carry: inert link / wiki-link text.
const EXCERPT_LINK_CLASS: &str = "excerpt-link";

/// Rewrite `[[target]]` / `[[target|display]]` as a Markdown link with the
/// display text, so the excerpt renderer styles it like any other link.
/// Brackets and backslashes in the display text are escaped so they can't
/// close the link early. An unclosed `[[` is left as-is.
fn excerpt_wikilinks(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(open) = rest.find("[[") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &after[..close];
        let display = inner.split_once('|').map_or(inner, |(_, d)| d).trim();
        out.push('[');
        for c in display.chars() {
            if matches!(c, '[' | ']' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push_str("](#)");
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

/// Strict sanitiser for excerpts: text-level markup and lists only. No
/// `<a>`, `<img>`, `<input>`, `style` or raw user HTML can survive, so an
/// excerpt is never interactive inside its (clickable) card.
fn excerpt_sanitizer() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.tags(
        [
            "p", "strong", "em", "del", "code", "ul", "ol", "li", "span", "br",
        ]
        .into_iter()
        .collect(),
    );
    b.add_allowed_classes("span", &[EXCERPT_LINK_CLASS]);
    b
}

/// Render the top of a node body as a card-safe HTML excerpt (#175), or
/// `None` when the body has no visible text.
///
/// Paragraphs, lists, emphasis, inline code and links render; headings are
/// demoted to bold paragraphs (no heading levels or heading scale in a card);
/// links and wiki-links become inert styled text; task-list markers become
/// ☐ / ☑; a table collapses to its header cells joined by " · ". Images, code
/// blocks, rules, footnotes and raw HTML are dropped. Rendering stops at the
/// first top-level block boundary or list item past [`EXCERPT_BUDGET`]
/// characters, so a body that is one long list is cut short too.
pub fn render_excerpt(source: &str) -> Option<String> {
    let prepared = excerpt_wikilinks(source);
    let mut out: Vec<Event> = Vec::new();
    let mut depth = 0usize;
    // Depth at which a dropped region started; everything inside is skipped.
    let mut skip_from: Option<usize> = None;
    let mut in_table_head = false;
    let mut head_cells = 0usize;
    let mut text_len = 0usize;
    // Closers of the tags emitted and still open, innermost last, so a cut
    // inside a list can close everything it opened.
    let mut open: Vec<Event> = Vec::new();

    for ev in Parser::new_ext(&prepared, MD_OPTIONS) {
        match &ev {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        if let Some(from) = skip_from {
            if matches!(ev, Event::End(_)) && depth < from {
                skip_from = None;
            }
            continue;
        }
        let mapped: Option<Event> = match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph
                | Tag::List(_)
                | Tag::Item
                | Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough => {
                    open.push(Event::End(tag.to_end()));
                    Some(Event::Start(tag))
                }
                Tag::Heading { .. } => {
                    open.push(Event::Html("</strong></p>".into()));
                    Some(Event::Html("<p><strong>".into()))
                }
                Tag::Link { .. } => {
                    open.push(Event::Html("</span>".into()));
                    Some(Event::Html(
                        format!(r#"<span class="{EXCERPT_LINK_CLASS}">"#).into(),
                    ))
                }
                Tag::Table(_) => {
                    open.push(Event::Html("</p>".into()));
                    Some(Event::Html("<p>".into()))
                }
                Tag::TableHead => {
                    in_table_head = true;
                    head_cells = 0;
                    None
                }
                Tag::TableCell if in_table_head => {
                    head_cells += 1;
                    (head_cells > 1).then(|| Event::Text(" \u{b7} ".into()))
                }
                // Blockquote wrappers vanish; their paragraphs stay.
                Tag::BlockQuote(_) => None,
                // Body rows, images, code, raw HTML, footnotes, metadata, …
                _ => {
                    skip_from = Some(depth);
                    None
                }
            },
            Event::End(end) => match end {
                TagEnd::Paragraph
                | TagEnd::List(_)
                | TagEnd::Item
                | TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Heading(_)
                | TagEnd::Link
                | TagEnd::Table => open.pop(),
                TagEnd::TableHead => {
                    in_table_head = false;
                    None
                }
                _ => None,
            },
            Event::Text(t) => {
                text_len += t.trim().chars().count();
                Some(Event::Text(t))
            }
            Event::Code(c) => {
                text_len += c.chars().count();
                Some(Event::Code(c))
            }
            Event::TaskListMarker(done) => Some(Event::Text(
                if done { "\u{2611} " } else { "\u{2610} " }.into(),
            )),
            e @ (Event::SoftBreak | Event::HardBreak) => Some(e),
            // Raw HTML, rules, footnote refs, math: not in a card.
            _ => None,
        };
        let item_done = matches!(mapped, Some(Event::End(TagEnd::Item)));
        out.extend(mapped);
        if text_len >= EXCERPT_BUDGET && (depth == 0 || item_done) {
            out.extend(open.drain(..).rev());
            break;
        }
    }

    if text_len == 0 {
        return None;
    }
    let mut html_out = String::new();
    cmark_html::push_html(&mut html_out, out.into_iter());
    Some(excerpt_sanitizer().clean(&html_out).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_renders_inline_markup_and_lists() {
        let html = render_excerpt("Some **bold** and *em* and `code`.\n\n- one\n- two").unwrap();
        assert!(html.contains("<strong>bold</strong>"), "{html}");
        assert!(html.contains("<em>em</em>"), "{html}");
        assert!(html.contains("<code>code</code>"), "{html}");
        assert!(html.contains("<ul>"), "{html}");
        assert!(html.contains("<li>one</li>"), "{html}");
    }

    #[test]
    fn excerpt_demotes_headings_to_bold_paragraphs() {
        let html = render_excerpt("## Status\n\nOn track.").unwrap();
        assert!(html.contains("<p><strong>Status</strong></p>"), "{html}");
        assert!(!html.contains("<h"), "{html}");
        assert!(!html.contains('#'), "{html}");
    }

    #[test]
    fn excerpt_links_are_inert_text() {
        let html = render_excerpt("See [the docs](https://example.com) now.").unwrap();
        assert!(!html.contains("<a"), "{html}");
        assert!(!html.contains("example.com"), "{html}");
        assert!(
            html.contains(r#"<span class="excerpt-link">the docs</span>"#),
            "{html}"
        );
    }

    #[test]
    fn excerpt_wikilinks_show_display_text() {
        let html = render_excerpt("Ask [[Alice Smith]] about [[Project X|the project]].").unwrap();
        assert!(!html.contains("[["), "{html}");
        assert!(html.contains(">Alice Smith</span>"), "{html}");
        assert!(html.contains(">the project</span>"), "{html}");
        assert!(!html.contains("Project X"), "{html}");
    }

    #[test]
    fn excerpt_wikilink_brackets_cannot_break_out() {
        let html = render_excerpt("x [[a|b] (c)]] y").unwrap();
        assert!(!html.contains("<a"), "{html}");
        assert!(html.contains("b] (c)"), "{html}");
    }

    #[test]
    fn excerpt_task_markers_become_glyphs() {
        let html = render_excerpt("- [ ] open\n- [x] done").unwrap();
        assert!(html.contains("\u{2610} open"), "{html}");
        assert!(html.contains("\u{2611} done"), "{html}");
        assert!(!html.contains("<input"), "{html}");
    }

    #[test]
    fn excerpt_table_collapses_to_header_cells() {
        let html = render_excerpt("| Name | Role |\n|---|---|\n| Bob | Lead |").unwrap();
        assert!(html.contains("Name \u{b7} Role"), "{html}");
        assert!(!html.contains("Bob"), "{html}");
        assert!(!html.contains('|'), "{html}");
    }

    #[test]
    fn excerpt_drops_images_code_blocks_and_rules() {
        let html =
            render_excerpt("![alt](https://x/y.png)\n\n```\nfn main() {}\n```\n\n---\n\nText.")
                .unwrap();
        assert!(!html.contains("img"), "{html}");
        assert!(!html.contains("main"), "{html}");
        assert!(!html.contains("<hr"), "{html}");
        assert!(html.contains("<p>Text.</p>"), "{html}");
    }

    #[test]
    fn excerpt_strips_raw_html() {
        let html = render_excerpt(
            "<script>alert(1)</script>\n\nHi <img src=x onerror=alert(1)> <span style=\"position:fixed\">there</span>",
        )
        .unwrap();
        assert!(!html.contains("script"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("style"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("there"), "{html}");
    }

    #[test]
    fn excerpt_of_empty_or_markup_only_body_is_none() {
        assert_eq!(render_excerpt(""), None);
        assert_eq!(render_excerpt("  \n\n---\n"), None);
        assert_eq!(render_excerpt("![](https://x/y.png)"), None);
    }

    #[test]
    fn excerpt_stops_at_a_block_boundary_past_the_budget() {
        let para = "word ".repeat(100);
        let body = format!("{para}\n\nSECOND\n\nTHIRD");
        let html = render_excerpt(&body).unwrap();
        assert!(html.trim_end().ends_with("</p>"), "{html}");
        assert!(!html.contains("SECOND"), "{html}");
        // A short body is rendered whole.
        let short = render_excerpt("one\n\ntwo").unwrap();
        assert!(short.contains("two"), "{short}");
    }

    #[test]
    fn excerpt_of_one_long_list_stops_at_an_item_past_the_budget() {
        // A body that is a single long list (a big task list) has no
        // top-level boundary until its end; it must still be cut short.
        let items: String = (0..500).map(|i| format!("- [ ] task {i}\n")).collect();
        let html = render_excerpt(&format!("{items}- LAST")).unwrap();
        assert!(!html.contains("LAST"), "the whole list was rendered");
        assert!(html.len() < 4_000, "{} bytes", html.len());
        // Cut at an item boundary, with every open tag closed.
        assert!(html.trim_end().ends_with("</li>\n</ul>"), "{html}");
        // The same inside a nested list: both lists are closed.
        let nested: String = (0..500).map(|i| format!("  - sub {i}\n")).collect();
        let html = render_excerpt(&format!("- top\n{nested}- LAST")).unwrap();
        assert!(!html.contains("LAST"), "the whole list was rendered");
        assert_eq!(
            html.matches("<ul>").count(),
            html.matches("</ul>").count(),
            "{html}"
        );
        assert_eq!(
            html.matches("<li>").count(),
            html.matches("</li>").count(),
            "{html}"
        );
    }

    // ── Body outline (#185) ──

    fn body(src: &str) -> RenderedBody {
        render_body(src, &HashMap::new())
    }

    #[test]
    fn outline_lists_h2_and_h3_in_order_with_matching_ids() {
        let r = body("# Title\n\n## Status\n\ntext\n\n### Detail\n\n## Goals\n\n#### Deep\n");
        let got: Vec<_> = r
            .outline
            .iter()
            .map(|e| (e.level, e.text.as_str(), e.id.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (2, "Status", "ember-h-0"),
                (3, "Detail", "ember-h-1"),
                (2, "Goals", "ember-h-2"),
            ]
        );
        // Every outline id is on the matching heading in the rendered body.
        assert!(
            r.html.contains(r#"<h2 id="ember-h-0">Status</h2>"#),
            "{}",
            r.html
        );
        assert!(
            r.html.contains(r#"<h3 id="ember-h-1">Detail</h3>"#),
            "{}",
            r.html
        );
        assert!(
            r.html.contains(r#"<h2 id="ember-h-2">Goals</h2>"#),
            "{}",
            r.html
        );
        // h1 and h4 are not in the outline and get no id.
        assert!(r.html.contains("<h1>Title</h1>"), "{}", r.html);
        assert!(r.html.contains("<h4>Deep</h4>"), "{}", r.html);
    }

    #[test]
    fn outline_text_is_plain_text_of_the_heading() {
        let mut map = HashMap::new();
        map.insert("alice".to_string(), NodeId(uuid::Uuid::nil()));
        let r = render_body(
            "## Ask **[[Alice|her]]** about `code` & [docs](https://x)\n",
            &map,
        );
        assert_eq!(r.outline[0].text, "Ask her about code & docs");
    }

    #[test]
    fn setext_headings_count_too() {
        let r = body("Status\n------\n\ntext\n");
        assert_eq!(r.outline.len(), 1);
        assert_eq!(r.outline[0].text, "Status");
        assert!(r.html.contains(r#"<h2 id="ember-h-0">"#), "{}", r.html);
    }

    #[test]
    fn empty_headings_get_no_outline_entry() {
        let r = body("##\n\n## Real\n");
        let texts: Vec<_> = r.outline.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["Real"]);
    }

    #[test]
    fn outline_shows_from_three_entries() {
        assert_eq!(body("## a\n## b\n").visible_outline(), None);
        assert_eq!(
            body("## a\n## b\n### c\n")
                .visible_outline()
                .map(<[_]>::len),
            Some(OUTLINE_MIN_HEADINGS)
        );
        assert_eq!(body("").visible_outline(), None);
    }

    #[test]
    fn body_html_matches_render_markdown_apart_from_heading_ids() {
        let src = "## A\n\nSome *text* with [[x]].\n\n- [ ] t\n";
        let map = HashMap::new();
        let plain = render_markdown(src, &map);
        let with_ids = render_body(src, &map)
            .html
            .replace(r#" id="ember-h-0""#, "");
        assert_eq!(plain, with_ids);
    }

    /// SECURITY: only the generated `ember-h-N` ids on h2/h3 survive the
    /// sanitiser. A user-written id could otherwise shadow an app element
    /// (`getElementById`) or a `window` global (DOM clobbering).
    #[test]
    fn user_written_ids_are_stripped() {
        let r = body(
            "<h2 id=\"node-tasks-heading\">x</h2>\n\n<div id=\"ember-h-0\">y</div>\n\n<h3 id=\"ember-h-9x\">z</h3>\n\n<a id=\"ember-h-1\">w</a>\n",
        );
        assert!(!r.html.contains("node-tasks-heading"), "{}", r.html);
        assert!(!r.html.contains("id="), "{}", r.html);
        // The plain renderers keep no ids at all.
        assert!(!render_markdown("## A\n", &HashMap::new()).contains("id="));
        assert!(!render_markdown_plain("## A\n").contains("id="));
    }
}
