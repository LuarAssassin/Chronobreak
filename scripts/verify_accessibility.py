"""Exercise the real AT-SPI bus in an isolated dbus-run-session and X11 server."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--app", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--existing-isolated-display", action="store_true")
args = parser.parse_args()
output = args.output.resolve()
project = Path(__file__).resolve().parents[1]
if not output.is_relative_to(project / ".local/verification"):
    parser.error("output must be private verification directory")
output.mkdir(parents=True, exist_ok=False)
records = []
processes = []

def wait(function, label, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            result = function()
            if result:
                return result
        except gi.repository.GLib.Error:
            pass
        time.sleep(.1)
    raise RuntimeError("timeout: " + label)

def walk(node):
    yield node
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child:
            yield from walk(child)

def named(name):
    return next((node for node in walk(Atspi.get_desktop(0)) if node.get_name() == name), None)

def check(name, condition):
    records.append({"check": name, "passed": bool(condition)})
    if not condition:
        raise RuntimeError("failed: " + name)

def key(*keys):
    env = dict(os.environ)
    if "CHRONOBREAK_INPUT_DISPLAY" in env:
        env["DISPLAY"] = env["CHRONOBREAK_INPUT_DISPLAY"]
        windows = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--class", "^Weston Compositor$"], env=env, text=True, timeout=10).splitlines()
        if len(windows) != 1:
            raise RuntimeError("private compositor window is ambiguous")
        outer = windows[0]
        subprocess.run(["xdotool", "windowactivate", "--sync", outer], env=env, check=True, timeout=10)
    subprocess.run(["xdotool", "key", *keys], env=env, check=True, timeout=10)

def close_request(window):
    class ClientMessage(ctypes.Structure):
        _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int), ("display", ctypes.c_void_p), ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong), ("format", ctypes.c_int), ("data", ctypes.c_long * 5)]
    class Event(ctypes.Union):
        _fields_ = [("message", ClientMessage), ("padding", ctypes.c_long * 24)]
    library = ctypes.CDLL("libX11.so.6")
    library.XOpenDisplay.argtypes = [ctypes.c_char_p]
    library.XOpenDisplay.restype = ctypes.c_void_p
    library.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    library.XInternAtom.restype = ctypes.c_ulong
    library.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.POINTER(Event)]
    library.XFlush.argtypes = [ctypes.c_void_p]
    library.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = library.XOpenDisplay(os.environ["DISPLAY"].encode())
    if not display:
        raise RuntimeError("cannot open private X11 display")
    try:
        event = Event()
        event.message.type = 33
        event.message.window = int(window)
        event.message.message_type = library.XInternAtom(display, b"WM_PROTOCOLS", 0)
        event.message.format = 32
        event.message.data[0] = library.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
        if not library.XSendEvent(display, int(window), 0, 0, ctypes.byref(event)):
            raise RuntimeError("WM_DELETE_WINDOW delivery failed")
        library.XFlush(display)
    finally:
        library.XCloseDisplay(display)

try:
    if args.existing_isolated_display:
        if os.environ.get("CHRONOBREAK_ISOLATED_X11") != "1":
            raise RuntimeError("existing display must be created by isolated_xwayland.py")
    else:
        with (output / "display").open("w") as display, (output / "xvfb.log").open("w") as log:
            xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(display.fileno()), "-screen", "0", "1024x900x24", "-nolisten", "tcp"], pass_fds=(display.fileno(),), stdout=log, stderr=log, start_new_session=True)
        processes.append(xvfb)
        os.environ["DISPLAY"] = ":" + wait(lambda: (output / "display").read_text().strip(), "display")
    os.environ["WINIT_UNIX_BACKEND"] = "x11"
    os.environ.pop("WAYLAND_DISPLAY", None)
    os.environ["ACCESSKIT_UNIX_FORCE_ENABLED"] = "1"
    with (output / "app.log").open("w") as log:
        app = subprocess.Popen([str(args.app.resolve()), "--verify"], stdout=log, stderr=log, start_new_session=True)
    processes.append(app)
    window = wait(lambda: subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)], text=True, capture_output=True, timeout=10).stdout.strip(), "native window").splitlines()[0]
    subprocess.run(["xdotool", "windowactivate" if args.existing_isolated_display else "windowfocus", "--sync", window], check=True, timeout=10)
    switch = wait(lambda: named("theme-switch"), "real AT-SPI tree")
    check("button-role", switch.get_role() == Atspi.Role.PUSH_BUTTON)
    checkbox = wait(lambda: named("include-sources"), "checkbox")
    check("checkbox-role", checkbox.get_role() == Atspi.Role.CHECK_BOX)
    initial = checkbox.get_state_set().contains(Atspi.StateType.CHECKED)
    check("checkbox-action-present", checkbox.get_action_iface().get_n_actions() > 0)
    check("checkbox-action-accepted", checkbox.get_action_iface().do_action(0))
    wait(lambda: named("include-sources").get_state_set().contains(Atspi.StateType.CHECKED) != initial, "live checked state")
    check("checkbox-live-state", True)
    target = wait(lambda: named("source"), "editable target")
    check("text-role", target.get_role() == Atspi.Role.ENTRY)
    check("focus-action", target.get_component_iface().grab_focus())
    wait(lambda: named("source").get_state_set().contains(Atspi.StateType.FOCUSED), "AT-SPI focus")
    check("focus-live-state", True)
    check("editable-set-value", target.get_editable_text_iface().set_text_contents("AT-SPI 中文 source"))
    wait(lambda: Atspi.Text.get_text(named("source"), 0, -1) == "AT-SPI 中文 source", "real edited value")
    check("editable-live-value", True)
    readonly = named("clipboard-target")
    check("readonly-refuses-edit", not readonly.get_editable_text_iface().set_text_contents("must not replace"))
    check("readonly-keeps-value", Atspi.Text.get_text(readonly, 0, -1) == "Read only / 保留源文")
    check("disabled-state", not named("Disabled / 不可用").get_state_set().contains(Atspi.StateType.ENABLED))
    window = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)], text=True, timeout=10).splitlines()[0]
    subprocess.run(["xdotool", "windowactivate" if args.existing_isolated_display else "windowfocus", "--sync", window], check=True, timeout=10)
    key("Tab")
    wait(lambda: named("clipboard-target").get_state_set().contains(Atspi.StateType.FOCUSED) and not named("source").get_state_set().contains(Atspi.StateType.FOCUSED), "keyboard tab source to readonly target")
    check("keyboard-tab-focus", True)
    close_request(window)
    check("orderly-exit", app.wait(timeout=5) == 0)
    wait(lambda: named("theme-switch") is None, "AT-SPI teardown")
    check("teardown-removes-tree", True)
finally:
    (output / "checks.json").write_text(json.dumps(records, ensure_ascii=False, indent=2))
    for process in reversed(processes):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
print(json.dumps({"passed": True, "checks": len(records)}))
