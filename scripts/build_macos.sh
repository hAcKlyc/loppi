#!/bin/zsh
set -euo pipefail
hear_root=${0:A:h:h}
exec node "$hear_root/scripts/build.mjs" build "$@"
