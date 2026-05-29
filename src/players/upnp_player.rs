use crate::{
    config::UpnpConfig,
    state::{Metadata, PlayerState, SessionManager},
    web::endpoints::UpnpEvent,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::{Method, Url, header::HeaderMap};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    time::Duration,
};
use tokio::sync::mpsc;

use super::{Player, UpnpPlayer};

const INFO_SERVICE_ID: &str = "urn:av-openhome-org:service:Info:1";
const PLAYLIST_SERVICE_ID: &str = "urn:av-openhome-org:service:Playlist:1";

#[derive(Clone, Debug)]
struct ServiceEndpoint {
    service_id: String,
    event_sub_url: Url,
}

#[derive(Debug)]
struct Subscription {
    sid: String,
    endpoint: ServiceEndpoint,
}

impl UpnpPlayer {
    pub fn new(
        config: UpnpConfig,
        web_port: u16,
        session_manager: SessionManager,
        event_rx: mpsc::Receiver<UpnpEvent>,
    ) -> Self {
        Self {
            config,
            web_port,
            session_manager,
            event_rx,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Player for UpnpPlayer {
    fn name(&self) -> &'static str {
        "upnp"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    async fn start(self: Box<Self>) -> Result<()> {
        let mut player = *self;

        if !player.config.enabled {
            println!("UPnP: disabled.");
            return Ok(());
        }

        loop {
            if let Err(err) = player.run_once().await {
                println!("UPnP Error/Retry: {err:#}");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

impl UpnpPlayer {
    async fn run_once(&mut self) -> Result<()> {
        println!("UPnP: Searching for devices...");
        let device = match self.find_target_device().await? {
            Some(device) => device,
            None => {
                println!(
                    "UPnP: Target renderer not found in search results. Retrying in {}s.",
                    self.config.search_retry_secs
                );
                tokio::time::sleep(Duration::from_secs(self.config.search_retry_secs)).await;
                return Ok(());
            }
        };

        println!("UPnP: Target renderer found");
        let local_ip = local_ip_for(device.location.host_str().unwrap_or_default())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let callback_base = format!("http://{}:{}/upnp/events", local_ip, self.web_port);

        let mut subscriptions = Vec::new();
        for endpoint in device.services {
            let source = event_source_name(&endpoint.service_id);
            let callback_url = format!("{callback_base}/{source}");
            let sid = self
                .subscribe(&endpoint.event_sub_url, &callback_url)
                .await?;
            subscriptions.push(Subscription { sid, endpoint });
        }

        let mut resubscribe_interval =
            tokio::time::interval(Duration::from_secs(self.config.resubscribe_secs));
        loop {
            tokio::select! {
                _ = resubscribe_interval.tick() => {
                    println!("UPnP: Resubscribing...");
                    for subscription in &mut subscriptions {
                        match self.renew_subscription(subscription).await {
                            Ok(sid) => subscription.sid = sid,
                            Err(err) => return Err(err).context("renewing UPnP subscription"),
                        }
                    }
                }
                event = self.event_rx.recv() => {
                    let Some(event) = event else {
                        return Err(anyhow!("UPnP event channel closed"));
                    };
                    self.process_event(event).await;
                }
            }
        }
    }

    async fn find_target_device(&self) -> Result<Option<TargetDevice>> {
        let locations = tokio::task::spawn_blocking(send_msearch).await??;
        for location in locations {
            let Ok(url) = Url::parse(&location) else {
                continue;
            };
            let Ok(body) = self.http_client.get(url.clone()).send().await else {
                continue;
            };
            let Ok(body) = body.error_for_status() else {
                continue;
            };
            let Ok(xml) = body.text().await else {
                continue;
            };

            match parse_device_description(&url, &xml, &self.config.renderer_name) {
                Ok(Some(device)) => return Ok(Some(device)),
                Ok(None) => {}
                Err(err) => println!("UPnP: cannot parse device at {url}: {err:#}"),
            }
        }
        Ok(None)
    }

    async fn subscribe(&self, event_sub_url: &Url, callback_url: &str) -> Result<String> {
        let method = Method::from_bytes(b"SUBSCRIBE")?;
        let response = self
            .http_client
            .request(method, event_sub_url.clone())
            .header("CALLBACK", format!("<{callback_url}>"))
            .header("NT", "upnp:event")
            .header("TIMEOUT", "Second-1800")
            .send()
            .await?
            .error_for_status()?;
        sid_from_headers(response.headers()).context("subscription missing SID")
    }

    async fn renew_subscription(&self, subscription: &Subscription) -> Result<String> {
        let method = Method::from_bytes(b"SUBSCRIBE")?;
        let response = self
            .http_client
            .request(method, subscription.endpoint.event_sub_url.clone())
            .header("SID", &subscription.sid)
            .header("TIMEOUT", "Second-1800")
            .send()
            .await?
            .error_for_status()?;
        sid_from_headers(response.headers())
            .or_else(|| Some(subscription.sid.clone()))
            .context("renewal missing SID")
    }

    async fn process_event(&self, event: UpnpEvent) {
        match parse_event_vars(&event.body) {
            Ok(vars) => {
                if let Some(transport_state) = vars.get("TransportState") {
                    match PlayerState::parse(transport_state) {
                        Some(transport_state) => {
                            self.session_manager
                                .update_transport_state("UPNP", transport_state)
                                .await;
                        }
                        None => {
                            println!("UPnP: Ignoring unknown transport state {transport_state}")
                        }
                    }
                }
                if let Some(metadata_xml) = vars.get("Metadata") {
                    match parse_track_metadata(metadata_xml) {
                        Ok(metadata) => {
                            self.session_manager.update_metadata("UPNP", metadata).await;
                        }
                        Err(err) => println!("Error processing UPnP metadata: {err:#}"),
                    }
                }
            }
            Err(err) => println!("Error processing UPnP event from {}: {err:#}", event.source),
        }
    }
}

#[derive(Debug)]
struct TargetDevice {
    location: Url,
    services: Vec<ServiceEndpoint>,
}

fn send_msearch() -> Result<Vec<String>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let message = concat!(
        "M-SEARCH * HTTP/1.1\r\n",
        "HOST: 239.255.255.250:1900\r\n",
        "MAN: \"ssdp:discover\"\r\n",
        "MX: 3\r\n",
        "ST: ssdp:all\r\n",
        "\r\n"
    );
    socket.send_to(message.as_bytes(), "239.255.255.250:1900")?;

    let mut locations = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok((n, _)) = socket.recv_from(&mut buf) {
        let response = String::from_utf8_lossy(&buf[..n]);
        if let Some(location) = header_value(&response, "location") {
            if !locations.iter().any(|known| known == &location) {
                locations.push(location);
            }
        }
    }
    Ok(locations)
}

fn parse_device_description(
    location: &Url,
    xml: &str,
    renderer_name: &str,
) -> Result<Option<TargetDevice>> {
    let doc = roxmltree::Document::parse(xml)?;
    let friendly_name = doc
        .descendants()
        .find(|node| node.has_tag_name("friendlyName"))
        .and_then(|node| node.text())
        .unwrap_or_default();

    if !friendly_name.eq_ignore_ascii_case(renderer_name) {
        return Ok(None);
    }

    let mut services = Vec::new();
    for service in doc
        .descendants()
        .filter(|node| node.has_tag_name("service"))
    {
        let service_id = child_text(service, "serviceId").unwrap_or_default();
        if service_id != INFO_SERVICE_ID && service_id != PLAYLIST_SERVICE_ID {
            continue;
        }
        let Some(event_sub_url) = child_text(service, "eventSubURL") else {
            continue;
        };
        services.push(ServiceEndpoint {
            service_id,
            event_sub_url: location.join(&event_sub_url)?,
        });
    }

    Ok(Some(TargetDevice {
        location: location.clone(),
        services,
    }))
}

fn parse_event_vars(xml: &str) -> Result<HashMap<String, String>> {
    let doc = roxmltree::Document::parse(xml)?;
    let mut vars = HashMap::new();
    for property in doc
        .descendants()
        .filter(|node| node.has_tag_name("property"))
    {
        for child in property.children().filter(|node| node.is_element()) {
            vars.insert(
                child.tag_name().name().to_string(),
                child.text().unwrap_or_default().to_string(),
            );
        }
    }
    Ok(vars)
}

fn parse_track_metadata(track_metadata_xml: &str) -> Result<Metadata> {
    if track_metadata_xml.trim().is_empty() {
        return Ok(Metadata::default());
    }

    let doc = roxmltree::Document::parse(track_metadata_xml)?;
    let title = namespaced_text(&doc, "http://purl.org/dc/elements/1.1/", "title");
    let artist = namespaced_text(&doc, "urn:schemas-upnp-org:metadata-1-0/upnp/", "artist");
    let cover_url = namespaced_text(
        &doc,
        "urn:schemas-upnp-org:metadata-1-0/upnp/",
        "albumArtURI",
    );
    let songid = doc
        .descendants()
        .find(|node| node.attribute("id").is_some())
        .and_then(|node| node.attribute("id"))
        .map(str::to_string);

    Ok(Metadata {
        player_state: None,
        title,
        artist,
        cover_url,
        songid,
        album: None,
    })
}

fn child_text(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.has_tag_name(name))
        .and_then(|child| child.text())
        .map(str::to_string)
}

fn namespaced_text(doc: &roxmltree::Document<'_>, namespace: &str, name: &str) -> Option<String> {
    doc.descendants()
        .find(|node| {
            node.tag_name().namespace() == Some(namespace) && node.tag_name().name() == name
        })
        .and_then(|node| node.text())
        .map(str::to_string)
}

fn header_value(response: &str, name: &str) -> Option<String> {
    response.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.eq_ignore_ascii_case(name) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn sid_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("sid")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn event_source_name(service_id: &str) -> &'static str {
    if service_id == INFO_SERVICE_ID {
        "info"
    } else {
        "playlist"
    }
}

fn local_ip_for(remote_host: &str) -> Option<String> {
    let remote = format!("{remote_host}:80").parse::<SocketAddr>().ok()?;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(remote).ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}
