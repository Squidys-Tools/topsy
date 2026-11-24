//! The floating player, Droppy's: pulled out of the notch, it floats where it is put, at one of
//! three sizes, and comes back there after a restart.

/// Smallest to largest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// The art, the title and play/pause: out of the way.
    Mini,
    /// The notch's player: the title, the progress bar and every button.
    Player,
    /// The cover fills it, over the player.
    Expanded,
}

impl Size {
    pub fn grow(self) -> Option<Size> {
        match self {
            Size::Mini => Some(Size::Player),
            Size::Player => Some(Size::Expanded),
            Size::Expanded => None,
        }
    }

    pub fn shrink(self) -> Option<Size> {
        match self {
            Size::Mini => None,
            Size::Player => Some(Size::Mini),
            Size::Expanded => Some(Size::Player),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Size::Mini => "mini",
            Size::Player => "player",
            Size::Expanded => "expanded",
        }
    }

    pub fn parse(name: &str) -> Option<Size> {
        [Size::Mini, Size::Player, Size::Expanded]
            .into_iter()
            .find(|size| size.name() == name)
    }
}

/// Where the player was last put: its centre in physical screen pixels, its size, and whether
/// it was left open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Saved {
    pub center: (i32, i32),
    pub size: Size,
    pub open: bool,
}

impl Saved {
    /// `x,y,size,open|closed`, as kept in the registry.
    pub fn serialize(self) -> String {
        let open = if self.open { "open" } else { "closed" };
        let (x, y) = self.center;
        format!("{x},{y},{},{open}", self.size.name())
    }

    /// Anything else, a hand-edited value or one from a later version, is no player.
    pub fn parse(text: &str) -> Option<Saved> {
        let mut parts = text.trim().split(',').map(str::trim);
        let x = parts.next()?.parse().ok()?;
        let y = parts.next()?.parse().ok()?;
        let size = Size::parse(parts.next()?)?;
        let open = match parts.next()? {
            "open" => true,
            "closed" => false,
            _ => return None,
        };
        parts.next().is_none().then_some(Saved {
            center: (x, y),
            size,
            open,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_and_shrinks_one_size_at_a_time_and_stops_at_the_ends() {
        assert_eq!(Size::Mini.grow(), Some(Size::Player));
        assert_eq!(Size::Player.grow(), Some(Size::Expanded));
        assert_eq!(Size::Expanded.grow(), None);
        assert_eq!(Size::Expanded.shrink(), Some(Size::Player));
        assert_eq!(Size::Player.shrink(), Some(Size::Mini));
        assert_eq!(Size::Mini.shrink(), None);
    }

    #[test]
    fn a_saved_player_round_trips_even_on_a_monitor_left_of_the_primary() {
        for size in [Size::Mini, Size::Player, Size::Expanded] {
            for open in [true, false] {
                let saved = Saved {
                    center: (-1700, 412),
                    size,
                    open,
                };
                assert_eq!(Saved::parse(&saved.serialize()), Some(saved));
            }
        }
        assert_eq!(
            Saved::parse(" 10, 20 ,expanded, closed\n"),
            Some(Saved {
                center: (10, 20),
                size: Size::Expanded,
                open: false,
            })
        );
    }

    #[test]
    fn junk_is_no_player() {
        for text in [
            "",
            "10,20",
            "10,20,mini",
            "10,20,huge,open",
            "10,20,mini,shut",
            "x,20,mini,open",
            "10,20,mini,open,extra",
            "10.5,20,mini,open",
        ] {
            assert_eq!(Saved::parse(text), None, "{text:?}");
        }
    }
}
