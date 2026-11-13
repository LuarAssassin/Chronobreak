import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import sys
import time

project = Path(__file__).resolve().parents[2]
target = Path(os.environ.get("CARGO_TARGET_DIR", project / "target")).resolve()
parser = argparse.ArgumentParser()
parser.add_argument("run")
parser.add_argument("scale", choices=("1.0", "1.5"))
parser.add_argument("--verification-root", type=Path, default=project / ".local/verification")
parser.add_argument("--stack-root", type=Path)
parser.add_argument("--app", type=Path, default=target / "debug/examples/native_ime")
parser.add_argument("--clipboard-reader", type=Path, default=target / "debug/examples/ime_clipboard_reader")
args = parser.parse_args()
if not args.run.isalnum():
    parser.error("run must be alphanumeric")
root = args.verification_root.resolve()
reader = args.clipboard_reader.resolve()
app_binary = args.app.resolve()
stack = (args.stack_root or root / "native-stack/root/usr").resolve()
for label, path in (("Independent clipboard reader", reader), ("Native IME example", app_binary),
                    ("Fcitx5", stack / "bin/fcitx5")):
    if not path.is_file() or not os.access(path, os.X_OK):
        parser.error(f"{label} is required and must be executable: {path}")
run = root / args.run
root.mkdir(parents=True, exist_ok=True)
run.mkdir(mode=0o700)
env = {"PATH": str(stack / "bin") + os.pathsep + os.defpath, "LANG": "C.UTF-8"}
for key, name in [("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"),
                  ("XDG_DATA_HOME", "data"), ("XDG_RUNTIME_DIR", "r"), ("TMPDIR", "tmp")]:
    path = run / name
    path.mkdir(mode=0o700)
    env[key] = str(path)
display = next(number for number in range(200, 230) if not Path(f"/tmp/.X{number}-lock").exists())
env.update(DISPLAY=f":{display}", XAUTHORITY=str(run / "xauthority"),
           DBUS_SESSION_BUS_ADDRESS=f"unix:path={run}/r/d", XMODIFIERS="@im=fcitx",
           GSETTINGS_BACKEND="memory", NO_AT_BRIDGE="1", CHRONOBREAK_TRACE="1",
           WINIT_UNIX_BACKEND="x11", WINIT_X11_SCALE_FACTOR=args.scale,
           LD_LIBRARY_PATH=str(stack / "lib/x86_64-linux-gnu"),
           FCITX_ADDON_DIRS=str(stack / "lib/x86_64-linux-gnu/fcitx5"),
           FCITX_DATA_DIRS=str(stack / "share/fcitx5"),
           XDG_DATA_DIRS=str(stack / "share") + ":/usr/share",
           LIBIME_MODEL_DIRS=str(stack / "lib/x86_64-linux-gnu/libime"))
config = Path(env["XDG_CONFIG_HOME"]) / "fcitx5"
(config / "conf").mkdir(parents=True)
(config / "profile").write_text("[Groups/0]\nName=Private\nDefault Layout=us\nDefaultIM=pinyin\n\n[Groups/0/Items/0]\nName=keyboard-us\nLayout=\n\n[Groups/0/Items/1]\nName=pinyin\nLayout=\n\n[GroupOrder]\n0=Private\n")
(config / "conf/pinyin.conf").write_text('PreeditMode="Commit preview"\n')
(config / "conf/xim.conf").write_text("UseOnTheSpot=True\n")
(config / "config").write_text("[Behavior]\nActiveByDefault=True\n")
Path(env["XAUTHORITY"]).touch(mode=0o600)
subprocess.run(["xauth", "-f", env["XAUTHORITY"]], input=f"add :{display} . {secrets.token_hex(16)}\n",
               text=True, capture_output=True, check=True)
processes = []

def launch(name, args):
    log = open(run / f"{name}.log", "w")
    process = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    processes.append((name, process, log))
    return process

try:
    launch("xvfb", ["Xvfb", env["DISPLAY"], "-screen", "0", "1600x1200x24", "-auth",
                    env["XAUTHORITY"], "-nolisten", "tcp", "-noreset"])
    time.sleep(.6)
    launch("dbus", ["dbus-daemon", "--session", "--nofork", "--address=" + env["DBUS_SESSION_BUS_ADDRESS"]])
    time.sleep(.5)
    launch("fcitx", [str(stack / "bin/fcitx5"), "-D", "--disable", "all", "--enable",
                     "xcb,xim,dbus,keyboard,classicui,pinyin,punctuation", "--ui", "classicui"])
    time.sleep(2)
    assert all(process.poll() is None for _, process, _ in processes)
    app = launch("app", [str(app_binary)])
    time.sleep(2)
    (run / "env.json").write_text(json.dumps(env))
    (run / "env.json").chmod(0o600)
    (run / "pids.json").write_text(json.dumps({name: proc.pid for name, proc, _ in processes}))
    executable = Path(f"/proc/{app.pid}/exe")
    (run / "provenance.json").write_text(json.dumps({"source_head": subprocess.check_output(
        ["git", "-C", str(project), "rev-parse", "HEAD"], text=True).strip() if (project / ".git").exists() else None,
        "source_root": str(project), "runtime_path": str(app_binary),
        "runtime_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
        "clipboard_reader_path": str(reader), "clipboard_reader_sha256": hashlib.sha256(reader.read_bytes()).hexdigest(),
        "backend": "X11/XIM", "scale": args.scale, "wm": None}))
    sources = [project / "Cargo.toml", project / "Cargo.lock", *sorted((project / "src").glob("*")),
               *sorted((project / "examples").glob("*.rs")),
               *sorted(Path(__file__).parent.glob("*.py"))]
    provenance = {str(path.relative_to(project)): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in sources if path.is_file()}
    (run / "source-hashes.json").write_text(json.dumps(provenance, indent=2))
    for name, process, _ in processes:
        proc = Path(f"/proc/{process.pid}")
        (run / f"{name}-maps.txt").write_text((proc / "maps").read_text())
    with (run / "exercise-command.log").open("w") as log:
        subprocess.run([sys.executable, str(Path(__file__).with_name("exercise.py")), str(run)],
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    with (run / "clipboard-copy.log").open("w") as log:
        subprocess.run([sys.executable, str(Path(__file__).with_name("actions.py")), str(run),
                        "key", "ctrl+a", "ctrl+c"], stdout=log, stderr=subprocess.STDOUT,
                       check=True, timeout=15)
    result = subprocess.run([str(reader)], env=env, capture_output=True, text=True, timeout=15)
    final = json.loads((run / "19-after-remove-second.json").read_text())
    expected = json.loads(final["model"][-1].split("=", 1)[1])
    record = {"exit": result.returncode, "text": result.stdout.removesuffix("\n"),
              "expected_last_signal": expected, "stderr": result.stderr}
    record["matches"] = result.returncode == 0 and record["text"] == expected
    (run / "clipboard-reader.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
    if not record["matches"]:
        raise RuntimeError("Independent clipboard reader did not match the final Signal")
    print((run / "exercise-command.log").read_text().strip())
finally:
    result = {}
    for name, process, log in reversed(processes):
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            result[name] = process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            result[name] = process.wait()
        log.close()
    (run / "cleanup.json").write_text(json.dumps(result))
