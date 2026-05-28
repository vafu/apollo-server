use crate::state::session::ART_CACHE_DIR;
use anyhow::Result;
use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tower_http::services::ServeDir;

#[derive(Clone, Debug)]
pub struct UpnpEvent {
    pub source: String,
    pub body: String,
}

#[derive(Clone)]
struct AppState {
    event_tx: mpsc::Sender<UpnpEvent>,
}

pub async fn serve(host: &str, port: u16, event_tx: mpsc::Sender<UpnpEvent>) -> Result<()> {
    let state = AppState { event_tx };
    let app = Router::new()
        .nest_service("/art", ServeDir::new(ART_CACHE_DIR))
        .route("/upnp/events/{source}", post(upnp_event))
        .with_state(state);

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("WEB: Serving on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn upnp_event(
    State(state): State<AppState>,
    Path(source): Path<String>,
    body: Bytes,
) -> StatusCode {
    let body = String::from_utf8_lossy(&body).into_owned();
    println!("WEB: Received UPnP event from {source}");
    match state.event_tx.send(UpnpEvent { source, body }).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
