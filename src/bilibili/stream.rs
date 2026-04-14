use serde::Deserialize;

use crate::bilibili::api::BilibiliClient;
use crate::error::BilibiliError;
use crate::queue::track::{AudioQuality, TrackSource};

const REFERER: &str = "https://www.bilibili.com";

#[derive(Debug, Deserialize)]
struct PlayUrlResponse {
    code: i64,
    data: Option<PlayUrlData>,
}

#[derive(Debug, Deserialize)]
struct PlayUrlData {
    #[serde(default)]
    dash: Option<DashData>,
    #[serde(default)]
    durl: Option<Vec<DurlItem>>,
}

#[derive(Debug, Deserialize)]
struct DashData {
    #[serde(default)]
    audio: Vec<DashAudioItem>,
    #[serde(default)]
    dolby: Option<DolbyData>,
    #[serde(default)]
    flac: Option<FlacData>,
}

#[derive(Debug, Deserialize)]
struct DashAudioItem {
    id: u32,
    base_url: Option<String>,
    #[serde(default)]
    backup_url: Option<Vec<String>>,
    bandwidth: u32,
}

#[derive(Debug, Deserialize)]
struct DolbyData {
    #[serde(default)]
    audio: Option<Vec<DashAudioItem>>,
}

#[derive(Debug, Deserialize)]
struct FlacData {
    #[serde(default)]
    audio: Option<DashAudioItem>,
}

#[derive(Debug, Deserialize)]
struct DurlItem {
    url: String,
    #[serde(default)]
    backup_url: Option<Vec<String>>,
}

/// Get the audio stream URL for a video.
///
/// Quality priority: FLAC > Dolby Atmos > DASH audio (best bitrate) > Legacy MP4
pub async fn get_audio_stream(
    client: &BilibiliClient,
    bvid: &str,
    cid: u64,
) -> Result<TrackSource, BilibiliError> {
    let params = vec![
        ("bvid".to_string(), bvid.to_string()),
        ("cid".to_string(), cid.to_string()),
        ("qn".to_string(), "64".to_string()),
        ("fnval".to_string(), "16".to_string()), // Request DASH format
    ];

    let resp: PlayUrlResponse = client
        .get("/x/player/wbi/playurl", params, true)
        .await?;

    if resp.code != 0 {
        return Err(BilibiliError::ApiResponse {
            code: resp.code,
            message: "Failed to get play URL".into(),
        });
    }

    let data = resp.data.ok_or_else(|| {
        BilibiliError::Parse("No data in play URL response".into())
    })?;

    // Try DASH format first
    if let Some(dash) = data.dash {
        // 1. Try FLAC
        if let Some(flac) = dash.flac {
            if let Some(audio) = flac.audio {
                if let Some(url) = pick_url(&audio.base_url, &audio.backup_url) {
                    return Ok(TrackSource {
                        stream_url: url,
                        audio_quality: AudioQuality::Flac,
                        referer: REFERER.to_string(),
                    });
                }
            }
        }

        // 2. Try Dolby Atmos
        if let Some(dolby) = dash.dolby {
            if let Some(audio_list) = dolby.audio {
                if let Some(audio) = audio_list.into_iter().next() {
                    if let Some(url) = pick_url(&audio.base_url, &audio.backup_url) {
                        return Ok(TrackSource {
                            stream_url: url,
                            audio_quality: AudioQuality::DolbyAtmos,
                            referer: REFERER.to_string(),
                        });
                    }
                }
            }
        }

        // 3. Try DASH audio (select best bitrate)
        if !dash.audio.is_empty() {
            let best = dash
                .audio
                .into_iter()
                .max_by_key(|a| a.bandwidth)
                .unwrap();

            if let Some(url) = pick_url(&best.base_url, &best.backup_url) {
                let quality = AudioQuality::from_quality_id(best.id)
                    .unwrap_or(AudioQuality::Dash { bitrate: best.id });
                return Ok(TrackSource {
                    stream_url: url,
                    audio_quality: quality,
                    referer: REFERER.to_string(),
                });
            }
        }
    }

    // 4. Fallback to legacy MP4 (durl)
    if let Some(durl) = data.durl {
        if let Some(first) = durl.into_iter().next() {
            return Ok(TrackSource {
                stream_url: first.url,
                audio_quality: AudioQuality::LegacyMp4,
                referer: REFERER.to_string(),
            });
        }
    }

    Err(BilibiliError::NoAudioStream)
}

/// Pick the best URL from base_url and backup URLs.
fn pick_url(base_url: &Option<String>, backup_url: &Option<Vec<String>>) -> Option<String> {
    base_url
        .as_ref()
        .filter(|s| !s.is_empty())
        .cloned()
        .or_else(|| {
            backup_url
                .as_ref()
                .and_then(|urls| urls.first().cloned())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick_url_prefers_base() {
        let base = Some("https://primary.example.com/audio.m4s".to_string());
        let backup = Some(vec!["https://backup.example.com/audio.m4s".to_string()]);
        assert_eq!(
            pick_url(&base, &backup),
            Some("https://primary.example.com/audio.m4s".to_string())
        );
    }

    #[test]
    fn test_pick_url_falls_back() {
        let base = Some(String::new());
        let backup = Some(vec!["https://backup.example.com/audio.m4s".to_string()]);
        assert_eq!(
            pick_url(&base, &backup),
            Some("https://backup.example.com/audio.m4s".to_string())
        );
    }

    #[test]
    fn test_pick_url_none() {
        assert_eq!(pick_url(&None, &None), None);
    }

    #[test]
    fn test_dash_response_deserialization() {
        let json = r#"{
            "code": 0,
            "data": {
                "dash": {
                    "audio": [
                        {
                            "id": 30280,
                            "base_url": "https://audio.example.com/high.m4s",
                            "backup_url": ["https://backup.example.com/high.m4s"],
                            "bandwidth": 320000
                        },
                        {
                            "id": 30232,
                            "base_url": "https://audio.example.com/mid.m4s",
                            "backup_url": [],
                            "bandwidth": 132000
                        }
                    ],
                    "dolby": null,
                    "flac": null
                }
            }
        }"#;

        let resp: PlayUrlResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.code, 0);
        let dash = resp.data.unwrap().dash.unwrap();
        assert_eq!(dash.audio.len(), 2);
        // Best bitrate should be the 30280 one
        let best = dash.audio.into_iter().max_by_key(|a| a.bandwidth).unwrap();
        assert_eq!(best.id, 30280);
    }

    #[test]
    fn test_flac_priority() {
        let json = r#"{
            "code": 0,
            "data": {
                "dash": {
                    "audio": [
                        {
                            "id": 30280,
                            "base_url": "https://audio.example.com/high.m4s",
                            "bandwidth": 320000
                        }
                    ],
                    "dolby": null,
                    "flac": {
                        "audio": {
                            "id": 30251,
                            "base_url": "https://audio.example.com/flac.m4s",
                            "bandwidth": 800000
                        }
                    }
                }
            }
        }"#;

        let resp: PlayUrlResponse = serde_json::from_str(json).unwrap();
        let data = resp.data.unwrap();
        let dash = data.dash.unwrap();
        assert!(dash.flac.unwrap().audio.is_some());
    }
}
