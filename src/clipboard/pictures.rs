//! What the palette shows that Windows has to look up: app icons, copied images, file
//! thumbnails, site icons, and the names apps give themselves. Each is loaded once, off the UI
//! thread, as it is first shown.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, RenderImage};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use super::favicon;
use crate::{icon, win32};

/// Something the palette wants looked up.
pub enum Want {
    /// The shell's picture of a file, about this many logical pixels square.
    Shell(PathBuf, f32),
    /// A site's icon, kept at the path.
    Site(PathBuf, String),
    /// The name an exe gives itself.
    Name(PathBuf),
}

/// One picture at one size, or one name; a file is wanted at more than one size.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Picture(PathBuf, u32),
    Name(PathBuf),
}

enum Got {
    Picture(Arc<RenderImage>),
    Name(String),
}

impl Want {
    fn key(&self) -> Key {
        match self {
            Want::Shell(path, size) => Key::Picture(path.clone(), size.round() as u32),
            Want::Site(path, _) => Key::Picture(path.clone(), 0),
            Want::Name(exe) => Key::Name(exe.clone()),
        }
    }

    /// Names are quick, the shell's pictures nearly so; the network waits behind them.
    fn order(&self) -> u8 {
        match self {
            Want::Name(_) => 0,
            Want::Shell(..) => 1,
            Want::Site(..) => 2,
        }
    }

    fn load(&self, scale: f32) -> Option<Got> {
        match self {
            Want::Shell(path, size) => {
                icon::load(path, (size * scale).round() as i32).map(Got::Picture)
            }
            Want::Site(path, host) => favicon::load(path, host).map(Got::Picture),
            Want::Name(exe) => win32::exe_description(exe).map(Got::Name),
        }
    }
}

#[derive(Default)]
pub struct Pictures {
    loaded: HashMap<Key, Got>,
    asked: HashSet<Key>,
}

impl Pictures {
    /// The shell's picture of `path` at `size`, as asked for with [`Want::Shell`].
    pub fn get(&self, path: &Path, size: f32) -> Option<Arc<RenderImage>> {
        self.picture(Key::Picture(path.to_path_buf(), size.round() as u32))
    }

    /// A site's icon kept at `path`.
    pub fn site(&self, path: &Path) -> Option<Arc<RenderImage>> {
        self.picture(Key::Picture(path.to_path_buf(), 0))
    }

    fn picture(&self, key: Key) -> Option<Arc<RenderImage>> {
        match self.loaded.get(&key)? {
            Got::Picture(picture) => Some(picture.clone()),
            Got::Name(_) => None,
        }
    }

    /// What `exe` calls itself, else its file name.
    pub fn name(&self, exe: &Path) -> String {
        match self.loaded.get(&Key::Name(exe.to_path_buf())) {
            Some(Got::Name(name)) => name.clone(),
            Some(Got::Picture(_)) | None => (exe.file_stem())
                .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
        }
    }

    /// Look up what is wanted and not asked for before, on a worker thread of its own (the
    /// shell wants COM there); `pictures` finds this store in the view, which is redrawn as each
    /// arrives.
    pub fn fetch<T: 'static>(
        &mut self,
        wants: impl IntoIterator<Item = Want>,
        scale: f32,
        cx: &mut Context<T>,
        pictures: fn(&mut T) -> &mut Pictures,
    ) {
        let mut wanted: Vec<Want> = wants
            .into_iter()
            .filter(|want| self.asked.insert(want.key()))
            .collect();
        if wanted.is_empty() {
            return;
        }
        wanted.sort_by_key(Want::order);
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || {
            win32::with_com(COINIT_MULTITHREADED, || {
                for want in wanted {
                    if tx.send_blocking((want.key(), want.load(scale))).is_err() {
                        break;
                    }
                }
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok((key, got)) = rx.recv().await {
                let Some(got) = got else {
                    continue;
                };
                let updated = this.update(cx, |this, cx| {
                    pictures(this).loaded.insert(key, got);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}
