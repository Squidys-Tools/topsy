//! DWM's live copy of the desktop behind the window, blurred and saturated by DirectComposition.
//!
//! dwmapi's undocumented "shared multi-window visual" (ordinals 163 and 164, the shapes VLC uses)
//! hands us a visual of every window on the virtual screen but the ones we exclude, updated by DWM
//! as they change. It goes in a non-topmost DirectComposition target under GPUI's topmost one,
//! shifted so the part behind the window lines up with it. Unlike the host backdrop, its blur is
//! ours to choose. DWM does the work; topsy only moves the visual when the window moves.

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;

use windows::Win32::Foundation::{HMODULE, HWND, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::D2D1_BORDER_MODE_HARD;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice3, IDCompositionDesktopDevice, IDCompositionDevice3,
    IDCompositionGaussianBlurEffect, IDCompositionRectangleClip, IDCompositionTarget,
    IDCompositionVisual, IDCompositionVisual2,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute, DwmUnregisterThumbnail,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_SHOWNOACTIVATE, ShowWindow, WINDOW_STYLE, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, Error, HRESULT, IUnknown, Interface, PCSTR, s, w};

use super::{Outline, SATURATION};
use crate::log::LogErr;

/// The blur's standard deviation in logical pixels: light enough that what is behind stays
/// recognisable through the glass, as it does through Droppy's.
const BLUR: f32 = 10.0;
/// `DWM_CLOAKED_APP`.
const CLOAKED_APP: u32 = 2;
/// `WCA_EXCLUDED_FROM_LIVEPREVIEW`, for `SetWindowCompositionAttribute`.
const WCA_EXCLUDED_FROM_LIVEPREVIEW: u32 = 13;

type CreateSharedMultiWindowVisual =
    unsafe extern "system" fn(HWND, *mut c_void, *mut *mut c_void, *mut isize) -> HRESULT;
type UpdateSharedMultiWindowVisual = unsafe extern "system" fn(
    isize,
    *const HWND,
    u32,
    *const HWND,
    u32,
    *const RECT,
    *const SIZE,
    u32,
) -> HRESULT;

#[repr(C)]
struct CompositionAttribute {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}
type SetWindowCompositionAttribute =
    unsafe extern "system" fn(HWND, *mut CompositionAttribute) -> BOOL;

/// The device and the entry points every window's glass shares, made on first use.
#[derive(Clone)]
struct Shared {
    device: IDCompositionDesktopDevice,
    create: CreateSharedMultiWindowVisual,
    update: UpdateSharedMultiWindowVisual,
    /// The visual must be made for a window with live preview turned off; a cloaked window of
    /// its own keeps that off the island.
    host: HWND,
}

impl gpui::Global for Shared {}

/// An exported function of `module` by ordinal or name, as `F`.
///
/// # Safety
/// `F` must be the function's real signature.
unsafe fn export<F: Copy>(module: HMODULE, name: PCSTR) -> windows::core::Result<F> {
    let found = unsafe { GetProcAddress(module, name) }.ok_or_else(Error::from_thread)?;
    // SAFETY: a function pointer to a function pointer of the caller's stated signature.
    Ok(unsafe { std::mem::transmute_copy(&found) })
}

fn shared(hwnd: HWND, cx: &mut gpui::App) -> windows::core::Result<Shared> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return Ok(shared.clone());
    }
    unsafe {
        // Ordinal 164 took one argument fewer before build 20000. The host backdrop attribute
        // arrived after it (Windows 11), so accepting it proves the newer signature.
        let enable = BOOL::from(true);
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            &enable as *const _ as *const c_void,
            std::mem::size_of_val(&enable) as u32,
        )?;
        let dwmapi = LoadLibraryW(w!("dwmapi.dll"))?;
        let user32 = LoadLibraryW(w!("user32.dll"))?;
        let create = export(dwmapi, PCSTR(163 as *const u8))?;
        let update = export(dwmapi, PCSTR(164 as *const u8))?;
        let set_attribute: SetWindowCompositionAttribute =
            export(user32, s!("SetWindowCompositionAttribute"))?;

        let host = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("STATIC"),
            w!("topsy glass"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )?;
        DwmSetWindowAttribute(
            host,
            DWMWA_CLOAK,
            &CLOAKED_APP as *const _ as *const c_void,
            std::mem::size_of_val(&CLOAKED_APP) as u32,
        )?;
        #[expect(
            clippy::let_underscore_must_use,
            reason = "returns whether it was visible before, not an error"
        )]
        let _ = ShowWindow(host, SW_SHOWNOACTIVATE);
        let mut excluded = BOOL::from(true);
        let mut attribute = CompositionAttribute {
            attribute: WCA_EXCLUDED_FROM_LIVEPREVIEW,
            data: &mut excluded as *mut _ as *mut c_void,
            size: std::mem::size_of_val(&excluded),
        };
        set_attribute(host, &mut attribute).ok()?;

        let device: IDCompositionDesktopDevice = DCompositionCreateDevice3(None::<&IUnknown>)?;
        let shared = Shared {
            device,
            create,
            update,
            host,
        };
        cx.set_global(shared.clone());
        Ok(shared)
    }
}

/// The live glass of one window. Lives on the UI thread.
pub struct Live {
    _target: IDCompositionTarget,
    shape: LiveShape,
}

impl Live {
    pub fn new(hwnd: HWND, cx: &mut gpui::App) -> windows::core::Result<Self> {
        let shared = shared(hwnd, cx)?;
        unsafe {
            let device = &shared.device;
            let target = device.CreateTargetForHwnd(hwnd, false)?;
            let root = device.CreateVisual()?;
            let mut raw = std::ptr::null_mut();
            let mut thumbnail = 0;
            (shared.create)(shared.host, device.as_raw(), &mut raw, &mut thumbnail).ok()?;
            let copy = IDCompositionVisual2::from_raw(raw);

            let effects: IDCompositionDevice3 = device.cast()?;
            let saturation = effects.CreateSaturationEffect()?;
            saturation.SetSaturation2(SATURATION)?;
            let blur = effects.CreateGaussianBlurEffect()?;
            // A hard border keeps the top of the screen from fading into the blur.
            blur.SetBorderMode(D2D1_BORDER_MODE_HARD)?;
            blur.SetInput(0, &saturation, 0)?;
            root.SetEffect(&blur)?;
            let clip = device.CreateRectangleClip()?;
            root.SetClip(&clip)?;
            root.AddVisual(&copy, false, None::<&IDCompositionVisual>)?;
            target.SetRoot(&root)?;

            Ok(Self {
                _target: target,
                shape: LiveShape(Rc::new(Parts {
                    device: device.clone(),
                    update: shared.update,
                    excluded: [hwnd, shared.host],
                    thumbnail,
                    copy,
                    clip,
                    blur,
                    shown: Cell::new(None),
                })),
            })
        }
    }

    pub fn shape(&self) -> LiveShape {
        self.shape.clone()
    }
}

struct Parts {
    device: IDCompositionDesktopDevice,
    update: UpdateSharedMultiWindowVisual,
    excluded: [HWND; 2],
    thumbnail: isize,
    copy: IDCompositionVisual2,
    clip: IDCompositionRectangleClip,
    blur: IDCompositionGaussianBlurEffect,
    /// The virtual screen and scale the copy was last made for.
    shown: Cell<Option<(RECT, f32)>>,
}

impl Drop for Parts {
    fn drop(&mut self) {
        unsafe { DwmUnregisterThumbnail(self.thumbnail) }.log_err();
    }
}

#[derive(Clone)]
pub struct LiveShape(Rc<Parts>);

impl LiveShape {
    pub fn set(&self, origin: (i32, i32), scale: f32, outline: Outline) {
        let parts = &*self.0;
        unsafe {
            let virtual_screen = RECT {
                left: GetSystemMetrics(SM_XVIRTUALSCREEN),
                top: GetSystemMetrics(SM_YVIRTUALSCREEN),
                right: GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN),
                bottom: GetSystemMetrics(SM_YVIRTUALSCREEN) + GetSystemMetrics(SM_CYVIRTUALSCREEN),
            };
            if parts.shown.get() != Some((virtual_screen, scale)) {
                parts.shown.set(Some((virtual_screen, scale)));
                let size = SIZE {
                    cx: virtual_screen.right - virtual_screen.left,
                    cy: virtual_screen.bottom - virtual_screen.top,
                };
                (parts.update)(
                    parts.thumbnail,
                    std::ptr::null(),
                    0,
                    parts.excluded.as_ptr(),
                    parts.excluded.len() as u32,
                    &virtual_screen,
                    &size,
                    1,
                )
                .ok()
                .log_err();
                parts.blur.SetStandardDeviation2(BLUR * scale).log_err();
            }
            let copy = &parts.copy;
            copy.SetOffsetX2((virtual_screen.left - origin.0) as f32)
                .log_err();
            copy.SetOffsetY2((virtual_screen.top - origin.1) as f32)
                .log_err();
            let Outline {
                x,
                y,
                width,
                height,
                radius,
            } = outline;
            let clip = &parts.clip;
            clip.SetLeft2(x).log_err();
            clip.SetTop2(y).log_err();
            clip.SetRight2(x + width).log_err();
            clip.SetBottom2(y + height).log_err();
            for set in [
                IDCompositionRectangleClip::SetTopLeftRadiusX2,
                IDCompositionRectangleClip::SetTopLeftRadiusY2,
                IDCompositionRectangleClip::SetTopRightRadiusX2,
                IDCompositionRectangleClip::SetTopRightRadiusY2,
                IDCompositionRectangleClip::SetBottomLeftRadiusX2,
                IDCompositionRectangleClip::SetBottomLeftRadiusY2,
                IDCompositionRectangleClip::SetBottomRightRadiusX2,
                IDCompositionRectangleClip::SetBottomRightRadiusY2,
            ] {
                set(clip, radius).log_err();
            }
            parts.device.Commit().log_err();
        }
    }
}
