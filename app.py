# app.py
import asyncio
import config
import threading
from tcp_server import TCPServer
from state.session import SessionManager
from players.upnp_player import UPnPPlayer
from players.shairport_player import ShairportPlayer
from web.endpoints import app  # Still need Flask for the /art endpoint


def run_flask():
    app.run(host=config.WEB_SERVER_HOST,
            port=config.WEB_SERVER_PORT)


async def main(loop):
    tcp_server = TCPServer(
        config.WEB_SERVER_HOST,
        config.TCP_SERVER_PORT,
    )
    session_manager = SessionManager(tcp_server, loop)

    upnp_player = UPnPPlayer(
        renderer_name=config.TARGET_RENDERER_NAME,
        session_manager=session_manager
    )

    shairport_player = ShairportPlayer(
        session_manager=session_manager,
        pipe_path="/tmp/shairport-sync-metadata"
    )

    flask_thread = threading.Thread(target=run_flask, daemon=True)
    flask_thread.start()

    await asyncio.gather(
        tcp_server.start(),
        upnp_player.start(),
        shairport_player.start()
    )

if __name__ == '__main__':
    try:
        main_loop = asyncio.get_event_loop()
        main_loop.run_until_complete(main(main_loop))
    except KeyboardInterrupt:
        print("Service stopped.")
