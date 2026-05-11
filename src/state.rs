/// Runtime state persisted across sessions (state.json).
///
/// Stored in the same config directory as `config.toml` and `playlists.json`.
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::queue::PlayMode;
use crate::queue::track::Track;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppState {
    /// Ordered list of tracks in the playback queue.
    #[serde(default)]
    pub queue_tracks: Vec<Track>,

    /// Index of the currently selected track within `queue_tracks`.
    /// `None` means nothing is selected (empty queue or playback stopped at end).
    #[serde(default)]
    pub queue_current_index: Option<usize>,

    /// Last active play mode.
    #[serde(default)]
    pub play_mode: PlayMode,
}

impl AppState {
    fn storage_path() -> anyhow::Result<PathBuf> {
        let dir = Config::config_dir().context("cannot determine config dir")?;
        Ok(dir.join("state.json"))
    }

    /// Load from disk. Returns a default (empty) state if the file does not exist.
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::storage_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let state: Self = serde_json::from_str(&data)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(state)
    }

    /// Persist to disk. Creates the config directory if absent.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::storage_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating dir {}", dir.display()))?;
        }
        let data = serde_json::to_string_pretty(self).context("serializing state")?;
        std::fs::write(&path, &data)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn track(bvid: &str) -> Track {
        Track {
            bvid: bvid.to_string(),
            cid: 1,
            title: bvid.to_string(),
            author: "author".to_string(),
            duration: Duration::from_secs(180),
            cover_url: None,
            source: None,
        }
    }

    #[test]
    fn round_trip_empty() {
        let state = AppState::default();
        let json = serde_json::to_string_pretty(&state).unwrap();
        let back: AppState = serde_json::from_str(&json).unwrap();
        assert!(back.queue_tracks.is_empty());
        assert_eq!(back.queue_current_index, None);
        assert_eq!(back.play_mode, PlayMode::Sequential);
    }

    #[test]
    fn round_trip_with_tracks() {
        let state = AppState {
            queue_tracks: vec![track("BV1xx411c7mD"), track("BV2yy522d8Ne")],
            queue_current_index: Some(1),
            play_mode: PlayMode::RepeatList,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let back: AppState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.queue_tracks.len(), 2);
        assert_eq!(back.queue_tracks[1].bvid, "BV2yy522d8Ne");
        assert_eq!(back.queue_current_index, Some(1));
        assert_eq!(back.play_mode, PlayMode::RepeatList);
    }

    #[test]
    fn play_mode_serde_variants() {
        for mode in [
            PlayMode::Sequential,
            PlayMode::RepeatList,
            PlayMode::RepeatOne,
            PlayMode::Shuffle,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: PlayMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn source_field_skipped() {
        use crate::queue::track::TrackSource;
        use crate::queue::track::AudioQuality;
        let mut t = track("BV1xx");
        t.source = Some(TrackSource {
            stream_url: "https://example.com/audio.m4s".to_string(),
            audio_quality: AudioQuality::Dash { bitrate: 30280 },
            referer: "https://www.bilibili.com/".to_string(),
        });
        let state = AppState {
            queue_tracks: vec![t],
            queue_current_index: Some(0),
            play_mode: PlayMode::Sequential,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        // `source` must not appear in the serialized form
        assert!(!json.contains("stream_url"), "source should be skipped in JSON");
        let back: AppState = serde_json::from_str(&json).unwrap();
        assert!(back.queue_tracks[0].source.is_none());
    }
}
