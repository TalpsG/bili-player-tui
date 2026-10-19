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

use self::disk_cache::DiskCache;
use crate::config::Config;

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
        prepared_size: Option<(u16, u16, u32, u32)>,
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
    pub fn new(mut picker: Picker) -> Self {
        picker.font_size = (picker.font_size.0.max(1), picker.font_size.1.max(1));
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
        if area.is_empty() {
            return false;
        }
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
                            c.put(
                                url_owned,
                                CoverState::Ready {
                                    proto,
                                    img,
                                    prepared_size: None,
                                },
                            );
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
        if let Some(CoverState::Ready {
            proto,
            img,
            prepared_size,
        }) = cache.get_mut(url)
        {
            let geometry = contain_geometry(img.width(), img.height(), self.picker.font_size, area);
            let Some((target, pixels)) = geometry else {
                return false;
            };
            let size = (target.width, target.height);
            let key = (size.0, size.1, pixels.0, pixels.1);
            if *prepared_size != Some(key) {
                let canvas = prepare_image(img, pixels, self.picker.font_size, size);
                *proto = self.picker.new_resize_protocol(canvas);
                *prepared_size = Some(key);
            }
            // The prepared canvas is already cell-aligned. Crop performs no rescaling.
            let image = StatefulImage::new(None).resize(Resize::Crop(None));
            f.render_stateful_widget(image, target, proto);
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
                    c.put(
                        url_owned,
                        CoverState::Ready {
                            proto,
                            img,
                            prepared_size: None,
                        },
                    );
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
            if let CoverState::Ready {
                proto,
                img,
                prepared_size,
            } = state
            {
                *proto = picker.new_resize_protocol(img.clone());
                *prepared_size = None;
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

/// Fit in physical pixels, including upscaling; center the cell rectangle and
/// the sub-cell padding independently. Cap axes because ratatui-image 1.x
/// multiplies its cell counts and font dimensions as u16 internally.
fn contain_geometry(w: u32, h: u32, cell: (u16, u16), area: Rect) -> Option<(Rect, (u32, u32))> {
    if w == 0 || h == 0 || area.is_empty() {
        return None;
    }
    let cw = cell.0.max(1);
    let ch = cell.1.max(1);
    let cols = area.width.min(u16::MAX / cw);
    let rows = area.height.min(u16::MAX / ch);
    if cols == 0 || rows == 0 {
        return None;
    }
    let pw = u64::from(cols) * u64::from(cw);
    let ph = u64::from(rows) * u64::from(ch);
    let (iw, ih) = if pw * u64::from(h) <= ph * u64::from(w) {
        (pw, (pw * u64::from(h) / u64::from(w)).max(1))
    } else {
        ((ph * u64::from(w) / u64::from(h)).max(1), ph)
    };
    let width = iw.div_ceil(u64::from(cw)) as u16;
    let height = ih.div_ceil(u64::from(ch)) as u16;
    Some((
        Rect::new(
            area.x.saturating_add((area.width - width) / 2),
            area.y.saturating_add((area.height - height) / 2),
            width,
            height,
        ),
        (iw as u32, ih as u32),
    ))
}

fn prepare_image(
    img: &DynamicImage,
    pixels: (u32, u32),
    cell: (u16, u16),
    size: (u16, u16),
) -> DynamicImage {
    let width = u32::from(size.0) * u32::from(cell.0.max(1));
    let height = u32::from(size.1) * u32::from(cell.1.max(1));
    let resized = img.resize_exact(pixels.0, pixels.1, image::imageops::FilterType::Lanczos3);
    let mut canvas = DynamicImage::new_rgb8(width, height);
    image::imageops::overlay(
        &mut canvas,
        &resized,
        i64::from((width - pixels.0) / 2),
        i64::from((height - pixels.1) / 2),
    );
    canvas
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    #[test]
    fn contain_varied_aspects_and_cells() {
        for (w, h) in [(160, 90), (90, 160), (100, 100), (2, 1)] {
            for cell in [(8, 16), (10, 13), (1, 1)] {
                for area in [Rect::new(3, 5, 80, 24), Rect::new(0, 0, 1, 1)] {
                    let (r, (pw, ph)) = contain_geometry(w, h, cell, area).unwrap();
                    assert!(r.right() <= area.right() && r.bottom() <= area.bottom());
                    assert!(
                        (i32::from(r.x - area.x) * 2 - i32::from(area.width - r.width)).abs() <= 1
                    );
                    assert!(
                        (i32::from(r.y - area.y) * 2 - i32::from(area.height - r.height)).abs()
                            <= 1
                    );
                    assert!(pw <= u32::from(r.width) * u32::from(cell.0));
                    assert!(ph <= u32::from(r.height) * u32::from(cell.1));
                    let canvas = prepare_image(
                        &DynamicImage::new_rgb8(w, h),
                        (pw, ph),
                        cell,
                        (r.width, r.height),
                    );
                    assert_eq!(
                        (canvas.width(), canvas.height()),
                        (
                            u32::from(r.width) * u32::from(cell.0),
                            u32::from(r.height) * u32::from(cell.1)
                        )
                    );
                }
            }
        }
    }

    #[test]
    fn output_content_is_centered_and_keeps_aspect() {
        use image::{GenericImageView, Rgb, RgbImage};
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(7, 13, Rgb([255, 255, 255])));
        let (area, pixels) = contain_geometry(7, 13, (10, 13), Rect::new(0, 0, 21, 9)).unwrap();
        let canvas = prepare_image(&img, pixels, (10, 13), (area.width, area.height));
        let white: Vec<_> = canvas.pixels().filter(|(_, _, c)| c.0[0] == 255).collect();
        let min_x = white.iter().map(|p| p.0).min().unwrap();
        let max_x = white.iter().map(|p| p.0).max().unwrap();
        let min_y = white.iter().map(|p| p.1).min().unwrap();
        let max_y = white.iter().map(|p| p.1).max().unwrap();
        assert_eq!((max_x - min_x + 1, max_y - min_y + 1), pixels);
        assert!(min_x.abs_diff(canvas.width() - max_x - 1) <= 1);
        assert!(min_y.abs_diff(canvas.height() - max_y - 1) <= 1);
        assert!((i64::from(pixels.0) * 13 - i64::from(pixels.1) * 7).abs() <= 13);
    }

    #[test]
    fn small_images_upscale_and_empty_is_safe() {
        let (_, pixels) = contain_geometry(2, 1, (10, 13), Rect::new(0, 0, 80, 24)).unwrap();
        assert_eq!(pixels, (624, 312));
        assert!(contain_geometry(2, 1, (8, 16), Rect::default()).is_none());
        let (r, _) = contain_geometry(
            u32::MAX,
            u32::MAX,
            (u16::MAX, u16::MAX),
            Rect::new(0, 0, u16::MAX, u16::MAX),
        )
        .unwrap();
        assert_eq!((r.width, r.height), (1, 1));
    }
}
