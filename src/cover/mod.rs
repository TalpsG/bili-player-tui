/// Cover image management: async download + LRU cache + ratatui-image protocol state.
///
/// Public entry point: `CoverManager::render_cover()`.
/// Call it from the draw function each frame; it fires background downloads automatically
/// and renders the image when ready, returning `false` while still loading so callers
/// can draw the ASCII placeholder instead.
///
/// ## Two-level cache
///
/// L1 — in-memory LRU (up to `CACHE_CAPACITY` decoded images).
/// L2 — flat-directory disk cache under `<config_dir>/covers/`.
///
/// On every `render_cover()` or `prewarm()` call the cache layers are checked in order:
///   L1 hit  → render immediately (no I/O)
///   L1 miss, L2 hit → read bytes from disk → decode → populate L1 → render
///   both miss → reqwest GET → save to disk (L2) → decode → populate L1 → render
///
/// ## tmux / focus-change robustness
///
/// Terminal graphics protocols (Kitty, Sixel) store pixel data inside the terminal
/// emulator, not in the ratatui buffer.  When the user switches away in tmux and returns,
/// the terminal clears those placements — ratatui only replays its cell buffer which has no
/// pixel data.  To recover:
///
/// 1. `CoverState::Ready` keeps a clone of the decoded `DynamicImage` alongside the
///    `StatefulProtocol`.
/// 2. `CoverManager::invalidate_all()` rebuilds every `Ready` entry from its stored
///    `DynamicImage`, forcing a full re-upload on the next render frame.
/// 3. `App::run()` enables `EnableFocusChange` and calls `invalidate_all()` on
///    `CrosstermEvent::FocusGained`.
pub mod disk_cache;
pub mod protocol;

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use image::DynamicImage;
use lru::LruCache;
use ratatui::layout::Rect;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

use crate::config::Config;
use self::disk_cache::DiskCache;

/// Number of decoded images kept in the LRU cache.
const CACHE_CAPACITY: usize = 20;

/// Internal state for one cover URL.
enum CoverState {
    /// Decoded image + encoded protocol state, ready to render.
    /// We keep the `DynamicImage` so we can rebuild the protocol state after a
    /// terminal focus-loss/gain cycle (e.g. tmux window switch).
    Ready {
        proto: Box<dyn StatefulProtocol>,
        img: DynamicImage,
    },
    /// Download/decode still in progress.
    Loading,
    /// Download or decode failed permanently.
    Failed,
}

// StatefulProtocol is Send + Sync, so CoverState is too.
unsafe impl Send for CoverState {}
unsafe impl Sync for CoverState {}

type CoverMap = Arc<Mutex<LruCache<String, CoverState>>>;

/// Manages asynchronous cover image fetching and rendering.
pub struct CoverManager {
    picker: Picker,
    cache: CoverMap,
    disk_cache: Option<Arc<DiskCache>>,
}

impl CoverManager {
    pub fn new(picker: Picker) -> Self {
        // Try to set up disk cache; if config_dir fails, run without it.
        let disk_cache = Config::config_dir()
            .ok()
            .map(|dir| Arc::new(DiskCache::new(dir.join("covers"))));

        Self {
            picker,
            cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(CACHE_CAPACITY).unwrap(),
            ))),
            disk_cache,
        }
    }

    /// Try to render the cover for `url` into `area`.
    ///
    /// - Returns `true`  → image was rendered; caller should skip the ASCII placeholder.
    /// - Returns `false` → still loading / failed; caller should draw the ASCII placeholder.
    ///
    /// A background download is spawned automatically on first call for a URL.
    pub fn render_cover(&mut self, url: &str, area: Rect, f: &mut ratatui::Frame) -> bool {
        // Phase 1: ensure a download is in flight (lock, check, maybe insert + spawn).
        {
            let mut cache = self.cache.lock().unwrap();
            if !cache.contains(url) {
                cache.put(url.to_string(), CoverState::Loading);
                let url_owned = url.to_string();
                let cache2 = Arc::clone(&self.cache);
                let mut picker = self.picker;
                let disk_cache = self.disk_cache.clone();
                tokio::spawn(async move {
                    match fetch_image(&url_owned, disk_cache).await {
                        Ok(img) => {
                            let proto = picker.new_resize_protocol(img.clone());
                            let mut c = cache2.lock().unwrap();
                            c.put(url_owned, CoverState::Ready { proto, img });
                        }
                        Err(e) => {
                            eprintln!("Cover download failed: {e}");
                            let mut c = cache2.lock().unwrap();
                            c.put(url_owned, CoverState::Failed);
                        }
                    }
                });
                return false;
            }
        }

        // Phase 2: if ready, render while holding the lock.
        let mut cache = self.cache.lock().unwrap();
        if let Some(CoverState::Ready { proto, .. }) = cache.get_mut(url) {
            let image = StatefulImage::new(None).resize(Resize::Fit(None));
            f.render_stateful_widget(image, area, proto);
            true
        } else {
            false
        }
    }

    /// Kick off a background load for `url` without rendering.
    /// Used at startup to populate L1 from disk (or network) for all known cover URLs.
    pub fn prewarm(&mut self, url: &str) {
        let mut cache = self.cache.lock().unwrap();
        if cache.contains(url) {
            return; // already loading or ready
        }
        cache.put(url.to_string(), CoverState::Loading);
        let url_owned = url.to_string();
        let cache2 = Arc::clone(&self.cache);
        let mut picker = self.picker;
        let disk_cache = self.disk_cache.clone();
        tokio::spawn(async move {
            match fetch_image(&url_owned, disk_cache).await {
                Ok(img) => {
                    let proto = picker.new_resize_protocol(img.clone());
                    let mut c = cache2.lock().unwrap();
                    c.put(url_owned, CoverState::Ready { proto, img });
                }
                Err(e) => {
                    eprintln!("Cover prewarm failed: {e}");
                    let mut c = cache2.lock().unwrap();
                    c.put(url_owned, CoverState::Failed);
                }
            }
        });
    }

    /// Rebuild every `Ready` entry's protocol state from its stored `DynamicImage`.
    ///
    /// Call this when the terminal regains focus (e.g. after a tmux window switch) so
    /// that all cover images are fully re-uploaded on the next render frame.
    pub fn invalidate_all(&mut self) {
        let mut cache = self.cache.lock().unwrap();
        let mut picker = self.picker;
        for (_, state) in cache.iter_mut() {
            if let CoverState::Ready { proto, img } = state {
                *proto = picker.new_resize_protocol(img.clone());
            }
        }
    }

    /// Return `true` if a cover has been successfully downloaded and encoded.
    pub fn is_ready(&self, url: &str) -> bool {
        self.cache
            .lock()
            .unwrap()
            .peek(url)
            .is_some_and(|s| matches!(s, CoverState::Ready { .. }))
    }

    /// Return `true` if the active graphics protocol stores pixel data outside
    /// ratatui's cell buffer (Sixel, Kitty, iTerm2) and therefore requires a
    /// `terminal.clear()` to force ratatui to re-emit the escape sequences after
    /// a tmux window switch.
    ///
    /// For `Halfblocks`, images are rendered as real Unicode characters tracked in
    /// ratatui's normal cell buffer — tmux preserves them just fine, so no clear
    /// is needed and we can avoid the visible screen flash.
    pub fn needs_terminal_clear_on_focus(&self) -> bool {
        !matches!(self.picker.protocol_type, ProtocolType::Halfblocks)
    }
}

/// Fetch a cover image: check disk cache first, then network.
/// Saves new downloads to disk automatically.
async fn fetch_image(
    url: &str,
    disk_cache: Option<Arc<DiskCache>>,
) -> anyhow::Result<DynamicImage> {
    // L2: disk cache — on any failure (no entry, I/O error, corrupt bytes)
    // we fall through to the network fetch below.
    if let Some(ref dc) = disk_cache
        && dc.has(url)
        && let Ok(bytes) = dc.load(url).await
        && let Ok(img) = image::load_from_memory(&bytes)
    {
        return Ok(img);
    }

    // Network download
    let normalized = normalize_url(url);
    let bytes: Bytes = reqwest::get(normalized.as_ref()).await?.bytes().await?;

    // Save to disk cache (best-effort — don't fail if save fails)
    if let Some(ref dc) = disk_cache {
        let _ = dc.save(url, &bytes).await;
    }

    let img = image::load_from_memory(&bytes)?;
    Ok(img)
}

/// Convert a protocol-relative URL (`//host/path`) to `https://host/path`.
/// Absolute URLs are returned unchanged.
fn normalize_url(url: &str) -> std::borrow::Cow<'_, str> {
    if url.starts_with("//") {
        std::borrow::Cow::Owned(format!("https:{url}"))
    } else {
        std::borrow::Cow::Borrowed(url)
    }
}
