//! Disk Space: how full one drive is -- the space used and free on it, its
//! capacity, and the two as the pie a Win98 drive's Properties page draws.
//!
//! One drive at a time.  The spin buttons beside its name, or the wheel over
//! it, step to the drive before and the one after; the right-click menu has
//! them all by letter.  Which one is kept in settings.ini.
//!
//! Asking a drive for its figures is one system call, and for a drive on
//! this machine it comes straight back: those are asked on the tick, every
//! few seconds.  A network drive that is not there, a card reader, a disc
//! drive spinning up can each take many seconds to say so, and the tick is
//! every widget's -- so those are asked from a thread that lives as long as
//! the one question, and the answer is picked up on a later tick.  Nothing
//! is repainted unless something that is shown has changed.
//!
//! The pie is whole pixels with no smoothing, like the one it is after: each
//! pixel of [MeterPie] is one part of the picture or none of it, and the
//! ink is wherever two parts meet.  It is worked out once for each reading
//! that moves the slice, and kept.

use core::f32::consts::{PI, TAU};
use core::ptr::null_mut;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::canvas::Canvas;
use crate::skin::{self, Hooks, Meter, Skin};
use crate::sys;
use crate::text::Text;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Request, Widget};

static SKIN: &Skin = &skin::disk::SKIN;
const SECTION: &str = "Disk";

/// The slice is kept in this many parts of a turn: half degrees.  A drive
/// filling a byte at a time moves the pie only when it has moved that far.
const TURN: u16 = 720;

/// The longest label the field has room for with the drive's letter after
/// it, in characters: about what fits, the font not being one of fixed width.
const LABEL: usize = 16;

/// What the right-click menu calls each drive.
const NAMES: [&str; 26] = [
    "Drive A:", "Drive B:", "Drive C:", "Drive D:", "Drive E:", "Drive F:", "Drive G:",
    "Drive H:", "Drive I:", "Drive J:", "Drive K:", "Drive L:", "Drive M:", "Drive N:",
    "Drive O:", "Drive P:", "Drive Q:", "Drive R:", "Drive S:", "Drive T:", "Drive U:",
    "Drive V:", "Drive W:", "Drive X:", "Drive Y:", "Drive Z:",
];

/// What a letter stands for, as Windows sorts drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// No drive has the letter: a stick that has been pulled out.
    Missing,
    Removable,
    Fixed,
    Remote,
    Disc,
    Ram,
}

impl Kind {
    fn from(code: u32) -> Kind {
        match code {
            sys::DRIVE_REMOVABLE => Kind::Removable,
            sys::DRIVE_FIXED => Kind::Fixed,
            sys::DRIVE_REMOTE => Kind::Remote,
            sys::DRIVE_CDROM => Kind::Disc,
            sys::DRIVE_RAMDISK => Kind::Ram,
            _ => Kind::Missing,
        }
    }

    /// Known without touching the drive itself.
    fn of(letter: u8) -> Kind {
        Kind::from(unsafe { sys::GetDriveTypeW(root(letter).as_ptr()) })
    }

    /// The "Type:" line of the Properties page.
    fn name(self) -> &'static str {
        match self {
            Kind::Missing => "No Disk",
            Kind::Removable => "Removable Disk",
            Kind::Fixed => "Local Disk",
            Kind::Remote => "Network Drive",
            Kind::Disc => "Compact Disc",
            Kind::Ram => "RAM Disk",
        }
    }

    /// Whether asking it may keep the caller waiting.
    fn slow(self) -> bool {
        !matches!(self, Kind::Missing | Kind::Fixed | Kind::Ram)
    }

    /// Ticks from one reading to the next.
    fn every(self) -> u32 {
        if self.slow() { 30 } else { 5 }
    }
}

/// What a drive said of itself.
struct Reading {
    label: String,
    /// Bytes in all, and bytes free to this user; None from a drive that
    /// would not say -- one with no disc in it.
    space: Option<(u64, u64)>,
}

/// "C:\"
fn root(letter: u8) -> Vec<u16> {
    sys::wide(&format!("{}:\\", letter as char))
}

/// Ask the drive.  This is the call that can take its time.
fn read(letter: u8) -> Reading {
    let root = root(letter);
    let mut label = [0u16; 262];
    let (mut ours, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: every pointer is to something that outlives the call it is
    // handed to, and the ones left null are optional.
    unsafe {
        // an empty drive is to fail, not to put up "there is no disk in the drive"
        let mut before = 0;
        let quiet = sys::SetThreadErrorMode(sys::SEM_FAILCRITICALERRORS, &mut before) != 0;
        let named = sys::GetVolumeInformationW(
            root.as_ptr(), label.as_mut_ptr(), label.len() as u32,
            null_mut(), null_mut(), null_mut(), null_mut(), 0,
        ) != 0;
        let sized = sys::GetDiskFreeSpaceExW(root.as_ptr(), &mut ours, &mut total, &mut free) != 0;
        if quiet {
            sys::SetThreadErrorMode(before, null_mut());
        }
        Reading {
            label: if named { sys::from_wide(&label) } else { String::new() },
            space: sized.then_some((total, ours)),
        }
    }
}

/// Ask the drive from a thread of its own, which ends with the answer.  If
/// nobody is waiting for it by then -- another drive has been picked -- the
/// answer goes nowhere.
fn ask(letter: u8) -> Option<Receiver<Reading>> {
    let (answer, waiting) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || drop(answer.send(read(letter))))
        .ok()
        .map(|_| waiting)
}

/// Every letter that has a drive, in order.
fn drives() -> Vec<u8> {
    let mask = unsafe { sys::GetLogicalDrives() };
    (0..26u8).filter(|i| mask >> i & 1 != 0).map(|i| b'A' + i).collect()
}

/// The drive `by` places along from `from`, round the end of the list.  From
/// a letter that is not on it, the nearest one on the side asked for.
fn neighbour(drives: &[u8], from: u8, by: i32) -> Option<u8> {
    if drives.is_empty() {
        return None;
    }
    let at = match drives.binary_search(&from) {
        Ok(i) => i as i32 + by,
        Err(i) if by > 0 => i as i32 + by - 1,
        Err(i) => i as i32 + by,
    };
    Some(drives[at.rem_euclid(drives.len() as i32) as usize])
}

/// A letter out of settings.ini: "d", "D:" and "D:\" are all D.
fn letter_in(text: &str) -> Option<u8> {
    text.trim().bytes().next().filter(u8::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase())
}

/// A number of bytes the way Explorer writes one: three figures and a unit,
/// cut short rather than rounded up, and "0.97 GB" rather than "1000 MB".
fn size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    let (mut n, mut unit) = (bytes as f64, 0);
    while n >= 1000.0 && unit + 1 < UNITS.len() {
        n /= 1024.0;
        unit += 1;
    }
    let (places, shift) = match n {
        _ if unit == 0 => (0, 1.0),
        n if n < 10.0 => (2, 100.0),
        n if n < 100.0 => (1, 10.0),
        _ => (0, 1.0),
    };
    format!("{:.*} {}", places, (n * shift).floor() / shift, UNITS[unit])
}

// What a pixel of the pie is part of.
const OUT: u8 = 0;
const INK: u8 = 1;
const USED: u8 = 2;
const USED_SIDE: u8 = 3;
const FREE: u8 = 4;
const FREE_SIDE: u8 = 5;
const NONE: u8 = 6;
const NONE_SIDE: u8 = 7;

/// All that is drawn, as it is drawn: a reading that changes none of it
/// repaints nothing.
#[derive(Default, PartialEq)]
struct Shown {
    kind: &'static str,
    name: String,
    used: String,
    used_pct: String,
    free: String,
    free_pct: String,
    capacity: String,
    /// The free slice in `TURN`ths of a turn; None with no figures to draw.
    slice: Option<u16>,
}

pub struct Disk {
    /// b'C'
    letter: u8,
    kind: Kind,
    /// None until the drive has answered.
    reading: Option<Reading>,
    /// The answer a slow drive has been asked for.
    waiting: Option<Receiver<Reading>>,
    /// Ticks until the drive is asked again.
    left: u32,
    shown: Shown,

    /// The pie as the skin lays it out: [MeterPie]'s size, the rim's depth,
    /// and a colour for each part.
    w: i32,
    h: i32,
    depth: i32,
    colours: [u32; 8],
    /// The picture, a part a pixel, row after row.
    pie: Vec<u8>,

    /// For the test harness: what it was told to show, whatever the machine has.
    frozen: bool,
}

impl Disk {
    pub fn new(ctx: &Ctx) -> Disk {
        let (w, h) = (SKIN.int("PieW", 104).max(2), SKIN.int("PieH", 54).max(2));
        let system = std::env::var("SystemDrive").ok().and_then(|d| letter_in(&d));
        let mut disk = Disk {
            letter: letter_in(&ctx.settings.text(SECTION, "Drive", SKIN.var("Drive"))).or(system).unwrap_or(b'C'),
            kind: Kind::Missing,
            reading: None,
            waiting: None,
            // asked on the first tick, which comes before the first picture
            left: 0,
            shown: Shown::default(),
            w,
            h,
            depth: SKIN.int("PieDepth", 10).clamp(0, h - 1),
            colours: [
                0,
                SKIN.colour("Ink"),
                SKIN.colour("PieUsed"),
                SKIN.colour("PieUsedSide"),
                SKIN.colour("PieFree"),
                SKIN.colour("PieFreeSide"),
                SKIN.colour("PieNone"),
                SKIN.colour("PieNoneSide"),
            ],
            pie: Vec::new(),
            frozen: false,
        };
        disk.refresh();
        disk
    }

    /// For the test harness: this drive with these figures, never the
    /// machine's.  `kind` is Windows' own number for it; a `total` of 0 is a
    /// drive that would not say.
    pub fn set_fixed(&mut self, letter: u8, kind: u32, label: &str, total: u64, free: u64) {
        self.frozen = true;
        self.letter = letter.to_ascii_uppercase();
        self.kind = Kind::from(kind);
        self.waiting = None;
        self.reading = Some(Reading { label: label.to_owned(), space: (total > 0).then_some((total, free)) });
        self.refresh();
    }

    /// Ask the drive how it stands: there and then, or from a thread whose
    /// answer a later tick collects.
    fn poll(&mut self) {
        self.kind = Kind::of(self.letter);
        self.left = self.kind.every();
        match self.kind {
            Kind::Missing => self.reading = None,
            kind if kind.slow() => self.waiting = ask(self.letter),
            _ => self.reading = Some(read(self.letter)),
        }
    }

    fn select(&mut self, letter: u8, ctx: &mut Ctx) -> Outcome {
        if letter == self.letter {
            return Outcome::none();
        }
        self.letter = letter;
        ctx.settings.set(SECTION, "Drive", &(letter as char).to_string());
        // what was known, and what was on its way, was the other drive's
        self.reading = None;
        self.waiting = None;
        if !self.frozen {
            self.poll();
        }
        self.refresh();
        Outcome::all()
    }

    /// Work out what to show.  Says whether that is any different.
    fn refresh(&mut self) -> bool {
        let label = self.reading.as_ref().map_or("", |r| r.label.trim());
        let letter = self.letter as char;
        // A long label is cut short here, so that it is the label that loses
        // its end and not the letter after it.
        let name = match label.char_indices().nth(LABEL) {
            _ if label.is_empty() => format!("({letter}:)"),
            None => format!("{label} ({letter}:)"),
            Some(_) => {
                let kept = label.char_indices().nth(LABEL - 2).map_or(label, |(end, _)| &label[..end]);
                format!("{}... ({letter}:)", kept.trim_end())
            }
        };
        let mut shown = Shown { kind: self.kind.name(), name, ..Shown::default() };
        match self.reading.as_ref().and_then(|r| r.space).filter(|&(total, _)| total > 0) {
            Some((total, free)) => {
                let free = free.min(total);
                let share = free as f64 / total as f64;
                // the two shares are of one drive: they come to a hundred
                let free_pct = (share * 100.0).round() as u32;
                shown.used = size(total - free);
                shown.used_pct = format!("{}%", 100 - free_pct);
                shown.free = size(free);
                shown.free_pct = format!("{free_pct}%");
                shown.capacity = size(total);
                shown.slice = Some((share * TURN as f64).round() as u16);
            }
            None => {
                for figure in [&mut shown.used, &mut shown.free, &mut shown.capacity] {
                    figure.push_str("--");
                }
            }
        }
        if shown == self.shown && !self.pie.is_empty() {
            return false;
        }
        if shown.slice != self.shown.slice || self.pie.is_empty() {
            self.pie = self.picture(shown.slice);
        }
        self.shown = shown;
        true
    }

    /// What the pixel at (x, y) of the pie is part of, before the ink: the
    /// top or the rim, and of which slice.  The top is an ellipse as wide as
    /// the meter; the rim is the same ellipse `depth` rows lower and all
    /// that lies between the two.  The free slice starts at three o'clock
    /// and runs clockwise for its share of a turn -- across the front, so a
    /// drive with room on it shows that on the rim as well.
    fn part(&self, x: i32, y: i32, slice: Option<u16>) -> u8 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return OUT;
        }
        let (rx, ry) = (self.w as f32 / 2.0, (self.h - self.depth) as f32 / 2.0);
        // from the middle of the top, in radii, at the pixel's centre
        let (u, v) = ((x as f32 + 0.5 - rx) / rx, (y as f32 + 0.5 - ry) / ry);
        let top = u * u + v * v <= 1.0;
        if !top {
            let drop = self.depth as f32 / ry;
            let rim = u.abs() <= 1.0 && v >= 0.0 && (v <= drop || u * u + (v - drop) * (v - drop) <= 1.0);
            if !rim {
                return OUT;
            }
        }
        let Some(slice) = slice else { return if top { NONE } else { NONE_SIDE } };
        // How far round from three o'clock, clockwise: on the top the pixel's
        // own bearing, on the rim that of the edge straight above it.
        let turned = if top { v.atan2(u).rem_euclid(TAU) } else { u.clamp(-1.0, 1.0).acos().min(PI) };
        let free = slice >= TURN || turned / TAU * f32::from(TURN) < f32::from(slice);
        match (free, top) {
            (true, true) => FREE,
            (true, false) => FREE_SIDE,
            (false, true) => USED,
            (false, false) => USED_SIDE,
        }
    }

    /// The whole picture: each pixel its part, and ink where the picture
    /// ends or two parts meet -- round the top, round the rim, and down each
    /// side of a slice.
    fn picture(&self, slice: Option<u16>) -> Vec<u8> {
        let mut pie = Vec::with_capacity((self.w * self.h) as usize);
        for y in 0..self.h {
            for x in 0..self.w {
                let here = self.part(x, y, slice);
                let other = |dx: i32, dy: i32| self.part(x + dx, y + dy, slice);
                let (right, below) = (other(1, 0), other(0, 1));
                let edge = right == OUT || below == OUT || other(-1, 0) == OUT || other(0, -1) == OUT;
                pie.push(if here != OUT && (edge || right != here || below != here) { INK } else { here });
            }
        }
        pie
    }
}

impl Hooks for Disk {
    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "DriveKind" => self.shown.kind,
            "DriveName" => &self.shown.name,
            "UsedSize" => &self.shown.used,
            "UsedPct" => &self.shown.used_pct,
            "FreeSize" => &self.shown.free,
            "FreePct" => &self.shown.free_pct,
            "Capacity" => &self.shown.capacity,
            _ => "",
        });
    }

    /// [MeterPie]: the picture, a run of one colour at a time.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        if meter.name != "MeterPie" {
            return;
        }
        for (row, parts) in self.pie.chunks_exact(self.w as usize).enumerate() {
            let mut from = 0;
            while from < parts.len() {
                let part = parts[from];
                let run = parts[from..].iter().take_while(|&&p| p == part).count();
                if part != OUT {
                    canvas.fill(x + from as i32, y + row as i32, run as i32, 1, self.colours[part as usize]);
                }
                from += run;
            }
        }
    }
}

impl Widget for Disk {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Disk Space\n\n\
         How full a drive is: the space used and the space free on it, its \
         capacity, and the two as a pie.\n\n\
         Arrow buttons\tthe drive before, the drive after\n\
         Wheel\t\tthe same, over the drive's name\n\
         Right-click\t\tany drive, by its letter\n\n\
         A drive on this machine is read every five seconds.  A network \
         drive, a disc or a card is read every half minute, and nothing \
         waits for it while it answers."
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> Dirty {
        if self.frozen {
            return Dirty::None;
        }
        let mut news = false;
        if let Some(answer) = &self.waiting {
            match answer.try_recv() {
                Ok(reading) => {
                    self.reading = Some(reading);
                    self.waiting = None;
                    news = true;
                }
                Err(TryRecvError::Disconnected) => self.waiting = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        self.left = self.left.saturating_sub(1);
        // one question at a time: a drive that has not answered the last is
        // not asked another
        if self.left == 0 && self.waiting.is_none() {
            self.poll();
            news = true;
        }
        if news && self.refresh() { Dirty::All } else { Dirty::None }
    }

    fn command(&mut self, cmd: &str, arg: i32, _x: i32, _y: i32, ctx: &mut Ctx) -> Outcome {
        match cmd {
            "Step" => match neighbour(&drives(), self.letter, arg) {
                Some(letter) => self.select(letter, ctx),
                None => Outcome::none(),
            },
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        for letter in drives() {
            items.push(MenuItem { id: (letter - b'A') as u32 + 1, label: NAMES[(letter - b'A') as usize], checked: letter == self.letter });
        }
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            1..=26 => self.select(b'A' + (id - 1) as u8, ctx),
            _ => Outcome::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn disk(settings: &mut Settings) -> Disk {
        Disk::new(&Ctx { now: 1_000, settings, locked: false, signed_in: None })
    }

    #[test]
    fn sizes_are_three_figures_cut_short() {
        const KB: u64 = 1024;
        assert_eq!(size(0), "0 bytes");
        assert_eq!(size(999), "999 bytes");
        // a thousand of one unit is under one of the next, never four figures
        assert_eq!(size(1000), "0.97 KB");
        assert_eq!(size(KB), "1.00 KB");
        assert_eq!(size(1536), "1.50 KB");
        assert_eq!(size(KB * KB * 91 / 2), "45.5 MB");
        assert_eq!(size(KB * KB * KB * 366), "366 GB");
        // cut short, not rounded up: 1.999 GB is not yet 2
        assert_eq!(size(KB * KB * KB * 2 - 1), "1.99 GB");
        assert_eq!(size(KB * KB * KB * 1000), "0.97 TB");
        assert_eq!(size(u64::MAX), "16384 PB");
    }

    #[test]
    fn stepping_goes_round_the_drives_there_are() {
        let all = [b'C', b'D', b'G'];
        assert_eq!(neighbour(&all, b'C', 1), Some(b'D'));
        assert_eq!(neighbour(&all, b'G', 1), Some(b'C'));
        assert_eq!(neighbour(&all, b'C', -1), Some(b'G'));
        // from a drive that has gone, to the nearest on the side asked for
        assert_eq!(neighbour(&all, b'E', 1), Some(b'G'));
        assert_eq!(neighbour(&all, b'E', -1), Some(b'D'));
        assert_eq!(neighbour(&all, b'Z', 1), Some(b'C'));
        assert_eq!(neighbour(&all, b'A', -1), Some(b'G'));
        assert_eq!(neighbour(&[b'C'], b'C', 1), Some(b'C'));
        assert_eq!(neighbour(&[], b'C', 1), None);
    }

    #[test]
    fn the_drive_is_the_one_settings_names() {
        assert_eq!((letter_in("d"), letter_in(" E:\\ "), letter_in(""), letter_in("3")), (Some(b'D'), Some(b'E'), None, None));
        let mut settings = Settings::in_memory();
        settings.set(SECTION, "Drive", "q:");
        let mut disk = disk(&mut settings);
        assert_eq!(disk.letter, b'Q');

        // picking another writes it down, and forgets what the first one said
        disk.set_fixed(b'Q', sys::DRIVE_FIXED, "Data", 1000, 250);
        let mut ctx = Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None };
        assert_eq!(disk.menu_command(3, &mut ctx).dirty, Dirty::All);
        assert_eq!((disk.letter, disk.shown.name.as_str(), disk.shown.slice), (b'C', "(C:)", None));
        assert_eq!(disk.menu_command(3, &mut ctx).dirty, Dirty::None);
        assert_eq!(settings.get(SECTION, "Drive"), Some("C"));
    }

    #[test]
    fn the_figures_are_shown_and_the_shares_come_to_a_hundred() {
        const GB: u64 = 1 << 30;
        let mut settings = Settings::in_memory();
        let mut disk = disk(&mut settings);
        disk.set_fixed(b'c', sys::DRIVE_FIXED, "Windows", 476 * GB, 110 * GB);
        let s = &disk.shown;
        assert_eq!((s.kind, s.name.as_str()), ("Local Disk", "Windows (C:)"));
        assert_eq!((s.used.as_str(), s.free.as_str(), s.capacity.as_str()), ("366 GB", "110 GB", "476 GB"));
        assert_eq!((s.used_pct.as_str(), s.free_pct.as_str()), ("77%", "23%"));
        assert_eq!(s.slice, Some(166));

        // a drive that will not say: no figures, and a pie with no slices
        disk.set_fixed(b'E', sys::DRIVE_CDROM, "", 0, 0);
        let s = &disk.shown;
        assert_eq!((s.kind, s.name.as_str(), s.used.as_str(), s.used_pct.as_str(), s.slice), ("Compact Disc", "(E:)", "--", "", None));
        assert!(disk.pie.iter().all(|&p| matches!(p, OUT | INK | NONE | NONE_SIDE)));

        // the same figures again are nothing new
        assert!(!disk.refresh());

        // a label too long for the field loses its end, not the letter
        disk.set_fixed(b'Z', sys::DRIVE_REMOTE, "A very long share name indeed", 4 * GB, GB);
        assert_eq!(disk.shown.name, "A very long sh... (Z:)");
        disk.set_fixed(b'Z', sys::DRIVE_REMOTE, "Sixteen letters!", 4 * GB, GB);
        assert_eq!(disk.shown.name, "Sixteen letters! (Z:)");
    }

    #[test]
    fn the_free_slice_runs_clockwise_from_three_o_clock() {
        let mut settings = Settings::in_memory();
        let mut disk = disk(&mut settings);
        let (w, h, depth) = (disk.w, disk.h, disk.depth);
        let (cx, cy) = (w / 2, (h - depth) / 2);
        let at = |d: &Disk, x: i32, y: i32| d.pie[(y * d.w + x) as usize];

        // a quarter free: the quarter below and right of the middle
        disk.set_fixed(b'C', sys::DRIVE_FIXED, "", 400, 100);
        assert_eq!(at(&disk, cx + 20, cy + 6), FREE);
        assert_eq!(at(&disk, cx - 20, cy + 6), USED);
        assert_eq!(at(&disk, cx + 20, cy - 6), USED);
        assert_eq!(at(&disk, cx - 20, cy - 6), USED);
        // and that much of the rim under it
        assert_eq!(at(&disk, cx + 20, h - 6), FREE_SIDE);
        assert_eq!(at(&disk, cx - 20, h - 6), USED_SIDE);
        // inked where the two meet, and all round the outside
        assert_eq!(at(&disk, cx + 20, cy - 1), INK);
        assert_eq!(at(&disk, cx, 0), INK);
        assert_eq!(at(&disk, 0, cy + depth / 2), INK);
        assert_eq!(at(&disk, cx, h - 1), INK);
        assert_eq!((at(&disk, 0, 0), at(&disk, w - 1, h - 1)), (OUT, OUT));

        // full, and empty: one colour, with no slice to draw the side of
        disk.set_fixed(b'C', sys::DRIVE_FIXED, "", 400, 0);
        assert!(disk.pie.iter().all(|&p| matches!(p, OUT | INK | USED | USED_SIDE)));
        disk.set_fixed(b'C', sys::DRIVE_FIXED, "", 400, 400);
        assert!(disk.pie.iter().all(|&p| matches!(p, OUT | INK | FREE | FREE_SIDE)));
    }

    #[test]
    fn this_machine_has_a_drive_and_it_answers() {
        let all = drives();
        assert!(!all.is_empty() && all.windows(2).all(|pair| pair[0] < pair[1]));
        let mut settings = Settings::in_memory();
        let mut disk = disk(&mut settings);
        assert!(all.contains(&disk.letter), "the drive Windows is on is not among the drives");
        let mut ctx = Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None };
        assert_eq!(disk.tick(&mut ctx), Dirty::All);
        assert!(disk.shown.slice.is_some() && disk.shown.capacity != "--");
        // and is not asked again for a while
        assert_eq!(disk.tick(&mut ctx), Dirty::None);
        assert_eq!(disk.left, disk.kind.every() - 1);
    }
}
