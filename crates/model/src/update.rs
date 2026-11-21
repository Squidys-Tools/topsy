//! Updating a nightly build: which published release is newer than the one running, and where
//! its exe and checksum are. Asking GitHub and swapping the exe are the app's.

use std::fmt;

use serde::Deserialize;

/// The exe a nightly carries, and the checksums beside it (`.github/workflows/nightly.yml`).
pub const EXE: &str = "topsy.exe";
pub const SUMS: &str = "SHA256SUMS";

/// A nightly's version, `0.1.1-nightly.20261121.12`, ordered as nightlies come out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Nightly {
    base: (u32, u32, u32),
    date: u32,
    run: u32,
}

impl Nightly {
    /// A version or its tag (`v` first); `None` for anything that isn't a nightly.
    pub fn parse(version: &str) -> Option<Self> {
        let version = version.strip_prefix('v').unwrap_or(version);
        let (base, rest) = version.split_once("-nightly.")?;
        let (date, run) = rest.split_once('.')?;
        let mut parts = base.split('.').map(|part| part.parse().ok());
        let base = (parts.next()??, parts.next()??, parts.next()??);
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            base,
            date: date.parse().ok()?,
            run: run.parse().ok()?,
        })
    }

    /// Where a downloaded copy is kept until it is installed.
    pub fn file_name(self) -> String {
        format!("topsy-{self}.exe")
    }

    pub fn from_file_name(name: &str) -> Option<Self> {
        Self::parse(name.strip_prefix("topsy-")?.strip_suffix(".exe")?)
    }

    /// Short enough for the HUD: `0.1.1 #12`.
    pub fn short(self) -> String {
        let (major, minor, patch) = self.base;
        format!("{major}.{minor}.{patch} #{}", self.run)
    }
}

impl fmt::Display for Nightly {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (major, minor, patch) = self.base;
        write!(
            f,
            "{major}.{minor}.{patch}-nightly.{}.{}",
            self.date, self.run
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub url: String,
    pub size: u64,
}

/// A newer nightly, with what to download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: Nightly,
    pub exe: Asset,
    pub sums: Asset,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// The newest published nightly in GitHub's release list newer than `current`, if one carries
/// both files.
pub fn newest(releases: &[u8], current: Nightly) -> Option<Offer> {
    let releases: Vec<Release> = serde_json::from_slice(releases).ok()?;
    releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let version = Nightly::parse(&release.tag_name).filter(|v| *v > current)?;
            let asset = |name: &str| {
                let found = release.assets.iter().find(|asset| asset.name == name)?;
                Some(Asset {
                    url: found.browser_download_url.clone(),
                    size: found.size,
                })
            };
            Some(Offer {
                version,
                exe: asset(EXE)?,
                sums: asset(SUMS)?,
            })
        })
        .max_by_key(|offer| offer.version)
}

/// `file`'s hash in a `sha256sum` listing (`<hex>  name`, or `<hex> *name` in binary mode).
pub fn sha256_of(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(' ')?;
        let name = name.trim_start().trim_start_matches('*');
        let hex = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        (name == file && hex).then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nightly(version: &str) -> Nightly {
        Nightly::parse(version).expect("a nightly")
    }

    #[test]
    fn nightlies_order_by_base_then_date_then_run() {
        assert!(nightly("0.1.1-nightly.20261121.3") > nightly("0.1.1-nightly.20261120.9"));
        assert!(nightly("0.1.1-nightly.20261121.10") > nightly("0.1.1-nightly.20261121.9"));
        assert!(nightly("0.2.0-nightly.20261101.1") > nightly("0.1.9-nightly.20261130.50"));
        assert_eq!(
            nightly("v0.1.1-nightly.20261121.3"),
            nightly("0.1.1-nightly.20261121.3")
        );
        for not in [
            "0.1.0",
            "0.1-nightly.20261121.3",
            "0.1.1-preview.20261121.3",
            "v0.1.1-nightly.x.3",
            "",
        ] {
            assert_eq!(Nightly::parse(not), None, "{not}");
        }
    }

    #[test]
    fn a_nightly_round_trips_through_its_file_name() {
        let version = nightly("0.1.1-nightly.20261121.12");
        assert_eq!(version.file_name(), "topsy-0.1.1-nightly.20261121.12.exe");
        assert_eq!(Nightly::from_file_name(&version.file_name()), Some(version));
        assert_eq!(
            Nightly::from_file_name("topsy-0.1.1-nightly.20261121.12.exe.part"),
            None
        );
        assert_eq!(version.short(), "0.1.1 #12");
    }

    fn release(tag: &str, draft: bool, assets: &[&str]) -> String {
        let assets: Vec<String> = assets
            .iter()
            .map(|name| {
                format!(
                    r#"{{"name":"{name}","size":7,"browser_download_url":"https://github.com/o/r/releases/download/{tag}/{name}"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":true,"assets":[{}]}}"#,
            assets.join(",")
        )
    }

    #[test]
    fn the_newest_complete_published_nightly_is_offered() {
        let both = [EXE, SUMS];
        let list = format!(
            "[{},{},{},{},{}]",
            release("v0.1.1-nightly.20261121.14", true, &both),
            release("v0.1.1-nightly.20261121.13", false, &[EXE]),
            release("v0.1.1-nightly.20261121.12", false, &both),
            release("v1.0.0", false, &both),
            release("v0.1.1-nightly.20261120.9", false, &both),
        );
        let offer = newest(list.as_bytes(), nightly("0.1.1-nightly.20261120.9")).expect("an offer");
        assert_eq!(offer.version, nightly("0.1.1-nightly.20261121.12"));
        assert_eq!(
            offer.exe,
            Asset {
                url:
                    "https://github.com/o/r/releases/download/v0.1.1-nightly.20261121.12/topsy.exe"
                        .into(),
                size: 7
            }
        );
        assert!(offer.sums.url.ends_with("/SHA256SUMS"));
        assert_eq!(
            newest(list.as_bytes(), nightly("0.1.1-nightly.20261121.12")),
            None
        );
        assert_eq!(
            newest(
                b"{\"message\":\"rate limited\"}",
                nightly("0.1.1-nightly.20261121.12")
            ),
            None
        );
    }

    #[test]
    fn the_exe_hash_is_read_from_either_sha256sum_mode() {
        let hash = "73CB3858A687A8494CA3323053016282F3DAD39D42CF62CA4E79DDA2AAC7D9AC";
        let lower = hash.to_ascii_lowercase();
        assert_eq!(
            sha256_of(&format!("{hash}  topsy.exe\n"), EXE),
            Some(lower.clone())
        );
        assert_eq!(
            sha256_of(&format!("{hash} *topsy.exe\r\n"), EXE),
            Some(lower)
        );
        assert_eq!(sha256_of(&format!("{hash}  other.exe\n"), EXE), None);
        assert_eq!(sha256_of("abc  topsy.exe\n", EXE), None);
    }
}
