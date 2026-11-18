// A release build is a GUI app: a console window would take the focus at launch. Debug keeps it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod app;
mod archive;
mod autostart;
mod backdrop;
mod basket;
mod booking;
mod clipboard;
mod config;
mod frame;
mod hud;
mod icon;
mod icons;
mod island;
mod log;
mod look;
mod now_playing;
mod settings;
mod state;
mod transform;
mod tray;
mod widgets;
mod win32;

use crate::frame::Mode;
use crate::island::Island;
use crate::log::log;

fn main() {
    app::run();
}
