//! The rules of fetching a large model file once: whether it's there, where a broken download
//! picks up again, what to make of the server's answer, and when to tell the panel.

#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStatus {
    /// Not on disk: the panel offers "Get".
    Missing,
    Downloading,
    Ready,
}

impl ModelStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Missing => "Get",
            Self::Downloading => "Getting…",
            Self::Ready => "Ready",
        }
    }

    /// A finished file counts only at its exact size: it's renamed into place after its hash
    /// checks out, so anything else is foreign or truncated.
    pub fn of(downloading: bool, file_len: Option<u64>, expected: u64) -> Self {
        match (downloading, file_len) {
            (true, _) => Self::Downloading,
            (false, Some(len)) if len == expected => Self::Ready,
            _ => Self::Missing,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Start {
    Fresh,
    /// Ask for the bytes from here on and append them to the partial file.
    Resume(u64),
    /// The partial file is complete; only its hash remains to be checked.
    Verify,
}

pub fn start(part_len: Option<u64>, total: u64) -> Start {
    match part_len {
        Some(n) if n > 0 && n < total => Start::Resume(n),
        Some(n) if n == total => Start::Verify,
        _ => Start::Fresh,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// The body continues the partial file.
    Append,
    /// The body is the whole file: truncate and write from the start.
    Restart,
    Fail,
}

pub fn reply(asked_from: u64, status: u32, range_start: Option<u64>) -> Reply {
    match status {
        206 if range_start == Some(asked_from) => Reply::Append,
        200 => Reply::Restart,
        _ => Reply::Fail,
    }
}

/// The first byte of `Content-Range: bytes 100-199/200`.
pub fn range_start(header: &str) -> Option<u64> {
    header
        .trim()
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Splits an absolute https URL into host and path (with query). A path on its own, as a
/// relative `Location`, keeps `host`.
pub fn split_url<'a>(url: &'a str, host: &'a str) -> Option<(&'a str, &'a str)> {
    if url.starts_with('/') {
        return Some((host, url));
    }
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    (!host.is_empty()).then_some((host, if path.is_empty() { "/" } else { path }))
}

pub fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reports progress in steps of half a percent, so a fast connection doesn't flood the panel.
#[derive(Default)]
pub struct Ticker {
    last: Option<u32>,
}

impl Ticker {
    pub fn tick(&mut self, done: u64, total: u64) -> Option<f32> {
        let step = (done.min(total) * 200 / total.max(1)) as u32;
        (self.last != Some(step)).then(|| {
            self.last = Some(step);
            step as f32 / 200.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_ready_only_at_the_exact_size() {
        assert_eq!(ModelStatus::of(false, None, 10), ModelStatus::Missing);
        assert_eq!(ModelStatus::of(false, Some(9), 10), ModelStatus::Missing);
        assert_eq!(ModelStatus::of(false, Some(10), 10), ModelStatus::Ready);
        assert_eq!(
            ModelStatus::of(true, Some(10), 10),
            ModelStatus::Downloading
        );
    }

    #[test]
    fn a_partial_file_resumes_where_it_stopped() {
        assert_eq!(start(None, 10), Start::Fresh);
        assert_eq!(start(Some(0), 10), Start::Fresh);
        assert_eq!(start(Some(4), 10), Start::Resume(4));
        assert_eq!(start(Some(10), 10), Start::Verify);
        assert_eq!(start(Some(11), 10), Start::Fresh);
    }

    #[test]
    fn a_server_that_ignores_the_range_restarts_the_file() {
        assert_eq!(reply(4, 206, Some(4)), Reply::Append);
        assert_eq!(reply(4, 206, Some(0)), Reply::Fail);
        assert_eq!(reply(0, 200, None), Reply::Restart);
        assert_eq!(reply(4, 200, None), Reply::Restart);
        assert_eq!(reply(4, 416, None), Reply::Fail);
        assert_eq!(reply(4, 404, None), Reply::Fail);
    }

    #[test]
    fn content_range_gives_its_first_byte() {
        assert_eq!(range_start("bytes 100-199/200"), Some(100));
        assert_eq!(range_start("bytes */200"), None);
    }

    #[test]
    fn redirects_split_into_host_and_path() {
        assert_eq!(
            split_url("https://cdn.example/a/b?x=1", "h"),
            Some(("cdn.example", "/a/b?x=1"))
        );
        assert_eq!(
            split_url("https://cdn.example", "h"),
            Some(("cdn.example", "/"))
        );
        assert_eq!(
            split_url("/api/resolve", "huggingface.co"),
            Some(("huggingface.co", "/api/resolve"))
        );
        assert_eq!(split_url("http://plain.example/a", "h"), None);
    }

    #[test]
    fn ticker_reports_each_half_percent_once() {
        let mut t = Ticker::default();
        assert_eq!(t.tick(0, 1000), Some(0.0));
        assert_eq!(t.tick(1, 1000), None);
        assert_eq!(t.tick(5, 1000), Some(0.005));
        assert_eq!(t.tick(6, 1000), None);
        assert_eq!(t.tick(1000, 1000), Some(1.0));
    }
}
