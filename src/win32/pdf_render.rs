//! PDF pages to pictures through Windows' own renderer (`Windows.Data.Pdf`, built in since
//! Windows 8.1), so no PDF engine ships with topsy. Call from a worker thread inside
//! [`super::with_com`].

use std::io;
use std::path::Path;

use image::RgbaImage;
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Storage::Streams::{DataReader, IRandomAccessStream, InMemoryRandomAccessStream};
use windows::Win32::System::WinRT::CreateRandomAccessStreamOnFile;
use windows::core::HSTRING;

use crate::log;

fn fail(what: &str, e: windows::core::Error) -> io::Error {
    log(&format!("pdf render: {what}: {e:?}"));
    io::Error::other(format!("Windows couldn't {what}"))
}

pub struct Document(PdfDocument);

/// Opens `path` for rendering.
pub fn open(path: &Path) -> io::Result<Document> {
    let stream: IRandomAccessStream =
        unsafe { CreateRandomAccessStreamOnFile(&HSTRING::from(path.as_os_str()), 0) }
            .map_err(|e| fail("open the PDF", e))?;
    let doc = PdfDocument::LoadFromStreamAsync(&stream)
        .and_then(|op| op.join())
        .map_err(|e| {
            log(&format!("pdf render: load {}: {e:?}", path.display()));
            // 0x8007052B is ERROR_WRONG_PASSWORD.
            if e.code().0 as u32 == 0x8007_052B {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "This PDF is password-protected",
                )
            } else {
                io::Error::new(io::ErrorKind::InvalidData, "This PDF couldn't be read")
            }
        })?;
    Ok(Document(doc))
}

impl Document {
    pub fn pages(&self) -> io::Result<u32> {
        self.0
            .PageCount()
            .map_err(|e| fail("count the PDF's pages", e))
    }

    /// Page `index` (from 0) at `dpi`, on white.
    pub fn render(&self, index: u32, dpi: u32) -> io::Result<RgbaImage> {
        let page = self.0.GetPage(index).map_err(|e| fail("read a page", e))?;
        // Size is in DIPs, 96 to the inch, with the page's rotation applied.
        let size = page.Size().map_err(|e| fail("read a page", e))?;
        let scale = f64::from(dpi) / 96.0;
        let (w, h) = (
            ((f64::from(size.Width) * scale).round() as u32).clamp(1, 20_000),
            ((f64::from(size.Height) * scale).round() as u32).clamp(1, 20_000),
        );
        let options = PdfPageRenderOptions::new().map_err(|e| fail("render a page", e))?;
        options
            .SetDestinationWidth(w)
            .map_err(|e| fail("render a page", e))?;
        options
            .SetDestinationHeight(h)
            .map_err(|e| fail("render a page", e))?;
        let out = InMemoryRandomAccessStream::new().map_err(|e| fail("render a page", e))?;
        page.RenderWithOptionsToStreamAsync(&out, &options)
            .and_then(|op| op.join())
            .map_err(|e| fail("render a page", e))?;
        let png = read_all(&out).map_err(|e| fail("render a page", e))?;
        image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map(|i| i.into_rgba8())
            .map_err(|_| io::Error::other("Windows rendered a page topsy couldn't read"))
    }
}

fn read_all(stream: &InMemoryRandomAccessStream) -> windows::core::Result<Vec<u8>> {
    let len = stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    reader.LoadAsync(len)?.join()?;
    let mut bytes = vec![0u8; len as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}
