//! The Sticky Note: a text file on a sheet of paper, with checkboxes in the
//! margin and a Win98 scrollbar, typed on where it lies.
//!
//! The file is the note.  A line that opens with `[ ]` or `[x]` is an item:
//! its brackets are never shown, a checkbox in the margin stands for them, and
//! a click on the box flips the marker.  A click on the text puts a caret
//! there, and from then until something else is clicked the keyboard is the
//! note's.
//!
//! There is one picture of the note, not one for reading and another for
//! typing.  `editor` holds the text and wraps it into rows; this draws those
//! rows -- checkbox, words, strike-through -- and, while the note has the
//! keyboard, a selection and a caret on top.  Nothing is laid over the sheet
//! and nothing on it moves when the typing starts or stops.
//!
//! The rows are measured with the font they are drawn in (`Text::advances`),
//! so a line breaks where its words actually stop fitting, in any language.
//!
//! Saving is the note's own business: a second after the typing pauses, and
//! again when the keyboard goes elsewhere.  The first write of each spell of
//! typing copies the note as it was to `.bak`.
//!
//! There can be any number of notes.  Each is a section of settings.ini and
//! the file that section names -- `[Notes]` and Notes.txt for the first,
//! `[Notes2]` and Notes2.txt for the next -- so a `Notes` is made for a
//! section, and what is typed on one, and the paper it is typed on, are that
//! one's alone.  The host makes the windows; which sections are notes, and
//! starting and forgetting one, are here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::canvas::{Canvas, Rect};
use crate::clock;
use crate::editor::{self, Doc, Row, TAB};
use crate::keys::Key;
use crate::settings::{Settings, write_whole};
use crate::skin::{self, Hooks, Meter, Skin};
use crate::text::{Spec, Text};
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Pressed, Request, Widget};

static SKIN: &Skin = &skin::notes::SKIN;
/// The first note's section in settings.ini.  Every other note has one named
/// after it with its number on the end, from 2 up: `[Notes2]`, `[Notes3]`.
pub const FIRST: &str = "Notes";

/// Px of the text column kept clear on the right, for the caret that stands
/// after a row that fills it.
const SLACK: i32 = 2;
/// A line indented past this much of the column keeps its words and loses
/// its indent.
const MIN_ROOM: f32 = 24.0;
/// Px below a row's top where the line through a done item sits.
const STRIKE_DY: i32 = 6;
/// The thumb never gets shorter than the arrow buttons it runs between.
const THUMB_MIN: i32 = 11;
/// Rows a page-click, or Page Down, leaves on screen.
const PAGE_KEEP: i32 = 1;
/// Px a selected line break is shown as, at the end of its row.
const BREAK_W: i32 = 4;
/// The scrollbar's width: the sheet ends where it starts.
const BAR_W: i32 = 11;
/// Px to the left of the text column that still count as text: a click just
/// short of the first letter is a click before it.
const REACH: i32 = 2;
const BOM: &[u8] = b"\xEF\xBB\xBF";

// ---------------------------------------------------------------------------
//  The note on disk
// ---------------------------------------------------------------------------

/// What the file looked like, so that it is written back the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Format {
    bom: bool,
    crlf: bool,
}

/// Windows-1252 0x80..0x9F, the one block where it is not Latin-1.
const CP1252: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D,
    0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
    0x0153, 0x009D, 0x017E, 0x0178,
];

fn from_1252(b: u8) -> char {
    let cp = if (0x80..=0x9F).contains(&b) { CP1252[(b - 0x80) as usize] as u32 } else { b as u32 };
    char::from_u32(cp).unwrap_or('\u{FFFD}')
}

/// File bytes to the text that is typed on: characters, with `\n` between the
/// lines whatever ended them in the file, and without the line break the file
/// ends in.  A note that is not valid UTF-8 is from before the program was,
/// and is read as Windows-1252.
fn decode(bytes: &[u8]) -> (Vec<char>, Format) {
    let (bom, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let crlf = body.windows(2).any(|w| w == b"\r\n");
    let raw: Vec<char> = match core::str::from_utf8(body) {
        Ok(s) => s.chars().collect(),
        Err(_) => body.iter().map(|&b| from_1252(b)).collect(),
    };
    let mut text = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == '\r' {
            text.push('\n');
            if raw.get(i + 1) == Some(&'\n') {
                i += 1;
            }
        } else {
            text.push(raw[i]);
        }
        i += 1;
    }
    if text.last() == Some(&'\n') {
        text.pop();
    }
    (text, Format { bom, crlf })
}

/// The text back to file bytes: UTF-8, in the file's own line endings, ending
/// in one line break when there is anything in it.
fn encode(text: &[char], format: Format) -> Vec<u8> {
    let eol = if format.crlf { "\r\n" } else { "\n" };
    let mut s = String::with_capacity(text.len() + 2);
    for &c in text {
        if c == '\n' { s.push_str(eol) } else { s.push(c) }
    }
    if !text.is_empty() {
        s.push_str(eol);
    }
    let mut out = if format.bom { BOM.to_vec() } else { Vec::new() };
    out.extend_from_slice(s.as_bytes());
    out
}

/// A missing file is not an error: it is a note nobody has started.
fn read_optional(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Where the note as it was before the latest spell of typing is kept.
fn bak_of(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    PathBuf::from(bak)
}

// ---------------------------------------------------------------------------
//  More than one note
// ---------------------------------------------------------------------------
//
// A note is a section of settings.ini -- where it sits, its paper, the name
// of its file -- and that file.  There are as many notes as there are such
// sections, and nothing of one is shared with another.

/// Which note a section is: 1 for `[Notes]`, n for `[Notes<n>]`, and None for
/// a section that is not a note's.
fn number(section: &str) -> Option<u32> {
    let rest = section.get(FIRST.len()..).filter(|_| section[..FIRST.len()].eq_ignore_ascii_case(FIRST))?;
    if rest.is_empty() {
        return Some(1);
    }
    rest.parse().ok().filter(|n: &u32| *n >= 2 && n.to_string() == rest)
}

/// The file a note is kept in unless settings.ini names another: the first
/// note's, with the note's number put after its name.
fn default_file(n: u32) -> String {
    let first = SKIN.var("NoteFile");
    match first.rsplit_once('.') {
        _ if n < 2 => first.to_owned(),
        Some((stem, ext)) => format!("{stem}{n}.{ext}"),
        None => format!("{first}{n}"),
    }
}

/// Where a note's text is kept: the name as settings.ini has it, and the file
/// that names -- beside settings.ini, unless the name says where itself.
fn place(settings: &Settings, section: &str) -> (String, PathBuf) {
    let name = settings.text(section, "NoteFile", &default_file(number(section).unwrap_or(1)));
    let path = PathBuf::from(&name);
    let path = if path.is_absolute() { path } else { settings.dir().join(path) };
    (name, path)
}

pub fn file(settings: &Settings, section: &str) -> PathBuf {
    place(settings, section).1
}

/// What a note is called in the menus: the first is the Sticky Note, and the
/// others have their numbers.
pub fn name(section: &str) -> String {
    match number(section) {
        Some(n) if n > 1 => format!("Sticky Note {n}"),
        _ => String::from("Sticky Note"),
    }
}

/// The sections of the notes after the first, in the order of their numbers.
pub fn others(settings: &Settings) -> Vec<String> {
    let mut found: Vec<(u32, &str)> =
        settings.sections().filter_map(|s| Some((number(s).filter(|&n| n > 1)?, s))).collect();
    found.sort();
    found.dedup_by_key(|f| f.0);
    found.into_iter().map(|(_, s)| s.to_owned()).collect()
}

/// Start another note: a section of its own, and an empty file of its own
/// for it to name.  Returns the section.
pub fn create(settings: &mut Settings) -> String {
    let dir = settings.dir();
    let taken: Vec<u32> = settings.sections().filter_map(number).collect();
    // A number nothing has: not a note there is, and not a file left behind
    // by one there was -- the new note would open with that one's text on it.
    let n = (2..).find(|n| !taken.contains(n) && !dir.join(default_file(*n)).exists()).unwrap_or(u32::MAX);
    let (section, name) = (format!("{FIRST}{n}"), default_file(n));
    // An empty file rather than none: a sheet to type on, not one that says
    // its file is missing.  If it cannot be made the note says that instead.
    let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&name));
    settings.set(&section, "NoteFile", &name);
    section
}

/// Whether there is nothing on a note: no file, or only blank space in it.
/// A file that cannot be read is not known to be empty.
pub fn is_blank(path: &Path) -> bool {
    match read_optional(path) {
        Ok(Some(bytes)) => decode(&bytes).0.iter().all(|c| c.is_whitespace()),
        Ok(None) => true,
        Err(_) => false,
    }
}

/// Take a note out of settings.ini.  What is returned is what it leaves on
/// disk, for the caller to dispose of: its text and the copy from before the
/// last edit -- unless another note is kept in the same file.
pub fn forget(settings: &mut Settings, section: &str) -> Vec<PathBuf> {
    let path = file(settings, section);
    settings.remove_section(section);
    if settings.sections().any(|s| number(s).is_some() && file(settings, s) == path) {
        return Vec::new();
    }
    [bak_of(&path), path].into_iter().filter(|p| p.is_file()).collect()
}

/// The file's size and time: enough to tell whether it needs reading again.
fn stamp_of(path: &Path) -> Option<(u64, SystemTime)> {
    std::fs::metadata(path).ok().and_then(|m| Some((m.len(), m.modified().ok()?)))
}

/// The advance of each character of a line, a tab counting as `TAB` spaces --
/// which is also how it is drawn.
fn advances(measure: &mut Option<Text>, line: &[char]) -> Vec<f32> {
    let Some(text) = measure else {
        return line.iter().map(|&c| if c == '\t' { 6.0 * TAB as f32 } else { 6.0 }).collect();
    };
    let mut shown = String::with_capacity(line.len());
    show(line, &mut shown);
    let mut each = Vec::new();
    text.advances(0, &shown, &mut each);
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    for &c in line {
        let n = if c == '\t' { TAB } else { 1 };
        out.push(each[i..i + n].iter().sum());
        i += n;
    }
    out
}

/// The characters as they are drawn.
fn show(chars: &[char], out: &mut String) {
    for &c in chars {
        if c == '\t' {
            out.extend(core::iter::repeat_n(' ', TAB));
        } else {
            out.push(c);
        }
    }
}

/// The papers a note can be on: Paper1, Paper2, ... of the skin, for as long
/// as there is a next one.  A note's `Colour` in settings.ini is the number.
fn papers() -> Vec<u32> {
    let found: Vec<u32> = (1..).map_while(|i| skin::parse_colour(SKIN.var(&format!("Paper{i}")))).collect();
    if found.is_empty() { vec![SKIN.colour("FieldBg")] } else { found }
}

/// A fractional x as the pixel column it falls on.
fn px(x: f32) -> i32 {
    (x + 0.5).floor() as i32
}

// ---------------------------------------------------------------------------
//  The widget
// ---------------------------------------------------------------------------

pub struct Notes {
    pad: i32,
    rows_shown: i32,
    pitch: i32,
    first_y: i32,
    text_x: i32,
    text_w: i32,
    text_dy: i32,
    /// The sheet -- the paper left of the scrollbar -- measured from the
    /// frame's corner, as every figure here is.
    paper: Rect,
    trough_x: i32,
    trough_y: i32,
    trough_h: i32,
    ink: u32,
    faded: u32,
    select_bg: u32,
    select_ink: u32,
    /// The papers the swatch button goes round; never empty.
    papers: Vec<u32>,
    palette: [u32; 5],
    /// The font the rows are drawn in, as the host's `Text` numbers it.
    font: usize,
    autosave_ms: u32,
    /// The stamps the sheet is printed with, a row at a time.
    box_meter: Option<&'static Meter>,
    tick_meter: Option<&'static Meter>,
    rule_meter: Option<&'static Meter>,
    /// The same font, to measure with.
    measure: Option<Text>,
    /// The advances of each line now on the sheet, by its text: typing
    /// changes one line, and only that one is measured again.
    widths: HashMap<String, Vec<f32>>,

    /// The section of settings.ini that is this note's, and no other's.
    section: String,
    path: PathBuf,
    filename: String,
    /// The file as last seen: its size and time, to skip re-reading it.
    stamp: Option<(u64, SystemTime)>,
    /// The file has been read, or found not to exist.
    loaded: bool,
    /// The bytes on disk; None is "no such file".
    source: Option<Vec<u8>>,
    format: Format,

    doc: Doc,
    rows: Vec<Row>,
    /// Which revision of the text `rows` was wrapped from.
    laid: u64,
    /// Index in `rows` of the topmost visible row.
    top: usize,
    words: usize,
    /// The sheet has changed since `refresh` last looked.
    stale: bool,

    colour: usize,
    ruled: bool,
    created: String,
    /// The note has the keyboard.
    editing: bool,
    caret_on: bool,
    /// The text is not what is on disk.
    unsaved: bool,
    /// The last attempt to write it did not work.
    failed: bool,
    /// The next write is the first of this spell of typing: the note as it
    /// was goes to .bak first.
    bak_due: bool,
    /// Ms since the last change, while there is one to save.
    idle_ms: u32,
    notice: Option<(&'static str, u32)>,
    ticks: u32,

    thumb_y: i32,
    thumb_h: i32,
    can_up: bool,
    can_down: bool,
    status: String,
}

impl Notes {
    /// The note that `section` of settings.ini describes, on the file that
    /// names -- or on `file`, for a note that is only being looked at.
    pub fn new(ctx: &mut Ctx, section: &str, file: Option<PathBuf>) -> Notes {
        let n = |name: &str, default: i32| SKIN.int(name, default);
        let (filename, kept) = place(ctx.settings, section);
        let path = file.unwrap_or(kept);
        let papers = papers();
        let colour = (ctx.settings.int(section, "Colour", 1).clamp(1, papers.len() as i32) - 1) as usize;
        let ruled = ctx.settings.flag(section, "Ruled", n("Ruled", 0) != 0);

        // The first run stamps the note with the day it was started and never
        // touches it again; any date already there is shown as it stands.
        let mut created = ctx.settings.text(section, "Created", "");
        if created.trim().is_empty() {
            let t = clock::local(ctx.now);
            created = format!("{} {} {}", t.day, &clock::MONTHS[t.month as usize - 1][..3], t.year);
            ctx.settings.set(section, "Created", &created);
        }

        let points = SKIN.num("FontSize", 8.0) as f32;
        let (paper_x, paper_y) = (n("PaperX", 8), n("PaperY", 29));
        let (trough_y, trough_h) = (n("TroughY", 40), n("TroughH", 136));
        let mut notes = Notes {
            pad: n("Pad", 12),
            rows_shown: n("Rows", 12).max(1),
            pitch: n("RowPitch", 13),
            first_y: n("Row1Y", 30),
            text_x: n("TextX", 22),
            text_w: n("TextW", 146),
            text_dy: n("TextDY", 0),
            paper: Rect::xywh(paper_x, paper_y, n("PaperW", 163), n("PaperH", 158)),
            trough_x: n("TroughX", 171),
            trough_y,
            trough_h,
            ink: SKIN.colour("Ink"),
            faded: SKIN.colour("Shadow"),
            select_bg: SKIN.colour("SelectBg"),
            select_ink: SKIN.colour("SelectInk"),
            papers,
            palette: ["Face", "Hilite", "DkShadow", "Light", "Shadow"].map(|c| SKIN.colour(c)),
            font: skin::FONTS.iter().position(|&(pt, bold)| pt == points && !bold).unwrap_or(0),
            autosave_ms: n("AutoSave", 1000).max(0) as u32,
            box_meter: SKIN.meter("MeterBox"),
            tick_meter: SKIN.meter("MeterTick"),
            rule_meter: SKIN.meter("MeterRule"),
            measure: Text::new(SKIN.var("Font"), &[(points, false)]),
            widths: HashMap::new(),
            section: section.to_owned(),
            filename: path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(filename),
            path,
            stamp: None,
            loaded: false,
            source: None,
            format: Format::default(),
            doc: Doc::new(),
            rows: Vec::new(),
            laid: 0,
            top: 0,
            words: 0,
            stale: true,
            colour,
            ruled,
            created,
            editing: false,
            caret_on: false,
            unsaved: false,
            failed: false,
            bak_due: false,
            idle_ms: 0,
            notice: None,
            ticks: 0,
            thumb_y: trough_y,
            thumb_h: trough_h,
            can_up: false,
            can_down: false,
            status: String::new(),
        };
        if !notes.reload() {
            notes.layout();
        }
        notes.refresh();
        notes
    }

    fn say(&mut self, text: &'static str) {
        self.notice = Some((text, 4));
    }

    // ---- the note file --------------------------------------------------------

    /// Read the file, if it is not the one already on the sheet.  True if
    /// the sheet changed.
    fn reload(&mut self) -> bool {
        let stamp = stamp_of(&self.path);
        if self.loaded && stamp.is_some() && stamp == self.stamp {
            return false;
        }
        // There but not to be read just now: leave the sheet as it is and
        // look again later.  Taking it for empty would end in writing over it.
        let Ok(bytes) = read_optional(&self.path) else { return false };
        self.stamp = stamp;
        if self.loaded && bytes == self.source {
            return false;
        }
        self.loaded = true;
        let (text, format) = bytes.as_deref().map(decode).unwrap_or_default();
        self.format = format;
        self.source = bytes;
        self.doc.load(text);
        self.layout();
        true
    }

    /// Write the text out.  On a failure it stays unsaved, and the same call
    /// can simply be made again.
    fn save(&mut self) -> bool {
        let ok = self.write();
        self.failed = !ok;
        if ok {
            self.unsaved = false;
        }
        ok
    }

    fn write(&mut self) -> bool {
        if !self.loaded {
            return false;
        }
        let bytes = encode(self.doc.text(), self.format);
        let same = match self.source.as_deref() {
            Some(disk) => disk == bytes.as_slice(),
            None => bytes.is_empty(),
        };
        if same {
            return true;
        }
        // The note as it was before this spell of typing: once a spell, not
        // once a save, or the backup would only ever be a second old.  It is
        // read from the disk, so if something else wrote the note meanwhile
        // that version is the one kept.
        if self.bak_due {
            match read_optional(&self.path) {
                Ok(Some(old)) => {
                    if std::fs::write(bak_of(&self.path), old).is_err() {
                        return false;
                    }
                }
                Ok(None) => {}
                Err(_) => return false,
            }
            self.bak_due = false;
        }
        if write_whole(&self.path, &bytes).is_err() {
            return false;
        }
        self.stamp = stamp_of(&self.path);
        self.source = Some(bytes);
        true
    }

    // ---- the sheet ------------------------------------------------------------

    /// Wrap the text into rows, and count its words.
    fn layout(&mut self) {
        let mut known = core::mem::take(&mut self.widths);
        let mut fresh: HashMap<String, Vec<f32>> = HashMap::new();
        let measure = &mut self.measure;
        let room = (self.text_w - SLACK) as f32;
        self.rows = editor::layout(self.doc.text(), room, MIN_ROOM, &mut |line: &[char]| {
            let key: String = line.iter().collect();
            if let Some(adv) = fresh.get(&key) {
                return adv.clone();
            }
            let adv = known.remove(&key).unwrap_or_else(|| advances(measure, line));
            fresh.insert(key, adv.clone());
            adv
        });
        self.widths = fresh;
        self.laid = self.doc.rev();
        // Counted off the shown part of each line, which is what makes a
        // checkbox marker not a word.
        self.words = self
            .doc
            .text()
            .split(|&c| c == '\n')
            .map(|line| {
                let shown = &line[editor::marker(line).map_or(0, |m| m.hidden)..];
                shown.split(|c| c.is_whitespace()).filter(|w| !w.is_empty()).count()
            })
            .sum();
        self.stale = true;
    }

    /// The last value `top` may take.
    fn last_top(&self) -> usize {
        self.rows.len().saturating_sub(self.rows_shown as usize)
    }

    /// The scrollbar that says where the window of rows is, and the status
    /// panel.  True if anything on screen changed.
    fn refresh(&mut self) -> bool {
        let shown = self.rows_shown as usize;
        let extent = self.rows.len();
        let last = self.last_top();
        let top = self.top.min(last);

        // The thumb is as much of the trough as is on screen, where it is.
        // With nothing to scroll it fills the trough and both arrows go grey.
        let (mut thumb_y, mut thumb_h) = (self.trough_y, self.trough_h);
        if extent > shown {
            thumb_h = ((self.trough_h as f64 * shown as f64 / extent as f64 + 0.5).floor() as i32).max(THUMB_MIN);
            thumb_y = self.trough_y
                + ((self.trough_h - thumb_h) as f64 * top as f64 / (extent - shown) as f64 + 0.5).floor() as i32;
        }

        // The right panel counts the words; something that went wrong
        // interrupts it, and stays for as long as it is still wrong.
        let status = if self.failed {
            String::from("not saved")
        } else if let Some((text, _)) = self.notice {
            text.to_owned()
        } else if self.source.is_none() && !self.unsaved {
            String::from("no file")
        } else {
            match self.words {
                0 => String::from("empty"),
                1 => String::from("1 word"),
                n => format!("{n} words"),
            }
        };

        let changed = core::mem::take(&mut self.stale)
            || (top, thumb_y, thumb_h, top > 0, top < last) != (self.top, self.thumb_y, self.thumb_h, self.can_up, self.can_down)
            || status != self.status;
        self.top = top;
        self.thumb_y = thumb_y;
        self.thumb_h = thumb_h;
        self.can_up = top > 0;
        self.can_down = top < last;
        self.status = status;
        changed
    }

    /// Scroll just far enough that the caret's row is on the sheet.
    fn show_caret(&mut self) {
        let (r, _) = self.doc.locate(&self.rows);
        let shown = self.rows_shown as usize;
        if r < self.top {
            self.top = r;
        } else if r >= self.top + shown {
            self.top = r + 1 - shown;
        }
    }

    /// After anything the keyboard or the pointer did: wrap again if the text
    /// changed, keep the caret in sight, and draw.
    fn edited(&mut self) -> Outcome {
        if self.doc.rev() != self.laid {
            self.layout();
            self.unsaved = true;
            self.idle_ms = 0;
        }
        self.caret_on = true;
        self.show_caret();
        self.stale = true;
        self.refresh();
        Outcome::all()
    }

    /// Whether a point of the frame is on the text, rather than in the margin
    /// or off the sheet.
    fn on_text(&self, fx: i32, fy: i32) -> bool {
        self.paper.contains(fx, fy) && fx >= self.text_x - REACH
    }

    /// Whether a point of the window is on the sheet or its scrollbar: the
    /// part of the note that can be clicked without the typing being over.
    fn on_sheet(&self, x: i32, y: i32) -> bool {
        Rect::new(self.paper.x0, self.paper.y0, self.trough_x + BAR_W, self.paper.y1).contains(x - self.pad, y - self.pad)
    }

    /// The row slot a y of the frame falls in.  The paper stands a pixel
    /// proud of the rows at the top and the foot: those two slivers belong to
    /// the row they touch.
    fn slot_at(&self, fy: i32) -> usize {
        (fy - self.first_y).div_euclid(self.pitch).clamp(0, self.rows_shown - 1) as usize
    }

    /// The place in the text under a point of the frame.
    fn place_at(&self, fx: i32, fy: i32) -> (usize, bool) {
        let r = self.top + self.slot_at(fy);
        if r >= self.rows.len() {
            return (self.doc.text().len(), false);
        }
        Doc::hit(&self.rows, r, (fx - self.text_x) as f32)
    }

    /// Where the caret is drawn, as a rectangle of the frame; None while it
    /// is scrolled out of sight.
    fn caret_box(&self) -> Option<Rect> {
        let (r, x) = self.doc.locate(&self.rows);
        let slot = r.checked_sub(self.top).filter(|&s| s < self.rows_shown as usize)? as i32;
        // a caret after blanks that run past the column waits at its edge
        let cx = self.text_x + px(x).min(self.text_w - 1);
        Some(Rect::xywh(cx, self.first_y + slot * self.pitch, 1, self.pitch))
    }

    // ---- commands -------------------------------------------------------------

    /// The arrows and the wheel: a row at a time.  The caret stays where it
    /// is in the text, on the sheet or off it.
    fn scroll(&mut self, by: i32) -> Outcome {
        let top = (self.top as i64 + by as i64).clamp(0, self.last_top() as i64) as usize;
        if top == self.top {
            return Outcome::none();
        }
        self.top = top;
        self.stale = true;
        self.refresh();
        Outcome::all()
    }

    /// A click in the trough: page towards it, or sit still if it landed on
    /// the thumb.  `y` is relative to the trough, so it becomes a frame
    /// coordinate before it is compared with the thumb.
    fn page(&mut self, y: i32) -> Outcome {
        let y = y + self.trough_y;
        if y >= self.thumb_y && y < self.thumb_y + self.thumb_h {
            return Outcome::none();
        }
        let page = (self.rows_shown - PAGE_KEEP).max(1);
        self.scroll(if y < self.thumb_y { -page } else { page })
    }

    /// A click on the paper that was not a press on the text, as a point
    /// relative to the paper: the margin, where the checkboxes are.  A tick
    /// is written at once, typing or not.
    fn click(&mut self, x: i32, y: i32) -> Outcome {
        let (fx, fy) = (x + self.paper.x0, y + self.paper.y0);
        if self.on_text(fx, fy) || !self.loaded {
            return Outcome::none();
        }
        let Some(line) = self.rows.get(self.top + self.slot_at(fy)).filter(|r| r.head && r.todo).map(|r| r.line) else {
            return Outcome::none();
        };
        self.doc.toggle(line);
        self.layout();
        self.unsaved = true;
        self.idle_ms = 0;
        self.save();
        self.refresh();
        Outcome::all()
    }

    fn next_paper(&mut self, ctx: &mut Ctx) -> Outcome {
        self.colour = (self.colour + 1) % self.papers.len();
        ctx.settings.set_int(&self.section, "Colour", self.colour as i32 + 1);
        Outcome::all()
    }

    fn toggle_rule(&mut self, ctx: &mut Ctx) -> Outcome {
        self.ruled = !self.ruled;
        ctx.settings.set_int(&self.section, "Ruled", self.ruled as i32);
        Outcome::all()
    }

    // ---- drawing --------------------------------------------------------------

    /// The sheet: for each row its rule, its checkbox, its words and the line
    /// through them, then whatever of it is selected, and the caret last.
    /// The same for a note that is being typed on and one that is not, bar
    /// those last two.
    fn draw_sheet(&self, ox: i32, oy: i32, canvas: &mut Canvas, text: &mut Text) {
        let outer = canvas.clip;
        let sheet = Rect::new(ox + self.paper.x0, oy + self.paper.y0, ox + self.paper.x1, oy + self.paper.y1);
        let clip = outer.intersect(&sheet);
        if clip.is_empty() {
            return;
        }
        canvas.clip = clip;

        let left = ox + self.text_x;
        let (sel_a, sel_b) = if self.editing { self.doc.range() } else { (0, 0) };
        // No file yet: the paper says so, until it is clicked.
        let missing = self.source.is_none() && !self.editing && self.doc.text().is_empty();
        let mut line = String::new();
        let mut scratch = String::new();

        for slot in 0..self.rows_shown {
            let down = slot * self.pitch;
            let ry = oy + self.first_y + down;
            if !Rect::new(sheet.x0, ry, sheet.x1, ry + self.pitch).intersects(&clip) {
                continue;
            }
            if let Some(m) = self.rule_meter.filter(|_| self.ruled) {
                skin::draw_meter(m, m.x, m.y + down, self, canvas, text, &mut scratch);
            }
            if missing {
                line.clear();
                match slot {
                    0 => {
                        line.push_str(&self.filename);
                        line.push_str(" is missing.");
                    }
                    1 => line.push_str("Click here to start one."),
                    _ => continue,
                }
                self.draw_row(canvas, text, &line, left, ry, self.faded);
                continue;
            }
            let Some(row) = self.rows.get(self.top + slot as usize) else { continue };

            if row.head && row.todo {
                if let Some(m) = self.box_meter {
                    skin::draw_meter(m, m.x, m.y + down, self, canvas, text, &mut scratch);
                }
                if let Some(m) = self.tick_meter.filter(|_| row.done) {
                    skin::draw_meter(m, m.x, m.y + down, self, canvas, text, &mut scratch);
                }
            }

            let chars = &self.doc.text()[row.start..row.end];
            line.clear();
            show(chars, &mut line);
            let x0 = left + px(row.xs[0]);
            // the line through a done item is as wide as its words
            let inked = chars.len() - chars.iter().rev().take_while(|&&c| editor::blank(c)).count();
            let strike = if row.done { left + px(row.xs[inked]) - x0 } else { 0 };
            let colour = if row.done { self.faded } else { self.ink };
            self.draw_row(canvas, text, &line, x0, ry, colour);
            canvas.fill(x0, ry + STRIKE_DY, strike, 1, colour);

            // What is selected of it, drawn again light on dark.  A selected
            // line break shows as a few px past the end of its row.
            let broken = row.last && sel_a <= row.end && sel_b > row.end;
            let (lo, hi) = (sel_a.clamp(row.start, row.end), sel_b.clamp(row.start, row.end));
            if hi > lo || broken {
                let a = left + px(row.xs[lo - row.start]);
                let b = left + px(row.xs[hi - row.start]) + if broken { BREAK_W } else { 0 };
                let band = Rect::new(a, ry, b, ry + self.pitch).intersect(&clip);
                canvas.fill(band.x0, band.y0, band.x1 - band.x0, band.y1 - band.y0, self.select_bg);
                canvas.clip = band;
                self.draw_row(canvas, text, &line, x0, ry, self.select_ink);
                canvas.fill(x0, ry + STRIKE_DY, strike, 1, self.select_ink);
                canvas.clip = clip;
            }
        }

        if self.editing && self.caret_on {
            if let Some(c) = self.caret_box() {
                // at the head of a selection it stands on the dark
                let colour = if sel_a != sel_b && self.doc.caret() == sel_a { self.select_ink } else { self.ink };
                canvas.fill(ox + c.x0, oy + c.y0, 1, self.pitch, colour);
            }
        }
        canvas.clip = outer;
    }

    /// One row's words, from (x, y), in one colour.
    fn draw_row(&self, canvas: &mut Canvas, text: &mut Text, string: &str, x: i32, y: i32, colour: u32) {
        if string.is_empty() {
            return;
        }
        text.draw(canvas, &Spec {
            text: string,
            font: self.font,
            colour,
            x,
            y: y + self.text_dy,
            align: 0,
            // wide enough never to matter: the canvas's clip is what ends a row
            w: 4000,
            h: self.pitch,
            clip: false,
            solid: None,
            pad: [0; 4],
        });
    }
}

impl Hooks for Notes {
    fn text(&self, var: &str, out: &mut String) {
        out.push_str(match var {
            "Words" => &self.status,
            "Created" => &self.created,
            _ => "",
        });
    }

    fn colour(&self, var: &str) -> u32 {
        match var {
            "NoteBg" => self.papers[self.colour],
            "UpInk" => if self.can_up { self.ink } else { self.faded },
            "DownInk" => if self.can_down { self.ink } else { self.faded },
            // the lines on the rule button: dark while the paper has them
            "RuleInk" => if self.ruled { self.ink } else { self.faded },
            _ => 0,
        }
    }

    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, text: &mut Text) {
        match meter.name {
            // The moving part of the scrollbar: an outset drawn round the two
            // numbers refresh() worked out, as [MeterThumb] spells it.
            "MeterThumb" => {
                let [face, hilite, dk_shadow, light, shadow] = self.palette;
                let (ty, th) = (y + self.thumb_y, self.thumb_h);
                canvas.fill(x, ty, 11, th, face);
                canvas.fill(x, ty, 11, 1, hilite);
                canvas.fill(x, ty, 1, th, hilite);
                canvas.fill(x, ty + th - 1, 11, 1, dk_shadow);
                canvas.fill(x + 10, ty, 1, th, dk_shadow);
                canvas.fill(x + 1, ty + 1, 9, 1, light);
                canvas.fill(x + 1, ty + 1, 1, th - 2, light);
                canvas.fill(x + 1, ty + th - 2, 9, 1, shadow);
                canvas.fill(x + 9, ty + 1, 1, th - 2, shadow);
            }
            "MeterSheet" => self.draw_sheet(x, y, canvas, text),
            _ => {}
        }
    }
}

impl Widget for Notes {
    fn skin(&self) -> &'static Skin {
        SKIN
    }

    fn about(&self) -> &'static str {
        "Win98 Sticky Note\n\n\
         Click the text\ttype there; drag to select\n\
         Click elsewhere\tsave and finish (so does Escape)\n\
         Click a checkbox\ttick or untick it\n\
         [ ] or [x]\t\ttyped at the start of a line makes a checkbox\n\
         Enter\t\ton a checkbox line starts another\n\
         Backspace\tat the start of one takes the checkbox away\n\
         Ctrl+Z, Ctrl+Y\tundo, redo\n\
         Swatch button\tthe next paper colour, round all of them\n\
         Lines button\truled or plain paper\n\
         Double-click icon\topen the note in a text editor\n\
         Right-click\t\tNew note: another, with a text of its own\n\n\
         Each note is a plain text file, saved as you type; the copy from \
         before the latest edit is kept beside it as .bak."
    }

    /// The file is looked at every other second, and what is drawn changes
    /// only if what it holds has.  A note with typing it could not write
    /// tries again instead: until that works, the typing wins.
    fn tick(&mut self, _ctx: &mut Ctx) -> Dirty {
        self.ticks += 1;
        if let Some((text, left)) = self.notice {
            self.notice = (left > 1).then_some((text, left - 1));
        }
        if self.ticks % 2 == 0 {
            if !self.unsaved {
                self.reload();
            } else if !self.editing {
                self.save();
            }
        }
        if self.refresh() { Dirty::All } else { Dirty::None }
    }

    fn command(&mut self, cmd: &str, arg: i32, x: i32, y: i32, ctx: &mut Ctx) -> Outcome {
        match cmd {
            "Scroll" => self.scroll(arg),
            "Page" => self.page(y),
            "Click" => self.click(x, y),
            "NextPaper" => self.next_paper(ctx),
            "ToggleRule" => self.toggle_rule(ctx),
            "OpenFile" => Outcome::request(Request::Open(self.path.clone())),
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    /// A press on the text puts the caret there and asks for the keyboard;
    /// held, it selects.  Anywhere else is the host's: the margin and the
    /// scrollbar answer when the button comes up, and the rest drags the
    /// window.
    fn press(&mut self, x: i32, y: i32, double: bool, shift: bool) -> Pressed {
        let (fx, fy) = (x - self.pad, y - self.pad);
        if !self.on_text(fx, fy) {
            return Pressed::No;
        }
        let mut request = Request::None;
        if !self.editing {
            // the note may have been changed elsewhere since it was last read
            if !self.unsaved {
                self.reload();
            }
            if !self.loaded {
                self.say("cannot read");
                self.refresh();
                return Pressed::Taken(Outcome::all());
            }
            self.bak_due = true;
            request = Request::TakeKeys;
        }
        let (pos, eol) = self.place_at(fx, fy);
        if double {
            self.doc.select_word(pos);
        } else {
            self.doc.click(pos, eol, shift && self.editing);
        }
        self.editing = true;
        Pressed::Taken(Outcome { request, ..self.edited() })
    }

    /// The selection follows the pointer, and the sheet follows it off the
    /// top or the foot.
    fn drag(&mut self, x: i32, y: i32) -> Dirty {
        if !self.editing {
            return Dirty::None;
        }
        let (fx, fy) = (x - self.pad, y - self.pad);
        if fy < self.paper.y0 {
            self.top = self.top.saturating_sub(1);
        } else if fy >= self.paper.y1 {
            self.top = (self.top + 1).min(self.last_top());
        }
        let (pos, eol) = self.place_at(fx, fy);
        self.doc.drag(pos, eol);
        self.caret_on = true;
        self.stale = true;
        self.refresh();
        Dirty::All
    }

    fn ibeam(&self, x: i32, y: i32) -> bool {
        self.on_text(x - self.pad, y - self.pad)
    }

    // ---- the keyboard -----------------------------------------------------------

    /// The wheel and the scrollbar move the sheet without ending the typing.
    fn typing_at(&self, x: i32, y: i32) -> bool {
        self.on_sheet(x, y)
    }

    /// One typed character.
    fn typed(&mut self, c: char) -> Outcome {
        if !self.editing {
            return Outcome::none();
        }
        self.doc.type_char(c);
        self.edited()
    }

    fn key(&mut self, key: Key, shift: bool, ctrl: bool) -> Outcome {
        if !self.editing {
            return Outcome::none();
        }
        let page = (self.rows_shown - PAGE_KEEP).max(1) as isize;
        match key {
            Key::Left => self.doc.left(ctrl, shift),
            Key::Right => self.doc.right(ctrl, shift),
            Key::Up => self.doc.vertical(&self.rows, -1, shift),
            Key::Down => self.doc.vertical(&self.rows, 1, shift),
            Key::PageUp => self.doc.vertical(&self.rows, -page, shift),
            Key::PageDown => self.doc.vertical(&self.rows, page, shift),
            Key::Home => self.doc.home(&self.rows, ctrl, shift),
            Key::End => self.doc.end(&self.rows, ctrl, shift),
            Key::Backspace => self.doc.backspace(ctrl),
            Key::Delete => self.doc.delete(ctrl),
            Key::Enter => self.doc.enter(),
            Key::Tab => self.doc.type_char('\t'),
            Key::SelectAll => self.doc.select_all(),
            Key::Undo => {
                self.doc.undo();
            }
            Key::Redo => {
                self.doc.redo();
            }
            Key::Escape => return Outcome::request(Request::DropKeys),
            // the host's: it is the one that can reach the clipboard
            Key::Copy | Key::Cut | Key::Paste => return Outcome::none(),
        }
        self.edited()
    }

    /// What is selected, for the clipboard.
    fn copy(&self) -> Option<String> {
        self.doc.selection().filter(|_| self.editing)
    }

    /// What is selected, taken out of the note.
    fn cut(&mut self) -> (Option<String>, Outcome) {
        let Some(taken) = self.copy() else { return (None, Outcome::none()) };
        self.doc.backspace(false);
        (Some(taken), self.edited())
    }

    fn paste(&mut self, text: &str) -> Outcome {
        if !self.editing {
            return Outcome::none();
        }
        self.doc.insert(text);
        self.edited()
    }

    /// The keyboard's timer, `ms` after the last: the caret blinks, if it is
    /// one that does, and typing that has paused long enough is written.
    fn pulse(&mut self, ms: u32, blink: bool) -> Dirty {
        if !self.editing {
            return Dirty::None;
        }
        let mut dirty = Dirty::None;
        if self.unsaved && self.autosave_ms > 0 {
            self.idle_ms = self.idle_ms.saturating_add(ms);
            if self.idle_ms >= self.autosave_ms {
                self.idle_ms = 0;
                self.save();
                if self.refresh() {
                    dirty = Dirty::All;
                }
            }
        }
        let on = !blink || !self.caret_on;
        if on != self.caret_on {
            self.caret_on = on;
            if let Some(c) = self.caret_box() {
                dirty = dirty.and(Dirty::Part(Rect::new(c.x0 + self.pad, c.y0 + self.pad, c.x1 + self.pad, c.y1 + self.pad)));
            }
        }
        dirty
    }

    /// The keyboard has gone elsewhere: write the note, and take the caret
    /// and the selection off the sheet.
    fn blur(&mut self, _ctx: &mut Ctx) -> Dirty {
        if !self.editing {
            return Dirty::None;
        }
        self.editing = false;
        self.doc.deselect();
        if self.unsaved {
            self.save();
        }
        self.bak_due = false;
        self.refresh();
        Dirty::All
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { id: 1, label: "Open the note in a text editor", checked: false });
        items.push(MenuItem { id: 2, label: "Next paper colour", checked: false });
        items.push(MenuItem { id: 4, label: "Ruled paper", checked: self.ruled });
        items.push(MenuItem { id: 3, label: "About Sticky Note", checked: false });
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            1 => Outcome::request(Request::Open(self.path.clone())),
            2 => self.next_paper(ctx),
            3 => Outcome::request(Request::About),
            4 => self.toggle_rule(ctx),
            _ => Outcome::none(),
        }
    }

    /// Whatever has not reached the file goes there now.
    fn shutdown(&mut self, _ctx: &mut Ctx) {
        if self.unsaved {
            self.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn lf_file_round_trips() {
        let file = b"[ ] milk\n[x] eggs\n";
        let (text, format) = decode(file);
        assert_eq!(text, chars("[ ] milk\n[x] eggs"));
        assert_eq!(encode(&text, format), file);
    }

    #[test]
    fn crlf_and_bom_are_kept() {
        let file = b"\xEF\xBB\xBFone\r\ntwo\r\n";
        let (text, format) = decode(file);
        assert_eq!(text, chars("one\ntwo"));
        assert_eq!(encode(&text, format), file);
    }

    #[test]
    fn blank_lines_at_the_end_survive_and_one_line_break_is_added() {
        let (text, format) = decode(b"a\n\n\n");
        assert_eq!(text, chars("a\n\n"));
        assert_eq!(encode(&text, format), b"a\n\n\n");
        assert_eq!(encode(&chars("a\nb"), Format::default()), b"a\nb\n");
        assert_eq!(encode(&[], Format::default()), b"");
    }

    #[test]
    fn a_1252_note_is_read_as_1252_and_written_as_utf8() {
        let (text, format) = decode(b"caf\xE9 \x80\x96\n");
        assert_eq!(text, chars("caf\u{e9} \u{20ac}\u{2013}"));
        assert_eq!(encode(&text, format), "caf\u{e9} \u{20ac}\u{2013}\n".as_bytes());
    }

    /// An empty folder that is this test's alone.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("win98-note-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A note in a folder of its own, and the window point at the start of
    /// one of its rows.
    fn note(name: &str, file: Option<&str>) -> (Notes, PathBuf) {
        let dir = folder(name);
        let path = dir.join("Notes.txt");
        if let Some(text) = file {
            std::fs::write(&path, text).unwrap();
        }
        let mut settings = Settings::in_memory();
        let mut ctx = Ctx { now: 0, settings: &mut settings, locked: false, signed_in: None };
        (Notes::new(&mut ctx, FIRST, Some(path.clone())), path)
    }

    fn at(n: &Notes, row: i32, dx: i32) -> (i32, i32) {
        (n.pad + n.text_x + dx, n.pad + n.first_y + row * n.pitch + 4)
    }

    #[test]
    fn typing_on_the_sheet_ends_up_in_the_file() {
        let (mut n, path) = note("typing", Some("[ ] milk\r\nplain\r\n"));
        assert_eq!(n.rows.len(), 2);
        assert!(n.rows[0].todo && !n.rows[0].done);

        // a click past the end of the first row, then a second item under it
        let (x, y) = at(&n, 0, 120);
        assert!(matches!(n.press(x, y, false, false), Pressed::Taken(Outcome { request: Request::TakeKeys, .. })));
        n.key(Key::Enter, false, false);
        for c in "eggs".chars() {
            n.typed(c);
        }
        assert!(n.unsaved);
        let mut settings = Settings::in_memory();
        n.blur(&mut Ctx { now: 0, settings: &mut settings, locked: false, signed_in: None });
        assert!(!n.unsaved && !n.editing);
        assert_eq!(std::fs::read(&path).unwrap(), b"[ ] milk\r\n[ ] eggs\r\nplain\r\n");
        // the note as it was is beside it
        let mut bak = path.clone().into_os_string();
        bak.push(".bak");
        assert_eq!(std::fs::read(PathBuf::from(bak)).unwrap(), b"[ ] milk\r\nplain\r\n");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_click_in_the_margin_ticks_the_box_and_writes_it() {
        let (mut n, path) = note("tick", Some("[ ] milk\n"));
        // relative to the paper, as [MeterPaper] hands it over
        let outcome = n.click(4, n.first_y - n.paper.y0 + 4);
        assert_eq!(outcome.dirty, Dirty::All);
        assert!(n.rows[0].done);
        assert_eq!(std::fs::read(&path).unwrap(), b"[x] milk\n");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_file_is_started_by_typing() {
        let (mut n, path) = note("missing", None);
        assert_eq!(n.status, "no file");
        let (x, y) = at(&n, 3, 10);
        n.press(x, y, false, false);
        n.typed('a');
        assert!(!path.exists());
        // the pause that lets it save itself
        n.pulse(n.autosave_ms, false);
        assert_eq!(std::fs::read(&path).unwrap(), b"a\n");
        assert_eq!(n.status, "1 word");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rows_break_where_the_font_says() {
        let (n, path) = note("wrap", Some("Finish the quarterly report before Friday and send it to the whole team\n"));
        let room = (n.text_w - SLACK) as f32;
        assert!(n.rows.len() >= 3);
        for row in &n.rows {
            // nothing but trailing blanks may run past the column
            let chars = &n.doc.text()[row.start..row.end];
            let inked = chars.len() - chars.iter().rev().take_while(|&&c| editor::blank(c)).count();
            assert!(row.xs[inked] <= room, "{:?} is {} px", chars.iter().collect::<String>(), row.xs[inked]);
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_swatch_button_goes_round_every_paper_and_remembers_which() {
        let (mut n, path) = note("papers", Some("milk\n"));
        let mut settings = Settings::in_memory();
        let mut ctx = Ctx { now: 0, settings: &mut settings, locked: false, signed_in: None };
        // canary and white are where they always were; the rest follow
        assert!(n.papers.len() >= 7);
        assert_eq!((n.papers[0], n.papers[1]), (SKIN.colour("Paper1"), SKIN.colour("Paper2")));
        let mut seen = vec![n.colour("NoteBg")];
        for _ in 1..n.papers.len() {
            n.next_paper(&mut ctx);
            seen.push(n.colour("NoteBg"));
        }
        assert_eq!(seen, n.papers);
        assert_eq!(ctx.settings.int(FIRST, "Colour", 0), n.papers.len() as i32);
        // no two alike, and after the last the first again
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), n.papers.len());
        n.next_paper(&mut ctx);
        assert_eq!((n.colour("NoteBg"), ctx.settings.int(FIRST, "Colour", 0)), (n.papers[0], 1));

        // a number past the last paper, written by hand, is the last paper
        ctx.settings.set(FIRST, "Colour", "99");
        let last = Notes::new(&mut ctx, FIRST, Some(path.clone()));
        assert_eq!(last.colour("NoteBg"), *last.papers.last().unwrap());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn only_a_notes_section_is_a_note() {
        assert_eq!(number("Notes"), Some(1));
        assert_eq!(number("notes12"), Some(12));
        for other in ["Notes1", "Notes02", "Notes2x", "Notes 2", "Note", "Weather", ""] {
            assert_eq!(number(other), None, "{other:?}");
        }
        assert_eq!((name("Notes"), name("Notes3")), (String::from("Sticky Note"), String::from("Sticky Note 3")));
    }

    #[test]
    fn another_note_has_a_text_and_a_paper_of_its_own() {
        let dir = folder("two");
        std::fs::write(dir.join("Notes.txt"), "first\n").unwrap();
        let mut settings = Settings::in_memory_at(&dir);
        let second = create(&mut settings);
        assert_eq!(second, "Notes2");
        assert_eq!(std::fs::read(dir.join("Notes2.txt")).unwrap(), b"");
        assert_eq!(create(&mut settings), "Notes3");
        assert_eq!(others(&settings), ["Notes2", "Notes3"]);
        {
            let mut ctx = Ctx { now: 0, settings: &mut settings, locked: false, signed_in: None };
            let mut a = Notes::new(&mut ctx, FIRST, None);
            let mut b = Notes::new(&mut ctx, &second, None);
            // a new note is an empty sheet, not a file gone missing
            assert_eq!(b.status, "empty");

            let (x, y) = at(&b, 0, 10);
            b.press(x, y, false, false);
            "second".chars().for_each(|c| drop(b.typed(c)));
            b.blur(&mut ctx);
            b.next_paper(&mut ctx);
            // long enough for the first to have looked at its file again
            a.tick(&mut ctx);
            a.tick(&mut ctx);
            assert_eq!(a.doc.text(), chars("first"));
            assert_eq!(b.doc.text(), chars("second"));
        }
        assert_eq!(std::fs::read(dir.join("Notes.txt")).unwrap(), b"first\n");
        assert_eq!(std::fs::read(dir.join("Notes2.txt")).unwrap(), b"second\n");
        assert_eq!((settings.int("Notes", "Colour", 0), settings.int("Notes2", "Colour", 0)), (1, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_forgotten_note_hands_over_its_files_and_frees_its_number() {
        let dir = folder("forget");
        let mut settings = Settings::in_memory_at(&dir);
        let (second, third) = (create(&mut settings), create(&mut settings));
        let (text, bak) = (dir.join("Notes2.txt"), dir.join("Notes2.txt.bak"));
        std::fs::write(&text, "\u{feff}milk\n").unwrap();
        std::fs::write(&bak, "").unwrap();
        assert!(!is_blank(&file(&settings, &second)));
        assert!(is_blank(&file(&settings, &third)));
        assert!(is_blank(&dir.join("no-such-note.txt")));

        assert_eq!(forget(&mut settings, &second), [bak.clone(), text.clone()]);
        assert_eq!(others(&settings), ["Notes3"]);
        // while its file is still there, the next note is not given it
        assert_eq!(create(&mut settings), "Notes4");
        std::fs::remove_file(&text).unwrap();
        assert_eq!(create(&mut settings), "Notes2");

        // two notes told to share a file: forgetting one leaves the other its text
        settings.set("Notes4", "NoteFile", "Notes2.txt");
        assert_eq!(forget(&mut settings, "Notes4"), Vec::<PathBuf>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
