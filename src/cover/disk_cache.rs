use std::path::PathBuf;

use anyhow::Context;
use bytes::Bytes;

/// A simple flat-directory disk cache for cover image bytes.
///
/// File naming: take the last path segment of the URL, e.g.
///   `https://i2.hdslb.com/bfs/archive/a1b2c3.jpg` → `a1b2c3.jpg`
/// Bilibili CDN filenames are content hashes, so collisions are negligible.
pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn filename(url: &str) -> String {
        // strip query string first, then take last segment
        let url_no_query = url.split('?').next().unwrap_or(url);
        url_no_query
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("cover")
            .to_string()
    }

    fn path(&self, url: &str) -> PathBuf {
        self.dir.join(Self::filename(url))
    }

    /// Return `true` if the URL has a cached file on disk.
    pub fn has(&self, url: &str) -> bool {
        self.path(url).exists()
    }

    /// Read cached bytes for `url` from disk.
    pub async fn load(&self, url: &str) -> anyhow::Result<Bytes> {
        let path = self.path(url);
        let data = tokio::fs::read(&path)
            .await
            .with_context(|| format!("reading cover cache {}", path.display()))?;
        Ok(Bytes::from(data))
    }

    /// Write `data` to disk for `url`.  Creates the cache directory if needed.
    pub async fn save(&self, url: &str, data: &[u8]) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .with_context(|| format!("creating cover cache dir {}", self.dir.display()))?;
        let path = self.path(url);
        tokio::fs::write(&path, data)
            .await
            .with_context(|| format!("writing cover cache {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DiskCache;

    #[test]
    fn filename_strips_query_and_takes_last_segment() {
        assert_eq!(
            DiskCache::filename("https://i2.hdslb.com/bfs/archive/a1b2c3.jpg?foo=bar"),
            "a1b2c3.jpg"
        );
        assert_eq!(
            DiskCache::filename("https://i2.hdslb.com/bfs/archive/a1b2c3.jpg"),
            "a1b2c3.jpg"
        );
    }

    #[test]
    fn filename_fallback_for_empty_segment() {
        assert_eq!(DiskCache::filename("https://example.com/"), "cover");
    }

    #[tokio::test]
    async fn round_trip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let url = "https://i2.hdslb.com/bfs/archive/testimage.jpg";
        let data = b"fake image bytes";

        cache.save(url, data).await.unwrap();
        assert!(cache.has(url));

        let loaded = cache.load(url).await.unwrap();
        assert_eq!(&loaded[..], data);
    }
}
