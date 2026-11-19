//! Drawing the lens: a Direct3D 11 device, a composition swapchain on the lens window, and the
//! shader that bends the screen read in [`Source`].

use std::ffi::c_void;

use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    ID3DBlob,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2, IDXGISwapChain1,
};
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::core::{Interface, PCSTR, s};

use super::Placed;
use super::capture::failed;
use crate::backdrop::BLUR;
use crate::backdrop::Outline;
use crate::log::log;
use model::float::Rect;

const SHADER: &str = include_str!("lens.hlsl");
/// The soft round bevel: how far in from the edge it curves and how far the very edge bends
/// what is behind it, both in logical pixels.
const BEVEL: f32 = 36.0;
const STRENGTH: f32 = 34.0;
/// The colour split at the edge, as a share of the bend.
const DISPERSION: f32 = 0.08;
/// Mip levels of the screen read: enough for the blur's level and the taps around it.
const MIPS: u32 = 6;
/// `D3DCOMPILE_OPTIMIZATION_LEVEL3`.
const OPTIMIZE: u32 = 1 << 15;

/// The screen read, as a mipmapped texture: its mips are the blur.
pub struct Source {
    pub rect: Rect,
    pub texture: ID3D11Texture2D,
    pub view: ID3D11ShaderResourceView,
}

pub struct Painter {
    device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    /// The same device, for Windows.Graphics.Capture.
    pub capture_device: IDirect3DDevice,
    swapchain: IDXGISwapChain1,
    target: Option<ID3D11RenderTargetView>,
    chain: (u32, u32),
    _composition: (
        IDCompositionDevice,
        IDCompositionTarget,
        IDCompositionVisual,
    ),
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    params: ID3D11Buffer,
}

fn compile(entry: PCSTR, target: PCSTR) -> windows::core::Result<ID3DBlob> {
    let mut blob = None;
    let mut errors: Option<ID3DBlob> = None;
    let result = unsafe {
        D3DCompile(
            SHADER.as_ptr() as *const c_void,
            SHADER.len(),
            s!("lens.hlsl"),
            None,
            None,
            entry,
            target,
            OPTIMIZE,
            0,
            &mut blob,
            Some(&mut errors),
        )
    };
    if let Some(errors) = errors {
        log(&format!(
            "lens shader: {}",
            String::from_utf8_lossy(bytes(&errors))
        ));
    }
    result?;
    blob.ok_or_else(failed)
}

fn bytes(blob: &ID3DBlob) -> &[u8] {
    // SAFETY: the blob owns this many bytes for as long as it lives.
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
    }
}

const ONE_SAMPLE: DXGI_SAMPLE_DESC = DXGI_SAMPLE_DESC {
    Count: 1,
    Quality: 0,
};

impl Painter {
    pub fn new(window: HWND) -> windows::core::Result<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
            let device: ID3D11Device = device.ok_or_else(failed)?;
            let dxgi: IDXGIDevice = device.cast()?;
            let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
            let chain = (1, 1);
            let swapchain = factory.CreateSwapChainForComposition(
                &device,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: chain.0,
                    Height: chain.1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: ONE_SAMPLE,
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                    ..Default::default()
                },
                None,
            )?;
            let composition: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = composition.CreateTargetForHwnd(window, true)?;
            let visual = composition.CreateVisual()?;
            visual.SetContent(&swapchain)?;
            target.SetRoot(&visual)?;
            composition.Commit()?;

            let mut vertex = None;
            let mut pixel = None;
            let vertex_code = compile(s!("vs"), s!("vs_5_0"))?;
            let pixel_code = compile(s!("ps"), s!("ps_5_0"))?;
            device.CreateVertexShader(bytes(&vertex_code), None, Some(&mut vertex))?;
            device.CreatePixelShader(bytes(&pixel_code), None, Some(&mut pixel))?;
            let mut sampler = None;
            device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;
            let mut params = None;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<[f32; 16]>() as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut params),
            )?;
            Ok(Self {
                capture_device: CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?,
                device,
                context: context.ok_or_else(failed)?,
                swapchain,
                target: None,
                chain,
                _composition: (composition, target, visual),
                vertex: vertex.ok_or_else(failed)?,
                pixel: pixel.ok_or_else(failed)?,
                sampler: sampler.ok_or_else(failed)?,
                params: params.ok_or_else(failed)?,
            })
        }
    }

    /// A texture to hold the screen in `rect`, its mips filled by [`Painter::filled`].
    pub fn source(&self, rect: Rect) -> windows::core::Result<Source> {
        let mut texture = None;
        let mut view = None;
        unsafe {
            self.device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: (rect.right - rect.left) as u32,
                    Height: (rect.bottom - rect.top) as u32,
                    MipLevels: MIPS,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: ONE_SAMPLE,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                    MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut texture),
            )?;
            let texture = texture.ok_or_else(failed)?;
            self.device
                .CreateShaderResourceView(&texture, None, Some(&mut view))?;
            Ok(Source {
                rect,
                texture,
                view: view.ok_or_else(failed)?,
            })
        }
    }

    /// Fill `source` with top-down BGRA `pixels` of its size.
    pub fn upload(&self, source: &Source, pixels: &[u8]) {
        unsafe {
            self.context.UpdateSubresource(
                &source.texture,
                0,
                None,
                pixels.as_ptr() as *const c_void,
                ((source.rect.right - source.rect.left) * 4) as u32,
                0,
            );
        }
        self.filled(source);
    }

    /// `source`'s top level has changed: blur it down its mips.
    pub fn filled(&self, source: &Source) {
        unsafe { self.context.GenerateMips(&source.view) };
    }

    pub fn draw(&mut self, source: &Source, placed: Placed) -> windows::core::Result<()> {
        let wanted = (placed.size.0.max(1) as u32, placed.size.1.max(1) as u32);
        // The swapchain only grows: the window clips what lies past its edge, and resizing
        // buffers on every frame of the spring would cost more than drawing a few spare pixels.
        if wanted.0 > self.chain.0 || wanted.1 > self.chain.1 || self.target.is_none() {
            self.target = None;
            let chain = (wanted.0.max(self.chain.0), wanted.1.max(self.chain.1));
            unsafe {
                self.swapchain.ResizeBuffers(
                    2,
                    chain.0,
                    chain.1,
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?;
                let back: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
                self.device
                    .CreateRenderTargetView(&back, None, Some(&mut self.target))?;
            }
            self.chain = chain;
        }
        let Outline {
            x,
            y,
            width,
            height,
            radius,
        } = placed.outline;
        let params: [f32; 16] = [
            (source.rect.right - source.rect.left) as f32,
            (source.rect.bottom - source.rect.top) as f32,
            (placed.origin.0 - source.rect.left) as f32,
            (placed.origin.1 - source.rect.top) as f32,
            x,
            y,
            width,
            height,
            radius,
            placed.scale,
            BEVEL,
            STRENGTH,
            BLUR * placed.scale,
            DISPERSION,
            0.0,
            0.0,
        ];
        unsafe {
            let context = &self.context;
            context.UpdateSubresource(
                &self.params,
                0,
                None,
                params.as_ptr() as *const c_void,
                0,
                0,
            );
            context.ClearRenderTargetView(self.target.as_ref(), &[0.0; 4]);
            context.OMSetRenderTargets(Some(std::slice::from_ref(&self.target)), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: self.chain.0 as f32,
                Height: self.chain.1 as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.pixel, None);
            context.PSSetShaderResources(0, Some(&[Some(source.view.clone())]));
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            context.Draw(3, 0);
            self.swapchain.Present(0, DXGI_PRESENT(0)).ok()
        }
    }
}
