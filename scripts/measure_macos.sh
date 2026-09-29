#!/bin/zsh
set -euo pipefail

hear_root=${0:A:h:h}
cd "$hear_root"
hear_samples=${1:-60}
hear_csv=${2:-/tmp/hear-process-usage.csv}
hear_app=${3:-/Applications/hear.app}
hear_mode=${4:-new}
hear_report=${5:-}
hear_report_every=${HEAR_REPORT_EVERY:-15}
if [[ ! "$hear_samples" == <-> || "$hear_samples" -lt 1 ]]; then
  print -u2 'Usage: zsh scripts/measure_macos.sh <minutes> [output.csv] [target.app] [new|resume] [report.html]'
  exit 2
fi
if [[ "$hear_mode" != new && "$hear_mode" != resume ]] ||
   [[ ! "$hear_report_every" == <-> || "$hear_report_every" -lt 1 ]]; then
  print -u2 'Invalid capture mode or HEAR_REPORT_EVERY interval.'
  exit 2
fi
hear_app=${hear_app:A}
[[ -d "$hear_app/Contents" ]] || { print -u2 "App not found: $hear_app"; exit 2; }

hear_path() {
  target/hear-rusage --path "$1" 2>/dev/null
}

hear_pids_for() {
  local hear_name=$1 hear_parent=${2:-}
  local hear_pid hear_executable hear_ppid
  local -a hear_candidates
  if [[ "$hear_name" == hear-core ]]; then
    hear_candidates=($(pgrep -x hear-core 2>/dev/null || true) $(pgrep -x app.hear.core 2>/dev/null || true))
  else
    hear_candidates=($(pgrep -x "$hear_name" 2>/dev/null || true))
  fi
  for hear_pid in $hear_candidates; do
    hear_executable=$(hear_path "$hear_pid") || continue
    [[ "$hear_executable" == "$hear_app"/Contents/* ]] || continue
    if [[ -n "$hear_parent" ]]; then
      hear_ppid=$(ps -p "$hear_pid" -o ppid= 2>/dev/null | tr -d ' ') || continue
      [[ "$hear_ppid" == "$hear_parent" ]] || continue
    fi
    print -r -- "$hear_pid"
  done
  return 0
}

mkdir -p target
clang -O2 -Wall -Wextra -o target/hear-rusage scripts/macos_rusage.c
umask 077
hear_header='timestamp_utc,role,status,executable_path,pid,user_time_ns,system_time_ns,resident_bytes,physical_footprint_bytes,disk_read_bytes,disk_write_bytes,energy_nj,penergy_nj'
if [[ "$hear_mode" == resume && -f "$hear_csv" ]]; then
  [[ "$(head -n 1 "$hear_csv")" == "$hear_header" ]] || {
    print -u2 "Cannot resume CSV with a different header: $hear_csv"
    exit 2
  }
else
  print -r -- "$hear_header" > "$hear_csv"
fi

hear_render_report() {
  [[ -n "$hear_report" ]] || return 0
  python3 scripts/report_macos_resources.py "$hear_csv" --output "$hear_report" \
    --summary "$hear_report.json" || print -u2 "Resource report failed: $hear_report"
}
hear_render_report
for ((hear_sample=0; hear_sample<hear_samples; hear_sample++)); do
  hear_stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  hear_cores=($(hear_pids_for hear-core))
  for hear_role in hear-core hear-worker hear-desktop; do
    case "$hear_role" in
      hear-core) hear_pids=($hear_cores) ;;
      hear-worker)
        hear_pids=()
        for hear_core_pid in $hear_cores; do
          hear_pids+=($(hear_pids_for hear-worker "$hear_core_pid"))
        done ;;
      hear-desktop) hear_pids=($(hear_pids_for hear-desktop)) ;;
    esac
    # Transient supervised processes may overlap. Record each PID and inspect
    # transitions in the CSV rather than ending the measurement.
    hear_present=0
    for hear_pid in $hear_pids; do
      hear_executable=$(hear_path "$hear_pid") || continue
      hear_values=$(target/hear-rusage "$hear_pid") || continue
      hear_executable=${hear_executable//\"/\"\"}
      if [[ -n "$hear_values" ]]; then
        print -r -- "$hear_stamp,$hear_role,present,\"$hear_executable\",$hear_values" >> "$hear_csv"
        hear_present=1
      fi
    done
    if (( hear_present == 0 )); then
      print -r -- "$hear_stamp,$hear_role,absent,\"\",,,,,,,,," >> "$hear_csv"
    fi
  done
  if (( (hear_sample + 1) % hear_report_every == 0 )); then
    hear_render_report
  fi
  (( hear_sample + 1 == hear_samples )) || sleep 60
done
hear_render_report
