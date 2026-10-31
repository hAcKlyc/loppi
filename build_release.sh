#!/bin/zsh
set -euo pipefail
loppi_root=${0:A:h}
exec node "$loppi_root/scripts/build.mjs" release "$@"
