//! The sticky-note paper every capture surface shares (#240): the Captures
//! wall and the quick-capture composer. The look lives in the card only:
//! the text stays Inter, left-aligned, so a wall of captures still scans.
//!
//! Colour is never the only signal (#240's accessibility rule): each
//! surface also carries the visible "Captures"/"Quick capture" label and
//! the capture glyph (`CAPTURE_ICON`). The tape strip is decoration and
//! is hidden from assistive tech.

use leptos::prelude::*;

/// The capture glyph, shared with the palette's Quick capture command and
/// the composer. `sticky_note_2` is Notes' icon (`sidebar::NOTES_ICON`),
/// and a capture is not a note, so it doesn't borrow it.
pub const CAPTURE_ICON: &str = "bolt";

/// A sheet of sticky-note paper with a strip of tape on top. `class` adds
/// layout (padding, height) for the surface using it.
#[component]
pub fn StickyPaper(
    children: Children,
    #[prop(optional, into)] class: String,
    /// A stale capture's sheet is tilted a little, a cue that rides along
    /// with the age tag (which says it in words).
    #[prop(optional)]
    stale: bool,
) -> impl IntoView {
    let tilt = if stale { " -rotate-1" } else { "" };
    let class = format!(
        "relative rounded-sm border border-sticky-edge dark:border-sticky-dim-edge \
         bg-sticky dark:bg-sticky-dim text-stone-800 dark:text-stone-200 \
         shadow-md shadow-stone-900/10 dark:shadow-black/40{tilt} {class}"
    );
    view! {
        <div class=class>
            <span aria-hidden="true"
                class="pointer-events-none absolute -top-2 left-1/2 -translate-x-1/2 -rotate-2
                       h-4 w-14 rounded-[2px] bg-sticky-tape dark:bg-sticky-dim-tape
                       border border-stone-300/40 dark:border-stone-500/30"></span>
            {children()}
        </div>
    }
}
