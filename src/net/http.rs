//! One GET over WinHTTP -- which is loaded here and nowhere else, and only
//! ever in a child that exits when it has its answer: see `net`.

use core::ffi::c_void;
use core::ptr::{null, null_mut};

use crate::sys;

/// The body of the answer, if the answer was 200.
pub fn get(host: &str, port: u16, secure: bool, path: &str) -> Option<String> {
    type H = *mut c_void;
    type Open = unsafe extern "system" fn(*const u16, u32, *const u16, *const u16, u32) -> H;
    type SetTimeouts = unsafe extern "system" fn(H, i32, i32, i32, i32) -> i32;
    type Connect = unsafe extern "system" fn(H, *const u16, u16, u32) -> H;
    type OpenRequest = unsafe extern "system" fn(H, *const u16, *const u16, *const u16, *const u16, *const *const u16, u32) -> H;
    type Send = unsafe extern "system" fn(H, *const u16, u32, *const c_void, u32, u32, usize) -> i32;
    type Receive = unsafe extern "system" fn(H, *mut c_void) -> i32;
    type QueryHeaders = unsafe extern "system" fn(H, u32, *const u16, *mut c_void, *mut u32, *mut u32) -> i32;
    type Read = unsafe extern "system" fn(H, *mut u8, u32, *mut u32) -> i32;
    type Close = unsafe extern "system" fn(H) -> i32;

    const FLAG_SECURE: u32 = 0x0080_0000;
    const QUERY_STATUS_NUMBER: u32 = 19 | 0x2000_0000;

    unsafe {
        let f = |name: &'static [u8]| sys::proc_address("winhttp.dll", name);
        let open: Open = core::mem::transmute(f(b"WinHttpOpen\0")?);
        let set_timeouts: SetTimeouts = core::mem::transmute(f(b"WinHttpSetTimeouts\0")?);
        let connect: Connect = core::mem::transmute(f(b"WinHttpConnect\0")?);
        let open_request: OpenRequest = core::mem::transmute(f(b"WinHttpOpenRequest\0")?);
        let send: Send = core::mem::transmute(f(b"WinHttpSendRequest\0")?);
        let receive: Receive = core::mem::transmute(f(b"WinHttpReceiveResponse\0")?);
        let query: QueryHeaders = core::mem::transmute(f(b"WinHttpQueryHeaders\0")?);
        let read: Read = core::mem::transmute(f(b"WinHttpReadData\0")?);
        let close: Close = core::mem::transmute(f(b"WinHttpCloseHandle\0")?);

        // 4 = WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: whatever the system uses
        let session = open(sys::wide("Win98Widgets").as_ptr(), 4, null(), null(), 0);
        if session.is_null() {
            return None;
        }
        set_timeouts(session, 8000, 8000, 8000, 12000);
        let mut body = None;
        let connection = connect(session, sys::wide(host).as_ptr(), port, 0);
        if !connection.is_null() {
            let request = open_request(
                connection, sys::wide("GET").as_ptr(), sys::wide(path).as_ptr(),
                null(), null(), null(), if secure { FLAG_SECURE } else { 0 },
            );
            if !request.is_null() {
                if send(request, null(), 0, null(), 0, 0, 0) != 0 && receive(request, null_mut()) != 0 {
                    let (mut status, mut len) = (0u32, 4u32);
                    query(request, QUERY_STATUS_NUMBER, null(), &mut status as *mut u32 as *mut c_void, &mut len, null_mut());
                    if status == 200 {
                        let mut bytes = Vec::new();
                        let mut chunk = [0u8; 4096];
                        loop {
                            let mut got = 0u32;
                            if read(request, chunk.as_mut_ptr(), chunk.len() as u32, &mut got) == 0 || got == 0 {
                                break;
                            }
                            bytes.extend_from_slice(&chunk[..got as usize]);
                            if bytes.len() > 1 << 20 {
                                break;
                            }
                        }
                        body = Some(String::from_utf8_lossy(&bytes).into_owned());
                    }
                }
                close(request);
            }
            close(connection);
        }
        close(session);
        body
    }
}
