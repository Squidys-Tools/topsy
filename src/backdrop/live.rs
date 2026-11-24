//! DWM's live copy of the desktop behind the window, blurred and saturated by DirectComposition.
//!
//! dwmapi's undocumented "shared multi-window visual" (ordinals 163 and 164, the shapes VLC uses)
//! hands us a visual of every window on the virtual screen but the ones we exclude, updated by DWM
//! as they change. It goes in a non-topmost DirectComposition target under GPUI's topmost one,
//! shifted so the part behind the window lines up with it. Unlike the host backdrop, its blur is
//! ours to choose. A faint fixed grain over it, as on acrylic, keeps large flat areas from
//! banding and reads as material. DWM does the work; topsy only moves the visual when the window
//! moves, or when the displays change and the virtual screen with them.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BORDER_MODE_HARD, D2D1_TURBULENCE_NOISE_FRACTAL_SUM,
};
use windows::Win32::Graphics::DirectComposition::{
    DCOMPOSITION_BORDER_MODE_SOFT, DCompositionCreateDevice3, IDCompositionDesktopDevice,
    IDCompositionDevice3, IDCompositionGaussianBlurEffect, IDCompositionRectangleClip,
    IDCompositionTarget, IDCompositionVisual, IDCompositionVisual2,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute, DwmUnregisterThumbnail,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetSystemMetrics, RegisterClassW, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNOACTIVATE, ShowWindow,
    WINDOW_STYLE, WM_DISPLAYCHANGE, WNDCLASSW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, Error, HRESULT, IUnknown, Interface, PCSTR, s, w};
use windows_numerics::{Vector2, Vector4};

use super::BLUR;
use super::lens::park_mirror;
use super::{Outline, SATURATION};
use crate::log;
use crate::log::LogErr;

/// The grain: how much of the noise is added, how fine (cycles per physical pixel), and how
/// far it reaches, which only has to cover the largest window.
const GRAIN: f32 = 0.3;
const GRAIN_FREQUENCY: f32 = 0.8;
const GRAIN_REACH: f32 = 8192.0;
/// What one octave of fractal-sum turbulence averages, so the grain can be centred on zero.
const GRAIN_MEAN: f32 = 0.39;
/// `DWM_CLOAKED_APP`.
const CLOAKED_APP: u32 = 2;
/// `WCA_EXCLUDED_FROM_LIVEPREVIEW`, for `SetWindowCompositionAttribute`.
const WCA_EXCLUDED_FROM_LIVEPREVIEW: u32 = 13;

pub(super) type CreateSharedMultiWindowVisual =
    unsafe extern "system" fn(HWND, *mut c_void, *mut *mut c_void, *mut isize) -> HRESULT;
pub(super) type UpdateSharedMultiWindowVisual = unsafe extern "system" fn(
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
pub(super) struct Shared {
    pub(super) device: IDCompositionDesktopDevice,
    pub(super) create: CreateSharedMultiWindowVisual,
    pub(super) update: UpdateSharedMultiWindowVisual,
    /// The visual must be made for a window with live preview turned off; a cloaked window of
    /// its own keeps that off the island.
    pub(super) host: HWND,
}

impl gpui::Global for Shared {}

thread_local! {
    /// Everything lined up by the virtual screen, on the UI thread, set again when it changes.
    static ALIGNED: RefCell<Aligned> = RefCell::default();
}

#[derive(Default)]
struct Aligned {
    panes: Vec<Weak<Parts>>,
    mirrors: Vec<HWND>,
}

/// A lens's mirror, to keep beside the screens when they change.
pub(super) fn track_mirror(window: HWND) {
    ALIGNED.with_borrow_mut(|aligned| aligned.mirrors.push(window));
}

pub(super) fn untrack_mirror(window: HWND) {
    ALIGNED.with_borrow_mut(|aligned| aligned.mirrors.retain(|&mirror| mirror != window));
}

/// The displays changed. Every glass lines up with the screen by the virtual screen's origin,
/// which a monitor added, removed or moved can shift under a window that stays put.
fn refit() {
    log("displays changed; glass set again");
    let (panes, mirrors) = ALIGNED.with_borrow_mut(|aligned| {
        aligned.panes.retain(|pane| pane.strong_count() > 0);
        let panes: Vec<_> = aligned.panes.iter().filter_map(Weak::upgrade).collect();
        (panes, aligned.mirrors.clone())
    });
    for parts in panes {
        if let Some((origin, scale, outline)) = parts.last.get() {
            LiveShape(parts).set(origin, scale, outline);
        }
    }
    for mirror in mirrors {
        park_mirror(mirror);
    }
}

/// The host window's procedure: a top-level window, so it hears of display changes.
unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DISPLAYCHANGE {
        refit();
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// An exported function of `module` by ordinal or name, as `F`.
///
/// # Safety
/// `F` must be the function's real signature.
unsafe fn export<F: Copy>(module: HMODULE, name: PCSTR) -> windows::core::Result<F> {
    let found = unsafe { GetProcAddress(module, name) }.ok_or_else(Error::from_thread)?;
    // SAFETY: a function pointer to a function pointer of the caller's stated signature.
    Ok(unsafe { std::mem::transmute_copy(&found) })
}

/// What the live glass made, if any window has it.
pub(super) fn made(cx: &gpui::App) -> Option<Shared> {
    cx.try_global::<Shared>().cloned()
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

        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            hInstance: module.into(),
            lpszClassName: w!("topsy-glass"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let host = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("topsy-glass"),
            w!("topsy glass"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
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

/// The live glass of one window: the notch's pane and the page capsule's, each with a copy of its
/// own, since a visual can sit in only one place. Lives on the UI thread.
pub struct Live {
    _target: IDCompositionTarget,
    shape: LiveShape,
    capsule: LiveShape,
}

impl Live {
    pub fn new(hwnd: HWND, cx: &mut gpui::App) -> windows::core::Result<Self> {
        let shared = shared(hwnd, cx)?;
        unsafe {
            let device = &shared.device;
            let target = device.CreateTargetForHwnd(hwnd, false)?;
            let root = device.CreateVisual()?;
            let shape = pane(&shared, hwnd, &root)?;
            let capsule = pane(&shared, hwnd, &root)?;
            target.SetRoot(&root)?;
            Ok(Self {
                _target: target,
                shape,
                capsule,
            })
        }
    }

    pub fn shape(&self) -> LiveShape {
        self.shape.clone()
    }

    pub fn capsule(&self) -> LiveShape {
        self.capsule.clone()
    }
}

/// A pane of glass in `root`: a live copy of what is behind `hwnd`, saturated, blurred and
/// clipped.
fn pane(
    shared: &Shared,
    hwnd: HWND,
    root: &IDCompositionVisual2,
) -> windows::core::Result<LiveShape> {
    unsafe {
        let device = &shared.device;
        let glass = device.CreateVisual()?;
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

        // Perlin noise, made grey, then added to the glass centred on zero, so it shifts each
        // pixel a little up or down without lightening the whole.
        let noise = effects.CreateTurbulenceEffect()?;
        noise.SetNoise(D2D1_TURBULENCE_NOISE_FRACTAL_SUM)?;
        noise.SetBaseFrequency(&Vector2::new(GRAIN_FREQUENCY, GRAIN_FREQUENCY))?;
        noise.SetNumOctaves(1)?;
        noise.SetSize(&Vector2::new(GRAIN_REACH, GRAIN_REACH))?;
        let grey = effects.CreateSaturationEffect()?;
        grey.SetSaturation2(0.0)?;
        grey.SetInput(0, &noise, 0)?;
        // DirectComposition's arithmetic composite weighs input 0 by the second coefficient and
        // input 1 by the third.
        let grained = effects.CreateArithmeticCompositeEffect()?;
        grained.SetCoefficients(&Vector4::new(0.0, GRAIN, 1.0, -GRAIN * GRAIN_MEAN))?;
        grained.SetClampOutput(true)?;
        grained.SetInput(0, &grey, 0)?;
        grained.SetInput(1, &blur, 0)?;
        glass.SetEffect(&grained)?;
        let clip = device.CreateRectangleClip()?;
        glass.SetClip(&clip)?;
        // The copy spans the virtual screen; cut it down to what the blur can reach before the
        // effect runs, or DWM blurs every changing pixel of every monitor.
        let source = device.CreateRectangleClip()?;
        copy.SetClip(&source)?;
        // Clips are aliased unless asked otherwise, which steps the notch's rounded corners.
        glass.SetBorderMode(DCOMPOSITION_BORDER_MODE_SOFT)?;
        glass.AddVisual(&copy, false, None::<&IDCompositionVisual>)?;
        root.AddVisual(&glass, true, None::<&IDCompositionVisual>)?;

        let parts = Rc::new(Parts {
            device: device.clone(),
            update: shared.update,
            excluded: [hwnd, shared.host],
            thumbnail,
            copy,
            clip,
            source,
            blur,
            shown: Cell::new(None),
            last: Cell::new(None),
        });
        ALIGNED.with_borrow_mut(|aligned| aligned.panes.push(Rc::downgrade(&parts)));
        Ok(LiveShape(parts))
    }
}

struct Parts {
    device: IDCompositionDesktopDevice,
    update: UpdateSharedMultiWindowVisual,
    excluded: [HWND; 2],
    thumbnail: isize,
    copy: IDCompositionVisual2,
    clip: IDCompositionRectangleClip,
    /// The copy's own clip, in virtual-screen coordinates.
    source: IDCompositionRectangleClip,
    blur: IDCompositionGaussianBlurEffect,
    /// The virtual screen and scale the copy was last made for.
    shown: Cell<Option<(RECT, f32)>>,
    /// What it was last set to.
    last: Cell<Option<Fit>>,
}

impl Drop for Parts {
    fn drop(&mut self) {
        unsafe { DwmUnregisterThumbnail(self.thumbnail) }.log_err();
    }
}

/// The origin, scale and outline a pane was set to.
type Fit = ((i32, i32), f32, Outline);

#[derive(Clone)]
pub struct LiveShape(Rc<Parts>);

impl LiveShape {
    pub fn set(&self, origin: (i32, i32), scale: f32, outline: Outline) {
        let parts = &*self.0;
        parts.last.set(Some((origin, scale, outline)));
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
            // Three deviations is as far as the blur reaches.
            let reach = 3.0 * BLUR * scale;
            let (left, top) = (
                (origin.0 - virtual_screen.left) as f32,
                (origin.1 - virtual_screen.top) as f32,
            );
            let source = &parts.source;
            source.SetLeft2(left + x - reach).log_err();
            source.SetTop2(top + y - reach).log_err();
            source.SetRight2(left + x + width + reach).log_err();
            source.SetBottom2(top + y + height + reach).log_err();
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
