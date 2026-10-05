//! On-disk picture cache: `<dir>/index.json` plus one image file per entry.
//! Keyed by the search phrase, so identical parts share one picture and a
//! part is looked up online only once.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use syscura_core::now_ms;

use crate::FoundImage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedImage {
    pub query: String,
    /// File name inside the cache directory.
    pub file: String,
    pub mime: String,
    pub image_url: String,
    pub page_url: String,
    /// "web", "wikipedia" or "user".
    pub source: String,
    pub fetched_ms: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    images: BTreeMap<String, CachedImage>,
    /// Failed lookups: query -> earliest time (ms) to try again.
    retry_after: BTreeMap<String, i64>,
}

pub struct ImageCache {
    dir: PathBuf,
    index: Mutex<Index>,
}

impl ImageCache {
    pub fn open(dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let index = std::fs::read_to_string(dir.join("index.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(ImageCache { dir: dir.to_path_buf(), index: Mutex::new(index) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Index> {
        self.index.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The cached picture and its full path, if any.
    pub fn get(&self, query: &str) -> Option<(CachedImage, PathBuf)> {
        let entry = self.lock().images.get(query).cloned()?;
        let path = self.dir.join(&entry.file);
        path.exists().then_some((entry, path))
    }

    /// True if no picture is cached and no recent failure says "wait".
    pub fn should_try(&self, query: &str) -> bool {
        let idx = self.lock();
        !idx.images.contains_key(query) && idx.retry_after.get(query).is_none_or(|&t| now_ms() >= t)
    }

    pub fn put(&self, query: &str, img: &FoundImage) -> io::Result<CachedImage> {
        self.store(query, &img.bytes, &img.mime, &img.image_url, &img.page_url, &img.source)
    }

    /// A picture the user chose themselves (file or link).
    pub fn put_user(&self, query: &str, bytes: &[u8], mime: &str, origin: &str) -> io::Result<CachedImage> {
        self.store(query, bytes, mime, origin, origin, "user")
    }

    fn store(&self, query: &str, bytes: &[u8], mime: &str, image_url: &str, page_url: &str, source: &str) -> io::Result<CachedImage> {
        let ext = match mime {
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/avif" => "avif",
            _ => "jpg",
        };
        let file = format!("{:016x}.{ext}", fnv1a(query));
        std::fs::write(self.dir.join(&file), bytes)?;
        let entry = CachedImage {
            query: query.to_string(),
            file,
            mime: mime.to_string(),
            image_url: image_url.to_string(),
            page_url: page_url.to_string(),
            source: source.to_string(),
            fetched_ms: now_ms(),
        };
        let mut idx = self.lock();
        idx.images.insert(query.to_string(), entry.clone());
        idx.retry_after.remove(query);
        self.save(&idx)?;
        Ok(entry)
    }

    pub fn mark_failed(&self, query: &str, retry_in_ms: i64) {
        let mut idx = self.lock();
        idx.retry_after.insert(query.to_string(), now_ms() + retry_in_ms);
        let _ = self.save(&idx);
    }

    pub fn remove(&self, query: &str) -> io::Result<()> {
        let mut idx = self.lock();
        if let Some(e) = idx.images.remove(query) {
            let _ = std::fs::remove_file(self.dir.join(e.file));
        }
        idx.retry_after.remove(query);
        self.save(&idx)
    }

    fn save(&self, idx: &Index) -> io::Result<()> {
        let json = serde_json::to_string_pretty(idx).map_err(io::Error::other)?;
        // Write then rename, so a crash never leaves a half-written index.
        let tmp = self.dir.join("index.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, self.dir.join("index.json"))
    }
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reloads() {
        let dir = std::env::temp_dir().join(format!("syscura-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let img = FoundImage {
            bytes: vec![0x89, b'P', b'N', b'G'],
            mime: "image/png".into(),
            image_url: "https://x/i.png".into(),
            page_url: "https://x/".into(),
            source: "web".into(),
        };
        {
            let c = ImageCache::open(&dir).unwrap();
            assert!(c.should_try("q1"));
            c.put("q1", &img).unwrap();
            c.mark_failed("q2", 60_000);
            assert!(!c.should_try("q1"), "already cached");
            assert!(!c.should_try("q2"), "failed recently");
        }
        let c = ImageCache::open(&dir).unwrap();
        let (entry, path) = c.get("q1").expect("survives reopen");
        assert_eq!(entry.mime, "image/png");
        assert_eq!(std::fs::read(path).unwrap(), img.bytes);
        c.remove("q1").unwrap();
        assert!(c.get("q1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
