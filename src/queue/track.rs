use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A playable track from a Bilibili video.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// BV number (unique identifier)
    pub bvid: String,
    /// CID for multi-part videos
    pub cid: u64,
    /// Video title
    pub title: String,
    /// UP主 / author
    pub author: String,
    /// Video duration
    #[serde(
        serialize_with = "serialize_duration",
        deserialize_with = "deserialize_duration"
    )]
    pub duration: Duration,
    /// Cover image URL
    pub cover_url: Option<String>,
    /// Part title for multi-part videos (分P). `None` = single-part or whole video.
    ///
    /// `#[serde(default)]` so that `state.json` / `playlists.json` written before
    /// multi-part support existed still deserialize (as `None`).
    #[serde(default)]
    pub part: Option<String>,
    /// 1-based part index for multi-part videos. `None` = single-part or whole video.
    #[serde(default)]
    pub page: Option<u32>,
    /// Resolved audio source (lazy, not persisted)
    #[serde(skip)]
    pub source: Option<TrackSource>,
}

impl Track {
    /// Display title: the part title, falling back to the video title.
    pub fn display_title(&self) -> &str {
        match &self.part {
            Some(part) if !part.is_empty() => part,
            _ => &self.title,
        }
    }

    /// Whether two tracks point at the same playable item (same video *and* same part).
    ///
    /// Two parts of one video are distinct tracks, so `bvid` alone is not enough.
    pub fn same_item(&self, other: &Track) -> bool {
        self.bvid == other.bvid
            && match (self.page, other.page) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    if self.cid != 0 && other.cid != 0 {
                        self.cid == other.cid
                    } else {
                        a == b
                    }
                }
                _ => false,
            }
    }
}

/// Resolved audio stream source for playback.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSource {
    /// Audio stream URL (expires ~120 minutes)
    pub stream_url: String,
    /// Audio quality of this stream
    pub audio_quality: AudioQuality,
    /// Referer header for anti-hotlink bypass
    pub referer: String,
}

/// Audio quality levels from Bilibili DASH streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioQuality {
    /// FLAC lossless (quality id: 30251)
    Flac,
    /// Dolby Atmos (quality id: 30250)
    DolbyAtmos,
    /// DASH audio with bitrate info (quality id: 30280/30232/30216)
    Dash { bitrate: u32 },
    /// Legacy MP4 fallback
    LegacyMp4,
}

impl AudioQuality {
    /// Parse from Bilibili's quality id.
    pub fn from_quality_id(id: u32) -> Option<Self> {
        match id {
            30251 => Some(Self::Flac),
            30250 => Some(Self::DolbyAtmos),
            30280 | 30232 | 30216 => Some(Self::Dash { bitrate: id }),
            _ => None,
        }
    }

    /// Priority for quality selection (higher = better).
    pub fn priority(&self) -> u32 {
        match self {
            Self::Flac => 4,
            Self::DolbyAtmos => 3,
            Self::Dash { bitrate } => match bitrate {
                30280 => 2,
                30232 => 1,
                30216 => 0,
                _ => 0,
            },
            Self::LegacyMp4 => 0,
        }
    }
}

fn serialize_duration<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(d.as_secs())
}

fn deserialize_duration<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    let secs = u64::deserialize(d)?;
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(bvid: &str, cid: u64) -> Track {
        Track {
            bvid: bvid.to_string(),
            cid,
            title: "video".to_string(),
            author: "up".to_string(),
            duration: Duration::from_secs(60),
            cover_url: None,
            part: None,
            page: None,
            source: None,
        }
    }

    #[test]
    fn legacy_json_without_part_fields() {
        // `state.json` / `playlists.json` written before multi-part support.
        let json = r#"{
            "bvid": "BV1xx411c7mD",
            "cid": 789012,
            "title": "Legacy",
            "author": "UP",
            "duration": 225,
            "cover_url": null
        }"#;

        let t: Track = serde_json::from_str(json).unwrap();
        assert_eq!(t.bvid, "BV1xx411c7mD");
        assert_eq!(t.part, None);
        assert_eq!(t.page, None);
        assert_eq!(t.display_title(), "Legacy");
    }

    #[test]
    fn part_fields_round_trip() {
        let mut t = track("BV1", 1);
        t.part = Some("P2 title".to_string());
        t.page = Some(2);

        let json = serde_json::to_string(&t).unwrap();
        let back: Track = serde_json::from_str(&json).unwrap();
        assert_eq!(back.part, Some("P2 title".to_string()));
        assert_eq!(back.page, Some(2));
        assert_eq!(back.display_title(), "P2 title");
    }

    #[test]
    fn same_item_distinguishes_parts() {
        let mut p1 = track("BV1", 111);
        let mut p2 = track("BV1", 222);
        assert!(
            p1.same_item(&p1.clone()),
            "identical tracks are the same item"
        );

        p1.page = Some(1);
        p2.page = Some(2);
        assert!(
            !p1.same_item(&p2),
            "two parts of the same video must not be deduped together"
        );

        let other = track("BV2", 111);
        assert!(!p1.same_item(&other), "different videos differ");
    }

    #[test]
    fn empty_part_falls_back_to_title() {
        let mut t = track("BV1", 1);
        t.part = Some(String::new());
        assert_eq!(t.display_title(), "video");
    }
}
