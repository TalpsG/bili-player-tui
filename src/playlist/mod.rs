pub mod storage;

use crate::queue::track::Track;
use serde::{Deserialize, Serialize};

/// A named user playlist.  The "Queue" pseudo-playlist is NOT stored here —
/// it lives in `App::queue` and is represented at runtime by `playlist_cursor == 0`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    /// Display name (user-supplied, must be non-empty after trim)
    pub name: String,
    /// Tracks in insertion order.  `source` is skipped by serde on Track.
    pub tracks: Vec<Track>,
}

impl Playlist {
    /// Create an empty named playlist.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tracks: Vec::new(),
        }
    }

    /// Append a track if it is not already present (dedup by `bvid`).
    /// Returns `true` if the track was actually inserted.
    pub fn add_track(&mut self, track: Track) -> bool {
        if self.tracks.iter().any(|t| t.bvid == track.bvid) {
            return false;
        }
        self.tracks.push(track);
        true
    }

    /// Remove a track by index.  Returns the removed track, or `None` if index is out of range.
    pub fn remove_track(&mut self, index: usize) -> Option<Track> {
        if index < self.tracks.len() {
            Some(self.tracks.remove(index))
        } else {
            None
        }
    }
}

// ─── Unit tests ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
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
    fn dedup_by_bvid() {
        let mut pl = Playlist::new("test");
        assert!(pl.add_track(t("BV1")));
        assert!(!pl.add_track(t("BV1")));  // duplicate → rejected
        assert!(pl.add_track(t("BV2")));
        assert_eq!(pl.tracks.len(), 2);
    }

    #[test]
    fn remove_track() {
        let mut pl = Playlist::new("test");
        pl.add_track(t("BV1"));
        pl.add_track(t("BV2"));
        let removed = pl.remove_track(0).unwrap();
        assert_eq!(removed.bvid, "BV1");
        assert_eq!(pl.tracks.len(), 1);
        assert_eq!(pl.tracks[0].bvid, "BV2");
        assert!(pl.remove_track(99).is_none());
    }
}
