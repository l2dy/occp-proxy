use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// GitHub domain configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GitHubDomain {
    /// Standard github.com
    #[serde(rename = "public")]
    Public,
    /// GitHub Enterprise with custom domain
    #[serde(rename = "enterprise")]
    Enterprise { domain: String },
}

impl GitHubDomain {
    /// Returns the OAuth domain for authentication
    pub fn auth_domain(&self) -> &str {
        match self {
            GitHubDomain::Public => "github.com",
            GitHubDomain::Enterprise { domain } => domain.as_str(),
        }
    }

    /// Returns the Copilot API base URL
    pub fn api_base_url(&self) -> String {
        match self {
            GitHubDomain::Public => "https://api.githubcopilot.com".to_string(),
            GitHubDomain::Enterprise { domain } => {
                format!("https://copilot-api.{}", domain)
            }
        }
    }

    /// Returns the Copilot API host (without scheme)
    pub fn api_host(&self) -> String {
        match self {
            GitHubDomain::Public => "api.githubcopilot.com".to_string(),
            GitHubDomain::Enterprise { domain } => {
                format!("copilot-api.{}", domain)
            }
        }
    }
}

impl Default for GitHubDomain {
    fn default() -> Self {
        GitHubDomain::Public
    }
}

/// OAuth access token from GitHub
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubToken {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub scope: String,
}

/// Complete authentication state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub github_token: GithubToken,
    pub domain: GitHubDomain,
    pub authenticated_at: DateTime<Utc>,
}

/// Device flow response from GitHub
#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Access token polling response
#[derive(Debug, Deserialize)]
pub struct TokenPollResponse {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
    #[serde(default)]
    pub interval: Option<u64>,
}
