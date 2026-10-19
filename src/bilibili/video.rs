use serde::Deserialize;

use crate::bilibili::api::BilibiliClient;
use crate::error::BilibiliError;
use crate::queue::track::Track;

#[derive(Debug, Deserialize)]
struct VideoInfoResponse {
    code: i64,
    data: Option<VideoInfoData>,
}

#[derive(Debug, Deserialize)]
struct VideoInfoData {
    bvid: String,
    #[serde(default)]
    #[allow(dead_code)]
    aid: u64,
    title: String,
    owner: VideoOwner,
    #[serde(default)]
    #[allow(dead_code)]
    duration: u64,
    pic: Option<String>,
    #[serde(default)]
    pages: Vec<VideoPage>,
}

#[derive(Debug, Deserialize)]
struct VideoOwner {
    name: String,
}

/// A single part (分P) of a video.
///
/// Returned by both `/x/web-interface/wbi/view` (`data.pages`) and
/// `/x/player/pagelist`.  Note that neither search endpoint exposes the part
/// count, so this is the only way to discover that a video is multi-part.
#[derive(Debug, Clone, Deserialize)]
pub struct VideoPage {
    pub cid: u64,
    pub page: u32,
    #[serde(default)]
    pub part: String,
    /// Per-part duration in seconds (the view response uses the same field name
    /// for the total duration, but here it is always the part's own duration).
    #[serde(rename = "duration", default)]
    pub duration_secs: u64,
}

impl VideoPage {
    pub fn duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.duration_secs)
    }

    /// Build a page-level track from this part, inheriting video-level metadata
    /// (author, cover) from `base`.
    pub fn to_track(&self, base: &Track) -> Track {
        Track {
            bvid: base.bvid.clone(),
            cid: self.cid,
            title: base.title.clone(),
            author: base.author.clone(),
            duration: self.duration(),
            cover_url: base.cover_url.clone(),
            source: None,
            part: Some(self.part.clone()),
            page: Some(self.page),
        }
    }
}

/// Get video information by BV number.
/// Returns a Track with cid resolved (first page, or specified page).
pub async fn get_video_info(client: &BilibiliClient, bvid: &str) -> Result<Track, BilibiliError> {
    let params = vec![("bvid".to_string(), bvid.to_string())];

    let resp: VideoInfoResponse = client
        .get("/x/web-interface/wbi/view", params, true)
        .await?;

    if resp.code != 0 {
        return Err(BilibiliError::ApiResponse {
            code: resp.code,
            message: "Failed to get video info".into(),
        });
    }

    let data = resp
        .data
        .ok_or_else(|| BilibiliError::Parse("No data in video info response".into()))?;

    // Use first page's cid
    let cid = data
        .pages
        .first()
        .map(|p| p.cid)
        .ok_or_else(|| BilibiliError::Parse("No pages in video info".into()))?;

    Ok(Track {
        bvid: data.bvid,
        cid,
        title: data.title,
        author: data.owner.name,
        duration: std::time::Duration::from_secs(data.duration),
        cover_url: data.pic,
        part: None,
        page: None,
        source: None,
    })
}

/// Get the part (分P) list of a video — cid, title and duration for each part.
///
/// Uses `/x/player/pagelist`, which needs no WBI signing and returns every part
/// in a single request.
pub async fn get_video_pages(
    client: &BilibiliClient,
    bvid: &str,
) -> Result<Vec<VideoPage>, BilibiliError> {
    let params = vec![("bvid".to_string(), bvid.to_string())];

    #[derive(Debug, Deserialize)]
    struct PagelistResponse {
        code: i64,
        data: Option<Vec<VideoPage>>,
    }

    let resp: PagelistResponse = client.get("/x/player/pagelist", params, false).await?;

    if resp.code != 0 {
        return Err(BilibiliError::ApiResponse {
            code: resp.code,
            message: "Failed to get pagelist".into(),
        });
    }

    let mut pages = resp.data.unwrap_or_default();
    if pages.is_empty() || pages.iter().any(|p| p.cid == 0 || p.page == 0) {
        return Err(BilibiliError::Parse("Video has no valid parts".into()));
    }
    pages.sort_by_key(|p| p.page);
    pages.dedup_by_key(|p| p.page);
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_info_deserialization() {
        let json = r#"{
            "code": 0,
            "data": {
                "bvid": "BV1xx411c7mD",
                "aid": 123456,
                "title": "Test Video",
                "owner": { "name": "TestUP" },
                "duration": 225,
                "pic": "https://example.com/cover.jpg",
                "pages": [
                    { "cid": 789012, "page": 1, "part": "P1", "duration": 225 }
                ]
            }
        }"#;

        let resp: VideoInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.code, 0);
        let data = resp.data.unwrap();
        assert_eq!(data.bvid, "BV1xx411c7mD");
        assert_eq!(data.title, "Test Video");
        assert_eq!(data.owner.name, "TestUP");
        assert_eq!(data.duration, 225);
        assert_eq!(data.pages[0].cid, 789012);
    }

    #[test]
    fn test_pagelist_deserialization() {
        // Shape captured from a real /x/player/pagelist?bvid=... response.
        let json = r#"[
            {
                "cid": 39252201332,
                "page": 1,
                "from": "vupload",
                "part": "001.周杰伦-晴天",
                "duration": 270,
                "vid": "",
                "weblink": "",
                "dimension": { "width": 1920, "height": 1080, "rotate": 0 },
                "first_frame": "http://i2.hdslb.com/bfs/storyff/first.jpg",
                "ctime": 1781908951
            },
            {
                "cid": 39252201462,
                "page": 2,
                "from": "vupload",
                "part": "002.周杰伦-夜曲",
                "duration": 227,
                "dimension": { "width": 1920, "height": 1080, "rotate": 0 }
            }
        ]"#;

        let pages: Vec<VideoPage> = serde_json::from_str(json).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].cid, 39252201332);
        assert_eq!(pages[0].page, 1);
        assert_eq!(pages[0].part, "001.周杰伦-晴天");
        assert_eq!(pages[0].duration(), std::time::Duration::from_secs(270));
        assert_eq!(pages[1].page, 2);
        assert_eq!(pages[1].duration(), std::time::Duration::from_secs(227));
    }

    #[test]
    fn test_video_page_to_track() {
        let base = Track {
            bvid: "BV1FPjy6TEiE".to_string(),
            cid: 0,
            title: "50首精选合集".to_string(),
            author: "UP".to_string(),
            duration: std::time::Duration::from_secs(13348), // whole-video total
            cover_url: Some("https://img/cover.jpg".to_string()),
            source: None,
            part: None,
            page: None,
        };
        let page = VideoPage {
            cid: 39252201462,
            page: 2,
            part: "002.周杰伦-夜曲".to_string(),
            duration_secs: 227,
        };

        let track = page.to_track(&base);
        assert_eq!(track.bvid, "BV1FPjy6TEiE");
        assert_eq!(track.cid, 39252201462, "cid must be the part's own cid");
        assert_eq!(track.title, "50首精选合集");
        assert_eq!(track.part, Some("002.周杰伦-夜曲".to_string()));
        assert_eq!(track.page, Some(2));
        assert_eq!(track.duration, std::time::Duration::from_secs(227));
        assert!(
            track.source.is_none(),
            "source must be re-resolved per part"
        );
    }
}
