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
//!
//! The process is DPI aware, so every rectangle here is in the monitors' own
//! pixels.  How large a widget is drawn on each monitor is `scale_of`.

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

// ---------------------------------------------------------------------------
//  Scale
// ---------------------------------------------------------------------------

/// How many times its own size a widget is drawn at on the monitor most of
/// `around` is on (or the nearest): that monitor's scale setting, rounded to
/// a whole number, so that every pixel of a skin is a square block of the
/// monitor's own -- 2 at 175%, 200% and 225%, 1 at 100% and 125%.  A
/// stretch by anything else would have to blur them.  Always 1 in a
/// process that is not DPI aware, which Windows stretches instead.
pub fn scale_of(around: &RECT) -> i32 {
    scale_of_monitor(unsafe { MonitorFromRect(around, MONITOR_DEFAULTTONEAREST) })
}

/// The same for the monitor at (or nearest) a point of the screen.
pub fn scale_at(x: i32, y: i32) -> i32 {
    scale_of_monitor(unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) })
}

/// A DPI as the whole number of times 96 nearest to it, half up, and at
/// least 1.
pub fn scale_for(dpi: u32) -> i32 {
    ((dpi as i32 + 48) / 96).max(1)
}

fn scale_of_monitor(monitor: isize) -> i32 {
    if dpi_aware() { scale_for(dpi_of(monitor)) } else { 1 }
}

/// The monitor's DPI, from its scale setting; 96 where it cannot be asked.
fn dpi_of(monitor: isize) -> u32 {
    type GetDpi = unsafe extern "system" fn(isize, i32, *mut u32, *mut u32) -> HRESULT;
    unsafe {
        let Some(get) = proc_address("shcore.dll", b"GetDpiForMonitor\0") else { return 96 };
        let get: GetDpi = core::mem::transmute(get);
        let (mut x, mut y) = (0u32, 0u32);
        if get(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) == 0 && x > 0 { x } else { 96 }
    }
}

// ---------------------------------------------------------------------------
//  Places written down before the program was DPI aware
// ---------------------------------------------------------------------------

/// A monitor's work area as a process that is not DPI aware is shown it --
/// where a build of this program from before it was aware wrote its widgets'
/// places -- and as it really is, and the scale the widgets are drawn at
/// there now.
#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub was: RECT,
    pub is: RECT,
    pub n: i32,
}

/// Every monitor there is, both ways.
pub fn monitors_both_ways() -> Vec<Monitor> {
    unsafe extern "system" fn add(monitor: isize, _dc: HDC, _rect: *mut RECT, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the Vec below, which outlives the enumeration.
        unsafe { (*(list as *mut Vec<isize>)).push(monitor) };
        1 // and the next
    }
    let mut handles: Vec<isize> = Vec::new();
    unsafe { EnumDisplayMonitors(0, null(), add, (&raw mut handles) as LPARAM) };
    handles
        .into_iter()
        .filter_map(|m| {
            let is = work_area_of(m)?;
            let was = with_dpi_context(DPI_UNAWARE, || work_area_of(m))?;
            Some(Monitor { was, is, n: scale_of_monitor(m) })
        })
        .collect()
}

/// Where frames that were placed in the coordinates an unaware process is
/// shown (`frames`, each as it was then: a skin's own size) go now, on
/// monitors that are DPI aware: the top left corner of each, and the scale
/// it is drawn at.
///
/// Each frame goes with the monitor it was on (most of it, or the nearest),
/// and stays where it was on that monitor, drawn at the monitor's scale.
/// Where that scale is not the monitor's own setting -- 2 on a monitor at
/// 225% -- the widgets come out smaller than Windows stretched them, and
/// cannot stand both where they were and as close to one another as they
/// were.  So the frames that stood together (no further apart than `near`)
/// stay together, scaled by as much as each of them is, and the group keeps
/// its distance, scaled by as much, from the nearer edge of its monitor on
/// each axis: a column down the right-hand side stays down it, and a margin
/// stays a margin.  Where the scale is the monitor's setting -- 200% -- that
/// is exactly where they were.
pub fn from_unaware(frames: &[RECT], monitors: &[Monitor], near: i32) -> Vec<((i32, i32), i32)> {
    let mut out: Vec<((i32, i32), i32)> = frames.iter().map(|f| ((f.left, f.top), 1)).collect();
    if monitors.is_empty() {
        return out;
    }
    let was: Vec<RECT> = monitors.iter().map(|m| m.was).collect();
    let home: Vec<usize> = frames.iter().map(|f| monitor_of(f, &was)).collect();
    for (m, monitor) in monitors.iter().enumerate() {
        let here: Vec<usize> = (0..frames.len()).filter(|&i| home[i] == m).collect();
        let Monitor { was, is, n } = *monitor;
        for group in groups(&here, frames, near) {
            let Some(b) = group.iter().map(|&i| frames[i]).reduce(|a, b| bounds(&a, &b)) else { continue };
            // the group's first edge along an axis, from the nearer edge of the monitor
            let from_nearer = |b0: i32, b1: i32, w0: i32, w1: i32, i0: i32, i1: i32| {
                if b0 - w0 <= w1 - b1 { i0 + (b0 - w0) * n } else { i1 - (w1 - b1) * n - (b1 - b0) * n }
            };
            let x = from_nearer(b.left, b.right, was.left, was.right, is.left, is.right);
            let y = from_nearer(b.top, b.bottom, was.top, was.bottom, is.top, is.bottom);
            for i in group {
                out[i] = ((x + (frames[i].left - b.left) * n, y + (frames[i].top - b.top) * n), n);
            }
        }
    }
    out
}

/// Which of `screens` most of `r` is on; with none of it on any, the nearest.
fn monitor_of(r: &RECT, screens: &[RECT]) -> usize {
    let shared = |s: &RECT| {
        let o = overlap(r, s);
        if is_empty(&o) { 0 } else { i64::from(o.right - o.left) * i64::from(o.bottom - o.top) }
    };
    let distance = |s: &RECT| {
        let (dx, dy) = pull(r, s);
        i64::from(dx).pow(2) + i64::from(dy).pow(2)
    };
    (0..screens.len()).max_by_key(|&k| (shared(&screens[k]), -distance(&screens[k]))).unwrap_or(0)
}

/// How far each of `wanted` has to move to be seen whole on `screens`.
///
/// One that is seen whole -- on one screen, or across several that meet --
/// stays where it is: (0, 0).  The others are lost, in part or altogether,
/// and are brought back the least distance that will do, in the groups they
/// stand in: rectangles that touch or overlap, or stand no more than `near`
/// apart, move as one, so that what was laid out together arrives laid out
/// the same.  A group goes to the nearest screen that has room for all of
/// it; with no such screen it goes to the nearest, and each of its
/// rectangles is then pushed inside that one.
///
/// With no screens at all there is nowhere to bring anything, and nothing
/// moves.
pub fn settle(wanted: &[RECT], screens: &[RECT], near: i32) -> Vec<(i32, i32)> {
    let mut moves = vec![(0, 0); wanted.len()];
    let lost: Vec<usize> = (0..wanted.len()).filter(|&i| !seen_whole(&wanted[i], screens)).collect();
    for group in groups(&lost, wanted, near) {
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
    settle(core::slice::from_ref(wanted), screens, 0)[0]
}

/// Is every part of `r` under one or another of `over` -- windows in front
/// of it, which then hide all of it?  The same question as `seen_whole`, of
/// other rectangles.
pub fn covered(r: &RECT, over: &[RECT]) -> bool {
    seen_whole(r, over)
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
/// another of its group, or is no more than `near` from it, and none of any
/// other.
fn groups(lost: &[usize], rects: &[RECT], near: i32) -> Vec<Vec<usize>> {
    let reach = |r: &RECT| RECT { left: r.left - near, top: r.top - near, right: r.right + near, bottom: r.bottom + near };
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &i in lost {
        // whatever groups this one touches are one group, with it in
        let (joined, mut apart): (Vec<_>, Vec<_>) = groups.into_iter().partition(|group| group.iter().any(|&j| meet(&reach(&rects[i]), &rects[j])));
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

    #[test]
    fn a_frame_is_covered_only_when_windows_hide_every_part_of_it() {
        let widget = rect(100, 100, 300, 250);
        // under a maximised window, or one larger than it
        assert!(covered(&widget, &[rect(0, 0, 2160, 1504)]));
        assert!(covered(&widget, &[rect(100, 100, 300, 250)]));
        // under two side by side that meet over it, or overlap there
        assert!(covered(&widget, &[rect(0, 0, 200, 1000), rect(200, 0, 900, 1000)]));
        assert!(covered(&widget, &[rect(0, 0, 220, 1000), rect(180, 0, 900, 1000)]));
        // but not with a strip of it showing between them, beside one, or
        // under nothing at all
        assert!(!covered(&widget, &[rect(0, 0, 199, 1000), rect(200, 0, 900, 1000)]));
        assert!(!covered(&widget, &[rect(0, 0, 299, 1000)]));
        assert!(!covered(&widget, &[rect(0, 0, 900, 249)]));
        assert!(!covered(&widget, &[]));
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
        wanted.iter().zip(settle(wanted, screens, 0)).map(|(r, by)| at(r, by)).collect()
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
        assert_eq!(settle(&[on_one, across_two, down_a_join, filling_one], &DESK, 0), [(0, 0); 4]);
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
        let moves = settle(&[a, b, c], &DESK, 0);
        assert!(moves[0] != (0, 0) && moves.iter().all(|&by| by == moves[0]), "{moves:?}");
        // on the one screen with room for the three of them as they stood
        assert!(settled(&[a, b, c], &DESK).iter().all(|r| within(r, &UP_LEFT)));
    }

    #[test]
    fn groups_that_stand_apart_move_apart() {
        // one just over the laptop's foot, one far off to the right
        let (near, far) = (rect(100, 500, 300, 600), rect(5000, -500, 5200, -300));
        assert_eq!(settle(&[near, far], &DESK, 0), [(0, -48), (-2853, 0)]);
        // and one that can be seen is not moved by a lost one touching it
        let (seen, lost) = (rect(0, 300, 200, 500), rect(-200, 300, 0, 500));
        assert_eq!(settle(&[seen, lost], &DESK, 0)[0], (0, 0));
    }

    #[test]
    fn what_stands_near_enough_arrives_together_too() {
        // two of the L, ten apart: alone, each to its own screen; as near as
        // that, both to one
        let (a, b) = (rect(-960, 423, -556, 812), rect(-546, 423, -342, 667));
        let apart = settle(&[a, b], &DESK, 0);
        assert_ne!(apart[0], apart[1]);
        let together = settle(&[a, b], &DESK, 10);
        assert!(together[0] != (0, 0) && together[0] == together[1], "{together:?}");
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
            assert_eq!(settle(&found, screens, 0), [(0, 0); 4], "{screens:?}");
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
    fn a_scale_is_a_whole_number() {
        let scales: Vec<i32> = [0, 96, 120, 144, 168, 192, 216, 240, 288].map(scale_for).into();
        assert_eq!(scales, [1, 1, 1, 2, 2, 2, 2, 3, 3]);
    }

    // A laptop at 200% to the left of a 4K monitor at 225%, and a second 4K
    // monitor at 225% standing on end to the right of that: work areas as a
    // process that is not DPI aware is shown them, and as they are.
    const LAPTOP_THEN_AND_NOW: Monitor = Monitor { was: rect(-960, 423, 0, 1023), is: rect(-1920, 846, 0, 2046), n: 2 };
    const MAIN_THEN_AND_NOW: Monitor = Monitor { was: rect(0, 0, 1707, 912), is: rect(0, 0, 3840, 2052), n: 2 };
    const TALL_THEN_AND_NOW: Monitor = Monitor { was: rect(1920, -259, 2880, 1448), is: rect(3840, -517, 6000, 3323), n: 2 };
    const THREE: [Monitor; 3] = [LAPTOP_THEN_AND_NOW, MAIN_THEN_AND_NOW, TALL_THEN_AND_NOW];

    #[test]
    fn at_a_scale_of_its_own_setting_a_place_is_where_it_was() {
        // on the laptop, Windows stretched by exactly 2: wherever they were,
        // in the left half or the right, together or apart
        let frames = [rect(-948, 435, -544, 824), rect(-532, 435, -342, 625), rect(-948, 836, -758, 1011), rect(-200, 800, -12, 1011)];
        let (was, is) = (LAPTOP_THEN_AND_NOW.was, LAPTOP_THEN_AND_NOW.is);
        let exactly: Vec<((i32, i32), i32)> = frames.iter().map(|f| ((is.left + (f.left - was.left) * 2, is.top + (f.top - was.top) * 2), 2)).collect();
        assert_eq!(from_unaware(&frames, &THREE, 34), exactly);
    }

    #[test]
    fn a_column_down_the_right_stays_down_it_on_a_grid_twice_the_size() {
        // at 225% drawn at 2: a column 12 in from the right-hand edge, 24
        // apart, and one on its own near the top left
        let (top, under, alone) = (rect(1291, 12, 1695, 401), rect(1291, 425, 1695, 539), rect(100, 50, 290, 240));
        let found = from_unaware(&[top, under, alone], &THREE, 34);
        let size = |f: &RECT| (f.right - f.left, f.bottom - f.top);
        let right = |k: usize, f: &RECT| found[k].0.0 + size(f).0 * 2;
        assert_eq!((right(0, &top), found[0].0.1), (3840 - 24, 24));
        assert_eq!((right(1, &under), found[1].0.1 - (found[0].0.1 + size(&top).1 * 2)), (3840 - 24, 48));
        assert_eq!(found[2], ((200, 100), 2));
    }

    #[test]
    fn a_place_on_no_monitor_goes_by_the_nearest() {
        // above the monitor standing on end
        let found = from_unaware(&[rect(2121, -852, 2311, -632)], &THREE, 34);
        assert_eq!(found, [((3840 + 201 * 2, -517 - 593 * 2), 2)]);
        // and with no monitors at all, nothing moves
        assert_eq!(from_unaware(&[rect(5, 6, 7, 8)], &[], 34), [((5, 6), 1)]);
    }

    #[test]
    fn windows_is_asked_for_every_monitor_both_ways() {
        let monitors = monitors_both_ways();
        assert!(!monitors.is_empty());
        assert!(monitors.iter().all(|m| !is_empty(&m.was) && !is_empty(&m.is) && m.n >= 1), "{monitors:?}");
    }

    #[test]
    fn with_no_screen_nothing_moves() {
        assert_eq!(settle(&[rect(-600, 200, -400, 400), rect(0, 0, 10, 10)], &[], 0), [(0, 0); 2]);
        assert!(settle(&[], &DESK, 0).is_empty());
    }
}
