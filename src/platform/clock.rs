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

/// Local wall-clock time to a timestamp, like mktime in C: a field past its
/// end rolls into the next -- a day past the end of the month into the next
/// month, hour 24 into the next day -- which is how "tomorrow at midnight" is
/// asked for without adding 86400 to anything.  Windows itself takes only
/// hours 0..23, and the rest must not reach it.
pub fn mktime(year: i32, month: i32, day: i32, hour: i32, min: i32, sec: i32) -> i64 {
    let time = hour as i64 * 3600 + min as i64 * 60 + sec as i64;
    let (days, time) = (time.div_euclid(86_400), time.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days_from_civil(year, month, 1) + (day - 1) as i64 + days);
    let local = sys::SYSTEMTIME {
        wYear: y as u16,
        wMonth: m as u16,
        wDayOfWeek: 0,
        wDay: d as u16,
        wHour: (time / 3600) as u16,
        wMinute: (time % 3600 / 60) as u16,
        wSecond: (time % 60) as u16,
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

/// Somewhere that keeps another time: one of the time zones Windows has the
/// rules of, summer time and all.
pub struct Elsewhere {
    rules: sys::DYNAMIC_TIME_ZONE_INFORMATION,
    convert: unsafe extern "system" fn(*const sys::DYNAMIC_TIME_ZONE_INFORMATION, *const sys::SYSTEMTIME, *mut sys::SYSTEMTIME) -> sys::BOOL,
}

/// The time zone Windows knows by this name -- "Tokyo Standard Time", the
/// name of its key in the registry -- if it knows one.
pub fn elsewhere(key: &str) -> Option<Elsewhere> {
    type Enumerate = unsafe extern "system" fn(u32, *mut sys::DYNAMIC_TIME_ZONE_INFORMATION) -> u32;
    if key.trim().is_empty() {
        return None;
    }
    // SAFETY: both are documented with these signatures, and are looked up
    // by name because not every import library has them.
    unsafe {
        let enumerate: Enumerate = core::mem::transmute(sys::proc_address("advapi32.dll", b"EnumDynamicTimeZoneInformation\0")?);
        let convert = core::mem::transmute(sys::proc_address("kernel32.dll", b"SystemTimeToTzSpecificLocalTimeEx\0")?);
        let mut rules: sys::DYNAMIC_TIME_ZONE_INFORMATION = core::mem::zeroed();
        // there are some hundred and forty; the list ends with an error
        for index in 0..1000 {
            if enumerate(index, &mut rules) != 0 {
                break;
            }
            if sys::from_wide(&rules.TimeZoneKeyName).eq_ignore_ascii_case(key.trim()) {
                return Some(Elsewhere { rules, convert });
            }
        }
    }
    None
}

impl Elsewhere {
    /// The time there.
    pub fn local(&self, ts: i64) -> Tm {
        let (mut st, mut out) = (sys::SYSTEMTIME::default(), sys::SYSTEMTIME::default());
        unsafe {
            sys::FileTimeToSystemTime(&unix_to_filetime(ts), &mut st);
            if (self.convert)(&self.rules, &st, &mut out) == 0 {
                out = st;
            }
        }
        tm_of(&out)
    }
}

/// What a clock showing `t` has counted since 1970, in seconds: the
/// difference between two of these, for the same moment, is how far apart
/// the two clocks stand.
pub fn reading(t: &Tm) -> i64 {
    days_from_civil(t.year, t.month, t.day) * 86_400 + (t.hour * 3600 + t.min * 60 + t.sec) as i64
}

/// The name Windows keeps a tz database zone under: "Asia/Tokyo" is "Tokyo
/// Standard Time".  ICU knows, and Windows carries ICU -- which is some
/// thirty megabytes, so this is for a child that is about to exit to ask,
/// never the program that stays.
pub fn windows_zone(tz: &str) -> Option<String> {
    type Lookup = unsafe extern "C" fn(*const u16, i32, *mut u16, i32, *mut i32) -> i32;
    let name = sys::wide(tz);
    let mut out = [0u16; 128];
    let mut status = 0;
    // icu.dll is all of ICU, since Windows 10 1903; before that it came in two
    let lookup = ["icu.dll", "icuin.dll"].iter().find_map(|dll| unsafe { sys::proc_address(dll, b"ucal_getWindowsTimeZoneID\0") })?;
    // SAFETY: the signature is ICU's; the lengths given are the buffers'.
    let len = unsafe {
        let lookup: Lookup = core::mem::transmute(lookup);
        lookup(name.as_ptr(), -1, out.as_mut_ptr(), out.len() as i32, &mut status)
    };
    // an error is above zero; a zone Windows has no name for comes back empty
    (status <= 0 && len > 0 && (len as usize) < out.len()).then(|| sys::from_wide(&out))
}

/// "2026-11-11 17:30:45" as local time; for the test harness.
pub fn parse_local(s: &str) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut d = date.split('-').map(|p| p.parse::<i32>().ok());
    let mut t = time.split(':').map(|p| p.parse::<i32>().ok());
    Some(mktime(d.next()??, d.next()??, d.next()??, t.next()??, t.next()??, t.next()??))
}

/// A moment as the web writes one -- "2026-11-20T19:00:00.123456+00:00", or
/// with a Z for the offset, or none at all, which is UTC -- as a timestamp.
/// Fractions of a second are dropped.
pub fn parse_iso_utc(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let mut d = date.split('-').map(|p| p.parse::<i32>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // the clock, then the offset: Z, +hh:mm, -hh:mm, +hhmm, or nothing
    let at = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (time, zone) = rest.split_at(at);
    let time = time.split('.').next()?;
    let mut t = time.split(':').map(|p| p.parse::<i32>().ok());
    let (hour, min) = (t.next()??, t.next()??);
    let sec = t.next().map_or(Some(0), |v| v)?;
    if t.next().is_some() || !(0..=23).contains(&hour) || !(0..=59).contains(&min) || !(0..=60).contains(&sec) {
        return None;
    }
    let ahead = match zone.as_bytes().first() {
        None | Some(b'Z' | b'z') if zone.len() <= 1 => 0,
        Some(&sign @ (b'+' | b'-')) => {
            let digits: String = zone[1..].chars().filter(|c| *c != ':').collect();
            if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let minutes = digits[..2].parse::<i64>().ok()? * 60 + digits[2..].parse::<i64>().ok()?;
            if sign == b'+' { minutes * 60 } else { -minutes * 60 }
        }
        _ => return None,
    };
    Some(days_from_civil(year, month, day) * 86_400 + (hour * 3600 + min * 60 + sec) as i64 - ahead)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_moment_as_the_web_writes_it() {
        // 2026-11-20 19:00:00 UTC
        let at = 1_795_201_200;
        assert_eq!(days_from_civil(2026, 11, 20) * 86_400 + 19 * 3600, at);
        assert_eq!(parse_iso_utc("2026-11-20T19:00:00.943648+00:00"), Some(at));
        assert_eq!(parse_iso_utc("2026-11-20T19:00:00Z"), Some(at));
        assert_eq!(parse_iso_utc("2026-11-20T19:00:00"), Some(at));
        assert_eq!(parse_iso_utc("2026-11-20T19:00Z"), Some(at));
        // an offset is how far ahead of UTC the clock that wrote it was
        assert_eq!(parse_iso_utc("2026-11-20T21:00:00+02:00"), Some(at));
        assert_eq!(parse_iso_utc("2026-11-20T14:30:00.5-0430"), Some(at));
        // into the next day, and the year after
        assert_eq!(parse_iso_utc("2026-12-31T23:59:59Z").map(|t| t + 1), parse_iso_utc("2027-01-01T00:00:00Z"));
        for bad in ["", "null", "2026-11-20", "2026-13-01T00:00:00Z", "2026-11-20T25:00:00Z", "2026-11-20T19:00:00+2", "2026-11-20T19:00:00Q"] {
            assert_eq!(parse_iso_utc(bad), None, "{bad}");
        }
    }

    #[test]
    fn mktime_rolls_a_time_past_its_end_into_the_next_day() {
        // hour 24 is the midnight that ends the day: the end of a graph that
        // runs to the end of the day is asked for that way
        assert_eq!(mktime(2026, 11, 18, 24, 0, 0), mktime(2026, 11, 19, 0, 0, 0));
        assert_eq!(mktime(2026, 12, 31, 24, 0, 0), mktime(2027, 1, 1, 0, 0, 0));
        assert_eq!(mktime(2026, 11, 18, 25, 90, 61), mktime(2026, 11, 19, 2, 31, 1));
        // and back, before the day began
        assert_eq!(mktime(2026, 11, 18, -1, 0, 0), mktime(2026, 11, 17, 23, 0, 0));
        assert_eq!(mktime(2026, 11, 18, 0, 0, -1), mktime(2026, 11, 17, 23, 59, 59));
        // what Windows is handed stays the same for a time that needs no rolling
        let t = local(mktime(2026, 11, 18, 23, 59, 59));
        assert_eq!((t.year, t.month, t.day, t.hour, t.min, t.sec), (2026, 11, 18, 23, 59, 59));
    }
}
