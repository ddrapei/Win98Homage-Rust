//! The note's keyboard: a real multi-line Windows edit box laid over the
//! paper, which saves the way a sticky note does -- as you type, and when you
//! click anywhere else.
//!
//!   open     the file as it is on disk, with the caret where the pointer is
//!   typing   written back a second after it pauses
//!   leaving  a click outside the box, Escape, another window coming forward:
//!            save, hide, and tell the note to read the file back
//!
//! The box is a borderless popup owned by the widget's window rather than a
//! child of it: a layered window does not paint children.  Owned, it stays
//! above the widget, and losing activation is exactly "clicked elsewhere".

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicIsize, AtomicU32, Ordering::Relaxed};
use std::path::{Path, PathBuf};

use crate::settings::write_whole;
use crate::sys::*;

/// Posted to the box when the user asks to leave it.
pub const WM_LEAVE: u32 = WM_APP + 1;
const AUTOSAVE_TIMER: usize = 1;
const CLASS: &str = "Win98NoteEdit";

/// Where things stand after an open or a close, as the note needs to know it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Open,
    Saved,
    Unchanged,
    /// The note could not be written.  The typing is kept in the box and
    /// comes back the next time it opens.
    WriteError,
    ReadError,
    NoWindow,
}

/// Where the box sits over the widget and how it is painted.
pub struct Look {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// left, top, right, bottom: where the text may not go
    pub padding: [i32; 4],
    pub paper: u32,
    pub ink: u32,
    pub face: String,
    pub points: f64,
    pub autosave_ms: u32,
}

// ---------------------------------------------------------------------------
//  The note on disk
// ---------------------------------------------------------------------------

const CR: u16 = b'\r' as u16;
const LF: u16 = b'\n' as u16;
const BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Clone, Copy, Default)]
struct Format {
    bom: bool,
    crlf: bool,
}

/// File bytes to what an edit control wants: UTF-16 with every line break as
/// CR LF.  A note that is not valid UTF-8 is from before the skin was, and is
/// read as Windows-1252.
fn decode(bytes: &[u8]) -> (Vec<u16>, Format) {
    let (bom, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let crlf = bytes.windows(2).any(|w| w == b"\r\n");
    let wide: Vec<u16> = match core::str::from_utf8(body) {
        Ok(s) => s.encode_utf16().collect(),
        Err(_) => body.iter().map(|&b| from_1252(b)).collect(),
    };
    let mut out = Vec::with_capacity(wide.len() + wide.len() / 16);
    let mut i = 0;
    while i < wide.len() {
        match wide[i] {
            CR => {
                out.extend_from_slice(&[CR, LF]);
                if wide.get(i + 1) == Some(&LF) {
                    i += 1;
                }
            }
            LF => out.extend_from_slice(&[CR, LF]),
            c => out.push(c),
        }
        i += 1;
    }
    (out, Format { bom, crlf })
}

/// Edit control text back to file bytes: UTF-8, in the file's own line
/// endings, ending in one line break when there is anything in it.
fn encode(text: &[u16], format: Format) -> Vec<u8> {
    let mut wide: Vec<u16> = Vec::with_capacity(text.len() + 2);
    let mut i = 0;
    while i < text.len() {
        if !format.crlf && text[i] == CR && text.get(i + 1) == Some(&LF) {
            i += 1; // drop the CR, keep the LF
        }
        wide.push(text[i]);
        i += 1;
    }
    if wide.last().is_some_and(|&c| c != LF) {
        if format.crlf {
            wide.push(CR);
        }
        wide.push(LF);
    }
    let mut out = if format.bom { BOM.to_vec() } else { Vec::new() };
    out.extend_from_slice(String::from_utf16_lossy(&wide).as_bytes());
    out
}

fn from_1252(b: u8) -> u16 {
    const HIGH: [u16; 32] = [
        0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152,
        0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122,
        0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
    ];
    match b {
        0x80..=0x9F => HIGH[usize::from(b - 0x80)],
        _ => u16::from(b),
    }
}

struct NoteFile {
    path: PathBuf,
    format: Format,
    /// The file's bytes as of the last read or write; None if there was none.
    disk: Option<Vec<u8>>,
    /// Whether this edit has already put the previous note in the .bak file.
    backed_up: bool,
}

fn read_optional(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

impl NoteFile {
    /// A missing file is an empty note; one that is there but cannot be read
    /// is an error, because saving over it would destroy it.
    fn open(path: &Path) -> std::io::Result<(NoteFile, Vec<u16>)> {
        let disk = read_optional(path)?;
        let (text, format) = disk.as_deref().map(decode).unwrap_or_default();
        Ok((NoteFile { path: path.to_owned(), format, disk, backed_up: false }, text))
    }

    /// Catch up with the file, for typing that is being kept because it could
    /// not be saved: if something else wrote the note meanwhile, that version
    /// becomes what the next save backs up.
    fn resync(&mut self) -> std::io::Result<()> {
        let now = read_optional(&self.path)?;
        if now != self.disk {
            self.disk = now;
            self.backed_up = false;
        }
        Ok(())
    }

    /// Ok(true) is a write, Ok(false) is nothing to write.  On an error the
    /// note is as it was, and the same call can simply be made again.
    fn save(&mut self, text: &[u16]) -> std::io::Result<bool> {
        let bytes = encode(text, self.format);
        let unchanged = match self.disk.as_deref() {
            Some(disk) => disk == bytes.as_slice(),
            None => bytes.is_empty(),
        };
        if unchanged {
            return Ok(false);
        }
        // The note as it was before this edit: once per edit, not once per
        // autosave, or the backup would only ever be a second old.
        if !self.backed_up {
            if let Some(disk) = self.disk.as_deref() {
                let mut bak = self.path.as_os_str().to_owned();
                bak.push(".bak");
                std::fs::write(PathBuf::from(bak), disk)?;
            }
            self.backed_up = true;
        }
        write_whole(&self.path, &bytes)?;
        self.disk = Some(bytes);
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
//  Finding your way around the box
// ---------------------------------------------------------------------------

/// Where the 1-based `line` of `text` starts, in UTF-16 units.
fn line_start(text: &[u16], line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    text.iter().enumerate().filter(|&(_, &c)| c == LF).nth(line - 2).map_or(text.len(), |(i, _)| i + 1)
}

/// The start of the word before `at`, for CTRL-Backspace: back over any
/// spaces, then back over the word -- but never across a line break.
fn word_start(text: &[u16], at: usize) -> usize {
    let at = at.min(text.len());
    let blank = |c: u16| c == b' ' as u16 || c == b'\t' as u16;
    let mut i = at;
    while i > 0 && blank(text[i - 1]) {
        i -= 1;
    }
    if i == at && i > 0 && text[i - 1] == LF {
        return if i >= 2 && text[i - 2] == CR { i - 2 } else { i - 1 };
    }
    while i > 0 && !blank(text[i - 1]) && text[i - 1] != LF {
        i -= 1;
    }
    i
}

// ---------------------------------------------------------------------------
//  The box
// ---------------------------------------------------------------------------

pub struct Editor {
    host: HWND,
    edit: HWND,
    /// The edit control's own window procedure, under the subclass.
    edit_proc: isize,
    font: HGDIOBJ,
    brush: HGDIOBJ,
    file: Option<NoteFile>,
    registered: bool,
}

/// What the box's window procedure needs.  Kept apart from the Editor because
/// Windows calls that procedure in the middle of the Editor's own calls --
/// showing the box activates it -- when the Editor is not free to be read.
static EDIT: AtomicIsize = AtomicIsize::new(0);
static BRUSH: AtomicIsize = AtomicIsize::new(0);
static INK: AtomicU32 = AtomicU32::new(0);
static PAPER: AtomicU32 = AtomicU32::new(0x00FF_FFFF);
static AUTOSAVE_MS: AtomicU32 = AtomicU32::new(0);

/// Win32 packs a colour as 0x00BBGGRR.
pub fn colorref(argb: u32) -> u32 {
    (argb >> 16 & 255) | (argb & 0xFF00) | (argb & 255) << 16
}

impl Editor {
    pub const fn new() -> Editor {
        Editor { host: 0, edit: 0, edit_proc: 0, font: 0, brush: 0, file: None, registered: false }
    }

    pub fn is_open(&self) -> bool {
        self.host != 0 && unsafe { IsWindowVisible(self.host) } != 0
    }

    fn send(&self, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { SendMessageW(self.edit, msg, wp, lp) }
    }

    fn text(&self) -> Vec<u16> {
        unsafe {
            let n = GetWindowTextLengthW(self.edit).max(0);
            let mut buf = vec![0u16; n as usize + 1];
            let got = GetWindowTextW(self.edit, buf.as_mut_ptr(), n + 1).max(0);
            buf.truncate(got as usize);
            buf
        }
    }

    fn is_modified(&self) -> bool {
        self.edit != 0 && self.send(EM_GETMODIFY, 0, 0) != 0
    }

    fn create(&mut self, owner: HWND, proc_: WNDPROC) -> bool {
        if self.host != 0 {
            return true;
        }
        unsafe {
            let module = GetModuleHandleW(null());
            let class = wide(CLASS);
            if !self.registered {
                let wc = WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    lpfnWndProc: Some(proc_),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: module,
                    hIcon: 0,
                    hCursor: LoadCursorW(0, IDC_IBEAM as PCWSTR),
                    hbrBackground: 0,
                    lpszMenuName: null(),
                    lpszClassName: class.as_ptr(),
                    hIconSm: 0,
                };
                RegisterClassExW(&wc);
                self.registered = true;
            }
            self.host = CreateWindowExW(
                WS_EX_TOOLWINDOW, class.as_ptr(), wide("").as_ptr(), WS_POPUP | WS_CLIPCHILDREN,
                0, 0, 0, 0, owner, 0, module, null_mut(),
            );
            if self.host == 0 {
                return false;
            }
            self.edit = CreateWindowExW(
                0, wide("EDIT").as_ptr(), wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
                0, 0, 0, 0, self.host, 1, module, null_mut(),
            );
            if self.edit == 0 {
                DestroyWindow(self.host);
                self.host = 0;
                return false;
            }
            self.send(EM_SETLIMITTEXT, 0, 0); // as long as a note gets
            EDIT.store(self.edit, Relaxed);
            self.edit_proc = SetWindowLongPtrW(self.edit, GWLP_WNDPROC, keys as *const () as isize);
            SetWindowLongPtrW(self.edit, GWLP_USERDATA, self.edit_proc);
            true
        }
    }

    /// Put the box up over `owner`, showing the file's 1-based `line` at the
    /// top and `rows` wrapped rows into it, with the caret under the pointer.
    pub fn open(&mut self, owner: HWND, proc_: WNDPROC, look: &Look, path: &Path, line: usize, rows: usize) -> Status {
        if !self.create(owner, proc_) {
            return Status::NoWindow;
        }
        let reopening = self.is_open();
        unsafe {
            // New objects in before the old ones go: the control never holds
            // a deleted font.  Points become pixels at 96 dpi, as on the skin.
            let font = CreateFontW(
                -((look.points * 96.0 / 72.0).round() as i32), 0, 0, 0, FW_NORMAL, 0, 0, 0,
                DEFAULT_CHARSET, 0, 0, NONANTIALIASED_QUALITY, 0, wide(&look.face).as_ptr(),
            );
            if font != 0 {
                self.send(WM_SETFONT, font as WPARAM, 0);
                DeleteObject(core::mem::replace(&mut self.font, font));
            }
            let brush = CreateSolidBrush(colorref(look.paper));
            if brush != 0 {
                BRUSH.store(brush, Relaxed);
                DeleteObject(core::mem::replace(&mut self.brush, brush));
            }
            INK.store(colorref(look.ink), Relaxed);
            PAPER.store(colorref(look.paper), Relaxed);
            AUTOSAVE_MS.store(look.autosave_ms, Relaxed);

            let mut frame = RECT::default();
            GetWindowRect(owner, &mut frame);
            MoveWindow(self.host, frame.left + look.x, frame.top + look.y, look.w, look.h, 0);
            MoveWindow(self.edit, 0, 0, look.w, look.h, 0);
            // A resize resets the text rectangle, so this comes after it.
            self.send(EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN, 0);
            let area = RECT { left: look.padding[0], top: look.padding[1], right: look.w - look.padding[2], bottom: look.h - look.padding[3] };
            self.send(EM_SETRECTNP, 0, &area as *const RECT as LPARAM);
        }

        if !reopening {
            // Typing that could not be saved last time is still in the box,
            // and stays there: reloading the note would lose it a second time.
            let pending = self.is_modified() && self.file.as_ref().is_some_and(|f| f.path == path);
            let ready = if pending {
                self.file.as_mut().is_some_and(|f| f.resync().is_ok())
            } else {
                match NoteFile::open(path) {
                    Ok((file, text)) => {
                        self.file = Some(file);
                        let mut z = text;
                        z.push(0);
                        unsafe { SetWindowTextW(self.edit, z.as_ptr()) };
                        self.send(EM_EMPTYUNDOBUFFER, 0, 0);
                        self.send(EM_SETMODIFY, 0, 0);
                        true
                    }
                    Err(_) => false,
                }
            };
            if !ready {
                return Status::ReadError;
            }
            let at = line_start(&self.text(), line);
            let target = self.send(EM_LINEFROMCHAR, at, 0) + rows as isize;
            let first = self.send(EM_GETFIRSTVISIBLELINE, 0, 0);
            self.send(EM_LINESCROLL, 0, target - first);
        }
        unsafe {
            SetWindowPos(self.host, HWND_TOP, 0, 0, 0, 0, SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE);
            SetForegroundWindow(self.host);
            SetFocus(self.edit);
        }
        if !reopening {
            self.caret_to_pointer();
        }
        Status::Open
    }

    /// The caret under the mouse pointer -- where the click that opened the
    /// box landed -- or at the end if the pointer is not over the control.
    fn caret_to_pointer(&self) {
        let (mut p, mut r) = (POINT::default(), RECT::default());
        unsafe {
            GetCursorPos(&mut p);
            ScreenToClient(self.edit, &mut p);
            GetClientRect(self.edit, &mut r);
        }
        let inside = p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom;
        let at = if inside {
            let point = ((p.y as u16 as u32) << 16 | p.x as u16 as u32) as LPARAM;
            let hit = self.send(EM_CHARFROMPOS, 0, point) as u32;
            // Both halves are 16 bits, so past 64K characters the index wraps;
            // the line's own start puts back what was lost.
            let (low, line) = (hit & 0xFFFF, hit >> 16);
            let start = self.send(EM_LINEINDEX, line as usize, 0) as u32;
            start + (low.wrapping_sub(start) & 0xFFFF)
        } else {
            unsafe { GetWindowTextLengthW(self.edit) }.max(0) as u32
        };
        self.send(EM_SETSEL, at as usize, at as LPARAM);
    }

    /// Write the box out.  The box stays modified until a write works, which
    /// is what makes a failed one get tried again.
    fn save(&mut self) -> Status {
        if !self.is_modified() {
            return Status::Unchanged;
        }
        let typed = self.text();
        match self.file.as_mut().map(|f| f.save(&typed)) {
            Some(Err(_)) => Status::WriteError,
            Some(Ok(wrote)) => {
                self.send(EM_SETMODIFY, 0, 0);
                if wrote { Status::Saved } else { Status::Unchanged }
            }
            None => Status::Unchanged,
        }
    }

    /// Typing has paused for the autosave delay: the box stays open.
    pub fn autosave(&mut self) {
        unsafe { KillTimer(self.host, AUTOSAVE_TIMER) };
        if self.is_open() {
            self.save();
        }
    }

    /// Leaving the note: save and hide.  None if the box was not open.
    pub fn leave(&mut self) -> Option<Status> {
        if !self.is_open() {
            return None;
        }
        let status = self.save();
        unsafe {
            KillTimer(self.host, AUTOSAVE_TIMER);
            ShowWindow(self.host, SW_HIDE);
        }
        Some(status)
    }

    /// The end of the program: whatever the box holds is written as it goes.
    pub fn destroy(&mut self) {
        if self.host != 0 {
            if self.is_open() {
                self.save();
            }
            unsafe { DestroyWindow(self.host) };
            self.host = 0;
            self.edit = 0;
            EDIT.store(0, Relaxed);
        }
        BRUSH.store(0, Relaxed);
        unsafe {
            DeleteObject(core::mem::take(&mut self.font));
            DeleteObject(core::mem::take(&mut self.brush));
        }
    }

    /// What the box does with a message by itself; None leaves it to the
    /// caller, which is where the note hears about WM_LEAVE and the timer.
    pub fn handle(wnd: HWND, msg: u32, wp: WPARAM, _lp: LPARAM) -> Option<LRESULT> {
        unsafe {
            match msg {
                // Losing activation is leaving.  Gaining it, the keyboard goes
                // to the text and not to the frame around it.
                WM_ACTIVATE => {
                    if lo_word(wp) as usize == WA_INACTIVE {
                        PostMessageW(wnd, WM_LEAVE, 0, 0);
                    } else if EDIT.load(Relaxed) != 0 {
                        SetFocus(EDIT.load(Relaxed));
                    }
                    Some(0)
                }
                // Every keystroke restarts the timer, so it fires once typing
                // has stopped for the whole delay.
                WM_COMMAND if hi_word(wp) as usize == EN_CHANGE => {
                    if AUTOSAVE_MS.load(Relaxed) > 0 {
                        SetTimer(wnd, AUTOSAVE_TIMER, AUTOSAVE_MS.load(Relaxed), null());
                    }
                    Some(0)
                }
                WM_CTLCOLOREDIT => {
                    SetTextColor(wp as HDC, INK.load(Relaxed));
                    SetBkColor(wp as HDC, PAPER.load(Relaxed));
                    Some(BRUSH.load(Relaxed))
                }
                WM_ERASEBKGND => Some(1), // the edit control covers every pixel
                _ => None,
            }
        }
    }

    pub fn is_autosave_timer(wp: WPARAM) -> bool {
        wp == AUTOSAVE_TIMER
    }
}

/// The three keys a plain edit control gets wrong for a note.
unsafe extern "system" fn keys(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let previous = GetWindowLongPtrW(wnd, GWLP_USERDATA);
        let send = |m: u32, w: WPARAM, l: LPARAM| SendMessageW(wnd, m, w, l);
        match (msg, wp) {
            (WM_KEYDOWN, VK_ESCAPE) => {
                PostMessageW(parent_of(wnd), WM_LEAVE, 0, 0);
                return 0;
            }
            (WM_CHAR, 0x1B) => return 0, // the character Escape types: no beep
            (WM_CHAR, 0x01) => {
                send(EM_SETSEL, 0, -1); // CTRL-A
                return 0;
            }
            (WM_CHAR, 0x7F) => {
                // CTRL-Backspace: the selection if there is one, else the
                // word before the caret.  It would otherwise type a box glyph.
                let (mut start, mut end) = (0u32, 0u32);
                send(EM_GETSEL, &mut start as *mut u32 as WPARAM, &mut end as *mut u32 as LPARAM);
                if start == end {
                    let n = GetWindowTextLengthW(wnd).max(0);
                    let mut buf = vec![0u16; n as usize + 1];
                    let got = GetWindowTextW(wnd, buf.as_mut_ptr(), n + 1).max(0);
                    buf.truncate(got as usize);
                    send(EM_SETSEL, word_start(&buf, start as usize), start as LPARAM);
                }
                let nothing = [0u16];
                send(EM_REPLACESEL, 1, nothing.as_ptr() as LPARAM);
                return 0;
            }
            _ => {}
        }
        CallWindowProcW(previous, wnd, msg, wp, lp)
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetParent(wnd: HWND) -> HWND;
}

fn parent_of(wnd: HWND) -> HWND {
    unsafe { GetParent(wnd) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn lf_file_round_trips() {
        let file = b"[ ] milk\n[x] eggs\n";
        let (text, format) = decode(file);
        assert_eq!(text, w("[ ] milk\r\n[x] eggs\r\n"));
        assert_eq!(encode(&text, format), file);
    }

    #[test]
    fn crlf_and_bom_are_kept() {
        let file = b"\xEF\xBB\xBFone\r\ntwo\r\n";
        let (text, format) = decode(file);
        assert_eq!(text, w("one\r\ntwo\r\n"));
        assert_eq!(encode(&text, format), file);
    }

    #[test]
    fn a_final_line_break_is_added_once() {
        let format = Format::default();
        assert_eq!(encode(&w("a\r\nb"), format), b"a\nb\n");
        assert_eq!(encode(&w("a\r\nb\r\n"), format), b"a\nb\n");
        assert_eq!(encode(&w(""), format), b"");
    }

    #[test]
    fn a_1252_note_is_read_as_1252_and_written_as_utf8() {
        let (text, format) = decode(b"caf\xE9 \x80\x96\n");
        assert_eq!(text, w("caf\u{e9} \u{20ac}\u{2013}\r\n"));
        assert_eq!(encode(&text, format), "caf\u{e9} \u{20ac}\u{2013}\n".as_bytes());
    }

    #[test]
    fn line_and_word_starts() {
        let text = w("ab\r\ncd\r\nef");
        assert_eq!(line_start(&text, 1), 0);
        assert_eq!(line_start(&text, 2), 4);
        assert_eq!(line_start(&text, 9), text.len());
        let text = w("buy oat milk  ");
        assert_eq!(word_start(&text, text.len()), 8);
        let text = w("ab\r\n  cd");
        assert_eq!(word_start(&text, 4), 2);
    }
}
