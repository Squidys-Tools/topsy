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
    Brightness => "brightness",
    Close => "close",
    Combine => "combine",
    Compress => "compress",
    Convert => "convert",
    File => "file",
    Gif => "gif",
    More => "more",
    Next => "next",
    Open => "open",
    Pause => "pause",
    Play => "play",
    Previous => "previous",
    RemoveBackground => "remove-background",
    RemoveMetadata => "remove-metadata",
    Rename => "rename",
    Resize => "resize",
    Rotate => "rotate",
    Search => "search",
    Star => "star",
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
