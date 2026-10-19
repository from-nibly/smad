#!/usr/bin/env python3
"""Launch the mapping POC on the current Wayland display, with private bus/state/socket.
Run under nix-shell after building smad and smad-cli. Closing the window ends its bus.
No installed browser or sync service is stopped or changed.
"""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
fixture = ROOT / "tests/fixtures/markdown/source-mapping.md"
run = Path(tempfile.mkdtemp(prefix="smad-markdown-mapping-", dir="/tmp"))
env = os.environ.copy()
keys = {"HYPRLAND_INSTANCE_SIGNATURE", "WAYLAND_DISPLAY", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP"}
manager = dict(line.split("=", 1) for line in subprocess.check_output(
    ["systemctl", "--user", "show-environment"], text=True).splitlines() if "=" in line)
for key in keys:
    env.pop(key, None)
    if key in manager:
        env[key] = manager[key]
assert env.get("XDG_SESSION_TYPE") == "wayland", "This visible preview expects Wayland"
wayland = Path(env["WAYLAND_DISPLAY"])
if not wayland.is_absolute():
    wayland = Path(env["XDG_RUNTIME_DIR"]) / wayland
assert wayland.is_socket()
for key in list(env):
    if key.startswith(("HERDR_", "ZELLIJ_", "SMAD_", "LAVISH_BROWSER_")):
        del env[key]
for key in ("LD_LIBRARY_PATH", "LIBGL_DRIVERS_PATH", "__EGL_VENDOR_LIBRARY_FILENAMES", "GBM_BACKENDS_PATH", "GTK_IM_MODULE"):
    env.pop(key, None)
for directory in ("state", "cache", "config", "data", "runtime"):
    (run / directory).mkdir(mode=0o700)
needles = ("SECOND source", "second inline", "STRONG TARGET", "HIGHLIGHT ONLY", "second indented", "second = 2", "SELECT THIS ROW", "B --> C")
lines = fixture.read_text().splitlines()
ranges = ",".join(str(next(i for i, line in enumerate(lines, 1) if needle in line)) for needle in needles)
env.update({
    "SMAD_MARKDOWN_MAPPING_POC": "1", "SMAD_MARKDOWN_MAPPING_LINES": ranges,
    "SMAD_SOCKET": str(run / "control.sock"),
    "SMAD_EXECUTABLE": str(ROOT / "target/debug/smad"),
    "XDG_STATE_HOME": str(run / "state"), "XDG_CACHE_HOME": str(run / "cache"),
    "XDG_CONFIG_HOME": str(run / "config"), "XDG_DATA_HOME": str(run / "data"),
    "XDG_RUNTIME_DIR": str(run / "runtime"), "WAYLAND_DISPLAY": str(wayland),
    "TMPDIR": str(run), "GDK_BACKEND": "wayland", "GSK_RENDERER": "cairo", "GTK_USE_PORTAL": "0", "GTK_A11Y": "none",
})
(run / "bus.conf").write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
with (run / "preview.log").open("w") as log:
    launcher = subprocess.Popen([
        "/usr/bin/dbus-run-session", "--dbus-daemon=/usr/bin/dbus-daemon",
        f"--config-file={run / 'bus.conf'}", "--", env["SMAD_EXECUTABLE"],
    ], env=env, stdout=log, stderr=log, start_new_session=True)

def state():
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(2)
        client.connect(env["SMAD_SOCKET"])
        client.sendall(json.dumps({"protocol_version": 2, "request_id": "mapping-preview", "command": {"type": "inspect_state"}}).encode() + b"\n")
        return json.loads(client.makefile("rb").readline())["state"]

for _ in range(150):
    if launcher.poll() is not None:
        raise RuntimeError(f"Preview exited; inspect {run / 'preview.log'}")
    try:
        snapshot = state()
        break
    except (ConnectionError, FileNotFoundError, TimeoutError):
        time.sleep(0.1)
else:
    raise RuntimeError(f"Preview did not start; inspect {run / 'preview.log'}")
result = json.loads(subprocess.check_output([
    str(ROOT / "target/debug/smad-axi"), "open", "md://" + str(fixture) + "?highlight=" + ranges, "--json",
], env=env, text=True))
assert result["document"]["renderer"] == "markdown", result
snapshot = state()
receipt = {"run_directory": str(run), "launcher_pid": launcher.pid, "browser_pid": snapshot["process_id"], "socket": env["SMAD_SOCKET"], "preset_lines": ranges}
evidence = ROOT / "target/markdown-poc/mapping-preview.json"
evidence.parent.mkdir(parents=True, exist_ok=True)
evidence.write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps(receipt, indent=2))
