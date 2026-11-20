//! A month calendar, browsed and clicked: the Date/Time widget's and the
//! Countdown's.
//!
//! Above the grid, a month field that drops down a list of the twelve months
//! and a year field with two arrows; under them a band of day names and six
//! weeks of day numbers.  The wheel over the month field or the grid steps a
//! month.  This is the part the two have in common: which month is on show,
//! the day numbers in its cells, and the list.  What a highlighted day means
//! -- the day the usage panel describes, today -- is the widget's own.
//!
//! The skin draws it all from variables the calendar answers: D0..D41 (the day
//! in each cell, empty for a blank one), W0..W6 (the day names), MonthName,
//! YearText, MLHide (1 while the list is closed) and MLMonth (the row of the
//! open list the pointer is on, 1..12, 0 for none).  The cells are numbered
//! row by row from the top left; MeterMonthListBg is where the open list is.

use crate::canvas::Rect;
use crate::clock::{self, Tm, MONTHS};
use crate::skin::{index_of, Skin};
use crate::widget::Dirty;

const DAY_NAMES: [[&str; 7]; 2] = [
    ["S", "M", "T", "W", "T", "F", "S"],        // WeekStart=0, Sunday first
    ["M", "T", "W", "Th", "F", "Sa", "Su"],     // WeekStart=1, Monday first
];

/// What one of the calendar's commands did.
#[derive(Debug, PartialEq)]
pub enum Answer {
    /// The month on show is another: the widget fills the grid again, and
    /// whatever it marks on it.
    Moved,
    /// Only the list changed: this much of the window wants repainting.
    Repaint(Dirty),
}

pub struct Calendar {
    /// 1 = Monday first, 0 = Sunday first.
    pub week_start: i32,
    /// The month browsed to; None follows the real date.
    pub view: Option<(i32, i32)>,
    /// The month list is open.
    pub list_open: bool,
    /// The row of the open list the pointer is on, 0 = none.
    pub hovered: i32,
    /// Where the open list is drawn.
    list: Rect,
    day_names: [&'static str; 7],
    /// The six weeks of the grid: day numbers, 0 for a blank cell.
    days: [u8; 42],
    month_name: &'static str,
    year_text: String,
}

impl Calendar {
    /// The calendar in `skin`, with the week starting on Monday if
    /// `week_start` is anything but 0.
    pub fn new(skin: &'static Skin, week_start: i32) -> Calendar {
        Calendar {
            week_start: if week_start == 0 { 0 } else { 1 },
            view: None,
            list_open: false,
            hovered: 0,
            list: skin.meter("MeterMonthListBg").map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3])),
            day_names: DAY_NAMES[1],
            days: [0; 42],
            month_name: "",
            year_text: String::new(),
        }
    }

    /// The month on show, `t` being today: the one browsed to, or today's.
    pub fn month(&self, t: &Tm) -> (i32, i32) {
        self.view.unwrap_or((t.year, t.month))
    }

    /// The column of the 1st: 0 is whichever day WeekStart puts first.
    pub fn first_column(&self, year: i32, month: i32) -> i32 {
        (clock::weekday(year, month, 1) - self.week_start).rem_euclid(7)
    }

    /// Show this month.  Today's month is not browsed to: it is followed, so
    /// that the grid moves on with the date.
    pub fn show(&mut self, year: i32, month: i32, t: &Tm) {
        self.view = (year != t.year || month != t.month).then_some((year, month));
    }

    /// Move the month on show by n months.
    pub fn shift(&mut self, n: i32, t: &Tm) {
        let (year, month) = self.month(t);
        let months = year * 12 + (month - 1) + n;
        self.show(months.div_euclid(12), months.rem_euclid(12) + 1, t);
    }

    /// Month m of the year on show, picked from the list, which closes.
    pub fn set_month(&mut self, m: i32, t: &Tm) {
        self.close_list();
        let year = self.month(t).0;
        self.show(year, m, t);
    }

    pub fn close_list(&mut self) {
        self.list_open = false;
        self.hovered = 0;
    }

    /// The date in cell i (0..41) of the month on show, if the cell has one.
    pub fn date_at(&self, i: i32, t: &Tm) -> Option<(i32, i32, i32)> {
        if !(0..42).contains(&i) {
            return None;
        }
        let (year, month) = self.month(t);
        let day = i - self.first_column(year, month) + 1;
        (day >= 1 && day <= clock::days_in_month(year, month)).then_some((year, month, day))
    }

    /// The cell a date is in -- its column and its row -- if it is in the
    /// month on show.
    pub fn cell_of(&self, date: (i32, i32, i32), t: &Tm) -> Option<(i32, i32)> {
        let (year, month, day) = date;
        if (year, month) != self.month(t) {
            return None;
        }
        let index = day - 1 + self.first_column(year, month);
        Some((index % 7, index / 7))
    }

    /// Fill the grid for the month on show, leaving the cell of day `blank`
    /// empty: the widget draws that one on its highlight, in light ink.
    pub fn fill(&mut self, t: &Tm, blank: Option<i32>) {
        let (year, month) = self.month(t);
        self.day_names = DAY_NAMES[self.week_start as usize];
        let column = self.first_column(year, month);
        let length = clock::days_in_month(year, month);
        for i in 0..42 {
            let day = i as i32 - column + 1;
            let inside = day >= 1 && day <= length;
            self.days[i] = if inside && blank != Some(day) { day as u8 } else { 0 };
        }
        self.month_name = MONTHS[month as usize - 1];
        self.year_text = year.to_string();
    }

    /// The calendar's own commands, as the skin names them; None for any
    /// other.  `t` is today.
    pub fn command(&mut self, cmd: &str, arg: i32, t: &Tm) -> Option<Answer> {
        let list = Answer::Repaint(Dirty::Part(self.list));
        Some(match cmd {
            "Shift" => {
                self.shift(arg, t);
                Answer::Moved
            }
            "SetMonth" => {
                self.set_month(arg, t);
                Answer::Moved
            }
            "ToggleMonthList" => {
                self.list_open = !self.list_open;
                self.hovered = 0;
                Answer::Repaint(Dirty::All)
            }
            "HoverMonth" => {
                self.hovered = arg;
                list
            }
            // A leave is only allowed to clear the bar while it still owns it.
            "UnhoverMonth" if self.hovered == arg => {
                self.hovered = 0;
                list
            }
            "UnhoverMonth" => Answer::Repaint(Dirty::None),
            _ => return None,
        })
    }

    /// MLHide and MLMonth.
    pub fn num(&self, var: &str) -> Option<i32> {
        match var {
            "MLHide" => Some(!self.list_open as i32),
            "MLMonth" => Some(self.hovered),
            _ => None,
        }
    }

    /// D0..D41, W0..W6, MonthName and YearText; false for any other.
    pub fn text(&self, var: &str, out: &mut String) -> bool {
        if let Some(i) = index_of(var, "D") {
            if let Some(&day) = self.days.get(i).filter(|&&d| d != 0) {
                out.push_str(&day.to_string());
            }
            return true;
        }
        if let Some(i) = index_of(var, "W") {
            out.push_str(self.day_names.get(i).copied().unwrap_or(""));
            return true;
        }
        match var {
            "MonthName" => out.push_str(self.month_name),
            "YearText" => out.push_str(&self.year_text),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin;

    fn calendar(week_start: i32) -> Calendar {
        Calendar::new(&skin::datetime::SKIN, week_start)
    }

    /// 20 November 2026, a Friday.
    fn today() -> Tm {
        clock::local(clock::mktime(2026, 11, 20, 12, 0, 0))
    }

    fn day_in(c: &Calendar, i: usize) -> String {
        let mut s = String::new();
        c.text(&format!("D{i}"), &mut s);
        s
    }

    #[test]
    fn a_date_and_its_cell_find_each_other_whichever_day_the_week_starts() {
        let t = today();
        for week_start in [0, 1] {
            let c = calendar(week_start);
            for day in 1..=30 {
                let (col, row) = c.cell_of((2026, 11, day), &t).unwrap();
                assert_eq!(c.date_at(row * 7 + col, &t), Some((2026, 11, day)), "week start {week_start}, day {day}");
            }
            // only this month's
            assert_eq!(c.cell_of((2026, 12, 1), &t), None);
        }
        // 1 November 2026 is a Sunday: the last column on Monday-first, the first on Sunday-first
        assert_eq!(calendar(1).cell_of((2026, 11, 1), &t), Some((6, 0)));
        assert_eq!(calendar(0).cell_of((2026, 11, 1), &t), Some((0, 0)));
        // a blank cell has no date
        assert_eq!(calendar(1).date_at(0, &t), None);
        assert_eq!(calendar(1).date_at(41, &t), None);
        assert_eq!(calendar(1).date_at(42, &t), None);
    }

    #[test]
    fn the_grid_leaves_the_highlighted_day_blank() {
        let t = today();
        let mut c = calendar(1);
        c.fill(&t, Some(20));
        let (col, row) = c.cell_of((2026, 11, 20), &t).unwrap();
        let cell = (row * 7 + col) as usize;
        assert_eq!((day_in(&c, cell - 1), day_in(&c, cell), day_in(&c, cell + 1)), ("19".into(), "".into(), "21".into()));
        let mut name = String::new();
        assert!(c.text("MonthName", &mut name) && c.text("YearText", &mut name));
        assert_eq!(name, "November2026");
        assert!(!c.text("Count", &mut name));
    }

    #[test]
    fn browsing_crosses_years_and_comes_back_to_following_the_date() {
        let t = today();
        let mut c = calendar(1);
        assert_eq!(c.command("Shift", 2, &t), Some(Answer::Moved));
        assert_eq!((c.month(&t), c.view), ((2027, 1), Some((2027, 1))));
        // the year arrows are twelve months
        c.command("Shift", -12, &t);
        assert_eq!(c.month(&t), (2026, 1));
        // back on this month, the grid follows the date again
        c.command("Shift", 10, &t);
        assert_eq!(c.view, None);
        c.show(2026, 11, &t);
        assert_eq!(c.view, None);
    }

    #[test]
    fn a_month_picked_from_the_list_closes_it() {
        let t = today();
        let mut c = calendar(1);
        c.command("Shift", 12, &t);
        assert_eq!(c.command("ToggleMonthList", 0, &t), Some(Answer::Repaint(Dirty::All)));
        assert_eq!((c.num("MLHide"), c.num("MLMonth")), (Some(0), Some(0)));
        // the bar follows the pointer, and only the row that has it lets go of it
        let list = Answer::Repaint(Dirty::Part(c.list));
        assert_eq!(c.command("HoverMonth", 3, &t), Some(list));
        assert_eq!(c.command("HoverMonth", 4, &t).map(|_| c.hovered), Some(4));
        assert_eq!(c.command("UnhoverMonth", 3, &t), Some(Answer::Repaint(Dirty::None)));
        assert_eq!(c.num("MLMonth"), Some(4));
        c.command("UnhoverMonth", 4, &t);
        assert_eq!(c.num("MLMonth"), Some(0));
        // March of the year on show
        assert_eq!(c.command("SetMonth", 3, &t), Some(Answer::Moved));
        assert_eq!((c.month(&t), c.num("MLHide")), ((2027, 3), Some(1)));
        assert_eq!(c.command("Select", 3, &t), None);
    }
}
