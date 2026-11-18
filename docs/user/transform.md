# Transform

Transform compresses, converts, resizes, trims, turns into a GIF or a PDF, strips metadata from, or cuts the
background out of the files on the shelf or in the basket.

## Using it

Choose *Transform…* on a chip's or a basket tile's menu, or in the shelf's header. A panel opens in the
bottom-right corner of the screen the pointer is on: the files at the top, a tile for each action that suits
them, the chosen action's options, and one button. Drag its header to move it.

While it works, the panel steps aside for a column of result cards, one per file (one for a zip, a PDF or a
rename). Each shows its progress, then the size before and after. Click a card to open its result, or copy it
to paste into a mail or a chat. The cards go by themselves six seconds after the run, unless the cursor is on
them. A card's stop button stops the run, deletes what it had made and brings the panel back.

Results are saved beside the originals (`photo (compressed).jpg`, `report (page 2).png`, `Combined.pdf`),
numbered rather than overwriting, and parked on the shelf. Only *Rename* changes the originals.

## Actions

- **Images:** Compress (by quality or to a size, as JPEG, PNG, HEIC, WebP or AVIF, or *Most compatible*: JPEG,
  or PNG when there is transparency; a largest side), Convert (JPEG, PNG, HEIC, WebP, AVIF, TIFF, GIF, BMP, ICO,
  PDF), Resize (largest side, percent, or an exact size fitted or cropped to fill), Remove background, Rotate and
  flip, Remove metadata, Combine into PDF.
- **Video and audio:** Compress (by quality or to a size; a resolution cap; a bitrate for audio), Convert (MP4
  H.264 or HEVC, MOV, WebM, MKV or the sound alone; M4A, MP3, WAV, FLAC, OGG Opus, AIFF), Trim and Make GIF on a
  filmstrip with a range to drag, Rotate, Remove metadata.
- **PDF:** Compress (three presets or a size; the pictures inside shrink), Convert to PNG or JPEG pages at 72,
  150 or 300 dpi, Rotate pages, Remove metadata, Combine.
- **Anything:** Zip, Rename.

## Tools downloaded on first use

The work is done by mature tools that topsy downloads the first time an action needs them, into
`%LOCALAPPDATA%\topsy\tools`: FFmpeg (video, audio and HEIC, about 115 MB), ImageMagick and oxipng (images),
pdfium (PDF pages) and a background-removal model (about 178 MB). The card says *Getting FFmpeg…* while it
downloads. Everything runs on this PC; nothing is uploaded.

## Good to know

- The image actions need the Visual C++ runtime (`vcruntime140.dll`), which most PCs have. Writing HEIC also
  needs FFmpeg.
- A video keeps its main picture and its sound: subtitles, data tracks and cover art are dropped.
- An animated GIF or WebP comes out as its first frame.
- HDR video stays HDR when written as HEVC; other formats and GIFs are tone-mapped to SDR.
- Compress PDF leaves alone the pictures it can't re-encode without changing them (black-and-white scans,
  JPEG 2000, some CMYK pictures). A PDF that is mostly text gets only a little smaller.
- Unlike the notch, the panel takes the keyboard while it is open, since you type into it.
