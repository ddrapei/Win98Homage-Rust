//! Flip Clock: the time on cards that flip, as a bedside clock of the
//! seventies kept it.  There is a card for each figure, split across the
//! middle into two leaves on a hinge.  On the minute the top leaf of each
//! card that changes falls over, forwards and down, and the next figure is
//! there: its top half on the leaf that was behind, its bottom half on the
//! back of the one that fell.
//!
//! All of it is pixel art in the skin's own colours, as the rain is.  The
//! figures are a font of their own, set a pixel at a time, and nothing is
//! blended:
//!
//! - a falling leaf is foreshortened by leaving rows out of it, never by
//!   squeezing them together, so every row it shows is one of its own -- its
//!   two edge rows always among them;
//! - a leaf turned from the light is drawn a step or two down the skin's
//!   ramp of card colours, and so is the shadow it throws on the leaf below;
//! - near edge-on, its free edge is nearer, so it stands a pixel or two wider
//!   than the card -- its paper wider, not its figure, which no pose ever
//!   stretches.
//!
//! The fall is a handful of poses, each held as long as it would be seen
//! (`POSES`): slow off the top, quicker and quicker down, a small bounce, and
//! one pose edge-on, which is the one that makes it a flip.  When the minute
//! carries, each card to the left goes a moment after the one to its right,
//! as on the mechanism.
//!
//! Between the flips it asks for no frames, and costs nothing.  The hours are
//! on the 12 or the 24 hour clock as the Date/Time widget is.

use core::ops::Range;

use crate::canvas::{Canvas, Rect};
use crate::clock::{self, Tm, MONTHS};
use crate::skin::{self, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, Outcome, Request, Widget};

static SKIN: &Skin = &skin::flip::SKIN;

/// Milliseconds between frames while a card turns: each pose is held for 30
/// or 40 of them, so each is on the screen for a frame at least.
const FRAME_MS: u32 = 20;
/// When the minute carries, how long after a card starts to fall the one to
/// its left does.
const CASCADE_MS: i32 = 80;

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// A figure is 18 px wide and 30 tall: the top 15 rows on the top leaf and
/// the bottom 15 on the bottom one.  The seam between the leaves falls
/// between two of its rows, and hides none.
const GLYPH_W: usize = 18;
const GLYPH_H: i32 = 30;

/// The figures, top row first, '#' for ink.  Four-pixel strokes, corners
/// rounded on the outside by a step of two and then one, and filled by a
/// pixel on the inside, so that a stroke is as thick round a corner as along
/// a side.  The 8 is nipped in at the seam; the 2 and the 7 are the only
/// slants, at even steps -- one and one, and three rows to the pixel.
const DIGITS: [[&str; 30]; 10] = [
    [
        "...############...",
        ".################.",
        ".################.",
        "##################",
        "#####........#####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "#####........#####",
        "##################",
        ".################.",
        ".################.",
        "...############...",
    ],
    [
        ".......#####......",
        ".....#######......",
        "...#########......",
        "...###..####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
        "........####......",
    ],
    [
        "...############...",
        ".################.",
        ".################.",
        "##################",
        "####.........#####",
        "####..........####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        ".............#####",
        "............######",
        "...........######.",
        "..........######..",
        ".........######...",
        "........######....",
        ".......######.....",
        "......######......",
        ".....######.......",
        "....######........",
        "...######.........",
        "..######..........",
        ".######...........",
        "######............",
        "##################",
        "##################",
        "##################",
        "##################",
    ],
    [
        "...############...",
        ".################.",
        ".################.",
        "##################",
        "####.........#####",
        "####..........####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        ".............#####",
        "......###########.",
        "......##########..",
        "......##########..",
        "......###########.",
        ".............#####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "####..........####",
        "####.........#####",
        "##################",
        ".################.",
        ".################.",
        "...############...",
    ],
    [
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "####.......####...",
        "##################",
        "##################",
        "##################",
        "##################",
        "...........####...",
        "...........####...",
        "...........####...",
        "...........####...",
        "...........####...",
        "...........####...",
        "...........####...",
        "...........####...",
        "...........####...",
    ],
    [
        "##################",
        "##################",
        "##################",
        "##################",
        "####..............",
        "####..............",
        "####..............",
        "####..............",
        "####..............",
        "####..............",
        "####..............",
        "###############...",
        "#################.",
        "#################.",
        "##################",
        ".............#####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "####..........####",
        "####.........#####",
        "##################",
        ".################.",
        ".################.",
        "...############...",
    ],
    [
        "...############...",
        ".################.",
        ".################.",
        "##################",
        "#####.........####",
        "####..........####",
        "####..............",
        "####..............",
        "####..............",
        "####..............",
        "####..............",
        "###############...",
        "#################.",
        "#################.",
        "##################",
        "#####........#####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "#####........#####",
        "##################",
        ".################.",
        ".################.",
        "...############...",
    ],
    [
        "##################",
        "##################",
        "##################",
        "##################",
        ".............#####",
        ".............#####",
        ".............#####",
        "............#####.",
        "............#####.",
        "............#####.",
        "...........#####..",
        "...........#####..",
        "...........#####..",
        "..........#####...",
        "..........#####...",
        "..........#####...",
        ".........#####....",
        ".........#####....",
        ".........#####....",
        "........#####.....",
        "........#####.....",
        "........#####.....",
        ".......#####......",
        ".......#####......",
        ".......#####......",
        "......#####.......",
        "......#####.......",
        "......#####.......",
        ".....#####........",
        ".....#####........",
    ],
    [
        "...############...",
        ".################.",
        ".################.",
        "##################",
        "#####........#####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "#####........#####",
        ".################.",
        "..##############..",
        "..##############..",
        ".################.",
        "#####........#####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "#####........#####",
        "##################",
        ".################.",
        ".################.",
        "...############...",
    ],
    [
        "...############...",
        ".################.",
        ".################.",
        "##################",
        "#####........#####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "####..........####",
        "#####........#####",
        "##################",
        ".#################",
        ".#################",
        "...###############",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "..............####",
        "####..........####",
        "####.........#####",
        "##################",
        ".################.",
        ".################.",
        "...############...",
    ],
];

/// The colon: a round dot on each leaf, as far above the seam as below it.
const COLON: [&str; 30] = [
    "....", "....", "....", "....", "....", "....", "....",
    ".##.", "####", "####", ".##.",
    "....", "....", "....", "....", "....", "....", "....", "....",
    ".##.", "####", "####", ".##.",
    "....", "....", "....", "....", "....", "....", "....",
];

/// One pose of a falling leaf: how far it has turned from upright, past 90
/// the back of it showing as it comes down; how long it is held, in ms; how
/// many steps down the ramp of card colours it is drawn, turned from the
/// light; and how many rows of shadow it throws on the leaf below it.
struct Pose {
    angle: f32,
    hold: i32,
    shade: usize,
    shadow: i32,
}

const POSES: [Pose; 9] = [
    // off the top, slowly, darkening as it tips over
    Pose { angle: 15.0, hold: 40, shade: 0, shadow: 0 },
    Pose { angle: 35.0, hold: 40, shade: 0, shadow: 0 },
    Pose { angle: 55.0, hold: 30, shade: 1, shadow: 0 },
    Pose { angle: 75.0, hold: 30, shade: 2, shadow: 0 },
    // edge-on: a line of the lit face over the seam
    Pose { angle: 90.0, hold: 30, shade: 0, shadow: 1 },
    // the back coming down, quicker and quicker, into the light and over
    // the old bottom leaf, which its shadow reaches first
    Pose { angle: 110.0, hold: 30, shade: 2, shadow: 2 },
    Pose { angle: 140.0, hold: 30, shade: 1, shadow: 2 },
    Pose { angle: 180.0, hold: 40, shade: 0, shadow: 0 },
    // and up off it again by a pixel, and still
    Pose { angle: 165.0, hold: 30, shade: 0, shadow: 1 },
];

/// Where a turning card has got to, `t` ms after it was due to start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// Its turn in the carry has not come: it shows the old figure, still.
    Waiting,
    Pose(usize),
    Done,
}

fn stage(t: i32) -> Stage {
    if t < 0 {
        return Stage::Waiting;
    }
    let mut end = 0;
    for (i, pose) in POSES.iter().enumerate() {
        end += pose.hold;
        if t < end {
            return Stage::Pose(i);
        }
    }
    Stage::Done
}

/// What a pixel of a card is made of.  Its colour is looked up as it is
/// drawn, in whatever light the leaf it is on is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Px {
    /// Off the card: a cut corner.
    Clear,
    Face,
    /// The leaf's lower and right-hand edges, a step darker.
    Edge,
    Ink,
    Seam,
}

/// A figure card's figure: a digit, or nothing -- the leading hour on the
/// 12-hour clock before ten.
type Figure = Option<u8>;

fn print(figure: Figure) -> Option<&'static [&'static str; 30]> {
    figure.map(|d| &DIGITS[d as usize])
}

/// A card on its way from one figure to the one it shows.
struct Turn {
    from: Figure,
    /// Milliseconds since it was due to start to fall: below 0 while it
    /// waits for its turn in the carry.
    t: i32,
}

struct Card {
    shown: Figure,
    turn: Option<Turn>,
}

/// The four figures of the time: on the 12-hour clock the hour has no
/// leading 0, and its first card is blank before ten.
fn figures(t: &Tm, use24: bool) -> [Figure; 4] {
    let hour = if use24 { t.hour } else if t.hour % 12 == 0 { 12 } else { t.hour % 12 };
    let tens = (hour / 10) as u8;
    let min = t.min as u8;
    [(use24 || tens > 0).then_some(tens), Some(hour as u8 % 10), Some(min / 10), Some(min % 10)]
}

fn date_of(t: &Tm) -> String {
    format!("{}, {} {}", DAYS[t.wday as usize % 7], t.day, MONTHS[(t.month - 1).clamp(0, 11) as usize])
}

/// The 12 or the 24 hour clock: whichever the Date/Time widget is set to.
fn use24(ctx: &Ctx) -> bool {
    ctx.settings.flag("DateTime", "Use24Hour", skin::datetime::SKIN.int("Use24Hour", 1) != 0)
}

/// A row of colours from (x, y) rightwards, a run of the same colour at a
/// time; None is left as it is.
fn paint_row(canvas: &mut Canvas, x: i32, y: i32, colours: impl Iterator<Item = Option<u32>>) {
    let mut run: Option<(i32, u32)> = None;
    let mut k = 0;
    for c in colours {
        if run.map(|r| r.1) != c {
            if let Some((start, colour)) = run {
                canvas.fill(x + start, y, k - start, 1, colour);
            }
            run = c.map(|c| (k, c));
        }
        k += 1;
    }
    if let Some((start, colour)) = run {
        canvas.fill(x + start, y, k - start, 1, colour);
    }
}

/// The column of a card `w` wide that is shown `k` px in from the left of a
/// row of it `s` px wider each side: its outermost column stays outermost,
/// the one inside that is drawn s more times, and the rest -- the figure
/// among it -- is as it was.
fn stretched(k: i32, s: i32, w: i32) -> i32 {
    match k {
        0 => 0,
        k if k <= s => 1,
        k if k >= w + 2 * s - 1 => w - 1,
        k if k >= w + s - 1 => w - 2,
        k => k - s,
    }
}

/// `n * a / b`, to the nearest whole.
fn share(n: i32, a: i32, b: i32) -> i32 {
    (2 * n * a + b) / (2 * b)
}

pub struct Flip {
    cards: [Card; 4],
    date: String,
    ampm: &'static str,

    /// The top left of [MeterCards] in the window, and its size.
    origin: (i32, i32),
    slot_w: i32,
    slot_h: i32,
    /// The cards in the slot: the first's left and all of their tops, the
    /// size of a figure card and of the colon's, and the room between.
    card_x: i32,
    card_y: i32,
    card_w: i32,
    card_h: i32,
    /// The rows of a leaf: the seam is the row after the top leaf's.
    half: i32,
    colon_w: i32,
    gap: i32,

    /// The card's colours, lightest first.
    ramp: [u32; 5],
    ink: u32,
    slot: u32,
    seam: u32,
    /// A pin, top to bottom.
    pin: [u32; 3],
}

impl Flip {
    pub fn new(ctx: &Ctx) -> Flip {
        let t = clock::local(ctx.now);
        let use24 = use24(ctx);
        let shown = figures(&t, use24);
        let card_h = SKIN.int("CardH", 51).max(3) | 1;
        let mut flip = Flip {
            cards: shown.map(|shown| Card { shown, turn: None }),
            date: date_of(&t),
            ampm: if use24 { "" } else if t.hour < 12 { "AM" } else { "PM" },
            origin: SKIN.meter("MeterCards").map_or((0, 0), |m| (m.b[0], m.b[1])),
            slot_w: SKIN.int("SlotW", 174),
            slot_h: SKIN.int("SlotH", 61),
            card_x: SKIN.int("CardX", 7),
            card_y: SKIN.int("CardY", 4),
            card_w: SKIN.int("CardW", 34).max(GLYPH_W as i32 + 2),
            card_h,
            half: card_h / 2,
            colon_w: SKIN.int("ColonW", 12),
            gap: SKIN.int("CardGap", 3),
            // all from the skin's colours, just below
            ramp: [0; 5],
            ink: 0,
            slot: 0,
            seam: 0,
            pin: [0; 3],
        };
        flip.read_colours();
        flip
    }

    /// The cards' colours, from the skin's, in the tint: at the start, and
    /// again for every change of tint.
    fn read_colours(&mut self) {
        self.ramp = core::array::from_fn(|i| SKIN.colour(&format!("Card{i}")));
        self.ink = SKIN.colour("CardInk");
        self.slot = SKIN.colour("Slot");
        self.seam = SKIN.colour("Seam");
        self.pin = [SKIN.colour("PinLight"), SKIN.colour("PinFace"), SKIN.colour("PinDark")];
    }

    /// Figure card `k`'s left, from the slot's: two cards, the colon's, two.
    fn left(&self, k: usize) -> i32 {
        let step = self.card_w + self.gap;
        let k = k as i32;
        self.card_x + k * step + if k >= 2 { self.colon_w + self.gap } else { 0 }
    }

    fn colon_left(&self) -> i32 {
        self.card_x + 2 * (self.card_w + self.gap)
    }

    /// What a repaint of figure card `k` has to cover, in the window: the
    /// card, as much wider as a leaf ever stands out, and the line under it.
    fn card_rect(&self, k: usize) -> Rect {
        let (x, y) = (self.origin.0 + self.left(k), self.origin.1 + self.card_y);
        Rect::xywh(x - 2, y, self.card_w + 4, self.card_h + 1)
    }

    /// What the pixel at (col, row) of a card `w` wide is made of, with
    /// `print` on it.
    fn px(&self, print: Option<&[&str; 30]>, w: i32, col: i32, row: i32) -> Px {
        let (half, h) = (self.half, self.card_h);
        if row == half {
            return Px::Seam;
        }
        if (col == 0 || col == w - 1) && (row == 0 || row == h - 1) {
            return Px::Clear;
        }
        if let Some(rows) = print {
            let gw = rows[0].len() as i32;
            let gc = col - (w - gw) / 2;
            let gr = if row < half { row - (half - GLYPH_H / 2) } else { row - half - 1 + GLYPH_H / 2 };
            if (0..gw).contains(&gc) && (0..GLYPH_H).contains(&gr) && rows[gr as usize].as_bytes()[gc as usize] == b'#' {
                return Px::Ink;
            }
        }
        if col == w - 1 || row == half - 1 || row == h - 1 { Px::Edge } else { Px::Face }
    }

    /// A card's pixel, `shade` steps down the ramp; the ink and the seam are
    /// as dark as they get already.
    fn colour(&self, px: Px, shade: usize) -> Option<u32> {
        match px {
            Px::Clear => None,
            Px::Face => Some(self.ramp[shade.min(4)]),
            Px::Edge => Some(self.ramp[(2 + shade).min(4)]),
            Px::Ink => Some(self.ink),
            Px::Seam => Some(self.seam),
        }
    }

    /// Rows `rows` of a card `w` wide with `print` on it, standing still with
    /// its top left at (x, y), each `shade(row)` steps down the ramp.
    fn rows(&self, canvas: &mut Canvas, print: Option<&[&str; 30]>, w: i32, x: i32, y: i32, rows: Range<i32>, shade: impl Fn(i32) -> usize) {
        for row in rows {
            let s = shade(row);
            paint_row(canvas, x, y + row, (0..w).map(|col| self.colour(self.px(print, w, col, row), s)));
        }
    }

    /// What every card stands in: a pin at each end of its seam, and a line
    /// of shadow under it.
    fn base(&self, canvas: &mut Canvas, w: i32, x: i32, y: i32) {
        for (dy, colour) in (-1..=1).zip(self.pin) {
            canvas.fill(x - 1, y + self.half + dy, 1, 1, colour);
            canvas.fill(x + w, y + self.half + dy, 1, 1, colour);
        }
        canvas.fill(x + 1, y + self.card_h, w - 2, 1, self.seam);
    }

    /// What stands still of a figure card: all of it, or -- while a leaf
    /// falls -- the new figure's top leaf, already behind the falling one,
    /// over the old figure's bottom leaf, with the shadow the leaf coming
    /// down onto it throws.
    fn back(&self, canvas: &mut Canvas, card: &Card, x: i32, y: i32) {
        let (w, half, h) = (self.card_w, self.half, self.card_h);
        let Some(turn) = &card.turn else {
            return self.rows(canvas, print(card.shown), w, x, y, 0..h, |_| 0);
        };
        let Stage::Pose(i) = stage(turn.t) else {
            let figure = if stage(turn.t) == Stage::Waiting { turn.from } else { card.shown };
            return self.rows(canvas, print(figure), w, x, y, 0..h, |_| 0);
        };
        let pose = &POSES[i];
        // the last row the falling leaf covers: the seam, until it is past
        // edge-on and coming down over the bottom leaf
        let covered = if pose.angle > 90.0 { half + self.leaf_rows(pose) } else { half };
        self.rows(canvas, print(card.shown), w, x, y, 0..half + 1, |_| 0);
        self.rows(canvas, print(turn.from), w, x, y, half + 1..h, |row| {
            let d = row - covered;
            if (1..=pose.shadow).contains(&d) { (pose.shadow - d + 1) as usize } else { 0 }
        });
    }

    /// How many rows of a leaf show, turned as `pose` is: as many as its
    /// height foreshortened comes to, and never none.
    fn leaf_rows(&self, pose: &Pose) -> i32 {
        ((self.half as f32 * pose.angle.to_radians().cos().abs()).round() as i32).clamp(1, self.half)
    }

    /// The falling leaf of a card going from `old` to `new`, turned as
    /// `pose` is.  Before edge-on it is the old figure's top leaf, hinged at
    /// its foot; after, it is the new figure's bottom leaf, on its back,
    /// hinged at its head.  The rows it shows are its own, evenly chosen,
    /// first and last always among them; the free edge stands out by up to
    /// `swell` px each side, narrowing to none at the hinge.
    fn leaf(&self, canvas: &mut Canvas, new: Figure, old: Figure, pose: &Pose, x: i32, y: i32) {
        let (w, half) = (self.card_w, self.half);
        let swell = (2.0 * pose.angle.to_radians().sin() + 0.001).floor().max(0.0) as i32;
        if pose.angle == 90.0 {
            let lit = self.ramp[pose.shade.min(4)];
            paint_row(canvas, x - swell, y + half - 1, (0..w + 2 * swell).map(|_| Some(lit)));
            paint_row(canvas, x - swell, y + half, (0..w + 2 * swell).map(|_| Some(self.seam)));
            return;
        }
        let rows = self.leaf_rows(pose);
        let falling = pose.angle < 90.0;
        let print = print(if falling { old } else { new });
        // a leaf's rows, counted from its first, go to `last`
        let last = half - 1;
        for i in 0..rows {
            let along = if rows == 1 { if falling { last } else { 0 } } else { share(i, last, rows - 1) };
            // how far the row is from the hinge, out of rows - 1
            let out = if falling { rows - 1 - i } else { i };
            let s = if rows == 1 { swell } else { share(swell, out, rows - 1) };
            let (src, dst) = if falling { (along, half - rows + i) } else { (half + 1 + along, half + 1 + i) };
            paint_row(canvas, x - s, y + dst, (0..w + 2 * s).map(|k| self.colour(self.px(print, w, stretched(k, s, w), src), pose.shade)));
        }
    }

    /// Everything in the slot, with (x, y) its top left: what shows between
    /// the cards, then every card as it stands, then any leaf that is
    /// falling -- in front of all of it, as a leaf near edge-on stands out
    /// over the gap beside its card.
    fn draw_cards(&self, x: i32, y: i32, canvas: &mut Canvas) {
        let outside = canvas.clip;
        canvas.clip = outside.intersect(&Rect::xywh(x, y, self.slot_w, self.slot_h));
        canvas.fill(x, y, self.slot_w, self.slot_h, self.slot);
        let top = y + self.card_y;
        let colon = x + self.colon_left();
        self.base(canvas, self.colon_w, colon, top);
        self.rows(canvas, Some(&COLON), self.colon_w, colon, top, 0..self.card_h, |_| 0);
        for (k, card) in self.cards.iter().enumerate() {
            let left = x + self.left(k);
            self.base(canvas, self.card_w, left, top);
            self.back(canvas, card, left, top);
        }
        for (k, card) in self.cards.iter().enumerate() {
            if let Some(turn) = &card.turn
                && let Stage::Pose(i) = stage(turn.t)
            {
                self.leaf(canvas, card.shown, turn.from, &POSES[i], x + self.left(k), top);
            }
        }
        canvas.clip = outside;
    }
}

impl Hooks for Flip {
    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "DateText" => &self.date,
            "AmPm" => self.ampm,
            _ => "",
        });
    }

    /// [MeterCards]: every card, still or turning.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        if meter.name == "MeterCards" {
            self.draw_cards(x, y, canvas);
        }
    }
}

impl Widget for Flip {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Flip Clock\n\n\
         The time on cards that flip, as a bedside clock of the seventies \
         kept it: a card for each figure, split across the middle, and on \
         the minute the top half of each card that changes falls over to \
         show the next.  When the minute carries, the cards go one after \
         another, right to left.\n\n\
         The hours follow the 12 or 24 hour choice of the Date/Time widget.  \
         Between the flips it costs nothing."
    }

    /// The time, to the minute: a card that should show another figure
    /// starts to fall, those to the left a moment after those to the right.
    /// A card still falling from a tick before has had no frames since --
    /// the workstation is locked, or the screen is off -- and lands at once.
    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        let mut dirty = Dirty::None;
        for k in 0..self.cards.len() {
            if self.cards[k].turn.take().is_some() {
                dirty = dirty.and(Dirty::Part(self.card_rect(k)));
            }
        }
        let t = clock::local(ctx.now);
        let use24 = use24(ctx);
        let (date, ampm) = (date_of(&t), if use24 { "" } else if t.hour < 12 { "AM" } else { "PM" });
        if date != self.date || ampm != self.ampm {
            (self.date, self.ampm) = (date, ampm);
            dirty = Dirty::All;
        }
        let mut wait = 0;
        for (k, figure) in figures(&t, use24).into_iter().enumerate().rev() {
            if figure == self.cards[k].shown {
                continue;
            }
            let card = &mut self.cards[k];
            card.turn = Some(Turn { from: card.shown, t: -wait });
            card.shown = figure;
            // the first to fall is in its first pose already
            if wait == 0 {
                dirty = dirty.and(Dirty::Part(self.card_rect(k)));
            }
            wait += CASCADE_MS;
        }
        dirty
    }

    fn frame_ms(&self) -> u32 {
        if self.cards.iter().any(|c| c.turn.is_some()) { FRAME_MS } else { 0 }
    }

    /// `ms` more of every turn: a card is repainted when it comes to another
    /// pose, and for the last time when it lands.
    fn frame(&mut self, ms: u32) -> Dirty {
        let mut dirty = Dirty::None;
        for k in 0..self.cards.len() {
            let Some(turn) = &mut self.cards[k].turn else { continue };
            let before = stage(turn.t);
            turn.t = turn.t.saturating_add(ms.min(i32::MAX as u32) as i32);
            let after = stage(turn.t);
            if after == Stage::Done {
                self.cards[k].turn = None;
            }
            if after != before {
                dirty = dirty.and(Dirty::Part(self.card_rect(k)));
            }
        }
        dirty
    }

    fn command(&mut self, cmd: &str, _arg: i32, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        match cmd {
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn retint(&mut self) {
        self.read_colours();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    /// A clock made at this local time, then ticked at the next minute: the
    /// cards that change are falling.
    fn turning(settings: &mut Settings, from: (i32, i32), to: (i32, i32)) -> Flip {
        turning_and(settings, from, to).0
    }

    /// The same, and what the tick repainted.
    fn turning_and(settings: &mut Settings, from: (i32, i32), to: (i32, i32)) -> (Flip, Dirty) {
        let at = |(h, m): (i32, i32)| clock::mktime(2026, 11, 21, h, m, 0);
        let mut flip = Flip::new(&Ctx { now: at(from), settings, locked: false, signed_in: None });
        let dirty = flip.tick(&mut Ctx { now: at(to), settings, locked: false, signed_in: None });
        (flip, dirty)
    }

    /// The cards drawn into a buffer the size of the slot.
    fn drawn(flip: &Flip) -> Vec<u32> {
        let mut px = vec![0u32; (flip.slot_w * flip.slot_h) as usize];
        // SAFETY: the canvas is exactly the vector.
        let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), flip.slot_w, flip.slot_w, flip.slot_h) };
        flip.draw_cards(0, 0, &mut canvas);
        px
    }

    #[test]
    fn the_figures_are_eighteen_by_thirty_in_ink_and_paper() {
        for (d, rows) in DIGITS.iter().enumerate() {
            for row in rows {
                assert_eq!(row.len(), GLYPH_W, "{d}: {row:?}");
                assert!(row.bytes().all(|c| c == b'#' || c == b'.'), "{d}: {row:?}");
            }
            // every figure is inked on both leaves, and none touches the
            // card's own edge
            assert!(rows[..15].iter().any(|r| r.contains('#')) && rows[15..].iter().any(|r| r.contains('#')), "{d}");
        }
        assert!(COLON.iter().all(|row| row.len() == 4));
    }

    #[test]
    fn a_leaf_shortens_to_edge_on_and_lengthens_again_as_it_falls() {
        let flip = Flip::new(&Ctx { now: 0, settings: &mut Settings::in_memory(), locked: false, signed_in: None });
        let before: Vec<i32> = POSES.iter().filter(|p| p.angle < 90.0).map(|p| flip.leaf_rows(p)).collect();
        let after: Vec<i32> = POSES.iter().filter(|p| p.angle > 90.0).map(|p| flip.leaf_rows(p)).collect();
        assert!(before.windows(2).all(|w| w[0] > w[1]), "{before:?}");
        // down to the landing, then the bounce up off it
        assert!(after[..after.len() - 1].windows(2).all(|w| w[0] < w[1]), "{after:?}");
        assert_eq!((after[after.len() - 2], *after.last().unwrap()), (flip.half, flip.half - 1));
        assert!(POSES.iter().any(|p| p.angle == 90.0));
        assert!(POSES.windows(2).all(|w| w[0].angle != w[1].angle));
        assert_eq!(POSES.iter().map(|p| p.hold).sum::<i32>(), 300);
        assert!(POSES.iter().all(|p| p.hold >= FRAME_MS as i32));
    }

    #[test]
    fn a_minute_turns_only_the_cards_that_change_right_to_left() {
        let mut settings = Settings::in_memory();
        settings.set_int("DateTime", "Use24Hour", 1);
        let (mut flip, dirty) = turning_and(&mut settings, (10, 59), (11, 0));
        let waits: Vec<Option<i32>> = flip.cards.iter().map(|c| c.turn.as_ref().map(|t| t.t)).collect();
        assert_eq!(waits, [None, Some(-160), Some(-80), Some(0)]);
        assert_eq!(flip.cards.each_ref().map(|c| c.shown), [Some(1), Some(1), Some(0), Some(0)]);
        assert_eq!(flip.cards[1].turn.as_ref().unwrap().from, Some(0));
        // the first to fall is drawn falling at once; the rest wait, still
        assert_eq!(dirty, Dirty::Part(flip.card_rect(3)));
        assert_eq!(flip.frame_ms(), FRAME_MS);

        // Frame by frame, only the cards that turn are repainted, and each
        // lands a carry's moment after the one to its right.
        let turning = flip.card_rect(1).union(&flip.card_rect(3));
        let mut landed = [0; 4];
        for f in 1..=40 {
            match flip.frame(FRAME_MS) {
                Dirty::Part(r) => assert_eq!(r.union(&turning), turning, "frame {f}"),
                Dirty::None => {}
                Dirty::All => panic!("frame {f} repainted the whole window"),
            }
            for (k, card) in flip.cards.iter().enumerate().skip(1) {
                if landed[k] == 0 && card.turn.is_none() {
                    landed[k] = f;
                }
            }
        }
        let frames = |ms: i32| ms / FRAME_MS as i32;
        assert_eq!(landed, [0, frames(300 + 160), frames(300 + 80), frames(300)]);
        assert_eq!((flip.frame_ms(), flip.frame(FRAME_MS)), (0, Dirty::None));
    }

    #[test]
    fn a_landed_leaf_is_the_card_as_it_stands() {
        let mut settings = Settings::in_memory();
        let mut flip = turning(&mut settings, (10, 58), (10, 59));
        // the 180 degree pose, and then the card with no turn at all
        let landed: i32 = POSES.iter().take_while(|p| p.angle != 180.0).map(|p| p.hold).sum();
        flip.cards[3].turn.as_mut().unwrap().t = landed;
        let falling = drawn(&flip);
        flip.cards[3].turn = None;
        assert_eq!(falling, drawn(&flip));
        // and part way down it is something else
        flip.cards[3].turn = Some(Turn { from: Some(8), t: 0 });
        assert_ne!(falling, drawn(&flip));
    }

    #[test]
    fn edge_on_the_leaf_stands_out_beside_its_card() {
        let mut settings = Settings::in_memory();
        let mut flip = turning(&mut settings, (10, 58), (10, 59));
        let edge: i32 = POSES.iter().take_while(|p| p.angle != 90.0).map(|p| p.hold).sum();
        flip.cards[3].turn.as_mut().unwrap().t = edge;
        let px = drawn(&flip);
        let at = |x: i32, y: i32| px[(y * flip.slot_w + x) as usize];
        let (left, top) = (flip.left(3), flip.card_y);
        // two px out to the left, on the row over the seam, is the lit face
        assert_eq!(at(left - 2, top + flip.half - 1), flip.ramp[0]);
        assert_eq!(at(left - 2, top + flip.half), flip.seam);
        // and well above it, the gap between cards
        assert_eq!(at(left - 2, top + 2), flip.slot);
    }

    #[test]
    fn a_turn_the_frames_never_came_for_lands_at_the_next_tick() {
        let mut settings = Settings::in_memory();
        let mut flip = turning(&mut settings, (10, 58), (10, 59));
        assert!(flip.cards[3].turn.is_some());
        let now = clock::mktime(2026, 11, 21, 10, 59, 1);
        let dirty = flip.tick(&mut Ctx { now, settings: &mut settings, locked: false, signed_in: None });
        assert!(flip.cards.iter().all(|c| c.turn.is_none()));
        assert_eq!(dirty, Dirty::Part(flip.card_rect(3)));
    }

    #[test]
    fn the_hours_are_the_date_time_widgets() {
        let mut settings = Settings::in_memory();
        let at = |h: i32, m: i32| clock::mktime(2026, 11, 21, h, m, 0);
        settings.set_int("DateTime", "Use24Hour", 1);
        let flip = Flip::new(&Ctx { now: at(9, 5), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((flip.cards.each_ref().map(|c| c.shown), flip.ampm), ([Some(0), Some(9), Some(0), Some(5)], ""));
        assert_eq!(flip.date, "Saturday, 21 November");

        settings.set_int("DateTime", "Use24Hour", 0);
        let mut flip = Flip::new(&Ctx { now: at(9, 5), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((flip.cards.each_ref().map(|c| c.shown), flip.ampm), ([None, Some(9), Some(0), Some(5)], "AM"));
        let dirty = flip.tick(&mut Ctx { now: at(21, 5), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((flip.ampm, dirty), ("PM", Dirty::All));
        assert_eq!(figures(&clock::local(at(0, 7)), false), [Some(1), Some(2), Some(0), Some(7)]);
        assert_eq!(figures(&clock::local(at(12, 30)), false), [Some(1), Some(2), Some(3), Some(0)]);
    }
}
