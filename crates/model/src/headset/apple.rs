//! AirPods and Beats: each bud's and the case's battery, from the proximity-pairing message they
//! broadcast in Apple's Bluetooth LE manufacturer data (company `0x004C`), as public research on
//! Apple's Bluetooth protocols describes it. Windows reports one battery for them at most.

use super::{Identity, Power, VidSource};

/// Apple's Bluetooth SIG company id.
pub const APPLE: u16 = 0x004C;
const PROXIMITY_PAIRING: u8 = 0x07;
/// The message's length, after its type and length bytes.
const LENGTH: u8 = 0x19;
/// A nibble that says nothing: the bud is out of range, or not reporting.
const UNKNOWN: u8 = 0x0F;
/// The weakest signal taken for the user's own: a pair across the room is someone else's.
pub const NEAREST: i16 = -70;

/// The product id of an Apple headset, which its broadcasts carry too.
pub fn model_of(identity: &Identity) -> Option<u16> {
    let product = identity.product?;
    (product.source == VidSource::Bluetooth && product.vid == APPLE).then_some(product.pid)
}

/// A proximity-pairing message: which model sent it, and its batteries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proximity {
    pub model: u16,
    pub power: Power,
}

/// Apple's manufacturer data, without the company id. Each battery is a nibble in tens of
/// percent; which nibble is the left bud depends on which bud is the primary.
pub fn parse(data: &[u8]) -> Option<Proximity> {
    let [
        kind,
        length,
        _,
        model_low,
        model_high,
        status,
        buds,
        case,
        ..,
    ] = *data
    else {
        return None;
    };
    if kind != PROXIMITY_PAIRING || length != LENGTH || data.len() != 2 + LENGTH as usize {
        return None;
    }
    let level = |nibble: u8| (nibble <= 10).then_some(nibble * 10);
    let (low, high) = (buds & 0x0F, buds >> 4);
    // The right bud is the primary when this bit is clear, and the nibbles swap.
    let (left, right) = if status & 0x20 == 0 {
        (high, low)
    } else {
        (low, high)
    };
    let case = case & 0x0F;
    if [left, right, case].iter().all(|n| *n == UNKNOWN) {
        return None;
    }
    Some(Proximity {
        model: u16::from_le_bytes([model_low, model_high]),
        power: Power::Split {
            left: level(left),
            right: level(right),
            case: level(case),
        },
    })
}

/// One short scan for a connected headset's broadcasts: it takes its model's, near enough,
/// and keeps the nearest, since others' of the same model may be about.
#[derive(Debug)]
pub struct Scan {
    model: u16,
    nearest: Option<(i16, Power)>,
}

impl Scan {
    pub fn new(model: u16) -> Scan {
        Scan {
            model,
            nearest: None,
        }
    }

    /// A broadcast heard at `rssi` dBm: the batteries to show, if it is the nearest of this
    /// model so far and says something new.
    pub fn hear(&mut self, rssi: i16, data: &[u8]) -> Option<Power> {
        let heard = parse(data).filter(|p| p.model == self.model && rssi >= NEAREST)?;
        match self.nearest {
            Some((nearest, _)) if rssi < nearest => None,
            Some((_, power)) if power == heard.power => {
                self.nearest = Some((rssi, power));
                None
            }
            Some(_) | None => {
                self.nearest = Some((rssi, heard.power));
                Some(heard.power)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Product;
    use super::*;

    /// A message as AirPods Pro (2nd generation) send it, with `status`, the buds' byte and
    /// the case's.
    fn message(status: u8, buds: u8, case: u8) -> Vec<u8> {
        let mut data = vec![0x07, 0x19, 0x01, 0x14, 0x20, status, buds, case];
        data.resize(27, 0x00);
        data
    }

    fn split(left: Option<u8>, right: Option<u8>, case: Option<u8>) -> Power {
        Power::Split { left, right, case }
    }

    #[test]
    fn each_bud_and_the_case_in_tens_of_percent() {
        // Left the primary: the low nibble is the left bud.
        assert_eq!(
            parse(&message(0x2B, 0x98, 0x54)),
            Some(Proximity {
                model: 0x2014,
                power: split(Some(80), Some(90), Some(40)),
            })
        );
        // Right the primary: they swap.
        assert_eq!(
            parse(&message(0x0B, 0x98, 0x54)).map(|p| p.power),
            Some(split(Some(90), Some(80), Some(40)))
        );
        // A bud in its closed case, or away, says nothing.
        assert_eq!(
            parse(&message(0x2B, 0xF7, 0x0F)).map(|p| p.power),
            Some(split(Some(70), None, None))
        );
        assert_eq!(parse(&message(0x2B, 0xFF, 0x0F)), None);
    }

    #[test]
    fn other_messages_are_not_batteries() {
        let mut nearby = message(0x2B, 0x98, 0x54);
        nearby[0] = 0x10;
        assert_eq!(parse(&nearby), None);
        assert_eq!(parse(&message(0x2B, 0x98, 0x54)[..20]), None);
        let mut long = message(0x2B, 0x98, 0x54);
        long.push(0);
        assert_eq!(parse(&long), None);
    }

    #[test]
    fn a_scan_keeps_the_nearest_of_its_model() {
        let mut scan = Scan::new(0x2014);
        let ours = message(0x2B, 0x98, 0x54);
        let theirs = message(0x2B, 0x22, 0x11);
        assert_eq!(scan.hear(-80, &ours), None, "too far to be the user's");
        assert_eq!(
            scan.hear(-50, &ours),
            Some(split(Some(80), Some(90), Some(40)))
        );
        assert_eq!(scan.hear(-50, &ours), None, "nothing new");
        assert_eq!(scan.hear(-65, &theirs), None, "a farther pair");
        let mut max = message(0x2B, 0x98, 0x54);
        max[3..5].copy_from_slice(&[0x0A, 0x20]);
        assert_eq!(scan.hear(-30, &max), None, "another model");
        assert_eq!(
            scan.hear(-45, &message(0x2B, 0x88, 0x54)),
            Some(split(Some(80), Some(80), Some(40)))
        );
    }

    #[test]
    fn only_apple_headsets_are_scanned_for() {
        let identity = |source, vid| Identity {
            product: Some(Product {
                source,
                vid,
                pid: 0x2014,
            }),
            ..Identity::default()
        };
        assert_eq!(
            model_of(&identity(VidSource::Bluetooth, APPLE)),
            Some(0x2014)
        );
        assert_eq!(model_of(&identity(VidSource::Usb, APPLE)), None);
        assert_eq!(model_of(&Identity::default()), None);
    }
}
