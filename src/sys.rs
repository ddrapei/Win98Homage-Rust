//! The Win32 declarations the program uses, and nothing else.  Written out by
//! hand, so the crate has no run-time dependencies and builds in seconds:
//! every one of these is a stable, documented ABI.
//!
//! Only user32, gdi32, kernel32, shell32 and advapi32 are imported outright.
//! DirectWrite and the session API are found with GetProcAddress, and WinHTTP
//! is only ever loaded by the short-lived child that fetches the weather.

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms, dead_code)]

use core::ffi::c_void;

pub type HWND = isize;
pub type HDC = isize;
pub type HANDLE = isize;
pub type HMENU = isize;
pub type HICON = isize;
pub type HGDIOBJ = isize;
pub type HINSTANCE = isize;
pub type HKEY = isize;
pub type BOOL = i32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type HRESULT = i32;
pub type PCWSTR = *const u16;

pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
pub type WINEVENTPROC = unsafe extern "system" fn(isize, u32, HWND, i32, i32, u32, u32);

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: Option<WNDPROC>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: isize,
    pub hbrBackground: isize,
    pub lpszMenuName: PCWSTR,
    pub lpszClassName: PCWSTR,
    pub hIconSm: HICON,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}

#[repr(C)]
pub struct WINDOWPOS {
    pub hwnd: HWND,
    pub hwndInsertAfter: HWND,
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
    pub flags: u32,
}

#[repr(C)]
pub struct BLENDFUNCTION {
    pub BlendOp: u8,
    pub BlendFlags: u8,
    pub SourceConstantAlpha: u8,
    pub AlphaFormat: u8,
}

#[repr(C)]
pub struct UPDATELAYEREDWINDOWINFO {
    pub cbSize: u32,
    pub hdcDst: HDC,
    pub pptDst: *const POINT,
    pub psize: *const SIZE,
    pub hdcSrc: HDC,
    pub pptSrc: *const POINT,
    pub crKey: u32,
    pub pblend: *const BLENDFUNCTION,
    pub dwFlags: u32,
    pub prcDirty: *const RECT,
}

#[repr(C)]
#[derive(Default)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32,
    pub biWidth: i32,
    pub biHeight: i32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: u32,
    pub biSizeImage: u32,
    pub biXPelsPerMeter: i32,
    pub biYPelsPerMeter: i32,
    pub biClrUsed: u32,
    pub biClrImportant: u32,
}

#[repr(C)]
pub struct TRACKMOUSEEVENT {
    pub cbSize: u32,
    pub dwFlags: u32,
    pub hwndTrack: HWND,
    pub dwHoverTime: u32,
}

#[repr(C)]
pub struct MONITORINFO {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GUID {
    pub a: u32,
    pub b: u16,
    pub c: u16,
    pub d: [u8; 8],
}

#[repr(C)]
pub struct NOTIFYICONDATAW {
    pub cbSize: u32,
    pub hWnd: HWND,
    pub uID: u32,
    pub uFlags: u32,
    pub uCallbackMessage: u32,
    pub hIcon: HICON,
    pub szTip: [u16; 128],
    pub dwState: u32,
    pub dwStateMask: u32,
    pub szInfo: [u16; 256],
    pub uVersion: u32,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: u32,
    pub guidItem: GUID,
    pub hBalloonIcon: HICON,
}

#[repr(C)]
pub struct ICONINFO {
    pub fIcon: BOOL,
    pub xHotspot: u32,
    pub yHotspot: u32,
    pub hbmMask: HGDIOBJ,
    pub hbmColor: HGDIOBJ,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FILETIME {
    pub lo: u32,
    pub hi: u32,
}

#[repr(C)]
pub struct TIME_ZONE_INFORMATION {
    pub Bias: i32,
    pub StandardName: [u16; 32],
    pub StandardDate: SYSTEMTIME,
    pub StandardBias: i32,
    pub DaylightName: [u16; 32],
    pub DaylightDate: SYSTEMTIME,
    pub DaylightBias: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct MEMORYSTATUSEX {
    pub dwLength: u32,
    pub dwMemoryLoad: u32,
    pub ullTotalPhys: u64,
    pub ullAvailPhys: u64,
    pub ullTotalPageFile: u64,
    pub ullAvailPageFile: u64,
    pub ullTotalVirtual: u64,
    pub ullAvailVirtual: u64,
    pub ullAvailExtendedVirtual: u64,
}

/// WTSINFOEXW at level 1: the lock state and the sign-in time in one query.
#[repr(C)]
pub struct WTSINFOEXW {
    pub Level: u32,
    /// The level's data is a union holding 64-bit times, so it starts on an
    /// eight-byte boundary.
    pub _align: u32,
    pub SessionId: u32,
    pub SessionState: i32,
    pub SessionFlags: i32,
    pub WinStationName: [u16; 33],
    pub UserName: [u16; 21],
    pub DomainName: [u16; 18],
    pub LogonTime: i64,
    pub ConnectTime: i64,
    pub DisconnectTime: i64,
    pub LastInputTime: i64,
    pub CurrentTime: i64,
}

// ---- constants --------------------------------------------------------------

pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_DISABLED: u32 = 0x0800_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const CS_DBLCLKS: u32 = 0x0008;

pub const ES_MULTILINE: u32 = 0x0004;
pub const ES_AUTOVSCROLL: u32 = 0x0040;
pub const ES_WANTRETURN: u32 = 0x1000;

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUERYENDSESSION: u32 = 0x0011;
pub const WM_ENDSESSION: u32 = 0x0016;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SETTINGCHANGE: u32 = 0x001A;
pub const WM_MOUSEACTIVATE: u32 = 0x0021;
pub const WM_SETFONT: u32 = 0x0030;
pub const WM_WINDOWPOSCHANGING: u32 = 0x0046;
pub const WM_DISPLAYCHANGE: u32 = 0x007E;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_CHAR: u32 = 0x0102;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_CTLCOLOREDIT: u32 = 0x0133;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_CAPTURECHANGED: u32 = 0x0215;
pub const WM_POWERBROADCAST: u32 = 0x0218;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
pub const WM_APP: u32 = 0x8000;
pub const WM_NULL: u32 = 0;

pub const WA_INACTIVE: usize = 0;
pub const MA_NOACTIVATE: isize = 3;
pub const EN_CHANGE: usize = 0x0300;
pub const VK_ESCAPE: usize = 0x1B;
pub const VK_CONTROL: i32 = 0x11;
pub const VK_SHIFT: i32 = 0x10;

pub const WTS_SESSION_LOCK: usize = 7;
pub const WTS_SESSION_UNLOCK: usize = 8;
pub const WTS_CURRENT_SESSION: u32 = 0xFFFF_FFFF;
pub const WTS_INFO_SESSION_EX: i32 = 25;
pub const WTS_SESSIONSTATE_LOCK: i32 = 0;

pub const PBT_APMSUSPEND: usize = 4;
pub const PBT_APMRESUMEAUTOMATIC: usize = 18;

pub const EM_GETSEL: u32 = 0x00B0;
pub const EM_SETSEL: u32 = 0x00B1;
pub const EM_SETRECTNP: u32 = 0x00B4;
pub const EM_LINESCROLL: u32 = 0x00B6;
pub const EM_GETMODIFY: u32 = 0x00B8;
pub const EM_SETMODIFY: u32 = 0x00B9;
pub const EM_LINEINDEX: u32 = 0x00BB;
pub const EM_REPLACESEL: u32 = 0x00C2;
pub const EM_SETLIMITTEXT: u32 = 0x00C5;
pub const EM_LINEFROMCHAR: u32 = 0x00C9;
pub const EM_EMPTYUNDOBUFFER: u32 = 0x00CD;
pub const EM_GETFIRSTVISIBLELINE: u32 = 0x00CE;
pub const EM_SETMARGINS: u32 = 0x00D3;
pub const EM_CHARFROMPOS: u32 = 0x00D7;
pub const EC_LEFTMARGIN: usize = 0x0001;
pub const EC_RIGHTMARGIN: usize = 0x0002;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const SW_SHOWNORMAL: i32 = 1;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const SWP_NOOWNERZORDER: u32 = 0x0200;
pub const SWP_NOSENDCHANGING: u32 = 0x0400;
pub const HWND_TOP: HWND = 0;
pub const HWND_BOTTOM: HWND = 1;
pub const HWND_TOPMOST: HWND = -1;
pub const HWND_NOTOPMOST: HWND = -2;
pub const GWLP_WNDPROC: i32 = -4;
pub const GWLP_USERDATA: i32 = -21;
pub const GWL_EXSTYLE: i32 = -20;
pub const GW_HWNDPREV: u32 = 3;
pub const IDC_ARROW: usize = 32512;
pub const IDC_IBEAM: usize = 32513;

pub const ULW_ALPHA: u32 = 2;
pub const AC_SRC_ALPHA: u8 = 1;
pub const TME_LEAVE: u32 = 2;
pub const WHEEL_DELTA: i32 = 120;

pub const SM_CXDRAG: i32 = 68;
pub const SM_CYDRAG: i32 = 69;
pub const SM_XVIRTUALSCREEN: i32 = 76;
pub const SM_YVIRTUALSCREEN: i32 = 77;
pub const SM_CXVIRTUALSCREEN: i32 = 78;
pub const SM_CYVIRTUALSCREEN: i32 = 79;
pub const MONITOR_DEFAULTTONEAREST: u32 = 2;

pub const MF_STRING: u32 = 0x0000;
pub const MF_GRAYED: u32 = 0x0001;
pub const MF_CHECKED: u32 = 0x0008;
pub const MF_POPUP: u32 = 0x0010;
pub const MF_SEPARATOR: u32 = 0x0800;
pub const TPM_RIGHTBUTTON: u32 = 0x0002;
pub const TPM_RETURNCMD: u32 = 0x0100;
pub const TPM_NONOTIFY: u32 = 0x0080;

pub const MB_OK: u32 = 0;
pub const MB_ICONINFORMATION: u32 = 0x40;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIF_MESSAGE: u32 = 1;
pub const NIF_ICON: u32 = 2;
pub const NIF_TIP: u32 = 4;

pub const EVENT_SYSTEM_FOREGROUND: u32 = 3;
pub const WINEVENT_OUTOFCONTEXT: u32 = 0;
pub const WINEVENT_SKIPOWNPROCESS: u32 = 2;

pub const FW_NORMAL: i32 = 400;
pub const DEFAULT_CHARSET: u32 = 1;
pub const NONANTIALIASED_QUALITY: u32 = 3;
pub const DIB_RGB_COLORS: u32 = 0;

pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as i32 as isize;
pub const KEY_READ: u32 = 0x20019;
pub const KEY_SET_VALUE: u32 = 0x0002;
pub const REG_SZ: u32 = 1;

pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const TIME_ZONE_ID_DAYLIGHT: u32 = 2;

/// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
pub const DPI_PER_MONITOR_V2: isize = -4;

// ---- functions --------------------------------------------------------------

#[link(name = "user32")]
unsafe extern "system" {
    pub fn RegisterClassExW(wc: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32, class: PCWSTR, title: PCWSTR, style: u32,
        x: i32, y: i32, w: i32, h: i32,
        parent: HWND, menu: HMENU, instance: HINSTANCE, param: *mut c_void,
    ) -> HWND;
    pub fn DestroyWindow(wnd: HWND) -> BOOL;
    pub fn DefWindowProcW(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT;
    pub fn CallWindowProcW(prev: isize, wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT;
    pub fn GetMessageW(msg: *mut MSG, wnd: HWND, min: u32, max: u32) -> BOOL;
    pub fn TranslateMessage(msg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
    pub fn PostMessageW(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> BOOL;
    pub fn SendMessageW(wnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT;
    pub fn RegisterWindowMessageW(name: PCWSTR) -> u32;
    pub fn SetWindowLongPtrW(wnd: HWND, index: i32, value: isize) -> isize;
    pub fn GetWindowLongPtrW(wnd: HWND, index: i32) -> isize;
    pub fn SetWindowPos(wnd: HWND, after: HWND, x: i32, y: i32, w: i32, h: i32, flags: u32) -> BOOL;
    pub fn MoveWindow(wnd: HWND, x: i32, y: i32, w: i32, h: i32, repaint: BOOL) -> BOOL;
    pub fn ShowWindow(wnd: HWND, cmd: i32) -> BOOL;
    pub fn IsWindow(wnd: HWND) -> BOOL;
    pub fn IsWindowVisible(wnd: HWND) -> BOOL;
    pub fn GetWindowRect(wnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn GetClientRect(wnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn GetWindow(wnd: HWND, cmd: u32) -> HWND;
    pub fn GetShellWindow() -> HWND;
    pub fn FindWindowExW(parent: HWND, after: HWND, class: PCWSTR, title: PCWSTR) -> HWND;
    pub fn GetClassNameW(wnd: HWND, buf: *mut u16, len: i32) -> i32;
    pub fn GetWindowThreadProcessId(wnd: HWND, pid: *mut u32) -> u32;
    pub fn SetForegroundWindow(wnd: HWND) -> BOOL;
    pub fn SetFocus(wnd: HWND) -> HWND;
    pub fn SetCapture(wnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> BOOL;
    pub fn GetCursorPos(point: *mut POINT) -> BOOL;
    pub fn ScreenToClient(wnd: HWND, point: *mut POINT) -> BOOL;
    pub fn GetKeyState(key: i32) -> i16;
    pub fn SetTimer(wnd: HWND, id: usize, ms: u32, proc_: *const c_void) -> usize;
    pub fn KillTimer(wnd: HWND, id: usize) -> BOOL;
    pub fn LoadCursorW(instance: HINSTANCE, name: PCWSTR) -> isize;
    pub fn UpdateLayeredWindowIndirect(wnd: HWND, info: *const UPDATELAYEREDWINDOWINFO) -> BOOL;
    pub fn TrackMouseEvent(tme: *mut TRACKMOUSEEVENT) -> BOOL;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn MonitorFromRect(rect: *const RECT, flags: u32) -> isize;
    pub fn GetMonitorInfoW(monitor: isize, info: *mut MONITORINFO) -> BOOL;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(menu: HMENU, flags: u32, id: usize, text: PCWSTR) -> BOOL;
    pub fn TrackPopupMenu(menu: HMENU, flags: u32, x: i32, y: i32, reserved: i32, wnd: HWND, rect: *const RECT) -> BOOL;
    pub fn DestroyMenu(menu: HMENU) -> BOOL;
    pub fn MessageBoxW(wnd: HWND, text: PCWSTR, caption: PCWSTR, kind: u32) -> i32;
    pub fn SetWinEventHook(min: u32, max: u32, module: HINSTANCE, proc_: WINEVENTPROC, pid: u32, tid: u32, flags: u32) -> isize;
    pub fn UnhookWinEvent(hook: isize) -> BOOL;
    pub fn CreateIconIndirect(info: *const ICONINFO) -> HICON;
    pub fn DestroyIcon(icon: HICON) -> BOOL;
    pub fn GetWindowTextLengthW(wnd: HWND) -> i32;
    pub fn GetWindowTextW(wnd: HWND, buf: *mut u16, len: i32) -> i32;
    pub fn SetWindowTextW(wnd: HWND, text: PCWSTR) -> BOOL;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn DeleteDC(dc: HDC) -> BOOL;
    pub fn CreateDIBSection(dc: HDC, info: *const BITMAPINFOHEADER, usage: u32, bits: *mut *mut c_void, section: HANDLE, offset: u32) -> HGDIOBJ;
    pub fn CreateBitmap(w: i32, h: i32, planes: u32, bpp: u32, bits: *const c_void) -> HGDIOBJ;
    pub fn SelectObject(dc: HDC, object: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(object: HGDIOBJ) -> BOOL;
    pub fn CreateFontW(
        height: i32, width: i32, escapement: i32, orientation: i32, weight: i32,
        italic: u32, underline: u32, strike: u32, charset: u32, out_precision: u32,
        clip_precision: u32, quality: u32, pitch_and_family: u32, face: PCWSTR,
    ) -> HGDIOBJ;
    pub fn CreateSolidBrush(colour: u32) -> HGDIOBJ;
    pub fn SetTextColor(dc: HDC, colour: u32) -> u32;
    pub fn SetBkColor(dc: HDC, colour: u32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(name: PCWSTR) -> HINSTANCE;
    pub fn LoadLibraryW(name: PCWSTR) -> HINSTANCE;
    pub fn FreeLibrary(module: HINSTANCE) -> BOOL;
    pub fn GetProcAddress(module: HINSTANCE, name: *const u8) -> *const c_void;
    pub fn GetLastError() -> u32;
    pub fn CreateMutexW(security: *const c_void, initial_owner: BOOL, name: PCWSTR) -> HANDLE;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn SetProcessWorkingSetSize(process: HANDLE, min: usize, max: usize) -> BOOL;
    pub fn GetSystemTimeAsFileTime(ft: *mut FILETIME);
    pub fn FileTimeToSystemTime(ft: *const FILETIME, st: *mut SYSTEMTIME) -> BOOL;
    pub fn SystemTimeToFileTime(st: *const SYSTEMTIME, ft: *mut FILETIME) -> BOOL;
    pub fn SystemTimeToTzSpecificLocalTime(tz: *const c_void, utc: *const SYSTEMTIME, local: *mut SYSTEMTIME) -> BOOL;
    pub fn TzSpecificLocalTimeToSystemTime(tz: *const c_void, local: *const SYSTEMTIME, utc: *mut SYSTEMTIME) -> BOOL;
    pub fn GetTimeZoneInformation(tz: *mut TIME_ZONE_INFORMATION) -> u32;
    pub fn GetSystemTimes(idle: *mut FILETIME, kernel: *mut FILETIME, user: *mut FILETIME) -> BOOL;
    pub fn GlobalMemoryStatusEx(status: *mut MEMORYSTATUSEX) -> BOOL;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn Shell_NotifyIconW(message: u32, data: *const NOTIFYICONDATAW) -> BOOL;
    pub fn ShellExecuteW(wnd: HWND, verb: PCWSTR, file: PCWSTR, params: PCWSTR, dir: PCWSTR, show: i32) -> isize;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    pub fn RegOpenKeyExW(key: HKEY, sub: PCWSTR, options: u32, access: u32, out: *mut HKEY) -> i32;
    pub fn RegQueryValueExW(key: HKEY, name: PCWSTR, reserved: *const u32, kind: *mut u32, data: *mut u8, len: *mut u32) -> i32;
    pub fn RegSetValueExW(key: HKEY, name: PCWSTR, reserved: u32, kind: u32, data: *const u8, len: u32) -> i32;
    pub fn RegDeleteValueW(key: HKEY, name: PCWSTR) -> i32;
    pub fn RegCloseKey(key: HKEY) -> i32;
}

// ---- small helpers ----------------------------------------------------------

/// `s` as UTF-16 with a terminating NUL, ready to pass as an LPCWSTR.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// A NUL-terminated (or full) UTF-16 buffer as a String.
pub fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// A function from a DLL that is not imported outright.  `name` must end in NUL.
pub unsafe fn proc_address(dll: &str, name: &'static [u8]) -> Option<*const c_void> {
    debug_assert!(name.ends_with(&[0]));
    unsafe {
        let mut module = GetModuleHandleW(wide(dll).as_ptr());
        if module == 0 {
            module = LoadLibraryW(wide(dll).as_ptr());
        }
        if module == 0 {
            return None;
        }
        let p = GetProcAddress(module, name.as_ptr());
        (!p.is_null()).then_some(p)
    }
}

/// Runs `f` with this thread's windows per-monitor DPI aware, so that a menu
/// or a message box it puts up is drawn sharp instead of stretched.  The
/// widgets themselves stay unaware on purpose -- see `host`.
pub fn with_sharp_dpi<R>(f: impl FnOnce() -> R) -> R {
    type SetCtx = unsafe extern "system" fn(isize) -> isize;
    unsafe {
        let set: Option<SetCtx> = proc_address("user32.dll", b"SetThreadDpiAwarenessContext\0")
            .map(|p| core::mem::transmute::<*const c_void, SetCtx>(p));
        let old = set.map(|s| s(DPI_PER_MONITOR_V2)).unwrap_or(0);
        let r = f();
        if let (Some(s), true) = (set, old != 0) {
            s(old);
        }
        r
    }
}

pub fn lo_word(v: usize) -> i32 {
    (v & 0xFFFF) as i32
}

pub fn hi_word(v: usize) -> i32 {
    ((v >> 16) & 0xFFFF) as i32
}

/// The signed x of a mouse message's lParam.
pub fn x_of(lp: LPARAM) -> i32 {
    (lp & 0xFFFF) as i16 as i32
}

pub fn y_of(lp: LPARAM) -> i32 {
    ((lp >> 16) & 0xFFFF) as i16 as i32
}
