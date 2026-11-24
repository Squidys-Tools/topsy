//! The interface's icons: Hugeicons' free set (MIT, `assets/icons/LICENSE.md`), built into the exe
//! and drawn by GPUI as SVG masks in the colour each use gives.

use std::borrow::Cow;

use gpui::{AssetSource, Hsla, Result, SharedString, Svg, prelude::*, px, svg};

macro_rules! icons {
    ($($name:ident => $file:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon { $($name,)* }

        impl Icon {
            fn path(self) -> &'static str {
                match self { $(Icon::$name => concat!("icons/", $file, ".svg"),)* }
            }
        }

        fn bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $file, ".svg") =>
                    Some(include_bytes!(concat!("../assets/icons/", $file, ".svg"))),)*
                _ => None,
            }
        }
    };
}

icons! {
    Add => "add",
    Archive => "archive",
    Basket => "basket",
    Brightness => "brightness",
    Buds => "buds",
    CheckCircle => "check-circle",
    ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    Close => "close",
    Cloud => "cloud",
    Combine => "combine",
    Compress => "compress",
    Convert => "convert",
    Copy => "copy",
    Cpu => "cpu",
    Delete => "delete",
    Download => "download",
    Drizzle => "drizzle",
    File => "file",
    Fog => "fog",
    FolderExport => "folder-export",
    Gif => "gif",
    Headphones => "headphones",
    Home => "home",
    Inbox => "inbox",
    Location => "location",
    Minus => "minus",
    Moon => "moon",
    MoonCloud => "moon-cloud",
    Next => "next",
    Open => "open",
    Pause => "pause",
    Play => "play",
    Previous => "previous",
    Rain => "rain",
    Ram => "ram",
    RemoveBackground => "remove-background",
    RemoveMetadata => "remove-metadata",
    Rename => "rename",
    Reset => "reset",
    Resize => "resize",
    Rotate => "rotate",
    Search => "search",
    Settings => "settings",
    Shrink => "shrink",
    Snow => "snow",
    Speaker => "speaker",
    Star => "star",
    StemBuds => "stem-buds",
    Sun => "sun",
    SunCloud => "sun-cloud",
    Thunder => "thunder",
    Tick => "tick",
    Timer => "timer",
    Tray => "tray",
    Trim => "trim",
    Unfold => "unfold",
    VolumeHigh => "volume-high",
    VolumeLow => "volume-low",
    VolumeMute => "volume-mute",
    VolumeOff => "volume-off",
    Zip => "zip",
}

/// The icons, for `Application::with_assets`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(bytes(path).map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// `icon` drawn `size` px square in `color`.
pub fn icon(icon: Icon, size: f32, color: impl Into<Hsla>) -> Svg {
    svg()
        .path(icon.path())
        .size(px(size))
        .flex_none()
        .text_color(color)
}
