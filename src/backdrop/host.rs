//! The fallback glass: Windows.UI.Composition's host backdrop (the desktop behind the window,
//! blurred by the system by a fixed amount), saturated and clipped to the notch. A window can
//! carry one non-topmost tree from Windows.UI.Composition, which DWM composes underneath GPUI's
//! topmost DirectComposition tree.

use std::cell::RefCell;
use std::ffi::c_void;

use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBrush, CompositionEffectSourceParameter, CompositionRoundedRectangleGeometry,
    Compositor,
};
use windows::Win32::Foundation::{E_BOUNDS, E_INVALIDARG, HWND};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    GRAPHICS_EFFECT_PROPERTY_MAPPING, IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR, implement};
use windows_numerics::Vector2;

use super::{Outline, SATURATION};
use crate::log::LogErr;
/// Direct2D's colour matrix effect, which the compositor runs itself.
const CLSID_D2D1_COLOR_MATRIX: GUID = GUID::from_u128(0x921f03d6_641c_47df_852d_b4bb6153ae11);
/// `D2D1_COLORMATRIX_ALPHA_MODE_PREMULTIPLIED`.
const ALPHA_PREMULTIPLIED: u32 = 1;

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

/// The host backdrop glass of one window. Lives on the UI thread, which owns the dispatcher
/// queue Windows.UI.Composition needs.
pub struct Host {
    _target: DesktopWindowTarget,
    shape: CompositionRoundedRectangleGeometry,
}

impl Host {
    /// Put the blurred backdrop behind `hwnd`'s content. Fails on systems without host backdrop
    /// support (before Windows 11).
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

            let glass = compositor.CreateSpriteVisual()?;
            glass.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
            glass.SetBrush(&saturated(&compositor)?)?;
            let shape = compositor.CreateRoundedRectangleGeometry()?;
            glass.SetClip(&compositor.CreateGeometricClipWithGeometry(&shape)?)?;
            target.SetRoot(&glass)?;

            Ok(Self {
                _target: target,
                shape,
            })
        }
    }

    pub fn shape(&self) -> HostShape {
        HostShape(self.shape.clone())
    }
}

#[derive(Clone)]
pub struct HostShape(CompositionRoundedRectangleGeometry);

impl HostShape {
    pub fn set(&self, outline: Outline) {
        let Outline {
            x,
            y,
            width,
            height,
            radius,
        } = outline;
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

/// The host backdrop through a colour matrix that saturates it by [`SATURATION`]. Falls back to
/// the plain backdrop if the compositor turns the effect down, since glass beats none.
fn saturated(compositor: &Compositor) -> windows::core::Result<CompositionBrush> {
    let backdrop: CompositionBrush = compositor.CreateHostBackdropBrush()?.cast()?;
    let brush = || -> windows::core::Result<CompositionBrush> {
        let source = HSTRING::from("backdrop");
        let effect: IGraphicsEffect = Effect {
            id: CLSID_D2D1_COLOR_MATRIX,
            // Matrix, alpha mode, clamp output: every property, in Direct2D's order.
            properties: vec![
                PropertyValue::CreateSingleArray(&saturation_matrix(SATURATION))?.cast()?,
                PropertyValue::CreateUInt32(ALPHA_PREMULTIPLIED)?.cast()?,
                PropertyValue::CreateBoolean(true)?.cast()?,
            ],
            sources: vec![CompositionEffectSourceParameter::Create(&source)?.cast()?],
            name: RefCell::default(),
        }
        .into();
        let brush = compositor.CreateEffectFactory(&effect)?.CreateBrush()?;
        brush.SetSourceParameter(&source, &backdrop)?;
        brush.cast()
    };
    Ok(brush().log_err().unwrap_or(backdrop))
}

/// Direct2D's 5x4 colour matrix (rows are the input R, G, B, A and a constant; columns the
/// output R, G, B, A) that scales saturation by `amount` around Rec. 709 luma.
fn saturation_matrix(amount: f32) -> [f32; 20] {
    const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
    let mut matrix = [0.0; 20];
    for (input, luma) in LUMA.iter().enumerate() {
        for output in 0..3 {
            let own = if input == output { amount } else { 0.0 };
            matrix[input * 4 + output] = luma * (1.0 - amount) + own;
        }
    }
    matrix[3 * 4 + 3] = 1.0;
    matrix
}

/// A Direct2D effect described for the compositor: its id, every property in order, and its
/// inputs.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct Effect {
    id: GUID,
    properties: Vec<IPropertyValue>,
    sources: Vec<IGraphicsEffectSource>,
    name: RefCell<HSTRING>,
}

impl IGraphicsEffect_Impl for Effect_Impl {
    fn Name(&self) -> windows::core::Result<HSTRING> {
        Ok(self.name.borrow().clone())
    }

    fn SetName(&self, name: &HSTRING) -> windows::core::Result<()> {
        *self.name.borrow_mut() = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for Effect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for Effect_Impl {
    fn GetEffectId(&self) -> windows::core::Result<GUID> {
        Ok(self.id)
    }

    /// Only asked for properties that are animated, and none are.
    fn GetNamedPropertyMapping(
        &self,
        _name: &PCWSTR,
        _index: *mut u32,
        _mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> windows::core::Result<()> {
        Err(E_INVALIDARG.into())
    }

    fn GetPropertyCount(&self) -> windows::core::Result<u32> {
        Ok(self.properties.len() as u32)
    }

    fn GetProperty(&self, index: u32) -> windows::core::Result<IPropertyValue> {
        self.properties
            .get(index as usize)
            .cloned()
            .ok_or_else(|| E_BOUNDS.into())
    }

    fn GetSource(&self, index: u32) -> windows::core::Result<IGraphicsEffectSource> {
        self.sources
            .get(index as usize)
            .cloned()
            .ok_or_else(|| E_BOUNDS.into())
    }

    fn GetSourceCount(&self) -> windows::core::Result<u32> {
        Ok(self.sources.len() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::saturation_matrix;

    fn apply(matrix: &[f32; 20], rgb: [f32; 3]) -> [f32; 3] {
        let mut out = [0.0; 3];
        for (output, value) in out.iter_mut().enumerate() {
            *value = (0..3)
                .map(|input| rgb[input] * matrix[input * 4 + output])
                .sum();
        }
        out
    }

    #[test]
    fn saturation_keeps_greys_and_luma_and_spreads_colour() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let boosted = saturation_matrix(1.5);
        let grey = apply(&boosted, [0.4; 3]);
        assert!(grey.iter().all(|&v| close(v, 0.4)), "{grey:?}");

        let colour = [0.6, 0.3, 0.2];
        let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let out = apply(&boosted, colour);
        assert!(close(luma(out), luma(colour)));
        assert!(out[0] - out[2] > colour[0] - colour[2]);

        let same = apply(&saturation_matrix(1.0), colour);
        assert!(same.iter().zip(colour).all(|(&a, b)| close(a, b)));
    }
}
