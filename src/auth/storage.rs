use crate::auth::token::AuthState;
use crate::error::AppError;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

const AUTH_FILE: &str = "auth.toml";

pub struct TokenStorage {
    config_dir: PathBuf,
}

impl TokenStorage {
    pub fn new() -> Result<Self, AppError> {
        let project_dirs = ProjectDirs::from("", "", "occp-proxy").ok_or_else(|| {
            AppError::ConfigError("Could not determine config directory".to_string())
        })?;

        let config_dir = project_dirs.config_dir().to_path_buf();
        fs::create_dir_all(&config_dir)?;

        Ok(Self { config_dir })
    }

    fn auth_file_path(&self) -> PathBuf {
        self.config_dir.join(AUTH_FILE)
    }

    /// Load saved authentication state
    pub fn load(&self) -> Result<Option<AuthState>, AppError> {
        let path = self.auth_file_path();
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)?;
        let state: AuthState = toml::from_str(&content)?;
        Ok(Some(state))
    }

    /// Save authentication state
    pub fn save(&self, state: &AuthState) -> Result<(), AppError> {
        let path = self.auth_file_path();
        let content = toml::to_string_pretty(state)?;

        // Write atomically using temp file
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, &content)?;

        // Set restrictive permissions on Unix before rename
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600))?;
        }

        fs::rename(&temp_path, &path)?;

        Ok(())
    }

    /// Clear saved authentication
    pub fn clear(&self) -> Result<(), AppError> {
        let path = self.auth_file_path();
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Get config directory path (for display)
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }
}
