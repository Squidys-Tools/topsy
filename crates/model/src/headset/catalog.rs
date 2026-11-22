//! The catalog of known headsets (`devices/catalog.toml`): each model's name, shape, and the
//! rules that recognise it. Product ids win over names; a device nothing matches gets a shape
//! guessed from its name and class.

use serde::Deserialize;

use super::{Form, Identity, Power, Product, VidSource};

/// The catalog topsy ships, checked by this crate's tests and `cargo devices check`.
pub const BUILTIN: &str = include_str!("../../../../devices/catalog.toml");

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    #[serde(rename = "device", default)]
    pub devices: Vec<Entry>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Lowercase words joined by `-`; later, the name of its picture's folder.
    pub slug: String,
    pub name: String,
    pub form: Form,
    #[serde(rename = "match")]
    pub rules: Vec<Rule>,
}

/// One way to recognise a model.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged, deny_unknown_fields)]
pub enum Rule {
    /// A Bluetooth SIG company id and product id.
    Bluetooth { vid: u16, pid: u16 },
    /// A USB-IF vendor id and product id.
    Usb { usb_vid: u16, pid: u16 },
    /// Part of the device's name, any case. Only when nothing has product ids for it.
    Name { name: String },
}

impl Entry {
    pub fn matches_product(&self, product: Product) -> bool {
        self.rules.iter().any(|rule| *rule == Rule::of(product))
    }
}

impl Rule {
    /// The rule that matches exactly this product.
    pub fn of(product: Product) -> Rule {
        let Product { source, vid, pid } = product;
        match source {
            VidSource::Bluetooth => Rule::Bluetooth { vid, pid },
            VidSource::Usb => Rule::Usb { usb_vid: vid, pid },
        }
    }

    /// As `devices/catalog.toml` writes it.
    pub fn to_toml(&self) -> String {
        match self {
            Rule::Bluetooth { vid, pid } => format!("{{ vid = 0x{vid:04X}, pid = 0x{pid:04X} }}"),
            Rule::Usb { usb_vid, pid } => {
                format!("{{ usb_vid = 0x{usb_vid:04X}, pid = 0x{pid:04X} }}")
            }
            Rule::Name { name } => format!("{{ name = {name:?} }}"),
        }
    }
}

/// How a device is drawn: its catalog entry if it has one, and its shape either way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look<'a> {
    pub entry: Option<&'a Entry>,
    pub form: Form,
}

/// What `TOPSY_FAKE_DEVICE` asks to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Fake {
    pub name: String,
    pub form: Form,
    pub power: Option<Power>,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog, String> {
        toml::from_str(text).map_err(|err| err.to_string())
    }

    pub fn identify(&self, identity: &Identity) -> Look<'_> {
        let by_product = identity
            .product
            .and_then(|product| (self.devices.iter()).find(|entry| entry.matches_product(product)));
        let name = identity.name.to_lowercase();
        // The longest name that fits, so "AirPods Pro" beats "AirPods".
        let by_name = || {
            self.devices
                .iter()
                .filter_map(|entry| {
                    let longest = entry
                        .rules
                        .iter()
                        .filter_map(|rule| match rule {
                            Rule::Name { name: part } => Some(part),
                            Rule::Bluetooth { .. } | Rule::Usb { .. } => None,
                        })
                        .filter(|part| name.contains(&part.to_lowercase()))
                        .map(String::len)
                        .max()?;
                    Some((longest, entry))
                })
                .max_by_key(|(longest, _)| *longest)
                .map(|(_, entry)| entry)
        };
        match by_product.or_else(by_name) {
            Some(entry) => Look {
                entry: Some(entry),
                form: entry.form,
            },
            None => Look {
                entry: None,
                form: Form::guess(&identity.name, identity.class),
            },
        }
    }

    /// `slug[:power]`, where the slug is a catalog entry's or a shape's (`over-ear`), and the
    /// power is `80` or `L80,R75,C40`.
    pub fn fake(&self, spec: &str) -> Option<Fake> {
        let (slug, power) = match spec.split_once(':') {
            Some((slug, power)) => (slug.trim(), Some(Power::parse(power.trim())?)),
            None => (spec.trim(), None),
        };
        if let Some(entry) = self.devices.iter().find(|entry| entry.slug == slug) {
            return Some(Fake {
                name: entry.name.clone(),
                form: entry.form,
                power,
            });
        }
        let form = Form::ALL.into_iter().find(|form| form.slug() == slug)?;
        Some(Fake {
            name: form.noun().to_owned(),
            form,
            power,
        })
    }

    /// What is wrong with it, if anything: each line names the entry.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (at, entry) in self.devices.iter().enumerate() {
            let slug = &entry.slug;
            let well_formed = !slug.is_empty()
                && !slug.starts_with('-')
                && !slug.ends_with('-')
                && !slug.contains("--")
                && slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !well_formed {
                problems.push(format!("{slug}: a slug is lowercase words joined by -"));
            }
            if Form::ALL.iter().any(|form| form.slug() == slug) {
                problems.push(format!("{slug}: that slug is a shape's"));
            }
            if entry.name.trim().is_empty() {
                problems.push(format!("{slug}: no name"));
            }
            if entry.rules.is_empty() {
                problems.push(format!("{slug}: no match rules"));
            }
            for rule in &entry.rules {
                if let Rule::Name { name } = rule
                    && name.trim().chars().count() < 3
                {
                    problems.push(format!("{slug}: name rule {name:?} is too short to match"));
                }
            }
            for other in &self.devices[..at] {
                if other.slug == *slug {
                    problems.push(format!("{slug}: slug used twice"));
                }
                for rule in entry.rules.iter().filter(|rule| other.rules.contains(rule)) {
                    problems.push(format!("{slug}: {rule:?} also matches {}", other.slug));
                }
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[[device]]
slug = "airpods-pro-2"
name = "AirPods Pro (2nd generation)"
form = "stem-buds"
match = [{ vid = 0x004C, pid = 0x2014 }, { name = "AirPods Pro" }]

[[device]]
slug = "airpods-2"
name = "AirPods (2nd generation)"
form = "stem-buds"
match = [{ vid = 0x004C, pid = 0x200F }, { name = "AirPods" }]

[[device]]
slug = "wh-1000xm5"
name = "Sony WH-1000XM5"
form = "over-ear"
match = [{ usb_vid = 0x054C, pid = 0x0D58 }, { name = "WH-1000XM5" }]
"#;

    fn identity(name: &str, product: Option<(VidSource, u16, u16)>) -> Identity {
        Identity {
            name: name.into(),
            product: product.map(|(source, vid, pid)| Product { source, vid, pid }),
            class: None,
        }
    }

    fn slug(look: Look<'_>) -> Option<&str> {
        look.entry.map(|entry| entry.slug.as_str())
    }

    #[test]
    fn the_builtin_catalog_parses_and_has_no_problems() {
        let catalog = Catalog::parse(BUILTIN).unwrap();
        assert!(!catalog.devices.is_empty());
        assert_eq!(catalog.problems(), Vec::<String>::new());
    }

    #[test]
    fn product_ids_beat_names_and_the_longest_name_wins() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let bt = VidSource::Bluetooth;
        let renamed = identity("Kitchen AirPods Pro", Some((bt, 0x004C, 0x200F)));
        assert_eq!(slug(catalog.identify(&renamed)), Some("airpods-2"));
        let by_name = identity("Chris’s AirPods Pro", None);
        assert_eq!(slug(catalog.identify(&by_name)), Some("airpods-pro-2"));
        let usb = identity("x", Some((VidSource::Usb, 0x054C, 0x0D58)));
        assert_eq!(slug(catalog.identify(&usb)), Some("wh-1000xm5"));
        let wrong_source = identity("x", Some((bt, 0x054C, 0x0D58)));
        assert_eq!(catalog.identify(&wrong_source).entry, None);
        let usb = Rule::of(Product {
            source: VidSource::Usb,
            vid: 0x054C,
            pid: 0x0D58,
        });
        assert_eq!(usb.to_toml(), "{ usb_vid = 0x054C, pid = 0x0D58 }");
        let written = format!(
            "[[device]]
slug = \"a\"
name = \"A\"
form = \"buds\"
match = [{}]",
            usb.to_toml()
        );
        assert_eq!(Catalog::parse(&written).unwrap().devices[0].rules, [usb]);
    }

    #[test]
    fn an_unknown_device_gets_a_guessed_shape() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let look = catalog.identify(&identity("Galaxy Buds3", None));
        assert_eq!((look.entry.is_none(), look.form), (true, Form::Buds));
    }

    #[test]
    fn a_fake_device_is_a_catalog_slug_or_a_shape() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        assert_eq!(
            catalog.fake("airpods-pro-2:L80,R75,C40"),
            Some(Fake {
                name: "AirPods Pro (2nd generation)".into(),
                form: Form::StemBuds,
                power: Power::parse("L80,R75,C40"),
            })
        );
        assert_eq!(
            catalog.fake("speaker:60"),
            Some(Fake {
                name: "Speaker".into(),
                form: Form::Speaker,
                power: Some(Power::Single(60)),
            })
        );
        assert_eq!(catalog.fake("over-ear").and_then(|f| f.power), None);
        assert_eq!(catalog.fake("nope:50"), None);
        assert_eq!(catalog.fake("buds:200"), None);
    }

    #[test]
    fn problems_name_bad_slugs_shared_rules_and_empty_entries() {
        let catalog = Catalog::parse(
            r#"
[[device]]
slug = "Bad Slug"
name = "A"
form = "buds"
match = [{ vid = 1, pid = 2 }]

[[device]]
slug = "b"
name = ""
form = "buds"
match = [{ vid = 1, pid = 2 }, { name = "x" }]

[[device]]
slug = "buds"
name = "C"
form = "buds"
match = []
"#,
        )
        .unwrap();
        assert_eq!(
            catalog.problems(),
            [
                "Bad Slug: a slug is lowercase words joined by -",
                "b: no name",
                "b: name rule \"x\" is too short to match",
                "b: Bluetooth { vid: 1, pid: 2 } also matches Bad Slug",
                "buds: that slug is a shape's",
                "buds: no match rules",
            ]
        );
        assert!(Catalog::parse("[[device]]\nslug = \"a\"\nnope = 1").is_err());
    }
}
