//! The Weather widget's second look: the weather itself, out of a window
//! onto a place -- the Rain Window's picture grown into one for every kind
//! of weather there is.  `weather` is the widget; this is only the picture.
//!
//! Twelve scenes ([`Scene`]), one for each kind of weather the forecast's WMO
//! codes come to -- clear, partly cloudy, overcast, fog, drizzle, rain, heavy
//! rain, sleet, snow, heavy snow, a thunderstorm and hail -- and each over a
//! place of its own (`land`): a lake among mountains, farmland, a town, a
//! valley, an autumn avenue, a street, a coast, a country road, a forest
//! cabin, the Alps, a prairie and a farm.  Each is built back to front:
//!
//!   the sky      a colour a row, in bands dithered where they meet: night,
//!                dusk or day as the sun stands where the forecast is for,
//!                and clear or grey as the weather is
//!   stars        by night under a clear sky, each twinkling at its own
//!                pace, and now and then one falling
//!   the sun      on an arc from sunrise to sunset, deeper in colour low
//!                down; by night the moon, in the phase it is in tonight
//!   clouds       heaps of puffs, each lit as a ball from the side the sun or
//!                the moon is on (`clouds`): fair-weather cumulus, a grey
//!                layer in rows that shrink towards the horizon, or a
//!                thunderhead with its anvil -- drifting with the wind
//!   birds        now and then, across a fine day
//!   the place    in the light of the moment, the far of it hazed; its
//!                windows and lamps lit after dark; its water showing what
//!                stands over it, rippling; smoke from its chimneys, a
//!                lighthouse's beam, a windmill's sails, falling leaves,
//!                fireflies, the sea's waves
//!   fog          banks of it between the ridges, dithered in whole pixels
//!   what falls   the Rain Window's rain, splashing where it lands and
//!                ringing the water; snow, swaying; sleet; hailstones that
//!                bounce; and lightning, a bolt and the flash it lights
//!                everything with
//!
//! The wind the forecast gives slants what falls and drives the clouds and
//! the smoke.  Every colour is one of the skin's -- nature's, untouched by
//! the tint -- mixed: nothing is drawn in a colour the skin does not name.
//!
//! It draws into a picture of its own, the pane's size, a frame at a time;
//! the widget puts that up, and the readings over it.  What changes only
//! with the light -- the sky's bands, the place in its light, the clouds'
//! shading -- is worked out when the light changes, once a minute; a frame
//! copies it, moves what moves, and draws that.

use core::f32::consts::{PI, TAU};

use crate::skin::Skin;

mod clouds;
mod land;
mod sprites;

use clouds::Form;
use land::{Land, FLOOD, LIGHT, SALTIRE, WATER};
use sprites::Ride;

/// The cities with a place of their own: when the forecast is for one, the
/// weather is seen over it instead of over the scene's own place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum City {
    London,
    Aberdeen,
}

impl City {
    pub const ALL: [City; 2] = [City::London, City::Aberdeen];

    /// What the menu calls it.
    pub fn label(self) -> &'static str {
        match self {
            City::London => "London",
            City::Aberdeen => "Aberdeen",
        }
    }

    /// What settings.ini calls it.
    pub fn key(self) -> &'static str {
        match self {
            City::London => "london",
            City::Aberdeen => "aberdeen",
        }
    }

    pub fn from_key(key: &str) -> Option<City> {
        City::ALL.into_iter().find(|c| c.key().eq_ignore_ascii_case(key.trim()))
    }

    /// Its middle, in degrees: Charing Cross, and the Castlegate.
    fn centre(self) -> (f64, f64) {
        match self {
            City::London => (51.5074, -0.1278),
            City::Aberdeen => (57.1497, -2.0943),
        }
    }

    /// The city a place is in -- within 25 km of its middle -- if it is one
    /// of them.  By where, not by name: the IP address may say Westminster,
    /// or Bridge of Don.
    pub fn near(lat: f64, lon: f64) -> Option<City> {
        City::ALL.into_iter().find(|c| {
            let (clat, clon) = c.centre();
            let dy = (lat - clat) * 111.2;
            let dx = (lon - clon) * 111.2 * clat.to_radians().cos();
            dx * dx + dy * dy < 25.0 * 25.0
        })
    }
}

/// Milliseconds between frames while something falls -- twenty-five a
/// second, as the Rain Window's -- while only clouds and stars move, and in
/// fog, which drifts a pixel or two a second and is the dearest to draw.
const FALLING_MS: u32 = 40;
const CALM_MS: u32 = 80;
const FOG_MS: u32 = 160;

/// The twelve scenes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    Clear,
    Partly,
    Overcast,
    Fog,
    Drizzle,
    Rain,
    Downpour,
    Sleet,
    Snow,
    Blizzard,
    Storm,
    Hail,
}

impl Scene {
    pub const ALL: [Scene; 12] = [
        Scene::Clear,
        Scene::Partly,
        Scene::Overcast,
        Scene::Fog,
        Scene::Drizzle,
        Scene::Rain,
        Scene::Downpour,
        Scene::Sleet,
        Scene::Snow,
        Scene::Blizzard,
        Scene::Storm,
        Scene::Hail,
    ];

    /// What the menu calls it.
    pub fn label(self) -> &'static str {
        match self {
            Scene::Clear => "Clear",
            Scene::Partly => "Partly cloudy",
            Scene::Overcast => "Overcast",
            Scene::Fog => "Fog",
            Scene::Drizzle => "Drizzle",
            Scene::Rain => "Rain",
            Scene::Downpour => "Heavy rain",
            Scene::Sleet => "Sleet",
            Scene::Snow => "Snow",
            Scene::Blizzard => "Heavy snow",
            Scene::Storm => "Thunderstorm",
            Scene::Hail => "Hail",
        }
    }

    /// What settings.ini calls it.
    pub fn key(self) -> &'static str {
        match self {
            Scene::Clear => "clear",
            Scene::Partly => "partly",
            Scene::Overcast => "overcast",
            Scene::Fog => "fog",
            Scene::Drizzle => "drizzle",
            Scene::Rain => "rain",
            Scene::Downpour => "heavy-rain",
            Scene::Sleet => "sleet",
            Scene::Snow => "snow",
            Scene::Blizzard => "heavy-snow",
            Scene::Storm => "storm",
            Scene::Hail => "hail",
        }
    }

    pub fn from_key(key: &str) -> Option<Scene> {
        let key = key.trim();
        Scene::ALL.into_iter().find(|s| s.key().eq_ignore_ascii_case(key))
    }

    /// The scene for a WMO weather code, as Open-Meteo gives one.  Mainly
    /// clear is clear (the clear sky has a cloud or two of its own);
    /// freezing drizzle and freezing rain are sleet; snow grains are snow; a
    /// storm with hail is hail.  What is not a code is overcast: a grey sky
    /// says nothing it does not know.
    pub fn of_code(code: i32) -> Scene {
        match code {
            0 | 1 => Scene::Clear,
            2 => Scene::Partly,
            45 | 48 => Scene::Fog,
            51 | 53 | 55 => Scene::Drizzle,
            56 | 57 | 66 | 67 => Scene::Sleet,
            61 | 63 | 80 | 81 => Scene::Rain,
            65 | 82 => Scene::Downpour,
            71 | 73 | 77 | 85 => Scene::Snow,
            75 | 86 => Scene::Blizzard,
            95 => Scene::Storm,
            96 | 99 => Scene::Hail,
            _ => Scene::Overcast,
        }
    }
}

/// Where the sun is, as the scene needs it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Light {
    /// -1 deep in the night, 0 with the sun on the horizon, 1 as high as it
    /// climbs today.  Between -0.6 and 0.3 is dusk -- or dawn.
    pub day: f32,
    /// How far across the sky the sun has come: 0 at sunrise, 1 at sunset.
    pub sun: f32,
    /// How far through the night it is, sunset to sunrise.
    pub night: f32,
    /// The moon's age, as a share of the month: 0 new, 0.5 full.
    pub moon: f32,
}

impl Light {
    /// Midday and midnight, for when the sun's times are not known.
    pub const NOON: Light = Light { day: 0.85, sun: 0.5, night: 0.0, moon: 0.5 };
    pub const MIDNIGHT: Light = Light { day: -1.0, sun: 1.0, night: 0.5, moon: 0.5 };
}

// ---------------------------------------------------------------------------
//  What each scene is made of
// ---------------------------------------------------------------------------

/// What the sky is covered with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cover {
    None,
    /// A few small cumulus, low, and streaks of high cloud.
    Fair,
    /// Big fair-weather cumulus, with sky between.
    Scattered,
    /// A grey layer, row on row to the horizon.
    Layer,
    /// A thunderhead, with a ragged layer over it.
    Tower,
}

/// Rain: how many drops are in the air, how fast and how long they are
/// against the Rain Window's, and whether they splash or only dot the
/// ground.
#[derive(Clone, Copy, Debug)]
struct Rainfall {
    drops: usize,
    speed: f32,
    len: f32,
    splash: bool,
}

#[derive(Clone, Copy, Debug)]
struct Mood {
    /// 0 a clear sky, 1 a grey one.
    gloom: f32,
    /// How much darker again, towards a storm's.
    dark: f32,
    cover: Cover,
    rain: Option<Rainfall>,
    /// Snowflakes in the air, and whether it is a blizzard.
    snow: usize,
    blizzard: bool,
    /// Ice in the air: sleet's pellets, or hailstones.
    ice: usize,
    hail: bool,
    fog: bool,
    /// Seconds between strikes, from and to.
    lightning: Option<(f32, f32)>,
    /// The sun or the moon shows, and birds may cross.
    open: bool,
}

fn mood(scene: Scene) -> Mood {
    let calm = Mood {
        gloom: 0.0,
        dark: 0.0,
        cover: Cover::None,
        rain: None,
        snow: 0,
        blizzard: false,
        ice: 0,
        hail: false,
        fog: false,
        lightning: None,
        open: false,
    };
    let rain = |drops, speed, len| Some(Rainfall { drops, speed, len, splash: true });
    let grey = Mood { cover: Cover::Layer, ..calm };
    match scene {
        Scene::Clear => Mood { cover: Cover::Fair, open: true, ..calm },
        Scene::Partly => Mood { gloom: 0.1, cover: Cover::Scattered, open: true, ..calm },
        Scene::Overcast => Mood { gloom: 0.85, ..grey },
        Scene::Fog => Mood { gloom: 0.75, fog: true, ..calm },
        Scene::Drizzle => Mood {
            gloom: 0.8,
            dark: 0.05,
            rain: Some(Rainfall { drops: 130, speed: 0.55, len: 0.35, splash: false }),
            ..grey
        },
        Scene::Rain => Mood { gloom: 0.9, dark: 0.12, rain: rain(90, 1.0, 1.0), ..grey },
        Scene::Downpour => Mood { gloom: 1.0, dark: 0.25, rain: rain(170, 1.3, 1.35), ..grey },
        Scene::Sleet => Mood { gloom: 0.9, dark: 0.12, rain: rain(45, 1.1, 0.8), ice: 40, ..grey },
        Scene::Snow => Mood { gloom: 0.8, dark: 0.06, snow: 160, ..grey },
        Scene::Blizzard => Mood { gloom: 0.95, dark: 0.18, snow: 300, blizzard: true, ..grey },
        Scene::Storm => Mood { gloom: 0.7, dark: 0.35, cover: Cover::Tower, rain: rain(140, 1.3, 1.3), lightning: Some((2.5, 7.0)), ..calm },
        Scene::Hail => Mood {
            gloom: 0.7,
            dark: 0.3,
            cover: Cover::Tower,
            rain: rain(55, 1.25, 1.1),
            ice: 54,
            hail: true,
            lightning: Some((6.0, 13.0)),
            ..calm
        },
    }
}

impl Mood {
    fn falls(&self) -> bool {
        self.rain.is_some() || self.snow > 0 || self.ice > 0
    }
}

// ---------------------------------------------------------------------------
//  Colour, chance and the picture
// ---------------------------------------------------------------------------

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |shift: u32| -> u32 {
        let (a, b) = (((a >> shift) & 255) as f32, ((b >> shift) & 255) as f32);
        (a + (b - a) * t + 0.5) as u32
    };
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn mix5(a: [u32; 5], b: [u32; 5], t: f32) -> [u32; 5] {
    core::array::from_fn(|i| mix(a[i], b[i], t))
}

/// How light a colour looks, 0 to 1.
fn luma(c: u32) -> f32 {
    let ch = |shift: u32| ((c >> shift) & 255) as f32 / 255.0;
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

/// 0 below `lo`, 1 above `hi`, and an S between.
fn smooth(lo: f32, hi: f32, v: f32) -> f32 {
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The 4 x 4 Bayer matrix: the order in which a cell's pixels tip over as a
/// shade between two levels deepens.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// xorshift32: all the chance the weather needs.
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

    /// `0 <= n < 1`
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// `lo <= n < hi`
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// The seed a fixed scene starts from: the same weather every time.
const SEED: u32 = 0x9E37_79B9;

/// The picture: opaque 0xFFRRGGBB, a row at a time.
struct Pic {
    px: Vec<u32>,
    w: i32,
    h: i32,
}

impl Pic {
    #[inline]
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        ((0..self.w).contains(&x) && (0..self.h).contains(&y)).then(|| (y * self.w + x) as usize)
    }

    #[inline]
    fn set(&mut self, x: i32, y: i32, c: u32) {
        if let Some(i) = self.at(x, y) {
            self.px[i] = c;
        }
    }

    /// `c` over what is there, `a` of the way.
    #[inline]
    fn blend(&mut self, x: i32, y: i32, c: u32, a: f32) {
        if let Some(i) = self.at(x, y) {
            self.px[i] = mix(self.px[i], c, a);
        }
    }

    /// A soft round glow of `c` about (x, y), `a` at its heart.
    fn glow(&mut self, x: i32, y: i32, r: i32, c: u32, a: f32) {
        for dy in -r..=r {
            for dx in -r..=r {
                let d = ((dx * dx + dy * dy) as f32).sqrt() / r as f32;
                if d < 1.0 {
                    self.blend(x + dx, y + dy, c, a * (1.0 - d) * (1.0 - d));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
//  The skin's colours, and what the light makes of them
// ---------------------------------------------------------------------------

/// Every colour of the sky and the weather, as the skin gives them.
#[derive(Clone, Copy, Default)]
struct Palette {
    sky_night: [u32; 2],
    sky_dusk: [u32; 2],
    sky_day: [u32; 2],
    grey_night: [u32; 2],
    grey_dusk: [u32; 2],
    grey_day: [u32; 2],
    storm: u32,
    /// Core, rim, glow, and the colour of a setting sun.
    sun: [u32; 4],
    moon: [u32; 2],
    star: u32,
    cloud_day: [u32; 5],
    cloud_dusk: [u32; 5],
    cloud_night: [u32; 5],
    cloud_grey: [u32; 5],
    cloud_storm: [u32; 5],
    /// The light the land is seen in by day, at dusk and by night.
    light: [u32; 3],
    glass: u32,
    lamp: u32,
    rain: u32,
    glint: u32,
    flake: u32,
    ice: u32,
    ice_shade: u32,
    fog_day: u32,
    fog_night: u32,
    bolt: u32,
    flash: u32,
    bird: u32,
    smoke: u32,
    firefly: u32,
    foam: u32,
    sail: u32,
    leaves: [u32; 4],
    /// The light floodlit stone is lit by after dark.
    flood: u32,
    /// The London Eye's steel, and its light after dark.
    steel: u32,
    eye: u32,
    /// A clock's hands.
    hands: u32,
    /// The flags' red, white and blue, and a traffic light's red, amber and
    /// green.
    flags: [u32; 3],
    signals: [u32; 3],
    /// Mica in granite catching the sun.
    mica: u32,
    /// A waterfall's water and its foam.
    falls: [u32; 2],
}

impl Palette {
    fn read(skin: &Skin) -> Palette {
        let c = |name: &str| skin.colour(name);
        let pair = |a: &str, b: &str| [c(a), c(b)];
        let five = |stem: &str| core::array::from_fn(|i| c(&format!("{stem}{}", i + 1)));
        Palette {
            sky_night: pair("SkyNight1", "SkyNight2"),
            sky_dusk: pair("SkyDusk1", "SkyDusk2"),
            sky_day: pair("SkyDay1", "SkyDay2"),
            grey_night: pair("GreyNight1", "GreyNight2"),
            grey_dusk: pair("GreyDusk1", "GreyDusk2"),
            grey_day: pair("GreyDay1", "GreyDay2"),
            storm: c("StormSky"),
            sun: [c("SunCore"), c("SunRim"), c("SunGlow"), c("SunLow")],
            moon: pair("MoonLit", "MoonSea"),
            star: c("Star"),
            cloud_day: five("CloudDay"),
            cloud_dusk: five("CloudDusk"),
            cloud_night: five("CloudNight"),
            cloud_grey: five("CloudGrey"),
            cloud_storm: five("CloudStorm"),
            light: [c("LightDay"), c("LightDusk"), c("LightNight")],
            glass: c("Glass"),
            lamp: c("LampLit"),
            rain: c("Rain"),
            glint: c("RainGlint"),
            flake: c("Flake"),
            ice: c("Ice"),
            ice_shade: c("IceShade"),
            fog_day: c("FogDay"),
            fog_night: c("FogNight"),
            bolt: c("Bolt"),
            flash: c("Flash"),
            bird: c("Bird"),
            smoke: c("Smoke"),
            firefly: c("Firefly"),
            foam: c("Foam"),
            sail: c("Plaster"),
            leaves: [c("AutumnDark"), c("Autumn"), c("AutumnLight"), c("AutumnBright")],
            flood: c("FloodLight"),
            steel: c("Trim"),
            eye: c("EyeLight"),
            hands: c("Wire"),
            flags: [c("PaintRed"), c("PaintWhite"), c("PaintBlue")],
            signals: [c("PaintRed"), c("PaintYellow"), c("PaintGreen")],
            mica: c("GraniteGlint"),
            falls: [c("WaterLight"), c("Foam")],
        }
    }
}

/// The palette as this light and this weather have it, and what the light
/// makes of the sky and the place: worked out when either changes, not
/// every frame.
#[derive(Default)]
struct Tones {
    /// The sky, a pixel at a time: its bands, dithered.
    backdrop: Vec<u32>,
    /// The place in this light, the far of it hazed; nothing where the sky
    /// shows.
    land: Vec<u32>,
    /// The clouds' five tones, near and far; softer for a layer, lit as it
    /// is from all over; and a thunderhead's, bright where the sun is on it
    /// and slate where it is not.
    ramp: [u32; 5],
    far_ramp: [u32; 5],
    soft_ramp: [u32; 5],
    far_soft: [u32; 5],
    tower_ramp: [u32; 5],
    /// How much the lamps and the windows shine, 0 by day to 1 by night.
    glow: f32,
    rain: u32,
    glint: u32,
    /// Far, middle, near.
    flake: [u32; 3],
    ice: u32,
    ice_shade: u32,
    fog: u32,
    /// How much the stars show, 0 to 1.
    stars: f32,
    star: u32,
    /// Core, rim, glow.
    sun: [u32; 3],
    moon: [u32; 2],
    bird: u32,
    smoke: u32,
    lamp: u32,
    firefly: u32,
    foam: u32,
    sail: u32,
    leaves: [u32; 4],
    flash: u32,
    bolt: u32,
    /// The sky behind the readings is light: they are written dark.
    bright: bool,
    /// Each rider's colours in this light -- dry and in the rain -- by
    /// the character that stands for each.
    rides: Vec<Vec<(u8, u32)>>,
    /// The Eye's rim and its capsules, a clock's hands, the flags, gulls,
    /// a waterfall, mica glinting.
    rim: u32,
    capsule: u32,
    hands: u32,
    flags: [u32; 3],
    gull: u32,
    falls: [u32; 2],
    mica: u32,
}

// ---------------------------------------------------------------------------
//  The things in the picture
// ---------------------------------------------------------------------------

struct Star {
    x: i32,
    y: i32,
    /// How bright at its brightest, how fast it twinkles, and where in its
    /// twinkle it starts.
    base: f32,
    speed: f32,
    phase: f32,
    /// A bright one, with a pixel of light on each side.
    big: bool,
}

/// A cloud in the sky: its form, shaded for the light, where it is, and how
/// it goes.
struct Cloud {
    x: f32,
    y: i32,
    form: Form,
    /// Its form shaded in the light of the moment: a tone a pixel.
    px: Vec<u8>,
    /// Its share of the wind's speed.
    pace: f32,
    /// A layer's: it goes round with the rest of its row, this far, keeping
    /// the row closed.  None for one that goes off and comes back elsewhere,
    /// at a height between `rows`.
    period: Option<f32>,
    rows: (f32, f32),
    /// Towards the horizon, hazed.
    far: bool,
    /// Lit from above, as under a grey sky, whatever side the sun is on.
    flat: bool,
    /// A thunderhead, in its own tones.
    tall: bool,
}

/// A streak of high cloud: a line or two of light across the sky.
struct Streak {
    x: f32,
    y: i32,
    len: i32,
}

/// One depth of rain -- the Rain Window's three, far to near.
struct Layer {
    /// Pixels a second.
    speed: f32,
    /// Pixels from head to tail.
    len: i32,
    /// Its share of the drops in the air.
    share: f32,
    /// How much of what is behind it shows through.
    alpha: f32,
    /// Where between the far and the near edge of the ground its drops land.
    depth: (f32, f32),
}

const FAR: usize = 0;
const MID: usize = 1;
const NEAR: usize = 2;
const LAYERS: [Layer; 3] = [
    Layer { speed: 100.0, len: 4, share: 0.45, alpha: 0.4, depth: (0.0, 0.25) },
    Layer { speed: 160.0, len: 7, share: 0.35, alpha: 0.6, depth: (0.25, 0.6) },
    Layer { speed: 240.0, len: 12, share: 0.20, alpha: 0.85, depth: (0.6, 1.0) },
];
/// Seconds a splash and its ripple last.
const SPLASH_LIFE: f32 = 0.5;

/// Snow's three depths: pixels a second down, and how far a flake sways.
const FLAKE_SPEED: [f32; 3] = [14.0, 22.0, 32.0];
const FLAKE_SWAY: [f32; 3] = [0.8, 1.6, 2.6];

struct Drop {
    layer: usize,
    /// Where it would be at the top of the pane; the wind takes it aside of
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
    /// It fell in water: a ring, not a splash.
    ring: bool,
}

struct Flake {
    layer: usize,
    x: f32,
    y: f32,
    land: f32,
    /// Where in its sway it is, and its shape when it is near.
    phase: f32,
}

/// A pellet of sleet or a hailstone.
struct Stone {
    x: f32,
    y: f32,
    /// Pixels a second, down; up after a bounce.
    vy: f32,
    land: f32,
    /// 1 a pellet, 2 a small stone, 3 a large one.
    size: u8,
    bounces: u8,
    /// Seconds left lying on the ground, once it has stopped bouncing.
    rest: f32,
}

/// A bank of fog: where it lies, how thick, how its edge rolls, and how fast
/// it drifts.
struct Bank {
    y: f32,
    half: f32,
    k: f32,
    phase: f32,
    speed: f32,
    strength: f32,
}

struct Bird {
    x: f32,
    y: f32,
    vx: f32,
    phase: f32,
}

struct Meteor {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    age: f32,
    life: f32,
}

struct Strike {
    age: f32,
    /// The bolt's pixels; none for sheet lightning, a flash and no bolt.
    path: Vec<(i32, i32)>,
}

impl Strike {
    /// How bright its flash is, `age` seconds in: a flicker, a second
    /// stroke, then fading.
    fn flash(&self) -> f32 {
        let a = self.age;
        let f = if a < 0.06 {
            1.0
        } else if a < 0.12 {
            0.35
        } else if a < 0.2 {
            0.85
        } else {
            0.85 * (-(a - 0.2) * 7.0).exp()
        };
        if self.path.is_empty() { f * 0.5 } else { f }
    }

    fn done(&self) -> bool {
        self.age > 0.75
    }
}

/// A puff of smoke from a chimney.
struct Smoke {
    x: f32,
    y: f32,
    age: f32,
}

/// Seconds a puff of smoke lasts, and between two from one chimney.
const SMOKE_LIFE: f32 = 3.6;
const SMOKE_EVERY: f32 = 0.5;

/// A leaf falling from the avenue's trees.
struct Leaf {
    x: f32,
    y: f32,
    speed: f32,
    phase: f32,
    colour: usize,
}

/// A firefly over the meadow.
struct Fly {
    x: f32,
    y: f32,
    phase: f32,
    speed: f32,
}

// ---------------------------------------------------------------------------
//  The sky
// ---------------------------------------------------------------------------

/// Someone, or something, going along a lane.
struct Rider {
    lane: usize,
    x: f32,
    /// 1 going right, -1 going left.
    dir: f32,
    /// Where in its frames it is.
    phase: f32,
}

pub struct Sky {
    w: i32,
    h: i32,
    skin: &'static Skin,
    scene: Scene,
    /// The city whose place this is, if it is one's -- and, for Aberdeen,
    /// whether it is the harbour's turn, after dark.
    city: Option<City>,
    harbour: bool,
    /// The time there, for a clock's hands: hours and minutes.
    clock: (u32, u32),
    riders: Vec<Rider>,
    mood: Mood,
    light: Light,
    /// km/h, plus for a wind blowing to the right.
    wind: f32,
    pal: Palette,
    tones: Tones,
    /// The place, and its water: each pixel of it, and the row it mirrors.
    land: Land,
    water: Vec<(u32, u16)>,
    rng: Rng,
    /// Seconds since the scene began.
    time: f32,

    stars: Vec<Star>,
    /// Far ones first, so the near are drawn over them.
    clouds: Vec<Cloud>,
    streaks: Vec<Streak>,
    drops: Vec<Drop>,
    splashes: Vec<Splash>,
    flakes: Vec<Flake>,
    stones: Vec<Stone>,
    banks: Vec<Bank>,
    birds: Vec<Bird>,
    next_birds: f32,
    meteor: Option<Meteor>,
    next_meteor: f32,
    strike: Option<Strike>,
    next_strike: f32,
    /// The next strike is a bolt, not sheet lightning: for a fixed scene.
    bolt_next: bool,
    smoke: Vec<Smoke>,
    next_smoke: f32,
    leaves: Vec<Leaf>,
    flies: Vec<Fly>,

    /// The fog's scratch: each bank's roll along the row.
    roll: Vec<f32>,
    /// The water's: how far each row sways, twice over.
    ripples: Vec<f32>,
    /// A sine a step at a time, for the sea's waves.
    sine: Vec<f32>,
    pic: Pic,
}

/// Steps in a turn of `Sky::sine`.
const SINE: usize = 1024;

/// The clock of a scene goes back to 0 this often, in seconds, before it can
/// lose its fractions: a picture that runs for weeks moves as smoothly as one
/// just begun.
const TIME_WRAP: f32 = 6.0 * 3600.0;

/// `b` over `a`, `k` 256ths of the way: a blend in whole numbers.
fn blend256(a: u32, b: u32, k: u32) -> u32 {
    let k = k.min(256);
    let ch = |s: u32| ((((a >> s) & 255) * (256 - k) + ((b >> s) & 255) * k + 128) >> 8).min(255);
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

impl Sky {
    /// The scene the skin lays out, over its place, with the clock's own
    /// chance in it.
    pub fn new(skin: &'static Skin, scene: Scene, light: Light, wind: f32, seed: u32) -> Sky {
        let (w, h) = (skin.int("PaneW", 246).max(16), skin.int("PaneH", 183).max(32));
        let land = Land::of(scene, None, false, skin, w, h);
        let mut sky = Sky {
            w,
            h,
            skin,
            scene,
            city: None,
            harbour: false,
            clock: (12, 0),
            riders: Vec::new(),
            mood: mood(scene),
            light,
            wind,
            pal: Palette::read(skin),
            tones: Tones::default(),
            water: Vec::new(),
            land,
            rng: Rng(seed | 1),
            time: 0.0,
            stars: Vec::new(),
            clouds: Vec::new(),
            streaks: Vec::new(),
            drops: Vec::new(),
            splashes: Vec::new(),
            flakes: Vec::new(),
            stones: Vec::new(),
            banks: Vec::new(),
            birds: Vec::new(),
            next_birds: 0.0,
            meteor: None,
            next_meteor: 0.0,
            strike: None,
            next_strike: 0.0,
            bolt_next: false,
            smoke: Vec::new(),
            next_smoke: 0.0,
            leaves: Vec::new(),
            flies: Vec::new(),
            roll: Vec::new(),
            ripples: vec![0.0; 2 * h as usize],
            sine: (0..SINE).map(|i| (i as f32 * TAU / SINE as f32).sin()).collect(),
            pic: Pic { px: vec![0xFF00_0000; (w * h) as usize], w, h },
        };
        sky.find_water();
        sky.fill();
        sky.relight();
        sky.render();
        sky
    }

    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }

    /// The picture as it stands: opaque, a row at a time.
    pub fn pixels(&self) -> &[u32] {
        &self.pic.px
    }

    #[cfg(test)]
    pub fn scene(&self) -> Scene {
        self.scene
    }

    /// Whether the sky behind the readings is light, and they are best
    /// written dark.
    pub fn bright(&self) -> bool {
        self.tones.bright
    }

    pub fn frame_ms(&self) -> u32 {
        if self.mood.falls() {
            FALLING_MS
        } else if self.mood.fog {
            FOG_MS
        } else {
            CALM_MS
        }
    }

    /// Another scene: its place, and its sky made again, already in full
    /// swing.  Whether anything changed.
    pub fn set_scene(&mut self, scene: Scene) -> bool {
        if scene == self.scene {
            return false;
        }
        self.scene = scene;
        self.mood = mood(scene);
        self.rebuild();
        true
    }

    /// The weather over a city's own place -- or, with none, over the
    /// scene's.  Whether anything changed.
    pub fn set_city(&mut self, city: Option<City>) -> bool {
        if city == self.city {
            return false;
        }
        self.city = city;
        self.rebuild();
        true
    }

    /// The time where the forecast is for, for a clock's hands.  Whether
    /// there is a clock to show it.
    pub fn set_clock(&mut self, hour: u32, minute: u32) -> bool {
        let was = core::mem::replace(&mut self.clock, (hour % 24, minute % 60));
        was != self.clock && self.land.clock.is_some()
    }

    /// The place made again -- and its weather -- for the scene, the city
    /// and, in Aberdeen, the time of day.
    fn rebuild(&mut self) {
        self.harbour = self.city == Some(City::Aberdeen) && self.light.day < -0.12;
        self.land = Land::of(self.scene, self.city, self.harbour, self.skin, self.w, self.h);
        self.find_water();
        self.fill();
        self.relight();
    }

    /// The sun somewhere else.  Whether it has moved far enough to show.
    pub fn set_light(&mut self, light: Light) -> bool {
        let l = self.light;
        let near = |a: f32, b: f32| (a - b).abs() < 0.002;
        if near(l.day, light.day) && near(l.sun, light.sun) && near(l.night, light.night) && near(l.moon, light.moon) {
            return false;
        }
        self.light = light;
        // Aberdeen turns to its harbour after dark, and back by day
        if self.city == Some(City::Aberdeen) && (light.day < -0.12) != self.harbour {
            self.rebuild();
        } else {
            self.relight();
        }
        true
    }

    /// Another wind, in km/h, plus blowing to the right.
    pub fn set_wind(&mut self, wind: f32) -> bool {
        if (wind - self.wind).abs() < 0.5 {
            return false;
        }
        self.wind = wind;
        true
    }

    /// The skin's colours again: the picture is drawn afresh.
    pub fn retint(&mut self) {
        self.pal = Palette::read(self.skin);
        self.relight();
        self.render();
    }

    /// For the test harness: the same weather every time, `seconds` in --
    /// and, for a storm, a bolt just struck.
    pub fn set_fixed(&mut self, seconds: f32) {
        self.rng = Rng(SEED);
        self.fill();
        self.relight();
        let step = FALLING_MS as f32 / 1000.0;
        let frames = (seconds / step) as u32;
        if self.mood.lightning.is_some() && frames > 2 {
            // struck in the last frame but one: a frame in, at its brightest
            self.next_strike = frames as f32 * step - 0.05;
            self.bolt_next = true;
        }
        for _ in 0..frames {
            self.step(step);
        }
        self.render();
    }

    /// Every pixel of the place's water, and the row above its surface it
    /// shows -- as far above as it is below.
    fn find_water(&mut self) {
        let w = self.w;
        self.water = (0..self.land.kind.len())
            .filter(|&i| self.land.kind[i] == WATER)
            .map(|i| {
                let y = i as i32 / w;
                let up = (y - 2 * self.land.depth[i] as i32 - 1).max(0);
                (i as u32, up as u16)
            })
            .collect();
    }

    /// The row the sky ends at, behind the place: where the sun sets.
    fn horizon(&self) -> i32 {
        self.land.ground.0
    }

    // ---- the light -----------------------------------------------------------

    /// What the light and the weather make of the palette, the sky, the
    /// place and the clouds.
    fn relight(&mut self) {
        let (p, m, l) = (self.pal, self.mood, self.light);
        let d = l.day;
        // night, dusk and day
        let by_day = |night: u32, dusk: u32, day: u32| {
            if d >= 0.3 {
                day
            } else if d >= 0.0 {
                mix(dusk, day, d / 0.3)
            } else if d >= -0.6 {
                mix(night, dusk, (d + 0.6) / 0.6)
            } else {
                night
            }
        };
        let sky = |i: usize| {
            let clear = by_day(p.sky_night[i], p.sky_dusk[i], p.sky_day[i]);
            let grey = by_day(p.grey_night[i], p.grey_dusk[i], p.grey_day[i]);
            mix(mix(clear, grey, m.gloom), p.storm, m.dark)
        };
        let (top, low) = (sky(0), sky(1));
        // How much of the day's light there is, for what has only a day and
        // a night colour; and how near the sun is to the horizon.
        let lit = smooth(-0.35, 0.2, d);
        let dusk = (1.0 - d.abs() / 0.35).clamp(0.0, 1.0);
        let (w, h) = (self.w, self.h);
        let horizon = self.horizon();

        let t = &mut self.tones;
        // the sky, in bands down to the horizon, dithered where they meet
        const BANDS: f32 = 11.0;
        let band = |i: f32| mix(top, low, (i / BANDS).powf(0.9));
        t.backdrop.resize((w * h) as usize, 0);
        for y in 0..h {
            let f = (y as f32 / horizon.max(1) as f32).min(1.0) * BANDS;
            let (lo, hi) = (band(f.floor()), band((f.floor() + 1.0).min(BANDS)));
            let over = (f - f.floor()) * 16.0;
            for x in 0..w {
                let up = over > BAYER[y as usize & 3][x as usize & 3] as f32 + 0.5;
                t.backdrop[(y * w + x) as usize] = if up { hi } else { lo };
            }
        }

        // the light the place is seen in: by day, at dusk, by night; greyed
        // under a grey sky; and the haze its distance takes on
        let light = mix(mix(p.light[2], p.light[0], lit), p.light[1], dusk * 0.85 * (1.0 - 0.6 * m.gloom));
        let mul = [16, 8, 0].map(|s| ((light >> s) & 255) as f32 / 255.0);
        let (desat, dim) = (0.4 * m.gloom + if m.fog { 0.15 } else { 0.0 }, 1.0 - 0.12 * m.gloom - 0.3 * m.dark);
        let grade = |c: u32| -> u32 {
            let ch = |s: u32, k: usize| ((c >> s) & 255) as f32 * mul[k];
            let (r, g, b) = (ch(16, 0), ch(8, 1), ch(0, 2));
            let grey = 0.3 * r + 0.59 * g + 0.11 * b;
            let f = |v: f32| ((v + (grey - v) * desat) * dim).clamp(0.0, 255.0) as u32;
            0xFF00_0000 | f(r) << 16 | f(g) << 8 | f(b)
        };
        t.fog = mix(mix(p.fog_night, p.fog_day, lit), low, 0.3 * dusk);
        let haze_to = if m.fog { t.fog } else { low };
        let haze_k = 1.0 + 0.5 * m.gloom + if m.fog { 0.9 } else { 0.0 } + if m.blizzard { 0.7 } else { 0.0 };
        t.glow = (1.0 - lit).max(if m.gloom > 0.8 { 0.3 } else { 0.0 });
        let glass = grade(p.glass);
        let land = &self.land;
        t.land.resize(land.base.len(), 0);
        for i in 0..land.base.len() {
            let c = land.base[i];
            if c >> 24 == 0 {
                t.land[i] = 0;
                continue;
            }
            let (g, hz) = match land.kind[i] {
                LIGHT => (mix(glass, c, t.glow), 0.5),
                // stone, floodlit: its colour in the floodlight's, and some
                // of the light itself
                FLOOD if t.glow > 0.0 => {
                    // the stone's own colour, brightened, warmed by the lamps'
                    let lit = |s: u32| {
                        let own = ((c >> s) & 255) as f32;
                        (own * 1.15 * 0.55 + own * ((p.flood >> s) & 255) as f32 / 255.0 * 0.45).min(255.0) as u32
                    };
                    let flooded = 0xFF00_0000 | lit(16) << 16 | lit(8) << 8 | lit(0);
                    (mix(grade(c), mix(flooded, p.flood, 0.18), t.glow), 0.6)
                }
                _ => (grade(c), 1.0),
            };
            t.land[i] = mix(g, haze_to, (land.haze[i] as f32 / 255.0 * haze_k * hz).min(0.94));
        }
        // the riders' colours, their windows and lamps lit after dark
        let skin = self.skin;
        t.rides = Ride::ALL
            .iter()
            .flat_map(|ride| [false, true].map(|wet| (*ride, wet)))
            .map(|(ride, wet)| {
                ride.sprite(wet)
                    .keys
                    .iter()
                    .map(|&(ch, day, night)| {
                        let by_day = grade(skin.colour(day));
                        (ch, if night.is_empty() { by_day } else { mix(by_day, skin.colour(night), t.glow) })
                    })
                    .collect()
            })
            .collect();
        t.rim = mix(grade(p.steel), p.eye, t.glow);
        t.capsule = mix(grade(p.steel), p.lamp, t.glow);
        t.hands = grade(p.hands);
        t.flags = p.flags.map(grade);
        t.gull = grade(p.flags[1]);
        t.falls = p.falls.map(grade);
        t.mica = p.mica;

        // the clouds: white by day, warm at dusk, navy by night; greyer under
        // a grey sky, darker in a storm
        let day = mix5(mix5(p.cloud_day, p.cloud_grey, m.gloom), p.cloud_storm, m.dark * 1.6);
        let dusky = mix5(mix5(p.cloud_dusk, p.cloud_grey, m.gloom * 0.7), p.cloud_storm, m.dark * 1.4);
        let night = mix5(p.cloud_night, p.cloud_storm, m.dark * 0.4);
        t.ramp = core::array::from_fn(|i| by_day(night[i], dusky[i], day[i]));
        t.far_ramp = t.ramp.map(|c| mix(c, low, 0.38));
        let ramp = t.ramp;
        t.soft_ramp = ramp.map(|c| mix(c, ramp[2], 0.32));
        t.far_soft = t.soft_ramp.map(|c| mix(c, low, 0.38));
        let s = p.cloud_storm;
        let tower_day = [s[0], s[2], mix(p.cloud_grey[2], p.cloud_day[2], 0.5), p.cloud_day[3], p.cloud_day[4]];
        let tower_dusk = [s[0], p.cloud_dusk[0], p.cloud_dusk[2], p.cloud_dusk[3], p.cloud_dusk[4]];
        let tower_night = mix5(p.cloud_night, s, 0.3);
        t.tower_ramp = core::array::from_fn(|i| by_day(tower_night[i], tower_dusk[i], tower_day[i]));

        let dim_night = 0.4 * (1.0 - lit);
        t.rain = mix(p.rain, top, dim_night);
        t.glint = mix(p.glint, top, dim_night * 0.6);
        let flake = mix(p.flake, top, dim_night);
        t.flake = [mix(flake, low, 0.5), mix(flake, low, 0.25), flake];
        t.ice = mix(p.ice, top, dim_night);
        t.ice_shade = mix(p.ice_shade, top, dim_night);
        t.stars = ((-d - 0.12) / 0.35).clamp(0.0, 1.0) * (1.0 - 2.2 * m.gloom).clamp(0.0, 1.0);
        t.star = p.star;
        let low_sun = (1.0 - d / 0.35).clamp(0.0, 1.0);
        t.sun = [mix(p.sun[0], p.sun[1], 0.5 * low_sun), mix(p.sun[1], p.sun[3], low_sun), mix(p.sun[2], p.sun[3], low_sun)];
        t.moon = p.moon;
        t.bird = mix(p.bird, top, 0.2);
        t.smoke = grade(p.smoke);
        t.lamp = p.lamp;
        t.firefly = p.firefly;
        t.foam = grade(p.foam);
        t.sail = grade(p.sail);
        t.leaves = p.leaves.map(grade);
        t.flash = p.flash;
        t.bolt = p.bolt;
        // what is behind the readings, at the top left: the place there, or
        // the sky -- or the layer of cloud that covers it
        let (mut sum, mut n) = (0.0, 0);
        for y in (2..70.min(h)).step_by(3) {
            for x in (6..96.min(w)).step_by(3) {
                let i = (y * w + x) as usize;
                let c = if t.land[i] >> 24 != 0 { t.land[i] } else if m.cover == Cover::Layer { t.soft_ramp[3] } else { t.backdrop[i] };
                sum += luma(if m.fog { mix(c, t.fog, 0.35) } else { c });
                n += 1;
            }
        }
        t.bright = sum / n.max(1) as f32 > 0.56;
        self.shade_clouds();
    }

    /// Where the sun -- or the moon -- is, `t` of the way across: on an arc
    /// over the right of the sky, clear of the readings, rising and setting
    /// at the horizon.
    fn arc(&self, t: f32) -> (f32, f32) {
        arc(self.w, self.horizon(), t)
    }

    /// Each cloud's form shaded in the light from where the sun or the moon
    /// stands now.
    fn shade_clouds(&mut self) {
        let l = self.light;
        let (lx, up) = if l.day > -0.05 {
            (self.arc(l.sun).0, l.day.max(0.0))
        } else {
            (self.arc(l.night).0, 0.3 + 0.5 * (PI * l.night).sin())
        };
        let w = self.w as f32;
        for c in &mut self.clouds {
            let light = if c.flat {
                clouds::light_from(0.0, 1.0)
            } else {
                clouds::light_from((lx - c.x - c.form.w as f32 / 2.0) / w * 1.6, up)
            };
            c.px = clouds::shade(&c.form, light, 0.06);
        }
    }

    // ---- the scene, from the start -------------------------------------------

    fn slant(&self) -> f32 {
        (self.wind / 45.0).clamp(-0.6, 0.6)
    }

    /// The wind the snow, the clouds and the smoke feel: a blizzard's is
    /// never less than a gale's, and gusts.
    fn gust(&self) -> f32 {
        let wind = if self.wind.abs() < 1.0 { -6.0 } else { self.wind };
        if self.mood.blizzard {
            let w = wind.signum() * wind.abs().max(35.0);
            w * (1.0 + 0.35 * (self.time * 0.9).sin() + 0.2 * (self.time * 2.3 + 1.0).sin())
        } else {
            wind
        }
    }

    /// Everything the scene has, made afresh and already on its way.
    fn fill(&mut self) {
        let m = self.mood;
        let horizon = self.horizon();
        self.time = 0.0;
        self.stars = (0..60)
            .map(|_| {
                let y = (self.rng.unit().powf(1.3) * (horizon - 16).max(8) as f32) as i32;
                Star {
                    x: self.rng.range(0.0, self.w as f32) as i32,
                    y,
                    base: self.rng.range(0.35, 1.0),
                    speed: self.rng.range(0.6, 2.4),
                    phase: self.rng.range(0.0, TAU),
                    big: self.rng.unit() < 0.08,
                }
            })
            .collect();
        self.fill_clouds();

        self.drops.clear();
        self.splashes.clear();
        if let Some(rain) = m.rain {
            let far = (rain.drops as f32 * LAYERS[FAR].share + 0.5) as usize;
            let mid = (rain.drops as f32 * LAYERS[MID].share + 0.5) as usize;
            for (layer, count) in [(FAR, far), (MID, mid), (NEAR, rain.drops - far - mid)] {
                for _ in 0..count {
                    let drop = self.drop(layer, true);
                    self.drops.push(drop);
                }
            }
        }
        self.flakes.clear();
        let far = (m.snow as f32 * 0.45) as usize;
        let mid = (m.snow as f32 * 0.33) as usize;
        for (layer, count) in [(FAR, far), (MID, mid), (NEAR, m.snow.saturating_sub(far + mid))] {
            for _ in 0..count {
                let flake = self.flake(layer, true);
                self.flakes.push(flake);
            }
        }
        self.stones = (0..m.ice).map(|_| self.stone(true)).collect();
        self.banks.clear();
        if m.fog {
            for (i, &y) in self.land.fog.iter().enumerate() {
                let speed = self.rng.range(0.15, 0.35) * if i % 2 == 0 { 1.0 } else { -1.0 };
                self.banks.push(Bank {
                    y,
                    half: self.rng.range(7.0, 12.0),
                    k: TAU / self.rng.range(40.0, 80.0),
                    phase: self.rng.range(0.0, TAU),
                    speed,
                    strength: self.rng.range(0.5, 0.8),
                });
            }
        }
        self.birds.clear();
        self.next_birds = self.rng.range(2.0, 10.0);
        self.meteor = None;
        self.next_meteor = self.rng.range(6.0, 20.0);
        self.strike = None;
        self.next_strike = m.lightning.map_or(0.0, |(lo, hi)| self.rng.range(lo * 0.3, hi * 0.5));
        self.bolt_next = false;
        // smoke already rising, leaves already falling, flies already out
        self.smoke.clear();
        self.next_smoke = 0.0;
        for _ in 0..(SMOKE_LIFE / SMOKE_EVERY) as usize {
            self.puff_smoke(SMOKE_EVERY);
        }
        self.leaves = match self.land.canopy {
            Some([x, y, w, _]) => {
                let ground = self.land.ground.1 as f32;
                (0..18)
                    .map(|_| Leaf {
                        x: self.rng.range(x as f32, (x + w) as f32),
                        y: self.rng.range(y as f32, ground),
                        speed: self.rng.range(7.0, 14.0),
                        phase: self.rng.range(0.0, TAU),
                        colour: (self.rng.unit() * 4.0) as usize % 4,
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        self.flies = match self.land.meadow {
            Some([x, y, w, h]) => (0..14)
                .map(|_| Fly {
                    x: self.rng.range(x as f32, (x + w) as f32),
                    y: self.rng.range(y as f32, (y + h) as f32),
                    phase: self.rng.range(0.0, TAU),
                    speed: self.rng.range(0.6, 1.6),
                })
                .collect(),
            None => Vec::new(),
        };
        // whoever goes along the place's lanes, spread out along them
        self.riders.clear();
        for (k, lane) in self.land.lanes.iter().enumerate() {
            for i in 0..lane.count {
                let span = lane.to - lane.from;
                let x = lane.from + span * (i as f32 + self.rng.range(0.15, 0.85)) / lane.count as f32;
                let dir = if lane.both { if self.rng.unit() < 0.5 { -1.0 } else { 1.0 } } else { lane.speed.signum() };
                self.riders.push(Rider { lane: k, x, dir, phase: self.rng.range(0.0, 8.0) });
            }
        }
    }

    fn fill_clouds(&mut self) {
        self.clouds.clear();
        self.streaks.clear();
        let w = self.w as f32;
        let horizon = self.horizon() as f32;
        let m = self.mood;
        match m.cover {
            Cover::None => {}
            Cover::Fair => {
                for i in 0..3 {
                    let (cw, ch) = (self.rng.range(22.0, 38.0) as i32, self.rng.range(9.0, 14.0) as i32);
                    let x = (i as f32 + self.rng.range(0.0, 0.7)) * w / 3.0;
                    self.cumulus(x, (horizon * 0.5, horizon - 22.0), cw, ch, 0.5, true);
                }
                self.add_streaks(5, (10.0, horizon * 0.55));
            }
            Cover::Scattered => {
                for i in 0..2 {
                    let (cw, ch) = (self.rng.range(74.0, 100.0) as i32, self.rng.range(38.0, 50.0) as i32);
                    let x = i as f32 * w * 0.55 + self.rng.range(-20.0, 20.0);
                    self.cumulus(x, (2.0, horizon * 0.32), cw, ch, 1.0, false);
                }
                for i in 0..2 {
                    let (cw, ch) = (self.rng.range(40.0, 58.0) as i32, self.rng.range(20.0, 28.0) as i32);
                    let x = (i as f32 + 0.3) * w * 0.5 + self.rng.range(-20.0, 20.0);
                    self.cumulus(x, (horizon * 0.3, horizon * 0.55), cw, ch, 0.8, false);
                }
                for i in 0..3 {
                    let (cw, ch) = (self.rng.range(18.0, 30.0) as i32, self.rng.range(8.0, 12.0) as i32);
                    let x = (i as f32 + self.rng.range(0.0, 0.6)) * w / 3.0;
                    self.cumulus(x, (horizon * 0.6, horizon - 16.0), cw, ch, 0.45, true);
                }
                self.add_streaks(3, (horizon * 0.4, horizon * 0.75));
            }
            Cover::Layer => {
                // rows of cloud from overhead to the horizon, each lower row
                // smaller and flatter, as distance makes them, overlapping
                // so the sky shows only here and there between them
                let rows = [(-0.12, 0.34), (0.1, 0.26), (0.3, 0.19), (0.46, 0.14), (0.58, 0.1), (0.68, 0.07)];
                for (k, &(at, tall)) in rows.iter().enumerate() {
                    let ch = (horizon * tall).max(4.0) as i32;
                    let y = (horizon * at) as i32;
                    let wide = 4.0;
                    let spacing = ch as f32 * wide * 0.7;
                    let n = ((w + ch as f32 * wide * 1.3 + 10.0) / spacing).ceil() as usize;
                    let period = n as f32 * spacing;
                    for i in 0..n {
                        let cw = (ch as f32 * wide * self.rng.range(0.8, 1.3)) as i32;
                        let ch = (ch as f32 * self.rng.range(0.8, 1.2)) as i32;
                        let x = -(ch as f32 * wide) + i as f32 * spacing + self.rng.range(-6.0, 6.0);
                        let form = Form::layer(&mut self.rng, cw, ch);
                        let cy = y + self.rng.range(-4.0, 4.0) as i32;
                        self.clouds.push(Cloud {
                            x,
                            y: cy,
                            form,
                            px: Vec::new(),
                            pace: 1.0 - k as f32 * 0.12,
                            period: Some(period),
                            rows: (cy as f32, cy as f32 + 1.0),
                            far: k >= 3,
                            flat: true,
                            tall: false,
                        });
                    }
                }
            }
            Cover::Tower => {
                // the thunderhead, standing on the horizon on the right, its
                // anvil spread against the sky
                let (cw, ch) = (150, (horizon - 2.0) as i32);
                let form = Form::tower(&mut self.rng, cw, ch);
                self.clouds.push(Cloud {
                    x: w - cw as f32 + 6.0,
                    y: horizon as i32 + 4 - ch,
                    form,
                    px: Vec::new(),
                    pace: 0.0,
                    period: None,
                    rows: (0.0, 1.0),
                    far: false,
                    flat: false,
                    tall: true,
                });
                // a towering cumulus far off to the left, building, and rags
                // of dark scud going by in front of them both
                let form = Form::cumulus(&mut self.rng, 84, (horizon * 0.36) as i32);
                self.clouds.push(Cloud {
                    x: 4.0,
                    y: (horizon * 0.66) as i32,
                    form,
                    px: Vec::new(),
                    pace: 0.0,
                    period: None,
                    rows: (0.0, 1.0),
                    far: true,
                    flat: false,
                    tall: false,
                });
                for i in 0..4 {
                    let (cw, ch) = (self.rng.range(22.0, 40.0) as i32, self.rng.range(8.0, 13.0) as i32);
                    let x = (i as f32 + self.rng.range(0.0, 0.7)) * w / 4.0;
                    self.cumulus(x, (horizon * 0.2, horizon * 0.72), cw, ch, 1.6, false);
                }
            }
        }
        // far ones drawn first, and the thunderhead behind what goes by
        self.clouds.sort_by_key(|c| (!c.far, c.pace != 0.0));
        self.shade_clouds();
    }

    /// A cumulus on its own, somewhere between the `rows` given, that goes
    /// off one side and comes back from the other, at `pace` of the wind.
    fn cumulus(&mut self, x: f32, rows: (f32, f32), w: i32, h: i32, pace: f32, far: bool) {
        let form = Form::cumulus(&mut self.rng, w, h);
        let y = self.rng.range(rows.0, rows.1.max(rows.0 + 1.0)) as i32;
        self.clouds.push(Cloud { x, y, form, px: Vec::new(), pace, period: None, rows, far, flat: false, tall: false });
    }

    fn add_streaks(&mut self, n: usize, rows: (f32, f32)) {
        for _ in 0..n {
            let len = self.rng.range(14.0, 46.0) as i32;
            self.streaks.push(Streak { x: self.rng.range(-20.0, self.w as f32), y: self.rng.range(rows.0, rows.1) as i32, len });
        }
    }

    /// Where between the far and the near edge of the ground something of
    /// this depth lands.
    fn landing(&mut self, layer: usize) -> f32 {
        let (g0, g1) = (self.land.ground.0 as f32, self.land.ground.1 as f32);
        let (a, b) = LAYERS[layer].depth;
        (g0 + (g1 - g0) * self.rng.range(a, b)).min(self.h as f32 - 1.0)
    }

    /// A drop of `layer`: somewhere on its way down already, or about to
    /// come in at the top.
    fn drop(&mut self, layer: usize, falling: bool) -> Drop {
        let land = self.landing(layer);
        // The wind takes a drop `slant * land` aside by the time it lands, so
        // some have to start that far off the other side for the rain to
        // reach every corner of the pane.
        let reach = -self.slant() * land;
        let x = self.rng.range(reach.min(0.0), self.w as f32 + reach.max(0.0));
        let y = if falling { self.rng.range(0.0, land) } else { -self.rng.range(0.0, 30.0) };
        Drop { layer, x, y, land }
    }

    fn flake(&mut self, layer: usize, falling: bool) -> Flake {
        let land = self.landing(layer);
        Flake {
            layer,
            x: self.rng.range(-8.0, self.w as f32 + 8.0),
            y: if falling { self.rng.range(0.0, land) } else { -self.rng.range(0.0, 14.0) },
            land,
            phase: self.rng.range(0.0, TAU),
        }
    }

    fn stone(&mut self, falling: bool) -> Stone {
        let hail = self.mood.hail;
        let depth = if self.rng.unit() < 0.5 { MID } else { NEAR };
        let land = self.landing(depth);
        let reach = -self.slant() * land;
        Stone {
            x: self.rng.range(reach.min(0.0), self.w as f32 + reach.max(0.0)),
            y: if falling { self.rng.range(0.0, land) } else { -self.rng.range(0.0, 40.0) },
            vy: if hail { self.rng.range(230.0, 290.0) } else { self.rng.range(160.0, 200.0) },
            land,
            size: if !hail { 1 } else if self.rng.unit() < 0.65 { 2 } else { 3 },
            bounces: 0,
            rest: 0.0,
        }
    }

    /// A bolt from the foot of the cloud to the ground, forking on the way --
    /// or, now and then, sheet lightning: the flash with no bolt.
    fn new_strike(&mut self, sheet: bool) -> Strike {
        let mut path = Vec::new();
        if !sheet {
            let horizon = self.horizon();
            let top = horizon / 4;
            let bottom = horizon + self.rng.range(1.0, 5.0) as i32;
            let x = self.rng.range(self.w as f32 * 0.45, self.w as f32 * 0.9) as i32;
            // the channel: two pixels wide for its first half, one after
            let trunk = self.bolt_walk(x, top, bottom, 0);
            let half = trunk.len() / 2;
            for (i, &(x, y)) in trunk.iter().enumerate() {
                path.push((x, y));
                if i < half {
                    path.push((x + 1, y));
                }
            }
            // and a fork or two off its upper part
            for _ in 0..1 + (self.rng.unit() < 0.5) as usize {
                let from = (self.rng.range(0.1, 0.55) * trunk.len() as f32) as usize;
                let Some(&(bx, by)) = trunk.get(from) else { continue };
                let lean = if self.rng.unit() < 0.5 { -1 } else { 1 };
                let end = (by + self.rng.range(12.0, 30.0) as i32).min(bottom - 4);
                path.extend(self.bolt_walk(bx, by, end, lean));
            }
        }
        Strike { age: 0.0, path }
    }

    /// A bolt's way down from (x, top) to `bottom`, a pixel a row: a run
    /// straight down, then a step or a jog aside -- leaning `lean` if it is a
    /// fork -- with the rows of a jog joined so it never breaks.
    fn bolt_walk(&mut self, mut x: i32, top: i32, bottom: i32, lean: i32) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        let mut run = 0;
        for y in top..bottom {
            out.push((x, y));
            run -= 1;
            if run > 0 {
                continue;
            }
            run = self.rng.range(2.0, 6.0) as i32;
            let side = if lean != 0 && self.rng.unit() < 0.7 { lean } else if self.rng.unit() < 0.5 { -1 } else { 1 };
            let step = if self.rng.unit() < 0.25 { self.rng.range(2.0, 4.0) as i32 } else { 1 };
            for k in 1..=step {
                out.push((x + side * k, y));
            }
            x += side * step;
        }
        out
    }

    /// `dt` seconds of smoke: what has risen rises on, and each chimney puts
    /// out another puff when it is time.
    fn puff_smoke(&mut self, dt: f32) {
        let drift = self.gust() * 0.22;
        for s in &mut self.smoke {
            s.age += dt;
            s.y -= 5.5 * dt;
            s.x += drift * dt * (0.4 + s.age * 0.4);
        }
        self.smoke.retain(|s| s.age < SMOKE_LIFE);
        self.next_smoke -= dt;
        if self.next_smoke <= 0.0 {
            self.next_smoke += SMOKE_EVERY;
            for &(x, y) in &self.land.chimneys {
                self.smoke.push(Smoke { x: x as f32 + 0.5, y: y as f32, age: 0.0 });
            }
        }
    }

    // ---- time passing --------------------------------------------------------

    /// `dt` seconds of weather.
    pub fn step(&mut self, dt: f32) {
        self.time = (self.time + dt) % TIME_WRAP;
        let (w, h) = (self.w as f32, self.h as f32);
        let slant = self.slant();

        // the clouds drift with the wind; one on its own that has gone comes
        // back from the other side, somewhere else
        let gust = self.gust();
        let speed = 2.6 * (0.5 + gust.abs() / 18.0).min(3.5) * gust.signum();
        for c in &mut self.clouds {
            c.x += speed * c.pace * dt;
            let cw = c.form.w as f32;
            match c.period {
                Some(period) => c.x = (c.x - (w - period)).rem_euclid(period) + (w - period),
                None if c.pace == 0.0 => {}
                None => {
                    if speed < 0.0 && c.x + cw < -2.0 {
                        c.x = w + self.rng.range(0.0, 30.0);
                    } else if speed > 0.0 && c.x > w + 2.0 {
                        c.x = -cw - self.rng.range(0.0, 30.0);
                    } else {
                        continue;
                    }
                    c.y = self.rng.range(c.rows.0, c.rows.1.max(c.rows.0 + 1.0)) as i32;
                }
            }
        }
        for s in &mut self.streaks {
            s.x += speed * 0.3 * dt;
            if s.x > w + 4.0 {
                s.x = -(s.len as f32) - 4.0;
            } else if s.x + (s.len as f32) < -4.0 {
                s.x = w + 4.0;
            }
        }

        // the rain, as the Rain Window's: a drop that lands splashes -- or
        // rings the water -- and comes round again from the top
        if let Some(rain) = self.mood.rain {
            for i in 0..self.drops.len() {
                let d = &mut self.drops[i];
                d.y += LAYERS[d.layer].speed * rain.speed * dt;
                if d.y < d.land {
                    continue;
                }
                let (layer, x, y) = (d.layer, (d.x + slant * d.land).floor() as i32, d.land as i32);
                if (0..self.w).contains(&x) {
                    let ring = self.land.kind_at(x, y) == WATER;
                    self.splashes.push(Splash { layer, x, y, age: 0.0, ring });
                }
                let again = self.drop(layer, false);
                self.drops[i] = again;
            }
        }
        for s in &mut self.splashes {
            s.age += dt;
        }
        self.splashes.retain(|s| s.age < if s.layer == FAR && !s.ring { SPLASH_LIFE / 3.0 } else { SPLASH_LIFE });

        // snow: down at its own pace, aside with the wind, round again
        let drift = gust * if self.mood.blizzard { 0.9 } else { 0.45 };
        let fall = if self.mood.blizzard { 1.9 } else { 1.0 };
        for i in 0..self.flakes.len() {
            let f = &mut self.flakes[i];
            let layer = f.layer;
            f.y += FLAKE_SPEED[layer] * fall * dt;
            f.x += drift * [0.6, 0.8, 1.0][layer] * dt;
            f.x = (f.x + 8.0).rem_euclid(w + 16.0) - 8.0;
            if f.y >= f.land {
                let again = self.flake(layer, false);
                self.flakes[i] = again;
            }
        }

        // ice: down, a bounce or two where it lands, and -- hail -- a moment
        // lying there before it comes round again
        let gravity = 900.0;
        for i in 0..self.stones.len() {
            let hail = self.mood.hail;
            let s = &mut self.stones[i];
            if s.rest > 0.0 {
                s.rest -= dt;
                if s.rest <= 0.0 {
                    let again = self.stone(false);
                    self.stones[i] = again;
                }
                continue;
            }
            if s.bounces > 0 {
                s.vy += gravity * dt;
            }
            s.y += s.vy * dt;
            s.x += slant * s.vy.abs() * dt * if s.bounces > 0 { 0.4 } else { 1.0 };
            if s.y < s.land || s.vy < 0.0 {
                continue;
            }
            s.y = s.land;
            let most = if hail { 2 } else { 1 };
            if s.bounces < most {
                s.bounces += 1;
                s.vy = -s.vy * if s.bounces == 1 { 0.3 } else { 0.25 };
            } else if hail {
                s.rest = 0.5 + 0.9 * ((s.x * 7.3).sin() * 0.5 + 0.5);
                s.vy = 0.0;
            } else {
                let again = self.stone(false);
                self.stones[i] = again;
            }
        }

        // birds across a fine day, a few at a time
        // songbirds over a fine day; gulls over a city by the sea whatever
        // the weather, while it is light
        let fine = (self.mood.open || self.land.gulls) && self.light.day > 0.15;
        for b in &mut self.birds {
            b.x += b.vx * dt;
        }
        self.birds.retain(|b| b.x > -12.0 && b.x < w + 12.0);
        self.next_birds -= dt;
        if self.next_birds <= 0.0 {
            self.next_birds = self.rng.range(9.0, 24.0);
            if fine && self.birds.is_empty() {
                let right = self.rng.unit() < 0.5;
                let vx = self.rng.range(14.0, 22.0) * if right { 1.0 } else { -1.0 };
                let y = self.rng.range(30.0, (self.horizon() - 30).max(31) as f32);
                let x = if right { -6.0 } else { w + 6.0 };
                for k in 0..1 + (self.rng.unit() * 3.0) as usize {
                    let (dx, dy) = (self.rng.range(4.0, 9.0) * k as f32, self.rng.range(-5.0, 5.0));
                    self.birds.push(Bird { x: x - dx * vx.signum(), y: y + dy, vx, phase: self.rng.range(0.0, TAU) });
                }
            }
        }

        // a falling star, now and then, on a clear night
        if let Some(m) = &mut self.meteor {
            m.age += dt;
            m.x += m.vx * dt;
            m.y += m.vy * dt;
            if m.age >= m.life {
                self.meteor = None;
            }
        }
        self.next_meteor -= dt;
        if self.next_meteor <= 0.0 {
            self.next_meteor = self.rng.range(14.0, 40.0);
            if self.scene == Scene::Clear && self.tones.stars > 0.5 {
                self.meteor = Some(Meteor {
                    x: self.rng.range(w * 0.35, w),
                    y: self.rng.range(4.0, h * 0.25),
                    vx: -self.rng.range(90.0, 140.0),
                    vy: self.rng.range(40.0, 70.0),
                    age: 0.0,
                    life: self.rng.range(0.45, 0.8),
                });
            }
        }

        // lightning
        if let Some((lo, hi)) = self.mood.lightning {
            if let Some(s) = &mut self.strike {
                s.age += dt;
                if s.done() {
                    self.strike = None;
                }
            }
            self.next_strike -= dt;
            if self.next_strike <= 0.0 && self.strike.is_none() {
                self.next_strike = self.rng.range(lo, hi);
                let sheet = self.rng.unit() < 0.35 && !core::mem::take(&mut self.bolt_next);
                self.strike = Some(self.new_strike(sheet));
            }
        }

        // who goes by: each at its lane's pace, tumbleweed with the wind,
        // off one end and back on at the other
        for r in &mut self.riders {
            let lane = &self.land.lanes[r.lane];
            let pace = if lane.ride == Ride::Weed {
                r.dir = if gust < 0.0 { -1.0 } else { 1.0 };
                (gust.abs() / 18.0).clamp(0.4, 2.5)
            } else {
                1.0
            };
            r.x += r.dir * lane.speed.abs() * pace * dt;
            if r.x > lane.to {
                r.x = lane.from;
            } else if r.x < lane.from {
                r.x = lane.to;
            }
        }

        // the place's own: smoke, leaves, fireflies
        self.puff_smoke(dt);
        if let Some([x0, y0, cw, _]) = self.land.canopy {
            let ground = self.land.ground.1 as f32;
            for i in 0..self.leaves.len() {
                let l = &mut self.leaves[i];
                l.y += l.speed * dt;
                l.x += (gust * 0.25 + 6.0 * (self.time * 1.7 + l.phase).cos()) * dt;
                if l.y > ground || l.x < -4.0 || l.x > w + 4.0 {
                    l.y = y0 as f32 + self.rng.range(0.0, 20.0);
                    l.x = x0 as f32 + self.rng.range(0.0, cw as f32);
                }
            }
        }
        let t = self.time;
        for f in &mut self.flies {
            f.x += (t * 0.7 * f.speed + f.phase).cos() * 4.0 * dt;
            f.y += (t * 0.9 * f.speed + f.phase * 1.3).sin() * 2.5 * dt;
        }
    }

    // ---- the picture ---------------------------------------------------------

    /// Draw the scene as it stands, back to front.
    pub fn render(&mut self) {
        let (w, time) = (self.w, self.time);
        let horizon = self.horizon();
        let m = self.mood;
        let l = self.light;
        let gust = self.gust();
        let t = &self.tones;
        let pic = &mut self.pic;
        if t.backdrop.len() == pic.px.len() {
            pic.px.copy_from_slice(&t.backdrop);
        }

        if t.stars > 0.0 {
            for s in &self.stars {
                let twinkle = s.base * (0.7 + 0.3 * (time * s.speed + s.phase).sin()) * t.stars;
                pic.blend(s.x, s.y, t.star, twinkle);
                if s.big {
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        pic.blend(s.x + dx, s.y + dy, t.star, 0.35 * twinkle);
                    }
                }
            }
        }
        if let Some(me) = &self.meteor {
            let fade = (1.0 - me.age / me.life).max(0.0).sqrt() * t.stars;
            let len = (me.vx * me.vx + me.vy * me.vy).sqrt();
            let (ux, uy) = (me.vx / len, me.vy / len);
            for i in 0..8 {
                let (x, y) = (me.x - ux * i as f32, me.y - uy * i as f32);
                pic.blend(x as i32, y as i32, t.star, fade * (1.0 - i as f32 / 8.0));
            }
        }

        // the sun by day, the moon by night -- and behind a thunderhead, a
        // sun going down
        if m.open || m.cover == Cover::Tower {
            if l.day > -0.05 {
                let (x, y) = arc(w, horizon, l.sun);
                draw_sun(pic, t, x, y, time, l.day > 0.12 && m.open);
            } else if m.open {
                let (x, y) = arc(w, horizon, l.night);
                draw_moon(pic, t, x, y + 6.0, l.moon);
            }
        }

        // streaks of high cloud, then the clouds, far to near
        for s in &self.streaks {
            let x0 = s.x as i32;
            for k in 0..s.len {
                let edge = k.min(s.len - 1 - k);
                let a = if edge < 3 { 0.15 * edge as f32 } else { 0.45 };
                pic.blend(x0 + k, s.y, t.ramp[4], a);
                if k > s.len / 4 && k < s.len * 3 / 4 {
                    pic.blend(x0 + k + 3, s.y + 1, t.ramp[3], a * 0.6);
                }
            }
        }
        for c in &self.clouds {
            let ramp = match (c.tall, c.flat, c.far) {
                (true, _, _) => &t.tower_ramp,
                (_, true, false) => &t.soft_ramp,
                (_, true, true) => &t.far_soft,
                (_, false, true) => &t.far_ramp,
                _ => &t.ramp,
            };
            draw_cloud(pic, c, ramp);
        }

        for b in &self.birds {
            let up = ((time * 6.0 + b.phase) as i32) % 2 == 0;
            let bob = (time * 2.0 + b.phase).sin().round() as i32;
            let shape: [(i32, i32); 5] = if up {
                [(-2, -1), (2, -1), (-1, 0), (1, 0), (0, 1)]
            } else {
                [(-2, 1), (-1, 0), (0, 0), (1, 0), (2, 1)]
            };
            for (dx, dy) in shape {
                pic.blend(b.x as i32 + dx, b.y as i32 + dy + bob, if self.land.gulls { t.gull } else { t.bird }, 0.85);
            }
        }

        // the place, in this light, its water left for after
        let land = &self.land;
        for (i, p) in pic.px.iter_mut().enumerate() {
            let c = t.land[i];
            if c >> 24 != 0 && land.kind[i] != WATER {
                *p = c;
            }
        }
        // its water, showing what stands over it, rippling -- more in rain --
        // and under a clear sky a path of glitter towards the sun, or the moon
        let ripple = if m.rain.is_some() || m.blizzard { 1.6 } else { 0.7 };
        let glitter = match (m.open, l.day > 0.05, l.day < -0.15) {
            (true, true, _) => Some((arc(w, horizon, l.sun).0, t.sun[0])),
            (true, _, true) => Some((arc(w, horizon, l.night).0, t.moon[0])),
            _ => None,
        };
        let tick = (time * 4.0) as i32;
        // how far each row of what is mirrored sways, and each row of water
        let (drift, sway) = self.ripples.split_at_mut(self.h as usize);
        for (row, (d, s)) in drift.iter_mut().zip(sway.iter_mut()).enumerate() {
            *d = (row as f32 * 0.9 + time * 2.4).sin() * ripple;
            *s = (row as f32 * 0.7 + time * 1.3).sin() * 0.4;
        }
        for &(i, up) in &self.water {
            let i = i as usize;
            let (x, y) = (i as i32 % w, i as i32 / w);
            let wob = (drift[up as usize] + sway[y as usize]).round() as i32;
            let src = up as usize * w as usize + (x + wob).clamp(0, w - 1) as usize;
            let seen = if land.kind[src] == WATER { t.land[i] } else { pic.px[src] };
            let below = (y - up as i32) as f32 / 2.0;
            let mut c = blend256(seen, t.land[i], (77.0 + 102.0 * below / (below + 18.0)) as u32);
            if let Some((gx, light)) = glitter {
                let reach = 1.0 + below * 0.3;
                let off = (x as f32 - gx).abs();
                if off < reach {
                    // a sparkle here and there, thickest down the middle,
                    // each lasting a moment
                    let hash = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ (tick as u32).wrapping_mul(0xC2B2_AE3D);
                    let odds = if off < reach * 0.4 { 4 } else { 9 };
                    if (hash >> 16) % odds == 0 {
                        c = blend256(c, light, 200);
                    }
                }
            }
            pic.px[i] = c;
        }
        // the sea's waves: crests catching the light, broader nearer
        if let Some((top, bottom)) = land.sea {
            let rough = if m.rain.is_some() { 1.0 } else { 0.6 };
            let sine = &self.sine;
            let at = |v: f32| sine[(v * (SINE as f32 / TAU)) as i32 as usize & (SINE - 1)];
            for y in top + 1..bottom {
                let d = (y - top) as f32;
                let k = 0.5 / (1.0 + d * 0.06);
                let a = ((0.25 + 0.5 * d / (bottom - top) as f32) * rough * 256.0) as u32;
                let (p1, p2) = (time * 1.6 + y as f32 * 1.9 + 100.0, -time * 2.1 + y as f32 + 400.0);
                let row = (y * w) as usize;
                for x in 0..w as usize {
                    if land.kind[row + x] == WATER && at(x as f32 * k + p1) + 0.5 * at(x as f32 * k * 2.3 + p2) > 1.05 {
                        pic.px[row + x] = blend256(pic.px[row + x], t.foam, a);
                    }
                }
            }
        }

        // the place's own lights and what moves in it
        for s in &self.smoke {
            let f = s.age / SMOKE_LIFE;
            let r = 0.8 + s.age * 0.9;
            let a = 0.55 * (1.0 - f);
            let ri = r.ceil() as i32;
            for dy in -ri..=ri {
                for dx in -ri..=ri {
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    if d <= r {
                        pic.blend(s.x as i32 + dx, s.y as i32 + dy, t.smoke, a * (1.0 - 0.5 * d / r));
                    }
                }
            }
        }
        if t.glow > 0.05 {
            for &(x, y) in &land.lamps {
                pic.glow(x, y, 9, t.lamp, 0.45 * t.glow);
            }
        }
        if let Some((bx, by)) = land.beacon {
            if t.glow > 0.15 {
                let turn = (time * 1.1).cos();
                let len = 150;
                let side = if turn > 0.0 { 1 } else { -1 };
                for k in 2..len {
                    let half = 1 + k / 18;
                    let a = 0.4 * turn.abs() * t.glow * (1.0 - k as f32 / len as f32);
                    for dy in -half..=half {
                        pic.blend(bx + side * k, by + dy, t.lamp, a * (1.0 - dy.abs() as f32 / (half + 1) as f32));
                    }
                }
                pic.glow(bx, by, 6, t.lamp, (1.0 - turn.abs()) * 0.8 * t.glow);
            }
        }
        if let Some((mx, my, len)) = land.mill {
            let turn = time * (0.8 + self.wind.abs() / 18.0).min(3.0);
            for s in 0..4 {
                let a = turn + s as f32 * PI / 2.0;
                let (dx, dy) = (a.cos(), a.sin());
                for k in 1..=len {
                    let (x, y) = (mx as f32 + dx * k as f32, my as f32 + dy * k as f32);
                    pic.set(x.round() as i32, y.round() as i32, t.sail);
                    if k > 3 {
                        pic.set((x - dy * 1.5).round() as i32, (y + dx * 1.5).round() as i32, t.sail);
                    }
                }
            }
            pic.set(mx, my, t.bird);
        }
        // the London Eye: its rim, its spokes, and its capsules going round
        if let Some((ex, ey, er)) = land.wheel {
            let (cx, cy, r) = (ex as f32 + 0.5, ey as f32 + 0.5, er as f32);
            let steps = (TAU * r) as i32 * 2;
            for k in 0..steps {
                let a = k as f32 / steps as f32 * TAU;
                pic.set((cx + a.cos() * r).floor() as i32, (cy + a.sin() * r).floor() as i32, t.rim);
            }
            let turn = time * 0.03;
            for k in 0..16 {
                let a = turn + k as f32 / 16.0 * TAU;
                for d in (2..er).step_by(2) {
                    pic.blend((cx + a.cos() * d as f32).floor() as i32, (cy + a.sin() * d as f32).floor() as i32, t.rim, 0.45);
                }
            }
            for k in 0..32 {
                let a = turn + k as f32 / 32.0 * TAU;
                let (x, y) = ((cx + a.cos() * (r + 1.5)).floor() as i32, (cy + a.sin() * (r + 1.5)).floor() as i32);
                pic.set(x, y, t.capsule);
                pic.set(x + 1, y, t.capsule);
            }
            pic.set(ex, ey, t.hands);
        }
        // a clock's hands, at the time where the forecast is for
        if let Some((cx, cy, r)) = land.clock {
            let (hour, minute) = self.clock;
            let centre = (cx as f32 + 0.5, cy as f32 + 0.5);
            for (turn, len) in [(minute as f32 / 60.0, r as f32 - 0.6), (((hour % 12) as f32 + minute as f32 / 60.0) / 12.0, r as f32 - 2.2)] {
                let a = turn * TAU - PI / 2.0;
                for k in 0..=(len * 2.0) as i32 {
                    let d = k as f32 / 2.0;
                    pic.set((centre.0 + a.cos() * d).floor() as i32, (centre.1 + a.sin() * d).floor() as i32, t.hands);
                }
            }
        }
        // a waterfall: water running down over the rock, foam at its foot
        if let Some((fx, top, foot, fw)) = land.falls {
            for y in top..foot {
                for x in fx..fx + fw {
                    let run = ((y as f32 - time * 38.0 + (x - fx) as f32 * 2.0) as i32).rem_euclid(6);
                    pic.set(x, y, if run < 2 { t.falls[1] } else { t.falls[0] });
                }
            }
            for k in 0..8 {
                let a = time * 3.0 + k as f32;
                pic.blend(fx + (a.sin() * 4.0) as i32 + fw / 2, foot - (a * 1.7).cos().abs() as i32 * 2, t.falls[1], 0.6);
            }
        }
        // a traffic light: green, amber, red, round again
        if let Some((sx, sy)) = land.signal {
            let phase = time % 12.0;
            let on = if phase < 6.0 { 2 } else if phase < 8.0 { 1 } else { 0 };
            for k in 0..3 {
                let c = if k == on { self.pal.signals[k] } else { mix(self.pal.signals[k], 0xFF00_0000, 0.75) };
                pic.set(sx, sy + 2 * k as i32, c);
            }
        }
        // mica in granite, glinting while the sun is out
        if m.open || (l.day > 0.2 && m.gloom < 0.5) {
            let tick = (time * 3.0) as u32;
            for &(x, y) in &land.glints {
                let hash = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ tick.wrapping_mul(0xC2B2_AE3D);
                if (hash >> 16) % 6 == 0 {
                    pic.set(x, y, t.mica);
                }
            }
        }
        // the lights that blink: cranes', masts', buoys'
        for (k, &(x, y, c)) in land.blinkers.iter().enumerate() {
            if (time * 0.9 + k as f32 * 0.37) % 1.6 < 0.55 {
                pic.set(x, y, c);
                if t.glow > 0.3 {
                    pic.glow(x, y, 3, c, 0.5 * t.glow);
                }
            }
        }
        // a flag flying downwind of its staff, rippling
        if let Some((fx, fy, which)) = land.flag {
            let side = if gust < 0.0 { -1 } else { 1 };
            let [red, white, blue] = t.flags;
            for u in 0..9 {
                let wave = ((time * 5.0 - u as f32 * 0.8).sin() * u as f32 / 8.0).round() as i32;
                for v in 0..6 {
                    let (fu, fv) = (u as f32 / 8.0, v as f32 / 5.0);
                    let diagonal = (fv - fu).abs() < 0.18 || (fv - (1.0 - fu)).abs() < 0.18;
                    let c = if which == SALTIRE {
                        if diagonal { white } else { blue }
                    } else if (fv - 0.5).abs() < 0.12 || u == 4 {
                        red
                    } else if (fv - 0.5).abs() < 0.25 || (3..=5).contains(&u) || diagonal {
                        white
                    } else {
                        blue
                    };
                    pic.set(fx + side * (u + 1), fy + v + wave, c);
                }
            }
        }
        // who goes by
        let wet = m.rain.is_some();
        for r in &self.riders {
            let lane = &land.lanes[r.lane];
            let sprite = lane.ride.sprite(wet);
            let keys = &t.rides[lane.ride.index() * 2 + wet as usize];
            let steps = lane.ride.steps();
            let frame = if steps > 0.0 { (time * steps + r.phase) as usize % sprite.frames.len() } else { 0 };
            let rows = sprite.frames[frame];
            let (sw, sh) = (rows[0].len() as i32, rows.len() as i32);
            let foot = lane.y + lane.slope * r.x;
            let (x0, y0) = (r.x.round() as i32 - sw / 2, if lane.ride == Ride::Gondola { foot.round() as i32 } else { foot.round() as i32 - sh + 1 });
            for (ry, row) in rows.iter().enumerate() {
                for (rx, ch) in row.bytes().enumerate() {
                    if ch == b'.' {
                        continue;
                    }
                    let col = if r.dir < 0.0 { sw - 1 - rx as i32 } else { rx as i32 };
                    if let Some(&(_, c)) = keys.iter().find(|k| k.0 == ch) {
                        pic.set(x0 + col, y0 + ry as i32, c);
                    }
                }
            }
        }
        for lf in &self.leaves {
            let x = (lf.x + (time * 3.0 + lf.phase).sin() * 1.5).round() as i32;
            pic.set(x, lf.y as i32, t.leaves[lf.colour]);
            if (time * 4.0 + lf.phase).sin() > 0.0 {
                pic.set(x + 1, lf.y as i32, t.leaves[lf.colour.saturating_sub(1)]);
            }
        }
        if t.glow > 0.6 && m.open {
            for f in &self.flies {
                let on = ((time * 1.8 + f.phase).sin().max(0.0)).powi(2) * t.glow;
                if on > 0.05 {
                    pic.blend(f.x as i32, f.y as i32, t.firefly, on);
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        pic.blend(f.x as i32 + dx, f.y as i32 + dy, t.firefly, on * 0.25);
                    }
                }
            }
        }

        if m.fog {
            draw_fog(pic, &self.banks, &mut self.roll, t.fog, horizon, time);
        }

        // what falls, back to front: the splashes under the drops
        if let Some(rain) = m.rain {
            let slant = (self.wind / 45.0).clamp(-0.6, 0.6);
            for s in &self.splashes {
                draw_splash(pic, t, s, rain.splash);
            }
            for d in &self.drops {
                draw_streak(pic, t, d, slant, rain.len);
            }
        }
        for f in &self.flakes {
            let sway = FLAKE_SWAY[f.layer] * (time * 1.4 + f.phase).sin();
            let (x, y) = ((f.x + sway).round() as i32, f.y as i32);
            let c = t.flake[f.layer];
            if f.layer < NEAR {
                pic.set(x, y, c);
            } else if m.blizzard {
                // driven sideways: a streak along its way
                let lean = (self.wind.signum() * if self.wind.abs() < 1.0 { -1.0 } else { 1.0 }) as i32;
                pic.set(x, y, c);
                pic.blend(x - lean, y - 1, c, 0.6);
                pic.blend(x - 2 * lean, y - 1, c, 0.3);
            } else if (f.phase * 3.0) as i32 % 2 == 0 {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    pic.set(x + dx, y + dy, c);
                }
            } else {
                pic.set(x, y, c);
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    pic.blend(x + dx, y + dy, c, 0.55);
                }
            }
        }
        for s in &self.stones {
            let (x, y) = (s.x.round() as i32, s.y.round() as i32 - s.size as i32 + 1);
            match s.size {
                1 => {
                    pic.set(x, y - 1, t.ice);
                    pic.set(x, y, t.ice_shade);
                }
                2 => {
                    pic.set(x, y, t.ice);
                    pic.set(x + 1, y, t.ice);
                    pic.set(x, y + 1, t.ice);
                    pic.set(x + 1, y + 1, t.ice_shade);
                }
                _ => {
                    pic.set(x + 1, y, t.ice);
                    pic.set(x, y + 1, t.ice);
                    pic.set(x + 1, y + 1, t.ice);
                    pic.set(x + 2, y + 1, t.ice_shade);
                    pic.set(x + 1, y + 2, t.ice_shade);
                }
            }
        }

        // lightning: the whole scene lifted towards the flash, the ground
        // less than the sky, and the bolt over it
        if let Some(s) = &self.strike {
            let f = s.flash();
            if f > 0.01 {
                let sky_end = ((horizon * w) as usize).min(pic.px.len());
                let (sky, ground) = pic.px.split_at_mut(sky_end);
                lift(sky, t.flash, f * 0.32);
                lift(ground, t.flash, f * 0.16);
            }
            if s.age < 0.22 {
                for &(x, y) in &s.path {
                    pic.blend(x - 1, y, t.bolt, 0.35);
                    pic.blend(x + 1, y, t.bolt, 0.35);
                }
                for &(x, y) in &s.path {
                    pic.set(x, y, t.bolt);
                }
            }
        }
    }
}

/// Where the sun -- or the moon -- is, `t` of the way across a sky `w` wide
/// down to `horizon`: on an arc over the right of the sky, clear of the
/// readings, rising and setting at the horizon.
fn arc(w: i32, horizon: i32, t: f32) -> (f32, f32) {
    let t = t.clamp(0.0, 1.0);
    let up = (PI * t).sin();
    (w as f32 * (0.48 + 0.44 * t), horizon as f32 - 4.0 - (horizon as f32 - 30.0) * up)
}

/// Every pixel of `px` lifted `a` of the way towards `c`, in whole numbers:
/// a lightning flash.
fn lift(px: &mut [u32], c: u32, a: f32) {
    let k = (a.clamp(0.0, 1.0) * 256.0) as u32;
    let lit = [16, 8, 0].map(|s| ((c >> s) & 255) * k);
    for p in px {
        let ch = |s: u32, i: usize| ((((*p >> s) & 255) * (256 - k) + lit[i] + 128) >> 8).min(255);
        *p = 0xFF00_0000 | ch(16, 0) << 16 | ch(8, 1) << 8 | ch(0, 2);
    }
}

fn draw_cloud(pic: &mut Pic, c: &Cloud, ramp: &[u32; 5]) {
    let (cw, ch) = (c.form.w, c.form.h);
    if c.px.len() != (cw * ch) as usize {
        return;
    }
    let x0 = c.x.floor() as i32;
    // the columns of it that are in the pane
    let (from, to) = ((-x0).max(0), (pic.w - x0).min(cw));
    if from >= to {
        return;
    }
    for yy in 0..ch {
        let y = c.y + yy;
        if !(0..pic.h).contains(&y) {
            continue;
        }
        let shape = &c.px[(yy * cw + from) as usize..(yy * cw + to) as usize];
        let start = (y * pic.w + x0 + from) as usize;
        let row = &mut pic.px[start..start + shape.len()];
        for (p, &tone) in row.iter_mut().zip(shape) {
            if tone != 0 {
                *p = ramp[tone as usize - 1];
            }
        }
    }
}

/// The sun: a disc with a rim, rings of glow, and -- high in a clear sky --
/// eight short rays, the long and the short taking turns to reach out.  Low
/// down it is the colour of a sunset.
fn draw_sun(pic: &mut Pic, t: &Tones, cx: f32, cy: f32, time: f32, rays: bool) {
    let r = 7.5;
    let (icx, icy) = (cx.round() as i32, cy.round() as i32);
    for dy in -18..=18 {
        for dx in -18..=18 {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let (x, y) = (icx + dx, icy + dy);
            if d <= r {
                pic.set(x, y, if d > r - 1.2 { t.sun[1] } else { t.sun[0] });
            } else if d <= r + 3.0 {
                pic.blend(x, y, t.sun[2], 0.35);
            } else if d <= r + 8.0 {
                pic.blend(x, y, t.sun[2], 0.14);
            } else if d <= r + 10.0 {
                pic.blend(x, y, t.sun[2], 0.06);
            }
        }
    }
    if !rays {
        return;
    }
    const WAYS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    for (i, (ux, uy)) in WAYS.into_iter().enumerate() {
        let diagonal = ux != 0 && uy != 0;
        let start = if diagonal { 8 } else { 11 };
        let pulse = (time * 1.8 + i as f32 * PI / 2.0).sin();
        let len = if i % 2 == 0 { 3 + (pulse > 0.0) as i32 } else { 2 + (pulse > 0.4) as i32 };
        let len = if diagonal { len - 1 } else { len };
        for k in 0..len {
            pic.blend(icx + ux * (start + k), icy + uy * (start + k), t.sun[0], 0.85 - 0.15 * k as f32);
        }
    }
}

/// The moon in the phase it is in: lit on the right while it waxes and on
/// the left as it wanes, the rest of it faint, its seas a little darker,
/// and a faint ring of light round it.
fn draw_moon(pic: &mut Pic, t: &Tones, cx: f32, cy: f32, age: f32) {
    let r = 6.5f32;
    let (icx, icy) = (cx.round() as i32, cy.round() as i32);
    let waxing = age < 0.5;
    let k = (TAU * age).cos();
    const SEAS: [(i32, i32); 6] = [(-2, -2), (-1, -2), (-2, -1), (1, 1), (2, 1), (1, 2)];
    for dy in -12..=12 {
        for dx in -12..=12 {
            let (x, y) = (icx + dx, icy + dy);
            let (nx, ny) = (dx as f32 / r, dy as f32 / r);
            let d = (nx * nx + ny * ny).sqrt();
            if d > 1.0 {
                if d <= 1.8 {
                    pic.blend(x, y, t.moon[0], 0.12 * (1.8 - d) / 0.8);
                }
                continue;
            }
            let edge = (1.0 - ny * ny).max(0.0).sqrt() * k;
            let lit = if waxing { nx > edge } else { nx < -edge };
            if !lit {
                pic.blend(x, y, t.moon[0], 0.14);
            } else if SEAS.contains(&(dx, dy)) {
                pic.set(x, y, t.moon[1]);
            } else {
                pic.set(x, y, t.moon[0]);
            }
        }
    }
}

/// Fog: a haze that thickens towards the horizon, and banks across the
/// place, each rolling along its length and drifting at its own pace -- in
/// four levels of the fog's colour, dithered, so that every pixel is one of
/// them.
fn draw_fog(pic: &mut Pic, banks: &[Bank], roll: &mut Vec<f32>, fog: u32, horizon: i32, time: f32) {
    let (w, h) = (pic.w as usize, pic.h);
    roll.clear();
    for b in banks {
        for x in 0..w {
            roll.push(0.55 + 0.45 * (x as f32 * b.k + b.phase + time * b.speed).sin());
        }
    }
    // the fog's colour a channel at a time, a quarter to the whole of it
    let shares: [[u32; 3]; 5] = core::array::from_fn(|q| [16, 8, 0].map(|s| ((fog >> s) & 255) * q as u32));
    for y in 0..h {
        let haze = if y < horizon { 0.14 + 0.32 * (y as f32 / horizon as f32).powi(2) } else { 0.3 };
        // only the banks this row passes through
        let mut near = [(0f32, 0usize); 8];
        let mut n = 0;
        for (i, b) in banks.iter().enumerate().take(8) {
            let off = (y as f32 - b.y).abs() / b.half;
            if off < 1.0 {
                near[n] = ((1.0 - off).powf(1.5) * b.strength, i * w);
                n += 1;
            }
        }
        let bayer = &BAYER[y as usize & 3];
        let row = &mut pic.px[y as usize * w..(y as usize + 1) * w];
        for (x, p) in row.iter_mut().enumerate() {
            let mut density = haze;
            for &(weight, at) in &near[..n] {
                density += weight * roll[at + x];
            }
            // in quarters, the fraction between two of them dithered
            let level = density.clamp(0.0, 0.9) * 4.0;
            let whole = level as u32;
            let q = whole + ((level - whole as f32) * 16.0 > bayer[x & 3] as f32 + 0.5) as u32;
            if q > 0 {
                let (c, f) = (*p, &shares[q as usize]);
                let ch = |s: u32, k: usize| (((c >> s) & 255) * (4 - q) + f[k] + 2) / 4;
                *p = 0xFF00_0000 | ch(16, 0) << 16 | ch(8, 1) << 8 | ch(0, 2);
            }
        }
    }
}

/// One streak of rain, head lowest, leaning the way the wind blows, part
/// way over what is behind it -- the far drops the least -- the nearest
/// with a bright head.
fn draw_streak(pic: &mut Pic, t: &Tones, d: &Drop, slant: f32, len_k: f32) {
    let layer = &LAYERS[d.layer];
    let len = ((layer.len as f32 * len_k).round() as i32).max(2);
    let head = d.y.floor() as i32;
    for i in 0..len {
        let row = head - i;
        if row < 0 {
            break;
        }
        let x = (d.x + slant * row as f32).floor() as i32;
        if d.layer == NEAR && i < 2 && len > 3 {
            pic.blend(x, row, t.glint, 0.95);
        } else {
            let fade = if i * 3 >= len * 2 { 0.6 } else { 1.0 };
            pic.blend(x, row, t.rain, layer.alpha * fade);
        }
    }
}

/// What a drop leaves where it lands, as the Rain Window has it: far off,
/// one brief speck; nearer, a spark and a ripple that widens and fades.  In
/// water it leaves a ring, widening.  Drizzle only ever leaves the speck.
fn draw_splash(pic: &mut Pic, t: &Tones, s: &Splash, ripple: bool) {
    let (sx, sy) = (s.x, s.y);
    let age = s.age / SPLASH_LIFE;
    if s.ring {
        // a ring seen from the side: an ellipse's outline, row by row, its
        // top and bottom rows drawn across
        let rx = 1.0 + age * if s.layer == NEAR { 7.0 } else { 4.0 };
        let ry = rx * 0.3;
        let a = 0.5 * (1.0 - age);
        let rows = ry.round() as i32;
        for dy in -rows..=rows {
            let f = dy as f32 / (ry + 0.5);
            let ex = (rx * (1.0 - f * f).max(0.0).sqrt()).round() as i32;
            if dy.abs() == rows {
                for x in sx - ex..=sx + ex {
                    pic.blend(x, sy + dy, t.rain, a);
                }
            } else {
                pic.blend(sx - ex, sy + dy, t.rain, a);
                pic.blend(sx + ex, sy + dy, t.rain, a);
            }
        }
        return;
    }
    if s.layer == FAR || !ripple {
        return pic.blend(sx, sy, t.rain, 0.6);
    }
    let near = s.layer == NEAR;
    let a = 0.6 * (1.0 - age);
    let reach = 1 + (age * if near { 6.0 } else { 4.0 }) as i32;
    let line = |pic: &mut Pic, x0: i32, y: i32, n: i32| {
        for x in x0..x0 + n {
            pic.blend(x, y, t.rain, a);
        }
    };
    if reach < 3 {
        line(pic, sx - reach, sy, 2 * reach + 1);
    } else {
        line(pic, sx - reach, sy, 2);
        line(pic, sx + reach - 1, sy, 2);
        if near {
            line(pic, sx - reach + 2, sy - 1, 2 * reach - 3);
            line(pic, sx - reach + 2, sy + 1, 2 * reach - 3);
        }
    }
    if age < 0.2 {
        pic.blend(sx, sy - 1, t.glint, 0.9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin;

    fn sky(scene: Scene, light: Light) -> Sky {
        let mut s = Sky::new(&skin::weather_window::SKIN, scene, light, -9.0, 7);
        s.set_fixed(3.0);
        s
    }

    #[test]
    fn every_scene_has_a_key_and_a_code_and_comes_back_from_its_key() {
        for s in Scene::ALL {
            assert_eq!(Scene::from_key(s.key()), Some(s));
            assert_eq!(Scene::from_key(&s.key().to_uppercase()), Some(s));
        }
        assert_eq!(Scene::from_key(""), None);
        assert_eq!(Scene::from_key("sunny"), None);
        // every WMO code there is comes to one of them, and each of the
        // twelve is one some code comes to
        let codes = [0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82, 85, 86, 95, 96, 99];
        for s in Scene::ALL {
            assert!(codes.iter().any(|&c| Scene::of_code(c) == s), "no code comes to {s:?}");
        }
        assert_eq!(Scene::of_code(0), Scene::Clear);
        assert_eq!(Scene::of_code(66), Scene::Sleet);
        assert_eq!(Scene::of_code(75), Scene::Blizzard);
        assert_eq!(Scene::of_code(99), Scene::Hail);
        assert_eq!(Scene::of_code(-1), Scene::Overcast);
    }

    #[test]
    fn the_same_weather_twice_and_no_two_scenes_alike() {
        let mut seen: Vec<Vec<u32>> = Vec::new();
        for s in Scene::ALL {
            let (a, b) = (sky(s, Light::NOON), sky(s, Light::NOON));
            assert!(a.pixels() == b.pixels(), "{s:?} came out two ways");
            assert!(!seen.iter().any(|p| p == a.pixels()), "{s:?} is the same picture as another scene");
            seen.push(a.pixels().to_vec());
        }
    }

    #[test]
    fn night_is_darker_than_day_and_the_lights_come_on() {
        let mean = |s: &Sky| s.pixels().iter().map(|&p| luma(p)).sum::<f32>() / s.pixels().len() as f32;
        for scene in Scene::ALL {
            let (day, night) = (sky(scene, Light::NOON), sky(scene, Light::MIDNIGHT));
            assert!(mean(&night) < mean(&day) - 0.1, "{scene:?}: night no darker than day");
            assert_eq!(night.tones.stars > 0.0, matches!(scene, Scene::Clear | Scene::Partly), "{scene:?}");
            assert_eq!(day.tones.stars, 0.0);
            assert!(night.tones.glow > 0.9 && day.tones.glow < 0.35, "{scene:?}");
        }
        // the readings are dark over a pale sky, light over a dark one
        assert!(sky(Scene::Fog, Light::NOON).bright());
        assert!(!sky(Scene::Clear, Light::MIDNIGHT).bright());
        assert!(!sky(Scene::Storm, Light::MIDNIGHT).bright());
    }

    #[test]
    fn what_falls_lands_on_the_ground_and_comes_round_again() {
        for scene in Scene::ALL {
            let s = sky(scene, Light::NOON);
            let m = mood(scene);
            let (g0, g1) = s.land.ground;
            assert_eq!(s.drops.len(), m.rain.map_or(0, |r| r.drops), "{scene:?}");
            assert_eq!(s.flakes.len(), m.snow, "{scene:?}");
            assert_eq!(s.stones.len(), m.ice, "{scene:?}");
            for d in &s.drops {
                assert!(d.y < d.land && d.land >= g0 as f32 && d.land <= g1 as f32);
            }
            for f in &s.flakes {
                assert!(f.y < f.land && f.land >= g0 as f32 && f.land <= g1 as f32);
            }
            for st in &s.stones {
                assert!(st.y <= st.land + 0.01 && st.land < s.h as f32);
            }
            assert_eq!(s.frame_ms(), if m.falls() { FALLING_MS } else if m.fog { FOG_MS } else { CALM_MS });
        }
    }

    #[test]
    fn a_storm_strikes_and_its_flash_lights_the_sky() {
        let mut s = sky(Scene::Storm, Light::MIDNIGHT);
        // set_fixed has a bolt just struck
        let strike = s.strike.as_ref().expect("no strike in a fixed storm");
        assert!(!strike.path.is_empty() && strike.flash() > 0.5);
        let bolt = s.tones.bolt;
        assert!(s.pixels().contains(&bolt));
        // and a while later it has gone
        for _ in 0..25 {
            s.step(0.04);
        }
        s.render();
        assert!(s.strike.is_none());
        assert!(!s.pixels().contains(&bolt));
    }

    #[test]
    fn the_moon_wanes_on_the_other_side() {
        let lit = |age: f32| {
            let mut s = Sky::new(&skin::weather_window::SKIN, Scene::Clear, Light { moon: age, ..Light::MIDNIGHT }, 0.0, 1);
            s.set_fixed(0.0);
            // nothing passing in front of it
            s.clouds.clear();
            s.streaks.clear();
            s.render();
            let (x, y) = s.arc(0.5);
            let (cx, cy) = (x.round() as i32, (y + 6.0).round() as i32);
            let moon = s.tones.moon[0];
            let count = |side: i32| (1..6).filter(|&dx| s.pixels()[(cy * s.w + cx + side * dx) as usize] == moon).count();
            (count(-1), count(1))
        };
        let (left, right) = lit(0.25);
        assert!(right > left, "first quarter: lit on the right ({left} {right})");
        let (left, right) = lit(0.75);
        assert!(left > right, "last quarter: lit on the left ({left} {right})");
        let (left, right) = lit(0.5);
        assert!(left >= 4 && right >= 4, "full: lit both sides");
    }

    #[test]
    fn water_shows_what_stands_over_it() {
        // the lake by day: the sky and the mountains in it, lighter than the
        // water's own colour -- most of all in its nearer rows, which mirror
        // what is highest
        let s = sky(Scene::Clear, Light::NOON);
        assert!(!s.water.is_empty());
        let near: Vec<u32> = s.water.iter().filter(|&&(i, up)| i as i32 / s.w - up as i32 > 40).map(|&(i, _)| i).collect();
        assert!(!near.is_empty());
        let own = near.iter().map(|&i| luma(s.tones.land[i as usize])).sum::<f32>();
        let seen = near.iter().map(|&i| luma(s.pixels()[i as usize])).sum::<f32>();
        assert!(seen > own * 1.3, "the lake shows nothing of the sky ({seen} against {own})");
    }

    #[test]
    fn another_scene_starts_afresh_over_its_own_place() {
        let mut s = sky(Scene::Rain, Light::NOON);
        assert!(!s.set_scene(Scene::Rain));
        assert!(s.set_scene(Scene::Snow));
        assert!(s.drops.is_empty() && s.flakes.len() == mood(Scene::Snow).snow);
        assert_eq!(s.land.chimneys.len(), 1);
        assert!(!s.set_light(Light::NOON));
        assert!(s.set_light(Light::MIDNIGHT));
    }

    #[test]
    fn a_city_is_known_by_where_it_is() {
        assert_eq!(City::near(51.4995, -0.1248), Some(City::London)); // Westminster
        assert_eq!(City::near(51.4, -0.3), Some(City::London)); // Kingston
        assert_eq!(City::near(57.1764, -2.0896), Some(City::Aberdeen)); // Bridge of Don
        assert_eq!(City::near(56.4620, -2.9707), None); // Dundee
        assert_eq!(City::near(48.8566, 2.3522), None); // Paris
        for c in City::ALL {
            assert_eq!(City::from_key(c.key()), Some(c));
        }
    }

    #[test]
    fn aberdeen_turns_to_its_harbour_after_dark_and_back() {
        let mut s = sky(Scene::Clear, Light::NOON);
        assert!(s.set_city(Some(City::Aberdeen)));
        assert!(!s.harbour && s.land.glints.len() > 20, "Broad Street by day, its granite glinting");
        s.set_light(Light::MIDNIGHT);
        assert!(s.harbour && s.land.blinkers.len() >= 4, "the harbour after dark, its cranes' and buoys' lights");
        s.set_light(Light::NOON);
        assert!(!s.harbour);
        // London has the time on Big Ben
        assert!(s.set_city(Some(City::London)));
        assert!(s.land.clock.is_some() && s.land.wheel.is_some());
        assert!(s.set_clock(15, 40));
        assert!(!s.set_clock(15, 40));
        s.render();
    }

    #[test]
    fn what_goes_by_goes_by() {
        let mut s = sky(Scene::Rain, Light::NOON);
        s.set_city(Some(City::London));
        assert!(!s.riders.is_empty());
        let before: Vec<f32> = s.riders.iter().map(|r| r.x).collect();
        for _ in 0..25 {
            s.step(0.04);
        }
        let moved = s.riders.iter().zip(&before).filter(|(r, x)| (r.x - **x).abs() > 1.0).count();
        assert_eq!(moved, s.riders.len());
        for r in &s.riders {
            let lane = &s.land.lanes[r.lane];
            assert!(r.x >= lane.from && r.x <= lane.to);
        }
    }
}
