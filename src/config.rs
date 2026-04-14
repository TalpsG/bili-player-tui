use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub bilibili: BilibiliConfig,

    #[serde(default)]
    pub player: PlayerConfig,

    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BilibiliConfig {
    /// SESSDATA cookie for authenticated access
    #[serde(default)]
    pub sessdata: String,
}

impl Default for BilibiliConfig {
    fn default() -> Self {
        Self {
            sessdata: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    /// Volume 0-100
    #[serde(default = "default_volume")]
    pub volume: u16,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            volume: default_volume(),
        }
    }
}

fn default_volume() -> u16 {
    80
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Theme name
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
        }
    }
}

fn default_theme() -> String {
    "dark".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bilibili: BilibiliConfig::default(),
            player: PlayerConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Config {
    pub fn config_dir() -> Result<PathBuf, AppError> {
        let dir = dirs::config_dir()
            .or_else(dirs::data_dir)
            .ok_or_else(|| AppError::Config("Cannot determine config directory".into()))?;
        Ok(dir.join("bili-player-cli"))
    }

    pub fn config_path() -> Result<PathBuf, AppError> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn load(path: Option<&str>) -> Result<Self, AppError> {
        let path = match path {
            Some(p) => PathBuf::from(p),
            None => Self::config_path()?,
        };

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Config(format!("Failed to read config: {e}")))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse config: {e}")))?;

        Ok(config)
    }
}
