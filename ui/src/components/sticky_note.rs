//! The sticky-note paper every capture surface shares (#240): the Captures
//! wall and the quick-capture composer. The look lives in the card only:
//! the text stays Inter, left-aligned, so a wall of captures still scans.
//!
//! Colour is never the only signal (#240's accessibility rule): each
//! surface also carries the visible "Captures"/"Quick capture" label and
//! the capture glyph (`CAPTURE_ICON`). The tape strip is decoration and
//! is hidden from assistive tech.
//!
//! The paper colour is the capture's own (#273): the notes' palette keys
//! (`CaptureColor`), the default being the classic yellow. Only the paper
//! changes; the tape, shadow, tilt and wall stay, so a pink sticky note
//! still reads as a capture.

use common::capture::CaptureColor;
use leptos::prelude::*;

/// The capture glyph, shared with the palette's Quick capture command and
/// the composer. `sticky_note_2` is Notes' icon (`sidebar::NOTES_ICON`),
/// and a capture is not a note, so it doesn't borrow it.
pub const CAPTURE_ICON: &str = "bolt";

/// The paper and its edge, light and dark, for each colour. Whole class
/// strings, so Tailwind's scanner sees them. Amber is a shade deeper than
/// the notes' (200, not 100) so it doesn't look like the default yellow.
#[must_use]
pub fn paper_class(color: CaptureColor) -> &'static str {
    match color {
        CaptureColor::Default => {
            "border-sticky-edge dark:border-sticky-dim-edge bg-sticky dark:bg-sticky-dim"
        }
        CaptureColor::Amber => {
            "border-amber-300 dark:border-amber-800 bg-amber-200 dark:bg-amber-950"
        }
        CaptureColor::Rose => "border-rose-200 dark:border-rose-900 bg-rose-100 dark:bg-rose-950",
        CaptureColor::Lime => "border-lime-200 dark:border-lime-900 bg-lime-100 dark:bg-lime-950",
        CaptureColor::Sky => "border-sky-200 dark:border-sky-900 bg-sky-100 dark:bg-sky-950",
        CaptureColor::Violet => {
            "border-violet-200 dark:border-violet-900 bg-violet-100 dark:bg-violet-950"
        }
    }
}

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
    /// The paper (#273); the default yellow when absent.
    #[prop(optional, into)]
    color: MaybeProp<CaptureColor>,
) -> impl IntoView {
    let tilt = if stale { " -rotate-1" } else { "" };
    let class = move || {
        format!(
            "relative rounded-sm border {} text-stone-800 dark:text-stone-200 \
             shadow-md shadow-stone-900/10 dark:shadow-black/40{tilt} {class}",
            paper_class(color.get().unwrap_or_default())
        )
    };
    // `data-color` names the paper for tests, which can't read a colour.
    let key = move || color.get().unwrap_or_default().as_str();
    view! {
        <div class=class data-color=key>
            <span aria-hidden="true"
                class="pointer-events-none absolute -top-2 left-1/2 -translate-x-1/2 -rotate-2
                       h-4 w-14 rounded-[2px] bg-sticky-tape dark:bg-sticky-dim-tape
                       border border-stone-300/40 dark:border-stone-500/30"></span>
            {children()}
        </div>
    }
}

/// The paper colour row (#273): one swatch per colour, in a labelled
/// group. Each swatch says its colour's name to assistive tech and whether
/// it is chosen (`aria-pressed`), so colour is never the only signal.
#[component]
pub fn PaperPicker(
    selected: RwSignal<CaptureColor>,
    /// Runs after a pick; the composers use it to put the focus back in
    /// their text box.
    #[prop(optional)]
    on_pick: Option<Callback<()>>,
) -> impl IntoView {
    view! {
        <div role="group" aria-label="Paper colour" data-testid="paper-picker"
            class="flex items-center gap-1.5">
            {CaptureColor::ALL.into_iter().map(|color| {
                let is_on = move || selected.get() == color;
                view! {
                    <button
                        type="button"
                        title=color.label()
                        aria-label=color.label()
                        aria-pressed=move || if is_on() { "true" } else { "false" }
                        class=move || format!(
                            "h-5 w-5 shrink-0 rounded-full border transition-transform {} {}",
                            paper_class(color),
                            if is_on() {
                                "ring-2 ring-ember ring-offset-1 ring-offset-white \
                                 dark:ring-offset-stone-900 scale-110"
                            } else {
                                "hover:scale-110"
                            }
                        )
                        on:click=move |_| {
                            selected.set(color);
                            if let Some(cb) = on_pick {
                                cb.run(());
                            }
                        }
                    ></button>
                }
            }).collect_view()}
        </div>
    }
}
