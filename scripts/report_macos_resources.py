#!/usr/bin/env python3
"""Render the macOS process sampler CSV without external Python packages."""

import argparse
import csv
import html
import json
import os
import statistics
import tempfile
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

MIB = 1024 * 1024
COLORS = {
    "loppi-core": "#247567",
    "loppi-worker": "#bf7647",
    "loppi-capture": "#4596a2",
    "loppi-webview": "#a05caa",
    "loppi-desktop": "#626faf",
}
GAP_SECONDS = 150


def read_samples(paths):
    samples = {}
    timestamps = set()
    absences = defaultdict(int)
    seen_absences = set()
    for path in paths:
        with path.open(newline="") as source:
            reader = csv.DictReader(source)
            for row in reader:
                try:
                    stamp = datetime.fromisoformat(row["timestamp_utc"].replace("Z", "+00:00"))
                    role = row["role"]
                    if role not in COLORS:
                        continue
                    timestamps.add(stamp)
                    if row["status"] == "absent":
                        absence = (role, stamp)
                        if absence not in seen_absences:
                            absences[role] += 1
                            seen_absences.add(absence)
                        continue
                    if row["status"] != "present":
                        continue
                    pid = int(row["pid"])
                    item = {
                        "time": stamp,
                        "role": role,
                        "pid": pid,
                        "birth": row.get("process_birth", ""),
                        "lifetime_peak_mib": int(row.get("lifetime_peak_footprint_bytes") or 0) / MIB,
                        "footprint_mib": int(row["physical_footprint_bytes"]) / MIB,
                        "resident_mib": int(row["resident_bytes"]) / MIB,
                        "cpu_ns": int(row["user_time_ns"]) + int(row["system_time_ns"]),
                        "energy_nj": int(row["energy_nj"]),
                    }
                    samples[(role, pid, item["birth"], stamp)] = item
                except (KeyError, TypeError, ValueError):
                    # An interrupted write leaves at most one incomplete line.
                    continue
    groups = defaultdict(list)
    for item in samples.values():
        groups[(item["role"], str(item["pid"]) + ("@" + item["birth"] if item["birth"] else ""))].append(item)
    for rows in groups.values():
        rows.sort(key=lambda row: row["time"])
    return groups, sorted(timestamps), dict(absences)


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int((len(ordered) - 1) * fraction))]


def summarize(groups, timestamps, absences):
    global GAP_SECONDS
    intervals = [(b-a).total_seconds() for a,b in zip(timestamps,timestamps[1:]) if b>a]
    GAP_SECONDS=max(1,statistics.median(intervals)*2.5) if intervals else 150
    gaps = []
    for before, after in zip(timestamps, timestamps[1:]):
        seconds = (after - before).total_seconds()
        if seconds > GAP_SECONDS:
            gaps.append({"start": before.isoformat(), "end": after.isoformat(), "seconds": seconds})
    processes = []
    for (role, pid), rows in sorted(groups.items()):
        footprint = [row["footprint_mib"] for row in rows]
        elapsed = (rows[-1]["time"] - rows[0]["time"]).total_seconds()
        cpu_delta = rows[-1]["cpu_ns"] - rows[0]["cpu_ns"]
        processes.append({
            "role": role,
            "pid": rows[0]["pid"],
            "birth": rows[0]["birth"],
            "lifetime_peak_mib": round(max(r["lifetime_peak_mib"] for r in rows),1),
            "first": rows[0]["time"].isoformat(),
            "last": rows[-1]["time"].isoformat(),
            "samples": len(rows),
            "footprint_first_mib": round(footprint[0], 1),
            "footprint_last_mib": round(footprint[-1], 1),
            "footprint_median_mib": round(statistics.median(footprint), 1),
            "footprint_p95_mib": round(percentile(footprint, 0.95), 1),
            "footprint_peak_mib": round(max(footprint), 1),
            "cpu_avg_one_core_percent": round(100 * cpu_delta / (elapsed * 1e9), 2)
            if elapsed > 0 and cpu_delta >= 0 else None,
        })
    return {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "first": timestamps[0].isoformat() if timestamps else None,
        "last": timestamps[-1].isoformat() if timestamps else None,
        "sampling_gap_threshold_seconds": GAP_SECONDS,
        "sampling_gaps": gaps,
        "absent_samples": absences,
        "processes": processes,
    }


def chart(groups, start, end, metric, label):
    width, height = 1100, 310
    left, right, top, bottom = 64, 20, 18, 46
    inner_width, inner_height = width - left - right, height - top - bottom
    span = max((end - start).total_seconds(), 1)
    values = []
    series = []
    for (role, pid), rows in sorted(groups.items()):
        points = []
        previous = None
        for row in rows:
            if metric == "footprint_mib":
                value = row[metric]
            else:
                delta_seconds = (row["time"] - previous["time"]).total_seconds() if previous else 0
                delta_cpu = row["cpu_ns"] - previous["cpu_ns"] if previous else 0
                value = 100 * delta_cpu / (delta_seconds * 1e9) if 0 < delta_seconds <= GAP_SECONDS and delta_cpu >= 0 else None
            if value is not None:
                points.append((row["time"], value, previous is not None and
                               (row["time"] - previous["time"]).total_seconds() > GAP_SECONDS))
                values.append(value)
            previous = row
        if points:
            series.append((role, pid, points))
    ceiling = max(values, default=1) * 1.08
    if metric == "cpu_percent":
        ceiling = max(10, ceiling)
    else:
        ceiling = max(100, ceiling)
    x = lambda stamp: left + (stamp - start).total_seconds() / span * inner_width
    y = lambda value: top + inner_height * (1 - value / ceiling)
    parts = [f'<svg viewBox="0 0 {width} {height}" role="img" aria-label="{html.escape(label)}">']
    for step in range(5):
        value = ceiling * step / 4
        yy = y(value)
        parts.append(f'<line x1="{left}" y1="{yy:.1f}" x2="{width-right}" y2="{yy:.1f}" class="grid"/>')
        parts.append(f'<text x="{left-8}" y="{yy+4:.1f}" text-anchor="end">{value:.0f}</text>')
    for step in range(6):
        seconds = span * step / 5
        stamp = start.timestamp() + seconds
        xx = left + inner_width * step / 5
        parts.append(f'<text x="{xx:.1f}" y="{height-15}" text-anchor="middle">'
                     f'{datetime.fromtimestamp(stamp, timezone.utc):%m-%d %H:%M}</text>')
    for role, pid, points in series:
        commands = []
        previous = None
        for stamp, value, gap in points:
            xx, yy = x(stamp), y(value)
            disconnected = previous is None or gap or (stamp - previous).total_seconds() > GAP_SECONDS
            commands.append(f'{"M" if disconnected else "L"}{xx:.1f},{yy:.1f}')
            previous = stamp
        path = " ".join(commands)
        parts.append(f'<path d="{path}" fill="none" stroke="{COLORS[role]}" stroke-width="2" '
                     f'stroke-linecap="round"><title>{role} · PID {pid}</title></path>')
    parts.append("</svg>")
    return "".join(parts)


def render_html(groups, timestamps, report):
    if timestamps:
        memory = chart(groups, timestamps[0], timestamps[-1], "footprint_mib", "物理内存足迹 MiB")
        cpu = chart(groups, timestamps[0], timestamps[-1], "cpu_percent", "单核 CPU 占比 %")
    else:
        memory = cpu = "<p>还没有有效采样。</p>"
    table_rows = []
    for process in report["processes"]:
        cpu_avg = process["cpu_avg_one_core_percent"]
        table_rows.append("<tr>" + "".join(f"<td>{html.escape(str(value))}</td>" for value in [
            process["role"], process["pid"], process["samples"],
            process["footprint_first_mib"], process["footprint_last_mib"],
            process["footprint_median_mib"], process["footprint_p95_mib"],
            process["footprint_peak_mib"], f"{cpu_avg:.2f}%" if cpu_avg is not None else "—",
        ]) + "</tr>")
    gaps = report["sampling_gaps"]
    gap_rows = "".join(f'<li>{html.escape(gap["start"])} → {html.escape(gap["end"])} · '
                       f'{gap["seconds"] / 60:.1f} 分钟</li>' for gap in gaps)
    legend = "".join(f'<span><i style="background:{color}"></i>{role}</span>'
                     for role, color in COLORS.items())
    return f"""<!doctype html><html lang="zh-CN"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>loppi · 资源曲线</title>
<style>body{{font:15px/1.55 -apple-system,BlinkMacSystemFont,sans-serif;color:#21342f;background:#f5f7f5;margin:0;padding:28px}}
main{{max-width:1200px;margin:auto}}h1{{font-size:25px;margin:0 0 4px}}h2{{font-size:18px;margin:0 0 12px}}p{{color:#62746c;margin:0 0 24px}}
section{{background:#fff;border:1px solid #e0e8e2;border-radius:16px;padding:20px 24px;margin:18px 0;overflow:auto}}
svg{{width:100%;min-width:680px}}svg text{{font-size:11px;fill:#77877f}}.grid{{stroke:#e8eeea;stroke-width:1}}
.legend{{display:flex;gap:20px;flex-wrap:wrap;margin-bottom:12px}}.legend span{{display:flex;align-items:center;gap:7px}}.legend i{{width:12px;height:12px;border-radius:3px}}
table{{border-collapse:collapse;width:100%;white-space:nowrap}}th,td{{text-align:right;padding:9px;border-bottom:1px solid #e9eeea}}th:first-child,td:first-child{{text-align:left}}
th{{color:#62746c;font-weight:600}}.note{{font-size:13px;color:#66786f}}ul{{margin:0;padding-left:20px}}</style>
<main><h1>loppi · 资源曲线</h1><p>更新：{html.escape(report["generated_at"])} · 所有时刻为 UTC · 按 PID 分段；采样空档不连线</p>
<section><h2>物理内存足迹 · MiB</h2><p class="note">分钟摘要保留各进程该分钟峰值；精确同时总量见采样器 meta.json，不能相加不同时间的进程峰值。</p><div class="legend">{legend}</div>{memory}</section>
<section><h2>单核 CPU 占比 · %</h2><div class="legend">{legend}</div>{cpu}</section>
<section><h2>进程摘要</h2><table><thead><tr><th>进程</th><th>PID</th><th>采样</th><th>初值 MiB</th><th>末值 MiB</th><th>中位 MiB</th><th>P95 MiB</th><th>峰值 MiB</th><th>平均单核 CPU</th></tr></thead>
<tbody>{''.join(table_rows)}</tbody></table></section>
<section><h2>采样空档 · {len(gaps)}</h2><ul>{gap_rows or f'<li>无超过 {GAP_SECONDS:g} 秒的采样空档</li>'}</ul>
<p class="note">absent 样本：{html.escape(json.dumps(report["absent_samples"], ensure_ascii=False))}。进程能耗计数与物理足迹只能用于同机趋势，不等于整机电池续航；采样空档内的峰值未知。</p></section></main></html>"""


def atomic_write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=path.parent, delete=False) as handle:
        temporary = Path(handle.name)
        handle.write(content)
    os.replace(temporary, path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("csv", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--summary", type=Path)
    args = parser.parse_args()
    groups, timestamps, absences = read_samples(args.csv)
    report = summarize(groups, timestamps, absences)
    atomic_write(args.output, render_html(groups, timestamps, report))
    if args.summary:
        atomic_write(args.summary, json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"{len(timestamps)} time points, {len(report['processes'])} process runs, "
          f"{len(report['sampling_gaps'])} sampling gaps → {args.output}")


if __name__ == "__main__":
    main()
