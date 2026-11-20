//! The Audio widget's list: a row per app holding an audio session on the output device, in
//! name order. An app with several sessions (a browser's tabs, its helper processes) is one
//! row, and its slider sets them all. `P` is the app's picture, which the list only carries.

/// A session as Windows reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The session's instance id, unique on the device.
    pub id: String,
    /// What groups sessions into one row: the app's exe.
    pub app: String,
    pub name: String,
    pub volume: f32,
    pub muted: bool,
}

#[derive(Debug)]
pub struct App<P> {
    pub key: String,
    pub name: String,
    pub picture: Option<P>,
    pub volume: f32,
    pub muted: bool,
    sessions: Vec<String>,
}

#[derive(Debug)]
pub struct Mixer<P> {
    apps: Vec<App<P>>,
}

impl<P> Default for Mixer<P> {
    fn default() -> Self {
        Self { apps: Vec::new() }
    }
}

impl<P> Mixer<P> {
    pub fn apps(&self) -> &[App<P>] {
        &self.apps
    }

    /// A session appeared: it joins its app's row, or starts one. The picture back is one the
    /// list did not keep, for the caller to free.
    pub fn add(&mut self, session: Session, picture: Option<P>) -> Option<P> {
        if self
            .apps
            .iter()
            .any(|app| app.sessions.contains(&session.id))
        {
            return picture;
        }
        if let Some(app) = self.apps.iter_mut().find(|app| app.key == session.app) {
            app.sessions.push(session.id);
            return picture;
        }
        let app = App {
            key: session.app,
            name: session.name,
            picture,
            volume: level(session.volume),
            muted: session.muted,
            sessions: vec![session.id],
        };
        let at = self
            .apps
            .partition_point(|other| order(other) < order(&app));
        self.apps.insert(at, app);
        None
    }

    /// Something else changed a session's volume. Whether the list changed.
    pub fn changed(&mut self, id: &str, volume: f32, muted: bool) -> bool {
        let volume = level(volume);
        let Some(app) = self
            .apps
            .iter_mut()
            .find(|app| app.sessions.iter().any(|s| s == id))
        else {
            return false;
        };
        let changed = app.volume != volume || app.muted != muted;
        app.volume = volume;
        app.muted = muted;
        changed
    }

    /// A session ended. The picture of a row that went with it, for the caller to free.
    pub fn remove(&mut self, id: &str) -> Option<Option<P>> {
        let at = self
            .apps
            .iter()
            .position(|app| app.sessions.iter().any(|s| s == id))?;
        let app = &mut self.apps[at];
        app.sessions.retain(|s| s != id);
        if !app.sessions.is_empty() {
            return Some(None);
        }
        Some(self.apps.remove(at).picture)
    }

    /// The list's pictures, for the caller to free.
    pub fn into_pictures(self) -> Vec<P> {
        self.apps
            .into_iter()
            .filter_map(|app| app.picture)
            .collect()
    }

    /// The slider moved: the app's new volume, which unmutes it as the volume keys do. What the
    /// app's sessions should be set to, or `None` for an app no longer listed.
    pub fn slide(&mut self, key: &str, volume: f32) -> Option<(f32, bool)> {
        let app = self.apps.iter_mut().find(|app| app.key == key)?;
        app.volume = level(volume);
        app.muted = false;
        Some((app.volume, app.muted))
    }

    /// The mute button: as [`Mixer::slide`].
    pub fn toggle_mute(&mut self, key: &str) -> Option<(f32, bool)> {
        let app = self.apps.iter_mut().find(|app| app.key == key)?;
        app.muted = !app.muted;
        Some((app.volume, app.muted))
    }
}

fn order<P>(app: &App<P>) -> (String, &str) {
    (app.name.to_lowercase(), &app.key)
}

/// A volume as a slider shows it: 0 to 1, and a nonsense reading as silence.
fn level(volume: f32) -> f32 {
    if volume.is_nan() {
        0.0
    } else {
        volume.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, app: &str, name: &str, volume: f32) -> Session {
        Session {
            id: id.into(),
            app: app.into(),
            name: name.into(),
            volume,
            muted: false,
        }
    }

    fn names(mixer: &Mixer<u8>) -> Vec<&str> {
        mixer.apps().iter().map(|app| app.name.as_str()).collect()
    }

    #[test]
    fn sessions_of_one_app_are_one_row_in_name_order() {
        let mut mixer = Mixer::default();
        assert_eq!(
            mixer.add(session("1", "spotify", "Spotify", 0.5), Some(1)),
            None
        );
        assert_eq!(
            mixer.add(session("2", "chrome", "Google Chrome", 1.0), Some(2)),
            None
        );
        assert_eq!(
            mixer.add(session("3", "chrome", "Google Chrome", 0.2), Some(3)),
            Some(3),
            "the row keeps its first picture"
        );
        assert_eq!(
            mixer.add(session("4", "discord", "discord", 1.0), None),
            None
        );
        assert_eq!(names(&mixer), ["discord", "Google Chrome", "Spotify"]);
        assert_eq!(
            mixer.add(session("1", "spotify", "Spotify", 0.5), Some(4)),
            Some(4)
        );
        assert_eq!(mixer.apps().len(), 3, "a session seen twice");
    }

    #[test]
    fn a_row_goes_with_its_last_session() {
        let mut mixer = Mixer::default();
        mixer.add(session("1", "chrome", "Chrome", 1.0), Some(7));
        mixer.add(session("2", "chrome", "Chrome", 1.0), None);
        assert_eq!(mixer.remove("1"), Some(None));
        assert_eq!(names(&mixer), ["Chrome"]);
        assert_eq!(mixer.remove("2"), Some(Some(7)));
        assert!(mixer.apps().is_empty());
        assert_eq!(mixer.remove("2"), None);
    }

    #[test]
    fn volumes_are_clamped_and_changes_reported() {
        let mut mixer: Mixer<u8> = Mixer::default();
        mixer.add(session("1", "app", "App", 1.7), None);
        assert_eq!(mixer.apps()[0].volume, 1.0);
        assert!(!mixer.changed("1", 1.0, false), "nothing new");
        assert!(mixer.changed("1", f32::NAN, true));
        assert_eq!((mixer.apps()[0].volume, mixer.apps()[0].muted), (0.0, true));
        assert!(!mixer.changed("gone", 0.5, false));
    }

    #[test]
    fn sliding_unmutes_and_the_button_flips() {
        let mut mixer: Mixer<u8> = Mixer::default();
        mixer.add(session("1", "app", "App", 0.5), None);
        assert_eq!(mixer.toggle_mute("app"), Some((0.5, true)));
        assert_eq!(mixer.slide("app", -0.2), Some((0.0, false)));
        assert_eq!(mixer.toggle_mute("app"), Some((0.0, true)));
        assert_eq!(mixer.slide("gone", 0.5), None);
    }
}
