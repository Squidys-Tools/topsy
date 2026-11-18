//! Which sites a link card may ask for an icon. Asking reaches out over the network, so only a
//! public name is asked, over https, and never this machine or the local network.

use std::net::IpAddr;

/// Suffixes that name the local network, not the internet.
const LOCAL: [&str; 8] = [
    ".local",
    ".lan",
    ".home",
    ".internal",
    ".intranet",
    ".corp",
    ".localhost",
    ".home.arpa",
];

/// The host whose icon a link card shows, lowercased, or `None` when it must not be asked: a
/// bare name (`localhost`, `intranet`), an address, a local-network suffix, or anything that is
/// not plain letters, digits, dots and dashes.
pub fn icon_host(link: &str) -> Option<String> {
    let t = link.trim();
    let scheme = ["https://", "http://"]
        .into_iter()
        .find(|s| t.get(..s.len()).is_some_and(|p| p.eq_ignore_ascii_case(s)))
        .map_or(0, str::len);
    host_of(&t[scheme..])
}

/// Where a redirect or a page's link from `host` goes, as a host and a path, when it stays on
/// https and on a public name. A path stays on `host`; a bare one is taken from the root.
pub fn resolve(target: &str, host: &str) -> Option<(String, String)> {
    let target = target.trim().replace("&amp;", "&");
    let target = target.split('#').next().unwrap_or_default();
    let rest = if let Some(rest) = target.strip_prefix("//") {
        rest
    } else if let Some(https) = target
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
    {
        &target[https.len()..]
    } else if target.split(['/', '?']).next()?.contains(':') {
        // Another scheme: http, data, javascript.
        return None;
    } else if target.starts_with('/') {
        return Some((host.to_string(), target.to_string()));
    } else {
        return Some((host.to_string(), format!("/{target}")));
    };
    let path = rest.find(['/', '?']).map_or("/", |at| &rest[at..]);
    Some((host_of(rest)?, path.to_string()))
}

/// The best icon a page names in its `<link rel="icon">` tags, as written: the big
/// apple-touch-icon first, then the largest other, never an SVG (which can't be drawn here).
pub fn page_icon(html: &str) -> Option<&str> {
    let lower = html.to_ascii_lowercase();
    let mut best: Option<(u32, &str)> = None;
    for (at, _) in lower.match_indices("<link") {
        let end = lower[at..].find('>').map_or(lower.len(), |e| at + e);
        let tag = &html[at..end];
        let (Some(rel), Some(href)) = (attribute(tag, "rel"), attribute(tag, "href")) else {
            continue;
        };
        let rel = rel.to_ascii_lowercase();
        let kind = attribute(tag, "type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let path = href
            .split(['?', '#'])
            .next()
            .unwrap_or(href)
            .to_ascii_lowercase();
        let icon = rel
            .split_whitespace()
            .any(|r| r == "icon" || r == "apple-touch-icon");
        if !icon || kind.contains("svg") || path.ends_with(".svg") {
            continue;
        }
        let size = attribute(tag, "sizes")
            .and_then(|s| s.split(['x', 'X']).next()?.parse().ok())
            .unwrap_or(16);
        let score = if rel.contains("apple-touch-icon") {
            1000
        } else {
            size
        };
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, href));
        }
    }
    best.map(|(_, href)| href)
}

/// An attribute's value in one tag, quoted or not.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before = lower[..at].chars().next_back();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let rest = lower[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let start = tag.len() - rest.trim_start().len();
        let value = &tag[start..];
        return Some(match value.chars().next()? {
            q @ ('"' | '\'') => value[1..].split(q).next()?,
            _ => value.split(char::is_whitespace).next()?,
        });
    }
    None
}

fn host_of(rest: &str) -> Option<String> {
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = authority
        .split(':')
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let plain = host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    // The last label has letters only: that rules out every way of writing an address too.
    let tld = host.rsplit('.').next().unwrap_or("");
    let named =
        host.contains('.') && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic());
    let local = LOCAL.iter().any(|s| host.ends_with(s));
    (plain && named && !local).then_some(host)
}

/// Whether an address a public name resolved to is on the internet, not this machine or a
/// private network (a name can point anywhere).
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_names_are_asked() {
        let host = |s: &str| icon_host(s);
        assert_eq!(
            host("https://www.GitHub.com/x?y"),
            Some("www.github.com".into())
        );
        assert_eq!(host("  getdroppy.app/blog "), Some("getdroppy.app".into()));
        assert_eq!(
            host("https://user@example.org:8443/"),
            Some("example.org".into())
        );
        for local in [
            "http://localhost:3000",
            "https://intranet/wiki",
            "http://192.168.1.10/",
            "http://10.0.0.1:8080",
            "http://127.0.0.1",
            "http://0x7f.1/",
            "http://[::1]:80/",
            "https://printer.local",
            "https://nas.lan/share",
            "https://router.home.arpa",
            "https://api.localhost",
            "https://ex%41mple.com",
        ] {
            assert_eq!(host(local), None, "{local}");
        }
    }

    #[test]
    fn redirects_and_links_stay_on_public_https() {
        let to = |h: &str, p: &str| Some((h.to_string(), p.to_string()));
        assert_eq!(
            resolve("/static/i.png", "github.com"),
            to("github.com", "/static/i.png")
        );
        assert_eq!(
            resolve("i.png?v=2&amp;x=1", "a.com"),
            to("a.com", "/i.png?v=2&x=1")
        );
        assert_eq!(
            resolve("https://www.example.com/favicon.ico#x", "example.com"),
            to("www.example.com", "/favicon.ico")
        );
        assert_eq!(
            resolve("//cdn.example.net/i.png", "a.com"),
            to("cdn.example.net", "/i.png")
        );
        assert_eq!(
            resolve("https://example.com", "a.com"),
            to("example.com", "/")
        );
        for refused in [
            "http://example.com/favicon.ico",
            "https://192.168.0.1/x",
            "//evil.local/x",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
        ] {
            assert_eq!(resolve(refused, "example.com"), None, "{refused}");
        }
    }

    #[test]
    fn a_page_names_its_best_drawable_icon() {
        let rust = r##"<head><link rel="icon" sizes="16x16" type="image/png" href="/f-16.png">
            <LINK REL="icon" sizes="32x32" type="image/png" href="/f-32.png">
            <link rel="icon" type="image/svg+xml" href="/f.svg">
            <link rel="mask-icon" href="/pin.svg" color="#000"></head>"##;
        assert_eq!(page_icon(rust), Some("/f-32.png"));
        let apple = r#"<link rel=icon href=/f.ico><link href='/t.png' rel='apple-touch-icon'>"#;
        assert_eq!(page_icon(apple), Some("/t.png"));
        assert_eq!(page_icon(r#"<link rel="stylesheet" href="/a.css">"#), None);
        assert_eq!(page_icon(r#"<link rel="icon" href="/only.svg">"#), None);
        assert_eq!(page_icon(r#"<link data-rel="icon" href="/x.png">"#), None);
    }

    #[test]
    fn private_addresses_are_not_public() {
        let public = |s: &str| public_ip(s.parse().unwrap());
        assert!(public("140.82.112.3"));
        assert!(public("2606:4700::6810:84e5"));
        for private in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!public(private), "{private}");
        }
    }
}
