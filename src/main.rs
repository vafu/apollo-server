mod config;
mod players;
mod state;
mod tcp_server;
mod web;

use anyhow::Result;
use players::{shairport_player::ShairportPlayer, upnp_player::UpnpPlayer};
use state::session::SessionManager;
use std::sync::Arc;
use tcp_server::TcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    let tcp_server = Arc::new(TcpServer::new(
        config::WEB_SERVER_HOST,
        config::TCP_SERVER_PORT,
    ));
    let session_manager = SessionManager::new(Arc::clone(&tcp_server));
    let (upnp_event_tx, upnp_event_rx) = tokio::sync::mpsc::channel(128);

    let web_task = {
        let upnp_event_tx = upnp_event_tx.clone();
        tokio::spawn(async move {
            web::endpoints::serve(
                config::WEB_SERVER_HOST,
                config::WEB_SERVER_PORT,
                upnp_event_tx,
            )
            .await
        })
    };

    let tcp_task = {
        let tcp_server = Arc::clone(&tcp_server);
        tokio::spawn(async move { tcp_server.start().await })
    };

    let upnp_task = {
        let session_manager = session_manager.clone();
        tokio::spawn(async move {
            UpnpPlayer::new(config::TARGET_RENDERER_NAME, session_manager, upnp_event_rx)
                .start()
                .await;
            Ok::<(), anyhow::Error>(())
        })
    };

    let shairport_task = {
        let session_manager = session_manager.clone();
        tokio::spawn(async move {
            ShairportPlayer::new("/tmp/shairport-sync-metadata", session_manager)
                .start()
                .await;
            Ok::<(), anyhow::Error>(())
        })
    };

    tokio::select! {
        result = web_task => result??,
        result = tcp_task => result??,
        result = upnp_task => result??,
        result = shairport_task => result??,
        _ = tokio::signal::ctrl_c() => {
            println!("Service stopped.");
        }
    }

    Ok(())
}
