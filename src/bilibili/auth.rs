use std::sync::Mutex;

use serde::Deserialize;

use crate::error::BilibiliError;

/// Cached WBI keys for request signing.
pub struct WbiKeys {
    inner: Mutex<WbiKeysInner>,
}

#[derive(Clone)]
struct WbiKeysInner {
    img_key: String,
    sub_key: String,
    mixin_key: String,
}

impl Clone for WbiKeys {
    fn clone(&self) -> Self {
        let inner = self.inner.lock().unwrap().clone();
        Self {
            inner: Mutex::new(inner),
        }
    }
}

impl Default for WbiKeys {
    fn default() -> Self {
        Self::new()
    }
}

impl WbiKeys {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(WbiKeysInner {
                img_key: String::new(),
                sub_key: String::new(),
                mixin_key: String::new(),
            }),
        }
    }

    /// Get the current mixin key, if cached.
    pub fn mixin_key(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        if inner.mixin_key.is_empty() {
            None
        } else {
            Some(inner.mixin_key.clone())
        }
    }

    /// Refresh WBI keys from the nav API.
    pub fn refresh(&self, img_key: &str, sub_key: &str) {
        let mixin_key = crate::bilibili::wbi::get_mixin_key(img_key, sub_key);
        let mut inner = self.inner.lock().unwrap();
        inner.img_key = img_key.to_string();
        inner.sub_key = sub_key.to_string();
        inner.mixin_key = mixin_key;
    }
}

#[derive(Debug, Deserialize)]
struct NavResponse {
    #[allow(dead_code)]
    code: i64,
    data: Option<NavData>,
}

#[derive(Debug, Deserialize)]
struct NavData {
    wbi_img: WbiImg,
}

#[derive(Debug, Deserialize)]
struct WbiImg {
    img_url: String,
    sub_url: String,
}

/// Fetch WBI keys from Bilibili nav API.
///
/// The nav API returns WBI keys even without authentication.
/// When not logged in, `code` may be non-zero but `data.wbi_img` is still present.
/// We only need the wbi_img data, so we don't require code == 0.
pub async fn fetch_wbi_keys(
    client: &reqwest::Client,
    sessdata: Option<&str>,
) -> Result<(String, String), BilibiliError> {
    let mut request = client.get("https://api.bilibili.com/x/web-interface/nav");

    // Only send Cookie with real SESSDATA
    if let Some(sessdata) = sessdata {
        request = request.header("Cookie", format!("SESSDATA={sessdata}"));
    }

    let resp = request
        .send()
        .await
        .map_err(|e| BilibiliError::Wbi(format!("Failed to fetch nav API: {e}")))?;

    let nav: NavResponse = resp
        .json()
        .await
        .map_err(|e| BilibiliError::Wbi(format!("Failed to parse nav response: {e}")))?;

    // WBI keys are available even when code != 0 (e.g. not logged in)
    let data = nav.data.ok_or_else(|| {
        BilibiliError::Wbi("No data in nav response, cannot get WBI keys".into())
    })?;

    if data.wbi_img.img_url.is_empty() || data.wbi_img.sub_url.is_empty() {
        return Err(BilibiliError::Wbi("WBI image URLs are empty".into()));
    }

    // Extract filename stems from URLs as keys
    let img_key = extract_key_from_url(&data.wbi_img.img_url);
    let sub_key = extract_key_from_url(&data.wbi_img.sub_url);

    if img_key.is_empty() || sub_key.is_empty() {
        return Err(BilibiliError::Wbi("Failed to extract WBI keys from URLs".into()));
    }

    Ok((img_key, sub_key))
}

/// Extract the key (filename without extension) from a Bilibili CDN URL.
/// e.g. "https://i0.hdslb.com/bfs/wbi/abcd1234.png" -> "abcd1234"
fn extract_key_from_url(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("")
        .split('.')
        .next()
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_key_from_url() {
        let url = "https://i0.hdslb.com/bfs/wbi/7cd08e551f1e4e4a.json";
        assert_eq!(extract_key_from_url(url), "7cd08e551f1e4e4a");

        let url = "https://i0.hdslb.com/bfs/wbi/img.png";
        assert_eq!(extract_key_from_url(url), "img");

        let url = "nopath";
        assert_eq!(extract_key_from_url(url), "nopath");
    }

    #[test]
    fn test_wbi_keys_cache() {
        let keys = WbiKeys::new();
        assert!(keys.mixin_key().is_none());

        keys.refresh("test_img_key_1234567890", "test_sub_key_1234567890");
        let mk = keys.mixin_key().unwrap();
        assert_eq!(mk.len(), 32);
    }
}
