mod config;
mod players;
mod state;
mod tcp_server;
mod web;

use anyhow::Result;
use config::AppConfig;
use log::{debug, info};
use players::{MockPlayer, Player, SendspinPlayer, ShairportPlayer, UpnpPlayer};
use state::SessionManager;
use std::sync::Arc;
use tcp_server::TcpServer;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let config = AppConfig::load()?;
    let tcp_server = Arc::new(TcpServer::new(&config.server.host, config.server.tcp_port));
    let session_manager =
        SessionManager::new(Arc::clone(&tcp_server), config.server.art_cache_dir.clone());
    let (upnp_event_tx, upnp_event_rx) = tokio::sync::mpsc::channel(128);

    let web_task = {
        let upnp_event_tx = upnp_event_tx.clone();
        let host = config.server.host.clone();
        let art_cache_dir = config.server.art_cache_dir.clone();
        let web_port = config.server.web_port;
        tokio::spawn(async move {
            web::endpoints::serve(&host, web_port, art_cache_dir, upnp_event_tx).await
        })
    };

    let tcp_task = {
        let tcp_server = Arc::clone(&tcp_server);
        tokio::spawn(async move { tcp_server.start().await })
    };

    let players: Vec<Box<dyn Player>> = vec![
        Box::new(ShairportPlayer::new(
            config.players.shairport.clone(),
            session_manager.clone(),
        )),
        Box::new(UpnpPlayer::new(
            config.players.upnp.clone(),
            config.server.web_port,
            session_manager.clone(),
            upnp_event_rx,
        )),
        Box::new(SendspinPlayer::new(
            config.players.sendspin.clone(),
            session_manager.clone(),
        )),
        Box::new(MockPlayer::new(
            config.players.mock.clone(),
            session_manager.clone(),
        )),
    ];

    let mut player_tasks = JoinSet::new();
    for player in players {
        let name = player.name();
        let enabled = player.enabled();
        debug!("PLAYER: {name} enabled={enabled}");
        player_tasks.spawn(async move { (name, enabled, player.start().await) });
    }

    tokio::select! {
        result = web_task => result??,
        result = tcp_task => result??,
        result = wait_for_player_error(&mut player_tasks) => result?,
        _ = tokio::signal::ctrl_c() => {
            info!("service stopped");
        }
    }

    Ok(())
}

async fn wait_for_player_error(
    player_tasks: &mut JoinSet<(&'static str, bool, Result<()>)>,
) -> Result<()> {
    while let Some(result) = player_tasks.join_next().await {
        let (name, enabled, result) = result?;
        result?;

        if enabled {
            info!("PLAYER: {name} exited cleanly");
        } else {
            debug!("PLAYER: {name} disabled; startup task finished");
        }
    }

    std::future::pending().await
}
