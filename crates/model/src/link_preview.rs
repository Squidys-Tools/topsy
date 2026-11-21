//! A copied link's preview: whether its page may be fetched at all, and what the page says about
//! itself (its Open Graph tags, else its title). Fetching a link someone copied is riskier than
//! asking a site for its icon: some links act when opened (a one-time sign-in, a password
//! reset). So only a public https name is asked (see `favicon`), and never a link that carries
//! a query, a user name or a path segment that looks like a token.

use super::favicon::{attribute, icon_host};

/// A path segment this long is a name; longer, or shorter with letters and digits mixed in a
/// way words aren't, it may be a key.
const TOKEN: usize = 20;
const LONG_TOKEN: usize = 32;
/// A description longer than this is cut; the preview shows a couple of lines.
const DESCRIPTION: usize = 300;

/// What a page says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Where its picture is, as written: resolve it against the page's host.
    pub image: Option<String>,
}

/// The host and path to fetch for `link`'s preview, or `None` when it must not be fetched.
pub fn fetchable(link: &str) -> Option<(String, String)> {
    let t = link.trim();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            t.get(..scheme.len())
                .filter(|s| s.eq_ignore_ascii_case(scheme))
                .map(|s| &t[s.len()..])
        })
        .unwrap_or(t);
    let rest = rest.split('#').next().unwrap_or(rest);
    let (authority, path) = rest.find('/').map_or((rest, "/"), |at| rest.split_at(at));
    if rest.contains('?') || authority.contains('@') {
        return None;
    }
    let host = icon_host(authority)?;
    let secret = path.split('/').any(|segment| {
        let letters = segment.bytes().any(|b| b.is_ascii_alphabetic());
        let digits = segment.bytes().filter(u8::is_ascii_digit).count();
        segment.len() >= LONG_TOKEN || (segment.len() >= TOKEN && letters && digits >= 3)
    });
    (!secret).then(|| (host, path.to_string()))
}

/// What `html` says about itself: `og:` tags first, then `twitter:` ones, then its `<title>`.
pub fn page_preview(html: &str) -> Preview {
    let lower = html.to_ascii_lowercase();
    let meta = |names: [&str; 2]| {
        let mut found: [Option<String>; 2] = [None, None];
        for (at, _) in lower.match_indices("<meta") {
            let end = lower[at..].find('>').map_or(lower.len(), |e| at + e);
            let tag = &html[at..end];
            let key = attribute(tag, "property").or_else(|| attribute(tag, "name"));
            let (Some(key), Some(content)) = (key, attribute(tag, "content")) else {
                continue;
            };
            let key = key.to_ascii_lowercase();
            if let Some(slot) = names.iter().position(|name| key == *name)
                && found[slot].is_none()
            {
                found[slot] = Some(tidy(content)).filter(|c| !c.is_empty());
            }
        }
        let [first, second] = found;
        first.or(second)
    };
    let title = meta(["og:title", "twitter:title"]).or_else(|| {
        let start = lower.find("<title")?;
        let open = start + lower[start..].find('>')? + 1;
        let close = open + lower[open..].find("</title")?;
        Some(tidy(&html[open..close])).filter(|t| !t.is_empty())
    });
    let description = meta(["og:description", "twitter:description"])
        .map(|d| d.chars().take(DESCRIPTION).collect());
    let image = meta(["og:image", "twitter:image"]);
    Preview {
        title,
        description,
        image,
    }
}

/// Entities decoded and runs of whitespace made one space.
fn tidy(text: &str) -> String {
    let decoded = text
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_public_links_are_fetched() {
        let to = |h: &str, p: &str| Some((h.to_string(), p.to_string()));
        assert_eq!(
            fetchable("https://github.com/zed-industries/zed"),
            to("github.com", "/zed-industries/zed")
        );
        assert_eq!(fetchable(" www.example.com "), to("www.example.com", "/"));
        assert_eq!(
            fetchable("https://en.wikipedia.org/wiki/Rust_(programming_language)#History"),
            to("en.wikipedia.org", "/wiki/Rust_(programming_language)")
        );
        assert_eq!(
            fetchable("https://example.com/2026/11/the-long-read-about-something"),
            to("example.com", "/2026/11/the-long-read-about-something")
        );
        for refused in [
            "https://example.com/reset?token=abc",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://user:pass@example.com/",
            "https://example.com/invite/a8F3kq92LmZx7Pq0Rt5Y",
            "https://example.com/s/0123456789abcdef0123456789abcdef",
            "http://localhost:3000/x",
            "https://192.168.1.4/admin",
            "https://nas.local/share",
        ] {
            assert_eq!(fetchable(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_page_describes_itself_with_its_best_tags() {
        let page = r#"<html><head><title>Fallback</title>
            <meta name="twitter:title" content="Tweet title">
            <meta property="og:title" content="Zed &amp; friends">
            <META PROPERTY="og:description" CONTENT="  Code at the   speed of thought ">
            <meta name="twitter:image" content="https://x.com/t.png">
            <meta property="og:image" content="/og.png"></head>"#;
        assert_eq!(
            page_preview(page),
            Preview {
                title: Some("Zed & friends".into()),
                description: Some("Code at the speed of thought".into()),
                image: Some("/og.png".into()),
            }
        );
        let bare = "<title>\n  Just a  title </title><meta name='twitter:image' content=t.png>";
        assert_eq!(
            page_preview(bare),
            Preview {
                title: Some("Just a title".into()),
                description: None,
                image: Some("t.png".into()),
            }
        );
        assert_eq!(page_preview("<p>nothing</p>"), Preview::default());
        let long = format!(
            r#"<meta property="og:description" content="{}">"#,
            "x".repeat(900)
        );
        assert_eq!(
            page_preview(&long).description.map(|d| d.len()),
            Some(DESCRIPTION)
        );
    }
}
