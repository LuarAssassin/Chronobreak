"""Isolated X11 synthetic workbench acceptance; never touches the user's clipboard."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
from PIL import ImageGrab

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--app", type=Path, required=True)
parser.add_argument("--owner", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
project = Path(__file__).resolve().parents[1]
output = args.output.resolve()
if not output.is_relative_to(project / ".local/verification"):
    parser.error("output must be inside this worktree's .local/verification")
for binary in (args.app, args.owner):
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"missing executable: {binary}")
output.mkdir(parents=True, exist_ok=False)
env = {"PATH": os.environ.get("PATH", os.defpath), "LANG": "C.UTF-8",
       "WINIT_UNIX_BACKEND": "x11", "CHRONOBREAK_WORKBENCH_VERIFY": "1"}
processes = []
checks = []
app = None
log = None

def require(condition, name):
    checks.append({"name": name, "passed": bool(condition)})
    if not condition:
        raise RuntimeError(name)

def wait(predicate, label, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.05)
    raise RuntimeError("timeout: " + label)

def command(*parts):
    result = subprocess.run(list(map(str, parts)), env=env, capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise RuntimeError(result.stderr)
    return result.stdout.strip()

def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)

def start(name, path):
    global app, log
    stop(app)
    if log:
        log.close()
    log = (output / f"{name}.log").open("w")
    app = subprocess.Popen([str(args.app.resolve()), "--path", str(path)], env=env, stdout=log, stderr=log)
    processes.append(app)
    def window():
        result = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)], env=env, text=True, capture_output=True)
        return result.stdout.strip().splitlines()[0] if result.returncode == 0 else None
    found = wait(window, "own application window")
    command("xdotool", "windowfocus", found)
    return found

def probe():
    before = len(Path(log.name).read_text())
    command("xdotool", "key", "F9")
    def read():
        log.flush()
        text = Path(log.name).read_text()
        new = text[before:]
        lines = [line.removeprefix("workbench-probe ") for line in new.splitlines() if line.startswith("workbench-probe ")]
        return json.loads(lines[-1]) if lines else None
    return wait(read, "fresh semantic probe")

def ready():
    def complete():
        value = probe()
        return value if not value["busy"] else None
    return wait(complete, "background completion")

def named(state, name):
    return next(node for node in state["nodes"] if node["name"] == name)

def click(name):
    bounds = named(probe(), name)["bounds"]
    command("xdotool", "mousemove", int(bounds[0] + min(100, bounds[2] / 2)), int(bounds[1] + bounds[3] / 2), "click", "1")
    time.sleep(.15)

def capture(name):
    ImageGrab.grab(xdisplay=env["DISPLAY"]).save(output / f"{name}.png")

try:
    display_file = (output / "display").open("w")
    xvfb_log = (output / "xvfb.log").open("w")
    xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(display_file.fileno()), "-screen", "0", "1024x768x24", "-nolisten", "tcp"], pass_fds=(display_file.fileno(),), stdout=xvfb_log, stderr=xvfb_log)
    processes.append(xvfb)
    number = wait(lambda: (output / "display").read_text().strip(), "isolated X display")
    env["DISPLAY"] = ":" + number
    path = output / "document.json"
    start("first", path)
    require("Empty workbench" in ready()["status"], "missing file opens honest empty state")
    click("load-fixture")
    initial = probe()
    require(initial["source_bytes"] == 82, "public synthetic source loaded")
    root_before = named(initial, "knowledge-notes")
    capture("light-fixture")
    click("knowledge-notes")
    collapsed = probe()
    require(not named(collapsed, "knowledge-notes")["expanded"], "root collapses")
    require(not any(node["name"] == "knowledge-water" for node in collapsed["nodes"]), "descendants leave visible semantics")
    click("theme-switch")
    dark = probe()
    require(dark["theme"] == "Dark" and named(dark, "knowledge-notes")["id"] == root_before["id"], "theme and collapse preserve mounted identity")
    require(named(dark, "knowledge-notes")["selected"], "selection survives theme switch")
    click("save-document")
    require("Saved snapshot" in ready()["status"], "real asynchronous save finishes")
    saved = json.loads(path.read_text())
    require(saved["source"] == "Synthetic notes — 离线示例\r\n\nPlants need light. 🌱\nWater supports growth.\n", "exact CRLF and Unicode source on disk")
    require(not saved["nodes"][0]["expanded"], "collapsed state saved")
    capture("dark-collapsed-saved")
    start("restart", path)
    restored = ready()
    require("Restored" in restored["status"] and not named(restored, "knowledge-notes")["expanded"], "real process restart restores collapsed document")
    capture("restored")
    source = "Pasted synthetic 中文\r\n\n" + "🌱 exact line\n" * 20
    owner_log = (output / "clipboard-owner.log").open("w")
    owner = subprocess.Popen([str(args.owner.resolve())], env=env, stdin=subprocess.PIPE, stdout=owner_log, stderr=owner_log, text=True)
    processes.append(owner)
    owner.stdin.write(source)
    owner.stdin.close()
    wait(lambda: "ready" in (output / "clipboard-owner.log").read_text(), "independent clipboard owner")
    click("paste-source")
    pasted = ready()
    require(pasted["source_bytes"] == len(source.encode()) and named(pasted, "knowledge-notes")["id"] == named(restored, "knowledge-notes")["id"], "explicit paste preserves previous valid tree")
    require(json.loads(path.read_text()) == saved, "paste does not implicitly save or replace valid document")
    click("offline-organize")
    require(named(probe(), "knowledge-source")["expanded"], "explicit offline organization replaces valid tree")
    capture("pasted-line-tree")
    command("xdotool", "mousemove", "650", "400", "click", "5", "click", "5", "click", "5")
    scrolled = probe()
    require(named(scrolled, "knowledge-source")["bounds"][1] < 192, "real wheel scroll changes tree geometry")
    capture("scrolled")
    click("save-document")
    ready()
    require(json.loads(path.read_text())["source"] == source, "pasted source saved byte-for-byte")
    start("pasted-restart", path)
    require(ready()["source_bytes"] == len(source.encode()), "pasted document restored after restart")
    bad_path = output / "missing-parent" / "document.json"
    start("failed-save", bad_path)
    ready()
    click("load-fixture")
    failed_before = named(probe(), "knowledge-notes")["id"]
    click("save-document")
    failed = ready()
    require("Save failed" in failed["status"] and named(failed, "knowledge-notes")["id"] == failed_before, "real save failure retains valid in-memory tree")
    capture("save-error")
finally:
    for process in reversed(processes):
        stop(process)
    if log:
        log.close()
    (output / "checks.json").write_text(json.dumps(checks, indent=2))
print(json.dumps({"passed": len(checks), "output": str(output)}))
