//! Background picture finder: one worker thread works through the parts
//! that have no cached picture, pacing its searches, and records a status
//! the UI can poll.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde::Serialize;
use syscura_online::cache::ImageCache;
use syscura_online::query::PartQuery;
use syscura_online::{FindError, ImageFinder, sniff_image};

const MINUTE: i64 = 60 * 1000;

#[derive(Debug, Clone, Serialize)]
pub struct PartImage {
    pub key: String,
    pub kind: String,
    pub query: String,
    /// "ready", "queued", "searching", "waiting" (rate-limited), "offline",
    /// "not_found".
    pub status: String,
    /// Changes whenever the picture changes, so the UI can cache it.
    pub version: i64,
    pub page_url: String,
    pub source: String,
}

struct Inner {
    cache: ImageCache,
    finder: ImageFinder,
    parts: Mutex<Vec<PartQuery>>,
    /// Status by search phrase, for parts that are not ready yet.
    status: Mutex<HashMap<String, String>>,
}

pub struct ImageService {
    inner: Arc<Inner>,
    queue: Sender<PartQuery>,
}

impl ImageService {
    pub fn start(dir: &Path) -> std::io::Result<Self> {
        let inner = Arc::new(Inner {
            cache: ImageCache::open(dir)?,
            finder: ImageFinder::new(),
            parts: Mutex::new(Vec::new()),
            status: Mutex::new(HashMap::new()),
        });
        let (queue, rx) = mpsc::channel::<PartQuery>();
        let worker = inner.clone();
        std::thread::Builder::new()
            .name("pictures".into())
            .spawn(move || {
                for part in rx {
                    worker.fetch(&part);
                }
            })?;
        Ok(ImageService { inner, queue })
    }

    /// Sets the PC's parts and queues lookups for those without a picture.
    pub fn set_parts(&self, parts: Vec<PartQuery>) {
        for p in &parts {
            let cached = self.inner.cache.get(&p.query);
            // A generic (Wikipedia) picture is a stand-in: keep looking for
            // the exact model, at most once per app start.
            let upgrade = cached.as_ref().is_some_and(|(e, _)| e.source == "wikipedia")
                && !self.inner.lock_status().contains_key(&p.query);
            let direct_missing = p.direct_url.is_some() && cached.is_none();
            if (cached.is_none() && self.inner.cache.should_try(&p.query)) || direct_missing || upgrade {
                let mut status = self.inner.lock_status();
                if !matches!(status.get(&p.query).map(String::as_str), Some("queued" | "searching")) {
                    status.insert(p.query.clone(), "queued".into());
                    let _ = self.queue.send(p.clone());
                }
            }
        }
        *self.inner.parts.lock().unwrap_or_else(|e| e.into_inner()) = parts;
    }

    pub fn list(&self) -> Vec<PartImage> {
        let parts = self.inner.parts.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let status = self.inner.lock_status();
        parts
            .into_iter()
            .map(|p| match self.inner.cache.get(&p.query) {
                Some((e, _)) => PartImage {
                    key: p.key,
                    kind: p.kind,
                    query: p.query,
                    status: "ready".into(),
                    version: e.fetched_ms,
                    page_url: e.page_url,
                    source: e.source,
                },
                None => PartImage {
                    status: status.get(&p.query).cloned().unwrap_or_else(|| "waiting".into()),
                    key: p.key,
                    kind: p.kind,
                    query: p.query,
                    version: 0,
                    page_url: String::new(),
                    source: String::new(),
                },
            })
            .collect()
    }

    fn part(&self, key: &str) -> Option<PartQuery> {
        self.inner.parts.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|p| p.key == key).cloned()
    }

    /// The picture as a `data:` URL (the window's security policy only
    /// allows local and data images).
    pub fn data_url(&self, key: &str) -> Option<String> {
        let part = self.part(key)?;
        let (entry, path) = self.inner.cache.get(&part.query)?;
        let bytes = std::fs::read(path).ok()?;
        Some(format!("data:{};base64,{}", entry.mime, base64::engine::general_purpose::STANDARD.encode(bytes)))
    }

    /// Forgets the picture (and any failure) and searches again.
    pub fn retry(&self, key: &str) -> Result<(), String> {
        let part = self.part(key).ok_or("unknown part")?;
        self.inner.cache.remove(&part.query).map_err(|e| e.to_string())?;
        self.inner.lock_status().insert(part.query.clone(), "queued".into());
        self.queue.send(part).map_err(|e| e.to_string())
    }

    pub fn set_from_url(&self, key: &str, url: &str) -> Result<(), String> {
        let part = self.part(key).ok_or("unknown part")?;
        let (bytes, mime) = self
            .inner
            .finder
            .download_image(url.trim())
            .ok_or("That link is not a PNG, JPEG, WebP, GIF or AVIF picture.")?;
        self.inner.cache.put_user(&part.query, &bytes, &mime, url.trim()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_from_bytes(&self, key: &str, bytes: &[u8], file_name: &str) -> Result<(), String> {
        let part = self.part(key).ok_or("unknown part")?;
        let mime = sniff_image(bytes).ok_or("That file is not a PNG, JPEG, WebP, GIF or AVIF picture.")?;
        self.inner.cache.put_user(&part.query, bytes, mime, file_name).map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl Inner {
    fn lock_status(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fetch(&self, part: &PartQuery) {
        let existing = self.cache.get(&part.query).map(|(e, _)| e.source);
        if existing.as_deref().is_some_and(|s| s != "wikipedia") {
            self.lock_status().remove(&part.query);
            return;
        }
        if existing.is_some() {
            // Upgrading a generic picture: only accept an exact one.
            if let Ok(img) = self.finder.find(part)
                && img.source != "wikipedia"
            {
                let _ = self.cache.put(&part.query, &img);
            }
            self.lock_status().insert(part.query.clone(), "done".into());
            return;
        }
        self.lock_status().insert(part.query.clone(), "searching".into());
        let (status, retry_in) = match self.finder.find(part) {
            Ok(img) => match self.cache.put(&part.query, &img) {
                Ok(_) => {
                    self.lock_status().remove(&part.query);
                    return;
                }
                Err(_) => ("offline", 10 * MINUTE),
            },
            Err(FindError::Throttled) => ("waiting", 30 * MINUTE),
            Err(FindError::NotFound) => ("not_found", 7 * 24 * 60 * MINUTE),
            Err(FindError::Network(_)) => ("offline", 10 * MINUTE),
        };
        self.cache.mark_failed(&part.query, retry_in);
        self.lock_status().insert(part.query.clone(), status.into());
    }
}
