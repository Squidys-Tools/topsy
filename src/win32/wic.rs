//! Windows Imaging Component, for the formats only Windows' own codecs read or write here:
//! HEIC (the HEIF Image Extensions, plus HEVC Video Extensions to encode) and AVIF (the AV1
//! Video Extension). Everything else decodes in Rust. Call these on a worker thread inside
//! [`super::with_com`] with `COINIT_MULTITHREADED`.

use std::io;
use std::path::Path;
use std::sync::OnceLock;

use image::RgbaImage;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatHeif, GUID_WICPixelFormat32bppBGRA,
    GUID_WICPixelFormat32bppRGBA, IWICBitmapCodecInfo, IWICBitmapFrameDecode, IWICColorContext,
    IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapPaletteTypeCustom, WICColorContextProfile, WICDecodeMetadataCacheOnDemand, WICDecoder,
};
use windows::Win32::System::Com::StructuredStorage::{PROPBAG2, PROPVARIANT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{GUID, HSTRING, Interface, PWSTR};

use crate::log;

pub struct Decoded {
    pub image: RgbaImage,
    pub icc: Option<Vec<u8>>,
    /// EXIF's numbering, 1 when upright or unknown.
    pub orientation: u16,
}

/// What this machine's codecs can do, probed once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub heic_read: bool,
    pub heic_write: bool,
    pub avif_read: bool,
}

fn factory() -> io::Result<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| fail("WIC", e))
}

fn fail(what: &str, e: windows::core::Error) -> io::Error {
    log(&format!("wic: {what}: {e:?}"));
    io::Error::other(format!("Windows couldn't {what}"))
}

/// The first frame of `path` as straight RGBA, with its ICC profile and orientation tag.
pub fn decode(path: &Path) -> io::Result<Decoded> {
    let f = factory()?;
    let decoder = unsafe {
        f.CreateDecoderFromFilename(
            &HSTRING::from(path.as_os_str()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
    }
    .map_err(|e| missing_codec(path, e))?;
    let frame = unsafe { decoder.GetFrame(0) }.map_err(|e| missing_codec(path, e))?;
    let (mut w, mut h) = (0, 0);
    unsafe { frame.GetSize(&mut w, &mut h) }.map_err(|e| fail("read the picture's size", e))?;
    let converter =
        unsafe { f.CreateFormatConverter() }.map_err(|e| fail("convert the picture", e))?;
    unsafe {
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .map_err(|e| fail("convert the picture", e))?;
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    unsafe { converter.CopyPixels(std::ptr::null(), w * 4, &mut pixels) }
        .map_err(|e| missing_codec(path, e))?;
    let image = RgbaImage::from_raw(w, h, pixels)
        .ok_or_else(|| io::Error::other("The picture is too large"))?;
    Ok(Decoded {
        image,
        icc: icc(&f, &frame),
        orientation: orientation(&frame),
    })
}

/// HEIF and AVIF fail at decode, not open, when their codec extension is missing.
fn missing_codec(path: &Path, e: windows::core::Error) -> io::Error {
    log(&format!("wic: decode {}: {e:?}", path.display()));
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let message = match ext.as_str() {
        "heic" | "heif" | "hif" => {
            "To open HEIC photos, install HEIF Image Extensions from the Microsoft Store"
        }
        "avif" => "To open AVIF pictures, install AV1 Video Extension from the Microsoft Store",
        _ => "This picture's format isn't supported",
    };
    io::Error::new(io::ErrorKind::Unsupported, message)
}

fn icc(f: &IWICImagingFactory, frame: &IWICBitmapFrameDecode) -> Option<Vec<u8>> {
    let mut count = 0;
    unsafe { frame.GetColorContexts(&mut [], &mut count) }.ok()?;
    if count == 0 {
        return None;
    }
    let mut contexts: Vec<Option<IWICColorContext>> = (0..count)
        .map(|_| unsafe { f.CreateColorContext() }.ok())
        .collect();
    unsafe { frame.GetColorContexts(&mut contexts, &mut count) }.ok()?;
    for c in contexts.into_iter().flatten() {
        if unsafe { c.GetType() }.ok()? != WICColorContextProfile {
            continue;
        }
        let mut len = 0;
        unsafe { c.GetProfileBytes(&mut [], &mut len) }.ok()?;
        let mut bytes = vec![0u8; len as usize];
        unsafe { c.GetProfileBytes(&mut bytes, &mut len) }.ok()?;
        return Some(bytes);
    }
    None
}

fn orientation(frame: &IWICBitmapFrameDecode) -> u16 {
    let read = || -> Option<u16> {
        let reader = unsafe { frame.GetMetadataQueryReader() }.ok()?;
        let mut value = PROPVARIANT::default();
        unsafe { reader.GetMetadataByName(&HSTRING::from("System.Photo.Orientation"), &mut value) }
            .ok()?;
        u16::try_from(&value).ok()
    };
    read().filter(|o| (1..=8).contains(o)).unwrap_or(1)
}

/// `image` as a HEIC file at `quality` (1..=100). ICC is carried; EXIF isn't (WIC's HEIF writer
/// has no path for it).
pub fn encode_heic(image: &RgbaImage, quality: u8, icc: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let f = factory()?;
    let stream =
        unsafe { SHCreateMemStream(None) }.ok_or_else(|| io::Error::other("Out of memory"))?;
    write_heic(&f, &stream, image, quality, icc)?;
    read_stream(&stream).map_err(|e| fail("encode HEIC", e))
}

fn read_stream(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    let mut end = 0u64;
    unsafe { stream.Seek(0, STREAM_SEEK_END, Some(&mut end)) }?;
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }?;
    let mut bytes = vec![0u8; end as usize];
    let mut read = 0u32;
    unsafe {
        stream.Read(
            bytes.as_mut_ptr().cast(),
            bytes.len() as u32,
            Some(&mut read),
        )
    }
    .ok()?;
    bytes.truncate(read as usize);
    Ok(bytes)
}

fn write_heic(
    f: &IWICImagingFactory,
    stream: &IStream,
    image: &RgbaImage,
    quality: u8,
    icc: Option<&[u8]>,
) -> io::Result<()> {
    let no_encoder = |e: windows::core::Error| {
        log(&format!("wic: heic encode: {e:?}"));
        io::Error::new(
            io::ErrorKind::Unsupported,
            "To save HEIC, install HEIF Image Extensions and HEVC Video Extensions from the Microsoft Store",
        )
    };
    let encoder = unsafe { f.CreateEncoder(&GUID_ContainerFormatHeif, std::ptr::null()) }
        .map_err(no_encoder)?;
    unsafe { encoder.Initialize(stream, WICBitmapEncoderNoCache) }.map_err(no_encoder)?;
    let (mut frame, mut bag) = (None, None);
    unsafe { encoder.CreateNewFrame(&mut frame, &mut bag) }.map_err(no_encoder)?;
    let frame = frame.ok_or_else(|| io::Error::other("Windows couldn't start a HEIC frame"))?;
    if let Some(bag) = &bag {
        let mut name: Vec<u16> = "ImageQuality".encode_utf16().chain([0]).collect();
        let option = PROPBAG2 {
            pstrName: PWSTR(name.as_mut_ptr()),
            ..Default::default()
        };
        let value = VARIANT::from(f32::from(quality.clamp(1, 100)) / 100.0);
        if let Err(e) = unsafe { bag.Write(1, &option, &value) } {
            log(&format!("wic: heic quality: {e:?}"));
        }
    }
    unsafe { frame.Initialize(bag.as_ref()) }.map_err(no_encoder)?;
    let (w, h) = image.dimensions();
    unsafe { frame.SetSize(w, h) }.map_err(no_encoder)?;
    let mut format: GUID = GUID_WICPixelFormat32bppBGRA;
    unsafe { frame.SetPixelFormat(&mut format) }.map_err(no_encoder)?;
    if let Some(icc) = icc
        && let Ok(context) = unsafe { f.CreateColorContext() }
        && unsafe { context.InitializeFromMemory(icc) }.is_ok()
    {
        let _ = unsafe { frame.SetColorContexts(&[Some(context)]) };
    }
    let bgra: Vec<u8> = if format == GUID_WICPixelFormat32bppBGRA {
        image
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect()
    } else {
        // The encoder asked for something else; let WIC convert from straight RGBA.
        return write_converted(f, &frame, &encoder, image, format);
    };
    unsafe { frame.WritePixels(h, w * 4, &bgra) }.map_err(no_encoder)?;
    unsafe { frame.Commit() }.map_err(no_encoder)?;
    unsafe { encoder.Commit() }.map_err(no_encoder)
}

fn write_converted(
    f: &IWICImagingFactory,
    frame: &windows::Win32::Graphics::Imaging::IWICBitmapFrameEncode,
    encoder: &windows::Win32::Graphics::Imaging::IWICBitmapEncoder,
    image: &RgbaImage,
    format: GUID,
) -> io::Result<()> {
    let (w, h) = image.dimensions();
    let bitmap = unsafe {
        f.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppRGBA, w * 4, image.as_raw())
    }
    .map_err(|e| fail("encode HEIC", e))?;
    let converter = unsafe { f.CreateFormatConverter() }.map_err(|e| fail("encode HEIC", e))?;
    unsafe {
        converter.Initialize(
            &bitmap,
            &format,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .map_err(|e| fail("encode HEIC", e))?;
    unsafe { frame.WriteSource(&converter, std::ptr::null()) }
        .map_err(|e| fail("encode HEIC", e))?;
    unsafe { frame.Commit() }.map_err(|e| fail("encode HEIC", e))?;
    unsafe { encoder.Commit() }.map_err(|e| fail("encode HEIC", e))
}

/// What the installed codecs allow. The HEIC writer is tried for real (the HEIF extension
/// registers an encoder even when the HEVC one it needs is missing).
pub fn capabilities() -> Capabilities {
    static CAPS: OnceLock<Capabilities> = OnceLock::new();
    *CAPS.get_or_init(|| {
        super::with_com(windows::Win32::System::Com::COINIT_MULTITHREADED, || {
            let Ok(f) = factory() else {
                return Capabilities {
                    heic_read: false,
                    heic_write: false,
                    avif_read: false,
                };
            };
            let heic_write = unsafe { SHCreateMemStream(None) }.is_some_and(|s| {
                let probe = RgbaImage::from_pixel(16, 16, image::Rgba([200, 100, 50, 255]));
                write_heic(&f, &s, &probe, 50, None).is_ok()
            });
            Capabilities {
                heic_read: has_decoder(&f, "heic"),
                heic_write,
                avif_read: has_decoder(&f, "avif"),
            }
        })
    })
}

fn has_decoder(f: &IWICImagingFactory, ext: &str) -> bool {
    let Ok(list) = (unsafe { f.CreateComponentEnumerator(WICDecoder.0 as u32, 0) }) else {
        return false;
    };
    let want = format!(".{ext}");
    loop {
        let mut item = [None];
        let mut got = 0;
        if unsafe { list.Next(&mut item, Some(&mut got)) }.is_err() || got == 0 {
            return false;
        }
        let Some(info) = item[0]
            .take()
            .and_then(|u| u.cast::<IWICBitmapCodecInfo>().ok())
        else {
            continue;
        };
        let mut len = 0;
        let _ = unsafe { info.GetFileExtensions(&mut [], &mut len) };
        let mut buf = vec![0u16; len as usize];
        if unsafe { info.GetFileExtensions(&mut buf, &mut len) }.is_ok() {
            let exts =
                String::from_utf16_lossy(&buf[..buf.len().saturating_sub(1)]).to_ascii_lowercase();
            if exts.split(',').any(|e| e.trim() == want) {
                return true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("topsy-wic-{}-{name}", std::process::id()))
    }

    #[test]
    fn heic_round_trips_when_the_codecs_are_installed() {
        let caps = capabilities();
        if !caps.heic_write || !caps.heic_read {
            eprintln!("skipped: no HEIC codecs ({caps:?})");
            return;
        }
        super::super::with_com(windows::Win32::System::Com::COINIT_MULTITHREADED, || {
            let img = RgbaImage::from_fn(96, 64, |x, y| {
                image::Rgba([(x * 2) as u8, (y * 3) as u8, 128, 255])
            });
            let path = tmp("round.heic");
            std::fs::write(&path, encode_heic(&img, 90, None).unwrap()).unwrap();
            let back = decode(&path).unwrap();
            let _ = std::fs::remove_file(&path);
            assert_eq!(back.image.dimensions(), (96, 64));
            let err: f64 = back
                .image
                .as_raw()
                .iter()
                .zip(img.as_raw())
                .map(|(a, b)| f64::from(a.abs_diff(*b)))
                .sum::<f64>()
                / img.as_raw().len() as f64;
            assert!(err < 4.0, "mean error {err}");
        });
    }

    #[test]
    fn a_missing_file_reads_as_an_error_not_a_panic() {
        super::super::with_com(windows::Win32::System::Com::COINIT_MULTITHREADED, || {
            assert!(decode(Path::new("Z:/no/such/file.heic")).is_err());
        });
    }
}
