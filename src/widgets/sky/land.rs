//! The places the weather is seen over -- one for each scene, each its own:
//!
//!   Clear          a lake among mountains, a meadow and pines on its shore
//!   Partly cloudy  farmland: hills of fields and hedgerows, an oak on a knoll
//!   Overcast       a town's roofs, chimneys smoking, a church spire beyond
//!   Fog            a valley, ridge behind ridge, a river, a pine on a crag
//!   Drizzle        an avenue of autumn trees, leaves on the path, puddles
//!   Rain           a street: house fronts, a shop, lamps, the road shining
//!   Heavy rain     a coast: a lighthouse on its rock, a jetty, the sea
//!   Sleet          a country road in winter: poles and wires, bare trees
//!   Snow           a cabin in a pine forest, smoke from its chimney
//!   Heavy snow     the Alps: peaks, snowy firs, a hut with a light on
//!   Thunderstorm   a prairie: grass to the horizon, a lone dead tree, a mill
//!   Hail           a farm: a red barn and its silo, crop rows, hay bales
//!
//! A place is painted once, a pixel at a time, in the colours it has by day,
//! and each pixel is marked with how far off it is: the light of the moment
//! and the haze of the distance are laid over it each time the light
//! changes (see `sky`).  Windows and lamps are marked as lights, which shine
//! after dark; water is marked with how far below its surface each row is,
//! and shows, a frame at a time, what stands over it.  What moves -- smoke,
//! a lighthouse's beam, a windmill's sails, falling leaves, fireflies -- the
//! place only says where; `sky` draws it.
//!
//! Every place is the same every time: its chance is seeded with its name.

use core::f32::consts::TAU;

use super::clouds::{self, Form};
use super::sprites::Ride;
use super::{mix, City, Rng, Scene};
use crate::skin::Skin;

mod cities;

/// What a pixel of a place is.
pub const SOLID: u8 = 0;
/// A window or a lamp: dark glass by day, its own light after dark.
pub const LIGHT: u8 = 1;
/// Water, or a road shining with it: it shows what is over it.
pub const WATER: u8 = 2;
/// Stone floodlit after dark: its own colour by day, gold at night.
pub const FLOOD: u8 = 3;

/// The flags a staff can fly: the Union Flag, and Scotland's Saltire.
pub const UNION: u8 = 0;
pub const SALTIRE: u8 = 1;

/// A line things go along through a place -- a road, a pavement, a river.
#[derive(Clone, Copy, Debug)]
pub struct Lane {
    pub ride: Ride,
    /// The row its riders stand on at the left edge, and how far that falls
    /// a column -- or, for a cable car, the row of its cable.
    pub y: f32,
    pub slope: f32,
    /// The columns it runs between: a rider goes off one end and comes back
    /// on at the other.
    pub from: f32,
    pub to: f32,
    /// Pixels a second.
    pub speed: f32,
    /// Its riders go both ways; otherwise all go right -- or, for
    /// tumbleweed, the way the wind blows.
    pub both: bool,
    pub count: usize,
}

/// What the weather has done to a place: snow lying on its ledges, slush,
/// or its streets wet.
#[derive(Clone, Copy, Default, Debug)]
pub struct Wx {
    pub snow: bool,
    pub slush: bool,
    pub wet: bool,
}

impl Wx {
    pub fn of(scene: Scene) -> Wx {
        Wx {
            snow: matches!(scene, Scene::Snow | Scene::Blizzard),
            slush: scene == Scene::Sleet,
            wet: matches!(scene, Scene::Drizzle | Scene::Rain | Scene::Downpour | Scene::Sleet | Scene::Storm | Scene::Hail),
        }
    }
}

pub struct Land {
    pub w: i32,
    pub h: i32,
    /// The colour of each pixel by day; none, where the sky shows.
    pub base: Vec<u32>,
    /// How far off each is, as how much of the horizon's colour it takes.
    pub haze: Vec<u8>,
    pub kind: Vec<u8>,
    /// For water, how many rows below its surface each row is.
    pub depth: Vec<u8>,
    /// Water only a film deep, on something solid -- a wet road: it shines
    /// as water does, but hail bounces off it.
    pub film: Vec<bool>,
    /// The rows what falls lands on: the far edge and the near.
    pub ground: (i32, i32),
    /// Where smoke rises from.
    pub chimneys: Vec<(i32, i32)>,
    /// Lamps that throw a glow after dark.
    pub lamps: Vec<(i32, i32)>,
    /// A lighthouse's lantern.
    pub beacon: Option<(i32, i32)>,
    /// A windmill's hub, and how long its sails are.
    pub mill: Option<(i32, i32, i32)>,
    /// The rows of open sea, for its waves.
    pub sea: Option<(i32, i32)>,
    /// Where fireflies dance on a fine night: x, y, w, h.
    pub meadow: Option<[i32; 4]>,
    /// Trees that shed their leaves: x, y, w, h.
    pub canopy: Option<[i32; 4]>,
    /// The rows fog lies along.
    pub fog: Vec<f32>,
    /// What goes by.
    pub lanes: Vec<Lane>,
    /// A clock's face: its middle and radius, for hands telling the time.
    pub clock: Option<(i32, i32, i32)>,
    /// A great wheel's hub and radius: the London Eye, turning.
    pub wheel: Option<(i32, i32, i32)>,
    /// A waterfall: its column, top, foot and width.
    pub falls: Option<(i32, i32, i32, i32)>,
    /// A traffic light's top lamp.
    pub signal: Option<(i32, i32)>,
    /// Where granite catches the sun and glints.
    pub glints: Vec<(i32, i32)>,
    /// Lights that blink: cranes', buoys', masts' -- where, and in what.
    pub blinkers: Vec<(i32, i32, u32)>,
    /// A flag's staff top, the flag flying downwind of it, and which flag.
    pub flag: Option<(i32, i32, u8)>,
    /// Gulls, not songbirds, cross its sky -- in any weather.
    pub gulls: bool,
}

impl Land {
    /// The place the scene is seen over, `w` by `h`: the scene's own, or
    /// the city's -- and of Aberdeen, its harbour after dark.
    pub fn of(scene: Scene, city: Option<City>, night: bool, skin: &Skin, w: i32, h: i32) -> Land {
        let n = (w * h).max(0) as usize;
        let l = Land {
            w,
            h,
            base: vec![0; n],
            haze: vec![0; n],
            kind: vec![SOLID; n],
            depth: vec![0; n],
            film: vec![false; n],
            ground: (h * 2 / 3, h - 1),
            chimneys: Vec::new(),
            lamps: Vec::new(),
            beacon: None,
            mill: None,
            sea: None,
            meadow: None,
            canopy: None,
            fog: Vec::new(),
            lanes: Vec::new(),
            clock: None,
            wheel: None,
            falls: None,
            signal: None,
            glints: Vec::new(),
            blinkers: Vec::new(),
            flag: None,
            gulls: false,
        };
        let name = city.map_or(scene.key(), City::key);
        let seed = name.bytes().fold(0x811C_9DC5u32, |a, c| (a ^ c as u32).wrapping_mul(0x0100_0193)) | 1;
        let mut b = Brush { l, skin, rng: Rng(seed), haze: 0 };
        let wx = Wx::of(scene);
        match city {
            Some(City::London) => cities::london(&mut b, wx),
            Some(City::Aberdeen) if night => cities::aberdeen_harbour(&mut b, wx),
            Some(City::Aberdeen) => cities::aberdeen(&mut b, wx),
            None => scenery(&mut b, scene),
        }
        b.l
    }

    pub fn kind_at(&self, x: i32, y: i32) -> u8 {
        if (0..self.w).contains(&x) && (0..self.h).contains(&y) { self.kind[(y * self.w + x) as usize] } else { SOLID }
    }

    /// Whether here is water deep enough to fall into -- a river, the sea,
    /// a puddle -- not a wet road.
    pub fn deep_at(&self, x: i32, y: i32) -> bool {
        self.kind_at(x, y) == WATER && !self.film[(y * self.w + x) as usize]
    }

    /// When here is the face of something standing in deep water -- a
    /// ship's hull, a bridge's arches -- the row its foot meets the water.
    /// What comes down here comes down in the water behind it.
    pub fn standing_in_water(&self, x: i32, y: i32) -> Option<i32> {
        let solid = |y: i32| (0..self.h).contains(&y) && self.kind_at(x, y) != WATER && self.base[(y * self.w + x) as usize] != 0;
        if !solid(y) {
            return None;
        }
        let mut foot = y;
        while foot < self.ground.1 && solid(foot) {
            foot += 1;
        }
        self.deep_at(x, foot).then_some(foot)
    }

    /// The highest row any of the place reaches, column by column.
    #[cfg(test)]
    pub fn skyline(&self) -> Vec<i32> {
        (0..self.w).map(|x| (0..self.h).find(|&y| self.base[(y * self.w + x) as usize] != 0).unwrap_or(self.h)).collect()
    }
}

/// The scene's own place.
fn scenery(b: &mut Brush, scene: Scene) {
    match scene {
        Scene::Clear => lake(b),
        Scene::Partly => farmland(b),
        Scene::Overcast => town(b),
        Scene::Fog => valley(b),
        Scene::Drizzle => avenue(b),
        Scene::Rain => street(b),
        Scene::Downpour => coast(b),
        Scene::Sleet => winter_road(b),
        Scene::Snow => cabin(b),
        Scene::Blizzard => alps(b),
        Scene::Storm => prairie(b),
        Scene::Hail => barnyard(b),
    }
}

// ---------------------------------------------------------------------------
//  The brush
// ---------------------------------------------------------------------------

struct Brush<'a> {
    l: Land,
    skin: &'a Skin,
    rng: Rng,
    /// How far off what is painted now is: the haze it takes, 0 to 255.
    haze: u8,
}

/// A wall, a roof, or anything painted in a lit tone and a shaded one.
type Pair = [u32; 2];

/// A house seen from a little to its left: the gable end facing us, the
/// side wall running back in shade, and the roof over it.
struct House {
    x: i32,
    base: i32,
    /// The gable end's width, and the side wall's.
    w: i32,
    side: i32,
    /// The walls' height, and the roof's from eaves to ridge.
    wall: i32,
    roof: i32,
    walls: Pair,
    roofs: Pair,
    /// The share of its windows lit after dark.
    lit: f32,
    chimney: bool,
    /// Logs, not plaster: a line between each course.
    logs: bool,
}

impl Brush<'_> {
    fn c(&self, name: &str) -> u32 {
        self.skin.colour(name) | 0xFF00_0000
    }

    fn cs<const N: usize>(&self, names: [&str; N]) -> [u32; N] {
        names.map(|n| self.c(n))
    }

    fn at(&self, x: i32, y: i32) -> Option<usize> {
        ((0..self.l.w).contains(&x) && (0..self.l.h).contains(&y)).then(|| (y * self.l.w + x) as usize)
    }

    fn set(&mut self, x: i32, y: i32, c: u32) {
        if let Some(i) = self.at(x, y) {
            self.l.base[i] = c | 0xFF00_0000;
            self.l.haze[i] = self.haze;
            self.l.kind[i] = SOLID;
        }
    }

    /// A pixel of a window or a lamp, lit after dark in `c`.
    fn light(&mut self, x: i32, y: i32, c: u32) {
        self.set(x, y, c);
        if let Some(i) = self.at(x, y) {
            self.l.kind[i] = LIGHT;
        }
    }

    /// A pixel of floodlit stone: `c` by day, lit gold after dark.
    fn flood(&mut self, x: i32, y: i32, c: u32) {
        self.set(x, y, c);
        if let Some(i) = self.at(x, y) {
            self.l.kind[i] = FLOOD;
        }
    }

    /// A lane of `count` of `ride` along row `y` from `from` to `to`.
    #[allow(clippy::too_many_arguments)]
    fn lane(&mut self, ride: Ride, y: f32, slope: f32, from: f32, to: f32, speed: f32, both: bool, count: usize) {
        self.l.lanes.push(Lane { ride, y, slope, from, to, speed, both, count });
    }

    /// A pixel of water, `depth` rows below its surface, its own colour `c`.
    fn wet(&mut self, x: i32, y: i32, c: u32, depth: i32) {
        self.set(x, y, c);
        if let Some(i) = self.at(x, y) {
            self.l.kind[i] = WATER;
            self.l.depth[i] = depth.clamp(0, 255) as u8;
            self.l.film[i] = false;
        }
    }

    /// A pixel of something solid under a film of water -- a wet road --
    /// shining as water does, `depth` rows below where it mirrors from.
    fn slick(&mut self, x: i32, y: i32, c: u32, depth: i32) {
        self.wet(x, y, c, depth);
        if let Some(i) = self.at(x, y) {
            self.l.film[i] = true;
        }
    }

    fn get(&self, x: i32, y: i32) -> u32 {
        self.at(x, y).map_or(0, |i| self.l.base[i])
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: u32) {
        let n = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.set((x0 + (x1 - x0) * t).round() as i32, (y0 + (y1 - y0) * t).round() as i32, c);
        }
    }

    /// A filled oval.
    fn oval(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: u32) {
        for y in (cy - ry).floor() as i32..=(cy + ry).ceil() as i32 {
            for x in (cx - rx).floor() as i32..=(cx + rx).ceil() as i32 {
                let (dx, dy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                if dx * dx + dy * dy <= 1.0 {
                    self.set(x, y, c);
                }
            }
        }
    }

    fn chance(&mut self, p: f32) -> bool {
        self.rng.unit() < p
    }

    /// `lo <= n < hi`
    fn int(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.rng.unit() * (hi - lo).max(1) as f32) as i32
    }

    fn pick(&mut self, xs: &[u32]) -> u32 {
        xs[(self.rng.unit() * xs.len() as f32) as usize % xs.len()]
    }

    /// A line of land across the place: `y`, and on it waves of the given
    /// heights and lengths, each from a phase of its own.
    fn waves(&mut self, y: f32, parts: &[(f32, f32)]) -> Vec<f32> {
        let phases: Vec<f32> = parts.iter().map(|_| self.rng.range(0.0, TAU)).collect();
        (0..self.l.w)
            .map(|x| y + parts.iter().zip(&phases).map(|(&(a, len), p)| a * (x as f32 * TAU / len + p).sin()).sum::<f32>())
            .collect()
    }

    fn top(line: &[f32], x: i32) -> i32 {
        line[x.clamp(0, line.len() as i32 - 1) as usize].round() as i32
    }

    /// A hillside from `line` to `bottom`, lit where it faces the light on
    /// the left, shaded where it turns away, along a band of its top.
    fn hill(&mut self, line: &[f32], bottom: i32, [dark, mid, lit]: [u32; 3], band: i32) {
        let w = self.l.w;
        for x in 0..w {
            let t = Self::top(line, x);
            let slope = line[(x + 1).min(w - 1) as usize] - line[(x - 1).max(0) as usize];
            for y in t.max(0)..bottom {
                let edge = y - t < band;
                let c = if edge && slope < -0.3 { lit } else if edge && slope > 0.3 { dark } else { mid };
                self.set(x, y, c);
            }
        }
    }

    /// A forest along `line`: small pines shoulder to shoulder, their feet
    /// on the line, and under them the dark of the wood down to `bottom`.
    fn forest(&mut self, line: &[f32], sizes: (i32, i32), ramp: [u32; 3], snow: Option<u32>, bottom: i32) {
        let w = self.l.w;
        for x in 0..w {
            let t = Self::top(line, x) + 1;
            for y in t.max(0)..bottom {
                // the odd lit trunk in the wood's dark
                let c = if (x * 7 + y * 3) % 23 == 0 { ramp[1] } else { ramp[0] };
                self.set(x, y, c);
            }
        }
        let mut x = -3;
        while x < w + 3 {
            let h = self.int(sizes.0, sizes.1 + 1);
            let base = Self::top(line, x) + 1 + self.int(0, 2);
            self.spire(x, base, h, ramp, snow);
            x += (h / 3).max(2) + self.int(0, 2);
        }
    }

    /// A small pine, as a forest's are: a spire of boughs, lit on its left.
    fn spire(&mut self, cx: i32, base: i32, h: i32, [dark, mid, lit]: [u32; 3], snow: Option<u32>) {
        for row in 0..h {
            let y = base - h + row;
            let half = (row as f32 * 0.36).round() as i32;
            for dx in -half..=half {
                let c = match snow {
                    Some(s) if row % 3 == 0 && dx <= 0 => s,
                    _ if dx < 0 => mid,
                    _ if dx == 0 && row < 2 => lit,
                    _ => dark,
                };
                self.set(cx + dx, y, c);
            }
        }
    }

    /// A pine, `h` tall standing at (cx, base): its trunk, and tiers of
    /// boughs, each wider at its foot, lit on the left and shaded on the
    /// right and under each tier -- and, if it has snowed, snow lying along
    /// the top of each.
    fn pine(&mut self, cx: i32, base: i32, h: i32, [dark, mid, lit]: [u32; 3], snow: Option<Pair>) {
        let bark = self.cs(["Bark", "BarkDark"]);
        let trunk = (h / 7).max(2);
        let tw = if h > 40 { 2 } else { 1 };
        for y in base - trunk - 2..base {
            for dx in 0..tw {
                self.set(cx - tw / 2 + dx, y, bark[(dx == tw - 1 && tw > 1) as usize]);
            }
        }
        let crown = h - trunk;
        let top = base - h;
        let widest = (h as f32 * 0.24).max(1.5);
        let tiers = (crown / 6).clamp(2, 8);
        let tier = crown as f32 / tiers as f32;
        for row in 0..crown {
            let y = top + row;
            let f = row as f32 / crown as f32;
            let within = (row as f32 % tier) / tier;
            let half = (widest * (0.2 + 0.8 * f) * (0.5 + 0.5 * within)).round() as i32;
            for dx in -half..=half {
                // a ragged edge
                if dx.abs() == half && half > 1 && self.chance(0.35) {
                    continue;
                }
                let c = match snow {
                    Some(s) if within < 0.3 && dx < half / 2 => s[(dx < -half / 3) as usize],
                    Some(s) if within < 0.18 => s[0],
                    _ if within > 0.82 => dark,
                    _ if dx < -half / 3 => lit,
                    _ if dx > half / 3 => dark,
                    _ => mid,
                };
                self.set(cx + dx, y, c);
            }
        }
        self.set(cx, top - 1, snow.map_or(mid, |s| s[1]));
    }

    /// A round tree: its trunk, and a crown of leafy puffs shaded as a
    /// cloud is, in five tones of leaf, speckled.
    fn tree(&mut self, cx: i32, base: i32, cw: i32, ch: i32, trunk: i32, leaves: [u32; 5]) {
        let bark = self.cs(["Bark", "BarkDark"]);
        let tw = (cw / 12).clamp(1, 4);
        let crown_top = base - trunk - ch;
        for y in crown_top + ch / 2..base {
            for dx in 0..tw {
                self.set(cx - tw / 2 + dx, y, bark[(dx == tw - 1 && tw > 1) as usize]);
            }
        }
        if tw > 1 {
            self.set(cx - tw / 2 - 1, base - 1, bark[0]);
            self.set(cx - tw / 2 + tw, base - 1, bark[1]);
        }
        let form = Form::crown(&mut self.rng, cw.max(2), ch.max(2));
        let tones = clouds::shade(&form, clouds::light_from(-0.6, 0.75), 0.1);
        let x0 = cx - cw / 2;
        for yy in 0..form.h {
            for xx in 0..form.w {
                let t = tones[(yy * form.w + xx) as usize];
                if t == 0 {
                    continue;
                }
                let t = if cw > 8 && self.chance(0.16) { (t as i32 + if self.chance(0.5) { 1 } else { -1 }).clamp(1, 5) } else { t as i32 };
                self.set(x0 + xx, crown_top + yy, leaves[t as usize - 1]);
            }
        }
    }

    /// A bare tree's limb from (x, y), leaning `angle` from upright: it
    /// wanders as it grows, thins, and forks.
    #[allow(clippy::too_many_arguments)]
    fn limb(&mut self, x: f32, y: f32, angle: f32, len: f32, thick: f32, forks: u32, c: u32) {
        let (mut px, mut py, mut a) = (x, y, angle);
        for _ in 0..len.max(1.0) as i32 {
            a += self.rng.range(-0.09, 0.09);
            px += a.sin();
            py -= a.cos();
            let t = thick.round().max(1.0) as i32;
            for k in 0..t {
                self.set(px.round() as i32 + k - t / 2, py.round() as i32, c);
            }
        }
        if forks == 0 {
            return;
        }
        for i in 0..2 + self.chance(0.4) as u32 {
            let spread = self.rng.range(0.25, 0.7) * if i % 2 == 0 { 1.0 } else { -1.0 };
            let len = len * self.rng.range(0.55, 0.78);
            self.limb(px, py, a + spread, len, (thick * 0.6).max(1.0), forks - 1, c);
        }
    }

    /// A window `w` by `h`: glass, lit after dark if `lit`, with a pale sill.
    fn window(&mut self, x: i32, y: i32, w: i32, h: i32, lit: bool, sill: u32) {
        let (glass, warm) = (self.c("Glass"), self.c("WindowLit"));
        for yy in y..y + h {
            for xx in x..x + w {
                if lit { self.light(xx, yy, warm) } else { self.set(xx, yy, glass) }
            }
        }
        if w > 1 {
            for xx in x..x + w {
                self.set(xx, y + h, sill);
            }
        }
    }

    fn house(&mut self, s: &House) {
        let (x, b, w, side) = (s.x, s.base, s.w, s.side);
        let eave = b - s.wall;
        let apex = eave - s.roof;
        let mid = x + w / 2;
        let sill = self.c("Trim");
        // the gable end, and its triangle up to the ridge
        for y in apex..b {
            let half = if y < eave { ((y - apex) as f32 / s.roof as f32 * w as f32 / 2.0).round() as i32 } else { w / 2 };
            for xx in mid - half..mid + half + w % 2 {
                let course = s.logs && (y - eave).rem_euclid(2) == 1 && y >= eave;
                self.set(xx, y, if course { s.walls[1] } else { s.walls[0] });
            }
        }
        // the side wall, in shade, and the eaves' shadow along its top
        for y in eave..b {
            for xx in x + w..x + w + side {
                let course = s.logs && (y - eave).rem_euclid(2) == 1;
                let c = if y == eave { mix(s.walls[1], 0xFF00_0000, 0.3) } else if course { mix(s.walls[1], 0xFF00_0000, 0.18) } else { s.walls[1] };
                self.set(xx, y, c);
            }
        }
        // the roof over the side wall, from the gable's rafter back, in
        // courses of tiles -- or, snowed on, white with its edge in shade
        for y in apex..=eave {
            let r = y - apex;
            let left = mid + (r as f32 / s.roof as f32 * w as f32 / 2.0).round() as i32;
            let right = left + side + (y == eave) as i32;
            for xx in left..=right {
                let c = if r % 2 == 1 { s.roofs[1] } else { s.roofs[0] };
                self.set(xx, y, if xx == right { s.roofs[1] } else { c });
            }
        }
        // the rafters along the gable's two slopes
        for r in 0..=s.roof {
            let dx = (r as f32 / s.roof as f32 * w as f32 / 2.0).round() as i32;
            self.set(mid - dx - 1, apex + r, s.roofs[1]);
            self.set(mid - dx, apex + r, s.roofs[0]);
            self.set(mid + dx, apex + r, s.roofs[1]);
        }
        // the windows: one in the gable, and a row on each wall
        let wh = (s.wall / 3).clamp(2, 5);
        let wy = eave + (s.wall - wh) / 2 - 1;
        if s.roof >= 6 {
            let lit = self.chance(s.lit);
            self.window(mid - 1, apex + s.roof / 2, 2, 2, lit, sill);
        }
        let door = s.wall >= 8;
        for k in 0..(w / 7).max(1) {
            let wx = x + 2 + k * 7 + if door && k == 0 { 0 } else { 0 };
            if door && (wx..wx + 3).contains(&(mid - 1)) {
                continue;
            }
            let lit = self.chance(s.lit);
            self.window(wx, wy, 2, wh, lit, sill);
        }
        for k in 0..(side / 6) {
            let lit = self.chance(s.lit);
            self.window(x + w + 2 + k * 6, wy, 2, wh, lit, s.walls[0]);
        }
        if door {
            let dh = (s.wall * 2 / 3).max(4);
            self.rect(mid - 1, b - dh, 3, dh, self.c("WoodDark"));
        }
        if s.chimney {
            let cx = mid + side / 2 + (s.roof as f32 * 0.25) as i32 + 1;
            let top = apex + s.roof / 4 - 4;
            let brick = self.cs(["Brick", "BrickDark"]);
            for y in top..apex + s.roof / 2 {
                self.set(cx, y, brick[0]);
                self.set(cx + 1, y, brick[1]);
            }
            self.set(cx - 1, top, brick[1]);
            self.set(cx + 2, top, brick[1]);
            self.l.chimneys.push((cx, top - 1));
        }
    }

    /// A wire hung from (x0, y0) to (x1, y1), sagging `sag` in the middle.
    fn wire(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, sag: f32, c: u32) {
        let n = (x1 - x0).abs().ceil().max(1.0) as i32;
        let mut last: Option<i32> = None;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let x = (x0 + (x1 - x0) * t).round() as i32;
            let y = (y0 + (y1 - y0) * t + sag * 4.0 * t * (1.0 - t)).round() as i32;
            match last {
                Some(l) if (y - l).abs() > 1 => {
                    let (a, b) = (l.min(y), l.max(y));
                    for yy in a + 1..b {
                        self.set(x, yy, c);
                    }
                }
                _ => {}
            }
            self.set(x, y, c);
            last = Some(y);
        }
    }

    /// A telegraph pole `h` tall at (x, base), with its crossarm: the ends of
    /// the arm, where the wires hang from.
    fn pole(&mut self, x: i32, base: i32, h: i32) -> ((f32, f32), (f32, f32)) {
        let wood = self.cs(["Pole", "WoodDark"]);
        let thick = if h > 70 { 2 } else { 1 };
        for y in base - h..base {
            for k in 0..thick {
                self.set(x + k, y, wood[k as usize]);
            }
        }
        let arm = (h / 10).max(2);
        let y = base - h + (h / 16).max(1);
        for dx in -arm..=arm + thick - 1 {
            self.set(x + dx, y, wood[0]);
        }
        ((x as f32 - arm as f32, y as f32 + 1.0), ((x + arm + thick - 1) as f32, y as f32 + 1.0))
    }

    /// Blades of grass over the rows `y0..y1`: short strokes, longer nearer.
    fn blades(&mut self, x0: i32, x1: i32, y0: i32, y1: i32, colours: &[u32], count: usize) {
        for _ in 0..count {
            let x = self.int(x0, x1);
            let y = self.int(y0, y1);
            let near = (y - y0) as f32 / (y1 - y0).max(1) as f32;
            let len = 1 + (near * 3.5) as i32;
            if self.get(x, y) == 0 {
                continue;
            }
            let c = self.pick(colours);
            for k in 0..len {
                self.set(x, y - k, c);
            }
        }
    }

    /// Flowers dotted over the rows `y0..y1`, thicker nearer.
    fn flowers(&mut self, x0: i32, x1: i32, y0: i32, y1: i32, colours: &[u32], count: usize) {
        for _ in 0..count {
            let x = self.int(x0, x1);
            let f = self.rng.unit();
            let y = y0 + ((y1 - y0) as f32 * f.sqrt()) as i32;
            if self.get(x, y) == 0 {
                continue;
            }
            let c = self.pick(colours);
            self.set(x, y, c);
            if f > 0.5 {
                self.set(x + 1, y, c);
                self.set(x, y - 1, mix(c, 0xFFFF_FFFF, 0.3));
            }
        }
    }

    /// Peaks -- (x, summit row) each -- with flanks falling `steep` rows a
    /// column: rock lit on the faces turned left, towards the light, and in
    /// shade on the faces turned right; snow above a ragged snowline and in
    /// the gullies; down to `bottom`.
    fn peaks(&mut self, summits: &[(f32, f32)], steep: f32, bottom: i32, rock: [u32; 3], snow: Pair, snowline: f32) {
        let w = self.l.w;
        let rough: Vec<f32> = {
            let mut v = 0.0;
            (0..w).map(|_| {
                v = (v + self.rng.range(-1.2, 1.2)) * 0.6;
                v
            }).collect()
        };
        let mut line = vec![0f32; w as usize];
        let mut owner = vec![0usize; w as usize];
        for x in 0..w {
            let (i, y) = summits
                .iter()
                .enumerate()
                .map(|(i, &(sx, sy))| (i, sy + (x as f32 - sx).abs() * steep * (1.0 + 0.18 * (x as f32 * 0.21 + i as f32).sin())))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((0, bottom as f32));
            line[x as usize] = y + rough[x as usize];
            owner[x as usize] = i;
        }
        let ragged = |x: i32| snowline + 7.0 * (x as f32 * 0.13).sin() + 4.0 * (x as f32 * 0.47 + 1.0).sin();
        for x in 0..w {
            let t = line[x as usize].round() as i32;
            let (sx, _) = summits[owner[x as usize]];
            let lit = (x as f32) < sx;
            for y in t.max(0)..bottom {
                let d = y - t;
                let c = if (y as f32) < ragged(x) {
                    if lit { snow[1] } else { snow[0] }
                } else if d == 0 && lit {
                    rock[2]
                } else if lit {
                    // the lit face darkens a little down its height
                    let up = d as f32 / 50.0 * 16.0;
                    if up > super::BAYER[y as usize & 3][x as usize & 3] as f32 + 8.0 { rock[0] } else { rock[1] }
                } else {
                    rock[0]
                };
                self.set(x, y, c);
            }
        }
        // gullies down every face, away from its summit: dark clefts in the
        // rock, filled with snow high up, a lit edge on their far side
        for _ in 0..w / 4 {
            let x = self.int(0, w);
            let (sx, _) = summits[owner[x as usize]];
            let lit = (x as f32) < sx;
            let out = if lit { -1.0 } else { 1.0 };
            let (mut gx, mut gy) = (x as f32, line[x as usize] + self.rng.range(1.0, 6.0));
            let len = self.rng.range(6.0, 28.0) as i32;
            let lean = self.rng.range(0.25, 0.6) * out;
            for _ in 0..len {
                let (px, py) = (gx.round() as i32, gy.round() as i32);
                if py >= bottom || self.get(px, py) == 0 {
                    break;
                }
                let snowy = (py as f32) < ragged(px) + 16.0;
                let cleft = match (snowy, lit) {
                    (true, true) => snow[0],
                    (true, false) => snow[1],
                    (false, _) => mix(rock[0], 0xFF00_0000, 0.25),
                };
                self.set(px, py, cleft);
                if !snowy {
                    self.set(px - out as i32, py, if lit { rock[2] } else { rock[1] });
                }
                gx += lean + self.rng.range(-0.3, 0.3);
                gy += 1.0;
            }
        }
    }

    /// Land banded in fields, the bands following `line` down to `bottom`,
    /// each band cut into fields `cuts` px apart give or take, a hedge along
    /// every edge -- a patchwork, as farmland looks from a hill.
    fn patchwork(&mut self, line: &[f32], bottom: i32, fields: &[u32], hedge: Pair, cuts: (i32, i32), band: i32) {
        let w = self.l.w;
        let bands = ((bottom - line.iter().cloned().fold(f32::MAX, f32::min) as i32) / band + 2).max(1) as usize;
        let mut edges: Vec<Vec<i32>> = Vec::new();
        let mut colours: Vec<Vec<u32>> = Vec::new();
        for _ in 0..bands {
            let mut cut = vec![-self.int(0, cuts.1)];
            while *cut.last().unwrap() < w {
                let next = cut.last().unwrap() + self.int(cuts.0, cuts.1);
                cut.push(next);
            }
            let fill: Vec<u32> = (0..cut.len()).map(|_| self.pick(fields)).collect();
            edges.push(cut);
            colours.push(fill);
        }
        for x in 0..w {
            let t = Self::top(line, x);
            for y in t.max(0)..bottom {
                let d = y - t;
                let bi = (d / band) as usize;
                if bi >= bands {
                    break;
                }
                let lean = d % band;
                let cut = &edges[bi];
                let j = cut.iter().position(|&c| c > x - lean / 2).unwrap_or(cut.len()).saturating_sub(1);
                let fx = x - lean / 2 - cut[j];
                let mut c = colours[bi][j];
                // a hedge along some edges -- a broken line of bushes, darker
                // than the fields, not a wall -- and the furrows faint
                let along = d % band == 0 && (bi * 7 + j * 3) % 5 < 3;
                let across = fx == 0 && (bi * 5 + j) % 4 != 0;
                if (along || across) && (x * 5 + y * 3) % 7 != 0 {
                    c = mix(c, if (x + y) % 3 == 0 { hedge[1] } else { hedge[0] }, 0.75);
                } else if d % 2 == 0 {
                    c = mix(c, 0xFF00_0000, 0.05);
                }
                self.set(x, y, c);
            }
        }
    }

    /// The ground plane from row `from` to the foot, in `colours` dithered
    /// into one another down the rows, light to dark.
    fn plane(&mut self, from: i32, colours: &[u32]) {
        let (w, h) = (self.l.w, self.l.h);
        let n = colours.len() - 1;
        for y in from..h {
            let f = (y - from) as f32 / (h - from).max(1) as f32 * n as f32;
            let i = (f as usize).min(n.saturating_sub(1));
            for x in 0..w {
                let up = (f - i as f32) * 16.0 > super::BAYER[y as usize & 3][x as usize & 3] as f32;
                let c = colours[(i + up as usize).min(n)];
                self.set(x, y, c);
            }
        }
    }
}

/// The five tones of a summer crown, of an autumn one.
fn leaves(b: &Brush) -> [u32; 5] {
    b.cs(["LeafShade", "LeafDark", "Leaf", "LeafLight", "LeafBright"])
}

fn autumn(b: &Brush) -> [u32; 5] {
    b.cs(["AutumnShade", "AutumnDark", "Autumn", "AutumnLight", "AutumnBright"])
}

fn pines(b: &Brush) -> [u32; 3] {
    b.cs(["PineDark", "Pine", "PineLight"])
}

// ---------------------------------------------------------------------------
//  The twelve places
// ---------------------------------------------------------------------------

/// Clear: a lake among mountains.  Blue peaks with snow on them far off, a
/// wooded shore, the lake still enough to hold the sky, a boat on it, and
/// on the near side a meadow of flowers, reeds at its edge and tall pines.
fn lake(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    b.haze = 70;
    let far = b.cs(["MountFarDark", "MountFarDark", "MountFar"]);
    let snow = b.cs(["SnowShade", "Snow"]);
    b.peaks(&[(18.0, 82.0), (74.0, 70.0), (126.0, 84.0), (176.0, 66.0), (236.0, 80.0)], 0.6, 116, far, snow, 80.0);
    b.haze = 40;
    let shore = b.waves(108.0, &[(2.5, 70.0), (1.2, 23.0)]);
    let pine = pines(b);
    b.forest(&shore, (5, 10), pine, None, 120);
    b.haze = 0;
    let top = 118;
    let (water, deep) = (b.c("Water"), b.c("WaterDeep"));
    let near: Vec<i32> = (0..w).map(|x| (145.0 + 5.0 * (x as f32 / 37.0).sin() + 2.0 * (x as f32 / 11.0 + 1.0).sin()) as i32).collect();
    for x in 0..w {
        for y in top..near[x as usize] {
            b.wet(x, y, mix(water, deep, (y - top) as f32 / 24.0), y - top);
        }
    }
    // a rowing boat, and its oars
    let wood = b.cs(["Wood", "WoodDark"]);
    b.rect(148, 125, 10, 1, wood[0]);
    b.rect(149, 126, 8, 1, wood[1]);
    b.line(146.0, 127.0, 150.0, 124.0, wood[1]);
    b.line(160.0, 127.0, 156.0, 124.0, wood[1]);
    b.set(153, 123, b.c("RoofRed"));
    b.set(153, 124, b.c("RoofRedDark"));
    // the meadow, lighter on its bank, darker towards us
    let grass = b.cs(["GrassLight", "Grass", "Grass", "GrassDark"]);
    let lowest = *near.iter().min().unwrap_or(&136);
    for x in 0..w {
        for y in near[x as usize]..h {
            let f = (y - near[x as usize]) as f32 / (h - near[x as usize]).max(1) as f32;
            let c = if y == near[x as usize] { b.c("Sand") } else { grass[((f * 3.2) as usize + 1).min(3)] };
            b.set(x, y, c);
        }
    }
    let blades = b.cs(["GrassDark", "GrassLight", "Grass"]);
    b.blades(0, w, lowest + 2, h, &blades, 900);
    let reeds = b.cs(["GrassDry", "GrassDark", "WheatDark"]);
    for _ in 0..40 {
        let x = b.int(4, 80);
        let len = b.int(3, 8);
        let c = b.pick(&reeds);
        for k in 0..len {
            b.set(x, near[x as usize] - k, c);
        }
    }
    let colours = b.cs(["FlowerPink", "FlowerYellow", "FlowerWhite", "FlowerBlue"]);
    b.flowers(0, w, lowest + 6, h, &colours, 140);
    // tall pines on the right, and a bush on the left
    b.pine(212, 176, 62, pine, None);
    b.pine(233, 190, 92, pine, None);
    b.pine(196, 168, 30, pine, None);
    let bush = leaves(b);
    b.tree(22, 168, 26, 16, 1, bush);
    b.tree(42, 172, 16, 10, 1, bush);
    // a cabin on the far shore, its window lit after dark, smoke rising;
    // its jetty; and ducks out on the water
    let walls = b.cs(["Wood", "WoodDark"]);
    let roofs = b.cs(["RoofRed", "RoofRedDark"]);
    b.haze = 30;
    b.house(&House { x: 38, base: top, w: 8, side: 6, wall: 5, roof: 4, walls, roofs, lit: 1.0, chimney: true, logs: true });
    b.haze = 0;
    for x in 52..62 {
        b.set(x, top, walls[0]);
    }
    for x in (52..62).step_by(3) {
        b.set(x, top + 1, walls[1]);
    }
    b.lane(Ride::Duck, (top + 13) as f32, 0.0, 70.0, 190.0, 2.5, true, 3);
    b.l.ground = (top, h - 1);
    b.l.meadow = Some([0, lowest, w, h - lowest]);
    b.l.fog = vec![110.0, 126.0];
}

/// Partly cloudy: farmland from a hillside.  Far hills in a patchwork of
/// fields with hedgerows between them and a farmhouse; nearer fields; and in
/// front a grassy knoll with an oak on it, a path, and a fence along it.
fn farmland(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let fields = b.cs(["Field", "GrassLight", "Wheat", "Grass", "GrassDry", "Field"]);
    let hedge = b.cs(["PineDark", "LeafDark"]);
    b.haze = 120;
    let far = b.waves(102.0, &[(4.0, 90.0), (2.0, 31.0)]);
    b.patchwork(&far, 130, &fields, hedge, (18, 40), 6);
    let walls = b.cs(["Plaster", "PlasterShade"]);
    let roofs = b.cs(["RoofRed", "RoofRedDark"]);
    let at = Brush::top(&far, 60) + 6;
    b.house(&House { x: 52, base: at, w: 8, side: 6, wall: 5, roof: 4, walls, roofs, lit: 0.8, chimney: false, logs: false });
    let crown = leaves(b);
    b.tree(48, at, 7, 6, 1, crown);
    b.tree(72, at + 1, 8, 6, 1, crown);
    b.haze = 55;
    let middle = b.waves(124.0, &[(6.0, 110.0), (3.0, 41.0)]);
    b.patchwork(&middle, 160, &fields, hedge, (34, 70), 12);
    for _ in 0..7 {
        let x = b.int(0, w);
        let y = Brush::top(&middle, x) + b.int(2, 14);
        let s = b.int(6, 11);
        b.tree(x, y, s, s * 3 / 4, 2, crown);
    }
    b.haze = 0;
    // the knoll the oak stands on
    let mut near = b.waves(150.0, &[(4.0, 150.0), (2.0, 50.0)]);
    for (x, y) in near.iter_mut().enumerate() {
        *y -= 12.0 * (-((x as f32 - 176.0) / 34.0).powi(2)).exp();
    }
    let grass = b.cs(["GrassLight", "Grass", "GrassDark"]);
    b.hill(&near, h, grass, 2);
    let blades = b.cs(["GrassDark", "GrassLight", "Grass"]);
    b.blades(0, w, 140, h, &blades, 700);
    let colours = b.cs(["FlowerYellow", "FlowerWhite", "FlowerPink"]);
    b.flowers(0, w, 150, h, &colours, 70);
    // the path, winding up from us to the far fields
    let earth = b.cs(["Earth", "EarthLight", "EarthDark"]);
    let foot = Brush::top(&near, 60);
    for y in foot..h {
        let f = (y - foot) as f32 / (h - foot) as f32;
        let cx = 70.0 - 28.0 * f + 6.0 * (f * 5.0).sin();
        let half = 1.0 + 8.0 * f;
        for x in (cx - half).round() as i32..=(cx + half).round() as i32 {
            let c = if (x as f32 - cx).abs() > half - 1.0 { earth[2] } else if (x + y) % 5 == 0 { earth[1] } else { earth[0] };
            b.set(x, y, c);
        }
        // the fence along its left
        if y % 9 == 0 {
            let post = (cx - half - 6.0).round() as i32;
            let tall = 2 + (f * 7.0) as i32;
            for k in 0..tall {
                b.set(post, y - k, b.c("WoodDark"));
            }
            b.set(post + 1, y - tall + 1, b.c("Wood"));
        }
    }
    // the oak, and its shadow on the grass
    let base = Brush::top(&near, 176) + 3;
    b.oval(176.0, base as f32, 26.0, 3.0, b.c("GrassDark"));
    b.tree(176, base, 64, 48, 14, crown);
    // a tractor working a field on the far side
    let furrow = Brush::top(&middle, 30) + 8;
    b.lane(Ride::Tractor, furrow as f32, 0.0, -12.0, 140.0, 3.5, false, 1);
    // sheep grazing on the nearer fields, and two by the oak
    let (wool, face) = (b.c("Trim"), b.c("Bird"));
    for &(x, y) in &[(96, 138), (104, 141), (116, 137), (88, 144), (214, 160), (224, 163)] {
        let y = y.max(Brush::top(&middle, x) + 2);
        let near = y > 150;
        let (bw, bh) = if near { (4, 3) } else { (3, 2) };
        b.rect(x, y - bh, bw, bh, wool);
        b.set(x + bw, y - bh, face);
        if near {
            b.set(x + bw, y - bh + 1, face);
            b.set(x, y, face);
            b.set(x + bw - 1, y, face);
        }
    }
    b.l.ground = (106, h - 1);
}

/// Overcast: a town's roofs.  A skyline far off with a church spire and a
/// mill chimney; a street of houses, chimneys smoking, windows lit after
/// dark; the tiled roof just below us with its chimney stack; and a
/// telegraph pole with its wires across the view, a bird on one.
fn town(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let slate = b.cs(["SlateDark", "Slate"]);
    let warm = b.c("WindowLit");
    b.haze = 150;
    let mut x = -4;
    while x < w + 4 {
        let bw = b.int(7, 18);
        let top = b.int(98, 110);
        let gable = b.chance(0.55);
        for xx in x..x + bw {
            let t = if gable { top - (bw / 2 - (xx - x - bw / 2).abs()) / 2 } else { top };
            for y in t..124 {
                b.set(xx, y, if xx == x { slate[1] } else { slate[0] });
            }
        }
        for _ in 0..b.int(0, 3) {
            let (wx, wy) = (b.int(x + 1, x + bw - 1), b.int(top + 3, 122));
            b.light(wx, wy, warm);
        }
        x += bw;
    }
    // the church: its tower and spire
    let stone = b.cs(["RockDark", "Rock"]);
    b.rect(146, 80, 8, 44, stone[0]);
    b.rect(146, 80, 3, 44, stone[1]);
    for r in 0..24 {
        let half = r / 6;
        for dx in -half..=half {
            b.set(150 + dx - 1 + (dx >= 0) as i32, 56 + r, if dx < 0 { stone[1] } else { stone[0] });
        }
    }
    b.set(149, 88, b.c("Trim"));
    b.light(149, 96, warm);
    // the mill chimney
    let brick = b.cs(["BrickDark", "Brick"]);
    b.rect(30, 70, 4, 40, brick[0]);
    b.rect(30, 70, 2, 40, brick[1]);
    b.rect(29, 70, 6, 2, brick[0]);
    b.l.chimneys.push((31, 68));
    // a street of houses
    b.haze = 30;
    let walls = [b.cs(["Plaster", "PlasterShade"]), b.cs(["PlasterYellow", "PlasterShade"]), b.cs(["Brick", "BrickDark"])];
    let roofs = [b.cs(["RoofRed", "RoofRedDark"]), b.cs(["Slate", "SlateDark"])];
    let mut x = -10;
    while x < w + 6 {
        let (hw, side) = (b.int(14, 22), b.int(6, 11));
        let wi = b.int(0, 3) as usize;
        let ri = b.int(0, 2) as usize;
        let house = House {
            x,
            base: 150,
            w: hw,
            side,
            wall: b.int(14, 20),
            roof: b.int(8, 12),
            walls: walls[wi],
            roofs: roofs[ri],
            lit: 0.45,
            chimney: b.chance(0.7),
            logs: false,
        };
        b.house(&house);
        x += hw + side + b.int(1, 5);
    }
    // the gardens' hedge behind them
    let hedge = b.cs(["LeafDark", "Leaf"]);
    for x in 0..w {
        for y in 150..156 {
            b.set(x, y, if (x * 3 + y) % 7 == 0 { hedge[1] } else { hedge[0] });
        }
    }
    b.haze = 0;
    // the gardens below, and the roof just below us: courses of tiles down
    // to the foot
    let garden = b.cs(["GrassDark", "LeafDark", "Grass"]);
    for y in 156..h {
        for x in 0..w {
            let c = if (x * 7 + y * 5) % 13 == 0 { garden[2] } else if (x + y * 3) % 5 == 0 { garden[1] } else { garden[0] };
            b.set(x, y, c);
        }
    }
    let fence = b.c("WoodDark");
    for x in 0..w {
        b.set(x, 160, fence);
        if x % 6 == 0 {
            for y in 157..164 {
                b.set(x, y, fence);
            }
        }
    }
    let tiles = b.cs(["RoofRed", "RoofRedDark", "Brick"]);
    for y in 156..h {
        let left = 104 - (y - 156) * 3;
        for x in left.max(0)..w {
            let course = (y - 156) / 3;
            let joint = (x + course * 3) % 7 == 0 || (y - 156) % 3 == 0;
            b.set(x, y, if x == left { tiles[1] } else if joint { tiles[1] } else if (x + y) % 11 == 0 { tiles[2] } else { tiles[0] });
        }
    }
    // its chimney stack, with two pots, and an aerial
    let stack = b.cs(["Brick", "BrickDark"]);
    b.rect(196, 140, 14, 18, stack[0]);
    b.rect(205, 140, 5, 18, stack[1]);
    for y in (141..158).step_by(3) {
        for x in 196..210 {
            if (x + y / 3 * 2) % 5 == 0 {
                b.set(x, y, stack[1]);
            }
        }
    }
    b.rect(195, 139, 16, 2, b.c("Plaster"));
    b.rect(198, 135, 3, 4, b.c("RoofRedDark"));
    b.rect(204, 136, 3, 3, b.c("RoofRedDark"));
    b.l.chimneys.push((199, 134));
    b.l.chimneys.push((205, 135));
    let pole = b.c("Pole");
    b.line(214.0, 158.0, 214.0, 122.0, pole);
    b.line(208.0, 126.0, 220.0, 126.0, pole);
    b.line(210.0, 130.0, 218.0, 130.0, pole);
    // the telegraph pole and its wires
    let (a, z) = b.pole(14, h, 140);
    let wire = b.c("Wire");
    b.wire(a.0, a.1, w as f32 + 2.0, 74.0, 12.0, wire);
    b.wire(z.0, z.1, w as f32 + 2.0, 80.0, 14.0, wire);
    // a bird on the wire
    let bird = b.c("Bird");
    let (bx, by) = (118, 0);
    let y = (a.1 + (74.0 - a.1) * ((bx as f32 - a.0) / (w as f32 + 2.0 - a.0))
        + 12.0 * 4.0 * ((bx as f32 - a.0) / (w as f32 + 2.0 - a.0)) * (1.0 - (bx as f32 - a.0) / (w as f32 + 2.0 - a.0))).round() as i32
        + by;
    b.rect(bx, y - 3, 2, 3, bird);
    b.set(bx + 2, y - 3, bird);
    b.set(bx - 1, y - 2, bird);
    // a cat sitting on the roof below, its tail hanging
    const CAT: [&str; 6] = ["x.x..", "xxx..", "xxx..", ".xxx.", ".xxxx", "...x."];
    for (r, row) in CAT.iter().enumerate() {
        for (k, c) in row.bytes().enumerate() {
            if c == b'x' {
                b.set(150 + k as i32, 152 + r as i32, bird);
            }
        }
    }
    b.l.ground = (120, h - 1);
}

/// Fog: a valley.  Ridge behind ridge, each fainter, pines along every
/// crest; a river winding down between them to us; and on the left a crag
/// with an old pine leaning out from it.
fn valley(b: &mut Brush) {
    let h = b.l.h;
    let pine = pines(b);
    let layers = [(86.0, 8.0, 205u8, (3, 6)), (100.0, 8.0, 170, (4, 7)), (115.0, 9.0, 130, (4, 8)), (132.0, 9.0, 85, (5, 10))];
    for &(y, amp, haze, sizes) in &layers {
        b.haze = haze;
        let line = b.waves(y, &[(amp, 130.0), (amp * 0.45, 43.0), (amp * 0.2, 13.0)]);
        b.forest(&line, sizes, pine, None, h);
    }
    // the river, winding down from the valley's floor to us
    b.haze = 60;
    let (water, deep) = (b.c("WaterLight"), b.c("Water"));
    for y in 138..h {
        let f = (y - 138) as f32 / (h - 138) as f32;
        let cx = 128.0 + 14.0 * (y as f32 / 9.0).sin() - 20.0 * f;
        let half = 1.5 + f * 30.0;
        for x in (cx - half).round() as i32..=(cx + half).round() as i32 {
            b.wet(x, y, mix(water, deep, f), y - 138);
        }
    }
    // the nearest ridge, parted where the river comes through
    b.haze = 25;
    let mut line = b.waves(152.0, &[(10.0, 130.0), (4.0, 43.0), (2.0, 13.0)]);
    for (x, y) in line.iter_mut().enumerate() {
        let gap = (x as f32 - 112.0) / 34.0;
        *y += 40.0 * (-gap * gap).exp();
    }
    b.forest(&line, (6, 12), pine, None, h);
    // the crag, and the pine leaning out from it
    b.haze = 0;
    // the crag: a rounded top, ledges across its face each lit along its
    // lip with a shadow under it, cracks down between them, its face to the
    // valley in shade
    let rock = b.cs(["RockDark", "Rock", "RockLight"]);
    let edge = |y: i32| 52 - (y - 104) / 5 + ((y as f32 / 6.0).sin() * 2.5) as i32 - (8 - (y - 104).min(8)).pow(2) / 6;
    for y in 104..h {
        for x in 0..edge(y) {
            let c = if y == 104 || x >= edge(y) - 2 { rock[0] } else { rock[1] };
            b.set(x, y, c);
        }
    }
    let mut y = 108;
    while y < h {
        let from = b.int(0, 20);
        let to = (edge(y) - b.int(2, 8)).max(from + 4);
        for x in from..to {
            b.set(x, y, rock[2]);
            b.set(x, y + 1, rock[0]);
        }
        for _ in 0..b.int(2, 5) {
            let x = b.int(2, edge(y) - 3);
            let len = b.int(3, 9);
            for k in 2..len {
                b.set(x, y + k, rock[0]);
            }
        }
        y += b.int(6, 11);
    }
    // moss and grass along its top
    let grass = b.cs(["GrassDark", "Grass"]);
    for x in 0..edge(104) - 1 {
        b.set(x, 104, grass[(x % 3 == 0) as usize]);
        if b.chance(0.4) {
            b.set(x, 103, grass[0]);
        }
    }
    let bark = b.c("BarkDark");
    let trunk = [(36.0, 105.0), (44.0, 98.0), (54.0, 95.0), (66.0, 88.0), (78.0, 86.0)];
    for pair in trunk.windows(2) {
        b.line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, bark);
        b.line(pair[0].0, pair[0].1 + 1.0, pair[1].0, pair[1].1 + 1.0, bark);
    }
    b.line(54.0, 95.0, 52.0, 84.0, bark);
    b.line(66.0, 88.0, 72.0, 78.0, bark);
    for &(px, py, rx) in &[(52.0, 82.0, 8.0), (72.0, 76.0, 9.0), (82.0, 84.0, 8.0), (60.0, 90.0, 6.0), (42.0, 96.0, 6.0)] {
        b.oval(px, py, rx, 2.6, pine[0]);
        b.oval(px - 1.0, py - 1.0, rx - 2.0, 1.4, pine[1]);
        b.oval(px - 2.0, py - 1.5, rx * 0.4, 0.8, pine[2]);
    }
    // a waterfall down the crag's face, into a pool of foam at its foot
    let pool = b.cs(["WaterLight", "Foam"]);
    for x in 30..46 {
        for y in 160..166 {
            if b.chance(0.5) {
                b.set(x, y, pool[(x + y) as usize % 2]);
            }
        }
    }
    b.l.falls = Some((34, 108, 162, 3));
    b.l.ground = (128, h - 1);
    b.l.fog = vec![94.0, 108.0, 124.0, 142.0, 164.0];
}

/// Drizzle: an avenue in autumn.  A path running away between two rows of
/// trees in orange and gold, lamps along it, the misty hills it leads to,
/// fallen leaves on the verges, and puddles on the path holding the sky.
fn avenue(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let (vx, vy) = (123.0f32, 104.0f32);
    b.haze = 150;
    let hills = b.waves(99.0, &[(3.0, 60.0), (1.5, 21.0)]);
    let green = b.cs(["GrassDark", "Grass", "GrassLight"]);
    b.hill(&hills, vy as i32 + 2, green, 1);
    b.haze = 0;
    let verge = b.cs(["GrassDry", "Grass", "GrassDark", "GrassDark"]);
    b.plane(vy as i32 + 1, &verge);
    let earth = b.cs(["EarthLight", "Earth", "EarthDark"]);
    let half = |y: f32| 0.5 + 0.5 * (y - vy);
    for y in vy as i32..h {
        let hw = half(y as f32);
        for x in (vx - hw).round() as i32..=(vx + hw).round() as i32 {
            let edge = (x as f32 - vx).abs() > hw - 1.5;
            let c = if edge { earth[2] } else if (x * 7 + y * 13) % 9 == 0 { earth[0] } else { earth[1] };
            b.set(x, y, c);
        }
    }
    // puddles on the path
    let water = b.c("Water");
    for &(py, dx, rx, ry) in &[(170.0f32, -16.0f32, 15.0f32, 4.0f32), (150.0, 9.0, 8.0, 2.2), (128.0, -3.0, 3.5, 1.0)] {
        let top = (py - ry).floor() as i32;
        for y in top..=(py + ry).ceil() as i32 {
            for x in (vx + dx - rx) as i32..=(vx + dx + rx) as i32 {
                let (ex, ey) = ((x as f32 + 0.5 - vx - dx) / rx, (y as f32 + 0.5 - py) / ry);
                if ex * ex + ey * ey <= 1.0 {
                    b.wet(x, y, water, y - top);
                }
            }
        }
    }
    // the fallen leaves, thicker nearer
    let fallen = b.cs(["AutumnDark", "Autumn", "AutumnLight", "AutumnBright"]);
    for _ in 0..520 {
        let f = b.rng.unit().sqrt();
        let y = vy as i32 + 2 + (f * (h as f32 - vy - 2.0)) as i32;
        let x = b.int(0, w);
        let on_path = (x as f32 - vx).abs() < half(y as f32) - 2.0;
        if on_path && b.chance(0.8) {
            continue;
        }
        let c = b.pick(&fallen);
        b.set(x, y, c);
        if f > 0.6 {
            b.set(x + 1, y, c);
        }
    }
    // the two rows of trees, far to near, and the lamps between them
    let crown = autumn(b);
    let lamp = b.c("LampLit");
    let iron = b.c("Pole");
    for k in (0..8).rev() {
        let s = 1.0 / (1.0 + k as f32 * 0.85);
        let base = (vy + (h as f32 + 14.0 - vy) * s) as i32;
        let off = 104.0 * s;
        let (cw, ch, trunk) = (((58.0 * s) as i32).max(4), ((48.0 * s) as i32).max(3), ((64.0 * s) as i32).max(3));
        for side in [-1.0f32, 1.0] {
            b.tree((vx + side * off) as i32, base, cw, ch, trunk, crown);
        }
        if k % 2 == 1 {
            let x = (vx + 72.0 * s) as i32;
            let tall = (60.0 * s) as i32;
            let lb = base - (6.0 * s) as i32;
            for y in lb - tall..lb {
                b.set(x, y, iron);
            }
            b.rect(x - 1, lb - tall - 2, 3, 2, iron);
            b.light(x, lb - tall, lamp);
            if s > 0.3 {
                b.light(x - 1, lb - tall, lamp);
                b.light(x + 1, lb - tall, lamp);
            }
            b.l.lamps.push((x, lb - tall));
        }
    }
    // benches along the verges, facing the path
    let wood = b.cs(["Wood", "WoodDark"]);
    for &(x, y, s) in &[(50, 160, 2), (190, 160, 2), (88, 128, 1), (156, 128, 1)] {
        let len = 4 * s + 2;
        for k in 0..len {
            b.set(x + k, y - 2 * s, wood[0]);
            b.set(x + k, y - s, wood[1]);
        }
        for k in 0..s * 2 {
            b.set(x, y - k, wood[1]);
            b.set(x + len - 1, y - k, wood[1]);
        }
    }
    b.l.canopy = Some([0, 30, w, 70]);
    b.l.ground = (vy as i32 + 4, h - 1);
}

/// Rain: a street.  House fronts across it -- brick and plaster, a shop with
/// its awning and its window lit, rows of windows above -- the pavement,
/// two street lamps, a car at the kerb, and the road, wet, shining with all
/// of it.
fn street(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let (kerb, road) = (140, 148);
    let trim = b.c("Trim");
    let warm = b.c("WindowLit");
    let fronts: [(i32, i32, i32, [&str; 2]); 4] = [
        (-4, 60, 44, ["Brick", "BrickDark"]),
        (56, 62, 30, ["Plaster", "PlasterShade"]),
        (118, 56, 50, ["PlasterYellow", "PlasterShade"]),
        (174, 80, 36, ["BrickDark", "Brick"]),
    ];
    for (i, &(x, fw, top, names)) in fronts.iter().enumerate() {
        let wall = b.cs(names);
        let brick = names[0].starts_with("Brick");
        for y in top..kerb {
            for xx in x..x + fw {
                let mortar = brick && ((y - top) % 3 == 2 || (xx + (y - top) / 3 * 3) % 7 == 0);
                b.set(xx, y, if mortar { mix(wall[0], wall[1], 0.6) } else { wall[0] });
            }
        }
        // its roofline: a cornice and, behind it, a roof or a parapet
        b.rect(x, top, fw, 2, trim);
        b.rect(x, top + 2, fw, 1, wall[1]);
        let slate = b.cs(["Slate", "SlateDark"]);
        if i % 2 == 0 {
            for r in 0..8 {
                b.rect(x + r, top - 8 + r, fw - 2 * r, 1, slate[(r % 2) as usize]);
            }
        }
        let stack = x + fw - 10;
        b.rect(stack, top - 12, 5, 12, b.c("BrickDark"));
        b.rect(stack - 1, top - 13, 7, 1, trim);
        b.l.chimneys.push((stack + 2, top - 14));
        // the windows, floor by floor
        let shop = i == 0 || i == 2;
        let mut y = top + 6;
        while y + 10 < kerb - 18 {
            let mut wx = x + 5;
            while wx + 6 < x + fw - 2 {
                let lit = b.chance(0.45);
                b.rect(wx - 1, y - 1, 7, 11, trim);
                b.window(wx, y, 5, 9, lit, trim);
                for yy in y..y + 9 {
                    if !lit {
                        b.set(wx + 2, yy, trim);
                    }
                }
                b.rect(wx, y + 4, 5, 1, trim);
                wx += 12;
            }
            y += 17;
        }
        // the shop, or the front door
        if shop {
            for yy in kerb - 16..kerb - 3 {
                for xx in x + 4..x + fw - 12 {
                    b.light(xx, yy, warm);
                }
            }
            for xx in x + 3..x + fw - 11 {
                for yy in kerb - 20..kerb - 17 {
                    let stripe = ((xx - x) / 3) % 2 == 0;
                    b.set(xx, yy, if stripe { b.c("RoofRed") } else { trim });
                }
                b.set(xx, kerb - 17, b.c("RoofRedDark"));
            }
            b.rect(x + fw - 9, kerb - 15, 5, 15, b.c("WoodDark"));
        } else {
            b.rect(x + fw / 2 - 3, kerb - 15, 6, 15, b.c("WoodDark"));
            b.rect(x + fw / 2 - 4, kerb - 16, 8, 1, trim);
            b.light(x + fw / 2 - 1, kerb - 14, warm);
            b.light(x + fw / 2, kerb - 14, warm);
        }
        // a drainpipe down its edge
        for yy in top + 2..kerb {
            b.set(x + fw - 1, yy, b.c("Pole"));
        }
    }
    // the pavement, the kerb, and the road shining with the rain
    let paving = b.cs(["AsphaltLight", "Asphalt"]);
    for y in kerb..road {
        for x in 0..w {
            let joint = (y - kerb) % 4 == 3 || (x + (y - kerb) / 4 * 4) % 9 == 0;
            b.set(x, y, if y == road - 1 { trim } else if joint { paving[1] } else { paving[0] });
        }
    }
    let asphalt = b.c("Asphalt");
    for y in road..h {
        for x in 0..w {
            b.slick(x, y, asphalt, y - road);
        }
    }
    let line = b.c("RoadLine");
    for x in 0..w {
        if (x / 13) % 2 == 0 {
            b.set(x, 168, line);
        }
    }
    // two street lamps
    let iron = b.c("Pole");
    let lamp = b.c("LampLit");
    for &lx in &[34, 206] {
        for y in kerb - 64..kerb + 2 {
            b.set(lx, y, iron);
            b.set(lx + 1, y, iron);
        }
        b.rect(lx - 1, kerb, 4, 2, iron);
        b.rect(lx - 2, kerb - 69, 6, 1, iron);
        for y in kerb - 68..kerb - 64 {
            for xx in lx - 1..lx + 3 {
                b.light(xx, y, lamp);
            }
        }
        b.rect(lx - 1, kerb - 64, 4, 1, iron);
        b.l.lamps.push((lx, kerb - 66));
    }
    // a car at the kerb
    let body = b.cs(["RoofRed", "RoofRedDark"]);
    let glass = b.c("Glass");
    b.rect(94, road - 1, 40, 7, body[0]);
    b.rect(94, road + 5, 40, 1, body[1]);
    for r in 0..6 {
        b.rect(104 - r / 2, road - 7 + r, 20 + r, 1, body[0]);
    }
    b.rect(105, road - 5, 8, 3, glass);
    b.rect(115, road - 5, 8, 3, glass);
    b.rect(92, road + 1, 2, 2, b.c("WindowLit"));
    for &cx in &[102.0, 126.0] {
        b.oval(cx, road as f32 + 6.0, 3.0, 3.0, b.c("Wire"));
        b.set(cx as i32, road + 6, b.c("AsphaltLight"));
    }
    // a traffic light at the crossing, and people along the pavement
    let iron = b.c("Pole");
    for y in kerb - 24..kerb + 2 {
        b.set(152, y, iron);
    }
    b.rect(151, kerb - 29, 3, 7, iron);
    b.l.signal = Some((152, kerb - 28));
    b.lane(Ride::Walker, (road - 2) as f32, 0.0, -8.0, w as f32 + 8.0, 8.0, true, 5);
    b.l.ground = (road, h - 1);
}

/// Heavy rain: a coast.  A headland far off, the open sea, a rock with a
/// lighthouse on it and the keeper's cottage, and a jetty running out from
/// us with a lamp at its end.
fn coast(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let sea = 112;
    b.haze = 160;
    let rock = b.cs(["RockDark", "Rock", "RockLight"]);
    let head: Vec<f32> = (0..w).map(|x| 100.0 + (x as f32 / 78.0).powi(2) * 14.0).collect();
    b.hill(&head, sea, rock, 1);
    b.haze = 0;
    let (water, deep) = (b.c("Water"), b.c("WaterDeep"));
    for y in sea..h {
        for x in 0..w {
            b.wet(x, y, mix(water, deep, (y - sea) as f32 / (h - sea) as f32), y - sea);
        }
    }
    // the rock, the grass on it, and the surf at its foot
    let mut crag = b.waves(96.0, &[(3.0, 40.0), (1.5, 13.0)]);
    for (x, y) in crag.iter_mut().enumerate() {
        let x = x as f32;
        if x < 156.0 {
            *y = 999.0;
        } else if x < 180.0 {
            *y += (180.0 - x) * 0.9;
        }
    }
    for x in 156..w {
        let t = Brush::top(&crag, x);
        let slope = crag[(x + 1).min(w - 1) as usize] - crag[(x - 1) as usize];
        for y in t..sea + 6 {
            let c = if y - t < 2 { b.c("GrassDark") } else if slope < -0.3 && y - t < 6 { rock[2] } else if (x + y / 2) % 6 == 0 { rock[1] } else { rock[0] };
            b.set(x, y, c);
        }
    }
    let foam = b.c("Foam");
    for x in 152..w {
        for y in sea + 4..sea + 8 {
            if b.chance(0.35) && b.get(x, y - 1) != 0 {
                b.set(x, y, foam);
            }
        }
    }
    // the lighthouse: white with red bands, a gallery, the lantern, a cap
    let base = Brush::top(&crag, 206) + 1;
    let white = b.cs(["Trim", "PlasterShade"]);
    let red = b.cs(["RoofRed", "RoofRedDark"]);
    let top = base - 38;
    for y in top..base {
        let f = (y - top) as f32 / 38.0;
        let half = (3.0 + 2.0 * f).round() as i32;
        let band = ((y - top) / 7) % 2 == 1;
        for dx in -half..=half {
            let shade = (dx > 0) as usize;
            b.set(206 + dx, y, if band { red[shade] } else { white[shade] });
        }
    }
    b.rect(201, top - 2, 11, 2, b.c("Pole"));
    for y in top - 6..top - 2 {
        for x in 204..209 {
            b.light(x, y, b.c("LampLit"));
        }
    }
    b.set(203, top - 4, b.c("Pole"));
    b.set(209, top - 4, b.c("Pole"));
    b.oval(206.0, top as f32 - 7.0, 3.5, 2.0, red[1]);
    b.l.beacon = Some((206, top - 4));
    let walls = b.cs(["Plaster", "PlasterShade"]);
    let roofs = b.cs(["Slate", "SlateDark"]);
    b.house(&House { x: 218, base: Brush::top(&crag, 222) + 2, w: 9, side: 8, wall: 7, roof: 5, walls, roofs, lit: 1.0, chimney: true, logs: false });
    // the jetty, running out from us, and its lamp
    let wood = b.cs(["Wood", "WoodDark"]);
    for y in 132..h {
        let f = (y - 132) as f32 / (h - 132) as f32;
        let (left, right) = ((30.0 - 40.0 * f) as i32, (44.0 + 30.0 * f) as i32);
        // boards across it, thicker nearer, a dark seam between each, the
        // grain showing, the edge in shade
        let board = 1.5 + 4.0 * f;
        let seam = ((y as f32 - 132.0) / board).fract() < 1.0 / board;
        for x in left..=right {
            let grain = (x * 5 + (y as f32 / board) as i32 * 7) % 9 == 0;
            b.set(x, y, if x >= right - 1 || seam { wood[1] } else if grain { mix(wood[0], wood[1], 0.5) } else { wood[0] });
        }
        if y % 6 == 0 {
            for k in 1..(3.0 + 6.0 * f) as i32 {
                b.set(right + 1, y + k, wood[1]);
            }
        }
    }
    let iron = b.c("Pole");
    for y in 112..132 {
        b.set(40, y, iron);
    }
    b.rect(38, 109, 5, 3, b.c("LampLit"));
    for x in 38..43 {
        b.light(x, 110, b.c("LampLit"));
    }
    b.rect(38, 108, 5, 1, iron);
    b.l.lamps.push((40, 110));
    b.rect(36, 126, 3, 6, wood[1]);
    // a fishing boat far out, a buoy blinking, gulls overhead
    b.lane(Ride::Tug, (sea + 8) as f32, 0.0, -20.0, w as f32 + 20.0, 3.0, false, 1);
    b.l.blinkers.push((118, sea + 30, b.c("PaintRed")));
    b.l.gulls = true;
    b.l.sea = Some((sea, h));
    b.l.ground = (sea, h - 1);
}

/// Sleet: a country road in winter.  Muddy fields with slush lying on them,
/// a row of bare trees far off, a farmhouse with a light on, the wet road
/// running away to them, telegraph poles along it with their wires, and two
/// bare trees near us.
fn winter_road(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let (vx, vy) = (100.0f32, 112.0f32);
    b.haze = 140;
    let twig = b.c("BarkDark");
    let mut x = 0;
    while x < w {
        let tall = b.int(3, 9);
        let lean = b.rng.range(-0.2, 0.2);
        b.limb(x as f32, vy, lean, tall as f32, 1.0, 1, twig);
        x += b.int(2, 6);
    }
    let walls = b.cs(["Plaster", "PlasterShade"]);
    let roofs = b.cs(["Slate", "SlateDark"]);
    b.house(&House { x: 186, base: vy as i32 + 4, w: 10, side: 9, wall: 7, roof: 6, walls, roofs, lit: 1.0, chimney: true, logs: false });
    b.haze = 0;
    // ploughed fields, the furrows running away to the horizon and the slush
    // lying in them -- thicker in some fields than others, a hedge between
    let mud = b.cs(["EarthDark", "Earth", "GrassDry"]);
    let slush = b.cs(["SnowShade", "Snow"]);
    for y in vy as i32 + 1..h {
        let d = (y as f32 - vy).max(1.0);
        for x in 0..w {
            let u = (x as f32 - vx) / d;
            // which field, and how much of it is under slush
            let field = (u * 0.9).floor() as i32;
            let cover = 0.35 + 0.3 * ((field * 37 % 7) as f32 / 7.0);
            // the furrows: ridges of mud with slush in the troughs
            let furrow = (u * 9.0).rem_euclid(1.0);
            let lies = furrow < cover && ((x * 13 + y * 7) % 11 != 0);
            // far off, where a furrow is narrower than a pixel, only the
            // mud and the slush mixed, dithered
            if d < 14.0 {
                let up = cover * 16.0 > super::BAYER[y as usize & 3][x as usize & 3] as f32;
                b.set(x, y, if up { slush[0] } else { mud[1] });
                continue;
            }
            let c = if (u * 0.9).rem_euclid(1.0) < 0.03 * (60.0 / d).min(3.0) {
                mud[2]
            } else if lies {
                slush[(furrow < cover * 0.5) as usize]
            } else if furrow > 0.85 {
                mud[0]
            } else {
                mud[1]
            };
            b.set(x, y, c);
        }
    }
    // the road, wet, the slush along its edges, its centre line
    let asphalt = b.c("Asphalt");
    let line = b.c("RoadLine");
    let half = |y: f32| 0.5 + 0.62 * (y - vy);
    for y in vy as i32..h {
        let hw = half(y as f32);
        for x in (vx - hw).round() as i32..=(vx + hw).round() as i32 {
            let edge = (x as f32 - vx).abs() > hw - 1.0 - (hw * 0.08);
            if edge && b.chance(0.6) {
                let s = slush[b.int(0, 2) as usize];
                b.set(x, y, s);
            } else {
                b.slick(x, y, asphalt, y - vy as i32);
            }
        }
        let z = 60.0 / (y as f32 - vy).max(1.0);
        if (z * 4.0) as i32 % 2 == 0 {
            b.set(vx.round() as i32, y, line);
            if y > 150 {
                b.set(vx.round() as i32 + 1, y, line);
            }
        }
    }
    // the poles along the left, far to near, and the wires between them
    let wire = b.c("Wire");
    let mut last: Option<((f32, f32), (f32, f32))> = None;
    for k in (0..7).rev() {
        let s = 1.0 / (1.0 + k as f32 * 1.0);
        let base = (vy + (h as f32 + 8.0 - vy) * s) as i32;
        let x = (vx - 8.0 - 116.0 * s) as i32;
        let arms = b.pole(x, base, ((100.0 * s) as i32).max(4));
        if let Some((a, z)) = last {
            b.wire(a.0, a.1, arms.0 .0, arms.0 .1, 4.0 * s, wire);
            b.wire(z.0, z.1, arms.1 .0, arms.1 .1, 4.0 * s, wire);
        }
        last = Some(arms);
    }
    // two bare trees, near, and a fence along the right
    let bark = b.c("BarkDark");
    b.limb(212.0, h as f32 + 2.0, -0.08, 44.0, 4.0, 4, bark);
    b.limb(238.0, h as f32 - 6.0, 0.1, 30.0, 3.0, 3, bark);
    let wood = b.cs(["Wood", "WoodDark"]);
    for k in 0..9 {
        let s = 1.0 / (1.0 + k as f32 * 0.6);
        let base = (vy + (h as f32 - vy) * s) as i32;
        let x = (vx + 30.0 + 110.0 * s) as i32;
        let tall = ((14.0 * s) as i32).max(2);
        for y in base - tall..base {
            b.set(x, y, wood[1]);
        }
        b.set(x, base - tall, slush[1]);
    }
    // a warning sign by the road: deer crossing
    let (red, white) = (b.c("PaintRed"), b.c("PaintWhite"));
    for y in 146..162 {
        b.set(60, y, b.c("Pole"));
    }
    for r in 0..8i32 {
        for dx in -r / 2..=r / 2 {
            let edge = dx.abs() == r / 2 || r == 7;
            b.set(60 + dx, 139 + r, if edge { red } else { white });
        }
    }
    b.set(60, 143, b.c("Wire"));
    b.set(59, 144, b.c("Wire"));
    b.set(61, 144, b.c("Wire"));
    b.l.ground = (vy as i32 + 2, h - 1);
}

/// Snow: a cabin in a pine forest.  Snowy hills far off with a forest on
/// them, the snow drifted up to a log cabin -- its window lit, smoke from
/// its chimney, logs stacked by it -- pines about it, a fence half under the
/// snow, and footprints to the door.
fn cabin(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let snow = b.cs(["SnowShade", "Snow"]);
    let pine = pines(b);
    b.haze = 150;
    let far = b.waves(100.0, &[(4.0, 90.0), (2.0, 33.0)]);
    b.hill(&far, 124, [snow[0], snow[1], snow[1]], 2);
    let ridge = b.waves(104.0, &[(2.0, 60.0)]);
    b.forest(&ridge, (3, 6), pine, Some(snow[1]), 110);
    b.haze = 60;
    let middle = b.waves(118.0, &[(3.0, 70.0), (1.5, 23.0)]);
    b.forest(&middle, (6, 12), pine, Some(snow[1]), 128);
    b.haze = 0;
    // the drifts, their far sides in shade
    let drifts = b.waves(126.0, &[(3.0, 80.0), (1.5, 27.0)]);
    b.hill(&drifts, h, [snow[0], snow[1], snow[1]], 3);
    for _ in 0..120 {
        let (x, y) = (b.int(0, w), b.int(130, h));
        b.set(x, y, snow[0]);
    }
    // the cabin
    let walls = b.cs(["Wood", "WoodDark"]);
    b.house(&House { x: 112, base: 154, w: 26, side: 20, wall: 14, roof: 12, walls, roofs: snow, lit: 1.0, chimney: true, logs: true });
    for x in 108..162 {
        b.set(x, 154, snow[1]);
        b.set(x, 153, if x % 3 == 0 { snow[0] } else { snow[1] });
    }
    // the woodpile
    let wood = b.cs(["EarthLight", "WoodDark"]);
    for row in 0..3 {
        for k in 0..5 - row {
            let (lx, ly) = (162 + k * 3 + row, 152 - row * 2);
            b.set(lx, ly, wood[0]);
            b.set(lx + 1, ly, wood[1]);
        }
    }
    b.rect(161, 145, 15, 1, snow[1]);
    // pines about it
    for &(x, base, tall) in &[(30, 166, 66), (12, 160, 52), (52, 150, 44), (200, 160, 50), (224, 178, 86), (88, 140, 26)] {
        b.pine(x, base, tall, pine, Some(snow));
    }
    // the fence, near, half under the snow, and footprints to the door
    let post = b.cs(["Wood", "WoodDark"]);
    for x in (2..w).step_by(15) {
        let base = 172 + (x as f32 / 30.0).sin() as i32 * 2;
        for y in base - 7..base {
            b.set(x, y, post[0]);
            b.set(x + 1, y, post[1]);
        }
        b.set(x, base - 8, snow[1]);
        b.set(x + 1, base - 8, snow[1]);
        if x + 15 < w {
            b.line(x as f32 + 2.0, base as f32 - 5.0, x as f32 + 14.0, base as f32 - 5.0, post[1]);
        }
    }
    for k in 0..18 {
        let f = k as f32 / 18.0;
        let x = (84.0 + (125.0 - 84.0) * f + if k % 2 == 0 { -1.5 } else { 1.5 }) as i32;
        let y = (h as f32 - 2.0 - (h as f32 - 2.0 - 156.0) * f) as i32;
        b.set(x, y, snow[0]);
        if f < 0.5 {
            b.set(x + 1, y, snow[0]);
        }
    }
    // a snowman in the garden: coal eyes, a carrot, stick arms
    let white = b.cs(["SnowShade", "Snow"]);
    // each ball shaded on its far side, lit on the near
    for &(cy, r) in &[(171.0f32, 4.5f32), (163.5, 3.5), (157.5, 2.5)] {
        b.oval(84.0, cy, r, r, white[0]);
        b.oval(83.5, cy - 0.5, r - 1.0, r - 1.0, white[1]);
    }
    let coal = b.c("Wire");
    b.set(83, 157, coal);
    b.set(85, 157, coal);
    b.set(84, 164, coal);
    b.set(84, 166, coal);
    b.set(85, 158, b.c("PaintOrange"));
    b.set(86, 158, b.c("PaintOrange"));
    b.line(80.0, 162.0, 76.0, 158.0, b.c("BarkDark"));
    b.line(88.0, 162.0, 92.0, 159.0, b.c("BarkDark"));
    b.rect(81, 153, 7, 1, coal);
    b.rect(82, 150, 5, 3, coal);
    b.l.ground = (124, h - 1);
}

/// Heavy snow: the Alps.  Great peaks, rock and snow; a forest of snowy firs
/// on the slopes under them; a hut with a light on; drifts near us, and two
/// big firs on the left.
fn alps(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let rock = b.cs(["RockDark", "Rock", "RockLight"]);
    let snow = b.cs(["SnowShade", "Snow"]);
    let pine = pines(b);
    b.haze = 70;
    b.peaks(&[(28.0, 48.0), (98.0, 20.0), (152.0, 46.0), (206.0, 28.0), (252.0, 56.0)], 1.05, 140, rock, snow, 66.0);
    b.haze = 30;
    let slope = b.waves(122.0, &[(4.0, 90.0), (2.0, 29.0)]);
    b.forest(&slope, (6, 13), pine, Some(snow[1]), h);
    // the hut on a knoll
    b.haze = 15;
    let knoll: Vec<f32> = (0..w).map(|x| 138.0 - 10.0 * (-((x as f32 - 184.0) / 26.0).powi(2)).exp()).collect();
    b.hill(&knoll, h, [snow[0], snow[1], snow[1]], 2);
    let walls = b.cs(["Wood", "WoodDark"]);
    b.house(&House { x: 176, base: 130, w: 12, side: 10, wall: 8, roof: 7, walls, roofs: snow, lit: 1.0, chimney: true, logs: true });
    b.haze = 0;
    let drifts = b.waves(152.0, &[(5.0, 90.0), (2.0, 30.0)]);
    b.hill(&drifts, h, [snow[0], snow[1], snow[1]], 3);
    b.pine(22, h + 6, 92, pine, Some(snow));
    b.pine(46, h + 2, 60, pine, Some(snow));
    b.pine(232, h + 4, 50, pine, Some(snow));
    // a cable car up the mountain: its cable, its pylons, and the cars
    let wire = b.c("Wire");
    let (x0, y0, x1, y1) = (96.0f32, 74.0f32, 252.0f32, 126.0f32);
    let slope = (y1 - y0) / (x1 - x0);
    b.wire(x0, y0, x1, y1, 0.0, wire);
    for &px in &[136.0f32, 196.0] {
        let top = y0 + (px - x0) * slope - 1.0;
        b.line(px - 3.0, top + 22.0, px, top, b.c("Pole"));
        b.line(px + 3.0, top + 22.0, px, top, b.c("Pole"));
        b.line(px - 2.0, top, px + 2.0, top, b.c("Pole"));
    }
    b.lane(Ride::Gondola, y0 - x0 * slope + 1.0, slope, x0 + 4.0, x1, 5.0, true, 2);
    b.l.ground = (126, h - 1);
}

/// Thunderstorm: a prairie.  A flat land to a low horizon -- a thin line of
/// trees on it, a farmhouse and its windmill -- in bands of grass, gold and
/// green, narrowing away; a track across it; a fence; and near us a lone
/// dead tree.
fn prairie(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let horizon = 128;
    b.haze = 150;
    let crown = leaves(b);
    let mut x = 0;
    while x < w {
        if b.chance(0.55) {
            let s = b.int(3, 6);
            b.tree(x, horizon + 1, s, (s * 2 / 3).max(2), 1, crown);
        }
        x += b.int(3, 8);
    }
    let walls = b.cs(["Plaster", "PlasterShade"]);
    let roofs = b.cs(["RoofRed", "RoofRedDark"]);
    b.house(&House { x: 182, base: horizon + 2, w: 9, side: 7, wall: 6, roof: 5, walls, roofs, lit: 1.0, chimney: false, logs: false });
    // the windmill: a lattice tower, and the hub its sails turn on
    let iron = b.c("Pole");
    let (mx, top) = (216, horizon - 26);
    b.line(mx as f32 - 4.0, horizon as f32 + 1.0, mx as f32 - 1.0, top as f32, iron);
    b.line(mx as f32 + 4.0, horizon as f32 + 1.0, mx as f32 + 1.0, top as f32, iron);
    for y in (top + 4..horizon).step_by(5) {
        let half = 1 + (y - top) * 3 / 26;
        b.line((mx - half) as f32, y as f32, (mx + half) as f32, (y + 4) as f32, iron);
        b.line((mx + half) as f32, y as f32, (mx - half) as f32, (y + 4) as f32, iron);
    }
    b.rect(mx - 1, top - 2, 4, 3, b.c("WoodDark"));
    b.l.mill = Some((mx, top - 1, 11));
    b.haze = 0;
    // the bands of grass, narrow far off and broad near
    let bands = b.cs(["GrassDry", "Wheat", "Field", "WheatDark", "GrassDry", "Grass", "Wheat"]);
    let strokes = b.cs(["WheatDark", "GrassDark", "Wheat", "GrassDry"]);
    let mut y = horizon as f32 + 1.0;
    let mut k = 0usize;
    while (y as i32) < h {
        let tall = 1.0 + 0.09 * (y - horizon as f32) * (1.0 + 0.3 * (k as f32 * 1.7).sin());
        let c = bands[k % bands.len()];
        for yy in y as i32..(y + tall) as i32 {
            for x in 0..w {
                b.set(x, yy, c);
            }
        }
        y += tall;
        k += 1;
    }
    b.blades(0, w, horizon + 4, h, &strokes, 1400);
    // the track, two ruts running off to the farm
    let ruts = b.cs(["EarthDark", "Earth"]);
    for y in horizon + 1..h {
        let f = (y - horizon) as f32 / (h - horizon) as f32;
        let cx = 186.0 - 60.0 * f * f;
        let gap = 0.5 + 9.0 * f;
        for side in [-1.0f32, 1.0] {
            let x = (cx + side * gap).round() as i32;
            b.set(x, y, ruts[0]);
            if f > 0.4 {
                b.set(x + 1, y, ruts[1]);
            }
        }
    }
    // a fence running across, and the lone tree
    let wood = b.c("WoodDark");
    for k in 0..12 {
        let f = k as f32 / 11.0;
        let (x, base) = ((f * 130.0) as i32, (152.0 - 18.0 * f) as i32);
        let tall = (10.0 - 6.0 * f) as i32;
        for yy in base - tall..base {
            b.set(x, yy, wood);
        }
        if k > 0 {
            let (px, pb) = (((f - 1.0 / 11.0) * 130.0) as f32, 152.0 - 18.0 * (f - 1.0 / 11.0));
            b.line(px, pb - (10.0 - 6.0 * (f - 1.0 / 11.0)) * 0.7, x as f32, base as f32 - tall as f32 * 0.7, wood);
        }
    }
    let bark = b.c("BarkDark");
    b.limb(52.0, h as f32, -0.06, 36.0, 4.0, 5, bark);
    // cattle grazing far off, and tumbleweed rolling with the wind
    let (black, white) = (b.c("PaintBlack"), b.c("PaintWhite"));
    for &(x, y) in &[(98, 134), (106, 135), (122, 133), (150, 136)] {
        b.rect(x, y - 3, 5, 2, black);
        b.set(x + 1, y - 3, white);
        b.set(x + 3, y - 2, white);
        b.set(x + 5, y - 3, black);
        b.set(x, y - 1, black);
        b.set(x + 4, y - 1, black);
    }
    b.lane(Ride::Weed, (h - 14) as f32, 0.0, -10.0, w as f32 + 10.0, 14.0, false, 2);
    b.l.ground = (horizon, h - 1);
}

/// Hail: a farm.  Hills far off; the farmyard; a red barn with white trim,
/// its big doors and its hay loft; the silo beside it; round bales of hay;
/// and on the left the crop rows running away to the hills.
fn barnyard(b: &mut Brush) {
    let (w, h) = (b.l.w, b.l.h);
    let horizon = 124;
    b.haze = 140;
    let far = b.waves(116.0, &[(4.0, 90.0), (2.0, 30.0)]);
    let green = b.cs(["GrassDark", "Field", "GrassLight"]);
    b.hill(&far, horizon + 2, green, 2);
    let crown = leaves(b);
    for _ in 0..12 {
        let x = b.int(0, w);
        let s = b.int(4, 8);
        b.tree(x, Brush::top(&far, x) + 3, s, s * 2 / 3, 1, crown);
    }
    b.haze = 0;
    let grass = b.cs(["Grass", "Grass", "GrassDark"]);
    b.plane(horizon, &grass);
    let blades = b.cs(["GrassDark", "GrassLight"]);
    b.blades(0, w, horizon + 4, h, &blades, 600);
    // the crop rows, converging on the hills
    let (vx, vy) = (40.0f32, horizon as f32);
    let rows = b.cs(["Soil", "Field"]);
    for y in horizon + 1..h {
        let d = y as f32 - vy;
        for x in 0..w {
            let u = (x as f32 - vx) / d;
            if u > 2.4 {
                continue;
            }
            let c = rows[((u * 6.0).floor() as i32).rem_euclid(2) as usize];
            b.set(x, y, if (y + x) % 5 == 0 && c == rows[1] { b.c("LeafLight") } else { c });
        }
    }
    // the barn: its gable end, a gambrel roof, the doors, the loft
    let red = b.cs(["Barn", "BarnDark"]);
    let trim = b.c("Trim");
    let (bx, bw, base, eave) = (134, 46, 152, 122);
    for y in eave..base {
        for x in bx..bx + bw {
            b.set(x, y, if (x - bx) % 3 == 0 { red[1] } else { red[0] });
        }
    }
    for r in 0..22 {
        // the gambrel: steep below its knee, shallow above it
        let half = if r < 8 { bw / 2 - 2 * r / 3 } else { (bw / 2 - 5) * (22 - r) / 14 };
        let y = eave - r;
        for x in bx + bw / 2 - half..bx + bw / 2 + half {
            b.set(x, y, if (x - bx) % 3 == 0 { red[1] } else { red[0] });
        }
        b.set(bx + bw / 2 - half - 1, y, trim);
        b.set(bx + bw / 2 + half, y, trim);
    }
    let roof = b.cs(["Slate", "SlateDark"]);
    for y in eave - 21..=eave {
        let r = eave - y;
        let half = if r < 8 { bw / 2 - 2 * r / 3 } else { (bw / 2 - 5) * (22 - r) / 14 };
        let left = bx + bw / 2 + half + 1;
        for x in left..left + 22 {
            b.set(x, y, roof[(r % 2) as usize]);
        }
    }
    for y in eave..base {
        for x in bx + bw..bx + bw + 22 {
            b.set(x, y, if (x - bx) % 3 == 0 { red[1] } else { mix(red[1], red[0], 0.4) });
        }
    }
    // the doors, with their white cross-braces, and the loft door
    let (dx, dw, dy) = (bx + 13, 20, 132);
    b.rect(dx, dy, dw, base - dy, red[1]);
    for x in dx..dx + dw {
        b.set(x, dy, trim);
    }
    for y in dy..base {
        b.set(dx, y, trim);
        b.set(dx + dw - 1, y, trim);
        b.set(dx + dw / 2, y, trim);
    }
    b.line(dx as f32, dy as f32, (dx + dw / 2) as f32, base as f32 - 1.0, trim);
    b.line((dx + dw / 2) as f32, dy as f32, dx as f32, base as f32 - 1.0, trim);
    b.line((dx + dw / 2) as f32, dy as f32, (dx + dw) as f32 - 1.0, base as f32 - 1.0, trim);
    b.line((dx + dw) as f32 - 1.0, dy as f32, (dx + dw / 2) as f32, base as f32 - 1.0, trim);
    b.rect(bx + bw / 2 - 4, eave - 12, 8, 9, red[1]);
    b.rect(bx + bw / 2 - 4, eave - 12, 8, 1, trim);
    b.rect(bx + 2, base - 18, 4, 4, b.c("Glass"));
    b.light(bx + 3, base - 17, b.c("WindowLit"));
    b.light(bx + 4, base - 17, b.c("WindowLit"));
    // the silo
    let tin = b.cs(["Trim", "Plaster", "PlasterShade"]);
    let (sx, sw, top) = (208, 16, 84);
    for y in top..base {
        for x in sx..sx + sw {
            let f = (x - sx) as f32 / sw as f32;
            let c = if f < 0.3 { tin[0] } else if f < 0.7 { tin[1] } else { tin[2] };
            b.set(x, y, if (y - top) % 9 == 0 { tin[2] } else { c });
        }
    }
    b.oval(sx as f32 + sw as f32 / 2.0, top as f32, sw as f32 / 2.0, 5.0, roof[0]);
    b.oval(sx as f32 + sw as f32 / 2.0 - 2.0, top as f32 - 1.0, 3.0, 2.0, roof[1]);
    // round bales of hay
    let hay = b.cs(["Wheat", "WheatDark", "EarthLight"]);
    for &(x, y, r) in &[(110.0f32, 150.0f32, 6.0f32), (96.0, 155.0, 7.0), (122.0, 160.0, 8.0)] {
        b.oval(x, y, r, r * 0.85, hay[0]);
        b.oval(x + r * 0.3, y, r * 0.5, r * 0.45, hay[1]);
        b.set(x as i32 + (r * 0.3) as i32, y as i32, hay[2]);
    }
    // the fence along the yard
    let wood = b.cs(["Wood", "WoodDark"]);
    for x in (130..w).step_by(8) {
        for y in 160..170 {
            b.set(x, y, wood[1]);
        }
    }
    for x in 130..w {
        b.set(x, 162, wood[0]);
        b.set(x, 166, wood[0]);
    }
    // a weathervane on the barn's ridge, a scarecrow in the crop rows, and
    // hens about the yard
    let iron = b.c("Wire");
    for y in eave - 28..eave - 21 {
        b.set(bx + bw / 2, y, iron);
    }
    for dx in -3..=3 {
        b.set(bx + bw / 2 + dx, eave - 26, iron);
    }
    b.rect(bx + bw / 2 - 1, eave - 30, 3, 2, iron);
    b.set(bx + bw / 2 + 2, eave - 31, iron);
    let (coat, straw) = (b.c("PaintBlue"), b.c("Wheat"));
    for y in 140..158 {
        b.set(60, y, b.c("WoodDark"));
    }
    for x in 53..68 {
        b.set(x, 145, b.c("WoodDark"));
    }
    b.rect(57, 144, 7, 6, coat);
    b.rect(58, 139, 5, 4, straw);
    b.rect(57, 138, 7, 1, b.c("PaintBrown"));
    b.rect(58, 136, 5, 2, b.c("PaintBrown"));
    b.set(53, 146, straw);
    b.set(67, 146, straw);
    b.lane(Ride::Hen, 174.0, 0.0, 96.0, 200.0, 2.0, true, 4);
    b.l.ground = (horizon, h - 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin;

    #[test]
    fn every_place_is_its_own_and_leaves_sky_above_it() {
        let skin = &skin::weather_window::SKIN;
        let mut seen: Vec<Vec<u32>> = Vec::new();
        // the twelve scenes' own places, and the cities' -- Aberdeen by day
        // and by night
        let places = Scene::ALL
            .iter()
            .map(|&s| (s, None, false))
            .chain([(Scene::Clear, Some(City::London), false), (Scene::Clear, Some(City::Aberdeen), false), (Scene::Clear, Some(City::Aberdeen), true)]);
        for (scene, city, night) in places {
            let land = Land::of(scene, city, night, skin, 246, 183);
            let again = Land::of(scene, city, night, skin, 246, 183);
            let name = format!("{scene:?} {city:?} {night}");
            assert!(land.base == again.base, "{name}: two different places");
            assert!(!seen.contains(&land.base), "{name}: the same place as another");
            seen.push(land.base.clone());
            // sky along the top -- even the street, between its roofs, has
            // some -- and land along the foot
            let skyline = land.skyline();
            let open = skyline.iter().filter(|&&y| y > 25).count();
            assert!(open > 180, "{name}: too little sky ({open} columns)");
            assert!((0..246).all(|x| land.base[(182 * 246 + x) as usize] != 0), "{name}: a hole at the foot");
            assert!(land.ground.0 < land.ground.1 && land.ground.1 < 183);
            for lane in &land.lanes {
                assert!(lane.from < lane.to && lane.count > 0, "{name}: an empty lane");
            }
        }
    }

    #[test]
    fn snow_lies_on_a_city_and_its_streets_shine_in_the_rain() {
        let skin = &skin::weather_window::SKIN;
        let snow = skin.colour("Snow") | 0xFF00_0000;
        let count = |l: &Land| l.base.iter().filter(|&&c| c == snow).count();
        let (dry, snowy) = (Land::of(Scene::Clear, Some(City::London), false, skin, 246, 183), Land::of(Scene::Snow, Some(City::London), false, skin, 246, 183));
        assert!(count(&snowy) > count(&dry) + 100, "no snow on London");
        let wet = |s: Scene| Land::of(s, Some(City::Aberdeen), false, skin, 246, 183).kind.iter().filter(|&&k| k == WATER).count();
        assert!(wet(Scene::Rain) > wet(Scene::Clear) + 1000, "Broad Street dry in the rain");
    }
}
