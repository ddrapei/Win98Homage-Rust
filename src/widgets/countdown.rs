//! The Countdown widget: how many days are left until a day picked on a
//! calendar, and what the day is, typed into a field under the count.
//!
//! The calendar is the Date/Time widget's (`calendar`): today is in its dark
//! block, and the day counted to is ringed in red, as by hand with a pen,
//! whenever its month is on show.  A click on a day counts to that day; the
//! wheel, the month list and the year arrows browse, and a double-click goes
//! back to the month counted to.  The count is the number of days from today
//! to that day, "Today" on the day itself, and 0 once it has gone by.  It
//! moves on at midnight, local time.  The name is typed where it stands, as
//! the weather's city is (`field`).
//!
//! There can be any number of these, as there can be of notes: the first is
//! [Countdown] in settings.ini, the others [Countdown2], [Countdown3], ...
//! Each keeps its day (`Date=2026-11-27`) and its name (`Name=`) under its own
//! section, never under a fixed "Countdown"; the host makes and deletes them
//! with the functions at the foot of this file.

use std::path::PathBuf;

use crate::calendar::{Answer, Calendar};
use crate::canvas::{Canvas, Rect};
use crate::clock::{self, Tm, MONTHS};
use crate::field::Field;
use crate::keys::Key;
use crate::settings::{self, Settings};
use crate::skin::{self, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, Outcome, Pressed, Request, Widget};

static SKIN: &Skin = &skin::countdown::SKIN;

/// The first countdown's section of settings.ini; the others add a number.
pub const FIRST: &str = "Countdown";

/// A cell of the calendar's grid, and where its number is in it: right-
/// aligned on x 18 of the cell (the D meters' X), its digits inked on rows
/// 6..14, so their middle is 10.5 rows down.
const CELL_W: i32 = 23;
const CELL_H: i32 = 18;
const NUMBER_RIGHT: i32 = 18;
const NUMBER_MIDDLE: f32 = 10.5;
/// The digits' ink ends a pixel short of their advances (a glyph's room to
/// its right): this much to the right of the middle of the advances is the
/// middle of the ink.
const INK_DX: f32 = 0.5;

/// The ring round the number: this many rows from its middle to its top and
/// bottom, and as many columns to either side -- or, for a number wider than
/// a digit, that much clear of the number's half-width.  So it is round
/// round one digit, a little wider round two, and in every cell clear of
/// the next row's number.
const RING_RY: f32 = 7.0;
const RING_ROOM: f32 = 4.0;
/// How it is drawn, as a pen would: starting at the upper right and going
/// round anticlockwise once and this much more (radians), from this much
/// under its size to as much over, so that its end runs on past its start on
/// the outside rather than closing on it -- the two strands side by side on
/// the slope at the upper right -- and over its last stretch it flicks out by
/// this much more; and tilted this much.
const RING_START: f32 = -0.45;
const RING_OVERRUN: f32 = 0.7;
const RING_GROW: f32 = 0.12;
const RING_FLICK: f32 = 0.12;
const RING_FLICK_FROM: f32 = 0.9;
const RING_TILT: f32 = -0.15;

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// What the date line says until a day is picked.
const NO_DATE: &str = "Click a day on the calendar";

/// "2026-11-27" as a date there is.
fn parse_date(s: &str) -> Option<(i32, i32, i32)> {
    let mut parts = s.trim().splitn(3, '-').map(|p| p.trim().parse::<i32>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    ((1..=12).contains(&month) && day >= 1 && day <= clock::days_in_month(year, month)).then_some((year, month, day))
}

fn format_date((year, month, day): (i32, i32, i32)) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

/// The days from today to `event`: 0 on the day, less once it has gone by.
fn days_to(event: (i32, i32, i32), t: &Tm) -> i64 {
    clock::days_from_civil(event.0, event.1, event.2) - clock::days_from_civil(t.year, t.month, t.day)
}

/// The count and the date line, for `event` seen from `t`.
fn readings(event: Option<(i32, i32, i32)>, t: &Tm) -> (String, String) {
    let Some(event) = event else { return (String::from("--"), String::from(NO_DATE)) };
    let count = match days_to(event, t) {
        0 => String::from("Today"),
        n if n < 0 => String::from("0"),
        n => n.to_string(),
    };
    let (year, month, day) = event;
    let weekday = DAYS[clock::weekday(year, month, day) as usize];
    (count, format!("{weekday}, {day} {} {year}", MONTHS[month as usize - 1]))
}

/// Which day the week starts on: the Date/Time widget's choice, so that the
/// two calendars agree.
fn week_start(settings: &Settings) -> i32 {
    settings.int("DateTime", "WeekStart", skin::datetime::SKIN.int("WeekStart", 1))
}

/// The pixels of a ring round (cx, cy), of radii (rx, ry), as a pen draws it
/// (see RING_START and the rest): a line a pixel wide, in the order it is
/// drawn.  It is followed in small steps, every pixel it passes through
/// taken once, and then the pixel in the elbow of each diagonal step left
/// out, as a pixel artist would, so that it does not thicken on the slopes.
fn ring_pixels(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<(i32, i32)> {
    const STEPS: usize = 720;
    let sweep = core::f32::consts::TAU + RING_OVERRUN;
    let (sin, cos) = RING_TILT.sin_cos();
    let mut path: Vec<(i32, i32)> = Vec::new();
    for k in 0..=STEPS {
        let s = k as f32 / STEPS as f32;
        // y runs down the screen, so going round anticlockwise is the angle
        // getting smaller
        let a = RING_START - s * sweep;
        let flick = ((s - RING_FLICK_FROM) / (1.0 - RING_FLICK_FROM)).max(0.0).powi(2);
        let size = 1.0 - RING_GROW + 2.0 * RING_GROW * s + RING_FLICK * flick;
        let (ex, ey) = (rx * size * a.cos(), ry * size * a.sin());
        let p = ((cx + ex * cos - ey * sin).floor() as i32, (cy + ex * sin + ey * cos).floor() as i32);
        if path.last() != Some(&p) {
            path.push(p);
        }
    }
    let mut line: Vec<(i32, i32)> = Vec::with_capacity(path.len());
    for (i, &p) in path.iter().enumerate() {
        let elbow = match (line.last(), path.get(i + 1)) {
            (Some(&a), Some(&b)) => (a.0 - b.0).abs() == 1 && (a.1 - b.1).abs() == 1,
            _ => false,
        };
        if !elbow {
            line.push(p);
        }
    }
    line
}

pub struct Countdown {
    /// Its own section of settings.ini.
    section: String,
    calendar: Calendar,
    field: Field,
    /// The day counted to.
    event: Option<(i32, i32, i32)>,
    name: String,
    /// The ring's red, which no tint turns, and the font of the numbers it
    /// goes round, as the host's `Text` numbers it.
    ring: u32,
    font: usize,
    count: String,
    event_date: String,
    today: String,
    /// The cells of today and of the day counted to, when they are in the
    /// month on show.
    today_cell: Option<(i32, i32)>,
    ring_cell: Option<(i32, i32)>,
    /// What the readings were worked out from: the date, the week, the
    /// month on show and the day counted to.
    key: (Tm, i32, Option<(i32, i32)>, Option<(i32, i32, i32)>),
}

impl Countdown {
    pub fn new(ctx: &Ctx, section: &str) -> Countdown {
        let s = &*ctx.settings;
        let event = s.get(section, "Date").and_then(parse_date);
        let mut w = Countdown {
            section: section.to_owned(),
            calendar: Calendar::new(SKIN, week_start(s)),
            field: Field::new(SKIN, "MeterName"),
            event,
            name: s.text(section, "Name", ""),
            ring: SKIN.colour("Ring"),
            font: skin::FONTS.iter().position(|&(pt, bold)| pt == SKIN.num("FontSize", 8.0) as f32 && !bold).unwrap_or(0),
            count: String::new(),
            event_date: String::new(),
            today: String::new(),
            today_cell: None,
            ring_cell: None,
            key: (Tm::default(), -1, None, None),
        };
        // it opens on the month counted to
        let t = clock::local(ctx.now);
        if let Some((year, month, _)) = event {
            w.calendar.show(year, month, &t);
        }
        w.refresh(&t);
        w
    }

    fn key_of(&self, t: &Tm) -> (Tm, i32, Option<(i32, i32)>, Option<(i32, i32, i32)>) {
        (Tm { hour: 0, min: 0, sec: 0, ..*t }, self.calendar.week_start, self.calendar.view, self.event)
    }

    /// Everything that follows from the date, the month on show and the day
    /// counted to.
    fn refresh(&mut self, t: &Tm) {
        self.today_cell = self.calendar.cell_of((t.year, t.month, t.day), t);
        // Today is left blank in the grid: MeterSelText draws it in light ink
        // on its block.
        self.calendar.fill(t, self.today_cell.map(|_| t.day));
        self.today = t.day.to_string();
        self.ring_cell = self.event.and_then(|e| self.calendar.cell_of(e, t));
        (self.count, self.event_date) = readings(self.event, t);
        self.key = self.key_of(t);
    }

    /// Where the ring round the day counted to goes, the grid starting at
    /// (x, y): its middle and its radii.  It is centred on the number's
    /// digits -- which are right-aligned in the cell, so one digit sits
    /// further right than two -- as measured with the font they are drawn in.
    fn ring_at(&self, x: i32, y: i32, text: Option<&mut Text>) -> Option<(f32, f32, f32, f32)> {
        let ((col, row), (_, _, day)) = (self.ring_cell?, self.event?);
        let number = day.to_string();
        let width = match text {
            Some(text) => {
                let mut each = Vec::new();
                text.advances(self.font, &number, &mut each);
                each.iter().sum()
            }
            // nothing to measure with: the width of a digit, as good a guess as any
            None => 6.0 * number.len() as f32,
        };
        let right = (x + col * CELL_W + NUMBER_RIGHT) as f32;
        let cy = (y + row * CELL_H) as f32 + NUMBER_MIDDLE;
        Some((right - width / 2.0 + INK_DX, cy, (width / 2.0 + RING_ROOM).max(RING_RY), RING_RY))
    }

    /// The ring round the day counted to, kept inside the grid: a ring round
    /// a one-digit day at the right-hand edge would otherwise touch the
    /// calendar's sunken edge.
    fn draw_ring(&self, x: i32, y: i32, canvas: &mut Canvas, text: &mut Text) {
        let Some((cx, cy, rx, ry)) = self.ring_at(x, y, Some(text)) else { return };
        let grid = Rect::new(x, y, x + 7 * CELL_W, y + 6 * CELL_H);
        // pulled in from an edge rather than cut off by it
        // (its widest is out at the side; the flick is at the top)
        let reach = rx * (1.0 + RING_GROW);
        let cx = cx.clamp(grid.x0 as f32 + reach, grid.x1 as f32 - reach);
        let outer = canvas.clip;
        canvas.clip = outer.intersect(&grid);
        for (px, py) in ring_pixels(cx, cy, rx, ry) {
            canvas.fill(px, py, 1, 1, self.ring);
        }
        canvas.clip = outer;
    }
}

impl Hooks for Countdown {
    fn num(&self, var: &str) -> f64 {
        if let Some(v) = self.calendar.num(var) {
            return v as f64;
        }
        (match (var, self.today_cell) {
            ("SelHide", cell) => cell.is_none() as i32,
            ("SelCol", Some((col, _))) => col,
            ("SelRow", Some((_, row))) => row,
            _ => 0,
        }) as f64
    }

    fn text(&self, var: &str, out: &mut String) {
        if self.calendar.text(var, out) {
            return;
        }
        out.push_str(match var {
            "Today" => &self.today,
            "Count" => &self.count,
            "EventDate" => &self.event_date,
            _ => "",
        });
    }

    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, text: &mut Text) {
        match meter.name {
            "MeterRing" => self.draw_ring(x, y, canvas, text),
            // the name, in its field: typed on or not, it is drawn here
            "MeterName" => self.field.draw(&self.name, canvas, text),
            _ => {}
        }
    }
}

impl Widget for Countdown {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Countdown\n\n\
         Calendar day\tclick to count the days to it\n\
         Month field\tclick for the list, scroll to step a month\n\
         Year arrows\ta year back or forward\n\
         Calendar\t\tscroll to browse months, double-click to go back\n\
         \t\tto the month counted to\n\
         Name field\tclick and type what the day is: Enter takes it,\n\
         \t\tEscape does not\n\n\
         The count is the days left from today.  It says Today on the day \
         itself, and 0 once the day has gone by.  For another countdown: \
         right-click, Widgets, Countdown +."
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        // the Date/Time widget's menu may have turned the week round
        self.calendar.week_start = if week_start(ctx.settings) == 0 { 0 } else { 1 };
        let t = clock::local(ctx.now);
        if self.key_of(&t) == self.key {
            return Dirty::None;
        }
        self.refresh(&t);
        Dirty::All
    }

    fn command(&mut self, cmd: &str, arg: i32, _x: i32, _y: i32, ctx: &mut Ctx) -> Outcome {
        let t = clock::local(ctx.now);
        match self.calendar.command(cmd, arg, &t) {
            Some(Answer::Moved) => {
                self.refresh(&t);
                return Outcome::all();
            }
            Some(Answer::Repaint(dirty)) => return Outcome::dirty(dirty),
            None => {}
        }
        match cmd {
            // count to the day in cell `arg`, and keep it for next time
            "Select" => {
                let Some(date) = self.calendar.date_at(arg, &t) else { return Outcome::none() };
                self.event = Some(date);
                ctx.settings.set(&self.section, "Date", &format_date(date));
                self.refresh(&t);
                Outcome::all()
            }
            // back to the month counted to
            "Home" => {
                match self.event {
                    Some((year, month, _)) => self.calendar.show(year, month, &t),
                    None => self.calendar.view = None,
                }
                self.refresh(&t);
                Outcome::all()
            }
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    /// A press in the name field starts the typing, with the name that is
    /// there all selected -- closing the month list, if it was open; once it
    /// has the keyboard a press puts the caret down, and held, it selects.
    /// Anywhere else is the host's.
    fn press(&mut self, x: i32, y: i32, double: bool, shift: bool) -> Pressed {
        if !self.field.reach().contains(x, y) {
            return Pressed::No;
        }
        if self.field.editing {
            return Pressed::Taken(self.field.press(x, double, shift));
        }
        let list = self.calendar.list_open;
        self.calendar.close_list();
        let name = self.name.clone();
        let outcome = self.field.begin(&name);
        Pressed::Taken(if list { Outcome { dirty: Dirty::All, ..outcome } } else { outcome })
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

    /// The keyboard has gone elsewhere: what was typed is the name, if it is
    /// an answer -- Enter, or a click elsewhere after a change -- and is kept
    /// under this countdown's own section.
    fn blur(&mut self, ctx: &mut Ctx) -> Dirty {
        if !self.field.editing {
            return Dirty::None;
        }
        if let Some(typed) = self.field.end() {
            ctx.settings.set(&self.section, "Name", &typed);
            self.name = typed;
        }
        Dirty::Part(self.field.rect)
    }

    fn retint(&mut self) {
        // the ring is red pen whatever the tint
        self.field.retint();
    }

    /// Closed while the name was being typed: it is kept as a click
    /// elsewhere would keep it.
    fn shutdown(&mut self, ctx: &mut Ctx) {
        self.blur(ctx);
    }
}

// ---------------------------------------------------------------------------
//  More than one countdown
// ---------------------------------------------------------------------------
//
// A countdown is a section of settings.ini -- where it sits, its day and its
// name -- and nothing else: there is no file of its own to make or to bin.

/// Which countdown a section is: 1 for `[Countdown]`, n for `[Countdown<n>]`,
/// and None for a section that is not a countdown's.
fn number(section: &str) -> Option<u32> {
    settings::numbered(section, FIRST)
}

/// What a countdown is called in the menus: the first is the Countdown, and
/// the others have their numbers.
pub fn name(section: &str) -> String {
    match number(section) {
        Some(n) if n > 1 => format!("Countdown {n}"),
        _ => String::from("Countdown"),
    }
}

/// The sections of the countdowns after the first, in the order of their
/// numbers.
pub fn others(settings: &Settings) -> Vec<String> {
    let mut found: Vec<(u32, &str)> =
        settings.sections().filter_map(|s| Some((number(s).filter(|&n| n > 1)?, s))).collect();
    found.sort();
    found.dedup_by_key(|f| f.0);
    found.into_iter().map(|(_, s)| s.to_owned()).collect()
}

/// Start another countdown: a section of its own, under the lowest number
/// none has, counting to nothing yet.  Returns the section.
pub fn create(settings: &mut Settings) -> String {
    let taken: Vec<u32> = settings.sections().filter_map(number).collect();
    let n = (2..).find(|n| !taken.contains(n)).unwrap_or(u32::MAX);
    let section = format!("{FIRST}{n}");
    settings.set(&section, "Date", "");
    settings.set(&section, "Name", "");
    section
}

/// Whether there is nothing to lose in a countdown: no day, and no name.
pub fn is_blank(settings: &Settings, section: &str) -> bool {
    settings.get(section, "Date").and_then(parse_date).is_none()
        && settings.get(section, "Name").is_none_or(|n| n.trim().is_empty())
}

/// Take a countdown out of settings.ini.  It leaves nothing on disk, so
/// there is nothing to hand back.
pub fn forget(settings: &mut Settings, section: &str) -> Vec<PathBuf> {
    settings.remove_section(section);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: i32, hour: i32, min: i32, sec: i32) -> i64 {
        clock::mktime(2026, 11, day, hour, min, sec)
    }

    /// The countdown in `section`, counting to `date`, made at `now`.
    fn countdown(settings: &mut Settings, section: &str, date: &str, now: i64) -> Countdown {
        settings.set(section, "Date", date);
        Countdown::new(&Ctx { now, settings, locked: false, signed_in: None }, section)
    }

    /// The cell of a date in the month on show, as the skin numbers them.
    fn cell(w: &Countdown, date: (i32, i32, i32), now: i64) -> i32 {
        let (col, row) = w.calendar.cell_of(date, &clock::local(now)).unwrap();
        row * 7 + col
    }

    #[test]
    fn a_date_is_read_only_if_there_is_such_a_day() {
        assert_eq!(parse_date("2026-11-27"), Some((2026, 11, 27)));
        assert_eq!(parse_date(" 2028-2-29 "), Some((2028, 2, 29)));
        for bad in ["", "2026-11", "2026-13-01", "2026-02-29", "2026-11-0", "tomorrow", "2026-11-27-1"] {
            assert_eq!(parse_date(bad), None, "{bad:?}");
        }
        assert_eq!(format_date((2026, 3, 7)), "2026-03-07");
    }

    #[test]
    fn the_count_runs_down_to_today_and_stops_at_nought() {
        let mut settings = Settings::in_memory();
        let mut w = countdown(&mut settings, FIRST, "2026-11-27", at(20, 12, 0, 0));
        assert_eq!((w.count.as_str(), w.event_date.as_str()), ("7", "Friday, 27 November 2026"));
        for (now, count) in [(at(26, 23, 59, 59), "1"), (at(27, 0, 0, 0), "Today"), (at(28, 9, 0, 0), "0"), (at(30, 9, 0, 0), "0")] {
            w.tick(&mut Ctx { now, settings: &mut settings, locked: false, signed_in: None });
            assert_eq!(w.count, count);
        }
        // nothing picked yet
        let w = countdown(&mut settings, "Countdown2", "", at(20, 12, 0, 0));
        assert_eq!((w.count.as_str(), w.event_date.as_str(), w.ring_cell), ("--", NO_DATE, None));
    }

    #[test]
    fn the_count_moves_on_at_midnight_and_not_before() {
        let mut settings = Settings::in_memory();
        let mut w = countdown(&mut settings, FIRST, "2026-11-21", at(20, 23, 0, 0));
        let mut tick = |w: &mut Countdown, now| w.tick(&mut Ctx { now, settings: &mut settings, locked: false, signed_in: None });
        assert_eq!((tick(&mut w, at(20, 23, 59, 59)), w.count.as_str()), (Dirty::None, "1"));
        assert_eq!((tick(&mut w, at(21, 0, 0, 0)), w.count.as_str()), (Dirty::All, "Today"));
        // and today's block moves with it
        assert_eq!(w.today, "21");
    }

    #[test]
    fn a_day_clicked_is_counted_to_and_kept_in_its_own_section() {
        let now = at(20, 12, 0, 0);
        let mut settings = Settings::in_memory();
        let mut w = countdown(&mut settings, "Countdown3", "", now);
        let mut ctx = Ctx { now, settings: &mut settings, locked: false, signed_in: None };
        let i = cell(&w, (2026, 11, 27), now);
        assert!(matches!(w.command("Select", i, 0, 0, &mut ctx).dirty, Dirty::All));
        assert_eq!((w.count.as_str(), w.ring_cell.is_some()), ("7", true));
        assert_eq!(ctx.settings.get("Countdown3", "Date"), Some("2026-11-27"));
        assert_eq!(ctx.settings.get(FIRST, "Date"), Some(""));
        // a cell with no day in it is nothing
        assert!(matches!(w.command("Select", 0, 0, 0, &mut ctx).dirty, Dirty::None));
        assert_eq!(w.event, Some((2026, 11, 27)));
    }

    #[test]
    fn the_ring_is_only_on_the_month_counted_to() {
        let now = at(20, 12, 0, 0);
        let mut settings = Settings::in_memory();
        let mut w = countdown(&mut settings, FIRST, "2027-01-08", now);
        let mut ctx = Ctx { now, settings: &mut settings, locked: false, signed_in: None };
        // it opens on that month, where today is not
        assert_eq!((w.calendar.view, w.ring_cell.is_some(), w.today_cell), (Some((2027, 1)), true, None));
        w.command("Shift", -2, 0, 0, &mut ctx);
        assert_eq!((w.calendar.view, w.ring_cell, w.today_cell.is_some()), (None, None, true));
        // a double-click goes back to it
        w.command("Home", 0, 0, 0, &mut ctx);
        assert_eq!((w.calendar.month(&clock::local(now)), w.ring_cell.is_some()), ((2027, 1), true));
    }

    #[test]
    fn the_ring_is_drawn_round_once_and_a_bit_and_left_open() {
        let (cx, cy, rx) = (50.0, 50.0, 10.0);
        let ring = ring_pixels(cx, cy, rx, RING_RY);
        // how far out a pixel is, 1 on the ellipse of the ring's own size,
        // tilted as it is
        let away = |p: (i32, i32)| {
            let (x, y, (sin, cos)) = (p.0 as f32 + 0.5 - cx, p.1 as f32 + 0.5 - cy, RING_TILT.sin_cos());
            ((x * cos + y * sin) / rx).powi(2) + ((y * cos - x * sin) / RING_RY).powi(2)
        };
        let (first, last) = (ring[0], ring[ring.len() - 1]);
        // its end is not its start: it has run on past it, on the outside
        assert!((first.0 - last.0).abs() + (first.1 - last.1).abs() > 2, "{first:?} .. {last:?}");
        assert!(away(last) > away(first) + 0.1, "{first:?} {} .. {last:?} {}", away(first), away(last));
        // a line: each pixel touches the one before it, and none is in the
        // elbow of a diagonal step
        for w in ring.windows(2) {
            assert!((w[0].0 - w[1].0).abs() <= 1 && (w[0].1 - w[1].1).abs() <= 1, "{w:?}");
        }
        for w in ring.windows(3) {
            assert!(!((w[0].0 - w[2].0).abs() == 1 && (w[0].1 - w[2].1).abs() == 1), "{w:?}");
        }
    }

    #[test]
    fn the_ring_is_centred_on_the_number_and_kept_in_its_row() {
        let mut text = Text::new(SKIN.var("Font"), skin::FONTS).unwrap();
        let mut settings = Settings::in_memory();
        // a one-digit day and a two-digit one, the first in the top row and
        // the second in the last row a month can reach
        for (date, number) in [("2027-01-01", "1"), ("2026-11-30", "30")] {
            let w = countdown(&mut settings, FIRST, date, at(20, 12, 0, 0));
            let (cx, cy, rx, ry) = w.ring_at(0, 0, Some(&mut text)).unwrap();
            let (col, row) = w.ring_cell.unwrap();
            let ring = ring_pixels(cx, cy, rx, ry);
            let (x0, x1) = (ring.iter().map(|p| p.0).min().unwrap(), ring.iter().map(|p| p.0).max().unwrap());
            let (y0, y1) = (ring.iter().map(|p| p.1).min().unwrap(), ring.iter().map(|p| p.1).max().unwrap());
            // the middle of the number's digits, right-aligned in the cell
            let mut each = Vec::new();
            text.advances(w.font, number, &mut each);
            let digits = (col * CELL_W + NUMBER_RIGHT) as f32 - each.iter().sum::<f32>() / 2.0 + INK_DX;
            // as much room to the left of it as to its right, and above as below
            let middle = ((x0 + x1 + 1) as f32 / 2.0, (y0 + y1 + 1) as f32 / 2.0);
            assert!((middle.0 - digits).abs() <= 1.0, "{date}: ring {x0}..{x1} round digits centred on {digits}");
            assert!((middle.1 - (row * CELL_H) as f32 - NUMBER_MIDDLE).abs() <= 1.0, "{date}: ring rows {y0}..{y1}");
            // and clear of the rows above and below
            assert!(y0 >= row * CELL_H && y1 < (row + 1) * CELL_H, "{date}: ring rows {y0}..{y1} in row {row}");
        }
    }

    #[test]
    fn the_name_is_typed_in_its_field_and_kept_by_enter_only() {
        let now = at(20, 12, 0, 0);
        let mut settings = Settings::in_memory();
        let mut w = countdown(&mut settings, "Countdown2", "2026-11-27", now);
        let mut ctx = Ctx { now, settings: &mut settings, locked: false, signed_in: None };
        let (x, y) = (w.field.rect.x0 + 20, w.field.rect.y0 + 5);
        let Pressed::Taken(started) = w.press(x, y, false, false) else { panic!("the field did not take the press") };
        assert_eq!(started.request, Request::TakeKeys);
        "Birthday".chars().for_each(|c| drop(w.typed(c)));
        assert_eq!(w.key(Key::Enter, false, false).request, Request::DropKeys);
        w.blur(&mut ctx);
        assert_eq!((w.name.as_str(), ctx.settings.get("Countdown2", "Name")), ("Birthday", Some("Birthday")));
        // Escape leaves it as it was
        drop(w.press(x, y, false, false));
        "Party".chars().for_each(|c| drop(w.typed(c)));
        w.key(Key::Escape, false, false);
        w.blur(&mut ctx);
        assert_eq!((w.name.as_str(), ctx.settings.get("Countdown2", "Name")), ("Birthday", Some("Birthday")));
        // a press outside the field is the host's
        assert!(matches!(w.press(x, w.field.rect.y0 - 30, false, false), Pressed::No));
    }

    #[test]
    fn a_press_on_the_name_closes_the_month_list() {
        let now = at(20, 12, 0, 0);
        let mut settings = Settings::in_memory();
        let mut w = countdown(&mut settings, FIRST, "", now);
        let mut ctx = Ctx { now, settings: &mut settings, locked: false, signed_in: None };
        w.command("ToggleMonthList", 0, 0, 0, &mut ctx);
        let (x, y) = (w.field.rect.x0 + 20, w.field.rect.y0 + 5);
        let Pressed::Taken(started) = w.press(x, y, false, false) else { panic!("the field did not take the press") };
        assert_eq!((started.dirty, w.calendar.list_open), (Dirty::All, false));
    }

    #[test]
    fn countdowns_are_numbered_like_notes() {
        let mut settings = Settings::in_memory();
        assert!(others(&settings).is_empty());
        let second = create(&mut settings);
        let third = create(&mut settings);
        assert_eq!((second.as_str(), third.as_str()), ("Countdown2", "Countdown3"));
        assert_eq!((name(FIRST), name(&third)), ("Countdown".to_owned(), "Countdown 3".to_owned()));
        assert_eq!(others(&settings), [second.clone(), third.clone()]);
        // a note's section is not a countdown's, nor the other way round
        assert_eq!((number("Notes2"), number("Countdown02"), number("Countdowns")), (None, None, None));
        // blank until it has a day or a name
        assert!(is_blank(&settings, &second));
        settings.set(&second, "Name", "Trip");
        assert!(!is_blank(&settings, &second));
        settings.set(&third, "Date", "2026-12-24");
        assert!(!is_blank(&settings, &third));
        // forgotten, it leaves nothing behind, and its number is free again
        assert!(forget(&mut settings, &second).is_empty());
        assert_eq!(others(&settings), [third]);
        assert_eq!(create(&mut settings), "Countdown2");
    }
}
