//! World Clock: three clocks side by side, each keeping the time of a city --
//! its hands, the time in figures, and on its box whether it is today there
//! and how far ahead of here it is.
//!
//! A city is typed into the field under its clock, exactly as the weather's
//! is (`field`).  The name goes to Open-Meteo's geocoder, which answers with
//! the place and the time zone it keeps by its tz database name
//! ("Asia/Tokyo"); ICU, which Windows carries, turns that into the name
//! Windows keeps the zone's rules under ("Tokyo Standard Time"), and that is
//! what is kept in settings.ini beside the city.  From then on the time
//! there is Windows' own arithmetic, with nothing asked of anybody: summer
//! time comes and goes in each city as its rules say.
//!
//! Neither the network nor ICU is ever loaded by this process (see `net`).
//! Looking a city up is the program run again as `--find-zone`, which asks,
//! writes two lines to a file and exits.
//!
//! The clocks have no second hands, so there is something new to draw once a
//! minute, and nothing is drawn in between.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use crate::canvas::Canvas;
use crate::clock::{self, Elsewhere, Tm};
use crate::field::Field;
use crate::geocode::{self, Miss};
use crate::keys::Key;
use crate::settings::{data_dir, write_whole};
use crate::skin::{self, index_of, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, Outcome, Pressed, Request, Widget};

static SKIN: &Skin = &skin::world::SKIN;
const SECTION: &str = "World";

/// How many clocks the skin has.
const CLOCKS: usize = 3;
/// A lookup still running after this many seconds is given up on.
const FIND_LIMIT: i64 = 60;
/// Ticks a city field spends saying what became of a name typed into it.
const NOTICE: u32 = 5;

/// How `--find-zone` ends when it has no answer to write: the name matched
/// nothing, the geocoder could not be asked, or the place keeps a time
/// Windows has no zone for.
const EXIT_NO_CITY: i32 = 3;
const EXIT_NO_GEOCODER: i32 = 4;
const EXIT_NO_ZONE: i32 = 5;

/// One clock.
struct Face {
    field: Field,
    /// What its field shows; nothing for a clock that has been put away.
    city: String,
    /// The time zone the city keeps, if Windows knows it.
    zone: Option<Elsewhere>,
    /// "Today, +7 hrs"
    legend: String,
    /// "21:15"
    time: String,
    /// Degrees clockwise from twelve: the hour hand, the minute hand.
    angles: (i32, i32),
    /// What became of the last name typed here, and the ticks it has left to show.
    notice: Option<(&'static str, u32)>,
}

pub struct World {
    faces: [Face; CLOCKS],
    /// The clock whose field has the keyboard.
    typing: Option<usize>,
    /// The child that is looking a city up, and when it was started.
    child: Option<(Child, i64)>,
    /// Which clock it is for, and what that clock's field says meanwhile.
    finding: Option<(usize, String)>,
    /// The file the child leaves its answer in.
    answer: PathBuf,
    /// The minute the clocks were last worked out for.
    minute: i64,
    use24: bool,
    /// For the test harness: never look anything up.
    frozen: bool,
}

/// Where the hands point: the hour hand moves with the minutes, as on the
/// Date/Time clock; the minute hand stands on its minute.
fn hands(t: &Tm) -> (i32, i32) {
    (((t.hour % 12) as f64 * 30.0 + t.min as f64 * 0.5 + 0.5).floor() as i32, t.min * 6)
}

/// How far one clock is ahead of another: "+7 hrs", "-1 hr", "+5:30".
fn apart(minutes: i32) -> String {
    let (sign, minutes) = (if minutes < 0 { '-' } else { '+' }, minutes.abs());
    match (minutes / 60, minutes % 60) {
        (1, 0) => format!("{sign}1 hr"),
        (hours, 0) => format!("{sign}{hours} hrs"),
        (hours, odd) => format!("{sign}{hours}:{odd:02}"),
    }
}

/// What a clock's box says: which day it is `there`, as seen from `here` at
/// the same moment, and how far ahead of here that clock is.
fn legend(here: &Tm, there: &Tm) -> String {
    let days = clock::days_from_civil(there.year, there.month, there.day) - clock::days_from_civil(here.year, here.month, here.day);
    let day = match days {
        0 => "Today",
        d if d > 0 => "Tomorrow",
        _ => "Yesterday",
    };
    format!("{day}, {}", apart(((clock::reading(there) - clock::reading(here)) as f64 / 60.0).round() as i32))
}

/// The time in figures, to the minute.
fn figures(t: &Tm, use24: bool) -> String {
    if use24 {
        format!("{:02}:{:02}", t.hour, t.min)
    } else {
        let hour = if t.hour % 12 == 0 { 12 } else { t.hour % 12 };
        format!("{}:{:02} {}", hour, t.min, if t.hour < 12 { "AM" } else { "PM" })
    }
}

/// The 12 or the 24 hour clock: whichever the Date/Time widget is set to.
fn use24(ctx: &Ctx) -> bool {
    ctx.settings.flag("DateTime", "Use24Hour", skin::datetime::SKIN.int("Use24Hour", 1) != 0)
}

impl World {
    pub fn new(ctx: &Ctx) -> World {
        let settings = &*ctx.settings;
        let faces = core::array::from_fn(|i| {
            let get = |key: &str| {
                let key = format!("{key}{}", i + 1);
                settings.text(SECTION, &key, SKIN.var(&key)).trim().to_owned()
            };
            Face {
                field: Field::new(SKIN, &format!("MeterCity{}", i + 1)),
                city: get("City"),
                zone: clock::elsewhere(&get("Zone")),
                legend: String::new(),
                time: String::new(),
                angles: (0, 0),
                notice: None,
            }
        });
        let mut world = World {
            faces,
            typing: None,
            child: None,
            finding: None,
            answer: data_dir().join("zone.txt"),
            minute: ctx.now.div_euclid(60),
            use24: use24(ctx),
            frozen: false,
        };
        world.render(ctx.now);
        world
    }

    /// For the test harness: the clocks as settings.ini has them, and no
    /// lookups.
    pub fn set_fixed(&mut self) {
        self.frozen = true;
    }

    /// Work every clock out for this moment.  Says whether any of them
    /// shows anything it did not.
    fn render(&mut self, now: i64) -> bool {
        let here = clock::local(now);
        let mut changed = false;
        for face in &mut self.faces {
            let (angles, legend, time) = match &face.zone {
                Some(zone) => {
                    let there = zone.local(now);
                    (hands(&there), legend(&here, &there), figures(&there, self.use24))
                }
                // a bare ring, and why
                None => ((0, 0), String::from(if face.city.is_empty() { "No city" } else { "No time zone" }), String::new()),
            };
            if (angles, &legend, &time) != (face.angles, &face.legend, &face.time) {
                (face.angles, face.legend, face.time) = (angles, legend, time);
                changed = true;
            }
        }
        changed
    }

    /// What a city field says while nobody is typing in it: what became of
    /// the name just typed, while that is news; the lookup, while it runs;
    /// otherwise the city -- or, with none, what to do about that.
    fn label(&self, i: usize) -> &str {
        let face = &self.faces[i];
        match (&face.notice, &self.finding) {
            (Some((text, _)), _) => text,
            (None, Some((at, text))) if *at == i => text,
            _ if face.city.is_empty() => "Type a city",
            _ => &face.city,
        }
    }

    /// Clock `i` is this city's from now on, and next time.
    fn set(&mut self, i: usize, city: &str, key: &str, ctx: &mut Ctx) {
        self.faces[i].city = city.to_owned();
        self.faces[i].zone = clock::elsewhere(key);
        ctx.settings.set(SECTION, &format!("City{}", i + 1), city);
        ctx.settings.set(SECTION, &format!("Zone{}", i + 1), key);
        self.render(ctx.now);
    }

    fn stop_child(&mut self) {
        if let Some((mut child, _)) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.finding = None;
    }

    /// This program again, as the child that looks a city up.
    fn launch(&mut self, typed: &str, now: i64) -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        // an answer left by an earlier child is not this one's
        let _ = std::fs::remove_file(&self.answer);
        let mut command = Command::new(exe);
        command.arg("--find-zone").arg(&self.answer).arg(typed);
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        match command.creation_flags(CREATE_NO_WINDOW).spawn() {
            Ok(child) => {
                self.child = Some((child, now));
                true
            }
            Err(_) => false,
        }
    }

    /// A name typed into clock `i`'s field.  It is looked up before anything
    /// changes, so a name that is not a place leaves the clock as it was; an
    /// empty one puts the clock away.
    fn set_city(&mut self, i: usize, typed: &str, ctx: &mut Ctx) -> Dirty {
        let typed = typed.trim();
        if typed.is_empty() {
            if self.finding.as_ref().is_some_and(|(at, _)| *at == i) {
                self.stop_child();
            }
            self.faces[i].notice = None;
            self.set(i, "", "", ctx);
            return Dirty::All;
        }
        if self.frozen {
            return Dirty::None;
        }
        // one lookup at a time: the latest name typed is the one that counts
        self.stop_child();
        if self.launch(typed, ctx.now) {
            self.finding = Some((i, format!("Finding {typed}...")));
            self.faces[i].notice = None;
        } else {
            self.faces[i].notice = Some(("Lookup failed", NOTICE));
        }
        Dirty::All
    }

    /// The child has exited; `code` is how.
    fn finish(&mut self, code: Option<i32>, ctx: &mut Ctx) -> Dirty {
        let Some((i, _)) = self.finding.take() else { return Dirty::None };
        let failed = match code {
            Some(0) => {
                let text = std::fs::read_to_string(&self.answer).unwrap_or_default();
                let value = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).unwrap_or("").trim().to_owned();
                let (city, key) = (value("city"), value("zone"));
                // the name Windows was asked for is one Windows should know
                if !city.is_empty() && clock::elsewhere(&key).is_some() {
                    self.set(i, &city, &key, ctx);
                    None
                } else {
                    Some("Lookup failed")
                }
            }
            Some(EXIT_NO_CITY) => Some("No such city"),
            Some(EXIT_NO_ZONE) => Some("No time zone"),
            _ => Some("Lookup failed"),
        };
        let _ = std::fs::remove_file(&self.answer);
        self.faces[i].notice = failed.map(|text| (text, NOTICE));
        Dirty::All
    }

    /// The field that is being typed in.
    fn typed_on(&mut self) -> Option<&mut Field> {
        self.typing.map(|i| &mut self.faces[i].field)
    }
}

impl Hooks for World {
    fn num(&self, var: &str) -> f64 {
        let face = |prefix: &str| index_of(var, prefix).and_then(|n| self.faces.get(n.wrapping_sub(1)));
        if let Some(face) = face("AngleHour") {
            face.angles.0 as f64
        } else if let Some(face) = face("AngleMin") {
            face.angles.1 as f64
        } else if let Some(face) = face("NoZone") {
            face.zone.is_none() as i32 as f64
        } else {
            0.0
        }
    }

    fn text(&self, var: &str, out: &mut String) {
        let face = |prefix: &str| index_of(var, prefix).and_then(|n| self.faces.get(n.wrapping_sub(1)));
        if let Some(face) = face("Leg") {
            out.push_str(&face.legend);
        } else if let Some(face) = face("Time") {
            out.push_str(&face.time);
        }
    }

    /// [MeterCity1..3]: a city in its field, typed on or not.
    fn custom(&self, meter: &Meter, _x: i32, _y: i32, canvas: &mut Canvas, text: &mut Text) {
        if let Some(i) = index_of(meter.name, "MeterCity").and_then(|n| n.checked_sub(1)).filter(|&i| i < CLOCKS) {
            self.faces[i].field.draw(self.label(i), canvas, text);
        }
    }
}

impl Widget for World {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 World Clock\n\n\
         The time in three cities.  Each box says whether it is today there, \
         and how far ahead of here its clock is.\n\n\
         To change a city, click its name and type another: \"Lviv\", or \
         \"Paris, US\" or \"Portland, Maine\" to say which one.  Enter takes \
         it, Escape does not, and an empty field puts that clock away.\n\n\
         The city is looked up once, with Open-Meteo; after that its time is \
         worked out here, summer time and all.  The figures follow the 12 or \
         24 hour choice of the Date/Time widget."
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        if self.frozen {
            return Dirty::None;
        }
        let now = ctx.now;
        let mut dirty = Dirty::None;
        for face in &mut self.faces {
            if let Some((text, left)) = face.notice {
                face.notice = (left > 1).then_some((text, left - 1));
                if face.notice.is_none() {
                    dirty = Dirty::All;
                }
            }
        }
        if let Some((child, started)) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    dirty = dirty.and(self.finish(status.code(), ctx));
                }
                Ok(None) if now - *started <= FIND_LIMIT => {}
                _ => {
                    let at = self.finding.as_ref().map(|(at, _)| *at);
                    self.stop_child();
                    if let Some(i) = at {
                        self.faces[i].notice = Some(("Lookup failed", NOTICE));
                        dirty = Dirty::All;
                    }
                }
            }
        }
        // on the minute, or when the Date/Time widget has changed its mind
        // about the 12 and the 24 hour clock
        let (minute, use24) = (now.div_euclid(60), use24(ctx));
        if minute != self.minute || use24 != self.use24 {
            (self.minute, self.use24) = (minute, use24);
            if self.render(now) {
                dirty = Dirty::All;
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

    /// A press in a city field starts the typing there, with the name that
    /// is in it all selected; once it has the keyboard a press puts the caret
    /// down, and held, it selects.  Anywhere else is the host's.
    fn press(&mut self, x: i32, y: i32, double: bool, shift: bool) -> Pressed {
        let Some(i) = self.faces.iter().position(|f| f.field.reach().contains(x, y)) else { return Pressed::No };
        if self.faces[i].field.editing {
            return Pressed::Taken(self.faces[i].field.press(x, double, shift));
        }
        // The host has ended any typing in another of the fields by now; if
        // it has not, what was typed there goes with it.
        if let Some(other) = self.typing.replace(i) {
            self.faces[other].field.end();
        }
        // whatever the field was saying about the last name, it has been read
        self.faces[i].notice = None;
        let city = self.faces[i].city.clone();
        Pressed::Taken(self.faces[i].field.begin(&city))
    }

    fn drag(&mut self, x: i32, _y: i32) -> Dirty {
        self.typed_on().map_or(Dirty::None, |field| field.drag(x))
    }

    fn ibeam(&self, x: i32, y: i32) -> bool {
        self.faces.iter().any(|f| f.field.rect.contains(x, y))
    }

    // ---- the keyboard -----------------------------------------------------------

    /// Only the field that is being typed in keeps the typing going: a press
    /// in another of the three ends it, and starts that one's.
    fn typing_at(&self, x: i32, y: i32) -> bool {
        self.typing.is_some_and(|i| self.faces[i].field.reach().contains(x, y))
    }

    fn typed(&mut self, c: char) -> Outcome {
        self.typed_on().map_or_else(Outcome::none, |field| field.typed(c))
    }

    fn key(&mut self, key: Key, shift: bool, ctrl: bool) -> Outcome {
        self.typed_on().map_or_else(Outcome::none, |field| field.key(key, shift, ctrl))
    }

    fn copy(&self) -> Option<String> {
        self.typing.and_then(|i| self.faces[i].field.copy())
    }

    fn cut(&mut self) -> (Option<String>, Outcome) {
        self.typed_on().map_or_else(|| (None, Outcome::none()), |field| field.cut())
    }

    fn paste(&mut self, text: &str) -> Outcome {
        self.typed_on().map_or_else(Outcome::none, |field| field.paste(text))
    }

    fn pulse(&mut self, _ms: u32, blink: bool) -> Dirty {
        self.typed_on().map_or(Dirty::None, |field| field.pulse(blink))
    }

    /// The keyboard has gone elsewhere: the field goes back to naming its
    /// city, and what was typed in it -- if that is an answer, and not the
    /// city the clock has already -- is looked up.
    fn blur(&mut self, ctx: &mut Ctx) -> Dirty {
        let Some(i) = self.typing.take() else { return Dirty::None };
        let field = Dirty::Part(self.faces[i].field.rect);
        match self.faces[i].field.end() {
            Some(typed) if typed.is_empty() || typed != self.faces[i].city => field.and(self.set_city(i, &typed, ctx)),
            _ => field,
        }
    }

    fn shutdown(&mut self, _ctx: &mut Ctx) {
        self.stop_child();
    }
}

// ---------------------------------------------------------------------------
//  The lookup: this program run as `--find-zone <answer> <name>`
// ---------------------------------------------------------------------------

pub fn find_main(args: &[String]) -> i32 {
    let (Some(answer), Some(typed)) = (args.first(), args.get(1)) else { return 2 };
    let place = match geocode::find(typed) {
        Ok(place) => place,
        Err(Miss::NoAnswer) => return EXIT_NO_GEOCODER,
        Err(Miss::NoCity) => return EXIT_NO_CITY,
    };
    let Some(zone) = clock::windows_zone(&place.zone) else { return EXIT_NO_ZONE };
    let answer = Path::new(answer);
    if let Some(dir) = answer.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // the tz name is not read back: it is there for whoever looks in the file
    let text = format!("city={}\nzone={}\ntz={}\n", place.name, zone, place.zone);
    if write_whole(answer, text.as_bytes()).is_ok() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn world(settings: &mut Settings) -> World {
        let mut world = World::new(&Ctx { now: 1_000, settings, locked: false, signed_in: None });
        world.set_fixed();
        world
    }

    fn tm(day: i32, hour: i32, min: i32) -> Tm {
        Tm { year: 2026, month: 11, day, hour, min, sec: 30, wday: 0 }
    }

    #[test]
    fn hands_and_figures() {
        assert_eq!(hands(&tm(14, 0, 0)), (0, 0));
        assert_eq!(hands(&tm(14, 15, 30)), (105, 180));
        assert_eq!(hands(&tm(14, 23, 59)), (360, 354));
        assert_eq!((figures(&tm(14, 0, 5), true), figures(&tm(14, 15, 30), true)), (String::from("00:05"), String::from("15:30")));
        assert_eq!((figures(&tm(14, 0, 5), false), figures(&tm(14, 15, 30), false)), (String::from("12:05 AM"), String::from("3:30 PM")));
        assert_eq!(figures(&tm(14, 12, 0), false), "12:00 PM");
    }

    #[test]
    fn a_box_says_which_day_and_how_far_ahead() {
        assert_eq!((apart(0), apart(60), apart(-60), apart(420)), ("+0 hrs".into(), "+1 hr".into(), "-1 hr".into(), "+7 hrs".into()));
        assert_eq!((apart(330), apart(-750), apart(45)), ("+5:30".into(), "-12:30".into(), "+0:45".into()));
        let here = tm(14, 17, 30);
        assert_eq!(legend(&here, &here), "Today, +0 hrs");
        assert_eq!(legend(&here, &tm(15, 0, 30)), "Tomorrow, +7 hrs");
        assert_eq!(legend(&here, &tm(14, 12, 30)), "Today, -5 hrs");
        assert_eq!(legend(&tm(14, 3, 0), &tm(13, 17, 30)), "Yesterday, -9:30");
        // across the end of a month, and of a year
        let (eve, new_year) = (Tm { year: 2026, month: 12, day: 31, hour: 20, ..here }, Tm { year: 2027, month: 1, day: 1, hour: 5, ..here });
        assert_eq!(legend(&eve, &new_year), "Tomorrow, +9 hrs");
    }

    #[test]
    fn windows_keeps_the_time_of_other_places() {
        // Tokyo has no summer time: nine hours ahead of UTC, always
        let tokyo = clock::elsewhere("tokyo standard time").expect("Windows has no Tokyo");
        let t = tokyo.local(0);
        assert_eq!((t.year, t.month, t.day, t.hour, t.min), (1970, 1, 1, 9, 0));
        // London keeps summer time: an hour ahead in July, not in January
        let london = clock::elsewhere("GMT Standard Time").expect("Windows has no London");
        let (july, january) = (clock::mktime(2026, 7, 1, 12, 0, 0), clock::mktime(2026, 1, 15, 12, 0, 0));
        let ahead = |at: i64| (clock::reading(&london.local(at)) - clock::reading(&clock::utc(at))) / 60;
        assert_eq!((ahead(july), ahead(january)), (60, 0));
        assert!(clock::elsewhere("No Such Standard Time").is_none() && clock::elsewhere("").is_none());
    }

    #[test]
    fn icu_knows_what_windows_calls_a_zone() {
        assert_eq!(clock::windows_zone("Asia/Tokyo").as_deref(), Some("Tokyo Standard Time"));
        assert_eq!(clock::windows_zone("Europe/London").as_deref(), Some("GMT Standard Time"));
        assert_eq!(clock::windows_zone("Nowhere/At_All"), None);
        assert_eq!(clock::windows_zone(""), None);
    }

    #[test]
    fn the_clocks_start_as_the_skin_has_them() {
        let mut settings = Settings::in_memory();
        let world = world(&mut settings);
        assert_eq!([world.label(0), world.label(1), world.label(2)], ["London", "New York", "Tokyo"]);
        assert!(world.faces.iter().all(|f| f.zone.is_some() && !f.time.is_empty() && f.legend.contains(", ")));
        assert_eq!((world.num("NoZone1"), world.num("NoZone3"), world.num("NoZone4")), (0.0, 0.0, 0.0));
        // Tokyo is nine hours ahead of London in winter and eight in summer:
        // never the same hand
        assert_ne!(world.num("AngleHour1"), world.num("AngleHour3"));
        assert_eq!(world.num("AngleMin1"), world.num("AngleMin3"));
        let mut time = String::new();
        world.text("Time3", &mut time);
        assert_eq!(time, world.faces[2].time);
    }

    #[test]
    fn each_field_is_typed_in_on_its_own() {
        let mut settings = Settings::in_memory();
        let mut world = world(&mut settings);
        let middle = |w: &World, i: usize| {
            let r = w.faces[i].field.rect;
            ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2)
        };
        let (x, y) = middle(&world, 1);
        assert!(matches!(world.press(x, y, false, false), Pressed::Taken(Outcome { request: Request::TakeKeys, .. })));
        assert_eq!(world.typing, Some(1));
        // the name that was there is selected: typing replaces it
        "Kyiv".chars().for_each(|c| drop(world.typed(c)));
        assert_eq!(world.faces[1].field.text(), "Kyiv");
        assert!(world.typing_at(x, y));
        // another field is not this one's: the host ends the typing first
        let (x0, y0) = middle(&world, 0);
        assert!(!world.typing_at(x0, y0) && world.ibeam(x0, y0));
        assert!(matches!(world.press(1, 1, false, false), Pressed::No));

        // Escape leaves the city as it was
        world.key(Key::Escape, false, false);
        let mut ctx = Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None };
        world.blur(&mut ctx);
        assert_eq!((world.typing, world.label(1)), (None, "New York"));

        // emptied, the clock is put away -- and stays away
        world.press(x, y, false, false);
        world.key(Key::Delete, false, false);
        assert_eq!(world.blur(&mut ctx), Dirty::All);
        assert_eq!((world.label(1), world.faces[1].legend.as_str(), world.num("NoZone2")), ("Type a city", "No city", 1.0));
        assert_eq!((settings.get(SECTION, "City2"), settings.get(SECTION, "Zone2")), (Some(""), Some("")));
        assert_eq!(World::new(&Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None }).label(1), "Type a city");
    }

    #[test]
    fn an_answer_sets_the_clock_and_a_failure_says_so() {
        let dir = std::env::temp_dir().join(format!("win98-world-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut settings = Settings::in_memory();
        let mut world = world(&mut settings);
        world.answer = dir.join("zone.txt");
        let mut ctx = Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None };

        // while it is looked up the field says so, and the clock runs on
        world.finding = Some((0, String::from("Finding Sydney...")));
        assert_eq!((world.label(0), world.label(1)), ("Finding Sydney...", "New York"));
        std::fs::write(&world.answer, "city=Sydney\nzone=AUS Eastern Standard Time\ntz=Australia/Sydney\n").unwrap();
        assert_eq!(world.finish(Some(0), &mut ctx), Dirty::All);
        assert_eq!((world.label(0), world.faces[0].notice), ("Sydney", None));
        assert!(world.faces[0].zone.is_some() && !world.answer.exists());

        // no such place: the clock is as it was, and the field says why for a while
        world.finding = Some((0, String::from("Finding Xyzzy...")));
        world.finish(Some(EXIT_NO_CITY), &mut ctx);
        assert_eq!((world.label(0), world.faces[0].city.as_str()), ("No such city", "Sydney"));
        world.frozen = false;
        for _ in 0..NOTICE {
            world.tick(&mut ctx);
        }
        assert_eq!(world.label(0), "Sydney");

        // an answer naming a zone Windows has not got is no answer
        world.finding = Some((2, String::new()));
        std::fs::write(&world.answer, "city=Atlantis\nzone=Atlantis Standard Time\n").unwrap();
        world.finish(Some(0), &mut ctx);
        assert_eq!((world.label(2), world.faces[2].city.as_str()), ("Lookup failed", "Tokyo"));

        assert_eq!((settings.get(SECTION, "City1"), settings.get(SECTION, "Zone1")), (Some("Sydney"), Some("AUS Eastern Standard Time")));
        assert_eq!(settings.get(SECTION, "City3"), Some("Tokyo"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
