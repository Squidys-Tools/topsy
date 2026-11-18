//! The "Black notch" and "Liquid glass" settings: values under `HKCU\Software\topsy`, absent
//! when off.

use crate::autostart::RegString;

const LOOK: RegString<'static> = RegString::new(r"Software\topsy", "Look");
const REFRACT: RegString<'static> = RegString::new(r"Software\topsy", "Refract");

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

pub fn is_refracting() -> bool {
    REFRACT.value().as_deref() == Some("on")
}

pub fn set_refracting(on: bool) -> windows::core::Result<()> {
    if on {
        REFRACT.set("on")
    } else {
        REFRACT.remove()
    }
}
