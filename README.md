# Apollo Media State Service

A multi-source media state aggregator and processor, designed to be a single source of truth for a [client display board](https://github.com/vafu/apollo-led-panel).

---
## Overview

The Apollo service is a Python application that runs on a central server (e.g., a Raspberry Pi running Moode Audio). Its primary purpose is to listen to various music player daemons, consolidate their state into a single, unified format, and push real-time updates to a client device.

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

2.  **Create a Virtual Environment**
    ```bash
    python3 -m venv .venv
    source .venv/bin/activate
    ```

3.  **Install System Dependencies**
    The service relies on libraries that may need system-level build tools. On a Debian-based system (like Raspberry Pi OS), install the following:
    ```bash
    sudo apt-get update
    sudo apt-get install -y build-essential python3-dev libxml2-dev libxslt1-dev libjpeg-dev
    ```

4.  **Install Python Dependencies**
    Create a `requirements.txt` file with the following content:
    ```
    # requirements.txt
    waitress
    flask
    lxml
    Pillow
    requests
    async_upnp_client
    aiohttp
    ```
    Then, install the requirements:
    ```bash
    pip install -r requirements.txt
    ```

5.  **Configure the Application**
    Adjust `config.py` file to your needs.

7.  **Run the Service**
    ```bash
    python app.py
    ```
    The service will start, initialize the players, and begin listening for connections and events.

---
