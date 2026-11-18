//! The app's on/off settings, in one list that both the tray icon's menu and the settings
//! window are built from. A new toggle is a variant here and an arm in each match below.

use crate::win32::sendto;
use crate::{activity, autostart, clipboard, hud, look};

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
}

impl Setting {
    /// In the order the menu and the window show them.
    pub const ALL: [Setting; 6] = [
        Setting::Black,
        Setting::Autostart,
        Setting::SendTo,
        Setting::Clipboard,
        Setting::VolumeHud,
        Setting::LiveActivities,
    ];

    /// The label, with `&` before the menu's mnemonic.
    pub fn menu_label(self) -> &'static str {
        match self {
            Setting::Black => "&Black notch",
            Setting::Autostart => "Start with &Windows",
            Setting::SendTo => "Show in Sen&d to",
            Setting::Clipboard => "Clipboard &history",
            Setting::VolumeHud => "&Volume HUD",
            Setting::LiveActivities => "&Live Activities",
        }
    }

    pub fn label(self) -> String {
        self.menu_label().replace('&', "")
    }

    /// Read from where the setting is kept, so every surface agrees with it.
    pub fn is_on(self) -> bool {
        match self {
            Setting::Black => look::is_black(),
            Setting::Autostart => autostart::is_enabled(),
            Setting::SendTo => sendto::is_enabled(),
            Setting::Clipboard => clipboard::is_enabled(),
            Setting::VolumeHud => hud::takes_volume_keys(),
            Setting::LiveActivities => activity::is_enabled(),
        }
    }

    pub fn set(self, on: bool) -> windows::core::Result<()> {
        match self {
            Setting::Black => look::set_black(on),
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
