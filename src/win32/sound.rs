//! The system's own sounds.

use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::core::w;

use crate::log::LogErr;

/// The system's asterisk sound, played without waiting for it.
pub fn chime() {
    // SAFETY: a static alias name and no module; SND_ASYNC returns at once.
    let played = unsafe {
        PlaySoundW(
            w!("SystemAsterisk"),
            None,
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        )
    };
    played.ok().log_err();
}
