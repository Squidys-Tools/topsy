//! The `TOPSY_*` switches (README, Running), read once at startup. The rest of the app asks
//! [`get`] and never the environment, so a new switch is one field here and one README line.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use crate::{Glass, Mode};

pub struct Config {
    /// `TOPSY_LOG`: trace events to stderr, for the live checks.
    pub log: bool,
    /// `TOPSY_GLASS=acrylic|tint|solid|clear`: force a fill. Any value also overrides the
    /// *Black notch* setting; an unknown one forces the default blur.
    pub glass: Option<Glass>,
    /// What the island folds back to: `TOPSY_IDLE=peek|tray` keeps it down for the live checks,
    /// and without the mouse hook nothing could reveal it, so it peeks.
    pub rest: Mode,
    /// `TOPSY_NO_HOOK`: no global mouse hook, to measure idle cost without hook traffic.
    pub no_hook: bool,
    /// `TOPSY_NO_TRAY`: no notification-area icon.
    pub no_tray: bool,
    /// `TOPSY_DEMO`: open and fold on a timer, to watch the motion without a mouse.
    pub demo: bool,
    /// `TOPSY_SEED=path;path`: files put on the shelf at startup.
    pub seed: Option<Vec<PathBuf>>,
    /// `TOPSY_EXIT_AFTER_MS`: quit after this long.
    pub exit_after: Option<Duration>,
    /// `TOPSY_STATE`: where the shelf is saved; empty turns saving off.
    pub state: Option<OsString>,
}

impl Config {
    fn parse(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name| var(name).and_then(|v| v.into_string().ok());
        let no_hook = var("TOPSY_NO_HOOK").is_some();
        Self {
            log: var("TOPSY_LOG").is_some(),
            glass: text("TOPSY_GLASS").map(|v| match v.as_str() {
                "acrylic" => Glass::Acrylic,
                "tint" => Glass::Tint,
                "solid" => Glass::Solid,
                "clear" => Glass::Clear,
                _ => Glass::Blur,
            }),
            rest: match text("TOPSY_IDLE").as_deref() {
                Some("peek") => Mode::Peek,
                Some("tray") => Mode::Tray,
                _ if no_hook => Mode::Peek,
                _ => Mode::Hidden,
            },
            no_hook,
            no_tray: var("TOPSY_NO_TRAY").is_some(),
            demo: var("TOPSY_DEMO").is_some(),
            seed: var("TOPSY_SEED").map(|s| std::env::split_paths(&s).collect()),
            exit_after: text("TOPSY_EXIT_AFTER_MS")
                .and_then(|v| v.parse().ok())
                .map(Duration::from_millis),
            state: var("TOPSY_STATE"),
        }
    }
}

pub fn get() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| Config::parse(|name| std::env::var_os(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(vars: &[(&str, &str)]) -> Config {
        Config::parse(|name| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| OsString::from(v))
        })
    }

    #[test]
    fn nothing_set_is_the_real_app() {
        let c = config(&[]);
        assert_eq!(c.rest, Mode::Hidden);
        assert_eq!(c.glass, None);
        assert!(!c.no_hook && !c.no_tray && !c.demo && !c.log);
        assert!(c.seed.is_none() && c.exit_after.is_none() && c.state.is_none());
    }

    #[test]
    fn without_the_hook_the_notch_rests_at_peek_unless_told_otherwise() {
        assert_eq!(config(&[("TOPSY_NO_HOOK", "1")]).rest, Mode::Peek);
        assert_eq!(
            config(&[("TOPSY_NO_HOOK", "1"), ("TOPSY_IDLE", "tray")]).rest,
            Mode::Tray
        );
        assert_eq!(config(&[("TOPSY_IDLE", "bogus")]).rest, Mode::Hidden);
    }

    #[test]
    fn any_glass_value_forces_a_fill() {
        assert_eq!(config(&[("TOPSY_GLASS", "tint")]).glass, Some(Glass::Tint));
        assert_eq!(config(&[("TOPSY_GLASS", "blur")]).glass, Some(Glass::Blur));
        assert_eq!(config(&[("TOPSY_GLASS", "")]).glass, Some(Glass::Blur));
    }

    #[test]
    fn exit_after_ignores_junk_and_seed_splits_paths() {
        assert_eq!(
            config(&[("TOPSY_EXIT_AFTER_MS", "250")]).exit_after,
            Some(Duration::from_millis(250))
        );
        assert_eq!(config(&[("TOPSY_EXIT_AFTER_MS", "soon")]).exit_after, None);
        let seed = config(&[("TOPSY_SEED", r"C:\a.txt;C:\b")]).seed.unwrap();
        assert_eq!(seed, [PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b")]);
    }
}
