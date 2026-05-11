use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use super::Playlist;

/// Top-level on-disk format.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlaylistStore {
    pub playlists: Vec<Playlist>,
}

impl PlaylistStore {
    fn storage_path() -> anyhow::Result<PathBuf> {
        let dir = Config::config_dir()
            .context("cannot determine config dir")?;
        Ok(dir.join("playlists.json"))
    }

    /// Load from disk.  If the file does not exist, return an empty store (not an error).
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::storage_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let store: Self = serde_json::from_str(&data)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(store)
    }

    /// Persist to disk.  Creates the config directory if absent.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::storage_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating dir {}", dir.display()))?;
        }
        let data = serde_json::to_string_pretty(self)
            .context("serializing playlists")?;
        std::fs::write(&path, data)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlist::Playlist;
    use crate::queue::track::Track;
    use std::time::Duration;

    fn t(bvid: &str) -> Track {
        Track {
            bvid: bvid.to_string(),
            cid: 0,
            title: bvid.to_string(),
            author: "a".to_string(),
            duration: Duration::from_secs(60),
            cover_url: None,
            source: None,
        }
    }

    #[test]
    fn round_trip_json() {
        // Test serialization/deserialization directly without touching the filesystem config path
        let mut store = PlaylistStore::default();
        let mut pl = Playlist::new("Favourites");
        pl.add_track(t("BV1"));
        pl.add_track(t("BV2"));
        store.playlists.push(pl);

        let data = serde_json::to_string_pretty(&store).unwrap();
        let store2: PlaylistStore = serde_json::from_str(&data).unwrap();

        assert_eq!(store2.playlists.len(), 1);
        assert_eq!(store2.playlists[0].name, "Favourites");
        assert_eq!(store2.playlists[0].tracks.len(), 2);
        assert_eq!(store2.playlists[0].tracks[0].bvid, "BV1");
    }
}
