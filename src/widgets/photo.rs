//! Photo Frame: the photos in a folder, one after another, each shown as the
//! screen of a Siemens C60 would have shown it.  That phone's screen was 101
//! x 80 pixels and showed 4096 colours, so a photo is brought down to exactly
//! that many pixels -- 101 x 80 in a wide frame, 80 x 101 in a tall one,
//! 80 x 80 in a square one -- and to 16 levels of red, green and blue, with
//! an ordered dither for the levels in between, as the phones of the time
//! showed a photo.  Each of its pixels is then drawn as a 2 x 2 square of the
//! skin's (4 x 4 on a monitor at 2x), so the picture is as coarse as the
//! phone's was.
//!
//! A photo fills the frame: it is scaled until it covers the shape, and what
//! sticks out on one side or the other is cut off (see `decode`).  The photos
//! come round in a shuffled order, every one of them before any comes again;
//! the folder is looked at afresh for each round, so a photo put in it shows
//! up in the next.
//!
//! Under the picture, in a shallow tray, is a row of three buttons: back to
//! the photo before, stop (play while stopped, the same as Pause in the menu)
//! and on to the next.  The frame remembers the photos it has shown, so the
//! back button walks back through them and the next one walks forward again
//! before any new photo is drawn from the round.
//!
//! The buttons are drawn here, in the skin's colours, in the manner of a
//! classic media player's: a little apart, each with a dark line round it,
//! each a light bevelled face with its symbol cut into it, every symbol cut the
//! same way.  They move when pressed: held down, a button gives at once and
//! goes in a step a frame -- its highlight going, then its top and left
//! falling into shade and its symbol moving down and right -- and let go, it
//! comes back the same way.  However short the click, a button goes all the
//! way in before it comes out, and slid off while held it comes out without
//! doing anything: what it is for is done when it is let go of over itself.
//!
//! There is a first frame and as many more as are asked for, like the notes
//! and the countdowns: `[Photo]`, then `[Photo2]`, `[Photo3]`, ... in
//! settings.ini.  Each has its own folder and shape and keeps them in its own
//! section, never in another's.
//!
//! The Dither Frame is the same frame with another picture in it (`Look`): a
//! photo brought down to one bit a pixel, ink and paper, at the skin's own
//! pixels (see `dither`).  Its frames are `[Dither]`, `[Dither2]`, ... with a
//! folder of their own, and a dithering method as well; all the rest -- the
//! folder, the round, the buttons, the shapes -- is this, as it is.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::canvas::{Canvas, Rect};
use crate::decode;
use crate::dither::{self, Method};
use crate::settings::{self, Settings};
use crate::sys;
use crate::skin::{self, Hooks, Meter, Skin};
use crate::text::Text;
use crate::widget::{Ctx, Dirty, MenuItem, Outcome, Pressed, Request, Widget};

/// The first frame's section; the others are `Photo2`, `Photo3`, ...
pub const FIRST: &str = "Photo";

/// Which frame it is: the Photo Frame, a phone's screen of colour, or the
/// Dither Frame, a picture of one bit a pixel in ink on paper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Photo,
    Dither,
}

impl Look {
    /// What the child that reads a photo is told the look is (`read_main`).
    fn arg(self) -> &'static str {
        match self {
            Look::Photo => "photo",
            Look::Dither => "dither",
        }
    }

    /// The first frame's section; the others are numbered after it.
    pub fn first(self) -> &'static str {
        match self {
            Look::Photo => FIRST,
            Look::Dither => dither::FIRST,
        }
    }

    /// What the frames of this look are called in the menus.
    fn title(self) -> &'static str {
        match self {
            Look::Photo => "Photo Frame",
            Look::Dither => "Dither Frame",
        }
    }

    /// The skin a frame of this look starts in, whose [Variables] say what
    /// a frame starts with.
    fn defaults(self) -> &'static Skin {
        Shape::Wide.skin(self)
    }
}

/// Seconds a photo stays, as the menu offers them.
const INTERVALS: [(u32, &str); 5] =
    [(5, "Every 5 seconds"), (10, "Every 10 seconds"), (30, "Every 30 seconds"), (60, "Every minute"), (300, "Every 5 minutes")];
/// With no photo to show, the folder is looked at again this often, so that
/// one put in it is shown without waiting out a long interval.
const EMPTY_RETRY: u32 = 5;
/// How many photos back the back button can go.
const HISTORY: usize = 100;
/// How many photos read and brought down are kept in memory, 16 KB each in a
/// Photo Frame and 64 KB in a Dither Frame: the one read ahead and the last
/// few shown, so that going on or back to one of them is a swap and no more.
const KEPT: usize = 5;
/// The stack a thread reading a photo is given.  It only waits for a child
/// to read it, but without a child it reads the photo itself, and WIC takes
/// more than a little; only what is used of it is ever more than an address.
const READER_STACK: usize = 1 << 20;
/// Milliseconds a button takes to move a pixel, down or up.
const PUSH_MS: u32 = 35;

/// The buttons, left to right: back, stop or play, next.
const BACK: usize = 0;
const STOP: usize = 1;
const NEXT: usize = 2;

/// The symbols cut into the buttons, a row a string, every one 9 px tall:
/// the skip symbols a bar, a gap and a narrow triangle, 8 wide; play a
/// broad triangle and stop a square, each 9 wide.
const GLYPH_BACK: [&str; 9] =
    ["##.....#", "##....##", "##...###", "##..####", "##.#####", "##..####", "##...###", "##....##", "##.....#"];
const GLYPH_NEXT: [&str; 9] =
    ["#.....##", "##....##", "###...##", "####..##", "#####.##", "####..##", "###...##", "##....##", "#.....##"];
const GLYPH_PLAY: [&str; 9] =
    ["#........", "###......", "#####....", "#######..", "#########", "#######..", "#####....", "###......", "#........"];
const GLYPH_STOP: [&str; 9] = ["#########"; 9];

/// Where a button is in its travel, and where it is headed.
#[derive(Clone, Copy, Default, Debug)]
struct Push {
    /// How far in it is, in steps: 0 up, the skin's BtnTravel all the way in.
    depth: i32,
    /// The pointer is holding it down.
    held: bool,
    /// It has been pressed and has not been all the way in since:
    /// however short the click, it goes in before it comes out.
    owed: bool,
}

/// The buttons' shape, as the skin gives it: each one's size inside its
/// line, the floor between them, and how many steps a press takes.
struct Keys {
    w: i32,
    h: i32,
    gap: i32,
    travel: i32,
}

/// The buttons' colours, from the skin.
#[derive(Clone, Default)]
struct KeyColours {
    line: u32,
    hi: u32,
    face: u32,
    face_down: u32,
    edge: [u32; 3],
    dark: u32,
    fill: u32,
    lit: u32,
    off: u32,
}

// the menu's entries
const MENU_NEXT: u32 = 1;
const MENU_PREVIOUS: u32 = 2;
/// From here, the shapes, in the order of `Shape::ALL`.
const MENU_SHAPE: u32 = 10;
/// From here, the intervals, in the order of `INTERVALS`.
const MENU_INTERVAL: u32 = 20;
const MENU_PAUSE: u32 = 30;
const MENU_CHOOSE: u32 = 31;
const MENU_OPEN: u32 = 32;
/// From here, the Dither Frame's methods, in the order of `Method::ALL`.
const MENU_METHOD: u32 = 40;
/// What the folder dialog asks.
const CHOOSE_PROMPT: &str = "Choose the folder of photos this frame shows:";

/// The shape of a frame, and with it the skin it is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Tall,
    Wide,
    Square,
}

impl Shape {
    const ALL: [Shape; 3] = [Shape::Tall, Shape::Wide, Shape::Square];

    /// What settings.ini calls it.
    fn key(self) -> &'static str {
        match self {
            Shape::Tall => "tall",
            Shape::Wide => "wide",
            Shape::Square => "square",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Shape::Tall => "Upright frame",
            Shape::Wide => "Wide frame",
            Shape::Square => "Square frame",
        }
    }

    pub fn skin(self, look: Look) -> &'static Skin {
        match (look, self) {
            (Look::Photo, Shape::Tall) => &skin::photo_tall::SKIN,
            (Look::Photo, Shape::Wide) => &skin::photo_wide::SKIN,
            (Look::Photo, Shape::Square) => &skin::photo_square::SKIN,
            (Look::Dither, Shape::Tall) => &skin::dither_tall::SKIN,
            (Look::Dither, Shape::Wide) => &skin::dither_wide::SKIN,
            (Look::Dither, Shape::Square) => &skin::dither_square::SKIN,
        }
    }

    /// The shape a section asks for; a frame that says nothing, or nothing
    /// known, is wide.
    fn of(settings: &Settings, section: &str) -> Shape {
        let key = settings.text(section, "Shape", skin::photo_wide::SKIN.var("Shape"));
        Shape::ALL.into_iter().find(|s| s.key().eq_ignore_ascii_case(key.trim())).unwrap_or(Shape::Wide)
    }
}

/// The skin of the frame with this section: the one for its shape.
pub fn skin_of(settings: &Settings, section: &str) -> &'static Skin {
    look_skin(Look::Photo, settings, section)
}

/// The same for a frame of either look.
pub fn look_skin(look: Look, settings: &Settings, section: &str) -> &'static Skin {
    Shape::of(settings, section).skin(look)
}

/// The 4 x 4 Bayer matrix: where in each 4 x 4 square a level in between
/// tips over to the next one up first.
const BAYER: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Bring opaque pixels (0xFFRRGGBB, `w` a row) down to 4096 colours, 16
/// levels of each of red, green and blue, as the C60's screen had them.  A
/// colour between two levels becomes a pattern of the two, by the Bayer
/// matrix, in the proportion it lies between them; black and white stay as
/// they are.  Each level is written back as 17 times itself, so the 16 run
/// evenly from 0 to 255.
pub fn dither12(px: &mut [u32], w: usize) {
    for (i, p) in px.iter_mut().enumerate() {
        // between 1/32 and 31/32 of a level
        let nudge = (2 * BAYER[(i / w) % 4][(i % w) % 4] + 1) * 255;
        let colour = *p;
        let level = |shift: u32| (((colour >> shift) & 0xFF) * 15 * 32 + nudge) / (255 * 32);
        *p = 0xFF00_0000 | level(16).min(15) * 17 << 16 | level(8).min(15) * 17 << 8 | level(0).min(15) * 17;
    }
}

/// A pixel `dither12` has brought down, kept in the 12 bits its three levels
/// take: 0xRGB, each level 0 to 15.
fn pack12(p: u32) -> u16 {
    let level = |shift: u32| (p >> shift & 0xFF) / 17;
    (level(16) << 8 | level(8) << 4 | level(0)) as u16
}

/// And back, as it is drawn: 0xFFRRGGBB.
fn unpack12(v: u16) -> u32 {
    let v = v as u32;
    0xFF00_0000 | (v >> 8 & 15) * 17 << 16 | (v >> 4 & 15) * 17 << 8 | (v & 15) * 17
}

/// Which frame of a look a section is: 1 for `[Photo]`, n for `[Photo<n>]`,
/// and None for a section that is not a frame's.
fn number(look: Look, section: &str) -> Option<u32> {
    settings::numbered(section, look.first())
}

/// What a frame is called in the menus: the first is the Photo Frame, and the
/// others have their numbers.
pub fn name(section: &str) -> String {
    name_of(Look::Photo, section)
}

/// The same for a frame of either look.
pub fn name_of(look: Look, section: &str) -> String {
    match number(look, section) {
        Some(n) if n > 1 => format!("{} {n}", look.title()),
        _ => String::from(look.title()),
    }
}

/// The sections of the frames after the first, in the order of their numbers.
pub fn others(settings: &Settings) -> Vec<String> {
    others_of(Look::Photo, settings)
}

/// The same for the frames of either look.
pub fn others_of(look: Look, settings: &Settings) -> Vec<String> {
    let mut found: Vec<(u32, &str)> =
        settings.sections().filter_map(|s| Some((number(look, s).filter(|&n| n > 1)?, s))).collect();
    found.sort();
    found.dedup_by_key(|f| f.0);
    found.into_iter().map(|(_, s)| s.to_owned()).collect()
}

/// Start another frame: a section of its own, under the lowest number none
/// has, on the folder and in the shape a frame starts with.  Returns the
/// section.
pub fn create(settings: &mut Settings) -> String {
    create_in(Look::Photo, settings)
}

/// The same for a frame of either look.
pub fn create_in(look: Look, settings: &mut Settings) -> String {
    let taken: Vec<u32> = settings.sections().filter_map(|s| number(look, s)).collect();
    let n = (2..).find(|n| !taken.contains(n)).unwrap_or(u32::MAX);
    let section = format!("{}{n}", look.first());
    let defaults = look.defaults();
    settings.set(&section, "Folder", defaults.var("Folder"));
    settings.set(&section, "Shape", defaults.var("Shape"));
    section
}

/// Take a frame out of settings.ini.  Its photos are the user's and stay in
/// their folder, so there is nothing to hand back.
pub fn forget(settings: &mut Settings, section: &str) -> Vec<PathBuf> {
    settings.remove_section(section);
    Vec::new()
}

/// The folder a frame shows: as settings.ini names it, beside settings.ini
/// unless the name says where itself.  With it, whether that is the folder a
/// frame starts with, which the program may make.  Settings kept nowhere (the
/// test harness's) have nothing beside them: a name that does not say where
/// is no folder at all.
fn folder_of(look: Look, settings: &Settings, section: &str) -> (PathBuf, bool) {
    let default = look.defaults().var("Folder");
    let name = settings.text(section, "Folder", default);
    let name = if name.trim().is_empty() { default } else { name.trim() };
    let path = PathBuf::from(name);
    let beside = settings.dir();
    let path = match () {
        _ if path.is_absolute() => path,
        _ if beside.as_os_str().is_empty() => PathBuf::new(),
        _ => beside.join(path),
    };
    (path, name.eq_ignore_ascii_case(default))
}

/// The photos in a folder -- not in the folders inside it -- by name.  What
/// each entry is comes with the listing itself, so this asks the disk
/// nothing more for each file.
fn photos_in(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| decode::is_photo(Path::new(&e.file_name())) && e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect();
    found.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    found
}

/// A photo read and brought down, `w` x `h`, 16 bits a pixel; None if it
/// cannot be read.  For the Photo Frame, to the C60's pixels and colours,
/// each pixel's three levels (`pack12`); for the Dither Frame, to the light of
/// each pixel (`dither::tones`), which is brought down to one bit when it is
/// put up (`Photo::develop`).
fn read_photo(look: Look, path: &Path, w: usize, h: usize, bg: u32) -> Option<Vec<u16>> {
    let mut px = decode::load(path, w as u32, h as u32, bg)?;
    Some(match look {
        Look::Photo => {
            dither12(&mut px, w);
            px.into_iter().map(pack12).collect()
        }
        Look::Dither => dither::tones(&px),
    })
}

/// The most pixels a child is asked to read a photo down to: far more than
/// any frame's, and a bound on what a wrong argument could make it allocate.
const MOST_PIXELS: usize = 1 << 20;

/// `--read-photo <photo|dither> <w> <h> <bg> <path>`: the short-lived child
/// that reads one photo for a frame (`read_photo_apart`).  The decoding --
/// WIC and its codecs, a COM apartment, and the photo at its full size, tens
/// of megabytes for a large PNG or WebP -- happens in it, and is gone with
/// it, never in the program that stays running.  What the photo is brought
/// down to goes out on stdout, two bytes a pixel (`to_bytes`).  It ends 1 if
/// the photo would not read, 2 if it was asked wrongly.
pub fn read_main(args: &[String]) -> i32 {
    use std::io::Write;
    let look = match args.first().map(String::as_str) {
        Some(arg) if arg == Look::Photo.arg() => Look::Photo,
        Some(arg) if arg == Look::Dither.arg() => Look::Dither,
        _ => return 2,
    };
    let size = |i: usize| args.get(i).and_then(|v| v.parse::<usize>().ok()).filter(|&n| n > 0);
    let bg = args.get(3).and_then(|v| u32::from_str_radix(v, 16).ok());
    let (Some(w), Some(h), Some(bg), Some(path)) = (size(1), size(2), bg, args.get(4)) else { return 2 };
    if w.saturating_mul(h) > MOST_PIXELS {
        return 2;
    }
    let Some(px) = read_photo(look, Path::new(path), w, h, bg) else { return 1 };
    let mut out = std::io::stdout().lock();
    match out.write_all(&to_bytes(&px)).and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// `read_photo`, done by a child (`read_main`): this program again, asked for
/// the one photo, its answer read off its stdout.  The answer is taken only
/// from a child that ended well and said exactly as much as was asked for.
/// Without a child to be had -- or with a path that is not valid Unicode,
/// which a child's arguments must be -- the photo is read here, as it was
/// before there were children; and so it is in the tests, whose program is
/// the test harness.
fn read_photo_apart(look: Look, path: &Path, w: usize, h: usize, bg: u32) -> Option<Vec<u16>> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let here = || read_photo(look, path, w, h, bg);
    let (false, Ok(exe), Some(_)) = (cfg!(test), std::env::current_exe(), path.to_str()) else { return here() };
    let spawned = Command::new(exe)
        .arg("--read-photo")
        .arg(look.arg())
        .arg(w.to_string())
        .arg(h.to_string())
        .arg(format!("{bg:08X}"))
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let Ok(mut child) = spawned else { return here() };
    let mut bytes = Vec::with_capacity(w * h * 2);
    let heard = child.stdout.take().is_some_and(|mut out| out.read_to_end(&mut bytes).is_ok());
    let ended_well = child.wait().is_ok_and(|status| status.success());
    if heard && ended_well { from_bytes(&bytes, w * h) } else { None }
}

/// A photo read down, as a child hands it over: each pixel's 16 bits, low
/// byte first.
fn to_bytes(px: &[u16]) -> Vec<u8> {
    px.iter().flat_map(|p| p.to_le_bytes()).collect()
}

/// And as the frame takes it back: None unless it is exactly `pixels` long.
fn from_bytes(bytes: &[u8], pixels: usize) -> Option<Vec<u16>> {
    (bytes.len() == pixels * 2).then(|| bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect())
}

/// Which way the frame is going: on to the next photo, or back to the one
/// before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Going {
    Next,
    Back,
}

/// What a thread of the frame's has gone to read: a photo, or the list of
/// the photos in the folder.  The thread ends with the answer.
struct Reading {
    /// The photo's path; None for the folder.
    path: Option<PathBuf>,
    answer: Receiver<Answer>,
}

enum Answer {
    /// The photo brought down, or None if it would not read.
    Photo(Option<Vec<u16>>),
    Folder(Vec<PathBuf>),
}

/// Where the frame would go next.
enum Upcoming {
    Photo(PathBuf),
    /// Nowhere: nothing before, or no photo in the folder.
    Nowhere,
    /// Not known until the folder has been looked at.
    Wait,
}

pub struct Photo {
    look: Look,
    section: String,
    shape: Shape,
    skin: &'static Skin,
    /// The photo's own pixels across and down, and how many of the skin's
    /// each is drawn as, each way.
    w: usize,
    h: usize,
    block: i32,
    /// The photo shown, `w` x `h`, as it was read and brought down
    /// (`read_photo`); empty while there is none.
    source: Vec<u16>,
    /// The same, as it is drawn: 0xFFRRGGBB.  For the Photo Frame it is the
    /// source's colours; for the Dither Frame, the source brought down to one
    /// bit and painted, again whenever the method or the tint changes.
    picture: Vec<u32>,
    /// What a transparent part of a photo is laid on.
    bg: u32,
    /// The Dither Frame's: how it brings a photo down to one bit, and the
    /// ink and the paper it paints it in.
    method: Method,
    ink: u32,
    paper: u32,

    folder: PathBuf,
    /// Still to come this round, the next one last.
    round: Vec<PathBuf>,
    shown: Option<PathBuf>,
    /// The photos shown before this one, the last of them last -- what the
    /// back button goes back to -- and those it has gone back past, the
    /// nearest last, which come again before anything new.
    past: Vec<PathBuf>,
    ahead: Vec<PathBuf>,
    /// Seconds each photo stays, and how many are left of this one's.
    interval: u32,
    left: u32,
    paused: bool,
    /// xorshift32, never zero.
    rng: u32,
    /// For the test harness: the photos in the order of their names, not
    /// shuffled, so the same one is drawn every time.
    in_order: bool,

    /// Photos read and brought down already, the latest last -- the one read
    /// ahead, and the last few shown -- and None for a file that would not
    /// read.  At most `KEPT`.
    kept: Vec<(PathBuf, Option<Vec<u16>>)>,
    /// The photo, or the folder, being read, if one is; a folder's list once
    /// it has been read, for the next round; and where the frame is going
    /// once what it is waiting for has been read.  The photo up stays up
    /// while it waits.
    reading: Option<Reading>,
    listed: Option<Vec<PathBuf>>,
    going: Option<Going>,
    /// Whether the round under way has had a photo put up from it.  A round
    /// that ends without one -- the folder's files all broken -- is not begun
    /// again until the folder is next looked at.
    round_tried: bool,
    round_shown: bool,
    /// For --dump and the tests: photos read there and then, on the thread
    /// that asks, so that what is drawn is the photo asked for.
    at_once: bool,

    /// The three buttons: their shape and colours, where each is in its
    /// travel, the one a press took hold of, and the milliseconds towards
    /// their next pixel of movement.
    keys: Keys,
    colours: KeyColours,
    pushes: [Push; 3],
    pressing: Option<usize>,
    since: u32,
}

impl Photo {
    /// The frame of `look` with this section.  Its photos are read on threads
    /// of their own, and the first is put up when it has been read.
    pub fn new(ctx: &Ctx, look: Look, section: &str) -> Photo {
        Photo::build(ctx, look, section, false)
    }

    /// The same, reading each photo there and then, on the thread that asks:
    /// for --dump and the tests, which draw what is asked for at once.
    pub fn reading_at_once(ctx: &Ctx, look: Look, section: &str) -> Photo {
        Photo::build(ctx, look, section, true)
    }

    fn build(ctx: &Ctx, look: Look, section: &str, at_once: bool) -> Photo {
        let shape = Shape::of(ctx.settings, section);
        let skin = shape.skin(look);
        let block = skin.int("Block", 2).max(1);
        let (folder, is_default) = folder_of(look, ctx.settings, section);
        // The folder a frame starts on is made for it, beside settings.ini,
        // so that there is somewhere to put the photos.
        if is_default && !folder.as_os_str().is_empty() && !folder.exists() {
            let _ = std::fs::create_dir_all(&folder);
        }
        let interval = ctx.settings.int(section, "Interval", skin.int("Interval", 10)).clamp(1, 86_400) as u32;
        // a different order for each frame, even on the same folder
        let seed = section.bytes().fold(ctx.now as u32, |s, b| s.wrapping_mul(31).wrapping_add(b as u32));
        let mut photo = Photo {
            look,
            section: section.to_owned(),
            shape,
            skin,
            w: (skin.int("PicW", 202) / block) as usize,
            h: (skin.int("PicH", 160) / block) as usize,
            block,
            source: Vec::new(),
            picture: Vec::new(),
            // the colours, just below
            bg: 0,
            method: Method::of(ctx.settings, section, skin),
            ink: 0,
            paper: 0,
            folder,
            round: Vec::new(),
            shown: None,
            past: Vec::new(),
            ahead: Vec::new(),
            interval,
            left: interval,
            paused: ctx.settings.flag(section, "Paused", skin.int("Paused", 0) != 0),
            rng: seed | 1,
            in_order: false,
            kept: Vec::new(),
            reading: None,
            listed: None,
            going: None,
            round_tried: false,
            round_shown: false,
            at_once,
            keys: Keys {
                w: skin.int("BtnW", 22),
                h: skin.int("BtnH", 16),
                gap: skin.int("BtnGap", 3),
                travel: skin.int("BtnTravel", 2).clamp(1, 2),
            },
            colours: KeyColours::default(),
            pushes: [Push::default(); 3],
            pressing: None,
            since: 0,
        };
        photo.read_colours();
        photo.next();
        photo
    }

    /// The colours of the pane, the buttons and the Dither Frame's picture,
    /// from the skin: at the start, and again for every change of tint.  Of
    /// them only the Dither Frame's ink is tinted.  A transparent part of a
    /// photo is laid on the pane's dark in the Photo Frame, and on the paper
    /// in the Dither Frame.
    fn read_colours(&mut self) {
        let c = |name: &str| self.skin.colour(name);
        (self.ink, self.paper) = (c("DitherInk"), c("DitherPaper"));
        self.bg = if self.look == Look::Dither { self.paper } else { c("PhotoBg") };
        self.colours = KeyColours {
            line: c("BtnLine"),
            hi: c("BtnHi"),
            face: c("BtnFace"),
            face_down: c("BtnFaceDown"),
            edge: [c("BtnEdge1"), c("BtnEdge2"), c("BtnEdge3")],
            dark: c("GlyphDark"),
            fill: c("GlyphFill"),
            lit: c("GlyphLit"),
            off: c("GlyphOff"),
        };
    }

    /// The photo as it is drawn, from what was read of it: the Photo Frame's
    /// in its colours, the Dither Frame's brought down to one bit and painted.
    fn develop(&self, source: &[u16]) -> Vec<u32> {
        match self.look {
            Look::Photo => source.iter().map(|&p| unpack12(p)).collect(),
            Look::Dither => dither::render(source, self.w, self.method, self.ink, self.paper),
        }
    }

    /// For the test harness: the first photo by name, and the rest in the
    /// order of their names after it.
    pub fn set_fixed(&mut self) {
        self.in_order = true;
        self.forget_shown();
        self.next();
    }

    /// Start afresh: no round under way, nothing shown or read before, and
    /// nothing being read -- a thread still reading answers to no one.
    fn forget_shown(&mut self) {
        self.round.clear();
        self.shown = None;
        self.past.clear();
        self.ahead.clear();
        self.kept.clear();
        self.reading = None;
        self.listed = None;
        self.going = None;
        (self.round_tried, self.round_shown) = (false, false);
    }

    fn random(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// A new round: every photo in the folder, `all`, shuffled -- with the
    /// one shown last not first, so that no photo is shown twice running.
    fn new_round(&mut self, mut all: Vec<PathBuf>) {
        if self.in_order {
            all.reverse();
        } else {
            for i in (1..all.len()).rev() {
                let j = self.random() as usize % (i + 1);
                all.swap(i, j);
            }
            if all.len() > 1 && all.last() == self.shown.as_ref() {
                let last = all.len() - 1;
                all.swap(0, last);
            }
        }
        (self.round_tried, self.round_shown) = (!all.is_empty(), false);
        self.round = all;
    }

    /// Keep the photo that is going in the past, for the back button.
    fn remember(&mut self, photo: Option<PathBuf>) {
        if let Some(photo) = photo {
            if self.past.len() == HISTORY {
                self.past.remove(0);
            }
            self.past.push(photo);
        }
    }

    /// On to the next photo: one the back button went past, if there is one,
    /// or else the next of the round that can be read -- starting a new round
    /// when this one is done -- or none, if the folder has none to show.
    fn next(&mut self) -> Dirty {
        self.going = Some(Going::Next);
        self.advance()
    }

    /// Back to the photo shown before this one, if there is one.
    fn previous(&mut self) -> Dirty {
        self.going = Some(Going::Back);
        self.advance()
    }

    /// Carry on towards where the frame is going.  A photo already read is
    /// put up at once; one that would not read, or has gone from the disk
    /// since it was read, is passed over; one not read yet is read -- on a
    /// thread of its own, the photo there is staying up meanwhile (see
    /// `collect`), or there and then in --dump and the tests.  The folder is
    /// looked at the same way when a round begins.
    ///
    /// Each file is tried at most once a round and the folder looked at
    /// most once each time, so a folder of broken files costs one look and
    /// no more.
    fn advance(&mut self) -> Dirty {
        let mut looked = false;
        while let Some(going) = self.going {
            let path = match self.upcoming(going, &mut looked) {
                Upcoming::Photo(path) => path,
                Upcoming::Nowhere => return self.arrive_nowhere(going),
                Upcoming::Wait => return Dirty::None,
            };
            match self.kept.iter().find(|k| k.0 == path).map(|k| k.1.is_some()) {
                Some(true) if path.exists() => {
                    self.arrive(going, path);
                    return Dirty::Part(self.changed());
                }
                Some(_) => {
                    self.take_kept(&path);
                    self.pass_over(going);
                }
                None if self.at_once => {
                    let px = read_photo(self.look, &path, self.w, self.h, self.bg);
                    self.keep(path, px);
                }
                None => {
                    // one at a time: a photo read ahead finishes first
                    if self.reading.is_none() {
                        self.read(path);
                    }
                    return Dirty::None;
                }
            }
        }
        Dirty::None
    }

    /// The photo the frame would go to next, going this way: the last one
    /// shown before, going back; going on, the nearest one gone back past,
    /// or the next of the round, beginning a new round if this one is done
    /// -- unless the folder has been looked at already (`looked`), or the
    /// round now done had nothing put up from it.  A new round waits for the
    /// folder to have been looked at, on a thread of its own.
    fn upcoming(&mut self, going: Going, looked: &mut bool) -> Upcoming {
        let photo = |path: Option<&PathBuf>| path.map_or(Upcoming::Nowhere, |p| Upcoming::Photo(p.clone()));
        match going {
            Going::Back => photo(self.past.last()),
            Going::Next => {
                if !self.ahead.is_empty() {
                    return photo(self.ahead.last());
                }
                if self.round.is_empty() {
                    if *looked || (self.round_tried && !self.round_shown) {
                        // looked at afresh the next time
                        self.round_tried = false;
                        return Upcoming::Nowhere;
                    }
                    let all = match self.listed.take() {
                        Some(all) => all,
                        None if self.at_once => photos_in(&self.folder),
                        None => {
                            // one at a time: a photo being read finishes first
                            if self.reading.is_none() {
                                self.list();
                            }
                            return Upcoming::Wait;
                        }
                    };
                    self.new_round(all);
                    *looked = true;
                }
                photo(self.round.last())
            }
        }
    }

    /// Take the photo `upcoming` gave off the list it came from.
    fn pass_over(&mut self, going: Going) {
        match going {
            Going::Back => drop(self.past.pop()),
            Going::Next if !self.ahead.is_empty() => drop(self.ahead.pop()),
            Going::Next => drop(self.round.pop()),
        }
    }

    /// Put up `path`, read already: the photo that was up kept in memory, and
    /// in the past going on, or ahead going back.  Then the clock starts
    /// again, and the photo after it is read ahead.
    fn arrive(&mut self, going: Going, path: PathBuf) {
        if going == Going::Next && self.ahead.is_empty() {
            self.round_shown = true;
        }
        self.pass_over(going);
        let px = self.take_kept(&path).flatten().unwrap_or_default();
        let old = std::mem::replace(&mut self.source, px);
        self.picture = self.develop(&self.source);
        if let Some(was) = self.shown.replace(path) {
            if !old.is_empty() {
                self.keep(was.clone(), Some(old));
            }
            match going {
                Going::Next => self.remember(Some(was)),
                Going::Back => self.ahead.push(was),
            }
        }
        self.going = None;
        self.restart_clock();
        self.read_ahead();
    }

    /// There is nowhere to go.  Back, nothing changes.  On, the folder has no
    /// photo to show: the pane says so -- what was up can still be gone back
    /// to -- and it is looked at again soon.
    fn arrive_nowhere(&mut self, going: Going) -> Dirty {
        self.going = None;
        if going == Going::Back {
            return Dirty::None;
        }
        let had = !self.picture.is_empty();
        let was = self.shown.take();
        self.remember(was);
        self.source.clear();
        self.picture.clear();
        self.restart_clock();
        if had { Dirty::Part(self.changed()) } else { Dirty::None }
    }

    /// While a photo is up, read the one that comes after it, so that going
    /// on to it is quick.
    fn read_ahead(&mut self) {
        if self.at_once || self.reading.is_some() {
            return;
        }
        let mut looked = false;
        if let Upcoming::Photo(path) = self.upcoming(Going::Next, &mut looked) {
            if !self.kept.iter().any(|k| k.0 == path) {
                self.read(path);
            }
        }
    }

    /// Read a photo on a thread of its own, which ends with it -- the thread
    /// only waits while a child reads it (`read_photo_apart`).
    fn read(&mut self, path: PathBuf) {
        let (look, w, h, bg) = (self.look, self.w, self.h, self.bg);
        let file = path.clone();
        self.start(Some(path), move || Answer::Photo(read_photo_apart(look, &file, w, h, bg)));
    }

    /// Look at the folder for the photos in it, on a thread of its own.
    fn list(&mut self) {
        let folder = self.folder.clone();
        self.start(None, move || Answer::Folder(photos_in(&folder)));
    }

    /// Start a thread doing `work` for `path` (None for the folder), which
    /// ends with the answer; the frame looks for it as it ticks and frames.
    /// Without a thread to be had, the work is done here and now, and its
    /// answer looked for all the same.
    fn start<F: FnOnce() -> Answer + Send + 'static>(&mut self, path: Option<PathBuf>, work: F) {
        use std::sync::{Arc, Mutex};
        let (answer, waiting) = mpsc::channel();
        // shared, so that it can be taken back if the thread never starts
        let work = Arc::new(Mutex::new(Some(work)));
        let (theirs, sender) = (Arc::clone(&work), answer.clone());
        let started = std::thread::Builder::new().stack_size(READER_STACK).spawn(move || {
            let job = theirs.lock().ok().and_then(|mut w| w.take());
            if let Some(job) = job {
                let found = job();
                // if WIC started COM on this thread, it ends here
                sys::com_done();
                drop(sender.send(found));
            }
        });
        if started.is_err() {
            if let Some(job) = work.lock().ok().and_then(|mut w| w.take()) {
                drop(answer.send(job()));
            }
        }
        self.reading = Some(Reading { path, answer: waiting });
    }

    /// What a thread has read, once it is ready.  A photo is kept, and the
    /// folder's list kept for the round about to begin; then the frame
    /// carries on towards where it was going -- or, going nowhere, reads the
    /// next photo ahead.
    fn collect(&mut self) -> Dirty {
        let Some(reading) = &self.reading else { return Dirty::None };
        let answer = match reading.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return Dirty::None,
            // the thread gave up without a word: as good as unreadable
            Err(TryRecvError::Disconnected) => Answer::Photo(None),
        };
        let Some(Reading { path, .. }) = self.reading.take() else { return Dirty::None };
        match (path, answer) {
            (Some(path), Answer::Photo(px)) => self.keep(path, px),
            (None, Answer::Folder(all)) => self.listed = Some(all),
            // a folder that could not be looked at has no photos in it
            (None, Answer::Photo(_)) => self.listed = Some(Vec::new()),
            (Some(_), Answer::Folder(_)) => {}
        }
        // with nothing up, the pane was waiting, not saying it had nothing
        let waiting = self.picture.is_empty() && self.going.is_some();
        let mut dirty = self.advance();
        if self.going.is_none() && !self.picture.is_empty() {
            self.read_ahead();
        }
        if waiting {
            dirty = dirty.and(Dirty::Part(self.changed()));
        }
        dirty
    }

    /// Keep a photo read, or that it would not read, the latest last; the
    /// oldest goes when there are more than `KEPT`.
    fn keep(&mut self, path: PathBuf, px: Option<Vec<u16>>) {
        self.take_kept(&path);
        if self.kept.len() == KEPT {
            self.kept.remove(0);
        }
        self.kept.push((path, px));
    }

    fn take_kept(&mut self, path: &Path) -> Option<Option<Vec<u16>>> {
        let at = self.kept.iter().position(|k| k.0 == path)?;
        Some(self.kept.remove(at).1)
    }

    fn restart_clock(&mut self) {
        self.left = if self.picture.is_empty() { self.interval.min(EMPTY_RETRY) } else { self.interval };
    }

    /// Stop the photos changing, or start them again -- the stop button and
    /// the play button in its place, and Pause in the menu -- and keep it in
    /// the frame's own section.
    fn set_stopped(&mut self, stopped: bool, ctx: &mut Ctx) -> Outcome {
        if stopped == self.paused {
            return Outcome::none();
        }
        self.paused = stopped;
        ctx.settings.set_int(&self.section, "Paused", stopped as i32);
        if !stopped {
            self.restart_clock();
        }
        Outcome::dirty(Dirty::Part(self.buttons()))
    }

    /// The picture's rectangle in the window, as the skin lays it out.
    fn pane(&self) -> Rect {
        self.rect_of("MeterPhoto")
    }

    /// The tray's floor, where the buttons stand.
    fn buttons(&self) -> Rect {
        self.rect_of("MeterButtons")
    }

    /// What another photo changes: the picture, and whether the back button
    /// has anywhere to go.
    fn changed(&self) -> Rect {
        self.pane().union(&self.buttons())
    }

    fn rect_of(&self, meter: &str) -> Rect {
        self.skin.meter(meter).map_or(Rect::EMPTY, |m| Rect::new(m.b[0], m.b[1], m.b[2], m.b[3]))
    }

    /// The row of buttons, each with its line round it and the floor between
    /// them, in the window: centred on the tray's floor.
    fn row(&self) -> Rect {
        let floor = self.buttons();
        let (w, h) = (3 * (self.keys.w + 2) + 2 * self.keys.gap, self.keys.h + 2);
        Rect::xywh(floor.x0 + (floor.x1 - floor.x0 - w) / 2, floor.y0 + (floor.y1 - floor.y0 - h) / 2, w, h)
    }

    /// The top left corner of button `k`, inside its line, in the window.
    fn key_origin(&self, k: usize) -> (i32, i32) {
        let row = self.row();
        (row.x0 + 1 + k as i32 * (self.keys.w + 2 + self.keys.gap), row.y0 + 1)
    }

    /// The button a point of the window is on.
    fn key_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..3).find(|&k| {
            let (kx, ky) = self.key_origin(k);
            Rect::xywh(kx, ky, self.keys.w, self.keys.h).contains(x, y)
        })
    }

    /// Whether button `k` can be pressed: the back button only while there
    /// is a photo to go back to.
    fn can_press(&self, k: usize) -> bool {
        k != BACK || !self.past.is_empty()
    }

    /// Where a button is headed: down while it is held, or while it still
    /// owes a press; up otherwise.
    fn aim(&self, p: &Push) -> i32 {
        if p.held || (p.owed && p.depth < self.keys.travel) { self.keys.travel } else { 0 }
    }

    /// Whether any button is on its way down or up.
    fn moving(&self) -> bool {
        self.pushes.iter().any(|p| p.depth != self.aim(p))
    }

    /// Button `k` held down: it gives at once, and owes the rest of its way.
    fn hold(&mut self, k: usize) {
        let p = &mut self.pushes[k];
        (p.held, p.owed, p.depth) = (true, true, p.depth.max(1));
        self.since = 0;
    }

    /// What button `k` is for, done when it is let go of over itself.
    fn activate(&mut self, k: usize, ctx: &mut Ctx) -> Outcome {
        match k {
            BACK => Outcome::dirty(self.previous()),
            STOP => self.set_stopped(!self.paused, ctx),
            _ => Outcome::dirty(self.next()),
        }
    }

    /// Button `k`, inside the line round it, its top left corner at (x, y).
    ///
    /// Up: a flat light face, a bright top row and left column, the right
    /// edge two shades darker and the foot three shades deep.  On its way in:
    /// the highlight gone and a shade along the top, the right edge and the
    /// foot a pixel thinner, the symbol a pixel lower.  In: the top and left
    /// in shade two pixels deep, the face a shade darker, the right edge and
    /// the foot gone, the symbol a pixel down and right of where it was.
    ///
    /// Every symbol is cut into the face the same way: the pixels on its top
    /// and left edges dark, the rest of it a shade under the face, and the
    /// face just under it and to its right lit -- light falling from the top
    /// left into a groove.  On a button that cannot be pressed the cut is
    /// faint: a pale edge, no darker middle.
    fn draw_key(&self, k: usize, x: i32, y: i32, canvas: &mut Canvas) {
        let Keys { w, h, travel, .. } = self.keys;
        let c = &self.colours;
        let d = self.pushes[k].depth;
        let face = if d == travel { c.face_down } else { c.face };
        canvas.fill(x, y, w, h, face);
        let (dx, dy) = match d {
            0 => {
                canvas.fill(x, y, w - 1, 1, c.hi);
                canvas.fill(x, y + 1, 1, h - 4, c.hi);
                canvas.fill(x + w - 2, y + 1, 1, h - 4, c.edge[0]);
                canvas.fill(x + w - 1, y, 1, h - 3, c.edge[1]);
                canvas.fill(x, y + h - 3, w, 1, c.edge[0]);
                canvas.fill(x, y + h - 2, w, 1, c.edge[1]);
                canvas.fill(x, y + h - 1, w, 1, c.edge[2]);
                (0, 0)
            }
            _ if d < travel => {
                canvas.fill(x, y, w, 1, c.edge[0]);
                canvas.fill(x + w - 1, y + 1, 1, h - 3, c.edge[0]);
                canvas.fill(x, y + h - 2, w, 1, c.edge[0]);
                canvas.fill(x, y + h - 1, w, 1, c.edge[1]);
                (0, 1)
            }
            _ => {
                canvas.fill(x, y, w, 1, c.edge[1]);
                canvas.fill(x, y + 1, 1, h - 1, c.edge[1]);
                canvas.fill(x + 1, y + 1, w - 1, 1, c.edge[0]);
                canvas.fill(x + 1, y + 2, 1, h - 2, c.edge[0]);
                (1, 1)
            }
        };

        // the symbol, centred on the face between the top row and the foot,
        // its light edge counted in
        let glyph: &[&str] = match k {
            BACK => &GLYPH_BACK,
            NEXT => &GLYPH_NEXT,
            _ if self.paused => &GLYPH_PLAY,
            _ => &GLYPH_STOP,
        };
        let (gw, gh) = (glyph[0].len() as i32, glyph.len() as i32);
        let (gx, gy) = (x + 1 + (w - 3 - gw) / 2 + dx, y + 1 + (h - 4 - (gh + 1)) / 2 + dy);
        let on = |col: i32, row: i32| {
            (0..gw).contains(&col) && (0..gh).contains(&row) && glyph[row as usize].as_bytes()[col as usize] == b'#'
        };
        let (dark, fill) = if self.can_press(k) { (c.dark, c.fill) } else { (c.off, face) };
        for row in 0..=gh {
            for col in 0..=gw {
                let colour = if on(col, row) {
                    if on(col - 1, row) && on(col, row - 1) { fill } else { dark }
                } else if on(col - 1, row) || on(col, row - 1) {
                    c.lit
                } else {
                    continue;
                };
                canvas.fill(gx + col, gy + row, 1, 1, colour);
            }
        }
    }
}

impl Hooks for Photo {
    /// HasPhoto keeps the pane from saying it has no photo: there is one up,
    /// or one on its way.
    fn num(&self, var: &str) -> f64 {
        match var {
            "HasPhoto" => (!self.picture.is_empty() || self.going.is_some()) as i32 as f64,
            _ => 0.0,
        }
    }

    /// [MeterPhoto]: a `block` x `block` square for each of the photo's
    /// pixels, a run of the same colour along a row at a time -- with none,
    /// the pane's own dark shows, and the skin says why.  [MeterButtons]: the
    /// three buttons, as far down as each is pressed.
    fn custom(&self, meter: &Meter, x: i32, y: i32, canvas: &mut Canvas, _text: &mut Text) {
        match meter.name {
            "MeterPhoto" => {
                let b = self.block;
                for (row, line) in self.picture.chunks(self.w.max(1)).enumerate() {
                    let top = y + row as i32 * b;
                    if top + b <= canvas.clip.y0 || top >= canvas.clip.y1 {
                        continue;
                    }
                    let mut start = 0;
                    while start < line.len() {
                        let end = start + line[start..].iter().take_while(|&&c| c == line[start]).count();
                        canvas.fill(x + start as i32 * b, top, (end - start) as i32 * b, b, line[start]);
                        start = end;
                    }
                }
            }
            "MeterButtons" => {
                let outside = canvas.clip;
                canvas.clip = outside.intersect(&self.buttons());
                let Keys { w, h, .. } = self.keys;
                for k in 0..3 {
                    let (kx, ky) = self.key_origin(k);
                    canvas.fill(kx - 1, ky - 1, w + 2, h + 2, self.colours.line);
                    self.draw_key(k, kx, ky, canvas);
                }
                canvas.clip = outside;
            }
            _ => {}
        }
    }
}

impl Widget for Photo {
    fn skin(&self) -> &'static Skin {
        self.skin
    }

    fn about(&self) -> &'static str {
        if self.look == Look::Dither {
            return "Win98 Dither Frame\n\n\
                    The photos in a folder, one after another, each brought down to \
                    one bit a pixel -- ink or paper, nothing between -- as a Macintosh \
                    of 1984 showed a scanned photo.  The ink is the colour the widgets \
                    are drawn in, so it turns with the colour picked in the menu.\n\n\
                    Put photos in its folder (right-click, Open folder), or choose \
                    another.  JPEG, PNG, BMP, GIF, TIFF and WebP photos are shown.\n\n\
                    Right-click for how it dithers: Atkinson's, the Macintosh's own, \
                    crisp and contrasty; Floyd and Steinberg's, with the most detail; \
                    or Bayer's even cross-hatching.  And, as on the Photo Frame, for \
                    the frame's shape and how long each photo stays; the buttons under \
                    the photo go back, stop and go on.  Dither Frame + in the Widgets \
                    menu makes another frame.";
        }
        "Win98 Photo Frame\n\n\
         The photos in a folder, one after another, each shown as the screen \
         of a Siemens C60 would have shown it: 101 x 80 pixels and 4096 \
         colours.\n\n\
         Put photos in its folder (right-click, Open folder), or choose \
         another.  JPEG, PNG, BMP, GIF, TIFF and WebP photos are shown \
         (WebP with the extension for it that Windows comes with).  HEIC \
         photos are not: Windows reads those only with extensions from the \
         Microsoft Store.\n\n\
         The buttons under the photo go back to the one before, stop the \
         photos changing (and start them again), and go on to the next; a \
         click on the photo goes on to the next as well.  Right-click for \
         the frame's shape -- upright, wide or square -- and how long each \
         photo stays.  Photo Frame + in the Widgets menu makes another frame, \
         with a folder and a shape of its own."
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Dirty {
        let collected = self.collect();
        if self.paused || ctx.locked {
            return collected;
        }
        self.left = self.left.saturating_sub(1);
        if self.left > 0 {
            return collected;
        }
        collected.and(self.next())
    }

    /// A press on a button is the button's: it goes down, and the window is
    /// not dragged.  (On the back button with nothing to go back to it is
    /// taken too, and nothing moves, as on a greyed Windows 98 button.)
    fn press(&mut self, x: i32, y: i32, _double: bool, _shift: bool) -> Pressed {
        let Some(k) = self.key_at(x, y) else { return Pressed::No };
        if !self.can_press(k) {
            self.pressing = None;
            return Pressed::Taken(Outcome::none());
        }
        self.pressing = Some(k);
        self.hold(k);
        Pressed::Taken(Outcome::dirty(Dirty::Part(self.buttons())))
    }

    /// Slid off the button it holds, the pointer lets it come up; back on
    /// it, the button goes down again.
    fn drag(&mut self, x: i32, y: i32) -> Dirty {
        let Some(k) = self.pressing else { return Dirty::None };
        let over = self.key_at(x, y) == Some(k);
        if over == self.pushes[k].held {
            return Dirty::None;
        }
        if over {
            self.hold(k);
        } else {
            self.pushes[k].held = false;
        }
        Dirty::Part(self.buttons())
    }

    /// Let go of over itself, a button does what it is for; it comes up
    /// either way.
    fn release(&mut self, x: i32, y: i32, ctx: &mut Ctx) -> Outcome {
        let Some(k) = self.pressing.take() else { return Outcome::none() };
        let fire = self.pushes[k].held && self.key_at(x, y) == Some(k);
        self.pushes[k].held = false;
        let mut outcome = if fire { self.activate(k, ctx) } else { Outcome::none() };
        outcome.dirty = outcome.dirty.and(Dirty::Part(self.buttons()));
        outcome
    }

    /// Frames only while a button is moving, or a photo is being read: each
    /// frame looks for it, so that it goes up soon after it is ready.
    fn frame_ms(&self) -> u32 {
        if self.moving() || self.reading.is_some() { PUSH_MS } else { 0 }
    }

    /// The photo being read, if it is ready; and a step further on for every
    /// button on its way, each `PUSH_MS`.
    fn frame(&mut self, ms: u32) -> Dirty {
        let collected = self.collect();
        self.since += ms;
        if self.since < PUSH_MS {
            return collected;
        }
        self.since = 0;
        let travel = self.keys.travel;
        let mut moved = false;
        for k in 0..3 {
            let aim = self.aim(&self.pushes[k]);
            let p = &mut self.pushes[k];
            if p.depth != aim {
                p.depth += (aim - p.depth).signum();
                moved = true;
            }
            if p.depth == travel {
                p.owed = false;
            }
        }
        if moved { collected.and(Dirty::Part(self.buttons())) } else { collected }
    }

    fn command(&mut self, cmd: &str, _arg: i32, _x: i32, _y: i32, ctx: &mut Ctx) -> Outcome {
        match cmd {
            "Next" => Outcome::dirty(self.next()),
            "Previous" => Outcome::dirty(self.previous()),
            "Stop" => self.set_stopped(true, ctx),
            "Play" => self.set_stopped(false, ctx),
            "About" => Outcome::request(Request::About),
            "Close" => Outcome::request(Request::Close),
            _ => Outcome::none(),
        }
    }

    fn menu(&self, items: &mut Vec<MenuItem>) {
        let line = || MenuItem { id: 0, label: "", checked: false };
        items.push(MenuItem { id: MENU_PREVIOUS, label: "Previous photo", checked: false });
        items.push(MenuItem { id: MENU_NEXT, label: "Next photo", checked: false });
        items.push(line());
        for (k, shape) in Shape::ALL.into_iter().enumerate() {
            items.push(MenuItem { id: MENU_SHAPE + k as u32, label: shape.label(), checked: shape == self.shape });
        }
        items.push(line());
        if self.look == Look::Dither {
            for (k, method) in Method::ALL.into_iter().enumerate() {
                items.push(MenuItem { id: MENU_METHOD + k as u32, label: method.label(), checked: method == self.method });
            }
            items.push(line());
        }
        for (k, &(seconds, label)) in INTERVALS.iter().enumerate() {
            items.push(MenuItem { id: MENU_INTERVAL + k as u32, label, checked: seconds == self.interval });
        }
        items.push(MenuItem { id: MENU_PAUSE, label: "Pause", checked: self.paused });
        items.push(line());
        items.push(MenuItem { id: MENU_CHOOSE, label: "Choose folder...", checked: false });
        items.push(MenuItem { id: MENU_OPEN, label: "Open folder", checked: false });
    }

    fn menu_command(&mut self, id: u32, ctx: &mut Ctx) -> Outcome {
        match id {
            MENU_NEXT => Outcome::dirty(self.next()),
            MENU_PREVIOUS => Outcome::dirty(self.previous()),
            _ if (MENU_SHAPE..MENU_SHAPE + Shape::ALL.len() as u32).contains(&id) => {
                let shape = Shape::ALL[(id - MENU_SHAPE) as usize];
                if shape == self.shape {
                    return Outcome::none();
                }
                ctx.settings.set(&self.section, "Shape", shape.key());
                Outcome::request(Request::Reshape)
            }
            _ if (MENU_INTERVAL..MENU_INTERVAL + INTERVALS.len() as u32).contains(&id) => {
                self.interval = INTERVALS[(id - MENU_INTERVAL) as usize].0;
                self.left = self.interval;
                ctx.settings.set_int(&self.section, "Interval", self.interval as i32);
                Outcome::none()
            }
            _ if self.look == Look::Dither && (MENU_METHOD..MENU_METHOD + Method::ALL.len() as u32).contains(&id) => {
                let method = Method::ALL[(id - MENU_METHOD) as usize];
                if method == self.method {
                    return Outcome::none();
                }
                self.method = method;
                ctx.settings.set(&self.section, "Method", method.key());
                // the photo up, again, from its tones: nothing is read
                self.picture = self.develop(&self.source);
                Outcome::dirty(Dirty::Part(self.pane()))
            }
            MENU_PAUSE => self.set_stopped(!self.paused, ctx),
            MENU_CHOOSE => Outcome::request(Request::ChooseFolder(self.folder.clone(), CHOOSE_PROMPT)),
            MENU_OPEN if self.folder.is_dir() => Outcome::request(Request::Open(self.folder.clone())),
            _ => Outcome::none(),
        }
    }

    fn folder_chosen(&mut self, folder: PathBuf, ctx: &mut Ctx) -> Dirty {
        ctx.settings.set(&self.section, "Folder", &folder.to_string_lossy());
        self.folder = folder;
        // the photos gone back to are another folder's
        self.forget_shown();
        self.next();
        Dirty::Part(self.changed())
    }

    /// The colours read again.  The Dither Frame's ink is the tint, so the
    /// photo up is painted again in it -- the Photo Frame's is in its own
    /// colours, and stays as it is.
    fn retint(&mut self) {
        self.read_colours();
        self.picture = self.develop(&self.source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, empty.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("win98-photo-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A small photo of one colour in `dir`.
    fn put(dir: &Path, name: &str, colour: u32) {
        std::fs::write(dir.join(name), decode::bmp(40, 30, |_, _| colour)).unwrap();
    }

    /// A frame with this section, on `dir`.
    fn frame(settings: &mut Settings, section: &str, dir: &Path) -> Photo {
        settings.set(section, "Folder", &dir.to_string_lossy());
        Photo::reading_at_once(&Ctx { now: 1_000, settings, locked: false, signed_in: None }, Look::Photo, section)
    }

    fn ctx(settings: &mut Settings) -> Ctx<'_> {
        Ctx { now: 1_000, settings, locked: false, signed_in: None }
    }

    /// A photo of a grey ramp, dark on the left to light on the right, in
    /// `dir`: something for a dither frame to dither.
    fn put_ramp(dir: &Path, name: &str) {
        let grey = |v: u32| 0xFF00_0000 | v << 16 | v << 8 | v;
        std::fs::write(dir.join(name), decode::bmp(64, 48, |x, _| grey(x as u32 * 4))).unwrap();
    }

    /// A dither frame on `dir`.
    fn dither_frame(settings: &mut Settings, dir: &Path) -> Photo {
        settings.set(dither::FIRST, "Folder", &dir.to_string_lossy());
        Photo::reading_at_once(&ctx(settings), Look::Dither, dither::FIRST)
    }

    #[test]
    fn a_dither_frame_is_one_bit_a_skin_pixel_in_ink_and_paper() {
        let dir = folder("dither-bits");
        put_ramp(&dir, "ramp.bmp");
        let mut settings = Settings::in_memory();
        for shape in Shape::ALL {
            settings.set(dither::FIRST, "Shape", shape.key());
            let photo = dither_frame(&mut settings, &dir);
            // as many 1-bit pixels as the skin has, each drawn as one of its own
            let skin = shape.skin(Look::Dither);
            assert_eq!((photo.block, photo.w, photo.h), (1, skin.int("PicW", 0) as usize, skin.int("PicH", 0) as usize));
            assert_eq!(skin.w, shape.skin(Look::Photo).w, "{shape:?}: the same frame as the Photo Frame's");
            assert_eq!(photo.picture.len(), photo.w * photo.h);
            // ink and paper and nothing between, dark on the left and light
            // on the right
            assert!(photo.picture.iter().all(|&p| p == photo.ink || p == photo.paper));
            let row = &photo.picture[photo.w * (photo.h / 2)..][..photo.w];
            assert_eq!((row[0], row[photo.w - 1]), (photo.ink, photo.paper));
        }
        // its own folder, beside settings.ini, not the Photo Frame's
        assert_eq!(Look::Dither.defaults().var("Folder"), "Dither");
        assert_ne!(Look::Dither.defaults().var("Folder"), Look::Photo.defaults().var("Folder"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dither_frame_dithers_as_its_menu_says_and_keeps_it() {
        let dir = folder("dither-method");
        put_ramp(&dir, "ramp.bmp");
        let mut settings = Settings::in_memory();
        let mut photo = dither_frame(&mut settings, &dir);
        let mut items = Vec::new();
        photo.menu(&mut items);
        let methods: Vec<(&str, bool)> = items.iter().filter(|i| (MENU_METHOD..MENU_METHOD + 3).contains(&i.id)).map(|i| (i.label, i.checked)).collect();
        assert_eq!(methods, [("Atkinson dither", true), ("Floyd-Steinberg dither", false), ("Bayer dither", false)]);
        let atkinson = photo.picture.clone();

        // Bayer's: another picture, at once, from the same photo -- nothing
        // read again -- and kept for the next run
        let outcome = photo.menu_command(MENU_METHOD + 2, &mut ctx(&mut settings));
        assert_eq!(outcome.dirty, Dirty::Part(photo.pane()));
        assert_ne!(photo.picture, atkinson);
        assert_eq!((photo.method, settings.text(dither::FIRST, "Method", "")), (Method::Bayer, String::from("bayer")));
        let again = dither_frame(&mut settings, &dir);
        assert_eq!((again.method, &again.picture), (Method::Bayer, &photo.picture));

        // and the Photo Frame has no such entries
        let mut settings = Settings::in_memory();
        let plain = frame(&mut settings, FIRST, &dir);
        let mut items = Vec::new();
        plain.menu(&mut items);
        assert!(!items.iter().any(|i| (MENU_METHOD..MENU_METHOD + 3).contains(&i.id)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dither_frames_ink_is_the_tint() {
        let dir = folder("dither-tint");
        put_ramp(&dir, "ramp.bmp");
        let mut settings = Settings::in_memory();
        crate::tint::set(crate::tint::BASE);
        let mut photo = dither_frame(&mut settings, &dir);
        let (ink, paper) = (photo.ink, photo.paper);
        let navy = crate::tint::PRESETS.iter().find(|p| p.0 == "Navy").unwrap().1;
        crate::tint::set(navy);
        photo.retint();
        // the ink turns with the tint, and the photo up is painted in it;
        // the paper is the fields' cream whatever the tint
        assert_ne!(photo.ink, ink);
        assert_eq!(photo.paper, paper);
        assert!(photo.picture.contains(&photo.ink) && !photo.picture.contains(&ink));
        crate::tint::set(crate::tint::BASE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_shape_is_the_c60s_pixels_drawn_two_by_two() {
        for (shape, pixels, frame) in
            [(Shape::Wide, (101, 80), (218, 223)), (Shape::Tall, (80, 101), (176, 265)), (Shape::Square, (80, 80), (176, 223))]
        {
            let skin = shape.skin(Look::Photo);
            let block = skin.int("Block", 0);
            assert_eq!(block, 2);
            assert_eq!((skin.int("PicW", 0) / block, skin.int("PicH", 0) / block), pixels, "{shape:?}");
            let pad = skin.int("Pad", 0);
            assert_eq!((skin.w - 2 * pad, skin.h - 2 * pad), frame, "{shape:?}");
        }
    }

    #[test]
    fn colours_come_down_to_sixteen_levels_each_dithered() {
        // black and white stay as they are
        let mut px = vec![0xFF00_0000, 0xFFFF_FFFF];
        dither12(&mut px, 2);
        assert_eq!(px, [0xFF00_0000, 0xFFFF_FFFF]);

        // every channel of anything lands on one of the 16 levels
        let mut px: Vec<u32> = (0..256u32).map(|v| 0xFF00_0000 | v << 16 | (255 - v) << 8 | (v * 7 % 256)).collect();
        dither12(&mut px, 16);
        assert!(px.iter().all(|p| [16, 8, 0].iter().all(|s| ((p >> s) & 0xFF) % 17 == 0)));

        // a flat grey half way between levels 7 and 8 becomes the Bayer
        // pattern of the two, half and half in every 4 x 4 square
        let mut px = vec![0xFF80_8080; 64];
        dither12(&mut px, 8);
        let (seven, eight) = (0xFF77_7777, 0xFF88_8888);
        assert!(px.iter().all(|&p| p == seven || p == eight));
        for (sx, sy) in [(0, 0), (4, 0), (0, 4), (4, 4)] {
            let eights = (0..16).filter(|k| px[(sy + k / 4) * 8 + sx + k % 4] == eight).count();
            assert_eq!(eights, 8);
        }
        // and its first row is the matrix's: 0 8 2 10 -- the two over half
        assert_eq!(&px[..4], [seven, eight, seven, eight]);
    }

    #[test]
    fn a_photo_is_kept_in_twelve_bits_and_drawn_as_it_was_brought_down() {
        // every one of the 4096 colours goes into its 12 bits and comes out
        // the same, and each is kept as 0xRGB
        for v in 0..4096u16 {
            let colour = unpack12(v);
            assert_eq!(colour >> 24, 0xFF);
            assert!([16, 8, 0].iter().all(|s| ((colour >> s) & 0xFF) % 17 == 0));
            assert_eq!(pack12(colour), v);
        }
        assert_eq!((unpack12(0x000), unpack12(0xFFF), unpack12(0xF80)), (0xFF00_0000, 0xFFFF_FFFF, 0xFFFF_8800));
        // and so is any photo, once it has been brought down
        let mut px: Vec<u32> = (0..1024u32).map(|v| 0xFF00_0000 | (v * 41 % 256) << 16 | (v * 7 % 256) << 8 | (v * 13 % 256)).collect();
        dither12(&mut px, 32);
        let kept: Vec<u16> = px.iter().map(|&p| pack12(p)).collect();
        assert_eq!(kept.iter().map(|&v| unpack12(v)).collect::<Vec<_>>(), px);
    }

    #[test]
    fn what_a_child_says_is_taken_only_if_it_is_the_whole_photo() {
        let px: Vec<u16> = vec![0, 1, 0xFFF, 0x1234, u16::MAX, 0x8000];
        let bytes = to_bytes(&px);
        assert_eq!(bytes.len(), 12);
        assert_eq!(&bytes[6..8], [0x34, 0x12]);
        assert_eq!(from_bytes(&bytes, px.len()), Some(px.clone()));
        // a child cut short, or saying more than was asked, is not believed
        assert_eq!(from_bytes(&bytes[..11], px.len()), None);
        assert_eq!(from_bytes(&bytes[..10], px.len()), None);
        assert_eq!(from_bytes(&bytes, px.len() - 1), None);
        assert_eq!(from_bytes(&[], 0), Some(Vec::new()));
    }

    #[test]
    fn a_child_asked_wrongly_reads_nothing() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(read_main(&args(&[])), 2);
        assert_eq!(read_main(&args(&["sepia", "101", "80", "FF000000", "a.jpg"])), 2);
        assert_eq!(read_main(&args(&["photo", "0", "80", "FF000000", "a.jpg"])), 2);
        assert_eq!(read_main(&args(&["photo", "101", "80", "not hex", "a.jpg"])), 2);
        assert_eq!(read_main(&args(&["dither", "101", "80", "FF000000"])), 2);
        assert_eq!(read_main(&args(&["dither", "4096", "4096", "FF000000", "a.jpg"])), 2);
        // and one asked rightly for a file that is not there says so
        assert_eq!(read_main(&args(&["photo", "101", "80", "FF000000", "Z:\\no\\such\\photo.jpg"])), 1);
    }

    #[test]
    fn every_photo_comes_round_once_and_a_new_one_joins_the_next_round() {
        let dir = folder("rounds");
        put(&dir, "a.bmp", 0xFF0000);
        put(&dir, "b.bmp", 0x00FF00);
        put(&dir, "c.BMP", 0x0000FF);
        // neither of these is shown: one Windows reads only with a Store
        // extension, one that is no picture at all
        std::fs::write(dir.join("d.heic"), b"heic").unwrap();
        std::fs::write(dir.join("e.jpg"), b"not a jpeg").unwrap();
        let mut settings = Settings::in_memory();
        let mut photo = frame(&mut settings, FIRST, &dir);

        let mut seen = vec![photo.shown.clone().expect("no photo shown")];
        for _ in 0..2 {
            photo.next();
            seen.push(photo.shown.clone().unwrap());
        }
        seen.sort();
        assert_eq!(seen, [dir.join("a.bmp"), dir.join("b.bmp"), dir.join("c.BMP")]);

        put(&dir, "f.bmp", 0xFFFFFF);
        let last = photo.shown.clone();
        let mut seen = Vec::new();
        for _ in 0..4 {
            photo.next();
            seen.push(photo.shown.clone().unwrap());
        }
        // the round does not start with the one just shown
        assert_ne!(seen[0], last.unwrap());
        seen.sort();
        assert_eq!(seen, [dir.join("a.bmp"), dir.join("b.bmp"), dir.join("c.BMP"), dir.join("f.bmp")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_photo_is_drawn_two_by_two_in_the_c60s_colours() {
        let dir = folder("drawn");
        // a colour between levels, which the dither turns into two
        put(&dir, "grey.bmp", 0x808080);
        let mut settings = Settings::in_memory();
        let photo = frame(&mut settings, FIRST, &dir);
        assert_eq!((photo.w, photo.h, photo.picture.len()), (101, 80, 101 * 80));
        assert_eq!(photo.num("HasPhoto"), 1.0);
        assert!(photo.picture.iter().all(|&p| p == 0xFF77_7777 || p == 0xFF88_8888));

        let skin = photo.skin();
        let mut px = vec![0u32; (skin.w * skin.h) as usize];
        // SAFETY: the canvas is exactly the vector.
        let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), skin.w, skin.w, skin.h) };
        let mut text = Text::new(skin.var("Font"), skin::FONTS).unwrap();
        let pane = photo.pane();
        photo.custom(skin.meter("MeterPhoto").unwrap(), pane.x0, pane.y0, &mut canvas, &mut text);
        drop(canvas);
        // each photo pixel a 2 x 2 square, the first two as the matrix has them
        let at = |x: i32, y: i32| px[((pane.y0 + y) * skin.w + pane.x0 + x) as usize];
        assert_eq!((pane.x1 - pane.x0, pane.y1 - pane.y0), (202, 160));
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(at(x, y), photo.picture[0]);
        }
        assert_eq!((at(2, 0), at(3, 1)), (photo.picture[1], photo.picture[1]));
        assert_ne!(photo.picture[0], photo.picture[1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_or_missing_folder_says_so_and_looks_again_soon() {
        let dir = folder("empty");
        let mut settings = Settings::in_memory();
        let mut photo = frame(&mut settings, FIRST, &dir);
        assert!(photo.picture.is_empty() && photo.shown.is_none());
        assert_eq!(photo.num("HasPhoto"), 0.0);
        assert_eq!(photo.next(), Dirty::None);
        assert_eq!(photo.left, EMPTY_RETRY);

        // a photo put in it turns up at the next look
        put(&dir, "late.bmp", 0x123456);
        let mut ctx = ctx(&mut settings);
        let changed: Vec<Dirty> = (0..EMPTY_RETRY).map(|_| photo.tick(&mut ctx)).collect();
        assert_eq!(changed.last(), Some(&Dirty::Part(photo.changed())));
        assert_eq!(photo.shown, Some(dir.join("late.bmp")));

        let gone = frame(&mut settings, "Photo2", &dir.join("not-there"));
        assert!(gone.picture.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_photo_changes_at_the_interval_and_not_while_paused() {
        let dir = folder("interval");
        put(&dir, "a.bmp", 0xFF0000);
        put(&dir, "b.bmp", 0x0000FF);
        let mut settings = Settings::in_memory();
        let mut photo = frame(&mut settings, FIRST, &dir);
        let mut ctx = ctx(&mut settings);
        photo.menu_command(MENU_INTERVAL, &mut ctx);
        assert_eq!(photo.interval, 5);
        assert_eq!(ctx.settings.get(FIRST, "Interval"), Some("5"));
        let first = photo.shown.clone();
        for _ in 0..4 {
            assert_eq!(photo.tick(&mut ctx), Dirty::None);
        }
        assert_eq!(photo.tick(&mut ctx), Dirty::Part(photo.changed()));
        assert_ne!(photo.shown, first);

        photo.menu_command(MENU_PAUSE, &mut ctx);
        assert_eq!(ctx.settings.get(FIRST, "Paused"), Some("1"));
        assert!((0..20).all(|_| photo.tick(&mut ctx) == Dirty::None));
        // a click still moves it on
        let before = photo.shown.clone();
        assert_eq!(photo.command("Next", 0, 0, 0, &mut ctx).dirty, Dirty::Part(photo.changed()));
        assert_ne!(photo.shown, before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_frame_keeps_its_shape_and_folder_in_its_own_section() {
        let dir = folder("own");
        let mut settings = Settings::in_memory();
        let mut second = frame(&mut settings, "Photo2", &dir);
        assert_eq!(second.skin().w, skin::photo_wide::SKIN.w);
        let mut ctx = ctx(&mut settings);

        // another shape is the host's to put up: it makes the window again
        let tall = MENU_SHAPE + Shape::ALL.iter().position(|&s| s == Shape::Tall).unwrap() as u32;
        assert_eq!(second.menu_command(tall, &mut ctx).request, Request::Reshape);
        assert_eq!(ctx.settings.get("Photo2", "Shape"), Some("tall"));
        assert_eq!(skin_of(ctx.settings, "Photo2").h, skin::photo_tall::SKIN.h);
        // the shape it has already is no change
        let wide = MENU_SHAPE + Shape::ALL.iter().position(|&s| s == Shape::Wide).unwrap() as u32;
        assert_eq!(second.menu_command(wide, &mut ctx).request, Request::None);

        // the folder is asked for from where it is, and kept when chosen
        assert_eq!(second.menu_command(MENU_CHOOSE, &mut ctx).request, Request::ChooseFolder(dir.clone(), CHOOSE_PROMPT));
        let other = folder("own-other");
        put(&other, "x.bmp", 0x00FF00);
        assert_eq!(second.folder_chosen(other.clone(), &mut ctx), Dirty::Part(second.changed()));
        assert_eq!(ctx.settings.get("Photo2", "Folder"), Some(other.to_string_lossy().as_ref()));
        assert_eq!(second.shown, Some(other.join("x.bmp")));

        // and the first frame's section is as it was
        assert_eq!(ctx.settings.get(FIRST, "Shape"), Some("wide"));
        assert_eq!(ctx.settings.get(FIRST, "Folder"), Some("Photos"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn frames_after_the_first_are_numbered_and_named() {
        let mut settings = Settings::in_memory();
        assert!(others(&settings).is_empty());
        assert_eq!(create(&mut settings), "Photo2");
        assert_eq!(create(&mut settings), "Photo3");
        assert_eq!(others(&settings), ["Photo2", "Photo3"]);
        assert_eq!((name(FIRST), name("Photo3")), (String::from("Photo Frame"), String::from("Photo Frame 3")));
        assert_eq!(settings.get("Photo3", "Shape"), Some("wide"));
        forget(&mut settings, "Photo2");
        assert_eq!(others(&settings), ["Photo3"]);
        assert_eq!(create(&mut settings), "Photo2");
    }

    #[test]
    fn the_folder_is_beside_settings_ini_unless_it_says_where() {
        let dir = folder("beside");
        let mut settings = Settings::in_memory_at(&dir);
        assert_eq!(folder_of(Look::Photo, &settings, FIRST), (dir.join("Photos"), true));
        // the folder a frame starts on is made for it
        let _ = Photo::new(&ctx(&mut settings), Look::Photo, FIRST);
        assert!(dir.join("Photos").is_dir());
        settings.set(FIRST, "Folder", "C:\\Pictures\\Holiday");
        assert_eq!(folder_of(Look::Photo, &settings, FIRST), (PathBuf::from("C:\\Pictures\\Holiday"), false));
        settings.set(FIRST, "Folder", "Mine");
        assert_eq!(folder_of(Look::Photo, &settings, FIRST), (dir.join("Mine"), false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn back_walks_through_what_was_shown_and_next_walks_forward_again() {
        let dir = folder("back");
        for (name, colour) in [("a.bmp", 0xFF0000), ("b.bmp", 0x00FF00), ("c.bmp", 0x0000FF), ("d.bmp", 0xFFFFFF)] {
            put(&dir, name, colour);
        }
        let mut settings = Settings::in_memory();
        let mut photo = frame(&mut settings, FIRST, &dir);
        // in the order of their names, so that each step is known
        photo.set_fixed();
        let mut ctx = ctx(&mut settings);
        let shown = |photo: &Photo| photo.shown.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let mut step = |photo: &mut Photo, cmd: &str| {
            let dirty = photo.command(cmd, 0, 0, 0, &mut ctx).dirty;
            (shown(photo).unwrap(), dirty != Dirty::None)
        };
        // nothing to go back to at first: the back button is greyed and does nothing
        assert_eq!(shown(&photo).as_deref(), Some("a.bmp"));
        assert!(!photo.can_press(BACK));
        assert_eq!(step(&mut photo, "Previous"), (String::from("a.bmp"), false));

        assert_eq!(step(&mut photo, "Next"), (String::from("b.bmp"), true));
        assert_eq!(step(&mut photo, "Next"), (String::from("c.bmp"), true));
        assert!(photo.can_press(BACK));
        assert_eq!(step(&mut photo, "Previous"), (String::from("b.bmp"), true));
        assert_eq!(step(&mut photo, "Previous"), (String::from("a.bmp"), true));
        assert_eq!(step(&mut photo, "Previous"), (String::from("a.bmp"), false));
        // forward again over the same ones, then on to a new one
        assert_eq!(step(&mut photo, "Next"), (String::from("b.bmp"), true));
        assert_eq!(step(&mut photo, "Next"), (String::from("c.bmp"), true));
        assert_eq!(step(&mut photo, "Next"), (String::from("d.bmp"), true));

        // a photo that has gone since is passed over
        assert_eq!(step(&mut photo, "Previous"), (String::from("c.bmp"), true));
        std::fs::remove_file(dir.join("b.bmp")).unwrap();
        assert_eq!(step(&mut photo, "Previous"), (String::from("a.bmp"), true));
        assert_eq!(step(&mut photo, "Next"), (String::from("c.bmp"), true));
        assert_eq!(step(&mut photo, "Next"), (String::from("d.bmp"), true));

        // and a new folder starts with nothing to go back to
        let other = folder("back-other");
        put(&other, "x.bmp", 0x808080);
        photo.folder_chosen(other.clone(), &mut ctx);
        assert!(!photo.can_press(BACK));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    /// A frame on a folder of two photos of its own, for the buttons.
    fn two_photos(settings: &mut Settings, name: &str) -> (Photo, PathBuf) {
        let dir = folder(name);
        put(&dir, "a.bmp", 0xFF0000);
        put(&dir, "b.bmp", 0x0000FF);
        (frame(settings, FIRST, &dir), dir)
    }

    /// The middle of button `k`.
    fn middle(photo: &Photo, k: usize) -> (i32, i32) {
        let (x, y) = photo.key_origin(k);
        (x + photo.keys.w / 2, y + photo.keys.h / 2)
    }

    /// Frames until no button is moving: how far down button `k` is after
    /// each.
    fn run(photo: &mut Photo, k: usize) -> Vec<i32> {
        let mut seen = Vec::new();
        while photo.frame_ms() > 0 && seen.len() < 20 {
            photo.frame(PUSH_MS);
            seen.push(photo.pushes[k].depth);
        }
        seen
    }

    #[test]
    fn a_button_sinks_while_held_and_does_its_work_when_let_go() {
        let mut settings = Settings::in_memory();
        let (mut photo, dir) = two_photos(&mut settings, "held");
        let mut ctx = ctx(&mut settings);
        let travel = photo.keys.travel;
        let (x, y) = middle(&photo, NEXT);
        let first = photo.shown.clone();
        // taken -- not the start of a drag of the window -- and it gives at
        // once, then sinks a pixel a frame to the bottom, and stays there
        assert!(matches!(photo.press(x, y, false, false), Pressed::Taken(_)));
        assert_eq!(photo.pushes[NEXT].depth, 1);
        assert_eq!(run(&mut photo, NEXT), (2..=travel).collect::<Vec<_>>());
        assert_eq!((photo.frame_ms(), photo.pushes[NEXT].depth), (0, travel));
        // held, it has done nothing yet
        assert_eq!(photo.shown, first);
        // let go of over itself: the next photo, and up a pixel a frame
        let outcome = photo.release(x, y, &mut ctx);
        assert_ne!(photo.shown, first);
        assert_eq!(outcome.dirty, Dirty::Part(photo.changed()));
        assert_eq!(run(&mut photo, NEXT), (0..travel).rev().collect::<Vec<_>>());
        assert_eq!(photo.frame_ms(), 0);
        // and a press anywhere else is not the buttons'
        let pane = photo.pane();
        assert!(matches!(photo.press(pane.x0 + 5, pane.y0 + 5, false, false), Pressed::No));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn however_short_the_click_the_button_goes_all_the_way_down() {
        let mut settings = Settings::in_memory();
        let (mut photo, dir) = two_photos(&mut settings, "short");
        let mut ctx = ctx(&mut settings);
        let travel = photo.keys.travel;
        let (x, y) = middle(&photo, NEXT);
        let first = photo.shown.clone();
        photo.press(x, y, false, false);
        photo.release(x, y, &mut ctx);
        assert_ne!(photo.shown, first);
        let all_the_way: Vec<i32> = (2..=travel).chain((0..travel).rev()).collect();
        assert_eq!(run(&mut photo, NEXT), all_the_way);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn slid_off_while_held_a_button_comes_up_and_does_nothing() {
        let mut settings = Settings::in_memory();
        let (mut photo, dir) = two_photos(&mut settings, "slid");
        let mut ctx = ctx(&mut settings);
        let (x, y) = middle(&photo, NEXT);
        let first = photo.shown.clone();
        photo.press(x, y, false, false);
        run(&mut photo, NEXT);
        // off it: up it comes
        assert_eq!(photo.drag(x, y - 40), Dirty::Part(photo.buttons()));
        assert_eq!(run(&mut photo, NEXT).last(), Some(&0));
        // back on it: down again
        assert_eq!(photo.drag(x, y), Dirty::Part(photo.buttons()));
        assert_eq!(run(&mut photo, NEXT).last(), Some(&photo.keys.travel));
        // let go of off it, or out of the window's sight: nothing done
        photo.drag(x + 200, y);
        photo.release(x + 200, y, &mut ctx);
        assert_eq!(photo.shown, first);
        photo.press(x, y, false, false);
        photo.release(i32::MIN, i32::MIN, &mut ctx);
        assert_eq!(photo.shown, first);
        assert_eq!(run(&mut photo, NEXT).last(), Some(&0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_back_button_cannot_be_pressed_with_nothing_to_go_back_to() {
        let mut settings = Settings::in_memory();
        let (mut photo, dir) = two_photos(&mut settings, "greyed");
        let mut ctx = ctx(&mut settings);
        let (x, y) = middle(&photo, BACK);
        let first = photo.shown.clone();
        // taken, so the window is not dragged, but nothing moves
        assert!(matches!(photo.press(x, y, false, false), Pressed::Taken(_)));
        assert_eq!((photo.pushes[BACK].depth, photo.frame_ms()), (0, 0));
        photo.release(x, y, &mut ctx);
        assert_eq!(photo.shown, first);
        // with a photo behind it, it goes down and back
        photo.next();
        assert!(photo.can_press(BACK));
        photo.press(x, y, false, false);
        photo.release(x, y, &mut ctx);
        assert_eq!(photo.shown, first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_stop_button_stops_the_photos_and_turns_into_play() {
        let mut settings = Settings::in_memory();
        let (mut photo, dir) = two_photos(&mut settings, "stop");
        let mut ctx = ctx(&mut settings);
        let (x, y) = middle(&photo, STOP);
        let playing = picture(&photo);
        photo.press(x, y, false, false);
        photo.release(x, y, &mut ctx);
        run(&mut photo, STOP);
        assert_eq!((photo.paused, ctx.settings.get(FIRST, "Paused")), (true, Some("1")));
        // the square is a triangle now, and the menu's Pause is ticked
        assert_ne!(picture(&photo), playing);
        let mut items = Vec::new();
        photo.menu(&mut items);
        assert!(items.iter().any(|m| m.label == "Pause" && m.checked));
        let still = photo.shown.clone();
        assert!((0..30).all(|_| photo.tick(&mut ctx) == Dirty::None));
        assert_eq!(photo.shown, still);
        // pressed again, the photos change again, the next after its full time
        photo.press(x, y, false, false);
        photo.release(x, y, &mut ctx);
        run(&mut photo, STOP);
        assert_eq!((photo.paused, ctx.settings.get(FIRST, "Paused")), (false, Some("0")));
        assert_eq!(picture(&photo), playing);
        assert!((1..photo.interval).all(|_| photo.tick(&mut ctx) == Dirty::None));
        assert_ne!(photo.tick(&mut ctx), Dirty::None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole frame drawn, as the window shows it at 1x.
    fn picture(photo: &Photo) -> Vec<u32> {
        let skin = photo.skin;
        let mut px = vec![0u32; (skin.w * skin.h) as usize];
        // SAFETY: the canvas is exactly the vector.
        let mut canvas = unsafe { Canvas::new(px.as_mut_ptr(), skin.w, skin.w, skin.h) };
        let mut text = Text::new(skin.var("Font"), skin::FONTS).unwrap();
        skin::draw(skin, photo as &dyn Hooks, &mut canvas, &mut text, &mut Vec::new());
        drop(canvas);
        px
    }

    #[test]
    fn a_button_is_drawn_up_on_its_way_and_in() {
        let mut settings = Settings::in_memory();
        let (mut photo, dir) = two_photos(&mut settings, "key-drawn");
        let Keys { w, h, .. } = photo.keys;
        let (kx, ky) = photo.key_origin(NEXT);
        let stride = photo.skin.w;
        let at = |px: &[u32], x: i32, y: i32| px[(y * stride + x) as usize];
        let c = photo.colours.clone();
        // the first row of the symbol with its dark edge on it
        let cut = |px: &[u32]| (ky..ky + h).find(|&yy| (kx..kx + w).any(|xx| at(px, xx, yy) == c.dark)).unwrap();

        // up: the line round it, the bright top and left, the right edge two
        // shades and the foot three
        let up = picture(&photo);
        assert_eq!((at(&up, kx - 1, ky + 5), at(&up, kx + 5, ky - 1)), (c.line, c.line));
        assert_eq!((at(&up, kx + 5, ky), at(&up, kx, ky + 5), at(&up, kx + 3, ky + 3)), (c.hi, c.hi, c.face));
        assert_eq!((at(&up, kx + w - 2, ky + 5), at(&up, kx + w - 1, ky + 5)), (c.edge[0], c.edge[1]));
        assert_eq!((at(&up, kx + 5, ky + h - 3), at(&up, kx + 5, ky + h - 2), at(&up, kx + 5, ky + h - 1)), (c.edge[0], c.edge[1], c.edge[2]));

        // a step in: the highlight gone, the foot thinner, the symbol lower
        let (x, y) = middle(&photo, NEXT);
        photo.press(x, y, false, false);
        let half = picture(&photo);
        assert_eq!((at(&half, kx + 5, ky), at(&half, kx, ky + 5)), (c.edge[0], c.face));
        assert_eq!((at(&half, kx + 5, ky + h - 3), at(&half, kx + 5, ky + h - 1)), (c.face, c.edge[1]));
        assert_eq!(cut(&half) - cut(&up), 1);

        // all the way in: the top and left in shade, the face darker, the
        // foot gone, the symbol down and right
        run(&mut photo, NEXT);
        let down = picture(&photo);
        assert_eq!((at(&down, kx + 5, ky), at(&down, kx + 5, ky + 1), at(&down, kx, ky + 5)), (c.edge[1], c.edge[0], c.edge[1]));
        assert_eq!((at(&down, kx + 5, ky + h - 1), at(&down, kx + w - 1, ky + 5)), (c.face_down, c.face_down));
        assert_eq!(cut(&down) - cut(&up), 1);
        let left_of = |px: &[u32]| (kx + 1..kx + w - 2).find(|&xx| (ky + 1..ky + h - 3).any(|yy| at(px, xx, yy) == c.dark)).unwrap();
        assert_eq!(left_of(&down) - left_of(&up), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_symbol_is_cut_the_same_way_and_is_as_tall() {
        let mut settings = Settings::in_memory();
        let (mut photo, dir) = two_photos(&mut settings, "cut");
        let mut ctx = ctx(&mut settings);
        let Keys { w, h, .. } = photo.keys;
        let stride = photo.skin.w;
        let c = photo.colours.clone();
        // a button's symbol: the rows and columns it takes, dark edge, middle
        // and light edge all together, and whether each of the three is there
        let symbol = |px: &[u32], k: usize, photo: &Photo| {
            let (kx, ky) = photo.key_origin(k);
            let mut rows = Vec::new();
            let mut found = [false; 3];
            for yy in ky + 1..ky + h - 3 {
                for xx in kx + 1..kx + w - 2 {
                    let p = px[(yy * stride + xx) as usize];
                    if let Some(i) = [c.dark, c.fill, c.lit].iter().position(|&q| q == p) {
                        found[i] = true;
                        rows.push(yy - ky);
                    }
                }
            }
            (rows.iter().min().copied(), rows.iter().max().copied(), found)
        };
        // with a photo to go back to, and stopped, so that play is drawn too
        photo.next();
        photo.set_stopped(true, &mut ctx);
        let stopped = picture(&photo);
        photo.set_stopped(false, &mut ctx);
        let playing = picture(&photo);
        let all = [symbol(&playing, BACK, &photo), symbol(&playing, STOP, &photo), symbol(&playing, NEXT, &photo), symbol(&stopped, STOP, &photo)];
        for s in &all {
            // nine rows and the light edge under them, in the same rows on
            // every button, each with its dark edge, middle and light edge
            assert_eq!((s.0, s.1), (all[0].0, all[0].1));
            assert_eq!(s.1.unwrap() - s.0.unwrap(), 9);
            assert_eq!(s.2, [true, true, true]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_buttons_are_a_row_in_a_shallow_tray_under_the_picture() {
        for shape in Shape::ALL {
            let mut settings = Settings::in_memory();
            settings.set(FIRST, "Shape", shape.key());
            let photo = Photo::new(&ctx(&mut settings), Look::Photo, FIRST);
            let skin = photo.skin;
            let pad = skin.int("Pad", 0);
            let frame_h = skin.h - 2 * pad;
            let b = |meter: &str| skin.meter(meter).unwrap().b;
            let (tray, pane, floor, row) = (b("MeterTray"), b("MeterPane"), photo.buttons(), photo.row());
            let Keys { w, gap, .. } = photo.keys;
            // just round the row, centred under the picture's bezel, 4 px
            // under it and 4 above the frame's bevel: 32 px from the bezel down
            assert_eq!(tray[0] - pane[0], pane[2] - tray[2], "{shape:?}");
            assert_eq!((tray[1] - pane[3], pad + frame_h - 2 - tray[3]), (4, 4), "{shape:?}");
            assert_eq!(pad + frame_h - pane[3], 32, "{shape:?}");
            // its floor inside its single ring, as much of it at either end of
            // the row as between two buttons, and a row above and below
            assert_eq!((floor.x0, floor.y0, floor.x1, floor.y1), (tray[0] + 1, tray[1] + 1, tray[2] - 1, tray[3] - 1), "{shape:?}");
            assert_eq!((row.x0 - floor.x0, floor.x1 - row.x1), (gap, gap), "{shape:?}");
            assert_eq!((row.y0 - floor.y0, floor.y1 - row.y1), (1, 1), "{shape:?}");
            // three buttons, each in its own line, `gap` of floor between
            let lines: Vec<(i32, i32)> = (0..3).map(|k| (photo.key_origin(k).0 - 1, photo.key_origin(k).0 + w + 1)).collect();
            assert_eq!((lines[0].0, lines[2].1), (row.x0, row.x1), "{shape:?}");
            assert_eq!((lines[1].0 - lines[0].1, lines[2].0 - lines[1].1), (gap, gap), "{shape:?}");
            assert!(gap > 0);
        }
    }

    /// Frames, as the host gives them while a photo is being read, until
    /// nothing is.
    fn settle(photo: &mut Photo) {
        let start = std::time::Instant::now();
        while photo.reading.is_some() {
            assert!(start.elapsed().as_secs() < 20, "a photo took too long to read");
            photo.frame(PUSH_MS);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// A frame as the host makes it, reading its photos on threads.
    fn threaded(settings: &mut Settings, dir: &Path) -> Photo {
        settings.set(FIRST, "Folder", &dir.to_string_lossy());
        Photo::new(&ctx(settings), Look::Photo, FIRST)
    }

    #[test]
    fn photos_are_read_on_threads_of_their_own_and_the_next_one_ahead() {
        let dir = folder("threads");
        for (name, colour) in [("a.bmp", 0xFF0000), ("b.bmp", 0x00FF00), ("c.bmp", 0x0000FF)] {
            put(&dir, name, colour);
        }
        let mut settings = Settings::in_memory();
        let mut photo = threaded(&mut settings, &dir);
        // made with its first photo still being read: nothing up yet, but the
        // pane does not say there is no photo, and it asks for frames to look
        assert!(photo.picture.is_empty() && photo.reading.is_some());
        assert_eq!(photo.num("HasPhoto"), 1.0);
        assert!(photo.frame_ms() > 0);

        // read, put up, and the one after it read ahead; then nothing moves
        settle(&mut photo);
        let first = photo.shown.clone().expect("the first photo never came");
        assert!(!photo.picture.is_empty());
        assert_eq!(photo.frame_ms(), 0);
        let upcoming = photo.round.last().cloned().unwrap();
        assert!(photo.kept.iter().any(|k| k.0 == upcoming && k.1.is_some()), "the next photo was not read ahead");

        // so going on to it is a swap, there and then, and so is going back
        assert_eq!(photo.next(), Dirty::Part(photo.changed()));
        assert_eq!(photo.shown.as_ref(), Some(&upcoming));
        assert_eq!(photo.previous(), Dirty::Part(photo.changed()));
        assert_eq!(photo.shown.as_ref(), Some(&first));
        // never more than a few kept in memory
        settle(&mut photo);
        assert!(photo.kept.len() <= KEPT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_of_broken_files_is_read_once_and_then_left() {
        let dir = folder("broken");
        std::fs::write(dir.join("a.jpg"), b"not a jpeg").unwrap();
        std::fs::write(dir.join("b.png"), b"not a png").unwrap();
        let mut settings = Settings::in_memory();
        let mut photo = threaded(&mut settings, &dir);
        settle(&mut photo);
        // every file tried once, and then the pane says there is nothing,
        // asks for no more frames, and waits to look again
        assert!(photo.picture.is_empty() && photo.going.is_none());
        assert_eq!((photo.num("HasPhoto"), photo.frame_ms(), photo.left), (0.0, 0, EMPTY_RETRY));
        // a photo put in it is found at the next look
        put(&dir, "c.bmp", 0x123456);
        let mut ctx = ctx(&mut settings);
        for _ in 0..EMPTY_RETRY {
            photo.tick(&mut ctx);
        }
        settle(&mut photo);
        assert_eq!(photo.shown, Some(dir.join("c.bmp")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
