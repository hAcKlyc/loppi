#!/bin/zsh
set -euo pipefail
loppi_root=${0:A:h:h}
exec node "$loppi_root/scripts/build.mjs" build "$@"
