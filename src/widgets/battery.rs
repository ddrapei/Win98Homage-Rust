//! Battery: how much charge is left, as a battery cell standing on end, seen
//! from a little above, its side sections (rings, in the code) that light up
//! from the bottom -- red at the foot, through orange and yellow, to green at
//! the top -- with the figure under it, where the power is coming from, and
//! what the battery is doing.
//!
//! Windows is asked once a second.  That is one call, which answers from what
//! the power manager already knows, and nothing is repainted unless something
//! shown has changed.  The sections move in whole steps and the figure in
//! whole percents, so a battery that is running down repaints a few times an
//! hour.
//!
//! The battery is drawn as pixel art draws a cylinder, like the disk pie: a
//! pixel at a time, each one part of the picture or none of it.  Its top is
//! an ellipse of flat metal with the terminal standing up from it; down its
//! side are metal bands at the top and the foot and the sections between,
//! shaded in stripes down its length, lit from the left; and every edge
//! across it follows the ellipse's front, so they curve round it.  Nothing is
//! outlined, and nothing is drawn outside the battery.  The sections stand
//! apart, with the battery's narrower core showing between them.  Those not
//! lit are dark grey, like the segments of a bar of lights that are off.
//! The picture is worked out from the skin's figures and colours whenever
//! the sections that are lit change, and kept.

use crate::canvas::Canvas;
use crate::skin::{self, Hooks, Meter, Skin};
use crate::sys;
use crate::text::Text;
use crate::widget::{Ctx, Dirty, Outcome, Request, Widget};

static SKIN: &Skin = &skin::battery::SKIN;

// BatteryFlag's bits, and what the first three fields say when Windows does
// not know.
const CHARGING: u8 = 8;
const NO_BATTERY: u8 = 128;
const UNKNOWN: u8 = 255;
/// An estimate longer than this is no estimate: a battery that has only just
/// been unplugged can be said to last for years.
const LONGEST: u32 = 99 * 3600;

const WHITE: u32 = 0xFFFF_FFFF;
const BLACK: u32 = 0xFF00_0000;

/// Where the power is coming from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    NoBattery,
    Charging,
    PluggedIn,
    OnBattery,
}

/// What Windows said, made sense of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Power {
    state: State,
    /// 0 to 100; None with no battery, or from one that did not say.
    percent: Option<u8>,
    /// Seconds left, on battery, once Windows has worked it out.
    left: Option<u32>,
}

fn power(s: &sys::SYSTEM_POWER_STATUS) -> Power {
    let flag = s.BatteryFlag;
    let known = flag != UNKNOWN;
    let percent = (s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent);
    // A machine with no battery says so in the flag.  One that cannot read
    // the flag either and has no figure has nothing to show.
    let none = if known { flag & NO_BATTERY != 0 } else { percent.is_none() };
    let state = if none {
        State::NoBattery
    } else if known && flag & CHARGING != 0 {
        State::Charging
    } else if s.ACLineStatus == 1 {
        State::PluggedIn
    } else {
        State::OnBattery
    };
    Power {
        state,
        percent: percent.filter(|_| state != State::NoBattery),
        // u32::MAX, "not known", is past the longest too
        left: (state == State::OnBattery && s.BatteryLifeTime <= LONGEST).then_some(s.BatteryLifeTime),
    }
}

/// What Windows would say of a battery with these figures -- the charge,
/// plugged in, charging, seconds left -- or of none, for the test harness.
fn reported(reading: Option<(u8, bool, bool, Option<u32>)>) -> sys::SYSTEM_POWER_STATUS {
    let (ac, flag, percent, left) = match reading {
        None => (1, NO_BATTERY, UNKNOWN, None),
        Some((percent, ac, charging, left)) => (u8::from(ac), if charging { CHARGING } else { 0 }, percent, left),
    };
    sys::SYSTEM_POWER_STATUS {
        ACLineStatus: ac,
        BatteryFlag: flag,
        BatteryLifePercent: percent,
        SystemStatusFlag: 0,
        BatteryLifeTime: left.unwrap_or(u32::MAX),
        BatteryFullLifeTime: u32::MAX,
    }
}

/// The Status row's words: on battery, the time left, or "..." until Windows
/// has worked it out.  Plugged in -- and a machine with no battery is on the
/// mains -- the row shows the plug instead, and no words.
fn status_time(power: &Power) -> String {
    match power.state {
        State::OnBattery => power.left.map_or_else(|| String::from("..."), time_left),
        _ => String::new(),
    }
}

/// Rings lit, of `rings`: the charge to the nearest ring, but never none
/// while there is any, so a battery that is nearly flat still shows a ring
/// of red.
fn lit(percent: Option<u8>, rings: i32) -> i32 {
    match percent {
        None | Some(0) => 0,
        Some(p) => ((i32::from(p) * rings + 50) / 100).clamp(1, rings),
    }
}

/// "2h 15m", to the nearest minute: short, to fit the row beside its label.
fn time_left(seconds: u32) -> String {
    let minutes = (seconds.saturating_add(30) / 60).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// `to` laid `percent` of the way over `from`, both 0xAARRGGBB, opaque.
fn mix(from: u32, to: u32, percent: i32) -> u32 {
    let k = percent.clamp(0, 100) as u32;
    let channel = |shift: u32| ((from >> shift & 255) * (100 - k) + (to >> shift & 255) * k + 50) / 100 << shift;
    0xFF00_0000 | channel(16) | channel(8) | channel(0)
}

/// The colour `t` of the way up the battery, 0 at the foot and 1 at the top:
/// from `low` to `mid` over the lower half, and on to `high` over the upper.
fn up_the_battery(low: u32, mid: u32, high: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.5 {
        mix(low, mid, (t * 200.0).round() as i32)
    } else {
        mix(mid, high, ((t - 0.5) * 200.0).round() as i32)
    }
}

/// The stripes the side is shaded in down its length, lit from the left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stripe {
    Body,
    Lighter,
    Shine,
    Shade,
    Edge,
    /// The light thrown back at the right-hand edge.
    Rim,
}

/// The stripe a px is in, by how far across the battery it is: -1 at the
/// left-hand edge, 0 in the middle, 1 at the right.
fn stripe(u: f32) -> Stripe {
    match u {
        u if u < -0.82 => Stripe::Body,
        u if u < -0.55 => Stripe::Lighter,
        u if u < -0.25 => Stripe::Shine,
        u if u < 0.40 => Stripe::Body,
        u if u < 0.72 => Stripe::Shade,
        u if u < 0.90 => Stripe::Edge,
        _ => Stripe::Rim,
    }
}

/// One colour of the side, shaded for each stripe.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Shades {
    shine: u32,
    lighter: u32,
    body: u32,
    shade: u32,
    edge: u32,
    rim: u32,
}

impl Shades {
    fn at(&self, stripe: Stripe) -> u32 {
        match stripe {
            Stripe::Body => self.body,
            Stripe::Lighter => self.lighter,
            Stripe::Shine => self.shine,
            Stripe::Shade => self.shade,
            Stripe::Edge => self.edge,
            Stripe::Rim => self.rim,
        }
    }
}

/// How far a band's lip is from its own metal: towards white where it
/// catches the light, towards black where it steps in.
const LIP_LIGHT: i32 = 35;
const LIP_DARK: i32 = 30;
/// How far the terminal's shadow is from the top it falls on.
const SHADOW: i32 = 22;

/// A row of a metal band: its metal, or a lip catching the light or in the
/// shade.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lip {
    None,
    Light,
    Dark,
}

/// What a px of the battery is part of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    Out,
    /// The top's edge, behind its middle and in front of it, where the light
    /// catches it; and the flat of the top inside that.
    RimBack,
    RimFront,
    Face,
    /// The terminal standing up from the top: its own top, the glint on it,
    /// its side, and its shadow on the top.
    TermTop,
    Glint,
    TermSide,
    TermShadow,
    /// A row of the metal band round the top of the side or round its foot.
    Band(Lip),
    /// The section `i` from the bottom: its top row, catching the light, and
    /// the rest of it.
    RingTop(i32),
    Ring(i32),
    /// The battery's core, in the space between two sections.
    Core,
}

/// All that is drawn, as it is drawn: a reading that changes none of it
/// repaints nothing.
#[derive(Default, PartialEq, Debug)]
struct Shown {
    percent: String,
    /// The Status row: the plug, unless it is on battery, when it is the
    /// time left; and the bolt, on the battery and beside the plug, while
    /// it charges.
    on_battery: bool,
    time: String,
    charging: bool,
    /// Sections lit, of `rings`.
    lit: i32,
}

pub struct Battery {
    shown: Shown,
    /// The battery as the skin lays it out: its size, how far the top's
    /// ellipse reaches from its middle to its front; the terminal's width,
    /// its own ellipse's reach and its height; the sections down the side --
    /// how many, how tall, and the space before each -- the band at the
    /// foot, and how far in from the edge the core is in the spaces.
    w: i32,
    h: i32,
    ry: i32,
    term_w: i32,
    term_ry: i32,
    term_h: i32,
    rings: i32,
    ring_h: i32,
    gap: i32,
    band_foot: i32,
    core_inset: i32,
    /// The colours: each section's, from the bottom one up, lit; a section
    /// not lit; the metal, and its glint; how far towards black the core is
    /// from the metal, and towards white a section's top row from the rest.
    lit_rings: Vec<Shades>,
    off: Shades,
    metal: Shades,
    glint: u32,
    core_shade: i32,
    bevel: i32,
    /// The picture, a px at a time, row after row: a colour where the
    /// battery is, and 0 where it is not.
    picture: Vec<u32>,
    /// For the test harness: what it was told to show, whatever the machine has.
    frozen: bool,
}

impl Battery {
    pub fn new(_ctx: &Ctx) -> Battery {
        let w = SKIN.int("BattW", 36).max(2);
        let mut battery = Battery {
            // the skin's own defaults, until the first tick
            shown: Shown { percent: String::from("--"), ..Shown::default() },
            w,
            h: SKIN.int("BattH", 88).max(2),
            ry: SKIN.int("BattRY", 8).clamp(1, w),
            term_w: SKIN.int("TermW", 12).clamp(2, w),
            term_ry: SKIN.int("TermRY", 3).max(1),
            term_h: SKIN.int("TermH", 4).max(0),
            rings: SKIN.int("Cells", 5).max(1),
            ring_h: SKIN.int("CellH", 8).max(1),
            gap: SKIN.int("CellGap", 3).max(0),
            band_foot: SKIN.int("BandFoot", 6).max(0),
            core_inset: SKIN.int("CoreInset", 3).clamp(0, w / 2 - 1),
            lit_rings: Vec::new(),
            off: Shades::default(),
            metal: Shades::default(),
            glint: 0,
            core_shade: 0,
            bevel: 0,
            picture: Vec::new(),
            frozen: false,
        };
        battery.retint();
        battery
    }

    /// For the test harness: a battery with these figures -- the charge,
    /// plugged in, charging, seconds left -- or none, never the machine's.
    pub fn set_fixed(&mut self, reading: Option<(u8, bool, bool, Option<u32>)>) {
        self.frozen = true;
        self.show(power(&reported(reading)));
    }

    /// Work out what to show.  Says whether that is any different.
    fn show(&mut self, power: Power) -> bool {
        let shown = Shown {
            percent: power.percent.map_or_else(|| String::from("--"), |p| format!("{p}%")),
            on_battery: power.state == State::OnBattery,
            time: status_time(&power),
            charging: power.state == State::Charging,
            lit: lit(power.percent, self.rings),
        };
        if shown == self.shown {
            return false;
        }
        let repaint = shown.lit != self.shown.lit;
        self.shown = shown;
        if repaint {
            self.paint();
        }
        true
    }

    /// How far down the picture the middle of the top is: far enough for
    /// the top's own back edge, and for the terminal standing on it.
    fn top_y(&self) -> i32 {
        self.ry.max(self.term_h + self.term_ry)
    }

    /// How long the side is, from the front of the top's edge to the front
    /// of the foot.
    fn side(&self) -> i32 {
        self.h - self.top_y() - self.ry
    }

    /// The metal bands round the top of the side and round its foot: the
    /// foot's as the skin has it, the top's what the sections, the space
    /// before each and the space after the last, and the foot leave.
    fn bands(&self) -> (i32, i32) {
        let rings = self.rings * (self.gap + self.ring_h) + self.gap;
        let room = (self.side() - rings).max(0);
        let foot = self.band_foot.min(room);
        (room - foot, foot)
    }

    /// What the px at (x, y) of the battery is part of.  The top is an
    /// ellipse as wide as the battery, with the terminal standing on it;
    /// below the top's front edge each column of the side runs down
    /// `side()` px, so every edge across the side -- the bands', each
    /// ring's, the foot -- is that same front edge lower down.
    fn part(&self, x: i32, y: i32) -> Part {
        let (rx, ry) = (self.w as f32 / 2.0, self.ry as f32);
        let (dx, dy) = (x as f32 + 0.5 - rx, y as f32 + 0.5 - self.top_y() as f32);
        let u = dx / rx;
        if u.abs() >= 1.0 {
            return Part::Out;
        }
        // the terminal stands in front of the back of the top
        if let Some(part) = self.terminal(dx, dy) {
            return part;
        }
        if u * u + (dy / ry) * (dy / ry) <= 1.0 {
            return self.top(dx, dy);
        }
        // how far down the side, from the top's front edge in this column
        let z = dy - ry * (1.0 - u * u).sqrt();
        let side = self.side() as f32;
        if z < 0.0 || z >= side {
            return Part::Out;
        }
        let (top, foot) = self.bands();
        let (top, foot) = (top as f32, foot as f32);
        if z < top {
            // stepping in to the rings at its lower edge
            return Part::Band(if z >= top - 1.0 { Lip::Dark } else { Lip::None });
        }
        if z >= side - foot {
            // catching the light along its upper edge, in the shade along its lower
            let k = z - (side - foot);
            return Part::Band(if k < 1.0 { Lip::Light } else if k >= foot - 1.0 { Lip::Dark } else { Lip::None });
        }
        // a space and a section, a space and a section, ... and a space
        let (gap, period) = (self.gap as f32, (self.gap + self.ring_h) as f32);
        let k = z - top;
        let from_top = (k / period) as i32;
        let into = k - from_top as f32 * period;
        if into < gap || from_top >= self.rings {
            // the core, narrower than the battery: past it is the face
            let core = rx - self.core_inset as f32;
            return if dx.abs() < core { Part::Core } else { Part::Out };
        }
        let i = self.rings - 1 - from_top;
        if into < gap + 1.0 { Part::RingTop(i) } else { Part::Ring(i) }
    }

    /// The terminal, by how far a px is from the middle of the top: a little
    /// cylinder standing `term_h` up from it, its top an ellipse with the
    /// glint on its left, the light being from there.  None for a px that is
    /// not on it.
    fn terminal(&self, dx: f32, dy: f32) -> Option<Part> {
        let (a, b, up) = (self.term_w as f32 / 2.0, self.term_ry as f32, self.term_h as f32);
        let v = dx / a;
        if v.abs() >= 1.0 {
            return None;
        }
        let over = (dy + up) / b;
        if v * v + over * over <= 1.0 {
            let glint = (-0.75..-0.2).contains(&v) && over < -0.05;
            return Some(if glint { Part::Glint } else { Part::TermTop });
        }
        // between its top and where it stands, down to the front of its foot
        (dy >= -up && dy <= b * (1.0 - v * v).sqrt()).then_some(Part::TermSide)
    }

    /// The top, by how far a px is from its middle: flat metal, with its
    /// edge -- the light catching it in front -- and the terminal's shadow
    /// falling to the right of it and a little back, the light being from
    /// the left and in front.
    fn top(&self, dx: f32, dy: f32) -> Part {
        let (rx, ry) = (self.w as f32 / 2.0, self.ry as f32);
        if (dx / (rx - 2.0)).powi(2) + (dy / (ry - 1.0)).powi(2) > 1.0 {
            return if dy > 0.0 { Part::RimFront } else { Part::RimBack };
        }
        let (a, b) = (self.term_w as f32 / 2.0, self.term_ry as f32);
        if dx > 0.0 && ((dx - 0.5 * a) / a).powi(2) + ((dy + 1.0) / b).powi(2) <= 1.0 {
            return Part::TermShadow;
        }
        Part::Face
    }

    /// The colour of a part of the battery, in the stripe `u` across it.
    fn colour(&self, part: Part, u: f32) -> u32 {
        match part {
            Part::Out => 0,
            Part::RimBack => self.metal.body,
            Part::RimFront | Part::TermTop => self.metal.shine,
            Part::Face => self.metal.lighter,
            Part::Glint => self.glint,
            // shaded across the terminal, as the side is across the battery
            Part::TermSide => self.metal.at(stripe(u * self.w as f32 / self.term_w as f32)),
            Part::TermShadow => mix(self.metal.lighter, BLACK, SHADOW),
            Part::Band(lip) => {
                let metal = self.metal.at(stripe(u));
                match lip {
                    Lip::None => metal,
                    Lip::Light => mix(metal, WHITE, LIP_LIGHT),
                    Lip::Dark => mix(metal, BLACK, LIP_DARK),
                }
            }
            Part::RingTop(i) => mix(self.ring(i).at(stripe(u)), WHITE, self.bevel),
            Part::Ring(i) => self.ring(i).at(stripe(u)),
            // striped across the core, which is narrower than the battery
            Part::Core => {
                let half = self.w as f32 / 2.0;
                let across = u * half / (half - self.core_inset as f32);
                mix(self.metal.at(stripe(across)), BLACK, self.core_shade)
            }
        }
    }

    /// The ring `i`'s colours, as it is now: lit, or not.
    fn ring(&self, i: i32) -> &Shades {
        if i < self.shown.lit { &self.lit_rings[i as usize] } else { &self.off }
    }

    /// Work the picture out again: the colour of every px of the battery.
    fn paint(&mut self) {
        let half = self.w as f32 / 2.0;
        self.picture = (0..self.w * self.h)
            .map(|n| {
                let x = n % self.w;
                self.colour(self.part(x, n / self.w), (x as f32 + 0.5 - half) / half)
            })
            .collect();
    }

    /// [MeterBattery], with its corner at `x`, `y`: a run of one colour at a
    /// time, and nothing where the battery is not.
    fn draw(&self, x: i32, y: i32, canvas: &mut Canvas) {
        for (row, line) in self.picture.chunks_exact(self.w as usize).enumerate() {
            let mut from = 0;
            while from < line.len() {
                let c = line[from];
                let run = line[from..].iter().take_while(|&&p| p == c).count();
                if c != 0 {
                    canvas.fill(x + from as i32, y + row as i32, run as i32, 1, c);
                }
                from += run;
            }
        }
    }
}

impl Hooks for Battery {
    fn num(&self, var: &str) -> f64 {
        match var {
            "Charging" => f64::from(u8::from(self.shown.charging)),
            "OnBattery" => f64::from(u8::from(self.shown.on_battery)),
            _ => 0.0,
        }
    }

    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "BattPct" => &self.shown.percent,
            "BattTime" => &self.shown.time,
            _ => "",
        });
    }

    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        if meter.name == "MeterBattery" {
            self.draw(x, y, canvas);
        }
    }
}

impl Widget for Battery {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Battery\n\n\
         How much charge the battery has left.  The sections run from red at \
         the foot of the battery, through yellow, to green at the top: the \
         less is left, the redder it is.\n\n\
         Status shows a plug while it is plugged in, with a bolt beside it \
         while it charges -- the bolt is on the battery too.  On battery it \
         shows the time left, Windows' own estimate, once it has made one."
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> Dirty {
        if self.frozen {
            return Dirty::None;
        }
        let mut status = sys::SYSTEM_POWER_STATUS::default();
        // SAFETY: the call fills in the one struct it is handed.
        if unsafe { sys::GetSystemPowerStatus(&mut status) } == 0 {
            return Dirty::None;
        }
        if self.show(power(&status)) { Dirty::All } else { Dirty::None }
    }

    fn command(&mut self, cmd: &str, _arg: i32, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        match cmd {
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn retint(&mut self) {
        // every one of them is untinted, and read again all the same, as
        // every colour taken from a skin is
        let pct = |name: &str, default: i32| SKIN.int(name, default);
        let (shine, lighter, shade, edge, rim) = (pct("CellShine", 55), pct("CellLight", 25), pct("CellShade", 22), pct("CellEdge", 42), pct("CellRim", 14));
        let shades = |body: u32, shine: i32, lighter: i32| Shades {
            shine: mix(body, WHITE, shine),
            lighter: mix(body, WHITE, lighter),
            body,
            shade: mix(body, BLACK, shade),
            edge: mix(body, BLACK, edge),
            rim: mix(body, BLACK, rim),
        };
        let (low, mid, high) = (SKIN.colour("BattLow"), SKIN.colour("BattMid"), SKIN.colour("BattHigh"));
        let last = (self.rings - 1).max(1) as f32;
        self.lit_rings = (0..self.rings).map(|i| shades(up_the_battery(low, mid, high, i as f32 / last), shine, lighter)).collect();
        self.off = shades(SKIN.colour("CellOff"), pct("OffShine", 16), pct("OffLight", 7));
        // the cap's metal is the chrome's greys
        let (hilite, light, shadow) = (SKIN.colour("Hilite"), SKIN.colour("Light"), SKIN.colour("Shadow"));
        self.metal = Shades {
            shine: hilite,
            lighter: mix(light, hilite, 50),
            body: light,
            shade: mix(light, shadow, 50),
            edge: shadow,
            rim: mix(shadow, light, 45),
        };
        self.glint = mix(hilite, WHITE, 60);
        self.core_shade = pct("CoreShade", 25);
        self.bevel = pct("CellBevel", 30);
        self.paint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn battery(settings: &mut Settings) -> Battery {
        Battery::new(&Ctx { now: 1_000, settings, locked: false, signed_in: None })
    }

    fn st(ac: u8, flag: u8, percent: u8, left: u32) -> sys::SYSTEM_POWER_STATUS {
        sys::SYSTEM_POWER_STATUS { ACLineStatus: ac, BatteryFlag: flag, BatteryLifePercent: percent, BatteryLifeTime: left, ..Default::default() }
    }

    #[test]
    fn what_windows_says_is_made_sense_of() {
        let p = |state, percent, left| Power { state, percent, left };
        // a desktop
        assert_eq!(power(&st(1, NO_BATTERY, UNKNOWN, u32::MAX)), p(State::NoBattery, None, None));
        // charging, and plugged in with nothing more to take
        assert_eq!(power(&st(1, CHARGING | 1, 76, u32::MAX)), p(State::Charging, Some(76), None));
        assert_eq!(power(&st(1, 1, 100, u32::MAX)), p(State::PluggedIn, Some(100), None));
        // on battery, with an estimate and before there is one
        assert_eq!(power(&st(0, 1, 76, 8100)), p(State::OnBattery, Some(76), Some(8100)));
        assert_eq!(power(&st(0, 2, 15, u32::MAX)), p(State::OnBattery, Some(15), None));
        // an estimate of years is none
        assert_eq!(power(&st(0, 1, 90, LONGEST + 1)).left, None);
        // a battery with no figure, and a machine that knows nothing at all
        assert_eq!(power(&st(0, 0, UNKNOWN, u32::MAX)), p(State::OnBattery, None, None));
        assert_eq!(power(&st(UNKNOWN, UNKNOWN, UNKNOWN, u32::MAX)).state, State::NoBattery);
        assert_eq!(power(&st(0, UNKNOWN, 40, u32::MAX)), p(State::OnBattery, Some(40), None));
        // the harness's figures are read the same way
        assert_eq!(power(&reported(None)).state, State::NoBattery);
        assert_eq!(power(&reported(Some((35, true, true, None)))), p(State::Charging, Some(35), None));
        assert_eq!(power(&reported(Some((12, false, false, Some(1500))))), p(State::OnBattery, Some(12), Some(1500)));
    }

    #[test]
    fn the_status_row_has_the_plug_or_the_time_left() {
        let mut settings = Settings::in_memory();
        let mut b = battery(&mut settings);
        // (on battery, so no plug; the words; the bolt)
        let mut row = |reading| {
            b.set_fixed(reading);
            (b.num("OnBattery"), b.shown.time.clone(), b.num("Charging"))
        };
        // on battery: the time left, or "..." until Windows has worked it out
        assert_eq!(row(Some((76, false, false, Some(8100)))), (1.0, "2h 15m".into(), 0.0));
        assert_eq!(row(Some((76, false, false, None))), (1.0, "...".into(), 0.0));
        // plugged in: the plug, with the bolt while it charges, and no words
        assert_eq!(row(Some((35, true, true, None))), (0.0, String::new(), 1.0));
        assert_eq!(row(Some((100, true, false, None))), (0.0, String::new(), 0.0));
        // held short of full, as a laptop that looks after its battery does
        assert_eq!(row(Some((80, true, false, None))), (0.0, String::new(), 0.0));
        // no battery: on the mains, so the plug
        assert_eq!(row(None), (0.0, String::new(), 0.0));
    }

    #[test]
    fn the_charge_to_the_nearest_section_and_any_charge_lights_one() {
        let five = |p: u8| lit(Some(p), 5);
        assert_eq!(lit(None, 5), 0);
        assert_eq!([five(0), five(1), five(10), five(29), five(30), five(49)], [0, 1, 1, 1, 2, 2]);
        assert_eq!([five(50), five(89), five(90), five(100)], [3, 4, 5, 5]);
        // and as many sections as the skin has
        let ten = |p: u8| lit(Some(p), 10);
        assert_eq!([ten(4), ten(5), ten(15), ten(94), ten(95)], [1, 1, 2, 9, 10]);
    }

    #[test]
    fn the_time_left_is_in_hours_and_minutes_short() {
        assert_eq!(time_left(8100), "2h 15m");
        assert_eq!(time_left(7200), "2h");
        // to the nearest minute
        assert_eq!(time_left(7170), "2h");
        assert_eq!(time_left(45 * 60 + 29), "45m");
        assert_eq!(time_left(LONGEST), "99h");
        // and never none
        assert_eq!(time_left(0), "1m");
    }

    #[test]
    fn the_colours_run_from_red_at_the_foot_to_green_at_the_top() {
        assert_eq!(mix(0xFF00_0000, 0xFFC8_6432, 50), 0xFF64_3219);
        let (low, mid, high) = (0xFFE2_2824, 0xFFFA_CC2C, 0xFF38_B448);
        assert_eq!([up_the_battery(low, mid, high, 0.0), up_the_battery(low, mid, high, 0.5), up_the_battery(low, mid, high, 1.0)], [low, mid, high]);
        let mut settings = Settings::in_memory();
        let b = battery(&mut settings);
        let rgb = |c: u32| (c >> 16 & 255, c >> 8 & 255, c & 255);
        let (bottom, top) = (rgb(b.lit_rings[0].body), rgb(b.lit_rings[b.rings as usize - 1].body));
        assert!(bottom.0 > 2 * bottom.1 && top.1 > 2 * top.0, "bottom {bottom:?}, top {top:?}");
        // and every ring is lighter at its shine than at its edge
        assert!(b.lit_rings.iter().all(|s| rgb(s.shine).1 > rgb(s.body).1 && rgb(s.edge).1 < rgb(s.body).1));
    }

    #[test]
    fn the_figures_are_shown_and_the_same_again_is_nothing_new() {
        let mut settings = Settings::in_memory();
        let mut b = battery(&mut settings);
        let read = |b: &Battery, var: &str| {
            let mut s = String::new();
            b.text(var, &mut s);
            s
        };

        b.set_fixed(Some((76, false, false, Some(8100))));
        assert_eq!((read(&b, "BattPct"), read(&b, "BattTime")), ("76%".into(), "2h 15m".into()));
        assert_eq!((b.shown.lit, b.num("Charging")), (4, 0.0));
        assert!(!b.show(power(&reported(Some((76, false, false, Some(8100)))))));

        b.set_fixed(Some((35, true, true, None)));
        assert_eq!((read(&b, "BattPct"), read(&b, "BattTime")), ("35%".into(), String::new()));
        assert_eq!((b.shown.lit, b.num("Charging")), (2, 1.0));

        b.set_fixed(Some((12, false, false, Some(1500))));
        assert_eq!((b.shown.lit, read(&b, "BattTime")), (1, "25m".into()));

        b.set_fixed(None);
        assert_eq!((read(&b, "BattPct"), read(&b, "BattTime"), b.shown.lit, b.num("Charging")), ("--".into(), String::new(), 0, 0.0));
    }

    /// The parts down one column of the battery, top to bottom and a row
    /// past its foot, each once.
    fn down(b: &Battery, x: i32) -> Vec<Part> {
        let mut parts: Vec<Part> = (0..=b.h).map(|y| b.part(x, y)).collect();
        parts.dedup();
        parts
    }

    #[test]
    fn metal_bands_at_either_end_and_the_sections_standing_apart_between_them() {
        let mut settings = Settings::in_memory();
        let b = battery(&mut settings);
        // down the middle: the top with the terminal standing on it, its
        // front edge, the band at the top, the sections with the core in the
        // space before each and after the last, each section's top row
        // catching the light, and the band at the foot, light along its
        // upper edge and dark along its lower
        let middle = down(&b, b.w / 2);
        let band = middle.iter().position(|p| matches!(p, Part::Band(_))).expect("a band");
        assert_eq!(middle[band - 1], Part::RimFront);
        for part in [Part::TermTop, Part::TermSide, Part::Face] {
            assert!(middle[..band].contains(&part), "no {part:?} down the middle: {middle:?}");
        }
        let mut side = vec![Part::Band(Lip::None), Part::Band(Lip::Dark)];
        for i in (0..b.rings).rev() {
            side.extend([Part::Core, Part::RingTop(i), Part::Ring(i)]);
        }
        side.extend([Part::Core, Part::Band(Lip::Light), Part::Band(Lip::None), Part::Band(Lip::Dark), Part::Out]);
        assert_eq!(&middle[band..], &side[..]);
        // the edges across the side are lower in the middle than at the sides
        let first = |x: i32, part: Part| (0..b.h).find(|&y| b.part(x, y) == part).unwrap();
        assert!(first(b.w / 2, Part::Ring(0)) > first(1, Part::Ring(0)) + 3);
        assert_eq!((b.part(-1, b.h / 2), b.part(b.w, b.h / 2), b.part(b.w / 2, b.h)), (Part::Out, Part::Out, Part::Out));
        // In the spaces the core is narrower than the battery: its edge steps
        // in there, and the face shows -- space, not a line drawn across.
        // At either edge, between one section and the next, is face.
        for x in [0, 1, b.w - 2, b.w - 1] {
            let column = down(&b, x);
            let at = |part: Part| column.iter().position(|&p| p == part).unwrap();
            assert_eq!(&column[at(Part::Ring(1)) + 1..at(Part::RingTop(0))], &[Part::Out], "column {x}: {column:?}");
        }
        // and in the middle, the core
        assert_eq!(first(b.w / 2, Part::Core) + b.gap, first(b.w / 2, Part::RingTop(b.rings - 1)));
    }

    #[test]
    fn the_terminal_stands_up_and_the_top_is_lit_as_the_rest() {
        let mut settings = Settings::in_memory();
        let b = battery(&mut settings);
        let every = || (0..b.h).flat_map(|y| (0..b.w).map(move |x| (x, y)));
        let all = |part: Part| every().filter(|&(x, y)| b.part(x, y) == part).collect::<Vec<_>>();
        // the terminal's top is the highest thing on the battery, standing up
        // past the top's back edge; its side shows under it, and the glint
        // is on its left
        let (sides, glints) = (all(Part::TermSide), all(Part::Glint));
        let top_row = every().find(|&(x, y)| b.part(x, y) != Part::Out).unwrap().1;
        let on_top: Vec<Part> = (0..b.w).map(|x| b.part(x, top_row)).filter(|&p| p != Part::Out).collect();
        assert!(on_top.iter().all(|&p| p == Part::TermTop || p == Part::Glint), "the terminal is not on top: {on_top:?}");
        assert!(sides.len() > 2 * b.term_w as usize && sides.iter().all(|&(_, y)| y > top_row));
        assert!(!glints.is_empty() && glints.iter().all(|&(x, _)| x < b.w / 2));
        // its shadow falls to the right of it, the light being from the left
        let shadow = all(Part::TermShadow);
        assert!(!shadow.is_empty() && shadow.iter().all(|&(x, _)| x >= b.w / 2));
        // and nothing on the top is in the dark but that shadow: the flat of
        // it and its edges are all at least as light as the metal itself
        let light = |c: u32| (c >> 16 & 255) + (c >> 8 & 255) + (c & 255);
        for part in [Part::RimBack, Part::RimFront, Part::Face, Part::TermTop, Part::Glint] {
            assert!(light(b.colour(part, 0.0)) >= light(b.metal.body), "{part:?} is darker than the metal");
        }
        assert!(light(b.colour(Part::TermShadow, 0.0)) < light(b.colour(Part::Face, 0.0)));
    }

    #[test]
    fn the_sections_light_up_apart_and_nothing_is_drawn_outside() {
        let mut settings = Settings::in_memory();
        let mut b = battery(&mut settings);
        let half = b.w as f32 / 2.0;
        let middle = b.w / 2;
        let first = |b: &Battery, part: Part| (0..b.h).find(|&y| b.part(middle, y) == part).unwrap();
        for (percent, showing) in [(0, 0), (30, 2), (76, 4), (100, 5)] {
            b.set_fixed(Some((percent, false, false, None)));
            let at = |b: &Battery, x: i32, y: i32| b.picture[(y * b.w + x) as usize];
            // each section in its colour, lit or not, its top row lighter,
            // and the core above it a shade of the metal -- never black
            let u = (middle as f32 + 0.5 - half) / half;
            for i in 0..b.rings {
                let top = first(&b, Part::RingTop(i));
                let want = if i < showing { b.lit_rings[i as usize].at(stripe(u)) } else { b.off.at(stripe(u)) };
                assert_eq!(at(&b, middle, top + 2), want, "{percent}%: section {i}");
                assert_eq!(at(&b, middle, top), mix(want, WHITE, b.bevel), "{percent}%: section {i}'s top row");
                assert_eq!(b.part(middle, top - 1), Part::Core);
                let core = at(&b, middle, top - 1);
                assert!((0..3).all(|k| (core >> (8 * k) & 255) > 60), "{percent}%: the core is too dark: {core:08X}");
            }
            // every px of the picture is the battery, in a solid colour, or
            // nothing at all
            for (n, &p) in b.picture.iter().enumerate() {
                let (x, y) = (n as i32 % b.w, n as i32 / b.w);
                assert_eq!(p == 0, b.part(x, y) == Part::Out, "{percent}%: {x},{y}");
                assert!(p == 0 || p >> 24 == 255, "{percent}%: {x},{y} is not solid");
            }
        }
        // drawn, the battery is put down as it is and what is round it is
        // left alone
        const UNDER: u32 = 0xFF60_5040;
        let (w, h) = (b.w, b.h);
        let mut px = vec![UNDER; (w * h) as usize];
        // SAFETY: the canvas is exactly the vector.
        let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), w, w, h) };
        b.draw(0, 0, &mut canvas);
        drop(canvas);
        for (n, (&drawn, &p)) in px.iter().zip(&b.picture).enumerate() {
            assert_eq!(drawn, if p == 0 { UNDER } else { p }, "{},{}", n as i32 % w, n as i32 / w);
        }
    }

    #[test]
    fn this_machine_answers() {
        let mut settings = Settings::in_memory();
        let mut b = battery(&mut settings);
        let mut ctx = Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None };
        // with a battery or without one, it has something to say: the plug,
        // or on battery the time left (or "...")
        assert_eq!(b.tick(&mut ctx), Dirty::All);
        assert!(!b.shown.percent.is_empty());
        assert_eq!(b.shown.on_battery, !b.shown.time.is_empty());
    }
}
