//! Today's weather: temperature, condition, place and the day's high and low.
//! Two lookups feed it: the public IP's location from ip-api.com (once a day,
//! or never if a place is pinned in settings.ini), then the forecast from
//! Open-Meteo every half hour.
//!
//! The network is kept out of this process altogether.  A fetch is the
//! program run again as `--fetch-weather`, which loads WinHTTP, writes a few
//! lines to a cache file and exits -- so the DLLs, sockets and TLS state a
//! request drags in live for a second every half hour instead of for good.
//! The cache also means a restart shows the last forecast at once.

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
            self.high_low = String::from("Set ManualLat/Lon");
            self.location = String::from("ip-api.com did not answer");
        } else {
            self.condition = String::from("No forecast");
            self.high_low = String::from("Open-Meteo unreachable");
        }
    }

    fn start_fetch(&mut self, now: i64) {
        let Ok(exe) = std::env::current_exe() else { return };
        let mut command = Command::new(exe);
        command.arg("--fetch-weather").arg(&self.cache).arg(&self.unit);
        match &self.manual {
            Some((lat, lon)) => {
                command.args([lat.as_str(), lon.as_str(), "", "0"]);
            }
            // where the machine was last found, while that is still fresh
            None if !self.last.lat.is_empty() && now - self.last.located < RELOCATE => {
                command.args([self.last.lat.as_str(), self.last.lon.as_str(), self.last.city.as_str()]);
                command.arg(self.last.located.to_string());
            }
            None => {}
        }
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Ok(child) = command.creation_flags(CREATE_NO_WINDOW).spawn() {
            self.child = Some((child, now));
        }
    }

    fn finish_fetch(&mut self, now: i64) -> Dirty {
        let report = std::fs::read_to_string(&self.cache).map(|t| Report::parse(&t)).unwrap_or_default();
        let fresh = report.fetched >= now - FETCH_LIMIT - 5 && report.temp.is_some() && report.error.is_empty();
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
            "LocationLabel" => &self.location,
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
         ip-api.com altogether -- set ManualLat and ManualLon in settings.ini."
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        if self.frozen {
            return Dirty::None;
        }
        let now = ctx.now;
        if let Some((child, started)) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    return self.finish_fetch(now);
                }
                Ok(None) if now - *started <= FETCH_LIMIT => {}
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    self.child = None;
                    self.next_fetch = now + RETRY;
                }
            }
        } else if now >= self.next_fetch {
            self.next_fetch = now + RETRY; // if it cannot even be started
            self.start_fetch(now);
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

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { id: 1, label: "Refresh now", checked: false });
        items.push(MenuItem { id: 2, label: "Fahrenheit", checked: self.unit == "fahrenheit" });
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
            _ => {}
        }
        Outcome::none()
    }

    fn shutdown(&mut self, _ctx: &mut Ctx) {
        if let Some((mut child, _)) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ---------------------------------------------------------------------------
//  The fetch: this program run as `--fetch-weather <cache> <unit> [lat lon city located]`
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

    let path = format!(
        "/v1/forecast?latitude={}&longitude={}&current_weather=true&daily=weathercode,temperature_2m_max,temperature_2m_min&temperature_unit={}&timezone=auto",
        report.lat, report.lon, report.unit
    );
    let ok = http_get("api.open-meteo.com", 443, true, &path).is_some_and(|json| parse_forecast(&json, &mut report));
    if !ok {
        report.error = String::from("forecast");
    }
    report.write(Path::new(cache));
    if ok { 0 } else { 1 }
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
    fn conditions_and_icons() {
        assert_eq!(condition(Some(3)), "Overcast");
        assert_eq!(condition(None), "Unknown");
        assert_eq!(icon_for(0, false), "weather-clear-night");
        assert_eq!(icon_for(45, true), "weather-overcast");
        assert_eq!(icon_for(99, true), "weather-severe-alert");
        assert_eq!(icon_for(95, true), "weather-storm");
    }
}
