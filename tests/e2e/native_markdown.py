#!/usr/bin/env python3
"""Run the opt-in native GTK reader test in an isolated X server, not the live desktop.

Usage: nix-shell --run 'python3 tests/e2e/native_markdown.py'
"""
import http.server
import json
import os
from pathlib import Path
import select
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / "target/markdown-poc"
EVIDENCE.mkdir(parents=True, exist_ok=True)

# The application ID is shared with production: a private bus is mandatory.
if not os.environ.get("SMAD_TEST_PRIVATE_BUS"):
    daemon = Path("/usr/bin/dbus-daemon")
    if not daemon.exists():
        daemon = Path(shutil.which("dbus-daemon"))
    env = os.environ.copy()
    env["SMAD_TEST_PRIVATE_BUS"] = "1"
    # No service directories: never activate desktop portals against the user's session.
    with tempfile.TemporaryDirectory(prefix="smad-test-bus-") as directory:
        config = Path(directory) / "bus.conf"
        config.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        subprocess.run(["dbus-run-session", f"--dbus-daemon={daemon}", f"--config-file={config}", "--", sys.executable, __file__], env=env, check=True)
    sys.exit(0)


def request(path, command):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(5)
        client.connect(path)
        client.sendall(json.dumps({"protocol_version": 2, "request_id": "markdown-poc", "command": command}).encode() + b"\n")
        response = client.makefile("rb").readline()
    value = json.loads(response)
    if value["status"] == "error":
        raise RuntimeError(value)
    return value


def wait_for(check, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (ConnectionError, FileNotFoundError):
            pass
        time.sleep(0.1)
    raise RuntimeError("Timed out waiting for native document state")


def application_smoke(env, temporary):
    subprocess.run(["cargo", "build", "-p", "smad", "-p", "smad-cli"], cwd=ROOT, env=env, check=True)
    env = env.copy()
    for key in list(env):
        if key.startswith("HERDR_") or key.startswith("ZELLIJ_"):
            del env[key]
    env["SMAD_SOCKET"] = str(Path(temporary) / "browser.sock")
    env["SMAD_EXECUTABLE"] = str(ROOT / "target/debug/smad")
    path = env["SMAD_SOCKET"]
    with (EVIDENCE / "application.log").open("w") as log:
        app = subprocess.Popen([env["SMAD_EXECUTABLE"]], env=env, cwd=ROOT, stdout=log, stderr=log)
        try:
            state = lambda: request(path, {"type": "inspect_state"})["state"]
            wait_for(state)
            rss_line = lambda: next(line for line in Path(f"/proc/{app.pid}/status").read_text().splitlines() if line.startswith("VmRSS:"))
            baseline_rss = rss_line()
            cli = [str(ROOT / "target/debug/smad-axi"), "open", "md://" + str(ROOT / "tests/fixtures/markdown/reader.md"), "--json"]
            subprocess.run(cli, env=env, check=True)
            wait_for(lambda: any(d["kind"] == "markdown" and d["lifecycle"] == "ready" for p in state()["projects"] for d in p["documents"]))
            subprocess.run(cli, env=env, check=True)
            native = state()
            assert sum(len(p["documents"]) for p in native["projects"]) == 1
            # Real process tree: Markdown must not create a WebKit web process.
            processes = subprocess.check_output(["ps", "-eo", "pid=,ppid=,comm="], text=True)
            descendants = {app.pid}
            rows = [line.split(None, 2) for line in processes.splitlines()]
            for _ in range(8):
                descendants.update(int(pid) for pid, parent, _ in rows if int(parent) in descendants)
            assert not any(int(pid) in descendants and name.startswith("WebKitWeb") for pid, _, name in rows)
            rss = rss_line()
            warning_open = subprocess.run([str(ROOT / "target/debug/smad-axi"), "open", "md://" + str(ROOT / "tests/fixtures/markdown/features.md"), "--json"], env=env, check=True, capture_output=True, text=True)
            warning_result = json.loads(warning_open.stdout)
            assert warning_result["document"]["state"] == "ready_with_warnings"
            assert warning_result["warnings"] and "1 diagram error" in warning_result["warnings"][0]
            key = {"kind": "herdr", "session_name": "poc", "workspace_id": "work", "tab_id": "tab-1"}
            request(path, {"type": "open_markdown", "project": {"key": key, "label": "Native POC"}, "source_file": str(ROOT / "tests/fixtures/markdown/next.md"), "fragment": "next-steps"})
            before = state()
            request(path, {"type": "rename_herdr_project", "session_name": "poc", "workspace_id": "work", "tab_id": "tab-1", "name": "Renamed POC"})
            request(path, {"type": "select_herdr_project", "session_name": "poc", "workspace_id": "work", "tab_id": "tab-1"})
            assert state()["presentation_count"] == before["presentation_count"]

            class Review(http.server.BaseHTTPRequestHandler):
                def do_GET(self):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/html")
                    self.end_headers()
                    self.wfile.write(b"<!doctype html><title>Review still works</title><h1>Existing WebKit review</h1>")

                def log_message(self, *_):
                    pass

            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Review)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                request(path, {"type": "open_url", "project": {"key": key, "label": "Renamed POC"}, "source_file": "/tmp/lavish-poc-review.html", "url": f"http://127.0.0.1:{server.server_port}/session/poc"})
                wait_for(lambda: any(d["kind"] == "lavish_review" and d["lifecycle"] == "ready" for p in state()["projects"] for d in p["documents"]))
                final = state()
                (EVIDENCE / "application-state.json").write_text(json.dumps(final, indent=2) + "\n")
                (EVIDENCE / "memory.txt").write_text(f"Empty application: {baseline_rss}\nNative-only application: {rss}\nNo descendant WebKitWeb process before opening review fixture.\n")
                print(f"Application smoke passed: native dedup, HerdR no-raise rename/select, review Ready. Native-only {rss}")
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
        finally:
            app.terminate()
            app.wait(timeout=10)


with tempfile.TemporaryDirectory(prefix="lavish-markdown-test-") as temporary:
    read_fd, write_fd = os.pipe()
    with (EVIDENCE / "xvfb.log").open("w") as log:
        server = subprocess.Popen(
            ["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1280x1024x24", "-nolisten", "tcp"],
            pass_fds=(write_fd,), stdout=log, stderr=log,
        )
        os.close(write_fd)
        try:
            if not select.select([read_fd], [], [], 10)[0]:
                raise RuntimeError("Xvfb did not report a display within 10 seconds")
            display = os.read(read_fd, 32).decode().strip()
            if not display.isdigit():
                raise RuntimeError(f"Invalid Xvfb display: {display!r}")
            env = os.environ.copy()
            for key in ("LD_LIBRARY_PATH", "LIBGL_DRIVERS_PATH", "__EGL_VENDOR_LIBRARY_FILENAMES", "GBM_BACKENDS_PATH", "GTK_IM_MODULE"):
                env.pop(key, None)
            runtime = Path(temporary) / "runtime"
            runtime.mkdir(mode=0o700)
            env.update({
                "DISPLAY": f":{display}", "GDK_BACKEND": "x11", "GSK_RENDERER": "cairo",
                "GTK_A11Y": "none", "XDG_STATE_HOME": temporary,
                "XDG_RUNTIME_DIR": str(runtime), "GTK_USE_PORTAL": "0",
                "WEBKIT_DISABLE_COMPOSITING_MODE": "1",
                "XDG_CONFIG_HOME": temporary, "XDG_CACHE_HOME": temporary,
                "SMAD_MARKDOWN_SCREENSHOT": str(EVIDENCE / "reader.png"),
            })
            if mesa := env.get("LAVISH_TEST_MESA"):
                env["__EGL_VENDOR_LIBRARY_FILENAMES"] = str(Path(mesa) / "share/glvnd/egl_vendor.d/50_mesa.json")
                env["LIBGL_DRIVERS_PATH"] = str(Path(mesa) / "lib/dri")
            env["LIBGL_ALWAYS_SOFTWARE"] = "1"
            env["WEBKIT_DISABLE_DMABUF_RENDERER"] = "1"
            command = ["cargo", "test", "-p", "smad", "--bin", "smad", "--", "--ignored", "--test-threads=1", "--nocapture"]
            result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
            (EVIDENCE / "native-test.log").write_text(result.stdout)
            print(result.stdout, end="")
            result.check_returncode()
            application_smoke(env, temporary)
        finally:
            os.close(read_fd)
            server.terminate()
            server.wait(timeout=10)
