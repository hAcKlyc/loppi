#!/bin/zsh
set -euo pipefail
loppi_root=${0:A:h:h}
cd "$loppi_root"
loppi_minutes=${1:-60}
[[ "$loppi_minutes" == <-> && "$loppi_minutes" -gt 0 ]] || { print -u2 'Expected positive duration in minutes'; exit 2; }
mkdir -p target
clang -O2 -Wall -Wextra -o target/loppi-rusage scripts/macos_rusage.c
exec python3 scripts/sample_macos_resources.py --seconds "$((loppi_minutes * 60))" \
  --output "${2:-/tmp/loppi-process-usage.csv}" --app "${3:-/Applications/loppi.app}" \
  --mode "${4:-new}" --report "${5:-}" --interval "${LOPPI_SAMPLE_INTERVAL:-1}" \
  --report-every "$(( ${LOPPI_REPORT_EVERY:-15} * 60 ))"
