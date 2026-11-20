//! The host: one small process, one thread, a layered window for each widget.
//!
//! There is one of each of the eleven, and as many more Sticky Notes,
//! Countdowns and Photo Frames as have been asked for: a note after the
//! first is a window like any other, made from the same skin, with a section
//! of settings.ini and a text file of its own (see `notes`), and a countdown
//! or a photo frame after the first is the same with a section and no file
//! (see `countdown`, `photo`).  Those three kinds are the families
//! (`FAMILIES`).  The windows are numbered -- the eleven first, then the
//! others as they were made -- and a number is never handed on: a deleted
//! one's stays its own, unused, until the program ends.
//!
//! A window's skin is its slot's: for every kind but one that is the kind's,
//! and a photo frame's is the one for the shape its section asks for.
//! Turned to another shape, its window is made again at the new size.
//!
//! Each widget is a borderless layered window that never takes the keyboard
//! and never shows in the taskbar or Alt-Tab -- a desktop widget, not an
//! application window.  (The note and the weather's city field are typed on
//! all the same: an unseen window the widget owns takes the keyboard for it
//! -- see `keys`.)  It is drawn into one shared 32-bit buffer and handed
//! to the window manager with UpdateLayeredWindow; only the part that changed
//! is redrawn and sent.
//!
//! Everything runs off one tick a second.  A widget that moves more often
//! than that -- the rain -- asks for frames, and a second timer runs for as
//! long as one does and there is a screen to show them on.
//!
//! The process is per-monitor DPI aware, and every place and size here is
//! in the monitors' own pixels.  The skins are laid out in whole pixels, and
//! a monitor at 225% would have to blur them to stretch them that much; so
//! each window is drawn at a whole number of times its skin instead -- its
//! slot's `n`, the scale of the monitor it is on (`screens::scale_of`):
//! drawn at its own size, then each pixel put up as an n x n block.  The
//! widgets know nothing of it -- they are drawn, and are handed the mouse,
//! in their own pixels -- and the margin and the gap are in those too.
//! Places written by a build from before this are in the coordinates Windows
//! made up for it, and are converted once (`convert_old_places`).
//!
//! Re-entrancy.  Windows calls a window procedure in the middle of other
//! calls (SetWindowPos sends WM_WINDOWPOSCHANGING, and so on).  So the
//! procedures only reach for the `App` on messages that arrive from the
//! queue, `with_app` refuses to nest, and anything modal -- a menu, a message
//! box -- is done after the `App` has been let go of.

use core::ffi::c_void;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering::Relaxed};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use crate::battery::Battery;
use crate::canvas::{self, Canvas, Rect};
use crate::claude::Claude;
use crate::clock;
use crate::countdown::{self, Countdown};
use crate::datetime::DateTime;
use crate::disk::Disk;
use crate::grid::{self, Spacing};
use crate::keys::{self, Axis, Input, Key, Keys};
use crate::notes::{self, Notes};
use crate::photo::{self, Photo};
use crate::rain::Rain;
use crate::resources::Resources;
use crate::screens::{self, work_area};
use crate::settings::Settings;
use crate::skin::{self, Act, Ev, Hooks, Skin};
use crate::sys::*;
use crate::text::Text;
use crate::tint;
use crate::weather::Weather;
use crate::widget::{Ctx, Dirty, Outcome, Pressed, Request, Widget};
use crate::world::World;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    DateTime,
    Resources,
    Notes,
    Weather,
    Rain,
    Disk,
    World,
    Battery,
    Countdown,
    Claude,
    Photo,
}

impl Kind {
    /// The kind's skin.  A photo frame's is the one for its shape, which
    /// `skin_in` knows; this is the shape a frame starts in.
    fn skin(self) -> &'static Skin {
        match self {
            Kind::DateTime => &skin::datetime::SKIN,
            Kind::Resources => &skin::resources::SKIN,
            Kind::Notes => &skin::notes::SKIN,
            Kind::Weather => &skin::weather::SKIN,
            Kind::Rain => &skin::rain::SKIN,
            Kind::Disk => &skin::disk::SKIN,
            Kind::World => &skin::world::SKIN,
            Kind::Battery => &skin::battery::SKIN,
            Kind::Countdown => &skin::countdown::SKIN,
            Kind::Claude => &skin::claude::SKIN,
            Kind::Photo => &skin::photo_wide::SKIN,
        }
    }

    /// The skin of the one of this kind with `section`: for a photo frame,
    /// the one its shape asks for.
    fn skin_in(self, settings: &Settings, section: &str) -> &'static Skin {
        match self {
            Kind::Photo => photo::skin_of(settings, section),
            _ => self.skin(),
        }
    }

    /// The family a kind there can be any number of is, if it is one.
    fn family(self) -> Option<&'static Family> {
        FAMILIES.iter().find(|f| f.kind == self)
    }
}

/// The widgets there is always one of, in the order their windows are
/// numbered: what each is, its section of settings.ini, and what the menus
/// call it.  The notes, countdowns and photo frames after the first follow
/// these.
const FIXED: [(Kind, &str, &str); 11] = [
    (Kind::DateTime, "DateTime", "Date/Time"),
    (Kind::Resources, "Resources", "Resource Meter"),
    (Kind::Notes, notes::FIRST, "Sticky Note"),
    (Kind::Weather, "Weather", "Weather"),
    (Kind::Rain, "Rain", "Rain Window"),
    (Kind::Disk, "Disk", "Disk Space"),
    (Kind::World, "World", "World Clock"),
    (Kind::Battery, "Battery", "Battery"),
    (Kind::Countdown, countdown::FIRST, "Countdown"),
    (Kind::Claude, "Claude", "Claude Usage"),
    (Kind::Photo, photo::FIRST, "Photo Frame"),
];
const DATETIME: usize = 0;
const NOTE: usize = 2;
const WEATHER: usize = 3;
const RAIN: usize = 4;
const DISK: usize = 5;
const WORLD: usize = 6;
const BATTERY: usize = 7;
const COUNTDOWN: usize = 8;
const CLAUDE: usize = 9;
const PHOTO: usize = 10;

/// A kind of widget there can be any number of: the first is in FIXED, and
/// the others are made from the menu, each with a section of settings.ini of
/// its own -- `[Notes2]`, `[Countdown3]`, `[Photo2]` -- that its own module
/// looks after.
struct Family {
    kind: Kind,
    /// The first's window.
    first: usize,
    /// Its entries in the menus: one more, one fewer, and -- on one after
    /// the first -- delete this one, with what that says.
    more: usize,
    fewer: usize,
    delete: usize,
    delete_label: &'static str,
    /// What one with this section is called in the menus.
    name: fn(&str) -> String,
    /// The sections of the ones after the first, in the order of their numbers.
    others: fn(&Settings) -> Vec<String>,
    /// A section for another; it returns the section.
    create: fn(&mut Settings) -> String,
    /// Take one out of settings.ini; what it leaves on disk is handed back.
    forget: fn(&mut Settings, &str) -> Vec<PathBuf>,
    /// What to ask before deleting one, if there is anything on it to lose.
    ask: fn(&Settings, &str) -> Option<String>,
}

const FAMILIES: [Family; 3] = [
    Family {
        kind: Kind::Notes,
        first: NOTE,
        more: CMD_MORE_NOTES,
        fewer: CMD_FEWER_NOTES,
        delete: CMD_DELETE_NOTE,
        delete_label: "Delete this note",
        name: notes::name,
        others: notes::others,
        create: notes::create,
        forget: notes::forget,
        // (The right click that opened the menu ended any typing, so what is
        // in the file is what is on the sheet.)
        ask: |settings, section| {
            let file = notes::file(settings, section);
            (!notes::is_blank(&file)).then(|| format!("Delete this note?\n\nIts text, {}, goes to the Recycle Bin.", file.display()))
        },
    },
    Family {
        kind: Kind::Countdown,
        first: COUNTDOWN,
        more: CMD_MORE_COUNTDOWNS,
        fewer: CMD_FEWER_COUNTDOWNS,
        delete: CMD_DELETE_COUNTDOWN,
        delete_label: "Delete this countdown",
        name: countdown::name,
        others: countdown::others,
        create: countdown::create,
        forget: countdown::forget,
        ask: |settings, section| {
            (!countdown::is_blank(settings, section))
                .then(|| String::from("Delete this countdown?\n\nThe day it counts to and its name are forgotten."))
        },
    },
    Family {
        kind: Kind::Photo,
        first: PHOTO,
        more: CMD_MORE_PHOTOS,
        fewer: CMD_FEWER_PHOTOS,
        delete: CMD_DELETE_PHOTO,
        delete_label: "Delete this photo frame",
        name: photo::name,
        others: photo::others,
        create: photo::create,
        forget: photo::forget,
        // nothing is lost: the photos stay in their folder
        ask: |_, _| None,
    },
];

const WIDGET_CLASS: &str = "Win98Widget";
const MAIN_CLASS: &str = "Win98WidgetsMain";
const MAIN_TITLE: &str = "Win98 Widgets";
const MUTEX: &str = "Local\\Win98Widgets.Running";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "Win98Widgets";

const TICK_TIMER: usize = 1;
/// Runs only while a widget is asking for frames.
const FRAME_TIMER: usize = 2;
const WM_TRAY: u32 = WM_APP + 2;
/// The tray's icon, the .ico file as it is on disk.
static TRAY_ICO: &[u8] = include_bytes!("../skins/Icons/gears_tweakui_b.ico");
/// The grid's margin and gap until settings.ini says otherwise: where
/// dragging left a widget before they could be chosen, when it snapped the
/// window -- frame and the 12 px of nothing round it -- to the screen's
/// edge and to the next window.
const SPACING: Spacing = Spacing { margin: 12, gap: 24 };
/// What the menu offers for the margin and for the gap, in px.  Any other
/// figure can be written into settings.ini.
const SPACINGS: [i32; 8] = [0, 4, 8, 10, 12, 16, 24, 32];
/// No margin or gap is wider than this.
const MOST_SPACE: i32 = 200;
/// How many widths, or heights, from a note or a countdown room for a new
/// one is looked for.
const ROOM_STEPS: i32 = 8;
/// With no room anywhere, a new one sits this far down and right of the old.
const CASCADE: i32 = 24;
/// Under [General] in settings.ini, 1 once the places there are in the
/// monitors' own pixels: written by a build that is DPI aware.
const DEVICE_PIXELS: &str = "DevicePixels";
/// Our own z-order changes do not ask the window for permission.
const ZPOS_FLAGS: u32 = SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING;

// menu commands
const CMD_DESKTOP: usize = 10;
const CMD_NORMAL: usize = 11;
const CMD_TOPMOST: usize = 12;
const CMD_SNAP: usize = 20;
const CMD_LOCK: usize = 21;
const CMD_CLOSE: usize = 30;
const CMD_MORE_NOTES: usize = 40;
const CMD_DELETE_NOTE: usize = 41;
const CMD_FEWER_NOTES: usize = 42;
const CMD_MORE_COUNTDOWNS: usize = 43;
const CMD_DELETE_COUNTDOWN: usize = 44;
const CMD_FEWER_COUNTDOWNS: usize = 45;
const CMD_MORE_PHOTOS: usize = 46;
const CMD_DELETE_PHOTO: usize = 47;
const CMD_FEWER_PHOTOS: usize = 48;
const CMD_AUTOSTART: usize = 50;
const CMD_SETTINGS: usize = 51;
const CMD_RELOAD: usize = 52;
const CMD_EXIT: usize = 60;
/// From here, the margin and the gap, by their place in the menu.
const CMD_MARGIN: usize = 70;
const CMD_GAP: usize = 80;
/// From here up to CMD_TINT: the widget's own entries.
const CMD_WIDGET: usize = 100;
/// From here, the tints the Colour menu names, by their place in it.
const CMD_TINT: usize = 900;
/// The Colour menu's last entry: any colour, from the colour dialog.
const CMD_TINT_CUSTOM: usize = 999;
/// From here up: show or close a window, by its number.
const CMD_TOGGLE: usize = 1000;
// Each list of numbers ends before the next begins: the margin's and the
// gap's each the figures always offered and one more, then the widgets'
// own, then the tints.
const _: () = assert!(CMD_MARGIN + SPACINGS.len() + 1 <= CMD_GAP);
const _: () = assert!(CMD_GAP + SPACINGS.len() + 1 <= CMD_WIDGET);
const _: () = assert!(CMD_TINT + tint::PRESETS.len() <= CMD_TINT_CUSTOM);
const _: () = assert!(CMD_TINT_CUSTOM < CMD_TOGGLE);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    /// Behind every window, and still there when the desktop is shown.
    Desktop = 0,
    Normal = 1,
    Topmost = 2,
}

/// What the widgets' window procedure may know without the App.
static POSITION: AtomicU8 = AtomicU8::new(0);
static MAIN: AtomicIsize = AtomicIsize::new(0);
static IN_APP: AtomicBool = AtomicBool::new(false);
static mut APP: Option<App> = None;

/// The App, unless it is already in use further up the stack.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    if IN_APP.swap(true, Relaxed) {
        return None;
    }
    // SAFETY: one thread, and IN_APP makes this the only reference.
    let r = unsafe { (*(&raw mut APP)).as_mut().map(f) };
    IN_APP.store(false, Relaxed);
    r
}

/// Something to do once the App has been let go of, because it runs a
/// message loop of its own.
enum Later {
    Nothing,
    Menu(Option<usize>),
    About(usize),
    Open(PathBuf),
    /// Delete the note or countdown with this section -- not its number:
    /// others may come and go while the question is on the screen.  With it,
    /// the title and the text of the question, for one that has something on
    /// it.
    Delete(String, Option<(String, String)>),
    /// The colour dialog, starting from this tint (0xRRGGBB): what is picked
    /// in it is the tint from then on.
    PickColour(u32),
    /// The folder dialog for the widget with this section -- again not its
    /// number -- starting from this folder.
    ChooseFolder(String, PathBuf),
    Quit,
}

/// The frame of a window of a `skin` at (x, y), drawn at `n` times that: a
/// window has `Pad` of nothing round it.
fn frame(skin: &Skin, x: i32, y: i32, n: i32) -> Rect {
    let pad = skin.int("Pad", 12) * n;
    Rect::new(x + pad, y + pad, x + skin.w * n - pad, y + skin.h * n - pad)
}

/// The scale a window of a `skin` at (x, y), drawn at `n` now, is to be
/// drawn at: that of the monitor most of its frame is on -- unless at that
/// scale, larger or smaller, most of it would be on a monitor of another.
/// Then it stays as it is, so that a window across the line between two
/// monitors is not made larger and smaller by turns.
fn settled_scale(skin: &Skin, x: i32, y: i32, n: i32) -> i32 {
    let here = screens::scale_of(&on_screens(frame(skin, x, y, n)));
    if here == n || screens::scale_of(&on_screens(frame(skin, x, y, here))) != here { n } else { here }
}

/// One copy of every skin fits in this many pixels across and down: each
/// kind's, and each shape of photo frame.
fn largest_skin() -> (i32, i32) {
    let photos = [&skin::photo_wide::SKIN, &skin::photo_tall::SKIN, &skin::photo_square::SKIN];
    FIXED.iter().map(|f| f.0.skin()).chain(photos).fold((0, 0), |(w, h), s| (w.max(s.w), h.max(s.h)))
}

/// A wheel turned `delta` on top of the part of a notch it had turned
/// already, `rest`: the skin's event for that way, and how many whole notches
/// to fire it for.  What is short of a notch stays in `rest` -- a touchpad
/// turns a little at a time.
fn notches(rest: &mut i32, axis: Axis, delta: i32) -> (Ev, u32) {
    *rest += delta;
    let whole = *rest / WHEEL_DELTA;
    *rest -= whole * WHEEL_DELTA;
    let ev = match (axis, whole > 0) {
        (Axis::Upright, true) => Ev::WheelUp,
        (Axis::Upright, false) => Ev::WheelDown,
        (Axis::Sideways, true) => Ev::WheelRight,
        (Axis::Sideways, false) => Ev::WheelLeft,
    };
    (ev, whole.unsigned_abs())
}

struct Slot {
    kind: Kind,
    /// What it is drawn from: the kind's skin, or for a photo frame the one
    /// for its shape.  The window is this size, n times over.
    skin: &'static Skin,
    /// Its section of settings.ini, and what the menus call it.
    section: String,
    title: String,
    /// A note, countdown or photo frame that has been deleted.  Nothing is
    /// left of it but its number, which no other window is given.
    deleted: bool,
    hwnd: HWND,
    /// Where the window is.
    x: i32,
    y: i32,
    /// Where it was put -- dragged to, or given when it was made -- and what
    /// settings.ini says.  It is shown there whenever the monitors there are
    /// show it whole there, and as near as they let it be when they do not.
    /// Only putting it somewhere changes this; the monitors coming and going
    /// never do, so a window comes back to its place with the monitor it was
    /// on.
    put: (i32, i32),
    /// How many times its skin the window is drawn at: the scale of the
    /// monitor it is on (see `settled_scale`).
    n: i32,
    /// The meter the pointer is over, if it is one that reacts to that.
    hover: Option<usize>,
    tracking: bool,
    seen: skin::Seen,
}

impl Slot {
    /// Window `kind` with `section`, drawn from the skin settings.ini gives
    /// that one.
    fn new(kind: Kind, section: &str, title: String, settings: &Settings) -> Slot {
        Slot {
            kind,
            skin: kind.skin_in(settings, section),
            section: section.to_owned(),
            title,
            deleted: false,
            hwnd: 0,
            x: 0,
            y: 0,
            put: (0, 0),
            n: 1,
            hover: None,
            tracking: false,
            seen: Vec::new(),
        }
    }

    /// Its frame where the window is.
    fn frame(&self) -> Rect {
        frame(self.skin, self.x, self.y, self.n)
    }
}

/// A press that has not been released yet.
struct Press {
    idx: usize,
    cursor: POINT,
    window: POINT,
    dragging: bool,
    /// The widget took the press for itself: it is not the window's to drag.
    owned: bool,
    /// The press was the second of a double-click that did something: the
    /// release that follows it is not a click.
    spent: bool,
}

/// What a window is handed to the window manager from: in the screen's
/// pixels, as large as the largest skin at the largest scale yet drawn.
struct Buffer {
    dc: HDC,
    bitmap: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
}

/// The widgets that are running, each under its slot's number; None for one
/// that is closed.  Always as long as the list of slots.
struct Widgets(Vec<Option<Box<dyn Widget>>>);

impl Widgets {
    fn get(&self, i: usize) -> Option<&dyn Widget> {
        self.0.get(i)?.as_deref()
    }

    fn get_mut(&mut self, i: usize) -> Option<&mut dyn Widget> {
        match self.0.get_mut(i) {
            Some(Some(w)) => Some(w.as_mut()),
            _ => None,
        }
    }
}

struct App {
    settings: Settings,
    text: Text,
    /// Where every widget is drawn, at its own size: as large as the largest
    /// skin, rows `largest_skin().0` apart.  Put up in `buffer` from there.
    picture: Vec<u32>,
    buffer: Buffer,
    widgets: Widgets,
    slots: Vec<Slot>,
    /// The window that holds the keyboard while a widget is typed on, and
    /// which widget that is.
    keys: Keys,
    typing: Option<usize>,
    press: Option<Press>,
    /// What the wheels have turned short of a whole notch, upright and
    /// sideways.
    wheel: i32,
    sideways: i32,
    position: Position,
    snap: bool,
    /// The grid the widgets are laid out on.
    spacing: Spacing,
    locked_in_place: bool,
    /// The workstation is locked: nothing is drawn, the log still runs.
    locked: bool,
    signed_in: Option<i64>,
    show_desktop: bool,
    tray: HICON,
    ticks: u32,
    /// The tick at which to hand unused pages back to Windows.
    trim_at: u32,
    /// What the frame timer is set to, 0 while it is not running, and when
    /// the last frame was, in milliseconds.
    frame_ms: u32,
    frame_at: i64,
}

// ---------------------------------------------------------------------------
//  Session, screen and registry helpers
// ---------------------------------------------------------------------------

/// Whether the workstation is locked right now, and when this user signed in.
fn session_state() -> (bool, Option<i64>) {
    type Query = unsafe extern "system" fn(isize, u32, i32, *mut *mut WTSINFOEXW, *mut u32) -> BOOL;
    type Free = unsafe extern "system" fn(*mut c_void);
    unsafe {
        let (Some(query), Some(free)) = (
            proc_address("wtsapi32.dll", b"WTSQuerySessionInformationW\0"),
            proc_address("wtsapi32.dll", b"WTSFreeMemory\0"),
        ) else {
            return (false, None);
        };
        let query: Query = core::mem::transmute(query);
        let free: Free = core::mem::transmute(free);
        let (mut info, mut len) = (null_mut::<WTSINFOEXW>(), 0u32);
        if query(0, WTS_CURRENT_SESSION, WTS_INFO_SESSION_EX, &mut info, &mut len) == 0 || info.is_null() {
            return (false, None);
        }
        let mut state = (false, None);
        if len as usize >= size_of::<WTSINFOEXW>() && (*info).Level == 1 {
            state.0 = (*info).SessionFlags == WTS_SESSIONSTATE_LOCK;
            if (*info).LogonTime > 0 {
                state.1 = Some(clock::from_filetime((*info).LogonTime));
            }
        }
        free(info as *mut c_void);
        state
    }
}

fn register_session_notifications(wnd: HWND) {
    type Register = unsafe extern "system" fn(HWND, u32) -> BOOL;
    unsafe {
        if let Some(p) = proc_address("wtsapi32.dll", b"WTSRegisterSessionNotification\0") {
            let register: Register = core::mem::transmute(p);
            register(wnd, 0); // this session only
        }
    }
}

/// A rectangle as the screens have them.
fn on_screens(r: Rect) -> RECT {
    RECT { left: r.x0, top: r.y0, right: r.x1, bottom: r.y1 }
}

/// What has to be on the monitors for a window of a `skin` at (x, y), drawn
/// at `n`, to be seen whole: its frame, and a margin of bare screen round
/// that (`margin` of the widget's own pixels, n times as many of the
/// screen's).
fn reach(skin: &Skin, x: i32, y: i32, margin: i32, n: i32) -> RECT {
    let (f, margin) = (frame(skin, x, y, n), margin * n);
    RECT { left: f.x0 - margin, top: f.y0 - margin, right: f.x1 + margin, bottom: f.y1 + margin }
}

/// Where a window of a `skin` meant for (x, y) can go and be seen whole,
/// `margin` in from the edge: there, or the least distance from there onto
/// the monitors there are now.
fn on_screen(skin: &Skin, x: i32, y: i32, margin: i32, n: i32) -> (i32, i32) {
    let (dx, dy) = screens::keep_on(&reach(skin, x, y, margin, n), &screens::work_areas());
    (x + dx, y + dy)
}

/// What the menu offers for a margin or a gap that is `now`: the figures
/// it always offers, and `now` in its place among them if it is not one.
fn spacings(now: i32) -> Vec<i32> {
    let mut all = SPACINGS.to_vec();
    if !all.contains(&now) {
        all.push(now);
        all.sort();
    }
    all
}

fn autostart_command() -> String {
    std::env::current_exe().map(|p| format!("\"{}\"", p.display())).unwrap_or_default()
}

fn autostart_enabled() -> bool {
    unsafe {
        let mut key: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return false;
        }
        let mut buf = [0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let found = RegQueryValueExW(key, wide(RUN_VALUE).as_ptr(), null(), null_mut(), buf.as_mut_ptr() as *mut u8, &mut len) == 0;
        RegCloseKey(key);
        found && from_wide(&buf).eq_ignore_ascii_case(&autostart_command())
    }
}

fn set_autostart(on: bool) {
    unsafe {
        let mut key: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut key) != 0 {
            return;
        }
        if on {
            let value = wide(&autostart_command());
            RegSetValueExW(key, wide(RUN_VALUE).as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32);
        } else {
            RegDeleteValueW(key, wide(RUN_VALUE).as_ptr());
        }
        RegCloseKey(key);
    }
}

// ---------------------------------------------------------------------------
//  The desktop: staying on it, and staying on it when it is "shown"
// ---------------------------------------------------------------------------

fn class_is(wnd: HWND, name: &str) -> bool {
    let mut buf = [0u16; 64];
    unsafe { GetClassNameW(wnd, buf.as_mut_ptr(), buf.len() as i32) };
    from_wide(&buf) == name
}

/// The window the desktop icons live in: Progman itself from Windows 11 24H2,
/// before that the WorkerW that holds SHELLDLL_DefView.
fn desktop_icons_host() -> HWND {
    unsafe {
        let shell = GetShellWindow();
        if shell == 0 || !class_is(shell, "Progman") {
            return 0;
        }
        let def_view = wide("SHELLDLL_DefView");
        if FindWindowExW(shell, 0, def_view.as_ptr(), null()) != 0 {
            return shell;
        }
        let worker = wide("WorkerW");
        let mut wnd = 0;
        loop {
            wnd = FindWindowExW(0, wnd, worker.as_ptr(), null());
            if wnd == 0 {
                return 0;
            }
            if IsWindowVisible(wnd) != 0 && FindWindowExW(wnd, 0, def_view.as_ptr(), null()) != 0 {
                return wnd;
            }
        }
    }
}

/// "Show desktop" raises the desktop over everything that cannot be
/// minimised, the widgets included.  It is on when our own always-at-the-
/// bottom window has ended up underneath the desktop's.
fn desktop_is_shown() -> bool {
    unsafe {
        let host = desktop_icons_host();
        host != 0
            && IsWindowVisible(host) != 0
            && FindWindowExW(0, host, wide(MAIN_CLASS).as_ptr(), wide(MAIN_TITLE).as_ptr()) != 0
    }
}

/// Put a widget where its position says it goes.
fn place_in_z_order(wnd: HWND, position: Position, show_desktop: bool) {
    unsafe {
        let after = match position {
            Position::Topmost => HWND_TOPMOST,
            Position::Normal => HWND_NOTOPMOST,
            Position::Desktop if show_desktop => {
                // Over the raised desktop, under everything that is really
                // topmost: after the backmost topmost window above it.
                let mut above = desktop_icons_host();
                loop {
                    above = GetWindow(above, GW_HWNDPREV);
                    if above == 0 {
                        break HWND_TOPMOST;
                    }
                    if GetWindowLongPtrW(above, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 && above != wnd {
                        break above;
                    }
                }
            }
            Position::Desktop => HWND_BOTTOM,
        };
        SetWindowPos(wnd, after, 0, 0, 0, 0, ZPOS_FLAGS);
    }
}

unsafe extern "system" fn foreground_changed(_hook: isize, _event: u32, _wnd: HWND, _object: i32, _child: i32, _thread: u32, _time: u32) {
    with_app(|app| app.check_desktop());
}

// ---------------------------------------------------------------------------
//  The App
// ---------------------------------------------------------------------------

impl Buffer {
    fn new(w: i32, h: i32) -> Buffer {
        unsafe {
            let header = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            };
            let dc = CreateCompatibleDC(0);
            let mut bits: *mut c_void = null_mut();
            let bitmap = CreateDIBSection(dc, &header, DIB_RGB_COLORS, &mut bits, 0, 0);
            SelectObject(dc, bitmap);
            Buffer { dc, bitmap, bits: bits as *mut u32, w, h }
        }
    }

    /// Its pixels, row after row.
    fn pixels(&mut self) -> &mut [u32] {
        if self.bits.is_null() {
            return &mut [];
        }
        // SAFETY: the DIB is w x h 32-bit pixels, and lives as long as self.
        unsafe { core::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }
}

impl App {
    /// The App as settings.ini describes it, with no widget started yet.
    fn new(settings: Settings, text: Text, locked: bool, signed_in: Option<i64>) -> App {
        // One picture, as large as the largest skin: every window is drawn in
        // it, and put up from it in the buffer, which is made larger when a
        // window is first drawn at a larger scale.
        let (w, h) = largest_skin();
        let mut app = App {
            settings,
            text,
            picture: vec![0; (w * h) as usize],
            buffer: Buffer::new(w, h),
            // both filled in by read_slots, from settings.ini
            widgets: Widgets(Vec::new()),
            slots: Vec::new(),
            keys: Keys::new(),
            typing: None,
            press: None,
            wheel: 0,
            sideways: 0,
            position: Position::Desktop,
            snap: true,
            spacing: SPACING,
            locked_in_place: false,
            locked,
            signed_in,
            show_desktop: false,
            tray: 0,
            ticks: 0,
            trim_at: 5,
            frame_ms: 0,
            frame_at: 0,
        };
        app.read_general();
        app.read_slots();
        app
    }

    fn ctx(&mut self) -> (Ctx<'_>, &mut Widgets) {
        (
            Ctx { now: clock::now(), settings: &mut self.settings, locked: self.locked, signed_in: self.signed_in },
            &mut self.widgets,
        )
    }

    /// Bring a widget to life: its state, then its window.
    fn start(&mut self, i: usize) {
        if self.slots[i].hwnd != 0 || self.slots[i].deleted {
            return;
        }
        let (kind, section) = (self.slots[i].kind, self.slots[i].section.clone());
        // the skin the widget is about to be made with: a photo frame's may
        // be another since the window was last made (`reshape`)
        self.slots[i].skin = kind.skin_in(&self.settings, &section);
        {
            let (mut ctx, widgets) = self.ctx();
            let mut widget: Box<dyn Widget> = match kind {
                Kind::DateTime => Box::new(DateTime::new(&ctx, None)),
                Kind::Resources => Box::new(Resources::new()),
                Kind::Notes => Box::new(Notes::new(&mut ctx, &section, None)),
                Kind::Weather => Box::new(Weather::new(&ctx)),
                Kind::Rain => Box::new(Rain::new(&ctx)),
                Kind::Disk => Box::new(Disk::new(&ctx)),
                Kind::World => Box::new(World::new(&ctx)),
                Kind::Battery => Box::new(Battery::new(&ctx)),
                Kind::Countdown => Box::new(Countdown::new(&ctx, &section)),
                Kind::Claude => Box::new(Claude::new(&ctx)),
                Kind::Photo => Box::new(Photo::new(&ctx, &section)),
            };
            widget.tick(&mut ctx);
            widgets.0[i] = Some(widget);
        }
        // Running now, so laid out with the others: where it was put, or
        // where it can be seen.
        self.lay_out();
        let (skin, x, y, n) = (self.slots[i].skin, self.slots[i].x, self.slots[i].y, self.slots[i].n);
        unsafe {
            let wnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                wide(WIDGET_CLASS).as_ptr(), wide(&self.slots[i].title).as_ptr(), WS_POPUP,
                x, y, skin.w * n, skin.h * n, 0, 0, GetModuleHandleW(null()), null_mut(),
            );
            SetWindowLongPtrW(wnd, GWLP_USERDATA, i as isize);
            self.slots[i].hwnd = wnd;
            self.slots[i].seen.clear();
            self.paint(i, Dirty::All);
            place_in_z_order(wnd, self.position, self.show_desktop);
            ShowWindow(wnd, SW_SHOWNOACTIVATE);
        }
        self.pace();
    }

    /// Take a widget away, window and state, so it costs nothing while shut.
    fn stop(&mut self, i: usize) {
        // The window that holds the keyboard for it is its own, and goes
        // before it does.  What was being typed is the widget's to keep or
        // not as it shuts down: the note saves, the city is let go.
        if self.typing == Some(i) {
            self.typing = None;
        }
        if self.keys.owned_by(self.slots[i].hwnd) {
            self.keys.destroy();
        }
        {
            let (mut ctx, widgets) = self.ctx();
            if let Some(mut w) = widgets.0[i].take() {
                w.shutdown(&mut ctx);
            }
        }
        let wnd = core::mem::take(&mut self.slots[i].hwnd);
        if wnd != 0 {
            unsafe { DestroyWindow(wnd) };
        }
        if self.press.as_ref().is_some_and(|p| p.idx == i) {
            self.press = None;
        }
        self.pace();
    }

    fn set_enabled(&mut self, i: usize, on: bool) {
        // a deleted one has no section to write to, and is not to be given one
        if self.slots[i].deleted {
            return;
        }
        self.settings.set_int(&self.slots[i].section, "Enabled", on as i32);
        if on { self.start(i) } else { self.stop(i) }
        self.settings.save();
    }

    /// Widget `i` is drawn from another skin now -- a photo frame turned to
    /// another shape -- so its window is made again, the new size, where it
    /// was put.  (What it showed goes with the old one; the new one starts
    /// afresh, as it would at the next run.)
    fn reshape(&mut self, i: usize) {
        let skin = self.slots[i].kind.skin_in(&self.settings, &self.slots[i].section);
        if core::ptr::eq(skin, self.slots[i].skin) || self.widgets.get(i).is_none() {
            return;
        }
        self.stop(i);
        self.start(i);
    }

    /// The folder chosen in the dialog a widget asked for, handed to it --
    /// if it is still there: it is found by its section, as others may have
    /// come and gone while the dialog was up.
    fn folder_chosen(&mut self, section: &str, folder: PathBuf) {
        let Some(i) = self.slots.iter().position(|s| !s.deleted && s.section == section) else { return };
        let dirty = {
            let (mut ctx, widgets) = self.ctx();
            match widgets.get_mut(i) {
                Some(w) => w.folder_chosen(folder, &mut ctx),
                None => return,
            }
        };
        self.paint(i, dirty);
        self.settings.save();
    }

    /// Every widget settings.ini does not say is closed.
    fn start_enabled(&mut self) {
        for i in 0..self.slots.len() {
            if self.settings.flag(&self.slots[i].section, "Enabled", true) {
                self.start(i);
            }
        }
    }

    /// Where a window sits now is where it has been put: its place from now
    /// on, written down for the next run.
    fn keep_place(&mut self, i: usize) {
        let (x, y) = (self.slots[i].x, self.slots[i].y);
        self.put(i, x, y);
    }

    /// Window `i` is put at (x, y): its place from now on, written down for
    /// the next run.  It is shown there when it is next laid out.
    fn put(&mut self, i: usize, x: i32, y: i32) {
        let slot = &mut self.slots[i];
        slot.put = (x, y);
        if !slot.deleted {
            self.settings.set_int(&slot.section, "X", x);
            self.settings.set_int(&slot.section, "Y", y);
        }
    }

    /// Show every running widget where it was put -- or, where the monitors
    /// there are now would lose it, the least distance from there that they
    /// show it whole, together with any it was put against (see
    /// `screens::settle`).  Where they were put is left as it was.
    fn lay_out(&mut self) {
        self.lay_out_on(&screens::work_areas());
    }

    fn lay_out_on(&mut self, work_areas: &[RECT]) {
        let Spacing { margin, gap } = self.spacing;
        let running = self.running();
        let wanted: Vec<RECT> = running.iter().map(|&i| {
            let slot = &self.slots[i];
            reach(slot.skin, slot.put.0, slot.put.1, margin, settled_scale(slot.skin, slot.put.0, slot.put.1, slot.n))
        }).collect();
        // Frames a gap apart are kept together.  With a margin round each,
        // that is two margins nearer.
        let most = running.iter().map(|&i| self.slots[i].n).max().unwrap_or(1);
        let near = (gap - 2 * margin).max(0) * most;
        for (&i, (dx, dy)) in running.iter().zip(screens::settle(&wanted, work_areas, near)) {
            let slot = &mut self.slots[i];
            let (mut x, mut y) = (slot.put.0 + dx, slot.put.1 + dy);
            // Where it goes may be a monitor of another scale, or the scale of
            // its own may have changed: drawn at that one, and seen whole at
            // the size that makes it.
            let n = settled_scale(slot.skin, x, y, slot.n);
            if n != slot.n {
                let (kx, ky) = screens::keep_on(&reach(slot.skin, x, y, margin, n), work_areas);
                (x, y) = (x + kx, y + ky);
            }
            let rescaled = core::mem::replace(&mut slot.n, n) != n;
            if (x, y) != (slot.x, slot.y) {
                (slot.x, slot.y) = (x, y);
                if slot.hwnd != 0 {
                    unsafe { SetWindowPos(slot.hwnd, 0, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
                }
            }
            if rescaled {
                self.redraw(i);
            }
        }
    }

    /// Draw window `i` afresh, all of it: at another size, or after Windows
    /// has lost what it showed.
    fn redraw(&mut self, i: usize) {
        self.slots[i].seen.clear();
        self.paint(i, Dirty::All);
    }

    /// The windows whose widgets are running.
    fn running(&self) -> Vec<usize> {
        (0..self.slots.len()).filter(|&i| self.widgets.get(i).is_some()).collect()
    }

    // ---- the colour ------------------------------------------------------------

    /// Draw every widget in `rgb` (0xRRGGBB) from now on, and write it down
    /// for the next run.  The widgets running are given it where they stand:
    /// each reads its colours again and is drawn afresh, and nothing else of
    /// it changes.
    fn set_tint(&mut self, rgb: u32) {
        let rgb = rgb & 0xFF_FFFF;
        self.settings.set_tint(rgb);
        if rgb == tint::current() {
            return;
        }
        tint::set(rgb);
        for i in self.running() {
            if let Some(w) = self.widgets.get_mut(i) {
                w.retint();
            }
            self.redraw(i);
        }
    }

    // ---- the grid --------------------------------------------------------------

    /// The margin and the gap from now on, written down for the next run,
    /// and the widgets moved onto them.
    fn set_spacing(&mut self, spacing: Spacing) {
        let old = core::mem::replace(&mut self.spacing, spacing);
        self.settings.set_int("General", "ScreenMargin", spacing.margin);
        self.settings.set_int("General", "WidgetGap", spacing.gap);
        if spacing != old {
            self.respace(old);
        }
    }

    /// Every running widget that stood on the grid of `old` -- against the
    /// edge of its screen, beside another or in line with one -- put in its
    /// place on the grid there is now, and shown there (see
    /// `grid::respace`).  A widget that is closed is left where it was put,
    /// and comes back there.
    fn respace(&mut self, old: Spacing) {
        let running = self.running();
        let work_areas = screens::work_areas();
        // The grid is as much larger as the widgets on it: those drawn at one
        // scale are respaced together, apart from those drawn at another.
        let mut scales: Vec<i32> = running.iter().map(|&i| self.slots[i].n).collect();
        scales.sort_unstable();
        scales.dedup();
        for n in scales {
            let these: Vec<usize> = running.iter().copied().filter(|&i| self.slots[i].n == n).collect();
            let frames: Vec<RECT> = these.iter().map(|&i| on_screens(frame(self.slots[i].skin, self.slots[i].put.0, self.slots[i].put.1, n))).collect();
            let moves = grid::respace(&frames, &work_areas, old, self.spacing, n);
            for (&i, (dx, dy)) in these.iter().zip(moves) {
                let (x, y) = self.slots[i].put;
                self.put(i, x + dx, y + dy);
            }
        }
        self.lay_out();
    }

    // ---- more notes, more countdowns ------------------------------------------

    /// Somewhere to put a window the size of slot `b`'s, next to it: the
    /// nearest place to its left, its right, below or above it, a gap from
    /// it, where the new frame is on the same screen, inside the margin, and
    /// no nearer than a gap to any widget that is showing.  (On the desktop
    /// a new window goes underneath the others: one that overlapped would be
    /// made and not seen.)
    fn room_beside(&self, b: usize, skin: &'static Skin) -> (i32, i32) {
        let base = &self.slots[b];
        let n = base.n;
        let Spacing { margin, gap } = self.spacing.times(n);
        let own = base.frame();
        let work = work_area(&on_screens(own));
        let free = |&(x, y): &(i32, i32)| {
            let f = frame(skin, x, y, n);
            let room = Rect::new(f.x0 - gap, f.y0 - gap, f.x1 + gap, f.y1 + gap);
            f.x0 >= work.left + margin && f.y0 >= work.top + margin && f.x1 <= work.right - margin && f.y1 <= work.bottom - margin
                && !self.slots.iter().any(|s| s.hwnd != 0 && s.frame().intersects(&room))
        };
        // The new window's frame `k` places along from the old one's, a gap
        // between each: level with its top beside it, level with its left
        // above and below.  (The new one may be another size: a photo frame
        // of another shape.)
        let pad = skin.int("Pad", 12) * n;
        let (w, h) = (skin.w * n - 2 * pad, skin.h * n - 2 * pad);
        let along = |k: i32, from: i32, to: i32, size: i32| match k {
            0 => from,
            k if k < 0 => from + k * (size + gap),
            k => to + gap + (k - 1) * (size + gap),
        };
        (1..=ROOM_STEPS)
            .flat_map(|k| [(-k, 0), (k, 0), (0, k), (0, -k)])
            .map(|(i, j)| (along(i, own.x0, own.x1, w) - pad, along(j, own.y0, own.y1, h) - pad))
            .find(free)
            .unwrap_or_else(|| on_screen(skin, base.x + CASCADE * n, base.y + CASCADE * n, self.spacing.margin, n))
    }

    /// The windows of a family there are: the first, then the others in the
    /// order of their numbers -- which, their names being alike but for the
    /// number, is shortest name first.
    fn members(&self, f: &Family) -> Vec<usize> {
        let mut more: Vec<usize> =
            (FIXED.len()..self.slots.len()).filter(|&j| self.slots[j].kind == f.kind && !self.slots[j].deleted).collect();
        more.sort_by_key(|&j| (self.slots[j].title.len(), &self.slots[j].title));
        core::iter::once(f.first).chain(more).collect()
    }

    /// The one of a family "one fewer" takes off the screen: the one it was
    /// asked for on, or, asked for anywhere else, the last that is showing.
    fn to_close(&self, f: &Family, from: Option<usize>) -> Option<usize> {
        let open = |&j: &usize| self.slots[j].kind == f.kind && self.widgets.get(j).is_some();
        from.filter(open).or_else(|| self.members(f).into_iter().rev().find(open))
    }

    /// One more of a family on the screen.  One that was closed comes back
    /// first, where it was and with what was on it, the lowest number before
    /// the others -- there is no other way back to it -- and with none closed
    /// a new one is made.
    fn more(&mut self, f: &Family, from: Option<usize>) {
        let Some(closed) = self.members(f).into_iter().find(|&j| self.widgets.get(j).is_none()) else {
            return self.new_one(f, from);
        };
        // Something may have been put where it was since: it would come back
        // underneath that, and not be seen.  Then it goes where a new one would.
        let ((x, y), skin) = (self.slots[closed].put, self.slots[closed].skin);
        let own = frame(skin, x, y, settled_scale(skin, x, y, self.slots[closed].n));
        if self.slots.iter().any(|s| s.hwnd != 0 && s.frame().intersects(&own)) {
            let beside = self.to_close(f, from).unwrap_or(closed);
            (self.slots[closed].x, self.slots[closed].y) = self.room_beside(beside, skin);
            self.slots[closed].n = self.slots[beside].n;
            self.keep_place(closed);
        }
        self.set_enabled(closed, true);
    }

    /// One fewer of a family on the screen: closed, as its X closes it,
    /// with what is on it kept.
    fn fewer(&mut self, f: &Family, from: Option<usize>) {
        if let Some(i) = self.to_close(f, from) {
            self.set_enabled(i, false);
        }
    }

    /// Another of a family: a window beside the one it was asked for on --
    /// or, asked for anywhere else, beside the last there is -- with a
    /// section of settings.ini of its own (and, for a note, an empty file).
    fn new_one(&mut self, f: &Family, from: Option<usize>) {
        let is_member = |s: &Slot| s.kind == f.kind && !s.deleted;
        let beside = from.filter(|&i| is_member(&self.slots[i])).or_else(|| self.slots.iter().rposition(is_member)).unwrap_or(f.first);
        let section = (f.create)(&mut self.settings);
        let title = (f.name)(&section);
        let slot = Slot::new(f.kind, &section, title, &self.settings);
        let (x, y) = self.room_beside(beside, slot.skin);
        self.slots.push(Slot { x, y, n: self.slots[beside].n, ..slot });
        self.widgets.0.push(None);
        let i = self.slots.len() - 1;
        self.keep_place(i);
        self.set_enabled(i, true);
    }

    /// Delete a note, a countdown or a photo frame after the first: its
    /// window, and its section of settings.ini.  What it leaves on disk is
    /// handed back, to be disposed of once the App has been let go of.
    fn delete(&mut self, section: &str) -> Vec<PathBuf> {
        let found = self.slots.iter().position(|s| !s.deleted && s.section == section);
        let Some((i, f)) = found.filter(|&i| i >= FIXED.len()).and_then(|i| Some((i, self.slots[i].kind.family()?))) else {
            return Vec::new();
        };
        self.stop(i);
        self.slots[i].deleted = true;
        let files = (f.forget)(&mut self.settings, section);
        self.settings.save();
        files
    }

    /// The menu's "Delete this ...", on window `i`: asked about first, unless
    /// there is nothing on it to lose.  Only one after the first is deleted.
    fn ask_delete(&self, f: &Family, i: Option<usize>) -> Later {
        let Some(slot) = i.filter(|&i| i >= FIXED.len() && self.slots[i].kind == f.kind).map(|i| &self.slots[i]) else {
            return Later::Nothing;
        };
        let ask = (f.ask)(&self.settings, &slot.section).map(|text| (format!("Delete {}", slot.title), text));
        Later::Delete(slot.section.clone(), ask)
    }

    /// Redraw what changed and hand it to the window manager.
    fn paint(&mut self, i: usize, dirty: Dirty) {
        let wnd = self.slots[i].hwnd;
        let Some(widget) = self.widgets.get(i) else { return };
        if wnd == 0 || self.locked {
            return;
        }
        let skin = widget.skin();
        let whole = Rect::new(0, 0, skin.w, skin.h);
        let rect = match dirty {
            Dirty::None => return,
            Dirty::Part(r) => r.intersect(&whole),
            Dirty::All => whole,
        };
        if rect.is_empty() {
            return;
        }
        // Drawn at its own size...
        let (stride, rows) = largest_skin();
        // SAFETY: the picture is `stride` x `rows`, at least as large as the largest skin.
        let mut canvas = unsafe { Canvas::new(self.picture.as_mut_ptr(), stride, skin.w, skin.h) };
        canvas.set_clip(rect);
        canvas.clear(rect);
        skin::draw(skin, widget as &dyn Hooks, &mut canvas, &mut self.text, &mut self.slots[i].seen);
        drop(canvas);

        // ...and put up n times that, in the buffer.
        let n = self.slots[i].n;
        if self.buffer.w < stride * n || self.buffer.h < rows * n {
            self.buffer = Buffer::new(stride * n, rows * n);
        }
        let wide = self.buffer.w as usize;
        let pixels = self.buffer.pixels();
        if pixels.is_empty() {
            return; // no bitmap to be had: nothing can be shown
        }
        canvas::scale_into(&self.picture, stride as usize, rect, pixels, wide, n);

        let size = SIZE { cx: skin.w * n, cy: skin.h * n };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA };
        let area = RECT { left: rect.x0 * n, top: rect.y0 * n, right: rect.x1 * n, bottom: rect.y1 * n };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: 0,
            pptDst: null(),
            psize: &size,
            hdcSrc: self.buffer.dc,
            pptSrc: &origin,
            crKey: 0,
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: &area,
        };
        unsafe { UpdateLayeredWindowIndirect(wnd, &info) };
    }

    /// Act on what a command asked for, and say what is left to do later.
    fn apply(&mut self, i: usize, outcome: Outcome) -> Later {
        self.paint(i, outcome.dirty);
        // the command may have been the one that pauses a widget, or wakes it
        self.pace();
        let later = match outcome.request {
            Request::None => Later::Nothing,
            Request::About => Later::About(i),
            Request::Open(path) => Later::Open(path),
            Request::Close => {
                self.set_enabled(i, false);
                Later::Nothing
            }
            Request::TakeKeys => {
                self.open_editor(i);
                Later::Nothing
            }
            Request::DropKeys => {
                self.leave_editor();
                Later::Nothing
            }
            Request::Reshape => {
                self.reshape(i);
                Later::Nothing
            }
            Request::ChooseFolder(start) => Later::ChooseFolder(self.slots[i].section.clone(), start),
        };
        self.settings.save();
        later
    }

    fn command(&mut self, i: usize, act: &Act, x: i32, y: i32) -> Later {
        let outcome = {
            let (mut ctx, widgets) = self.ctx();
            match widgets.get_mut(i) {
                Some(w) => w.command(act.cmd, act.arg, x, y, &mut ctx),
                None => return Later::Nothing,
            }
        };
        self.apply(i, outcome)
    }

    /// The meter action for `ev` under the point, if there is one.
    fn fire(&mut self, i: usize, ev: Ev, x: i32, y: i32) -> Option<Later> {
        let widget = self.widgets.get(i)?;
        let (act, mx, my) = skin::hit(widget.skin(), widget as &dyn Hooks, x, y, ev)?;
        Some(self.command(i, act, mx, my))
    }

    // ---- the keyboard ----------------------------------------------------------

    /// Widget `i` is being typed on -- the note, or a city field of the
    /// weather's or the world clock's -- give it the keyboard.  There is one keyboard: whichever had it is
    /// done.
    fn open_editor(&mut self, i: usize) {
        if self.typing.is_some_and(|other| other != i) {
            self.leave_editor();
        }
        self.typing = Some(i);
        let wnd = self.slots[i].hwnd;
        if wnd == 0 || !self.keys.open(wnd, keys_proc) {
            // no keyboard to give it: the widget is as it was
            self.leave_editor();
        }
    }

    /// The typing is over, however it came to be: the keyboard goes back, and
    /// the widget that had it does what it does with what was typed -- the
    /// note writes itself out, the weather looks the city up.
    fn leave_editor(&mut self) {
        if self.keys.close() {
            self.trim_at = self.ticks + 5;
        }
        let Some(i) = self.typing.take() else { return };
        let dirty = {
            let (mut ctx, widgets) = self.ctx();
            widgets.get_mut(i).map_or(Dirty::None, |w| w.blur(&mut ctx))
        };
        self.paint(i, dirty);
        self.settings.save();
    }

    /// A key or a character for the widget that is being typed on.  The
    /// clipboard's three are done here, where the clipboard is.
    fn key_input(&mut self, input: Input) {
        let owner = self.keys.window();
        let Some(i) = self.typing else { return };
        let Some(widget) = self.widgets.get_mut(i) else { return };
        let outcome = match input {
            Input::Char(c) => widget.typed(c),
            Input::Key { key: Key::Copy, .. } => {
                if let Some(text) = widget.copy() {
                    keys::set_clipboard(owner, &text);
                }
                Outcome::none()
            }
            Input::Key { key: Key::Cut, .. } => {
                let (text, outcome) = widget.cut();
                if let Some(text) = text {
                    keys::set_clipboard(owner, &text);
                }
                outcome
            }
            Input::Key { key: Key::Paste, .. } => match keys::clipboard(owner) {
                Some(text) => widget.paste(&text),
                None => Outcome::none(),
            },
            Input::Key { key, shift, ctrl } => widget.key(key, shift, ctrl),
        };
        self.apply(i, outcome);
    }

    /// The keyboard's timer: the caret blinks, and typing on the note that
    /// has paused is saved.
    fn key_pulse(&mut self) {
        let Some(i) = self.typing else { return };
        let dirty = self.widgets.get_mut(i).map_or(Dirty::None, |w| w.pulse(Keys::pulse_ms(), Keys::blinks()));
        self.paint(i, dirty);
    }

    // ---- once a second ---------------------------------------------------------

    fn tick(&mut self) {
        self.ticks += 1;
        for i in 0..self.slots.len() {
            let dirty = {
                let (mut ctx, widgets) = self.ctx();
                match widgets.get_mut(i) {
                    Some(w) => w.tick(&mut ctx),
                    None => continue,
                }
            };
            self.paint(i, dirty);
        }
        self.settings.save();
        self.check_desktop();
        // Starting up, a menu, typing: each touches a good deal that is
        // not needed again.  Hand those pages back once things have settled.
        if self.ticks == self.trim_at {
            unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
        }
    }

    // ---- in between: frames ------------------------------------------------------

    fn now_ms() -> i64 {
        let (seconds, ms) = clock::now_ms();
        seconds * 1000 + ms as i64
    }

    /// Set the frame timer to the pace the widgets ask for: the quickest of
    /// them, or no timer at all when none is moving, or the workstation is
    /// locked and nothing would be drawn anyway.  Called whenever either may
    /// have changed -- a widget started or stopped, a command, the lock.
    fn pace(&mut self) {
        let asked = self.widgets.0.iter().flatten().map(|w| w.frame_ms()).filter(|&ms| ms > 0).min();
        let want = if self.locked { 0 } else { asked.unwrap_or(0) };
        if want == self.frame_ms {
            return;
        }
        self.frame_ms = want;
        let main = MAIN.load(Relaxed);
        unsafe {
            if want > 0 {
                self.frame_at = Self::now_ms();
                SetTimer(main, FRAME_TIMER, want, null());
            } else {
                KillTimer(main, FRAME_TIMER);
            }
        }
    }

    /// One frame for each widget that wants them, told how long it has been
    /// since the last: timers are not punctual, and the rain should fall at
    /// the same speed whether or not this one is.
    fn animate(&mut self) {
        let now = Self::now_ms();
        let elapsed = (now - self.frame_at).clamp(0, 1000) as u32;
        self.frame_at = now;
        for i in 0..self.slots.len() {
            let dirty = match self.widgets.get_mut(i) {
                Some(w) if w.frame_ms() > 0 => w.frame(elapsed),
                _ => continue,
            };
            self.paint(i, dirty);
        }
        // a widget whose movement has come to an end -- a button back up --
        // asks for no more
        self.pace();
    }

    fn repaint_all(&mut self) {
        for i in 0..self.slots.len() {
            self.paint(i, Dirty::All);
        }
    }

    fn check_desktop(&mut self) {
        let shown = self.position == Position::Desktop && desktop_is_shown();
        if shown != self.show_desktop {
            self.show_desktop = shown;
            self.restack();
        }
    }

    fn restack(&mut self) {
        for slot in &self.slots {
            if slot.hwnd != 0 {
                place_in_z_order(slot.hwnd, self.position, self.show_desktop);
            }
        }
    }

    fn set_position(&mut self, position: Position) {
        self.position = position;
        POSITION.store(position as u8, Relaxed);
        self.show_desktop = false;
        let name = match position {
            Position::Desktop => "desktop",
            Position::Normal => "normal",
            Position::Topmost => "topmost",
        };
        self.settings.set("General", "Position", name);
        self.settings.save();
        self.restack();
        self.check_desktop();
    }

    // ---- the mouse ---------------------------------------------------------------

    /// A point of window `i`, in the screen's pixels, as the widget has it:
    /// in its own.  (Held by the mouse, a window is told of points left of
    /// it and above it too, so these are rounded down, not towards 0.)
    fn widget_point(&self, i: usize, x: i32, y: i32) -> (i32, i32) {
        let n = self.slots[i].n.max(1);
        (x.div_euclid(n), y.div_euclid(n))
    }

    fn mouse_down(&mut self, i: usize, wnd: HWND, x: i32, y: i32, double: bool) -> Later {
        let (x, y) = self.widget_point(i, x, y);
        // Clicking anywhere but on what is being typed on -- the note's sheet,
        // the weather's city field -- finishes the typing.
        if !(self.typing == Some(i) && self.widgets.get(i).is_some_and(|w| w.typing_at(x, y))) {
            self.leave_editor();
        }
        let mut later = Later::Nothing;
        // The widget may want the press for itself: text to put a caret in.
        let shift = unsafe { GetKeyState(VK_SHIFT) } < 0;
        let owned = match self.widgets.get_mut(i).map(|w| w.press(x, y, double, shift)) {
            Some(Pressed::Taken(outcome)) => {
                later = self.apply(i, outcome);
                true
            }
            _ => false,
        };
        // A double-click does what the meter under it says a double-click
        // does; where it says nothing, it is simply a second click.
        let spent = !owned && double && self.fire(i, Ev::Dbl, x, y).map(|l| later = l).is_some();
        let (mut cursor, mut frame) = (POINT::default(), RECT::default());
        unsafe {
            GetCursorPos(&mut cursor);
            GetWindowRect(wnd, &mut frame);
            SetCapture(wnd);
        }
        self.press = Some(Press { idx: i, cursor, window: POINT { x: frame.left, y: frame.top }, dragging: false, owned, spent });
        if self.position == Position::Normal {
            unsafe { SetWindowPos(wnd, HWND_TOP, 0, 0, 0, 0, ZPOS_FLAGS) };
        }
        later
    }

    fn mouse_move(&mut self, i: usize, wnd: HWND, x: i32, y: i32, button_down: bool) -> Later {
        let (x, y) = self.widget_point(i, x, y);
        // A press whose release went somewhere else is over -- and the widget
        // that took it is told so, as a release over nothing.
        if !button_down {
            if let Some(press) = self.press.take().filter(|p| p.owned) {
                self.release(press.idx, i32::MIN, i32::MIN);
            }
        }
        if self.press.as_ref().is_some_and(|p| p.idx == i && p.owned) {
            let dirty = self.widgets.get_mut(i).map_or(Dirty::None, |w| w.drag(x, y));
            self.paint(i, dirty);
            // a button let go of, or taken hold of again, moves
            self.pace();
            return Later::Nothing;
        }
        if let Some(press) = self.press.as_mut().filter(|p| p.idx == i) {
            let mut cursor = POINT::default();
            unsafe { GetCursorPos(&mut cursor) };
            let (dx, dy) = (cursor.x - press.cursor.x, cursor.y - press.cursor.y);
            if !press.dragging && !self.locked_in_place {
                let (limit_x, limit_y) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
                press.dragging = dx.abs() > limit_x || dy.abs() > limit_y;
            }
            if press.dragging {
                // Over a monitor of another scale, the window is drawn at that
                // one, and the spot of it that was taken hold of stays under
                // the pointer.  (The pointer's monitor, not the window's: the
                // window's would change as the window did.)
                let (was, n) = (self.slots[i].n, screens::scale_at(cursor.x, cursor.y));
                if n != was {
                    let held = (press.cursor.x - press.window.x, press.cursor.y - press.window.y);
                    press.window = POINT { x: press.cursor.x - held.0 * n / was, y: press.cursor.y - held.1 * n / was };
                }
                let (px, py) = (press.window.x + dx, press.window.y + dy);
                self.slots[i].n = n;
                let (nx, ny) = self.snapped(i, px, py);
                self.slots[i].x = nx;
                self.slots[i].y = ny;
                unsafe { SetWindowPos(wnd, 0, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
                if n != was {
                    self.redraw(i);
                }
            }
            return Later::Nothing;
        }

        // Nothing held: which meter is the pointer over?
        if !self.slots[i].tracking {
            let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: wnd, dwHoverTime: 0 };
            self.slots[i].tracking = unsafe { TrackMouseEvent(&mut tme) } != 0;
        }
        let over = self.widgets.get(i).and_then(|w| skin::hover(w.skin(), w as &dyn Hooks, x, y));
        self.set_hover(i, over);
        Later::Nothing
    }

    /// Tell the meter the pointer left and the one it arrived on, always in
    /// that order.
    fn set_hover(&mut self, i: usize, over: Option<usize>) {
        let was = self.slots[i].hover;
        if was == over {
            return;
        }
        self.slots[i].hover = over;
        let meters = self.slots[i].skin.meters;
        for (meter, ev) in [(was, Ev::Leave), (over, Ev::Over)] {
            if let Some(act) = meter.and_then(|m| meters[m].acts.iter().find(|a| a.ev == ev)) {
                self.command(i, act, 0, 0);
            }
        }
    }

    fn mouse_up(&mut self, i: usize, x: i32, y: i32) -> Later {
        let (x, y) = self.widget_point(i, x, y);
        let press = self.press.take();
        unsafe { ReleaseCapture() };
        let Some(press) = press.filter(|p| p.idx == i) else { return Later::Nothing };
        if press.dragging {
            self.keep_place(i);
            self.settings.save();
            return Later::Nothing;
        }
        if press.owned {
            return self.release(i, x, y);
        }
        if press.spent {
            return Later::Nothing;
        }
        self.fire(i, Ev::Up, x, y).unwrap_or(Later::Nothing)
    }

    /// The end of a press widget `i` took, at (x, y) of it: a button let go
    /// of over itself does what it is for.
    fn release(&mut self, i: usize, x: i32, y: i32) -> Later {
        let outcome = {
            let (mut ctx, widgets) = self.ctx();
            match widgets.get_mut(i) {
                Some(w) => w.release(x, y, &mut ctx),
                None => return Later::Nothing,
            }
        };
        self.apply(i, outcome)
    }

    /// A wheel turned over window `i`: a skin's wheel action for every whole
    /// notch.  Each way a wheel turns keeps its own part of a notch, so a
    /// touchpad swiped on the slant does not have the two cancel out.
    fn mouse_wheel(&mut self, i: usize, wnd: HWND, screen_x: i32, screen_y: i32, axis: Axis, delta: i32) -> Later {
        let mut p = POINT { x: screen_x, y: screen_y };
        unsafe { ScreenToClient(wnd, &mut p) };
        let (x, y) = self.widget_point(i, p.x, p.y);
        let rest = match axis {
            Axis::Upright => &mut self.wheel,
            Axis::Sideways => &mut self.sideways,
        };
        let (ev, count) = notches(rest, axis, delta);
        let mut later = Later::Nothing;
        for _ in 0..count {
            if let Some(l) = self.fire(i, ev, x, y) {
                later = l;
            }
        }
        later
    }

    /// Where a dragged window lands: pulled onto the grid -- the margin
    /// inside the screen's edge, the gap beside another widget, in line with
    /// one -- when its frame comes within a snap of it, unless Ctrl or Shift
    /// is held.
    fn snapped(&self, i: usize, mut x: i32, mut y: i32) -> (i32, i32) {
        let (skin, n) = (self.slots[i].skin, self.slots[i].n);
        let held = |key: i32| unsafe { GetKeyState(key) } < 0;
        if self.snap && !held(VK_CONTROL) && !held(VK_SHIFT) {
            let f = on_screens(frame(skin, x, y, n));
            let others: Vec<RECT> = (self.slots.iter().enumerate())
                .filter(|&(j, other)| j != i && other.hwnd != 0)
                .map(|(_, other)| on_screens(other.frame()))
                .collect();
            let (dx, dy) = grid::snap(&f, &others, &work_area(&f), self.spacing, n);
            (x, y) = (x + dx, y + dy);
        }
        // Not dropped where no monitor shows it, nor half off one.
        on_screen(skin, x, y, self.spacing.margin, n)
    }

    /// Is (x, y) on the screen in the margin round window `i`'s frame, and
    /// over another widget's frame?  A press there is for that one.
    fn over_another(&self, i: usize, x: i32, y: i32) -> bool {
        !self.slots[i].frame().contains(x, y)
            && (self.slots.iter().enumerate()).any(|(j, other)| j != i && other.hwnd != 0 && other.frame().contains(x, y))
    }

    // ---- menus ---------------------------------------------------------------------

    /// The right-click menu, as (id, label, checked) with 0 for a separator
    /// and usize::MAX opening and closing a submenu.
    fn menu(&self, i: Option<usize>) -> Vec<(usize, String, bool)> {
        const SUB: usize = usize::MAX;
        let mut m = Vec::new();
        if let Some(i) = i {
            let mut own = Vec::new();
            if let Some(w) = self.widgets.get(i) {
                w.menu(&mut own);
            }
            for item in &own {
                // id 0 is a line between its entries
                let id = if item.id == 0 { 0 } else { CMD_WIDGET + item.id as usize };
                m.push((id, item.label.to_owned(), item.checked));
            }
            if !own.is_empty() {
                m.push((0, String::new(), false));
            }
            // The first note or countdown is closed, like the widgets there
            // is one of; the ones after it can also be deleted.
            if let Some(f) = self.slots[i].kind.family().filter(|_| i >= FIXED.len()) {
                m.push((f.delete, String::from(f.delete_label), false));
                m.push((0, String::new(), false));
            }
        }
        m.push((SUB, String::from("Position"), false));
        m.push((CMD_DESKTOP, String::from("On desktop"), self.position == Position::Desktop));
        m.push((CMD_NORMAL, String::from("Normal"), self.position == Position::Normal));
        m.push((CMD_TOPMOST, String::from("Always on top"), self.position == Position::Topmost));
        m.push((SUB, String::new(), false));
        m.push((CMD_SNAP, String::from("Snap to edges"), self.snap));
        for (title, base, now) in [("Screen margin", CMD_MARGIN, self.spacing.margin), ("Gap between widgets", CMD_GAP, self.spacing.gap)] {
            m.push((SUB, String::from(title), false));
            for (k, px) in spacings(now).into_iter().enumerate() {
                m.push((base + k, format!("{px} px"), px == now));
            }
            m.push((SUB, String::new(), false));
        }
        // The colours by name, each with a swatch of what it comes to (see
        // show_menu), and any other from the colour dialog.
        let now = tint::current();
        m.push((SUB, String::from("Colour"), false));
        for (k, (name, rgb)) in tint::PRESETS.iter().enumerate() {
            m.push((CMD_TINT + k, String::from(*name), *rgb == now));
        }
        m.push((0, String::new(), false));
        m.push((CMD_TINT_CUSTOM, String::from("Custom..."), tint::preset_name(now).is_none()));
        m.push((SUB, String::new(), false));
        m.push((CMD_LOCK, String::from("Lock position"), self.locked_in_place));
        if let Some(i) = i {
            m.push((CMD_CLOSE, format!("Close {}", self.slots[i].title), false));
        }
        m.push((0, String::new(), false));
        m.push((SUB, String::from("Widgets"), false));
        for j in 0..FIXED.len() {
            let Some(f) = FAMILIES.iter().find(|f| f.first == j) else {
                m.push((CMD_TOGGLE + j, self.slots[j].title.clone(), self.slots[j].hwnd != 0));
                continue;
            };
            // There is one of each of the others, shown or not.  Of notes
            // and of countdowns there is a number: one more, and -- while any
            // is showing -- one fewer.
            m.push((f.more, format!("{} +", FIXED[j].2), false));
            if self.to_close(f, i).is_some() {
                m.push((f.fewer, format!("{} \u{2212}", FIXED[j].2), false));
            }
        }
        m.push((SUB, String::new(), false));
        m.push((CMD_AUTOSTART, String::from("Start with Windows"), autostart_enabled()));
        m.push((CMD_SETTINGS, String::from("Open settings file"), false));
        m.push((CMD_RELOAD, String::from("Reload settings"), false));
        m.push((0, String::new(), false));
        m.push((CMD_EXIT, String::from("Exit"), false));
        m
    }

    fn menu_command(&mut self, i: Option<usize>, cmd: usize) -> Later {
        self.trim_at = self.ticks + 5;
        if let Some(f) = FAMILIES.iter().find(|f| [f.more, f.fewer, f.delete].contains(&cmd)) {
            match cmd {
                c if c == f.delete => return self.ask_delete(f, i),
                c if c == f.more => self.more(f, i),
                _ => self.fewer(f, i),
            }
            self.settings.save();
            return Later::Nothing;
        }
        match cmd {
            CMD_DESKTOP => self.set_position(Position::Desktop),
            CMD_NORMAL => self.set_position(Position::Normal),
            CMD_TOPMOST => self.set_position(Position::Topmost),
            CMD_SNAP => {
                self.snap = !self.snap;
                self.settings.set_int("General", "SnapEdges", self.snap as i32);
            }
            CMD_LOCK => {
                self.locked_in_place = !self.locked_in_place;
                self.settings.set_int("General", "LockPosition", self.locked_in_place as i32);
            }
            c if (CMD_MARGIN..CMD_GAP).contains(&c) => {
                if let Some(&margin) = spacings(self.spacing.margin).get(c - CMD_MARGIN) {
                    self.set_spacing(Spacing { margin, ..self.spacing });
                }
            }
            c if (CMD_GAP..CMD_WIDGET).contains(&c) => {
                if let Some(&gap) = spacings(self.spacing.gap).get(c - CMD_GAP) {
                    self.set_spacing(Spacing { gap, ..self.spacing });
                }
            }
            CMD_TINT_CUSTOM => return Later::PickColour(tint::current()),
            c if (CMD_TINT..CMD_TINT_CUSTOM).contains(&c) => {
                if let Some(&(_, rgb)) = tint::PRESETS.get(c - CMD_TINT) {
                    self.set_tint(rgb);
                }
            }
            CMD_CLOSE => {
                if let Some(i) = i {
                    self.set_enabled(i, false);
                }
            }
            CMD_AUTOSTART => set_autostart(!autostart_enabled()),
            CMD_SETTINGS => {
                self.settings.save();
                return Later::Open(self.settings.path().to_owned());
            }
            CMD_RELOAD => self.reload(),
            CMD_EXIT => return Later::Quit,
            c if c >= CMD_TOGGLE => {
                let j = c - CMD_TOGGLE;
                if j < self.slots.len() {
                    self.set_enabled(j, self.slots[j].hwnd == 0);
                }
            }
            c if (CMD_WIDGET..CMD_TINT).contains(&c) => {
                let Some(i) = i else { return Later::Nothing };
                let outcome = {
                    let (mut ctx, widgets) = self.ctx();
                    match widgets.get_mut(i) {
                        Some(w) => w.menu_command((c - CMD_WIDGET) as u32, &mut ctx),
                        None => return Later::Nothing,
                    }
                };
                return self.apply(i, outcome);
            }
            _ => {}
        }
        self.settings.save();
        Later::Nothing
    }

    /// Read settings.ini again and start over from it.
    fn reload(&mut self) {
        for i in 0..self.slots.len() {
            self.stop(i);
        }
        let old = self.spacing;
        self.settings = Settings::load();
        self.read_general();
        self.read_slots();
        self.start_enabled();
        // a margin or a gap written in by hand is as good as one chosen
        if self.spacing != old {
            self.respace(old);
        }
        self.settings.save();
    }

    fn read_general(&mut self) {
        self.position = match self.settings.text("General", "Position", "desktop").to_ascii_lowercase().as_str() {
            "normal" => Position::Normal,
            "topmost" => Position::Topmost,
            _ => Position::Desktop,
        };
        POSITION.store(self.position as u8, Relaxed);
        self.snap = self.settings.flag("General", "SnapEdges", true);
        let margin = self.settings.int("General", "ScreenMargin", SPACING.margin).clamp(0, MOST_SPACE);
        let gap = self.settings.int("General", "WidgetGap", SPACING.gap).clamp(0, MOST_SPACE);
        self.spacing = Spacing { margin, gap };
        self.locked_in_place = self.settings.flag("General", "LockPosition", false);
        // read before any widget is made, so that each is made in it
        tint::set(self.settings.tint());
    }

    /// Which windows there are -- the eleven, and a note, a countdown or a
    /// photo frame for every other section of settings.ini that is one's --
    /// and where each sits: where it was left, or -- the first time -- on the
    /// grid, in a column down the right-hand side of the screen: the date and
    /// time in the top corner, the resource meter under it, and under that
    /// the weather, the note to its left, the rain to the left of the note,
    /// the disk to the left of the rain, the battery to the left of the disk
    /// and the Claude usage to the left of the battery; the world clock heads
    /// a second column, to the left of the date and time, and the countdown
    /// stands under it, against the first column; the photo frame heads a
    /// third, to the left of the world clock.
    /// Called with every window shut: at the start, and when settings.ini is
    /// read again.
    fn read_slots(&mut self) {
        self.slots = FIXED.iter().map(|&(kind, section, title)| Slot::new(kind, section, title.to_owned(), &self.settings)).collect();
        for f in &FAMILIES {
            for section in (f.others)(&self.settings) {
                let title = (f.name)(&section);
                self.slots.push(Slot::new(f.kind, &section, title, &self.settings));
            }
        }
        self.widgets.0.clear();
        self.widgets.0.resize_with(self.slots.len(), || None);
        self.convert_old_places();

        // the first layout, on the main monitor, at its scale
        let work = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let n = screens::scale_of(&work);
        let size = |i: usize| (self.slots[i].skin.w * n, self.slots[i].skin.h * n);
        let pad = FIXED[DATETIME].0.skin().int("Pad", 12) * n;
        let Spacing { margin, gap } = self.spacing.times(n);
        // From one window to the next, past the first's size: the two pads
        // of nothing between the frames are taken back and the gap put in.
        let step = gap - 2 * pad;
        let x0 = work.right - margin + pad - size(DATETIME).0;
        let y0 = work.top + margin - pad;
        let y1 = y0 + size(DATETIME).1 + step;
        let y2 = y1 + size(1).1 + step;
        let weather = x0 + size(DATETIME).0 - size(WEATHER).0;
        let note = weather - step - size(NOTE).0;
        let rain = note - step - size(RAIN).0;
        let disk = rain - step - size(DISK).0;
        let battery = disk - step - size(BATTERY).0;
        let world = x0 - step - size(WORLD).0;
        let defaults = [
            (x0, y0),
            (x0, y1),
            (note, y2),
            (weather, y2),
            (rain, y2),
            (disk, y2),
            (world, y0),
            (battery, y2),
            (x0 - step - size(COUNTDOWN).0, y0 + size(WORLD).1 + step),
            (battery - step - size(CLAUDE).0, y2),
            (world - step - size(PHOTO).0, y0),
        ];
        for i in 0..self.slots.len() {
            // A note, a countdown or a photo frame after the first was put
            // somewhere when it was made.  One written into settings.ini by
            // hand, with no place given, steps down from the first of its
            // kind, a step for each of its kind before it.
            let (kind, skin) = (self.slots[i].kind, self.slots[i].skin);
            let (x, y) = defaults.get(i).copied().unwrap_or_else(|| {
                let first = kind.family().map_or(NOTE, |f| f.first);
                let before = (FIXED.len()..=i).filter(|&j| self.slots[j].kind == kind).count();
                let step = CASCADE * n * before as i32;
                (defaults[first].0 + step, defaults[first].1 + step)
            });
            let section = &self.slots[i].section;
            let placed = self.settings.get(section, "X").is_some();
            let (x, y) = (self.settings.int(section, "X", x), self.settings.int(section, "Y", y));
            (self.slots[i].x, self.slots[i].y) = (x, y);
            self.slots[i].put = (x, y);
            // at the scale of the monitor most of it is on at its own size
            self.slots[i].n = settled_scale(skin, x, y, screens::scale_of(&on_screens(frame(skin, x, y, 1))));
            if !placed {
                self.keep_place(i);
            }
        }
    }

    /// A build from before the program was DPI aware wrote the places in
    /// settings.ini in the coordinates Windows made up for it, which are not
    /// the screens' pixels on a monitor that is scaled.  Turn them into those,
    /// once, keeping each widget where it was on its monitor (see
    /// `screens::from_unaware`) -- closed ones too -- and say so in [General].
    fn convert_old_places(&mut self) {
        if !dpi_aware() || self.settings.flag("General", DEVICE_PIXELS, false) {
            return;
        }
        let placed: Vec<(usize, i32, i32)> = (0..self.slots.len())
            .filter_map(|i| {
                let section = &self.slots[i].section;
                self.settings.get(section, "X")?;
                Some((i, self.settings.int(section, "X", 0), self.settings.int(section, "Y", 0)))
            })
            .collect();
        let frames: Vec<RECT> = placed.iter().map(|&(i, x, y)| on_screens(frame(self.slots[i].skin, x, y, 1))).collect();
        // those that stood on the grid together stay together
        let near = self.spacing.gap + grid::SNAP;
        for (&(i, _, _), ((x, y), n)) in placed.iter().zip(screens::from_unaware(&frames, &screens::monitors_both_ways(), near)) {
            let pad = self.slots[i].skin.int("Pad", 12) * n;
            let section = self.slots[i].section.clone();
            self.settings.set_int(&section, "X", x - pad);
            self.settings.set_int(&section, "Y", y - pad);
        }
        self.settings.set_int("General", DEVICE_PIXELS, 1);
    }

    fn shutdown(&mut self) {
        self.typing = None;
        self.keys.destroy();
        let (mut ctx, widgets) = self.ctx();
        for w in widgets.0.iter_mut().flatten() {
            w.shutdown(&mut ctx);
        }
        self.settings.save();
    }

    // ---- the tray icon -----------------------------------------------------------

    /// The gears from skins/Icons, straight out of the .ico: the largest image
    /// in it, at its own size.  The tray scales it to whatever it shows.
    fn make_icon() -> HICON {
        let ico = TRAY_ICO;
        let word = |at: usize| ico.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize);
        let long = |at: usize| ico.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        // ICONDIR: reserved, type (1 = icon), count; then 16 bytes an image:
        // width and height (0 = 256), colours, reserved, planes, bits a pixel,
        // the image's length and where in the file it starts.
        if word(2) != 1 {
            return 0;
        }
        let side = |entry: usize| ico.get(entry).map_or(0, |&w| if w == 0 { 256 } else { w as usize });
        let Some(entry) = (0..word(4)).map(|i| 6 + 16 * i).max_by_key(|&e| (side(e), word(e + 6))) else { return 0 };
        let (len, at) = (long(entry + 8), long(entry + 12));
        let Some(image) = ico.get(at..at.saturating_add(len)) else { return 0 };
        // SAFETY: the image is `len` bytes of the file; Windows copies what it needs.
        unsafe { CreateIconFromResourceEx(image.as_ptr(), len as u32, 1, ICON_VERSION, 0, 0, 0) }
    }

    fn tray(&self, message: u32) {
        let mut data: NOTIFYICONDATAW = unsafe { core::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = MAIN.load(Relaxed);
        data.uID = 1;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.tray;
        for (d, s) in data.szTip.iter_mut().zip(MAIN_TITLE.encode_utf16()) {
            *d = s;
        }
        unsafe { Shell_NotifyIconW(message, &data) };
    }
}

// ---------------------------------------------------------------------------
//  Things done with the App let go of
// ---------------------------------------------------------------------------

fn run_later(mut later: Later) {
    loop {
        later = match later {
            Later::Nothing => return,
            Later::Menu(i) => show_menu(i),
            Later::About(i) => {
                let Some((title, about)) = with_app(|app| Some((app.slots.get(i)?.title.clone(), app.widgets.get(i)?.about()))).flatten() else {
                    return;
                };
                let text = format!("{about}\n\nWin98 Widgets {}", env!("CARGO_PKG_VERSION"));
                unsafe { MessageBoxW(0, wide(&text).as_ptr(), wide(&format!("About {title}")).as_ptr(), MB_OK | MB_ICONINFORMATION) };
                Later::Nothing
            }
            Later::Delete(section, ask) => {
                let sure = ask.is_none_or(|(title, text)| {
                    (unsafe { MessageBoxW(0, wide(&text).as_ptr(), wide(&title).as_ptr(), MB_YESNO | MB_ICONQUESTION) }) == IDYES
                });
                if sure {
                    recycle(&with_app(|app| app.delete(&section)).unwrap_or_default());
                }
                Later::Nothing
            }
            Later::Open(path) => {
                unsafe {
                    ShellExecuteW(0, wide("open").as_ptr(), wide(&path.to_string_lossy()).as_ptr(), null(), null(), SW_SHOWNORMAL);
                }
                Later::Nothing
            }
            Later::PickColour(now) => {
                if let Some(rgb) = pick_colour(now) {
                    with_app(|app| {
                        app.set_tint(rgb);
                        app.settings.save();
                    });
                }
                Later::Nothing
            }
            Later::ChooseFolder(section, start) => {
                if let Some(folder) = choose_folder(&start) {
                    with_app(|app| app.folder_chosen(&section, folder));
                }
                Later::Nothing
            }
            Later::Quit => {
                unsafe { DestroyWindow(MAIN.load(Relaxed)) };
                return;
            }
        };
    }
}

/// Send files to the Recycle Bin, without a word: one that will not go is
/// left where it is.  Only a full path is handed over -- the shell does not
/// recycle a file named any other way, it deletes it.
fn recycle(files: &[PathBuf]) {
    // one path after another, each ending in a NUL, and a NUL to end the list
    let mut from: Vec<u16> = Vec::new();
    for file in files.iter().filter(|f| f.is_absolute()) {
        from.extend(file.as_os_str().encode_wide());
        from.push(0);
    }
    if from.is_empty() {
        return;
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: 0,
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: null(),
        fFlags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        fAnyOperationsAborted: 0,
        hNameMappings: null_mut(),
        lpszProgressTitle: null(),
    };
    unsafe { SHFileOperationW(&mut op) };
}

/// 0xRRGGBB as a COLORREF, 0x00BBGGRR -- and back again, the same way.
const fn colorref(c: u32) -> u32 {
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
}

thread_local! {
    /// The colour dialog's sixteen custom colours, as it last left them: the
    /// presets to begin with, the rest white.
    static CUSTOM_COLOURS: core::cell::Cell<[u32; 16]> = const {
        let mut all = [0x00FF_FFFF; 16];
        let mut k = 0;
        while k < tint::PRESETS.len() {
            all[k] = colorref(tint::PRESETS[k].1);
            k += 1;
        }
        core::cell::Cell::new(all)
    };
}

/// The Windows colour dialog, open at `rgb`: the colour picked in it, or
/// None if it was cancelled.
fn pick_colour(rgb: u32) -> Option<u32> {
    let mut custom = CUSTOM_COLOURS.get();
    let mut cc = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: 0,
        hInstance: 0,
        rgbResult: colorref(rgb),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN | CC_ANYCOLOR,
        lCustData: 0,
        lpfnHook: 0,
        lpTemplateName: null(),
    };
    let picked = unsafe { ChooseColorW(&mut cc) } != 0;
    CUSTOM_COLOURS.set(custom);
    picked.then(|| colorref(cc.rgbResult))
}

/// The Windows "Browse for Folder" dialog -- the tree of folders Windows 98
/// had -- open at `start` if it is there: the folder chosen in it, or None if
/// it was cancelled.
fn choose_folder(start: &std::path::Path) -> Option<PathBuf> {
    /// Once the dialog is up, show it the folder to start from.
    unsafe extern "system" fn callback(wnd: HWND, msg: u32, _lp: LPARAM, data: LPARAM) -> i32 {
        if msg == BFFM_INITIALIZED && data != 0 {
            unsafe { SendMessageW(wnd, BFFM_SETSELECTIONW, 1, data) };
        }
        0
    }
    if !com_init() {
        return None;
    }
    let start: Vec<u16> = start.as_os_str().encode_wide().chain(Some(0)).collect();
    let title = wide("Choose the folder of photos this frame shows:");
    let mut name = [0u16; MAX_PATH];
    let info = BROWSEINFOW {
        hwndOwner: 0,
        pidlRoot: null(),
        pszDisplayName: name.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS,
        lpfn: Some(callback),
        lParam: start.as_ptr() as LPARAM,
        iImage: 0,
    };
    unsafe {
        let list = SHBrowseForFolderW(&info);
        if list.is_null() {
            return None;
        }
        // SHGetPathFromIDListW writes at most MAX_PATH characters
        let mut path = [0u16; MAX_PATH];
        let found = SHGetPathFromIDListW(list, path.as_mut_ptr()) != 0;
        co_free(list);
        found.then(|| PathBuf::from(from_wide(&path)))
    }
}

/// What a Colour menu entry's swatch shows: the accent as that tint will
/// make it -- light and dark kept, so not always the colour of that name --
/// or None for an entry without one.
fn swatch_colour(id: usize) -> Option<u32> {
    let accent = 0xFF00_0000 | tint::BASE;
    match id {
        CMD_TINT_CUSTOM => tint::preset_name(tint::current()).is_none().then(|| tint::apply_with(tint::current(), accent)),
        c if (CMD_TINT..CMD_TINT_CUSTOM).contains(&c) => tint::PRESETS.get(c - CMD_TINT).map(|p| tint::apply_with(p.1, accent)),
        _ => None,
    }
}

/// A menu entry's swatch of `argb`, the size of a check mark: the colour in a
/// dark frame.  The caller deletes it.
fn swatch(argb: u32) -> HGDIOBJ {
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXMENUCHECK).max(8), GetSystemMetrics(SM_CYMENUCHECK).max(8)) };
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // top-down
        biPlanes: 1,
        biBitCount: 32,
        ..Default::default()
    };
    let mut bits: *mut c_void = null_mut();
    let bitmap = unsafe { CreateDIBSection(0, &header, DIB_RGB_COLORS, &mut bits, 0, 0) };
    if bitmap == 0 || bits.is_null() {
        return 0;
    }
    // SAFETY: the DIB is w x h 32-bit pixels.
    let px = unsafe { core::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize) };
    for y in 0..h {
        for x in 0..w {
            // a pixel of nothing round it, then the frame, then the colour
            let ring = x.min(y).min(w - 1 - x).min(h - 1 - y);
            px[(y * w + x) as usize] = match ring {
                0 => 0,
                1 => 0xFF40_4040,
                _ => 0xFF00_0000 | argb,
            };
        }
    }
    bitmap
}

fn show_menu(i: Option<usize>) -> Later {
    let Some(model) = with_app(|app| app.menu(i)) else { return Later::Nothing };
    let main = MAIN.load(Relaxed);
    let mut swatches: Vec<HGDIOBJ> = Vec::new();
    let chosen = unsafe {
        let root = CreatePopupMenu();
        let mut stack = vec![root];
        let mut pending: Option<String> = None;
        for (id, label, checked) in &model {
            let menu = *stack.last().unwrap();
            if *id == usize::MAX {
                if label.is_empty() {
                    // close the submenu and hang it on its parent
                    let sub = stack.pop().unwrap();
                    let title = pending.take().unwrap_or_default();
                    AppendMenuW(*stack.last().unwrap(), MF_POPUP, sub as usize, wide(&title).as_ptr());
                } else {
                    pending = Some(label.clone());
                    stack.push(CreatePopupMenu());
                }
            } else if *id == 0 {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
            } else {
                AppendMenuW(menu, MF_STRING | if *checked { MF_CHECKED } else { 0 }, *id, wide(label).as_ptr());
                if let Some(bitmap) = swatch_colour(*id).map(swatch).filter(|&b| b != 0) {
                    let info = MENUITEMINFOW {
                        cbSize: size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_BITMAP,
                        fType: 0,
                        fState: 0,
                        wID: 0,
                        hSubMenu: 0,
                        hbmpChecked: 0,
                        hbmpUnchecked: 0,
                        dwItemData: 0,
                        dwTypeData: null_mut(),
                        cch: 0,
                        hbmpItem: bitmap,
                    };
                    SetMenuItemInfoW(menu, *id as u32, 0, &info);
                    swatches.push(bitmap);
                }
            }
        }
        let mut at = POINT::default();
        GetCursorPos(&mut at);
        // so that a click anywhere else closes the menu
        SetForegroundWindow(main);
        let chosen = TrackPopupMenu(root, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, at.x, at.y, 0, main, null());
        PostMessageW(main, WM_NULL, 0, 0);
        DestroyMenu(root);
        // a menu does not own its bitmaps
        for bitmap in swatches {
            DeleteObject(bitmap);
        }
        chosen as usize
    };
    if chosen == 0 {
        return Later::Nothing;
    }
    with_app(|app| app.menu_command(i, chosen)).unwrap_or(Later::Nothing)
}

// ---------------------------------------------------------------------------
//  Window procedures
// ---------------------------------------------------------------------------

unsafe extern "system" fn widget_proc(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let i = GetWindowLongPtrW(wnd, GWLP_USERDATA) as usize;
        let later = match msg {
            // A widget never takes the keyboard from whatever has it.
            WM_MOUSEACTIVATE => return MA_NOACTIVATE,
            // On the desktop means on the desktop: nothing else restacks it.
            WM_WINDOWPOSCHANGING => {
                if POSITION.load(Relaxed) == Position::Desktop as u8 {
                    (*(lp as *mut WINDOWPOS)).flags |= SWP_NOZORDER;
                }
                return 0;
            }
            // The margin of nothing round a frame is there to drag the widget
            // by; but where a gap narrower than it lays it over the frame of
            // the next widget, the press is for that widget and not this one.
            WM_NCHITTEST => {
                if with_app(|app| app.over_another(i, x_of(lp), y_of(lp))).unwrap_or(false) {
                    return HTTRANSPARENT;
                }
                return DefWindowProcW(wnd, msg, wp, lp);
            }
            // Over text the pointer is the one for text.
            WM_SETCURSOR if lo_word(lp as usize) == HTCLIENT => {
                let mut p = POINT::default();
                GetCursorPos(&mut p);
                ScreenToClient(wnd, &mut p);
                let over_text = with_app(|app| {
                    let (x, y) = app.widget_point(i, p.x, p.y);
                    app.widgets.get(i).is_some_and(|w| w.ibeam(x, y))
                })
                .unwrap_or(false);
                SetCursor(LoadCursorW(0, if over_text { IDC_IBEAM } else { IDC_ARROW } as PCWSTR));
                return 1;
            }
            WM_LBUTTONDOWN => with_app(|app| app.mouse_down(i, wnd, x_of(lp), y_of(lp), false)),
            WM_LBUTTONDBLCLK => with_app(|app| app.mouse_down(i, wnd, x_of(lp), y_of(lp), true)),
            WM_MOUSEMOVE => with_app(|app| app.mouse_move(i, wnd, x_of(lp), y_of(lp), wp & 1 != 0)),
            WM_LBUTTONUP => with_app(|app| app.mouse_up(i, x_of(lp), y_of(lp))),
            WM_MOUSEWHEEL => with_app(|app| app.mouse_wheel(i, wnd, x_of(lp), y_of(lp), Axis::Upright, hi_word(wp) as i16 as i32)),
            WM_MOUSEHWHEEL => with_app(|app| app.mouse_wheel(i, wnd, x_of(lp), y_of(lp), Axis::Sideways, hi_word(wp) as i16 as i32)),
            WM_RBUTTONUP => {
                with_app(|app| app.leave_editor());
                Some(Later::Menu(Some(i)))
            }
            WM_MOUSELEAVE => with_app(|app| {
                app.slots[i].tracking = false;
                app.set_hover(i, None);
                Later::Nothing
            }),
            // Something took the mouse away mid-drag: the window stays where
            // it got to.  A plain press is left to its release.
            WM_CAPTURECHANGED => with_app(|app| {
                if lp != wnd && app.press.as_ref().is_some_and(|p| p.idx == i && p.dragging) {
                    app.press = None;
                    app.keep_place(i);
                    app.settings.save();
                }
                Later::Nothing
            }),
            WM_CLOSE => with_app(|app| {
                app.set_enabled(i, false);
                Later::Nothing
            }),
            // The monitor's scale setting changed, or the window was moved
            // onto one with another: the size Windows offers is its own
            // stretch, not a whole number of times the skin -- the windows
            // are laid out again instead, each at the scale its monitor has.
            // (A drag sees to the window it moves: see `mouse_move`.)
            WM_DPICHANGED => {
                with_app(|app| {
                    if !app.press.as_ref().is_some_and(|p| p.dragging) {
                        app.lay_out();
                    }
                });
                return 0;
            }
            _ => return DefWindowProcW(wnd, msg, wp, lp),
        };
        run_later(later.unwrap_or(Later::Nothing));
        0
    }
}

unsafe extern "system" fn keys_proc(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match keys::read(wnd, msg, wp) {
        keys::Event::Input(input) => {
            with_app(|app| app.key_input(input));
            0
        }
        keys::Event::Pulse => {
            with_app(|app| app.key_pulse());
            0
        }
        keys::Event::Leave => {
            with_app(|app| app.leave_editor());
            0
        }
        // the wheel turns what is being typed on
        keys::Event::Wheel(axis, delta) => {
            let later = with_app(|app| match app.typing {
                Some(i) => app.mouse_wheel(i, app.slots[i].hwnd, x_of(lp), y_of(lp), axis, delta),
                None => Later::Nothing,
            });
            run_later(later.unwrap_or(Later::Nothing));
            0
        }
        keys::Event::Done(result) => result,
        keys::Event::Other => unsafe { DefWindowProcW(wnd, msg, wp, lp) },
    }
}

/// Fire the tick just after each second begins, so the clock's second hand
/// moves when the second does.
fn schedule_tick(wnd: HWND) {
    let (_, ms) = clock::now_ms();
    unsafe { SetTimer(wnd, TICK_TIMER, 1000 - ms + 10, null()) };
}

unsafe extern "system" fn main_proc(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    static TASKBAR_CREATED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    unsafe {
        match msg {
            WM_TIMER if wp == TICK_TIMER => {
                with_app(|app| app.tick());
                schedule_tick(wnd);
            }
            WM_TIMER if wp == FRAME_TIMER => {
                with_app(|app| app.animate());
            }
            // This window marks the bottom of the stack; it stays there.
            WM_WINDOWPOSCHANGING => (*(lp as *mut WINDOWPOS)).flags |= SWP_NOZORDER,
            WM_TRAY if lp as u32 == WM_RBUTTONUP || lp as u32 == WM_LBUTTONUP => run_later(Later::Menu(None)),
            WM_WTSSESSION_CHANGE if wp == WTS_SESSION_LOCK || wp == WTS_SESSION_UNLOCK => {
                with_app(|app| {
                    app.locked = wp == WTS_SESSION_LOCK;
                    if app.locked {
                        app.leave_editor();
                    }
                    // let the log hear of it now, and catch the picture up
                    app.tick();
                    app.repaint_all();
                    app.pace();
                });
            }
            WM_POWERBROADCAST => {
                with_app(|app| match wp {
                    // going to sleep: leave the log as it stands on disk
                    PBT_APMSUSPEND => {
                        let (mut ctx, widgets) = app.ctx();
                        if let Some(w) = widgets.get_mut(DATETIME) {
                            w.shutdown(&mut ctx);
                        }
                    }
                    PBT_APMRESUMEAUTOMATIC => {
                        // put to sleep at one desk and woken at another
                        app.lay_out();
                        app.tick();
                        app.repaint_all();
                    }
                    _ => {}
                });
                return 1;
            }
            // A monitor plugged in, taken away, moved or rescaled, or the
            // taskbar moved: lay the widgets out again for the monitors there
            // are now, from where they were put -- so one that had to move
            // goes back to its place once its monitor does.
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                with_app(|app| app.lay_out());
            }
            WM_QUERYENDSESSION => return 1,
            WM_ENDSESSION if wp != 0 => {
                with_app(|app| app.shutdown());
            }
            WM_CREATE => {
                TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Relaxed);
            }
            WM_DESTROY => {
                with_app(|app| {
                    app.shutdown();
                    app.tray(NIM_DELETE);
                    for i in 0..app.slots.len() {
                        app.stop(i);
                    }
                });
                PostQuitMessage(0);
            }
            // Explorer restarted: the tray icon went with it.
            m if m != 0 && m == TASKBAR_CREATED.load(Relaxed) => {
                with_app(|app| app.tray(NIM_ADD));
            }
            _ => return DefWindowProcW(wnd, msg, wp, lp),
        }
        0
    }
}

const WM_CREATE: u32 = 0x0001;

fn register(class: &str, proc_: WNDPROC, style: u32) {
    unsafe {
        let name = wide(class);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style,
            lpfnWndProc: Some(proc_),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(null()),
            hIcon: 0,
            hCursor: LoadCursorW(0, IDC_ARROW as PCWSTR),
            hbrBackground: 0,
            lpszMenuName: null(),
            lpszClassName: name.as_ptr(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc);
    }
}

pub fn run() -> i32 {
    unsafe {
        // One copy at a time: a second would fight the first over the log.
        CreateMutexW(null(), 0, wide(MUTEX).as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return 0;
        }
        // Before there is a window: the widgets are scaled here, not
        // stretched by Windows (see the head of this file).
        make_dpi_aware();

        let font = skin::datetime::SKIN.var("Font");
        let Some(text) = Text::new(font, skin::FONTS) else {
            MessageBoxW(0, wide("DirectWrite is not available.").as_ptr(), wide(MAIN_TITLE).as_ptr(), MB_OK);
            return 1;
        };

        register(WIDGET_CLASS, widget_proc, CS_DBLCLKS);
        register(MAIN_CLASS, main_proc, 0);
        let main = CreateWindowExW(
            WS_EX_TOOLWINDOW, wide(MAIN_CLASS).as_ptr(), wide(MAIN_TITLE).as_ptr(), WS_POPUP | WS_DISABLED,
            0, 0, 0, 0, 0, 0, GetModuleHandleW(null()), null_mut(),
        );
        MAIN.store(main, Relaxed);
        SetWindowPos(main, HWND_BOTTOM, 0, 0, 0, 0, ZPOS_FLAGS);

        let (locked, signed_in) = session_state();
        let mut app = App::new(Settings::load(), text, locked, signed_in);
        app.tray = App::make_icon();
        *(&raw mut APP) = Some(app);

        with_app(|app| {
            app.start_enabled();
            app.settings.save();
            app.tray(NIM_ADD);
        });

        register_session_notifications(main);
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, 0, foreground_changed, 0, 0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        schedule_tick(main);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWinEvent(hook);
        *(&raw mut APP) = None;
        msg.wParam as i32
    }
}

/// Dropped with the App at exit; nothing else frees the buffer.
impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.dc);
            DeleteObject(self.bitmap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &Family = &FAMILIES[0];
    const COUNTDOWNS: &Family = &FAMILIES[1];
    const PHOTOS: &Family = &FAMILIES[2];

    /// An App whose settings.ini, and whose notes, are in a folder that is
    /// the test's own.  No window class is registered here, so no window is
    /// ever made: a widget that is started runs without one.
    fn app(name: &str) -> (App, PathBuf) {
        let dir = std::env::temp_dir().join(format!("win98-app-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = Text::new(skin::datetime::SKIN.var("Font"), skin::FONTS).unwrap();
        (App::new(Settings::in_memory_at(&dir), text, false, None), dir)
    }

    /// What the Widgets submenu lists, top to bottom.
    fn listed(app: &App) -> Vec<String> {
        let entry = |m: &(usize, String, bool)| {
            (CMD_TOGGLE..usize::MAX).contains(&m.0) || FAMILIES.iter().any(|f| [f.more, f.fewer].contains(&m.0))
        };
        app.menu(None).into_iter().filter(entry).map(|m| m.1).collect()
    }

    /// The notes there are, by what they are called.
    fn titles(app: &App) -> Vec<String> {
        family_titles(app, NOTES)
    }

    /// The ones of a family there are, by what they are called.
    fn family_titles(app: &App, f: &Family) -> Vec<String> {
        app.members(f).into_iter().map(|j| app.slots[j].title.clone()).collect()
    }

    fn offers(app: &App, i: usize, label: &str) -> bool {
        app.menu(Some(i)).iter().any(|m| m.1 == label)
    }

    /// As if slot `i` had a window, put at (x, y) and showing there.
    fn show(app: &mut App, i: usize, x: i32, y: i32) {
        (app.slots[i].hwnd, app.slots[i].x, app.slots[i].y, app.slots[i].put) = (1, x, y, (x, y));
    }

    #[test]
    fn notes_are_made_and_deleted() {
        let (mut app, dir) = app("notes");
        assert_eq!(app.slots.len(), FIXED.len());
        app.new_one(NOTES, Some(NOTE));
        app.new_one(NOTES, None);
        assert_eq!(titles(&app), ["Sticky Note", "Sticky Note 2", "Sticky Note 3"]);
        // each is running, on a file of its own, and will be there next time
        let (second, third) = (FIXED.len(), FIXED.len() + 1);
        assert!(app.widgets.get(second).is_some() && app.widgets.get(third).is_some());
        assert!(dir.join("Notes2.txt").is_file() && dir.join("Notes3.txt").is_file());
        assert_eq!(app.settings.get("Notes3", "Enabled"), Some("1"));
        assert!(app.settings.get("Notes3", "X").is_some());

        // only a note after the first can be deleted
        assert!(!offers(&app, NOTE, "Delete this note") && offers(&app, third, "Delete this note"));
        assert!(matches!(app.menu_command(Some(NOTE), CMD_DELETE_NOTE), Later::Nothing));
        assert!(app.delete(notes::FIRST).is_empty() && !app.slots[NOTE].deleted);

        // an empty one goes without a question, and leaves its file to be binned
        let Later::Delete(section, None) = app.menu_command(Some(second), CMD_DELETE_NOTE) else { panic!("asked about an empty note") };
        assert_eq!(app.delete(&section), [dir.join("Notes2.txt")]);
        assert!(app.slots[second].deleted && app.widgets.get(second).is_none());
        assert_eq!(titles(&app), ["Sticky Note", "Sticky Note 3"]);
        // nothing brings it back, and nothing writes its section again
        app.set_enabled(second, true);
        app.keep_place(second);
        assert!(app.widgets.get(second).is_none() && !app.settings.sections().any(|s| s == "Notes2"));

        // one with something on it is asked about first
        std::fs::write(dir.join("Notes3.txt"), "milk\n").unwrap();
        assert!(matches!(app.menu_command(Some(third), CMD_DELETE_NOTE), Later::Delete(_, Some(_))));

        // the next run finds the notes that are left, and only those
        for i in 0..app.slots.len() {
            app.stop(i);
        }
        app.read_slots();
        assert_eq!(titles(&app), ["Sticky Note", "Sticky Note 3"]);
        assert_eq!(app.slots.len(), app.widgets.0.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_are_counted_up_and_down_from_one_place() {
        let (mut app, dir) = app("count");
        // one entry for more of them wherever the menu is opened, in the list
        // of widgets, and none that ticks a note on and off
        for at in [None, Some(NOTE), Some(WEATHER)] {
            assert_eq!(listed(&app), ["Date/Time", "Resource Meter", "Sticky Note +", "Weather", "Rain Window", "Disk Space", "World Clock", "Battery", "Countdown +", "Claude Usage", "Photo Frame +"]);
            assert_eq!(app.menu(at).iter().filter(|m| m.0 == CMD_MORE_NOTES).count(), 1);
            assert!(!app.menu(at).iter().any(|m| m.0 == CMD_TOGGLE + NOTE));
        }
        let running = |app: &App| app.members(NOTES).into_iter().filter(|&j| app.widgets.get(j).is_some()).count();
        let minus = "Sticky Note \u{2212}";

        // the first comes out, and with it the way to put one back
        app.menu_command(None, CMD_MORE_NOTES);
        assert_eq!((running(&app), app.slots.len()), (1, FIXED.len()));
        assert_eq!(listed(&app), ["Date/Time", "Resource Meter", "Sticky Note +", minus, "Weather", "Rain Window", "Disk Space", "World Clock", "Battery", "Countdown +", "Claude Usage", "Photo Frame +"]);
        // with none closed, more is a new one each time
        app.menu_command(None, CMD_MORE_NOTES);
        app.menu_command(Some(WEATHER), CMD_MORE_NOTES);
        assert_eq!((running(&app), titles(&app).len()), (3, 3));

        // fewer takes the last, or the one it was asked for on, and keeps it
        let (second, third) = (FIXED.len(), FIXED.len() + 1);
        std::fs::write(dir.join("Notes3.txt"), "milk\n").unwrap();
        app.menu_command(None, CMD_FEWER_NOTES);
        assert!(app.widgets.get(third).is_none() && app.widgets.get(second).is_some());
        app.menu_command(Some(NOTE), CMD_FEWER_NOTES);
        assert!(app.widgets.get(NOTE).is_none() && app.widgets.get(second).is_some());
        assert_eq!(app.settings.get("Notes3", "Enabled"), Some("0"));
        assert_eq!(std::fs::read(dir.join("Notes3.txt")).unwrap(), b"milk\n");

        // more brings the closed ones back, lowest number first, before any is made
        app.menu_command(None, CMD_MORE_NOTES);
        assert!(app.widgets.get(NOTE).is_some() && app.widgets.get(third).is_none());
        app.menu_command(None, CMD_MORE_NOTES);
        assert_eq!((running(&app), titles(&app).len()), (3, 3));
        app.menu_command(None, CMD_MORE_NOTES);
        assert_eq!((running(&app), titles(&app).len()), (4, 4));

        // down to none: nothing left to take away, and no entry that offers to
        for _ in 0..4 {
            app.menu_command(None, CMD_FEWER_NOTES);
        }
        assert_eq!(running(&app), 0);
        assert!(!offers(&app, WEATHER, minus));
        app.menu_command(None, CMD_FEWER_NOTES);
        assert_eq!(titles(&app).len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn countdowns_come_and_go_as_notes_do_and_keep_to_their_own() {
        let (mut app, dir) = app("countdowns");
        // the first comes out, then more are made -- in turns with a note
        app.menu_command(None, CMD_MORE_COUNTDOWNS);
        assert!(app.widgets.get(COUNTDOWN).is_some() && app.slots.len() == FIXED.len());
        app.menu_command(None, CMD_MORE_COUNTDOWNS);
        app.new_one(NOTES, Some(NOTE));
        app.menu_command(Some(COUNTDOWN), CMD_MORE_COUNTDOWNS);
        let (second, note, third) = (FIXED.len(), FIXED.len() + 1, FIXED.len() + 2);
        // each family numbered on its own, each with sections of its own
        assert_eq!(family_titles(&app, COUNTDOWNS), ["Countdown", "Countdown 2", "Countdown 3"]);
        assert_eq!(titles(&app), ["Sticky Note", "Sticky Note 2"]);
        let sections: Vec<&str> = [second, note, third].iter().map(|&i| app.slots[i].section.as_str()).collect();
        assert_eq!(sections, ["Countdown2", "Notes2", "Countdown3"]);
        assert_eq!(app.settings.get("Countdown3", "Enabled"), Some("1"));
        assert!(app.settings.get("Countdown3", "X").is_some());
        // counted up and down where the first stands in the list
        let (notes_minus, minus) = ("Sticky Note \u{2212}", "Countdown \u{2212}");
        assert_eq!(
            listed(&app),
            ["Date/Time", "Resource Meter", "Sticky Note +", notes_minus, "Weather", "Rain Window", "Disk Space", "World Clock", "Battery", "Countdown +", minus, "Claude Usage", "Photo Frame +"]
        );

        // only one after the first can be deleted, and each by its own entry
        assert!(!offers(&app, COUNTDOWN, "Delete this countdown") && offers(&app, third, "Delete this countdown"));
        assert!(!offers(&app, third, "Delete this note") && offers(&app, note, "Delete this note"));
        assert!(matches!(app.menu_command(Some(COUNTDOWN), CMD_DELETE_COUNTDOWN), Later::Nothing));
        assert!(matches!(app.menu_command(Some(note), CMD_DELETE_COUNTDOWN), Later::Nothing));
        // a blank one goes without a question, and leaves nothing to be binned
        let Later::Delete(section, None) = app.menu_command(Some(second), CMD_DELETE_COUNTDOWN) else { panic!("asked about a blank countdown") };
        assert!(app.delete(&section).is_empty());
        assert!(app.slots[second].deleted && !app.settings.sections().any(|s| s == "Countdown2"));
        assert_eq!(family_titles(&app, COUNTDOWNS), ["Countdown", "Countdown 3"]);
        // one counting to a day is asked about first
        app.settings.set("Countdown3", "Date", "2026-12-24");
        assert!(matches!(app.menu_command(Some(third), CMD_DELETE_COUNTDOWN), Later::Delete(_, Some(_))));

        // one fewer is a countdown, not the note
        app.menu_command(None, CMD_FEWER_COUNTDOWNS);
        assert!(app.widgets.get(third).is_none() && app.widgets.get(note).is_some());
        assert_eq!(app.settings.get("Countdown3", "Date"), Some("2026-12-24"));

        // the next run finds what is left of both, and only that
        for i in 0..app.slots.len() {
            app.stop(i);
        }
        app.read_slots();
        assert_eq!(titles(&app), ["Sticky Note", "Sticky Note 2"]);
        assert_eq!(family_titles(&app, COUNTDOWNS), ["Countdown", "Countdown 3"]);
        assert_eq!(app.slots.len(), FIXED.len() + 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn photo_frames_come_and_go_and_change_shape_where_they_stand() {
        let (mut app, dir) = app("photos");
        let is = |a: &Skin, b: &Skin| core::ptr::eq(a, b);
        // the first, on the folder beside settings.ini, which is made for it
        app.menu_command(None, CMD_MORE_PHOTOS);
        assert!(app.widgets.get(PHOTO).is_some() && dir.join("Photos").is_dir());
        assert!(is(app.slots[PHOTO].skin, &skin::photo_wide::SKIN));
        app.menu_command(Some(PHOTO), CMD_MORE_PHOTOS);
        let second = FIXED.len();
        assert_eq!(family_titles(&app, PHOTOS), ["Photo Frame", "Photo Frame 2"]);
        assert_eq!(app.slots[second].section, "Photo2");
        assert!(listed(&app).ends_with(&[String::from("Photo Frame +"), String::from("Photo Frame \u{2212}")]));

        // its own entries in its menu, a line between the groups of them
        let menu = app.menu(Some(second));
        let at = |label: &str| menu.iter().position(|m| m.1 == label).unwrap();
        assert!(at("Next photo") < at("Upright frame") && menu[at("Next photo") + 1].0 == 0);
        assert!(menu[at("Wide frame")].2 && !menu[at("Upright frame")].2);

        // turned upright: made again where it was put, the new size, and the
        // first left as it was
        let (put, n) = (app.slots[second].put, app.slots[second].n);
        app.menu_command(Some(second), menu[at("Upright frame")].0);
        assert!(is(app.slots[second].skin, &skin::photo_tall::SKIN));
        assert!(app.widgets.get(second).is_some_and(|w| is(w.skin(), &skin::photo_tall::SKIN)));
        assert_eq!(app.slots[second].put, put);
        let f = app.slots[second].frame();
        assert_eq!((f.x1 - f.x0, f.y1 - f.y0), (176 * n, 265 * n));
        assert_eq!((app.settings.get("Photo2", "Shape"), app.settings.get(photo::FIRST, "Shape")), (Some("tall"), Some("wide")));

        // a new one beside the upright one, though it is wide: a gap from it,
        // level with it
        app.menu_command(Some(second), CMD_MORE_PHOTOS);
        let third = FIXED.len() + 1;
        let (a, b) = (app.slots[second].frame(), app.slots[third].frame());
        assert_eq!(b.x1 - b.x0, 218 * n);
        let gap = app.spacing.gap * n;
        let beside = (b.x1 + gap == a.x0 || a.x1 + gap == b.x0) && a.y0 == b.y0;
        let under = (b.y1 + gap == a.y0 || a.y1 + gap == b.y0) && a.x0 == b.x0;
        assert!(beside || under, "{a:?} and {b:?} are not a gap apart");

        // deleted without a question -- its photos are not its to lose -- and
        // gone from settings.ini, leaving nothing to be binned
        let Later::Delete(section, None) = app.menu_command(Some(second), CMD_DELETE_PHOTO) else { panic!("asked about a photo frame") };
        assert!(app.delete(&section).is_empty());
        assert!(app.slots[second].deleted && !app.settings.sections().any(|s| s == "Photo2"));

        // the next run finds the one that is left, in its own shape
        for i in 0..app.slots.len() {
            app.stop(i);
        }
        app.settings.set("Photo3", "Shape", "square");
        app.read_slots();
        assert_eq!(family_titles(&app, PHOTOS), ["Photo Frame", "Photo Frame 3"]);
        assert!(is(app.slots[FIXED.len()].skin, &skin::photo_square::SKIN));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_note_does_not_come_back_underneath_another_widget() {
        let (mut app, dir) = app("back");
        let work = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let skin = Kind::Notes.skin();
        let (x, y) = ((work.left + work.right - skin.w) / 2, (work.top + work.bottom - skin.h) / 2);
        // where it was left, with nothing there
        (app.slots[NOTE].x, app.slots[NOTE].y) = (x, y);
        app.keep_place(NOTE);
        app.more(NOTES, None);
        assert_eq!((app.slots[NOTE].x, app.slots[NOTE].y), (x, y));
        // the rain has been put there since: beside it, then
        app.fewer(NOTES, None);
        show(&mut app, RAIN, x, y);
        app.more(NOTES, None);
        let moved = app.slots[NOTE].frame();
        assert!(!moved.intersects(&frame(Kind::Rain.skin(), x, y, 1)));
        assert_eq!(app.settings.int(notes::FIRST, "X", -1), app.slots[NOTE].x);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_note_goes_where_there_is_room() {
        let (mut app, dir) = app("room");
        // the first note in the middle of the screen, as if it were showing
        let work = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let skin = Kind::Notes.skin();
        let (x, y) = ((work.left + work.right - skin.w) / 2, (work.top + work.bottom - skin.h) / 2);
        show(&mut app, NOTE, x, y);
        // to its left, the gap from it
        let step = skin.w - 2 * skin.int("Pad", 12) + app.spacing.gap;
        assert_eq!(app.room_beside(NOTE, Kind::Notes.skin()), (x - step, y));
        // the rain is there: to its right, then
        show(&mut app, RAIN, x - step, y);
        assert_eq!(app.room_beside(NOTE, Kind::Notes.skin()), (x + step, y));
        // a widget that only reaches into that place is as much in the way
        show(&mut app, WEATHER, x + step + 40, y - 40);
        assert_ne!(app.room_beside(NOTE, Kind::Notes.skin()), (x + step, y));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn widgets_lost_with_a_monitor_are_shown_and_come_back_with_it() {
        let (mut app, dir) = app("monitors");
        let rect = |left, top, right, bottom| RECT { left, top, right, bottom };
        // Where they were put: a laptop with a monitor to its left.  Where they
        // went missing: the same laptop with two monitors above it, and
        // nothing to its left.
        let laptop = rect(0, 0, 960, 552);
        let home = [laptop, rect(-1920, 0, 0, 1080)];
        let away = [laptop, rect(-725, -864, 811, 0), rect(811, -864, 2347, 0)];

        // two notes on the monitor at the left, frame against frame
        app.new_one(NOTES, Some(NOTE));
        let (first, second) = (NOTE, FIXED.len());
        app.set_enabled(first, true);
        let skin = Kind::Notes.skin();
        let step = skin.w - 2 * skin.int("Pad", 12);
        let places = [(first, (-900, 400)), (second, (-900 + step, 400))];
        for (i, (x, y)) in places {
            (app.slots[i].x, app.slots[i].y) = (x, y);
            app.keep_place(i);
        }
        let shown = |app: &App, i: usize| (app.slots[i].x, app.slots[i].y);
        app.lay_out_on(&home);
        assert!(places.iter().all(|&(i, put)| shown(&app, i) == put));

        // away: each wholly on a monitor, side by side as they were put
        app.lay_out_on(&away);
        for (i, _) in places {
            let (x, y) = shown(&app, i);
            assert_eq!(screens::keep_on(&reach(Kind::Notes.skin(), x, y, app.spacing.margin, 1), &away), (0, 0), "slot {i} at {x},{y}");
        }
        let ((x1, y1), (x2, y2)) = (shown(&app, first), shown(&app, second));
        assert_eq!((x2 - x1, y2 - y1), (step, 0));
        // and where they were put is still where they were put
        for (i, (x, y)) in places {
            assert_eq!(app.slots[i].put, (x, y));
            let section = &app.slots[i].section;
            assert_eq!((app.settings.int(section, "X", 0), app.settings.int(section, "Y", 0)), (x, y));
        }

        // home again: back in their places
        app.lay_out_on(&home);
        assert!(places.iter().all(|&(i, put)| shown(&app, i) == put));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The frame of slot `i` where it is put.
    fn put_frame(app: &App, i: usize) -> Rect {
        frame(app.slots[i].skin, app.slots[i].put.0, app.slots[i].put.1, app.slots[i].n)
    }

    /// Slot `i` put with the top left corner of its frame at (x, y).
    fn put_frame_at(app: &mut App, i: usize, x: i32, y: i32) {
        let pad = app.slots[i].skin.int("Pad", 12);
        app.put(i, x - pad, y - pad);
    }

    /// The menu's command for `px` in the margin's list (`CMD_MARGIN`) or
    /// the gap's (`CMD_GAP`).
    fn pick(app: &App, base: usize, px: i32) -> usize {
        let now = if base == CMD_MARGIN { app.spacing.margin } else { app.spacing.gap };
        base + spacings(now).iter().position(|&p| p == px).unwrap()
    }

    /// What the margin's or the gap's list in the menu has ticked.
    fn ticked(app: &App, base: usize) -> Vec<String> {
        app.menu(None).into_iter().filter(|m| (base..base + 10).contains(&m.0) && m.2).map(|m| m.1).collect()
    }

    #[test]
    fn the_first_layout_stands_on_the_grid() {
        let (app, dir) = app("first");
        let work = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let Spacing { margin, gap } = app.spacing;
        let at = |i: usize| put_frame(&app, i);
        assert_eq!((at(DATETIME).x1, at(DATETIME).y0), (work.right - margin, work.top + margin));
        // down the column, a gap apart
        assert_eq!((at(1).y0 - at(DATETIME).y1, at(WEATHER).y0 - at(1).y1), (gap, gap));
        assert_eq!((at(1).x1, at(WEATHER).x1), (at(DATETIME).x1, at(DATETIME).x1));
        // along the foot and along the top, a gap apart
        for (left, right) in [(NOTE, WEATHER), (RAIN, NOTE), (DISK, RAIN), (BATTERY, DISK), (CLAUDE, BATTERY), (WORLD, DATETIME), (PHOTO, WORLD)] {
            assert_eq!((at(right).x0 - at(left).x1, at(right).y0), (gap, at(left).y0), "slot {left} to slot {right}");
        }
        // the countdown under the world clock, against the first column, and
        // clear of the foot
        assert_eq!((at(COUNTDOWN).y0 - at(WORLD).y1, at(COUNTDOWN).x1), (gap, at(WORLD).x1));
        assert!(at(WEATHER).y0 - at(COUNTDOWN).y1 >= gap);
        // the photo frame, heading a third column, on top of none of them
        assert!((0..FIXED.len()).filter(|&i| i != PHOTO).all(|i| !at(PHOTO).intersects(&at(i))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_margin_and_the_gap_come_from_the_menu_and_the_layout_follows() {
        let (mut app, dir) = app("spacing");
        // three notes running: two down the right-hand side of the main
        // screen, on the grid there is, and one on its own
        app.new_one(NOTES, Some(NOTE));
        app.new_one(NOTES, Some(NOTE));
        let (top, under, alone) = (NOTE, FIXED.len(), FIXED.len() + 1);
        app.set_enabled(top, true);
        let work = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let Spacing { margin, gap } = app.spacing;
        let size = frame(Kind::Notes.skin(), 0, 0, 1);
        let (w, h) = (size.x1 - size.x0, size.y1 - size.y0);
        put_frame_at(&mut app, top, work.right - margin - w, work.top + margin);
        put_frame_at(&mut app, under, work.right - margin - w, work.top + margin + h + gap);
        put_frame_at(&mut app, alone, work.left + 100, work.top + 100);
        app.lay_out();
        let apart = put_frame(&app, alone);

        // a gap of 8: the one under closes up, the one on its own stays
        app.menu_command(Some(top), pick(&app, CMD_GAP, 8));
        assert_eq!(app.spacing, Spacing { margin, gap: 8 });
        assert_eq!(app.settings.int("General", "WidgetGap", 0), 8);
        let (a, b) = (put_frame(&app, top), put_frame(&app, under));
        assert_eq!((a.x1, a.y0), (work.right - margin, work.top + margin));
        assert_eq!((b.x1, b.y0 - a.y1), (a.x1, 8));
        assert_eq!(put_frame(&app, alone), apart);
        // written down for the next run, and shown where it was put
        assert_eq!(app.settings.int(&app.slots[under].section, "Y", 0), app.slots[under].put.1);
        assert_eq!((app.slots[under].x, app.slots[under].y), app.slots[under].put);

        // no margin: the column goes into the corner, the gap kept
        app.menu_command(None, pick(&app, CMD_MARGIN, 0));
        let (a, b) = (put_frame(&app, top), put_frame(&app, under));
        assert_eq!((a.x1, a.y0, b.x1, b.y0 - a.y1), (work.right, work.top, work.right, 8));
        assert_eq!(put_frame(&app, alone), apart);
        assert_eq!((ticked(&app, CMD_MARGIN), ticked(&app, CMD_GAP)), (vec![String::from("0 px")], vec![String::from("8 px")]));

        // a figure written in by hand is offered in its place among the others
        app.settings.set_int("General", "WidgetGap", 7);
        app.read_general();
        let gaps: Vec<String> = app.menu(None).into_iter().filter(|m| (CMD_GAP..CMD_WIDGET).contains(&m.0)).map(|m| m.1).collect();
        assert_eq!(gaps, ["0 px", "4 px", "7 px", "8 px", "10 px", "12 px", "16 px", "24 px", "32 px"]);
        assert_eq!(ticked(&app, CMD_GAP), ["7 px"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_colour_from_the_menu_is_drawn_in_kept_and_ticked() {
        let (mut app, dir) = app("tint");
        // what the Colour menu has ticked
        let ticked = |app: &App| -> Vec<String> {
            app.menu(None).into_iter().filter(|m| (CMD_TINT..=CMD_TINT_CUSTOM).contains(&m.0) && m.2).map(|m| m.1).collect()
        };
        assert_eq!((tint::current(), ticked(&app)), (tint::BASE, vec![String::from("Rust")]));

        // a preset, with a widget running to be given it
        app.set_enabled(NOTE, true);
        let navy = tint::PRESETS.iter().position(|p| p.0 == "Navy").unwrap();
        app.menu_command(Some(NOTE), CMD_TINT + navy);
        assert_eq!(tint::current(), 0x00_0080);
        assert_eq!(ticked(&app), ["Navy"]);
        let written = std::fs::read_to_string(dir.join("settings.ini")).unwrap();
        assert!(written.lines().any(|l| l == "Tint=0,0,128"), "{written}");

        // any other is the dialog's, which is opened once the App is let go
        assert!(matches!(app.menu_command(None, CMD_TINT_CUSTOM), Later::PickColour(0x00_0080)));
        app.set_tint(0x33_6699);
        assert_eq!(ticked(&app), ["Custom..."]);
        assert_eq!(app.settings.get("General", "Tint"), Some("51,102,153"));

        // written in by hand and read again, as Reload settings does
        app.settings.set("General", "Tint", "#123456");
        app.read_general();
        assert_eq!(tint::current(), 0x12_3456);
        app.settings.set("General", "Tint", "nonsense");
        app.read_general();
        assert_eq!(tint::current(), tint::BASE);

        // the skins' own is kept as nothing at all
        app.menu_command(None, CMD_TINT + navy);
        app.menu_command(None, CMD_TINT);
        assert_eq!((tint::current(), app.settings.get("General", "Tint")), (tint::BASE, Some("")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_press_in_the_margin_round_a_frame_is_for_the_frame_under_it() {
        let (mut app, dir) = app("press");
        let pad = Kind::Notes.skin().int("Pad", 12);
        let size = frame(Kind::Notes.skin(), 0, 0, 1);
        let w = size.x1 - size.x0;
        // two notes frame against frame: the first's margin lies over the
        // second's frame
        app.new_one(NOTES, Some(NOTE));
        let second = FIXED.len();
        show(&mut app, NOTE, 100, 100);
        show(&mut app, second, 100 + w, 100);
        let (edge, y) = (100 + pad + w, 100 + pad + 50);
        assert!(app.over_another(NOTE, edge + 3, y));
        // but not for the second there, nor for the first on its own frame,
        // nor in its margin where there is nothing under it
        assert!(!app.over_another(second, edge + 3, y));
        assert!(!app.over_another(NOTE, edge - 3, y));
        assert!(!app.over_another(NOTE, 102, y));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_window_drawn_at_two_is_its_skin_twice_over() {
        let (mut app, dir) = app("scale");
        let (skin, pad) = (Kind::Notes.skin(), Kind::Notes.skin().int("Pad", 12));
        // the frame: twice the pad in, twice as wide and as high
        let (one, two) = (frame(Kind::Notes.skin(), 100, 100, 1), frame(Kind::Notes.skin(), 100, 100, 2));
        assert_eq!((two.x0, two.y0), (100 + 2 * pad, 100 + 2 * pad));
        assert_eq!((two.x1 - two.x0, two.y1 - two.y0), (2 * (one.x1 - one.x0), 2 * (one.y1 - one.y0)));
        assert_eq!((two.x1, two.y1), (100 + 2 * skin.w - 2 * pad, 100 + 2 * skin.h - 2 * pad));
        // and the margin round it, twice as wide
        let around = reach(Kind::Notes.skin(), 100, 100, 12, 2);
        assert_eq!((around.left, around.right), (two.x0 - 24, two.x1 + 24));
        // the mouse, in the widget's own pixels; held, also left of the window
        app.slots[NOTE].n = 2;
        assert_eq!(app.widget_point(NOTE, 61, 50), (30, 25));
        assert_eq!(app.widget_point(NOTE, -1, -3), (-1, -2));
        app.slots[NOTE].n = 1;
        assert_eq!(app.widget_point(NOTE, 61, -3), (61, -3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wheel_fires_once_a_notch_whichever_way_it_turns() {
        let mut rest = 0;
        assert_eq!(notches(&mut rest, Axis::Upright, 120), (Ev::WheelUp, 1));
        assert_eq!(notches(&mut rest, Axis::Upright, -360), (Ev::WheelDown, 3));
        // a positive turn sideways is to the right
        assert_eq!(notches(&mut rest, Axis::Sideways, 240), (Ev::WheelRight, 2));
        assert_eq!(notches(&mut rest, Axis::Sideways, -120), (Ev::WheelLeft, 1));
        assert_eq!(rest, 0);

        // a touchpad's little turns add up to a notch, and what is over is kept
        let fired: Vec<u32> = [50, 50, 50, 50].map(|d| notches(&mut rest, Axis::Sideways, d).1).into();
        assert_eq!((fired, rest), (vec![0, 0, 1, 0], 80));
        // turning back takes from what was kept first
        assert_eq!((notches(&mut rest, Axis::Sideways, -90).1, rest), (0, -10));
    }

    #[test]
    fn the_tray_icon_loads_from_the_ico() {
        let icon = App::make_icon();
        assert_ne!(icon, 0, "Windows would not take skins/Icons/gears_tweakui_b.ico");
        unsafe { DestroyIcon(icon) };
    }

    #[test]
    fn the_colour_dialog_and_the_swatches_are_handed_what_windows_expects() {
        // the structures' sizes as the SDK's headers make them
        let wide = cfg!(target_pointer_width = "64");
        assert_eq!(size_of::<CHOOSECOLORW>(), if wide { 72 } else { 36 });
        assert_eq!(size_of::<MENUITEMINFOW>(), if wide { 80 } else { 48 });
        // a COLORREF is the other way round, and turning it back is the same
        assert_eq!(colorref(0x12_3456), 0x56_3412);
        assert_eq!(colorref(colorref(0x12_3456)), 0x12_3456);

        // a swatch for every preset, of the accent it makes; one for Custom...
        // only while the tint is none of them; none for anything else
        let accent = 0xFF00_0000 | tint::BASE;
        for (k, (_, rgb)) in tint::PRESETS.iter().enumerate() {
            assert_eq!(swatch_colour(CMD_TINT + k), Some(tint::apply_with(*rgb, accent)));
        }
        assert_eq!(swatch_colour(CMD_TINT_CUSTOM), None);
        tint::set(0x33_6699);
        assert_eq!(swatch_colour(CMD_TINT_CUSTOM), Some(tint::apply_with(0x33_6699, accent)));
        tint::set(tint::BASE);
        assert_eq!(swatch_colour(CMD_LOCK), None);
        let bitmap = swatch(0x00_0080);
        assert_ne!(bitmap, 0);
        unsafe { DeleteObject(bitmap) };
    }
}
