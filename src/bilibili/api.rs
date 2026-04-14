use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::bilibili::auth::WbiKeys;
use crate::error::BilibiliError;

const BILIBILI_API_BASE: &str = "https://api.bilibili.com";
const REFERER: &str = "https://www.bilibili.com";
/// Dummy SESSDATA value used as placeholder when user has no login.
/// We do NOT send this as a Cookie — it's just a sentinel to distinguish
/// "no SESSDATA configured" from "user provided SESSDATA".
const DUMMY_SESSDATA: &str = "dummyval";

/// Bilibili API client with WBI signing support.
pub struct BilibiliClient {
    http: Client,
    /// Real SESSDATA if user is logged in, or DUMMY_SESSDATA sentinel if not.
    /// Only real SESSDATA is sent as Cookie; dummy is never sent.
    sessdata: String,
    wbi_keys: WbiKeys,
}

impl BilibiliClient {
    pub fn new(sessdata: String) -> Self {
        let http = Client::builder()
            .cookie_store(true)
            .build()
            .expect("Failed to build HTTP client");

        let sessdata = if sessdata.is_empty() {
            DUMMY_SESSDATA.to_string()
        } else {
            sessdata
        };

        Self {
            http,
            sessdata,
            wbi_keys: WbiKeys::new(),
        }
    }

    /// Whether the user has provided a real SESSDATA (logged in).
    fn has_real_sessdata(&self) -> bool {
        self.sessdata != DUMMY_SESSDATA
    }

    /// Ensure WBI keys are available, refresh if needed.
    pub async fn ensure_wbi_keys(&self) -> Result<String, BilibiliError> {
        if let Some(key) = self.wbi_keys.mixin_key() {
            return Ok(key);
        }

        let (img_key, sub_key) =
            crate::bilibili::auth::fetch_wbi_keys(&self.http, &self.sessdata).await?;
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

        // Only send Cookie with real SESSDATA.
        // Dummy SESSDATA causes -101 on nav API and v_voucher on playurl API.
        if self.has_real_sessdata() {
            request = request.header("Cookie", format!("SESSDATA={}", self.sessdata));
        }

        // Add Referer for anti-hotlink bypass
        request = request.header("Referer", REFERER);

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
    /// Returns None if using dummy value (not logged in).
    pub fn sessdata(&self) -> Option<&str> {
        if self.sessdata == DUMMY_SESSDATA {
            None
        } else {
            Some(&self.sessdata)
        }
    }
}
