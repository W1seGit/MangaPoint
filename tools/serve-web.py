"""A static file server for the built website, with the right content types.

    python tools/serve-web.py [--dir web/dist] [--port 8080]

Python's own server already knows most types; the ones a WebAssembly site
needs are set here so the result matches a real static host. Files are served
without caching headers that would hide a rebuild.
"""
import argparse
import functools
import http.server
import mimetypes
import os

mimetypes.add_type("application/wasm", ".wasm")
mimetypes.add_type("text/javascript", ".js")
mimetypes.add_type("text/javascript", ".mjs")
mimetypes.add_type("application/manifest+json", ".webmanifest")


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      ".wasm": "application/wasm", ".js": "text/javascript",
                      ".webmanifest": "application/manifest+json"}

    def send_head(self):
        # The app is one page: /export, /settings and /read/... are the page itself.
        path = self.translate_path(self.path.split("?", 1)[0].split("#", 1)[0])
        if not os.path.exists(path) and "." not in os.path.basename(path):
            self.path = "/index.html"
        return super().send_head()

    def end_headers(self):
        # Revalidate every time, so a rebuild shows up; the service worker has its own cache.
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def log_message(self, fmt, *args):  # quiet unless asked
        if os.environ.get("SERVE_WEB_LOG"):
            super().log_message(fmt, *args)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", default="web/dist")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()
    handler = functools.partial(Handler, directory=args.dir)
    with http.server.ThreadingHTTPServer((args.host, args.port), handler) as server:
        print(f"Serving {args.dir} on http://{args.host}:{args.port}/  (Ctrl+C to stop)", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
