"""真实 X11 surface 复验；需要系统 Xvfb、xdotool、Pillow，不修改桌面设置。"""

import argparse
import hashlib
import json
import math
import os
import platform
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time

from PIL import ImageChops, ImageGrab, __version__ as pillow_version


PROJECT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--app", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--source-head", help="clean archive 的 40 位源提交 SHA")
args = parser.parse_args()
app = args.app.resolve()
output = args.output.resolve()
if not app.is_file() or not os.access(app, os.X_OK):
    parser.error("--app must be an executable built from the tracked visual_primitives example")
if not output.is_relative_to(PROJECT / ".local/verification"):
    parser.error("--output must be inside this feature worktree's .local/verification")
if args.source_head and not re.fullmatch(r"[0-9a-f]{40}", args.source_head):
    parser.error("--source-head must be a full lowercase commit SHA")
for name in ("text_input", "background_messages", "input_routing", "scroll_views"):
    if not (app.parent / name).is_file() or not os.access(app.parent / name, os.X_OK):
        parser.error(f"build all tracked examples first: missing {name}")
for command in ("Xvfb", "xdotool", "fc-match"):
    if not shutil.which(command):
        parser.error(f"system prerequisite missing: {command}")
output.mkdir(parents=True, exist_ok=False)
for directory in ("tmp", "runtime", "home", "cache"):
    (output / directory).mkdir(mode=0o700)
env = {"PATH": os.environ.get("PATH", os.defpath), "LANG": "C.UTF-8",
       "HOME": str(output / "home"), "XDG_RUNTIME_DIR": str(output / "runtime"),
       "XDG_CACHE_HOME": str(output / "cache"), "TMPDIR": str(output / "tmp"),
       "WINIT_UNIX_BACKEND": "x11", "CHRONOBREAK_TRACE": "1", "WGPU_BACKEND": "vulkan"}
processes = []
checks = []
measurements = {}


def require(condition, name, detail=None):
    checks.append({"name": name, "passed": bool(condition), "detail": detail})
    if not condition:
        raise RuntimeError(f"verification failed: {name}: {detail}")


def command(*argv):
    return subprocess.check_output(argv, env=env, text=True, stderr=subprocess.STDOUT).strip()


def wait_for(predicate, description, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(.04)
    raise RuntimeError(f"timed out: {description}")


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(5)


def launch(name, mode, scale=1.0, unbatched=False, binary=None):
    child_env = dict(env, WINIT_X11_SCALE_FACTOR=str(scale))
    if unbatched:
        child_env["CHRONOBREAK_UNBATCHED"] = "1"
    log_path = output / f"{name}.log"
    log = log_path.open("w")
    process = subprocess.Popen([str(binary or app), *([mode] if mode else [])], env=child_env,
                               stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    processes.append((process, log))

    def window():
        result = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(process.pid)],
                                env=env, text=True, capture_output=True)
        return result.stdout.splitlines()[0] if result.returncode == 0 else None

    target = wait_for(window, f"{name} window")
    command("xdotool", "windowmove", target, "40", "40")
    command("xdotool", "mousemove", "1750", "1250")
    wait_for(lambda: "render primitive_draws=" in log_path.read_text(), f"{name} presented frame")
    time.sleep(.25)
    return process, target, log_path


def metrics(log_path):
    lines = log_path.read_text().splitlines()
    result = {}
    for prefix in ("resources prepared=", "render primitive_draws=", "scene ", "layout version="):
        matching = [line for line in lines if line.startswith(prefix)]
        if matching:
            result.update({key: int(value) for key, value in re.findall(r"([a-z_]+)=(\d+)", matching[-1])})
    return result


def capture(target, name):
    geometry = dict(line.split("=", 1) for line in command("xdotool", "getwindowgeometry", "--shell", target).splitlines())
    left, top, width, height = (int(geometry[key]) for key in ("X", "Y", "WIDTH", "HEIGHT"))
    desktop = ImageGrab.grab(xdisplay=env["DISPLAY"])
    image = desktop.crop((left, top, left + width, top + height)).convert("RGB")
    image.save(output / f"{name}.png")
    (output / f"{name}-geometry.json").write_text(json.dumps(geometry, indent=2))
    return image


def decode(channel):
    value = channel / 255
    return value / 12.92 if value <= .04045 else ((value + .055) / 1.055) ** 2.4


def encode(value):
    value = max(0, min(1, value))
    return round(255 * (12.92 * value if value <= .0031308 else 1.055 * value ** (1 / 2.4) - .055))


def over(foreground, background, alpha):
    return tuple(encode(decode(front) * alpha + decode(back) * (1 - alpha))
                 for front, back in zip(foreground, background))


def near(actual, expected, tolerance=2):
    return max(abs(front - back) for front, back in zip(actual, expected)) <= tolerance


def sdf(x, y, bounds, radius):
    left, top, width, height = bounds
    delta_x = abs(x - left - width / 2) - width / 2 + radius
    delta_y = abs(y - top - height / 2) - height / 2 + radius
    return math.hypot(max(delta_x, 0), max(delta_y, 0)) + min(max(delta_x, delta_y), 0) - radius


def smoothstep(left, right, value):
    value = max(0, min(1, (value - left) / (right - left)))
    return value * value * (3 - 2 * value)


def inspect_pixels(image, scale):
    background, cyan, red, dark = (247, 250, 252), (185, 227, 240), (205, 65, 69), (24, 43, 54)

    def sample(x, y):
        return image.getpixel((int(x * scale), int(y * scale)))

    for name, point, expected in [
        ("cyan swatch", (30, 70), cyan), ("red swatch", (120, 70), red),
        ("dark swatch", (220, 70), dark),
        ("linear alpha swatch", (310, 70), over((0, 115, 138), background, 128 / 255)),
        ("inside border", (401, 80), dark), ("rounded fill", (460, 80), cyan),
        ("rounded outside", (10, 200), background), ("rounded child is not clipped", (201, 201), red),
        ("clipped shadow outside ancestor", (552, 205), background),
        ("shadow beyond support", (539, 80), background),
        ("radius and border clamp", (600, 80), dark),
        ("transparent border interior", (590, 190), background),
        ("line first endpoint", (320, 350), dark), ("line last endpoint", (440, 400), dark),
        ("zero length dot", (470, 400), red),
    ]:
        actual = sample(*point)
        require(near(actual, expected), f"{scale}: {name}", {"actual": actual, "expected": expected})

    for point in [(530, 80), (534, 80), (548, 205)]:
        pixel_x, pixel_y = (int(value * scale) for value in point)
        x, y = (pixel_x + .5) / scale, (pixel_y + .5) / scale
        bounds, radius = ((404, 46, 124, 84), 22) if point[1] == 80 else ((540, 180, 20, 50), 0)
        alpha = (1 - smoothstep(-8, 8, sdf(x, y, bounds, radius))) * 128 / 255
        expected = over((0, 0, 0), background, alpha)
        actual = image.getpixel((pixel_x, pixel_y))
        require(near(actual, expected, 4), f"{scale}: bounded shadow {point}", {"actual": actual, "expected": expected})

    for value, box in [(cyan, (10, 105, 160, 195)), (red, (170, 105, 320, 195)),
                       (over((0, 115, 138), dark, 128 / 255), (20, 295, 90, 350)),
                       (red, (90, 295, 150, 350))]:
        crop = image.crop(tuple(int(coordinate * scale) for coordinate in box))
        count = sum(near(crop.getpixel((x, y)), value) for y in range(crop.height) for x in range(crop.width))
        require(count >= 50, f"{scale}: glyph / order color {value}", {"pixels": count})

    partial = 0
    errors = []
    for pixel_y in range(int(200 * scale), int(219 * scale)):
        for pixel_x in range(int(10 * scale), int(29 * scale)):
            x, y = (pixel_x + .5) / scale, (pixel_y + .5) / scale
            distance = sdf(x, y, (10, 200, 100, 50), 17.25)
            left, top = pixel_x - pixel_x % 2, pixel_y - pixel_y % 2
            delta_x = sdf((left + 1.5) / scale, y, (10, 200, 100, 50), 17.25) - sdf((left + .5) / scale, y, (10, 200, 100, 50), 17.25)
            delta_y = sdf(x, (top + 1.5) / scale, (10, 200, 100, 50), 17.25) - sdf(x, (top + .5) / scale, (10, 200, 100, 50), 17.25)
            aa = max(abs(delta_x) + abs(delta_y), .0001) * .5
            coverage = 1 - smoothstep(-aa, aa, distance)
            expected = over(red, background, coverage)
            actual = image.getpixel((pixel_x, pixel_y))
            errors.append(max(abs(front - back) for front, back in zip(actual, expected)))
            partial += int(.02 < coverage < .98 and not near(actual, red) and not near(actual, background))
    require(max(errors) <= 4, f"{scale}: rounded fractional AA reference", {"max_error": max(errors)})
    require(partial >= 3, f"{scale}: partial AA coverage", {"pixels": partial})


try:
    xvfb_log = (output / "xvfb.log").open("w")
    xvfb = subprocess.Popen(["Xvfb", "-displayfd", "1", "-screen", "0", "1800x1300x24", "-nolisten", "tcp", "-noreset", "-ac"],
                            stdout=subprocess.PIPE, stderr=xvfb_log, text=True, env=env, start_new_session=True)
    processes.append((xvfb, xvfb_log))
    display = xvfb.stdout.readline().strip()
    if not display.isdecimal():
        raise RuntimeError("Xvfb did not return a dedicated display")
    env["DISPLAY"] = ":" + display
    source_head = args.source_head
    if source_head is None and (PROJECT / ".git").exists():
        source_head = subprocess.check_output(["git", "-C", str(PROJECT), "rev-parse", "HEAD"], text=True).strip()
    provenance = {"head": source_head,
                  "app_sha256": hashlib.sha256(app.read_bytes()).hexdigest(), "display": env["DISPLAY"],
                  "font": command("fc-match", "sans-serif"), "capture": "X11 screen cropped to queried client geometry; not Cua-attested",
                  "route": "native X11 wgpu surface, not offscreen", "tolerances": {"interior": 2, "edge": 4, "batch_diff": 0}}
    font_path = Path(command("fc-match", "-f", "%{file}", "sans-serif"))
    provenance.update(platform=platform.platform(), python=sys.version, pillow=pillow_version,
                      font_path=str(font_path), font_sha256=hashlib.sha256(font_path.read_bytes()).hexdigest())
    source_paths = ["Cargo.toml", "Cargo.lock"] + sorted(str(path.relative_to(PROJECT)) for folder in ("src", "examples", "scripts/visual_primitives") for path in (PROJECT / folder).rglob("*") if path.is_file() and "__pycache__" not in path.parts)
    provenance["source_sha256"] = {name: hashlib.sha256((PROJECT / name).read_bytes()).hexdigest() for name in source_paths}
    (output / "provenance.json").write_text(json.dumps(provenance, indent=2))
    for scale in (1.0, 1.5, 2.0):
        images = []
        counts = []
        for unbatched in (True, False):
            name = f"reference-{scale}-{'unbatched' if unbatched else 'batched'}"
            process, target, log_path = launch(name, "--verification", scale, unbatched)
            image = capture(target, name)
            require(image.size == (round(760 * scale), round(520 * scale)), f"{name}: actual client DPI", image.size)
            inspect_pixels(image, scale)
            images.append(image)
            counts.append(metrics(log_path))
            stop(process)
        difference = ImageChops.difference(*images)
        difference.save(output / f"reference-{scale}-diff.png")
        require(difference.getbbox() is None, f"{scale}: batched output byte identical")
        require(counts[1]["primitive_draws"] < counts[0]["primitive_draws"], f"{scale}: actual draws reduced", counts)
        require(counts[1]["upload_bytes"] == counts[0]["upload_bytes"], f"{scale}: unchanged geometry upload", counts)
        measurements[str(scale)] = counts

        contiguous_images = []
        contiguous_counts = []
        for unbatched in (True, False):
            name = f"contiguous-{scale}-{'unbatched' if unbatched else 'batched'}"
            process, target, log_path = launch(name, "--contiguous", scale, unbatched)
            contiguous_images.append(capture(target, name))
            contiguous_counts.append(metrics(log_path))
            stop(process)
        require(ImageChops.difference(*contiguous_images).getbbox() is None, f"{scale}: contiguous output byte identical")
        require([value["primitive_draws"] for value in contiguous_counts] == [33, 1], f"{scale}: actual contiguous draws 33 to 1", contiguous_counts)
        measurements[f"contiguous-{scale}"] = contiguous_counts

    for scale in (1.0, 1.5, 2.0):
        process, target, log_path = launch(f"tree-{scale}", "", scale)
        before = capture(target, f"tree-{scale}")
        command("xdotool", "mousemove", "--window", target, str(int(300 * scale)), str(int(200 * scale)), "click", "5")
        wait_for(lambda: re.search(r"scroll viewport=.*offset=[1-9]", log_path.read_text()), f"tree-{scale} scroll")
        time.sleep(.15)
        after = capture(target, f"tree-{scale}-scrolled")
        require(ImageChops.difference(before, after).getbbox() is not None, f"public tree {scale}: native wheel changes actual pixels")
        stop(process)

    process, target, log_path = launch("resources-visible", "--resources-visible")
    command("xdotool", "mousemove", "--window", target, "650", "450", "click", "1")
    wait_for(lambda: "resources prepared=16 " in log_path.read_text(), "visible-only baseline")
    baseline = metrics(log_path)
    measurements["visible-only-baseline"] = baseline
    require(baseline["skipped"] == 0, "visible-only baseline contains no hidden/clipped buffers", baseline)
    stop(process)

    process, target, log_path = launch("resources", "--resources")
    samples = []
    phase_images = {}
    for step in range(200):
        command("xdotool", "mousemove", "--window", target, "650", "450", "click", "1")
        wait_for(lambda: f"resource_step={step} " in log_path.read_text() and "render primitive_draws=" in log_path.read_text().split(f"resource_step={step} ", 1)[1], f"resource step {step}")
        current = metrics(log_path)
        status = Path(f"/proc/{process.pid}/status").read_text()
        current["rss_kib"] = int(re.search(r"VmRSS:\s+(\d+)", status).group(1))
        current["step"] = step
        expected = 0 if step % 5 in (1, 4) else 16
        require(current["text_cache"] == expected and current["prepared"] == expected, f"resources step {step}: visible-only prepare/cache", current)
        if expected:
            require(current["skipped"] == 128, f"resources step {step}: hidden+clipped skipped", current)
        samples.append(current)
        if step == 0:
            require(current["prepared"] == baseline["prepared"] and current["upload_bytes"] == baseline["upload_bytes"], "hidden/clipped additions do not add prepare or primitive upload", {"visible": baseline, "with_hidden_clipped": current})
        if step // 5 in (0, 19, 39):
            time.sleep(.1)
            image = capture(target, f"resource-step-{step}")
            phase_images[step % 5] = image.crop((0, 0, 600, 400))
            if step % 5 == 2:
                require(ImageChops.difference(phase_images[0], phase_images[2]).getbbox() is None, f"resource cycle {step // 5}: restore same text pixels")
            if step % 5 == 3:
                require(ImageChops.difference(phase_images[2], phase_images[3]).getbbox() is not None, f"resource cycle {step // 5}: replacement changes actual text pixels")
            if step % 5 == 4:
                require(ImageChops.difference(phase_images[1], phase_images[4]).getbbox() is None, f"resource cycle {step // 5}: unmount returns hidden pixels")
        if step % 5 == 4:
            require(current["live"] == 2 and current["slots"] == 147 and current["subscriptions"] == 0 and current["text_buffers"] == 0, f"resource cycle {step // 5}: live resources return to baseline", current)
    capture(target, "resources-after-40-cycles")
    settled = [sample for sample in samples if sample["step"] >= 100 and sample["step"] % 5 == 4]
    rss_span = max(sample["rss_kib"] for sample in settled) - min(sample["rss_kib"] for sample in settled)
    require(rss_span <= 16 * 1024, "resource late RSS span <= 16 MiB", rss_span)
    require(len({(sample["vertex_capacity"], sample["vertex_allocations"]) for sample in samples[10:]}) == 1, "resource vertex high-water reuse")
    (output / "resource-samples.json").write_text(json.dumps(samples, indent=2))
    measurements["resources"] = {"cycles": 40, "late_rss_span_kib": rss_span, "samples": len(samples), "gpu_memory": "not measured"}
    stop(process)

    process, target, log_path = launch("smoke-text-input", "", binary=app.parent / "text_input")
    command("xdotool", "windowfocus", target)
    wait_for(lambda: "input Focused(true)" in log_path.read_text(), "TextInput focus event")
    time.sleep(.2)
    command("xdotool", "key", "Tab", "ctrl+a")
    command("xdotool", "type", "--clearmodifiers", "--delay", "5", "M4A smoke")
    wait_for(lambda: 'text-input value="M4A smoke"' in log_path.read_text(), "controlled TextInput native keys")
    capture(target, "smoke-text-input")
    require("ime_area logical=" in log_path.read_text(), "TextInput candidate-area path retained")
    stop(process)

    process, target, log_path = launch("smoke-input-routing", "", binary=app.parent / "input_routing")
    command("xdotool", "windowfocus", target)
    wait_for(lambda: "input Focused(true)" in log_path.read_text(), "routing focus event")
    time.sleep(.2)
    command("xdotool", "key", "Tab", "Return")
    wait_for(lambda: "demo activate A total=1" in log_path.read_text(), "keyboard button activation")
    capture(target, "smoke-input-routing")
    require("Capture" in log_path.read_text() and "Bubble" in log_path.read_text(), "capture/bubble input routing retained")
    stop(process)

    process, target, log_path = launch("smoke-scroll-views", "", binary=app.parent / "scroll_views")
    before = capture(target, "smoke-scroll-before")
    command("xdotool", "mousemove", "--window", target, "300", "240", "click", "5")
    wait_for(lambda: re.search(r"scroll viewport=.*offset=[1-9]", log_path.read_text()), "nested vertical scroll")
    time.sleep(.2)
    after = capture(target, "smoke-scroll-after")
    require(ImageChops.difference(before, after).getbbox() is not None, "existing scroll example updates native pixels")
    stop(process)

    process, target, log_path = launch("smoke-background", "", binary=app.parent / "background_messages")
    wait_for(lambda: "step=10/10" in log_path.read_text() and "worker complete" in log_path.read_text(), "worker wakes and completes", 15)
    time.sleep(.3)
    count = len(re.findall(r"render primitive_draws=", log_path.read_text()))
    time.sleep(.6)
    require(len(re.findall(r"render primitive_draws=", log_path.read_text())) == count, "background completion returns to idle")
    require("user_event messages_ready" in log_path.read_text(), "background EventLoopProxy wake retained")
    capture(target, "smoke-background")
    stop(process)
except BaseException as error:
    checks.append({"name": "fatal verifier error", "passed": False, "detail": repr(error)})
    raise
finally:
    for process, log in reversed(processes):
        stop(process)
        log.close()
    (output / "checks.json").write_text(json.dumps(checks, indent=2))
    (output / "measurements.json").write_text(json.dumps(measurements, indent=2))
    (output / "cleanup.json").write_text(json.dumps({"own_processes_stopped": all(process.poll() is not None for process, _ in processes), "pids": [process.pid for process, _ in processes]}, indent=2))
print(json.dumps({"passed": len(checks), "output": str(output), "measurements": measurements}, indent=2))
