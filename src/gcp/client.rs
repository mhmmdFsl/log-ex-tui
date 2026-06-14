use std::sync::Arc;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use thiserror::Error;

use crate::auth::TokenCache;

#[derive(Error, Debug)]
pub enum GcpError {
    #[error("Auth: {0}")]
    Auth(#[from] crate::auth::AuthError),
    #[error("HTTP: {0}")]
    Http(String),
    #[error("API {0}: {1}")]
    Api(u16, String),
    #[error("Parse: {0}")]
    Parse(String),
}

#[derive(Debug, Clone)]
pub struct Client {
    inner: reqwest::Client,
    token_cache: Arc<TokenCache>,
}

impl Client {
    pub fn new(token_cache: Arc<TokenCache>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));

        Self {
            inner: reqwest::Client::builder()
                .default_headers(headers)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client build"),
            token_cache,
        }
    }

    fn auth_header(&self) -> Result<HeaderValue, GcpError> {
        let token = self.token_cache.get_sync()?;
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|e| GcpError::Http(e.to_string()))
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, GcpError> {
        let auth = self.auth_header()?;
        let resp = self
            .inner
            .get(url)
            .header(AUTHORIZATION, auth)
            .send()
            .await
            .map_err(|e| GcpError::Http(format!("GET {url}: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GcpError::Api(status.as_u16(), body));
        }

        resp.json()
            .await
            .map_err(|e| GcpError::Parse(e.to_string()))
    }

    pub async fn post_json<T: serde::de::DeserializeOwned, B: serde::Serialize + ?Sized>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T, GcpError> {
        let auth = self.auth_header()?;
        let resp = self
            .inner
            .post(url)
            .header(AUTHORIZATION, auth)
            .json(body)
            .send()
            .await
            .map_err(|e| GcpError::Http(format!("POST {url}: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GcpError::Api(status.as_u16(), body));
        }

        resp.json()
            .await
            .map_err(|e| GcpError::Parse(e.to_string()))
    }
}

impl GcpError {
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, GcpError::Api(429, _))
    }

    pub fn display_message(&self) -> String {
        match self {
            GcpError::Api(_, body) => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                    if let Some(msg) = json
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                    {
                        return msg.to_string();
                    }
                }
                format!("API error: {body}")
            }
            other => other.to_string(),
        }
    }
}
