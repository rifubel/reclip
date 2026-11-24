use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub buffer_seconds: u64,
    pub quality: Quality,
    pub audio_mode: AudioMode,
    pub save_folder: String,
    #[serde(default)]
    pub manual_games: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub enum Quality {
    #[serde(rename = "high")]
    High,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "economy")]
    Economy,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub enum AudioMode {
    #[serde(rename = "game_only")]
    GameOnly,
    #[serde(rename = "game_plus_mic")]
    GamePlusMic,
}

impl Quality {
    pub fn bitrate_kbps(&self) -> u64 {
        match self {
            Quality::High => 20_000,
            Quality::Medium => 12_000,
            Quality::Economy => 8_000,
        }
    }
}

impl AudioMode {
    pub fn source_args(&self) -> Vec<String> {
        match self {
            AudioMode::GameOnly => vec![
                "-a".to_string(),
                "default_output".to_string(),
            ],
            AudioMode::GamePlusMic => vec![
                "-a".to_string(),
                "default_output".to_string(),
                "-a".to_string(),
                "default_input".to_string(),
            ],
        }
    }
}

pub fn load(path: &std::path::Path) -> Result<Config, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&contents)?;
    Ok(config)
}

pub fn load_default() -> Result<Config, Box<dyn std::error::Error>> {
    load(&default_config_path())
}

pub fn default_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".config")
        .join("reclip")
        .join("config.json")
}
