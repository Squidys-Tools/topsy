//! The `TOPSY_*` switches (docs/operations/running.md), read once at startup. The rest of the app
//! asks [`get`] and never the environment, so a new switch is one field here and one row there.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use crate::frame::{Glass, Mode};

pub struct Config {
    /// `TOPSY_LOG`: trace events to stderr, for the live checks.
    pub log: bool,
    /// `TOPSY_GLASS=host|acrylic|tint|solid|clear`: force a fill. Any value also overrides the
    /// *Black notch* setting; an unknown one forces the default blur.
    pub glass: Option<Glass>,
    /// What the island folds back to: `TOPSY_IDLE=peek|tray` keeps it down for the live checks,
    /// and without the mouse hook nothing could reveal it, so it peeks.
    pub rest: Mode,
    /// `TOPSY_NO_HOOK`: no global mouse hook, to measure idle cost without hook traffic.
    pub no_hook: bool,
    /// `TOPSY_NO_TRAY`: no notification-area icon.
    pub no_tray: bool,
    /// `TOPSY_NO_MEDIA`: don't watch the system's media session (Now Playing).
    pub no_media: bool,
    /// `TOPSY_NO_KEYS`: leave the volume keys to Windows, whatever the setting says.
    pub no_keys: bool,
    /// `TOPSY_NO_WIDGETS`: no shelf widgets in the tray, whatever the lineup says, so the live
    /// checks find the shelf where they expect it.
    pub no_widgets: bool,
    /// `TOPSY_DEMO`: open and fold on a timer, to watch the motion without a mouse.
    pub demo: bool,
    /// `TOPSY_SEED=path;path`: files put on the shelf at startup.
    pub seed: Option<Vec<PathBuf>>,
    /// `TOPSY_TRANSFORM=path;path`: open the Transform panel with these files at startup.
    pub transform: Option<Vec<PathBuf>>,
    /// `TOPSY_EXIT_AFTER_MS`: quit after this long.
    pub exit_after: Option<Duration>,
    /// `TOPSY_STATE`: where the shelf is saved (the basket goes beside it); empty turns saving off.
    pub state: Option<OsString>,
    /// Where clipboard history is kept: `TOPSY_CLIPBOARD` (empty: nowhere), else
    /// `%LOCALAPPDATA%\topsy\clipboard`. A run with `TOPSY_STATE` set is scripted and keeps
    /// none unless `TOPSY_CLIPBOARD` says where, so a live check never writes the real history.
    pub clipboard: Option<PathBuf>,
}

impl Config {
    fn parse(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name| var(name).and_then(|v| v.into_string().ok());
        let no_hook = var("TOPSY_NO_HOOK").is_some();
        Self {
            log: var("TOPSY_LOG").is_some(),
            glass: text("TOPSY_GLASS").map(|v| match v.as_str() {
                "host" => Glass::HostBlur,
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
            no_media: var("TOPSY_NO_MEDIA").is_some(),
            no_keys: var("TOPSY_NO_KEYS").is_some(),
            no_widgets: var("TOPSY_NO_WIDGETS").is_some(),
            demo: var("TOPSY_DEMO").is_some(),
            seed: var("TOPSY_SEED").map(|s| std::env::split_paths(&s).collect()),
            transform: var("TOPSY_TRANSFORM").map(|s| std::env::split_paths(&s).collect()),
            exit_after: text("TOPSY_EXIT_AFTER_MS")
                .and_then(|v| v.parse().ok())
                .map(Duration::from_millis),
            clipboard: match var("TOPSY_CLIPBOARD") {
                Some(dir) => (!dir.is_empty()).then(|| dir.into()),
                None if var("TOPSY_STATE").is_some() => None,
                None => var("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"topsy\clipboard")),
            },
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
        assert!(!c.no_hook && !c.no_tray && !c.no_media && !c.no_keys && !c.demo && !c.log);
        assert!(!c.no_widgets && config(&[("TOPSY_NO_WIDGETS", "1")]).no_widgets);
        assert!(config(&[("TOPSY_NO_MEDIA", "1")]).no_media);
        assert!(c.seed.is_none() && c.exit_after.is_none() && c.state.is_none());
        assert!(c.transform.is_none());
    }

    #[test]
    fn scripted_runs_keep_no_clipboard_history_unless_told_where() {
        let home = ("LOCALAPPDATA", r"C:\Users\me\AppData\Local");
        assert_eq!(
            config(&[home]).clipboard,
            Some(PathBuf::from(r"C:\Users\me\AppData\Local\topsy\clipboard"))
        );
        assert_eq!(config(&[home, ("TOPSY_STATE", "")]).clipboard, None);
        assert_eq!(config(&[home, ("TOPSY_CLIPBOARD", "")]).clipboard, None);
        assert_eq!(
            config(&[home, ("TOPSY_STATE", "x"), ("TOPSY_CLIPBOARD", r"C:\t")]).clipboard,
            Some(PathBuf::from(r"C:\t"))
        );
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
        assert_eq!(
            config(&[("TOPSY_GLASS", "host")]).glass,
            Some(Glass::HostBlur)
        );
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
