//! The Date/Time Properties widget: clock, browsable calendar, time-zone page
//! and the hourly usage graph.  The variables the skin leaves to the program
//! are kept as plain fields the skin's meters are answered from.

use std::path::PathBuf;

use crate::canvas::{Canvas, Rect};
use crate::clock::{self, Tm, MONTHS, WEEKDAYS};
use crate::skin::{self, index_of, Hooks, Meter, Skin};
use crate::text::Text;
use crate::usage::{self, Usage};
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Request, Widget};

static SKIN: &Skin = &skin::datetime::SKIN;
const SECTION: &str = "DateTime";

/// Isometric depth of a block, in px.  The floor band in [MeterUseTrough] is
/// drawn five rows deep to match.
const DEPTH: i32 = 5;
/// Rows kept clear above a full sixty-minute block.
const HEADROOM: i32 = 3;
/// Px between neighbouring blocks; narrowed when the window is too wide.
const BAR_GAP: i32 = 5;
/// A few unlocked minutes still deserve a visible block.
const MIN_BLOCK: i32 = 4;
/// Rows the hour ruler occupies at the foot of the graph.
const RULE_ROWS: i32 = 8;
const MAX_HOURS: usize = 24;

const DAY_NAMES: [[&str; 7]; 2] = [
    ["S", "M", "T", "W", "T", "F", "S"],        // WeekStart=0, Sunday first
    ["M", "T", "W", "Th", "F", "Sa", "Su"],     // WeekStart=1, Monday first
];

/// One 3D block of the graph: left edge, where the side face begins, height.
#[derive(Clone, Copy, Default)]
struct Block {
    l: i32,
    r: i32,
    h: i32,
}

pub struct DateTime {
    week_start: i32,
    use24: bool,
    hide_seconds: bool,
    hide_ring: bool,
    history_days: i64,
    /// The graph's field and the hours it covers.
    bar: (i32, i32, i32, i32),
    graph: (i32, i32),
    ink: u32,
    use_ink: u32,
    use_top: u32,
    use_side: u32,

    page: i32,
    list_open: bool,
    /// Month dropdown: the row under the pointer, 0 = none.
    hovered: i32,
    /// None: follow the real date.
    view: Option<(i32, i32)>,
    selected: Option<(i32, i32, i32)>,

    month_name: &'static str,
    year_text: String,
    time_text: String,
    today: String,
    zone_name: String,
    zone_offset: String,
    utc_time: String,
    zone_dst: &'static str,
    legend: String,
    totals: String,
    day_names: [&'static str; 7],
    /// The six weeks of the grid: day numbers, 0 for a blank cell.
    days: [u8; 42],
    recorded: [bool; 42],
    sel_col: i32,
    sel_row: i32,
    sel_hidden: bool,
    angles: (i32, i32, i32),
    blocks: [Option<Block>; MAX_HOURS],
    face: i32,
    now_mark: Option<i32>,

    usage: Usage,
    ready: bool,
    next_flush: i64,
    next_minute: i64,
    grid_key: (Tm, i32, Option<(i32, i32)>, Option<(i32, i32, i32)>, u32),
}

impl DateTime {
    pub fn new(ctx: &Ctx, journal: Option<Option<PathBuf>>) -> DateTime {
        let s = &*ctx.settings;
        let var = |name: &str| s.int(SECTION, name, SKIN.int(name, 0));
        let (mut from, mut to) = (var("GraphFrom"), var("GraphTo"));
        // The window must be whole hours inside one day.
        if from < 0 || to > 24 || to <= from {
            (from, to) = (0, 24);
        }
        let usage = match journal {
            Some(path) => Usage::at(path),
            None => Usage::open(&s.dir()),
        };
        DateTime {
            week_start: if var("WeekStart") == 0 { 0 } else { 1 },
            use24: var("Use24Hour") != 0,
            hide_seconds: var("HideSeconds") != 0,
            hide_ring: var("HideRing") != 0,
            history_days: var("HistoryDays").max(1) as i64,
            bar: (SKIN.int("BarX", 29), SKIN.int("BarY", 293), SKIN.int("BarW", 322), SKIN.int("BarH", 40)),
            graph: (from, to),
            ink: SKIN.colour("Ink"),
            use_ink: SKIN.colour("UseInk"),
            use_top: SKIN.colour("UseTop"),
            use_side: SKIN.colour("UseSide"),
            page: 0,
            list_open: false,
            hovered: 0,
            view: None,
            selected: None,
            month_name: "",
            year_text: String::new(),
            time_text: String::new(),
            today: String::new(),
            zone_name: String::new(),
            zone_offset: String::new(),
            utc_time: String::new(),
            zone_dst: "",
            legend: String::from("Computer use"),
            totals: String::new(),
            day_names: DAY_NAMES[1],
            days: [0; 42],
            recorded: [false; 42],
            sel_col: 0,
            sel_row: 0,
            sel_hidden: false,
            angles: (0, 0, 0),
            blocks: [None; MAX_HOURS],
            face: 0,
            now_mark: None,
            usage,
            ready: false,
            next_flush: 0,
            next_minute: 0,
            grid_key: (Tm::default(), -1, None, None, 0),
        }
    }

    /// For the test harness: the interval being observed began at `start`.
    pub fn set_live(&mut self, start: i64, now: i64) {
        self.usage.set_live(start, false, now);
        self.ready = true;
        self.next_flush = i64::MAX;
    }

    // ---- helpers ---------------------------------------------------------

    /// The column of the 1st: 0 is whichever day WeekStart puts first.
    fn first_column(&self, year: i32, month: i32) -> i32 {
        (clock::weekday(year, month, 1) - self.week_start).rem_euclid(7)
    }

    /// What repainting the clock every second has to cover: the hands and
    /// their shadows, and the time field under them.
    fn clock_rect(&self) -> Rect {
        let (cx, cy) = (SKIN.int("Pad", 12) + SKIN.int("CX", 289), SKIN.int("Pad", 12) + SKIN.int("CY", 142));
        let hands = Rect::new(cx - 60, cy - 60, cx + 63, cy + 63);
        match SKIN.meter("MeterTimeText") {
            Some(m) => hands.union(&Rect::new(m.x - 2, m.y - 2, m.x + 84, m.y + 15)),
            None => hands,
        }
    }

    fn utc_rect(&self) -> Rect {
        SKIN.meter("MeterTzV3").map_or(Rect::EMPTY, |m| Rect::new(m.x - 2, m.y - 2, m.x + 120, m.y + 15))
    }

    // ---- rendering: clock and time zone ----------------------------------

    /// Hand angles in whole degrees and the digital time.  True if anything
    /// on screen changed.
    fn render_clock(&mut self, t: &Tm) -> bool {
        let before = (self.angles, self.time_text.clone());
        if self.page == 0 {
            let round = |v: f64| (v + 0.5).floor() as i32;
            self.angles.0 = round((t.hour % 12) as f64 * 30.0 + t.min as f64 * 0.5);
            self.angles.1 = round(t.min as f64 * 6.0 + t.sec as f64 * 0.1);
            if !self.hide_seconds {
                self.angles.2 = t.sec * 6;
            }
        }
        self.time_text = if self.use24 {
            format!("{:02}:{:02}:{:02}", t.hour, t.min, t.sec)
        } else {
            let hour = if t.hour % 12 == 0 { 12 } else { t.hour % 12 };
            format!("{}:{:02}:{:02} {}", hour, t.min, t.sec, if t.hour < 12 { "AM" } else { "PM" })
        };
        before != (self.angles, self.time_text.clone())
    }

    fn render_utc(&mut self, now: i64) {
        let u = clock::utc(now);
        self.utc_time = format!("{:02}:{:02}:{:02}", u.hour, u.min, u.sec);
    }

    fn render_zone(&mut self) {
        let zone = clock::zone();
        let offset = zone.offset.abs();
        self.zone_offset = format!("UTC {}{:02}:{:02}", if zone.offset >= 0 { '+' } else { '-' }, offset / 60, offset % 60);
        self.zone_name = zone.name;
        self.zone_dst = if zone.dst { "Summer time (DST) in effect" } else { "Standard time" };
    }

    // ---- rendering: usage panel -------------------------------------------

    /// One block per hour of the window, its height the unlocked minutes of
    /// that hour against a field exactly sixty minutes tall.
    fn render_bar(&mut self, intervals: &[(i64, i64, bool)], from: i64, to: i64, today: bool, now: i64) {
        let hours = (((to - from) as f64 / 3600.0 + 0.5).floor() as usize).clamp(1, MAX_HOURS);

        // Seconds of unlocked time inside each hour of the window.  Locked
        // time and anything outside the window are skipped, not clamped.
        let mut seconds = [0i64; MAX_HOURS];
        for &(s, e, locked) in intervals {
            if locked || e <= from || s >= to {
                continue;
            }
            let mut s = s.max(from);
            let e = e.min(to.min(from + hours as i64 * 3600));
            while s < e {
                let slot = ((s - from) / 3600) as usize;
                let edge = e.min(from + (slot as i64 + 1) * 3600);
                seconds[slot] += edge - s;
                s = edge;
            }
        }

        // Lay the blocks out across the field: BAR_GAP between front faces,
        // the last block's depth hanging into the final gap's worth of margin.
        let (bx, _, bw, bh) = self.bar;
        let n = hours as i32;
        let mut gap = BAR_GAP;
        let mut face = (bw - DEPTH - (n - 1) * gap) / n;
        while face < 6 && gap > 1 {
            gap -= 1;
            face = (bw - DEPTH - (n - 1) * gap) / n;
        }
        let row = n * face + (n - 1) * gap + DEPTH;
        let x0 = bx + (bw - row) / 2;
        let plot = bh - DEPTH - HEADROOM;

        for i in 0..MAX_HOURS {
            self.blocks[i] = (i < hours && seconds[i] > 0).then(|| {
                let h = ((seconds[i] as f64 / 3600.0 * plot as f64 + 0.5).floor() as i32).clamp(MIN_BLOCK, plot);
                let l = x0 + i as i32 * (face + gap);
                Block { l, r: l + face, h }
            });
        }
        self.face = face;

        // The cursor travels across the current hour's face and hops the gap
        // on the hour.  Outside the window there is nowhere honest to put it.
        self.now_mark = (today && now >= from && now <= to).then(|| {
            let slot = (((now - from) / 3600) as i32).min(n - 1);
            let frac = (now - from - slot as i64 * 3600) as f64 / 3600.0;
            let at = x0 + slot * (face + gap) + (frac * face as f64 + 0.5).floor() as i32;
            at.min(x0 + row - 1)
        });
    }

    fn render_panel(&mut self, now: i64) {
        let (key, today, date) = match self.selected {
            Some((y, m, d)) => (clock::key_of(y, m, d), false, (y, m, d)),
            None => {
                let t = clock::local(now);
                (clock::day_key(now), true, (t.year, t.month, t.day))
            }
        };
        let intervals = self.usage.intervals(&key, now);
        // Built from wall-clock hours on that date rather than by adding
        // seconds to midnight, which slips an hour when the clocks change.
        let window_from = clock::mktime(date.0, date.1, date.2, self.graph.0, 0, 0);
        let window_to = clock::mktime(date.0, date.1, date.2, self.graph.1, 0, 0);

        self.legend = if today {
            String::from("Computer use - today")
        } else {
            let wday = clock::weekday(date.0, date.1, date.2) as usize;
            format!("Computer use - {} {} {}", WEEKDAYS[wday], date.2, &MONTHS[date.1 as usize - 1][..3])
        };

        if intervals.is_empty() {
            self.totals = String::from("Nothing recorded for this day");
        } else {
            let (mut unlocked, mut locked) = (0, 0);
            for &(s, e, is_locked) in &intervals {
                if is_locked { locked += e - s } else { unlocked += e - s }
            }
            let hhmm = |ts: i64| {
                let t = clock::local(ts);
                format!("{:02}:{:02}", t.hour, t.min)
            };
            let since = match usage::latest_session(&intervals) {
                None => String::from("Locked all day"),
                Some(start) if today => format!("On since {}", hhmm(start)),
                Some(start) => format!("Last session from {}", hhmm(start)),
            };
            self.totals = format!("{since}      {} unlocked      {} locked", duration(unlocked), duration(locked));
        }
        self.render_bar(&intervals, window_from, window_to, today, now);
    }

    // ---- rendering: calendar grid -------------------------------------------

    fn render_calendar(&mut self, t: &Tm) {
        let (year, month) = self.view.unwrap_or((t.year, t.month));
        self.day_names = DAY_NAMES[self.week_start as usize];
        let column = self.first_column(year, month);
        let length = clock::days_in_month(year, month);

        // The highlight marks the selected date, which is today unless a
        // click moved it.
        let (sy, sm, sd) = self.selected.unwrap_or((t.year, t.month, t.day));
        let show_selection = year == sy && month == sm;

        for i in 0..42 {
            let day = i as i32 - column + 1;
            let inside = day >= 1 && day <= length;
            self.recorded[i] = inside && self.usage.has(&clock::key_of(year, month, day));
            // The selected day is left blank here: MeterSelText draws it in
            // light ink on top of the highlight block.
            self.days[i] = if inside && !(show_selection && day == sd) { day as u8 } else { 0 };
        }

        self.sel_hidden = !show_selection;
        if show_selection {
            let index = sd - 1 + column;
            self.sel_col = index % 7;
            self.sel_row = index / 7;
            self.today = sd.to_string();
        } else {
            self.today.clear();
        }
        self.month_name = MONTHS[month as usize - 1];
        self.year_text = year.to_string();
    }

    /// Everything the grid depends on.
    fn calendar_key(&self, t: &Tm) -> (Tm, i32, Option<(i32, i32)>, Option<(i32, i32, i32)>, u32) {
        let date = Tm { hour: 0, min: 0, sec: 0, ..*t };
        (date, self.week_start, self.view, self.selected, self.usage.rev)
    }

    /// Redraw everything that depends on the date, the selection, or the log.
    fn refresh(&mut self, now: i64) -> Outcome {
        let t = clock::local(now);
        self.grid_key = self.calendar_key(&t);
        self.render_calendar(&t);
        self.render_panel(now);
        Outcome::all()
    }

    // ---- commands -----------------------------------------------------------

    /// Move the browsed month by n months.  Landing back on the real month
    /// re-latches to it, so the grid keeps following the date.
    fn shift(&mut self, n: i32, now: i64) -> Outcome {
        let t = clock::local(now);
        let (year, month) = self.view.unwrap_or((t.year, t.month));
        let months = year * 12 + (month - 1) + n;
        let (year, month) = (months.div_euclid(12), months.rem_euclid(12) + 1);
        self.view = (year != t.year || month != t.month).then_some((year, month));
        self.refresh(now)
    }

    fn set_month(&mut self, m: i32, now: i64) -> Outcome {
        self.list_open = false;
        self.hovered = 0;
        let t = clock::local(now);
        let year = self.view.map_or(t.year, |v| v.0);
        self.view = (year != t.year || m != t.month).then_some((year, m));
        self.refresh(now)
    }

    /// Calendar cell i (0..41) clicked: select that date, so the usage panel
    /// describes it.  Clicking the cell for today goes back to following today.
    fn select(&mut self, i: i32, now: i64) -> Outcome {
        let t = clock::local(now);
        let (year, month) = self.view.unwrap_or((t.year, t.month));
        let day = i - self.first_column(year, month) + 1;
        if day < 1 || day > clock::days_in_month(year, month) {
            return Outcome::none();
        }
        self.selected = ((year, month, day) != (t.year, t.month, t.day)).then_some((year, month, day));
        self.refresh(now)
    }

    fn set_page(&mut self, n: i32, now: i64) -> Outcome {
        self.page = if n == 1 { 1 } else { 0 };
        self.list_open = false;
        self.hovered = 0;
        let t = clock::local(now);
        self.render_clock(&t);
        self.render_utc(now);
        self.render_zone();
        self.refresh(now)
    }

    /// The 12/24 hour choice: changed here, redrawn from here, and only then
    /// mirrored into settings.ini so it survives a restart.
    fn set_hour_format(&mut self, hours: i32, ctx: &mut Ctx) -> Outcome {
        self.use24 = hours != 12;
        self.render_clock(&clock::local(ctx.now));
        ctx.settings.set_int(SECTION, "Use24Hour", self.use24 as i32);
        Outcome::dirty(Dirty::Part(self.clock_rect()))
    }

    fn list_rect(&self) -> Rect {
        SKIN.meter("MeterMonthListBg").map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3]))
    }
}

/// 754 s -> "12 m", 22320 s -> "6 h 12 m"
fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (hours, minutes) = (seconds / 3600, seconds % 3600 / 60);
    if hours > 0 { format!("{hours} h {minutes:02} m") } else { format!("{minutes} m") }
}

impl Hooks for DateTime {
    fn num(&self, var: &str) -> f64 {
        (match var {
            "Page" => self.page,
            "MLHide" => !self.list_open as i32,
            "MLMonth" => self.hovered,
            "SelHide" => self.sel_hidden as i32,
            "SelCol" => self.sel_col,
            "SelRow" => self.sel_row,
            "HideRing" => self.hide_ring as i32,
            "HideSeconds" => self.hide_seconds as i32,
            "AngleHour" => self.angles.0,
            "AngleMin" => self.angles.1,
            "AngleSec" => self.angles.2,
            _ => 0,
        }) as f64
    }

    fn text(&self, var: &str, out: &mut String) {
        if let Some(i) = index_of(var, "D") {
            if let Some(&day) = self.days.get(i).filter(|&&d| d != 0) {
                out.push_str(&day.to_string());
            }
            return;
        }
        if let Some(i) = index_of(var, "W") {
            out.push_str(self.day_names.get(i).copied().unwrap_or(""));
            return;
        }
        out.push_str(match var {
            "MonthName" => self.month_name,
            "YearText" => &self.year_text,
            "TimeText" => &self.time_text,
            "Today" => &self.today,
            "TZName" => &self.zone_name,
            "TZOffset" => &self.zone_offset,
            "UTCTime" => &self.utc_time,
            "TZDst" => self.zone_dst,
            "UseLegend" => &self.legend,
            "UseTotals" => &self.totals,
            _ => "",
        });
    }

    fn colour(&self, var: &str) -> u32 {
        // Days that have a usage record print their number in the blocks' ink.
        match index_of(var, "C") {
            Some(i) if self.recorded.get(i).copied().unwrap_or(false) => self.use_ink,
            _ => self.ink,
        }
    }

    /// [MeterUseBar]: four shapes per block -- the ink silhouette, then the
    /// front, side and top faces 1px inside it, so the ink shows through as
    /// the outline and the seams -- and the "now" cursor over everything.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        if meter.name != "MeterUseBar" {
            return;
        }
        let (_, by, _, bh) = self.bar;
        let base = by + bh;
        let d = DEPTH;
        let p = |px: i32, py: i32| ((x + px) as f32, (y + py) as f32);
        for b in self.blocks.iter().flatten() {
            let (l, r, h) = (b.l, b.r, b.h);
            canvas.poly(
                &[p(l, base), p(l, base - h), p(l + d, base - h - d), p(r + d, base - h - d), p(r + d, base - d), p(r, base)],
                self.ink,
            );
            if h >= 3 {
                canvas.fill(x + l + 1, y + base - h + 1, self.face - 2, h - 2, self.use_ink);
            }
            canvas.poly(
                &[p(r, base - h + 1), p(r + d - 1, base - h - d + 2), p(r + d - 1, base - d), p(r, base - 1)],
                self.use_side,
            );
            canvas.poly(
                &[p(l + 2, base - h), p(r - 1, base - h), p(r + d - 2, base - h - d + 1), p(l + d + 1, base - h - d + 1)],
                self.use_top,
            );
        }
        if let Some(at) = self.now_mark {
            canvas.fill(x + at, y + by, 1, bh + RULE_ROWS, self.ink);
        }
    }
}

impl Widget for DateTime {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Date/Time Properties\n\n\
         Tabs\t\tswitch between Date & Time and Time Zone\n\
         Month field\tclick for the list, scroll to step a month\n\
         Year arrows\ta year back or forward\n\
         Calendar day\tclick to select it; the usage panel follows\n\
         Calendar\t\tscroll to browse months, double-click for today\n\
         Time field\tclick for 12-hour / 24-hour\n\n\
         The graph shows, hour by hour, how long this computer was on and \
         unlocked.  Its log is plain text and is kept for a month."
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        let now = ctx.now;
        let t = clock::local(now);
        let mut dirty = Dirty::None;

        if self.render_clock(&t) {
            dirty = if self.page == 0 { Dirty::Part(self.clock_rect()) } else { Dirty::None };
        }
        if self.page == 1 {
            self.render_utc(now);
            dirty = dirty.and(Dirty::Part(self.utc_rect()));
        }

        // Watch the machine.
        if !self.ready {
            self.ready = true;
            self.usage.resume(now, ctx.locked, ctx.signed_in);
            self.next_flush = now + usage::FLUSH_SECONDS;
        }
        let revision = self.usage.rev;
        self.usage.observe(now, ctx.locked);
        let log_changed = self.usage.rev != revision;

        // Minute work: the usage panel, the time-zone details, and the flush
        // of the open interval to disk.  A lock or unlock does not wait.
        if now >= self.next_minute || log_changed {
            self.next_minute = now - now.rem_euclid(60) + 60;
            self.render_panel(now);
            self.render_zone();
            dirty = Dirty::All;
        }
        if log_changed || now >= self.next_flush {
            self.next_flush = now + usage::FLUSH_SECONDS;
            self.usage.save(now, self.history_days);
        }

        // The grid changes at midnight, when the user browses or selects, and
        // when a day gets its first usage record.
        let key = self.calendar_key(&t);
        if key != self.grid_key {
            self.grid_key = key;
            self.render_calendar(&t);
            dirty = Dirty::All;
        }
        dirty
    }

    fn command(&mut self, cmd: &str, arg: i32, _x: i32, _y: i32, ctx: &mut Ctx) -> Outcome {
        let now = ctx.now;
        match cmd {
            "Shift" => self.shift(arg, now),
            "Home" => {
                self.view = None;
                self.selected = None;
                self.refresh(now)
            }
            "HoverMonth" => {
                self.hovered = arg;
                Outcome::dirty(Dirty::Part(self.list_rect()))
            }
            // A leave is only allowed to clear the bar while it still owns it.
            "UnhoverMonth" if self.hovered == arg => {
                self.hovered = 0;
                Outcome::dirty(Dirty::Part(self.list_rect()))
            }
            "ToggleMonthList" => {
                self.list_open = !self.list_open;
                self.hovered = 0;
                Outcome::all()
            }
            "SetMonth" => self.set_month(arg, now),
            "Select" => self.select(arg, now),
            "SetPage" => self.set_page(arg, now),
            "SetHourFormat" => self.set_hour_format(arg, ctx),
            "ToggleHourFormat" => self.set_hour_format(if self.use24 { 12 } else { 24 }, ctx),
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { id: 1, label: "24-hour clock", checked: self.use24 });
        items.push(MenuItem { id: 2, label: "Week starts on Monday", checked: self.week_start == 1 });
        items.push(MenuItem { id: 3, label: "Second hand", checked: !self.hide_seconds });
        items.push(MenuItem { id: 4, label: "Clock ring", checked: !self.hide_ring });
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            1 => {
                self.set_hour_format(if self.use24 { 12 } else { 24 }, ctx);
            }
            2 => {
                self.week_start = 1 - self.week_start;
                ctx.settings.set_int(SECTION, "WeekStart", self.week_start);
            }
            3 => {
                self.hide_seconds = !self.hide_seconds;
                ctx.settings.set_int(SECTION, "HideSeconds", self.hide_seconds as i32);
                self.render_clock(&clock::local(ctx.now));
            }
            4 => {
                self.hide_ring = !self.hide_ring;
                ctx.settings.set_int(SECTION, "HideRing", self.hide_ring as i32);
            }
            _ => return Outcome::none(),
        }
        self.refresh(ctx.now)
    }

    /// Leave the open interval on disk with its provisional end, so a restart
    /// within a few minutes picks it back up.
    fn shutdown(&mut self, ctx: &mut Ctx) {
        if self.ready {
            self.usage.observe(ctx.now, ctx.locked);
            self.usage.save(ctx.now, self.history_days);
        }
    }
}
