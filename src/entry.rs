//! The weather's keyboard: a one-line Windows edit box laid over the city
//! field, for typing the name of the place the forecast should be for.
//!
//!   open     holding the city as it stands, all of it selected
//!   Enter    take what was typed -- as does a click anywhere else
//!   Escape   leave things as they were
//!
//! It is a borderless popup owned by the widget's window rather than a child
//! of it: a layered window does not paint children.  Owned, it stays above the
//! widget, and losing activation is exactly "clicked elsewhere".
//!
//! The note does not use one of these.  A single line in a white field is
//! what a Windows edit box looks like anyway; a sheet of paper with
//! checkboxes in its margin is not, so the note draws its own typing (see
//! `notes`).

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::Relaxed};

use crate::sys::*;

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
}

/// Win32 packs a colour as 0x00BBGGRR.
fn colorref(argb: u32) -> u32 {
    (argb >> 16 & 255) | (argb & 0xFF00) | (argb & 255) << 16
}

/// Posted to the box when the user is done with it, either way.
pub const WM_LEAVE: u32 = WM_APP + 1;
const CLASS: &str = "Win98CityEdit";
/// Characters: no place name is longer.
const LIMIT: usize = 64;

pub struct Entry {
    host: HWND,
    edit: HWND,
    font: HGDIOBJ,
    brush: HGDIOBJ,
    /// What the box opened with: leaving it unchanged is not an answer.
    before: String,
    registered: bool,
}

/// What the box's window procedures need.  Kept apart from the Entry because
/// Windows calls them in the middle of the Entry's own calls -- showing the
/// box activates it -- when the Entry is not free to be read.
static HOST: AtomicIsize = AtomicIsize::new(0);
static EDIT: AtomicIsize = AtomicIsize::new(0);
static BRUSH: AtomicIsize = AtomicIsize::new(0);
static INK: AtomicU32 = AtomicU32::new(0);
static PAPER: AtomicU32 = AtomicU32::new(0x00FF_FFFF);
/// Escape was pressed: what the box holds is not wanted.
static CANCELLED: AtomicBool = AtomicBool::new(false);

impl Entry {
    pub const fn new() -> Entry {
        Entry { host: 0, edit: 0, font: 0, brush: 0, before: String::new(), registered: false }
    }

    pub fn is_open(&self) -> bool {
        self.host != 0 && unsafe { IsWindowVisible(self.host) } != 0
    }

    fn send(&self, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { SendMessageW(self.edit, msg, wp, lp) }
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
                WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL | ES_CENTER,
                0, 0, 0, 0, self.host, 1, module, null_mut(),
            );
            if self.edit == 0 {
                DestroyWindow(self.host);
                self.host = 0;
                return false;
            }
            self.send(EM_SETLIMITTEXT, LIMIT, 0);
            HOST.store(self.host, Relaxed);
            EDIT.store(self.edit, Relaxed);
            let previous = SetWindowLongPtrW(self.edit, GWLP_WNDPROC, keys as *const () as isize);
            SetWindowLongPtrW(self.edit, GWLP_USERDATA, previous);
            true
        }
    }

    /// Put the box up over `owner`, holding `text` with all of it selected, so
    /// that typing replaces it.  False if there is no box to put up.
    pub fn open(&mut self, owner: HWND, proc_: WNDPROC, look: &Look, text: &str) -> bool {
        if !self.create(owner, proc_) {
            return false;
        }
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

            // A one-line edit control has no text rectangle to set: the
            // padding is the strip of the box it is kept out of, which the
            // box paints the same colour.
            let [left, top, right, bottom] = look.padding;
            let mut frame = RECT::default();
            GetWindowRect(owner, &mut frame);
            MoveWindow(self.host, frame.left + look.x, frame.top + look.y, look.w, look.h, 0);
            MoveWindow(self.edit, left, top, look.w - left - right, look.h - top - bottom, 0);
            self.send(EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN, 0);

            SetWindowTextW(self.edit, wide(text).as_ptr());
            self.send(EM_SETSEL, 0, -1);
            CANCELLED.store(false, Relaxed);
            SetWindowPos(self.host, HWND_TOP, 0, 0, 0, 0, SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE);
            SetForegroundWindow(self.host);
            SetFocus(self.edit);
        }
        self.before = text.trim().to_owned();
        true
    }

    fn text(&self) -> String {
        unsafe {
            let n = GetWindowTextLengthW(self.edit).max(0);
            let mut buf = vec![0u16; n as usize + 1];
            let got = GetWindowTextW(self.edit, buf.as_mut_ptr(), n + 1).max(0);
            String::from_utf16_lossy(&buf[..got as usize])
        }
    }

    /// Take the box down.  What was typed, if it is an answer: None if the
    /// box was not open, was left with Escape, or holds what it opened with.
    pub fn leave(&mut self) -> Option<String> {
        if !self.is_open() {
            return None;
        }
        let typed = self.text().trim().to_owned();
        unsafe { ShowWindow(self.host, SW_HIDE) };
        let cancelled = CANCELLED.swap(false, Relaxed);
        (!cancelled && typed != self.before).then_some(typed)
    }

    /// The widget is closing, or the program is: nothing typed is kept.
    pub fn destroy(&mut self) {
        if self.host != 0 {
            unsafe { DestroyWindow(self.host) };
            self.host = 0;
            self.edit = 0;
            HOST.store(0, Relaxed);
            EDIT.store(0, Relaxed);
        }
        BRUSH.store(0, Relaxed);
        unsafe {
            DeleteObject(core::mem::take(&mut self.font));
            DeleteObject(core::mem::take(&mut self.brush));
        }
    }

    /// What the box does with a message by itself; None leaves it to the
    /// caller, which is where the weather hears about WM_LEAVE.
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
                WM_CTLCOLOREDIT => {
                    SetTextColor(wp as HDC, INK.load(Relaxed));
                    SetBkColor(wp as HDC, PAPER.load(Relaxed));
                    Some(BRUSH.load(Relaxed))
                }
                // the strip the padding keeps the edit control out of
                WM_ERASEBKGND => {
                    let mut all = RECT::default();
                    GetClientRect(wnd, &mut all);
                    FillRect(wp as HDC, &all, BRUSH.load(Relaxed));
                    Some(1)
                }
                _ => None,
            }
        }
    }
}

/// The keys a plain edit control gets wrong for a box with no OK button.
unsafe extern "system" fn keys(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let previous = GetWindowLongPtrW(wnd, GWLP_USERDATA);
        match (msg, wp) {
            (WM_KEYDOWN, VK_RETURN) | (WM_KEYDOWN, VK_ESCAPE) => {
                if wp == VK_ESCAPE {
                    CANCELLED.store(true, Relaxed);
                }
                PostMessageW(HOST.load(Relaxed), WM_LEAVE, 0, 0);
                return 0;
            }
            // the characters Enter, Ctrl-Enter, Escape and Ctrl-Backspace
            // type: no beep, and no box glyph in the name
            (WM_CHAR, 0x0D | 0x0A | 0x1B | 0x7F) => return 0,
            (WM_CHAR, 0x01) => {
                SendMessageW(wnd, EM_SETSEL, 0, -1); // CTRL-A
                return 0;
            }
            _ => {}
        }
        CallWindowProcW(previous, wnd, msg, wp, lp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The box is narrower than the longest name it takes: typing past its
    /// right edge has to scroll, not stop.  Nothing is shown.
    #[test]
    fn a_name_wider_than_the_box_can_still_be_typed() {
        unsafe {
            let edit = CreateWindowExW(
                0, wide("EDIT").as_ptr(), wide("").as_ptr(), WS_POPUP | ES_AUTOHSCROLL | ES_CENTER,
                0, 0, 132, 13, 0, 0, GetModuleHandleW(null()), null_mut(),
            );
            assert!(edit != 0);
            SendMessageW(edit, EM_SETLIMITTEXT, LIMIT, 0);
            for _ in 0..LIMIT + 5 {
                SendMessageW(edit, WM_CHAR, 'W' as usize, 0);
            }
            let typed = GetWindowTextLengthW(edit);
            DestroyWindow(edit);
            assert_eq!(typed, LIMIT as i32);
        }
    }
}
