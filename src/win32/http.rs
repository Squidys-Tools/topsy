//! One https GET through WinHTTP, the system's own client: no TLS stack of our own to ship. It
//! blocks, so call it from a worker thread.

use std::ffi::c_void;

use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_REDIRECTS,
    WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR, w};

/// Each step (resolve, connect, send, receive) gives up after this many milliseconds.
const TIMEOUT_MS: i32 = 5_000;

pub enum Response {
    Body(Vec<u8>),
    /// A 3xx, with its `Location`; redirects aren't followed here, so the caller can vet them.
    Redirect(String),
    /// The site answered, with nothing for us.
    Missing,
    /// It couldn't be reached.
    Failed,
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn open(handle: *mut c_void) -> Option<Handle> {
    (!handle.is_null()).then_some(Handle(handle))
}

/// GET `https://{host}{path}`, keeping at most the first `limit` bytes of the body.
pub fn get(host: &str, path: &str, limit: usize) -> Response {
    unsafe { request(host, path, limit) }.unwrap_or(Response::Failed)
}

unsafe fn request(host: &str, path: &str, limit: usize) -> Option<Response> {
    unsafe {
        let session = open(WinHttpOpen(
            w!("topsy"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS).ok()?;
        let connect = open(WinHttpConnect(
            session.0,
            &HSTRING::from(host),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ))?;
        let request = open(WinHttpOpenRequest(
            connect.0,
            w!("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let disable = WINHTTP_DISABLE_REDIRECTS.to_le_bytes();
        WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_DISABLE_FEATURE,
            Some(&disable),
        )
        .ok()?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;
        let mut status = 0u32;
        let mut len = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
        .ok()?;
        match status {
            200 => {}
            300..400 => return Some(Response::Redirect(location(request.0)?)),
            _ => return Some(Response::Missing),
        }
        let mut body = Vec::new();
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as u32,
                &mut read,
            )
            .ok()?;
            if read == 0 {
                return Some(Response::Body(body));
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() >= limit {
                body.truncate(limit);
                return Some(Response::Body(body));
            }
        }
    }
}

unsafe fn location(request: *mut c_void) -> Option<String> {
    let mut buffer = [0u16; 2048];
    let mut len = size_of_val(&buffer) as u32;
    unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_LOCATION,
            PCWSTR::null(),
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buffer[..len as usize / 2]))
}
