use crate::{
    config::{MockConfig, SendspinConfig, ShairportConfig, UpnpConfig},
    state::SessionManager,
    web::endpoints::UpnpEvent,
};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::mpsc;

pub mod mock_player;
pub mod sendspin_player;
pub mod shairport_player;
pub mod upnp_player;

#[async_trait]
pub(crate) trait Player: Send + 'static {
    fn name(&self) -> &'static str;
    fn enabled(&self) -> bool;
    async fn start(self: Box<Self>) -> Result<()>;
}

pub struct MockPlayer {
    config: MockConfig,
    session_manager: SessionManager,
}

pub struct ShairportPlayer {
    config: ShairportConfig,
    session_manager: SessionManager,
    buffer: String,
    staged_track_info: HashMap<String, Vec<u8>>,
}

pub struct SendspinPlayer {
    config: SendspinConfig,
    session_manager: SessionManager,
}

pub struct UpnpPlayer {
    config: UpnpConfig,
    web_port: u16,
    session_manager: SessionManager,
    event_rx: mpsc::Receiver<UpnpEvent>,
    http_client: reqwest::Client,
}
