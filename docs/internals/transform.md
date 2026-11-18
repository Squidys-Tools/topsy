# Transform

How [Transform](../user/transform.md) is built, and why.

- **Mature tools, fetched on first use.** FFmpeg 9.0.2 (Gyan's essentials build, 115 MB) does video and audio;
  ImageMagick 7.1.2 (portable Q16, 12 MB) and oxipng 10.2 do images; pdfium (bblanchon's Chromium build, 4 MB)
  draws PDF pages and lopdf rewrites PDFs. Each is pinned to an immutable release URL with its size and SHA-256
  (`fetch::Pinned`), downloaded under `%LOCALAPPDATA%\topsy\tools` the first time an action needs it, resumed if
  cut off, and checked before it is unpacked. Bump a version by changing the pin, size and hash together. They
  run as processes with no window, below normal priority (`tools::run`); a cancel kills the process. Nothing of
  them is bundled or compiled in.
- **HEIC is packed in Rust** (`model/heif`). The portable ImageMagick reads HEIC but has no HEVC encoder, so
  ImageMagick draws a PNG, FFmpeg's x265 encodes it as a one-frame MP4 (the alpha as a second, grey one), and
  topsy moves the coded frames into a HEIF with the colour profile and EXIF. x265 wants even sides in 4:2:0, so
  an odd picture is padded and cropped back with `clap`. FFmpeg's default scaler halves the quality before x265
  starts; the colour conversion asks for accurate rounding and Lanczos chroma.
- **HDR video stays HDR in HEVC,** written 10-bit with its BT.2020 tags. H.264, VP9, GIFs and the filmstrip are
  8-bit SDR, so an HDR source (PQ or HLG, from ffprobe's `color_transfer`) is tone-mapped first: zscale to
  linear light, then the Mobius curve, which keeps the tones SDR can show and rolls off only the highlights.
- **A PDF's preview is drawn by pdfium, not the shell** (see [Shelf](shelf-and-tray-icon.md)). The panel fetches
  pdfium when a PDF is added.
- **Compress PDF re-encodes pictures as JPEG at the size the page shows them,** which is not their own `/Width`:
  the page's content is walked (`cm` matrices, `q`/`Q`, form XObjects; `transform/pdf/place`) to find how big
  each `Do` really draws it, so a full-page scan, a slide's photo and a logo each shrink to their own dpi. The
  preset's quality then applies, and a result is kept only if it is smaller (a picture left at its size must save
  10%). Indexed pictures go through their palette; CMYK ones only if they are JPEGs written the Adobe way (the
  marker and `/Decode [1 0 1 0 1 0 1 0]` together), since ImageMagick round-trips those without a colour
  conversion. A picture with a colour-key `/Mask` is left alone, as a JPEG no longer has its exact colours. A
  size target the file already meets leaves it as it is.
- **JPEGs turn and strip losslessly, in Rust** (`model/jpeg`), since jpegtran ships for Windows only inside
  installers. A turn that would split an 8x8 block at an edge goes to ImageMagick instead.
- **A photo is turned once.** Its EXIF orientation and the chosen turn compose into one (`model/orientation`)
  and the tag is reset, so no viewer turns the result again.
- **The output's name is reserved before the work** (`create_new`), so two runs never write one file. Compress
  can settle on another format than the name guessed (an opaque PNG becomes a JPEG); the result is renamed to
  match.
- **Remove background runs on this PC:** BiRefNet-lite (a 178 MB model from Hugging Face, fetched once) in pure
  Rust on the CPU, through gemm and rayon.
- **Windows and focus.** The panel takes the focus, like the settings window, since it is typed into; the island
  and the result cards never do. The panel's window is transparent outside its rounded corners, with a region a
  pixel looser so they are click-through too (as the basket's); the cards' window has one per card, so the gaps
  between them are too, unless the column overflows and scrolls. A card's picture is cropped to the card before
  it is drawn: GPUI rounds a covering image's overflowing bounds, not the box that clips it. The filmstrip's
  frames are decoded once, in parallel, when a clip is picked; an idle panel renders nothing.
