//! The HUDs that grow out of the notch when the volume, brightness or battery changes, or an
//! update is ready: what each one shows, and which battery changes are worth showing.

use crate::update::Nightly;

/// Segments in a HUD's bar, as in Droppy's.
pub const SEGMENTS: usize = 16;
/// A battery running down is low below each of these.
const LOW: [u8; 2] = [20, 10];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hud {
    /// The output device's volume, 0 to 1.
    Volume {
        level: f32,
        muted: bool,
    },
    /// The screen's brightness, 0 to 1.
    Brightness(f32),
    Battery(Battery),
    /// A newer nightly is downloaded; clicking the HUD opens the settings to restart into it.
    Update(Nightly),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Battery {
    /// Just plugged in, at this percent.
    Charging(u8),
    /// Running down, at this percent.
    Low(u8),
    /// Charged to 100% on the charger.
    Full,
}

/// How many of the bar's segments a level lights. Any sound or light at all lights one.
pub fn lit(level: f32) -> usize {
    let level = level.clamp(0.0, 1.0);
    let lit = (level * SEGMENTS as f32).round() as usize;
    if level > 0.0 { lit.max(1) } else { 0 }
}

/// One reading of the battery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Power {
    pub percent: u8,
    pub plugged: bool,
}

/// Follows the battery's readings and says when one is worth a HUD: plugging in, falling
/// below a low mark (or unplugging below it), and reaching full on the charger. The first
/// reading only sets the baseline, so starting topsy shows nothing.
#[derive(Default)]
pub struct BatteryWatch {
    last: Option<Power>,
}

impl BatteryWatch {
    pub fn read(&mut self, now: Power) -> Option<Battery> {
        let last = self.last.replace(now)?;
        let percent = now.percent.min(100);
        if now.plugged {
            if !last.plugged {
                return Some(if percent == 100 {
                    Battery::Full
                } else {
                    Battery::Charging(percent)
                });
            }
            return (percent == 100 && last.percent < 100).then_some(Battery::Full);
        }
        let crossed = last.plugged && LOW.iter().any(|&mark| percent < mark);
        (crossed || fell_low(last.percent, percent)).then_some(Battery::Low(percent))
    }
}

/// A battery going from `was` to `now` percent just fell below a low mark.
pub fn fell_low(was: u8, now: u8) -> bool {
    LOW.iter().any(|&mark| now < mark && was >= mark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_lights_a_segment_per_sixteenth_and_one_for_any_level() {
        assert_eq!(lit(0.0), 0);
        assert_eq!(lit(0.01), 1);
        assert_eq!(lit(0.5), 8);
        assert_eq!(lit(1.0), SEGMENTS);
        assert_eq!(lit(1.7), SEGMENTS);
        assert_eq!(lit(-1.0), 0);
    }

    #[test]
    fn plugging_in_low_and_full_show_and_nothing_else_does() {
        let at = |percent, plugged| Power { percent, plugged };
        let mut watch = BatteryWatch::default();
        assert_eq!(
            watch.read(at(50, false)),
            None,
            "the first reading is the baseline"
        );
        assert_eq!(watch.read(at(49, false)), None);
        assert_eq!(watch.read(at(49, true)), Some(Battery::Charging(49)));
        assert_eq!(watch.read(at(99, true)), None);
        assert_eq!(watch.read(at(100, true)), Some(Battery::Full));
        assert_eq!(watch.read(at(100, true)), None);
        assert_eq!(watch.read(at(100, false)), None, "unplugging is not news");
        assert_eq!(watch.read(at(20, false)), None);
        assert_eq!(watch.read(at(19, false)), Some(Battery::Low(19)));
        assert_eq!(watch.read(at(15, false)), None);
        assert_eq!(watch.read(at(9, false)), Some(Battery::Low(9)));
        assert_eq!(watch.read(at(9, true)), Some(Battery::Charging(9)));
        assert_eq!(
            watch.read(at(12, false)),
            Some(Battery::Low(12)),
            "unplugged while low"
        );
    }

    #[test]
    fn plugging_in_a_full_battery_says_full() {
        let mut watch = BatteryWatch::default();
        watch.read(Power {
            percent: 100,
            plugged: false,
        });
        assert_eq!(
            watch.read(Power {
                percent: 100,
                plugged: true
            }),
            Some(Battery::Full)
        );
    }
}
