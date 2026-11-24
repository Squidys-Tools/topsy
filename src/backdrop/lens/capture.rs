//! Reading the screen behind the open notch: once with GDI as it opens, or continuously from a
//! live copy for the *Live glass* setting.

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HWND, RECT, SIZE};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionTarget, IDCompositionVisual, IDCompositionVisual2,
};
use windows::Win32::Graphics::Dwm::DwmUnregisterThumbnail;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetSystemMetrics, GetWindowRect, IsWindowVisible,
    SET_WINDOW_POS_FLAGS, SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE,
    SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Error, Interface, factory, w};

use super::super::live::{Shared, track_mirror, untrack_mirror};
use crate::log::LogErr;
use model::float::Rect;

/// The time between frames at `fps` frames a second, in the capture's 100 ns units.
fn interval(fps: u32) -> TimeSpan {
    TimeSpan {
        Duration: 10_000_000 / i64::from(fps.max(1)),
    }
}

/// The screen's pixels in `rect`, as top-down BGRA rows. Includes every window, as a
/// screenshot would.
pub fn snapshot(rect: Rect) -> windows::core::Result<Vec<u8>> {
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let result = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .and_then(|bitmap| {
                let old = SelectObject(memory, bitmap.into());
                let copied = BitBlt(
                    memory,
                    0,
                    0,
                    width,
                    height,
                    Some(screen),
                    rect.left,
                    rect.top,
                    SRCCOPY | CAPTUREBLT,
                )
                .map(|()| {
                    std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize)
                        .to_vec()
                });
                SelectObject(memory, old);
                DeleteObject(bitmap.into()).ok().log_err();
                copied
            });
        DeleteDC(memory).ok().log_err();
        ReleaseDC(None, screen);
        result
    }
}

/// A second live copy of what is behind the notch, shown in a window of its own beside every
/// screen and captured there. The copy leaves the island and the lens out, so nothing of
/// topsy's has to be hidden from screenshots or recordings, as a capture of the screen would
/// need.
pub struct Mirror {
    window: HWND,
    target: IDCompositionTarget,
    copy: IDCompositionVisual2,
    thumbnail: isize,
    shared: Shared,
}

impl Mirror {
    pub fn new(shared: &Shared) -> windows::core::Result<Self> {
        unsafe {
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TRANSPARENT,
                w!("STATIC"),
                w!("topsy mirror"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?;
            let device = &shared.device;
            let mut raw = std::ptr::null_mut();
            let mut thumbnail = 0;
            let made = device.CreateTargetForHwnd(window, true).and_then(|target| {
                (shared.create)(shared.host, device.as_raw(), &mut raw, &mut thumbnail).ok()?;
                Ok(target)
            });
            let target = match made {
                Ok(target) => target,
                Err(e) => {
                    DestroyWindow(window).log_err();
                    return Err(e);
                }
            };
            // From here on, dropping the mirror lets go of the window and the copy.
            track_mirror(window);
            let mirror = Self {
                window,
                target,
                copy: IDCompositionVisual2::from_raw(raw),
                thumbnail,
                shared: shared.clone(),
            };
            let root = device.CreateVisual()?;
            root.AddVisual(&mirror.copy, false, None::<&IDCompositionVisual>)?;
            mirror.target.SetRoot(&root)?;
            Ok(mirror)
        }
    }

    /// Copy `rect` of the screen, leaving `excluded` out. A running capture follows it if it
    /// keeps its size.
    pub fn aim(&self, rect: Rect, excluded: [HWND; 2]) -> windows::core::Result<()> {
        let excluded = [excluded[0], excluded[1], self.window, self.shared.host];
        let region = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        let size = SIZE {
            cx: rect.right - rect.left,
            cy: rect.bottom - rect.top,
        };
        unsafe {
            (self.shared.update)(
                self.thumbnail,
                std::ptr::null(),
                0,
                excluded.as_ptr(),
                excluded.len() as u32,
                &region,
                &size,
                1,
            )
            .ok()?;
            self.shared.device.Commit()
        }
    }

    /// Copy `rect` of the screen, leaving `excluded` out, and capture it at up to `fps` frames a
    /// second, waking `frames` as each frame arrives.
    pub fn capture(
        &self,
        rect: Rect,
        excluded: [HWND; 2],
        device: &IDirect3DDevice,
        frames: async_channel::Sender<()>,
        fps: u32,
    ) -> windows::core::Result<Capture> {
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        self.aim(rect, excluded)?;
        beside_screens(self.window, width, height, SWP_SHOWWINDOW)?;
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(self.window)? };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            SizeInt32 {
                Width: width,
                Height: height,
            },
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        session.SetIsBorderRequired(false)?;
        session.SetIsCursorCaptureEnabled(false)?;
        // A live copy is new on every composition, whether or not anything behind changed, so
        // frames never stop on their own; this caps them.
        session.SetMinUpdateInterval(interval(fps)).log_err();
        // Called on a capture thread: only wake the UI thread, which reads the frame itself.
        pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
            match frames.try_send(()) {
                // Full: the UI thread is already woken and reads the newest frame anyway.
                Ok(()) | Err(async_channel::TrySendError::Full(())) => Ok(()),
                Err(async_channel::TrySendError::Closed(())) => Err(failed()),
            }
        }))?;
        session.StartCapture()?;
        Ok(Capture { pool, session })
    }

    pub fn hide(&self) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "returns whether it was visible before, not an error"
        )]
        let _ = unsafe { ShowWindow(self.window, SW_HIDE) };
    }
}

/// Put a mirror beside every screen: never seen, but composed, so it can be captured.
fn beside_screens(
    window: HWND,
    width: i32,
    height: i32,
    flags: SET_WINDOW_POS_FLAGS,
) -> windows::core::Result<()> {
    unsafe {
        SetWindowPos(
            window,
            None,
            GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) + width,
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER | flags,
        )
    }
}

/// Keep a mirror in use beside the screens after they change, or it may show on a new monitor.
pub fn park(window: HWND) {
    let mut rect = RECT::default();
    unsafe {
        if !IsWindowVisible(window).as_bool() || GetWindowRect(window, &mut rect).is_err() {
            return;
        }
    }
    beside_screens(
        window,
        rect.right - rect.left,
        rect.bottom - rect.top,
        SET_WINDOW_POS_FLAGS(0),
    )
    .log_err();
}

impl Drop for Mirror {
    fn drop(&mut self) {
        untrack_mirror(self.window);
        unsafe { DwmUnregisterThumbnail(self.thumbnail) }.log_err();
        unsafe { DestroyWindow(self.window) }.log_err();
    }
}

/// A running capture of the mirror.
pub struct Capture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Capture {
    /// Follow the screen at up to `fps` frames a second from now on.
    pub fn pace(&self, fps: u32) {
        self.session.SetMinUpdateInterval(interval(fps)).log_err();
    }

    /// Hand the newest frame waiting to `read`, dropping older ones.
    pub fn newest(&self, read: impl FnOnce(&ID3D11Texture2D)) -> windows::core::Result<()> {
        let mut newest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            newest = Some(frame);
        }
        let Some(frame) = newest else {
            return Ok(());
        };
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        read(&texture);
        frame.Close()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.session.Close().log_err();
        self.pool.Close().log_err();
    }
}

/// `E_FAIL`, for a call that reported nothing more specific.
pub fn failed() -> Error {
    Error::from(windows::Win32::Foundation::E_FAIL)
}
