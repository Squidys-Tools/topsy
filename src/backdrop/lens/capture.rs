//! What is behind the notch, as a texture: a second live copy shown in a helper window beside
//! every screen, captured with Windows.Graphics.Capture.

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HWND, RECT, SIZE};
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionTarget, IDCompositionVisual, IDCompositionVisual2,
};
use windows::Win32::Graphics::Dwm::DwmUnregisterThumbnail;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetWindowPos, ShowWindow, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{IInspectable, Interface, w};

use super::super::live::Shared;
use crate::log::LogErr;

/// The least time between captured frames, in 100 ns units: 10 a second.
const FRAME_EVERY: i64 = 1_000_000;

pub(super) fn virtual_screen() -> RECT {
    unsafe {
        let (left, top) = (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
        );
        RECT {
            left,
            top,
            right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// The helper window and the copy it shows.
pub(super) struct Helper {
    window: HWND,
    _target: IDCompositionTarget,
    _copy: IDCompositionVisual2,
    thumbnail: isize,
    /// The region the copy was last aimed at.
    aimed: Option<RECT>,
}

impl Drop for Helper {
    fn drop(&mut self) {
        unsafe { DwmUnregisterThumbnail(self.thumbnail) }.log_err();
        unsafe { DestroyWindow(self.window) }.log_err();
    }
}

impl Helper {
    pub(super) fn new(shared: &Shared) -> windows::core::Result<Self> {
        unsafe {
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TRANSPARENT,
                w!("STATIC"),
                w!("topsy lens"),
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
            let target = device.CreateTargetForHwnd(window, true)?;
            let mut raw = std::ptr::null_mut();
            let mut thumbnail = 0;
            (shared.create)(shared.host, device.as_raw(), &mut raw, &mut thumbnail).ok()?;
            let copy = IDCompositionVisual2::from_raw(raw);
            let root = device.CreateVisual()?;
            root.AddVisual(&copy, false, None::<&IDCompositionVisual>)?;
            target.SetRoot(&root)?;
            Ok(Self {
                window,
                _target: target,
                _copy: copy,
                thumbnail,
                aimed: None,
            })
        }
    }

    /// Make the copy just `region` of the screen, leaving the island out. Copying no more than
    /// that also halves what the capture costs DWM.
    pub(super) fn aim(
        &mut self,
        shared: &Shared,
        island: HWND,
        region: RECT,
    ) -> windows::core::Result<()> {
        if self.aimed == Some(region) {
            return Ok(());
        }
        let excluded = [island, self.window, shared.host];
        unsafe {
            let size = SIZE {
                cx: region.right - region.left,
                cy: region.bottom - region.top,
            };
            (shared.update)(
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
            shared.device.Commit()?;
        }
        self.aimed = Some(region);
        Ok(())
    }

    /// Show the helper beside every screen and start capturing it; each frame pings `arrived`.
    pub(super) fn capture(
        &self,
        device: &IDirect3DDevice,
        size: (i32, i32),
        virtual_screen: RECT,
        arrived: async_channel::Sender<()>,
    ) -> windows::core::Result<Capture> {
        unsafe {
            SetWindowPos(
                self.window,
                None,
                virtual_screen.right + size.0,
                virtual_screen.top,
                size.0,
                size.1,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            )?;
            let interop =
                windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = interop.CreateForWindow(self.window)?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                device,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                SizeInt32 {
                    Width: size.0,
                    Height: size.1,
                },
            )?;
            pool.FrameArrived(
                &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |_, _| {
                    // Full means a wake-up is already on its way, which drains every frame.
                    arrived.try_send(()).ok();
                    Ok(())
                }),
            )?;
            let session = pool.CreateCaptureSession(&item)?;
            // A live copy is new on every composition, so frames come as fast as DWM composes,
            // even when nothing behind has changed. Only the rim shows them: a few a second do.
            (session.SetMinUpdateInterval(TimeSpan {
                Duration: FRAME_EVERY,
            }))
            .log_err();
            session.StartCapture()?;
            Ok(Capture { pool, session })
        }
    }

    pub(super) fn hide(&self) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "returns whether it was visible before, not an error"
        )]
        let _ = unsafe { ShowWindow(self.window, SW_HIDE) };
    }
}

pub(super) struct Capture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.session.Close().log_err();
        self.pool.Close().log_err();
    }
}

impl Capture {
    /// The newest frame waiting, dropping the older ones.
    pub(super) fn newest(&self) -> Option<Direct3D11CaptureFrame> {
        let mut newest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            newest = Some(frame);
        }
        newest
    }
}
