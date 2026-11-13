//! Rain Window: rain the colour of rust, falling past a small dark window.  It
//! measures nothing and says nothing; it is there to be looked at.
//!
//! The picture is three depths of streaks -- the far ones short, dim and
//! slow, the near ones long, bright and quick -- leaning with a steady wind,
//! over a dusk that lightens towards the horizon.  Where a drop lands on the
//! strip of ground at the foot of the pane it leaves a splash and a ripple.
//! All of it is whole pixels in the skin's own colours, like everything else
//! here.
//!
//! This is the one widget that moves more often than once a second, so it is
//! the one that asks the host for frames (`Widget::frame_ms`).  It asks only
//! while it has rain to show: paused, it asks for none and costs nothing.

use crate::canvas::{Canvas, Rect};
use crate::skin::{self, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Request, Widget};

static SKIN: &Skin = &skin::rain::SKIN;
const SECTION: &str = "Rain";

/// Milliseconds between frames: twenty-five a second.
const FRAME_MS: u32 = 40;
/// The longest stretch one frame may stand for, in seconds.  After a stall
/// the rain carries on from where it was rather than leaping ahead.
const MAX_STEP: f32 = 0.1;
/// How far a drop drifts left for each pixel it falls: the wind.
const SLANT: f32 = 0.2;
/// Drops in the air at once, by Rate: light, steady, heavy.
const DROPS: [usize; 3] = [36, 72, 130];
/// Seconds a splash and its ripple last.
const SPLASH_LIFE: f32 = 0.5;

/// One depth of rain.
struct Layer {
    /// Pixels a second.
    speed: f32,
    /// Pixels from head to tail.
    len: i32,
    /// Its share of the drops in the air.
    share: f32,
    /// How far below the horizon its drops land, from and to: the nearer the
    /// rain, the nearer the foot of the pane.
    depth: (f32, f32),
}

const FAR: usize = 0;
const MID: usize = 1;
const NEAR: usize = 2;
const LAYERS: [Layer; 3] = [
    Layer { speed: 100.0, len: 4, share: 0.45, depth: (0.0, 4.0) },
    Layer { speed: 160.0, len: 7, share: 0.35, depth: (4.0, 9.0) },
    Layer { speed: 240.0, len: 12, share: 0.20, depth: (8.0, 14.0) },
];

struct Drop {
    layer: usize,
    /// Where it would be at the top of the pane; the wind takes it left of
    /// that as it falls.
    x: f32,
    /// Its head, in rows down the pane.
    y: f32,
    /// The row it lands on.
    land: f32,
}

struct Splash {
    layer: usize,
    x: i32,
    y: i32,
    age: f32,
}

/// xorshift32: all the chance the rain needs.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// `lo <= n < hi`
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 8) as f32 / (1u32 << 24) as f32)
    }
}

pub struct Rain {
    /// The pane, as the skin lays it out.
    w: i32,
    h: i32,
    /// The first row of ground.
    horizon: i32,
    /// The picture behind the rain, a colour a row: dusk, then wet ground.
    rows: Vec<u32>,
    /// A colour for each depth, and the glint.
    ink: [u32; 3],
    glint: u32,

    /// 1 light, 2 steady, 3 heavy.
    rate: usize,
    paused: bool,
    /// Far ones first, so that nearer rain is drawn over them.
    drops: Vec<Drop>,
    splashes: Vec<Splash>,
    rng: Rng,
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| -> u32 {
        let (a, b) = (((a >> shift) & 255) as f32, ((b >> shift) & 255) as f32);
        (a + (b - a) * t + 0.5) as u32
    };
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// `colour` at `alpha` of 1.
fn faded(colour: u32, alpha: f32) -> u32 {
    colour & 0x00FF_FFFF | ((alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24
}

impl Rain {
    pub fn new(ctx: &Ctx) -> Rain {
        let (w, h) = (SKIN.int("PaneW", 174), SKIN.int("PaneH", 183));
        let horizon = (h - SKIN.int("GroundH", 16)).max(1);
        let (sky1, sky2, ground) = (SKIN.colour("RainSky1"), SKIN.colour("RainSky2"), SKIN.colour("RainGround"));
        // The dusk lightens down to the horizon; the wet ground under it
        // takes up a little of that light and loses it again towards the foot.
        let rows = (0..h)
            .map(|row| {
                if row < horizon {
                    mix(sky1, sky2, row as f32 / (horizon - 1).max(1) as f32)
                } else {
                    mix(ground, sky1, (row - horizon) as f32 / (h - horizon) as f32)
                }
            })
            .collect();
        let mut rain = Rain {
            w,
            h,
            horizon,
            rows,
            ink: [SKIN.colour("RainFar"), SKIN.colour("RainMid"), SKIN.colour("RainNear")],
            glint: SKIN.colour("RainGlint"),
            rate: ctx.settings.int(SECTION, "Rate", SKIN.int("Rate", 2)).clamp(1, 3) as usize,
            paused: ctx.settings.flag(SECTION, "Paused", SKIN.int("Paused", 0) != 0),
            drops: Vec::new(),
            splashes: Vec::new(),
            // never zero, which xorshift cannot leave
            rng: Rng(ctx.now as u32 | 1),
        };
        rain.fill();
        rain
    }

    /// For the test harness: the same shower every time, `seconds` in.
    pub fn set_fixed(&mut self, seconds: f32) {
        self.rng = Rng(0x9E37_79B9);
        self.fill();
        for _ in 0..(seconds * 1000.0 / FRAME_MS as f32) as u32 {
            self.step(FRAME_MS as f32 / 1000.0);
        }
    }

    /// A drop of `layer`: somewhere on its way down already, or about to
    /// come in at the top.
    fn drop(&mut self, layer: usize, falling: bool) -> Drop {
        let depth = LAYERS[layer].depth;
        let land = (self.horizon as f32 + self.rng.range(depth.0, depth.1)).min(self.h as f32 - 1.0);
        // The wind takes a drop SLANT * land to the left by the time it
        // lands, so some have to start that far off to the right for the
        // rain to reach the pane's bottom right corner.
        let x = self.rng.range(0.0, self.w as f32 + SLANT * land);
        let y = if falling { self.rng.range(0.0, land) } else { -self.rng.range(0.0, 30.0) };
        Drop { layer, x, y, land }
    }

    /// Start the shower over, already in full fall, at the present rate.
    fn fill(&mut self) {
        let total = DROPS[self.rate - 1];
        let far = (total as f32 * LAYERS[FAR].share + 0.5) as usize;
        let mid = (total as f32 * LAYERS[MID].share + 0.5) as usize;
        self.drops.clear();
        self.splashes.clear();
        for (layer, count) in [(FAR, far), (MID, mid), (NEAR, total - far - mid)] {
            for _ in 0..count {
                let drop = self.drop(layer, true);
                self.drops.push(drop);
            }
        }
    }

    /// `dt` seconds of rain.
    fn step(&mut self, dt: f32) {
        for i in 0..self.drops.len() {
            let d = &mut self.drops[i];
            d.y += LAYERS[d.layer].speed * dt;
            if d.y < d.land {
                continue;
            }
            let (layer, x, y) = (d.layer, (d.x - SLANT * d.land).floor() as i32, d.land as i32);
            if (0..self.w).contains(&x) {
                self.splashes.push(Splash { layer, x, y, age: 0.0 });
            }
            // the same drop again, from the top: the count never changes
            let again = self.drop(layer, false);
            self.drops[i] = again;
        }
        for s in &mut self.splashes {
            s.age += dt;
        }
        self.splashes.retain(|s| s.age < if s.layer == FAR { SPLASH_LIFE / 3.0 } else { SPLASH_LIFE });
    }

    /// The pane in the window: all a frame has to repaint.
    fn pane(&self) -> Rect {
        match SKIN.meter("MeterRain") {
            Some(m) => Rect::new(m.b[0], m.b[1], m.b[2], m.b[3]),
            None => Rect::new(0, 0, SKIN.w, SKIN.h),
        }
    }

    /// One streak, head lowest, leaning the way the wind blows.  Its tail is
    /// the colour of the rain one depth behind it, and the nearest have a
    /// bright head.
    fn streak(&self, d: &Drop, x: i32, y: i32, canvas: &mut Canvas) {
        let len = LAYERS[d.layer].len;
        let head = d.y.floor() as i32;
        for i in 0..len {
            let row = head - i;
            if row < 0 {
                break;
            }
            let colour = if d.layer == NEAR && i < 2 {
                self.glint
            } else if i * 3 >= len * 2 && d.layer > FAR {
                self.ink[d.layer - 1]
            } else {
                self.ink[d.layer]
            };
            canvas.fill(x + (d.x - SLANT * row as f32).floor() as i32, y + row, 1, 1, colour);
        }
    }

    /// What a drop leaves where it lands.  Far off that is one brief speck.
    /// Nearer, a spark where it struck and a ripple that widens and fades as
    /// it goes: a ring seen from the side, so a line that opens in the
    /// middle -- and, for the nearest, shows its far and near edges a row
    /// above and below.
    fn splash(&self, s: &Splash, x: i32, y: i32, canvas: &mut Canvas) {
        let (sx, sy) = (x + s.x, y + s.y);
        if s.layer == FAR {
            return canvas.fill(sx, sy, 1, 1, self.ink[FAR]);
        }
        let t = s.age / SPLASH_LIFE;
        let near = s.layer == NEAR;
        let ring = faded(self.ink[s.layer], 0.6 * (1.0 - t));
        let reach = 1 + (t * if near { 6.0 } else { 4.0 }) as i32;
        if reach < 3 {
            canvas.fill(sx - reach, sy, 2 * reach + 1, 1, ring);
        } else {
            canvas.fill(sx - reach, sy, 2, 1, ring);
            canvas.fill(sx + reach - 1, sy, 2, 1, ring);
            if near {
                canvas.fill(sx - reach + 2, sy - 1, 2 * reach - 3, 1, ring);
                canvas.fill(sx - reach + 2, sy + 1, 2 * reach - 3, 1, ring);
            }
        }
        if t < 0.2 {
            canvas.fill(sx, sy - 1, 1, 1, if near { self.glint } else { self.ink[NEAR] });
        }
    }

    fn set_rate(&mut self, rate: usize, ctx: &mut Ctx) -> Outcome {
        if rate == self.rate {
            return Outcome::none();
        }
        self.rate = rate;
        ctx.settings.set_int(SECTION, "Rate", rate as i32);
        self.fill();
        Outcome::dirty(Dirty::Part(self.pane()))
    }
}

impl Hooks for Rain {
    /// [MeterRain]: the whole picture, back to front, kept inside the pane.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        if meter.name != "MeterRain" {
            return;
        }
        let outside = canvas.clip;
        canvas.clip = outside.intersect(&Rect::xywh(x, y, self.w, self.h));
        for (row, colour) in self.rows.iter().enumerate() {
            canvas.fill(x, y + row as i32, self.w, 1, *colour);
        }
        for s in &self.splashes {
            self.splash(s, x, y, canvas);
        }
        for d in &self.drops {
            self.streak(d, x, y, canvas);
        }
        canvas.clip = outside;
    }
}

impl Widget for Rain {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Rain Window\n\n\
         Rain the colour of rust, falling past a small dark window.  It \
         measures nothing and tells you nothing: it is there to be looked \
         at.\n\n\
         Right-click for how hard it rains -- light, steady or heavy -- and \
         to pause it.  Paused, closed, or with the workstation locked, it \
         costs nothing."
    }

    /// Nothing here happens by the second.
    fn tick(&mut self, _ctx: &mut Ctx) -> Dirty {
        Dirty::None
    }

    fn frame_ms(&self) -> u32 {
        if self.paused { 0 } else { FRAME_MS }
    }

    fn frame(&mut self, ms: u32) -> Dirty {
        if self.paused {
            return Dirty::None;
        }
        self.step((ms as f32 / 1000.0).min(MAX_STEP));
        Dirty::Part(self.pane())
    }

    fn command(&mut self, cmd: &str, _arg: i32, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        match cmd {
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { id: 1, label: "Light rain", checked: self.rate == 1 });
        items.push(MenuItem { id: 2, label: "Steady rain", checked: self.rate == 2 });
        items.push(MenuItem { id: 3, label: "Heavy rain", checked: self.rate == 3 });
        items.push(MenuItem { id: 4, label: "Pause", checked: self.paused });
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            1..=3 => self.set_rate(id as usize, ctx),
            4 => {
                self.paused = !self.paused;
                ctx.settings.set_int(SECTION, "Paused", self.paused as i32);
                Outcome::none()
            }
            _ => Outcome::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn rain(settings: &mut Settings) -> Rain {
        Rain::new(&Ctx { now: 1_000, settings, locked: false, signed_in: None })
    }

    #[test]
    fn every_drop_that_lands_comes_round_again() {
        let mut settings = Settings::in_memory();
        let mut rain = rain(&mut settings);
        for rate in 1..=3 {
            rain.rate = rate;
            rain.set_fixed(20.0);
            assert_eq!(rain.drops.len(), DROPS[rate - 1]);
            for d in &rain.drops {
                assert!(d.y < d.land, "a drop below the row it lands on");
                assert!(d.land >= rain.horizon as f32 && d.land < rain.h as f32, "a drop landing off the ground");
            }
            // far ones first: the order they are drawn in
            assert!(rain.drops.windows(2).all(|pair| pair[0].layer <= pair[1].layer));
            assert!(!rain.splashes.is_empty());
            assert!(rain.splashes.iter().all(|s| (0..rain.w).contains(&s.x) && s.age < SPLASH_LIFE));
        }
    }

    #[test]
    fn the_same_shower_twice() {
        let mut settings = Settings::in_memory();
        let (mut a, mut b) = (rain(&mut settings), rain(&mut settings));
        a.set_fixed(3.0);
        b.set_fixed(3.0);
        let places = |r: &Rain| r.drops.iter().map(|d| (d.x.to_bits(), d.y.to_bits())).collect::<Vec<_>>();
        assert_eq!(places(&a), places(&b));
    }

    #[test]
    fn paused_it_asks_for_no_frames() {
        let mut settings = Settings::in_memory();
        let mut rain = rain(&mut settings);
        assert_eq!(rain.frame_ms(), FRAME_MS);
        assert_eq!(rain.frame(40), Dirty::Part(rain.pane()));

        let mut ctx = Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None };
        rain.menu_command(4, &mut ctx);
        assert_eq!(rain.frame_ms(), 0);
        assert_eq!(rain.frame(40), Dirty::None);
        assert!(settings.flag(SECTION, "Paused", false));
    }
}
