import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

run = Path(__file__).resolve().parents[2] / ".local/verification" / sys.argv[1]
env = json.loads((run / "env.json").read_text())
os.environ.clear()
os.environ.update(env)
from PIL import ImageGrab


def command(args, check=True):
    result = subprocess.run(args, env=env, capture_output=True, text=True, timeout=25)
    record = {"time": time.time(), "command": args, "status": result.returncode,
              "stdout": result.stdout, "stderr": result.stderr}
    with (run / "operations.jsonl").open("a") as stream:
        stream.write(json.dumps(record, ensure_ascii=False) + "\n")
    if check and result.returncode:
        raise RuntimeError(record)
    return result.stdout.strip()


window = command(["xdotool", "search", "--name", "^Chronobreak Native IME$"]).splitlines()[0]
scale = float(env["WINIT_X11_SCALE_FACTOR"])


def geometry(target):
    values = command(["xdotool", "getwindowgeometry", "--shell", str(target)])
    return {key: int(value) for key, value in (line.split("=", 1) for line in values.splitlines())}


def snapshot(name):
    time.sleep(.35)
    tree = command(["xwininfo", "-root", "-tree"])
    (run / f"{name}-tree.txt").write_text(tree)
    image = ImageGrab.grab(xdisplay=env["DISPLAY"])
    image.save(run / f"{name}-display.png")
    app = geometry(window)
    def crop(rect, suffix):
        image.crop((rect["X"], rect["Y"], rect["X"] + rect["WIDTH"],
                    rect["Y"] + rect["HEIGHT"])).save(run / f"{name}-{suffix}.png")
    crop(app, "app")
    candidates = []
    for match in re.finditer(r'(0x[0-9a-f]+) "Fcitx5 Input Window"', tree):
        target = match.group(1)
        info = command(["xwininfo", "-id", target])
        rect = geometry(target)
        if "IsViewable" in info and rect["WIDTH"] > 20 and rect["HEIGHT"] > 20:
            candidates.append(rect)
            crop(rect, "candidate")
            command(["xprop", "-id", target, "_NET_WM_PID", "WM_CLASS"])
    log = (run / "app.log").read_text()
    areas = re.findall(r'ime_area logical=Bounds \{ x: ([\d.-]+), y: ([\d.-]+), width: ([\d.-]+), height: ([\d.-]+) \} scale=([\d.]+)', log)
    record = {"app": app, "candidates": candidates, "last_area": areas[-1] if areas else None,
              "model": [line for line in log.splitlines() if line.startswith("model ")],
              "ime": [line for line in log.splitlines() if "input Ime(" in line]}
    (run / f"{name}.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
    print(json.dumps(record, ensure_ascii=False))


action = sys.argv[2]
args = sys.argv[3:]
if action == "init":
    command(["xdotool", "windowmove", window, "40", "40"])
    command(["xdotool", "windowfocus", window])
    time.sleep(.4)
    command(["xdotool", "mousemove", str(40 + round(25*scale)), str(40 + round(95*scale)), "click", "1"])
    time.sleep(.5)
    command(["xdotool", "key", "--clearmodifiers", "Control_L"])
    time.sleep(.4)
    command(["fcitx5-remote", "-o"])
    assert command(["fcitx5-remote", "-n"]) == "pinyin"
    assert command(["fcitx5-remote", "-m", "pinyin"]) == "pinyin"
    reply = command(["gdbus", "call", "--session", "--dest", "org.freedesktop.DBus",
        "--object-path", "/org/freedesktop/DBus", "--method",
        "org.freedesktop.DBus.GetConnectionUnixProcessID", "org.fcitx.Fcitx5"])
    pid = int(re.search(r'uint32 (\d+)', reply).group(1))
    assert pid == json.loads((run / "pids.json").read_text())["fcitx"]
    (run / "isolation.json").write_text(json.dumps({"private_bus_pid_matches": True,
        "session_socket_exists": (run / "r/d").exists(), "engine": "pinyin", "pid": pid}))
    snapshot("initial")
elif action == "type":
    command(["xdotool", "type", "--delay", "110", "--", args[0]])
elif action == "key":
    command(["xdotool", "key", "--clearmodifiers", *args])
elif action == "click":
    rect = geometry(window)
    command(["xdotool", "mousemove", str(rect["X"] + round(float(args[0]) * scale)),
             str(rect["Y"] + round(float(args[1]) * scale)), "click", "1"])
elif action == "wheel":
    command(["xdotool", "click", "--repeat", args[1], "--delay", "150", args[0]])
elif action in ("button-down", "button-up"):
    command(["xdotool", "mousedown" if action == "button-down" else "mouseup", args[0]])
elif action == "move":
    command(["xdotool", "windowmove", window, *args])
elif action == "resize":
    command(["xdotool", "windowsize", window, str(round(float(args[0])*scale)), str(round(float(args[1])*scale))])
elif action == "snap":
    snapshot(args[0])
elif action == "point":
    rect = geometry(window)
    command(["xdotool", "mousemove", str(rect["X"] + round(float(args[0])*scale)),
             str(rect["Y"] + round(float(args[1])*scale))])
else:
    raise ValueError(action)
