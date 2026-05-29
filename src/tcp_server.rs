use anyhow::Result;
use log::{debug, info, warn};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{RwLock, broadcast},
};

#[derive(Clone)]
pub struct TcpServer {
    host: String,
    port: u16,
    tx: broadcast::Sender<Vec<u8>>,
    last_message: Arc<RwLock<Option<Vec<u8>>>>,
}

impl TcpServer {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            host: host.into(),
            port,
            tx,
            last_message: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn broadcast<T: Serialize>(&self, state: &T) -> Result<()> {
        let payload = serde_json::to_vec(state)?;
        let mut message = Vec::with_capacity(4 + payload.len());
        message.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        message.extend_from_slice(&payload);

        *self.last_message.write().await = Some(message.clone());
        let _ = self.tx.send(message);
        Ok(())
    }

    pub async fn start(self: Arc<Self>) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        let listener = TcpListener::bind(&addr).await?;
        info!("TCP: serving on {}", listener.local_addr()?);

        loop {
            let (stream, addr) = listener.accept().await?;
            debug!("TCP: accepted connection from {addr}");
            let server = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(err) = server.handle_client(stream, addr).await {
                    warn!("TCP: client {addr} error: {err}");
                }
            });
        }
    }

    async fn handle_client(&self, stream: TcpStream, addr: SocketAddr) -> Result<()> {
        let (mut reader, mut writer) = stream.into_split();
        if let Some(message) = self.last_message.read().await.clone() {
            writer.write_all(&message).await?;
        }

        let mut rx = self.tx.subscribe();
        let mut keepalive = [0u8; 100];
        loop {
            tokio::select! {
                read = reader.read(&mut keepalive) => {
                    if read? == 0 {
                        break;
                    }
                }
                message = rx.recv() => {
                    match message {
                        Ok(message) => writer.write_all(&message).await?,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }

        debug!("TCP: closing connection for {addr}");
        Ok(())
    }
}
