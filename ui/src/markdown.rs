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

use std::borrow::Cow;
use std::collections::HashMap;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html as cmark_html};
use common::id::NodeId;
use crate::wikilink::preprocess_wikilinks;

/// Markdown extensions enabled for all renderers.
const MD_OPTIONS: Options = Options::ENABLE_STRIKETHROUGH
    .union(Options::ENABLE_TABLES)
    .union(Options::ENABLE_TASKLISTS);

/// Elements on which the `style` attribute is permitted.
const STYLED_ELEMENTS: &[&str] = &[
    "span", "div", "p",
    "h1", "h2", "h3", "h4", "h5", "h6",
    "strong", "em", "s", "u", "code", "pre", "blockquote",
    "ul", "ol", "li",
    "table", "thead", "tbody", "tr", "th", "td",
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
