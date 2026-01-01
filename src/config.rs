use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Configuration for the Immich uploader, storing multiple users and the current active user.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    /// The name of the currently active user.
    pub current_user: Option<String>,
    /// A map of user names to their respective configurations.
    pub users: HashMap<String, UserConfig>,
}

/// Configuration details for a specific Immich user.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserConfig {
    /// API key for authentication with the Immich server.
    pub api_key: String,
    /// Base URL of the Immich server.
    pub server_url: String,
}

impl Config {
    /// Loads the configuration from the default path (~/.immich/config.toml).
    /// Returns default config if the file does not exist.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Config::default());
        }
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Saves the current configuration to the default path.
    /// Creates parent directories if they don't exist.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Determines the configuration file path.
    /// Typically ~/.immich/config.toml on Unix systems.
    fn config_path() -> Result<PathBuf> {
        let home = std::env::var("HOME").map(PathBuf::from).or_else(|_| {
            #[allow(deprecated)]
            std::env::home_dir().context("Could not find home directory")
        })?;
        Ok(home.join(".immich").join("config.toml"))
    }

    /// Retrieves the current active user from the configuration map.
    pub fn get_current_user(&self) -> Option<(&String, &UserConfig)> {
        let name = self.current_user.as_ref()?;
        self.users.get(name).map(|u| (name, u))
    }
}
