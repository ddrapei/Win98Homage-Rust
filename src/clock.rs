//! Time, the way the widgets use it: seconds since 1970, broken-down local
//! time, and the trip back from a local date to a timestamp -- which goes
//! through the time-zone rules Windows keeps, so the two days a year a clock
//! changes come out 23 and 25 hours long, as they are.

use core::ptr::null;

use crate::sys;

/// Seconds between 1601-01-01 and 1970-01-01.
const EPOCH_1601: i64 = 11_644_473_600;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tm {
    pub year: i32,
    /// 1..=12
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub min: i32,
    pub sec: i32,
    /// 0 = Sunday .. 6 = Saturday
    pub wday: i32,
}

pub const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn filetime_to_unix(ft: &sys::FILETIME) -> (i64, u32) {
    let ticks = (ft.hi as u64) << 32 | ft.lo as u64;
    ((ticks / 10_000_000) as i64 - EPOCH_1601, ((ticks % 10_000_000) / 10_000) as u32)
}

fn unix_to_filetime(ts: i64) -> sys::FILETIME {
    let ticks = ((ts + EPOCH_1601).max(0) as u64) * 10_000_000;
    sys::FILETIME { lo: ticks as u32, hi: (ticks >> 32) as u32 }
}

/// A Windows FILETIME count (100 ns since 1601) as a Unix timestamp.
pub fn from_filetime(ticks: i64) -> i64 {
    ticks / 10_000_000 - EPOCH_1601
}

/// Now, and how far into the second it is in milliseconds.
pub fn now_ms() -> (i64, u32) {
    let mut ft = sys::FILETIME::default();
    unsafe { sys::GetSystemTimeAsFileTime(&mut ft) };
    filetime_to_unix(&ft)
}

pub fn now() -> i64 {
    now_ms().0
}

fn tm_of(st: &sys::SYSTEMTIME) -> Tm {
    Tm {
        year: st.wYear as i32,
        month: st.wMonth as i32,
        day: st.wDay as i32,
        hour: st.wHour as i32,
        min: st.wMinute as i32,
        sec: st.wSecond as i32,
        wday: st.wDayOfWeek as i32,
    }
}

pub fn utc(ts: i64) -> Tm {
    let mut st = sys::SYSTEMTIME::default();
    unsafe { sys::FileTimeToSystemTime(&unix_to_filetime(ts), &mut st) };
    tm_of(&st)
}

pub fn local(ts: i64) -> Tm {
    let (mut st, mut out) = (sys::SYSTEMTIME::default(), sys::SYSTEMTIME::default());
    unsafe {
        sys::FileTimeToSystemTime(&unix_to_filetime(ts), &mut st);
        if sys::SystemTimeToTzSpecificLocalTime(null(), &st, &mut out) == 0 {
            out = st;
        }
    }
    tm_of(&out)
}

/// Days since 1970-01-01 of a civil date (after Howard Hinnant).
pub fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let y = (if month <= 2 { year - 1 } else { year }) as i64;
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = ((month + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(days: i64) -> (i32, i32, i32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as i32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as i32;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
    (year, month, day)
}

/// 0 = Sunday .. 6 = Saturday.
pub fn weekday(year: i32, month: i32, day: i32) -> i32 {
    (days_from_civil(year, month, day) + 4).rem_euclid(7) as i32
}

pub fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Local wall-clock time to a timestamp, like mktime in C: a day past the end
/// of the month rolls into the next, which is how "tomorrow at midnight" is
/// asked for without adding 86400 to anything.
pub fn mktime(year: i32, month: i32, day: i32, hour: i32, min: i32, sec: i32) -> i64 {
    let (y, m, d) = civil_from_days(days_from_civil(year, month, 1) + (day - 1) as i64);
    let local = sys::SYSTEMTIME {
        wYear: y as u16,
        wMonth: m as u16,
        wDayOfWeek: 0,
        wDay: d as u16,
        wHour: hour as u16,
        wMinute: min as u16,
        wSecond: sec as u16,
        wMilliseconds: 0,
    };
    let mut utc_time = sys::SYSTEMTIME::default();
    let mut ft = sys::FILETIME::default();
    unsafe {
        if sys::TzSpecificLocalTimeToSystemTime(null(), &local, &mut utc_time) == 0 {
            utc_time = local;
        }
        sys::SystemTimeToFileTime(&utc_time, &mut ft);
    }
    filetime_to_unix(&ft).0
}

pub fn midnight(ts: i64) -> i64 {
    let t = local(ts);
    mktime(t.year, t.month, t.day, 0, 0, 0)
}

pub fn next_midnight(ts: i64) -> i64 {
    let t = local(ts);
    mktime(t.year, t.month, t.day + 1, 0, 0, 0)
}

/// "YYYY-MM-DD" in local time: how the usage log names a day.
pub fn day_key(ts: i64) -> String {
    let t = local(ts);
    key_of(t.year, t.month, t.day)
}

pub fn key_of(year: i32, month: i32, day: i32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

pub struct Zone {
    /// The name Windows gives the zone as it stands: its daylight name in summer.
    pub name: String,
    /// Minutes ahead of UTC right now.
    pub offset: i32,
    pub dst: bool,
}

pub fn zone() -> Zone {
    unsafe {
        let mut tz: sys::TIME_ZONE_INFORMATION = core::mem::zeroed();
        let dst = sys::GetTimeZoneInformation(&mut tz) == sys::TIME_ZONE_ID_DAYLIGHT;
        let (name, bias) = if dst { (&tz.DaylightName, tz.DaylightBias) } else { (&tz.StandardName, tz.StandardBias) };
        Zone { name: sys::from_wide(name), offset: -(tz.Bias + bias), dst }
    }
}

/// "2026-11-11 17:30:45" as local time; for the test harness.
pub fn parse_local(s: &str) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut d = date.split('-').map(|p| p.parse::<i32>().ok());
    let mut t = time.split(':').map(|p| p.parse::<i32>().ok());
    Some(mktime(d.next()??, d.next()??, d.next()??, t.next()??, t.next()??, t.next()??))
}
