#!/usr/bin/env python3
"""Start the local preview once, then open it in the default browser."""
from pathlib import Path
from urllib.error import URLError, HTTPError
from urllib.request import ProxyHandler, build_opener
import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import webbrowser

ROOT = Path(__file__).resolve().parent
OPENER = build_opener(ProxyHandler({}))


def probe(port):
    """Return own, free, or foreign. Never open an unrelated listener."""
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.3):
            pass
    except OSError:
        return "free"
    try:
        with OPENER.open(f"http://127.0.0.1:{port}/healthz", timeout=0.6) as response:
            result = json.loads(response.read(4096))
        return "own" if isinstance(result, dict) and result.get("app") == "forever-companion" else "foreign"
    except (OSError, URLError, HTTPError, ValueError):
        return "foreign"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-browser", action="store_true", help="Start/check the server without opening a browser")
    args = parser.parse_args()
    try:
        port = int(os.environ.get("COMPANION_PORT", "4317"))
        if not 1 <= port <= 65535:
            raise ValueError()
    except ValueError:
        raise SystemExit("COMPANION_PORT must be a whole number between 1 and 65535.")
    status = probe(port)
    if status == "foreign":
        raise SystemExit(f"Port {port} is occupied by another application. Set COMPANION_PORT to another port and try again. No browser was opened.")
    if status == "free":
        process = None
        use_systemd = False
        if shutil.which("systemd-run") and shutil.which("systemctl"):
            try:
                # Only inspect availability; never print the user manager's environment.
                use_systemd = subprocess.run(
                    ["systemctl", "--user", "show-environment"],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    stdin=subprocess.DEVNULL, timeout=2,
                ).returncode == 0
            except (OSError, subprocess.TimeoutExpired):
                pass
        if use_systemd:
            unit = f"forever-companion-preview-{port}"
            diagnostics = f"journalctl --user -u {unit} --no-pager -n 30"
            command = [
                "systemd-run", "--user", "--collect", "--quiet", "--no-ask-password",
                f"--unit={unit}", "--description=Forever Companion Preview",
                f"--property=WorkingDirectory={ROOT}",
                f"--setenv=COMPANION_PORT={port}",
                "--", sys.executable, str(ROOT / "tools" / "serve.py"),
            ]
            try:
                result = subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=3)
            except (OSError, subprocess.TimeoutExpired) as error:
                raise SystemExit(f"Could not start the companion user service: {error}. Check {diagnostics}.")
            if result.returncode and probe(port) != "own":
                detail = result.stderr.strip()[:500] or "systemd-run failed"
                raise SystemExit(f"Could not start the companion user service: {detail}. Check {diagnostics}.")
        else:
            # A transient user service survives SSH session cleanup. Direct process
            # launch remains available on systems without a reachable user manager.
            cache = Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))) / "forever-companion"
            cache.mkdir(parents=True, exist_ok=True)
            log = cache / "preview-server.log"
            diagnostics = str(log)
            with log.open("ab") as output:
                process = subprocess.Popen([sys.executable, str(ROOT / "tools" / "serve.py")], cwd=ROOT, stdout=output, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, start_new_session=True)
        deadline = time.monotonic() + 4.5
        while time.monotonic() < deadline:
            if probe(port) == "own":
                break
            if process is not None and process.poll() is not None:
                raise SystemExit(f"The companion server exited. See {diagnostics}.")
            time.sleep(0.15)
        else:
            raise SystemExit(f"The companion server did not become ready. See {diagnostics}.")
    url = f"http://127.0.0.1:{port}/"
    print(f"Forever Companion ready: {url}", flush=True)
    if not args.no_browser:
        if shutil.which("xdg-open"):
            try:
                result = subprocess.run(["xdg-open", url], stdin=subprocess.DEVNULL, timeout=15)
            except subprocess.TimeoutExpired:
                raise SystemExit(f"The browser did not respond. The app is ready at {url}.")
            if result.returncode:
                raise SystemExit(f"The app is ready. Open {url} in your browser.")
        elif not webbrowser.open(url):
            raise SystemExit(f"The app is ready. Open {url} in your browser.")


if __name__ == "__main__":
    main()
