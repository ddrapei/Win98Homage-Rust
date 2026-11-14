//! The host: one small process, one thread, a layered window for each widget.
//!
//! There is one of each of the six, and as many more Sticky Notes as have
//! been asked for: a note after the first is a window like any other, made
//! from the same skin, with a section of settings.ini and a text file of its
//! own (see `notes`).  The windows are numbered -- the six first, then the
//! other notes -- and a number is never handed on: a deleted note's stays
//! its own, unused, until the program ends.
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
//! The process is not DPI aware, on purpose: the skins are laid out in whole
//! pixels, and on a scaled display Windows stretches the widgets as a whole.
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

use crate::canvas::{Canvas, Rect};
use crate::clock;
use crate::datetime::DateTime;
use crate::disk::Disk;
use crate::keys::{self, Input, Key, Keys};
use crate::notes::{self, Notes};
use crate::rain::Rain;
use crate::resources::Resources;
use crate::settings::Settings;
use crate::skin::{self, Act, Ev, Hooks, Skin};
use crate::sys::*;
use crate::text::Text;
use crate::weather::Weather;
use crate::widget::{Ctx, Dirty, Outcome, Pressed, Request, Widget};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    DateTime,
    Resources,
    Notes,
    Weather,
    Rain,
    Disk,
}

impl Kind {
    fn skin(self) -> &'static Skin {
        match self {
            Kind::DateTime => &skin::datetime::SKIN,
            Kind::Resources => &skin::resources::SKIN,
            Kind::Notes => &skin::notes::SKIN,
            Kind::Weather => &skin::weather::SKIN,
            Kind::Rain => &skin::rain::SKIN,
            Kind::Disk => &skin::disk::SKIN,
        }
    }
}

/// The widgets there is always one of, in the order their windows are
/// numbered: what each is, its section of settings.ini, and what the menus
/// call it.  The notes after the first follow these.
const FIXED: [(Kind, &str, &str); 6] = [
    (Kind::DateTime, "DateTime", "Date/Time"),
    (Kind::Resources, "Resources", "Resource Meter"),
    (Kind::Notes, notes::FIRST, "Sticky Note"),
    (Kind::Weather, "Weather", "Weather"),
    (Kind::Rain, "Rain", "Rain Window"),
    (Kind::Disk, "Disk", "Disk Space"),
];
const DATETIME: usize = 0;
const NOTE: usize = 2;
const WEATHER: usize = 3;
const RAIN: usize = 4;
const DISK: usize = 5;

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
/// Skins snap to each other and to the screen edge from this far away.
const SNAP: i32 = 10;
/// Px between the frames of two widgets laid out one above the other.
const APART: i32 = 10;
/// How many notes' widths, or heights, from a note room for a new one is
/// looked for.
const ROOM_STEPS: i32 = 8;
/// With no room anywhere, a new note sits this far down and right of the old.
const CASCADE: i32 = 24;
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
const CMD_AUTOSTART: usize = 50;
const CMD_SETTINGS: usize = 51;
const CMD_RELOAD: usize = 52;
const CMD_EXIT: usize = 60;
/// From here up to CMD_TOGGLE: the widget's own entries.
const CMD_WIDGET: usize = 100;
/// From here up: show or close a window, by its number.
const CMD_TOGGLE: usize = 1000;

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
    /// Delete the note with this section -- not its number: other notes may
    /// come and go while the question is on the screen.  With it, the title
    /// and the text of the question, for a note that has something on it.
    Delete(String, Option<(String, String)>),
    Quit,
}

/// The frame of a window at (x, y): a window has `Pad` of nothing round it.
fn frame(kind: Kind, x: i32, y: i32) -> Rect {
    let (skin, pad) = (kind.skin(), kind.skin().int("Pad", 12));
    Rect::new(x + pad, y + pad, x + skin.w - pad, y + skin.h - pad)
}

struct Slot {
    kind: Kind,
    /// Its section of settings.ini, and what the menus call it.
    section: String,
    title: String,
    /// A note that has been deleted.  Nothing is left of it but its number,
    /// which no other window is given.
    deleted: bool,
    hwnd: HWND,
    x: i32,
    y: i32,
    /// The meter the pointer is over, if it is one that reacts to that.
    hover: Option<usize>,
    tracking: bool,
    seen: skin::Seen,
}

impl Slot {
    fn new(kind: Kind, section: &str, title: String) -> Slot {
        Slot { kind, section: section.to_owned(), title, deleted: false, hwnd: 0, x: 0, y: 0, hover: None, tracking: false, seen: Vec::new() }
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

struct Buffer {
    dc: HDC,
    bitmap: HGDIOBJ,
    bits: *mut u32,
    w: i32,
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
    buffer: Buffer,
    widgets: Widgets,
    slots: Vec<Slot>,
    /// The window that holds the keyboard while a widget is typed on, and
    /// which widget that is.
    keys: Keys,
    typing: Option<usize>,
    press: Option<Press>,
    wheel: i32,
    position: Position,
    snap: bool,
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

fn work_area(around: &RECT) -> RECT {
    unsafe {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, rcMonitor: RECT::default(), rcWork: RECT::default(), dwFlags: 0 };
        GetMonitorInfoW(MonitorFromRect(around, MONITOR_DEFAULTTONEAREST), &mut info);
        info.rcWork
    }
}

fn virtual_screen() -> RECT {
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        RECT { left: x, top: y, right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN), bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN) }
    }
}

/// Keep a window of w x h wholly on the screens.
fn clamp_to_screen(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    let v = virtual_screen();
    (x.clamp(v.left, (v.right - w).max(v.left)), y.clamp(v.top, (v.bottom - h).max(v.top)))
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
            Buffer { dc, bitmap, bits: bits as *mut u32, w }
        }
    }
}

impl App {
    /// The App as settings.ini describes it, with no widget started yet.
    fn new(settings: Settings, text: Text, locked: bool, signed_in: Option<i64>) -> App {
        // one buffer, as large as the largest skin: every window is drawn in it
        let (w, h) = FIXED.iter().fold((0, 0), |(w, h), f| (w.max(f.0.skin().w), h.max(f.0.skin().h)));
        let mut app = App {
            settings,
            text,
            buffer: Buffer::new(w, h),
            // both filled in by read_slots, from settings.ini
            widgets: Widgets(Vec::new()),
            slots: Vec::new(),
            keys: Keys::new(),
            typing: None,
            press: None,
            wheel: 0,
            position: Position::Desktop,
            snap: true,
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
        {
            let (mut ctx, widgets) = self.ctx();
            let mut widget: Box<dyn Widget> = match kind {
                Kind::DateTime => Box::new(DateTime::new(&ctx, None)),
                Kind::Resources => Box::new(Resources::new()),
                Kind::Notes => Box::new(Notes::new(&mut ctx, &section, None)),
                Kind::Weather => Box::new(Weather::new(&ctx)),
                Kind::Rain => Box::new(Rain::new(&ctx)),
                Kind::Disk => Box::new(Disk::new(&ctx)),
            };
            widget.tick(&mut ctx);
            widgets.0[i] = Some(widget);
        }
        let skin = kind.skin();
        let (x, y) = clamp_to_screen(self.slots[i].x, self.slots[i].y, skin.w, skin.h);
        self.slots[i].x = x;
        self.slots[i].y = y;
        unsafe {
            let wnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                wide(WIDGET_CLASS).as_ptr(), wide(&self.slots[i].title).as_ptr(), WS_POPUP,
                x, y, skin.w, skin.h, 0, 0, GetModuleHandleW(null()), null_mut(),
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
        // a deleted note has no section to write to, and is not to be given one
        if self.slots[i].deleted {
            return;
        }
        self.settings.set_int(&self.slots[i].section, "Enabled", on as i32);
        if on { self.start(i) } else { self.stop(i) }
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

    /// Where a window sits, written down for the next run.
    fn keep_place(&mut self, i: usize) {
        let slot = &self.slots[i];
        if !slot.deleted {
            self.settings.set_int(&slot.section, "X", slot.x);
            self.settings.set_int(&slot.section, "Y", slot.y);
        }
    }

    // ---- more notes ------------------------------------------------------------

    /// Somewhere to put a window the size of slot `b`'s, next to it: the
    /// nearest place to its left, its right, below or above it where the new
    /// frame is on the same screen and on top of no widget that is showing.
    /// (On the desktop a new window goes underneath the others: one that
    /// overlapped would be made and not seen.)  Side by side the frames
    /// touch, and one above the other they are APART, as when first laid out.
    fn room_beside(&self, b: usize) -> (i32, i32) {
        let base = &self.slots[b];
        let skin = base.kind.skin();
        let own = frame(base.kind, base.x, base.y);
        let work = work_area(&RECT { left: own.x0, top: own.y0, right: own.x1, bottom: own.y1 });
        let free = |&(x, y): &(i32, i32)| {
            let f = frame(base.kind, x, y);
            f.x0 >= work.left && f.y0 >= work.top && f.x1 <= work.right && f.y1 <= work.bottom
                && !self.slots.iter().any(|s| s.hwnd != 0 && frame(s.kind, s.x, s.y).intersects(&f))
        };
        let (dx, dy) = (own.x1 - own.x0, own.y1 - own.y0 + APART);
        (1..=ROOM_STEPS)
            .flat_map(|k| [(-k, 0), (k, 0), (0, k), (0, -k)])
            .map(|(i, j)| (base.x + i * dx, base.y + j * dy))
            .find(free)
            .unwrap_or_else(|| clamp_to_screen(base.x + CASCADE, base.y + CASCADE, skin.w, skin.h))
    }

    /// The notes there are: the first, then the others in the order of their
    /// numbers -- which, their names being alike but for the number, is
    /// shortest name first.
    fn notes(&self) -> Vec<usize> {
        let mut more: Vec<usize> = (FIXED.len()..self.slots.len()).filter(|&j| !self.slots[j].deleted).collect();
        more.sort_by_key(|&j| (self.slots[j].title.len(), &self.slots[j].title));
        core::iter::once(NOTE).chain(more).collect()
    }

    /// The note "one fewer" takes off the screen: the one it was asked for
    /// on, or, asked for anywhere else, the last that is showing.
    fn note_to_close(&self, from: Option<usize>) -> Option<usize> {
        let open = |&j: &usize| self.slots[j].kind == Kind::Notes && self.widgets.get(j).is_some();
        from.filter(open).or_else(|| self.notes().into_iter().rev().find(open))
    }

    /// One more note on the screen.  A note that was closed comes back first,
    /// where it was and with what was on it, the lowest number before the
    /// others -- there is no other way back to it -- and with none closed a
    /// new one is made.
    fn more_notes(&mut self, from: Option<usize>) {
        let Some(closed) = self.notes().into_iter().find(|&j| self.widgets.get(j).is_none()) else {
            return self.new_note(from);
        };
        // Something may have been put where it was since: it would come back
        // underneath that, and not be seen.  Then it goes where a new one would.
        let own = frame(Kind::Notes, self.slots[closed].x, self.slots[closed].y);
        if self.slots.iter().any(|s| s.hwnd != 0 && frame(s.kind, s.x, s.y).intersects(&own)) {
            let beside = self.note_to_close(from).unwrap_or(closed);
            (self.slots[closed].x, self.slots[closed].y) = self.room_beside(beside);
            self.keep_place(closed);
        }
        self.set_enabled(closed, true);
    }

    /// One note fewer on the screen: closed, as its X closes it, text kept.
    fn fewer_notes(&mut self, from: Option<usize>) {
        if let Some(i) = self.note_to_close(from) {
            self.set_enabled(i, false);
        }
    }

    /// Another note: a window beside the note it was asked for on -- or, asked
    /// for anywhere else, beside the last note there is -- with a section of
    /// settings.ini and an empty file of its own.
    fn new_note(&mut self, from: Option<usize>) {
        let is_note = |s: &Slot| s.kind == Kind::Notes && !s.deleted;
        let beside = from.filter(|&i| is_note(&self.slots[i])).or_else(|| self.slots.iter().rposition(is_note)).unwrap_or(NOTE);
        let (x, y) = self.room_beside(beside);
        let section = notes::create(&mut self.settings);
        let title = notes::name(&section);
        self.slots.push(Slot { x, y, ..Slot::new(Kind::Notes, &section, title) });
        self.widgets.0.push(None);
        let i = self.slots.len() - 1;
        self.keep_place(i);
        self.set_enabled(i, true);
    }

    /// Delete a note after the first: its window, and its section of
    /// settings.ini.  What it leaves on disk is handed back, to be disposed
    /// of once the App has been let go of.
    fn delete_note(&mut self, section: &str) -> Vec<PathBuf> {
        let found = self.slots.iter().position(|s| !s.deleted && s.section == section);
        let Some(i) = found.filter(|&i| i >= FIXED.len()) else { return Vec::new() };
        self.stop(i);
        self.slots[i].deleted = true;
        let files = notes::forget(&mut self.settings, section);
        self.settings.save();
        files
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
        // SAFETY: the buffer is at least as large as the largest skin.
        let mut canvas = unsafe { Canvas::new(self.buffer.bits, self.buffer.w, skin.w, skin.h) };
        canvas.set_clip(rect);
        canvas.clear(rect);
        skin::draw(skin, widget as &dyn Hooks, &mut canvas, &mut self.text, &mut self.slots[i].seen);

        let size = SIZE { cx: skin.w, cy: skin.h };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA };
        let area = RECT { left: rect.x0, top: rect.y0, right: rect.x1, bottom: rect.y1 };
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

    /// Widget `i` is being typed on -- the note, or the weather's city field
    /// -- give it the keyboard.  There is one keyboard: whichever had it is
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

    fn mouse_down(&mut self, i: usize, wnd: HWND, x: i32, y: i32, double: bool) -> Later {
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
        // A press whose release went somewhere else is over.
        if !button_down {
            self.press = None;
        }
        if self.press.as_ref().is_some_and(|p| p.idx == i && p.owned) {
            let dirty = self.widgets.get_mut(i).map_or(Dirty::None, |w| w.drag(x, y));
            self.paint(i, dirty);
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
                let (px, py) = (press.window.x + dx, press.window.y + dy);
                let (nx, ny) = self.snapped(i, px, py);
                self.slots[i].x = nx;
                self.slots[i].y = ny;
                unsafe { SetWindowPos(wnd, 0, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
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
        let meters = self.slots[i].kind.skin().meters;
        for (meter, ev) in [(was, Ev::Leave), (over, Ev::Over)] {
            if let Some(act) = meter.and_then(|m| meters[m].acts.iter().find(|a| a.ev == ev)) {
                self.command(i, act, 0, 0);
            }
        }
    }

    fn mouse_up(&mut self, i: usize, x: i32, y: i32) -> Later {
        let press = self.press.take();
        unsafe { ReleaseCapture() };
        let Some(press) = press.filter(|p| p.idx == i) else { return Later::Nothing };
        if press.dragging {
            self.keep_place(i);
            self.settings.save();
            return Later::Nothing;
        }
        if press.spent || press.owned {
            return Later::Nothing;
        }
        self.fire(i, Ev::Up, x, y).unwrap_or(Later::Nothing)
    }

    fn mouse_wheel(&mut self, i: usize, wnd: HWND, screen_x: i32, screen_y: i32, delta: i32) -> Later {
        let mut p = POINT { x: screen_x, y: screen_y };
        unsafe { ScreenToClient(wnd, &mut p) };
        self.wheel += delta;
        let mut later = Later::Nothing;
        while self.wheel.abs() >= WHEEL_DELTA {
            let ev = if self.wheel > 0 { Ev::WheelUp } else { Ev::WheelDown };
            self.wheel -= WHEEL_DELTA * self.wheel.signum();
            if let Some(l) = self.fire(i, ev, p.x, p.y) {
                later = l;
            }
        }
        later
    }

    /// Where a dragged window lands: pulled onto the edges of the other
    /// widgets and of the screen when it comes within SNAP of them, unless
    /// Ctrl or Shift is held.
    fn snapped(&self, i: usize, mut x: i32, mut y: i32) -> (i32, i32) {
        let skin = self.slots[i].kind.skin();
        let (w, h) = (skin.w, skin.h);
        let held = |key: i32| unsafe { GetKeyState(key) } < 0;
        if self.snap && !held(VK_CONTROL) && !held(VK_SHIFT) {
            let near = |a: i32, b: i32| (a - b).abs() < SNAP;
            for (j, other) in self.slots.iter().enumerate() {
                if j == i || other.hwnd == 0 {
                    continue;
                }
                let (ox, oy, ow, oh) = (other.x, other.y, other.kind.skin().w, other.kind.skin().h);
                if y < oy + oh && y + h > oy {
                    if near(x, ox) { x = ox }
                    if near(x, ox + ow) { x = ox + ow }
                    if near(x + w, ox) { x = ox - w }
                    if near(x + w, ox + ow) { x = ox + ow - w }
                }
                if x < ox + ow && x + w > ox {
                    if near(y, oy) { y = oy }
                    if near(y, oy + oh) { y = oy + oh }
                    if near(y + h, oy) { y = oy - h }
                    if near(y + h, oy + oh) { y = oy + oh - h }
                }
            }
            let work = work_area(&RECT { left: x, top: y, right: x + w, bottom: y + h });
            if near(x, work.left) { x = work.left }
            if near(y, work.top) { y = work.top }
            if near(x, work.right - w) { x = work.right - w }
            if near(y, work.bottom - h) { y = work.bottom - h }
        }
        clamp_to_screen(x, y, w, h)
    }

    /// After the screens change: nothing may be left where it cannot be seen.
    fn keep_on_screen(&mut self) {
        for slot in &mut self.slots {
            if slot.hwnd == 0 {
                continue;
            }
            let skin = slot.kind.skin();
            let (x, y) = clamp_to_screen(slot.x, slot.y, skin.w, skin.h);
            if (x, y) != (slot.x, slot.y) {
                slot.x = x;
                slot.y = y;
                unsafe { SetWindowPos(slot.hwnd, 0, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
            }
        }
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
                m.push((CMD_WIDGET + item.id as usize, item.label.to_owned(), item.checked));
            }
            if !own.is_empty() {
                m.push((0, String::new(), false));
            }
            // The first note is closed, like the other five widgets; the ones
            // after it can also be deleted.
            if self.slots[i].kind == Kind::Notes && i >= FIXED.len() {
                m.push((CMD_DELETE_NOTE, String::from("Delete this note"), false));
                m.push((0, String::new(), false));
            }
        }
        m.push((SUB, String::from("Position"), false));
        m.push((CMD_DESKTOP, String::from("On desktop"), self.position == Position::Desktop));
        m.push((CMD_NORMAL, String::from("Normal"), self.position == Position::Normal));
        m.push((CMD_TOPMOST, String::from("Always on top"), self.position == Position::Topmost));
        m.push((SUB, String::new(), false));
        m.push((CMD_SNAP, String::from("Snap to edges"), self.snap));
        m.push((CMD_LOCK, String::from("Lock position"), self.locked_in_place));
        if let Some(i) = i {
            m.push((CMD_CLOSE, format!("Close {}", self.slots[i].title), false));
        }
        m.push((0, String::new(), false));
        m.push((SUB, String::from("Widgets"), false));
        for j in 0..FIXED.len() {
            if j != NOTE {
                m.push((CMD_TOGGLE + j, self.slots[j].title.clone(), self.slots[j].hwnd != 0));
                continue;
            }
            // There is one of each of the others, shown or not.  Of notes
            // there is a number: one more, and -- while any is showing -- one
            // fewer.
            m.push((CMD_MORE_NOTES, format!("{} +", FIXED[NOTE].2), false));
            if self.note_to_close(i).is_some() {
                m.push((CMD_FEWER_NOTES, format!("{} \u{2212}", FIXED[NOTE].2), false));
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
            CMD_MORE_NOTES => self.more_notes(i),
            CMD_FEWER_NOTES => self.fewer_notes(i),
            CMD_DELETE_NOTE => {
                let Some(slot) = i.filter(|&i| i >= FIXED.len()).map(|i| &self.slots[i]) else { return Later::Nothing };
                // Asked about first, unless there is nothing on it to lose.
                // (The right click that opened the menu ended any typing, so
                // what is in the file is what is on the sheet.)
                let file = notes::file(&self.settings, &slot.section);
                let ask = (!notes::is_blank(&file)).then(|| {
                    let text = format!("Delete this note?\n\nIts text, {}, goes to the Recycle Bin.", file.display());
                    (format!("Delete {}", slot.title), text)
                });
                return Later::Delete(slot.section.clone(), ask);
            }
            c if c >= CMD_TOGGLE => {
                let j = c - CMD_TOGGLE;
                if j < self.slots.len() {
                    self.set_enabled(j, self.slots[j].hwnd == 0);
                }
            }
            c if c >= CMD_WIDGET => {
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
        self.settings = Settings::load();
        self.read_general();
        self.read_slots();
        self.start_enabled();
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
        self.locked_in_place = self.settings.flag("General", "LockPosition", false);
    }

    /// Which windows there are -- the six, and a note for every other
    /// section of settings.ini that is a note's -- and where each sits: where
    /// it was left, or -- the first time -- in a column down the right-hand
    /// side of the screen, frames ten pixels apart, the note and the weather
    /// side by side at the foot, the rain to the left of the note and the disk
    /// to the left of the rain, frame against frame.  Called with every window shut: at the start, and when
    /// settings.ini is read again.
    fn read_slots(&mut self) {
        self.slots = FIXED.iter().map(|&(kind, section, title)| Slot::new(kind, section, title.to_owned())).collect();
        for section in notes::others(&self.settings) {
            let title = notes::name(&section);
            self.slots.push(Slot::new(Kind::Notes, &section, title));
        }
        self.widgets.0.clear();
        self.widgets.0.resize_with(self.slots.len(), || None);

        let work = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let size = |i: usize| (FIXED[i].0.skin().w, FIXED[i].0.skin().h);
        let pad = FIXED[DATETIME].0.skin().int("Pad", 12);
        let overlap = 2 * pad - APART;
        let x0 = work.right - size(DATETIME).0 - 4;
        let y0 = work.top + 4;
        let y1 = y0 + size(DATETIME).1 - overlap;
        let y2 = y1 + size(1).1 - overlap;
        let defaults = [
            (x0, y0),
            (x0, y1),
            (x0, y2),
            (x0 + size(DATETIME).0 - size(WEATHER).0, y2),
            (x0 - size(RAIN).0 + 2 * pad, y2),
            (x0 - size(RAIN).0 - size(DISK).0 + 4 * pad, y2),
        ];
        for i in 0..self.slots.len() {
            // A note after the first was put somewhere when it was made.  One
            // written into settings.ini by hand, with no place given, steps
            // down from the first.
            let (x, y) = defaults.get(i).copied().unwrap_or_else(|| {
                let step = CASCADE * (i + 1 - FIXED.len()) as i32;
                (defaults[NOTE].0 + step, defaults[NOTE].1 + step)
            });
            let section = &self.slots[i].section;
            let placed = self.settings.get(section, "X").is_some();
            let (x, y) = (self.settings.int(section, "X", x), self.settings.int(section, "Y", y));
            (self.slots[i].x, self.slots[i].y) = (x, y);
            if !placed {
                self.keep_place(i);
            }
        }
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
                with_sharp_dpi(|| unsafe {
                    MessageBoxW(0, wide(&text).as_ptr(), wide(&format!("About {title}")).as_ptr(), MB_OK | MB_ICONINFORMATION)
                });
                Later::Nothing
            }
            Later::Delete(section, ask) => {
                let sure = ask.is_none_or(|(title, text)| {
                    with_sharp_dpi(|| unsafe { MessageBoxW(0, wide(&text).as_ptr(), wide(&title).as_ptr(), MB_YESNO | MB_ICONQUESTION) }) == IDYES
                });
                if sure {
                    recycle(&with_app(|app| app.delete_note(&section)).unwrap_or_default());
                }
                Later::Nothing
            }
            Later::Open(path) => {
                unsafe {
                    ShellExecuteW(0, wide("open").as_ptr(), wide(&path.to_string_lossy()).as_ptr(), null(), null(), SW_SHOWNORMAL);
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

fn show_menu(i: Option<usize>) -> Later {
    let Some(model) = with_app(|app| app.menu(i)) else { return Later::Nothing };
    let main = MAIN.load(Relaxed);
    let chosen = with_sharp_dpi(|| unsafe {
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
            }
        }
        let mut at = POINT::default();
        GetCursorPos(&mut at);
        // so that a click anywhere else closes the menu
        SetForegroundWindow(main);
        let chosen = TrackPopupMenu(root, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, at.x, at.y, 0, main, null());
        PostMessageW(main, WM_NULL, 0, 0);
        DestroyMenu(root);
        chosen as usize
    });
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
            // Over text the pointer is the one for text.
            WM_SETCURSOR if lo_word(lp as usize) == HTCLIENT => {
                let mut p = POINT::default();
                GetCursorPos(&mut p);
                ScreenToClient(wnd, &mut p);
                let over_text = with_app(|app| app.widgets.get(i).is_some_and(|w| w.ibeam(p.x, p.y))).unwrap_or(false);
                SetCursor(LoadCursorW(0, if over_text { IDC_IBEAM } else { IDC_ARROW } as PCWSTR));
                return 1;
            }
            WM_LBUTTONDOWN => with_app(|app| app.mouse_down(i, wnd, x_of(lp), y_of(lp), false)),
            WM_LBUTTONDBLCLK => with_app(|app| app.mouse_down(i, wnd, x_of(lp), y_of(lp), true)),
            WM_MOUSEMOVE => with_app(|app| app.mouse_move(i, wnd, x_of(lp), y_of(lp), wp & 1 != 0)),
            WM_LBUTTONUP => with_app(|app| app.mouse_up(i, x_of(lp), y_of(lp))),
            WM_MOUSEWHEEL => with_app(|app| app.mouse_wheel(i, wnd, x_of(lp), y_of(lp), hi_word(wp) as i16 as i32)),
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
        keys::Event::Wheel(delta) => {
            let later = with_app(|app| match app.typing {
                Some(i) => app.mouse_wheel(i, app.slots[i].hwnd, x_of(lp), y_of(lp), delta),
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
                        app.tick();
                        app.repaint_all();
                    }
                    _ => {}
                });
                return 1;
            }
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                with_app(|app| app.keep_on_screen());
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
        let entry = |m: &(usize, String, bool)| (CMD_TOGGLE..usize::MAX).contains(&m.0) || [CMD_MORE_NOTES, CMD_FEWER_NOTES].contains(&m.0);
        app.menu(None).into_iter().filter(entry).map(|m| m.1).collect()
    }

    /// The notes there are, by what they are called.
    fn titles(app: &App) -> Vec<String> {
        app.notes().into_iter().map(|j| app.slots[j].title.clone()).collect()
    }

    fn offers(app: &App, i: usize, label: &str) -> bool {
        app.menu(Some(i)).iter().any(|m| m.1 == label)
    }

    /// As if slot `i` had a window, showing at (x, y).
    fn show(app: &mut App, i: usize, x: i32, y: i32) {
        (app.slots[i].hwnd, app.slots[i].x, app.slots[i].y) = (1, x, y);
    }

    #[test]
    fn notes_are_made_and_deleted() {
        let (mut app, dir) = app("notes");
        assert_eq!(app.slots.len(), FIXED.len());
        app.new_note(Some(NOTE));
        app.new_note(None);
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
        assert!(app.delete_note(notes::FIRST).is_empty() && !app.slots[NOTE].deleted);

        // an empty one goes without a question, and leaves its file to be binned
        let Later::Delete(section, None) = app.menu_command(Some(second), CMD_DELETE_NOTE) else { panic!("asked about an empty note") };
        assert_eq!(app.delete_note(&section), [dir.join("Notes2.txt")]);
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
            assert_eq!(listed(&app), ["Date/Time", "Resource Meter", "Sticky Note +", "Weather", "Rain Window", "Disk Space"]);
            assert_eq!(app.menu(at).iter().filter(|m| m.0 == CMD_MORE_NOTES).count(), 1);
            assert!(!app.menu(at).iter().any(|m| m.0 == CMD_TOGGLE + NOTE));
        }
        let running = |app: &App| app.notes().into_iter().filter(|&j| app.widgets.get(j).is_some()).count();
        let minus = "Sticky Note \u{2212}";

        // the first comes out, and with it the way to put one back
        app.menu_command(None, CMD_MORE_NOTES);
        assert_eq!((running(&app), app.slots.len()), (1, FIXED.len()));
        assert_eq!(listed(&app), ["Date/Time", "Resource Meter", "Sticky Note +", minus, "Weather", "Rain Window", "Disk Space"]);
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
    fn a_note_does_not_come_back_underneath_another_widget() {
        let (mut app, dir) = app("back");
        let work = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let skin = Kind::Notes.skin();
        let (x, y) = ((work.left + work.right - skin.w) / 2, (work.top + work.bottom - skin.h) / 2);
        // where it was left, with nothing there
        (app.slots[NOTE].x, app.slots[NOTE].y) = (x, y);
        app.more_notes(None);
        assert_eq!((app.slots[NOTE].x, app.slots[NOTE].y), (x, y));
        // the rain has been put there since: beside it, then
        app.fewer_notes(None);
        show(&mut app, RAIN, x, y);
        app.more_notes(None);
        let moved = frame(Kind::Notes, app.slots[NOTE].x, app.slots[NOTE].y);
        assert!(!moved.intersects(&frame(Kind::Rain, x, y)));
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
        // to its left, frame against frame
        let step = skin.w - 2 * skin.int("Pad", 12);
        assert_eq!(app.room_beside(NOTE), (x - step, y));
        // the rain is there: to its right, then
        show(&mut app, RAIN, x - step, y);
        assert_eq!(app.room_beside(NOTE), (x + step, y));
        // a widget that only reaches into that place is as much in the way
        show(&mut app, WEATHER, x + step + 40, y - 40);
        assert_ne!(app.room_beside(NOTE), (x + step, y));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tray_icon_loads_from_the_ico() {
        let icon = App::make_icon();
        assert_ne!(icon, 0, "Windows would not take skins/Icons/gears_tweakui_b.ico");
        unsafe { DestroyIcon(icon) };
    }
}
