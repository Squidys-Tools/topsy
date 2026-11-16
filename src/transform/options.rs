//! The options under the action grid: what each action asks before it runs, as Droppy lays
//! them out (a label on the left, a control on the right), and the choices each one offers.

use gpui::{Context, Div, div, prelude::*, px, rgba};

use super::widgets::{field, note, picker, row, segmented, slider};
use super::{Field, File, Panel};
use crate::model::transform::{Action, Kind, rename_targets};

/// In `CompressFormat::ALL` order.
pub const IMAGE_FORMATS: [&str; 6] = [
    "Most compatible",
    "Same as original",
    "JPEG",
    "PNG",
    "WebP",
    "AVIF",
];
/// In `Format::ALL` order.
pub const CONVERT_IMAGE: [&str; 9] = [
    "JPEG", "PNG", "WebP", "AVIF", "TIFF", "GIF", "BMP", "ICO", "PDF",
];
/// The video formats in `VideoFormat::ALL` order, then the sound alone in `AudioFormat::FROM_VIDEO`'s.
pub const CONVERT_VIDEO: [&str; 9] = [
    "MP4 (H.264)",
    "MP4 (HEVC)",
    "MOV",
    "WebM",
    "MKV",
    "M4A (audio only)",
    "MP3 (audio only)",
    "WAV (audio only)",
    "FLAC (audio only)",
];
/// In `AudioFormat::ALL` order.
pub const CONVERT_AUDIO: [&str; 6] = ["M4A (AAC)", "MP3", "WAV", "FLAC", "OGG (Opus)", "AIFF"];
/// In `PageFormat::ALL` order.
pub const CONVERT_PDF: [&str; 2] = ["PNG images", "JPEG images"];
/// In `PdfQuality::ALL` order.
pub const PDF_QUALITY: [&str; 3] = ["High", "Balanced", "Smallest"];
/// In `PAGE_DPI` order.
pub const DPI: [&str; 3] = ["72 dpi", "150 dpi", "300 dpi"];
/// In `LargestSide::PRESETS` order.
pub const SIDES: [&str; 8] = [
    "Original", "3840 px", "2560 px", "1920 px", "1600 px", "1280 px", "1024 px", "640 px",
];
pub const IMAGE_TARGETS: [&str; 6] = ["100 KB", "250 KB", "500 KB", "1 MB", "2 MB", "5 MB"];
pub const VIDEO_TARGETS: [&str; 7] = [
    "8 MB", "10 MB", "25 MB", "50 MB", "100 MB", "250 MB", "500 MB",
];
pub const PDF_TARGETS: [&str; 5] = ["500 KB", "1 MB", "2 MB", "5 MB", "10 MB"];
/// In `MaxResolution::ALL` order.
pub const RESOLUTIONS: [&str; 6] = ["Original", "4K", "1440p", "1080p", "720p", "480p"];
pub const BITRATES: [&str; 5] = ["320 kbps", "256 kbps", "192 kbps", "128 kbps", "96 kbps"];
pub const RESIZE_BY: [&str; 3] = ["Largest side", "Percent", "Exact"];
pub const PERCENTS: [&str; 6] = ["200%", "75%", "50%", "33%", "25%", "10%"];
pub const FIT: [&str; 2] = ["Fit", "Fill"];
pub const ROTATIONS: [&str; 5] = ["Left", "Right", "180°", "Flip ↔", "Flip ↕"];
pub const FPS: [&str; 3] = ["10 fps", "15 fps", "24 fps"];
pub const TRIM_MODES: [&str; 2] = ["Exact", "Fast"];
pub const GIF_WIDTHS: [&str; 4] = ["320 px", "480 px", "640 px", "Original"];
pub const PAGES: [&str; 3] = ["Fit to image", "A4", "Letter"];

/// Every action's options, kept while the panel is open so switching back finds them as left.
pub struct Options {
    pub by_size: bool,
    pub quality: f32,
    pub pdf_quality: usize,
    pub image_format: usize,
    pub side: usize,
    pub target: usize,
    pub resolution: usize,
    pub bitrate: usize,
    pub convert: usize,
    pub dpi: usize,
    pub resize_by: usize,
    pub resize_side: usize,
    pub percent: usize,
    pub width: String,
    pub height: String,
    pub fit: usize,
    pub rotation: usize,
    /// Trim and GIF range, as fractions of the clip.
    pub range: (f32, f32),
    pub fps: usize,
    pub gif_width: usize,
    pub trim_fast: bool,
    pub page: usize,
    pub zip_name: String,
    pub rename: String,
    /// Formats this machine can't write, by label, greyed in the pickers.
    pub unavailable: Vec<&'static str>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            by_size: false,
            quality: 0.75,
            pdf_quality: 1,
            image_format: 0,
            side: 0,
            target: 3,
            resolution: 0,
            bitrate: 3,
            convert: 0,
            dpi: 1,
            resize_by: 0,
            resize_side: 4,
            percent: 2,
            width: String::new(),
            height: String::new(),
            fit: 0,
            rotation: 1,
            range: (0.0, 1.0),
            fps: 1,
            gif_width: 1,
            trim_fast: false,
            page: 0,
            zip_name: "Archive".into(),
            rename: String::new(),
            unavailable: Vec::new(),
        }
    }
}

impl Options {
    pub fn text(&mut self, field: Field) -> &mut String {
        match field {
            Field::ZipName => &mut self.zip_name,
            Field::RenameName => &mut self.rename,
            Field::Width => &mut self.width,
            Field::Height => &mut self.height,
        }
    }

    /// Defaults that depend on the files: the rename starts from the first file's name.
    pub fn fit_to(&mut self, files: &[File]) {
        if self.rename.is_empty()
            && let Some(stem) = files.first().and_then(|f| f.path.file_stem())
        {
            self.rename = stem.to_string_lossy().into_owned();
        }
        let videos = files.iter().any(|f| f.kind == Kind::Video);
        if videos && self.target == 3 {
            self.target = 2;
        }
    }

    pub fn targets(kind: Kind) -> &'static [&'static str] {
        match kind {
            Kind::Video | Kind::Audio => &VIDEO_TARGETS,
            Kind::Pdf => &PDF_TARGETS,
            _ => &IMAGE_TARGETS,
        }
    }

    pub fn conversions(kind: Kind) -> &'static [&'static str] {
        match kind {
            Kind::Video => &CONVERT_VIDEO,
            Kind::Audio => &CONVERT_AUDIO,
            Kind::Pdf => &CONVERT_PDF,
            _ => &CONVERT_IMAGE,
        }
    }

    fn menu(&self, list: &'static [&'static str]) -> Vec<(&'static str, bool)> {
        list.iter()
            .map(|&label| (label, !self.unavailable.contains(&label)))
            .collect()
    }
}

/// A picker for one of the option lists; `get`/`set` read and write its index.
macro_rules! choice {
    ($panel:expr, $cx:expr, $id:expr, $list:expr, $field:ident) => {{
        let list: &'static [&'static str] = $list;
        let at = $panel.options.$field.min(list.len() - 1);
        let menu = $panel.options.menu(list);
        picker($id, list[at], true, {
            let this = $cx.entity().downgrade();
            move |_, cx| {
                let menu = menu.clone();
                this.update(cx, |panel, cx| {
                    panel.choose(menu, at, |o, i| o.$field = i, cx);
                })
                .ok();
            }
        })
    }};
}

impl Panel {
    /// The options card for the chosen action, or `None` when it asks nothing.
    pub fn options_card(&self, cx: &mut Context<Self>) -> Option<Div> {
        let action = self.action?;
        let kind = self.files.first()?.kind;
        let o = &self.options;
        let this = cx.entity().downgrade();
        let set = move |f: fn(&mut Panel, f32)| {
            let this = this.clone();
            move |v: f32, _: &mut gpui::Window, cx: &mut gpui::App| {
                this.update(cx, |panel, cx| {
                    f(panel, v);
                    cx.notify();
                })
                .ok();
            }
        };
        let this = cx.entity().downgrade();
        let seg = move |f: fn(&mut Panel, usize)| {
            let this = this.clone();
            move |i: usize, _: &mut gpui::Window, cx: &mut gpui::App| {
                this.update(cx, |panel, cx| {
                    f(panel, i);
                    cx.notify();
                })
                .ok();
            }
        };
        let quality = || {
            row(
                "Quality",
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(slider(
                        "quality",
                        (o.quality - 0.1) / 0.9,
                        116.0,
                        set(|p, v| p.options.quality = 0.1 + v * 0.9),
                    ))
                    .child(
                        div()
                            .w(px(32.0))
                            .text_right()
                            .text_xs()
                            .child(format!("{:.0}%", o.quality * 100.0)),
                    ),
            )
        };
        let rows: Vec<Div> = match action {
            Action::Compress => {
                let mut rows = vec![row(
                    "Compress by",
                    segmented(
                        "by",
                        &["Quality", "File size"],
                        o.by_size as usize,
                        seg(|p, i| p.options.by_size = i == 1),
                    ),
                )];
                if o.by_size {
                    rows.push(row(
                        "Target size",
                        choice!(self, cx, "target", Options::targets(kind), target),
                    ));
                } else if kind == Kind::Pdf {
                    rows.push(row(
                        "Quality",
                        segmented(
                            "pdfq",
                            &PDF_QUALITY,
                            o.pdf_quality,
                            seg(|p, i| p.options.pdf_quality = i),
                        ),
                    ));
                } else if kind != Kind::Audio {
                    rows.push(quality());
                }
                match kind {
                    Kind::Image => {
                        rows.push(row(
                            "Image format",
                            choice!(self, cx, "format", &IMAGE_FORMATS, image_format),
                        ));
                        rows.push(row("Largest side", choice!(self, cx, "side", &SIDES, side)));
                    }
                    Kind::Video => rows.push(row(
                        "Resolution",
                        choice!(self, cx, "res", &RESOLUTIONS, resolution),
                    )),
                    Kind::Audio if !o.by_size => rows.push(row(
                        "Bitrate",
                        choice!(self, cx, "rate", &BITRATES, bitrate),
                    )),
                    _ => {}
                }
                rows
            }
            Action::Convert => {
                let mut rows = vec![row(
                    "Convert to",
                    choice!(self, cx, "to", Options::conversions(kind), convert),
                )];
                if kind == Kind::Pdf {
                    rows.push(row("Resolution", choice!(self, cx, "dpi", &DPI, dpi)));
                }
                rows
            }
            Action::Resize => {
                let mut rows = vec![row(
                    "Resize by",
                    segmented(
                        "resize",
                        &RESIZE_BY,
                        o.resize_by,
                        seg(|p, i| p.options.resize_by = i),
                    ),
                )];
                rows.push(match o.resize_by {
                    0 => row(
                        "Largest side",
                        choice!(self, cx, "rside", &SIDES[1..], resize_side),
                    ),
                    1 => row("Scale", choice!(self, cx, "pct", &PERCENTS, percent)),
                    _ => row(
                        "Size",
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(self.field_for(Field::Width, "Width", 64.0, cx))
                            .child("×")
                            .child(self.field_for(Field::Height, "Height", 64.0, cx))
                            .child(segmented("fit", &FIT, o.fit, seg(|p, i| p.options.fit = i))),
                    ),
                });
                rows
            }
            Action::Rotate => vec![row(
                "Rotate",
                segmented(
                    "rot",
                    // PDF pages turn but don't mirror.
                    if kind == Kind::Pdf {
                        &ROTATIONS[..3]
                    } else {
                        &ROTATIONS
                    },
                    o.rotation,
                    seg(|p, i| p.options.rotation = i),
                ),
            )],
            Action::Trim | Action::MakeGif => self.range_rows(action, cx),
            Action::RemoveBackground => vec![self.background_row(cx)],
            Action::RemoveMetadata => vec![note(match kind {
                Kind::Image => {
                    "Removes the location, camera details and dates. JPEG, PNG and WebP keep every pixel as it was."
                }
                _ => "Removes the location, device and dates the file carries.",
            })],
            Action::CombinePdf => vec![
                row("Page size", choice!(self, cx, "page", &PAGES, page)),
                note("Pages follow the order of the files above."),
            ],
            Action::Zip => vec![row(
                "Name",
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(self.field_for(Field::ZipName, "Archive", 150.0, cx))
                    .child(div().text_xs().text_color(rgba(0xffffff8c)).child(".zip")),
            )],
            Action::Rename => {
                let names = rename_targets(&self.paths(), &o.rename, 1);
                let preview = match names.as_slice() {
                    [] => String::new(),
                    [one] => one.clone(),
                    [first, .., last] => format!("{first} … {last}"),
                };
                vec![
                    row(
                        "New name",
                        self.field_for(Field::RenameName, "Name", 170.0, cx),
                    ),
                    note(preview),
                ]
            }
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .px(px(14.0))
                .py(px(8.0))
                .rounded(px(12.0))
                .bg(rgba(0xffffff0d))
                .text_sm()
                .children(rows),
        )
    }

    fn field_for(
        &self,
        which: Field,
        placeholder: &'static str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let text = match which {
            Field::ZipName => &self.options.zip_name,
            Field::RenameName => &self.options.rename,
            Field::Width => &self.options.width,
            Field::Height => &self.options.height,
        };
        field(
            match which {
                Field::ZipName => "zip-name",
                Field::RenameName => "rename",
                Field::Width => "width",
                Field::Height => "height",
            },
            text,
            placeholder,
            self.field == Some(which),
            width,
        )
        .on_click(cx.listener(move |panel, _, _, cx| {
            panel.field = Some(which);
            cx.notify();
        }))
    }
}
