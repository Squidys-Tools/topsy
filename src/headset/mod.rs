//! Headsets beyond their connection: the catalog that names and shapes them
//! (`devices/catalog.toml`, rules in `model::headset`), a fake one for screenshots
//! (`TOPSY_FAKE_DEVICE`), and *Add my headset*, which hands what topsy sees of the user's
//! devices to a coding agent to add them to the catalog. Their Live Activity is in `activity`.

pub mod art;
pub mod report;

use std::sync::OnceLock;

use crate::config;
use model::headset::catalog::{BUILTIN, Catalog, Fake};

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        Catalog::parse(BUILTIN).unwrap_or_else(|err| {
            eprintln!("topsy: the headset catalog doesn't parse: {err}");
            Catalog::default()
        })
    })
}

/// The device `TOPSY_FAKE_DEVICE` asks to show as connected, if it names one.
pub fn fake() -> Option<Fake> {
    let spec = config::get().fake_device.as_deref()?;
    let fake = catalog().fake(spec);
    if fake.is_none() {
        eprintln!("topsy: TOPSY_FAKE_DEVICE={spec} names no catalog entry or shape");
    }
    fake
}
