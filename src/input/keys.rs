//! The keyboard, for a widget that is typed on: the note, and the weather's
//! city field.
//!
//! A widget's window never takes the keyboard, and stays where it is in the
//! stack of windows whatever is clicked.  So while one is being typed on, a
//! window of no size that it owns takes the keyboard for it: what is typed
//! arrives here and is handed on as `Input`, and losing the keyboard again --
//! a click on the desktop, another window coming forward -- is exactly
//! "clicked elsewhere", which is when the typing is done with.
//!
//! The same window runs the one timer the typing needs, which blinks the
//! caret and lets the note save itself once the typing has paused; and it is
//! the window the clipboard is opened for.
//!
//! Nothing is drawn here.  The widget draws its own text, caret and
//! selection, the same way whether or not anyone is typing -- see `notes`
//! and `weather`.

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use crate::sys::*;

/// Posted to the window when the keyboard has gone elsewhere.
pub const WM_LEAVE: u32 = WM_APP + 1;
const PULSE_TIMER: usize = 1;
const CLASS: &str = "Win98Keys";
/// Ms between pulses when the caret is set not to blink.
const STEADY_MS: u32 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    SelectAll,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Key { key: Key, shift: bool, ctrl: bool },
    Char(char),
}

/// What a message to the window comes to.
pub enum Event {
    Input(Input),
    /// The caret's timer.
    Pulse,
    /// The wheel, by this much: where wheels turn whatever has the keyboard
    /// rather than whatever is under the pointer, that is this window.
    Wheel(i32),
    /// The keyboard has gone elsewhere, or the window was told to close.
    Leave,
    /// Dealt with here; the window procedure's answer.
    Done(LRESULT),
    /// Not ours: DefWindowProc's.
    Other,
}

/// The first half of a character that arrives as two WM_CHARs.
static HIGH: AtomicU32 = AtomicU32::new(0);

pub struct Keys {
    wnd: HWND,
    /// The widget's window it was made for, and is owned by.
    owner: HWND,
    registered: bool,
}

impl Keys {
    pub const fn new() -> Keys {
        Keys { wnd: 0, owner: 0, registered: false }
    }

    pub fn is_open(&self) -> bool {
        self.wnd != 0 && unsafe { IsWindowVisible(self.wnd) } != 0
    }

    /// The window, for the clipboard to be opened under.
    pub fn window(&self) -> HWND {
        self.wnd
    }

    /// Whether the window belongs to this widget's, and so goes when it does.
    pub fn owned_by(&self, owner: HWND) -> bool {
        self.wnd != 0 && self.owner == owner
    }

    /// Ms between pulses: half a blink of the caret, as the user has it set.
    pub fn pulse_ms() -> u32 {
        match unsafe { GetCaretBlinkTime() } {
            0 | u32::MAX => STEADY_MS,
            ms => ms,
        }
    }

    /// Whether the caret is to blink at all.
    pub fn blinks() -> bool {
        !matches!(unsafe { GetCaretBlinkTime() }, 0 | u32::MAX)
    }

    fn create(&mut self, owner: HWND, proc_: WNDPROC) -> bool {
        if self.owned_by(owner) {
            return true;
        }
        // There is one keyboard, lent to whichever widget is typed on, and
        // the window that holds it is owned by that one: made again for a
        // different widget rather than kept from the last.
        self.destroy();
        self.owner = owner;
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
                    hCursor: 0,
                    hbrBackground: 0,
                    lpszMenuName: null(),
                    lpszClassName: class.as_ptr(),
                    hIconSm: 0,
                };
                RegisterClassExW(&wc);
                self.registered = true;
            }
            self.wnd = CreateWindowExW(
                WS_EX_TOOLWINDOW, class.as_ptr(), wide("").as_ptr(), WS_POPUP,
                0, 0, 0, 0, owner, 0, module, null_mut(),
            );
        }
        self.wnd != 0
    }

    /// Take the keyboard for the widget whose window is `owner`.  False if
    /// there is no window to take it with.  The window procedure is the one
    /// given the first time; it is the same for every widget.
    pub fn open(&mut self, owner: HWND, proc_: WNDPROC) -> bool {
        if !self.create(owner, proc_) {
            return false;
        }
        HIGH.store(0, Relaxed);
        unsafe {
            // Over the widget, so that whatever the system puts beside the
            // typing -- an input method's window -- turns up near it.
            let mut frame = RECT::default();
            GetWindowRect(owner, &mut frame);
            SetWindowPos(self.wnd, HWND_TOP, frame.left, frame.top, 0, 0, SWP_SHOWWINDOW);
            SetForegroundWindow(self.wnd);
            SetFocus(self.wnd);
            SetTimer(self.wnd, PULSE_TIMER, Keys::pulse_ms(), null());
        }
        true
    }

    /// Give the keyboard up.  False if it was not held.
    pub fn close(&mut self) -> bool {
        if !self.is_open() {
            return false;
        }
        unsafe {
            KillTimer(self.wnd, PULSE_TIMER);
            ShowWindow(self.wnd, SW_HIDE);
        }
        true
    }

    pub fn destroy(&mut self) {
        if self.wnd != 0 {
            unsafe { DestroyWindow(self.wnd) };
            self.wnd = 0;
        }
    }
}

/// What a message to the keyboard window means.  Called from its window
/// procedure, which is all that procedure does.
pub fn read(wnd: HWND, msg: u32, wp: WPARAM) -> Event {
    let held = |key: i32| unsafe { GetKeyState(key) } < 0;
    match msg {
        // Losing activation is leaving -- but said by post, because this
        // arrives in the middle of whatever took it away.
        WM_ACTIVATE => {
            unsafe {
                if lo_word(wp) as usize == WA_INACTIVE {
                    PostMessageW(wnd, WM_LEAVE, 0, 0);
                } else {
                    SetFocus(wnd);
                }
            }
            Event::Done(0)
        }
        // Alt-F4 lands here, and would otherwise destroy the window.
        WM_LEAVE | WM_CLOSE => Event::Leave,
        WM_TIMER if wp == PULSE_TIMER => Event::Pulse,
        WM_MOUSEWHEEL => Event::Wheel(hi_word(wp) as i16 as i32),
        WM_KEYDOWN => {
            let (shift, ctrl) = (held(VK_SHIFT), held(VK_CONTROL));
            // AltGr is Ctrl and Alt together, and types a character
            let chord = ctrl && !held(VK_MENU);
            let key = match wp {
                VK_LEFT => Key::Left,
                VK_RIGHT => Key::Right,
                VK_UP => Key::Up,
                VK_DOWN => Key::Down,
                VK_HOME => Key::Home,
                VK_END => Key::End,
                VK_PRIOR => Key::PageUp,
                VK_NEXT => Key::PageDown,
                VK_BACK => Key::Backspace,
                VK_DELETE if shift && !ctrl => Key::Cut,
                VK_DELETE => Key::Delete,
                VK_INSERT if chord => Key::Copy,
                VK_INSERT if shift => Key::Paste,
                VK_RETURN => Key::Enter,
                VK_TAB => Key::Tab,
                VK_ESCAPE => Key::Escape,
                0x41 if chord => Key::SelectAll,
                0x43 if chord => Key::Copy,
                0x58 if chord => Key::Cut,
                0x56 if chord => Key::Paste,
                0x5A if chord && shift => Key::Redo,
                0x5A if chord => Key::Undo,
                0x59 if chord => Key::Redo,
                _ => return Event::Other,
            };
            Event::Input(Input::Key { key, shift, ctrl })
        }
        WM_CHAR => {
            let unit = wp as u32 & 0xFFFF;
            let c = match unit {
                // what Enter, Backspace, Escape and the Ctrl chords type: all
                // of them have been keys already
                0..=0x1F | 0x7F => None,
                0xD800..=0xDBFF => {
                    HIGH.store(unit, Relaxed);
                    None
                }
                0xDC00..=0xDFFF => match HIGH.swap(0, Relaxed) {
                    0 => None,
                    high => char::from_u32(0x10000 + ((high - 0xD800) << 10) + (unit - 0xDC00)),
                },
                _ => char::from_u32(unit),
            };
            c.map_or(Event::Done(0), |c| Event::Input(Input::Char(c)))
        }
        _ => Event::Other,
    }
}

// ---------------------------------------------------------------------------
//  The clipboard
// ---------------------------------------------------------------------------

/// The text on the clipboard, if that is what is on it.
pub fn clipboard(owner: HWND) -> Option<String> {
    unsafe {
        if OpenClipboard(owner) == 0 {
            return None;
        }
        let mut text = None;
        let data = GetClipboardData(CF_UNICODETEXT);
        if data != 0 {
            let units = GlobalSize(data) / 2;
            let p = GlobalLock(data) as *const u16;
            if !p.is_null() {
                text = Some(from_wide(core::slice::from_raw_parts(p, units)));
                GlobalUnlock(data);
            }
        }
        CloseClipboard();
        text
    }
}

/// Put `text` on the clipboard, with the line breaks every program expects
/// to find there.
pub fn set_clipboard(owner: HWND, text: &str) {
    let wide: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().chain(Some(0)).collect();
    unsafe {
        if OpenClipboard(owner) == 0 {
            return;
        }
        EmptyClipboard();
        let data = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        if data != 0 {
            let p = GlobalLock(data) as *mut u16;
            if !p.is_null() {
                core::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                GlobalUnlock(data);
                // the clipboard owns it from here, unless it would not take it
                if SetClipboardData(CF_UNICODETEXT, data) == 0 {
                    GlobalFree(data);
                }
            } else {
                GlobalFree(data);
            }
        }
        CloseClipboard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the window made of the messages it was sent, in order.
    static HEARD: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    /// The window procedure as the host writes it, noting down what arrives.
    unsafe extern "system" fn noting(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let heard = match read(wnd, msg, wp) {
            Event::Input(Input::Char(c)) => format!("char {c}"),
            Event::Input(Input::Key { key, .. }) => format!("key {key:?}"),
            Event::Leave => String::from("leave"),
            Event::Wheel(by) => format!("wheel {by}"),
            Event::Pulse => return 0,
            Event::Done(result) => return result,
            Event::Other => return unsafe { DefWindowProcW(wnd, msg, wp, lp) },
        };
        HEARD.lock().unwrap().push(heard);
        0
    }

    /// The window has no size and is never seen: it still has to be one the
    /// keyboard can be given to, and what is typed at it has to come out the
    /// other side -- through a real message queue, as it will.
    #[test]
    fn a_window_of_no_size_holds_the_keyboard_and_hears_it() {
        let mut keys = Keys::new();
        assert!(keys.open(0, noting));
        assert!(keys.is_open());
        let wnd = keys.window();
        unsafe {
            SetActiveWindow(wnd);
            assert_eq!(GetActiveWindow(), wnd);
            SetFocus(wnd);
            assert_eq!(GetFocus(), wnd);

            PostMessageW(wnd, WM_CHAR, 'h' as usize, 0);
            PostMessageW(wnd, WM_KEYDOWN, VK_LEFT, 0);
            PostMessageW(wnd, WM_CHAR, 0x0D, 0);
            PostMessageW(wnd, WM_MOUSEWHEEL, (-120i16 as u16 as usize) << 16, 0);
            // what Windows sends when another window takes the keyboard
            SendMessageW(wnd, WM_ACTIVATE, WA_INACTIVE, 0);
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        assert_eq!(*HEARD.lock().unwrap(), ["char h", "key Left", "wheel -120", "leave"]);
        assert!(keys.close());
        assert!(!keys.close());
        keys.destroy();
    }

    #[test]
    fn keys_and_characters_are_told_apart() {
        assert!(matches!(read(0, WM_CHAR, 'a' as usize), Event::Input(Input::Char('a'))));
        // Backspace's character is not typed: the key did the work
        assert!(matches!(read(0, WM_CHAR, 8), Event::Done(0)));
        // a character outside the first plane arrives in two halves
        assert!(matches!(read(0, WM_CHAR, 0xD83D), Event::Done(0)));
        assert!(matches!(read(0, WM_CHAR, 0xDE00), Event::Input(Input::Char('\u{1F600}'))));
        assert!(matches!(read(0, WM_KEYDOWN, VK_BACK), Event::Input(Input::Key { key: Key::Backspace, .. })));
        assert!(matches!(read(0, WM_KEYDOWN, 0x42), Event::Other));
    }
}
