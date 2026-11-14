"""Start a private headless Weston/Xwayland server, never the host desktop."""
import argparse
import os
from pathlib import Path
import signal
import subprocess
import time
import tempfile
import shutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--nested-x11", action="store_true")
parser.add_argument("--renderer", choices=("pixman", "gl"), default="pixman")
parser.add_argument("command", nargs=argparse.REMAINDER)
args = parser.parse_args()
root = args.root.resolve()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=False)
runtime = Path(tempfile.mkdtemp(prefix="chronobreak-x11-"))
libs = root / "usr/lib/x86_64-linux-gnu"
modules = list((libs / "libweston-14").glob("*.so")) + list((libs / "weston").glob("*.so"))
env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), LD_LIBRARY_PATH=f"{libs}:{libs / 'weston'}", WESTON_MODULE_MAP=";".join(f"{path.name}={path}" for path in modules), WESTON_DATA_DIR=str(root / "usr/share/weston"))
if not args.nested_x11:
    env.pop("DISPLAY", None)
env.pop("WAYLAND_DISPLAY", None)
command = args.command[1:] if args.command and args.command[0] == "--" else args.command
config = output / "weston.ini"
config.write_text(f"[shell]\nclient={root / 'usr/libexec/weston-desktop-shell'}\nbackground-image=\n[input-method]\npath={root / 'usr/libexec/weston-keyboard'}\n")
with (output / "weston.log").open("w") as log:
    server = subprocess.Popen([str(root / "usr/bin/weston"), "--backend=x11" if args.nested_x11 else "--backend=headless", f"--renderer={args.renderer}", "--width=1024", "--height=900", f"--config={config}", "--idle-time=0", "--xwayland", "--socket=qualification"], env=env, stdout=log, stderr=log, start_new_session=True)
try:
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        text = (output / "weston.log").read_text()
        display = next((line.split("xserver listening on display ")[1].strip() for line in text.splitlines() if "xserver listening on display " in line), None)
        if display:
            break
        if server.poll() is not None:
            raise RuntimeError("private compositor exited; inspect weston.log")
        time.sleep(.1)
    else:
        raise RuntimeError("private Xwayland display timeout")
    if args.nested_x11:
        env["CHRONOBREAK_INPUT_DISPLAY"] = os.environ["DISPLAY"]
        env["CHRONOBREAK_COMPOSITOR_PID"] = str(server.pid)
    env["DISPLAY"] = display
    env["WAYLAND_DISPLAY"] = "qualification"
    env["CHRONOBREAK_ISOLATED_X11"] = "1"
    subprocess.run(command, env=env, check=True, timeout=180)
finally:
    if server.poll() is None:
        os.killpg(server.pid, signal.SIGTERM)
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(server.pid, signal.SIGKILL)
            server.wait(timeout=5)
    shutil.rmtree(runtime)
