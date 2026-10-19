/// Runtime state persisted across sessions (state.json).
///
/// Stored in the same config directory as `config.toml` and `playlists.json`.
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::queue::PlayMode;
use crate::queue::track::Track;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
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
        Self::load_from(&Self::storage_path()?)
    }

    pub(crate) fn load_from(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let state: Self =
            serde_json::from_str(&data).with_context(|| format!("parsing {}", path.display()))?;
        Ok(state)
    }

    /// Persist to disk. Creates the config directory if absent.
    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::storage_path()?)
    }

    pub(crate) fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        save_json_atomic(path, self)
    }
}

/// Replace the complete snapshot atomically; an interrupted write leaves the old
/// file intact. Callers write synchronously so older snapshots cannot overtake newer ones.
pub(crate) fn save_json_atomic(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let dir = path.parent().context("storage path has no parent")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating dir {}", dir.display()))?;
    let mut file = tempfile::NamedTempFile::new_in(dir).context("creating snapshot temp file")?;
    serde_json::to_writer_pretty(&mut file, value).context("serializing snapshot")?;
    file.flush().context("flushing snapshot")?;
    file.as_file().sync_all().context("syncing snapshot")?;
    file.persist(path)
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
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
            part: None,
            page: None,
            source: None,
        }
    }

    #[test]
    fn multi_part_queue_disk_snapshot_replaces_deleted_and_empty_contents() {
        use crate::queue::Queue;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut queue = Queue::new();
        for page in 1..=3 {
            let mut part = track("BVparts");
            part.cid = 1000 + page as u64;
            part.page = Some(page);
            part.part = Some(format!("P{page}"));
            queue.push(part);
        }
        let save = |queue: &Queue| {
            let (queue_tracks, queue_current_index, play_mode) = queue.snapshot();
            AppState {
                queue_tracks,
                queue_current_index,
                play_mode,
            }
            .save_to(&path)
            .unwrap();
        };
        save(&queue);
        queue.remove(1);
        save(&queue);
        let state = AppState::load_from(&path).unwrap();
        assert_eq!(
            state
                .queue_tracks
                .iter()
                .map(|t| (t.cid, t.page))
                .collect::<Vec<_>>(),
            vec![(1001, Some(1)), (1003, Some(3))]
        );
        queue.clear();
        save(&queue);
        let state = AppState::load_from(&path).unwrap();
        assert!(
            Queue::restore(
                state.queue_tracks,
                state.queue_current_index,
                state.play_mode
            )
            .is_empty()
        );
    }

    #[test]
    fn failed_snapshot_write_preserves_previous_file() {
        struct InvalidSnapshot;
        impl Serialize for InvalidSnapshot {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("injected serialization failure"))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let initial = AppState {
            queue_tracks: vec![track("BVkeep")],
            ..Default::default()
        };
        initial.save_to(&path).unwrap();
        assert!(save_json_atomic(&path, &InvalidSnapshot).is_err());
        assert_eq!(AppState::load_from(&path).unwrap(), initial);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
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
        use crate::queue::track::AudioQuality;
        use crate::queue::track::TrackSource;
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
        assert!(
            !json.contains("stream_url"),
            "source should be skipped in JSON"
        );
        let back: AppState = serde_json::from_str(&json).unwrap();
        assert!(back.queue_tracks[0].source.is_none());
    }
}
