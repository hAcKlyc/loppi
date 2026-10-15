#!/bin/zsh
set -euo pipefail
hear_root=${0:A:h:h}
cd "$hear_root"
hear_minutes=${1:-60}
[[ "$hear_minutes" == <-> && "$hear_minutes" -gt 0 ]] || { print -u2 'Expected positive duration in minutes'; exit 2; }
mkdir -p target
clang -O2 -Wall -Wextra -o target/hear-rusage scripts/macos_rusage.c
exec python3 scripts/sample_macos_resources.py --seconds "$((hear_minutes * 60))" \
  --output "${2:-/tmp/hear-process-usage.csv}" --app "${3:-/Applications/hear.app}" \
  --mode "${4:-new}" --report "${5:-}" --interval "${HEAR_SAMPLE_INTERVAL:-1}" \
  --report-every "$(( ${HEAR_REPORT_EVERY:-15} * 60 ))"
