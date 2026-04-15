use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::bilibili::auth::WbiKeys;
use crate::error::BilibiliError;

const BILIBILI_API_BASE: &str = "https://api.bilibili.com";
const REFERER: &str = "https://www.bilibili.com";

/// Bilibili API client with WBI signing support.
pub struct BilibiliClient {
    http: Client,
    /// Real SESSDATA if user is logged in. None = not logged in.
    /// Only Some values are sent as Cookie; unauthenticated requests send no Cookie.
    sessdata: Option<String>,
    wbi_keys: WbiKeys,
}

impl BilibiliClient {
    pub fn new(sessdata: Option<String>) -> Self {
        let http = Client::builder()
            .cookie_store(false)
            .build()
            .expect("Failed to build HTTP client");

        // Treat empty string as None
        let sessdata = sessdata.filter(|s| !s.is_empty());

        Self {
            http,
            sessdata,
            wbi_keys: WbiKeys::new(),
        }
    }

    /// Ensure WBI keys are available, refresh if needed.
    pub async fn ensure_wbi_keys(&self) -> Result<String, BilibiliError> {
        if let Some(key) = self.wbi_keys.mixin_key() {
            return Ok(key);
        }

        let (img_key, sub_key) =
            crate::bilibili::auth::fetch_wbi_keys(&self.http, self.sessdata.as_deref()).await?;
        self.wbi_keys.refresh(&img_key, &sub_key);
        self.wbi_keys
            .mixin_key()
            .ok_or_else(|| BilibiliError::Wbi("WBI key refresh failed".into()))
    }

    /// Make a GET request to Bilibili API with optional WBI signing.
    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Vec<(String, String)>,
        need_wbi: bool,
    ) -> Result<T, BilibiliError> {
        let mut params = params;

        if need_wbi {
            let mixin_key = self.ensure_wbi_keys().await?;
            crate::bilibili::wbi::sign_wbi_params(&mut params, &mixin_key);
        }

        let url = format!("{BILIBILI_API_BASE}{path}");
        let mut request = self.http.get(&url).query(&params);

        // Only send Cookie when user has a real SESSDATA.
        if let Some(ref sessdata) = self.sessdata {
            request = request.header("Cookie", format!("SESSDATA={sessdata}"));
        }

        // Add Referer for anti-hotlink bypass
        request = request.header("Referer", REFERER);

        // Spoof User-Agent to bypass anti-bot checks on playurl API
        request = request.header("User-Agent", "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0");

        let resp = request
            .send()
            .await
            .map_err(|e| BilibiliError::Wbi(format!("Request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(BilibiliError::ApiResponse {
                code: status.as_u16() as i64,
                message: format!("HTTP {status}"),
            });
        }

        resp.json()
            .await
            .map_err(|e| BilibiliError::Parse(format!("Failed to parse response: {e}")))
    }

    /// Get the underlying HTTP client for direct requests (e.g., cover images).
    pub fn http_client(&self) -> &Client {
        &self.http
    }

    /// Get the SESSDATA (for checking login status).
    /// Returns None if not logged in.
    pub fn sessdata(&self) -> Option<&str> {
        self.sessdata.as_deref()
    }
}
