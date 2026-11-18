//! The glass's refraction (docs/internals/glass.md): while the tray is open, topsy draws the glass
//! itself, bent at the rim like a lens ([`model::lens`](crate::model::lens)), over the live glass.
//!
//! DirectComposition can't displace pixels, so the bend needs what is behind the notch as a
//! texture ([`capture`]): the copy leaves the island out, so the island still shows in
//! screenshots, and a frame only comes when what is behind changes. Direct2D saturates, blurs,
//! bends and grains it as the live glass is ([`graph`]), into a surface above it. The lens draws
//! the tint too, in place of the frame's, so the rim can show what it bends in brighter than the
//! glass around it. The devices start on first use; the capture runs only while the tray is out.

mod capture;
mod graph;

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use windows::Graphics::Capture::Direct3D11CaptureFrame;
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::{HMODULE, HWND, POINT, RECT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory,
    ID2D1Bitmap1, ID2D1Factory1,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
};
use windows::Win32::Graphics::DirectComposition::{
    DCOMPOSITION_BORDER_MODE_SOFT, IDCompositionRectangleClip, IDCompositionSurface,
    IDCompositionSurfaceFactory, IDCompositionVisual2,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::core::Interface;

use self::capture::{Capture, Helper, virtual_screen};
use self::graph::Graph;
use super::live::{BLUR, Shared};
use super::{Outline, Refract};
use crate::log::{LogErr, log};
use crate::model::lens::{Lens as Bend, Rounded, Shade};

/// How far in from the edge the rim bends, how far out it looks at most, and over how many pixels
/// at the edge the bend dies away, in logical pixels.
const BAND: f32 = 40.0;
const STRENGTH: f32 = 36.0;
const FADE: f32 = 4.0;
/// Red bends this much further than green and blue as much less: a faint colour fringe.
const FRINGE: f32 = 0.14;
/// How much of the tint the rim lifts at the edge.
const CLEAR: f32 = 0.8;

/// The refraction of one window's glass. Lives on the UI thread.
pub(super) struct Lens(Rc<Inner>);

struct Inner {
    shared: Shared,
    island: HWND,
    visual: IDCompositionVisual2,
    arrived: async_channel::Sender<()>,
    /// Something failed; the live glass carries on without the bend.
    broken: Cell<bool>,
    /// A call is under way. The capture's WinRT calls pump messages, so GPUI can run a frame
    /// in the middle of one and call back in; what it asks for waits in `wanted`.
    busy: Cell<bool>,
    wanted: Cell<Option<Wanted>>,
    state: RefCell<State>,
}

/// The last [`Lens::set`]: where the window is and how to bend its glass.
struct Wanted {
    origin: (i32, i32),
    scale: f32,
    outline: Outline,
    refract: Option<Refract>,
}

#[derive(Default)]
struct State {
    drawing: Option<Drawing>,
    sized: Option<Sized>,
    capture: Option<Capture>,
    latest: Option<Latest>,
    place: Option<Place>,
    /// The shape the bend map was last painted for, in the capture's pixels.
    bent: Option<Rounded>,
    /// The shape and tint the shade was last painted for, in the window's pixels.
    shaded: Option<(Rounded, Shade)>,
}

/// Where the window is, what is captured and how dark it is, in physical pixels.
#[derive(Clone, Copy, PartialEq)]
struct Place {
    origin: (i32, i32),
    outline: Outline,
    shade: Shade,
    region: RECT,
    virtual_screen: RECT,
}

/// The devices, the effect graph and the helper window, made on first use and kept.
struct Drawing {
    graph: Graph,
    winrt: IDirect3DDevice,
    surfaces: IDCompositionSurfaceFactory,
    helper: Helper,
}

/// What depends on the size of the open tray and the monitor's scale.
struct Sized {
    key: ((i32, i32), f32),
    window: (i32, i32),
    capture: (i32, i32),
    bend: Bend,
    surface: IDCompositionSurface,
    map: ID2D1Bitmap1,
    shade: ID2D1Bitmap1,
    pixels: Vec<u8>,
}

/// The newest frame, held so the bend can be redrawn as the notch moves.
struct Latest {
    _frame: Direct3D11CaptureFrame,
    bitmap: ID2D1Bitmap1,
}

impl Lens {
    /// A lens over `glass`, in the same tree under `parent`, clipped as the glass is.
    pub(super) fn new(
        shared: &Shared,
        island: HWND,
        parent: &IDCompositionVisual2,
        glass: &IDCompositionVisual2,
        clip: &IDCompositionRectangleClip,
        cx: &mut gpui::App,
    ) -> windows::core::Result<Self> {
        let visual = unsafe { shared.device.CreateVisual() }?;
        unsafe {
            visual.SetClip(clip)?;
            visual.SetBorderMode(DCOMPOSITION_BORDER_MODE_SOFT)?;
            parent.AddVisual(&visual, true, glass)?;
        }
        let (arrived, frames) = async_channel::bounded(1);
        let inner = Rc::new(Inner {
            shared: shared.clone(),
            island,
            visual,
            arrived,
            broken: Cell::new(false),
            busy: Cell::new(false),
            wanted: Cell::new(None),
            state: RefCell::default(),
        });
        let weak: Weak<Inner> = Rc::downgrade(&inner);
        cx.spawn(async move |_| {
            while frames.recv().await.is_ok() {
                let Some(inner) = weak.upgrade() else { break };
                inner.exclusive(Inner::arrived);
            }
        })
        .detach();
        Ok(Self(inner))
    }

    /// Whether it can draw; once it has failed, the frame tints the glass again.
    pub(super) fn works(&self) -> bool {
        !self.0.broken.get()
    }

    /// Bend and tint the glass of a window at `origin`; stop with `None`.
    pub(super) fn set(
        &self,
        origin: (i32, i32),
        scale: f32,
        outline: Outline,
        refract: Option<Refract>,
    ) {
        self.0.wanted.set(Some(Wanted {
            origin,
            scale,
            outline,
            refract,
        }));
        self.0.exclusive(|_| {});
    }
}

impl Inner {
    /// Run `work` unless a call is already under way, then whatever was asked for meanwhile.
    fn exclusive(&self, work: impl FnOnce(&Self)) {
        if self.busy.replace(true) {
            return;
        }
        work(self);
        while let Some(wanted) = self.wanted.take() {
            self.apply(wanted);
        }
        self.busy.set(false);
    }

    fn apply(&self, wanted: Wanted) {
        let Wanted {
            origin,
            scale,
            outline,
            refract,
        } = wanted;
        let Some(refract) = refract.filter(|_| !self.broken.get()) else {
            self.stop();
            return;
        };
        if let Err(e) = self.start(origin, scale, outline, refract) {
            log(&format!("refraction unavailable ({e}); plain glass"));
            self.broken.set(true);
            self.stop();
            return;
        }
        self.draw();
    }

    fn start(
        &self,
        origin: (i32, i32),
        scale: f32,
        outline: Outline,
        refract: Refract,
    ) -> windows::core::Result<()> {
        let mut guard = self.state.borrow_mut();
        let state = &mut *guard;
        if state.drawing.is_none() {
            state.drawing = Some(self.make_drawing()?);
        }
        let Some(drawing) = state.drawing.as_mut() else {
            return Ok(());
        };
        if state.sized.as_ref().map(|sized| sized.key) != Some((refract.open, scale)) {
            state.capture = None;
            state.latest = None;
            state.bent = None;
            state.shaded = None;
            state.sized = Some(make_sized(
                &drawing.graph,
                &drawing.surfaces,
                refract.open,
                scale,
            )?);
        }
        let Some(sized) = state.sized.as_ref() else {
            return Ok(());
        };
        // The capture is centred on the notch, which keeps it centred on its monitor.
        let centre = origin.0 + (outline.x + outline.width / 2.0).round() as i32;
        let left = centre - sized.capture.0 / 2;
        let region = RECT {
            left,
            top: origin.1,
            right: left + sized.capture.0,
            bottom: origin.1 + sized.capture.1,
        };
        let virtual_screen = virtual_screen();
        let shade = Shade {
            black_until: refract.black_until,
            bottom: refract.bottom,
            band: BAND * scale,
            clear: CLEAR,
        };
        drawing.helper.aim(&self.shared, self.island, region)?;
        if state.capture.is_none() {
            let arrived = self.arrived.clone();
            let capture =
                drawing
                    .helper
                    .capture(&drawing.winrt, sized.capture, virtual_screen, arrived)?;
            state.capture = Some(capture);
        }
        state.place = Some(Place {
            origin,
            outline,
            shade,
            region,
            virtual_screen,
        });
        Ok(())
    }

    fn make_drawing(&self) -> windows::core::Result<Drawing> {
        unsafe {
            let mut d3d = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let dxgi: IDXGIDevice = d3d.ok_or_else(windows::core::Error::empty)?.cast()?;
            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d = factory.CreateDevice(&dxgi)?;
            let context = d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            Ok(Drawing {
                graph: Graph::new(context)?,
                winrt: CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?,
                surfaces: self.shared.device.CreateSurfaceFactory(&d2d)?,
                helper: Helper::new(&self.shared)?,
            })
        }
    }

    fn stop(&self) {
        let mut state = self.state.borrow_mut();
        if state.capture.is_none() && state.place.is_none() {
            return;
        }
        state.capture = None;
        state.latest = None;
        state.place = None;
        if let Some(drawing) = &state.drawing {
            drawing.helper.hide();
        }
        unsafe { self.visual.SetContent(None::<&windows::core::IUnknown>) }.log_err();
        unsafe { self.shared.device.Commit() }.log_err();
    }

    /// A frame came in: keep the newest and draw with it.
    fn arrived(&self) {
        {
            let mut state = self.state.borrow_mut();
            let (Some(capture), Some(drawing)) = (&state.capture, &state.drawing) else {
                return;
            };
            let Some(frame) = capture.newest() else {
                return;
            };
            match drawing.graph.bitmap_of(&frame) {
                Ok(bitmap) => {
                    state.latest = Some(Latest {
                        _frame: frame,
                        bitmap,
                    })
                }
                Err(e) => return log(&format!("refraction frame: {e}")),
            }
        }
        self.draw();
    }

    fn draw(&self) {
        if let Err(e) = self.try_draw() {
            log(&format!("refraction draw: {e}"));
        }
    }

    /// Draw the tint, and the bent glass under it once a frame has come.
    fn try_draw(&self) -> windows::core::Result<()> {
        let mut guard = self.state.borrow_mut();
        let State {
            drawing: Some(drawing),
            sized: Some(sized),
            place: Some(place),
            latest,
            bent,
            shaded,
            ..
        } = &mut *guard
        else {
            return Ok(());
        };
        let Place {
            origin,
            outline,
            shade,
            region,
            ..
        } = *place;
        let window = Rounded {
            x: outline.x,
            y: outline.y,
            width: outline.width,
            height: outline.height,
            radius: outline.radius,
        };
        if *shaded != Some((window, shade)) {
            let (width, height) = sized.window;
            shade.paint(&window, width as usize, height as usize, &mut sized.pixels);
            copy_into(&sized.shade, &sized.pixels, width)?;
            *shaded = Some((window, shade));
        }
        // Where the capture's corner sits in the window.
        let shift = (
            (region.left - origin.0) as f32,
            (region.top - origin.1) as f32,
        );
        let glass = match latest {
            Some(latest) => {
                let shape = Rounded {
                    x: window.x - shift.0,
                    y: window.y - shift.1,
                    ..window
                };
                if *bent != Some(shape) {
                    let (width, height) = sized.capture;
                    sized
                        .bend
                        .paint(&shape, width as usize, height as usize, &mut sized.pixels);
                    copy_into(&sized.map, &sized.pixels, width)?;
                    *bent = Some(shape);
                }
                Some((&latest.bitmap, shift))
            }
            None => None,
        };
        unsafe {
            let (width, height) = sized.window;
            let update = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            let mut offset = POINT::default();
            let target: IDXGISurface = sized.surface.BeginDraw(Some(&update), &mut offset)?;
            let drawn = drawing
                .graph
                .paint(&target, offset, sized.window, glass, &sized.shade);
            sized.surface.EndDraw()?;
            drawn?;
            self.visual.SetContent(&sized.surface)?;
            self.shared.device.Commit()
        }
    }
}

fn copy_into(bitmap: &ID2D1Bitmap1, pixels: &[u8], width: i32) -> windows::core::Result<()> {
    unsafe { bitmap.CopyFromMemory(None, pixels.as_ptr().cast::<c_void>(), (width * 4) as u32) }
}

/// What is sized by the open tray: a surface and a shade as large as its window, and a capture
/// and bend map larger by as far as the bend and the blur reach.
fn make_sized(
    graph: &Graph,
    surfaces: &IDCompositionSurfaceFactory,
    window: (i32, i32),
    scale: f32,
) -> windows::core::Result<Sized> {
    let reach = (STRENGTH * (1.0 + FRINGE) + 3.0 * BLUR) * scale;
    let margin = reach.ceil() as i32 + 2;
    let capture = (window.0 + 2 * margin, window.1 + margin);
    let bend = Bend {
        band: BAND * scale,
        strength: STRENGTH * scale,
        fade: FADE * scale,
    };
    let surface = unsafe {
        surfaces.CreateSurface(
            window.0 as u32,
            window.1 as u32,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )
    }?;
    let map = graph.bitmap(capture)?;
    graph.set_bend(&map, 2.0 * bend.strength, FRINGE, BLUR * scale)?;
    Ok(Sized {
        key: (window, scale),
        window,
        capture,
        bend,
        surface,
        map,
        shade: graph.bitmap(window)?,
        pixels: Vec::new(),
    })
}
