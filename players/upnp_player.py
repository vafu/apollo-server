# players/upnp_player.py
import asyncio
from lxml import etree
from async_upnp_client.aiohttp import AiohttpRequester, AiohttpNotifyServer
from async_upnp_client.client_factory import UpnpFactory
from async_upnp_client.search import async_search
from async_upnp_client.utils import get_local_ip

class UPnPPlayer:
    def __init__(self, renderer_name, session_manager):
        self.name = "UPNP"
        self._renderer_name = renderer_name
        self.state = {"player_state": "stopped", "title": None, "artist": None, "cover_url": None}
        self.lock = asyncio.Lock()
        self._session_manager = session_manager

    def _parse_metadata(self, track_metadata_xml):
        if not track_metadata_xml: return {}
        
        root = etree.fromstring(track_metadata_xml)
        ns = {'dc': 'http://purl.org/dc/elements/1.1/', 'upnp': 'urn:schemas-upnp-org:metadata-1-0/upnp/'}
        
        title = root.xpath("//dc:title/text()", namespaces=ns)
        artist = root.xpath("//upnp:artist/text()", namespaces=ns)
        cover_art = root.xpath("//upnp:albumArtURI/text()", namespaces=ns)
        songid = root.xpath("//@id", namespaces=ns) # Use the item's 'id' attribute

        return {
            "title": title[0] if title else None,
            "artist": artist[0] if artist else None,
            "cover_url": cover_art[0] if cover_art else None,
            "songid": songid[0] if songid else None
        }

    def _event_callback(self, service, state_vars):
        try:
            transport_state = next((var.value for var in state_vars if var.name == 'TransportState'), None)
            if transport_state:
                self._session_manager.update_transport_state(self.name, transport_state)

            metadata_xml = next((var.value for var in state_vars if var.name == 'Metadata'), None)
            if metadata_xml:
                print(metadata_xml)
                new_metadata = self._parse_metadata(metadata_xml)
                self._session_manager.update_metadata(self.name, new_metadata)

        except Exception as e:
            print(f"Error processing UPnP event: {e}")

    async def start(self):
        requester = AiohttpRequester(timeout=10)
        factory = UpnpFactory(requester, non_strict=True)
        info_service_id = 'urn:av-openhome-org:service:Info:1'
        playlist_service_id = 'urn:av-openhome-org:service:Playlist:1'
        target_device = None

        while True:
            try:
                print("UPnP: Searching for devices...")

                async def on_device_found(headers):
                    nonlocal target_device
                    if target_device:
                        return
                    # maybe optimize to find target device and do early return
                    location = headers.get("location")
                    if not location:
                        return

                    try:
                        device = await factory.async_create_device(location)
                        if self._renderer_name.lower() == device.friendly_name.lower():
                            print("UPnP: Target renderer found")
                            target_device = device
                    except Exception as e:
                        print(f"cannot create device at {location} : {e} ")
                        return

                await async_search(async_callback=on_device_found, timeout=5)

                if not target_device:
                    print("UPnP: Target renderer not found in search results. Retrying in 15s.")
                    await asyncio.sleep(15)
                    continue

                source = (get_local_ip(target_device.device_url), 0)
                server = AiohttpNotifyServer(target_device.requester, source)
                await server.async_start_server()

                info_service = target_device.service(info_service_id)
                info_service.on_event = self._event_callback
                playlist_service = target_device.service(playlist_service_id)
                playlist_service.on_event = self._event_callback

                event_handler = server.event_handler
                await asyncio.gather(
                    event_handler.async_subscribe(info_service),
                    event_handler.async_subscribe(playlist_service)
                )

                while True:
                    await asyncio.sleep(60)

            except Exception as e:
                print(f"UPnP Error/Retry: {e}")
                await asyncio.sleep(5)
