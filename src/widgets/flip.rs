//! Flip Clock: the time on cards that turn over, as a bedside clock of the
//! seventies kept it -- and the one widget with no window round it.  It is an
//! object on the desktop: a plastic case with a rounded rim, in the warm grey
//! of the Claude gauges' bezels, the cards set into a dark recess in its
//! front, the day printed over them, and a shadow under it on the wallpaper.
//! There is a card for each figure, split across the middle into two leaves
//! on a hinge: on the minute the top leaf of each card that changes falls
//! over, forwards and down, and the next figure is there -- its top half on
//! the leaf that was behind, its bottom half on the back of the one that
//! fell.  A colon is no card -- nothing there turns -- but two dots in the
//! recess.
//!
//! It is drawn as the Battery and the Claude Usage gauges are, and in the same
//! light: a pixel at a time, from the upper left and a little in front, each
//! surface in the step of light it is turned to, and nothing outlined.
//!
//! - The case's rim is rounded over, so it catches the light along its top
//!   and left and turns from it along its foot and right, with the light
//!   thrown back at its very edge, as the gauges' bezels have.
//! - The recess goes down from the front by a wall that faces in, lit along
//!   its foot and in shade along its head, and its floor lies in the shadow of
//!   the walls over it.
//! - Each leaf is a slab of paper with its edges rounded over and its face
//!   bowed a little; the seam between the two is a groove, shade over it and
//!   light under it.  The leaves turn on metal pins, and under each card the
//!   edges of those that have fallen show.
//! - A leaf on its way over turns in the light as it turns in fact.  It falls
//!   forwards about its foot, like the flap of a letterbox, so its face turns
//!   to the floor, out of the light; past edge-on its back, which faced up,
//!   comes down in the light.  It is foreshortened by leaving rows out of it,
//!   never by squeezing them together, so every row it shows is one of its
//!   own; and near edge-on its free edge is nearer, so it stands a pixel or
//!   two wider than the card -- its paper wider, not its figure.
//! - The leaves round it are not still either.  The next leaf, which leaned
//!   back against the one that fell, comes up after it into place; and the
//!   landing pushes the bottom leaf and those hanging behind it back at their
//!   foot, and they swing forward and back again before they are still.
//!
//! The case, its shadow, the pins and the colons never change, and are worked
//! out once (`backdrop`); the cards are drawn over them.
//!
//! The fall is a handful of poses, each held as long as it would be seen
//! (`POSES`): slow off the top, quicker and quicker down, the leaves below
//! swinging, and one pose edge-on, which is the one that makes it a flip.
//! When the minute carries, each card to the left goes a moment after the one
//! to its right, as on the mechanism; with the seconds on, the seconds carry
//! in the same way every minute, and the minute every hour.
//!
//! Between the flips it asks for no frames, and costs nothing.  The hours are
//! on the 12 or the 24 hour clock as the Date/Time widget is, and there is a
//! card for each figure of the seconds or not: `skin_of` says which, from
//! settings.ini.

use crate::canvas::{Canvas, Rect};
use crate::clock::{self, Tm, MONTHS};
use crate::settings::Settings;
use crate::skin::{self, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Request, Widget};

/// There is one of these, and it keeps its own few settings in this section.
pub const FIRST: &str = "Flip";

/// Its entries in the right-click menu.
const MENU_SECONDS: u32 = 1;
const MENU_ABOUT: u32 = 2;

/// Milliseconds between frames while a card turns: each pose is held for 30
/// or 40 of them, so each is on the screen for a frame at least.
const FRAME_MS: u32 = 20;
/// When the minute carries, how long after a card starts to fall the one to
/// its left does.
const CASCADE_MS: i32 = 80;

/// A leaf's corners are rounded to this, and its edges rounded over this
/// many px; its face is bowed so that it is turned up this much at its head
/// and down at its foot -- the top leaf more than the bottom one, which hangs.
const LEAF_R: f32 = 3.5;
const LEAF_EDGE: f32 = 2.0;
const BOW_TOP: f32 = 0.35;
const BOW_FOOT: f32 = 0.25;
/// How far the recess's wall leans in, going down: 0 would be a step, 1 a
/// floor.
const WALL_SLOPE: f32 = 0.7;

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

/// The colon is no card -- nothing there turns -- but two dots on the floor
/// of the recess, each `DOT` px across with its corners off, as far above
/// the seam as below it: `DOT_FROM_SEAM` px from it to the dot's near edge.
const DOT: i32 = 4;
const DOT_FROM_SEAM: i32 = 5;

/// One pose of a card turning over.
///
/// The leaves are on a wheel that turns all the time, and the top one is
/// held back by a tab until the wheel lets it go.  It falls forwards about
/// its foot, like the flap of a letterbox, so its face turns to the floor and
/// out of the light, and past edge-on its back, which faced up, comes down
/// into view.  The leaves round it move too:
///
/// - the next leaf, which was leaning back against the one that fell, comes
///   up after it and stands against the tab;
/// - the leaves below -- the bottom leaf and those that fell before, hanging
///   behind it -- are pushed back by the landing, and swing forward past
///   where they hang and back again before they are still.
struct Pose {
    /// How far the falling leaf has turned from upright, in degrees: past 90
    /// its back shows, coming down; at 180 it has landed.
    angle: f32,
    /// How long the pose is held, in ms.
    hold: i32,
    /// How many rows of shadow there are beyond the falling leaf's free edge:
    /// on the leaf behind it as it tips forward, the gap it has opened, and on
    /// the leaf below it as it comes down, the shadow it throws.  Paper in
    /// front of paper needs that line to be seen apart from it.
    shadow: i32,
    /// How far the next leaf still leans back, in degrees.
    behind: f32,
    /// How far the leaves below are pushed back at their foot by the landing,
    /// in degrees; below 0, swung forward.
    below: f32,
}

const POSES: [Pose; 10] = [
    // off the top, slowly, turning out of the light; the next leaf comes up
    // after it
    Pose { angle: 15.0, hold: 40, shadow: 1, behind: 20.0, below: 0.0 },
    Pose { angle: 35.0, hold: 40, shadow: 1, behind: 12.0, below: 0.0 },
    Pose { angle: 55.0, hold: 30, shadow: 1, behind: 0.0, below: 0.0 },
    Pose { angle: 75.0, hold: 30, shadow: 1, behind: 0.0, below: 0.0 },
    // edge-on: its back, facing up into the light, over the seam
    Pose { angle: 90.0, hold: 30, shadow: 1, behind: 0.0, below: 0.0 },
    // the back coming down, quicker and quicker, in the light, over the old
    // bottom leaf, which its shadow reaches first
    Pose { angle: 110.0, hold: 30, shadow: 2, behind: 0.0, below: 0.0 },
    Pose { angle: 140.0, hold: 30, shadow: 2, behind: 0.0, below: 0.0 },
    // landed: it and the leaves under it are pushed back, swing forward
    // past where they hang, and back
    Pose { angle: 180.0, hold: 40, shadow: 0, behind: 0.0, below: 14.0 },
    Pose { angle: 180.0, hold: 30, shadow: 0, behind: 0.0, below: -12.0 },
    Pose { angle: 180.0, hold: 40, shadow: 0, behind: 0.0, below: 5.0 },
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

/// The figures of the time: four, or six with a card for each figure of the
/// seconds.  On the 12-hour clock the hour has no leading 0, and its first
/// card is blank before ten.
fn figures(t: &Tm, use24: bool, seconds: bool) -> Vec<Figure> {
    let hour = if use24 { t.hour } else if t.hour % 12 == 0 { 12 } else { t.hour % 12 };
    let tens = (hour / 10) as u8;
    let min = t.min as u8;
    let mut shown = vec![(use24 || tens > 0).then_some(tens), Some(hour as u8 % 10), Some(min / 10), Some(min % 10)];
    if seconds {
        let sec = t.sec as u8;
        shown.push(Some(sec / 10));
        shown.push(Some(sec % 10));
    }
    shown
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

// ---------------------------------------------------------------------------
//  The light, and the shapes it falls on
// ---------------------------------------------------------------------------

/// Which way the light comes from, as on the Battery and the gauges: the
/// upper left and a little in front.  x is to the right, y down, z out of the
/// screen.
const LIGHT: [f32; 3] = [-0.5, -0.6, 0.62];
/// A surface square to the eye.
const FRONT: [f32; 3] = [0.0, 0.0, 1.0];

/// How square to the light a surface facing `n` is: about 1 facing it, 0
/// edge on, and below 0 turned away.
fn facing(n: [f32; 3]) -> f32 {
    n[0] * LIGHT[0] + n[1] * LIGHT[1] + n[2] * LIGHT[2]
}

/// `n` turned by `angle` about a hinge -- the x axis -- so that a face
/// square to the eye comes to face the floor: as a leaf hinged at its foot
/// does when its top comes towards the eye, like the flap of a letterbox, and
/// one hinged at its head when its foot is pushed away.  A negative angle
/// turns it to face up, to the light.
fn turned(n: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    [n[0], n[1] * c + n[2] * s, n[2] * c - n[1] * s]
}

/// The steps of light one material is drawn in, as the gauges' are: a glint
/// facing the light square, shine, a lighter stretch -- a flat front -- its
/// own colour, shade and edge; and the light thrown back at the far edge of
/// a rounded one, a little lighter than the edge before it.
#[derive(Clone, Copy, Default)]
struct Shades {
    steps: [u32; 6],
    rim: u32,
}

impl Shades {
    /// `name`0 to `name`5 and `name`Rim, from the skin.
    fn from_skin(skin: &Skin, name: &str) -> Shades {
        Shades { steps: core::array::from_fn(|i| skin.colour(&format!("{name}{i}"))), rim: skin.colour(&format!("{name}Rim")) }
    }

    /// A surface facing `n`, `across` of the way over a rounded edge, and
    /// `darker` steps further into a shadow.  Stepped, as pixel art shades,
    /// never smoothed.
    fn lit(&self, n: [f32; 3], across: f32, darker: usize) -> u32 {
        let b = facing(n);
        if darker == 0 && across > 0.8 && b < -0.2 {
            return self.rim;
        }
        self.steps[(step(b) + darker).min(5)]
    }

    /// The same for a broad face, flat or bowed: never in the glint nor at
    /// the edge, which are a rounded edge's.  A whole leaf in the glint would
    /// be a blank, and one at the edge a stain.
    fn face(&self, n: [f32; 3], darker: usize) -> u32 {
        self.steps[(step(facing(n)).clamp(1, 4) + darker).min(5)]
    }
}

/// Which step of light a surface `b` square to it is in, 0 the glint.
fn step(b: f32) -> usize {
    match b {
        b if b >= 0.84 => 0,
        b if b >= 0.72 => 1,
        b if b >= 0.5 => 2,
        b if b >= 0.2 => 3,
        b if b >= -0.2 => 4,
        _ => 5,
    }
}

/// A rectangle with its corners rounded to `r`, edges anywhere, for the
/// light to be worked out on.
#[derive(Clone, Copy)]
struct Shape {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    r: f32,
}

impl Shape {
    fn of(rect: Rect, r: i32) -> Shape {
        Shape { x0: rect.x0 as f32, y0: rect.y0 as f32, x1: rect.x1 as f32, y1: rect.y1 as f32, r: r as f32 }
    }

    /// How far (x, y) is outside it -- below 0 inside -- and which way out of
    /// it is nearest.
    fn outside(&self, x: f32, y: f32) -> (f32, [f32; 2]) {
        let (hw, hh) = ((self.x1 - self.x0) / 2.0, (self.y1 - self.y0) / 2.0);
        let r = self.r.min(hw).min(hh).max(0.0);
        let (dx, dy) = (x - (self.x0 + hw), y - (self.y0 + hh));
        let (qx, qy) = (dx.abs() - (hw - r), dy.abs() - (hh - r));
        let (sx, sy) = (if dx < 0.0 { -1.0 } else { 1.0 }, if dy < 0.0 { -1.0 } else { 1.0 });
        if qx > 0.0 && qy > 0.0 {
            let len = (qx * qx + qy * qy).sqrt();
            (len - r, [sx * qx / len, sy * qy / len])
        } else if qx > qy {
            (qx - r, [sx, 0.0])
        } else {
            (qy - r, [0.0, sy])
        }
    }
}

/// Which way a surface faces `d` px in from an edge whose way out is `out`,
/// rounded over `width` px -- square out at the edge, square to the eye
/// `width` in -- and how far across the rounding the px is; None past it.
fn rounded_over(d: f32, out: [f32; 2], width: f32) -> Option<([f32; 3], f32)> {
    if d >= width {
        return None;
    }
    let w = (1.0 - d / width).clamp(0.0, 1.0);
    Some(([out[0] * w, out[1] * w, (1.0 - w * w).sqrt()], w))
}

/// A premultiplied pixel as the bytes `Canvas::image` takes: B, G, R, A.
fn bgra(argb: u32) -> [u8; 4] {
    argb.to_le_bytes()
}

// ---------------------------------------------------------------------------
//  The clock
// ---------------------------------------------------------------------------

pub struct Flip {
    /// What it is drawn from: four cards, or six with the seconds on.
    skin: &'static Skin,
    cards: Vec<Card>,
    seconds: bool,
    date: String,
    ampm: &'static str,

    /// [MeterClock]'s top left in the window, and its size: the window's.
    origin: (i32, i32),
    size: (i32, i32),
    /// The case; the recess in its front, to the top of its wall; the floor
    /// of the recess; all in [MeterClock].
    case: Rect,
    recess: Rect,
    floor: Rect,
    rim: i32,
    case_r: i32,
    recess_r: i32,
    wall: i32,
    cast: i32,
    /// The shadow on the desktop: across, down, how far it fades out, and
    /// how dark it is, out of 255.
    drop: (i32, i32, i32, i32),
    /// The first card's top left in [MeterClock].
    cards_at: (i32, i32),
    /// The cards' size, the figure cards' and the colons', the room between
    /// them, and the rows of fallen leaves showing under each.
    card_w: i32,
    card_h: i32,
    /// The rows of a leaf: the seam is the row after the top leaf's.
    half: i32,
    colon_w: i32,
    gap: i32,
    stack: i32,

    case_shades: Shades,
    paper: Shades,
    metal: Shades,
    floor_lit: u32,
    floor_cast: u32,
    seam: u32,
    ink: u32,
    /// The case, its shadow, the pins and the stacks under the cards, as
    /// they are whatever the time: premultiplied BGRA, [MeterClock]'s size.
    backdrop: Vec<u8>,
}

impl Flip {
    pub fn new(ctx: &Ctx) -> Flip {
        let skin = skin_of(ctx.settings, FIRST);
        let seconds = skin.int("Cards", 4) > 4;
        let t = clock::local(ctx.now);
        let use24 = use24(ctx);
        let int = |name: &str, default: i32| skin.int(name, default).max(0);
        let card_h = int("CardH", 51).max(3) | 1;
        let card_w = int("CardW", 34).max(GLYPH_W as i32 + 2 * LEAF_EDGE as i32 + 2);
        let (pad, rim, margin, strip, wall, inset, stack) =
            (int("Pad", 12), int("Rim", 5), int("CaseMargin", 5), int("Strip", 16), int("Wall", 2), int("Inset", 4), int("Stack", 2));
        let cards_w = int("CardsW", 160);
        let case = Rect::xywh(
            pad,
            pad,
            2 * (rim + margin + wall + inset) + cards_w,
            rim + strip + 2 * (wall + inset) + card_h + stack + margin + rim,
        );
        let recess = Rect::new(case.x0 + rim + margin, case.y0 + rim + strip, case.x1 - rim - margin, case.y1 - rim - margin);
        let floor = Rect::new(recess.x0 + wall, recess.y0 + wall, recess.x1 - wall, recess.y1 - wall);
        let meter = skin.meter("MeterClock");
        let mut flip = Flip {
            skin,
            cards: figures(&t, use24, seconds).into_iter().map(|shown| Card { shown, turn: None }).collect(),
            seconds,
            date: date_of(&t),
            ampm: if use24 { "" } else if t.hour < 12 { "AM" } else { "PM" },
            origin: meter.map_or((0, 0), |m| (m.b[0], m.b[1])),
            size: meter.map_or((skin.w, skin.h), |m| (m.b[2] - m.b[0], m.b[3] - m.b[1])),
            case,
            recess,
            floor,
            rim,
            case_r: int("CaseR", 9),
            recess_r: int("RecessR", 4),
            wall,
            cast: int("Cast", 4),
            drop: (int("DropX", 3), int("DropY", 4), int("DropSoft", 3), int("Drop", 90).min(255)),
            cards_at: (floor.x0 + inset, floor.y0 + inset),
            card_w,
            card_h,
            half: card_h / 2,
            colon_w: int("ColonW", 12),
            gap: int("CardGap", 3),
            stack,
            // all from the skin's colours, just below
            case_shades: Shades::default(),
            paper: Shades::default(),
            metal: Shades::default(),
            floor_lit: 0,
            floor_cast: 0,
            seam: 0,
            ink: 0,
            backdrop: Vec::new(),
        };
        flip.read_colours();
        flip
    }

    /// The colours, from the skin's, and what is worked out from them: at
    /// the start, and again for every change of tint (which changes none of
    /// them, but a skin may be given colours that it does).
    fn read_colours(&mut self) {
        let skin = self.skin;
        self.case_shades = Shades::from_skin(skin, "Case");
        self.paper = Shades::from_skin(skin, "Paper");
        self.metal = Shades::from_skin(skin, "Metal");
        self.floor_lit = skin.colour("Recess");
        self.floor_cast = skin.colour("RecessCast");
        self.seam = skin.colour("Seam");
        self.ink = skin.colour("CardInk");
        self.backdrop = self.make_backdrop();
    }

    /// The left of figure card `k`, from the first's: the colons' room
    /// counted in where it is, after the second card and after the fourth.
    fn left(&self, k: usize) -> i32 {
        let k = k as i32;
        k * (self.card_w + self.gap) + k / 2 * (self.colon_w + self.gap)
    }

    /// The left of each colon, from the first card's: one before every
    /// second card after the first two.
    fn colons(&self) -> impl Iterator<Item = i32> + '_ {
        (1..self.cards.len() / 2).map(|i| self.left(2 * i) - self.colon_w - self.gap)
    }

    /// What a repaint of figure card `k` has to cover, in the window: the
    /// card, as much wider as a leaf ever stands out, and the edges of the
    /// fallen leaves under it, which a landing sets swinging.
    fn card_rect(&self, k: usize) -> Rect {
        let x = self.origin.0 + self.cards_at.0 + self.left(k);
        Rect::xywh(x - 2, self.origin.1 + self.cards_at.1, self.card_w + 4, self.card_h + self.stack)
    }

    // ---- what never changes ------------------------------------------------

    /// The case, its shadow on the desktop and the recess, at (x, y) of
    /// [MeterClock]: a premultiplied colour, 0 where there is nothing.
    fn case_px(&self, x: i32, y: i32) -> u32 {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let case = Shape::of(self.case, self.case_r);
        let (out, way) = case.outside(px, py);
        if out > 0.0 {
            // The desktop, and the case's shadow on it: as dark as it gets
            // under the case moved down and across, and a step lighter for
            // each px of the fade beyond that.
            let (dx, dy, soft, dark) = self.drop;
            let (from, _) = case.outside(px - dx as f32, py - dy as f32);
            let steps = if from <= 0.0 { soft + 1 } else { soft + 1 - from.ceil() as i32 };
            return if steps <= 0 { 0 } else { ((dark * steps / (soft + 1)) as u32) << 24 };
        }
        if let Some((n, across)) = rounded_over(-out, way, self.rim as f32) {
            return self.case_shades.lit(n, across, 0);
        }
        let (out, way) = Shape::of(self.recess, self.recess_r).outside(px, py);
        if out > 0.0 {
            return self.case_shades.lit(FRONT, 0.0, 0);
        }
        if -out < self.wall as f32 {
            // the wall, going down into the recess: it faces in, towards
            // the middle of it
            let n = [-way[0] * WALL_SLOPE, -way[1] * WALL_SLOPE, (1.0 - WALL_SLOPE * WALL_SLOPE).sqrt()];
            return self.case_shades.lit(n, 0.0, 0);
        }
        // the floor, in the shadow of the walls over it and to its left
        if x - self.floor.x0 < self.cast || y - self.floor.y0 < self.cast { self.floor_cast } else { self.floor_lit }
    }

    /// Everything that is the same whatever the time: the case and its
    /// shadow, and in the recess the pins the leaves turn on -- at either
    /// side of every card, across its seam -- and the colons' dots.
    fn make_backdrop(&self) -> Vec<u8> {
        let (w, h) = self.size;
        let mut px: Vec<u32> = (0..w * h).map(|i| self.case_px(i % w, i / w)).collect();
        {
            // SAFETY: the canvas is exactly the vector.
            let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), w, w, h) };
            let (x0, y0) = self.cards_at;
            for k in 0..self.cards.len() {
                let left = x0 + self.left(k);
                // a pin either side, rounded across
                for (dy, n) in [(-1, [0.0, -WALL_SLOPE, 0.714]), (0, FRONT), (1, [0.0, WALL_SLOPE, 0.714])] {
                    let colour = self.metal.lit(n, 0.0, 0);
                    canvas.fill(left - self.gap, y0 + self.half + dy, self.gap, 1, colour);
                    canvas.fill(left + self.card_w, y0 + self.half + dy, self.gap, 1, colour);
                }
            }
            for left in self.colons() {
                let x = x0 + left + (self.colon_w - DOT) / 2;
                for top in [y0 + self.half - DOT_FROM_SEAM - DOT + 1, y0 + self.half + DOT_FROM_SEAM] {
                    self.dot(&mut canvas, x, top);
                }
            }
        }
        px.into_iter().flat_map(bgra).collect()
    }

    /// One of a colon's dots, its top left at (x, y): a little dome of paper,
    /// the light on its upper left.
    fn dot(&self, canvas: &mut Canvas, x: i32, y: i32) {
        let r = DOT as f32 / 2.0;
        for j in 0..DOT {
            for i in 0..DOT {
                let (u, v) = (i as f32 + 0.5 - r, j as f32 + 0.5 - r);
                if u * u + v * v > r * r {
                    continue;
                }
                let (a, b) = (u / (r + 0.2), v / (r + 0.2));
                let n = [a, b, (1.0 - a * a - b * b).max(0.0).sqrt()];
                canvas.fill(x + i, y + j, 1, 1, self.paper.lit(n, (a * a + b * b).sqrt(), 0));
            }
        }
    }

    // ---- the cards -----------------------------------------------------------

    /// Whether (col, row) of a card `w` wide with `print` on it is ink.
    fn inked(&self, print: Option<&[&str; 30]>, w: i32, col: i32, row: i32) -> bool {
        let Some(rows) = print else { return false };
        let half = self.half;
        let gw = rows[0].len() as i32;
        let gc = col - (w - gw) / 2;
        let gr = if row < half { row - (half - GLYPH_H / 2) } else { row - half - 1 + GLYPH_H / 2 };
        (0..gw).contains(&gc) && (0..GLYPH_H).contains(&gr) && rows[gr as usize].as_bytes()[gc as usize] == b'#'
    }

    /// Which way the paper faces at (col, row) of a card `w` wide, as it
    /// stands, and how far across a rounded edge the px is; None off its
    /// rounded corners.  Each leaf is a slab with its edges rounded over, its
    /// face bowed: turned up at its head, down at its foot.
    fn paper_facing(&self, w: i32, col: i32, row: i32) -> Option<([f32; 3], f32)> {
        let (half, h) = (self.half, self.card_h);
        let (head, foot) = if row < half { (0, half) } else { (half + 1, h) };
        let leaf = Shape { x0: 0.0, y0: head as f32, x1: w as f32, y1: foot as f32, r: LEAF_R };
        let (px, py) = (col as f32 + 0.5, row as f32 + 0.5);
        let (out, way) = leaf.outside(px, py);
        if out > 0.0 {
            return None;
        }
        if let Some(edge) = rounded_over(-out, way, LEAF_EDGE) {
            return Some(edge);
        }
        let t = (py - head as f32) / (foot - head) as f32 * 2.0 - 1.0;
        let ny = t * if row < half { BOW_TOP } else { BOW_FOOT };
        let len = (1.0 + ny * ny).sqrt();
        Some(([0.0, ny / len, 1.0 / len], 0.0))
    }

    /// (col, row) of a card `w` wide with `print` on it, turned `angle` from
    /// how it stands, and `darker` steps into a shadow: None where there is
    /// no card -- its rounded corners.  The ink is ink in any light.
    fn paper(&self, print: Option<&[&str; 30]>, w: i32, col: i32, row: i32, angle: f32, darker: usize) -> Option<u32> {
        if row == self.half {
            return Some(self.seam);
        }
        let (n, across) = self.paper_facing(w, col, row)?;
        if self.inked(print, w, col, row) {
            return Some(self.ink);
        }
        let n = turned(n, angle);
        Some(if across > 0.0 { self.paper.lit(n, across, darker) } else { self.paper.face(n, darker) })
    }

    /// How many rows of a leaf show, turned `degrees` from upright, either
    /// way: as many as its height foreshortened comes to, and never none.
    fn rows_at(&self, degrees: f32) -> i32 {
        ((self.half as f32 * degrees.to_radians().cos().abs()).round() as i32).clamp(1, self.half)
    }

    /// One leaf of a card `w` wide with `print` on it, hinged at the seam --
    /// the top leaf (`upper`), or the bottom one -- with its top left at
    /// (x, y), shown `rows` rows tall and its faces `turn` (radians) from
    /// facing the eye (see `turned`).  The rows it shows are its own, evenly
    /// chosen, first and last always among them; its free edge stands out
    /// by up to `swell` px each side, narrowing to none at the hinge; and
    /// each row is `darker(row)` steps into a shadow, the row counted down
    /// the card.  Rows it no longer reaches are left as they were.
    #[allow(clippy::too_many_arguments)]
    fn leaf_of(&self, canvas: &mut Canvas, print: Option<&[&str; 30]>, upper: bool, rows: i32, turn: f32, swell: i32, x: i32, y: i32, darker: impl Fn(i32) -> usize) {
        let (w, half) = (self.card_w, self.half);
        // a leaf's rows, counted from its first, go to `last`
        let last = half - 1;
        for i in 0..rows {
            let along = if rows == 1 { if upper { last } else { 0 } } else { share(i, last, rows - 1) };
            // how far the row is from the hinge, out of rows - 1
            let out = if upper { rows - 1 - i } else { i };
            let s = if rows == 1 { swell } else { share(swell, out, rows - 1) };
            let (src, dst) = if upper { (along, half - rows + i) } else { (half + 1 + along, half + 1 + i) };
            let d = darker(dst);
            paint_row(canvas, x - s, y + dst, (0..w + 2 * s).map(|k| self.paper(print, w, stretched(k, s, w), src, turn, d)));
        }
    }

    /// The edges of the leaves that have fallen, under a card whose top left
    /// is (x, y): `lift` rows higher while a landing sets them swinging.
    fn stack(&self, canvas: &mut Canvas, x: i32, y: i32, lift: i32) {
        for k in 0..self.stack.min(2) {
            let n = if k == 0 { [0.0, 0.5, 0.866] } else { [0.0, 0.8, 0.6] };
            canvas.fill(x + 1 + k, y + self.card_h + k - lift, self.card_w - 2 - 2 * k, 1, self.paper.lit(n, 0.0, 0));
        }
    }

    /// A card standing still with `figure` on it, its top left at (x, y).
    fn still(&self, canvas: &mut Canvas, figure: Figure, x: i32, y: i32) {
        let (print, half) = (print(figure), self.half);
        self.leaf_of(canvas, print, true, half, 0.0, 0, x, y, |_| 0);
        paint_row(canvas, x, y + half, (0..self.card_w).map(|_| Some(self.seam)));
        self.leaf_of(canvas, print, false, half, 0.0, 0, x, y, |_| 0);
        self.stack(canvas, x, y, 0);
    }

    /// Everything of a card going from `old` to `new` but the leaf that is
    /// falling: the new figure's top leaf behind it, coming up into place;
    /// and under the seam the old figure's bottom leaf -- or, once the
    /// falling one has landed, the new figure's, swinging with the leaves
    /// under it.  The shadow beyond the falling leaf's free edge is on
    /// whichever of them it is over.
    fn behind(&self, canvas: &mut Canvas, new: Figure, old: Figure, pose: &Pose, x: i32, y: i32) {
        let half = self.half;
        // The falling leaf's free edge: its first row over the seam while it
        // tips forward, its last row under it once it is coming down.  The
        // shadow is darkest next to it.
        let rows = self.rows_at(pose.angle);
        let (edge, away) = if pose.angle < 90.0 { (half - rows, -1) } else if pose.angle > 90.0 { (half + rows, 1) } else { (half, 1) };
        let shadow = |row: i32| {
            let d = (row - edge) * away;
            if (1..=pose.shadow).contains(&d) { (pose.shadow - d + 1) as usize } else { 0 }
        };
        // leaning back, it faces up, into the light
        let up = self.rows_at(pose.behind);
        self.leaf_of(canvas, print(new), true, up, -pose.behind.to_radians(), 0, x, y, shadow);
        paint_row(canvas, x, y + half, (0..self.card_w).map(|_| Some(self.seam)));
        if pose.angle < 180.0 {
            self.leaf_of(canvas, print(old), false, half, 0.0, 0, x, y, shadow);
            self.stack(canvas, x, y, 0);
        } else {
            // pushed back at its foot, it faces down, out of the light, and
            // swung forward it faces up; either way it shows shorter, and the
            // leaves hanging behind it rise with it
            let down = self.rows_at(pose.below);
            self.leaf_of(canvas, print(new), false, down, pose.below.to_radians(), 0, x, y, shadow);
            self.stack(canvas, x, y, half - down);
        }
    }

    /// The falling leaf of a card going from `old` to `new`, turned as
    /// `pose` is; nothing once it has landed.  Before edge-on it is the old
    /// figure's top leaf, hinged at its foot, its face turning to the floor;
    /// after, it is the new figure's bottom leaf, on its back, which faced up
    /// and comes down into view.
    fn leaf(&self, canvas: &mut Canvas, new: Figure, old: Figure, pose: &Pose, x: i32, y: i32) {
        if pose.angle >= 180.0 {
            return;
        }
        let (w, half) = (self.card_w, self.half);
        let angle = pose.angle.to_radians();
        let swell = (2.0 * angle.sin() + 0.001).floor().max(0.0) as i32;
        let back = angle - core::f32::consts::PI;
        if pose.angle == 90.0 {
            // edge-on: its back, square up to the light, over the seam
            let up = self.paper.lit(turned(FRONT, back), 0.0, 0);
            paint_row(canvas, x - swell, y + half - 1, (0..w + 2 * swell).map(|_| Some(up)));
            paint_row(canvas, x - swell, y + half, (0..w + 2 * swell).map(|_| Some(self.seam)));
            return;
        }
        let falling = pose.angle < 90.0;
        let (print, turn) = if falling { (print(old), angle) } else { (print(new), back) };
        self.leaf_of(canvas, print, falling, self.rows_at(pose.angle), turn, swell, x, y, |_| 0);
    }

    /// Everything [MeterClock] is, with (x, y) its top left: the case and
    /// all else that stands still, then every card as it stands, then any
    /// leaf that is falling -- in front of all of it, as a leaf near edge-on
    /// stands out over the gap beside its card.  Only what is in the
    /// canvas's clip is worked out.
    fn draw_clock(&self, x: i32, y: i32, canvas: &mut Canvas) {
        let outside = canvas.clip;
        let (w, h) = self.size;
        canvas.clip = outside.intersect(&Rect::xywh(x, y, w, h));
        if self.backdrop.len() == (w * h * 4) as usize {
            canvas.image(x, y, w, h, &self.backdrop);
        }
        let (cx, cy) = (x + self.cards_at.0, y + self.cards_at.1);
        let clip = canvas.clip;
        let near = |left: i32, width: i32| Rect::xywh(cx + left - 2, cy, width + 4, self.card_h + self.stack).intersects(&clip);
        for (k, card) in self.cards.iter().enumerate() {
            if !near(self.left(k), self.card_w) {
                continue;
            }
            let left = cx + self.left(k);
            match card.turn.as_ref().map(|turn| (turn, stage(turn.t))) {
                Some((turn, Stage::Pose(i))) => self.behind(canvas, card.shown, turn.from, &POSES[i], left, cy),
                Some((turn, Stage::Waiting)) => self.still(canvas, turn.from, left, cy),
                _ => self.still(canvas, card.shown, left, cy),
            }
        }
        for (k, card) in self.cards.iter().enumerate() {
            if let Some(turn) = &card.turn
                && let Stage::Pose(i) = stage(turn.t)
                && near(self.left(k), self.card_w)
            {
                self.leaf(canvas, card.shown, turn.from, &POSES[i], cx + self.left(k), cy);
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

    /// [MeterClock]: the case and every card, still or turning.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        if meter.name == "MeterClock" {
            self.draw_clock(x, y, canvas);
        }
    }
}

/// The skin settings.ini asks for: six cards with the seconds on, four
/// without.
pub fn skin_of(settings: &Settings, section: &str) -> &'static Skin {
    if settings.flag(section, "Seconds", false) { &skin::flip_secs::SKIN } else { &skin::flip::SKIN }
}

impl Widget for Flip {
    fn skin(&self) -> &'static Skin {
        self.skin
    }

    fn about(&self) -> &'static str {
        "Win98 Flip Clock\n\n\
         The time on cards that turn over, as a bedside clock of the seventies \
         kept it -- and the one widget with no window round it: a grey case \
         on the desktop, the cards set into its front and the day printed over \
         them.  A card for each figure, split across the middle, and on the \
         minute the top half of each card that changes falls over to show the \
         next.  When the minute carries, the cards go one after another, right \
         to left.\n\n\
         The hours follow the 12 or 24 hour choice of the Date/Time widget, and \
         *Show seconds* in the right-click menu adds a card for each figure of \
         the seconds, which turn over every second.  Between the flips it costs \
         nothing."
    }

    /// The time, to the second: a card that should show another figure
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
        for (k, figure) in figures(&t, use24, self.seconds).into_iter().enumerate().rev() {
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

    /// The clock has no title bar to put an entry in, so its own two are here:
    /// the seconds, which the host makes the window again for, and About.
    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem::new(MENU_SECONDS, "Show seconds", self.seconds));
        items.push(MenuItem::line());
        items.push(MenuItem::new(MENU_ABOUT, "About Flip Clock", false));
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            MENU_SECONDS => {
                let seconds = !self.seconds;
                ctx.settings.set(FIRST, "Seconds", if seconds { "1" } else { "0" });
                Outcome::request(Request::Reshape)
            }
            MENU_ABOUT => Outcome::request(Request::About),
            _ => Outcome::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    /// This local time on a Saturday.
    fn at(h: i32, m: i32, s: i32) -> i64 {
        clock::mktime(2026, 11, 21, h, m, s)
    }

    /// A clock made at this local time, then ticked at the next minute: the
    /// cards that change are falling.
    fn turning(settings: &mut Settings, from: (i32, i32), to: (i32, i32)) -> Flip {
        let mut flip = Flip::new(&Ctx { now: at(from.0, from.1, 0), settings, locked: false, signed_in: None });
        flip.tick(&mut Ctx { now: at(to.0, to.1, 0), settings, locked: false, signed_in: None });
        flip
    }

    /// The clock drawn into a buffer the size of [MeterClock], as its window
    /// is.
    fn drawn(flip: &Flip) -> Vec<u32> {
        let (w, h) = flip.size;
        let mut px = vec![0u32; (w * h) as usize];
        // SAFETY: the canvas is exactly the vector.
        let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), w, w, h) };
        flip.draw_clock(0, 0, &mut canvas);
        px
    }

    /// How light a pixel is, for saying which of two parts the light falls on.
    fn light(px: u32) -> u32 {
        (px >> 16 & 255) + (px >> 8 & 255) + (px & 255)
    }

    fn clock_at(settings: &mut Settings, h: i32, m: i32) -> Flip {
        settings.set_int("DateTime", "Use24Hour", 1);
        Flip::new(&Ctx { now: at(h, m, 0), settings, locked: false, signed_in: None })
    }

    #[test]
    fn the_figures_are_eighteen_by_thirty_in_ink_and_paper() {
        for (d, rows) in DIGITS.iter().enumerate() {
            for row in rows {
                assert_eq!(row.len(), GLYPH_W, "{d}: {row:?}");
                assert!(row.bytes().all(|c| c == b'#' || c == b'.'), "{d}: {row:?}");
            }
            // every figure is inked on both leaves
            assert!(rows[..15].iter().any(|r| r.contains('#')) && rows[15..].iter().any(|r| r.contains('#')), "{d}");
        }
    }

    #[test]
    fn the_case_is_laid_out_round_the_cards_as_the_skin_has_it() {
        for seconds in [false, true] {
            let mut settings = Settings::in_memory();
            settings.set(FIRST, "Seconds", if seconds { "1" } else { "0" });
            let flip = clock_at(&mut settings, 9, 41);
            // the window is the case and the Pad round it, the case's shadow
            // falling in the Pad
            let pad = flip.skin.int("Pad", 12);
            assert_eq!(flip.size, (flip.skin.w, flip.skin.h));
            assert_eq!((flip.case.x0, flip.case.y0, flip.case.x1 + pad, flip.case.y1 + pad), (pad, pad, flip.size.0, flip.size.1));
            assert!(pad >= flip.drop.0.max(flip.drop.1) + flip.drop.2);
            // the cards fill the floor of the recess but for its inset, and
            // their stacks fit under them
            let inset = flip.skin.int("Inset", 4);
            let end = flip.left(flip.cards.len() - 1) + flip.card_w;
            assert_eq!(end, flip.skin.int("CardsW", 0));
            assert_eq!(flip.cards_at.0 + end + inset, flip.floor.x1);
            assert_eq!(flip.cards_at.1 + flip.card_h + flip.stack + inset, flip.floor.y1);
            // a colon between every two cards, in the room left for it
            let colons: Vec<i32> = flip.colons().collect();
            assert_eq!(colons.len(), flip.cards.len() / 2 - 1);
            for (i, &left) in colons.iter().enumerate() {
                assert_eq!(left, flip.left(2 * i + 1) + flip.card_w + flip.gap);
            }
        }
    }

    #[test]
    fn a_colon_is_two_dots_on_the_floor_and_no_card() {
        let mut settings = Settings::in_memory();
        let flip = clock_at(&mut settings, 9, 41);
        let px = drawn(&flip);
        let at = |x: i32, y: i32| px[(y * flip.size.0 + x) as usize];
        let left = flip.cards_at.0 + flip.colons().next().unwrap();
        let (top, mid) = (flip.cards_at.1, flip.cards_at.0 + flip.colons().next().unwrap() + flip.colon_w / 2);
        // the floor shows above, between and below the dots...
        for row in [2, flip.half, flip.card_h - 3] {
            assert_eq!(at(mid, top + row), flip.floor_lit, "row {row}");
        }
        // ...and the dots are paper, lit from the upper left
        let (upper, lower) = (top + flip.half - DOT_FROM_SEAM - DOT + 1, top + flip.half + DOT_FROM_SEAM);
        for dot in [upper, lower] {
            let (lit, shaded) = (at(mid - 1, dot), at(mid, dot + DOT - 1));
            assert!(light(lit) > light(shaded) && light(shaded) > light(flip.floor_lit), "{lit:08x} {shaded:08x}");
        }
        // and every column of the colon's room is clear of a card
        for x in left..left + flip.colon_w {
            assert_ne!(at(x, top + 2), flip.paper.face(FRONT, 0));
        }
    }

    #[test]
    fn a_leaf_shortens_to_edge_on_and_lengthens_again_as_it_falls() {
        let flip = Flip::new(&Ctx { now: 0, settings: &mut Settings::in_memory(), locked: false, signed_in: None });
        let before: Vec<i32> = POSES.iter().filter(|p| p.angle < 90.0).map(|p| flip.rows_at(p.angle)).collect();
        let after: Vec<i32> = POSES.iter().filter(|p| p.angle > 90.0).map(|p| flip.rows_at(p.angle)).collect();
        assert!(before.windows(2).all(|w| w[0] > w[1]), "{before:?}");
        // down to the landing, and landed
        let coming = &after[..after.iter().position(|&r| r == flip.half).unwrap() + 1];
        assert!(coming.windows(2).all(|w| w[0] < w[1]), "{after:?}");
        assert!(POSES.iter().any(|p| p.angle == 90.0));
        // no pose is the one before it again, and every one is on the screen
        // for a frame at least
        assert!(POSES.windows(2).all(|w| (w[0].angle, w[0].behind, w[0].below) != (w[1].angle, w[1].behind, w[1].below)));
        assert!(POSES.iter().all(|p| p.hold >= FRAME_MS as i32));
        // and it is all over in a third of a second, near enough
        assert_eq!(POSES.iter().map(|p| p.hold).sum::<i32>(), 340);
    }

    #[test]
    fn the_next_leaf_comes_up_into_place_as_the_first_falls() {
        let mut settings = Settings::in_memory();
        let mut flip = turning(&mut settings, (10, 58), (10, 59));
        let (col, top) = (flip.cards_at.0 + flip.left(3) + 4, flip.cards_at.1);
        let head_at = |flip: &mut Flip, t: i32| {
            flip.cards[3].turn.as_mut().unwrap().t = t;
            drawn(flip)[(top * flip.size.0 + col) as usize]
        };
        // Leaning back, it stands short of where it will: the floor shows
        // over it at the start.  By the time the first leaf is half way
        // down, it stands against the tab, its head where a card's is.
        assert_eq!(head_at(&mut flip, 0), flip.floor_lit);
        let up: i32 = POSES.iter().take_while(|p| p.behind != 0.0).map(|p| p.hold).sum();
        let standing = head_at(&mut flip, up);
        flip.cards[3].turn = None;
        assert_eq!(standing, drawn(&flip)[(top * flip.size.0 + col) as usize]);
        // and leaning back, it faces up into the light
        let lean = POSES[0].behind.to_radians();
        assert!(facing(turned(FRONT, -lean)) > facing(FRONT));
    }

    #[test]
    fn the_leaves_below_are_pushed_back_by_the_landing_and_swing() {
        let mut settings = Settings::in_memory();
        let mut flip = turning(&mut settings, (10, 58), (10, 59));
        let (left, top) = (flip.cards_at.0 + flip.left(3), flip.cards_at.1);
        let (mid, face_row, foot, under) = (left + 4, top + flip.half + 13, top + flip.card_h - 1, top + flip.card_h + 1);
        let at_pose = |flip: &mut Flip, i: usize| {
            flip.cards[3].turn.as_mut().unwrap().t = POSES[..i].iter().map(|p| p.hold).sum();
            let (px, w) = (drawn(flip), flip.size.0);
            move |x: i32, y: i32| px[(y * w + x) as usize]
        };
        let landed = POSES.iter().position(|p| p.angle == 180.0).unwrap();
        let (pushed, swung) = (at_pose(&mut flip, landed), at_pose(&mut flip, landed + 1));
        flip.cards[3].turn = None;
        let px = drawn(&flip);
        let rest = |x: i32, y: i32| px[(y * flip.size.0 + x) as usize];
        // Pushed back, the bottom leaf turns from the light, and swung
        // forward it turns to it...
        assert!(light(pushed(mid, face_row)) < light(rest(mid, face_row)), "pushed back");
        assert!(light(swung(mid, face_row)) > light(rest(mid, face_row)), "swung forward");
        // ...and either way it shows a row shorter, the edges of the leaves
        // hanging behind it risen with it: the lowest is floor again
        for swinging in [&pushed, &swung] {
            assert_ne!(swinging(mid, foot), rest(mid, foot));
            assert_eq!((swinging(mid, under), rest(mid, under) != flip.floor_lit), (flip.floor_lit, true));
        }
    }

    #[test]
    fn a_minute_turns_only_the_cards_that_change_right_to_left() {
        let mut settings = Settings::in_memory();
        let mut flip = clock_at(&mut settings, 10, 59);
        let dirty = flip.tick(&mut Ctx { now: at(11, 0, 0), settings: &mut settings, locked: false, signed_in: None });
        let waits: Vec<Option<i32>> = flip.cards.iter().map(|c| c.turn.as_ref().map(|t| t.t)).collect();
        assert_eq!(waits, [None, Some(-160), Some(-80), Some(0)]);
        assert_eq!(flip.cards.iter().map(|c| c.shown).collect::<Vec<_>>(), [Some(1), Some(1), Some(0), Some(0)]);
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
        let all: i32 = POSES.iter().map(|p| p.hold).sum();
        let frames = |ms: i32| (ms + FRAME_MS as i32 - 1) / FRAME_MS as i32;
        assert_eq!(landed, [0, frames(all + 160), frames(all + 80), frames(all)]);
        assert_eq!((flip.frame_ms(), flip.frame(FRAME_MS)), (0, Dirty::None));
    }

    #[test]
    fn a_landed_leaf_with_nothing_swinging_is_the_card_as_it_stands() {
        let mut settings = Settings::in_memory();
        let flip = clock_at(&mut settings, 10, 59);
        let (w, h) = flip.size;
        let card = |draw: &dyn Fn(&mut Canvas)| {
            let mut px = vec![0u32; (w * h) as usize];
            // SAFETY: the canvas is exactly the vector.
            let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), w, w, h) };
            draw(&mut canvas);
            px
        };
        let (x, y) = flip.cards_at;
        let at_rest = Pose { angle: 180.0, hold: 0, shadow: 0, behind: 0.0, below: 0.0 };
        let landed = card(&|c| {
            flip.behind(c, Some(9), Some(8), &at_rest, x, y);
            flip.leaf(c, Some(9), Some(8), &at_rest, x, y);
        });
        assert_eq!(landed, card(&|c| flip.still(c, Some(9), x, y)));
        // and part way down it is something else
        assert_ne!(landed, card(&|c| flip.behind(c, Some(9), Some(8), &POSES[0], x, y)));
    }

    #[test]
    fn edge_on_the_leaf_stands_out_beside_its_card() {
        let mut settings = Settings::in_memory();
        let mut flip = turning(&mut settings, (10, 58), (10, 59));
        let edge: i32 = POSES.iter().take_while(|p| p.angle != 90.0).map(|p| p.hold).sum();
        flip.cards[3].turn.as_mut().unwrap().t = edge;
        let px = drawn(&flip);
        let at = |x: i32, y: i32| px[(y * flip.size.0 + x) as usize];
        let (left, top) = (flip.cards_at.0 + flip.left(3), flip.cards_at.1);
        // two px out to the left, on the row over the seam, is the leaf's
        // back, turned up to the light, and under it the seam, both standing
        // out over the gap
        assert_eq!(at(left - 2, top + flip.half - 1), flip.paper.lit([0.0, -1.0, 0.0], 0.0, 0));
        assert_eq!(at(left - 2, top + flip.half), flip.seam);
        // and where no leaf is, the gap shows the floor of the recess
        assert_eq!(at(left - 2, top + 2), flip.floor_lit);
    }

    #[test]
    fn a_falling_leaf_turns_from_the_light_and_its_back_comes_down_in_it() {
        // Hinged at its foot and falling towards the eye, like the flap of a
        // letterbox, its face turns to the floor, away from the light above...
        let face = |angle: f32| facing(turned(FRONT, angle.to_radians()));
        assert!(face(0.0) > face(35.0) && face(35.0) > face(75.0));
        assert!(turned(FRONT, (90f32).to_radians())[1] > 0.99, "edge-on, its face is to the floor");
        // ...and its back, which then faces up, is in the light coming down,
        // and settles to the light a card standing has
        let back = |angle: f32| facing(turned(FRONT, angle.to_radians() - core::f32::consts::PI));
        assert!(back(110.0) > back(180.0) && back(140.0) > back(180.0));
        assert!((back(180.0) - facing(FRONT)).abs() < 1e-6);
    }

    #[test]
    fn a_turn_the_frames_never_came_for_lands_at_the_next_tick() {
        let mut settings = Settings::in_memory();
        let mut flip = turning(&mut settings, (10, 58), (10, 59));
        assert!(flip.cards[3].turn.is_some());
        let dirty = flip.tick(&mut Ctx { now: at(10, 59, 1), settings: &mut settings, locked: false, signed_in: None });
        assert!(flip.cards.iter().all(|c| c.turn.is_none()));
        assert_eq!(dirty, Dirty::Part(flip.card_rect(3)));
    }

    #[test]
    fn the_hours_are_the_date_time_widgets() {
        let mut settings = Settings::in_memory();
        let flip = clock_at(&mut settings, 9, 5);
        assert_eq!((flip.cards.iter().map(|c| c.shown).collect::<Vec<_>>(), flip.ampm), (vec![Some(0), Some(9), Some(0), Some(5)], ""));
        assert_eq!(flip.date, "Saturday, 21 November");

        settings.set_int("DateTime", "Use24Hour", 0);
        let mut flip = Flip::new(&Ctx { now: at(9, 5, 0), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((flip.cards.iter().map(|c| c.shown).collect::<Vec<_>>(), flip.ampm), (vec![None, Some(9), Some(0), Some(5)], "AM"));
        let dirty = flip.tick(&mut Ctx { now: at(21, 5, 0), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((flip.ampm, dirty), ("PM", Dirty::All));
        assert_eq!(figures(&clock::local(at(0, 7, 0)), false, false), [Some(1), Some(2), Some(0), Some(7)]);
        assert_eq!(figures(&clock::local(at(12, 30, 0)), false, false), [Some(1), Some(2), Some(3), Some(0)]);
        // with the seconds on, the last two cards are the seconds
        assert_eq!(figures(&clock::local(at(12, 30, 8)), true, true), [Some(1), Some(2), Some(3), Some(0), Some(0), Some(8)]);
    }

    #[test]
    fn the_seconds_are_a_menu_tick_that_makes_the_clock_wider() {
        let mut settings = Settings::in_memory();
        let mut flip = Flip::new(&Ctx { now: at(9, 41, 0), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((flip.cards.len(), flip.seconds, flip.skin.int("CardsW", 0)), (4, false, 160));
        assert!(core::ptr::eq(flip.skin, &skin::flip::SKIN));
        let mut items = Vec::new();
        flip.menu(&mut items);
        assert_eq!(items.iter().filter(|i| i.id == MENU_SECONDS).map(|i| i.checked).collect::<Vec<_>>(), [false]);

        // the tick is kept in settings.ini, and the window made again for it
        let outcome = flip.menu_command(MENU_SECONDS, &mut Ctx { now: at(9, 41, 0), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!(outcome.request, Request::Reshape);
        assert_eq!(settings.text(FIRST, "Seconds", ""), "1");

        // made again as settings.ini now says, it is six cards on the wider
        // skin, the last two the seconds
        let mut wide = Flip::new(&Ctx { now: at(9, 41, 7), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((wide.cards.len(), wide.seconds, wide.skin.int("CardsW", 0)), (6, true, 249));
        assert!(core::ptr::eq(wide.skin, &skin::flip_secs::SKIN));
        assert!(wide.size.0 > flip.size.0 && wide.size.1 == flip.size.1);
        assert_eq!(wide.cards.iter().map(|c| c.shown).collect::<Vec<_>>(), [Some(0), Some(9), Some(4), Some(1), Some(0), Some(7)]);
        // and the seconds turn over on their own, one card at a time
        let dirty = wide.tick(&mut Ctx { now: at(9, 41, 8), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!(wide.cards[5].turn.as_ref().map(|t| t.from), Some(Some(7)));
        assert_eq!(dirty, Dirty::Part(wide.card_rect(5)));

        // ticking it back off puts the four-card clock back
        wide.menu_command(MENU_SECONDS, &mut Ctx { now: at(9, 41, 8), settings: &mut settings, locked: false, signed_in: None });
        assert_eq!(settings.text(FIRST, "Seconds", ""), "0");
        assert!(core::ptr::eq(skin_of(&settings, FIRST), &skin::flip::SKIN));
    }

    #[test]
    fn the_case_is_lit_from_the_upper_left_and_casts_a_shadow() {
        let mut settings = Settings::in_memory();
        let flip = clock_at(&mut settings, 9, 41);
        let px = drawn(&flip);
        let at = |x: i32, y: i32| px[(y * flip.size.0 + x) as usize];
        let (c, mid_x, mid_y) = (flip.case, (flip.case.x0 + flip.case.x1) / 2, (flip.case.y0 + flip.case.y1) / 2);
        // the rim catches the light along its top and left, and turns from
        // it along its foot and right
        assert!(light(at(mid_x, c.y0 + 1)) > light(at(mid_x, c.y1 - 2)));
        assert!(light(at(c.x0 + 1, mid_y)) > light(at(c.x1 - 2, mid_y)));
        // the recess's wall is in shade along its head and lit along its foot
        let r = flip.recess;
        assert!(light(at(mid_x, r.y0)) < light(at(mid_x, r.y1 - 1)));
        // its floor is dark, and darker still in the walls' shadow
        assert_eq!((at(flip.floor.x0, flip.floor.y0 + 20), at(flip.floor.x1 - 1, flip.floor.y0 + 20)), (flip.floor_cast, flip.floor_lit));
        assert!(light(flip.floor_lit) < light(at(mid_x, c.y0 + 1)) / 2);
        // The case is solid; its shadow falls down and to the right of it
        // on the desktop, fading, and none above it or to its left.
        assert_eq!(at(mid_x, mid_y) >> 24, 255);
        let (near, far) = (at(mid_x, c.y1) >> 24, at(mid_x, c.y1 + flip.drop.1 + 1) >> 24);
        assert!(near > far && far > 0 && near < 255, "near {near} far {far}");
        assert_eq!(at(mid_x, c.y1 + flip.drop.1 + flip.drop.2 + 1), 0);
        assert_eq!((at(mid_x, c.y0 - 1), at(c.x0 - 1, mid_y)), (0, 0));
    }

    #[test]
    fn the_paper_is_lit_from_the_upper_left() {
        let mut settings = Settings::in_memory();
        let flip = clock_at(&mut settings, 9, 41);
        let px = drawn(&flip);
        let at = |x: i32, y: i32| px[(y * flip.size.0 + x) as usize];
        let (left, top, cw, half) = (flip.cards_at.0 + flip.left(0), flip.cards_at.1, flip.card_w, flip.half);
        // four px in from the left-hand edge, clear of the figure: the head
        // of a leaf catches the light, its face less, its foot least -- and
        // the seam is a groove, shade over it and light under it
        let col = left + 4;
        let (head, face, foot, under) = (at(col, top), at(col, top + 6), at(col, top + half - 1), at(col, top + half + 1));
        assert!(light(head) > light(face), "head {head:08x} face {face:08x}");
        assert!(light(face) > light(foot), "face {face:08x} foot {foot:08x}");
        assert!(light(under) > light(foot), "under {under:08x} foot {foot:08x}");
        assert_eq!(at(col, top + half), flip.seam);
        // across the card, its left-hand edge is in the light and its
        // right-hand edge in shade
        assert!(light(at(left, top + 8)) > light(at(left + cw - 1, top + 8)));
        // its corners are rounded off: the floor shows there
        assert!(light(at(left, top)) < light(face) / 2);
        // and under it the fallen leaves' edges, then the floor
        assert!(light(at(left + cw / 2, top + flip.card_h)) > light(flip.floor_lit));
    }
}
