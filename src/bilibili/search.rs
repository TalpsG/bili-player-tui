use serde::Deserialize;

use crate::bilibili::api::BilibiliClient;
use crate::error::BilibiliError;
use crate::queue::track::Track;

#[derive(Debug, Deserialize)]
struct SearchResponse {
    code: i64,
    data: Option<SearchData>,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    result: Option<Vec<SearchItem>>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    bvid: String,
    title: String,
    author: String,
    duration: String, // "3:45" format
    pic: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    page: u32,
}

/// Search for videos by keyword.
pub async fn search_videos(
    client: &BilibiliClient,
    keyword: &str,
    page: u32,
) -> Result<Vec<Track>, BilibiliError> {
    let params = vec![
        ("search_type".to_string(), "video".to_string()),
        ("keyword".to_string(), keyword.to_string()),
        ("page".to_string(), page.to_string()),
    ];

    let resp: SearchResponse = client
        .get("/x/web-interface/search/type", params, true)
        .await?;

    if resp.code != 0 {
        return Err(BilibiliError::ApiResponse {
            code: resp.code,
            message: "Search failed".into(),
        });
    }

    let data = resp.data.ok_or_else(|| {
        BilibiliError::Parse("No data in search response".into())
    })?;

    let results = data.result.unwrap_or_default();
    let tracks = results
        .into_iter()
        .filter(|item| !item.bvid.is_empty())
        .map(|item| Track {
            bvid: item.bvid,
            cid: 0, // Will be resolved when playing
            title: strip_html_tags(&item.title),
            author: item.author,
            duration: parse_duration(&item.duration),
            cover_url: item.pic,
            source: None,
        })
        .collect();

    Ok(tracks)
}

/// Strip HTML tags from Bilibili search results (titles contain <em> highlights).
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result
}

/// Parse duration string like "3:45" or "1:23:45" into Duration.
fn parse_duration(s: &str) -> std::time::Duration {
    let parts: Vec<u64> = s.split(':').filter_map(|p| p.parse().ok()).collect();
    match parts.len() {
        2 => std::time::Duration::from_secs(parts[0] * 60 + parts[1]),
        3 => std::time::Duration::from_secs(parts[0] * 3600 + parts[1] * 60 + parts[2]),
        _ => std::time::Duration::ZERO,
    }
}

/// Extract BV number from a string (BV number or full URL).
pub fn extract_bvid(input: &str) -> Option<String> {
    // Direct BV number
    if input.starts_with("BV") && input.len() >= 12 {
        return Some(input.to_string());
    }

    // Extract from URL
    let input = input.trim_end_matches('/');
    if let Some(idx) = input.rfind('/') {
        let segment = &input[idx + 1..];
        if segment.starts_with("BV") && segment.len() >= 12 {
            // Remove query params
            let bvid = segment.split('?').next().unwrap_or(segment);
            return Some(bvid.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("3:45"), Duration::from_secs(225));
        assert_eq!(parse_duration("1:23:45"), Duration::from_secs(5025));
        assert_eq!(parse_duration("0:30"), Duration::from_secs(30));
        assert_eq!(parse_duration("invalid"), Duration::ZERO);
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("hello <em>world</em>"), "hello world");
        assert_eq!(strip_html_tags("no tags"), "no tags");
        assert_eq!(strip_html_tags("<em>all</em> <em>tags</em>"), "all tags");
    }

    #[test]
    fn test_extract_bvid() {
        // Direct BV number
        assert_eq!(
            extract_bvid("BV1xx411c7mD"),
            Some("BV1xx411c7mD".to_string())
        );

        // Full URL
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1xx411c7mD"),
            Some("BV1xx411c7mD".to_string())
        );

        // URL with query params
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1xx411c7mD?p=1"),
            Some("BV1xx411c7mD".to_string())
        );

        // URL with trailing slash
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1xx411c7mD/"),
            Some("BV1xx411c7mD".to_string())
        );

        // Invalid input
        assert_eq!(extract_bvid("not_a_bvid"), None);
    }
}
