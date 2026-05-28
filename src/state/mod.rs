pub mod session;

use serde::{Deserialize, Serialize};

pub use session::SessionManager;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl PlayerState {
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "playing" => Some(Self::Playing),
            "paused" | "paused_playback" => Some(Self::Paused),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

impl std::fmt::Display for PlayerState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Playing => formatter.write_str("playing"),
            Self::Paused => formatter.write_str("paused"),
            Self::Stopped => formatter.write_str("stopped"),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UnifiedState {
    pub player_state: PlayerState,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover_url: Option<String>,
    pub songid: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub songid: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover_url: Option<String>,
}
