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
mod headset;
mod hud;
mod icon;
mod icons;
mod install;
mod island;
mod log;
mod look;
mod new_folder;
mod now_playing;
mod ring;
mod settings;
mod state;
mod transform;
mod tray;
mod update;
mod widgets;
mod win32;

use crate::frame::Mode;
use crate::island::Island;
use crate::log::log;

fn main() {
    app::run();
}
