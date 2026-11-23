//! Bluetooth headsets: who a device is (its product ids, Bluetooth class and name), which shape
//! draws it, and what its batteries are at. The catalog of known models is in [`catalog`].

pub mod apple;
pub mod catalog;
pub mod spin;

use serde::{Deserialize, Serialize};

/// The shape a headset is drawn as when the catalog has no picture of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Form {
    /// Earbuds with a stem, like AirPods.
    StemBuds,
    /// Earbuds without one.
    Buds,
    /// Over-ear or on-ear headphones.
    OverEar,
    Speaker,
}

impl Form {
    pub const ALL: [Form; 4] = [Form::StemBuds, Form::Buds, Form::OverEar, Form::Speaker];

    pub fn slug(self) -> &'static str {
        match self {
            Form::StemBuds => "stem-buds",
            Form::Buds => "buds",
            Form::OverEar => "over-ear",
            Form::Speaker => "speaker",
        }
    }

    /// What to call a device of this shape when nothing names it.
    pub fn noun(self) -> &'static str {
        match self {
            Form::StemBuds | Form::Buds => "Earbuds",
            Form::OverEar => "Headphones",
            Form::Speaker => "Speaker",
        }
    }

    /// A guess from what any device says about itself: its name first, then its Bluetooth
    /// class. Headphones when neither tells.
    pub fn guess(name: &str, class: Option<u32>) -> Form {
        let name = name.to_ascii_lowercase();
        const BUDS: [&str; 6] = ["bud", "pods", "wf-", "tws", "earbud", "in-ear"];
        if BUDS.iter().any(|word| name.contains(word)) {
            return Form::Buds;
        }
        if ["speaker", "soundbar", "boom", "flip", "charge"]
            .iter()
            .any(|word| name.contains(word))
        {
            return Form::Speaker;
        }
        // A wearable headset may be earbuds or headphones: only a speaker is told apart.
        match class.and_then(audio_minor_class) {
            // Loudspeaker, portable audio, car audio, hi-fi.
            Some(0x05 | 0x07 | 0x08 | 0x0A) => Form::Speaker,
            _ => Form::OverEar,
        }
    }
}

/// The minor class of a Bluetooth Class of Device whose major class is audio/video.
fn audio_minor_class(class: u32) -> Option<u32> {
    ((class >> 8) & 0x1f == 0x04).then_some((class >> 2) & 0x3f)
}

/// Who assigned a product's vendor id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VidSource {
    /// A Bluetooth SIG company identifier (Apple is `0x004C`).
    Bluetooth,
    /// A USB-IF vendor id.
    Usb,
}

/// A product's ids, from the Bluetooth Device ID profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    pub source: VidSource,
    pub vid: u16,
    pub pid: u16,
}

impl Product {
    /// From the Device ID profile's fields: `source` 1 is the Bluetooth SIG, 2 USB-IF.
    pub fn from_parts(source: u16, vid: u16, pid: u16) -> Option<Product> {
        let source = match source {
            1 => VidSource::Bluetooth,
            2 => VidSource::Usb,
            _ => return None,
        };
        Some(Product { source, vid, pid })
    }

    /// The ids in a Windows device instance or hardware id: `_VID&0001004C_PID&2014` (a 4-digit
    /// source, then the vendor) on a classic profile node, `_VID&02054c_PID&0ce0` (2 digits)
    /// on a Bluetooth LE one.
    pub fn from_device_id(id: &str) -> Option<Product> {
        let upper = id.to_ascii_uppercase();
        let vid = hex_after(&upper, "VID&")?;
        let pid = hex_after(&upper, "PID&")?;
        let (source, vid) = match vid.len() {
            8 => (&vid[..4], &vid[4..]),
            6 => (&vid[..2], &vid[2..]),
            _ => return None,
        };
        Product::from_parts(
            u16::from_str_radix(source, 16).ok()?,
            u16::from_str_radix(vid, 16).ok()?,
            u16::from_str_radix(pid.get(..4)?, 16).ok()?,
        )
    }
}

fn hex_after<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let start = text.find(tag)? + tag.len();
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(rest.len());
    (end > 0).then_some(&rest[..end])
}

/// A device id with its Bluetooth address blanked: the address is in most of a Bluetooth
/// device's ids and names the user's own headset. A run of exactly 12 hex digits is taken for
/// one, unless it follows a `-`, as a GUID's last group does.
pub fn redact_address(id: &str) -> String {
    let bytes = id.as_bytes();
    let mut out = String::with_capacity(id.len());
    let mut at = 0;
    while at < bytes.len() {
        let run = bytes[at..]
            .iter()
            .take_while(|b| b.is_ascii_hexdigit())
            .count();
        if run == 0 {
            let next = id[at..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&id[at..at + next]);
            at += next;
            continue;
        }
        let after_dash = at > 0 && bytes[at - 1] == b'-';
        let alone = (at == 0 || !bytes[at - 1].is_ascii_alphanumeric())
            && bytes
                .get(at + run)
                .is_none_or(|b| !b.is_ascii_alphanumeric());
        if run == 12 && alone && !after_dash {
            out.push_str("XXXXXXXXXXXX");
        } else {
            out.push_str(&id[at..at + run]);
        }
        at += run;
    }
    out
}

/// What a Bluetooth audio device tells about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// The name the user knows it by, as Windows shows it.
    pub name: String,
    pub product: Option<Product>,
    /// Its Bluetooth Class of Device.
    pub class: Option<u32>,
}

/// What a headset's batteries are at, in percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    /// One battery, as Windows reports for most headsets.
    Single(u8),
    /// Earbuds that report each bud and their case.
    Split {
        left: Option<u8>,
        right: Option<u8>,
        case: Option<u8>,
    },
}

impl Power {
    /// `80` for one battery, or `L80,R75,C40` for each bud and the case (any may be left out).
    pub fn parse(text: &str) -> Option<Power> {
        let percent = |s: &str| s.parse::<u8>().ok().filter(|p| *p <= 100);
        if let Some(single) = percent(text) {
            return Some(Power::Single(single));
        }
        let (mut left, mut right, mut case) = (None, None, None);
        for part in text.split(',') {
            let part = part.trim();
            let slot = match part.get(..1)?.to_ascii_uppercase().as_str() {
                "L" => &mut left,
                "R" => &mut right,
                "C" => &mut case,
                _ => return None,
            };
            *slot = Some(percent(&part[1..])?);
        }
        Some(Power::Split { left, right, case })
    }

    /// The lowest battery it reports, which is the one that runs out first.
    pub fn lowest(self) -> Option<u8> {
        match self {
            Power::Single(percent) => Some(percent),
            Power::Split { left, right, .. } => left.into_iter().chain(right).min(),
        }
    }
}

/// Everything topsy can tell about the Bluetooth devices on this machine, for a contributor
/// adding one to the catalog. Written by topsy, read by `cargo devices`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dump {
    pub topsy: String,
    pub windows: String,
    pub devices: Vec<Dumped>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dumped {
    #[serde(flatten)]
    pub identity: Identity,
    /// Its own node is there now, not only remembered from pairing.
    pub present: bool,
    pub battery: Option<u8>,
    /// Its device nodes' instance ids, addresses blanked.
    pub ids: Vec<String>,
}

/// Bluetooth audio's services, as they start a profile node's id: A2DP's sink, the headset
/// and hands-free profiles, and LE Audio's stream control and capabilities.
const AUDIO_SERVICES: [&str; 5] = [
    "{0000110b-",
    "{00001108-",
    "{0000111e-",
    "{0000184e-",
    "{00001850-",
];

impl Dumped {
    /// An audio device by its class or its services: not a phone, a keyboard or a printer,
    /// whose names are none of the catalog's business.
    pub fn is_audio(&self) -> bool {
        self.identity.class.and_then(audio_minor_class).is_some()
            || self.ids.iter().any(|id| {
                let id = id.to_ascii_lowercase();
                AUDIO_SERVICES.iter().any(|service| id.contains(service))
            })
    }
}

/// GitHub turns away longer URLs.
const URL_LIMIT: usize = 6000;

/// A prefilled *Add a headset* issue on `repo`, for when no coding agent is installed: the
/// user reads it in the browser before it is sent. Kept under GitHub's URL limit by leaving
/// out ids, then the devices that aren't connected, then all but one.
pub fn issue_url(repo: &str, dump: &Dump) -> String {
    let url = |dump: &Dump| {
        let json = serde_json::to_string_pretty(dump).unwrap_or_default();
        format!(
            "{repo}/issues/new?template=add-headset.yml&title={}&dump={}",
            percent_encode("Add a headset"),
            percent_encode(&json)
        )
    };
    let mut dump = dump.clone();
    let mut full = url(&dump);
    let trims: [fn(&mut Dump); 3] = [
        |dump| dump.devices.iter_mut().for_each(|d| d.ids.clear()),
        |dump| dump.devices.retain(|d| d.present),
        |dump| dump.devices.truncate(1),
    ];
    for trim in trims {
        if full.len() <= URL_LIMIT {
            break;
        }
        trim(&mut dump);
        full = url(&dump);
    }
    full
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_issue_url_is_prefilled_and_trimmed_to_fit() {
        let device = |name: &str, present| Dumped {
            identity: Identity {
                name: name.into(),
                product: None,
                class: Some(0x240418),
            },
            present,
            battery: Some(80),
            ids: vec![r"BTHENUM\DEV_XXXXXXXXXXXX\7&1&BLUETOOTHDEVICE_XXXXXXXXXXXX".into(); 4],
        };
        let mut dump = Dump {
            topsy: "0.1.0".into(),
            windows: "26100".into(),
            devices: vec![device("Bob’s Buds", true)],
        };
        let url = issue_url("https://github.com/o/r", &dump);
        assert!(url.starts_with(
            "https://github.com/o/r/issues/new?template=add-headset.yml&title=Add%20a%20headset&dump="
        ));
        assert!(url.contains("Bob%E2%80%99s%20Buds") && url.contains("BLUETOOTHDEVICE"));

        dump.devices
            .extend((0..40).map(|n| device(&format!("Old {n}"), false)));
        let url = issue_url("https://github.com/o/r", &dump);
        assert!(url.len() <= URL_LIMIT);
        assert!(url.contains("Bob") && !url.contains("Old%201"));
    }

    #[test]
    fn only_audio_devices_go_in() {
        let device = |class, id: &str| Dumped {
            identity: Identity {
                class,
                ..Identity::default()
            },
            ids: vec![id.into()],
            ..Dumped::default()
        };
        assert!(device(Some(0x240418), "").is_audio());
        assert!(!device(Some(0x5a020c), "").is_audio(), "a phone");
        let le_audio =
            r"BTHLEDevice\{0000184E-0000-1000-8000-00805F9B34FB}_XXXXXXXXXXXX\8&1&0&0001";
        assert!(device(None, le_audio).is_audio());
        let keyboard =
            r"BTHLEDevice\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&0205ac_PID&024f";
        assert!(!device(None, keyboard).is_audio());
    }

    #[test]
    fn product_ids_come_from_classic_and_le_device_ids() {
        let classic = r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001004C_PID&2014\7&1A2B3C4D&0&A0B1C2D3E4F5_C00000000";
        assert_eq!(
            Product::from_device_id(classic),
            Some(Product {
                source: VidSource::Bluetooth,
                vid: 0x004C,
                pid: 0x2014
            })
        );
        let le = r"BTHLEDEVICE\{0000180F-0000-1000-8000-00805F9B34FB}_Dev_VID&02054c_PID&0ce0_REV&0100_a0b1c2d3e4f5\8&3b&0&0010";
        assert_eq!(
            Product::from_device_id(le),
            Some(Product {
                source: VidSource::Usb,
                vid: 0x054C,
                pid: 0x0CE0
            })
        );
        assert_eq!(
            Product::from_device_id(r"BTHENUM\DEV_A0B1C2D3E4F5\7&1&BLUETOOTHDEVICE_A0B1C2D3E4F5"),
            None
        );
        assert_eq!(Product::from_device_id("VID&0003004C_PID&2014"), None);
    }

    #[test]
    fn addresses_are_blanked_and_guids_kept() {
        assert_eq!(
            redact_address(r"BTHENUM\DEV_A0B1C2D3E4F5\7&1A2B&0&BLUETOOTHDEVICE_a0b1c2d3e4f5"),
            r"BTHENUM\DEV_XXXXXXXXXXXX\7&1A2B&0&BLUETOOTHDEVICE_XXXXXXXXXXXX"
        );
        let profile = r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001004C_PID&2014\7&1A2B3C4D&0&A0B1C2D3E4F5_C00000000";
        assert_eq!(
            redact_address(profile),
            r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001004C_PID&2014\7&1A2B3C4D&0&XXXXXXXXXXXX_C00000000"
        );
        assert_eq!(redact_address("Bob’s Buds"), "Bob’s Buds");
    }

    #[test]
    fn a_name_beats_the_class_and_headphones_are_the_default() {
        assert_eq!(Form::guess("Galaxy Buds2 Pro", None), Form::Buds);
        assert_eq!(Form::guess("WF-1000XM5", Some(0x240404)), Form::Buds);
        assert_eq!(Form::guess("JBL Flip 6", None), Form::Speaker);
        // Audio/video, loudspeaker.
        assert_eq!(Form::guess("Kitchen", Some(0x240414)), Form::Speaker);
        // Audio/video, wearable headset: headphones as often as earbuds.
        assert_eq!(Form::guess("Sonos Ace", Some(0x2c0404)), Form::OverEar);
        // Audio/video, headphones.
        assert_eq!(Form::guess("WH-1000XM5", Some(0x240418)), Form::OverEar);
        // A phone (major class 2): not audio, so no say.
        assert_eq!(Form::guess("Thing", Some(0x5a020c)), Form::OverEar);
    }

    #[test]
    fn power_reads_one_battery_or_each_bud_and_the_case() {
        assert_eq!(Power::parse("80"), Some(Power::Single(80)));
        let split = Power::parse("L80,R75,C40");
        assert_eq!(
            split,
            Some(Power::Split {
                left: Some(80),
                right: Some(75),
                case: Some(40)
            })
        );
        assert_eq!(split.and_then(Power::lowest), Some(75));
        assert_eq!(
            Power::parse("l5, c100"),
            Some(Power::Split {
                left: Some(5),
                right: None,
                case: Some(100)
            })
        );
        assert_eq!(Power::parse("101"), None);
        assert_eq!(Power::parse("X10"), None);
        assert_eq!(Power::parse(""), None);
    }
}
