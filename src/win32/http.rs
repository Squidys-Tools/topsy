//! https GETs through WinHTTP, the system's own client: no TLS stack of our own to ship. They
//! block, so call them from a worker thread.

use std::ffi::c_void;
use std::io;

use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_REDIRECTS,
    WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_CONTENT_RANGE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE,
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR, w};

/// Each step (resolve, connect, send, receive) gives up after this many milliseconds.
const TIMEOUT_MS: i32 = 5_000;
/// The same for a large download, which may wait longer on a slow line between pieces.
const STREAM_TIMEOUT_MS: i32 = 30_000;

pub enum Response {
    Body(Vec<u8>),
    /// A 3xx, with its `Location`; redirects aren't followed here, so the caller can vet them.
    Redirect(String),
    /// The site answered, with nothing for us.
    Missing,
    /// It couldn't be reached, or was busy.
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

/// An open request whose response headers have arrived; the handles close in reverse order.
struct Exchange {
    request: Handle,
    _connect: Handle,
    _session: Handle,
    status: u32,
}

unsafe fn send(host: &str, path: &str, headers: Option<&str>, timeout: i32) -> Option<Exchange> {
    unsafe {
        let session = open(WinHttpOpen(
            w!("topsy"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, timeout, timeout, timeout, timeout).ok()?;
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
        let headers: Option<Vec<u16>> = headers.map(|h| h.encode_utf16().collect());
        WinHttpSendRequest(request.0, headers.as_deref(), None, 0, 0, 0).ok()?;
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
        Some(Exchange {
            request,
            _connect: connect,
            _session: session,
            status,
        })
    }
}

/// Reads the next piece of the body into `chunk`; 0 at the end.
unsafe fn read(request: *mut c_void, chunk: &mut [u8]) -> Option<usize> {
    let mut read = 0u32;
    unsafe {
        WinHttpReadData(
            request,
            chunk.as_mut_ptr() as *mut c_void,
            chunk.len() as u32,
            &mut read,
        )
    }
    .ok()?;
    Some(read as usize)
}

unsafe fn request(host: &str, path: &str, limit: usize) -> Option<Response> {
    unsafe {
        let exchange = send(host, path, None, TIMEOUT_MS)?;
        match exchange.status {
            200 => {}
            300..400 => return Some(Response::Redirect(location(exchange.request.0)?)),
            // Busy or broken for now: worth asking again.
            408 | 429 | 500.. => return Some(Response::Failed),
            _ => return Some(Response::Missing),
        }
        let mut body = Vec::new();
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            let read = read(exchange.request.0, &mut chunk)?;
            if read == 0 {
                return Some(Response::Body(body));
            }
            body.extend_from_slice(&chunk[..read]);
            if body.len() >= limit {
                body.truncate(limit);
                return Some(Response::Body(body));
            }
        }
    }
}

/// The answer to a streamed GET, before its body.
pub struct Head {
    pub status: u32,
    /// `Content-Range`, on a 206.
    pub range: Option<String>,
}

pub enum Streamed {
    /// A 3xx, with its `Location`, unfollowed.
    Redirect(String),
    /// Anything else: `head` saw it and `body` had all of the body it accepted.
    Done,
}

/// GET `https://{host}{path}` from byte `from` on (a `Range` request when it isn't 0), for files
/// too large to hold. `head` sees the status first and may refuse the body with an error; `body`
/// then takes it a piece at a time, and its error stops the transfer. A broken connection is an
/// error too, so the caller can resume from what it kept.
pub fn stream(
    host: &str,
    path: &str,
    from: u64,
    head: &mut dyn FnMut(&Head) -> io::Result<()>,
    body: &mut dyn FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<Streamed> {
    let lost = || {
        io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "The download was interrupted",
        )
    };
    let range = (from > 0).then(|| format!("Range: bytes={from}-\r\n"));
    let exchange =
        unsafe { send(host, path, range.as_deref(), STREAM_TIMEOUT_MS) }.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotConnected,
                "Couldn't reach the download server",
            )
        })?;
    if (300..400).contains(&exchange.status) {
        let location = unsafe { location(exchange.request.0) }.ok_or_else(lost)?;
        return Ok(Streamed::Redirect(location));
    }
    let range = (exchange.status == 206)
        .then(|| unsafe { text_header(exchange.request.0, WINHTTP_QUERY_CONTENT_RANGE) })
        .flatten();
    head(&Head {
        status: exchange.status,
        range,
    })?;
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        match unsafe { read(exchange.request.0, &mut chunk) }.ok_or_else(lost)? {
            0 => return Ok(Streamed::Done),
            n => body(&chunk[..n])?,
        }
    }
}

unsafe fn text_header(request: *mut c_void, which: u32) -> Option<String> {
    let mut buffer = [0u16; 256];
    let mut len = size_of_val(&buffer) as u32;
    unsafe {
        WinHttpQueryHeaders(
            request,
            which,
            PCWSTR::null(),
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buffer[..len as usize / 2]))
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
