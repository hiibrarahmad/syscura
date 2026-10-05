//! Tiny, dependency-free HTML scraping helpers: search-result links and a
//! page's preview image (`og:image`). Pure functions, unit-tested.

/// Result links from a DuckDuckGo HTML results page. DuckDuckGo wraps each
/// link as `/l/?uddg=<percent-encoded target>&...`.
pub fn ddg_result_links(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("uddg=") {
        rest = &rest[i + 5..];
        let end = rest.find(['&', '"', '\'']).unwrap_or(rest.len());
        let url = percent_decode(&rest[..end]);
        if url.starts_with("http") && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// (image URL, page URL) pairs from a Bing Images results page. Each result
/// carries HTML-escaped JSON with `murl` (the image) and `purl` (its page).
pub fn bing_image_results(html: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let field = |chunk: &str, key: &str| -> Option<String> {
        let tag = format!("{key}&quot;:&quot;");
        let start = chunk.find(&tag)? + tag.len();
        let end = chunk[start..].find("&quot;")?;
        Some(decode_entities(&chunk[start..start + end]))
    };
    for chunk in html.split("m=\"{").skip(1) {
        let chunk = &chunk[..chunk.find("}\"").unwrap_or(chunk.len())];
        if let (Some(img), Some(page)) = (field(chunk, "murl"), field(chunk, "purl"))
            && img.starts_with("http")
            && !out.iter().any(|(i, _)| *i == img)
        {
            out.push((img, page));
        }
    }
    out
}

/// Outbound `https://` links from a plain results page (Brave Search),
/// skipping the engine's own links and static assets.
pub fn plain_result_links(html: &str, own_domain: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=\"https://") {
        rest = &rest[i + 6..];
        let end = rest.find('"').unwrap_or(rest.len());
        let url = decode_entities(&rest[..end]);
        let host = url[8..].split('/').next().unwrap_or("");
        let asset = [".css", ".js", ".png", ".jpg", ".svg", ".ico", ".woff2"].iter().any(|e| url.ends_with(e));
        if !host.ends_with(own_domain) && !asset && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// The page's preview image from `og:image`, `og:image:secure_url` or
/// `twitter:image`, resolved against `page_url`.
pub fn preview_image(html: &str, page_url: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut best: Option<(u8, String)> = None;
    let mut pos = 0;
    while let Some(i) = lower[pos..].find("<meta") {
        let start = pos + i;
        let end = lower[start..].find('>').map(|e| start + e)?;
        let tag = &html[start..end];
        pos = end;
        let key = attr(tag, "property").or_else(|| attr(tag, "name")).unwrap_or_default();
        let rank = match key.to_ascii_lowercase().as_str() {
            "og:image:secure_url" => 3,
            "og:image" => 2,
            "twitter:image" | "twitter:image:src" => 1,
            _ => continue,
        };
        let Some(content) = attr(tag, "content") else { continue };
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, content));
        }
    }
    best.and_then(|(_, url)| resolve(page_url, &decode_entities(&url)))
}

/// Reads `name="value"` or `name='value'` from inside a tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // Must be a whole attribute name.
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let after = lower[from..].trim_start();
        if !before_ok || !after.starts_with('=') {
            continue;
        }
        let value_start = tag.len() - after.len() + 1;
        let v = tag[value_start..].trim_start();
        let quote = v.chars().next()?;
        return if quote == '"' || quote == '\'' {
            v[1..].find(quote).map(|e| v[1..1 + e].to_string())
        } else {
            Some(v.split_whitespace().next().unwrap_or("").trim_end_matches('/').to_string())
        };
    }
    None
}

/// Makes `href` absolute. Only http(s) results are returned.
pub fn resolve(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    // Any other scheme (javascript:, data:, file:, ...) is refused.
    let scheme = href.split(['/', '?', '#']).next().unwrap_or("");
    if scheme.contains(':') && !href.starts_with("http://") && !href.starts_with("https://") {
        return None;
    }
    let url = if href.starts_with("https://") || href.starts_with("http://") {
        href.to_string()
    } else if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        let scheme_end = base.find("://")? + 3;
        let host_end = base[scheme_end..].find('/').map(|i| scheme_end + i).unwrap_or(base.len());
        if href.starts_with('/') {
            format!("{}{href}", &base[..host_end])
        } else {
            let dir_end = base.rfind('/').filter(|&i| i >= host_end).unwrap_or(host_end);
            format!("{}/{href}", &base[..dir_end])
        }
    };
    Some(url).filter(|u| u.starts_with("http"))
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x2F;", "/")
}

fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(if b == b'+' { b' ' } else { b });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddg_links() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.asus.com%2Fmotherboards%2Ftuf%2D570%2F&amp;rut=abc">x</a>
                      <a href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.asus.com%2Fmotherboards%2Ftuf%2D570%2F&amp;rut=abc">dup</a>
                      <a href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fa%3Fb%3D1&rut=x">y</a>"#;
        assert_eq!(
            ddg_result_links(html),
            vec!["https://www.asus.com/motherboards/tuf-570/", "https://example.org/a?b=1"]
        );
    }

    #[test]
    fn bing_images() {
        let html = r#"<a class="iusc" m="{&quot;cid&quot;:&quot;x&quot;,&quot;purl&quot;:&quot;https://www.gskill.com/product/1&quot;,&quot;murl&quot;:&quot;https://www.gskill.com/_upload/images/1.png&quot;,&quot;turl&quot;:&quot;t&quot;}" h="x">
            <a class="iusc" m="{&quot;purl&quot;:&quot;https://shop.example/p?a=1&amp;b=2&quot;,&quot;murl&quot;:&quot;https://cdn.example/2.jpg&quot;}">"#;
        assert_eq!(
            bing_image_results(html),
            vec![
                ("https://www.gskill.com/_upload/images/1.png".to_string(), "https://www.gskill.com/product/1".to_string()),
                ("https://cdn.example/2.jpg".to_string(), "https://shop.example/p?a=1&b=2".to_string()),
            ]
        );
    }

    #[test]
    fn brave_links() {
        let html = r#"<link href="https://cdn.search.brave.com/x.css"><a href="https://search.brave.com/settings">s</a>
            <a href="https://www.asus.com/us/tuf/">r1</a><a href="https://www.asus.com/us/tuf/">dup</a>
            <a href="https://example.org/a?x=1&amp;y=2">r2</a>"#;
        assert_eq!(
            plain_result_links(html, "brave.com"),
            vec!["https://www.asus.com/us/tuf/", "https://example.org/a?x=1&y=2"]
        );
    }

    #[test]
    fn og_image() {
        let html = r#"<head><meta name="twitter:image" content="/t.png">
            <META content='https://cdn.asus.com/x.png?a=1&amp;b=2' property='og:image' />
            <meta property="og:title" content="TUF"></head>"#;
        assert_eq!(
            preview_image(html, "https://www.asus.com/p/tuf/").as_deref(),
            Some("https://cdn.asus.com/x.png?a=1&b=2")
        );
        let only_twitter = r#"<meta name="twitter:image" content="/img/t.png">"#;
        assert_eq!(
            preview_image(only_twitter, "https://shop.example.com/a/b").as_deref(),
            Some("https://shop.example.com/img/t.png")
        );
        assert_eq!(preview_image("<p>none</p>", "https://a.com/"), None);
    }

    #[test]
    fn resolving() {
        assert_eq!(resolve("https://a.com/x/y.html", "//cdn.b.com/i.png").unwrap(), "https://cdn.b.com/i.png");
        assert_eq!(resolve("https://a.com/x/y.html", "i.png").unwrap(), "https://a.com/x/i.png");
        assert_eq!(resolve("https://a.com", "/i.png").unwrap(), "https://a.com/i.png");
        assert_eq!(resolve("https://a.com/", "javascript:alert(1)"), None);
        assert_eq!(resolve("https://a.com/", "data:image/png;base64,xx"), None);
        assert_eq!(resolve("https://a.com/", "file:///c:/x"), None);
    }
}
