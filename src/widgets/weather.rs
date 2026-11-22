//! Today's weather, in one of two looks, picked from the widget's right-click
//! menu.
//!
//! The panel: temperature, condition icon, place and the day's high and low,
//! as a Win98 property sheet, with the place in a field typed on where it
//! stands.  The window: the weather itself, out of a small dark window --
//! one of twelve scenes, by day, at dusk or by night, over a place of the
//! weather's own or over the city the forecast is for (see `sky`) -- with the
//! temperature, the condition, the high and low and whatever else matters
//! today written over it, and the place in the title bar.  Picking the other
//! look makes the window again at the other's size, where it was, as the
//! Photo Frame's shapes do.
//!
//! Two lookups feed both: the public IP's location from ip-api.com (once a
//! day, or never if a place is pinned in settings.ini), then the forecast
//! from Open-Meteo every half hour.
//!
//! The place is pinned by typing a city -- into the panel's city field, or,
//! in the window, into its title bar after Set city... in the menu.  The
//! name goes to Open-Meteo's geocoder, and what comes back is kept in
//! settings.ini as ManualLat, ManualLon and ManualLocation -- the same three
//! keys that can be written there by hand.
//!
//! The field is typed on where it stands: see `field`, which the world
//! clock's cities are typed into as well.
//!
//! The network is kept out of this process altogether (see `net`).  A fetch
//! is the program run again as `--fetch-weather`, which loads WinHTTP, writes
//! a few lines to a cache file and exits -- so the DLLs, sockets and TLS
//! state a request drags in live for a second every half hour instead of for
//! good.  Finding a city is the same again as `--find-city`, which goes on to
//! fetch that city's forecast.  The cache also means a restart shows the last
//! forecast at once.

use core::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use crate::canvas::{Canvas, Rect};
use crate::clock;
use crate::field::Field;
use crate::geocode::{self, after, until, Miss};
use crate::http;
use crate::keys::Key;
use crate::settings::{data_dir, write_whole, Settings};
use crate::skin::{self, Hooks, Meter, Skin, ICONS};
use crate::sky::{City, Light, Scene, Sky};
use crate::text::Text;
use crate::tint;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Pressed, Request, Widget};

static PANEL: &Skin = &skin::weather::SKIN;
static WINDOW: &Skin = &skin::weather_window::SKIN;
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
/// The longest stretch one frame of the window may stand for, in seconds.
/// After a stall the weather carries on from where it was rather than
/// leaping ahead.
const MAX_STEP: f32 = 0.1;

/// How `--find-city` ends when it has no place to report: the name matched
/// nothing, or the geocoder could not be asked.  Either way the cache file is
/// left alone, so the forecast on screen is still there after a restart.
const EXIT_NO_CITY: i32 = 3;
const EXIT_NO_GEOCODER: i32 = 4;

/// The menu's entries.  The first four are both looks'.
const MENU_REFRESH: u32 = 1;
const MENU_FAHRENHEIT: u32 = 2;
pub const MENU_CITY: u32 = 3;
const MENU_AUTOMATIC: u32 = 4;
const MENU_PANEL: u32 = 5;
const MENU_WINDOW: u32 = 6;
const MENU_PAUSE: u32 = 7;
const MENU_FOLLOW: u32 = 8;
/// ... and one for each scene, in `Scene::ALL`'s order, from here.
const MENU_SCENE: u32 = 10;
/// The scenery: the city the forecast is for, if it has a place of its own;
/// the weather's own places always; or one city always, from here.
const MENU_PLACE_AUTO: u32 = 30;
const MENU_PLACE_OWN: u32 = 31;
const MENU_PLACE_CITY: u32 = 32;

/// What the window's weather is seen over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scenery {
    /// The city the forecast is for, if it is one with a place of its own;
    /// otherwise the weather's own place.
    Auto,
    /// The weather's own places, wherever the forecast is for.
    Own,
    /// This city, wherever the forecast is for.
    City(City),
}

impl Scenery {
    fn of(key: &str) -> Scenery {
        match key.trim() {
            k if k.eq_ignore_ascii_case("weather") => Scenery::Own,
            k => City::from_key(k).map_or(Scenery::Auto, Scenery::City),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Scenery::Auto => "",
            Scenery::Own => "weather",
            Scenery::City(c) => c.key(),
        }
    }
}

/// A new moon, 6 January 2000 at 18:14 UTC, and the length of the month
/// from one to the next: enough to tell the moon's phase tonight.
const NEW_MOON: i64 = 947_182_440;
const MONTH_DAYS: f64 = 29.530_588_853;

/// Where the last forecast is kept.  A test never reads this machine's own,
/// nor writes over it.
fn cache_file() -> PathBuf {
    if cfg!(test) {
        std::env::temp_dir().join("win98-weather-test-no-cache").join("weather.txt")
    } else {
        data_dir().join("weather.txt")
    }
}

/// The two looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Panel,
    Window,
}

impl Look {
    fn of(settings: &Settings) -> Look {
        let look = settings.text(SECTION, "Look", PANEL.var("Look"));
        if look.trim().eq_ignore_ascii_case("window") { Look::Window } else { Look::Panel }
    }

    fn skin(self) -> &'static Skin {
        match self {
            Look::Panel => PANEL,
            Look::Window => WINDOW,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Look::Panel => "panel",
            Look::Window => "window",
        }
    }
}

/// The skin of the look settings.ini asks for: the window is made from it.
pub fn skin_of(settings: &Settings, _section: &str) -> &'static Skin {
    Look::of(settings).skin()
}

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

/// Whether what a day's code says will fall is snow.
fn snows(code: i32) -> bool {
    matches!(code, 71..=77 | 85 | 86)
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

/// Where the wind comes from, as the compass has it: the nearest of eight.
fn compass(degrees: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[((degrees.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

/// What the UV index means, as the WHO's bands have it.
fn uv_word(uv: f64) -> &'static str {
    match round(uv) {
        ..=2 => "low",
        3..=5 => "moderate",
        6..=7 => "high",
        8..=10 => "very high",
        _ => "extreme",
    }
}

/// The moon's age at `now`, as a share of the month: 0 new, 0.5 full.
fn moon_age(now: i64) -> f32 {
    ((now - NEW_MOON) as f64 / 86_400.0 / MONTH_DAYS).rem_euclid(1.0) as f32
}

/// The 12/24-hour choice, the Date/Time widget's.
fn use24(settings: &Settings) -> bool {
    settings.flag("DateTime", "Use24Hour", skin::datetime::SKIN.int("Use24Hour", 1) != 0)
}

/// The clock at `ts` where the forecast is for -- `offset` seconds ahead of
/// UTC -- as "16:02", or "4:02 PM".
fn clock_at(ts: i64, offset: i64, use24: bool) -> String {
    let tm = clock::utc(ts + offset);
    if use24 {
        format!("{:02}:{:02}", tm.hour, tm.min)
    } else {
        let hour = if tm.hour % 12 == 0 { 12 } else { tm.hour % 12 };
        format!("{hour}:{:02} {}", tm.min, if tm.hour < 12 { "AM" } else { "PM" })
    }
}

/// What a fetch leaves behind: `key=value` lines in the cache file.
#[derive(Default, Clone, Debug)]
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
    /// What it feels like: the apparent temperature, wind and damp and all.
    feels: Option<f64>,
    /// The wind's speed, in km/h or mph as the unit goes, and the bearing it
    /// blows from.
    wind: Option<f64>,
    wind_dir: Option<f64>,
    /// The UV index now.
    uv: Option<f64>,
    /// Today's best chance of rain or snow, in percent, and today's code --
    /// which says which it will be.
    precip: Option<f64>,
    day_code: Option<i32>,
    /// How far the place's clock is ahead of UTC, in seconds.
    offset: i64,
    /// Today's sunrise and sunset, and tomorrow's sunrise.
    sunrise: Option<i64>,
    sunset: Option<i64>,
    sunrise2: Option<i64>,
    /// "geo" or "forecast" when that request failed.
    error: String,
}

impl Report {
    fn parse(text: &str) -> Report {
        let mut r = Report { is_day: true, ..Report::default() };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let time = || v.trim().parse::<i64>().ok();
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
                "feels" => r.feels = v.parse().ok(),
                "wind" => r.wind = v.parse().ok(),
                "wind_dir" => r.wind_dir = v.parse().ok(),
                "uv" => r.uv = v.parse().ok(),
                "precip" => r.precip = v.parse().ok(),
                "day_code" => r.day_code = v.parse::<f64>().ok().map(|c| c as i32),
                "offset" => r.offset = v.parse().unwrap_or(0),
                "sunrise" => r.sunrise = time(),
                "sunset" => r.sunset = time(),
                "sunrise2" => r.sunrise2 = time(),
                "error" => r.error = v.to_owned(),
                _ => {}
            }
        }
        r
    }

    fn write(&self, path: &Path) {
        let num = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
        let int = |v: Option<i64>| v.map(|n| n.to_string()).unwrap_or_default();
        let text = format!(
            "fetched={}\nunit={}\nlat={}\nlon={}\ncity={}\nlocated={}\ntemp={}\ncode={}\nis_day={}\ntmax={}\ntmin={}\n\
             feels={}\nwind={}\nwind_dir={}\nuv={}\nprecip={}\nday_code={}\noffset={}\nsunrise={}\nsunset={}\nsunrise2={}\nerror={}\n",
            self.fetched, self.unit, self.lat, self.lon, self.city, self.located,
            num(self.temp), self.code.map(|c| c.to_string()).unwrap_or_default(),
            self.is_day as i32, num(self.tmax), num(self.tmin),
            num(self.feels), num(self.wind), num(self.wind_dir), num(self.uv), num(self.precip),
            self.day_code.map(|c| c.to_string()).unwrap_or_default(), self.offset,
            int(self.sunrise), int(self.sunset), int(self.sunrise2), self.error
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = write_whole(path, text.as_bytes());
    }

    fn fahrenheit(&self) -> bool {
        self.unit == "fahrenheit"
    }

    /// Where the sun is at `now` for the place this is for: from its sunrise
    /// and sunset when they are known, or else from whether it is day.
    fn light(&self, now: i64) -> Light {
        let moon = moon_age(now);
        let (Some(rise), Some(set)) = (self.sunrise, self.sunset) else {
            return if self.is_day { Light { moon, ..Light::NOON } } else { Light { moon, ..Light::MIDNIGHT } };
        };
        if set <= rise || set - rise >= 86_400 {
            return if self.is_day { Light { moon, ..Light::NOON } } else { Light { moon, ..Light::MIDNIGHT } };
        }
        if (rise..=set).contains(&now) {
            let t = (now - rise) as f32 / (set - rise) as f32;
            return Light { day: (core::f32::consts::PI * t).sin(), sun: t, night: 0.0, moon };
        }
        // The night: from this sunset to tomorrow's sunrise, or from last
        // night's sunset to this sunrise.  Within an hour or so of either it
        // is dusk; after that, night.
        let length = (86_400 - (set - rise)) as f32;
        let (since, until) = if now > set {
            let since = (now - set) as f32;
            (since, self.sunrise2.map_or(length - since, |r| (r - now) as f32))
        } else {
            let until = (rise - now) as f32;
            (length - until, until)
        };
        let (since, until) = (since.max(0.0), until.max(0.0));
        Light {
            day: -(since.min(until) / 4200.0).min(1.0),
            sun: if now > set { 1.0 } else { 0.0 },
            night: (since / (since + until).max(1.0)).clamp(0.0, 1.0),
            moon,
        }
    }

    /// The wind, in km/h, plus when it blows to the right of a window that
    /// looks north -- from the west.
    fn wind_across(&self) -> Option<f32> {
        let speed = self.wind? * if self.fahrenheit() { 1.609 } else { 1.0 };
        let from = self.wind_dir.unwrap_or(270.0).to_radians();
        Some((-speed * from.sin()) as f32)
    }
}

/// The readings, laid out once and put up over every frame of the window's
/// scene: what they said, and the picture of them.
#[derive(Default)]
struct Overlay {
    key: String,
    px: Vec<u32>,
}

pub struct Weather {
    look: Look,
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
    /// The report on show, if one is.
    shown: Option<Report>,
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

    // ---- the window ----------------------------------------------------------
    /// The scene, while the window is the look.
    sky: Option<Sky>,
    /// The scene kept to from the menu, or none to follow the weather.
    pinned: Option<Scene>,
    /// What the weather is seen over.
    scenery: Scenery,
    paused: bool,
    /// The readings under the condition: the high and low, then whatever
    /// else matters today.
    lines: Vec<String>,
    overlay: RefCell<Overlay>,
    /// The clock, as of the last tick, and the minute the readings and the
    /// light were last brought up to it.
    now: i64,
    minute: i64,
    use24: bool,
    /// For the test harness: the sun kept where it is put.
    fixed_light: Option<Light>,
}

impl Weather {
    /// The weather in the look settings.ini asks for.
    pub fn new(ctx: &Ctx) -> Weather {
        Weather::new_in(ctx, Look::of(ctx.settings))
    }

    /// The weather in `look`, whatever settings.ini asks for.
    pub fn new_in(ctx: &Ctx, look: Look) -> Weather {
        let s = &*ctx.settings;
        let get = |key: &str| s.text(SECTION, key, PANEL.var(key)).trim().to_owned();
        let unit = if get("TempUnit").eq_ignore_ascii_case("fahrenheit") { "fahrenheit" } else { "celsius" };
        let (lat, lon) = (get("ManualLat"), get("ManualLon"));
        let field = match look {
            Look::Panel => Field::new(PANEL, "MeterCity"),
            // in the title bar, inside the line round it, typed from the left
            // as a title is
            Look::Window => {
                let mut field = Field::new(WINDOW, "MeterTitleField");
                let b = field.rect;
                field.rect = Rect::new(b.x0 + 1, b.y0 + 1, b.x1 - 1, b.y1 - 1);
                field.flush_left = true;
                field
            }
        };
        let pinned = Scene::from_key(&get("Scene"));
        let mut weather = Weather {
            look,
            field,
            unit: unit.to_owned(),
            degrees: if unit == "fahrenheit" { "\u{b0}F" } else { "\u{b0}C" },
            // pinned to a place by hand: never ask ip-api.com anything
            manual: (!lat.is_empty() && !lon.is_empty()).then_some((lat, lon)),
            manual_label: get("ManualLocation"),
            temp_now: PANEL.var("TempNow").to_owned(),
            condition: PANEL.var("Condition").to_owned(),
            high_low: PANEL.var("HighLowText").to_owned(),
            location: String::new(),
            icon: None,
            cache: cache_file(),
            last: Report::default(),
            shown: None,
            showing: false,
            child: None,
            finding: None,
            notice: None,
            next_fetch: 0,
            frozen: false,
            sky: (look == Look::Window).then(|| {
                let scene = pinned.unwrap_or(Scene::Overcast);
                Sky::new(WINDOW, scene, Report::default().light(ctx.now), -9.0, ctx.now as u32)
            }),
            pinned,
            scenery: Scenery::of(&get("Scenery")),
            paused: s.flag(SECTION, "Paused", PANEL.int("Paused", 0) != 0),
            lines: Vec::new(),
            overlay: RefCell::new(Overlay::default()),
            now: ctx.now,
            minute: ctx.now.div_euclid(60),
            use24: use24(s),
            fixed_light: None,
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
        weather.refresh_window();
        weather
    }

    /// For the test harness: show exactly this.
    pub fn set_fixed(&mut self, temp: f64, code: i32, is_day: bool, tmax: f64, tmin: f64, city: &str) {
        self.frozen = true;
        let report = Report {
            temp: Some(temp), code: Some(code), is_day, tmax: Some(tmax), tmin: Some(tmin),
            city: city.to_owned(), unit: self.unit.clone(), ..Report::default()
        };
        self.show(&report);
    }

    /// For the test harness: the rest of what the window can say, on top of
    /// `set_fixed` -- what it feels like, the wind and the bearing it blows
    /// from, the UV index, today's chance of rain, and today's sunrise and
    /// sunset as "07:30" at the place, which is taken to keep this machine's
    /// clock.  What is not given is not known.
    #[allow(clippy::too_many_arguments)]
    pub fn set_more(&mut self, feels: Option<f64>, wind: Option<f64>, wind_dir: Option<f64>, uv: Option<f64>, precip: Option<f64>, sunrise: &str, sunset: &str) {
        let Some(mut report) = self.shown.clone() else { return };
        let today = clock::local(self.now);
        let at = |hm: &str| {
            let (h, m) = hm.trim().split_once(':')?;
            Some(clock::mktime(today.year, today.month, today.day, h.parse().ok()?, m.parse().ok()?, 0))
        };
        report.offset = clock::reading(&today) - self.now;
        (report.feels, report.wind, report.wind_dir, report.uv, report.precip) = (feels, wind, wind_dir, uv, precip);
        report.sunrise = at(sunrise);
        report.sunset = at(sunset);
        report.sunrise2 = report.sunrise.map(|r| r + 86_400);
        self.show(&report);
    }

    /// For the test harness: the window's scene `seconds` in, always the same
    /// one; and, if `light` is given, the sun kept where it says.
    pub fn set_sky(&mut self, seconds: f32, light: Option<Light>) {
        if light.is_some() {
            self.fixed_light = light;
        }
        self.refresh_window();
        if let Some(sky) = &mut self.sky {
            sky.set_fixed(seconds);
        }
    }

    /// For the test harness: keep to `scene`, or follow the weather.
    pub fn pin(&mut self, scene: Option<Scene>) {
        self.pinned = scene;
        self.refresh_window();
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
        self.shown = Some(r.clone());
        self.refresh_window();
    }

    /// With nothing to show, say which half failed.
    fn show_error(&mut self, error: &str) {
        if self.showing {
            return;
        }
        self.temp_now = String::from("--");
        if error == "geo" {
            self.condition = String::from("No location");
            self.high_low = String::from(match self.look {
                Look::Panel => "Type a city below",
                Look::Window => "Right-click: Set city...",
            });
            self.location = String::from("ip-api.com did not answer");
        } else {
            self.condition = String::from("No forecast");
            self.high_low = String::from("Open-Meteo unreachable");
        }
        self.refresh_window();
    }

    /// Back to "Loading...": what is on screen is for another place.
    fn blank(&mut self) {
        self.showing = false;
        self.shown = None;
        self.temp_now = PANEL.var("TempNow").to_owned();
        self.condition = PANEL.var("Condition").to_owned();
        self.high_low = PANEL.var("HighLowText").to_owned();
        self.location = self.manual_label.clone();
        self.icon = None;
        self.refresh_window();
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

    /// What the city field -- or the window's title -- says while nobody is
    /// typing in it: what became of the name just typed, while that is news;
    /// the lookup, while it runs; otherwise the place.
    fn label(&self) -> &str {
        match (&self.notice, &self.finding) {
            (Some((text, _)), _) => text,
            (None, Some(finding)) => finding,
            (None, None) => &self.location,
        }
    }

    /// The window's title: the label, or "Weather" while there is none.
    fn title(&self) -> &str {
        match self.label() {
            "" => WINDOW.var("Place"),
            label => label,
        }
    }

    /// The place by the name the typing starts from: the one on show, so
    /// that nothing in the field changes as it is clicked.  With no forecast
    /// the field may be saying what went wrong instead, and that is not a
    /// name: then it is the pinned place, or nothing.
    fn place(&self) -> &str {
        if self.showing { &self.location } else { self.city_text() }
    }

    /// The field the window's place is typed in, with the line round it.
    fn title_box(&self) -> Rect {
        WINDOW.meter("MeterTitleField").map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3]))
    }

    /// The window's scene: all a frame has to repaint.
    fn pane(&self) -> Rect {
        WINDOW.meter("MeterSky").map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3]))
    }

    /// Give the city field the keyboard: the panel's, or the window's title.
    fn type_city(&mut self) -> Outcome {
        if self.field.editing {
            return Outcome::none();
        }
        // whatever the field was saying about the last name, it has been read
        self.notice = None;
        let place = self.place().to_owned();
        let outcome = self.field.begin(&place);
        match self.look {
            Look::Panel => outcome,
            // the title gives way to the field
            Look::Window => Outcome { dirty: outcome.dirty.and(Dirty::Part(self.title_box())), ..outcome },
        }
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

    // ---- the window --------------------------------------------------------------

    /// The readings under the condition, as they stand: the high and low,
    /// then each of the others that is worth saying today -- the chance of
    /// rain or snow when it is one in five or more, what it feels like when
    /// that is 3 degrees (5 Fahrenheit) off the temperature, the wind when
    /// it is a breeze or more, the UV in daylight when it is moderate or
    /// worse -- and last the next sunrise or sunset.
    fn window_lines(&self) -> Vec<String> {
        let mut lines = vec![self.high_low.clone()];
        let Some(r) = self.shown.as_ref().filter(|_| self.showing) else { return lines };
        if let (Some(max), Some(min)) = (r.tmax, r.tmin) {
            lines[0] = format!("High {}\u{b0}  Low {}\u{b0}", round(max), round(min));
        }
        let f = r.fahrenheit();
        if let Some(chance) = r.precip.filter(|&p| p >= 20.0) {
            let snow = r.day_code.or(r.code).is_some_and(snows);
            lines.push(format!("{} {}%", if snow { "Snow" } else { "Rain" }, round(chance)));
        }
        if let (Some(feels), Some(temp)) = (r.feels, r.temp) {
            if (feels - temp).abs() >= if f { 5.0 } else { 3.0 } {
                lines.push(format!("Feels like {}\u{b0}", round(feels)));
            }
        }
        if let Some(wind) = r.wind.filter(|&w| w >= if f { 12.0 } else { 20.0 }) {
            let from = r.wind_dir.map(|d| format!(" {}", compass(d))).unwrap_or_default();
            lines.push(format!("Wind {} {}{from}", round(wind), if f { "mph" } else { "km/h" }));
        }
        if let Some(uv) = r.uv.filter(|&u| round(u) >= 3 && r.is_day) {
            lines.push(format!("UV {}, {}", round(uv), uv_word(uv)));
        }
        let now = self.now;
        let next = match (r.sunrise, r.sunset) {
            (Some(rise), _) if now < rise => Some(("Sunrise", rise)),
            (_, Some(set)) if now < set => Some(("Sunset", set)),
            _ => r.sunrise2.filter(|&r2| now < r2).map(|r2| ("Sunrise", r2)),
        };
        if let Some((what, at)) = next {
            lines.push(format!("{what} {}", clock_at(at, r.offset, self.use24)));
        }
        lines.truncate(6);
        lines
    }

    /// The scene the window shows: the one kept to, or the weather's.
    fn scene(&self) -> Scene {
        self.pinned
            .or_else(|| self.shown.as_ref().filter(|_| self.showing).and_then(|r| r.code).map(Scene::of_code))
            .unwrap_or(Scene::Overcast)
    }

    /// Bring the window up to the readings, the weather and the clock: the
    /// lines, and the scene, its light and its wind.  What of the window
    /// that touched.
    fn refresh_window(&mut self) -> Dirty {
        if self.look != Look::Window {
            return Dirty::None;
        }
        let lines = self.window_lines();
        let mut changed = lines != self.lines;
        self.lines = lines;
        let report = self.shown.clone().filter(|_| self.showing).unwrap_or_default();
        let light = self.fixed_light.unwrap_or_else(|| report.light(self.now));
        let wind = report.wind_across().unwrap_or(-9.0);
        let scene = self.scene();
        let city = self.city();
        // the time there, for a clock: the place's own, or this machine's
        let tm = if self.showing { clock::utc(self.now + report.offset) } else { clock::local(self.now) };
        if let Some(sky) = &mut self.sky {
            let moved = sky.set_scene(scene)
                | sky.set_city(city)
                | sky.set_light(light)
                | sky.set_wind(wind)
                | sky.set_clock(tm.hour as u32, tm.min as u32);
            if moved {
                sky.render();
                changed = true;
            }
        }
        if changed { Dirty::Part(self.pane()) } else { Dirty::None }
    }

    /// The city the weather is seen over, if any: the one the scenery keeps
    /// to, or the one the forecast's place is in -- by where it is, the
    /// pinned place's or the reported one's.
    fn city(&self) -> Option<City> {
        match self.scenery {
            Scenery::Own => None,
            Scenery::City(c) => Some(c),
            Scenery::Auto => {
                let (lat, lon) = match (&self.manual, &self.shown) {
                    (Some((lat, lon)), _) => (lat.clone(), lon.clone()),
                    (None, Some(r)) => (r.lat.clone(), r.lon.clone()),
                    (None, None) => (self.last.lat.clone(), self.last.lon.clone()),
                };
                City::near(lat.trim().parse().ok()?, lon.trim().parse().ok()?)
            }
        }
    }

    /// Keep the scenery to this, and say so in settings.ini.
    fn keep_scenery(&mut self, scenery: Scenery, ctx: &mut Ctx) -> Outcome {
        self.scenery = scenery;
        ctx.settings.set(SECTION, "Scenery", scenery.key());
        Outcome::dirty(self.refresh_window())
    }

    /// What the readings say, and in which colours: the overlay is laid out
    /// again when this changes.
    fn overlay_key(&self) -> String {
        let bright = self.sky.as_ref().is_some_and(Sky::bright);
        format!("{}|{}|{}|{bright}|{:06X}", self.temp_now, self.condition, self.lines.join("|"), tint::current())
    }

    /// [MeterSky]: the scene, then the readings over it -- laid out onto a
    /// clear picture once, as they change, and put up as they are every
    /// frame after.
    fn draw_sky(&self, x: i32, y: i32, canvas: &mut Canvas, text: &mut Text) {
        let Some(sky) = &self.sky else { return };
        let (w, h) = sky.size();
        canvas.image(x, y, w, h, bgra(sky.pixels()));
        let key = self.overlay_key();
        let mut overlay = self.overlay.borrow_mut();
        if overlay.key != key || overlay.px.len() != (w * h) as usize {
            overlay.key = key;
            overlay.px.clear();
            overlay.px.resize((w * h) as usize, 0);
            // SAFETY: the canvas is exactly the vector, which outlives it.
            let mut layer = unsafe { Canvas::new(overlay.px.as_mut_ptr(), w, w, h) };
            let mut buf = String::new();
            for m in WINDOW.meters.iter().filter(|m| m.name.starts_with("MeterRead")) {
                skin::draw_meter(m, m.x - x, m.y - y, self, &mut layer, text, &mut buf);
            }
        }
        canvas.image(x, y, w, h, bgra(&overlay.px));
    }

    /// [MeterTitleField]: the place being typed, in a FieldBg box with a line
    /// of Ink round it.
    fn draw_title_field(&self, canvas: &mut Canvas, text: &mut Text) {
        let r = self.field.rect;
        canvas.fill(r.x0 - 1, r.y0 - 1, r.x1 - r.x0 + 2, r.y1 - r.y0 + 2, WINDOW.colour("Ink"));
        canvas.fill(r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0, WINDOW.colour("FieldBg"));
        self.field.draw("", canvas, text);
    }

    /// Keep to `scene` -- or, with none, follow the weather -- and say so in
    /// settings.ini.
    fn keep_to(&mut self, scene: Option<Scene>, ctx: &mut Ctx) -> Outcome {
        self.pinned = scene;
        ctx.settings.set(SECTION, "Scene", scene.map_or("", Scene::key));
        Outcome::dirty(self.refresh_window())
    }
}

/// Opaque or premultiplied 0xAARRGGBB pixels as the bytes a canvas puts up:
/// B, G, R, A, which is how a little-endian machine keeps them anyway.
fn bgra(px: &[u32]) -> &[u8] {
    // SAFETY: four bytes to a u32, read where they lie; u8 has no alignment.
    unsafe { core::slice::from_raw_parts(px.as_ptr().cast::<u8>(), px.len() * 4) }
}

impl Hooks for Weather {
    fn num(&self, var: &str) -> f64 {
        match var {
            // the window's title gives way to the field the place is typed in
            "TitleEdit" => (self.look == Look::Window && self.field.editing) as i32 as f64,
            _ => 0.0,
        }
    }

    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "TempNow" => &self.temp_now,
            "Condition" => &self.condition,
            "HighLowText" => &self.high_low,
            "Place" => self.title(),
            line => match skin::index_of(line, "Line") {
                Some(n) => self.lines.get(n.wrapping_sub(1)).map_or("", String::as_str),
                None => "",
            },
        });
    }

    /// The readings over the window's scene are dark on a light sky and
    /// light on a dark one.
    fn colour(&self, var: &str) -> u32 {
        let bright = self.sky.as_ref().is_some_and(Sky::bright);
        match (var, bright) {
            ("ReadInk", true) => WINDOW.colour("ReadDarkInk"),
            ("ReadInk", false) => WINDOW.colour("ReadLightInk"),
            ("ReadShadow", true) => WINDOW.colour("ReadDarkShadow"),
            ("ReadShadow", false) => WINDOW.colour("ReadLightShadow"),
            _ => 0,
        }
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
            "MeterSky" => self.draw_sky(x, y, canvas, text),
            "MeterTitleField" if self.field.editing => self.draw_title_field(canvas, text),
            _ => {}
        }
    }
}

impl Widget for Weather {
    fn skin(&self) -> &'static Skin {
        self.look.skin()
    }

    fn about(&self) -> &'static str {
        match self.look {
            Look::Panel => {
                "Win98 Weather\n\n\
                 The current temperature and condition, and today's high and low, \
                 from Open-Meteo, refreshed every half hour.\n\n\
                 The place is where your public IP address appears to be (asked of \
                 ip-api.com once a day).  To pin it instead -- or to keep off \
                 ip-api.com altogether -- click the city field and type a city: \
                 \"Lviv\", or \"Paris, US\" or \"Portland, Maine\" to say which one.  \
                 Enter takes it, Escape does not, and an empty field goes back to \
                 the automatic place.\n\n\
                 Right-click, Animated window, for the weather itself."
            }
            Look::Window => {
                "Win98 Weather\n\n\
                 The weather out of a small window -- sun, cloud, fog, drizzle, \
                 rain, sleet, snow, storms and hail, by day, at dusk and by night -- \
                 with the temperature, today's high and low and whatever else \
                 matters today written over it: the chance of rain, what it feels \
                 like, the wind, the UV and the next sunrise or sunset.  From \
                 Open-Meteo, every half hour.\n\n\
                 The place is where your public IP address appears to be.  To pin \
                 it, right-click, Set city..., and type a city in the title bar: \
                 \"Lviv\", or \"Paris, US\" or \"Portland, Maine\".  Enter takes it, \
                 Escape does not, and an empty title goes back to the automatic \
                 place.\n\n\
                 In London or Aberdeen the weather is seen over the city itself: \
                 Big Ben and the Thames, or Marischal College by day and the \
                 harbour by night.  Right-click, Scenery, to keep to a city or to \
                 the weather's own places.\n\n\
                 Right-click for the animation -- the weather's own, or one kept \
                 to -- to pause it, and for the panel.  Paused, closed, or with \
                 the workstation locked, it costs nothing."
            }
        }
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        let now = ctx.now;
        self.now = now;
        // the readings and the light, once a minute
        let mut dirty = Dirty::None;
        if self.look == Look::Window && now.div_euclid(60) != self.minute {
            self.minute = now.div_euclid(60);
            self.use24 = use24(ctx.settings);
            dirty = self.refresh_window();
        }
        if self.frozen {
            return dirty;
        }
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
                    return dirty.and(self.finish_fetch(status.code(), ctx));
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

    /// The window's scene moves while it shows and is not paused.
    fn frame_ms(&self) -> u32 {
        match &self.sky {
            Some(sky) if !self.paused => sky.frame_ms(),
            _ => 0,
        }
    }

    fn frame(&mut self, ms: u32) -> Dirty {
        if self.paused {
            return Dirty::None;
        }
        let Some(sky) = &mut self.sky else { return Dirty::None };
        sky.step((ms as f32 / 1000.0).min(MAX_STEP));
        sky.render();
        Dirty::Part(self.pane())
    }

    fn command(&mut self, cmd: &str, _arg: i32, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        match cmd {
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    /// A press in the panel's city field starts the typing, with the name
    /// that is there all selected; once it has the keyboard a press puts the
    /// caret down, and held, it selects.  The window's title is typed in only
    /// after Set city..., and a press anywhere else is the host's.
    fn press(&mut self, x: i32, y: i32, double: bool, shift: bool) -> Pressed {
        if !self.field.reach().contains(x, y) {
            return Pressed::No;
        }
        if self.field.editing {
            return Pressed::Taken(self.field.press(x, double, shift));
        }
        match self.look {
            Look::Panel => Pressed::Taken(self.type_city()),
            Look::Window => Pressed::No,
        }
    }

    fn drag(&mut self, x: i32, _y: i32) -> Dirty {
        self.field.drag(x)
    }

    fn ibeam(&self, x: i32, y: i32) -> bool {
        (self.look == Look::Panel || self.field.editing) && self.field.rect.contains(x, y)
    }

    // ---- the keyboard -----------------------------------------------------------

    fn typing_at(&self, x: i32, y: i32) -> bool {
        (self.look == Look::Panel || self.field.editing) && self.field.reach().contains(x, y)
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
        let field = match self.look {
            Look::Panel => Dirty::Part(self.field.rect),
            Look::Window => Dirty::Part(self.title_box()),
        };
        match self.field.end() {
            Some(typed) if typed.is_empty() || typed != self.city_text() => field.and(self.set_city(&typed, ctx)),
            _ => field,
        }
    }

    fn retint(&mut self) {
        // the icons are pictures, and keep their own colours; the readings
        // over the scene are laid out again in the tint as they are next drawn
        self.field.retint();
        if let Some(sky) = &mut self.sky {
            sky.retint();
        }
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem::new(MENU_REFRESH, "Refresh now", false));
        items.push(MenuItem::new(MENU_FAHRENHEIT, "Fahrenheit", self.unit == "fahrenheit"));
        items.push(MenuItem::new(MENU_CITY, "Set city...", false));
        items.push(MenuItem::new(MENU_AUTOMATIC, "Automatic location", self.manual.is_none()));
        items.push(MenuItem::line());
        if self.look == Look::Window {
            items.push(MenuItem::sub("Animation"));
            items.push(MenuItem::new(MENU_FOLLOW, "Follow the weather", self.pinned.is_none()));
            items.push(MenuItem::line());
            for (i, scene) in Scene::ALL.into_iter().enumerate() {
                items.push(MenuItem::new(MENU_SCENE + i as u32, scene.label(), self.pinned == Some(scene)));
            }
            items.push(MenuItem::end_sub());
            items.push(MenuItem::sub("Scenery"));
            items.push(MenuItem::new(MENU_PLACE_AUTO, "Follow the place", self.scenery == Scenery::Auto));
            items.push(MenuItem::line());
            items.push(MenuItem::new(MENU_PLACE_OWN, "The weather's own", self.scenery == Scenery::Own));
            for (i, city) in City::ALL.into_iter().enumerate() {
                items.push(MenuItem::new(MENU_PLACE_CITY + i as u32, city.label(), self.scenery == Scenery::City(city)));
            }
            items.push(MenuItem::end_sub());
            items.push(MenuItem::new(MENU_PAUSE, "Pause animation", self.paused));
            items.push(MenuItem::line());
        }
        items.push(MenuItem::new(MENU_PANEL, "Panel", self.look == Look::Panel));
        items.push(MenuItem::new(MENU_WINDOW, "Animated window", self.look == Look::Window));
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            MENU_REFRESH => self.next_fetch = 0,
            MENU_FAHRENHEIT => {
                let fahrenheit = self.unit != "fahrenheit";
                self.unit = String::from(if fahrenheit { "fahrenheit" } else { "celsius" });
                self.degrees = if fahrenheit { "\u{b0}F" } else { "\u{b0}C" };
                ctx.settings.set(SECTION, "TempUnit", &self.unit);
                self.next_fetch = 0;
            }
            MENU_CITY => return self.type_city(),
            MENU_AUTOMATIC => return Outcome::dirty(self.set_automatic(ctx)),
            // the other look: its window is made again, at its size
            MENU_PANEL | MENU_WINDOW => {
                let look = if id == MENU_PANEL { Look::Panel } else { Look::Window };
                if look != self.look {
                    ctx.settings.set(SECTION, "Look", look.key());
                    return Outcome::request(Request::Reshape);
                }
            }
            MENU_PAUSE => {
                self.paused = !self.paused;
                ctx.settings.set_int(SECTION, "Paused", self.paused as i32);
            }
            MENU_FOLLOW => return self.keep_to(None, ctx),
            MENU_PLACE_AUTO => return self.keep_scenery(Scenery::Auto, ctx),
            MENU_PLACE_OWN => return self.keep_scenery(Scenery::Own, ctx),
            id if (MENU_PLACE_CITY..MENU_PLACE_CITY + City::ALL.len() as u32).contains(&id) => {
                return self.keep_scenery(Scenery::City(City::ALL[(id - MENU_PLACE_CITY) as usize]), ctx);
            }
            _ => {
                if let Some(&scene) = id.checked_sub(MENU_SCENE).and_then(|i| Scene::ALL.get(i as usize)) {
                    return self.keep_to(Some(scene), ctx);
                }
            }
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

/// `"city":"...","lat":..,"lon":..` out of ip-api.com's answer.
fn parse_place(json: &str) -> Option<(String, String, String)> {
    let city = after(json, "\"city\":\"").map(|s| until(s, &['"']).to_owned()).unwrap_or_default();
    let lat = until(after(json, "\"lat\":")?, &[',', '}']).to_owned();
    let lon = until(after(json, "\"lon\":")?, &[',', '}']).to_owned();
    (lat.parse::<f64>().is_ok() && lon.parse::<f64>().is_ok()).then_some((city, lat, lon))
}

/// The object after `key` -- `"current":{` -- up to its closing brace.  Each
/// of the forecast's has only numbers and lists in it, so the first brace is
/// its own.  The key is looked for with its colon and brace, so the units
/// block, "current_units", which comes first and has the same names in it,
/// is never mistaken for it.
fn block<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let rest = after(json, key)?;
    Some(&rest[..rest.find('}').unwrap_or(rest.len())])
}

/// A number in a block, by its name: `"temperature_2m":9.3`.  A null, as
/// Open-Meteo gives for what it does not know, is no number.
fn number(block: &str, name: &str) -> Option<f64> {
    until(after(block, &format!("\"{name}\":"))?, &[',', '}', ']']).parse().ok()
}

/// The entries of a list in a block, by its name, quotes and all.
fn list<'a>(block: &'a str, name: &str) -> Vec<&'a str> {
    let Some(rest) = after(block, &format!("\"{name}\":[")) else { return Vec::new() };
    rest[..rest.find(']').unwrap_or(rest.len())].split(',').map(str::trim).collect()
}

/// The current block, then today's entry of each daily list -- and for the
/// sunrise tomorrow's as well.  The sun's times are the place's own clock,
/// and become moments with the offset Open-Meteo gives for it.
fn parse_forecast(json: &str, report: &mut Report) -> bool {
    let Some(current) = block(json, "\"current\":{") else { return false };
    report.temp = number(current, "temperature_2m");
    report.feels = number(current, "apparent_temperature");
    report.is_day = number(current, "is_day").is_none_or(|v| v != 0.0);
    report.code = number(current, "weather_code").map(|c| c as i32);
    report.wind = number(current, "wind_speed_10m");
    report.wind_dir = number(current, "wind_direction_10m");
    report.uv = number(current, "uv_index");
    report.offset = until(after(json, "\"utc_offset_seconds\":").unwrap_or(""), &[',', '}']).parse().unwrap_or(0);
    if let Some(daily) = block(json, "\"daily\":{") {
        let first = |name: &str| list(daily, name).first().and_then(|v| v.parse::<f64>().ok());
        report.day_code = first("weather_code").map(|c| c as i32);
        report.tmax = first("temperature_2m_max");
        report.tmin = first("temperature_2m_min");
        report.precip = first("precipitation_probability_max");
        let moment = |v: &&str| clock::parse_iso_utc(v.trim_matches('"')).map(|t| t - report.offset);
        let rises: Vec<i64> = list(daily, "sunrise").iter().filter_map(moment).collect();
        let sets: Vec<i64> = list(daily, "sunset").iter().filter_map(moment).collect();
        report.sunrise = rises.first().copied();
        report.sunrise2 = rises.get(1).copied();
        report.sunset = sets.first().copied();
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
        _ => match http::get("ip-api.com", 80, false, "/json/").as_deref().and_then(parse_place) {
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
/// written to the cache whether or not it came.  The wind is asked for in
/// the unit that goes with the temperature's: mph with Fahrenheit.
fn forecast(mut report: Report, cache: &Path) -> i32 {
    let wind = if report.fahrenheit() { "mph" } else { "kmh" };
    let path = format!(
        "/v1/forecast?latitude={}&longitude={}\
         &current=temperature_2m,apparent_temperature,is_day,weather_code,wind_speed_10m,wind_direction_10m,uv_index\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset\
         &temperature_unit={}&wind_speed_unit={wind}&timezone=auto&forecast_days=2",
        report.lat, report.lon, report.unit
    );
    let ok = http::get("api.open-meteo.com", 443, true, &path).is_some_and(|json| parse_forecast(&json, &mut report));
    if !ok {
        report.error = String::from("forecast");
    }
    report.write(cache);
    if ok { 0 } else { 1 }
}


pub fn find_main(args: &[String]) -> i32 {
    let (Some(cache), Some(unit), Some(typed)) = (args.first(), args.get(1), args.get(2)) else { return 2 };
    let place = match geocode::find(typed) {
        Ok(place) => place,
        Err(Miss::NoAnswer) => return EXIT_NO_GEOCODER,
        Err(Miss::NoCity) => return EXIT_NO_CITY,
    };
    let report = Report {
        fetched: crate::clock::now(), unit: unit.clone(), is_day: true,
        city: place.label(), lat: place.lat, lon: place.lon, ..Report::default()
    };
    forecast(report, Path::new(cache))
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Open-Meteo's answer for London one November morning, as it came.
    const LONDON: &str = r#"{"latitude":51.51147,"longitude":-0.13078308,"generationtime_ms":0.40,"utc_offset_seconds":0,"timezone":"Europe/London","timezone_abbreviation":"GMT","elevation":29.0,"current_units":{"time":"iso8601","interval":"seconds","temperature_2m":"°C","apparent_temperature":"°C","is_day":"","weather_code":"wmo code","wind_speed_10m":"km/h","wind_direction_10m":"°","uv_index":""},"current":{"time":"2026-11-22T10:45","interval":900,"temperature_2m":7.9,"apparent_temperature":4.1,"is_day":1,"weather_code":1,"wind_speed_10m":13.7,"wind_direction_10m":92,"uv_index":0.50},"daily_units":{"time":"iso8601","weather_code":"wmo code","temperature_2m_max":"°C","temperature_2m_min":"°C","precipitation_probability_max":"%","sunrise":"iso8601","sunset":"iso8601"},"daily":{"time":["2026-11-22","2026-11-23"],"weather_code":[3,51],"temperature_2m_max":[8.9,7.0],"temperature_2m_min":[5.3,3.7],"precipitation_probability_max":[0,4],"sunrise":["2026-11-22T07:30","2026-11-23T07:31"],"sunset":["2026-11-22T16:02","2026-11-23T16:01"]}}"#;

    fn weather(settings: &mut Settings, look: Look) -> Weather {
        let now = clock::parse_iso_utc("2026-11-22T10:45").unwrap();
        let mut w = Weather::new_in(&Ctx { now, settings, locked: false, signed_in: None }, look);
        w.frozen = true;
        w
    }

    fn london() -> Report {
        let mut r = Report { unit: String::from("celsius"), city: String::from("London, GB"), ..Report::default() };
        assert!(parse_forecast(LONDON, &mut r));
        r
    }

    #[test]
    fn reads_the_place() {
        let json = r#"{"status":"success","country":"X","city":"Somewhere","lat":12.5,"lon":-3.25,"timezone":"Z"}"#;
        assert_eq!(parse_place(json), Some(("Somewhere".into(), "12.5".into(), "-3.25".into())));
        assert_eq!(parse_place("{}"), None);
    }

    #[test]
    fn reads_the_forecast_past_the_units_blocks() {
        let r = london();
        assert_eq!((r.temp, r.code, r.is_day, r.tmax, r.tmin), (Some(7.9), Some(1), true, Some(8.9), Some(5.3)));
        assert_eq!((r.feels, r.wind, r.wind_dir, r.uv), (Some(4.1), Some(13.7), Some(92.0), Some(0.5)));
        assert_eq!((r.precip, r.day_code, r.offset), (Some(0.0), Some(3), 0));
        let at = |s: &str| clock::parse_iso_utc(s);
        assert_eq!((r.sunrise, r.sunset, r.sunrise2), (at("2026-11-22T07:30"), at("2026-11-22T16:02"), at("2026-11-23T07:31")));

        // a place an hour ahead of UTC: the same clock times are an hour
        // earlier as moments; and what is null is not known
        let ahead = LONDON.replace("\"utc_offset_seconds\":0", "\"utc_offset_seconds\":3600").replace("\"uv_index\":0.50", "\"uv_index\":null");
        let mut r = Report::default();
        assert!(parse_forecast(&ahead, &mut r));
        assert_eq!((r.offset, r.uv), (3600, None));
        assert_eq!(r.sunrise, at("2026-11-22T06:30"));
        assert!(!parse_forecast("{}", &mut Report::default()));
    }

    #[test]
    fn the_cache_keeps_all_of_it() {
        let dir = std::env::temp_dir().join(format!("win98-weather-test-{}", std::process::id()));
        let path = dir.join("weather.txt");
        let r = london();
        r.write(&path);
        let back = Report::parse(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(format!("{r:?}"), format!("{back:?}"));
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn compass_and_uv() {
        assert_eq!([0.0, 44.0, 92.0, 200.0, 250.0, 338.0, 359.0].map(compass), ["N", "NE", "E", "S", "W", "N", "N"]);
        assert_eq!([2.4, 3.0, 6.0, 9.0, 11.0].map(uv_word), ["low", "moderate", "high", "very high", "extreme"]);
        assert_eq!(clock_at(clock::parse_iso_utc("2026-11-22T16:02").unwrap(), 3600, true), "17:02");
        assert_eq!(clock_at(clock::parse_iso_utc("2026-11-22T16:02").unwrap(), 0, false), "4:02 PM");
        assert_eq!(clock_at(clock::parse_iso_utc("2026-11-22T00:05").unwrap(), 0, false), "12:05 AM");
    }

    #[test]
    fn the_window_says_what_matters_today_and_nothing_else() {
        let mut settings = Settings::in_memory();
        settings.set_int("DateTime", "Use24Hour", 1);
        let mut w = weather(&mut settings, Look::Window);
        // a calm, dry London morning: the high and low, and the sunset --
        // feels like 4 against 8 is worth saying; a 14 km/h breeze, UV 0.5
        // and no chance of rain are not
        w.show(&london());
        assert_eq!(w.lines, ["High 9\u{b0}  Low 5\u{b0}", "Feels like 4\u{b0}", "Sunset 16:02"]);

        // a wet, windy, sunny afternoon with snow coming
        let mut r = london();
        (r.precip, r.day_code, r.feels, r.wind, r.wind_dir, r.uv) = (Some(70.0), Some(73), Some(7.0), Some(31.0), Some(225.0), Some(6.2));
        w.show(&r);
        assert_eq!(w.lines, ["High 9\u{b0}  Low 5\u{b0}", "Snow 70%", "Wind 31 km/h SW", "UV 6, high", "Sunset 16:02"]);

        // after sunset, tomorrow's sunrise; in mph and on the 12-hour clock
        w.now = r.sunset.unwrap() + 60;
        w.use24 = false;
        r.unit = String::from("fahrenheit");
        r.wind = Some(13.0);
        w.show(&r);
        assert_eq!(w.lines.last().unwrap(), "Sunrise 7:31 AM");
        assert!(w.lines.contains(&String::from("Wind 13 mph SW")));
        // and the UV is not said by night
        r.is_day = false;
        w.show(&r);
        assert!(!w.lines.iter().any(|l| l.starts_with("UV")));
    }

    #[test]
    fn the_scene_follows_the_weather_unless_it_is_kept_to_one() {
        let mut settings = Settings::in_memory();
        let mut w = weather(&mut settings, Look::Window);
        // nothing known: a grey sky
        assert_eq!(w.sky.as_ref().unwrap().scene(), Scene::Overcast);
        let mut r = london();
        r.code = Some(65);
        w.show(&r);
        assert_eq!(w.sky.as_ref().unwrap().scene(), Scene::Downpour);

        let mut ctx = Ctx { now: w.now, settings: &mut settings, locked: false, signed_in: None };
        w.menu_command(MENU_SCENE + 8, &mut ctx);
        assert_eq!(w.sky.as_ref().unwrap().scene(), Scene::Snow);
        assert_eq!(ctx.settings.get(SECTION, "Scene"), Some("snow"));
        // it stays kept to it as the weather changes
        r.code = Some(0);
        w.show(&r);
        assert_eq!(w.sky.as_ref().unwrap().scene(), Scene::Snow);
        // and comes back to it after a restart
        let again = Weather::new_in(&ctx, Look::Window);
        assert_eq!(again.pinned, Some(Scene::Snow));
        w.menu_command(MENU_FOLLOW, &mut ctx);
        assert_eq!(w.sky.as_ref().unwrap().scene(), Scene::Clear);
        assert_eq!(ctx.settings.get(SECTION, "Scene"), Some(""));
    }

    #[test]
    fn the_scenery_is_the_city_the_forecast_is_for_unless_kept_to_another() {
        let mut settings = Settings::in_memory();
        let mut w = weather(&mut settings, Look::Window);
        let mut r = london();
        (r.lat, r.lon) = (String::from("51.5085"), String::from("-0.1257"));
        w.show(&r);
        assert_eq!(w.city(), Some(City::London));
        // somewhere with no place of its own: the weather's
        (r.lat, r.lon) = (String::from("50.4501"), String::from("30.5234"));
        w.show(&r);
        assert_eq!(w.city(), None);

        let mut ctx = Ctx { now: w.now, settings: &mut settings, locked: false, signed_in: None };
        w.menu_command(MENU_PLACE_CITY + 1, &mut ctx);
        assert_eq!(w.city(), Some(City::Aberdeen));
        assert_eq!(ctx.settings.get(SECTION, "Scenery"), Some("aberdeen"));
        assert_eq!(Weather::new_in(&ctx, Look::Window).scenery, Scenery::City(City::Aberdeen));
        (r.lat, r.lon) = (String::from("51.5085"), String::from("-0.1257"));
        w.show(&r);
        assert_eq!(w.city(), Some(City::Aberdeen), "kept to Aberdeen, wherever the forecast is for");
        w.menu_command(MENU_PLACE_OWN, &mut ctx);
        assert_eq!(w.city(), None);
        assert_eq!(ctx.settings.get(SECTION, "Scenery"), Some("weather"));
        w.menu_command(MENU_PLACE_AUTO, &mut ctx);
        assert_eq!(w.city(), Some(City::London));
        assert_eq!(ctx.settings.get(SECTION, "Scenery"), Some(""));
        for s in [Scenery::Auto, Scenery::Own, Scenery::City(City::London), Scenery::City(City::Aberdeen)] {
            assert_eq!(Scenery::of(s.key()), s);
        }
    }

    #[test]
    fn the_other_look_is_another_window_and_only_the_window_moves() {
        let mut settings = Settings::in_memory();
        let panel = weather(&mut settings, Look::Panel);
        assert!(core::ptr::eq(panel.skin(), PANEL) && panel.sky.is_none() && panel.frame_ms() == 0);
        assert!(core::ptr::eq(skin_of(&settings, SECTION), PANEL));
        let mut ctx = Ctx { now: 0, settings: &mut settings, locked: false, signed_in: None };
        let mut panel = panel;
        assert!(matches!(panel.menu_command(MENU_WINDOW, &mut ctx).request, Request::Reshape));
        assert!(core::ptr::eq(skin_of(ctx.settings, SECTION), WINDOW));
        // the look it is in already asks for nothing
        assert!(matches!(panel.menu_command(MENU_PANEL, &mut ctx).request, Request::None));

        let mut window = Weather::new_in(&ctx, Look::Window);
        assert!(core::ptr::eq(window.skin(), WINDOW));
        assert!(window.frame_ms() > 0);
        assert_eq!(window.frame(40), Dirty::Part(window.pane()));
        // paused, it asks for no frames, and remembers that
        window.menu_command(MENU_PAUSE, &mut ctx);
        assert_eq!((window.frame_ms(), window.frame(40)), (0, Dirty::None));
        assert!(ctx.settings.flag(SECTION, "Paused", false));
        // and the window's menu has the animation in it, the panel's not
        let labels = |w: &Weather| {
            let mut items = Vec::new();
            w.menu(&mut items);
            items.iter().map(|i| i.label).collect::<Vec<_>>()
        };
        assert!(labels(&window).contains(&"Animation") && labels(&window).contains(&"Thunderstorm"));
        assert!(!labels(&panel).contains(&"Animation"));
    }

    #[test]
    fn the_place_is_typed_in_the_window_title_after_set_city() {
        let mut settings = Settings::in_memory();
        let mut w = weather(&mut settings, Look::Window);
        w.show(&london());
        assert_eq!(w.title(), "London, GB");
        let field = w.field.rect;
        // a press on the title moves the window, as it always has
        assert!(matches!(w.press(field.x0 + 4, field.y0 + 4, false, false), Pressed::No));
        assert_eq!(w.num("TitleEdit"), 0.0);
        let mut ctx = Ctx { now: 0, settings: &mut settings, locked: false, signed_in: None };
        let outcome = w.menu_command(MENU_CITY, &mut ctx);
        assert!(matches!(outcome.request, Request::TakeKeys));
        assert_eq!(w.num("TitleEdit"), 1.0);
        assert_eq!(w.field.text(), "London, GB");
        // and now a press there puts the caret down
        assert!(matches!(w.press(field.x0 + 4, field.y0 + 4, false, false), Pressed::Taken(_)));
        w.key(Key::Escape, false, false);
        assert_eq!(w.blur(&mut ctx), Dirty::Part(w.title_box()));
        assert_eq!(w.num("TitleEdit"), 0.0);
    }

    #[test]
    fn the_readings_are_drawn_over_the_scene_once_and_again_when_they_change() {
        let mut settings = Settings::in_memory();
        let mut w = weather(&mut settings, Look::Window);
        w.show(&london());
        let mut text = Text::new(WINDOW.var("Font"), skin::FONTS).unwrap();
        let mut px = vec![0u32; (WINDOW.w * WINDOW.h) as usize];
        let mut picture = |w: &Weather, text: &mut Text| {
            px.fill(0);
            // SAFETY: the canvas is exactly the vector.
            let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), WINDOW.w, WINDOW.w, WINDOW.h) };
            skin::draw(WINDOW, w, &mut canvas, text, &mut Vec::new());
            drop(canvas);
            px.clone()
        };
        let first = picture(&w, &mut text);
        let key = w.overlay.borrow().key.clone();
        assert!(key.contains("Sunset 16:02"));
        // something of the readings is on it, and every pixel of them is the
        // ink or the shadow
        let ink = [w.colour("ReadInk"), w.colour("ReadShadow")];
        assert!(w.overlay.borrow().px.iter().filter(|&&p| p != 0).all(|p| ink.contains(p)));
        assert!(w.overlay.borrow().px.iter().filter(|&&p| p != 0).count() > 200);
        // the same again: laid out as it was, and the same picture
        assert!(picture(&w, &mut text) == first);
        assert_eq!(w.overlay.borrow().key, key);
        let mut r = london();
        r.temp = Some(-3.0);
        w.show(&r);
        assert!(picture(&w, &mut text) != first);
        assert_ne!(w.overlay.borrow().key, key);
    }
}
