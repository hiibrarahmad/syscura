//! Free online helpers. Today: finding a product picture for each part.
//!
//! Sources, in order (all free, no API keys):
//! 0. A known exact picture (board profiles carry the maker's own photo).
//! 1. Bing Images, preferring pictures hosted on the brand's own website.
//! 2. Web search (Brave, then DuckDuckGo) -> the product page -> its
//!    `og:image` preview picture (what link previews in chat apps show).
//! 3. Wikipedia's page-image API: a *generic* example picture only, marked
//!    as such (source "wikipedia").
//!
//! Searches are spaced out (`MIN_SEARCH_GAP`) because search engines block
//! rapid automated queries. Results are cached forever by the caller, so
//! each part is looked up once per PC.

pub mod cache;
pub mod html;
pub mod query;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ureq::ResponseExt;

/// Browser-like (search pages reject unknown clients) and tagged "Syscura".
/// No personal data is ever sent.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36 Syscura/0.1";
const MIN_SEARCH_GAP: Duration = Duration::from_secs(20);
const MAX_PAGE: u64 = 3 * 1024 * 1024;
const MAX_IMAGE: u64 = 8 * 1024 * 1024;

/// Sites that never have a useful product picture for us.
const SKIP_DOMAINS: &[&str] = &[
    "youtube.", "reddit.", "facebook.", "twitter.", "x.com", "pinterest.", "tiktok.", "wikipedia.org",
    "quora.", "techpowerup.com/forums", "linustechtips.com", "tomshardware.com/forum", "duckduckgo.",
];

#[derive(Debug, Clone)]
pub struct FoundImage {
    pub bytes: Vec<u8>,
    /// "image/png", "image/jpeg", ...
    pub mime: String,
    pub image_url: String,
    /// The page the picture came from (shown as attribution).
    pub page_url: String,
    /// "web" or "wikipedia".
    pub source: String,
}

#[derive(Debug)]
pub enum FindError {
    /// The search engine is rate-limiting us; try again later.
    Throttled,
    NotFound,
    Network(String),
}

impl std::fmt::Display for FindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FindError::Throttled => write!(f, "search engine asked us to slow down"),
            FindError::NotFound => write!(f, "no picture found"),
            FindError::Network(e) => write!(f, "network error: {e}"),
        }
    }
}

#[derive(Clone, Copy)]
enum Engine {
    Brave,
    DuckDuckGo,
    BingImages,
}

pub struct ImageFinder {
    agent: ureq::Agent,
    /// Last query time per engine (Brave, DuckDuckGo, Bing Images).
    last_search: Mutex<[Option<Instant>; 3]>,
}

impl Default for ImageFinder {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageFinder {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .max_redirects(5)
            .build()
            .into();
        ImageFinder { agent, last_search: Mutex::new([None, None, None]) }
    }

    /// Finds a picture for one part. Blocks (it waits between searches).
    pub fn find(&self, part: &query::PartQuery) -> Result<FoundImage, FindError> {
        if let Some(url) = &part.direct_url
            && let Some((bytes, mime)) = self.download_image(url)
        {
            let page_url = part.direct_page.clone().unwrap_or_else(|| url.clone());
            return Ok(FoundImage { bytes, mime, image_url: url.clone(), page_url, source: "maker".into() });
        }
        let mut throttled = 0;
        match self.find_on_bing_images(&part.query, part.brand.as_deref()) {
            Ok(img) => return Ok(img),
            Err(FindError::Throttled) => throttled += 1,
            Err(_) => {}
        }
        for engine in [Engine::Brave, Engine::DuckDuckGo] {
            match self.find_on_web(engine, &part.query) {
                Ok(img) => return Ok(img),
                Err(FindError::Throttled) => throttled += 1,
                Err(_) => {}
            }
        }
        if let Some(hint) = &part.wiki_hint
            && let Ok(img) = self.find_on_wikipedia(hint)
        {
            return Ok(img);
        }
        Err(if throttled == 3 { FindError::Throttled } else { FindError::NotFound })
    }

    fn find_on_bing_images(&self, query: &str, brand: Option<&str>) -> Result<FoundImage, FindError> {
        self.wait_turn(Engine::BingImages);
        let mut resp = self
            .agent
            .get("https://www.bing.com/images/async")
            .query("q", query)
            .query("first", "0")
            .query("count", "20")
            .query("mmasync", "1")
            .header("User-Agent", USER_AGENT)
            .header("Accept-Language", "en-US,en;q=0.9")
            .call()
            .map_err(|e| FindError::Network(e.to_string()))?;
        if resp.status().as_u16() != 200 {
            return Err(FindError::Throttled);
        }
        let page = resp
            .body_mut()
            .with_config()
            .limit(MAX_PAGE)
            .read_to_string()
            .map_err(|e| FindError::Network(e.to_string()))?;
        let mut results = html::bing_image_results(&page);
        if results.is_empty() {
            return Err(FindError::NotFound);
        }
        // The brand's own website first: that is the exact product photo.
        if let Some(b) = brand.map(brand_domain_key).filter(|b| !b.is_empty()) {
            results.sort_by_key(|(img, pg)| !(host_of(img).contains(&b) || host_of(pg).contains(&b)));
        }
        for (img, page_url) in results.into_iter().take(5) {
            if let Some((bytes, mime)) = self.download_image(&img) {
                return Ok(FoundImage { bytes, mime, image_url: img, page_url, source: "web".into() });
            }
        }
        Err(FindError::NotFound)
    }

    fn wait_turn(&self, engine: Engine) {
        let mut last = self.last_search.lock().unwrap_or_else(|e| e.into_inner());
        let slot = &mut last[engine as usize];
        if let Some(t) = *slot {
            let since = t.elapsed();
            if since < MIN_SEARCH_GAP {
                std::thread::sleep(MIN_SEARCH_GAP - since);
            }
        }
        *slot = Some(Instant::now());
    }

    fn find_on_web(&self, engine: Engine, query: &str) -> Result<FoundImage, FindError> {
        self.wait_turn(engine);
        let request = match engine {
            Engine::Brave => self.agent.get("https://search.brave.com/search").query("q", query),
            Engine::DuckDuckGo | Engine::BingImages => self.agent.get("https://html.duckduckgo.com/html/").query("q", query),
        };
        let mut resp = request
            .header("User-Agent", USER_AGENT)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.9")
            .call()
            .map_err(|e| FindError::Network(e.to_string()))?;
        // Engines answer 202/429/403 with a challenge page when rate-limiting.
        if resp.status().as_u16() != 200 {
            return Err(FindError::Throttled);
        }
        let page = resp
            .body_mut()
            .with_config()
            .limit(MAX_PAGE)
            .read_to_string()
            .map_err(|e| FindError::Network(e.to_string()))?;
        let links = match engine {
            Engine::Brave => html::plain_result_links(&page, "brave.com"),
            Engine::DuckDuckGo | Engine::BingImages => html::ddg_result_links(&page),
        };
        let links: Vec<String> = links
            .into_iter()
            .filter(|u| !SKIP_DOMAINS.iter().any(|d| u.contains(d)))
            .take(4)
            .collect();
        if links.is_empty() {
            return Err(FindError::NotFound);
        }
        for link in links {
            if let Some(img) = self.image_from_page(&link) {
                return Ok(img);
            }
        }
        Err(FindError::NotFound)
    }

    fn image_from_page(&self, page_url: &str) -> Option<FoundImage> {
        let mut resp = self.agent.get(page_url).header("User-Agent", USER_AGENT).call().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let final_url = resp.get_uri().to_string();
        let body = resp.body_mut().with_config().limit(MAX_PAGE).read_to_string().ok()?;
        let image_url = html::preview_image(&body, &final_url)?;
        let (bytes, mime) = self.download_image(&image_url)?;
        Some(FoundImage { bytes, mime, image_url, page_url: final_url, source: "web".into() })
    }

    fn find_on_wikipedia(&self, title: &str) -> Result<FoundImage, FindError> {
        let mut resp = self
            .agent
            .get("https://en.wikipedia.org/w/api.php")
            .query("action", "query")
            .query("format", "json")
            .query("prop", "pageimages")
            .query("piprop", "original")
            .query("redirects", "1")
            .query("titles", title)
            .header("User-Agent", "Syscura/0.1 (hardware health app)")
            .call()
            .map_err(|e| FindError::Network(e.to_string()))?;
        let text = resp
            .body_mut()
            .with_config()
            .limit(MAX_PAGE)
            .read_to_string()
            .map_err(|e| FindError::Network(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).map_err(|_| FindError::NotFound)?;
        let pages = json["query"]["pages"].as_object().ok_or(FindError::NotFound)?;
        let (page, image_url) = pages
            .values()
            .find_map(|p| Some((p["title"].as_str()?.to_string(), p["original"]["source"].as_str()?.to_string())))
            .ok_or(FindError::NotFound)?;
        let (bytes, mime) = self.download_image(&image_url).ok_or(FindError::NotFound)?;
        Ok(FoundImage {
            bytes,
            mime,
            image_url,
            page_url: format!("https://en.wikipedia.org/wiki/{}", page.replace(' ', "_")),
            source: "wikipedia".into(),
        })
    }

    /// Downloads and checks that the bytes really are an image (the server's
    /// content type is not trusted).
    pub fn download_image(&self, url: &str) -> Option<(Vec<u8>, String)> {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return None;
        }
        let mut resp = self.agent.get(url).header("User-Agent", USER_AGENT).call().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = resp.body_mut().with_config().limit(MAX_IMAGE).read_to_vec().ok()?;
        let mime = sniff_image(&bytes)?;
        Some((bytes, mime.to_string()))
    }
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").to_ascii_lowercase()
}

/// "G.Skill" -> "gskill", "Western Digital" -> "westerndigital".
fn brand_domain_key(brand: &str) -> String {
    let b: String = brand.to_ascii_lowercase().chars().filter(char::is_ascii_alphanumeric).collect();
    match b.as_str() {
        "wd" | "wdc" => "westerndigital".into(),
        _ => b,
    }
}

/// Image type from magic bytes. Only raster formats; SVG is refused because
/// it can carry scripts.
pub fn sniff_image(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.len() > 12 && &b[4..8] == b"ftyp" && (&b[8..12] == b"avif" || &b[8..12] == b"avis") {
        Some("image/avif")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_keys() {
        assert_eq!(brand_domain_key("G.Skill"), "gskill");
        assert_eq!(host_of("https://www.GSkill.com/x.png"), "www.gskill.com");
    }

    #[test]
    fn sniffing() {
        assert_eq!(sniff_image(&[0x89, b'P', b'N', b'G', 0, 0]), Some("image/png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"<svg xmlns=..."), None);
        assert_eq!(sniff_image(b"<html>"), None);
    }
}
