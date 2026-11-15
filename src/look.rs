//! The "Black notch" setting: one value under `HKCU\Software\topsy`, absent for glass.

use crate::autostart::RegString;

const LOOK: RegString<'static> = RegString::new(r"Software\topsy", "Look");

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
