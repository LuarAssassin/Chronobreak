import json
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[2] / ".local/verification"
run = root / sys.argv[1]
actions = Path(__file__).with_name("actions.py")
checks = []

def act(*args):
    result = subprocess.run(["/usr/bin/python3", str(actions), str(run), *args], capture_output=True, text=True)
    with (run / "exercise.log").open("a") as stream:
        stream.write(json.dumps(args) + "\n" + result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(args)

def snap(name):
    act("snap", name)
    return json.loads((run / f"{name}.json").read_text())

def check(name, condition):
    checks.append({"name": name, "passed": bool(condition)})
    (run / "checks.json").write_text(json.dumps(checks, indent=2))

def model(snapshot):
    return snapshot["model"]

def popup_matches(snapshot):
    area = snapshot["last_area"]
    if not area or len(snapshot["candidates"]) != 1:
        return False
    x, y, width, height, scale = map(float, area)
    candidate = snapshot["candidates"][0]
    app = snapshot["app"]
    return abs(candidate["X"] - (app["X"] + x*scale)) <= 2 and abs(candidate["Y"] - (app["Y"] + (y+height)*scale + 10)) <= 2

act("init")
act("type", "nihao")
preedit = snap("01-preedit")
check("real_preedit_no_signal", not model(preedit) and any('Preedit("你好"' in event for event in preedit["ime"]) and bool(preedit["candidates"]))
check("initial_actual_candidate_at_caret", popup_matches(preedit))
act("key", "space")
committed = snap("02-commit")
check("space_commit_exactly_once", model(committed) == ['model first="你好"'] and sum('Commit("你好")' in event for event in committed["ime"]) == 1)
act("key", "ctrl+z")
undone = snap("03-undo")
check("one_undo_restores_empty", model(undone)[-1:] == ['model first=""'])
act("key", "ctrl+shift+z")
redone = snap("04-redo")
check("one_redo_restores_commit", model(redone)[-1:] == ['model first="你好"'])
act("key", "Shift_L")
act("type", " Latin ")
act("key", "Shift_L")
act("type", "zhongwen")
act("key", "space")
mixed = snap("05-mixed")
check("latin_chinese_mixed", model(mixed)[-1:] == ['model first="你好 Latin 中文"'])
act("type", "nihao")
cancel_before = snap("06-cancel-preedit")
act("key", "Escape")
cancel_after = snap("07-cancel")
check("escape_discards_without_signal", model(cancel_before) == model(cancel_after) and not cancel_after["candidates"])
act("type", "nihao")
snap("08-before-transfer")
act("click", "30", "225")
transfer = snap("09-transfer")
act("key", "space")
second_space = snap("10-second-no-leak")
check("focus_transfer_no_old_commit", model(transfer) == model(cancel_after) and model(second_space)[-1:] == ['model second=" "'])
act("key", "ctrl+a", "BackSpace")
act("type", "zhongwen")
act("key", "space")
second = snap("11-second-commit")
check("second_field_independent_commit", model(second)[-1:] == ['model second="中文"'])
act("type", "nihao")
nested_before = snap("12-nested-before")
act("point", "300", "225")
act("button-down", "5")
nested_after = snap("13-nested-scrolled")
check("nested_scroll_keeps_model", model(nested_before) == model(nested_after) and nested_before["last_area"] != nested_after["last_area"])
check("nested_actual_candidate_follows", popup_matches(nested_after))
act("button-up", "5")
act("wheel", "4", "1")
act("key", "Escape")
act("click", "25", "95")
act("type", "nihao")
moving_before = snap("14-before-move")
act("move", "210", "130")
moved = snap("15-moved")
check("move_actual_candidate_follows", popup_matches(moved))
act("resize", "300", "520")
reflow = snap("16-reflow")
check("reflow_actual_candidate_follows", popup_matches(reflow))
act("key", "Escape")
act("resize", "760", "520")
act("click", "25", "95")
act("type", "nihao")
before_remove = snap("17-before-remove")
act("click", "40", "460")
removed = snap("18-removed")
act("key", "Tab")
act("key", "space")
after_remove = snap("19-after-remove-second")
check("removal_no_old_commit", model(before_remove) == model(removed) and model(after_remove)[-1:] in
      [['model second=" 中文"'], ['model second="中 文"'], ['model second="中文 "']])
print(json.dumps({"run": run.name, "passed": sum(item["passed"] for item in checks), "total": len(checks), "failed": [item["name"] for item in checks if not item["passed"]]}))
assert len(checks) == 14 and all(item["passed"] for item in checks)
