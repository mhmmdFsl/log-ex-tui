use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use tracing;

const METADATA_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPES: &str =
    "https://www.googleapis.com/auth/logging.read https://www.googleapis.com/auth/cloud-platform.read-only";
const ADC_ENV: &str = "GOOGLE_APPLICATION_CREDENTIALS";
const CLOUDSDK_CONFIG_ENV: &str = "CLOUDSDK_CONFIG";

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("No ADC found: {0}")]
    Discovery(String),
    #[error("Token request failed: {0}")]
    Request(String),
    #[error("Token response parse error: {0}")]
    Parse(String),
    #[error("Lock poisoned")]
    Lock,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[allow(dead_code)]
    token_type: String,
}

#[derive(Deserialize)]
struct AdcFile {
    #[serde(rename = "type")]
    type_: String,
    client_id: Option<String>,
    client_secret: Option<String>,
    refresh_token: Option<String>,
    private_key: Option<String>,
    client_email: Option<String>,
    token_uri: Option<String>,
}

enum TokenSource {
    MetadataServer,
    ServiceAccount {
        client_email: String,
        private_key: String,
        token_uri: String,
    },
    AuthorizedUser {
        client_id: String,
        client_secret: String,
        refresh_token: String,
        token_uri: String,
    },
}

impl std::fmt::Debug for TokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MetadataServer => write!(f, "MetadataServer"),
            Self::ServiceAccount { .. } => write!(f, "ServiceAccount(…)"),
            Self::AuthorizedUser { .. } => write!(f, "AuthorizedUser(…)"),
        }
    }
}

pub struct TokenCache {
    token: RwLock<String>,
    expiry: RwLock<Instant>,
    source: TokenSource,
    client: Client,
}

impl std::fmt::Debug for TokenCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenCache").finish()
    }
}

impl TokenCache {
    pub async fn new() -> Result<std::sync::Arc<Self>, AuthError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AuthError::Request(e.to_string()))?;

        let source = discover_source(&client).await?;
        let cache = std::sync::Arc::new(Self {
            token: RwLock::new(String::new()),
            expiry: RwLock::new(Instant::now()),
            source,
            client,
        });

        cache.refresh().await?;

        let cache_clone = cache.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
                if let Err(e) = cache_clone.refresh().await {
                    tracing::warn!("token refresh error: {e}");
                }
            }
        });

        Ok(cache)
    }

    async fn refresh(&self) -> Result<(), AuthError> {
        let resp = match &self.source {
            TokenSource::MetadataServer => fetch_metadata_token(&self.client).await?,
            TokenSource::ServiceAccount {
                client_email,
                private_key,
                token_uri,
            } => fetch_sa_token(&self.client, client_email, private_key, token_uri).await?,
            TokenSource::AuthorizedUser {
                client_id,
                client_secret,
                refresh_token,
                token_uri,
            } => {
                fetch_user_token(
                    &self.client,
                    client_id,
                    client_secret,
                    refresh_token,
                    token_uri,
                )
                .await?
            }
        };

        let mut token = self.token.write().map_err(|_| AuthError::Lock)?;
        let mut expiry = self.expiry.write().map_err(|_| AuthError::Lock)?;
        *token = resp.access_token;
        *expiry = Instant::now() + Duration::from_secs(resp.expires_in.saturating_sub(120));
        tracing::debug!("token refreshed, expires in {}s", resp.expires_in);
        Ok(())
    }

    pub fn get_sync(&self) -> Result<String, AuthError> {
        let token = self.token.read().map_err(|_| AuthError::Lock)?;
        if token.is_empty() {
            return Err(AuthError::Request("no token yet".into()));
        }
        Ok(token.clone())
    }
}

async fn discover_source(client: &Client) -> Result<TokenSource, AuthError> {
    // 1. Try metadata server (GCE/GKE)
    if let Ok(source) = check_metadata_server(client).await {
        return Ok(source);
    }

    // 2. Try ADC file from env var or well-known path
    let adc_path = std::env::var(ADC_ENV)
        .map(PathBuf::from)
        .ok()
        .or_else(first_existing_adc_path)
        .ok_or_else(|| {
            AuthError::Discovery(format!(
                "ADC file not found in {}",
                adc_search_paths()
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

    let data = std::fs::read_to_string(&adc_path)
        .map_err(|e| AuthError::Discovery(format!("read {adc_path:?}: {e}")))?;
    let adc: AdcFile = serde_json::from_str(&data).map_err(|e| AuthError::Parse(e.to_string()))?;

    match adc.type_.as_str() {
        "service_account" => {
            let client_email = adc
                .client_email
                .ok_or_else(|| AuthError::Parse("service_account missing client_email".into()))?;
            let private_key = adc
                .private_key
                .ok_or_else(|| AuthError::Parse("service_account missing private_key".into()))?;
            let token_uri = adc.token_uri.unwrap_or_else(|| TOKEN_URL.to_string());
            Ok(TokenSource::ServiceAccount {
                client_email,
                private_key,
                token_uri,
            })
        }
        "authorized_user" => {
            let client_id = adc
                .client_id
                .ok_or_else(|| AuthError::Parse("authorized_user missing client_id".into()))?;
            let client_secret = adc
                .client_secret
                .ok_or_else(|| AuthError::Parse("authorized_user missing client_secret".into()))?;
            let refresh_token = adc
                .refresh_token
                .ok_or_else(|| AuthError::Parse("authorized_user missing refresh_token".into()))?;
            let token_uri = adc.token_uri.unwrap_or_else(|| TOKEN_URL.to_string());
            Ok(TokenSource::AuthorizedUser {
                client_id,
                client_secret,
                refresh_token,
                token_uri,
            })
        }
        other => Err(AuthError::Discovery(format!("unknown ADC type: {other}"))),
    }
}

fn first_existing_adc_path() -> Option<PathBuf> {
    adc_search_paths().into_iter().find(|path| path.exists())
}

fn adc_search_paths() -> Vec<PathBuf> {
    adc_search_paths_with(
        std::env::var(CLOUDSDK_CONFIG_ENV).ok().map(PathBuf::from),
        dirs::home_dir(),
        dirs::config_dir(),
    )
}

fn adc_search_paths_with(
    cloudsdk_config: Option<PathBuf>,
    home_dir: Option<PathBuf>,
    config_dir: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(path) = cloudsdk_config {
        paths.push(path.join("application_default_credentials.json"));
    }

    if let Some(home) = home_dir {
        paths.push(home.join(".config/gcloud/application_default_credentials.json"));
        paths.push(
            home.join("Library/Application Support/gcloud/application_default_credentials.json"),
        );
    }

    if let Some(config_dir) = config_dir {
        paths.push(config_dir.join("gcloud/application_default_credentials.json"));
    }

    paths.dedup();
    paths
}

async fn check_metadata_server(client: &Client) -> Result<TokenSource, AuthError> {
    let resp = client
        .get(METADATA_URL)
        .query(&[("scopes", SCOPES)])
        .header("Metadata-Flavor", "Google")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|_| AuthError::Discovery("metadata server unreachable".into()))?;

    if resp.status().is_success() {
        Ok(TokenSource::MetadataServer)
    } else {
        Err(AuthError::Discovery(
            "metadata server responded non-200".into(),
        ))
    }
}

async fn fetch_metadata_token(client: &Client) -> Result<TokenResponse, AuthError> {
    let resp = client
        .get(METADATA_URL)
        .query(&[("scopes", SCOPES)])
        .header("Metadata-Flavor", "Google")
        .send()
        .await
        .map_err(|e| AuthError::Request(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AuthError::Request(format!(
            "metadata server returned {}",
            resp.status()
        )));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| AuthError::Parse(e.to_string()))
}

async fn fetch_sa_token(
    client: &Client,
    client_email: &str,
    private_key_pem: &str,
    token_uri: &str,
) -> Result<TokenResponse, AuthError> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let now = chrono::Utc::now().timestamp() as u64;
    let claims = serde_json::json!({
        "iss": client_email,
        "scope": SCOPES,
        "aud": token_uri,
        "iat": now,
        "exp": now + 3600,
    });

    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .map_err(|e| AuthError::Request(format!("key parse: {e}")))?;

    let header = Header::new(Algorithm::RS256);
    let jwt =
        encode(&header, &claims, &key).map_err(|e| AuthError::Request(format!("jwt: {e}")))?;

    let params = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
        ("assertion", &jwt),
    ];

    let resp = client
        .post(token_uri)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::Request(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AuthError::Request(format!(
            "token endpoint {status}: {body}"
        )));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| AuthError::Parse(e.to_string()))
}

async fn fetch_user_token(
    client: &Client,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
    token_uri: &str,
) -> Result<TokenResponse, AuthError> {
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
    ];

    let resp = client
        .post(token_uri)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::Request(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AuthError::Request(format!(
            "refresh token {status}: {body}"
        )));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| AuthError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::adc_search_paths_with;
    use std::path::PathBuf;

    #[test]
    fn adc_search_paths_include_macos_and_linux_conventions() {
        let paths = adc_search_paths_with(
            None,
            Some(PathBuf::from("/Users/tester")),
            Some(PathBuf::from("/Users/tester/.config")),
        );
        let rendered: Vec<String> = paths
            .iter()
            .map(|path| path.display().to_string())
            .collect();

        assert!(rendered.iter().any(|path| path
            .contains("Library/Application Support/gcloud/application_default_credentials.json")));
        assert!(rendered
            .iter()
            .any(|path| path.contains(".config/gcloud/application_default_credentials.json")));
    }

    #[test]
    fn adc_search_paths_honor_cloudsdk_config() {
        let expected =
            PathBuf::from("/tmp/fake-gcloud").join("application_default_credentials.json");
        let paths = adc_search_paths_with(
            Some(PathBuf::from("/tmp/fake-gcloud")),
            Some(PathBuf::from("/Users/tester")),
            Some(PathBuf::from("/Users/tester/.config")),
        );

        assert_eq!(paths.first(), Some(&expected));
    }
}
