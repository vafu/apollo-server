use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::state::PlayerState;

pub const CONFIG_APP_DIR: &str = "apollo";
pub const CONFIG_FILE_NAME: &str = "server.toml";

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub players: PlayersConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let explicit_path = env::var_os("APOLLO_CONFIG").map(PathBuf::from);
        let path = explicit_path.clone().unwrap_or_else(default_config_path);

        if path.exists() {
            Self::load_from_path(&path)
        } else if explicit_path.is_some() {
            anyhow::bail!("config file does not exist: {}", path.display());
        } else {
            println!(
                "CONFIG: {} not found; using built-in defaults.",
                path.display()
            );
            Ok(Self::default())
        }
    }

    fn load_from_path(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let config = toml::from_str(&contents)
            .with_context(|| format!("parsing config file {}", path.display()))?;
        println!("CONFIG: Loaded {}", path.display());
        Ok(config)
    }
}

pub fn default_config_path() -> PathBuf {
    xdg_config_home()
        .join(CONFIG_APP_DIR)
        .join(CONFIG_FILE_NAME)
}

fn xdg_config_home() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return PathBuf::from(home).join(".config");
    }

    PathBuf::from(".config")
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            players: PlayersConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub web_port: u16,
    pub tcp_port: u16,
    pub art_cache_dir: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            web_port: 5556,
            tcp_port: 5557,
            art_cache_dir: PathBuf::from("/tmp/art_cache"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct PlayersConfig {
    pub shairport: ShairportConfig,
    pub upnp: UpnpConfig,
    pub sendspin: SendspinConfig,
    pub mock: MockConfig,
}

impl Default for PlayersConfig {
    fn default() -> Self {
        Self {
            shairport: ShairportConfig::default(),
            upnp: UpnpConfig::default(),
            sendspin: SendspinConfig::default(),
            mock: MockConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ShairportConfig {
    pub enabled: bool,
    pub pipe_path: PathBuf,
    pub art_staging_dir: PathBuf,
    pub retry_secs: u64,
}

impl Default for ShairportConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pipe_path: PathBuf::from("/tmp/shairport-sync-metadata"),
            art_staging_dir: PathBuf::from("/tmp/shairport_art_cache"),
            retry_secs: 5,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct UpnpConfig {
    pub enabled: bool,
    pub renderer_name: String,
    pub search_retry_secs: u64,
    pub resubscribe_secs: u64,
}

impl Default for UpnpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            renderer_name: "Apollo UPNP".to_string(),
            search_retry_secs: 15,
            resubscribe_secs: 20 * 60,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SendspinConfig {
    pub enabled: bool,
    pub server: String,
    pub client_id: String,
    pub name: String,
}

impl Default for SendspinConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: "pluto:8095".to_string(),
            client_id: "apollo-server".to_string(),
            name: "Apollo Server".to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MockConfig {
    pub enabled: bool,
    pub interval_secs: u64,
    pub tracks: Vec<MockTrackConfig>,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 8,
            tracks: vec![
                MockTrackConfig {
                    player_state: PlayerState::Playing,
                    artist: "Daft Punk".to_string(),
                    album: "Discovery".to_string(),
                    title: "One More Time".to_string(),
                    songid: "101".to_string(),
                    cover_url: None,
                },
                MockTrackConfig {
                    player_state: PlayerState::Paused,
                    artist: "Daft Punk".to_string(),
                    album: "Discovery".to_string(),
                    title: "One More Time".to_string(),
                    songid: "101".to_string(),
                    cover_url: None,
                },
                MockTrackConfig {
                    player_state: PlayerState::Playing,
                    artist: "Gorillaz".to_string(),
                    album: "Demon Days".to_string(),
                    title: "Feel Good Inc".to_string(),
                    songid: "102".to_string(),
                    cover_url: None,
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MockTrackConfig {
    pub player_state: PlayerState,
    pub artist: String,
    pub album: String,
    pub title: String,
    pub songid: String,
    pub cover_url: Option<String>,
}

impl Default for MockTrackConfig {
    fn default() -> Self {
        Self {
            player_state: PlayerState::Playing,
            artist: String::new(),
            album: String::new(),
            title: String::new(),
            songid: String::new(),
            cover_url: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_config() {
        let config: AppConfig = toml::from_str(include_str!("../config.example.toml")).unwrap();

        assert_eq!(config.server.web_port, 5556);
        assert!(!config.players.shairport.enabled);
        assert!(!config.players.upnp.enabled);
        assert!(!config.players.sendspin.enabled);
        assert_eq!(config.players.sendspin.server, "pluto:8095");
        assert!(!config.players.mock.enabled);
        assert_eq!(config.players.mock.tracks.len(), 3);
    }

    #[test]
    fn partial_config_uses_defaults() {
        let config: AppConfig = toml::from_str(
            r#"
            [players.upnp]
            enabled = false
            "#,
        )
        .unwrap();

        assert_eq!(config.server.tcp_port, 5557);
        assert!(!config.players.shairport.enabled);
        assert!(!config.players.upnp.enabled);
        assert!(!config.players.sendspin.enabled);
        assert!(!config.players.mock.enabled);
    }

    #[test]
    fn default_config_path_uses_apollo_server_toml() {
        let suffix = Path::new(CONFIG_APP_DIR).join(CONFIG_FILE_NAME);
        assert!(default_config_path().ends_with(suffix));
    }
}
