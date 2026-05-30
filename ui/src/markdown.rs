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
//! The `style` attribute is permitted on a wide range of block and inline
//! elements. Because this is a private, self-hosted PKM system the risk of
//! malicious style injection is negligible, and we trust the owner's content.

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

/// Build a pre-configured ammonia sanitiser that:
/// - Preserves the default-allowed tag set (headings, lists, links, etc.)
/// - Adds `<span>`, `<div>`, `<input>` (for task-list checkboxes)
/// - Permits `style` on all block/inline elements (inline colour + highlight)
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
