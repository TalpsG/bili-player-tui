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

#[derive(Debug, Deserialize)]
struct VideoPage {
    cid: u64,
    #[serde(default)]
    #[allow(dead_code)]
    part: String,
    #[serde(default)]
    #[allow(dead_code)]
    duration: u64,
}

/// Get video information by BV number.
/// Returns a Track with cid resolved (first page, or specified page).
pub async fn get_video_info(
    client: &BilibiliClient,
    bvid: &str,
) -> Result<Track, BilibiliError> {
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

    let data = resp.data.ok_or_else(|| {
        BilibiliError::Parse("No data in video info response".into())
    })?;

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
        source: None,
    })
}

/// Get video pagelist (cid for each page/part).
pub async fn get_video_pages(
    client: &BilibiliClient,
    bvid: &str,
) -> Result<Vec<(u64, String)>, BilibiliError> {
    let params = vec![("bvid".to_string(), bvid.to_string())];

    #[derive(Debug, Deserialize)]
    struct PagelistResponse {
        code: i64,
        data: Option<Vec<PageItem>>,
    }

    #[derive(Debug, Deserialize)]
    struct PageItem {
        cid: u64,
        #[allow(dead_code)]
    part: String,
    }

    let resp: PagelistResponse = client
        .get("/x/player/pagelist", params, false)
        .await?;

    if resp.code != 0 {
        return Err(BilibiliError::ApiResponse {
            code: resp.code,
            message: "Failed to get pagelist".into(),
        });
    }

    let data = resp.data.unwrap_or_default();
    Ok(data.into_iter().map(|p| (p.cid, p.part)).collect())
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
                    { "cid": 789012, "part": "P1", "duration": 225 }
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
}
