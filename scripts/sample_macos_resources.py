#!/usr/bin/env python3
"""Bounded, one-second macOS sampler. No process arguments or user content are saved."""
import argparse
import csv
import json
import os
import re
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HELPER = ROOT / "target/loppi-rusage"
HEADER = ("timestamp_utc,role,status,executable_path,pid,user_time_ns,system_time_ns,"
          "resident_bytes,physical_footprint_bytes,disk_read_bytes,disk_write_bytes,"
          "energy_nj,penergy_nj,lifetime_peak_footprint_bytes,ppid,process_birth").split(",")


def command(args):
    try:
        return subprocess.run(args, capture_output=True, text=True, timeout=3, check=True).stdout
    except (subprocess.SubprocessError, OSError):
        return ""


def coalition(pid):
    # XPC helpers are often children of launchd. Match their resource coalition
    # to the verified desktop process; never count every WebKit process.
    output = command(["launchctl", "print", f"pid/{pid}"])
    match = re.search(r"resource coalition = \{\s*ID = (\d+)", output)
    return match.group(1) if match else None


def inventory(app, cache):
    processes = {}
    for line in command([str(HELPER), "--inventory"]).splitlines():
        parts = line.split("\t", 3)
        if len(parts) == 4:
            pid, ppid, birth, path = parts
            processes[int(pid)] = (int(ppid), birth, path)
    cache_keys = {(pid, v[1]) for pid, v in processes.items()}
    for key in list(cache):
        if key not in cache_keys:
            del cache[key]
    owned = {pid: value for pid, value in processes.items() if value[2].startswith(str(app) + "/Contents/")}
    desktops = [pid for pid, v in owned.items() if Path(v[2]).name == "loppi-desktop"]
    groups = set()
    for pid in desktops:
        key = (pid, processes[pid][1])
        if key not in cache:
            cache[key] = coalition(pid)
        if cache[key]:
            groups.add(cache[key])
    roles = {}
    for pid, (ppid, birth, path) in owned.items():
        name = Path(path).name
        if name == "loppi-core":
            roles[pid] = "loppi-capture" if ppid in owned and Path(owned[ppid][2]).name == "loppi-core" else "loppi-core"
        elif name in ("loppi-worker", "loppi-desktop"):
            roles[pid] = name
    unresolved_webview = False
    for pid, value in processes.items():
        if "com.apple.WebKit." not in value[2] or not groups:
            continue
        key = (pid, value[1])
        if key not in cache:
            cache[key] = coalition(pid)
        unresolved_webview |= cache[key] is None
        if cache[key] in groups:
            roles[pid] = "loppi-webview"
    return processes, roles, bool(desktops) and (len(groups) != len(desktops) or unresolved_webview)


def rotate(path, maximum=64 * 1024 * 1024, keep=4):
    if not path.exists() or path.stat().st_size < maximum:
        return
    oldest = Path(str(path) + f".{keep}")
    oldest.unlink(missing_ok=True)
    for index in range(keep - 1, 0, -1):
        source = Path(str(path) + f".{index}")
        if source.exists():
            source.rename(str(path) + f".{index+1}")
    path.rename(str(path) + ".1")


def append(path, rows, header=HEADER):
    rotate(path)
    fresh = not path.exists()
    with path.open("a", newline="") as output:
        writer = csv.writer(output)
        if fresh:
            writer.writerow(header)
        writer.writerows(rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=float, required=True)
    parser.add_argument("--interval", type=float, default=1)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--mode", choices=["new", "resume"], default="new")
    parser.add_argument("--report", default="")
    parser.add_argument("--report-every", type=float, default=900)
    parser.add_argument("--data-dir", type=Path, default=os.environ.get("LOPPI_DATA_DIR"))
    args = parser.parse_args()
    if not 0 < args.seconds <= 7 * 86400 or not .1 <= args.interval <= 60 or args.report_every < 1:
        parser.error("duration must be <=7 days and sample interval 0.1–60 seconds")
    app = args.app.resolve()
    if not (app / "Contents").is_dir():
        parser.error("application bundle missing")
    os.umask(0o077)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    rollup = Path(str(args.output) + ".minutes.csv")
    diagnostic = Path(str(args.output) + ".diagnostics.jsonl")
    if args.mode == "new":
        for path in (args.output, rollup, diagnostic):
            path.write_text("")
            if path != diagnostic:
                with path.open("w", newline="") as f:
                    csv.writer(f).writerow(HEADER)
    elif args.output.exists():
        with args.output.open(newline="") as f:
            if next(csv.reader(f), None) != HEADER:
                parser.error("resume requires the current sampler header")
    began = time.monotonic()
    deadline = began + args.seconds
    next_inventory = next_report = next_rollup = began
    cache, processes, roles, minute = {}, {}, {}, {}
    unknown = False
    samples = 0
    peak_total = 0
    attribution_incomplete = False
    try:
        while time.monotonic() < deadline:
            tick = time.monotonic()
            if tick >= next_inventory:
                processes, roles, unknown = inventory(app, cache)
                attribution_incomplete |= unknown
                next_inventory = tick + 1
            stamp = datetime.now(timezone.utc).isoformat(timespec="milliseconds")
            rows = []
            for line in command([str(HELPER), *map(str, roles)]).splitlines():
                values = line.split(",")
                if len(values) != 10:
                    continue
                pid = int(values[0])
                if pid not in roles:
                    continue
                ppid, birth, path = processes[pid]
                row = [stamp, roles[pid], "present", path, *values, ppid, birth]
                rows.append(row)
                # Retain each minute's peak and last CPU counters, bounded by
                # observed PIDs in this minute; full samples stay in rotated CSV.
                old = minute.get((pid, birth))
                if old:
                    row = row.copy()
                    for index in (7, 8, 13):
                        row[index] = str(max(int(old[index]), int(row[index])))
                minute[(pid, birth)] = row
            present = {row[1] for row in rows}
            for role in ("loppi-core", "loppi-capture", "loppi-worker", "loppi-desktop", "loppi-webview"):
                if role not in present:
                    rows.append([stamp, role, "unknown" if role == "loppi-webview" and unknown else "absent", *([""] * 13)])
            append(args.output, rows)
            peak_total = max(peak_total, sum(int(r[8]) for r in rows if r[2] == "present"))
            if args.data_dir:
                try:
                    value = json.loads((Path(args.data_dir) / "diagnostics.json").read_text())
                    rotate(diagnostic, 32 * 1024 * 1024, 2)
                    with diagnostic.open("a") as out:
                        out.write(json.dumps({"sample_utc": stamp, "core": value}, separators=(",", ":")) + "\n")
                except (OSError, ValueError):
                    pass
            samples += 1
            if tick >= next_rollup:
                append(rollup, minute.values())
                minute.clear()
                next_rollup = tick + 60
            if args.report and tick >= next_report:
                command(["python3", str(ROOT / "scripts/report_macos_resources.py"), str(rollup),
                         "--output", args.report, "--summary", args.report + ".json"])
                next_report = tick + args.report_every
            time.sleep(max(0, min(deadline - time.monotonic(), args.interval - (time.monotonic() - tick))))
    finally:
        append(rollup, minute.values())
        metadata = {"samples": samples, "interval_seconds": args.interval,
                    "observed_seconds": time.monotonic() - began, "peak_observed_app_footprint_bytes": peak_total,
                    "webview_attribution_incomplete": attribution_incomplete, "attribution": "exact bundle path + matching launchd resource coalition",
                    "raw_csv_max_bytes": 5 * 64 * 1024 * 1024, "diagnostics_max_bytes": 3 * 32 * 1024 * 1024,
                    "limits": "Processes shorter than the inventory interval may be missed; sampled total is a lower bound, not sum of per-process lifetime peaks. Zero energy on old OS means unavailable."}
        Path(str(args.output) + ".meta.json").write_text(json.dumps(metadata, indent=2) + "\n")
        if args.report:
            command(["python3", str(ROOT / "scripts/report_macos_resources.py"), str(rollup),
                     "--output", args.report, "--summary", args.report + ".json"])


if __name__ == "__main__":
    main()
