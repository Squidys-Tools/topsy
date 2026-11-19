//! The "Black notch" and "Live glass" settings: values under `HKCU\Software\topsy`, absent for
//! the default glass.

use crate::autostart::RegString;

const LOOK: RegString<'static> = RegString::new(r"Software\topsy", "Look");
const LENS: RegString<'static> = RegString::new(r"Software\topsy", "Lens");

pub fn is_black() -> bool {
    LOOK.value().as_deref() == Some("black")
}

pub fn set_black(black: bool) -> windows::core::Result<()> {
    if black {
        LOOK.set("black")
    } else {
        LOOK.remove()
    }
}

/// Whether the open notch's lens glass follows the screen, rather than reading it once as the
/// notch opens.
pub fn is_live_lens() -> bool {
    LENS.value().as_deref() == Some("live")
}

pub fn set_live_lens(live: bool) -> windows::core::Result<()> {
    if live {
        LENS.set("live")
    } else {
        LENS.remove()
    }
}
