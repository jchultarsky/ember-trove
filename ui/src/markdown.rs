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
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html as cmark_html};
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
    // SECURITY: sanitise the value of every surviving `style` attribute so it
    // can carry colour/emphasis but not layout/positioning (overlay) CSS.
    b.attribute_filter(|_element, attribute, value| {
        if attribute == "style" {
            sanitize_style(value).map(Cow::Owned)
        } else {
            Some(Cow::Borrowed(value))
        }
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
}
