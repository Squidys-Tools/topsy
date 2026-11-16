//! Transform, after Droppy's: a panel that turns the files given to it into something else
//! (compressed, converted, resized, trimmed, a GIF, a PDF, a zip, renamed). It opens from a
//! chip's menu or the shelf's header, takes the focus like the settings window (the island
//! never does), and parks what it makes on the shelf. The rules are in `model::transform`;
//! the work itself is in the engines beside this file, run on a worker thread by `job`.

pub mod background;
mod clip;
mod engines;
pub mod fetch;
pub mod image;
mod job;
mod options;
pub mod pdf;
mod pictures;
mod steps;
pub mod tools;
pub mod video;
mod view;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    App, AsyncApp, Bounds, Context, Entity, FocusHandle, Global, KeyDownEvent, RenderImage,
    TitlebarOptions, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, point, prelude::*, px, size,
};
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use crate::model::transform::{Action, Kind, actions};
use crate::win32::{self, MenuEntry};
use crate::{Island, icon, log};

pub use options::Options;
pub use steps::Steps;

/// The panel's width; its height follows what it shows.
const WIDTH: f32 = 384.0;
/// The files' thumbnails, square.
const THUMB: f32 = 44.0;

struct Open(WindowHandle<Panel>);

impl Global for Open {}

pub struct File {
    pub path: PathBuf,
    pub kind: Kind,
    pub bytes: u64,
    pub thumb: Option<Arc<RenderImage>>,
}

/// Which text field has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    ZipName,
    RenameName,
    Width,
    Height,
}

pub enum State {
    Ready,
    Running {
        label: String,
        progress: f32,
        cancel: Arc<AtomicBool>,
    },
    Done {
        outputs: Vec<PathBuf>,
        message: String,
    },
    Failed(String),
}

pub struct Panel {
    island: WeakEntity<Island>,
    files: Vec<File>,
    action: Option<Action>,
    options: Options,
    field: Option<Field>,
    state: State,
    focus: FocusHandle,
    scale: f32,
    height: f32,
    /// The clip Trim and Make GIF show, once it is loaded.
    clip: Option<clip::Clip>,
}

fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    win32::hwnd_from_raw(handle.as_raw())
}

/// Open the panel with `paths`, or add them to the one that is open and bring it forward.
pub fn open(cx: &mut App, island: &Entity<Island>, paths: Vec<PathBuf>) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |panel, window, cx| {
                panel.add(paths.clone(), cx);
                if let Some(hwnd) = hwnd(window) {
                    win32::bring_to_front(hwnd);
                }
            })
            .is_ok()
    {
        return;
    }
    if !paths.iter().any(|p| p.is_file()) {
        return;
    }
    log(&format!("transform {} files", paths.len()));
    let Some((_, work, scale)) = win32::cursor_monitor() else {
        return;
    };
    let w = (WIDTH * scale).round() as i32;
    let h = (240.0 * scale).round() as i32;
    let x = work.left + (work.right - work.left - w) / 2;
    let y = work.top + (96.0 * scale).round() as i32;
    let logical = |v: i32| px(v as f32 / scale);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(logical(x), logical(y)),
            size(logical(w), logical(h)),
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Transform".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Opaque,
        window_min_size: Some(size(px(1.0), px(1.0))),
        ..Default::default()
    };
    let island = island.downgrade();
    let opened = cx.open_window(options, |window, cx| {
        if let Some(hwnd) = hwnd(window) {
            win32::apply_strip_styles(hwnd);
            win32::round_corners(hwnd);
            win32::bring_to_front(hwnd);
            let raw = hwnd.0 as usize;
            cx.defer(move |_| win32::move_window(HWND(raw as *mut _), x, y, w, h, true));
        }
        let panel = cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus);
            let mut panel = Panel {
                island,
                files: Vec::new(),
                action: None,
                options: Options::default(),
                field: None,
                state: State::Ready,
                focus,
                scale,
                height: 240.0,
                clip: None,
            };
            panel.add(paths, cx);
            panel
        });
        let weak = panel.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            weak.update(cx, |panel, _| panel.cancel()).ok();
            true
        });
        panel
    });
    match opened {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(err) => eprintln!("topsy: could not open Transform: {err}"),
    }
}

impl Panel {
    pub fn kinds(&self) -> Vec<Kind> {
        self.files.iter().map(|f| f.kind).collect()
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().map(|f| f.path.clone()).collect()
    }

    /// Add files not already in the panel, then read their sizes and thumbnails off the UI
    /// thread. The chosen action stays if it still suits them all.
    pub fn add(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.running() {
            return;
        }
        let new: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| p.is_file() && !self.files.iter().any(|f| &f.path == p))
            .collect();
        if new.is_empty() {
            return;
        }
        self.files.extend(new.iter().map(|path| File {
            kind: Kind::of(path),
            bytes: 0,
            thumb: None,
            path: path.clone(),
        }));
        self.files_changed(cx);
        let size = (THUMB * self.scale).round() as i32;
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || {
            win32::with_com(COINIT_MULTITHREADED, || {
                for path in new {
                    let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
                    let thumb = icon::load(&path, size);
                    if tx.send_blocking((path, bytes, thumb)).is_err() {
                        break;
                    }
                }
            })
        });
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            while let Ok((path, bytes, thumb)) = rx.recv().await {
                let live = this.update(cx, |this, cx| {
                    if let Some(file) = this.files.iter_mut().find(|f| f.path == path) {
                        file.bytes = bytes;
                        file.thumb = thumb;
                        cx.notify();
                    }
                });
                if live.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn remove(&mut self, at: usize, cx: &mut Context<Self>) {
        if at < self.files.len() && !self.running() {
            let file = self.files.remove(at);
            icon::forget(file.thumb.into_iter().collect(), cx);
            self.files_changed(cx);
        }
    }

    fn files_changed(&mut self, cx: &mut Context<Self>) {
        let available = actions(&self.kinds());
        if self.action.is_none_or(|a| !available.contains(&a)) {
            self.action = available.first().copied();
        }
        self.options.fit_to(&self.files);
        if !matches!(self.state, State::Running { .. }) {
            self.state = State::Ready;
        }
        self.show_clip(cx);
        cx.notify();
    }

    pub fn pick(&mut self, action: Action, cx: &mut Context<Self>) {
        if !self.running() {
            self.action = Some(action);
            self.field = None;
            self.state = State::Ready;
            self.show_clip(cx);
            cx.notify();
        }
    }

    pub fn running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }

    /// Close the panel; a job still running is cancelled first.
    pub fn close(&mut self, window: &mut Window) {
        self.cancel();
        window.remove_window();
    }

    /// Show `choices` in a menu at the cursor, and give the one picked to `apply`. The menu
    /// pumps messages, so it runs on a thread of its own.
    pub fn choose(
        &mut self,
        choices: Vec<(&'static str, bool)>,
        current: usize,
        apply: fn(&mut Options, usize),
        cx: &mut Context<Self>,
    ) {
        if self.running() {
            return;
        }
        let (tx, rx) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("topsy-transform-menu".into())
            .spawn(move || {
                let entries: Vec<MenuEntry> = (choices.iter().enumerate())
                    .map(|(i, &(label, enabled))| MenuEntry::Choice {
                        id: i + 1,
                        label,
                        checked: i == current,
                        enabled,
                    })
                    .collect();
                let _ = tx.send_blocking(win32::popup_menu(&entries));
            });
        if spawned.is_err() {
            return;
        }
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            if let Ok(Some(id)) = rx.recv().await {
                this.update(cx, |this, cx| {
                    if this.running() {
                        return;
                    }
                    apply(&mut this.options, id - 1);
                    this.state = State::Ready;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let command = key.modifiers.control || key.modifiers.alt || key.modifiers.platform;
        if let Some(field) = self.field {
            let text = self.options.text(field);
            match key.key.as_str() {
                "escape" | "tab" => self.field = None,
                "enter" => {
                    self.field = None;
                    self.start(cx);
                }
                "backspace" => {
                    text.pop();
                }
                _ => match key.key_char.as_deref() {
                    Some(typed) if !command && !typed.chars().any(char::is_control) => {
                        let digits = matches!(field, Field::Width | Field::Height);
                        if !digits || typed.chars().all(|c| c.is_ascii_digit()) {
                            text.push_str(typed);
                        }
                    }
                    _ => return,
                },
            }
            self.state = match self.state {
                State::Running { .. } => return cx.notify(),
                _ => State::Ready,
            };
            cx.stop_propagation();
            return cx.notify();
        }
        match key.key.as_str() {
            "escape" if self.running() => self.cancel(),
            "escape" => return self.close(window),
            "enter" => self.start(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Whether an option may change now: not while a run reads them. A change after a run
    /// brings the button back.
    pub fn edit(&mut self) -> bool {
        if self.running() {
            return false;
        }
        self.state = State::Ready;
        true
    }

    pub fn cancel(&mut self) {
        if let State::Running { cancel, .. } = &self.state {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Ask Windows' Open dialog for more files.
    pub fn browse(&mut self, cx: &mut Context<Self>) {
        let (tx, rx) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("topsy-transform-add".into())
            .spawn(move || {
                let _ = tx.send_blocking(win32::pick_files());
            });
        if spawned.is_err() {
            return;
        }
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            if let Ok(paths) = rx.recv().await {
                this.update(cx, |panel, cx| panel.add(paths, cx)).ok();
            }
        })
        .detach();
    }

    /// Park results on the shelf, as a zip of a chip is.
    fn shelve(&self, outputs: Vec<PathBuf>, cx: &mut Context<Self>) {
        if let Some(island) = self.island.upgrade() {
            island.update(cx, |island, cx| {
                island.add_paths(outputs, cx);
            });
        }
    }

    /// Keep the window as tall as its content, measured after layout (see `view`), from a
    /// foreground task: a resize from `render` or a GPUI callback would re-enter GPUI.
    fn fit_height(&mut self, wanted: f32, window: &mut Window, cx: &mut Context<Self>) {
        if (wanted - self.height).abs() < 0.5 {
            return;
        }
        self.height = wanted;
        let Some(hwnd) = hwnd(window) else { return };
        let scale = window.scale_factor();
        let (w, h) = (
            (WIDTH * scale).round() as i32,
            (wanted * scale).round() as i32,
        );
        let raw = hwnd.0 as usize;
        cx.spawn(async move |_, _: &mut AsyncApp| {
            win32::resize_window(HWND(raw as *mut _), w, h);
        })
        .detach();
    }
}
