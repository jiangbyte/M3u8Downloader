use crate::error::{AppError, AppResult};
use reqwest::{Client, Proxy, header::{HeaderMap, HeaderName, HeaderValue, RANGE}};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestOptions {
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub cookie: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub referer: Option<String>,
}

pub fn build_client(opts: &RequestOptions) -> AppResult<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(20))
        .pool_max_idle_per_host(32)
        .user_agent("M3U8Downloader/0.1");

    if let Some(proxy) = opts.proxy.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        builder = builder.proxy(Proxy::all(proxy).map_err(|e| AppError::msg(e.to_string()))?);
    }

    Ok(builder.build()?)
}

pub fn build_headers(opts: &RequestOptions) -> AppResult<HeaderMap> {
    let mut map = HeaderMap::new();
    for (k, v) in &opts.headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|e| AppError::msg(format!("Invalid header name {k}: {e}")))?;
        let value = HeaderValue::from_str(v)
            .map_err(|e| AppError::msg(format!("Invalid header value for {k}: {e}")))?;
        map.insert(name, value);
    }
    if let Some(cookie) = opts.cookie.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        map.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_str(cookie)
                .map_err(|e| AppError::msg(format!("Invalid cookie: {e}")))?,
        );
    }
    if let Some(referer) = opts.referer.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        map.insert(
            HeaderName::from_static("referer"),
            HeaderValue::from_str(referer)
                .map_err(|e| AppError::msg(format!("Invalid referer: {e}")))?,
        );
    }
    Ok(map)
}

pub async fn fetch_text(client: &Client, url: &str, headers: &HeaderMap) -> AppResult<String> {
    let resp = client.get(url).headers(headers.clone()).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!(
            "HTTP {} fetching {url}",
            resp.status()
        )));
    }
    Ok(resp.text().await?)
}

pub async fn fetch_bytes(
    client: &Client,
    url: &str,
    headers: &HeaderMap,
    byte_range: Option<(u64, Option<u64>)>,
) -> AppResult<Vec<u8>> {
    let mut req = client.get(url).headers(headers.clone());
    if let Some((length, start)) = byte_range {
        if let Some(offset) = start {
            let end = offset.saturating_add(length).saturating_sub(1);
            req = req.header(RANGE, format!("bytes={offset}-{end}"));
        } else {
            // length only without start is unusual for subsequent segments; treat as first length bytes
            let end = length.saturating_sub(1);
            req = req.header(RANGE, format!("bytes=0-{end}"));
        }
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!(
            "HTTP {} fetching {url}",
            resp.status()
        )));
    }
    Ok(resp.bytes().await?.to_vec())
}
