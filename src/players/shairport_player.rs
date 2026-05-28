use crate::state::session::{Metadata, SessionManager};
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{collections::HashMap, fs, os::unix::fs::OpenOptionsExt, path::Path};
use tokio::io::AsyncReadExt;

const PLAYER_CACHE_DIR: &str = "/tmp/shairport_art_cache";
const TRANSPORT_CODES: &[&str] = &["prsm", "paus", "pend"];
const METADATA_CODES: &[&str] = &["asar", "minm", "PICT", "mden", "mdst"];

pub struct ShairportPlayer {
    pipe_path: String,
    session_manager: SessionManager,
    buffer: String,
    staged_track_info: HashMap<String, Vec<u8>>,
}

impl ShairportPlayer {
    pub fn new(pipe_path: impl Into<String>, session_manager: SessionManager) -> Self {
        Self {
            pipe_path: pipe_path.into(),
            session_manager,
            buffer: String::new(),
            staged_track_info: HashMap::new(),
        }
    }

    pub async fn start(mut self) {
        loop {
            if let Err(err) = self.run_once().await {
                println!("SHAIRPORT: Main loop error: {err:#}");
            }
            self.buffer.clear();
            println!("SHAIRPORT: Cleanup complete. Retrying in 5s.");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    async fn run_once(&mut self) -> Result<()> {
        println!("SHAIRPORT: Opening pipe at {}...", self.pipe_path);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(0o4000)
            .open(&self.pipe_path)
            .with_context(|| format!("opening metadata pipe {}", self.pipe_path))?;
        let mut file = tokio::fs::File::from_std(file);
        println!("SHAIRPORT: Pipe reader registered. Waiting for events.");

        let mut chunk = [0u8; 4096];
        loop {
            match file.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => {
                    self.buffer.push_str(&String::from_utf8_lossy(&chunk[..n]));
                    self.process_buffer().await;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    }

    async fn process_buffer(&mut self) {
        let mut should_commit_metadata = false;

        while let Some(start_index) = self.buffer.find("<item>") {
            let Some(relative_end_index) = self.buffer[start_index..].find("</item>") else {
                break;
            };
            let full_item_len = start_index + relative_end_index + "</item>".len();
            let item_xml = self.buffer[start_index..full_item_len].to_string();
            self.buffer = self.buffer[full_item_len..].to_string();

            match parse_item_xml(&item_xml) {
                Ok((code, value)) => {
                    if !TRANSPORT_CODES.contains(&code.as_str())
                        && !METADATA_CODES.contains(&code.as_str())
                    {
                        continue;
                    }

                    if let Some(state) = transport_state(&code) {
                        self.session_manager
                            .update_transport_state("AirPlay", state)
                            .await;
                    } else if code == "mdst" {
                        self.staged_track_info.clear();
                    } else if let Some(value) = value {
                        self.staged_track_info.insert(code, value);
                        if self.staged_track_info.contains_key("minm")
                            && self.staged_track_info.contains_key("asar")
                        {
                            should_commit_metadata = true;
                        }
                    }
                }
                Err(err) => {
                    println!("SHAIRPORT: Failed to parse item block: {err:#}");
                    println!("SHAIRPORT: Malformed XML chunk was: {item_xml}");
                }
            }
        }

        if should_commit_metadata {
            println!("SHAIRPORT: Title and artist received. Committing metadata.");
            let artist = text_value(self.staged_track_info.get("asar"));
            let title = text_value(self.staged_track_info.get("minm"));
            let songid = format!("airplay-{artist}-{title}");
            let cover_url = self
                .staged_track_info
                .get("PICT")
                .and_then(|data| save_pict_data(&songid, data).ok());

            self.session_manager
                .update_metadata(
                    "AirPlay",
                    Metadata {
                        songid: Some(songid),
                        title: Some(title),
                        artist: Some(artist),
                        album: None,
                        cover_url,
                    },
                )
                .await;
        }
    }
}

fn parse_item_xml(item_xml: &str) -> Result<(String, Option<Vec<u8>>)> {
    let doc = roxmltree::Document::parse(item_xml)?;
    let code_hex = doc
        .descendants()
        .find(|node| node.has_tag_name("code"))
        .and_then(|node| node.text())
        .unwrap_or_default();
    let code_bytes = decode_hex(code_hex)?;
    let code = String::from_utf8_lossy(&code_bytes).into_owned();

    let value = doc
        .descendants()
        .find(|node| node.has_tag_name("data"))
        .and_then(|node| {
            let content = node.text()?.trim();
            if content.is_empty() {
                None
            } else if node.attribute("encoding") == Some("base64") {
                STANDARD.decode(content).ok()
            } else {
                Some(content.as_bytes().to_vec())
            }
        });

    Ok((code, value))
}

fn decode_hex(input: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 2);
    for pair in input.as_bytes().chunks(2) {
        if pair.len() == 2 {
            let text = std::str::from_utf8(pair)?;
            out.push(u8::from_str_radix(text, 16)?);
        }
    }
    Ok(out)
}

fn transport_state(code: &str) -> Option<&'static str> {
    match code {
        "prsm" => Some("playing"),
        "pend" => Some("stopped"),
        "paus" => Some("paused"),
        _ => None,
    }
}

fn save_pict_data(songid: &str, image_bytes: &[u8]) -> Result<String> {
    fs::create_dir_all(PLAYER_CACHE_DIR)?;
    let filename = format!("{:x}.tmp", md5::compute(songid.as_bytes()));
    let filepath = Path::new(PLAYER_CACHE_DIR).join(filename);
    if !filepath.exists() {
        fs::write(&filepath, image_bytes)?;
    }
    Ok(format!("file://{}", filepath.display()))
}

fn text_value(value: Option<&Vec<u8>>) -> String {
    value
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default()
}
