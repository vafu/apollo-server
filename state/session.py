# state/session.py
import threading
import asyncio
import requests
from PIL import Image
from io import BytesIO
import os
import hashlib

ART_CACHE_DIR = "/tmp/art_cache/"


class SessionManager:

    def __init__(self, tcp_server, loop):
        self._tcp_server = tcp_server
        self._loop = loop
        self.unified_state = {"player_state": "stopped", "songid": None}
        self._lock = threading.Lock()

    def _broadcast_state(self, state_to_send):
        if self._tcp_server and state_to_send:
            print(f"SESSION: Sending {state_to_send}")
            asyncio.run_coroutine_threadsafe(
                self._tcp_server.broadcast(state_to_send),
                self._loop
            )

    def _getFileName(self, songid, original_url):
        hash_object = hashlib.md5(original_url.encode())
        cache_filename = f"{hash_object.hexdigest()}.jpg"
        cache_filepath = os.path.join(ART_CACHE_DIR, cache_filename)
        return (os.path.exists(cache_filepath), f"/art/{cache_filename}", cache_filepath, cache_filename)

    def _process_and_cache_art(self, songid, original_url):
        (exists, relative_url, cache_filepath,
         cache_filename) = self._getFileName(songid, original_url)
        try:
            if exists:
                state_to_send = None
                print(f"SESSION: Art for songid {songid} found in cache.")
                with self._lock:
                    if self.unified_state.get("cover_url") != relative_url:
                        self.unified_state["cover_url"] = relative_url
                        state_to_send = self.unified_state.copy()
                self._broadcast_state(state_to_send)
                return

            print(f"SESSION: caching {original_url} for {songid}")

            os.makedirs(ART_CACHE_DIR, exist_ok=True)

            image = None
            if original_url.startswith("file://"):
                filepath = original_url[7:] # Strip the "file://" prefix
                print(f"SESSION: Processing art for {songid} from local file: {filepath}")
                image = Image.open(filepath)
            elif original_url.startswith("http"):
                print(f"SESSION: Processing art for {songid} from web URL: {source_url}")
                img_response = requests.get(original_url, timeout=10)
                img_response.raise_for_status()
                image = Image.open(BytesIO(img_response.content))

            resized_img = image.resize((128, 128))
            rgb_img = resized_img.convert("RGB")
            rgb_img.save(cache_filepath, "JPEG", quality=90)

            state_to_send = None
            with self._lock:
                if self.unified_state.get("songid") == songid:
                    self.unified_state["cover_url"] = relative_url
                    print(f"SESSION: cached {relative_url}")
                    state_to_send = self.unified_state.copy()
            self._broadcast_state(state_to_send)
        except Exception as e:
            print(f"SESSION: Failed to process art for songid {songid}: {e}")

    def update_transport_state(self, player_name, transport_state_str):
        transport_state_str = transport_state_str.lower()
        state_to_send = None
        with self._lock:
            if self.unified_state.get("player_state") == transport_state_str:
                return
            print(f"SESSION: {player_name} -> {transport_state_str}")
            self.unified_state["player_state"] = transport_state_str

            if transport_state_str == 'stopped':
                self.unified_state.update(
                    {"title": None, "artist": None, "album": None, "cover_url": None, "songid": None})
            state_to_send = self.unified_state.copy()
        self._broadcast_state(state_to_send)

    def update_metadata(self, player_name, metadata_dict):
        state_to_send = None
        with self._lock:
            new_songid = metadata_dict.get("songid")

            track_changed = not new_songid or not new_songid == self.unified_state.get("songid")
            metadata_changed = metadata_dict.get('cover_url') and not self.unified_state.get('cover_url')

            if not track_changed and not metadata_changed:
                return
            print(f"SESSION: Received new metadata from '{player_name}'.")

            # Set the new metadata
            self.unified_state.update(metadata_dict)
            self.unified_state["player_state"] = "playing"
            original_cover_url = metadata_dict.get("cover_url")
            self.unified_state["cover_url"] = None

            if original_cover_url:
                (exists, relative_url, _, _) = self._getFileName(
                    new_songid, original_cover_url)
                if not exists:
                    threading.Thread(
                        target=self._process_and_cache_art,
                        args=(new_songid, original_cover_url),
                        daemon=True
                    ).start()
                else:
                    self.unified_state["cover_url"] = relative_url
            state_to_send = self.unified_state.copy()

        self._broadcast_state(state_to_send)
