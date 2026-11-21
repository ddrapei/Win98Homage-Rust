//! Folders: folders of the user's own choosing, each a Windows icon with its
//! name under it in a white pane, like the large icons of an Explorer
//! window -- and, as on a Win98 desktop, a click selects one and a
//! double-click opens it in Explorer.
//!
//! It comes in three shapes, each a skin of its own from the one file: two
//! folders in a column, four in two columns, six in three.  The spots are
//! numbered down a column before the next, so a larger shape is the smaller
//! one with a column more on the right.  The folders of the spots a smaller
//! shape has no room for are kept, and come back with the larger one.
//!
//! A spot starts empty, a dotted square saying "Choose folder": a
//! double-click on it, or Choose folder... in its menu, asks for one.  A
//! folder's icon is picked in the right-click menu's Icon list from the ones
//! the skin names (`IconChoice1`, ...), each shown beside its name.  A
//! folder is called by its own name until another is typed for it: Rename
//! types it where the name stands, as a countdown's name is typed (`field`).
//!
//! It is the one widget whose title is the user's to give: "Folders" until
//! Rename title in its menu types another in the title bar, so that each can
//! say what the folders in it are.  There is a first and as many more as are
//! asked for, like the notes and the photo frames: `[Folders]`, then
//! `[Folders2]`, `[Folders3]`, ... in settings.ini.  Each keeps in its own
//! section its `Shape`, its `Title` (empty for "Folders"), and for each spot
//! n its folder `Pathn`, its icon `Iconn` and the name typed for it `Namen`
//! (empty for the folder's own).

use std::path::{Path, PathBuf};

use crate::canvas::{Canvas, Rect};
use crate::field::Field;
use crate::keys::Key;
use crate::settings::{self, Settings};
use crate::skin::{self, FOLDER_ICONS, Hooks, Meter, Skin};
use crate::text::{CENTER, Spec, Text};
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Pressed, Request, Widget};

/// The first one's section; the others are `Folders2`, `Folders3`, ...
pub const FIRST: &str = "Folders";
/// As many spots as the largest shape has.
const SPOTS: usize = 6;
/// What the folder dialog asks.
const CHOOSE_PROMPT: &str = "Choose the folder to keep here:";
/// What an empty spot says.
const EMPTY_LABEL: &str = "Choose folder";
/// The side of an icon.
const ICON: i32 = 32;

// the menu's entries
const MENU_OPEN: u32 = 1;
const MENU_CHOOSE: u32 = 2;
const MENU_RENAME: u32 = 3;
const MENU_REMOVE: u32 = 4;
const MENU_TITLE: u32 = 5;
/// From here, the shapes, in the order of `Shape::ALL`.
const MENU_SHAPE: u32 = 10;
/// From here, the icons, in the order of `FOLDER_ICONS`.
const MENU_ICON: u32 = 100;

/// How many folders it holds, and with that the skin it is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Two,
    Four,
    Six,
}

impl Shape {
    const ALL: [Shape; 3] = [Shape::Two, Shape::Four, Shape::Six];

    fn spots(self) -> usize {
        match self {
            Shape::Two => 2,
            Shape::Four => 4,
            Shape::Six => 6,
        }
    }

    /// What settings.ini calls it.
    fn key(self) -> &'static str {
        match self {
            Shape::Two => "2",
            Shape::Four => "4",
            Shape::Six => "6",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Shape::Two => "2 folders",
            Shape::Four => "4 folders",
            Shape::Six => "6 folders",
        }
    }

    pub fn skin(self) -> &'static Skin {
        match self {
            Shape::Two => &skin::folders_2::SKIN,
            Shape::Four => &skin::folders_4::SKIN,
            Shape::Six => &skin::folders_6::SKIN,
        }
    }

    /// The shape a section asks for; nothing, or nothing known, is the one
    /// the skin starts with.
    fn of(settings: &Settings, section: &str) -> Shape {
        let default = skin::folders_4::SKIN.var("Shape");
        let key = settings.text(section, "Shape", default);
        let find = |key: &str| Shape::ALL.into_iter().find(|s| s.key() == key.trim());
        find(&key).or_else(|| find(default)).unwrap_or(Shape::Four)
    }
}

/// The skin of the one with this section: the one for its shape.
pub fn skin_of(settings: &Settings, section: &str) -> &'static Skin {
    Shape::of(settings, section).skin()
}

// ---------------------------------------------------------------------------
//  More than one
// ---------------------------------------------------------------------------
//
// One is a section of settings.ini and nothing else: its folders are the
// user's, and stay where they are whatever becomes of it.

/// Which one a section is: 1 for `[Folders]`, n for `[Folders<n>]`, and None
/// for a section that is not one's.
fn number(section: &str) -> Option<u32> {
    settings::numbered(section, FIRST)
}

/// The title typed for the one with this section, if one was.
fn typed_title(settings: &Settings, section: &str) -> Option<String> {
    Some(settings.get(section, "Title")?.trim().to_owned()).filter(|t| !t.is_empty())
}

/// What one is called in the menus: the title typed for it, or else the
/// first is Folders and the others have their numbers.
pub fn name(settings: &Settings, section: &str) -> String {
    typed_title(settings, section).unwrap_or_else(|| match number(section) {
        Some(n) if n > 1 => format!("Folders {n}"),
        _ => String::from("Folders"),
    })
}

/// The sections of the ones after the first, in the order of their numbers.
pub fn others(settings: &Settings) -> Vec<String> {
    let mut found: Vec<(u32, &str)> = settings.sections().filter_map(|s| Some((number(s).filter(|&n| n > 1)?, s))).collect();
    found.sort();
    found.dedup_by_key(|f| f.0);
    found.into_iter().map(|(_, s)| s.to_owned()).collect()
}

/// Start another: a section of its own, under the lowest number none has, in
/// the shape one starts in and with every spot empty.  Returns the section.
pub fn create(settings: &mut Settings) -> String {
    let taken: Vec<u32> = settings.sections().filter_map(number).collect();
    let n = (2..).find(|n| !taken.contains(n)).unwrap_or(u32::MAX);
    let section = format!("{FIRST}{n}");
    settings.set(&section, "Shape", skin::folders_4::SKIN.var("Shape"));
    section
}

/// Take one out of settings.ini.  Its folders are the user's and stay where
/// they are, so there is nothing to hand back.
pub fn forget(settings: &mut Settings, section: &str) -> Vec<PathBuf> {
    settings.remove_section(section);
    Vec::new()
}

/// How many folders the one with this section holds, in its spots shown or
/// not: what deleting it would forget.
pub fn held(settings: &Settings, section: &str) -> usize {
    (1..=SPOTS).filter(|n| !settings.text(section, &format!("Path{n}"), "").trim().is_empty()).count()
}

/// What a folder is called where it is: its own name, or for the top of a
/// drive the drive itself, "C:\".
fn own_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// The place of an icon among `FOLDER_ICONS`, by its file's name; the first
/// for one that is not there.
fn icon_index(key: &str) -> usize {
    FOLDER_ICONS.iter().position(|i| i.0.eq_ignore_ascii_case(key.trim())).unwrap_or(0)
}

/// Where the folder dialog starts for a spot with nothing in it: the user's
/// own folder.
fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
}

/// How wide `s` is set on one line, in pixels.
fn width(text: &mut Text, font: usize, s: &str) -> f32 {
    let mut each = Vec::new();
    text.advances(font, s, &mut each);
    each.iter().sum()
}

/// How many of the characters of `s` fit in `room` px, with `after` px of
/// something else after them: at least one, whatever the room.
fn fitting(text: &mut Text, font: usize, s: &str, room: f32, after: f32) -> usize {
    let mut each = Vec::new();
    text.advances(font, s, &mut each);
    let mut at = 0.0;
    let mut n = 0;
    for (i, w) in each.iter().enumerate() {
        if i > 0 && at + w + after > room {
            break;
        }
        at += w;
        n = i + 1;
    }
    n
}

/// A name broken into at most two lines no wider than `room` px, as a Win98
/// desktop breaks an icon's name: between words where it can, inside a word
/// too long for a line of its own, and the second line cut short with "..."
/// when there is more than two lines' worth.
fn wrap(text: &mut Text, font: usize, s: &str, room: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if lines.len() > 2 {
            break; // there is more than will be shown, and that is all it takes
        }
        let joined = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if width(text, font, &joined) <= room {
            line = joined;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        let mut rest = word.to_owned();
        while width(text, font, &rest) > room && rest.chars().count() > 1 {
            let n = fitting(text, font, &rest, room, 0.0);
            lines.push(rest.chars().take(n).collect());
            rest = rest.chars().skip(n).collect();
        }
        line = rest;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.len() > 2 {
        let tail = lines[1..].join(" ");
        lines.truncate(1);
        lines.push(cut(text, font, &tail, room, true));
    }
    lines
}

/// As much of `s` as fits in `room` px followed by "...", the three stops
/// Win98 cut a name or a title short with -- or, if `always` is not set and
/// it fits whole, all of it as it is.
fn cut(text: &mut Text, font: usize, s: &str, room: f32, always: bool) -> String {
    if !always && width(text, font, s) <= room {
        return s.to_owned();
    }
    let dots = width(text, font, "...");
    let n = fitting(text, font, s, room, dots);
    let mut out: String = s.chars().take(n).collect::<String>().trim_end().to_owned();
    out.push_str("...");
    out
}

/// One place for a folder.
#[derive(Clone, Default)]
struct Spot {
    /// None for an empty spot.
    path: Option<PathBuf>,
    /// Its place among `FOLDER_ICONS`.
    icon: usize,
    /// The name typed for it; empty for the folder's own.
    name: String,
}

impl Spot {
    /// What is written under it.
    fn label(&self) -> String {
        match &self.path {
            _ if !self.name.is_empty() => self.name.clone(),
            Some(path) => own_name(path),
            None => String::from(EMPTY_LABEL),
        }
    }
}

/// The skin's colours.
#[derive(Clone, Copy, Default)]
struct Colours {
    ink: u32,
    empty: u32,
    select_bg: u32,
    select_ink: u32,
    field_bg: u32,
    title_ink: u32,
}

/// What is being typed: the name of one of the spots, or the title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Typing {
    Spot(usize),
    Title,
}

pub struct Folders {
    /// Its own section of settings.ini.
    section: String,
    /// The title typed for it; empty for the skin's own.
    title: String,
    shape: Shape,
    skin: &'static Skin,
    /// Every spot there can be, those this shape shows first.
    spots: Vec<Spot>,
    /// The spot selected: a click's, or the right button's.
    selected: Option<usize>,
    /// The spot the menu is about, if the right button came up on one.
    target: Option<usize>,
    /// The spot the folder dialog is choosing for.
    choosing: Option<usize>,
    /// What is being typed, in `field`: a spot's name, or the title.
    renaming: Option<Typing>,
    field: Field,
    colours: Colours,
    /// The font the names are in, and the bold one the title is in, as the
    /// host's `Text` numbers them.
    font: usize,
    bold: usize,
    /// Where in a cell the icon and the name go, how wide and tall a line of
    /// the name is, and how far its block reaches past it when it is
    /// selected (see the skin).
    icon_y: i32,
    label_y: i32,
    label_w: i32,
    line_h: i32,
    label_pad: i32,
}

impl Folders {
    /// The one with this section.
    pub fn new(ctx: &Ctx, section: &str) -> Folders {
        let shape = Shape::of(ctx.settings, section);
        let skin = shape.skin();
        let s = &*ctx.settings;
        let spots = (1..=SPOTS)
            .map(|n| {
                let path = s.text(section, &format!("Path{n}"), "");
                Spot {
                    path: (!path.trim().is_empty()).then(|| PathBuf::from(path.trim())),
                    icon: icon_index(&s.text(section, &format!("Icon{n}"), "")),
                    name: s.text(section, &format!("Name{n}"), "").trim().to_owned(),
                }
            })
            .collect();
        let points = skin.num("FontSize", 8.0) as f32;
        let mut w = Folders {
            section: section.to_owned(),
            title: typed_title(s, section).unwrap_or_default(),
            shape,
            skin,
            spots,
            selected: None,
            target: None,
            choosing: None,
            renaming: None,
            // its place is set to the name's when the typing starts
            field: Field::new(skin, "MeterCell1"),
            colours: Colours::default(),
            font: skin::FONTS.iter().position(|&(pt, bold)| pt == points && !bold).unwrap_or(0),
            bold: skin::FONTS.iter().position(|&(pt, bold)| pt == points && bold).unwrap_or(0),
            icon_y: skin.int("IconY", 16),
            label_y: skin.int("LabelY", 50),
            label_w: skin.int("LabelW", 74),
            line_h: skin.int("LineH", 13),
            label_pad: skin.int("LabelPad", 2),
        };
        w.read_colours();
        w
    }

    fn read_colours(&mut self) {
        let c = |name: &str| self.skin.colour(name);
        self.colours = Colours {
            ink: c("Ink"),
            empty: c("EmptyInk"),
            select_bg: c("SelectBg"),
            select_ink: c("SelectInk"),
            field_bg: c("FieldBg"),
            title_ink: c("TitleInk"),
        };
    }

    /// Spot `k`'s cell in the window.
    fn cell(&self, k: usize) -> Rect {
        self.skin.meter(&format!("MeterCell{}", k + 1)).map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3]))
    }

    /// The spot this shape shows at a point of the window.
    fn spot_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.shape.spots()).find(|&k| self.cell(k).contains(x, y))
    }

    /// The inside of the field a name is typed in, over where spot `k`'s
    /// name is: its text on the line the name's first is on.
    fn label_box(&self, k: usize) -> Rect {
        let c = self.cell(k);
        let w = self.label_w + 2 * self.skin.int("CityInset", 2);
        let drop = self.skin.int("CityDrop", 1);
        Rect::xywh(c.x0 + (c.x1 - c.x0 - w) / 2, c.y0 + self.label_y - drop, w, self.line_h + 2)
    }

    /// Select spot `k`, or none: what changed is the two spots' cells.
    fn select(&mut self, k: Option<usize>) -> Dirty {
        let was = std::mem::replace(&mut self.selected, k);
        if was == k {
            return Dirty::None;
        }
        [was, k].into_iter().flatten().fold(Dirty::None, |d, k| d.and(Dirty::Part(self.cell(k))))
    }

    /// Open spot `k`'s folder in Explorer.  An empty spot asks for a folder;
    /// so does one whose folder has gone -- deleted, moved, or on a drive
    /// that is not there -- starting from as near it as is still there.
    fn open(&mut self, k: usize) -> Outcome {
        match &self.spots[k].path {
            Some(path) if path.is_dir() => Outcome::request(Request::Open(path.clone())),
            Some(path) => {
                let near = path.ancestors().skip(1).find(|p| p.is_dir()).map(Path::to_path_buf);
                self.choose(k, near.unwrap_or_default())
            }
            None => self.choose(k, home()),
        }
    }

    fn choose(&mut self, k: usize, start: PathBuf) -> Outcome {
        self.choosing = Some(k);
        Outcome::request(Request::ChooseFolder(start, CHOOSE_PROMPT))
    }

    /// Start typing spot `k`'s name where it stands, all of it selected.
    fn rename(&mut self, k: usize) -> Outcome {
        if self.spots[k].path.is_none() {
            return Outcome::none();
        }
        self.renaming = Some(Typing::Spot(k));
        self.field.rect = self.label_box(k);
        self.field.flush_left = false;
        let name = self.spots[k].label();
        Outcome { dirty: Dirty::Part(self.cell(k)), ..self.field.begin(&name) }
    }

    /// What the title bar says: the title typed for it, or the skin's.
    fn shown_title(&self) -> &str {
        if self.title.is_empty() { self.skin.var("Title") } else { &self.title }
    }

    /// The field the title is typed in, with the line round it.
    fn title_box(&self) -> Rect {
        self.skin.meter("MeterTitleField").map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3]))
    }

    /// Start typing the title where it stands, all of it selected: from the
    /// left, where the title starts.
    fn rename_title(&mut self) -> Outcome {
        let b = self.title_box();
        self.renaming = Some(Typing::Title);
        self.field.rect = Rect::new(b.x0 + 1, b.y0 + 1, b.x1 - 1, b.y1 - 1);
        self.field.flush_left = true;
        let title = self.shown_title().to_owned();
        Outcome { dirty: Dirty::Part(b), ..self.field.begin(&title) }
    }

    /// The title in the title bar, bold and light, cut short with "..." if
    /// it is wider than the meter's room.
    fn draw_title(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, text: &mut Text) {
        let room = (meter.b[2] - meter.b[0]) as f32;
        let shown = cut(text, self.bold, self.shown_title(), room, false);
        text.draw(canvas, &Spec {
            text: &shown,
            font: self.bold,
            colour: self.colours.title_ink,
            x,
            y,
            align: 0,
            w: -1,
            h: -1,
            clip: false,
            solid: None,
            pad: [0; 4],
        });
    }

    /// Keep what spot `k` is under its own keys.
    fn save(&self, k: usize, settings: &mut Settings) {
        let spot = &self.spots[k];
        let n = k + 1;
        let section = &self.section;
        settings.set(section, &format!("Path{n}"), &spot.path.as_deref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
        settings.set(section, &format!("Icon{n}"), if spot.path.is_some() { FOLDER_ICONS[spot.icon].0 } else { "" });
        settings.set(section, &format!("Name{n}"), &spot.name);
    }

    /// The field, with its line of ink round it, on white.
    fn draw_field(&self, canvas: &mut Canvas, text: &mut Text) {
        let r = self.field.rect;
        canvas.fill(r.x0 - 1, r.y0 - 1, r.x1 - r.x0 + 2, r.y1 - r.y0 + 2, self.colours.ink);
        canvas.fill(r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0, self.colours.field_bg);
        self.field.draw("", canvas, text);
    }

    /// An icon whose every other pixel is the selection's colour, as Windows
    /// draws a selected icon.
    fn dithered(&self, x: i32, y: i32, bgra: &[u8], canvas: &mut Canvas) {
        for py in 0..ICON {
            for px in 0..ICON {
                let o = 4 * (py * ICON + px) as usize;
                if bgra[o + 3] == 0 {
                    continue;
                }
                let own = 0xFF00_0000 | (bgra[o + 2] as u32) << 16 | (bgra[o + 1] as u32) << 8 | bgra[o] as u32;
                canvas.fill(x + px, y + py, 1, 1, if (px + py) % 2 == 0 { self.colours.select_bg } else { own });
            }
        }
    }

    /// A dotted square, every other pixel of its edge, where an empty spot's
    /// icon would be.
    fn dotted(&self, x: i32, y: i32, canvas: &mut Canvas) {
        for i in 0..ICON {
            for (px, py) in [(i, 0), (i, ICON - 1), (0, i), (ICON - 1, i)] {
                if (px + py) % 2 == 0 {
                    canvas.fill(x + px, y + py, 1, 1, self.colours.empty);
                }
            }
        }
    }

    /// Spot `k`: its icon and its name, or the field its name is being
    /// typed in, or the empty spot.
    fn draw_cell(&self, k: usize, canvas: &mut Canvas, text: &mut Text) {
        let c = self.cell(k);
        let cx = c.x0 + (c.x1 - c.x0) / 2;
        let (ix, iy) = (cx - ICON / 2, c.y0 + self.icon_y);
        let spot = &self.spots[k];
        let selected = self.selected == Some(k);
        let ink = match &spot.path {
            Some(_) => {
                let icon = FOLDER_ICONS[spot.icon.min(FOLDER_ICONS.len() - 1)].2;
                if selected { self.dithered(ix, iy, icon, canvas) } else { canvas.image(ix, iy, ICON, ICON, icon) }
                self.colours.ink
            }
            None => {
                self.dotted(ix, iy, canvas);
                self.colours.empty
            }
        };

        if self.renaming == Some(Typing::Spot(k)) {
            self.draw_field(canvas, text);
            return;
        }
        let lines = wrap(text, self.font, &spot.label(), self.label_w as f32);
        let y = c.y0 + self.label_y;
        // each line centred as a String meter centres it: its own width,
        // rounded up, starting half of it left of the middle
        let spans: Vec<(i32, i32)> = lines
            .iter()
            .map(|l| {
                let w = width(text, self.font, l).ceil() as i32;
                (cx - w / 2, cx - w / 2 + w)
            })
            .collect();
        let colour = if selected {
            let (x0, x1) = spans.iter().fold((i32::MAX, i32::MIN), |(a, b), s| (a.min(s.0), b.max(s.1)));
            let pad = self.label_pad;
            // a row above the line and none below: the name's own line has
            // room under its ink, and a two-line block still ends in the cell
            canvas.fill(x0 - pad, y - 1, x1 - x0 + 2 * pad, lines.len() as i32 * self.line_h + 1, self.colours.select_bg);
            self.colours.select_ink
        } else {
            ink
        };
        for (i, line) in lines.iter().enumerate() {
            text.draw(canvas, &Spec {
                text: line,
                font: self.font,
                colour,
                x: cx,
                y: y + i as i32 * self.line_h,
                align: CENTER,
                w: -1,
                h: -1,
                clip: false,
                solid: None,
                pad: [0; 4],
            });
        }
    }
}

impl Hooks for Folders {
    /// TitleEdit: the title is being typed, so the field is shown instead.
    fn num(&self, var: &str) -> f64 {
        match var {
            "TitleEdit" => (self.renaming == Some(Typing::Title)) as i32 as f64,
            _ => 0.0,
        }
    }

    /// [MeterTitle]: the title.  [MeterTitleField]: the title being typed.
    /// [MeterCell1] ... [MeterCell6]: each spot, whole.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, text: &mut Text) {
        match meter.name {
            "MeterTitle" => self.draw_title(meter, x, y, canvas, text),
            "MeterTitleField" if self.renaming == Some(Typing::Title) => self.draw_field(canvas, text),
            name => {
                if let Some(n) = skin::index_of(name, "MeterCell").filter(|&n| (1..=self.shape.spots()).contains(&n)) {
                    self.draw_cell(n - 1, canvas, text);
                }
            }
        }
    }
}

impl Widget for Folders {
    fn skin(&self) -> &'static Skin {
        self.skin
    }

    fn about(&self) -> &'static str {
        "Win98 Folders\n\n\
         Folders of your own, to open in Explorer.\n\n\
         Click a folder to select it, double-click it to open it.  \
         Double-click an empty spot to choose a folder for it.\n\n\
         Right-click a folder to give it another icon, to rename it, to \
         choose another folder for it or to take it off.  Right-click \
         anywhere to give the widget a title of your own -- what the folders \
         in it are -- and for 2, 4 or 6 folders: a folder kept in a spot a \
         smaller shape has no room for comes back with the larger one.  \
         Folders + in the Widgets menu makes another, with folders and a \
         title of its own."
    }

    fn tick(&mut self, _ctx: &mut Ctx) -> Dirty {
        Dirty::None
    }

    fn command(&mut self, cmd: &str, arg: i32, _x: i32, _y: i32, _ctx: &mut Ctx) -> Outcome {
        let spot = usize::try_from(arg - 1).ok().filter(|&k| k < self.shape.spots());
        match (cmd, spot) {
            ("Select", k) => Outcome::dirty(self.select(k)),
            ("Open", Some(k)) => {
                let dirty = self.select(Some(k));
                Outcome { dirty, ..self.open(k) }
            }
            ("Rename", Some(k)) => {
                let dirty = self.select(Some(k));
                let outcome = self.rename(k);
                Outcome { dirty: dirty.and(outcome.dirty), ..outcome }
            }
            ("RenameTitle", _) => self.rename_title(),
            ("About", _) => Outcome::request(Request::About),
            ("Close", _) => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    /// A folder selected stays selected until a press somewhere else ends
    /// it -- on the pane off every folder (`Select(0)`), or anywhere outside
    /// the widget.
    fn watches_presses(&self) -> bool {
        self.selected.is_some()
    }

    fn pressed_elsewhere(&mut self) -> Dirty {
        self.select(None)
    }

    /// The right button selects the spot it came up on, as it does an icon
    /// on a Win98 desktop, and the menu is about that one; off every spot,
    /// none is selected and the menu is the widget's.
    fn context(&mut self, x: i32, y: i32) -> Dirty {
        self.target = self.spot_at(x, y);
        self.select(self.target)
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        if let Some(spot) = self.target.map(|k| &self.spots[k]) {
            if spot.path.is_some() {
                items.push(MenuItem::new(MENU_OPEN, "Open", false));
                items.push(MenuItem::new(MENU_CHOOSE, "Choose folder...", false));
                items.push(MenuItem::sub("Icon"));
                for (i, choice) in FOLDER_ICONS.iter().enumerate() {
                    items.push(MenuItem::new(MENU_ICON + i as u32, choice.1, i == spot.icon));
                }
                items.push(MenuItem::end_sub());
                items.push(MenuItem::new(MENU_RENAME, "Rename", false));
                items.push(MenuItem::new(MENU_REMOVE, "Remove", false));
            } else {
                items.push(MenuItem::new(MENU_CHOOSE, "Choose folder...", false));
            }
            items.push(MenuItem::line());
        }
        items.push(MenuItem::new(MENU_TITLE, "Rename title", false));
        items.push(MenuItem::line());
        for (i, shape) in Shape::ALL.into_iter().enumerate() {
            items.push(MenuItem::new(MENU_SHAPE + i as u32, shape.label(), shape == self.shape));
        }
    }

    /// Each icon in the Icon list is shown beside its name.
    fn menu_picture(&self, id: u32, px: i32) -> Option<(i32, &'static [u8])> {
        let icon = FOLDER_ICONS.get(id.checked_sub(MENU_ICON)? as usize)?;
        Some(if px >= ICON { (ICON, icon.2) } else { (16, icon.3) })
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        if let Some(&shape) = id.checked_sub(MENU_SHAPE).and_then(|i| Shape::ALL.get(i as usize)) {
            if shape == self.shape {
                return Outcome::none();
            }
            ctx.settings.set(&self.section, "Shape", shape.key());
            return Outcome::request(Request::Reshape);
        }
        if id == MENU_TITLE {
            return self.rename_title();
        }
        let Some(k) = self.target else { return Outcome::none() };
        match id {
            MENU_OPEN => self.open(k),
            MENU_CHOOSE => {
                let start = self.spots[k].path.clone().filter(|p| p.is_dir()).unwrap_or_else(home);
                self.choose(k, start)
            }
            MENU_RENAME => self.rename(k),
            MENU_REMOVE => {
                self.spots[k] = Spot::default();
                self.save(k, ctx.settings);
                Outcome::dirty(Dirty::Part(self.cell(k)))
            }
            _ => match id.checked_sub(MENU_ICON).map(|i| i as usize).filter(|&i| i < FOLDER_ICONS.len()) {
                Some(icon) if self.spots[k].path.is_some() => {
                    self.spots[k].icon = icon;
                    self.save(k, ctx.settings);
                    Outcome::dirty(Dirty::Part(self.cell(k)))
                }
                _ => Outcome::none(),
            },
        }
    }

    /// The folder for the spot that asked: one that had none starts with
    /// the first icon; one that had a folder keeps its icon and the name
    /// typed for it.
    fn folder_chosen(&mut self, folder: PathBuf, ctx: &mut Ctx) -> Dirty {
        let Some(k) = self.choosing.take() else { return Dirty::None };
        if self.spots[k].path.is_none() {
            self.spots[k] = Spot::default();
        }
        self.spots[k].path = Some(folder);
        self.save(k, ctx.settings);
        self.select(Some(k)).and(Dirty::Part(self.cell(k)))
    }

    // ---- typing a name ----------------------------------------------------------

    /// While a name is being typed, a press in its field puts the caret
    /// down, and held, it selects.  Anywhere else is the host's.
    fn press(&mut self, x: i32, y: i32, double: bool, shift: bool) -> Pressed {
        if self.renaming.is_none() || !self.field.reach().contains(x, y) {
            return Pressed::No;
        }
        Pressed::Taken(self.field.press(x, double, shift))
    }

    fn drag(&mut self, x: i32, _y: i32) -> Dirty {
        self.field.drag(x)
    }

    fn ibeam(&self, x: i32, y: i32) -> bool {
        self.renaming.is_some() && self.field.rect.contains(x, y)
    }

    fn typing_at(&self, x: i32, y: i32) -> bool {
        self.renaming.is_some() && self.field.reach().contains(x, y)
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

    /// The typing is over: what was typed is the name, or the title, if it
    /// is an answer -- Enter, or a click elsewhere after a change.  Nothing,
    /// or the folder's own name, is the folder's own name again; nothing, or
    /// the skin's title, is the skin's title again.
    fn blur(&mut self, ctx: &mut Ctx) -> Dirty {
        let Some(typing) = self.renaming.take() else { return Dirty::None };
        let typed = self.field.end();
        match typing {
            Typing::Spot(k) => {
                if let Some(typed) = typed {
                    let own = self.spots[k].path.as_deref().map(own_name).unwrap_or_default();
                    self.spots[k].name = if typed == own { String::new() } else { typed };
                    self.save(k, ctx.settings);
                }
                Dirty::Part(self.cell(k))
            }
            Typing::Title => {
                if let Some(typed) = typed {
                    self.title = if typed == self.skin.var("Title") { String::new() } else { typed };
                    ctx.settings.set(&self.section, "Title", &self.title);
                }
                Dirty::Part(self.title_box())
            }
        }
    }

    fn retint(&mut self) {
        self.read_colours();
        self.field.retint();
    }

    /// Closed while a name was being typed: it is kept as a click elsewhere
    /// would keep it.
    fn shutdown(&mut self, ctx: &mut Ctx) {
        self.blur(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, empty.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("win98-folders-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx(settings: &mut Settings) -> Ctx<'_> {
        Ctx { now: 1_000, settings, locked: false, signed_in: None }
    }

    /// A point in the middle of spot `k`'s cell.
    fn middle(w: &Folders, k: usize) -> (i32, i32) {
        let c = w.cell(k);
        ((c.x0 + c.x1) / 2, (c.y0 + c.y1) / 2)
    }

    #[test]
    fn each_shape_is_as_tall_as_its_row_and_holds_its_folders_down_each_column() {
        for (shape, frame, cols) in [(Shape::Two, (120, 190), 1), (Shape::Four, (190, 190), 2), (Shape::Six, (262, 190), 3)] {
            let skin = shape.skin();
            let pad = skin.int("Pad", 0);
            assert_eq!((skin.w - 2 * pad, skin.h - 2 * pad), frame, "{shape:?}");
            let mut settings = Settings::in_memory();
            settings.set(FIRST, "Shape", shape.key());
            let w = Folders::new(&ctx(&mut settings), FIRST);
            assert_eq!(w.shape, shape);
            // the pane's inside, under the title bar, inside the frame
            let pane = Rect::new(pad + 8, pad + 29, pad + frame.0 - 8, pad + 182);
            for k in 0..shape.spots() {
                let c = w.cell(k);
                assert_eq!(c.intersect(&pane), c, "{shape:?}: spot {k} is out of the pane");
                // down each column, then on to the next
                let (col, row) = (k as i32 / 2, k as i32 % 2);
                assert_eq!((c.x0, c.y0), (pane.x0 + col * (c.x1 - c.x0), pane.y0 + row * (c.y1 - c.y0)), "{shape:?}: spot {k}");
                assert_eq!(w.spot_at((c.x0 + c.x1) / 2, (c.y0 + c.y1) / 2), Some(k));
                // the name's field fits the cell
                let b = w.label_box(k);
                assert_eq!(Rect::new(b.x0 - 1, b.y0 - 1, b.x1 + 1, b.y1 + 1).intersect(&c), Rect::new(b.x0 - 1, b.y0 - 1, b.x1 + 1, b.y1 + 1));
            }
            // the columns fill the pane, near enough
            assert!(pane.x1 - w.cell(2 * cols - 1).x1 < cols as i32, "{shape:?}");
            // a name's block, selected and two lines long, is under its icon
            // and inside its cell, so it never touches the row below
            let cell_h = w.cell(0).y1 - w.cell(0).y0;
            assert!(w.label_y - 1 >= w.icon_y + ICON, "{shape:?}: the name's block is over the icon");
            assert!(w.label_y - 1 + 2 * w.line_h + 1 <= cell_h, "{shape:?}: a two-line name runs out of its cell");
            // the spots it has no room for are hidden, and take no clicks
            for k in shape.spots()..SPOTS {
                let m = skin.meter(&format!("MeterCell{}", k + 1)).unwrap();
                assert!(skin::formula(m.hidden, &w) != 0.0, "{shape:?}: spot {k} is shown");
            }
        }
    }

    #[test]
    fn every_icon_is_read_at_both_sizes_with_its_mask() {
        assert_eq!(FOLDER_ICONS[0].0, "folder", "a folder chosen gets the first, which is the plain one");
        assert!(FOLDER_ICONS.iter().any(|i| i.0 == "briefcase"));
        for &(file, label, big, small) in FOLDER_ICONS {
            assert!(!label.is_empty(), "{file}");
            for (side, px) in [(32, big), (16, small)] {
                assert_eq!(px.len(), side * side * 4, "{file} at {side}");
                let alpha: Vec<u8> = px.chunks_exact(4).map(|p| p[3]).collect();
                assert!(alpha.contains(&0) && alpha.contains(&255), "{file} at {side}: no mask");
                // the image in 256 colours, not the one in 16: the 16 are
                // the VGA's, and the pale yellows are none of them
                const VGA: [[u8; 3]; 16] = [
                    [0, 0, 0], [0, 0, 128], [0, 128, 0], [0, 128, 128], [128, 0, 0], [128, 0, 128], [128, 128, 0], [192, 192, 192],
                    [128, 128, 128], [0, 0, 255], [0, 255, 0], [0, 255, 255], [255, 0, 0], [255, 0, 255], [255, 255, 0], [255, 255, 255],
                ];
                assert!(px.chunks_exact(4).any(|p| p[3] == 255 && !VGA.contains(&[p[0], p[1], p[2]])), "{file} at {side}: only the 16 colours");
            }
        }
    }

    #[test]
    fn a_click_selects_a_double_click_opens_and_an_empty_spot_asks() {
        let dir = folder("open");
        let mut settings = Settings::in_memory();
        settings.set(FIRST, "Path1", &dir.to_string_lossy());
        settings.set(FIRST, "Path3", &dir.join("gone").to_string_lossy());
        let mut w = Folders::new(&ctx(&mut settings), FIRST);
        let mut ctx = ctx(&mut settings);

        assert_eq!(w.command("Select", 1, 0, 0, &mut ctx).dirty, Dirty::Part(w.cell(0)));
        assert_eq!(w.selected, Some(0));
        assert_eq!(w.command("Open", 1, 0, 0, &mut ctx).request, Request::Open(dir.clone()));
        // the pane off every folder selects none
        assert!(w.watches_presses());
        w.command("Select", 0, 0, 0, &mut ctx);
        assert_eq!(w.selected, None);
        assert!(!w.watches_presses());
        // and so does a press anywhere outside the widget
        w.command("Select", 1, 0, 0, &mut ctx);
        assert_eq!(w.pressed_elsewhere(), Dirty::Part(w.cell(0)));
        assert!(w.selected.is_none() && !w.watches_presses());

        // an empty spot asks for a folder, and keeps the one chosen
        let Request::ChooseFolder(_, prompt) = w.command("Open", 2, 0, 0, &mut ctx).request else { panic!("an empty spot did not ask") };
        assert_eq!(prompt, CHOOSE_PROMPT);
        let chosen = dir.join("chosen");
        std::fs::create_dir_all(&chosen).unwrap();
        w.folder_chosen(chosen.clone(), &mut ctx);
        assert_eq!(ctx.settings.get(FIRST, "Path2"), Some(chosen.to_str().unwrap()));
        assert_eq!(ctx.settings.get(FIRST, "Icon2"), Some("folder"));
        assert_eq!(w.spots[1].label(), "chosen");
        assert_eq!(w.command("Open", 2, 0, 0, &mut ctx).request, Request::Open(chosen));

        // a folder that has gone is looked for from as near it as is there
        assert_eq!(w.command("Open", 3, 0, 0, &mut ctx).request, Request::ChooseFolder(dir.clone(), CHOOSE_PROMPT));
        // and a spot this shape does not show is no spot at all
        settings.set(FIRST, "Shape", "2");
        let mut two = Folders::new(&Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None }, FIRST);
        assert_eq!(two.command("Open", 3, 0, 0, &mut Ctx { now: 1_000, settings: &mut settings, locked: false, signed_in: None }).request, Request::None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_right_button_picks_the_spot_and_its_menu() {
        let dir = folder("menu");
        let mut settings = Settings::in_memory();
        settings.set(FIRST, "Path1", &dir.to_string_lossy());
        settings.set(FIRST, "Icon1", "briefcase");
        let mut w = Folders::new(&ctx(&mut settings), FIRST);
        let mut ctx = ctx(&mut settings);
        let menu = |w: &Folders| {
            let mut items = Vec::new();
            w.menu(&mut items);
            items
        };

        // on a folder: open, choose, the icons, rename, remove, the shapes
        let (x, y) = middle(&w, 0);
        assert_eq!(w.context(x, y), Dirty::Part(w.cell(0)));
        assert_eq!(w.selected, Some(0));
        let items = menu(&w);
        let at = |label: &str| items.iter().position(|i| i.label == label).unwrap_or_else(|| panic!("no {label}"));
        assert!(at("Open") < at("Choose folder...") && at("Icon") < at("Rename") && at("Rename") < at("Remove"));
        assert_eq!(items[at("Icon")].id, MenuItem::SUB);
        let icons: Vec<&MenuItem> = items[at("Icon") + 1..].iter().take_while(|i| i.id != MenuItem::SUB).collect();
        assert_eq!(icons.len(), FOLDER_ICONS.len());
        assert!(icons.iter().all(|i| w.menu_picture(i.id, 16).is_some_and(|(s, p)| s == 16 && p.len() == 16 * 16 * 4)));
        assert!(icons.iter().all(|i| w.menu_picture(i.id, 32).is_some_and(|(s, _)| s == 32)));
        assert_eq!(icons.iter().filter(|i| i.checked).map(|i| i.label).collect::<Vec<_>>(), ["Briefcase"]);
        assert!(["2 folders", "4 folders", "6 folders"].iter().all(|s| items.iter().any(|i| i.label == *s)));
        assert!(items.iter().filter(|i| i.checked).any(|i| i.label == "4 folders"));

        // another icon, kept
        let fonts = MENU_ICON + icon_index("fonts") as u32;
        assert_eq!(w.menu_command(fonts, &mut ctx).dirty, Dirty::Part(w.cell(0)));
        assert_eq!(ctx.settings.get(FIRST, "Icon1"), Some("fonts"));

        // on an empty spot: a folder to choose, the title, and the shapes
        let (x, y) = middle(&w, 3);
        w.context(x, y);
        let labels: Vec<&str> = menu(&w).iter().map(|i| i.label).collect();
        assert_eq!(labels, ["Choose folder...", "", "Rename title", "", "2 folders", "4 folders", "6 folders"]);
        // off every spot: the title and the shapes alone, and nothing selected
        w.context(2, 2);
        let labels: Vec<&str> = menu(&w).iter().map(|i| i.label).collect();
        assert_eq!((w.selected, labels), (None, vec!["Rename title", "", "2 folders", "4 folders", "6 folders"]));

        // taken off: an empty spot again
        let (x, y) = middle(&w, 0);
        w.context(x, y);
        w.menu_command(MENU_REMOVE, &mut ctx);
        assert!(w.spots[0].path.is_none());
        assert_eq!((ctx.settings.get(FIRST, "Path1"), ctx.settings.get(FIRST, "Icon1")), (Some(""), Some("")));

        // another shape is another skin, made where this one was
        assert_eq!(w.menu_command(MENU_SHAPE, &mut ctx).request, Request::Reshape);
        assert_eq!(ctx.settings.get(FIRST, "Shape"), Some("2"));
        assert!(core::ptr::eq(skin_of(ctx.settings, FIRST), &skin::folders_2::SKIN));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn its_title_is_typed_in_the_title_bar_and_nothing_is_folders_again() {
        let mut settings = Settings::in_memory();
        let mut w = Folders::new(&ctx(&mut settings), FIRST);
        let mut ctx = ctx(&mut settings);
        let title = |w: &Folders| w.shown_title().to_owned();
        assert_eq!(title(&w), "Folders");
        assert_eq!(w.num("TitleEdit"), 0.0);

        // Rename title types over it where it stands, in the title bar
        w.context(2, 2);
        assert_eq!(w.menu_command(MENU_TITLE, &mut ctx).request, Request::TakeKeys);
        assert_eq!(w.num("TitleEdit"), 1.0);
        let bar = w.skin.meter("MeterTitleBar").unwrap().b;
        let r = w.field.rect;
        assert!(r.y0 > bar[1] && r.y1 < bar[1] + 24 && r.x1 < w.skin.meter("MeterBtnHelp").unwrap().b[0]);
        assert!(w.typing_at(r.x0 + 2, r.y0 + 2));
        "Work".chars().for_each(|c| drop(w.typed(c)));
        w.key(Key::Enter, false, false);
        w.blur(&mut ctx);
        assert_eq!((title(&w), ctx.settings.get(FIRST, "Title")), (String::from("Work"), Some("Work")));
        assert_eq!(w.num("TitleEdit"), 0.0);
        // and it is what the menus call it
        assert_eq!(name(ctx.settings, FIRST), "Work");

        // a title of nothing is "Folders" again
        w.menu_command(MENU_TITLE, &mut ctx);
        w.key(Key::Backspace, false, false);
        w.key(Key::Enter, false, false);
        w.blur(&mut ctx);
        assert_eq!((title(&w), ctx.settings.get(FIRST, "Title")), (String::from("Folders"), Some("")));
        assert_eq!(name(ctx.settings, FIRST), "Folders");
    }

    #[test]
    fn there_can_be_more_than_one_each_with_its_own_section() {
        let dir = folder("family");
        let mut settings = Settings::in_memory();
        settings.set(FIRST, "Path1", &dir.to_string_lossy());
        let second = create(&mut settings);
        assert_eq!(second, "Folders2");
        assert_eq!(name(&settings, &second), "Folders 2");
        let third = create(&mut settings);
        assert_eq!(others(&settings), [second.clone(), third.clone()]);

        // each keeps its own folders, shape and title
        settings.set(&second, "Shape", "6");
        settings.set(&second, "Title", "Games");
        let w = Folders::new(&ctx(&mut settings), &second);
        assert_eq!((w.shape, w.spots.iter().filter(|s| s.path.is_some()).count()), (Shape::Six, 0));
        assert_eq!(name(&settings, &second), "Games");
        assert_eq!((held(&settings, FIRST), held(&settings, &second)), (1, 0));

        // forgotten, its section goes, and nothing is left on disk to bin
        assert!(forget(&mut settings, &second).is_empty());
        assert_eq!(others(&settings), [third]);
        assert!(dir.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_is_typed_where_it_stands_and_nothing_is_its_own_again() {
        let dir = folder("rename");
        let mut settings = Settings::in_memory();
        settings.set(FIRST, "Path1", &dir.to_string_lossy());
        let mut w = Folders::new(&ctx(&mut settings), FIRST);
        let mut ctx = ctx(&mut settings);
        let own = own_name(&dir);
        let (x, y) = middle(&w, 0);
        w.context(x, y);

        // Rename types over the name, all of it selected; Enter keeps it
        assert_eq!(w.menu_command(MENU_RENAME, &mut ctx).request, Request::TakeKeys);
        assert!(w.typing_at(w.field.rect.x0 + 2, w.field.rect.y0 + 2));
        "Work".chars().for_each(|c| drop(w.typed(c)));
        assert_eq!(w.key(Key::Enter, false, false).request, Request::DropKeys);
        w.blur(&mut ctx);
        assert_eq!((w.spots[0].label(), ctx.settings.get(FIRST, "Name1")), (String::from("Work"), Some("Work")));

        // Escape leaves it as it was
        w.menu_command(MENU_RENAME, &mut ctx);
        "Play".chars().for_each(|c| drop(w.typed(c)));
        w.key(Key::Escape, false, false);
        w.blur(&mut ctx);
        assert_eq!(w.spots[0].label(), "Work");

        // nothing at all is the folder's own name again
        w.menu_command(MENU_RENAME, &mut ctx);
        w.key(Key::Backspace, false, false);
        w.key(Key::Enter, false, false);
        w.blur(&mut ctx);
        assert_eq!((w.spots[0].label(), ctx.settings.get(FIRST, "Name1")), (own, Some("")));
        // an empty spot has no name to type
        w.context(middle(&w, 1).0, middle(&w, 1).1);
        assert_eq!(w.menu_command(MENU_RENAME, &mut ctx).request, Request::None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_title_too_long_for_the_bar_is_cut_short_with_three_stops() {
        let mut text = Text::new(skin::folders_2::SKIN.var("Font"), skin::FONTS).unwrap();
        let bold = skin::FONTS.iter().position(|&f| f == (8.0, true)).unwrap();
        let room = skin::folders_2::SKIN.meter("MeterTitle").map(|m| (m.b[2] - m.b[0]) as f32).unwrap();
        assert_eq!(cut(&mut text, bold, "Work", room, false), "Work");
        let long = cut(&mut text, bold, "A very long title for two", room, false);
        assert!(long.ends_with("...") && long.len() < 20, "{long}");
        assert!(width(&mut text, bold, &long) <= room, "{long} is wider than the bar's room");
    }

    #[test]
    fn a_long_name_takes_two_lines_and_no_more() {
        let mut text = Text::new(skin::folders_4::SKIN.var("Font"), skin::FONTS).unwrap();
        let font = skin::FONTS.iter().position(|&f| f == (8.0, false)).unwrap();
        let room = 74.0;
        assert_eq!(wrap(&mut text, font, "Downloads", room), ["Downloads"]);
        let two = wrap(&mut text, font, "Holiday photos from Lviv", room);
        assert_eq!(two.len(), 2);
        assert_eq!(two.join(" "), "Holiday photos from Lviv");
        let cut = wrap(&mut text, font, "Projects that were started in the spring and never finished", room);
        assert_eq!(cut.len(), 2);
        assert!(cut[1].ends_with("..."), "{cut:?}");
        // one long word is broken where it reaches the edge
        let word = wrap(&mut text, font, "Supercalifragilisticexpialidocious", room);
        assert!(word.len() == 2 && word[0].len() > 4, "{word:?}");
        for line in two.iter().chain(&cut).chain(&word) {
            assert!(width(&mut text, font, line) <= room, "{line:?} is wider than the room");
        }
    }
}
