# players/mock_player.py
import time
import threading

class MockPlayer:
    """
    A mock player that simulates state changes internally and reports them
    via a callback, mimicking a real event-driven player.
    """
    def __init__(self, on_state_change_callback):
        self.name = "MOCK"
        self._on_state_change = on_state_change_callback
        self._state_index = 0
        self._lock = threading.Lock()
        self._mock_states = [
            {
                "player_state": "playing", "artist": "Daft Punk", "album": "Discovery",
                "title": "One More Time", "songid": "101"
            },
            {
                "player_state": "paused", "artist": "Daft Punk", "album": "Discovery",
                "title": "One More Time", "songid": "101"
            },
            {
                "player_state": "playing", "artist": "Gorillaz", "album": "Demon Days",
                "title": "Feel Good Inc", "songid": "102"
            },
            {
                "player_state": "stopped", "artist": None, "album": None,
                "title": None, "songid": None
            }
        ]
        
        self._thread = threading.Thread(target=self._run_simulation, daemon=True)
        self._thread.start()

    def _run_simulation(self):
        """Internal method to simulate state changes and fire the callback."""
        # Fire an initial event on startup
        time.sleep(1)
        state = self.get_state()
        self._on_state_change(self.name, state)

        while True:
            # Simulate a state change every 8 seconds
            time.sleep(8)
            
            with self._lock:
                self._state_index = (self._state_index + 1) % len(self._mock_states)
                print(f"MockPlayer: Internal state changed to index {self._state_index}")
            
            # Announce the new state via the callback
            state = self.get_state()
            self._on_state_change(self.name, state)

    def get_state(self):
        """Returns the current mock state in a thread-safe way."""
        with self._lock:
            return self._mock_states[self._state_index]
