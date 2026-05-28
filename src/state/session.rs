use crate::{
    state::{Metadata, PlayerState, UnifiedState},
    tcp_server::TcpServer,
};
use anyhow::{Context, Result};
use image::ImageFormat;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct SessionManager {
    tcp_server: Arc<TcpServer>,
    state: Arc<Mutex<UnifiedState>>,
    http_client: reqwest::Client,
    art_cache_dir: PathBuf,
}

impl SessionManager {
    pub fn new(tcp_server: Arc<TcpServer>, art_cache_dir: PathBuf) -> Self {
        Self {
            tcp_server,
            state: Arc::new(Mutex::new(UnifiedState {
                player_state: PlayerState::Stopped,
                ..UnifiedState::default()
            })),
            http_client: reqwest::Client::new(),
            art_cache_dir,
        }
    }

    pub async fn update_transport_state(&self, player_name: &str, transport_state: PlayerState) {
        let state_to_send = {
            let mut state = self.state.lock().await;
            if state.player_state == transport_state {
                return;
            }

            println!("SESSION: {player_name} -> {transport_state}");
            state.player_state = transport_state;
            if transport_state == PlayerState::Stopped {
                state.title = None;
                state.artist = None;
                state.album = None;
                state.cover_url = None;
                state.songid = None;
            }
            state.clone()
        };
        self.broadcast_state(state_to_send).await;
    }

    pub async fn update_metadata(&self, player_name: &str, metadata: Metadata) {
        let new_songid = metadata.songid.clone();
        let original_cover_url = metadata.cover_url.clone();

        let (state_to_send, art_to_process) = {
            let mut state = self.state.lock().await;
            let track_changed = new_songid.is_none() || new_songid != state.songid;
            let metadata_changed = metadata.cover_url.is_some() && state.cover_url.is_none();
            if !track_changed && !metadata_changed {
                return;
            }

            println!("SESSION: Received new metadata from '{player_name}'.");
            state.songid = metadata.songid.clone();
            state.title = metadata.title.clone();
            state.artist = metadata.artist.clone();
            state.album = metadata.album.clone();
            state.player_state = PlayerState::Playing;
            state.cover_url = None;

            let art_to_process = if let (Some(songid), Some(remote_cover_url)) =
                (new_songid.clone(), original_cover_url)
            {
                let (exists, relative_url, _) = art_paths(&self.art_cache_dir, &remote_cover_url);
                if exists {
                    state.cover_url = Some(relative_url);
                    None
                } else {
                    Some((songid, remote_cover_url))
                }
            } else {
                None
            };

            (state.clone(), art_to_process)
        };

        self.broadcast_state(state_to_send).await;

        if let Some((songid, remote_cover_url)) = art_to_process {
            let session = self.clone();
            tokio::spawn(async move {
                if let Err(err) = session
                    .process_and_cache_art(songid, remote_cover_url)
                    .await
                {
                    println!("SESSION: Failed to process art: {err:#}");
                }
            });
        }
    }

    async fn process_and_cache_art(&self, songid: String, remote_cover_url: String) -> Result<()> {
        let (exists, relative_url, cache_filepath) =
            art_paths(&self.art_cache_dir, &remote_cover_url);
        if exists {
            println!("SESSION: Art for songid {songid} found in cache.");
            let state_to_send = {
                let mut state = self.state.lock().await;
                if state.cover_url.as_deref() == Some(&relative_url) {
                    None
                } else {
                    state.cover_url = Some(relative_url);
                    Some(state.clone())
                }
            };
            if let Some(state) = state_to_send {
                self.broadcast_state(state).await;
            }
            return Ok(());
        }

        println!("SESSION: caching {remote_cover_url} for {songid}");
        fs::create_dir_all(&self.art_cache_dir).context("creating art cache directory")?;

        let image_bytes = if let Some(path) = remote_cover_url.strip_prefix("file://") {
            println!("SESSION: Processing art for {songid} from local file: {path}");
            tokio::fs::read(path).await?
        } else if remote_cover_url.starts_with("http") {
            println!("SESSION: Processing art for {songid} from web URL: {remote_cover_url}");
            self.http_client
                .get(&remote_cover_url)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?
                .to_vec()
        } else {
            anyhow::bail!("unsupported cover URL: {remote_cover_url}");
        };

        let output_path = cache_filepath.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let img = image::load_from_memory(&image_bytes).context("decoding image")?;
            let resized = img.resize_exact(128, 128, image::imageops::FilterType::Triangle);
            let rgb = resized.to_rgb8();
            rgb.save_with_format(output_path, ImageFormat::Jpeg)
                .context("saving cached JPEG")?;
            Ok(())
        })
        .await??;

        let state_to_send = {
            let mut state = self.state.lock().await;
            if state.songid.as_deref() == Some(&songid) {
                state.cover_url = Some(relative_url.clone());
                println!("SESSION: cached {relative_url}");
                Some(state.clone())
            } else {
                None
            }
        };
        if let Some(state) = state_to_send {
            self.broadcast_state(state).await;
        }

        Ok(())
    }

    async fn broadcast_state(&self, state: UnifiedState) {
        println!("SESSION: Sending {state:?}");
        if let Err(err) = self.tcp_server.broadcast(&state).await {
            println!("SESSION: Broadcast failed: {err:#}");
        }
    }
}

fn art_paths(art_cache_dir: &Path, remote_cover_url: &str) -> (bool, String, PathBuf) {
    let digest = md5::compute(remote_cover_url.as_bytes());
    let cache_filename = format!("{digest:x}.jpg");
    let cache_filepath = art_cache_dir.join(&cache_filename);
    (
        cache_filepath.exists(),
        format!("/art/{cache_filename}"),
        cache_filepath,
    )
}
