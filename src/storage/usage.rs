//! The usage journal: when this computer was on, and when it was on and
//! unlocked.
//!
//! Two facts are enough to reconstruct a day: the program ticked at second T,
//! so the machine was on at T; and the workstation was or was not locked.  A
//! tick more than GAP_SECONDS after the last means the machine was off or
//! asleep in between, and the interval is closed at the last tick actually
//! seen.  Everything else is one continuous interval, tagged U or L.
//!
//! File format, one interval per line, always inside a single day:
//!     YYYY-MM-DD HH:MM:SS HH:MM:SS U           (U = unlocked, L = locked)
//!     YYYY-MM-DD HH:MM:SS HH:MM:SS U open      still running when written

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::clock;

/// A longer gap between ticks than this means the machine was away.
const GAP_SECONDS: i64 = 90;
/// How often the still-open interval is written out.
pub const FLUSH_SECONDS: i64 = 120;
/// Start the session at the Windows sign-in if the program started within
/// this long of it.
const SEED_SECONDS: i64 = 600;
/// An interval still running when last written may be picked up again this
/// long afterwards: a restart, not an absence.
const RESUME_SECONDS: i64 = FLUSH_SECONDS + GAP_SECONDS;
/// Closed unlocked stretches shorter than this are noise, not use.  Dropped
/// for display only; the journal keeps every interval as observed.
const MIN_UNLOCK: i64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub s: i64,
    pub e: i64,
    pub locked: bool,
    /// Written with "open": had not finished when the journal was saved.
    unfinished: bool,
}

struct Live {
    key: String,
    /// Where its day stops.
    ends: i64,
    s: i64,
    locked: bool,
    seen: i64,
}

pub struct Usage {
    /// None: nowhere writable, so this run is remembered in memory only.
    path: Option<PathBuf>,
    days: BTreeMap<String, Vec<Span>>,
    /// Bumped whenever an interval is stored.
    pub rev: u32,
    live: Option<Live>,
}

/// The first of the skin's three places that can be opened for appending.
/// Outside Documents on purpose: that is often OneDrive-redirected and
/// shielded by Controlled Folder Access.
fn journal_path(fallback_dir: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        candidates.push(local.join("Win98DateTime").join("Usage.log"));
        candidates.push(local.join("Win98DateTime.log"));
    }
    candidates.push(fallback_dir.join("Usage.log"));
    candidates.into_iter().find(|p| std::fs::OpenOptions::new().append(true).create(true).open(p).is_ok())
}

impl Usage {
    pub fn open(fallback_dir: &Path) -> Usage {
        Usage::at(journal_path(fallback_dir))
    }

    pub fn at(path: Option<PathBuf>) -> Usage {
        let mut usage = Usage { path, days: BTreeMap::new(), rev: 0, live: None };
        usage.load();
        usage
    }

    pub fn has(&self, key: &str) -> bool {
        self.days.contains_key(key)
    }

    /// Store one interval.  One that continues the previous (same lock state,
    /// no real gap) is merged into it, so a restart does not fragment the day.
    fn store(&mut self, key: &str, from: i64, to: i64, locked: bool) {
        if to <= from {
            return;
        }
        let day = self.days.entry(key.to_owned()).or_default();
        match day.last_mut() {
            Some(last) if last.locked == locked && from - last.e <= 1 => last.e = to,
            _ => day.push(Span { s: from, e: to, locked, unfinished: false }),
        }
        self.rev += 1;
    }

    fn begin(&mut self, started_at: i64, locked: bool, seen_at: i64) {
        self.live = Some(Live {
            key: clock::day_key(started_at),
            ends: clock::next_midnight(started_at),
            s: started_at,
            locked,
            seen: seen_at,
        });
    }

    fn end(&mut self, at: i64) {
        if let Some(live) = self.live.take() {
            self.store(&live.key, live.s, at, live.locked);
        }
    }

    fn load(&mut self) {
        self.days.clear();
        self.rev += 1;
        let Some(text) = self.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) else { return };
        for line in text.lines() {
            let Some((key, from, to, locked, open)) = parse_line(line) else { continue };
            self.store(key, from, to, locked);
            // Remember the tail that had not finished, so resume() can tell a
            // restart apart from the machine having been away.
            if let Some(last) = self.days.get_mut(key).and_then(|d| d.last_mut()) {
                last.unfinished = open;
            }
        }
    }

    /// Rewrites the whole journal: it is a few kilobytes, and a full rewrite
    /// is how the history gets pruned and how the open interval keeps a
    /// provisional end.
    pub fn save(&mut self, now: i64, history_days: i64) {
        let Some(path) = &self.path else { return };
        let cutoff = clock::day_key(now - (history_days.max(1) - 1) * 86_400);
        let mut out = String::from(
            "# Win98 Date/Time usage log -- delete this file to forget it.\n\
             # date       from     to       U = unlocked, L = locked\n",
        );
        let hms = |ts: i64| {
            let t = clock::local(ts);
            format!("{:02}:{:02}:{:02}", t.hour, t.min, t.sec)
        };
        let mut line = |key: &str, from: i64, to: i64, locked: bool, open: bool| {
            out.push_str(&format!(
                "{key} {} {} {}{}\n",
                hms(from),
                hms(to),
                if locked { 'L' } else { 'U' },
                if open { " open" } else { "" }
            ));
        };
        for (key, day) in self.days.range(cutoff..) {
            for span in day {
                line(key, span.s, span.e, span.locked, false);
            }
            if let Some(live) = self.live.as_ref().filter(|l| l.key == *key) {
                line(key, live.s, live.seen, live.locked, true);
            }
        }
        if let Some(live) = self.live.as_ref().filter(|l| !self.days.contains_key(&l.key)) {
            line(&live.key, live.s, live.seen, live.locked, true);
        }
        let _ = std::fs::write(path, out);
    }

    /// Called once, on the first tick.
    pub fn resume(&mut self, now: i64, locked: bool, signed_in: Option<i64>) {
        let key = clock::day_key(now);
        // The journal says this interval had not finished, and it was written
        // just now: pick it back up instead of leaving a hole in the day.
        if let Some(day) = self.days.get_mut(&key) {
            if let Some(last) = day.last().copied().filter(|l| l.unfinished && now - l.e <= RESUME_SECONDS) {
                day.pop();
                if day.is_empty() {
                    self.days.remove(&key);
                }
                self.rev += 1;
                self.begin(last.s, last.locked, now);
                return;
            }
        }
        // Fresh start.  If Windows says the user signed in a moment ago, the
        // program came up with the session and the session began at sign-in.
        match signed_in {
            Some(at) if at <= now && now - at <= SEED_SECONDS && at >= clock::midnight(now) => self.begin(at, locked, now),
            _ => self.begin(now, locked, now),
        }
    }

    /// One second of evidence about the machine.
    pub fn observe(&mut self, now: i64, locked: bool) {
        if let Some(live) = &self.live {
            if now - live.seen > GAP_SECONDS {
                let seen = live.seen;
                self.end(seen); // off, asleep, or not running
            } else if now >= live.ends {
                let last = live.ends - 1;
                self.end(last); // midnight: close the day out
            } else if live.locked != locked {
                self.end(now); // locked or unlocked just now
            }
        }
        match &mut self.live {
            Some(live) => live.seen = now,
            None => self.begin(now, locked, now),
        }
    }

    /// For the test harness: pretend the running interval began at `start`.
    pub fn set_live(&mut self, start: i64, locked: bool, now: i64) {
        self.begin(start, locked, now);
    }

    /// Closed intervals for a day, plus the open one clipped to now, as
    /// (start, end, locked).  Momentary unlocks are filtered out here rather
    /// than in the journal; the open interval never is -- a session that has
    /// just started must show up at once.
    pub fn intervals(&self, key: &str, now: i64) -> Vec<(i64, i64, bool)> {
        let mut out: Vec<(i64, i64, bool)> = self
            .days
            .get(key)
            .into_iter()
            .flatten()
            .filter(|s| s.locked || s.e - s.s >= MIN_UNLOCK)
            .map(|s| (s.s, s.e, s.locked))
            .collect();
        if let Some(live) = self.live.as_ref().filter(|l| l.key == key && now >= l.s) {
            out.push((live.s, now, live.locked));
        }
        out
    }
}

/// "2026-07-28 08:16:04 12:03:12 U open"
fn parse_line(line: &str) -> Option<(&str, i64, i64, bool, bool)> {
    let b = line.as_bytes();
    if b.len() < 30 || b[10] != b' ' || b[19] != b' ' || b[28] != b' ' {
        return None;
    }
    let num = |from: usize, len: usize| -> Option<i32> {
        let s = line.get(from..from + len)?;
        s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten()
    };
    if b[4] != b'-' || b[7] != b'-' || b[13] != b':' || b[16] != b':' || b[22] != b':' || b[25] != b':' {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let from = clock::mktime(y, mo, d, num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let to = clock::mktime(y, mo, d, num(20, 2)?, num(23, 2)?, num(26, 2)?);
    let locked = match b[29] {
        b'L' => true,
        b'U' => false,
        _ => return None,
    };
    Some((&line[..10], from, to, locked, line[30..].contains("open")))
}

/// Start of the latest run of unlocked time.  Adjacent unlocked intervals are
/// one session; locked time or a gap ends it.  Time spent locked at the end of
/// the day is skipped over, so stepping away does not erase the session.
pub fn latest_session(intervals: &[(i64, i64, bool)]) -> Option<i64> {
    let last = intervals.iter().rposition(|s| !s.2)?;
    let mut start = intervals[last].0;
    for span in intervals[..last].iter().rev() {
        if span.2 || start - span.1 > 2 {
            break;
        }
        start = span.0;
    }
    Some(start)
}
