use crate::{
    config::SendspinConfig,
    state::{Metadata, PlayerState, SessionManager},
};
use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info, warn};
use sendspin::{
    ProtocolClientBuilder,
    protocol::client::ArtworkChunk,
    protocol::messages::{
        ArtworkChannel, ArtworkSource, ArtworkV1Support, GroupUpdate, ImageFormat, Message,
        MetadataState, PlaybackState, ServerState, TrackProgress,
    },
};
use std::future::poll_fn;
use tokio::sync::mpsc::UnboundedReceiver;

use super::{Player, SendspinPlayer};

impl SendspinPlayer {
    pub fn new(config: SendspinConfig, session_manager: SessionManager) -> Self {
        Self {
            config,
            session_manager,
        }
    }
}

#[async_trait]
impl Player for SendspinPlayer {
    fn name(&self) -> &'static str {
        "sendspin"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    async fn start(self: Box<Self>) -> Result<()> {
        let player = *self;

        if !player.config.enabled {
            debug!("SENDSPIN: disabled.");
            return Ok(());
        }

        player.run_once().await
    }
}

impl SendspinPlayer {
    async fn run_once(&self) -> Result<()> {
        let url = sendspin_url(&self.config.server);
        info!("SENDSPIN: connecting to {url}");

        let client = ProtocolClientBuilder::builder()
            .client_id(self.config.client_id.clone())
            .name(self.config.name.clone())
            .metadata()
            .artwork_v1_support(artwork_support())
            .build()
            .connect(&url)
            .await?;

        let connection = client.split();
        let mut messages = connection.messages;
        let mut artwork = Some(connection.artwork);

        let _sender = connection.sender;
        let _clock_sync = connection.clock_sync;
        let _visualizer = connection.visualizer;
        let _guard = connection.guard;

        info!("SENDSPIN: connected to {url}");

        loop {
            tokio::select! {
                message = poll_message(&mut messages) => {
                    match message {
                        Some(message) => self.handle_message(message).await,
                        None => {
                            warn!("SENDSPIN: protocol message channel closed");
                            std::future::pending::<()>().await;
                        }
                    }
                }
                artwork_chunk = poll_artwork(&mut artwork) => {
                    match artwork_chunk {
                        Some(artwork) => self.handle_artwork(artwork).await,
                        None => {
                            debug!("SENDSPIN: artwork channel closed");
                            artwork = None;
                        }
                    }
                }
            }
        }

        #[allow(unreachable_code)]
        Ok(())
    }

    async fn handle_message(&self, message: Message) {
        debug!("SENDSPIN: received {}", message_name(&message));

        match message {
            Message::ServerState(state) => self.handle_server_state(state).await,
            Message::GroupUpdate(group) => self.handle_group_update(group).await,
            Message::StreamStart(stream_start) => {
                debug!("SENDSPIN: stream/start {stream_start:?}");
            }
            Message::StreamEnd(stream_end) => {
                debug!("SENDSPIN: stream/end {stream_end:?}");
            }
            Message::StreamClear(stream_clear) => {
                debug!("SENDSPIN: stream/clear {stream_clear:?}");
            }
            Message::ServerCommand(command) => {
                debug!("SENDSPIN: server/command {command:?}");
            }
            _ => {}
        }
    }

    async fn handle_server_state(&self, state: ServerState) {
        debug!("SENDSPIN: server/state {state:?}");

        if let Some(metadata) = state.metadata {
            if has_track_metadata(&metadata) || metadata.progress.is_some() {
                self.session_manager
                    .update_metadata("Sendspin", metadata_from_sendspin(metadata))
                    .await;
            } else {
                self.session_manager
                    .update_metadata(
                        "Sendspin",
                        Metadata {
                            player_state: Some(PlayerState::Stopped),
                            ..Metadata::default()
                        },
                    )
                    .await;
            }
        } else {
            debug!("SENDSPIN: server/state missing metadata");
        }
    }

    async fn handle_group_update(&self, group: GroupUpdate) {
        debug!("SENDSPIN: group/update {group:?}");

        match group.playback_state {
            Some(PlaybackState::Playing) => {
                self.session_manager
                    .update_transport_state("Sendspin", PlayerState::Playing)
                    .await;
            }
            Some(PlaybackState::Stopped) | None => {
                // Music Assistant reports pause as group/update stopped. Let
                // metadata.progress distinguish paused from fully stopped.
            }
        }
    }

    async fn handle_artwork(&self, artwork: ArtworkChunk) {
        if artwork.is_clear() {
            debug!(
                "SENDSPIN: artwork clear channel={} timestamp={}",
                artwork.channel, artwork.timestamp
            );
            self.session_manager.clear_cover_art("Sendspin").await;
            return;
        }

        debug!(
            "SENDSPIN: artwork chunk channel={} timestamp={} bytes={}",
            artwork.channel,
            artwork.timestamp,
            artwork.data.len()
        );

        let source_key = format!(
            "sendspin-art://channel/{}/timestamp/{}/digest/{:x}",
            artwork.channel,
            artwork.timestamp,
            md5::compute(&artwork.data)
        );
        self.session_manager
            .update_raw_cover_art("Sendspin", source_key, artwork.data.to_vec())
            .await;
    }
}

async fn poll_message(messages: &mut UnboundedReceiver<Message>) -> Option<Message> {
    poll_fn(|context| messages.poll_recv(context)).await
}

async fn poll_artwork(
    artwork: &mut Option<UnboundedReceiver<ArtworkChunk>>,
) -> Option<ArtworkChunk> {
    let Some(artwork) = artwork else {
        std::future::pending::<()>().await;
        return None;
    };

    poll_fn(|context| artwork.poll_recv(context)).await
}

fn artwork_support() -> ArtworkV1Support {
    ArtworkV1Support {
        channels: vec![ArtworkChannel {
            source: ArtworkSource::Album,
            format: ImageFormat::Jpeg,
            media_width: 128,
            media_height: 128,
        }],
    }
}

fn message_name(message: &Message) -> &'static str {
    match message {
        Message::ClientHello(_) => "client/hello",
        Message::ServerHello(_) => "server/hello",
        Message::ClientTime(_) => "client/time",
        Message::ServerTime(_) => "server/time",
        Message::ClientState(_) => "client/state",
        Message::ServerState(_) => "server/state",
        Message::ServerCommand(_) => "server/command",
        Message::ClientCommand(_) => "client/command",
        Message::StreamStart(_) => "stream/start",
        Message::StreamEnd(_) => "stream/end",
        Message::StreamClear(_) => "stream/clear",
        Message::StreamRequestFormat(_) => "stream/request-format",
        Message::GroupUpdate(_) => "group/update",
        Message::ClientGoodbye(_) => "client/goodbye",
    }
}

fn sendspin_url(server: &str) -> String {
    if server.starts_with("ws://") || server.starts_with("wss://") {
        server.to_string()
    } else if server.contains('/') {
        format!("ws://{server}")
    } else {
        format!("ws://{server}/sendspin")
    }
}

fn has_track_metadata(metadata: &MetadataState) -> bool {
    metadata.title.is_some()
        || metadata.artist.is_some()
        || metadata.album.is_some()
        || metadata.artwork_url.is_some()
}

fn metadata_from_sendspin(metadata: MetadataState) -> Metadata {
    let player_state = Some(player_state_from_metadata(&metadata));
    let songid = synthetic_songid(&metadata);
    Metadata {
        player_state,
        songid,
        title: metadata.title,
        artist: metadata.artist,
        album: metadata.album,
        cover_url: metadata.artwork_url,
    }
}

fn synthetic_songid(metadata: &MetadataState) -> Option<String> {
    if !has_track_metadata(metadata) {
        return None;
    }

    Some(format!(
        "sendspin:{}:{}:{}:{}",
        metadata.title.as_deref().unwrap_or_default(),
        metadata.artist.as_deref().unwrap_or_default(),
        metadata.album.as_deref().unwrap_or_default(),
        metadata.artwork_url.as_deref().unwrap_or_default()
    ))
}

fn player_state_from_metadata(metadata: &MetadataState) -> PlayerState {
    match metadata.progress.as_ref() {
        Some(progress) => player_state_from_progress(progress),
        None => PlayerState::Stopped,
    }
}

fn player_state_from_progress(progress: &TrackProgress) -> PlayerState {
    if progress.playback_speed == 0 {
        PlayerState::Paused
    } else {
        PlayerState::Playing
    }
}
