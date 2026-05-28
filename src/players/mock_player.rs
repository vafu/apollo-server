use crate::state::session::{Metadata, SessionManager};
use std::time::Duration;

#[allow(dead_code)]
pub struct MockPlayer {
    session_manager: SessionManager,
}

#[allow(dead_code)]
impl MockPlayer {
    pub fn new(session_manager: SessionManager) -> Self {
        Self { session_manager }
    }

    pub async fn start(self) {
        let states = [
            ("playing", "Daft Punk", "Discovery", "One More Time", "101"),
            ("paused", "Daft Punk", "Discovery", "One More Time", "101"),
            ("playing", "Gorillaz", "Demon Days", "Feel Good Inc", "102"),
        ];
        let mut index = 0usize;

        loop {
            let (player_state, artist, album, title, songid) = states[index];
            self.session_manager
                .update_metadata(
                    "MOCK",
                    Metadata {
                        songid: Some(songid.to_string()),
                        title: Some(title.to_string()),
                        artist: Some(artist.to_string()),
                        album: Some(album.to_string()),
                        cover_url: None,
                    },
                )
                .await;
            self.session_manager
                .update_transport_state("MOCK", player_state)
                .await;

            index = (index + 1) % states.len();
            tokio::time::sleep(Duration::from_secs(8)).await;
        }
    }
}
