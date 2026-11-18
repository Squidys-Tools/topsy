//! The lens's Direct2D side: the live glass's recipe (saturate, blur, grain) with the bend
//! between the blur and the grain, and the tint drawn over it.

use windows::Graphics::Capture::Direct3D11CaptureFrame;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BORDER_MODE_HARD,
    D2D1_COMPOSITE_MODE_PLUS, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_PIXEL_FORMAT,
    D2D1_TURBULENCE_NOISE_FRACTAL_SUM,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1AlphaMask, CLSID_D2D1ArithmeticComposite, CLSID_D2D1ColorMatrix, CLSID_D2D1Composite,
    CLSID_D2D1DisplacementMap, CLSID_D2D1GaussianBlur, CLSID_D2D1Saturation, CLSID_D2D1Turbulence,
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_CHANNEL_SELECTOR_G, D2D1_CHANNEL_SELECTOR_R, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_PROPERTY_TYPE, D2D1_PROPERTY_TYPE_BOOL, D2D1_PROPERTY_TYPE_ENUM, D2D1_PROPERTY_TYPE_FLOAT,
    D2D1_PROPERTY_TYPE_MATRIX_5X4, D2D1_PROPERTY_TYPE_UINT32, D2D1_PROPERTY_TYPE_VECTOR2,
    D2D1_PROPERTY_TYPE_VECTOR4, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Effect, ID2D1Image,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::core::Interface;
use windows_numerics::{Matrix3x2, Vector2, Vector4};

use super::super::SATURATION;
use super::super::live::{GRAIN, GRAIN_FREQUENCY, GRAIN_MEAN, GRAIN_REACH};

/// `D2D1_COLORMATRIX_ALPHA_MODE_STRAIGHT`: the matrix runs on the values as they are.
const STRAIGHT: u32 = 2;

/// Set one property of a Direct2D effect.
fn property<T: Copy>(
    effect: &ID2D1Effect,
    index: u32,
    kind: D2D1_PROPERTY_TYPE,
    value: T,
) -> windows::core::Result<()> {
    // SAFETY: `value` is a plain value of the size and layout the property's type stands for.
    let bytes =
        unsafe { std::slice::from_raw_parts((&value as *const T).cast::<u8>(), size_of::<T>()) };
    unsafe { effect.SetValue(index, kind, bytes) }
}

fn bitmap_properties(options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: options,
        ..Default::default()
    }
}

/// The effect graph, made once per device context.
pub(super) struct Graph {
    context: ID2D1DeviceContext,
    input: ID2D1Effect,
    blur: ID2D1Effect,
    bends: [ID2D1Effect; 3],
    noise: ID2D1Effect,
    /// The map's blue channel made alpha: where the bent glass shows over the live glass.
    rim: ID2D1Effect,
    output: ID2D1Image,
}

impl Graph {
    /// The graph, with red bending `1 + fringe` times as far as green and blue `1 - fringe`.
    pub(super) fn new(context: ID2D1DeviceContext) -> windows::core::Result<Self> {
        unsafe {
            let input = context.CreateEffect(&CLSID_D2D1Saturation)?;
            property(&input, 0, D2D1_PROPERTY_TYPE_FLOAT, SATURATION)?;
            let blur = context.CreateEffect(&CLSID_D2D1GaussianBlur)?;
            property(
                &blur,
                2,
                D2D1_PROPERTY_TYPE_ENUM,
                D2D1_BORDER_MODE_HARD.0 as u32,
            )?;
            blur.SetInput(0, &input.GetOutput()?, true);
            let blurred = blur.GetOutput()?;
            // One displacement per colour, each keeping its own channel, added back together.
            let fringe = context.CreateEffect(&CLSID_D2D1Composite)?;
            fringe.SetInputCount(3)?;
            property(
                &fringe,
                0,
                D2D1_PROPERTY_TYPE_ENUM,
                D2D1_COMPOSITE_MODE_PLUS.0 as u32,
            )?;
            let mut bends = Vec::with_capacity(3);
            for channel in 0..3 {
                let bend = context.CreateEffect(&CLSID_D2D1DisplacementMap)?;
                property(
                    &bend,
                    1,
                    D2D1_PROPERTY_TYPE_ENUM,
                    D2D1_CHANNEL_SELECTOR_R.0 as u32,
                )?;
                property(
                    &bend,
                    2,
                    D2D1_PROPERTY_TYPE_ENUM,
                    D2D1_CHANNEL_SELECTOR_G.0 as u32,
                )?;
                bend.SetInput(0, &blurred, true);
                let mut matrix = [0.0f32; 20];
                matrix[channel * 5] = 1.0;
                // Opaque in every branch: a zero alpha would clamp the kept channel away.
                matrix[19] = 1.0;
                let keep = context.CreateEffect(&CLSID_D2D1ColorMatrix)?;
                property(&keep, 0, D2D1_PROPERTY_TYPE_MATRIX_5X4, matrix)?;
                property(&keep, 1, D2D1_PROPERTY_TYPE_ENUM, STRAIGHT)?;
                keep.SetInput(0, &bend.GetOutput()?, true);
                fringe.SetInput(channel as u32, &keep.GetOutput()?, true);
                bends.push(bend);
            }
            let noise = context.CreateEffect(&CLSID_D2D1Turbulence)?;
            property(
                &noise,
                1,
                D2D1_PROPERTY_TYPE_VECTOR2,
                Vector2::new(GRAIN_REACH, GRAIN_REACH),
            )?;
            let frequency = Vector2::new(GRAIN_FREQUENCY, GRAIN_FREQUENCY);
            property(&noise, 2, D2D1_PROPERTY_TYPE_VECTOR2, frequency)?;
            property(&noise, 3, D2D1_PROPERTY_TYPE_UINT32, 1u32)?;
            let fractal = D2D1_TURBULENCE_NOISE_FRACTAL_SUM.0 as u32;
            property(&noise, 5, D2D1_PROPERTY_TYPE_ENUM, fractal)?;
            let grey = context.CreateEffect(&CLSID_D2D1Saturation)?;
            property(&grey, 0, D2D1_PROPERTY_TYPE_FLOAT, 0.0f32)?;
            grey.SetInput(0, &noise.GetOutput()?, true);
            let grain = context.CreateEffect(&CLSID_D2D1ArithmeticComposite)?;
            // As in the live glass: input 0 is weighed by the second coefficient, input 1 by the third.
            let weights = Vector4::new(0.0, GRAIN, 1.0, -GRAIN * GRAIN_MEAN);
            property(&grain, 0, D2D1_PROPERTY_TYPE_VECTOR4, weights)?;
            property(&grain, 1, D2D1_PROPERTY_TYPE_BOOL, 1u32)?;
            grain.SetInput(0, &grey.GetOutput()?, true);
            grain.SetInput(1, &fringe.GetOutput()?, true);
            let mut matrix = [0.0f32; 20];
            matrix[2 * 4 + 3] = 1.0;
            let rim = context.CreateEffect(&CLSID_D2D1ColorMatrix)?;
            property(&rim, 0, D2D1_PROPERTY_TYPE_MATRIX_5X4, matrix)?;
            property(&rim, 1, D2D1_PROPERTY_TYPE_ENUM, STRAIGHT)?;
            let masked = context.CreateEffect(&CLSID_D2D1AlphaMask)?;
            masked.SetInput(0, &grain.GetOutput()?, true);
            masked.SetInput(1, &rim.GetOutput()?, true);
            let bends = bends
                .try_into()
                .map_err(|_| windows::core::Error::empty())?;
            Ok(Self {
                output: masked.GetOutput()?,
                rim,
                context,
                input,
                blur,
                bends,
                noise,
            })
        }
    }

    /// A bitmap of `size` to fill from memory.
    pub(super) fn bitmap(&self, size: (i32, i32)) -> windows::core::Result<ID2D1Bitmap1> {
        let size = D2D_SIZE_U {
            width: size.0 as u32,
            height: size.1 as u32,
        };
        let properties = bitmap_properties(D2D1_BITMAP_OPTIONS_NONE);
        unsafe { self.context.CreateBitmap(size, None, 0, &properties) }
    }

    pub(super) fn bitmap_of(
        &self,
        frame: &Direct3D11CaptureFrame,
    ) -> windows::core::Result<ID2D1Bitmap1> {
        unsafe {
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: IDXGISurface = access.GetInterface()?;
            self.context.CreateBitmapFromDxgiSurface(&texture, None)
        }
    }

    /// Bend by `map`, whose half-way red and green move nothing and whose extremes move by
    /// `span`, where its blue lets the bend show.
    pub(super) fn set_bend(
        &self,
        map: &ID2D1Bitmap1,
        span: f32,
        fringe: f32,
        blur: f32,
    ) -> windows::core::Result<()> {
        for (bend, gain) in self.bends.iter().zip([1.0 + fringe, 1.0, 1.0 - fringe]) {
            property(bend, 0, D2D1_PROPERTY_TYPE_FLOAT, span * gain)?;
            unsafe { bend.SetInput(1, map, true) };
        }
        unsafe { self.rim.SetInput(0, map, true) };
        property(&self.blur, 0, D2D1_PROPERTY_TYPE_FLOAT, blur)
    }

    /// Draw into the part of a surface `BeginDraw` handed out at `offset`: the bent glass when
    /// a frame of what is behind has come (the capture's corner at `shift` in the window), and
    /// the tint over it.
    pub(super) fn paint(
        &self,
        target: &IDXGISurface,
        offset: POINT,
        (width, height): (i32, i32),
        glass: Option<(&ID2D1Bitmap1, (f32, f32))>,
        shade: &ID2D1Bitmap1,
    ) -> windows::core::Result<()> {
        let context = &self.context;
        unsafe {
            let options = D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW;
            let bitmap =
                context.CreateBitmapFromDxgiSurface(target, Some(&bitmap_properties(options)))?;
            context.SetTarget(&bitmap);
            context.BeginDraw();
            context.SetTransform(&Matrix3x2::translation(offset.x as f32, offset.y as f32));
            // The surface may share its texture with others: stay inside the part handed out.
            let bounds = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: width as f32,
                bottom: height as f32,
            };
            context.PushAxisAlignedClip(&bounds, D2D1_ANTIALIAS_MODE_ALIASED);
            context.Clear(None);
            let mut drawn = Ok(());
            if let Some((frame, shift)) = glass {
                self.input.SetInput(0, frame, true);
                // The grain stays put in the window, as the live glass's does.
                drawn = property(
                    &self.noise,
                    0,
                    D2D1_PROPERTY_TYPE_VECTOR2,
                    Vector2::new(shift.0, shift.1),
                );
                context.DrawImage(
                    &self.output,
                    Some(&Vector2::new(shift.0, shift.1)),
                    None,
                    D2D1_INTERPOLATION_MODE_LINEAR,
                    D2D1_COMPOSITE_MODE_SOURCE_OVER,
                );
            }
            context.DrawImage(
                shade,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            context.PopAxisAlignedClip();
            let ended = context.EndDraw(None, None);
            context.SetTarget(None::<&ID2D1Image>);
            drawn.and(ended)
        }
    }
}
