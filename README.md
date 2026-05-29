# Apollo Media State Service

A Rust media state aggregator for the Apollo LED panel client.

The service listens to configured music player sources, normalizes their state into a single JSON shape, processes album art into a local cache, and broadcasts updates to clients over a lightweight TCP protocol.

## Runtime

- Web server: serves cached album art and receives UPnP event callbacks.
- TCP server: broadcasts length-prefixed JSON state to display clients.
- Players: Shairport Sync/AirPlay, UPnP/OpenHome, Sendspin, and an optional mock player.

By default, the service looks for `apollo/server.toml` under the XDG config directory:

- `$XDG_CONFIG_HOME/apollo/server.toml`
- `$HOME/.config/apollo/server.toml` when `XDG_CONFIG_HOME` is unset

If the file is missing, built-in defaults are used. To load a specific file:

```bash
APOLLO_CONFIG=/path/to/config.toml cargo run
```

## Configuration

See `config.example.toml` for all supported options.

Each player has an `enabled` flag:

```toml
[players.shairport]
enabled = false

[players.upnp]
enabled = false

[players.sendspin]
enabled = false
server = "pluto:8095"

[players.mock]
enabled = true
```

## Development

```bash
cargo check
cargo run
```

To see Sendspin crate internals:

```bash
RUST_LOG=sendspin=debug,artfetcher=debug cargo run
```

The default TCP state endpoint is `0.0.0.0:5557`. TCP messages are encoded as a 4-byte big-endian payload length followed by a JSON object:

```json
{
  "player_state": "playing",
  "title": "One More Time",
  "artist": "Daft Punk",
  "album": "Discovery",
  "cover_url": "/art/example.jpg",
  "songid": "101"
}
```
