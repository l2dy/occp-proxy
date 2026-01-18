use crate::auth::token::{
    AuthState, DeviceCodeResponse, GitHubDomain, GithubToken, TokenPollResponse,
};
use crate::config::{
    DEFAULT_OPENCODE_VERSION, GITHUB_CLIENT_ID, OAUTH_POLLING_SAFETY_MARGIN_MS,
    RFC8628_SLOW_DOWN_INCREMENT,
};
use crate::error::AppError;
use reqwest::Client;
use std::time::Duration;

pub struct DeviceFlowAuth {
    client: Client,
    domain: GitHubDomain,
}

impl DeviceFlowAuth {
    pub fn new(domain: GitHubDomain) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            domain,
        }
    }

    fn user_agent(&self) -> String {
        format!("opencode/{}", DEFAULT_OPENCODE_VERSION)
    }

    /// Step 1: Request device code from GitHub
    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse, AppError> {
        let url = format!(
            "https://{}/login/device/code",
            self.domain.auth_domain()
        );

        let response = self
            .client
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("User-Agent", self.user_agent())
            .json(&serde_json::json!({
                "client_id": GITHUB_CLIENT_ID,
                "scope": "read:user"
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AppError::AuthError(
                "Failed to initiate device authorization".to_string(),
            ));
        }

        Ok(response.json().await?)
    }

    /// Step 2: Poll for access token (with slow_down handling per RFC 8628)
    pub async fn poll_for_token(
        &self,
        device_code: &str,
        mut interval: u64,
    ) -> Result<GithubToken, AppError> {
        let url = format!(
            "https://{}/login/oauth/access_token",
            self.domain.auth_domain()
        );

        loop {
            let response = self
                .client
                .post(&url)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .header("User-Agent", self.user_agent())
                .json(&serde_json::json!({
                    "client_id": GITHUB_CLIENT_ID,
                    "device_code": device_code,
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                return Err(AppError::AuthError("Token request failed".to_string()));
            }

            let data: TokenPollResponse = response.json().await?;

            // Success - got access token
            if let Some(access_token) = data.access_token {
                return Ok(GithubToken {
                    access_token,
                    token_type: data.token_type.unwrap_or_else(|| "bearer".to_string()),
                    scope: data.scope.unwrap_or_default(),
                });
            }

            // Handle error cases
            match data.error.as_deref() {
                Some("authorization_pending") => {
                    // User hasn't completed auth yet, continue polling
                    tokio::time::sleep(Duration::from_millis(
                        interval * 1000 + OAUTH_POLLING_SAFETY_MARGIN_MS,
                    ))
                    .await;
                }
                Some("slow_down") => {
                    // RFC 8628: add 5 seconds to polling interval
                    interval += RFC8628_SLOW_DOWN_INCREMENT;
                    // If server provides new interval, use it
                    if let Some(new_interval) = data.interval {
                        interval = new_interval;
                    }
                    tokio::time::sleep(Duration::from_millis(
                        interval * 1000 + OAUTH_POLLING_SAFETY_MARGIN_MS,
                    ))
                    .await;
                }
                Some("expired_token") => {
                    return Err(AppError::AuthError(
                        "Device code expired. Please try again.".to_string(),
                    ));
                }
                Some("access_denied") => {
                    return Err(AppError::AuthError("Access denied by user.".to_string()));
                }
                Some(error) => {
                    return Err(AppError::AuthError(format!(
                        "Authorization failed: {}",
                        data.error_description.unwrap_or_else(|| error.to_string())
                    )));
                }
                None => {
                    // Unknown state, continue polling
                    tokio::time::sleep(Duration::from_millis(
                        interval * 1000 + OAUTH_POLLING_SAFETY_MARGIN_MS,
                    ))
                    .await;
                }
            }
        }
    }

    /// Complete authentication flow
    pub async fn authenticate(&self) -> Result<AuthState, AppError> {
        let device_code = self.request_device_code().await?;

        // Display instructions to user
        println!("\n=== GitHub Copilot Authentication ===");
        println!("1. Open: {}", device_code.verification_uri);
        println!("2. Enter code: {}", device_code.user_code);
        println!("\nWaiting for authorization...\n");

        // Optionally open browser
        if let Err(e) = open::that(&device_code.verification_uri) {
            log::warn!("Could not open browser: {}", e);
        }

        let github_token = self
            .poll_for_token(&device_code.device_code, device_code.interval)
            .await?;

        Ok(AuthState {
            github_token,
            domain: self.domain.clone(),
            authenticated_at: chrono::Utc::now(),
        })
    }
}
