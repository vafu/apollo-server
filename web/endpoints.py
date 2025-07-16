# web/endpoints.py
from flask import Flask, send_from_directory

app = Flask(__name__)
ART_CACHE_DIR = "/tmp/art_cache"


@app.route('/art/<filename>')
def get_cached_art(filename):
    print(f"WEB: Serving cached art: {filename}")
    return send_from_directory(ART_CACHE_DIR, filename, mimetype='application/octet-stream')
