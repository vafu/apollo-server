# tcp_server.py
import asyncio
import struct
import json


class TCPServer:
    def __init__(self, host, port):
        self._host = host
        self._port = port
        self._clients = []
        self.state_data = None

    async def _handle_client(self, reader, writer):
        addr = writer.get_extra_info('peername')
        print(f"TCP Server: Accepted connection from {addr}")
        self._clients.append(writer)
        await self.broadcast(self.state_data)
        try:
            # Keep the connection alive by waiting for data that will never come
            while True:
                data = await reader.read(100)
                if not data:
                    break
        except ConnectionResetError:
            print(f"TCP Server: Client {addr} disconnected abruptly.")
        finally:
            print(f"TCP Server: Closing connection for {addr}")
            self._clients.remove(writer)
            writer.close()
            await writer.wait_closed()

    async def broadcast(self, state_data):
        if not state_data:
            return
        self.state_data = state_data
        payload = json.dumps(state_data).encode('utf-8')
        header = struct.pack('>I', len(payload))
        message = header + payload

        for writer in self._clients[:]:
            try:
                writer.write(message)
                await writer.drain()
            except ConnectionResetError:
                print("TCP Server: Failed to broadcast to a client.")

    async def start(self):
        server = await asyncio.start_server(
            self._handle_client, self._host, self._port)
        addr = server.sockets[0].getsockname()
        print(f'TCP Server: Serving on {addr}')
        async with server:
            await server.serve_forever()
