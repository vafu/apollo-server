# Apollo Media State Service

A multi-source media state aggregator and processor, designed to be a single source of truth for a [client display board](https://github.com/vafu/apollo-led-panel).

---
## Overview

The Apollo service is a Rust application that runs on a central server (e.g., a Raspberry Pi running Moode Audio). Its primary purpose is to listen to various music player daemons, consolidate their state into a single, unified format, and push real-time updates to a client device.

It is architected to be modular, allowing for different player "plugins" to be added. Currently, it supports listening to Shairport Sync (for AirPlay) by reading its metadata pipe. The architecture is in place to support MPD and UPnP renderers as well.

In addition to state tracking, the service also processes album art by downloading the original image, resizing it to fit the boards resolution, and caching it. It then serves these cached images on demand via a simple HTTP endpoint.

---
## Features

* **Multi-Source:** Designed with a pluggable architecture to support various players (Shairport, UPnP, MPD).
* **Event-Driven:** Uses non-blocking I/O and callbacks to react instantly to state changes from players.
* **State Consolidation:** Provides a single, consistent JSON object to clients, regardless of the underlying music source.
* **Image Processing & Caching:** Offloads all heavy image processing from the client. It downloads, resizes, and caches album art on the server.
* **Efficient Communication:** Pushes state updates to clients via a lightweight, raw TCP socket using a length-prefix protocol.
* **Web Server:** Includes a simple, multi-threaded web server to serve the cached, processed album art.

---
## Quick Setup Guide

1.  **Clone the Repository**
    ```bash
    git clone https://github.com/vafu/apollo-server.git
    cd apollo_service
    ```

2.  **Install Rust**
    Install the Rust toolchain with `rustup` if it is not already available.

3.  **Install System Dependencies**
    On a Debian-based system (like Raspberry Pi OS), install the following:
    ```bash
    sudo apt-get update
    sudo apt-get install -y build-essential pkg-config
    ```

4.  **Configure the Application**
    Edit `src/config.rs` and set the host, ports, and target UPnP renderer name.

5.  **Run the Service**
    ```bash
    cargo run --release
    ```
    The service will start, initialize the players, and begin listening for connections and events.

---
## Configuration

The `src/config.rs` file contains the following settings:

* **`TCP_SERVER_PORT`**: The port for the display board to connect to for state updates (e.g., `5555`).
* **`WEB_SERVER_PORT`**: The port for the web server that serves cached album art and receives UPnP events (e.g., `5556`).
* **`TARGET_RENDERER_NAME`**: The friendly name for the UPnP player if used (e.g., `"Apollo UPNP"`).
