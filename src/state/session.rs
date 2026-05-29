use crate::{
    state::{Metadata, PlayerState, UnifiedState},
    tcp_server::TcpServer,
};
use anyhow::{Context, Result};
use image::ImageFormat;
use log::{debug, warn};
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

            debug!("SESSION: {player_name} -> {transport_state}");
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
        let new_player_state = metadata.player_state;
        let new_songid = metadata.songid.clone();
        let original_cover_url = metadata.cover_url.clone();
        let has_track_metadata = has_track_metadata(&metadata);

        let (state_to_send, art_to_process) = {
            let mut state = self.state.lock().await;
            let track_changed = has_track_metadata && new_songid != state.songid;
            let metadata_changed = metadata.cover_url.is_some() && state.cover_url.is_none();
            let state_changed =
                new_player_state.is_some_and(|new_state| new_state != state.player_state);
            if !track_changed && !metadata_changed && !state_changed {
                return;
            }

            debug!("SESSION: received new metadata from '{player_name}'");

            if let Some(new_player_state) = new_player_state {
                state.player_state = new_player_state;
            }

            let mut art_to_process = None;
            if state.player_state == PlayerState::Stopped {
                state.songid = None;
                state.title = None;
                state.artist = None;
                state.album = None;
                state.cover_url = None;
            } else if track_changed {
                state.songid = metadata.songid.clone();
                state.title = metadata.title.clone();
                state.artist = metadata.artist.clone();
                state.album = metadata.album.clone();
                state.cover_url = None;

                if let (Some(songid), Some(remote_cover_url)) =
                    (new_songid.clone(), original_cover_url)
                {
                    let (exists, relative_url, _) =
                        art_paths(&self.art_cache_dir, &remote_cover_url);
                    if exists {
                        state.cover_url = Some(relative_url);
                    } else {
                        art_to_process = Some((songid, remote_cover_url));
                    }
                }
            } else if metadata_changed {
                if let (Some(songid), Some(remote_cover_url)) =
                    (new_songid.clone(), original_cover_url)
                {
                    let (exists, relative_url, _) =
                        art_paths(&self.art_cache_dir, &remote_cover_url);
                    if exists {
                        state.cover_url = Some(relative_url);
                    } else {
                        art_to_process = Some((songid, remote_cover_url));
                    }
                } else {
                    state.cover_url = metadata.cover_url.clone();
                }
            }

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
                    warn!("SESSION: failed to process art: {err:#}");
                }
            });
        }
    }

    pub async fn update_raw_cover_art(
        &self,
        player_name: &str,
        source_key: String,
        image_bytes: Vec<u8>,
    ) {
        let session = self.clone();
        let player_name = player_name.to_string();
        tokio::spawn(async move {
            if let Err(err) = session
                .process_and_cache_raw_art(&player_name, source_key, image_bytes)
                .await
            {
                warn!("SESSION: failed to process raw art from {player_name}: {err:#}");
            }
        });
    }

    pub async fn clear_cover_art(&self, player_name: &str) {
        let state_to_send = {
            let mut state = self.state.lock().await;
            if state.cover_url.is_none() {
                return;
            }

            debug!("SESSION: clearing cover art from '{player_name}'");
            state.cover_url = None;
            state.clone()
        };
        self.broadcast_state(state_to_send).await;
    }

    async fn process_and_cache_art(&self, songid: String, remote_cover_url: String) -> Result<()> {
        let (exists, relative_url, cache_filepath) =
            art_paths(&self.art_cache_dir, &remote_cover_url);
        if exists {
            debug!("SESSION: art for songid {songid} found in cache");
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

        debug!("SESSION: caching {remote_cover_url} for {songid}");
        fs::create_dir_all(&self.art_cache_dir).context("creating art cache directory")?;

        let image_bytes = if let Some(path) = remote_cover_url.strip_prefix("file://") {
            debug!("SESSION: processing art for {songid} from local file: {path}");
            tokio::fs::read(path).await?
        } else if remote_cover_url.starts_with("http") {
            debug!("SESSION: processing art for {songid} from web URL: {remote_cover_url}");
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
                debug!("SESSION: cached {relative_url}");
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

    async fn process_and_cache_raw_art(
        &self,
        player_name: &str,
        source_key: String,
        image_bytes: Vec<u8>,
    ) -> Result<()> {
        let (exists, relative_url, cache_filepath) = art_paths(&self.art_cache_dir, &source_key);
        if exists {
            debug!("SESSION: raw art from '{player_name}' found in cache");
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

        debug!("SESSION: caching raw art from '{player_name}'");
        fs::create_dir_all(&self.art_cache_dir).context("creating art cache directory")?;

        let output_path = cache_filepath.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let img = image::load_from_memory(&image_bytes).context("decoding raw artwork")?;
            let resized = img.resize_exact(128, 128, image::imageops::FilterType::Triangle);
            let rgb = resized.to_rgb8();
            rgb.save_with_format(output_path, ImageFormat::Jpeg)
                .context("saving cached raw artwork JPEG")?;
            Ok(())
        })
        .await??;

        let state_to_send = {
            let mut state = self.state.lock().await;
            state.cover_url = Some(relative_url.clone());
            debug!("SESSION: cached raw art {relative_url}");
            state.clone()
        };
        self.broadcast_state(state_to_send).await;

        Ok(())
    }

    async fn broadcast_state(&self, state: UnifiedState) {
        debug!("SESSION: sending {state:?}");
        if let Err(err) = self.tcp_server.broadcast(&state).await {
            warn!("SESSION: broadcast failed: {err:#}");
        }
    }
}

fn has_track_metadata(metadata: &Metadata) -> bool {
    metadata.songid.is_some()
        || metadata.title.is_some()
        || metadata.artist.is_some()
        || metadata.album.is_some()
        || metadata.cover_url.is_some()
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
