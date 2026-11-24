//! The app's on/off settings, in one list that both the tray icon's menu and the settings
//! window are built from. A new toggle is a variant here and an arm in each match below.

mod controls;
mod lineup;
mod pages;
mod previews;
pub(crate) mod window;

use crate::win32::sendto;
use crate::{activity, autostart, clipboard, hud, look, update};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Black,
    Autostart,
    /// "Send to > topsy" in Explorer's right-click menu.
    SendTo,
    /// Keep what is copied (`clipboard`).
    Clipboard,
    /// Take the volume keys and show topsy's HUD for them (`hud`).
    VolumeHud,
    /// Downloads and Bluetooth audio in the notch (`activity`).
    LiveActivities,
    /// The open notch's glass follows the screen (`look`).
    LiveGlass,
    /// A nightly checks for a newer one and downloads it (`update`).
    Updates,
}

impl Setting {
    /// In the order the menu and the window show them.
    pub const ALL: [Setting; 8] = [
        Setting::Black,
        Setting::LiveGlass,
        Setting::Autostart,
        Setting::SendTo,
        Setting::Clipboard,
        Setting::VolumeHud,
        Setting::LiveActivities,
        Setting::Updates,
    ];

    /// The label, with `&` before the menu's mnemonic.
    pub fn menu_label(self) -> &'static str {
        match self {
            Setting::Black => "&Black notch",
            Setting::LiveGlass => "Live &glass",
            Setting::Autostart => "Start with &Windows",
            Setting::SendTo => "Show in Sen&d to",
            Setting::Clipboard => "Clipboard &history",
            Setting::VolumeHud => "&Volume HUD",
            Setting::LiveActivities => "&Live Activities",
            Setting::Updates => "Check for &updates",
        }
    }

    pub fn label(self) -> String {
        self.menu_label().replace('&', "")
    }

    /// The line under its label in the settings window.
    pub fn description(self) -> &'static str {
        match self {
            Setting::Black => "Draw the notch plain black instead of glass",
            Setting::LiveGlass => {
                "Keep the glass in step with the screen behind it while the tray is open"
            }
            Setting::Autostart => "Start topsy when you sign in",
            Setting::SendTo => "Send files to the shelf from Explorer's right-click menu",
            Setting::Clipboard => "Keep what you copy, to paste it again later",
            Setting::VolumeHud => "Show the volume keys' level in the notch, in place of Windows'",
            Setting::LiveActivities => "Show downloads and Bluetooth audio in the notch",
            Setting::Updates => "Download newer nightlies quietly, to install on restart",
        }
    }

    /// Read from where the setting is kept, so every surface agrees with it.
    pub fn is_on(self) -> bool {
        match self {
            Setting::Black => look::is_black(),
            Setting::LiveGlass => look::is_live_lens(),
            Setting::Autostart => autostart::is_enabled(),
            Setting::SendTo => sendto::is_enabled(),
            Setting::Clipboard => clipboard::is_enabled(),
            Setting::VolumeHud => hud::takes_volume_keys(),
            Setting::LiveActivities => activity::is_enabled(),
            Setting::Updates => update::is_enabled(),
        }
    }

    pub fn set(self, on: bool) -> windows::core::Result<()> {
        match self {
            Setting::Black => look::set_black(on),
            Setting::LiveGlass => look::set_live_lens(on),
            Setting::Autostart if on => std::env::current_exe()
                .map_err(windows::core::Error::from)
                .and_then(|exe| autostart::enable(&exe)),
            Setting::Autostart => autostart::disable(),
            Setting::SendTo if on => std::env::current_exe()
                .map_err(windows::core::Error::from)
                .and_then(|exe| sendto::enable(&exe)),
            Setting::SendTo => sendto::disable(),
            Setting::Clipboard => clipboard::set_enabled(on),
            Setting::VolumeHud => hud::set_volume_keys(on),
            Setting::LiveActivities => activity::set_enabled(on),
            Setting::Updates => update::set_enabled(on),
        }
    }

    /// Flip the setting and save it. The new value, or `None` if it could not be saved.
    pub fn toggle(self) -> Option<bool> {
        let on = !self.is_on();
        match self.set(on) {
            Ok(()) => Some(on),
            Err(err) => {
                eprintln!("topsy: could not change {}: {err}", self.label());
                None
            }
        }
    }
}
