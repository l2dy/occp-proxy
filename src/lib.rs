pub mod auth;
pub mod config;
pub mod error;
pub mod proxy;

pub use auth::{AuthState, DeviceFlowAuth, GitHubDomain, TokenStorage};
pub use config::Config;
pub use error::AppError;
pub use proxy::CopilotProxy;
