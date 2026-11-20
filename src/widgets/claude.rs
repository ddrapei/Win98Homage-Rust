//! Claude Usage: how much of a Claude plan's two usage limits has been used
//! -- the current session, which lasts five hours from its first message,
//! and the current week -- as two gauges side by side, after a car's fuel
//! gauge, each with the percentage at the foot of its face and how long
//! until the limit resets under it.
//!
//! The figures are the ones Claude Code's /usage shows.  There is no
//! published way to ask for them: they come from the address /usage itself
//! asks, with the sign-in Claude Code keeps in `.credentials.json`.  That
//! sign-in is Claude Code's, and it is only ever read here -- never renewed,
//! which would use up the renewal Claude Code holds and sign it out.  When it
//! has run out, the figures on show stay, and the line under each gauge says
//! "Signed out" until Claude Code, which renews it, has been opened; the
//! moment the file changes, they are asked for again.
//!
//! As with the weather, the network is kept out of this process (see `net`):
//! a fetch is the program run again as `--fetch-claude`, which reads the
//! sign-in, asks, writes the figures -- never the sign-in -- to a cache file
//! and exits.  This process only ever looks at when the sign-in file was
//! last changed, never at what is in it.  The cache also means a restart
//! shows the last figures at once.
//!
//! A gauge is drawn as the Battery is, and in the same light: a pixel at a
//! time, from the upper left and a little in front, each surface in the
//! shade of how far it is turned to the light, and nothing outlined.  Every
//! part is a shape with a front to it -- the bezel and the segments of the
//! scale are rounded across, the needle is a ridge, the hub and its cap are
//! domes -- so the shading follows from which way each px of it faces.  The
//! face is set in under the bezel and takes its shadow, and the needle's;
//! the glass over it all has a streak of light on it.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::SystemTime;

use crate::canvas::Canvas;
use crate::clock;
use crate::geocode::until;
use crate::http;
use crate::settings::{data_dir, write_whole};
use crate::skin::{self, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Request, Widget};

static SKIN: &Skin = &skin::claude::SKIN;

/// Seconds between asks; after one that found nobody at home; and after a
/// limit resets, before asking what it is now.
const REFRESH: i64 = 300;
const RETRY: i64 = 120;
const AFTER_RESET: i64 = 60;
/// Seconds between looks at the sign-in file while it is no use.
const LOOK: i64 = 15;
/// Figures older than this are not worth showing on start.
const STALE: i64 = 6 * 3600;
/// A fetch still running after this long is given up on.
const FETCH_LIMIT: i64 = 60;

/// How `--fetch-claude` ends.  The cache file says the same, in `error=`.
const EXIT_NETWORK: i32 = 1;
const EXIT_ANSWER: i32 = 2;
const EXIT_SIGNED_OUT: i32 = 3;
const EXIT_EXPIRED: i32 = 4;

/// What the address answers with is asked for with these, as /usage does.
const HOST: &str = "api.anthropic.com";
const PATH: &str = "/api/oauth/usage";
const BETA: &str = "oauth-2025-04-20";

/// Where Claude Code keeps its sign-in: in the folder CLAUDE_CONFIG_DIR
/// names, if it names one, and otherwise in .claude in the user's own.
fn credentials_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join(".credentials.json"));
    }
    let home = std::env::var_os("USERPROFILE").filter(|d| !d.is_empty()).or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".claude").join(".credentials.json"))
}

/// When the sign-in file was last changed, if there is one.
fn stamp(path: Option<&Path>) -> Option<SystemTime> {
    path?.metadata().ok()?.modified().ok()
}

// ---------------------------------------------------------------------------
//  Reading JSON the way everything from the network is read here: by
//  looking for the keys that are wanted.
// ---------------------------------------------------------------------------

/// What follows `"key":` in `json`, past any spaces either side of the colon.
/// The same name as a string value, not a key, is passed over.
fn field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let quoted = format!("\"{key}\"");
    let mut rest = json;
    loop {
        rest = &rest[rest.find(&quoted)? + quoted.len()..];
        if let Some(value) = rest.trim_start().strip_prefix(':') {
            return Some(value.trim_start());
        }
    }
}

/// A string value, up to its closing quote: none of the ones wanted here has
/// anything in it that would need escaping.
fn string(value: &str) -> Option<&str> {
    let s = value.strip_prefix('"')?;
    Some(&s[..s.find('"')?])
}

/// A number value.
fn number(value: &str) -> Option<f64> {
    until(value, &[',', '}', ']', ' ', '\r', '\n']).parse().ok()
}

/// What the sign-in file says: the token to ask with, and when it stops
/// being any use, if it says.
struct SignIn {
    token: String,
    expires: Option<i64>,
}

fn sign_in(json: &str) -> Option<SignIn> {
    let oauth = field(json, "claudeAiOauth")?;
    let token = string(field(oauth, "accessToken")?)?;
    if token.is_empty() {
        return None;
    }
    // in milliseconds
    let expires = field(oauth, "expiresAt").and_then(number).map(|ms| (ms / 1000.0) as i64);
    Some(SignIn { token: token.to_owned(), expires })
}

/// One limit: how much of it is used, in percent, and when it resets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Limit {
    pct: f64,
    resets: Option<i64>,
}

/// One of the answer's limits: `"five_hour": {"utilization": 42.0,
/// "resets_at": "2026-11-20T19:00:00+00:00"}`.  The limit, or `Some(None)`
/// when it is null -- there is none running -- or `None` when the key is not
/// there at all, which is an answer this does not understand.  The object is
/// flat, so it ends at its first brace, and nothing in it is taken from the
/// one after.
fn limit(json: &str, key: &str) -> Option<Option<Limit>> {
    let value = field(json, key)?;
    if value.starts_with("null") {
        return Some(None);
    }
    let body = value.strip_prefix('{')?;
    let body = &body[..body.find('}')?];
    let Some(pct) = field(body, "utilization").and_then(number) else { return Some(None) };
    let resets = field(body, "resets_at").and_then(string).and_then(clock::parse_iso_utc);
    Some(Some(Limit { pct, resets }))
}

/// The session's and the week's limits out of the answer.
fn parse_usage(json: &str) -> Option<(Option<Limit>, Option<Limit>)> {
    Some((limit(json, "five_hour")?, limit(json, "seven_day")?))
}

// ---------------------------------------------------------------------------
//  The cache
// ---------------------------------------------------------------------------

/// What a fetch leaves behind: `key=value` lines in the cache file.  Never
/// the sign-in.
#[derive(Clone, Debug, Default, PartialEq)]
struct Report {
    /// When the figures were got; 0 before there have been any.
    fetched: i64,
    /// When a fetch last ran, whatever became of it.
    tried: i64,
    session: Option<Limit>,
    week: Option<Limit>,
    /// What went wrong the last time, if anything: "signin" (there is no
    /// sign-in), "expired" (it has run out, or was turned away), "network"
    /// or "answer" (the answer was not one this understands).
    error: String,
}

impl Report {
    fn parse(text: &str) -> Report {
        let mut r = Report::default();
        let (mut session, mut session_resets, mut week, mut week_resets) = (None, None, None, None);
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "fetched" => r.fetched = v.parse().unwrap_or(0),
                "tried" => r.tried = v.parse().unwrap_or(0),
                "session" => session = v.parse::<f64>().ok(),
                "session_resets" => session_resets = v.parse::<i64>().ok(),
                "week" => week = v.parse::<f64>().ok(),
                "week_resets" => week_resets = v.parse::<i64>().ok(),
                "error" => r.error = v.to_owned(),
                _ => {}
            }
        }
        r.session = session.map(|pct| Limit { pct, resets: session_resets });
        r.week = week.map(|pct| Limit { pct, resets: week_resets });
        r
    }

    fn write(&self, path: &Path) {
        let pct = |l: Option<Limit>| l.map(|l| l.pct.to_string()).unwrap_or_default();
        let resets = |l: Option<Limit>| l.and_then(|l| l.resets).map(|t| t.to_string()).unwrap_or_default();
        let text = format!(
            "fetched={}\ntried={}\nsession={}\nsession_resets={}\nweek={}\nweek_resets={}\nerror={}\n",
            self.fetched, self.tried, pct(self.session), resets(self.session), pct(self.week), resets(self.week), self.error
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = write_whole(path, text.as_bytes());
    }

    /// There are figures to show, from some time.
    fn has_figures(&self) -> bool {
        self.fetched > 0
    }
}

// ---------------------------------------------------------------------------
//  The words
// ---------------------------------------------------------------------------

/// The time until something, short enough to fit under a gauge: "4h 19m";
/// from ten hours off, to the hour, "14h"; from a day off, "2d 7h".  Never
/// more than two figures and a letter twice over.  Rounded up, so it never
/// says 0m before it has happened.
fn span(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds >= 10 * 3600 {
        let hours = (seconds + 3599) / 3600;
        return match (hours / 24, hours % 24) {
            (0, h) => format!("{h}h"),
            (d, 0) => format!("{d}d"),
            (d, h) => format!("{d}d {h}h"),
        };
    }
    let minutes = ((seconds + 59) / 60).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// "43%", as the Battery writes its charge.
fn percent(pct: f64) -> String {
    format!("{}%", (pct.max(0.0) + 0.5).floor() as i64)
}

// ---------------------------------------------------------------------------
//  The light, and the shades a colour takes in it
// ---------------------------------------------------------------------------

const WHITE: u32 = 0xFFFF_FFFF;
const BLACK: u32 = 0xFF00_0000;

/// Which way the light comes from, as on the Battery: the upper left and a
/// little in front.  x is to the right, y down, z out of the screen.
const LIGHT: [f32; 3] = [-0.5, -0.6, 0.62];

/// `to` laid `percent` of the way over `from`, both 0xAARRGGBB, opaque: how
/// the Battery mixes its shades.
fn mix(from: u32, to: u32, percent: i32) -> u32 {
    let k = percent.clamp(0, 100) as u32;
    let channel = |shift: u32| ((from >> shift & 255) * (100 - k) + (to >> shift & 255) * k + 50) / 100 << shift;
    0xFF00_0000 | channel(16) | channel(8) | channel(0)
}

/// How square to the light a surface facing `n` is: about 1 facing it, 0
/// edge on, and below 0 turned away.
fn facing(n: [f32; 3]) -> f32 {
    n[0] * LIGHT[0] + n[1] * LIGHT[1] + n[2] * LIGHT[2]
}

/// Which way the rounded top of a ring faces at (u, v), `r` from the centre:
/// a tube bent round the centre at radius `mid`, `half` px either side of
/// it.  Also how far across the tube the px is, -1 on the inside to 1 on the
/// outside.
fn tube(u: f32, v: f32, r: f32, mid: f32, half: f32) -> ([f32; 3], f32) {
    let w = ((r - mid) / half).clamp(-1.0, 1.0);
    let (cu, cv) = if r > 0.0 { (u / r, v / r) } else { (0.0, 0.0) };
    ([w * cu, w * cv, (1.0 - w * w).sqrt()], w)
}

/// Which way a dome of `radius` faces at (u, v) from its middle.
fn dome(u: f32, v: f32, radius: f32) -> [f32; 3] {
    let (a, b) = (u / radius, v / radius);
    [a, b, (1.0 - a * a - b * b).max(0.0).sqrt()]
}

/// One colour in the shades a surface takes as it turns from the light: a
/// glint facing it square, shine, a lighter stretch, its own colour, shade
/// and edge -- and the light thrown back at the far edge of a rounded one,
/// a little lighter than the edge before it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Shades {
    glint: u32,
    shine: u32,
    lighter: u32,
    body: u32,
    shade: u32,
    edge: u32,
    rim: u32,
}

impl Shades {
    /// `body` shaded as a Battery section is: `shine` and `lighter` percent
    /// of the way to white, and `shade`, `edge` and `rim` to black.
    fn of(body: u32, shine: i32, lighter: i32, shade: i32, edge: i32, rim: i32) -> Shades {
        Shades {
            glint: mix(body, WHITE, (shine + 100) / 2),
            shine: mix(body, WHITE, shine),
            lighter: mix(body, WHITE, lighter),
            body,
            shade: mix(body, BLACK, shade),
            edge: mix(body, BLACK, edge),
            rim: mix(body, BLACK, rim),
        }
    }

    /// The shade of a surface `b` square to the light.  Stepped, as pixel art
    /// shades, never smoothed.
    fn lit(&self, b: f32) -> u32 {
        match b {
            b if b >= 0.84 => self.glint,
            b if b >= 0.72 => self.shine,
            b if b >= 0.5 => self.lighter,
            b if b >= 0.2 => self.body,
            b if b >= -0.2 => self.shade,
            _ => self.edge,
        }
    }

    /// The same for a rounded surface `w` of the way across it: at its very
    /// edge on the side turned away, the light thrown back.
    fn rounded(&self, b: f32, w: f32) -> u32 {
        if w.abs() > 0.8 && b < -0.2 { self.rim } else { self.lit(b) }
    }
}

/// The colour `t` of the way round the scale, 0 at its start and 1 at its
/// end: from `low` to `mid` over the first half, and on to `high`.
fn round_the_scale(low: u32, mid: u32, high: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.5 {
        mix(low, mid, (t * 200.0).round() as i32)
    } else {
        mix(mid, high, ((t - 0.5) * 200.0).round() as i32)
    }
}

/// Every colour a gauge is drawn in.
#[derive(Clone, Debug, Default, PartialEq)]
struct Palette {
    /// The bezel and the hub's cap: the chrome's greys.
    metal: Shades,
    face: u32,
    groove: u32,
    /// Each segment's when it is lit, from the first; one not lit.
    lit: Vec<Shades>,
    off: Shades,
    needle: Shades,
    /// Percents: the end of a segment turned to the light; the bezel's
    /// shadow in the face; the needle's on it; the glass's reflection.
    end: i32,
    recess: i32,
    cast: i32,
    glass: i32,
}

impl Palette {
    fn from_skin(segments: i32) -> Palette {
        let pct = |name: &str, default: i32| SKIN.int(name, default);
        let (shine, lighter) = (pct("ScaleShine", 55), pct("ScaleLight", 25));
        let (shade, edge, rim) = (pct("ScaleShade", 22), pct("ScaleEdge", 42), pct("ScaleRim", 14));
        let (low, mid, high) = (SKIN.colour("DialLow"), SKIN.colour("DialMid"), SKIN.colour("DialHigh"));
        let last = (segments - 1).max(1) as f32;
        // the metal is the chrome's greys, as the Battery's is
        let (hilite, light, shadow) = (SKIN.colour("Hilite"), SKIN.colour("Light"), SKIN.colour("Shadow"));
        let face = SKIN.colour("DialFace");
        Palette {
            metal: Shades {
                glint: mix(hilite, WHITE, 60),
                shine: hilite,
                lighter: mix(light, hilite, 50),
                body: light,
                shade: mix(light, shadow, 50),
                edge: shadow,
                rim: mix(shadow, light, 45),
            },
            face,
            groove: mix(face, BLACK, pct("DialGroove", 40)),
            lit: (0..segments)
                .map(|i| Shades::of(round_the_scale(low, mid, high, i as f32 / last), shine, lighter, shade, edge, rim))
                .collect(),
            off: Shades::of(SKIN.colour("DialOff"), pct("ScaleOffShine", 16), pct("ScaleOffLight", 7), shade, edge, rim),
            needle: Shades::of(SKIN.colour("Needle"), shine, lighter, shade, edge, rim),
            end: pct("ScaleEnd", 30),
            recess: pct("DialRecess", 35),
            cast: pct("DialCast", 40),
            glass: pct("DialGlass", 10),
        }
    }
}

// ---------------------------------------------------------------------------
//  The gauges
// ---------------------------------------------------------------------------

/// What is on the face where the needle and the hub are.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pointer {
    /// The metal cap in the middle of the hub, and the hub round it.
    Cap,
    Hub,
    /// The needle, `w` of the way across it, -1 to 1, and which way its
    /// side faces, as x and y.
    Needle { w: f32, side: [f32; 2] },
}

/// A gauge as the skin lays it out.
#[derive(Clone, Copy, Debug)]
struct Dial {
    /// The picture's side, and the bezel's outer diameter; the bezel's width.
    d: i32,
    bezel: f32,
    scale_in: f32,
    scale_out: f32,
    /// How far round the scale runs, in degrees: as far either side of
    /// twelve o'clock, so it starts at the lower left and ends at the lower
    /// right, and the foot of the face is open between its ends.
    sweep: f32,
    segments: i32,
    gap: f32,
    needle: f32,
    needle_w: f32,
    hub: f32,
    cap: f32,
}

impl Dial {
    fn from_skin() -> Dial {
        let num = |name: &str, default: f64| SKIN.num(name, default) as f32;
        Dial {
            d: SKIN.int("DialD", 89).max(9),
            bezel: num("DialBezel", 8.0).max(1.0),
            scale_in: num("ScaleIn", 23.0),
            scale_out: num("ScaleOut", 31.0),
            sweep: num("ScaleSweep", 240.0).clamp(10.0, 350.0),
            segments: SKIN.int("Segments", 10).max(1),
            gap: num("SegmentGap", 2.0),
            needle: num("NeedleLen", 21.0),
            needle_w: num("NeedleW", 2.0).max(0.5),
            hub: num("HubR", 5.0),
            cap: num("CapR", 2.0),
        }
    }

    /// Where the scale starts, in degrees anticlockwise from three o'clock.
    fn start(&self) -> f32 {
        90.0 + self.sweep / 2.0
    }

    fn radius(&self) -> f32 {
        self.d as f32 / 2.0
    }

    /// How far from the centre the face goes, inside the bezel.
    fn face(&self) -> f32 {
        self.radius() - self.bezel
    }

    /// From the centre to the middle of pixel (x, y), right and down.
    fn offset(&self, x: i32, y: i32) -> (f32, f32) {
        let c = self.radius();
        (x as f32 + 0.5 - c, y as f32 + 0.5 - c)
    }

    /// Whether (u, v) is in the band the scale runs round in.
    fn in_band(&self, r: f32) -> bool {
        r > self.scale_in && r <= self.scale_out
    }

    /// The segment of the scale at (u, v), if there is one there: which, how
    /// far along it from its start, 0 to 1, and how many of those is one px.
    /// The scale runs from the lower left clockwise over the top to the lower
    /// right; between each two segments there is `gap` px of groove,
    /// measured half way across the band.
    fn segment(&self, u: f32, v: f32) -> Option<(i32, f32, f32)> {
        let r = (u * u + v * v).sqrt();
        if !self.in_band(r) {
            return None;
        }
        // degrees anticlockwise from three o'clock, in the turn that ends
        // where the scale starts, so the scale is one unbroken run of them
        let mut a = (-v).atan2(u).to_degrees();
        if a < self.start() - 360.0 {
            a += 360.0;
        }
        let t = (self.start() - a) / self.sweep;
        if !(0.0..1.0).contains(&t) {
            return None;
        }
        let along = t * self.segments as f32;
        let k = along.floor();
        let length = self.sweep.to_radians() * (self.scale_in + self.scale_out) / 2.0 / self.segments as f32;
        let half_gap = self.gap / 2.0 / length;
        let f = along - k;
        let inside = 1.0 - 2.0 * half_gap;
        (f >= half_gap && f <= 1.0 - half_gap).then_some((k as i32, (f - half_gap) / inside, 1.0 / (length * inside)))
    }

    /// Which way the needle points at `pct`, as x and y on the screen.
    fn bearing(&self, pct: i32) -> [f32; 2] {
        let a = (self.start() - self.sweep * pct.clamp(0, 100) as f32 / 100.0).to_radians();
        [a.cos(), -a.sin()]
    }

    /// The needle on `pct` and the hub it turns on, at (u, v), if they are
    /// there.  The needle leaves the hub `needle_w` px either side of its
    /// middle and narrows to its tip.
    fn pointer(&self, u: f32, v: f32, pct: i32) -> Option<Pointer> {
        let r2 = u * u + v * v;
        if r2 <= (self.cap + 0.5) * (self.cap + 0.5) {
            return Some(Pointer::Cap);
        }
        if r2 <= (self.hub + 0.5) * (self.hub + 0.5) {
            return Some(Pointer::Hub);
        }
        let [dx, dy] = self.bearing(pct);
        let along = u * dx + v * dy;
        if along < 0.0 || along > self.needle {
            return None;
        }
        // across it, to its right as it points
        let across = v * dx - u * dy;
        let half = self.needle_w - (self.needle_w - 0.5) * along / self.needle;
        (across.abs() <= half + 0.25).then(|| Pointer::Needle { w: (across / (half + 0.25)).clamp(-1.0, 1.0), side: [-dy, dx] })
    }

    /// Whether the glass's reflection is on (u, v): the light from the upper
    /// left caught on the glass as a streak just inside the bezel, round
    /// from ten o'clock to half past eleven, widest in its middle and
    /// narrowing to nothing at either end.
    fn glass(&self, u: f32, v: f32) -> bool {
        let r = (u * u + v * v).sqrt();
        let a = (-v).atan2(u).to_degrees();
        let along = (a - 135.0) / 30.0;
        let half = 0.9 * (1.0 - along * along);
        half > 0.0 && (r - (self.face() - 3.25)).abs() <= half
    }

    /// The colour of (u, v): the bezel; or on the face, under the glass, the
    /// needle and the hub, the scale, the groove or the face itself, with the
    /// shadows that fall on them.  0 outside the bezel.
    fn colour(&self, u: f32, v: f32, lit: i32, pct: i32, p: &Palette) -> u32 {
        let r = (u * u + v * v).sqrt();
        if r > self.radius() {
            return 0;
        }
        if r > self.face() {
            // a ring rounded across, outer slope and inner
            let (n, w) = tube(u, v, r, self.radius() - self.bezel / 2.0, self.bezel / 2.0);
            return p.metal.rounded(facing(n), w);
        }
        let under = match self.pointer(u, v, pct) {
            Some(Pointer::Cap) => p.metal.lit(facing(dome(u, v, self.cap + 0.5))),
            Some(Pointer::Hub) => p.needle.lit(facing(dome(u, v, self.hub + 0.5))),
            Some(Pointer::Needle { w, side }) => {
                // a ridge along its length
                let n = [side[0] * w, side[1] * w, (1.0 - w * w).sqrt()];
                p.needle.rounded(facing(n), w)
            }
            None => {
                let ground = match self.segment(u, v) {
                    Some((k, f, px)) => self.segment_colour(u, v, r, k, f, px, lit, p),
                    None if self.in_band(r) => p.groove,
                    None => p.face,
                };
                // the needle's shadow, down and right of it, and the bezel's
                // in the face below its edge at the upper left
                let cast = self.pointer(u - 2.0, v - 2.0, pct).is_some();
                let (bu, bv) = (u - 2.0, v - 2.5);
                let recessed = (bu * bu + bv * bv).sqrt() > self.face();
                let ground = if cast { mix(ground, BLACK, p.cast) } else { ground };
                if recessed { mix(ground, BLACK, p.recess) } else { ground }
            }
        };
        if self.glass(u, v) { mix(under, WHITE, p.glass) } else { under }
    }

    /// A px of segment `k`, `f` of the way along it, where one px is `px` of
    /// that: rounded across like the bezel, with its end that is turned to
    /// the light catching it and the other in shade.
    #[allow(clippy::too_many_arguments)]
    fn segment_colour(&self, u: f32, v: f32, r: f32, k: i32, f: f32, px: f32, lit: i32, p: &Palette) -> u32 {
        let shades = if k < lit { &p.lit[k as usize] } else { &p.off };
        let half = (self.scale_out - self.scale_in) / 2.0;
        let (n, w) = tube(u, v, r, self.scale_in + half, half);
        let colour = shades.rounded(facing(n), w);
        // the way round the scale, clockwise, here
        let (cu, cv) = (u / r, v / r);
        let onward = [-cv, cu];
        let end = if f < px {
            Some(-1.0)
        } else if f > 1.0 - px {
            Some(1.0)
        } else {
            None
        };
        match end {
            Some(sign) => {
                let towards = sign * (onward[0] * LIGHT[0] + onward[1] * LIGHT[1]);
                if towards > 0.25 {
                    mix(colour, WHITE, p.end)
                } else if towards < -0.25 {
                    mix(colour, BLACK, p.end * 2 / 3)
                } else {
                    colour
                }
            }
            None => colour,
        }
    }

    /// The whole picture, a colour each px, row after row, 0 where the gauge
    /// is not: with `lit` of the segments lit and the needle on `pct`.
    fn picture(&self, lit: i32, pct: i32, p: &Palette) -> Vec<u32> {
        (0..self.d * self.d)
            .map(|i| {
                let (u, v) = self.offset(i % self.d, i / self.d);
                self.colour(u, v, lit, pct, p)
            })
            .collect()
    }
}

/// One gauge as it is on show.
#[derive(Clone, Debug, Default, PartialEq)]
struct Gauge {
    /// Segments lit: those the reading has gone past the end of.
    lit: i32,
    /// Where the needle points, in whole percent.
    needle: i32,
    pct: String,
    /// How long until the limit resets, or what the trouble is.
    reset: String,
}

/// Where the figures stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Nothing asked yet, or nothing heard back.
    Loading,
    /// The figures came with the last ask.
    Fresh,
    /// The last ask found nobody at home.
    Offline,
    /// It was answered, with something this does not understand.
    Unreadable,
    /// There is no sign-in, or it has run out: Claude Code has to be opened.
    SignedOut,
}

impl State {
    /// What the line under each gauge says instead of when the limit resets.
    fn trouble(self) -> Option<&'static str> {
        match self {
            State::Fresh => None,
            State::Loading => Some("Loading..."),
            State::Offline => Some("Offline"),
            State::Unreadable => Some("Unreadable"),
            State::SignedOut => Some("Signed out"),
        }
    }
}

pub struct Claude {
    dial: Dial,
    palette: Palette,

    session: Gauge,
    week: Gauge,
    /// The two gauges' pictures, each `dial.d` square.
    session_picture: Vec<u32>,
    week_picture: Vec<u32>,

    /// The last figures, and what became of the last ask.
    report: Report,
    state: State,
    cache: PathBuf,
    credentials: Option<PathBuf>,
    /// When the sign-in file had last been changed, as a fetch set out.
    launched_stamp: Option<SystemTime>,
    /// The same, at the last look while it was no use.
    seen: Option<SystemTime>,
    next_look: i64,
    child: Option<(Child, i64)>,
    next_fetch: i64,
    /// The minute the words were last worked out for.
    minute: i64,
    /// For the test harness: never fetch.
    frozen: bool,
}

impl Claude {
    pub fn new(ctx: &Ctx) -> Claude {
        Claude::with_cache(ctx, data_dir().join("claude.txt"))
    }

    fn with_cache(ctx: &Ctx, cache: PathBuf) -> Claude {
        let dial = Dial::from_skin();
        let mut claude = Claude {
            dial,
            palette: Palette::from_skin(dial.segments),
            session: Gauge::default(),
            week: Gauge::default(),
            session_picture: Vec::new(),
            week_picture: Vec::new(),
            report: Report::default(),
            state: State::Loading,
            cache,
            credentials: credentials_path(),
            launched_stamp: None,
            seen: None,
            next_look: 0,
            child: None,
            next_fetch: 0,
            minute: ctx.now.div_euclid(60),
            frozen: false,
        };
        // Figures from before the restart are better than "Loading...", as
        // long as they are not too old; if they are new enough, and came
        // with no trouble, they are not asked for again until they are due.
        if let Ok(text) = std::fs::read_to_string(&claude.cache) {
            let report = Report::parse(&text);
            if report.has_figures() && ctx.now - report.fetched < STALE {
                if report.error.is_empty() && ctx.now - report.fetched < REFRESH {
                    claude.state = State::Fresh;
                    claude.next_fetch = claude.due(&report, report.fetched);
                }
                claude.report = report;
            }
        }
        claude.render(ctx.now);
        claude
    }

    /// For the test harness: show exactly this -- the session's percentage
    /// and when it resets, then the week's -- got a minute ago; or, with
    /// nothing, a machine with no sign-in.
    pub fn set_fixed(&mut self, fixed: Option<(f64, Option<i64>, f64, Option<i64>)>, now: i64) {
        self.frozen = true;
        match fixed {
            Some((session, session_resets, week, week_resets)) => {
                self.report = Report {
                    fetched: now - 60,
                    tried: now - 60,
                    session: Some(Limit { pct: session, resets: session_resets }),
                    week: Some(Limit { pct: week, resets: week_resets }),
                    error: String::new(),
                };
                self.state = State::Fresh;
            }
            None => {
                self.report = Report { error: String::from("signin"), ..Report::default() };
                self.state = State::SignedOut;
            }
        }
        self.render(now);
    }

    /// When to ask again after figures got at `at`: after the usual wait, or
    /// a minute after one of the limits resets, if that is sooner.
    fn due(&self, report: &Report, at: i64) -> i64 {
        [report.session, report.week]
            .into_iter()
            .flatten()
            .filter_map(|l| l.resets)
            .filter(|&t| t > at)
            .map(|t| t + AFTER_RESET)
            .fold(at + REFRESH, i64::min)
    }

    /// A gauge as it stands at `now`.  With no figures at all, "--".  Once a
    /// limit's reset has gone by, it is at nothing until it is asked for
    /// again: the session has not started, the week is starting over.  While
    /// there is trouble, the line under it says what.
    fn gauge(&self, limit: Option<Limit>, week: bool, now: i64) -> Gauge {
        let not_started = if week { "" } else { "Not started" };
        let at = |pct: f64, reset: &str| Gauge {
            lit: ((pct / 100.0 * self.dial.segments as f64).floor() as i32).clamp(0, self.dial.segments),
            needle: ((pct + 0.5).floor() as i32).clamp(0, 100),
            pct: percent(pct),
            reset: reset.to_owned(),
        };
        let mut gauge = match (self.report.has_figures(), limit) {
            (false, _) => Gauge { pct: String::from("--"), ..Gauge::default() },
            // none running
            (true, None) => at(0.0, not_started),
            (true, Some(limit)) => match limit.resets {
                Some(t) if t <= now => at(0.0, if week { "Resetting..." } else { not_started }),
                Some(t) => at(limit.pct, &format!("{} till reset", span(t - now))),
                None => at(limit.pct, if limit.pct <= 0.0 { not_started } else { "" }),
            },
        };
        if let Some(trouble) = self.state.trouble() {
            gauge.reset = trouble.to_owned();
        }
        gauge
    }

    /// Work out again everything on show; whether any of it changed.
    fn render(&mut self, now: i64) -> bool {
        let session = self.gauge(self.report.session, false, now);
        let week = self.gauge(self.report.week, true, now);
        let changed = session != self.session || week != self.week || self.session_picture.is_empty();
        let redraw = |old: &Gauge, new: &Gauge, picture: &[u32]| picture.is_empty() || (old.lit, old.needle) != (new.lit, new.needle);
        if redraw(&self.session, &session, &self.session_picture) {
            self.session_picture = self.dial.picture(session.lit, session.needle, &self.palette);
        }
        if redraw(&self.week, &week, &self.week_picture) {
            self.week_picture = self.dial.picture(week.lit, week.needle, &self.palette);
        }
        (self.session, self.week) = (session, week);
        changed
    }

    /// This program again, as the child that talks to the network.
    fn start_fetch(&mut self, now: i64) {
        self.launched_stamp = stamp(self.credentials.as_deref());
        let spawned = std::env::current_exe().and_then(|exe| {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            Command::new(exe).arg("--fetch-claude").arg(&self.cache).creation_flags(CREATE_NO_WINDOW).spawn()
        });
        match spawned {
            Ok(child) => self.child = Some((child, now)),
            Err(_) => {
                self.state = State::Offline;
                self.next_fetch = now + RETRY;
            }
        }
    }

    fn stop_child(&mut self) {
        if let Some((mut child, _)) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// The child has exited: what it left in the cache says how it went.
    fn finish_fetch(&mut self, now: i64) {
        let report = std::fs::read_to_string(&self.cache).map(|t| Report::parse(&t)).unwrap_or_default();
        if report.tried < now - FETCH_LIMIT - 5 {
            // it left nothing new
            self.state = State::Offline;
            self.next_fetch = now + RETRY;
            return;
        }
        // The child keeps the last figures when it gets none, so these are
        // the newest there are either way.
        self.report = report;
        match self.report.error.as_str() {
            "" => {
                self.state = State::Fresh;
                self.next_fetch = self.due(&self.report, now);
            }
            "signin" | "expired" => {
                // Nothing to do until Claude Code signs in again, which
                // changes the file: look out for that.
                self.state = State::SignedOut;
                self.seen = self.launched_stamp;
                self.next_look = now + LOOK;
            }
            "answer" => {
                self.state = State::Unreadable;
                self.next_fetch = now + REFRESH;
            }
            _ => {
                self.state = State::Offline;
                self.next_fetch = now + RETRY;
            }
        }
    }

    /// A gauge's picture into its meter, with its corner at `x`, `y`: a run
    /// of one colour at a time, and nothing outside the bezel.
    fn draw(&self, picture: &[u32], x: i32, y: i32, canvas: &mut Canvas) {
        for (row, line) in picture.chunks_exact(self.dial.d as usize).enumerate() {
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

impl Hooks for Claude {
    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "SessionPct" => &self.session.pct,
            "SessionReset" => &self.session.reset,
            "WeekPct" => &self.week.pct,
            "WeekReset" => &self.week.reset,
            _ => "",
        });
    }

    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        match meter.name {
            "MeterSessionDial" => self.draw(&self.session_picture, x, y, canvas),
            "MeterWeekDial" => self.draw(&self.week_picture, x, y, canvas),
            _ => {}
        }
    }
}

impl Widget for Claude {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Claude Usage\n\n\
         Session\thow much of the five-hour limit is used; a session starts \
         with its first message\n\
         Week\thow much of the weekly limit is used\n\n\
         Each segment of a gauge is a tenth of the limit, from green to red \
         as it fills.  Under each gauge is how long until the limit resets.\n\n\
         The same figures as /usage in Claude Code, asked for every five \
         minutes with the sign-in Claude Code keeps on this computer, and \
         sent nowhere but api.anthropic.com.  The widget never signs in, or \
         renews the sign-in, itself: when it says \"Signed out\", open Claude \
         Code and the figures come back."
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        if self.frozen {
            return Dirty::None;
        }
        let (now, was) = (ctx.now, self.state);
        let mut changed = false;
        if let Some((child, started)) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    self.finish_fetch(now);
                    changed = true;
                }
                Ok(None) if now - *started <= FETCH_LIMIT => {}
                _ => {
                    self.stop_child();
                    self.state = State::Offline;
                    self.next_fetch = now + RETRY;
                    changed = true;
                }
            }
        } else if self.state == State::SignedOut {
            if now >= self.next_look {
                self.next_look = now + LOOK;
                let stamp = stamp(self.credentials.as_deref());
                if stamp != self.seen {
                    self.seen = stamp;
                    self.start_fetch(now);
                }
            }
        } else if now >= self.next_fetch {
            self.next_fetch = now + RETRY; // if it cannot even be started
            self.start_fetch(now);
        }
        // the reset times move on by the minute
        let minute = now.div_euclid(60);
        if changed || self.state != was || minute != self.minute {
            self.minute = minute;
            if self.render(now) {
                return Dirty::All;
            }
        }
        Dirty::None
    }

    fn command(&mut self, cmd: &str, _arg: i32, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        match cmd {
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn retint(&mut self) {
        // the needle takes the colour picked; the rest are read again all
        // the same, as every colour taken from a skin is
        self.palette = Palette::from_skin(self.dial.segments);
        self.session_picture = self.dial.picture(self.session.lit, self.session.needle, &self.palette);
        self.week_picture = self.dial.picture(self.week.lit, self.week.needle, &self.palette);
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { id: 1, label: "Refresh now", checked: false });
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        if id == 1 && self.child.is_none() && !self.frozen {
            self.start_fetch(ctx.now);
        }
        Outcome::none()
    }

    fn shutdown(&mut self, _ctx: &mut Ctx) {
        self.stop_child();
    }
}

// ---------------------------------------------------------------------------
//  The fetch: this program run as `--fetch-claude <cache>`
// ---------------------------------------------------------------------------

/// Ask for the figures with Claude Code's sign-in, and leave what came of it
/// in the cache.  The sign-in is read here and only here; it goes nowhere
/// but in the one request, and is never written down.  One that has run out
/// is not even sent.
pub fn fetch_main(args: &[String]) -> i32 {
    let Some(cache) = args.first().map(PathBuf::from) else { return EXIT_ANSWER };
    let now = clock::now();
    // the figures already there stay, unless new ones come
    let mut report = std::fs::read_to_string(&cache).map(|t| Report::parse(&t)).unwrap_or_default();
    report.tried = now;

    let signed_in = credentials_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|text| sign_in(&text));
    let (code, error) = match signed_in {
        None => (EXIT_SIGNED_OUT, "signin"),
        Some(s) if s.expires.is_some_and(|at| at <= now) => (EXIT_EXPIRED, "expired"),
        Some(s) => {
            let headers = format!("Authorization: Bearer {}\r\nanthropic-beta: {BETA}\r\n", s.token);
            match http::fetch(HOST, 443, true, PATH, &headers) {
                Ok(json) => match parse_usage(&json) {
                    Some((session, week)) => {
                        (report.session, report.week, report.fetched) = (session, week, now);
                        (0, "")
                    }
                    None => (EXIT_ANSWER, "answer"),
                },
                Err(401 | 403) => (EXIT_EXPIRED, "expired"),
                Err(_) => (EXIT_NETWORK, "network"),
            }
        }
    };
    report.error = error.to_owned();
    report.write(&cache);
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    /// An answer laid out as the address gives it, with the per-model weekly
    /// limits -- whose names start with the week's -- before the week's own.
    const ANSWER: &str = r#"{
        "five_hour": { "utilization": 42.0, "resets_at": "2026-11-20T19:00:00.943648+00:00" },
        "seven_day_opus": { "utilization": 3.0, "resets_at": null },
        "seven_day_oauth_apps": null,
        "seven_day": { "utilization": 17.6, "resets_at": "2026-11-26T09:00:00Z" },
        "extra_usage": { "is_enabled": false, "utilization": null }
    }"#;

    #[test]
    fn reads_the_two_limits_and_no_others() {
        let (session, week) = parse_usage(ANSWER).unwrap();
        assert_eq!(session, Some(Limit { pct: 42.0, resets: clock::parse_iso_utc("2026-11-20T19:00:00Z") }));
        assert_eq!(week, Some(Limit { pct: 17.6, resets: clock::parse_iso_utc("2026-11-26T09:00:00Z") }));
        // none running: null, and nothing borrowed from the limit after it
        let (session, week) = parse_usage(r#"{"five_hour":null,"seven_day":{"utilization":5,"resets_at":null}}"#).unwrap();
        assert_eq!((session, week), (None, Some(Limit { pct: 5.0, resets: None })));
        // an answer without them is not one this understands
        assert_eq!(parse_usage(r#"{"error":{"type":"authentication_error"}}"#), None);
        assert_eq!(parse_usage(""), None);
    }

    #[test]
    fn reads_the_sign_in_and_when_it_runs_out() {
        let json = r#"{"claudeAiOauth":{"accessToken":"tok-123","refreshToken":"ref-456","expiresAt":1795201200000,"scopes":["user:inference"],"subscriptionType":"max"}}"#;
        let s = sign_in(json).unwrap();
        assert_eq!((s.token.as_str(), s.expires), ("tok-123", Some(1_795_201_200)));
        // laid out with spaces, and without saying when
        let s = sign_in("{ \"claudeAiOauth\" : { \"accessToken\" : \"tok\" } }").unwrap();
        assert_eq!((s.token.as_str(), s.expires), ("tok", None));
        // signed in with something else, or not at all
        assert!(sign_in(r#"{"primaryApiKey":"x"}"#).is_none());
        assert!(sign_in(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
        assert!(sign_in("").is_none());
    }

    #[test]
    fn the_cache_keeps_the_figures_and_never_needs_the_sign_in() {
        let report = Report {
            fetched: 100,
            tried: 200,
            session: None,
            week: Some(Limit { pct: 17.6, resets: Some(1_795_201_200) }),
            error: String::from("network"),
        };
        let path = std::env::temp_dir().join(format!("win98-claude-test-{}.txt", std::process::id()));
        report.write(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(Report::parse(&text), report);
        assert!(!text.to_lowercase().contains("token"));
        assert_eq!(Report::parse(""), Report::default());
    }

    #[test]
    fn times_are_written_short() {
        assert_eq!([span(0), span(1), span(59), span(60), span(61)], ["1m", "1m", "1m", "1m", "2m"]);
        assert_eq!([span(3600), span(2 * 3600 + 14 * 60), span(2 * 3600 + 13 * 60 + 1)], ["1h", "2h 14m", "2h 14m"]);
        // from ten hours off, to the hour, and from a day off, in days too
        assert_eq!([span(10 * 3600 - 1), span(10 * 3600), span(23 * 3600), span(23 * 3600 + 1)], ["10h", "10h", "23h", "1d"]);
        assert_eq!([span(86_400), span(2 * 86_400 + 6 * 3600 + 1), span(3 * 86_400)], ["1d", "2d 7h", "3d"]);
        assert_eq!([percent(0.0), percent(41.6), percent(-3.0), percent(104.0)], ["0%", "42%", "0%", "104%"]);
    }

    fn widget(settings: &mut Settings, now: i64) -> Claude {
        // a cache that is not there: nothing of the user's is read
        let ctx = Ctx { now, settings, locked: false, signed_in: None };
        Claude::with_cache(&ctx, std::env::temp_dir().join("win98-claude-test-none").join("claude.txt"))
    }

    #[test]
    fn what_the_gauges_say() {
        let mut settings = Settings::in_memory();
        let now = 1_795_150_000;
        let mut w = widget(&mut settings, now);
        // nothing yet
        assert_eq!((w.session.pct.as_str(), w.session.reset.as_str()), ("--", "Loading..."));
        assert_eq!((w.week.pct.as_str(), w.week.lit, w.week.needle), ("--", 0, 0));

        w.set_fixed(Some((42.0, Some(now + 2 * 3600 + 14 * 60), 17.6, Some(now + 2 * 86_400 + 6 * 3600 + 1))), now);
        assert_eq!(w.session, Gauge { lit: 4, needle: 42, pct: "42%".into(), reset: "2h 14m till reset".into() });
        assert_eq!(w.week, Gauge { lit: 1, needle: 18, pct: "18%".into(), reset: "2d 7h till reset".into() });

        // nothing used, everything used, and past it
        w.set_fixed(Some((0.0, None, 100.0, Some(now + 60))), now);
        assert_eq!(w.session, Gauge { lit: 0, needle: 0, pct: "0%".into(), reset: "Not started".into() });
        assert_eq!((w.week.lit, w.week.needle, w.week.reset.as_str()), (10, 100, "1m till reset"));
        w.set_fixed(Some((99.9, Some(now + 60), 130.0, Some(now + 60))), now);
        assert_eq!((w.session.lit, w.session.needle, w.week.lit, w.week.needle), (9, 100, 10, 100));

        // a minute on, both have reset
        w.render(now + 61);
        assert_eq!(w.session, Gauge { lit: 0, needle: 0, pct: "0%".into(), reset: "Not started".into() });
        assert_eq!((w.week.pct.as_str(), w.week.reset.as_str()), ("0%", "Resetting..."));

        // trouble, with figures and without
        w.set_fixed(Some((42.0, Some(now + 600), 18.0, Some(now + 6000))), now);
        w.state = State::Offline;
        w.render(now);
        assert_eq!((w.session.pct.as_str(), w.session.reset.as_str(), w.week.reset.as_str()), ("42%", "Offline", "Offline"));
        w.set_fixed(None, now);
        assert_eq!((w.session.pct.as_str(), w.session.reset.as_str()), ("--", "Signed out"));
    }

    /// The brightness of a colour, 0 to 765.
    fn bright(c: u32) -> u32 {
        (c >> 16 & 255) + (c >> 8 & 255) + (c & 255)
    }

    #[test]
    fn a_gauge_is_round_lit_from_the_upper_left_and_outlined_nowhere() {
        let dial = Dial::from_skin();
        let palette = Palette::from_skin(dial.segments);
        let picture = dial.picture(5, 50, &palette);
        let (d, middle, last) = (dial.d, dial.d / 2, dial.d - 1);
        let at = |x: i32, y: i32| picture[(y * d + x) as usize];
        // round: nothing in the corners, and the bezel touching every side
        assert!([at(0, 0), at(last, 0), at(0, last), at(last, last)].iter().all(|&c| c == 0));
        assert!([at(middle, 0), at(0, middle), at(middle, last), at(last, middle)].iter().all(|&c| c != 0));
        // no outline: not a px in the ink (and lighting a segment changes
        // that segment and nothing round it: see the scale's test)
        assert!(!picture.contains(&SKIN.colour("Ink")), "something is drawn in ink");
        // The bezel catches the light on its outer slope at the upper left and
        // is in shade there on its inner slope; at the lower right the other
        // way about.
        let slope = |dir: f32, out: f32| {
            let r = dial.radius() - dial.bezel / 2.0 + out * dial.bezel / 4.0;
            let (x, y) = (middle + (dir * r * 0.7071) as i32, middle + (dir * r * 0.7071) as i32);
            bright(at(x, y))
        };
        assert!(slope(-1.0, 1.0) > slope(1.0, 1.0), "the outer slope");
        assert!(slope(-1.0, -1.0) < slope(1.0, -1.0), "the inner slope");
        // the metal has a glint on it somewhere, as the Battery's terminal has
        assert!(picture.contains(&palette.metal.glint));
    }

    #[test]
    fn the_scale_lights_from_the_lower_left_round_to_the_lower_right() {
        let dial = Dial::from_skin();
        let palette = Palette::from_skin(dial.segments);
        // lighting one more segment changes that segment and nothing else
        for lit in 1..=dial.segments {
            let (before, after) = (dial.picture(lit - 1, 0, &palette), dial.picture(lit, 0, &palette));
            for (i, (a, b)) in before.iter().zip(&after).enumerate() {
                if a != b {
                    let (u, v) = dial.offset(i as i32 % dial.d, i as i32 / dial.d);
                    assert_eq!(dial.segment(u, v).map(|s| s.0), Some(lit - 1), "{lit} lit: {u},{v} changed");
                }
            }
            assert_ne!(before, after, "lighting segment {}", lit - 1);
        }
        // the first segment is on the left and low, the last on the right
        let r = (dial.scale_in + dial.scale_out) / 2.0;
        let first = (dial.start() - dial.sweep / (2 * dial.segments) as f32).to_radians();
        let (u, v) = (r * first.cos(), -r * first.sin());
        assert!(u < 0.0 && v > 0.0, "the first segment's middle is at {u},{v}");
        assert_eq!(dial.segment(u, v).map(|s| s.0), Some(0));
        assert_eq!(dial.segment(-u, v).map(|s| s.0), Some(dial.segments - 1));
        // the first is green and the last red, the Battery's colours the
        // other way about
        let (first, last) = (palette.lit[0].body, palette.lit[dial.segments as usize - 1].body);
        assert_eq!((first, last), (SKIN.colour("DialLow"), SKIN.colour("DialHigh")));
    }

    #[test]
    fn the_needle_throws_its_shadow_down_and_to_the_right() {
        let dial = Dial::from_skin();
        let palette = Palette::from_skin(dial.segments);
        // straight up at fifty: its shadow on the face to its right, none
        // to its left
        let picture = dial.picture(0, 50, &palette);
        let middle = dial.d / 2;
        let y = middle - (dial.needle / 2.0) as i32;
        let at = |x: i32| picture[(y * dial.d + x) as usize];
        let shadow = mix(palette.face, BLACK, palette.cast);
        assert!((middle + 2..middle + 5).any(|x| at(x) == shadow), "no shadow to the right");
        assert!((middle - 6..middle - 2).all(|x| at(x) != shadow), "a shadow to the left");
    }

    #[test]
    fn the_foot_of_the_face_is_clear_for_the_figure() {
        // "100%" in the skin's bold figure is about 44 px wide and its ink
        // 11 rows tall, from 14 rows under the centre: nothing of the scale
        // is there, and the needle, at either end of it, stays above
        let dial = Dial::from_skin();
        for y in 14..25 {
            for x in -22..=22 {
                let (u, v) = (x as f32, y as f32);
                assert_eq!(dial.segment(u, v), None, "the scale at {x},{y}");
                assert_eq!(dial.pointer(u, v, 0).or(dial.pointer(u, v, 100)), None, "the needle at {x},{y}");
            }
        }
    }

    #[test]
    fn asks_again_when_due_or_just_after_a_reset() {
        let mut settings = Settings::in_memory();
        let now = 1_000_000;
        let w = widget(&mut settings, now);
        let report = |session: Option<i64>, week: Option<i64>| Report {
            fetched: now,
            session: Some(Limit { pct: 1.0, resets: session }),
            week: Some(Limit { pct: 1.0, resets: week }),
            ..Report::default()
        };
        assert_eq!(w.due(&report(None, None), now), now + REFRESH);
        assert_eq!(w.due(&report(Some(now + 100), Some(now + 9999)), now), now + 100 + AFTER_RESET);
        // one that has gone by already is no reason to hurry
        assert_eq!(w.due(&report(Some(now - 5), None), now), now + REFRESH);
    }
}
