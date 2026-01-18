use serde::{Deserialize, Serialize};

/// User-Agent emulates opencode for Copilot API compatibility
pub const DEFAULT_OPENCODE_VERSION: &str = "1.1.25";

/// Maximum request body size (10MB)
pub const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// OAuth polling safety margin
pub const OAUTH_POLLING_SAFETY_MARGIN_MS: u64 = 3000;

/// RFC 8628 slow_down increment
pub const RFC8628_SLOW_DOWN_INCREMENT: u64 = 5;

/// GitHub OAuth Client ID (from opencode)
pub const GITHUB_CLIENT_ID: &str = "Ov23li8tweQw6odWQebz";

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Server bind address
    #[serde(default = "default_bind_address")]
    pub bind_address: String,

    /// Server port
    #[serde(default = "default_port")]
    pub port: u16,

    /// Static API key for proxy authentication
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_address: default_bind_address(),
            port: default_port(),
            api_key: None,
        }
    }
}

fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}
