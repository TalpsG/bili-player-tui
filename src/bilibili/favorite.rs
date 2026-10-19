//! Favorites API, based on community reverse-engineered documentation (no official guarantee).
//! https://github.com/bilibili-plugins/bilibili-api-collect/blob/master/docs/fav/list.md
//! Results are video-level metadata; resolving a playable part is a separate step.

use std::collections::HashSet;

use serde::Deserialize;

use crate::bilibili::api::BilibiliClient;
use crate::error::BilibiliError;

#[derive(Debug, Clone, Deserialize)]
pub struct FavoriteFolderInfo {
    pub id: u64,
    pub title: String,
    pub media_count: u64,
}

#[derive(Debug, Clone)]
pub struct FavoriteVideo {
    pub bvid: String,
    pub title: String,
    pub author: String,
    pub duration_secs: u64,
    pub cover_url: Option<String>,
    pub page_count: u32,
}

#[derive(Debug, Clone)]
pub struct SkippedFavorite {
    pub id: u64,
    pub title: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct FavoriteFolderSnapshot {
    pub folder: FavoriteFolderInfo,
    pub videos: Vec<FavoriteVideo>,
    pub skipped: Vec<SkippedFavorite>,
}

fn invalid(message: &str) -> BilibiliError {
    BilibiliError::Parse(message.into())
}

fn positive_id(value: &str) -> Result<u64, BilibiliError> {
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(invalid("Enter a complete favorite folder ID or URL"));
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| invalid("Favorite folder ID is zero or out of range"))
}

/// Parse explicit folder links, never a space UID or a collection season ID.
pub fn parse_favorite_folder_id(input: &str) -> Result<u64, BilibiliError> {
    let input = input.trim();
    if input.bytes().all(|c| c.is_ascii_digit()) {
        return positive_id(input);
    }
    if let Some(id) = input.strip_prefix("ml") {
        return positive_id(id);
    }
    let url = url::Url::parse(input).map_err(|_| invalid("Unrecognized favorite folder URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(
            url.host_str(),
            Some("www.bilibili.com" | "bilibili.com" | "space.bilibili.com")
        )
    {
        return Err(invalid("Use a complete Bilibili favorite folder URL"));
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    if pairs
        .iter()
        .any(|(key, _)| matches!(key.as_ref(), "sid" | "season_id"))
    {
        return Err(invalid("Collection URLs are not favorite folder URLs"));
    }
    let mut candidates = Vec::new();
    let path = url.path().trim_end_matches('/');
    for prefix in ["/medialist/detail/ml", "/list/ml"] {
        if let Some(id) = path.strip_prefix(prefix) {
            candidates.push(positive_id(id)?);
        }
    }
    for (key, value) in &pairs {
        if key == "media_id"
            || (key == "fid"
                && url.host_str() == Some("space.bilibili.com")
                && (path == "/favlist" || path.ends_with("/favlist")))
        {
            candidates.push(positive_id(value)?);
        }
    }
    if candidates.len() != 1 {
        return Err(invalid(
            "URL must contain one unambiguous favorite folder ID",
        ));
    }
    Ok(candidates[0])
}

#[derive(Deserialize)]
struct Response {
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<Page>,
}
#[derive(Deserialize)]
struct Page {
    info: FavoriteFolderInfo,
    medias: Option<Vec<Media>>,
    has_more: bool,
}
#[derive(Deserialize)]
struct Media {
    id: u64,
    #[serde(rename = "type")]
    kind: u32,
    #[serde(default)]
    title: String,
    #[serde(default)]
    attr: u64,
    #[serde(default)]
    bvid: String,
    #[serde(default)]
    bv_id: String,
    #[serde(default)]
    duration: u64,
    #[serde(default)]
    cover: String,
    #[serde(default)]
    page: u32,
    upper: Option<Upper>,
}
#[derive(Deserialize)]
struct Upper {
    name: String,
}

#[derive(Default)]
struct Collector {
    folder: Option<FavoriteFolderInfo>,
    videos: Vec<FavoriteVideo>,
    skipped: Vec<SkippedFavorite>,
    seen: HashSet<(u32, u64)>,
}
impl Collector {
    fn accept(&mut self, response: Response, folder_id: u64) -> Result<bool, BilibiliError> {
        if response.code != 0 {
            return Err(BilibiliError::ApiResponse {
                code: response.code,
                message: response.message,
            });
        }
        let page = response
            .data
            .ok_or_else(|| invalid("Favorite folder data is missing or inaccessible"))?;
        if page.info.id != folder_id {
            return Err(invalid("Favorite folder response ID does not match"));
        }
        if let Some(folder) = &self.folder {
            if folder.media_count != page.info.media_count {
                return Err(invalid("Favorite folder changed during import; try again"));
            }
        }
        let medias = page.medias.unwrap_or_default();
        if medias.is_empty()
            && (page.has_more || (self.seen.is_empty() && page.info.media_count > 0))
        {
            return Err(invalid(
                "Favorite folder list is incomplete or inaccessible",
            ));
        }
        for media in medias {
            if !self.seen.insert((media.kind, media.id)) {
                return Err(invalid(
                    "Duplicate items across favorite folder pages; try again",
                ));
            }
            let bvid = if media.bvid.is_empty() {
                media.bv_id
            } else {
                media.bvid
            };
            let reason = if media.kind != 2 {
                Some("Unsupported favorite item type")
            } else if media.attr != 0 {
                Some("Video is unavailable")
            } else if bvid.len() != 12
                || !bvid.starts_with("BV")
                || !bvid.bytes().all(|c| c.is_ascii_alphanumeric())
            {
                Some("Video BV ID is missing or invalid")
            } else {
                None
            };
            if let Some(reason) = reason {
                self.skipped.push(SkippedFavorite {
                    id: media.id,
                    title: media.title,
                    reason: reason.into(),
                });
            } else {
                self.videos.push(FavoriteVideo {
                    bvid,
                    title: media.title,
                    author: media.upper.map(|u| u.name).unwrap_or_default(),
                    duration_secs: media.duration,
                    cover_url: (!media.cover.is_empty()).then_some(media.cover),
                    page_count: media.page,
                });
            }
        }
        self.folder = Some(page.info);
        Ok(page.has_more)
    }
    fn finish(self) -> Result<FavoriteFolderSnapshot, BilibiliError> {
        if let Some(folder) = &self.folder {
            if self.seen.len() as u64 != folder.media_count {
                return Err(invalid(
                    "Favorite item count does not match the total; try again",
                ));
            }
        }
        Ok(FavoriteFolderSnapshot {
            folder: self
                .folder
                .ok_or_else(|| invalid("Favorite folder response is empty"))?,
            videos: self.videos,
            skipped: self.skipped,
        })
    }
}

/// Fetch every page in server favorite-time order, returning no partial snapshot on failure.
pub async fn fetch_favorites_snapshot(
    client: &BilibiliClient,
    folder_id: u64,
) -> Result<FavoriteFolderSnapshot, BilibiliError> {
    if folder_id == 0 {
        return Err(invalid("Favorite folder ID cannot be zero"));
    }
    let mut collector = Collector::default();
    for pn in 1..=100_000u32 {
        let response = client
            .get(
                "/x/v3/fav/resource/list",
                vec![
                    ("media_id".into(), folder_id.to_string()),
                    ("pn".into(), pn.to_string()),
                    ("ps".into(), "20".into()),
                    ("order".into(), "mtime".into()),
                    ("type".into(), "0".into()),
                    ("platform".into(), "web".into()),
                ],
                false,
            )
            .await?;
        if !collector.accept(response, folder_id)? {
            return collector.finish();
        }
    }
    Err(invalid("Favorite folder exceeds the page limit"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn response(items: serde_json::Value, more: bool, count: u64) -> Response {
        serde_json::from_value(json!({"code":0,"data":{"info":{"id":123,"title":"夹","media_count":count},"medias":items,"has_more":more}})).unwrap()
    }
    fn video(id: u64) -> serde_json::Value {
        json!({"id":id,"type":2,"title":format!("视频{id}"),"bvid":"BV1xx411c7mD","page":3})
    }
    #[test]
    fn parses_explicit_folder_ids_only() {
        for input in [
            "123",
            "ml123",
            "https://www.bilibili.com/medialist/detail/ml123",
            "https://www.bilibili.com/list/ml123",
            "https://space.bilibili.com/42/favlist?fid=123",
            "https://www.bilibili.com/video/foo?media_id=123",
        ] {
            assert_eq!(parse_favorite_folder_id(input).unwrap(), 123, "{input}");
        }
        for input in [
            "",
            "0",
            "-1",
            "https://space.bilibili.com/123",
            "https://space.bilibili.com/42/lists?sid=123",
            "https://example.com/?media_id=123",
            "https://space.bilibili.com/42/favlist?fid=123&fid=456",
            "https://www.bilibili.com/list/ml123?media_id=456",
        ] {
            assert!(parse_favorite_folder_id(input).is_err(), "{input}");
        }
    }
    #[test]
    fn full_snapshot_preserves_order_and_reports_skipped() {
        let mut c = Collector::default();
        assert!(
            c.accept(
                response(
                    json!([video(5),{"id":6,"type":2,"attr":9}, {"id":7,"type":12}]),
                    true,
                    4
                ),
                123
            )
            .unwrap()
        );
        assert!(
            !c.accept(response(json!([video(8)]), false, 4), 123)
                .unwrap()
        );
        let snapshot = c.finish().unwrap();
        assert_eq!(
            snapshot
                .videos
                .iter()
                .map(|v| v.title.as_str())
                .collect::<Vec<_>>(),
            vec!["视频5", "视频8"]
        );
        assert_eq!(
            snapshot.skipped.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![6, 7]
        );
        assert_eq!(snapshot.videos[0].page_count, 3);
    }
    #[test]
    fn empty_folder_is_valid_but_missing_content_is_not() {
        let mut c = Collector::default();
        assert!(
            !c.accept(response(serde_json::Value::Null, false, 0), 123)
                .unwrap()
        );
        assert!(c.finish().unwrap().videos.is_empty());
        assert!(
            Collector::default()
                .accept(response(json!([]), false, 1), 123)
                .is_err()
        );
        assert!(
            Collector::default()
                .accept(response(json!([]), true, 0), 123)
                .is_err()
        );
    }
    #[test]
    fn incomplete_snapshot_and_invalid_video_are_reported() {
        let mut c = Collector::default();
        c.accept(response(json!([video(1)]), false, 2), 123)
            .unwrap();
        assert!(c.finish().is_err());
        let mut c = Collector::default();
        c.accept(
            response(json!([{"id":2,"type":2,"bvid":""}]), false, 1),
            123,
        )
        .unwrap();
        let snapshot = c.finish().unwrap();
        assert!(snapshot.videos.is_empty());
        assert_eq!(snapshot.skipped.len(), 1);
    }

    #[test]
    fn repeated_page_and_permissions_fail() {
        let mut c = Collector::default();
        c.accept(response(json!([video(1)]), true, 2), 123).unwrap();
        assert!(
            c.accept(response(json!([video(1)]), false, 2), 123)
                .is_err()
        );
        let denied: Response =
            serde_json::from_value(json!({"code":-403,"message":"权限不足","data":null})).unwrap();
        assert!(matches!(
            Collector::default().accept(denied, 123),
            Err(BilibiliError::ApiResponse { code: -403, .. })
        ));
    }
}
