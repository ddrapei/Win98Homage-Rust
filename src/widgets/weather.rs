//! Today's weather: temperature, condition, place and the day's high and low.
//! Two lookups feed it: the public IP's location from ip-api.com (once a day,
//! or never if a place is pinned in settings.ini), then the forecast from
//! Open-Meteo every half hour.
//!
//! The place is pinned by typing a city into the widget's city field.  The
//! name goes to Open-Meteo's geocoder, and what comes back is kept in
//! settings.ini as ManualLat, ManualLon and ManualLocation -- the same three
//! keys that can be written there by hand.
//!
//! The field is typed on where it stands (`Field`, below): there is one
//! picture of it, the name centred in its box, and while it has the keyboard
//! a selection and a caret are drawn on top.  Nothing is laid over it and the
//! name does not move when the typing starts.  The text, the caret and what
//! the keys do are the note's own (`editor::Doc`, as one line); the keyboard
//! reaches it the way it reaches the note (`keys`).
//!
//!   click    the field takes the keyboard, the name in it all selected
//!   Enter    take the name as it stands
//!   click anywhere else
//!            take it, if it is not what the typing started from
//!   Escape   leave things as they were
//!
//! The network is kept out of this process altogether.  A fetch is the
//! program run again as `--fetch-weather`, which loads WinHTTP, writes a few
//! lines to a cache file and exits -- so the DLLs, sockets and TLS state a
//! request drags in live for a second every half hour instead of for good.
//! Finding a city is the same again as `--find-city`, which goes on to fetch
//! that city's forecast.  The cache also means a restart shows the last
//! forecast at once.

use core::ffi::c_void;
use core::ptr::{null, null_mut};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use crate::canvas::{Canvas, Rect};
use crate::editor::{self, Doc};
use crate::keys::Key;
use crate::settings::{data_dir, write_whole};
use crate::skin::{self, Hooks, Meter, Skin, ICONS};
use crate::sys;
use crate::text::{Spec, Text, CENTER};
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Pressed, Request, Widget};

static SKIN: &Skin = &skin::weather::SKIN;
const SECTION: &str = "Weather";

/// Seconds between forecasts, between looks at where the machine is, and
/// between tries while there is nothing at all to show.
const REFRESH: i64 = 1800;
const RELOCATE: i64 = 86_400;
const RETRY: i64 = 120;
/// A cached forecast older than this is not worth showing on start.
const STALE: i64 = 6 * 3600;
/// A fetch still running after this long is given up on.
const FETCH_LIMIT: i64 = 60;
/// Ticks the city field spends saying what became of a name typed into it.
const NOTICE: u32 = 5;

/// How `--find-city` ends when it has no place to report: the name matched
/// nothing, or the geocoder could not be asked.  Either way the cache file is
/// left alone, so the forecast on screen is still there after a restart.
const EXIT_NO_CITY: i32 = 3;
const EXIT_NO_GEOCODER: i32 = 4;

/// Characters the city field takes: no place name is longer.
const LIMIT: usize = 64;
/// Px of sunken edge round the field that are still the field to a click.
const BEVEL: i32 = 2;
/// The height of the name's line, as the skin's other one-line meters have it.
const LINE_H: i32 = 14;

/// WMO weather codes, as Open-Meteo uses them.
fn condition(code: Option<i32>) -> &'static str {
    match code {
        Some(0) => "Clear",
        Some(1) => "Mainly Clear",
        Some(2) => "Partly Cloudy",
        Some(3) => "Overcast",
        Some(45 | 48) => "Fog",
        Some(51) => "Light Drizzle",
        Some(53) => "Drizzle",
        Some(55) => "Dense Drizzle",
        Some(56 | 57) => "Freezing Drizzle",
        Some(61) => "Light Rain",
        Some(63) => "Rain",
        Some(65) => "Heavy Rain",
        Some(66 | 67) => "Freezing Rain",
        Some(71) => "Light Snow",
        Some(73) => "Snow",
        Some(75) => "Heavy Snow",
        Some(77) => "Snow Grains",
        Some(80 | 81) => "Rain Showers",
        Some(82) => "Violent Showers",
        Some(85 | 86) => "Snow Showers",
        Some(95) => "Thunderstorm",
        Some(96 | 99) => "Thunderstorm, Hail",
        _ => "Unknown",
    }
}

/// The file in Icons, without its extension.  Clear and partly cloudy have a
/// night face; fog is drawn as overcast; drizzle and the lightest rain get
/// the scattered showers, and a storm with hail gets the alert.
fn icon_for(code: i32, day: bool) -> &'static str {
    match code {
        0 => if day { "weather-clear" } else { "weather-clear-night" },
        1 | 2 => if day { "weather-few-clouds" } else { "weather-few-clouds-night" },
        51..=57 | 61 | 80 => "weather-showers-scattered",
        63..=67 | 81 | 82 => "weather-showers",
        71..=77 | 85 | 86 => "weather-snow",
        96 | 99 => "weather-severe-alert",
        95.. => "weather-storm",
        _ => "weather-overcast",
    }
}

fn round(n: f64) -> i64 {
    (n + 0.5).floor() as i64
}

/// What a fetch leaves behind: `key=value` lines in the cache file.
#[derive(Default, Clone)]
struct Report {
    fetched: i64,
    unit: String,
    lat: String,
    lon: String,
    city: String,
    /// When `lat`/`lon` were last looked up from the IP address.
    located: i64,
    temp: Option<f64>,
    code: Option<i32>,
    is_day: bool,
    tmax: Option<f64>,
    tmin: Option<f64>,
    /// "geo" or "forecast" when that request failed.
    error: String,
}

impl Report {
    fn parse(text: &str) -> Report {
        let mut r = Report { is_day: true, ..Report::default() };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "fetched" => r.fetched = v.parse().unwrap_or(0),
                "unit" => r.unit = v.to_owned(),
                "lat" => r.lat = v.to_owned(),
                "lon" => r.lon = v.to_owned(),
                "city" => r.city = v.to_owned(),
                "located" => r.located = v.parse().unwrap_or(0),
                "temp" => r.temp = v.parse().ok(),
                "code" => r.code = v.parse::<f64>().ok().map(|c| c as i32),
                "is_day" => r.is_day = v.trim() != "0",
                "tmax" => r.tmax = v.parse().ok(),
                "tmin" => r.tmin = v.parse().ok(),
                "error" => r.error = v.to_owned(),
                _ => {}
            }
        }
        r
    }

    fn write(&self, path: &Path) {
        let num = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
        let text = format!(
            "fetched={}\nunit={}\nlat={}\nlon={}\ncity={}\nlocated={}\ntemp={}\ncode={}\nis_day={}\ntmax={}\ntmin={}\nerror={}\n",
            self.fetched, self.unit, self.lat, self.lon, self.city, self.located,
            num(self.temp), self.code.map(|c| c.to_string()).unwrap_or_default(),
            self.is_day as i32, num(self.tmax), num(self.tmin), self.error
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = write_whole(path, text.as_bytes());
    }
}

// ---------------------------------------------------------------------------
//  The city field
// ---------------------------------------------------------------------------

/// A fractional x as the pixel column it falls on.
fn px(x: f32) -> i32 {
    (x + 0.5).floor() as i32
}

/// Px from the start of `s` to each place a caret can stand in it: one more
/// entry than it has characters.  They are the font's own advances, so a
/// caret, a selection and a centred name all land where the glyphs do.
fn stops(text: Option<&mut Text>, font: usize, s: &str) -> Vec<f32> {
    let mut each = Vec::new();
    match text {
        Some(text) => text.advances(font, s, &mut each),
        // nothing to measure with: the width of a digit, as good a guess as any
        None => each.extend(s.chars().map(|_| 6.0)),
    }
    let mut at = 0.0;
    let mut out = Vec::with_capacity(each.len() + 1);
    out.push(at);
    for w in each {
        at += w;
        out.push(at);
    }
    out
}

/// The city field: one line of text in a sunken box, typed on where it is.
///
/// It knows nothing of weather.  It draws whatever name it is handed while
/// nobody is typing, and its own text while somebody is -- by the same code,
/// from the same x, so the one turns into the other without a pixel moving.
struct Field {
    /// The inside of the box, in the window: what is drawn on.
    rect: Rect,
    /// Px between its sides and the column the name is centred in.
    inset: i32,
    /// Px from its top to the top of the name's line.
    down: i32,
    ink: u32,
    select_bg: u32,
    select_ink: u32,
    /// The font the name is drawn in: as the host's `Text` numbers it, and
    /// its size in points.
    font: usize,
    points: f32,

    /// The field has the keyboard.
    editing: bool,
    doc: Doc,
    /// What the typing started from.
    before: String,
    /// Enter was pressed: the name is wanted as it stands.
    entered: bool,
    /// Escape was: it is not wanted at all.
    cancelled: bool,
    /// The press now held put the caret down, so dragging it selects.
    sweeping: bool,
    caret_on: bool,
    /// The same font, to measure with, for as long as the typing lasts.
    measure: Option<Text>,
    /// `stops` of the text, and the revision of it they were measured from.
    stops: Vec<f32>,
    laid: u64,
    /// The text fits the column, and is centred in it; otherwise it runs
    /// from the left and scrolls to keep the caret in sight.
    fits: bool,
    /// Px from the left of the column to the start of the text, either way.
    /// Centred, that is wherever the middle puts it, between pixels or not.
    left: f32,
}

impl Field {
    fn new() -> Field {
        let points = SKIN.num("FontSize", 8.0) as f32;
        Field {
            // [MeterCity] is the inside of the box; its bounds are its place
            rect: SKIN.meter("MeterCity").map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3])),
            inset: SKIN.int("CityInset", 2),
            down: SKIN.int("CityDrop", 3) + SKIN.int("TextDY", 0),
            ink: SKIN.colour("Ink"),
            select_bg: SKIN.colour("SelectBg"),
            select_ink: SKIN.colour("SelectInk"),
            font: skin::FONTS.iter().position(|&(pt, bold)| pt == points && !bold).unwrap_or(0),
            points,
            editing: false,
            doc: Doc::one_line(),
            before: String::new(),
            entered: false,
            cancelled: false,
            sweeping: false,
            caret_on: false,
            measure: None,
            stops: Vec::new(),
            laid: 0,
            fits: true,
            left: 0.0,
        }
    }

    /// The column the name is centred in: its left edge in the window, and
    /// its width.
    fn column(&self) -> (i32, i32) {
        (self.rect.x0 + self.inset, (self.rect.x1 - self.rect.x0 - 2 * self.inset).max(1))
    }

    /// The box with its sunken edge: what a click has to land in.
    fn reach(&self) -> Rect {
        let r = &self.rect;
        if r.is_empty() { *r } else { Rect::new(r.x0 - BEVEL, r.y0 - BEVEL, r.x1 + BEVEL, r.y1 + BEVEL) }
    }

    fn text(&self) -> String {
        self.doc.text().iter().collect()
    }

    /// Start typing, from `name` with all of it selected, so that typing
    /// replaces it.  The answer asks the host for the keyboard.
    fn begin(&mut self, name: &str) -> Outcome {
        self.doc.load(name.chars().filter(|c| !c.is_control()).take(LIMIT).collect());
        self.doc.select_all();
        self.before = self.text().trim().to_owned();
        self.editing = true;
        self.entered = false;
        self.cancelled = false;
        self.sweeping = false;
        if self.measure.is_none() {
            self.measure = Text::new(SKIN.var("Font"), &[(self.points, false)]);
        }
        self.stops.clear();
        self.left = 0.0;
        Outcome { request: Request::TakeKeys, ..self.edited() }
    }

    /// The typing is over.  What was typed, if it is an answer: Enter takes
    /// the name as it stands, a click elsewhere takes it only if it is not
    /// what the typing started from, and Escape never does.
    fn end(&mut self) -> Option<String> {
        self.editing = false;
        self.measure = None;
        self.stops = Vec::new();
        self.doc.deselect();
        let typed = self.text().trim().to_owned();
        (!self.cancelled && (self.entered || typed != self.before)).then_some(typed)
    }

    /// The characters there is still room for, what is selected counting as
    /// already gone.
    fn free(&self) -> usize {
        let (a, b) = self.doc.range();
        LIMIT.saturating_sub(self.doc.text().len() - (b - a))
    }

    /// After anything the keyboard or the pointer did: measure the text
    /// again if it changed, find where it starts, and draw.
    fn edited(&mut self) -> Outcome {
        if self.doc.rev() != self.laid || self.stops.is_empty() {
            let text = self.text();
            self.stops = stops(self.measure.as_mut(), 0, &text);
            self.laid = self.doc.rev();
        }
        self.caret_on = true;

        let (_, room) = self.column();
        let chars = self.doc.text();
        // blanks at the end hang past the name: they do not pull it off centre
        let inked = chars.len() - chars.iter().rev().take_while(|&&c| editor::blank(c)).count();
        let width = self.stops[inked];
        self.fits = width <= room as f32;
        if self.fits {
            // where the layout's own centring puts it: see `draw`
            self.left = (room as f32 - width) / 2.0;
        } else {
            // Too long to show whole: it runs from the left edge and scrolls,
            // no further than leaves a pixel for the caret after its end, and
            // always far enough to show the caret.
            let caret = px(self.stops[self.doc.caret()]);
            let full = px(self.stops[chars.len()]);
            let scroll = (-self.left as i32).clamp(0, (full + 1 - room).max(0)).clamp(caret - (room - 1), caret);
            self.left = -scroll as f32;
        }
        Outcome::dirty(Dirty::Part(self.rect))
    }

    /// The pixel column of the window that caret position `i` falls on.
    fn column_of(&self, i: usize) -> i32 {
        self.column().0 + px(self.left + self.stops.get(i).copied().unwrap_or(0.0))
    }

    /// The place in the text nearest an x of the window.
    fn place_at(&self, x: i32) -> usize {
        let at = (x - self.column().0) as f32 - self.left;
        let mut best = 0;
        for (i, stop) in self.stops.iter().enumerate() {
            if (stop - at).abs() < (self.stops[best] - at).abs() {
                best = i;
            }
        }
        best
    }

    /// Where the caret is drawn, in the window.  After blanks that run past
    /// the column it waits at the column's edge.
    fn caret_box(&self) -> Rect {
        let (x, room) = self.column();
        let cx = self.column_of(self.doc.caret()).clamp(x, x + room - 1);
        Rect::new(cx, self.rect.y0 + 1, cx + 1, self.rect.y1 - 1)
    }

    // ---- the pointer ------------------------------------------------------------

    /// A press in the field while it is being typed in: the caret goes there,
    /// or with Shift the selection reaches to it; a double-click takes a word.
    fn press(&mut self, x: i32, double: bool, shift: bool) -> Outcome {
        let pos = self.place_at(x);
        if double {
            self.doc.select_word(pos);
        } else {
            self.doc.click(pos, false, shift);
        }
        self.sweeping = !double;
        self.edited()
    }

    /// The selection follows the pointer, and the name scrolls under it.
    fn drag(&mut self, x: i32) -> Dirty {
        if !self.editing || !self.sweeping {
            return Dirty::None;
        }
        self.doc.drag(self.place_at(x), false);
        self.edited().dirty
    }

    // ---- the keyboard -----------------------------------------------------------

    fn typed(&mut self, c: char) -> Outcome {
        if !self.editing || c.is_control() || self.free() == 0 {
            return Outcome::none();
        }
        self.doc.type_char(c);
        self.edited()
    }

    fn key(&mut self, key: Key, shift: bool, ctrl: bool) -> Outcome {
        if !self.editing {
            return Outcome::none();
        }
        match key {
            // one line: up and down are along it
            Key::Left | Key::Up => self.doc.left(ctrl, shift),
            Key::Right | Key::Down => self.doc.right(ctrl, shift),
            Key::Home | Key::PageUp => self.doc.home(&[], true, shift),
            Key::End | Key::PageDown => self.doc.end(&[], true, shift),
            Key::Backspace => self.doc.backspace(ctrl),
            Key::Delete => self.doc.delete(ctrl),
            Key::SelectAll => self.doc.select_all(),
            Key::Undo => {
                self.doc.undo();
            }
            Key::Redo => {
                self.doc.redo();
            }
            Key::Enter => {
                self.entered = true;
                return Outcome::request(Request::DropKeys);
            }
            Key::Escape => {
                self.cancelled = true;
                return Outcome::request(Request::DropKeys);
            }
            // a tab has no place in a name; the clipboard's three are the host's
            Key::Tab | Key::Copy | Key::Cut | Key::Paste => return Outcome::none(),
        }
        self.edited()
    }

    fn copy(&self) -> Option<String> {
        self.doc.selection().filter(|_| self.editing)
    }

    fn cut(&mut self) -> (Option<String>, Outcome) {
        let Some(taken) = self.copy() else { return (None, Outcome::none()) };
        self.doc.backspace(false);
        (Some(taken), self.edited())
    }

    /// A name is one line: of what is pasted, the first that has anything on
    /// it, and as much of that as there is room for.
    fn paste(&mut self, text: &str) -> Outcome {
        if !self.editing {
            return Outcome::none();
        }
        let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let name: String = line.chars().filter(|c| !c.is_control()).take(self.free()).collect();
        if name.is_empty() {
            return Outcome::none();
        }
        self.doc.insert(&name);
        self.edited()
    }

    /// The keyboard's timer: the caret blinks, if it is one that does.
    fn pulse(&mut self, blink: bool) -> Dirty {
        let on = !blink || !self.caret_on;
        if !self.editing || on == self.caret_on {
            return Dirty::None;
        }
        self.caret_on = on;
        Dirty::Part(self.caret_box())
    }

    // ---- drawing ----------------------------------------------------------------

    /// The name in the box: `resting` while nobody is typing, the text being
    /// typed while somebody is -- with whatever of it is selected drawn again
    /// light on dark, and the caret last.
    fn draw(&self, resting: &str, canvas: &mut Canvas, text: &mut Text) {
        let outer = canvas.clip;
        let clip = outer.intersect(&self.rect);
        if clip.is_empty() {
            return;
        }
        if !self.editing {
            self.run(canvas, text, resting, true, self.ink);
            return;
        }
        if self.stops.len() != self.doc.text().len() + 1 {
            return;
        }

        canvas.clip = clip;
        let string = self.text();
        self.run(canvas, text, &string, self.fits, self.ink);

        let (a, b) = self.doc.range();
        let band = Rect::new(self.column_of(a), self.rect.y0 + 1, self.column_of(b), self.rect.y1 - 1).intersect(&clip);
        if !band.is_empty() {
            canvas.fill(band.x0, band.y0, band.x1 - band.x0, band.y1 - band.y0, self.select_bg);
            canvas.clip = band;
            self.run(canvas, text, &string, self.fits, self.select_ink);
            canvas.clip = clip;
        }
        if self.caret_on {
            let c = self.caret_box();
            // at the head of a selection it stands on the dark
            let colour = if a != b && self.doc.caret() == a { self.select_ink } else { self.ink };
            canvas.fill(c.x0, c.y0, 1, c.y1 - c.y0, colour);
        }
        canvas.clip = outer;
    }

    /// The name's glyphs, in one colour.
    ///
    /// `centred` is a name that fits, or one nobody is typing: it is laid out
    /// centred in the column, exactly as a String meter with StringAlign=Center
    /// and ClipString=1 would be -- so a name too long to show whole ends in
    /// an ellipsis, and a name looks the same at rest and with a caret in it.
    /// Otherwise it is being typed and is too long: it runs from `left`.
    fn run(&self, canvas: &mut Canvas, text: &mut Text, string: &str, centred: bool, colour: u32) {
        if string.is_empty() {
            return;
        }
        let (x, room) = self.column();
        let spec = Spec { text: string, font: self.font, colour, x, y: self.rect.y0 + self.down, align: 0, w: room, h: LINE_H, clip: false, solid: None, pad: [0; 4] };
        text.draw(canvas, &if centred {
            Spec { x: x + room / 2, align: CENTER, clip: true, ..spec }
        } else {
            // wide enough never to matter: the canvas's clip is what ends it
            Spec { x: x + self.left as i32, w: 4000, ..spec }
        });
    }
}

pub struct Weather {
    field: Field,
    unit: String,
    degrees: &'static str,
    manual: Option<(String, String)>,
    manual_label: String,

    temp_now: String,
    condition: String,
    high_low: String,
    location: String,
    icon: Option<usize>,

    cache: PathBuf,
    /// The last report: where the machine is, and whether there is a forecast.
    last: Report,
    showing: bool,
    child: Option<(Child, i64)>,
    /// The child is looking a typed city up: what the city field says
    /// meanwhile.
    finding: Option<String>,
    /// What became of the last name typed, and the ticks it has left to show.
    notice: Option<(&'static str, u32)>,
    next_fetch: i64,
    /// For the test harness: never fetch.
    frozen: bool,
}

impl Weather {
    pub fn new(ctx: &Ctx) -> Weather {
        let s = &*ctx.settings;
        let get = |key: &str| s.text(SECTION, key, SKIN.var(key)).trim().to_owned();
        let unit = if get("TempUnit").eq_ignore_ascii_case("fahrenheit") { "fahrenheit" } else { "celsius" };
        let (lat, lon) = (get("ManualLat"), get("ManualLon"));
        let mut weather = Weather {
            field: Field::new(),
            unit: unit.to_owned(),
            degrees: if unit == "fahrenheit" { "\u{b0}F" } else { "\u{b0}C" },
            // pinned to a place by hand: never ask ip-api.com anything
            manual: (!lat.is_empty() && !lon.is_empty()).then_some((lat, lon)),
            manual_label: get("ManualLocation"),
            temp_now: SKIN.var("TempNow").to_owned(),
            condition: SKIN.var("Condition").to_owned(),
            high_low: SKIN.var("HighLowText").to_owned(),
            location: String::new(),
            icon: None,
            cache: data_dir().join("weather.txt"),
            last: Report::default(),
            showing: false,
            child: None,
            finding: None,
            notice: None,
            next_fetch: 0,
            frozen: false,
        };
        // A forecast from before the restart is better than "Loading...",
        // as long as it is for the same place and unit and not too old.
        if let Ok(text) = std::fs::read_to_string(&weather.cache) {
            let report = Report::parse(&text);
            let same_place = match &weather.manual {
                Some((lat, lon)) => report.lat == *lat && report.lon == *lon,
                None => true,
            };
            if report.unit == weather.unit && same_place {
                if report.temp.is_some() && ctx.now - report.fetched < STALE {
                    weather.next_fetch = report.fetched + REFRESH;
                    weather.show(&report);
                }
                weather.last = report;
            }
        }
        weather
    }

    /// For the test harness: show exactly this.
    pub fn set_fixed(&mut self, temp: f64, code: i32, is_day: bool, tmax: f64, tmin: f64, city: &str) {
        self.frozen = true;
        let report = Report {
            temp: Some(temp), code: Some(code), is_day, tmax: Some(tmax), tmin: Some(tmin),
            city: city.to_owned(), ..Report::default()
        };
        self.show(&report);
    }

    fn show(&mut self, r: &Report) {
        let Some(temp) = r.temp else { return };
        self.showing = true;
        self.temp_now = format!("{}{}", round(temp), self.degrees);
        self.condition = condition(r.code).to_owned();
        if let (Some(max), Some(min)) = (r.tmax, r.tmin) {
            self.high_low = format!("High {}{d}   Low {}{d}", round(max), round(min), d = self.degrees);
        }
        let name = icon_for(r.code.unwrap_or(3), r.is_day);
        self.icon = ICONS.iter().position(|i| i.0 == name);

        let label = if !self.manual_label.is_empty() {
            self.manual_label.clone()
        } else if !r.city.is_empty() {
            r.city.clone()
        } else if !r.lat.is_empty() {
            format!("{}, {}", r.lat, r.lon)
        } else {
            String::new()
        };
        if !label.is_empty() {
            self.location = label;
        }
    }

    /// With nothing to show, say which half failed.
    fn show_error(&mut self, error: &str) {
        if self.showing {
            return;
        }
        self.temp_now = String::from("--");
        if error == "geo" {
            self.condition = String::from("No location");
            self.high_low = String::from("Type a city below");
            self.location = String::from("ip-api.com did not answer");
        } else {
            self.condition = String::from("No forecast");
            self.high_low = String::from("Open-Meteo unreachable");
        }
    }

    /// Back to "Loading...": what is on screen is for another place.
    fn blank(&mut self) {
        self.showing = false;
        self.temp_now = SKIN.var("TempNow").to_owned();
        self.condition = SKIN.var("Condition").to_owned();
        self.high_low = SKIN.var("HighLowText").to_owned();
        self.location = self.manual_label.clone();
        self.icon = None;
    }

    /// This program again, as the child that talks to the network.
    fn launch(&mut self, mode: &str, extra: &[String], now: i64) -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        let mut command = Command::new(exe);
        command.arg(mode).arg(&self.cache).arg(&self.unit).args(extra);
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        match command.creation_flags(CREATE_NO_WINDOW).spawn() {
            Ok(child) => {
                self.child = Some((child, now));
                true
            }
            Err(_) => false,
        }
    }

    fn start_fetch(&mut self, now: i64) {
        let last = &self.last;
        let place = match &self.manual {
            Some((lat, lon)) => vec![lat.clone(), lon.clone(), String::new(), String::from("0")],
            // where the machine was last found, while that is still fresh
            None if !last.lat.is_empty() && now - last.located < RELOCATE => {
                vec![last.lat.clone(), last.lon.clone(), last.city.clone(), last.located.to_string()]
            }
            None => Vec::new(),
        };
        self.launch("--fetch-weather", &place, now);
    }

    fn stop_child(&mut self) {
        if let Some((mut child, _)) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// The pinned place by name, or nothing while the place is the one the
    /// IP address gives.
    fn city_text(&self) -> &str {
        if self.manual.is_some() { &self.manual_label } else { "" }
    }

    /// What the city field says while nobody is typing in it: what became of
    /// the name just typed, while that is news; the lookup, while it runs;
    /// otherwise the place.
    fn label(&self) -> &str {
        match (&self.notice, &self.finding) {
            (Some((text, _)), _) => text,
            (None, Some(finding)) => finding,
            (None, None) => &self.location,
        }
    }

    /// The place by the name the typing starts from: the one on show, so
    /// that nothing in the field changes as it is clicked.  With no forecast
    /// the field may be saying what went wrong instead, and that is not a
    /// name: then it is the pinned place, or nothing.
    fn place(&self) -> &str {
        if self.showing { &self.location } else { self.city_text() }
    }

    /// Give the city field the keyboard.
    fn type_city(&mut self) -> Outcome {
        if self.field.editing {
            return Outcome::none();
        }
        // whatever the field was saying about the last name, it has been read
        self.notice = None;
        let place = self.place().to_owned();
        self.field.begin(&place)
    }

    /// A name typed into the city field.  It is looked up before anything
    /// changes, so a name that is not a place leaves the forecast as it was;
    /// an empty one goes back to the place the IP address gives.
    fn set_city(&mut self, typed: &str, ctx: &mut Ctx) -> Dirty {
        let typed = typed.trim();
        if typed.is_empty() {
            return self.set_automatic(ctx);
        }
        if self.frozen {
            return Dirty::None;
        }
        self.stop_child();
        if self.launch("--find-city", &[typed.to_owned()], ctx.now) {
            self.finding = Some(format!("Finding {typed}..."));
            self.notice = None;
        } else {
            self.finding = None;
            self.notice = Some(("Lookup failed", NOTICE));
        }
        Dirty::All
    }

    /// Unpin the place: forget the three settings and ask ip-api.com again.
    fn set_automatic(&mut self, ctx: &mut Ctx) -> Dirty {
        if self.manual.is_none() && self.finding.is_none() {
            return Dirty::None;
        }
        self.stop_child();
        self.finding = None;
        self.notice = None;
        if self.manual.take().is_some() {
            for key in ["ManualLat", "ManualLon", "ManualLocation"] {
                ctx.settings.set(SECTION, key, "");
            }
            self.manual_label.clear();
            self.last = Report::default();
            self.blank();
        }
        self.next_fetch = 0;
        Dirty::All
    }

    /// The child has exited; `code` is how.
    fn finish_fetch(&mut self, code: Option<i32>, ctx: &mut Ctx) -> Dirty {
        let now = ctx.now;
        let finding = self.finding.take().is_some();
        if finding && code == Some(EXIT_NO_CITY) {
            self.notice = Some(("No such city", NOTICE));
            return Dirty::All;
        }
        let report = std::fs::read_to_string(&self.cache).map(|t| Report::parse(&t)).unwrap_or_default();
        let recent = report.fetched >= now - FETCH_LIMIT - 5;
        if finding {
            // 0 and 1 both found the place; 1 is its forecast failing, which
            // the ordinary retry below takes care of.
            if !matches!(code, Some(0 | 1)) || !recent || report.lat.is_empty() {
                self.notice = Some(("Lookup failed", NOTICE));
                return Dirty::All;
            }
            for (key, value) in [("ManualLat", &report.lat), ("ManualLon", &report.lon), ("ManualLocation", &report.city)] {
                ctx.settings.set(SECTION, key, value);
            }
            self.manual = Some((report.lat.clone(), report.lon.clone()));
            self.manual_label = report.city.clone();
            self.blank();
        }
        let fresh = recent && report.temp.is_some() && report.error.is_empty();
        if fresh {
            self.show(&report);
            self.last = report;
            self.next_fetch = now + REFRESH;
        } else {
            // Nothing new: keep what is on screen, and try again sooner
            // while there is nothing on it at all.
            let error = if report.error.is_empty() { "forecast" } else { report.error.as_str() };
            self.show_error(error);
            if !report.lat.is_empty() {
                self.last.lat = report.lat;
                self.last.lon = report.lon;
                self.last.city = report.city;
                self.last.located = report.located;
            }
            self.next_fetch = now + if self.showing { REFRESH } else { RETRY };
        }
        Dirty::All
    }
}

impl Hooks for Weather {
    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "TempNow" => &self.temp_now,
            "Condition" => &self.condition,
            "HighLowText" => &self.high_low,
            _ => "",
        });
    }

    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, text: &mut Text) {
        match meter.name {
            // the condition glyph, one of the PNGs from Icons
            "MeterIconWeather" => {
                if let Some(&(_, w, h, bgra)) = self.icon.and_then(|i| ICONS.get(i)) {
                    canvas.image(x, y, w, h, bgra);
                }
            }
            // the city, in its field: typed on or not, it is drawn here
            "MeterCity" => self.field.draw(self.label(), canvas, text),
            _ => {}
        }
    }
}

impl Widget for Weather {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Weather\n\n\
         The current temperature and condition, and today's high and low, \
         from Open-Meteo, refreshed every half hour.\n\n\
         The place is where your public IP address appears to be (asked of \
         ip-api.com once a day).  To pin it instead -- or to keep off \
         ip-api.com altogether -- click the city field and type a city: \
         \"Lviv\", or \"Paris, US\" or \"Portland, Maine\" to say which one.  \
         Enter takes it, Escape does not, and an empty field goes back to \
         the automatic place."
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        if self.frozen {
            return Dirty::None;
        }
        let now = ctx.now;
        let mut dirty = Dirty::None;
        if let Some((text, left)) = self.notice {
            self.notice = (left > 1).then_some((text, left - 1));
            if self.notice.is_none() {
                dirty = Dirty::All;
            }
        }
        if let Some((child, started)) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    return self.finish_fetch(status.code(), ctx);
                }
                Ok(None) if now - *started <= FETCH_LIMIT => {}
                _ => {
                    self.stop_child();
                    if self.finding.take().is_some() {
                        // the forecast's own schedule stands
                        self.notice = Some(("Lookup failed", NOTICE));
                        dirty = Dirty::All;
                    } else {
                        self.next_fetch = now + RETRY;
                    }
                }
            }
        } else if now >= self.next_fetch {
            self.next_fetch = now + RETRY; // if it cannot even be started
            self.start_fetch(now);
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

    /// A press in the city field starts the typing, with the name that is
    /// there all selected; once it has the keyboard a press puts the caret
    /// down, and held, it selects.  Anywhere else is the host's.
    fn press(&mut self, x: i32, y: i32, double: bool, shift: bool) -> Pressed {
        if !self.field.reach().contains(x, y) {
            return Pressed::No;
        }
        Pressed::Taken(if self.field.editing { self.field.press(x, double, shift) } else { self.type_city() })
    }

    fn drag(&mut self, x: i32, _y: i32) -> Dirty {
        self.field.drag(x)
    }

    fn ibeam(&self, x: i32, y: i32) -> bool {
        self.field.rect.contains(x, y)
    }

    // ---- the keyboard -----------------------------------------------------------

    fn typing_at(&self, x: i32, y: i32) -> bool {
        self.field.reach().contains(x, y)
    }

    fn typed(&mut self, c: char) -> Outcome {
        self.field.typed(c)
    }

    fn key(&mut self, key: Key, shift: bool, ctrl: bool) -> Outcome {
        self.field.key(key, shift, ctrl)
    }

    fn copy(&self) -> Option<String> {
        self.field.copy()
    }

    fn cut(&mut self) -> (Option<String>, Outcome) {
        self.field.cut()
    }

    fn paste(&mut self, text: &str) -> Outcome {
        self.field.paste(text)
    }

    fn pulse(&mut self, _ms: u32, blink: bool) -> Dirty {
        self.field.pulse(blink)
    }

    /// The keyboard has gone elsewhere: the field goes back to naming the
    /// place, and what was typed in it -- if that is an answer, and not the
    /// name the place is pinned by already -- is looked up.
    fn blur(&mut self, ctx: &mut Ctx) -> Dirty {
        if !self.field.editing {
            return Dirty::None;
        }
        let field = Dirty::Part(self.field.rect);
        match self.field.end() {
            Some(typed) if typed.is_empty() || typed != self.city_text() => field.and(self.set_city(&typed, ctx)),
            _ => field,
        }
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { id: 1, label: "Refresh now", checked: false });
        items.push(MenuItem { id: 2, label: "Fahrenheit", checked: self.unit == "fahrenheit" });
        items.push(MenuItem { id: 3, label: "Set city...", checked: false });
        items.push(MenuItem { id: 4, label: "Automatic location", checked: self.manual.is_none() });
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            1 => self.next_fetch = 0,
            2 => {
                let fahrenheit = self.unit != "fahrenheit";
                self.unit = String::from(if fahrenheit { "fahrenheit" } else { "celsius" });
                self.degrees = if fahrenheit { "\u{b0}F" } else { "\u{b0}C" };
                ctx.settings.set(SECTION, "TempUnit", &self.unit);
                self.next_fetch = 0;
            }
            3 => return self.type_city(),
            4 => return Outcome::dirty(self.set_automatic(ctx)),
            _ => {}
        }
        Outcome::none()
    }

    fn shutdown(&mut self, _ctx: &mut Ctx) {
        self.stop_child();
    }
}

// ---------------------------------------------------------------------------
//  The fetch: this program run as `--fetch-weather <cache> <unit> [lat lon city located]`
//  or as `--find-city <cache> <unit> <name>`
// ---------------------------------------------------------------------------

/// The text after `key` in `s`.
fn after<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    s.find(key).map(|i| &s[i + key.len()..])
}

/// The text up to the first of `ends`.
fn until<'a>(s: &'a str, ends: &[char]) -> &'a str {
    s[..s.find(ends).unwrap_or(s.len())].trim()
}

/// `"city":"...","lat":..,"lon":..` out of ip-api.com's answer.
fn parse_place(json: &str) -> Option<(String, String, String)> {
    let city = after(json, "\"city\":\"").map(|s| until(s, &['"']).to_owned()).unwrap_or_default();
    let lat = until(after(json, "\"lat\":")?, &[',', '}']).to_owned();
    let lon = until(after(json, "\"lon\":")?, &[',', '}']).to_owned();
    (lat.parse::<f64>().is_ok() && lon.parse::<f64>().is_ok()).then_some((city, lat, lon))
}

/// The current_weather block, then today's entry of the two daily arrays.
/// The colon after the key matters: "current_weather_units" and "daily_units"
/// carry the same names and come first.
fn parse_forecast(json: &str, report: &mut Report) -> bool {
    let Some(current) = after(json, "\"current_weather\":") else { return false };
    let field = |block: &str, key: &str| after(block, key).map(|s| until(s, &[',', '}', ']']).to_owned());
    report.temp = field(current, "\"temperature\":").and_then(|v| v.parse().ok());
    report.is_day = field(current, "\"is_day\":").is_none_or(|v| v != "0");
    report.code = field(current, "\"weathercode\":").and_then(|v| v.parse::<f64>().ok()).map(|c| c as i32);
    if let Some(daily) = after(json, "\"daily\":") {
        report.tmax = field(daily, "\"temperature_2m_max\":[").and_then(|v| v.parse().ok());
        report.tmin = field(daily, "\"temperature_2m_min\":[").and_then(|v| v.parse().ok());
    }
    report.temp.is_some()
}

pub fn fetch_main(args: &[String]) -> i32 {
    let (Some(cache), Some(unit)) = (args.first(), args.get(1)) else { return 2 };
    let now = crate::clock::now();
    let mut report = Report { fetched: now, unit: unit.clone(), is_day: true, ..Report::default() };

    match (args.get(2), args.get(3)) {
        (Some(lat), Some(lon)) if !lat.is_empty() && !lon.is_empty() => {
            report.lat = lat.clone();
            report.lon = lon.clone();
            report.city = args.get(4).cloned().unwrap_or_default();
            report.located = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(0);
        }
        // http, not https: ip-api.com serves TLS to paying keys only
        _ => match http_get("ip-api.com", 80, false, "/json/").as_deref().and_then(parse_place) {
            Some((city, lat, lon)) => {
                (report.city, report.lat, report.lon) = (city, lat, lon);
                report.located = now;
            }
            None => {
                report.error = String::from("geo");
                report.write(Path::new(cache));
                return 1;
            }
        },
    }

    forecast(report, Path::new(cache))
}

/// The second half of either child: the forecast for the place in `report`,
/// written to the cache whether or not it came.
fn forecast(mut report: Report, cache: &Path) -> i32 {
    let path = format!(
        "/v1/forecast?latitude={}&longitude={}&current_weather=true&daily=weathercode,temperature_2m_max,temperature_2m_min&temperature_unit={}&timezone=auto",
        report.lat, report.lon, report.unit
    );
    let ok = http_get("api.open-meteo.com", 443, true, &path).is_some_and(|json| parse_forecast(&json, &mut report));
    if !ok {
        report.error = String::from("forecast");
    }
    report.write(cache);
    if ok { 0 } else { 1 }
}

/// What was typed, as the name to search for and what was said about where:
/// a two-letter country code ("Paris, US"), or a region or country in words
/// ("Portland, Maine").  Only the code can be asked of the geocoder; the
/// words are for choosing among what it answers.
fn split_query(typed: &str) -> (&str, Option<String>, &str) {
    let Some((name, place)) = typed.split_once(',') else { return (typed.trim(), None, "") };
    let (name, place) = (name.trim(), place.trim());
    if place.len() == 2 && place.bytes().all(|b| b.is_ascii_alphabetic()) {
        let code = place.to_ascii_uppercase();
        // the one country everybody writes by something other than its code
        return (name, Some(if code == "UK" { String::from("GB") } else { code }), "");
    }
    (name, None, place)
}

/// Everything but the unreserved characters as %XX, for a query string.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A place out of the geocoder's answer, as (label, lat, lon): the first of
/// its results, or -- when `region` names a state or a country -- the first
/// from there, if there is one.  The results are flat objects, most likely
/// first, so they come apart at "},{".
fn parse_city(json: &str, region: &str) -> Option<(String, String, String)> {
    let text = |item: &str, key: &str| after(item, key).map(|s| until(s, &['"']).to_owned());
    let mut first = None;
    for item in after(json, "\"results\":[")?.split("},{") {
        let number = |key: &str| after(item, key).map(|s| until(s, &[',', '}']).to_owned()).filter(|v| v.parse::<f64>().is_ok());
        let (Some(name), Some(lat), Some(lon)) = (text(item, "\"name\":\""), number("\"latitude\":"), number("\"longitude\":")) else {
            continue;
        };
        let label = match text(item, "\"country_code\":\"") {
            Some(code) if !code.is_empty() => format!("{name}, {code}"),
            _ => name,
        };
        let is_there = |key: &str| text(item, key).is_some_and(|v| v.to_lowercase() == region.to_lowercase());
        if region.is_empty() || is_there("\"admin1\":\"") || is_there("\"country\":\"") {
            return Some((label, lat, lon));
        }
        first.get_or_insert((label, lat, lon));
    }
    first
}

pub fn find_main(args: &[String]) -> i32 {
    let (Some(cache), Some(unit), Some(typed)) = (args.first(), args.get(1), args.get(2)) else { return 2 };
    let (name, country, region) = split_query(typed);
    // one result is enough unless there is a region to look for among them
    let mut path = format!(
        "/v1/search?name={}&count={}&language=en&format=json",
        url_encode(name),
        if region.is_empty() { 1 } else { 20 }
    );
    if let Some(code) = country {
        path.push_str("&countryCode=");
        path.push_str(&code);
    }
    let Some(json) = http_get("geocoding-api.open-meteo.com", 443, true, &path) else { return EXIT_NO_GEOCODER };
    let Some((city, lat, lon)) = parse_city(&json, region) else { return EXIT_NO_CITY };
    let report = Report { fetched: crate::clock::now(), unit: unit.clone(), is_day: true, lat, lon, city, ..Report::default() };
    forecast(report, Path::new(cache))
}

/// One GET over WinHTTP, which is loaded here and nowhere else.
fn http_get(host: &str, port: u16, secure: bool, path: &str) -> Option<String> {
    type H = *mut c_void;
    type Open = unsafe extern "system" fn(*const u16, u32, *const u16, *const u16, u32) -> H;
    type SetTimeouts = unsafe extern "system" fn(H, i32, i32, i32, i32) -> i32;
    type Connect = unsafe extern "system" fn(H, *const u16, u16, u32) -> H;
    type OpenRequest = unsafe extern "system" fn(H, *const u16, *const u16, *const u16, *const u16, *const *const u16, u32) -> H;
    type Send = unsafe extern "system" fn(H, *const u16, u32, *const c_void, u32, u32, usize) -> i32;
    type Receive = unsafe extern "system" fn(H, *mut c_void) -> i32;
    type QueryHeaders = unsafe extern "system" fn(H, u32, *const u16, *mut c_void, *mut u32, *mut u32) -> i32;
    type Read = unsafe extern "system" fn(H, *mut u8, u32, *mut u32) -> i32;
    type Close = unsafe extern "system" fn(H) -> i32;

    const FLAG_SECURE: u32 = 0x0080_0000;
    const QUERY_STATUS_NUMBER: u32 = 19 | 0x2000_0000;

    unsafe {
        let f = |name: &'static [u8]| sys::proc_address("winhttp.dll", name);
        let open: Open = core::mem::transmute(f(b"WinHttpOpen\0")?);
        let set_timeouts: SetTimeouts = core::mem::transmute(f(b"WinHttpSetTimeouts\0")?);
        let connect: Connect = core::mem::transmute(f(b"WinHttpConnect\0")?);
        let open_request: OpenRequest = core::mem::transmute(f(b"WinHttpOpenRequest\0")?);
        let send: Send = core::mem::transmute(f(b"WinHttpSendRequest\0")?);
        let receive: Receive = core::mem::transmute(f(b"WinHttpReceiveResponse\0")?);
        let query: QueryHeaders = core::mem::transmute(f(b"WinHttpQueryHeaders\0")?);
        let read: Read = core::mem::transmute(f(b"WinHttpReadData\0")?);
        let close: Close = core::mem::transmute(f(b"WinHttpCloseHandle\0")?);

        // 4 = WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: whatever the system uses
        let session = open(sys::wide("Win98Widgets").as_ptr(), 4, null(), null(), 0);
        if session.is_null() {
            return None;
        }
        set_timeouts(session, 8000, 8000, 8000, 12000);
        let mut body = None;
        let connection = connect(session, sys::wide(host).as_ptr(), port, 0);
        if !connection.is_null() {
            let request = open_request(
                connection, sys::wide("GET").as_ptr(), sys::wide(path).as_ptr(),
                null(), null(), null(), if secure { FLAG_SECURE } else { 0 },
            );
            if !request.is_null() {
                if send(request, null(), 0, null(), 0, 0, 0) != 0 && receive(request, null_mut()) != 0 {
                    let (mut status, mut len) = (0u32, 4u32);
                    query(request, QUERY_STATUS_NUMBER, null(), &mut status as *mut u32 as *mut c_void, &mut len, null_mut());
                    if status == 200 {
                        let mut bytes = Vec::new();
                        let mut chunk = [0u8; 4096];
                        loop {
                            let mut got = 0u32;
                            if read(request, chunk.as_mut_ptr(), chunk.len() as u32, &mut got) == 0 || got == 0 {
                                break;
                            }
                            bytes.extend_from_slice(&chunk[..got as usize]);
                            if bytes.len() > 1 << 20 {
                                break;
                            }
                        }
                        body = Some(String::from_utf8_lossy(&bytes).into_owned());
                    }
                }
                close(request);
            }
            close(connection);
        }
        close(session);
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_place() {
        let json = r#"{"status":"success","country":"X","city":"Somewhere","lat":12.5,"lon":-3.25,"timezone":"Z"}"#;
        assert_eq!(parse_place(json), Some(("Somewhere".into(), "12.5".into(), "-3.25".into())));
        assert_eq!(parse_place("{}"), None);
    }

    #[test]
    fn reads_the_forecast_past_the_units_blocks() {
        let json = r#"{"current_weather_units":{"temperature":"C","is_day":"","weathercode":"wmo code"},
            "current_weather":{"time":"t","temperature":9.3,"windspeed":4.0,"is_day":0,"weathercode":3},
            "daily_units":{"temperature_2m_max":"C","temperature_2m_min":"C"},
            "daily":{"time":["a","b"],"weathercode":[3,61],"temperature_2m_max":[10.1,12.0],"temperature_2m_min":[4.9,6.0]}}"#;
        let mut r = Report::default();
        assert!(parse_forecast(json, &mut r));
        assert_eq!((r.temp, r.code, r.is_day, r.tmax, r.tmin), (Some(9.3), Some(3), false, Some(10.1), Some(4.9)));
    }

    #[test]
    fn reads_the_city() {
        let json = r#"{"results":[{"id":1,"name":"Portland","latitude":45.52345,"longitude":-122.67621,"elevation":15.0,"feature_code":"PPLA2","country_code":"US","admin1_id":5744337,"timezone":"America/Los_Angeles","postcodes":["97201","97202"],"country_id":6252001,"country":"United States","admin1":"Oregon","admin2":"Multnomah"},{"id":2,"name":"Portland","latitude":43.65737,"longitude":-70.2589,"country_code":"US","country":"United States","admin1":"Maine"}],"generationtime_ms":0.2}"#;
        let oregon = Some(("Portland, US".to_owned(), "45.52345".to_owned(), "-122.67621".to_owned()));
        assert_eq!(parse_city(json, ""), oregon);
        assert_eq!(parse_city(json, "maine"), Some(("Portland, US".into(), "43.65737".into(), "-70.2589".into())));
        assert_eq!(parse_city(json, "united states"), oregon);
        // a region none of them is in: the likeliest is still an answer
        assert_eq!(parse_city(json, "Texas"), oregon);
        // no match at all comes back with no results key
        assert_eq!(parse_city(r#"{"generationtime_ms":0.1}"#, ""), None);
    }

    #[test]
    fn splits_what_was_typed() {
        assert_eq!(split_query(" Lviv "), ("Lviv", None, ""));
        assert_eq!(split_query("paris, us"), ("paris", Some("US".to_owned()), ""));
        assert_eq!(split_query("London,UK"), ("London", Some("GB".to_owned()), ""));
        assert_eq!(split_query("Portland, Maine"), ("Portland", None, "Maine"));
    }

    #[test]
    fn encodes_a_name_for_the_query() {
        assert_eq!(url_encode("New York"), "New%20York");
        assert_eq!(url_encode("M\u{fc}nchen&x=1"), "M%C3%BCnchen%26x%3D1");
    }

    fn type_in(field: &mut Field, s: &str) {
        for c in s.chars() {
            field.typed(c);
        }
    }

    #[test]
    fn typing_in_the_field_replaces_the_name_and_enter_takes_it() {
        let mut f = Field::new();
        assert!(matches!(f.begin("Lviv, UA"), Outcome { request: Request::TakeKeys, .. }));
        // all of it selected, so that typing replaces it
        assert_eq!(f.doc.range(), (0, 8));
        type_in(&mut f, "Kyiv");
        assert_eq!(f.text(), "Kyiv");
        assert!(matches!(f.key(Key::Enter, false, false), Outcome { request: Request::DropKeys, .. }));
        assert_eq!(f.end().as_deref(), Some("Kyiv"));
        assert!(!f.editing);
    }

    #[test]
    fn a_name_left_as_it_was_is_an_answer_only_to_enter() {
        let mut f = Field::new();
        // clicked in, clicked away
        f.begin("Lviv, UA");
        assert_eq!(f.end(), None);
        // typed on and put back
        f.begin("Lviv, UA");
        type_in(&mut f, "x");
        f.key(Key::Undo, false, false);
        assert_eq!(f.text(), "Lviv, UA");
        assert_eq!(f.end(), None);
        f.begin("Lviv, UA");
        f.key(Key::Enter, false, false);
        assert_eq!(f.end().as_deref(), Some("Lviv, UA"));
        // Escape takes nothing, whatever was typed
        f.begin("Lviv, UA");
        type_in(&mut f, "Kyiv");
        f.key(Key::Escape, false, false);
        assert_eq!(f.end(), None);
        // emptied: an answer, and the one that unpins the place
        f.begin("Lviv, UA");
        f.key(Key::Delete, false, false);
        assert_eq!(f.end().as_deref(), Some(""));
    }

    #[test]
    fn the_field_takes_one_line_and_no_more_than_a_name() {
        let mut f = Field::new();
        f.begin("");
        f.paste("\r\n  \r\nNew York\r\nLondon");
        assert_eq!(f.text(), "New York");
        type_in(&mut f, &"x".repeat(100));
        assert_eq!(f.doc.text().len(), LIMIT);
        // what is selected makes room for what replaces it
        f.key(Key::SelectAll, false, false);
        f.paste(&"y".repeat(100));
        assert_eq!(f.text(), "y".repeat(LIMIT));
        // brackets are brackets here, and a tab is nothing
        f.key(Key::SelectAll, false, false);
        type_in(&mut f, "[ ] x\t");
        assert_eq!(f.text(), "[ ] x");
    }

    #[test]
    fn a_short_name_is_centred_and_a_click_finds_its_letters() {
        let mut f = Field::new();
        f.begin("Lviv");
        let (x, room) = f.column();
        assert!(f.fits && f.left > 0.0);
        // as much room to its left as to its right
        assert!((f.column_of(0) - x - (x + room - f.column_of(4))).abs() <= 1);
        // the press that started the typing selected the name: moving the
        // pointer before letting go does not undo that
        assert_eq!(f.drag(f.column_of(1)), Dirty::None);
        assert_eq!(f.doc.range(), (0, 4));
        f.press(f.column_of(2), false, false);
        assert_eq!((f.doc.caret(), f.doc.range()), (2, (2, 2)));
        f.drag(f.column_of(4));
        assert_eq!(f.copy().as_deref(), Some("iv"));
        f.press(f.column_of(1), true, false);
        assert_eq!(f.copy().as_deref(), Some("Lviv"));
    }

    #[test]
    fn a_name_too_long_for_the_field_scrolls_under_the_caret() {
        let mut f = Field::new();
        f.begin("");
        type_in(&mut f, "Llanfairpwllgwyngyllgogerychwyrndrobwllllantysiliogogogoch");
        let (x, room) = f.column();
        assert!(!f.fits && f.left < 0.0);
        // the end, where the caret is, is in sight
        assert!((x..x + room).contains(&f.caret_box().x0));
        f.key(Key::Home, false, false);
        assert_eq!((f.left, f.caret_box().x0), (0.0, x));
        // and back again, a word at a time
        f.key(Key::End, false, false);
        assert!(f.left < 0.0 && (x..x + room).contains(&f.caret_box().x0));
        // a click puts the caret under the pointer, wherever the name has got to
        f.press(x + 40, false, false);
        assert!((f.caret_box().x0 - (x + 40)).abs() <= 4);
    }

    #[test]
    fn conditions_and_icons() {
        assert_eq!(condition(Some(3)), "Overcast");
        assert_eq!(condition(None), "Unknown");
        assert_eq!(icon_for(0, false), "weather-clear-night");
        assert_eq!(icon_for(45, true), "weather-overcast");
        assert_eq!(icon_for(99, true), "weather-severe-alert");
        assert_eq!(icon_for(95, true), "weather-storm");
    }
}
