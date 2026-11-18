//! The glass behind the pill, drawn by the system compositor in any shape we ask for.
//!
//! GPUI presents through DirectComposition as the window's *topmost* visual tree. A window can
//! carry a second, non-topmost tree from Windows.UI.Composition, which DWM composes underneath
//! GPUI's. That tree holds the host backdrop (the blurred desktop behind the window) clipped to
//! a rounded rectangle whose radius we choose, so the glass can be a true capsule rather than
//! DWM's fixed window corner, and a second such pane for the page capsule under the notch. Tint
//! and rim are drawn by GPUI on top.

use std::ffi::c_void;

use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{CompositionRoundedRectangleGeometry, Compositor};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::core::{BOOL, Interface};
use windows_numerics::Vector2;

use crate::log::LogErr;

/// The UI thread's compositor and the dispatcher queue it needs. A thread can have one queue, so
/// the island's glass and the basket's share them, for the life of the app.
struct Shared {
    /// Never read: it only has to live as long as the compositor.
    _queue: DispatcherQueueController,
    compositor: Compositor,
}

impl gpui::Global for Shared {}

/// The UI thread's compositor, made with its dispatcher queue the first time it is asked for.
fn compositor(cx: &mut gpui::App) -> windows::core::Result<Compositor> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return Ok(shared.compositor.clone());
    }
    // GPUI's thread already runs COM (OLE) and its own message loop; the queue only
    // needs to exist for the compositor to be created on this thread.
    let queue = unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })?
    };
    let compositor = Compositor::new()?;
    cx.set_global(Shared {
        _queue: queue,
        compositor: compositor.clone(),
    });
    Ok(compositor)
}

/// The compositor glass of one window. Lives on the UI thread, which owns the dispatcher
/// queue Windows.UI.Composition needs.
pub struct Backdrop {
    _target: DesktopWindowTarget,
    shape: CompositionRoundedRectangleGeometry,
    capsule: CompositionRoundedRectangleGeometry,
}

impl Backdrop {
    /// Put the blurred backdrop behind `hwnd`'s content. Fails on systems without host backdrop
    /// support (before Windows 11), where the caller falls back to window-wide acrylic.
    pub fn new(hwnd: HWND, cx: &mut gpui::App) -> windows::core::Result<Self> {
        unsafe {
            let compositor = compositor(cx)?;
            let enable = BOOL::from(true);
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                &enable as *const _ as *const c_void,
                std::mem::size_of_val(&enable) as u32,
            )?;

            let target = compositor
                .cast::<ICompositorDesktopInterop>()?
                .CreateDesktopWindowTarget(hwnd, false)?;

            let root = compositor.CreateContainerVisual()?;
            root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
            let pane = || -> windows::core::Result<CompositionRoundedRectangleGeometry> {
                let glass = compositor.CreateSpriteVisual()?;
                glass.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
                glass.SetBrush(&compositor.CreateHostBackdropBrush()?)?;
                let shape = compositor.CreateRoundedRectangleGeometry()?;
                glass.SetClip(&compositor.CreateGeometricClipWithGeometry(&shape)?)?;
                root.Children()?.InsertAtTop(&glass)?;
                Ok(shape)
            };
            let shape = pane()?;
            let capsule = pane()?;
            target.SetRoot(&root)?;

            Ok(Self {
                _target: target,
                shape,
                capsule,
            })
        }
    }

    /// The handle to reshape the glass from a task; cheap to clone.
    pub fn shape(&self) -> Shape {
        Shape(self.shape.clone())
    }

    /// The page capsule's pane, the same kind of handle.
    pub fn capsule(&self) -> Shape {
        Shape(self.capsule.clone())
    }
}

/// The glass's outline, in physical pixels of the window's client area.
#[derive(Clone)]
pub struct Shape(CompositionRoundedRectangleGeometry);

impl Shape {
    /// A rounded rectangle at `(x, y)`. Parts outside the window are cut off with it, which is
    /// how the notch gets a square top: its rounded top corners sit above the window.
    pub fn set(&self, x: f32, y: f32, width: f32, height: f32, radius: f32) {
        self.0.SetOffset(Vector2 { X: x, Y: y }).log_err();
        self.0
            .SetSize(Vector2 {
                X: width,
                Y: height,
            })
            .log_err();
        self.0
            .SetCornerRadius(Vector2 {
                X: radius,
                Y: radius,
            })
            .log_err();
    }
}
