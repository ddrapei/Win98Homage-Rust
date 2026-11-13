//! Today's weather: temperature, condition, place and the day's high and low.
//! Two lookups feed it: the public IP's location from ip-api.com (once a day,
//! or never if a place is pinned in settings.ini), then the forecast from
//! Open-Meteo every half hour.
//!
//! The place is pinned by typing a city into the widget's city field (the
//! box is `entry`).  The name goes to Open-Meteo's geocoder, and what comes
//! back is kept in settings.ini as ManualLat, ManualLon and ManualLocation --
//! the same three keys that can be written there by hand.
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

use crate::canvas::Canvas;
use crate::settings::{data_dir, write_whole};
use crate::skin::{self, Hooks, Meter, Skin, ICONS};
use crate::sys;
use crate::text::Text;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Request, Widget};

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

pub struct Weather {
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

    /// What the city box opens holding: the pinned place by name, or nothing
    /// while the place is the one the IP address gives.
    pub fn city_text(&self) -> &str {
        if self.manual.is_some() { &self.manual_label } else { "" }
    }

    /// A name typed into the city field.  It is looked up before anything
    /// changes, so a name that is not a place leaves the forecast as it was;
    /// an empty one goes back to the place the IP address gives.
    pub fn set_city(&mut self, typed: &str, ctx: &mut Ctx) -> Dirty {
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
            // The city field: what became of the name just typed, while that
            // is news; the lookup, while it runs; otherwise the place.
            "LocationLabel" => match (&self.notice, &self.finding) {
                (Some((text, _)), _) => text,
                (None, Some(finding)) => finding,
                (None, None) => &self.location,
            },
            _ => "",
        });
    }

    /// [MeterIconWeather]: the condition glyph, one of the PNGs from Icons.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        if meter.name == "MeterIconWeather" {
            if let Some(&(_, w, h, bgra)) = self.icon.and_then(|i| ICONS.get(i)) {
                canvas.image(x, y, w, h, bgra);
            }
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
            "EditCity" if !self.frozen => Outcome::request(Request::EditCity),
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
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
            3 => return Outcome::request(Request::EditCity),
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
