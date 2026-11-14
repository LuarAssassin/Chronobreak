"""Serial optimized CPU qualification; does not access credentials or services."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--target", type=Path, required=True)
parser.add_argument("--runs", type=int, default=3, choices=range(1, 6))
args = parser.parse_args()
project = Path(__file__).resolve().parents[1]
output = args.output.resolve()
if not output.is_relative_to(project / ".local/verification"):
    parser.error("output must remain under worktree .local/verification")
output.mkdir(parents=True, exist_ok=False)
env = dict(os.environ, CARGO_TARGET_DIR=str(args.target.resolve()), CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_DEV_OPT_LEVEL="2")

def capture(*command):
    result = subprocess.run(command, cwd=project, env=env, text=True, capture_output=True, timeout=30)
    return {"exit": result.returncode, "stdout": result.stdout, "stderr": result.stderr}

font = capture("fc-match", "-f", "%{file}\n", "sans-serif")
font_path = Path(font["stdout"].splitlines()[0]) if font["exit"] == 0 else None
metadata = {
    "platform": platform.platform(), "cpu": next(line for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")),
    "rustc": capture("rustc", "-Vv"), "gpu": capture("nvidia-smi", "--query-gpu=name,driver_version,memory.total", "--format=csv,noheader"),
    "font": font, "font_sha256": hashlib.sha256(font_path.read_bytes()).hexdigest() if font_path else None,
    "profile": "dev opt-level=2 debug=0", "backend": "CPU scene only; no surface, GPU timing or VRAM measurements",
    "cargo_lock_sha256": hashlib.sha256((project / "Cargo.lock").read_bytes()).hexdigest(),
    "tracked_diff": capture("git", "diff", "--stat"), "base": capture("git", "rev-parse", "HEAD"),
    "wall_clock": time.time(), "runs": args.runs,
    "source_sha256": {str(path.relative_to(project)): hashlib.sha256(path.read_bytes()).hexdigest() for pattern in ("src/*.rs", "scripts/qualify_performance.py", "docs/performance-budgets.json") for path in sorted(project.glob(pattern))},
}
(output / "metadata.json").write_text(json.dumps(metadata, indent=2))
command = ["cargo", "test", "--lib", "measured_alpha_workloads", "--locked", "--", "--ignored", "--nocapture", "--test-threads=1"]
all_records = []
for run in range(args.runs):
    started = time.monotonic()
    result = subprocess.run(command, cwd=project, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=600)
    (output / f"run-{run}.log").write_text(result.stdout)
    if result.returncode:
        raise RuntimeError(f"run {run} failed; see log")
    records = []
    for line in result.stdout.splitlines():
        if "performance {" in line:
            records.append(json.loads(line[line.index("performance ") + len("performance "):]))
    if len(records) != 5:
        raise RuntimeError("missing workloads")
    rss = [json.loads(line[4:]) for line in result.stdout.splitlines() if line.startswith("rss ")]
    if len(rss) != 5:
        raise RuntimeError("missing RSS samples")
    all_records.append({"run": run, "wall_seconds": time.monotonic() - started, "measurements": records, "rss": rss})
(output / "measurements.json").write_text(json.dumps(all_records, indent=2))
budgets = json.loads((project / "docs/performance-budgets.json").read_text())
checks = []
for run in all_records:
    for measured in run["measurements"]:
        checks.append({"run": run["run"], "workload": measured["workload"], "passed": measured["cold_us"] <= budgets["cold_us"] and measured["max_us"] <= budgets["max_us"] and measured["p95_us"] <= budgets["p95_us"][measured["workload"]]})
    for measured in run["rss"]:
        samples = measured["samples_cycle_kib"]
        growth = max(value for _, value in samples) - samples[0][1]
        checks.append({"run": run["run"], "workload": measured["workload"] + "-rss", "growth_kib": growth, "passed": growth <= budgets["rss_growth_kib"]})
(output / "checks.json").write_text(json.dumps(checks, indent=2))
if not all(check["passed"] for check in checks):
    raise RuntimeError("provisional budget exceeded; inspect checks, do not silently relax budgets")
print(json.dumps({"runs": len(all_records), "workloads_per_run": 5, "checks": len(checks), "passed": True, "output": str(output)}))
