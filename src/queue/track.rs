use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A playable track from a Bilibili video.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Resolved audio source (lazy, not persisted)
    #[serde(skip)]
    pub source: Option<TrackSource>,
}

/// Resolved audio stream source for playback.
#[derive(Debug, Clone)]
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
