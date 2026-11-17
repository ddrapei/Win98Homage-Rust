//! The screens, and keeping what is shown on them.
//!
//! The desktop is not a rectangle.  Each monitor is one, but the smallest
//! rectangle round them all -- the "virtual screen" -- has corners that no
//! monitor shows as soon as two of them differ in size, or stand one above
//! the other and off to a side.  A window kept only inside that rectangle can
//! be left in such a corner: running, and nowhere to be seen.
//!
//! So a window is kept on the monitors themselves -- on their work areas,
//! which leave out the taskbar -- and here is what that takes.  Two functions
//! ask Windows what the work areas are; the rest is arithmetic on plain
//! rectangles, which is what the tests at the foot give it.
//!
//! Nothing here remembers anything.  `settle` is given where the windows
//! were put and answers how far each has to move for the screens there are
//! now; asked again with the same, it answers the same.  So whoever calls it
//! keeps where a window was *put* apart from where it *is*, and loses
//! nothing to a monitor that is unplugged for an hour.

use core::ptr::null;

use crate::sys::*;

/// The work area of every monitor there is.
pub fn work_areas() -> Vec<RECT> {
    unsafe extern "system" fn add(monitor: isize, _dc: HDC, _rect: *mut RECT, areas: LPARAM) -> BOOL {
        // SAFETY: `areas` is the Vec below, which outlives the enumeration.
        let areas = unsafe { &mut *(areas as *mut Vec<RECT>) };
        areas.extend(work_area_of(monitor));
        1 // and the next
    }
    let mut areas: Vec<RECT> = Vec::new();
    unsafe { EnumDisplayMonitors(0, null(), add, (&raw mut areas) as LPARAM) };
    areas
}

/// The work area of the monitor nearest to `around`.
pub fn work_area(around: &RECT) -> RECT {
    work_area_of(unsafe { MonitorFromRect(around, MONITOR_DEFAULTTONEAREST) }).unwrap_or_default()
}

fn work_area_of(monitor: isize) -> Option<RECT> {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, rcMonitor: RECT::default(), rcWork: RECT::default(), dwFlags: 0 };
    let known = unsafe { GetMonitorInfoW(monitor, &mut info) } != 0;
    (known && !is_empty(&info.rcWork)).then_some(info.rcWork)
}

/// How far each of `wanted` has to move to be seen whole on `screens`.
///
/// One that is seen whole -- on one screen, or across several that meet --
/// stays where it is: (0, 0).  The others are lost, in part or altogether,
/// and are brought back the least distance that will do, in the groups they
/// stand in: rectangles that touch or overlap move as one, so that what was
/// laid out together arrives laid out the same.  A group goes to the nearest
/// screen that has room for all of it; with no such screen it goes to the
/// nearest, and each of its rectangles is then pushed inside that one.
///
/// With no screens at all there is nowhere to bring anything, and nothing
/// moves.
pub fn settle(wanted: &[RECT], screens: &[RECT]) -> Vec<(i32, i32)> {
    let mut moves = vec![(0, 0); wanted.len()];
    let lost: Vec<usize> = (0..wanted.len()).filter(|&i| !seen_whole(&wanted[i], screens)).collect();
    for group in groups(&lost, wanted) {
        let Some(around) = group.iter().map(|&i| wanted[i]).reduce(|a, b| bounds(&a, &b)) else { continue };
        let Some((screen, (dx, dy))) = nearest(&around, screens) else { break };
        for i in group {
            let moved = RECT { left: wanted[i].left + dx, top: wanted[i].top + dy, right: wanted[i].right + dx, bottom: wanted[i].bottom + dy };
            let (in_x, in_y) = pull(&moved, screen);
            moves[i] = (dx + in_x, dy + in_y);
        }
    }
    moves
}

/// How far the one rectangle `wanted` has to move to be seen whole.
pub fn keep_on(wanted: &RECT, screens: &[RECT]) -> (i32, i32) {
    settle(core::slice::from_ref(wanted), screens)[0]
}

/// Is every part of `r` on one screen or another?
fn seen_whole(r: &RECT, screens: &[RECT]) -> bool {
    let mut unseen = vec![*r];
    for screen in screens {
        unseen = unseen.iter().flat_map(|part| outside(part, screen)).collect();
    }
    unseen.is_empty()
}

/// The parts of `r` that are not on `screen`: a strip above it and one below
/// it, as wide as `r`, and what is left to either side between those.
fn outside(r: &RECT, screen: &RECT) -> Vec<RECT> {
    if !meet(r, screen) || is_empty(&overlap(r, screen)) {
        return vec![*r];
    }
    let (top, bottom) = (r.top.max(screen.top), r.bottom.min(screen.bottom));
    let parts = [
        RECT { bottom: screen.top, ..*r },
        RECT { top: screen.bottom, ..*r },
        RECT { top, bottom, right: screen.left, ..*r },
        RECT { top, bottom, left: screen.right, ..*r },
    ];
    parts.into_iter().filter(|part| !is_empty(part)).collect()
}

/// `lost`, as the groups its rectangles stand in: each touches or overlaps
/// another of its group, and none of any other.
fn groups(lost: &[usize], rects: &[RECT]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &i in lost {
        // whatever groups this one touches are one group, with it in
        let (joined, mut apart): (Vec<_>, Vec<_>) = groups.into_iter().partition(|group| group.iter().any(|&j| meet(&rects[i], &rects[j])));
        apart.push(joined.into_iter().flatten().chain([i]).collect());
        groups = apart;
    }
    groups
}

/// The screen `r` is brought onto, and how far it moves to get there: the
/// least distance, but a screen `r` fits on before one it does not.
fn nearest<'a>(r: &RECT, screens: &'a [RECT]) -> Option<(&'a RECT, (i32, i32))> {
    let fits = |screen: &RECT| r.right - r.left <= screen.right - screen.left && r.bottom - r.top <= screen.bottom - screen.top;
    screens
        .iter()
        .map(|screen| (screen, pull(r, screen)))
        .min_by_key(|&(screen, (dx, dy))| (!fits(screen), i64::from(dx).pow(2) + i64::from(dy).pow(2)))
}

/// The least that moves `r` inside `screen`.  One too large for it is left
/// with its top left corner showing.
fn pull(r: &RECT, screen: &RECT) -> (i32, i32) {
    let along = |from: i32, to: i32, low: i32, high: i32| from.clamp(low, (high - (to - from)).max(low)) - from;
    (along(r.left, r.right, screen.left, screen.right), along(r.top, r.bottom, screen.top, screen.bottom))
}

/// Do the two touch or overlap?
fn meet(a: &RECT, b: &RECT) -> bool {
    a.left <= b.right && b.left <= a.right && a.top <= b.bottom && b.top <= a.bottom
}

fn overlap(a: &RECT, b: &RECT) -> RECT {
    RECT { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) }
}

/// The smallest rectangle round both.
fn bounds(a: &RECT, b: &RECT) -> RECT {
    RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

fn is_empty(r: &RECT) -> bool {
    r.left >= r.right || r.top >= r.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    /// A laptop with two monitors above it, the left one reaching out past
    /// the laptop's left edge: the arrangement that lost the widgets.  Under
    /// the left monitor and beside the laptop there is no screen.
    const LAPTOP: RECT = rect(0, 0, 960, 552);
    const UP_LEFT: RECT = rect(-725, -864, 811, 0);
    const UP_RIGHT: RECT = rect(811, -864, 2347, 0);
    const DESK: [RECT; 3] = [LAPTOP, UP_LEFT, UP_RIGHT];

    fn at(r: &RECT, by: (i32, i32)) -> RECT {
        rect(r.left + by.0, r.top + by.1, r.right + by.0, r.bottom + by.1)
    }

    fn settled(wanted: &[RECT], screens: &[RECT]) -> Vec<RECT> {
        wanted.iter().zip(settle(wanted, screens)).map(|(r, by)| at(r, by)).collect()
    }

    fn within(r: &RECT, screen: &RECT) -> bool {
        r.left >= screen.left && r.top >= screen.top && r.right <= screen.right && r.bottom <= screen.bottom
    }

    #[test]
    fn what_can_be_seen_stays_where_it_is() {
        let on_one = rect(100, 100, 300, 300);
        let across_two = rect(700, -100, 900, -20); // UP_LEFT into UP_RIGHT
        let down_a_join = rect(100, -100, 300, 100); // UP_LEFT into LAPTOP
        let filling_one = LAPTOP;
        assert_eq!(settle(&[on_one, across_two, down_a_join, filling_one], &DESK), [(0, 0); 4]);
    }

    #[test]
    fn the_corner_no_screen_shows_is_not_a_place() {
        // inside the rectangle round all three screens, and on none of them
        let lost = rect(-600, 200, -400, 400);
        let found = settled(&[lost], &DESK)[0];
        assert!(DESK.iter().any(|screen| within(&found, screen)), "{found:?}");
        // and one that only reaches into it is as much to be brought back
        let half_seen = rect(-100, 200, 100, 400);
        assert_eq!(keep_on(&half_seen, &DESK), (100, 0));
    }

    #[test]
    fn a_lost_one_moves_the_least_that_will_do() {
        // off the foot of the laptop: up, and no further than onto it
        assert_eq!(keep_on(&rect(100, 500, 300, 700), &DESK), (0, -148));
        // off the far right: back by what hangs over, not across the desk
        assert_eq!(keep_on(&rect(2300, -500, 2500, -300), &DESK), (-153, 0));
        // nearer the laptop than the monitor above: onto the laptop
        assert_eq!(keep_on(&rect(-150, 300, -50, 400), &DESK), (150, 0));
    }

    #[test]
    fn what_was_laid_out_together_arrives_together() {
        // three in an L, touching, where a monitor to the left used to be
        let (a, b, c) = (rect(-960, 423, -556, 812), rect(-556, 423, -342, 667), rect(-960, 812, -746, 1026));
        let moves = settle(&[a, b, c], &DESK);
        assert!(moves[0] != (0, 0) && moves.iter().all(|&by| by == moves[0]), "{moves:?}");
        // on the one screen with room for the three of them as they stood
        assert!(settled(&[a, b, c], &DESK).iter().all(|r| within(r, &UP_LEFT)));
    }

    #[test]
    fn groups_that_stand_apart_move_apart() {
        // one just over the laptop's foot, one far off to the right
        let (near, far) = (rect(100, 500, 300, 600), rect(5000, -500, 5200, -300));
        assert_eq!(settle(&[near, far], &DESK), [(0, -48), (-2853, 0)]);
        // and one that can be seen is not moved by a lost one touching it
        let (seen, lost) = (rect(0, 300, 200, 500), rect(-200, 300, 0, 500));
        assert_eq!(settle(&[seen, lost], &DESK)[0], (0, 0));
    }

    #[test]
    fn a_group_too_large_for_any_screen_is_still_all_on_one() {
        let small = [rect(0, 0, 500, 400)];
        let spread = [rect(-900, 0, -700, 200), rect(-700, 0, -500, 200), rect(-500, 0, -300, 200), rect(-900, 200, -700, 500)];
        let found = settled(&spread, &small);
        assert!(found.iter().all(|r| within(r, &small[0])), "{found:?}");
        // as nearly as they stood as there is room for: the first two keep
        // their places by each other
        assert_eq!((found[1].left - found[0].left, found[1].top - found[0].top), (200, 0));
    }

    #[test]
    fn one_too_large_for_the_screen_shows_its_top_left() {
        let small = [rect(100, 100, 300, 300)];
        let found = settled(&[rect(-900, -900, -400, -400)], &small);
        assert_eq!((found[0].left, found[0].top), (100, 100));
    }

    #[test]
    fn settling_twice_is_settling_once() {
        let wanted = [rect(-960, 423, -556, 812), rect(-556, 423, -342, 667), rect(3000, 3000, 3100, 3100), rect(10, 10, 60, 60)];
        for screens in [&DESK[..], &[LAPTOP], &[rect(0, 0, 300, 300)]] {
            let found = settled(&wanted, screens);
            assert_eq!(settle(&found, screens), [(0, 0); 4], "{screens:?}");
        }
    }

    #[test]
    fn windows_is_asked_for_every_monitor() {
        // whatever monitors this machine has, the main one is among them
        let areas = work_areas();
        let main = work_area(&rect(0, 0, 1, 1));
        assert!(!is_empty(&main) && areas.contains(&main), "{main:?} not in {areas:?}");
        assert!(areas.iter().all(|area| !is_empty(area)));
    }

    #[test]
    fn with_no_screen_nothing_moves() {
        assert_eq!(settle(&[rect(-600, 200, -400, 400), rect(0, 0, 10, 10)], &[]), [(0, 0); 2]);
        assert!(settle(&[], &DESK).is_empty());
    }
}
