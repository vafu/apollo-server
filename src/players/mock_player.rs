use crate::{
    config::{MockConfig, MockTrackConfig},
    state::{Metadata, SessionManager},
};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

use super::{MockPlayer, Player};

impl MockPlayer {
    pub fn new(config: MockConfig, session_manager: SessionManager) -> Self {
        Self {
            config,
            session_manager,
        }
    }
}

#[async_trait]
impl Player for MockPlayer {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    async fn start(self: Box<Self>) -> Result<()> {
        let player = *self;

        if !player.config.enabled {
            println!("MOCK: disabled.");
            return Ok(());
        }

        if player.config.tracks.is_empty() {
            println!("MOCK: No tracks configured; mock player is idle.");
            return Ok(());
        }

        let mut index = 0usize;

        loop {
            let track = &player.config.tracks[index];
            player
                .session_manager
                .update_metadata("MOCK", metadata_from_track(track))
                .await;
            player
                .session_manager
                .update_transport_state("MOCK", track.player_state)
                .await;

            index = (index + 1) % player.config.tracks.len();
            tokio::time::sleep(Duration::from_secs(player.config.interval_secs)).await;
        }
    }
}

fn metadata_from_track(track: &MockTrackConfig) -> Metadata {
    Metadata {
        songid: Some(track.songid.clone()),
        title: Some(track.title.clone()),
        artist: Some(track.artist.clone()),
        album: Some(track.album.clone()),
        cover_url: track.cover_url.clone(),
    }
}
