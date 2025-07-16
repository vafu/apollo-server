# players/shairport_player.py
import asyncio
import os
from lxml import etree
import base64
import hashlib

PLAYER_CACHE_DIR = "/tmp/shairport_art_cache/"

transport_codes = [
    'prsm',
    'paus',
    'pend'
]

transport_state_map = {'prsm': 'playing', 'pend': 'stopped', 'paus': 'paused'}

metadata_codes = [
    'asar',
    'minm',
    'PICT',
    'mden',
    'mdst'
]

codes = transport_codes + metadata_codes


class ShairportPlayer:
    """An asyncio-native player that reads and parses the raw XML stream from the Shairport Sync metadata pipe."""

    def __init__(self, pipe_path, session_manager):
        self.name = "AirPlay"
        self._pipe_path = pipe_path
        self._session_manager = session_manager
        self._buffer = ""  # Buffer is now a string
        self._pipe_fd = None
        self._pipe_closed_event = asyncio.Event()
        self._staged_track_info = {}

    def _save_pict_data(self, songid, image_bytes):
        """Saves raw image bytes to a temporary file and returns its path."""
        try:
            os.makedirs(PLAYER_CACHE_DIR, exist_ok=True)
            filename = f"{hashlib.md5(songid.encode()).hexdigest()}.tmp"
            filepath = os.path.join(PLAYER_CACHE_DIR, filename)

            if os.path.exists(filepath):
                return f"file://{filepath}"

            with open(filepath, "wb") as f:
                f.write(image_bytes)

            return f"file://{filepath}"
        except Exception as e:
            print(f"SHAIRPORT: Failed to save PICT data to temp file: {e}")
            return None

    def _parse_item_xml(self, item_xml_str):
        """Parses a single <item>...</item> block and returns its code and value."""
        root = etree.fromstring(item_xml_str)
        code_hex = root.find('code').text
        code = bytes.fromhex(code_hex).decode(
            'utf-8', 'ignore') if code_hex else ''

        value = None
        data_node = root.find('data')
        if data_node is not None and data_node.text:
            encoding = data_node.get('encoding')
            content = data_node.text.strip()
            if content:
                value = base64.b64decode(
                    content) if encoding == 'base64' else content

        return code, value

    def _process_buffer(self):
        """Processes the buffer and calls session manager with complete metadata."""
        should_commit_metadata = False

        while True:
            start_tag = "<item>"
            end_tag = "</item>"
            start_index = self._buffer.find(start_tag)
            end_index = self._buffer.find(end_tag)


            if start_index != -1 and end_index > start_index:
                full_item_len = end_index + len(end_tag)
                item_xml = self._buffer[start_index:full_item_len]
                self._buffer = self._buffer[full_item_len:]
                try:
                    code, value = self._parse_item_xml(item_xml)
                    print(self._buffer)
                    if code not in codes:
                        continue

                    if code in transport_codes:
                        self._session_manager.update_transport_state(
                            self.name, transport_state_map[code])
                    elif code == 'mdst':
                        self._staged_track_info = {}
                    elif value is not None:
                        self._staged_track_info[code] = value
                        if 'minm' in self._staged_track_info and 'asar' in self._staged_track_info:
                            should_commit_metadata = True

                except Exception as e:
                    print(f"SHAIRPORT: Failed to parse item block: {e}")
                    print(f"SHAIRPORT: Malformed XML chunk was: {item_xml}")
            else:
                break

        if should_commit_metadata:
            print("SHAIRPORT: Title and artist received. Committing metadata.")
            artist = self._staged_track_info.get('asar', b'').decode('utf-8')
            title = self._staged_track_info.get('minm', b'').decode('utf-8')

            cover_url = None
            if 'PICT' in self._staged_track_info:
                songid = f"airplay-{artist}-{title}"
                file_uri = self._save_pict_data(
                    songid, self._staged_track_info['PICT'])
                cover_url = file_uri

            standardized_state = {
                "songid": f"airplay-{artist}-{title}",
                "title": title,
                "artist": artist,
                "cover_url": cover_url
            }

            self._session_manager.update_metadata(
                self.name, standardized_state)

    def _on_pipe_data(self):
        """Synchronous callback executed by the event loop when the pipe has data."""
        try:
            chunk = os.read(self._pipe_fd, 4096)
            if not chunk:
                self._pipe_closed_event.set()
            else:
                self._buffer += chunk.decode('utf-8', 'ignore')
                self._process_buffer()
        except (BlockingIOError, InterruptedError):
            pass  # Expected when no data is ready
        except Exception as e:
            print(f"SHAIRPORT: Error in read callback: {e}")
            self._pipe_closed_event.set()

    async def start(self):
        """Main asyncio task that opens the pipe and registers a non-blocking reader."""
        loop = asyncio.get_running_loop()
        while True:
            try:
                print(f"SHAIRPORT: Opening pipe at {self._pipe_path}...")
                self._pipe_fd = os.open(
                    self._pipe_path, os.O_RDONLY | os.O_NONBLOCK)
                loop.add_reader(self._pipe_fd, self._on_pipe_data)
                print("SHAIRPORT: Pipe reader registered. Waiting for events.")

                await self._pipe_closed_event.wait()
            except Exception as e:
                print(f"SHAIRPORT: Main loop error: {e}")
            finally:
                if self._pipe_fd:
                    loop.remove_reader(self._pipe_fd)
                    os.close(self._pipe_fd)
                    self._pipe_fd = None
                self._pipe_closed_event.clear()
                self._buffer = ""
                print("SHAIRPORT: Cleanup complete. Retrying in 5s.")
                await asyncio.sleep(5)
