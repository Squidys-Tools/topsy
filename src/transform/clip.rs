//! Trim and Make GIF's range, after Droppy's: the clip as a strip of frames with a yellow range
//! over it, dragged by either end, and the times it covers. Frames come from FFmpeg on a thread
//! of their own; until FFmpeg is here, the panel offers to get it.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use gpui::{
    AsyncApp, Bounds, Context, Div, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels,
    RenderImage, Stateful, canvas, div, img, prelude::*, px, relative, rgb, rgba,
};
use image::{Frame, RgbaImage};

use super::fetch::Status;
use super::options::{FPS, GIF_WIDTHS, TRIM_MODES};
use super::video;
use super::widgets::{button, note, row, segmented};
use super::{Panel, State};
use crate::log::LogErr;
use crate::model::transform::{Action, Kind};
use crate::model::video::{GIF_MAX, TrimRange};
use crate::{icon, log};

const FRAMES: usize = 6;
const STRIP_HEIGHT: f32 = 44.0;
/// Frames are drawn this many pixels tall, at most; the strip shows them a little smaller.
const FRAME_SIDE: u32 = 160;
const YELLOW: u32 = 0xffd60a;
/// The range never closes up past this fraction of the clip.
const MIN_GAP: f32 = 0.01;

pub struct Clip {
    pub path: PathBuf,
    pub duration: Option<Duration>,
    pub picture: bool,
    pub frames: Vec<Option<Arc<RenderImage>>>,
    pub failed: Option<String>,
}

enum Loaded {
    Probed { duration: Duration, picture: bool },
    Frame(usize, Arc<RenderImage>),
    Failed(String),
}

impl Panel {
    /// Load the clip the range is over when Trim or Make GIF is showing a new file.
    pub fn show_clip(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.action, Some(Action::Trim | Action::MakeGif)) {
            return;
        }
        let Some(file) = self.files.first() else {
            return;
        };
        if self.clip.as_ref().is_some_and(|c| c.path == file.path)
            || video::status() != Status::Ready
        {
            return;
        }
        let (path, picture) = (file.path.clone(), file.kind == Kind::Video);
        self.forget_frames(cx);
        self.clip = Some(Clip {
            path: path.clone(),
            duration: None,
            picture,
            frames: vec![None; FRAMES],
            failed: None,
        });
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || load(&path, &tx));
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            while let Ok(loaded) = rx.recv().await {
                let live = this.update(cx, |panel, cx| {
                    panel.loaded(loaded);
                    cx.notify();
                });
                if live.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn loaded(&mut self, loaded: Loaded) {
        let action = self.action;
        let Some(clip) = self.clip.as_mut() else {
            return;
        };
        match loaded {
            Loaded::Probed { duration, picture } => {
                clip.duration = Some(duration);
                clip.picture = picture;
                self.options.range = (0.0, 1.0);
                if action == Some(Action::MakeGif) {
                    self.fit_gif_range();
                }
            }
            Loaded::Frame(i, frame) => clip.frames[i] = Some(frame),
            Loaded::Failed(message) => clip.failed = Some(message),
        }
    }

    pub fn forget_frames(&mut self, cx: &mut Context<Self>) {
        if let Some(clip) = self.clip.take() {
            icon::forget(clip.frames.into_iter().flatten().collect(), cx);
        }
    }

    /// A GIF is at most `GIF_MAX` long: a longer range is cut back from its end.
    fn fit_gif_range(&mut self) {
        let Some(duration) = self.clip.as_ref().and_then(|c| c.duration) else {
            return;
        };
        let most = GIF_MAX.as_secs_f32() / duration.as_secs_f32().max(0.001);
        let (start, end) = self.options.range;
        if end - start > most {
            self.options.range.1 = start + most;
        }
    }

    /// The range as times, if the clip is loaded and the range is one the engine takes.
    pub fn trim_range(&self) -> Option<TrimRange> {
        let duration = self.clip.as_ref()?.duration?;
        let (start, end) = self.options.range;
        let at = |f: f32| duration.mul_f32(f.clamp(0.0, 1.0));
        TrimRange::new(at(start), at(end), duration).ok()
    }

    /// Download FFmpeg from the range card, with the run's progress bar and Cancel.
    fn get_ffmpeg(&mut self, cx: &mut Context<Self>) {
        if self.running() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.state = State::Fetching {
            progress: 0.0,
            cancel: cancel.clone(),
        };
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || {
            let report = |p: f32| {
                tx.try_send(Err(p)).log_err();
            };
            tx.send_blocking(Ok(video::prepare(&report, &cancel)))
                .log_err();
        });
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            while let Ok(message) = rx.recv().await {
                let live = this.update(cx, |panel, cx| {
                    match message {
                        Err(p) => {
                            if let State::Fetching { progress, .. } = &mut panel.state {
                                *progress = p;
                            }
                        }
                        Ok(Ok(())) => {
                            panel.state = State::Ready;
                            panel.show_clip(cx);
                        }
                        Ok(Err(err)) if err.kind() == std::io::ErrorKind::Interrupted => {
                            panel.state = State::Ready;
                        }
                        Ok(Err(err)) => panel.state = State::Failed(err.to_string()),
                    }
                    cx.notify();
                });
                if live.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub fn range_rows(&self, action: Action, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(clip) = &self.clip else {
            return vec![
                note("Trimming and GIFs use FFmpeg, a 115 MB download the first time."),
                div().flex().justify_end().child(
                    button("get-ffmpeg", "Get FFmpeg")
                        .on_click(cx.listener(|panel, _, _, cx| panel.get_ffmpeg(cx))),
                ),
            ];
        };
        if let Some(failed) = &clip.failed {
            return vec![note(failed.clone())];
        }
        let Some(duration) = clip.duration else {
            return vec![note("Loading the clip…")];
        };
        let (start, end) = self.options.range;
        let at = |f: f32| duration.mul_f32(f);
        let length = at(end).saturating_sub(at(start));
        let mut summary = format!(
            "{} – {} · {}",
            clock(at(start)),
            clock(at(end)),
            span(length)
        );
        if action == Action::MakeGif && length > GIF_MAX {
            summary.push_str(" · a GIF is at most 30 s");
        }
        let mut rows = vec![div().child(self.strip(clip, cx)), note(summary)];
        let this = cx.entity().downgrade();
        let seg = move |f: fn(&mut Panel, usize)| {
            let this = this.clone();
            move |i: usize, _: &mut gpui::Window, cx: &mut gpui::App| {
                this.update(cx, |panel, cx| {
                    if panel.edit() {
                        f(panel, i);
                        cx.notify();
                    }
                })
                .ok();
            }
        };
        if action == Action::Trim {
            rows.push(row(
                "Cut",
                segmented(
                    "trim-mode",
                    &TRIM_MODES,
                    self.options.trim_fast as usize,
                    seg(|p, i| p.options.trim_fast = i == 1),
                ),
            ));
        } else {
            rows.push(row(
                "Frame rate",
                segmented("fps", &FPS, self.options.fps, seg(|p, i| p.options.fps = i)),
            ));
            rows.push(row(
                "Width",
                segmented(
                    "gif-width",
                    &GIF_WIDTHS,
                    self.options.gif_width,
                    seg(|p, i| p.options.gif_width = i),
                ),
            ));
        }
        rows
    }

    /// The frames, the dimmed outside of the range and the yellow bracket around it.
    fn strip(&self, clip: &Clip, cx: &mut Context<Self>) -> Stateful<Div> {
        let (start, end) = self.options.range;
        let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let at = {
            let bounds = bounds.clone();
            move |x: Pixels| {
                let b = bounds.get();
                (b.size.width > px(0.0)).then(|| ((x - b.origin.x) / b.size.width).clamp(0.0, 1.0))
            }
        };
        let this = cx.entity().downgrade();
        let drag = move |x: Pixels, cx: &mut gpui::App| {
            if let Some(f) = at(x) {
                this.update(cx, |panel, cx| {
                    if panel.edit() {
                        panel.drag_range(f);
                        cx.notify();
                    }
                })
                .ok();
            }
        };
        let down = drag.clone();
        let frames = clip.frames.iter().map(|frame| {
            let cell = div().flex_1().h_full().overflow_hidden();
            match frame {
                Some(frame) => {
                    cell.child(img(frame.clone()).size_full().object_fit(ObjectFit::Cover))
                }
                None => cell.bg(rgba(if clip.picture { 0xffffff0f } else { 0x0a84ff33 })),
            }
        });
        let shade = |left: f32, width: f32| {
            div()
                .absolute()
                .top_0()
                .h_full()
                .left(relative(left))
                .w(relative(width))
                .bg(rgba(0x000000a6))
        };
        div()
            .id("strip")
            .relative()
            .w_full()
            .h(px(STRIP_HEIGHT))
            .my(px(4.0))
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .size_full()
                    .flex()
                    .rounded(px(6.0))
                    .overflow_hidden()
                    .children(frames),
            )
            .child(shade(0.0, start))
            .child(shade(end, 1.0 - end))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(relative(start))
                    .w(relative(end - start))
                    .border_2()
                    .border_x(px(7.0))
                    .border_color(rgb(YELLOW))
                    .rounded(px(6.0)),
            )
            .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, cx| {
                down(e.position.x, cx)
            })
            .on_mouse_move(move |e: &MouseMoveEvent, _, cx| {
                if e.pressed_button == Some(MouseButton::Left) {
                    drag(e.position.x, cx)
                }
            })
    }

    /// Move whichever end of the range is nearer `f`.
    fn drag_range(&mut self, f: f32) {
        let (start, end) = &mut self.options.range;
        if (f - *start).abs() <= (f - *end).abs() {
            *start = f.min(*end - MIN_GAP).max(0.0);
        } else {
            *end = f.max(*start + MIN_GAP).min(1.0);
        }
        if self.action == Some(Action::MakeGif) {
            self.fit_gif_range();
        }
    }
}

/// Probe the clip, then decode its frames one by one, sending each as it arrives.
fn load(path: &std::path::Path, tx: &async_channel::Sender<Loaded>) {
    let probe = match video::probe(path) {
        Ok(probe) => probe,
        Err(err) => {
            log(&format!("clip probe failed: {err}"));
            tx.send_blocking(Loaded::Failed(err.to_string())).log_err();
            return;
        }
    };
    let picture = probe.video.is_some();
    let duration = probe.duration;
    if tx
        .send_blocking(Loaded::Probed { duration, picture })
        .is_err()
        || !picture
    {
        return;
    }
    // One short FFmpeg run per frame, side by side: the strip fills in at once, not over seconds.
    std::thread::scope(|scope| {
        for i in 0..FRAMES {
            scope.spawn(move || {
                let at = duration.mul_f32((i as f32 + 0.5) / FRAMES as f32);
                match video::frame_at(path, at, FRAME_SIDE) {
                    Ok(frame) => {
                        tx.send_blocking(Loaded::Frame(i, picture_of(frame)))
                            .log_err();
                    }
                    Err(err) => log(&format!("clip frame {i} failed: {err}")),
                }
            });
        }
    });
}

/// GPUI draws BGRA, whatever the buffer's type says.
fn picture_of(mut frame: RgbaImage) -> Arc<RenderImage> {
    for px in frame.pixels_mut() {
        px.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(frame)]))
}

/// `1:05.3`
fn clock(d: Duration) -> String {
    let tenths = d.as_millis() / 100;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// `4.8 s`, or `1:05.3` once it is a minute.
fn span(d: Duration) -> String {
    if d < Duration::from_secs(60) {
        format!("{:.1} s", d.as_secs_f32())
    } else {
        clock(d)
    }
}
