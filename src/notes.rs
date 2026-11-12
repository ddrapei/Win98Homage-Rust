//! The Sticky Note: a text file wrapped onto twelve rows of paper, with
//! checkboxes in the margin and a Win98 scrollbar.
//!
//! The file is the note.  A line that opens with `[ ]` or `[x]` is an item
//! and gets a checkbox; clicking the box flips the marker and writes the file
//! back.  A click on the text hands the whole sheet to the edit box (see
//! `editor`), and the file is re-read when that closes.
//!
//! Wrapping is done here, a row at a time, because a row is what a checkbox
//! hangs off, what a strike-through crosses and what the scrollbar counts.
//! The widths are the skin's own table of advance widths, kept exactly as it
//! was -- so a note breaks where it always has -- and the rows are still
//! clipped to the column, so a line the table measures short ends in an
//! ellipsis rather than running into the scrollbar.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::canvas::Canvas;
use crate::clock;
use crate::editor::Status;
use crate::settings::write_whole;
use crate::skin::{self, index_of, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Request, Widget};

static SKIN: &Skin = &skin::notes::SKIN;
const SECTION: &str = "Notes";

/// Spaces a tab stands in for, before measuring.
const TAB: usize = 4;
/// Px held back from the column on every line, to cover the width table
/// being a pixel out.
const SLACK: i32 = 6;
/// A line indented past this much of the column keeps its words and loses
/// its indent.
const MIN_ROOM: i32 = 24;
/// Px below a row's top where the line through a done item sits.
const STRIKE_DY: i32 = 6;
/// The thumb never gets shorter than the arrow buttons it runs between.
const THUMB_MIN: i32 = 11;
/// Rows a page-click leaves on screen.
const PAGE_KEEP: i32 = 1;
const SWATCHES: usize = 2;
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Microsoft Sans Serif at 8 pt, in pixels, by eye: the skin's table.
/// Anything not listed is the width of a digit.
const DEFAULT_ADVANCE: i32 = 6;
const ADVANCES: [(i32, &str); 9] = [
    (2, "il|'!.,:;"),
    (3, "fjrtI()[]{}/\\-`\""),
    (4, "*^ "),
    (5, "acegsvxyzJ"),
    (6, "bdhknopqu0123456789?+=<>~_FL$#"),
    (7, "ABEKPSVXYZ"),
    (8, "CDGHNOQRTUw&"),
    (9, "m%"),
    (10, "MW@"),
];

/// Windows-1252 0x80..0x9F, the one block where it is not Latin-1.
const CP1252: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D,
    0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
    0x0153, 0x009D, 0x017E, 0x0178,
];

/// White space, as C's isspace has it in the C locale.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C')
}

fn width(s: &str) -> i32 {
    s.chars().map(advance).sum()
}

fn advance(c: char) -> i32 {
    ADVANCES.iter().find(|(_, set)| set.contains(c)).map_or(DEFAULT_ADVANCE, |(px, _)| *px)
}

/// Where to cut `s` so that its head fits in `room` px: the end of the last
/// whole character that fits, and never less than the first character.
fn fit(s: &str, room: i32) -> usize {
    let (mut total, mut cut) = (0, 0);
    for (at, c) in s.char_indices() {
        total += advance(c);
        if cut > 0 && total > room {
            break;
        }
        cut = at + c.len_utf8();
    }
    cut
}

/// One logical line, greedily filled into as many rows as it takes.  Runs of
/// spaces inside it close up, but its indent is kept and repeated on every
/// row.  A word too wide for the column on its own is cut rather than allowed
/// to overhang.
fn wrap(text: &str, indent: &str, room: i32) -> Vec<String> {
    let mut out = Vec::new();
    let (indent, hang) = if room - width(indent) < MIN_ROOM { ("", 0) } else { (indent, width(indent)) };
    let room = room - hang;

    let mut line: Option<String> = None;
    for word in text.split(is_space).filter(|w| !w.is_empty()) {
        let mut current = match line.take() {
            None => word.to_owned(),
            Some(sofar) => {
                let joined = format!("{sofar} {word}");
                // A word no row will hold whole joins the line anyway, so the
                // cut below fills this row out rather than leave it half empty.
                if width(&joined) <= room || width(word) > room {
                    joined
                } else {
                    out.push(format!("{indent}{sofar}"));
                    word.to_owned()
                }
            }
        };
        while width(&current) > room {
            let cut = fit(&current, room);
            if cut >= current.len() {
                break; // one character wider than the column: it has the row to itself
            }
            out.push(format!("{indent}{}", &current[..cut]));
            current = current[cut..].to_owned();
        }
        line = (!current.is_empty()).then_some(current);
    }
    if let Some(rest) = line {
        out.push(format!("{indent}{rest}"));
    }
    if out.is_empty() {
        out.push(String::new()); // a blank line still owns a row
    }
    out
}

/// A logical line of the file.
struct Item {
    text: String,
    indent: String,
    todo: bool,
    done: bool,
    /// Which line of `raw` it came from; None for the two lines shown when
    /// there is no file.
    line: Option<usize>,
    /// The byte between the brackets.
    mark: usize,
}

struct Row {
    text: String,
    item: usize,
    /// The first row of its item: the only one that gets a checkbox.
    head: bool,
}

/// What one of the twelve row slots shows.
#[derive(Clone, Default, PartialEq)]
struct Slot {
    text: String,
    faded: bool,
    boxed: bool,
    ticked: bool,
    /// Width of the line struck through it, 0 for none.
    strike: i32,
}

/// Everything on screen that changes: compared between ticks, so a note
/// nobody is touching repaints nothing.
#[derive(Clone, Default, PartialEq)]
struct View {
    slots: Vec<Slot>,
    thumb_y: i32,
    thumb_h: i32,
    can_up: bool,
    can_down: bool,
    words: String,
    paper: usize,
}

pub struct Notes {
    rows_shown: i32,
    pitch: i32,
    first_y: i32,
    text_x: i32,
    text_w: i32,
    paper_x: i32,
    paper_y: i32,
    trough_y: i32,
    trough_h: i32,
    ink: u32,
    faded: u32,
    papers: [u32; SWATCHES],
    palette: [u32; 5],

    path: PathBuf,
    filename: String,
    /// The file as last seen: its size and time, to skip re-reading it.
    stamp: Option<(u64, SystemTime)>,
    loaded: bool,
    /// The bytes last parsed; None is "no such file".
    source: Option<Vec<u8>>,
    crlf: bool,
    legacy: bool,
    bom: bool,
    raw: Vec<Vec<u8>>,
    items: Vec<Item>,
    rows: Vec<Row>,
    /// Index in `rows` of the topmost visible row.
    top: usize,

    colour: usize,
    created: String,
    editing: bool,
    /// The edit box is holding typing it could not write.
    unsaved: bool,
    notice: Option<(&'static str, u32)>,
    ticks: u32,
    view: View,
}

impl Notes {
    pub fn new(ctx: &mut Ctx, file: Option<PathBuf>) -> Notes {
        let n = |name: &str, default: i32| SKIN.int(name, default);
        let filename = ctx.settings.text(SECTION, "NoteFile", SKIN.var("NoteFile"));
        let path = file.unwrap_or_else(|| {
            let p = PathBuf::from(&filename);
            if p.is_absolute() { p } else { ctx.settings.dir().join(p) }
        });
        let colour = (ctx.settings.int(SECTION, "Colour", 1).clamp(1, SWATCHES as i32) - 1) as usize;

        // The first run stamps the note with the day it was started and never
        // touches it again; any date already there is shown as it stands.
        let mut created = ctx.settings.text(SECTION, "Created", "");
        if created.trim().is_empty() {
            let t = clock::local(ctx.now);
            created = format!("{} {} {}", t.day, &clock::MONTHS[t.month as usize - 1][..3], t.year);
            ctx.settings.set(SECTION, "Created", &created);
        }

        let mut notes = Notes {
            rows_shown: n("Rows", 12).max(1),
            pitch: n("RowPitch", 13),
            first_y: n("Row1Y", 30),
            text_x: n("TextX", 22),
            text_w: n("TextW", 146),
            paper_x: n("PaperX", 8),
            paper_y: n("PaperY", 29),
            trough_y: n("TroughY", 40),
            trough_h: n("TroughH", 136),
            ink: SKIN.colour("Ink"),
            faded: SKIN.colour("Shadow"),
            papers: [SKIN.colour("Paper1"), SKIN.colour("Paper2")],
            palette: ["Face", "Hilite", "DkShadow", "Light", "Shadow"].map(|c| SKIN.colour(c)),
            filename: path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(filename),
            path,
            stamp: None,
            loaded: false,
            source: None,
            crlf: false,
            legacy: false,
            bom: false,
            raw: Vec::new(),
            items: Vec::new(),
            rows: Vec::new(),
            top: 0,
            colour,
            created,
            editing: false,
            unsaved: false,
            notice: None,
            ticks: 0,
            view: View::default(),
        };
        notes.reload();
        notes.render();
        notes
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// The paper and ink the edit box is to be painted with.
    pub fn colours(&self) -> (u32, u32) {
        (self.papers[self.colour], self.ink)
    }

    fn say(&mut self, text: &'static str) {
        self.notice = Some((text, 4));
    }

    // ---- the note file --------------------------------------------------------

    fn decode(&self, bytes: &[u8]) -> String {
        if self.legacy {
            bytes
                .iter()
                .map(|&b| {
                    let cp = if (0x80..=0x9F).contains(&b) { CP1252[(b - 0x80) as usize] as u32 } else { b as u32 };
                    char::from_u32(cp).unwrap_or('\u{FFFD}')
                })
                .collect()
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }

    /// Bytes in, logical lines out.  A leading [ ] or [x] makes an item, and
    /// where the byte between the brackets sits is kept, so flipping it later
    /// is one substitution.
    fn parse(&mut self, text: Option<&[u8]>) {
        self.raw.clear();
        self.items.clear();
        self.bom = false;
        self.legacy = false;

        let Some(text) = text else {
            // No file: say so on the paper.  Two items with no line.
            for line in [format!("{} is missing.", self.filename), String::from("Click a line to start one.")] {
                self.items.push(Item { text: line, indent: String::new(), todo: false, done: false, line: None, mark: 0 });
            }
            return;
        };

        self.crlf = text.windows(2).any(|w| w == b"\r\n");
        self.legacy = core::str::from_utf8(text).is_err();
        self.bom = text.starts_with(BOM);
        let body = if self.bom { &text[BOM.len()..] } else { text };
        // Split, but not touched otherwise: an edit rewrites one of these and
        // joins them up again.  A file that ends in a newline ends here in an
        // empty line, which is what puts the newline back.
        let breaks = body.iter().filter(|&&b| b == b'\n').count();
        self.raw = body
            .split(|&b| b == b'\n')
            .enumerate()
            .map(|(i, l)| if i < breaks && l.ends_with(b"\r") { l[..l.len() - 1].to_vec() } else { l.to_vec() })
            .collect();

        let expand = |s: &str| s.replace('\t', &" ".repeat(TAB));
        for (i, raw) in self.raw.iter().enumerate() {
            let lead = raw.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
            let marker = raw.get(lead..lead + 3).filter(|m| m[0] == b'[' && matches!(m[1], b' ' | b'x' | b'X') && m[2] == b']');
            if let Some(m) = marker {
                self.items.push(Item {
                    text: expand(&self.decode(&raw[lead + 3..])).trim_matches(is_space).to_owned(),
                    indent: String::new(), // the box sits out in the margin
                    todo: true,
                    done: m[1] != b' ',
                    line: Some(i),
                    mark: lead + 1,
                });
            } else {
                let body = expand(&self.decode(raw));
                let rest = body.trim_start_matches(' ');
                let indent = body[..body.len() - rest.len()].to_owned();
                self.items.push(Item { text: rest.trim_end_matches(' ').to_owned(), indent, todo: false, done: false, line: Some(i), mark: 0 });
            }
        }
        // blank lines at the end of a file are not rows anyone wants to scroll to
        while self.items.last().is_some_and(|i| !i.todo && i.text.is_empty()) {
            self.items.pop();
        }
    }

    /// Every item, wrapped, in one flat list: the row meters show a window
    /// onto it.
    fn layout(&mut self) {
        self.rows.clear();
        for (i, item) in self.items.iter().enumerate() {
            for (j, piece) in wrap(&item.text, &item.indent, self.text_w - SLACK).into_iter().enumerate() {
                self.rows.push(Row { text: piece, item: i, head: j == 0 });
            }
        }
    }

    /// Read the file, and re-parse it only if it is not the one already parsed.
    fn reload(&mut self) -> bool {
        let stamp = std::fs::metadata(&self.path).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
        if self.loaded && stamp.is_some() && stamp == self.stamp {
            return false;
        }
        let text = std::fs::read(&self.path).ok();
        self.stamp = stamp;
        if self.loaded && text == self.source {
            return false;
        }
        self.loaded = true;
        self.parse(text.as_deref());
        self.source = text;
        self.layout();
        true
    }

    /// Write `raw` back out and re-derive everything from what was written:
    /// what is on screen is then what is in the file, with no second copy of
    /// the rules for keeping the two in step.
    fn rewrite(&mut self) -> bool {
        let eol: &[u8] = if self.crlf { b"\r\n" } else { b"\n" };
        let mut text = if self.bom { BOM.to_vec() } else { Vec::new() };
        text.extend_from_slice(&self.raw.join(eol));
        // One newline at the end and no more.
        if self.raw.last().is_some_and(|l| !l.is_empty()) {
            text.extend_from_slice(eol);
        }
        if write_whole(&self.path, &text).is_err() {
            return false;
        }
        self.stamp = None;
        self.parse(Some(&text));
        self.source = Some(text);
        self.layout();
        true
    }

    /// How many words the note runs to.  Counted off the items, which is what
    /// makes a checkbox marker not a word.
    fn words(&self) -> usize {
        self.items.iter().map(|i| i.text.split(is_space).filter(|w| !w.is_empty()).count()).sum()
    }

    // ---- rendering ------------------------------------------------------------

    /// The item whose checkbox is drawn against this row, if any.  render()
    /// and the click that flips a box both ask here, so there is one rule.
    fn box_on(&self, row: usize) -> Option<&Item> {
        let row = self.rows.get(row).filter(|r| r.head)?;
        self.items.get(row.item).filter(|i| i.todo)
    }

    /// The last value `top` may take.  The scrollable extent is one row longer
    /// than the note: that blank row is the line clicked to add to the end.
    fn last_top(&self) -> usize {
        (self.rows.len() + 1).saturating_sub(self.rows_shown as usize)
    }

    /// The window of rows, the scrollbar that says where it is, and the word
    /// count.  True if anything on screen changed.
    fn render(&mut self) -> bool {
        let shown = self.rows_shown as usize;
        let extent = self.rows.len() + 1;
        let last = self.last_top();
        self.top = self.top.min(last);

        let mut view = View { paper: self.colour, ..View::default() };
        for slot in 0..shown {
            let index = self.top + slot;
            let row = self.rows.get(index);
            let item = row.and_then(|r| self.items.get(r.item));
            // The margin goes blank for the duration of an edit: the box holds
            // the file's own lines, which do not line up with the wrapped rows
            // the checkboxes were placed against.
            let boxed = if self.editing { None } else { self.box_on(index) };
            let done = item.is_some_and(|i| i.done);
            let text = row.map_or("", |r| r.text.as_str());
            view.slots.push(Slot {
                text: text.to_owned(),
                faded: done,
                boxed: boxed.is_some(),
                ticked: boxed.is_some_and(|b| b.done),
                strike: if done && !text.is_empty() { width(text).min(self.text_w) } else { 0 },
            });
        }

        // The thumb is as much of the trough as is on screen, where it is.
        // With nothing to scroll it fills the trough and both arrows go grey.
        view.thumb_y = self.trough_y;
        view.thumb_h = self.trough_h;
        if extent > shown {
            view.thumb_h = ((self.trough_h as f64 * shown as f64 / extent as f64 + 0.5).floor() as i32).max(THUMB_MIN);
            view.thumb_y = self.trough_y
                + ((self.trough_h - view.thumb_h) as f64 * self.top as f64 / (extent - shown) as f64 + 0.5).floor() as i32;
        }
        view.can_up = self.top > 0;
        view.can_down = self.top < last;

        // The right panel counts the words; something that went wrong
        // interrupts it, long enough to be read.
        view.words = if self.source.is_none() {
            String::from("no file")
        } else if let Some((text, left)) = self.notice.filter(|n| n.1 > 0) {
            self.notice = Some((text, left - 1));
            text.to_owned()
        } else {
            match self.words() {
                0 => String::from("empty"),
                1 => String::from("1 word"),
                n => format!("{n} words"),
            }
        };

        let changed = view != self.view;
        self.view = view;
        changed
    }

    fn rendered(&mut self) -> Outcome {
        if self.render() { Outcome::all() } else { Outcome::none() }
    }

    // ---- commands -------------------------------------------------------------

    /// The arrows and the wheel: a row at a time.
    fn scroll(&mut self, by: i32) -> Outcome {
        if self.editing {
            return Outcome::none();
        }
        let top = (self.top as i64 + by as i64).clamp(0, self.last_top() as i64) as usize;
        if top == self.top {
            return Outcome::none();
        }
        self.top = top;
        self.rendered()
    }

    /// A click in the trough: page towards it, or sit still if it landed on
    /// the thumb.  `y` is relative to the trough, so it becomes a frame
    /// coordinate before it is compared with the thumb.
    fn page(&mut self, y: i32) -> Outcome {
        if self.editing {
            return Outcome::request(Request::LeaveEdit);
        }
        let y = y + self.trough_y;
        if y >= self.view.thumb_y && y < self.view.thumb_y + self.view.thumb_h {
            return Outcome::none();
        }
        let page = (self.rows_shown - PAGE_KEEP).max(1);
        self.scroll(if y < self.view.thumb_y { -page } else { page })
    }

    /// Flip the checkbox on one of the visible rows.  The file is where the
    /// state lives, so it is written first: if the write fails nothing moves
    /// and the panel says why.
    fn toggle(&mut self, slot: usize) {
        let Some((line, mark, done)) = self.box_on(self.top + slot).and_then(|i| Some((i.line?, i.mark, i.done))) else {
            return;
        };
        let before = self.raw[line].clone();
        self.raw[line][mark] = if done { b' ' } else { b'x' };
        if !self.rewrite() {
            self.raw[line] = before;
            self.say("read-only");
        }
    }

    /// Every click on the paper, as a point relative to the paper.  Left of
    /// the text column is the margin, where the checkboxes are; anywhere else
    /// opens the note for editing.
    fn click(&mut self, x: i32, y: i32) -> Outcome {
        if self.editing {
            return Outcome::request(Request::LeaveEdit);
        }
        let (fx, fy) = (x + self.paper_x, y + self.paper_y);
        // The paper stands a pixel proud of the rows at the top and the foot:
        // those two slivers belong to the row they touch.
        let slot = ((fy - self.first_y).div_euclid(self.pitch)).clamp(0, self.rows_shown - 1) as usize;

        if fx < self.text_x {
            if self.unsaved {
                self.say("not saved");
            } else {
                self.toggle(slot);
            }
            return self.rendered();
        }

        // Hand the whole sheet to the edit box, telling it which line of the
        // file is at the top of the view and how many wrapped rows into it.
        self.editing = true;
        self.render();
        let (mut line, mut down) = (1, 0);
        if let Some(row) = self.rows.get(self.top) {
            line = self.items[row.item].line.map_or(1, |l| l + 1);
            while down < self.top && self.rows[self.top - down - 1].item == row.item {
                down += 1;
            }
        }
        Outcome { dirty: Dirty::All, request: Request::Edit { line, down } }
    }

    /// The box has closed, and has already written the file -- or it never
    /// opened, and this is why.  Read the file back, put the checkboxes back
    /// in the margin, and say so if something went wrong.
    pub fn closed(&mut self, status: Status) -> Dirty {
        self.editing = false;
        match status {
            // The box keeps the typing it could not save and shows it again
            // next time; the checkboxes wait until a close says it is saved.
            Status::WriteError => {
                self.unsaved = true;
                self.say("not saved");
            }
            Status::Saved | Status::Unchanged => self.unsaved = false,
            Status::ReadError => self.say("cannot read"),
            Status::NoWindow => self.say("no editor"),
            Status::Open => {}
        }
        self.stamp = None;
        self.reload();
        self.render();
        Dirty::All
    }

    fn next_paper(&mut self, ctx: &mut Ctx) -> Outcome {
        if self.editing {
            return Outcome::request(Request::LeaveEdit);
        }
        self.colour = (self.colour + 1) % SWATCHES;
        ctx.settings.set_int(SECTION, "Colour", self.colour as i32 + 1);
        self.rendered()
    }
}

impl Hooks for Notes {
    fn num(&self, var: &str) -> f64 {
        // Hidden=#BoxN#: 1 takes the checkbox away
        match index_of(var, "Box").and_then(|n| self.view.slots.get(n.wrapping_sub(1))) {
            Some(slot) if slot.boxed => 0.0,
            _ => 1.0,
        }
    }

    fn text(&self, var: &str, out: &mut String) {
        if let Some(slot) = index_of(var, "Row").and_then(|n| self.view.slots.get(n.wrapping_sub(1))) {
            out.push_str(&slot.text);
            return;
        }
        out.push_str(match var {
            "Words" => &self.view.words,
            "Created" => &self.created,
            _ => "",
        });
    }

    fn colour(&self, var: &str) -> u32 {
        let slot = |prefix: &str| index_of(var, prefix).and_then(|n| self.view.slots.get(n.wrapping_sub(1)));
        if let Some(s) = slot("RowInk") {
            return if s.faded { self.faded } else { self.ink };
        }
        if let Some(s) = slot("Tick") {
            return if s.ticked { self.ink } else { 0 };
        }
        match var {
            "NoteBg" => self.papers[self.view.paper],
            "UpInk" => if self.view.can_up { self.ink } else { self.faded },
            "DownInk" => if self.view.can_down { self.ink } else { self.faded },
            _ => 0,
        }
    }

    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        match meter.name {
            // The moving part of the scrollbar: an outset drawn round the two
            // numbers render() worked out, as [MeterThumb] spells it.
            "MeterThumb" => {
                let [face, hilite, dk_shadow, light, shadow] = self.palette;
                let (ty, th) = (y + self.view.thumb_y, self.view.thumb_h);
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
            // One line per done row, as wide as the text it crosses.
            "MeterStrikes" => {
                for (i, slot) in self.view.slots.iter().enumerate().filter(|(_, s)| s.strike > 0) {
                    let row_y = self.first_y + i as i32 * self.pitch;
                    canvas.fill(x + self.text_x, y + row_y + STRIKE_DY, slot.strike, 1, self.faded);
                }
            }
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
         Click the paper\tedit the whole note in place\n\
         Click elsewhere\tsave and finish (so does Escape)\n\
         Click a checkbox\ttick or untick it\n\
         [ ] or [x]\t\tat the start of a line makes it an item\n\
         Swatch button\tswap canary and white paper\n\
         Double-click icon\topen the note in a text editor\n\n\
         The note is a plain text file; the copy from before the latest edit \
         is kept beside it as .bak."
    }

    /// Every other second, as the skin did: one look at the file, and a
    /// repaint only if what it holds has changed.
    fn tick(&mut self, _ctx: &mut Ctx) -> Dirty {
        self.ticks += 1;
        if self.ticks % 2 != 0 {
            return Dirty::None;
        }
        self.reload();
        if self.render() { Dirty::All } else { Dirty::None }
    }

    fn command(&mut self, cmd: &str, arg: i32, x: i32, y: i32, ctx: &mut Ctx) -> Outcome {
        match cmd {
            "Scroll" => self.scroll(arg),
            "Page" => self.page(y),
            "Click" => self.click(x, y),
            "NextPaper" => self.next_paper(ctx),
            "OpenFile" => Outcome::request(Request::Open(self.path.clone())),
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem { id: 1, label: "Open the note in a text editor", checked: false });
        items.push(MenuItem { id: 2, label: "Canary paper", checked: self.colour == 0 });
        items.push(MenuItem { id: 3, label: "About Sticky Note", checked: false });
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            1 => Outcome::request(Request::Open(self.path.clone())),
            2 => self.next_paper(ctx),
            3 => Outcome::request(Request::About),
            _ => Outcome::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_where_the_skin_did() {
        let rows = wrap("Finish the quarterly report before Friday and send it to the whole team", "", 140);
        assert_eq!(rows, ["Finish the quarterly report", "before Friday and send it to", "the whole team"]);
        assert_eq!(wrap("", "", 140), [""]);
    }

    #[test]
    fn a_long_word_is_cut_not_overhung() {
        let rows = wrap("Supercalifragilisticexpialidocious_and_more_text", "", 140);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| width(r) <= 140));
    }
}
