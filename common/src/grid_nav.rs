//! Keyboard movement over a wall of cards laid out in a responsive grid
//! (the Captures wall, #240). The UI reads each card's position at keypress
//! time and asks where a key goes; this module decides, so the rules are
//! tested without a browser.
//!
//! - `Next` / `Prev` follow reading order (the cards' order in the list) and
//!   stop at either end.
//! - `Up` / `Down` go to the row above or below, to the card nearest in x;
//!   on a tie, the earlier card. A ragged last row works: going down from a
//!   column it lacks lands on its nearest card. From the top or bottom row
//!   the focus stays put.
//! - With nothing focused yet, any step starts at the first card.

/// A card's top-left corner, in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub x: f64,
    pub y: f64,
}

/// Where a key asks to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Next,
    Prev,
    Up,
    Down,
}

/// Cards whose tops differ by less than this share a row (sub-pixel layout,
/// a tilted stale card).
const ROW_TOLERANCE: f64 = 8.0;

/// The index a step lands on, or `None` when there are no cards.
pub fn step(cells: &[Cell], current: Option<usize>, step: Step) -> Option<usize> {
    let last = cells.len().checked_sub(1)?;
    let Some(cur) = current.filter(|&i| i <= last) else {
        return Some(0);
    };
    Some(match step {
        Step::Next => (cur + 1).min(last),
        Step::Prev => cur.saturating_sub(1),
        Step::Down => vertical(cells, cur, true).unwrap_or(cur),
        Step::Up => vertical(cells, cur, false).unwrap_or(cur),
    })
}

/// The nearest card in the adjacent row below (`down`) or above.
fn vertical(cells: &[Cell], cur: usize, down: bool) -> Option<usize> {
    let here = cells[cur];
    let beyond = |c: &Cell| {
        if down {
            c.y > here.y + ROW_TOLERANCE
        } else {
            c.y < here.y - ROW_TOLERANCE
        }
    };
    // The adjacent row's top: the nearest y past this row.
    let row_y = cells
        .iter()
        .filter(|c| beyond(c))
        .map(|c| c.y)
        .reduce(|a, b| {
            if (down && b < a) || (!down && b > a) {
                b
            } else {
                a
            }
        })?;
    cells
        .iter()
        .enumerate()
        .filter(|(_, c)| (c.y - row_y).abs() < ROW_TOLERANCE)
        // `min_by` keeps the first of equal elements: the earlier card wins a tie.
        .min_by(|(_, a), (_, b)| (a.x - here.x).abs().total_cmp(&(b.x - here.x).abs()))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` cards in a grid of `cols` columns, 100 px apart.
    fn grid(n: usize, cols: usize) -> Vec<Cell> {
        (0..n)
            .map(|i| Cell {
                x: (i % cols) as f64 * 100.0,
                y: (i / cols) as f64 * 100.0,
            })
            .collect()
    }

    #[test]
    fn no_cards_no_focus() {
        for s in [Step::Next, Step::Prev, Step::Up, Step::Down] {
            assert_eq!(step(&[], None, s), None);
            assert_eq!(step(&[], Some(0), s), None);
        }
    }

    #[test]
    fn the_first_key_starts_at_the_first_card() {
        let cells = grid(5, 2);
        for s in [Step::Next, Step::Prev, Step::Up, Step::Down] {
            assert_eq!(step(&cells, None, s), Some(0));
        }
        // A stale index (the list shrank) starts over too.
        assert_eq!(step(&cells, Some(9), Step::Next), Some(0));
    }

    #[test]
    fn next_and_prev_follow_reading_order_and_stop_at_the_ends() {
        let cells = grid(5, 3);
        assert_eq!(step(&cells, Some(0), Step::Next), Some(1));
        assert_eq!(
            step(&cells, Some(2), Step::Next),
            Some(3),
            "wraps to the next row"
        );
        assert_eq!(step(&cells, Some(4), Step::Next), Some(4));
        assert_eq!(step(&cells, Some(3), Step::Prev), Some(2));
        assert_eq!(step(&cells, Some(0), Step::Prev), Some(0));
    }

    #[test]
    fn one_column_up_and_down_are_next_and_prev() {
        let cells = grid(3, 1);
        assert_eq!(step(&cells, Some(0), Step::Down), Some(1));
        assert_eq!(step(&cells, Some(1), Step::Up), Some(0));
        assert_eq!(
            step(&cells, Some(2), Step::Down),
            Some(2),
            "the last row stays"
        );
        assert_eq!(
            step(&cells, Some(0), Step::Up),
            Some(0),
            "the first row stays"
        );
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let cells = grid(9, 3);
        assert_eq!(step(&cells, Some(1), Step::Down), Some(4));
        assert_eq!(step(&cells, Some(4), Step::Down), Some(7));
        assert_eq!(step(&cells, Some(8), Step::Up), Some(5));
        let two = grid(6, 2);
        assert_eq!(step(&two, Some(1), Step::Down), Some(3));
        assert_eq!(step(&two, Some(3), Step::Up), Some(1));
    }

    #[test]
    fn a_ragged_last_row_takes_the_nearest_card() {
        // Row 0: 0 1 2; row 1: 3 4 (no third column).
        let cells = grid(5, 3);
        assert_eq!(step(&cells, Some(2), Step::Down), Some(4));
        assert_eq!(step(&cells, Some(4), Step::Up), Some(1));
    }

    #[test]
    fn a_tie_goes_to_the_earlier_card() {
        // The card below sits halfway between two columns.
        let cells = vec![
            Cell { x: 50.0, y: 0.0 },
            Cell { x: 0.0, y: 100.0 },
            Cell { x: 100.0, y: 100.0 },
        ];
        assert_eq!(step(&cells, Some(0), Step::Down), Some(1));
    }

    #[test]
    fn cards_a_few_pixels_apart_share_a_row() {
        // A tilted (stale) card's box sits a little higher.
        let cells = vec![
            Cell { x: 0.0, y: 0.0 },
            Cell { x: 100.0, y: -3.0 },
            Cell { x: 0.0, y: 100.0 },
            Cell { x: 100.0, y: 104.0 },
        ];
        assert_eq!(step(&cells, Some(1), Step::Down), Some(3));
        assert_eq!(step(&cells, Some(0), Step::Down), Some(2));
        assert_eq!(step(&cells, Some(3), Step::Up), Some(1));
    }
}
