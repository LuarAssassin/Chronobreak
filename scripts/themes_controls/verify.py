"""私有 Xvfb/XTEST 主题回归；预登记阈值见 docs/themes-controls.md。"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--app", required=True, type=Path)
parser.add_argument("--reader", required=True, type=Path)
parser.add_argument("--output", required=True, type=Path)
parser.add_argument("--source-head")
args = parser.parse_args()
try:
    from PIL import ImageGrab, __version__ as pillow_version
except ImportError:
    parser.error("system prerequisite missing: Pillow")
for binary in (args.app, args.reader):
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"required tracked executable missing: {binary}")
for name in ("Xvfb", "xdotool", "fc-match"):
    if not shutil.which(name):
        parser.error(f"system prerequisite missing: {name}")
PROJECT = Path(__file__).resolve().parents[2]
output = args.output.resolve()
if not output.is_relative_to(PROJECT / ".local/verification"):
    parser.error("output must be inside this worktree's .local/verification")
if args.source_head and not re.fullmatch(r"[0-9a-f]{40}", args.source_head):
    parser.error("source-head must be a full commit SHA")
output.mkdir(parents=True, exist_ok=False)
for folder in ("home", "runtime", "cache", "tmp"):
    (output / folder).mkdir(mode=0o700)
env = {"PATH": os.environ.get("PATH", os.defpath), "LANG": "C.UTF-8",
       "HOME": str(output / "home"), "XDG_RUNTIME_DIR": str(output / "runtime"),
       "XDG_CACHE_HOME": str(output / "cache"), "TMPDIR": str(output / "tmp"),
       "WINIT_UNIX_BACKEND": "x11", "WGPU_BACKEND": "vulkan", "CHRONOBREAK_TRACE": "1"}
checks, measurements, processes = [], [], []
TOLERANCE = 2
PALETTES = {
    "Light": {"background": "F7FAFC", "surface": "FFFFFF", "raised": "EDF5F8",
              "text": "182B36", "muted": "4C626D", "primary": "B9E3F0",
              "on_primary": "173541", "focus": "00738A", "error": "B42318",
              "success": "247342", "selection": "D9EFF7", "accent": "F4D35E", "on_accent": "342B0D"},
    "Dark": {"background": "111318", "surface": "191D23", "raised": "242A32",
             "text": "F2F4F7", "muted": "ADB7C4", "primary": "CD4145",
             "on_primary": "FFFFFF", "focus": "F0CD5A", "error": "FF9C98",
             "success": "8AD2A0", "selection": "4A282E", "accent": "E8C653", "on_accent": "211B09"}}


def require(condition, name, detail=None):
    checks.append({"name": name, "passed": bool(condition), "detail": detail})
    if not condition:
        raise RuntimeError(f"{name}: {detail}")


def command(*argv):
    result = subprocess.run([str(argument) for argument in argv], env=env, text=True,
                            capture_output=True, timeout=20)
    with (output / "operations.jsonl").open("a") as stream:
        stream.write(json.dumps({"command": [str(argument) for argument in argv],
            "status": result.returncode, "stdout": result.stdout, "stderr": result.stderr}) + "\n")
    if result.returncode:
        raise RuntimeError(result.stderr or result.stdout)
    return result.stdout.strip()


def wait_for(predicate, description, timeout=16):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(.04)
    raise RuntimeError(f"timeout: {description}")


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(5)


def geometry(window):
    return {key: int(value) for key, value in (line.split("=", 1) for line in
        command("xdotool", "getwindowgeometry", "--shell", window).splitlines())}


def capture(window, name):
    time.sleep(.12)
    bounds = geometry(window)
    image = ImageGrab.grab(xdisplay=env["DISPLAY"]).crop((bounds["X"], bounds["Y"],
        bounds["X"] + bounds["WIDTH"], bounds["Y"] + bounds["HEIGHT"])).convert("RGB")
    image.save(output / f"{name}.png")
    (output / f"{name}-geometry.json").write_text(json.dumps(bounds, indent=2))
    return image


def near(actual, expected):
    return max(abs(first - second) for first, second in zip(actual, expected)) <= TOLERANCE


def rgb(mode, role):
    value = PALETTES[mode][role]
    return tuple(int(value[index:index + 2], 16) for index in (0, 2, 4))


def contrast(first, second):
    def luminance(color):
        values = [channel / 255 for channel in color]
        linear = [value / 12.92 if value <= .04045 else ((value + .055) / 1.055) ** 2.4 for value in values]
        return sum(value * weight for value, weight in zip(linear, (.2126, .7152, .0722)))
    bright, dark = sorted((luminance(first), luminance(second)), reverse=True)
    return (bright + .05) / (dark + .05)


def pixel(image, scale, point):
    return image.getpixel(tuple(round(coordinate * scale) for coordinate in point))


def pair(image, scale, name, mode, first_role, first_point, second_role, second_point, threshold):
    first, second = pixel(image, scale, first_point), pixel(image, scale, second_point)
    require(near(first, rgb(mode, first_role)) and near(second, rgb(mode, second_role)),
            f"{scale} {mode} {name} actual colors", {"first": first, "second": second})
    ratio = contrast(first, second)
    require(ratio >= threshold, f"{scale} {mode} {name} contrast >= {threshold}", ratio)
    measurements.append({"dpi": scale, "mode": mode, "name": name, "ratio": ratio,
                         "foreground": first, "background": second})


def glyph(image, scale, name, mode, foreground, background, region, background_point):
    crop = image.crop(tuple(round(coordinate * scale) for coordinate in region))
    crop.save(output / f"{scale}-{mode}-{name}-glyph-crop.png")
    colors = Counter(crop.getpixel((left, top)) for top in range(crop.height) for left in range(crop.width))
    solid = [(count, color) for color, count in colors.items() if near(color, rgb(mode, foreground))]
    require(sum(count for count, _ in solid) >= 8, f"{scale} {mode} {name} real solid glyph pixels", sum(count for count, _ in solid))
    actual = max(solid)[1]
    body = pixel(image, scale, background_point)
    require(near(body, rgb(mode, background)), f"{scale} {mode} {name} body pixel", body)
    ratio = contrast(actual, body)
    require(ratio >= 4.5, f"{scale} {mode} {name} composited glyph contrast >= 4.5", ratio)
    measurements.append({"dpi": scale, "mode": mode, "name": name, "ratio": ratio,
                         "foreground": actual, "background": body})


def palette(image, scale, mode):
    for name, foreground, background, region, point in (
        ("accent", "on_accent", "accent", (630, 14, 736, 38), (738, 34)),
        ("body", "text", "background", (18, 12, 400, 46), (750, 500)),
        ("primary", "on_primary", "primary", (24, 62, 350, 87), (330, 90)),
        ("secondary", "text", "surface", (390, 62, 710, 87), (730, 90)),
        ("input", "text", "surface", (24, 156, 345, 184), (340, 187)),
        ("selected-tree", "text", "selection", (410, 212, 730, 237), (725, 238)),
        ("disabled", "muted", "raised", (26, 352, 350, 378), (340, 378)),
        ("error", "error", "background", (16, 258, 355, 286), (365, 272)),
        ("success", "success", "background", (16, 300, 355, 328), (365, 312)),
        ("muted", "muted", "background", (16, 400, 356, 428), (365, 412))):
        glyph(image, scale, name, mode, foreground, background, region, point)
    pair(image, scale, "input outer boundary", mode, "muted", (160, 156), "background", (160, 153), 3)
    pair(image, scale, "input inner boundary", mode, "muted", (160, 156), "surface", (160, 188), 3)
    pair(image, scale, "connector", mode, "muted", (550, 477), "background", (550, 481), 3)
    pair(image, scale, "invalid boundary", mode, "error", (160, 209), "surface", (330, 235), 3)
    pair(image, scale, "checkbox indicator", mode, "text", (30, 126), "selection", (340, 135), 3)
    pair(image, scale, "selected tree stripe", mode, "muted", (384, 222), "selection", (725, 238), 3)


def backdrop(window, scale, mode):
    command("xdotool", "windowsize", window, str(round(760 * scale)), str(round(560 * scale)))
    image = capture(window, f"{scale}-{mode}-window-clear")
    actual = pixel(image, scale, (750, 540))
    require(near(actual, rgb(mode, "background")), f"{scale} {mode} actual uncovered window clear", actual)
    command("xdotool", "windowsize", window, str(round(760 * scale)), str(round(520 * scale)))
    time.sleep(.12)


def probe(window, log_path):
    before = log_path.read_text().count("probe end")
    command("xdotool", "key", "F9")
    wait_for(lambda: log_path.read_text().count("probe end") > before, "semantic probe")
    text = log_path.read_text().rsplit("probe theme=", 1)[1].split("probe end", 1)[0]
    summary = re.match(r"(\w+) progress=(\d+)", text)
    nodes = {}
    for line in text.splitlines()[1:]:
        match = re.fullmatch(r'probe node name=(".*?") id=(NodeId \{.*?\}) role=(\w+) bounds=Bounds \{ x: ([\d.-]+), y: ([\d.-]+), width: ([\d.-]+), height: ([\d.-]+) \} value=(.*?) focused=(true|false) selected=(.*?) expanded=(.*)', line)
        if not match:
            raise RuntimeError(f"unparseable semantic probe: {line}")
        name, identity, role, left, top, width, height, value, focused, selected, expanded = match.groups()
        nodes[identity] = {"id": identity, "name": json.loads(name), "role": role,
            "bounds": [float(left), float(top), float(width), float(height)],
            "value": None if value == "None" else json.loads(value[5:-1]),
            "focused": focused == "true", "selected": selected, "expanded": expanded}
    return {"mode": summary[1], "progress": int(summary[2]), "nodes": nodes}


def named(snapshot, name):
    return next(node for node in snapshot["nodes"].values() if node["name"] == name)


def clipboard(window, expected):
    command("xdotool", "key", "ctrl+c")
    time.sleep(.15)
    actual = command(args.reader.resolve())
    require(actual == expected, "external tracked clipboard reader equals expected selection", {"actual": actual, "expected": expected})


try:
    log = (output / "xvfb.log").open("w")
    xvfb = subprocess.Popen(["Xvfb", "-displayfd", "1", "-screen", "0", "2000x1400x24",
        "-nolisten", "tcp", "-noreset", "-ac"], stdout=subprocess.PIPE, stderr=log,
        env=env, text=True, start_new_session=True)
    processes.append((xvfb, log))
    display = xvfb.stdout.readline().strip()
    require(display.isdecimal(), "dedicated X display allocated")
    env["DISPLAY"] = ":" + display
    source_paths = ["Cargo.toml", "Cargo.lock", "docs/themes-controls.md"] + sorted(
        str(path.relative_to(PROJECT)) for folder in ("src", "examples", "scripts/themes_controls")
        for path in (PROJECT / folder).rglob("*") if path.is_file() and "__pycache__" not in path.parts)
    provenance = {"source_head": args.source_head, "platform": platform.platform(),
        "python": sys.version, "pillow": pillow_version, "display": env["DISPLAY"],
        "route": "real X11 wgpu Vulkan surface; XTEST; screen crops from queried client geometry",
        "tolerances": {"flat_and_solid_glyph_channels": 2, "text": 4.5, "necessary_boundary_focus": 3},
        "build_profile": "CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false; locked examples",
        "source_sha256": {name: hashlib.sha256((PROJECT / name).read_bytes()).hexdigest() for name in source_paths},
        "binary_sha256": {str(path.resolve()): hashlib.sha256(path.read_bytes()).hexdigest() for path in (args.app, args.reader)},
        "font": command("fc-match", "sans-serif")}
    (output / "provenance.json").write_text(json.dumps(provenance, indent=2))
    for scale in (1.0, 1.5, 2.0):
        log_path = output / f"app-{scale}.log"
        log = log_path.open("w")
        process = subprocess.Popen([str(args.app.resolve()), "--verify"],
            env=dict(env, WINIT_X11_SCALE_FACTOR=str(scale)), stdout=log, stderr=subprocess.STDOUT,
            start_new_session=True)
        processes.append((process, log))
        def find_window():
            result = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(process.pid)], env=env, text=True, capture_output=True)
            return result.stdout.splitlines()[0] if result.returncode == 0 else None
        window = wait_for(find_window, "fixture window")
        command("xdotool", "windowmove", window, "20", "20")
        command("xdotool", "windowfocus", window)
        command("xdotool", "mousemove", "1900", "1350")
        wait_for(lambda: "render primitive_draws=" in log_path.read_text(), "actual presented frame")
        def click(left, top):
            command("xdotool", "mousemove", "--window", window, str(round(left * scale)), str(round(top * scale)), "click", "1")
            time.sleep(.06)
        initial = probe(window, log_path)
        image = capture(window, f"{scale}-Light-normal")
        require(image.size == (round(760 * scale), round(520 * scale)), f"{scale} native DPI geometry", image.size)
        palette(image, scale, "Light")
        image.convert("L").save(output / f"{scale}-Light-grayscale.png")
        backdrop(window, scale, "Light")
        click(90, 172)
        command("xdotool", "key", "ctrl+a")
        command("xdotool", "type", "--clearmodifiers", "--delay", "2", "M4B-preserve-42")
        command("xdotool", "key", "Home", "shift+Right", "shift+Right", "shift+Right", "shift+Right")
        clipboard(window, "M4B-")
        before = probe(window, log_path)
        selected_light = capture(window, f"{scale}-Light-selection")
        glyph(selected_light, scale, "selected", "Light", "text", "selection", (20, 156, 62, 183), (24, 181))
        command("xdotool", "key", "F6")
        clipboard(window, "M4B-")
        after = probe(window, log_path)
        require(after["mode"] == "Dark", f"{scale} actual runtime switch")
        require(set(before["nodes"]) == set(after["nodes"]), f"{scale} theme preserves mounted IDs")
        require(all(node["bounds"] == after["nodes"][identity]["bounds"] for identity, node in before["nodes"].items()), f"{scale} theme preserves all committed geometry")
        require(named(after, "source")["focused"] and named(after, "source")["value"] == "M4B-preserve-42", f"{scale} focus/value survive theme")
        require(after["progress"] > before["progress"] and before["progress"] < 100, f"{scale} active worker survives theme", [before["progress"], after["progress"]])
        selected = capture(window, f"{scale}-Dark-selection")
        glyph(selected, scale, "selected", "Dark", "text", "selection", (20, 156, 62, 183), (24, 181))
        command("xdotool", "key", "ctrl+z", "ctrl+a")
        clipboard(window, "M4B-preserve-4")
        command("xdotool", "key", "--repeat", "14", "--delay", "15", "ctrl+z")
        command("xdotool", "key", "ctrl+a")
        clipboard(window, "Knowledge source / 中文资料")
        command("xdotool", "key", "--repeat", "15", "--delay", "15", "ctrl+y")
        command("xdotool", "key", "ctrl+a")
        clipboard(window, "M4B-preserve-42")
        command("xdotool", "key", "F8")
        readonly = probe(window, log_path)
        require(named(readonly, "clipboard-target")["focused"], f"{scale} live semantic action consumed by Application")
        command("xdotool", "type", "--clearmodifiers", "--delay", "2", "MUST-NOT-EDIT")
        command("xdotool", "key", "ctrl+a")
        clipboard(window, "Read only / 保留源文")
        click(750, 500)
        command("xdotool", "mousemove", "1900", "1350")
        dark_normal = capture(window, f"{scale}-Dark-normal")
        palette(dark_normal, scale, "Dark")
        dark_normal.convert("L").save(output / f"{scale}-Dark-grayscale.png")
        backdrop(window, scale, "Dark")
        for mode in ("Dark", "Light"):
            if mode == "Light":
                command("xdotool", "key", "F6")
            command("xdotool", "mousemove", "--window", window, str(round(180 * scale)), str(round(76 * scale)))
            hover = capture(window, f"{scale}-{mode}-hover")
            glyph(hover, scale, "hover", mode, "on_primary", "primary", (24, 62, 350, 87), (330, 90))
            command("xdotool", "mousedown", "1")
            pressed = capture(window, f"{scale}-{mode}-pressed")
            glyph(pressed, scale, "pressed", mode, "on_primary", "selection", (24, 62, 350, 87), (330, 90))
            command("xdotool", "mousemove", "1900", "1350", "mouseup", "1")
            command("xdotool", "key", "F7")
            focused = capture(window, f"{scale}-{mode}-clipped-focus")
            pair(focused, scale, "clipped focus top", mode, "focus", (150, 457), "surface", (150, 460), 3)
            pair(focused, scale, "clipped focus bottom", mode, "focus", (150, 497), "surface", (150, 494), 3)
            click(170, 126)
            checked = probe(window, log_path)
            require(named(checked, "include-sources")["selected"] == "Some(false)", f"{scale} {mode} native checkbox click")
            command("xdotool", "key", "Return")
            require(named(probe(window, log_path), "include-sources")["selected"] == "Some(true)", f"{scale} {mode} native Enter checkbox")
            command("xdotool", "key", "space")
            require(named(probe(window, log_path), "include-sources")["selected"] == "Some(false)", f"{scale} {mode} native Space checkbox")
            unchecked = capture(window, f"{scale}-{mode}-unchecked-focus")
            require(near(pixel(unchecked, scale, (30, 126)), rgb(mode, "raised")), f"{scale} {mode} hovered unchecked indicator has no inner mark")
            command("xdotool", "key", "space")
        click(500, 223)
        collapsed = probe(window, log_path)
        require(not any(node["name"] == "node-0" for node in collapsed["nodes"].values()), f"{scale} collapse hides actual semantic subtree")
        capture(window, f"{scale}-collapsed")
        click(500, 223)
        expanded = probe(window, log_path)
        require(named(expanded, "node-0")["id"] == named(initial, "node-0")["id"], f"{scale} expand retains keyed identity")
        command("xdotool", "mousemove", "--window", window, str(round(700 * scale)), str(round(360 * scale)), "click", "--repeat", "3", "--delay", "40", "5")
        scrolled = probe(window, log_path)
        require(named(scrolled, "node-0")["bounds"][1] < named(expanded, "node-0")["bounds"][1], f"{scale} native wheel changes actual tree placement")
        command("xdotool", "key", "F6")
        preserved = probe(window, log_path)
        require(named(preserved, "node-0")["bounds"] == named(scrolled, "node-0")["bounds"], f"{scale} theme preserves scroll")
        require(named(preserved, "Sources / 展开知识树")["expanded"] == "Some(true)", f"{scale} theme preserves expansion")
        capture(window, f"{scale}-scrolled-theme")
        wait_for(lambda: probe(window, log_path)["progress"] == 100, "finite worker completion")
        time.sleep(.2)
        frames = log_path.read_text().count("render primitive_draws=")
        time.sleep(.4)
        require(log_path.read_text().count("render primitive_draws=") == frames, f"{scale} returns to idle after worker")
        (output / f"{scale}-preservation.json").write_text(json.dumps({"before": before, "after": after, "scrolled": scrolled, "preserved": preserved}, ensure_ascii=False, indent=2))
        stop(process)
except BaseException as error:
    checks.append({"name": "fatal", "passed": False, "detail": repr(error)})
    raise
finally:
    for process, log in reversed(processes):
        stop(process)
        log.close()
    (output / "checks.json").write_text(json.dumps(checks, indent=2))
    (output / "contrast.json").write_text(json.dumps(measurements, indent=2))
    (output / "cleanup.json").write_text(json.dumps({"own_processes_stopped": all(process.poll() is not None for process, _ in processes), "pids": [process.pid for process, _ in processes]}, indent=2))
print(json.dumps({"passed": len(checks), "contrast_samples": len(measurements), "output": str(output)}, indent=2))
