//! The grid the widgets stand on: a margin of bare screen inside the edges
//! of a monitor, and a gap of it between one widget and the next.
//!
//! Both are measured between frames -- what is seen of a widget -- and not
//! between windows, which have a margin of nothing round the frame.  Two
//! things are asked of them: where a frame that is being dragged is pulled
//! to (`snap`), and where frames laid out on one grid go when the margin or
//! the gap changes (`respace`).
//!
//! Like `screens`, this is arithmetic on plain rectangles, which is what the
//! tests at the foot give it, and it remembers nothing.

use crate::sys::RECT;

/// A frame is pulled onto the grid from this far away.
pub const SNAP: i32 = 10;

/// The margin and the gap, in px of bare screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spacing {
    /// between a frame and the edge of the work area of its monitor
    pub margin: i32,
    /// between the frames of two widgets
    pub gap: i32,
}

/// How far the frame `f`, dragged to where it is, is pulled along each
/// axis: onto the margin inside an edge of `work`, onto the gap beside a
/// frame in `others`, or into line with one -- whichever is nearest, if it
/// is less than SNAP away.  With nothing in reach, (0, 0).
pub fn snap(f: &RECT, others: &[RECT], work: &RECT, sp: Spacing) -> (i32, i32) {
    // Another frame is reached for only from within a gap and a snap of it:
    // beside it, or above or below it, and not across the screen.
    let reach = sp.gap + SNAP;
    let mut xs = vec![(f.left, work.left + sp.margin), (f.right, work.right - sp.margin)];
    let mut ys = vec![(f.top, work.top + sp.margin), (f.bottom, work.bottom - sp.margin)];
    for o in others {
        if f.top < o.bottom + reach && o.top < f.bottom + reach {
            xs.extend([(f.left, o.left), (f.left, o.right + sp.gap), (f.right, o.left - sp.gap), (f.right, o.right)]);
        }
        if f.left < o.right + reach && o.left < f.right + reach {
            ys.extend([(f.top, o.top), (f.top, o.bottom + sp.gap), (f.bottom, o.top - sp.gap), (f.bottom, o.bottom)]);
        }
    }
    (pull(&xs), pull(&ys))
}

/// Of the moves that take an edge onto a line, (edge, line), the shortest
/// that is less than SNAP; or none.
fn pull(edges_to_lines: &[(i32, i32)]) -> i32 {
    edges_to_lines.iter().map(|&(edge, line)| line - edge).filter(|d| d.abs() < SNAP).min_by_key(|d| d.abs()).unwrap_or(0)
}

/// How far each of `frames` moves when the spacing goes from `old` to `new`.
///
/// A frame that stands on the old grid moves with it.  It stands on it
/// against an edge of its screen when it is no further from it than the
/// margin and a snap; beside another frame when it is no further from that
/// than the gap and a snap; and in line with another when one stands above
/// the other that close and an edge of the one is even with the same edge
/// of the other.  That is where snapping leaves a frame, and the first
/// layout, and a hand that put it near enough to have been snapped.
///
/// Of the margin and the gap, the one that changed is made the new figure
/// wherever it is found, and the other is left as it was.  So a frame
/// against an edge stays against it, those beside it follow on from it one
/// by one, and those in line with it keep in line.  A frame tied more than
/// one way is placed by the edge of its screen first, then by the tie
/// nearest to one, and by a gap before a line.  Frames tied to no edge are
/// placed from the first of them from the left (or the top), which stays
/// where it is; one tied to nothing at all stays where it is.
pub fn respace(frames: &[RECT], screens: &[RECT], old: Spacing, new: Spacing) -> Vec<(i32, i32)> {
    fn x(r: &RECT) -> (i32, i32) {
        (r.left, r.right)
    }
    fn y(r: &RECT) -> (i32, i32) {
        (r.top, r.bottom)
    }
    let on: Vec<Option<&RECT>> = frames.iter().map(|f| screen_of(f, screens)).collect();
    let spans = |along: fn(&RECT) -> (i32, i32), across: fn(&RECT) -> (i32, i32)| -> Vec<Span> {
        frames.iter().zip(&on).map(|(f, screen)| Span { at: along(f), across: across(f), screen: screen.map(along) }).collect()
    };
    let dx = respace_along(&spans(x, y), old, new);
    let dy = respace_along(&spans(y, x), old, new);
    dx.into_iter().zip(dy).collect()
}

/// A frame as one axis sees it: where it starts and ends along the axis,
/// the same across it, and where its screen starts and ends along it.
struct Span {
    at: (i32, i32),
    across: (i32, i32),
    screen: Option<(i32, i32)>,
}

impl Span {
    fn len(&self) -> i32 {
        self.at.1 - self.at.0
    }
}

/// How a frame is placed by another one along an axis.
#[derive(Clone, Copy)]
enum Tie {
    /// it starts this far after the other ends
    After(i32),
    /// it ends this far before the other starts
    Before(i32),
    /// the two start in line
    Starts,
    /// the two end in line
    Ends,
}

/// `respace` along one axis: how far each span moves.
fn respace_along(spans: &[Span], old: Spacing, new: Spacing) -> Vec<i32> {
    // the figure that changed is the new one; the other is as it was found
    let margin = |d: i32| if new.margin != old.margin { new.margin } else { d };
    let gap = |d: i32| if new.gap != old.gap { new.gap } else { d };
    let near = |d: i32, was: i32| (0..was + SNAP).contains(&d);

    // Where each starts now: first, those against an edge of their screen.
    let mut at: Vec<Option<i32>> = spans
        .iter()
        .map(|s| {
            let (low, high) = s.screen?;
            if near(s.at.0 - low, old.margin) {
                Some(low + margin(s.at.0 - low))
            } else if near(high - s.at.1, old.margin) {
                Some(high - margin(high - s.at.1) - s.len())
            } else {
                None
            }
        })
        .collect();

    // Then, round by round, those tied to one placed in the round before.
    loop {
        let placed = at.clone();
        let mut more = false;
        for (i, s) in spans.iter().enumerate() {
            if placed[i].is_some() {
                continue;
            }
            let ties = spans.iter().enumerate().filter_map(|(j, o)| Some((o, placed[j]?, tie(s, o, old.gap)?)));
            let Some((o, from, tie)) = ties.min_by_key(|t| matches!(t.2, Tie::Starts | Tie::Ends)) else { continue };
            at[i] = Some(match tie {
                Tie::After(d) => from + o.len() + gap(d),
                Tie::Before(d) => from - gap(d) - s.len(),
                Tie::Starts => from,
                Tie::Ends => from + o.len() - s.len(),
            });
            more = true;
        }
        if !more {
            // What is left is tied to nothing placed: the first of it stays.
            let left = (0..spans.len()).filter(|&i| at[i].is_none());
            let Some(i) = left.min_by_key(|&i| spans[i].at.0) else { break };
            at[i] = Some(spans[i].at.0);
        }
    }
    spans.iter().zip(at).map(|(s, a)| a.map_or(0, |a| a - s.at.0)).collect()
}

/// How `s` is placed by `o` on a grid with a gap of `gap`, if it is.
fn tie(s: &Span, o: &Span, gap: i32) -> Option<Tie> {
    let near = |d: i32| (0..gap + SNAP).contains(&d);
    // side by side: the two overlap across the axis
    let beside = s.across.0 < o.across.1 && o.across.0 < s.across.1;
    if beside && near(s.at.0 - o.at.1) {
        return Some(Tie::After(s.at.0 - o.at.1));
    }
    if beside && near(o.at.0 - s.at.1) {
        return Some(Tie::Before(o.at.0 - s.at.1));
    }
    // one above the other, as near as that
    let stacked = near(s.across.0 - o.across.1) || near(o.across.0 - s.across.1);
    if stacked && s.at.0 == o.at.0 {
        return Some(Tie::Starts);
    }
    if stacked && s.at.1 == o.at.1 {
        return Some(Tie::Ends);
    }
    None
}

/// The screen most of `f` is on, if any of it is on one.
fn screen_of<'a>(f: &RECT, screens: &'a [RECT]) -> Option<&'a RECT> {
    let shared = |s: &&RECT| {
        let (w, h) = (f.right.min(s.right) - f.left.max(s.left), f.bottom.min(s.bottom) - f.top.max(s.top));
        if w > 0 && h > 0 { i64::from(w) * i64::from(h) } else { 0 }
    };
    screens.iter().filter(|s| shared(s) > 0).max_by_key(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    fn at(r: &RECT, by: (i32, i32)) -> RECT {
        rect(r.left + by.0, r.top + by.1, r.right + by.0, r.bottom + by.1)
    }

    fn respaced(frames: &[RECT], old: Spacing, new: Spacing) -> Vec<RECT> {
        frames.iter().zip(respace(frames, &[SCREEN], old, new)).map(|(f, by)| at(f, by)).collect()
    }

    const SCREEN: RECT = rect(0, 0, 1600, 900);
    const WAS: Spacing = Spacing { margin: 12, gap: 24 };

    /// A frame w x h with its top right corner at (right, top).
    fn from_right(right: i32, top: i32, w: i32, h: i32) -> RECT {
        rect(right - w, top, right, top + h)
    }

    #[test]
    fn a_frame_is_pulled_onto_the_margin_and_the_gap() {
        let sp = Spacing { margin: 8, gap: 6 };
        // near the top right corner: onto the margin inside both edges
        let f = rect(1400, 5, 1595, 200);
        assert_eq!(snap(&f, &[], &SCREEN, sp), (-3, 3));
        // near another's right side: the gap from it, its top in line
        let other = rect(100, 100, 300, 300);
        assert_eq!(snap(&rect(310, 104, 500, 280), &[other], &SCREEN, sp), (-4, -4));
        // a little under it: the gap below it, its left in line
        assert_eq!(snap(&rect(103, 302, 290, 400), &[other], &SCREEN, sp), (-3, 4));
    }

    #[test]
    fn nothing_in_reach_is_no_pull() {
        let sp = Spacing { margin: 8, gap: 6 };
        let other = rect(100, 100, 300, 300);
        assert_eq!(snap(&rect(700, 400, 900, 600), &[other], &SCREEN, sp), (0, 0));
        // in line with another, but far below it: not lined up with it
        assert_eq!(snap(&rect(103, 600, 300, 700), &[other], &SCREEN, sp), (0, 0));
    }

    #[test]
    fn the_nearest_line_wins() {
        let sp = Spacing { margin: 0, gap: 0 };
        // 4 from the screen's left, 3 from the other's right
        let other = rect(0, 0, 7, 50);
        assert_eq!(snap(&rect(4, 10, 100, 40), &[other], &SCREEN, sp).0, 3);
    }

    #[test]
    fn a_column_on_the_right_closes_up_with_the_gap() {
        // three frames down the right-hand side, 12 in from it, 24 apart
        let column = [from_right(1588, 12, 380, 365), from_right(1588, 401, 380, 114), from_right(1588, 539, 380, 100)];
        let found = respaced(&column, WAS, Spacing { margin: 12, gap: 8 });
        assert_eq!(found[0], column[0]);
        assert_eq!((found[1].top - found[0].bottom, found[2].top - found[1].bottom), (8, 8));
        assert!(found.iter().all(|f| f.right == 1588));
    }

    #[test]
    fn a_new_margin_moves_what_is_against_the_edge_and_its_neighbours() {
        // a row against the right-hand edge, and its left-hand neighbour
        let (right, left) = (from_right(1588, 600, 190, 190), from_right(1588 - 190 - 24, 600, 190, 190));
        let found = respaced(&[right, left], WAS, Spacing { margin: 30, gap: 24 });
        assert_eq!(found[0].right, 1570);
        assert_eq!(found[0].left - found[1].right, 24);
    }

    #[test]
    fn what_changed_is_set_and_what_did_not_is_kept() {
        // 16 from the edge, which is near enough the margin of 12 to be on it
        let a = from_right(1584, 16, 190, 190);
        let b = from_right(1584 - 190 - 30, 16, 190, 190);
        // the gap changes: the margin of 16 is kept, the gap of 30 becomes 4
        let found = respaced(&[a, b], WAS, Spacing { margin: 12, gap: 4 });
        assert_eq!((found[0].right, found[0].top), (1584, 16));
        assert_eq!(found[0].left - found[1].right, 4);
    }

    #[test]
    fn a_frame_placed_apart_stays_put() {
        let alone = rect(500, 300, 690, 490);
        let column = [from_right(1588, 12, 380, 365), from_right(1588, 401, 380, 114)];
        let found = respaced(&[column[0], column[1], alone], WAS, Spacing { margin: 0, gap: 0 });
        assert_eq!(found[2], alone);
        assert_eq!((found[0].top, found[1].top, found[0].right), (0, 365, 1600));
    }

    #[test]
    fn a_pair_on_its_own_closes_up_on_the_first() {
        let (a, b) = (rect(400, 300, 590, 490), rect(614, 300, 804, 490));
        let found = respaced(&[b, a], WAS, Spacing { margin: 12, gap: 2 });
        assert_eq!((found[1], found[0].left), (a, 592));
    }

    #[test]
    fn those_in_line_keep_in_line() {
        // a wide one against the right; a narrow one 24 to its left; under the
        // narrow one and lined up with its left edge, another
        let wide = from_right(1588, 12, 380, 200);
        let narrow = from_right(wide.left - 24, 12, 190, 100);
        let under = rect(narrow.left, narrow.bottom + 24, narrow.left + 100, narrow.bottom + 124);
        let found = respaced(&[wide, narrow, under], WAS, Spacing { margin: 12, gap: 10 });
        assert_eq!(found[1].right, wide.left - 10);
        assert_eq!(found[2].left, found[1].left);
        assert_eq!(found[2].top - found[1].bottom, 10);
    }

    #[test]
    fn a_gap_comes_before_a_line() {
        // the note and the weather side by side under a wide one, the note in
        // line with its left edge and touching the weather (as the first
        // layout had them)
        let wide = from_right(1588, 12, 380, 100);
        let weather = from_right(1588, 136, 190, 190);
        let note = from_right(weather.left, 136, 190, 190);
        assert_eq!(note.left, wide.left);
        let found = respaced(&[wide, weather, note], WAS, Spacing { margin: 12, gap: 6 });
        assert_eq!(found[1].left - found[2].right, 6);
        assert_eq!(found[2].top, found[1].top);
    }

    #[test]
    fn respacing_there_and_back_is_where_it_started() {
        let layout = [
            from_right(1588, 12, 380, 365),
            from_right(1588, 401, 380, 114),
            from_right(1588, 539, 190, 190),
            from_right(1588 - 190 - 24, 539, 190, 220),
            from_right(1588 - 380 - 24, 12, 380, 166),
        ];
        let there = Spacing { margin: 4, gap: 2 };
        let found = respaced(&respaced(&layout, WAS, there), there, WAS);
        assert_eq!(found, layout);
        // and the same spacing moves nothing
        assert_eq!(respace(&layout, &[SCREEN], WAS, WAS), [(0, 0); 5]);
    }

    #[test]
    fn a_frame_on_no_screen_is_placed_by_its_neighbours_only() {
        let off = rect(-500, 12, -310, 202);
        let beside = rect(-286, 12, -96, 202);
        let found = respaced(&[off, beside], WAS, Spacing { margin: 0, gap: 4 });
        assert_eq!((found[0], found[1].left), (off, -306));
    }
}
